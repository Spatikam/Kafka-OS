// src/process.rs
use crate::gdt::GDT;
use x86_64::VirtAddr;
use alloc::vec::Vec;
use core::arch::naked_asm;
use x86_64::structures::paging::{PhysFrame, FrameAllocator, Size4KiB, Mapper, Page, PageTableFlags, OffsetPageTable, PageTable};
use x86_64::registers::control::Cr3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

pub struct Process {
    pub id: ProcessId,
    pub name: &'static str,
    pub stack_pointer: VirtAddr,
    pub state: ProcessState,
    pub stack_memory: Vec<u8>,
    pub page_table: PhysFrame,
}

impl Process {
    pub fn new(id: u64, name: &'static str, stack_size: usize, frame_allocator: &mut impl FrameAllocator<Size4KiB>, phys_mem_offset: VirtAddr) -> Self {
        let mut stack: Vec<u8> = Vec::with_capacity(stack_size);
        let is_user = id >= 4;
        unsafe {
            stack.set_len(stack_size);
            for i in 0..stack_size {
                stack[i] = 0;
            }
        }
        let stack_start = VirtAddr::from_ptr(stack.as_ptr());
        let stack_end = stack_start + stack_size;
        let stack_pointer = stack_end;
        let page_table = unsafe { crate::memory::create_new_page_table(frame_allocator, phys_mem_offset, is_user) };
        Self {
            id: ProcessId(id),
            name,
            stack_pointer,
            state: ProcessState::Ready,
            stack_memory: stack,
            page_table,
        }
    }

    pub fn init_stack(&mut self, entry_point: u64) {
        let mut sp = self.stack_pointer.as_u64();
        let mut push = |value: u64| {
            sp -= 8;
            unsafe {
                let ptr = sp as *mut u64;
                *ptr = value;
            }
        };
        push(entry_point);
        push(0); // RBX
        push(0); // RBP
        push(0); // R12
        push(0); // R13
        push(0); // R14
        push(0); // R15
        self.stack_pointer = VirtAddr::new(sp);
    }

    pub fn load_elf(&mut self, elf_data: &[u8], _mapper: &mut impl Mapper<Size4KiB>, allocator: &mut impl FrameAllocator<Size4KiB>, phys_mem_offset: VirtAddr) -> u64 {
        let (kernel_frame, kernel_flags) = Cr3::read();
        unsafe {
            Cr3::write(self.page_table, kernel_flags);
            let phys_addr = self.page_table.start_address();
            let virt_addr = phys_mem_offset + phys_addr.as_u64();
            let page_table_ptr: *mut PageTable = virt_addr.as_mut_ptr();
            let root_table = &mut *page_table_ptr;
            let mut process_mapper = OffsetPageTable::new(root_table, phys_mem_offset);
            let mut loader = crate::elf_loader::ElfLoader::new(elf_data, &mut process_mapper, allocator);
            let entry_point = loader.load();
            Cr3::write(kernel_frame, kernel_flags);
            entry_point
        }
    }

    pub fn allocate_user_stack(&mut self, stack_addr: u64, size: u64, _mapper: &mut impl Mapper<Size4KiB>, allocator: &mut impl FrameAllocator<Size4KiB>, phys_mem_offset: VirtAddr) -> u64 {
        let (kernel_frame, kernel_flags) = Cr3::read();
        unsafe {
            Cr3::write(self.page_table, kernel_flags);
            let phys_addr = self.page_table.start_address();
            let virt_addr = phys_mem_offset + phys_addr.as_u64();
            let page_table_ptr: *mut PageTable = virt_addr.as_mut_ptr();
            let root_table = &mut *page_table_ptr;
            let mut process_mapper = OffsetPageTable::new(root_table, phys_mem_offset);
            let start_page = Page::containing_address(VirtAddr::new(stack_addr));
            let end_page = Page::containing_address(VirtAddr::new(stack_addr + size));
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
            for page in Page::range_inclusive(start_page, end_page) {
                if process_mapper.translate_page(page).is_ok() {
                    process_mapper.unmap(page).expect("unmap failed").1.flush();
                }
                let frame = allocator.allocate_frame().unwrap();
                process_mapper.map_to(page, frame, flags, allocator).unwrap().flush();
            }
            Cr3::write(kernel_frame, kernel_flags);
        }
        stack_addr + size
    }
}

pub fn sys_yield() {
    x86_64::instructions::interrupts::disable();
    let mut switch_info: Option<(u64, *mut u64, u64)> = None;
    {
        let mut sched = crate::scheduler::SCHEDULER.lock();
        switch_info = sched.rotate_and_get_next();
    }
    if let Some((new_stack, old_stack_write_target, new_cr3)) = switch_info {
        unsafe {
            // CR3 switch now happens INSIDE context_switch assembly
            context_switch(old_stack_write_target, new_stack, new_cr3);
        }
    }
    x86_64::instructions::interrupts::enable();
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn context_switch(old_stack_ptr: *mut u64, new_stack_ptr: u64, new_cr3: u64) {
    unsafe {
        naked_asm!(
            // Save callee-saved registers
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            // Save old stack pointer
            // RDI = old_stack_ptr
            "mov [rdi], rsp",
            // Switch page table BEFORE switching stack
            // RDX = new_cr3
            "mov cr3, rdx",
            // Switch to new stack
            // RSI = new_stack_ptr
            "mov rsp, rsi",
            // Restore callee-saved registers from new stack
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            // Jump to new process
            "ret"
        );
    }
}

pub unsafe extern "C" fn jump_to_userspace(entry_point: u64, user_stack_top: u64) {
    use core::arch::asm;
    let code_selector = crate::gdt::GDT.1.user_code_selector.0;
    let data_selector = crate::gdt::GDT.1.user_data_selector.0;
    asm!(
        "push {data_sel}",
        "push {stack_ptr}",
        "pushf",
        "pop rax",
        "or rax, 0x200",
        "push rax",
        "push {code_sel}",
        "push {entry}",
        "iretq",
        data_sel = in(reg) data_selector,
        stack_ptr = in(reg) user_stack_top,
        code_sel = in(reg) code_selector,
        entry = in(reg) entry_point,
        options(noreturn)
    );
}