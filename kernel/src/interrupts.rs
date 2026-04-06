// src/interrupts.rs
use crate::{gdt, hlt_loop, print, println};
use crate::task::keyboard::WAKER;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crossbeam_queue::ArrayQueue;
use core::sync::atomic::{AtomicU64,AtomicBool,Ordering};
use crate::gui::graphics;
use alloc::vec::Vec;

pub static TICKS:AtomicU64=AtomicU64::new(0);
pub static SNAKE_SCANCODES: spin::Mutex<alloc::vec::Vec<u8>> = spin::Mutex::new(alloc::vec::Vec::new());
pub static SNAKE_ACTIVE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

// --- 1. Constants & Enums ---
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Mouse = PIC_1_OFFSET + 12, // Interrupt 12 is usually Mouse
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

// --- 2. Global Static Objects ---

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

// THE QUEUE: This is where we store keys for the User App
lazy_static! {
    pub static ref KEYBOARD_QUEUE: ArrayQueue<char> = ArrayQueue::new(100);
}

// THE IDT: This maps hardware events to our functions
lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        
        // Exceptions
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);

        // Hardware Interrupts
        idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse.as_usize()].set_handler_fn(mouse_interrupt_handler);
        
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}
pub fn uptime_seconds() -> f64 {
    let ticks = TICKS.load(Ordering::Relaxed);
    // Standard PC timer frequency is roughly 18.2 Hz
    ticks as f64 / 1000.0
}
// --- 3. Exception Handlers ---

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    let fault_addr = Cr2::read();
    let resolved = try_resolve_page_fault(fault_addr,error_code);
    if !resolved{
        crate::serial_println!("====== EXCEPTION: PAGE FAULT ======");
        crate::serial_println!("Accessed Address : {:?}", Cr2::read());
        crate::serial_println!("Error Code       : {:?}", error_code);
        crate::serial_println!("{:#?}", stack_frame);
        hlt_loop();
    }
}

fn try_resolve_page_fault(fault_addr:x86_64::VirtAddr,error_code:PageFaultErrorCode,) -> bool{
    use crate::memory::{GLOBAL_FRAME_ALLOCATOR, GLOBAL_PHYS_MEM_OFFSET};
    use core::sync::atomic::Ordering;

    let phys_offset = GLOBAL_PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    if phys_offset == 0 {crate::serial_println!("[PF] No physical offset");return false;}

    let mut sched = crate::scheduler::SCHEDULER.lock();
    let current = match sched.current_process_mut() {
        Some(p) => p,
        None => { crate::serial_println!("[PF] No current process, cannot resolve"); return false;}
    };
    if current.vma_list.is_empty() {
        // Process has no VMAs registered — can't do demand paging
        crate::serial_println!("[PF] Process '{}' has no VMAs", current.name);
        return false;
    }
    let pt_phys = current.page_table.start_address();
    let pt_virt = x86_64::VirtAddr::new(phys_offset + pt_phys.as_u64());
    let page_table = unsafe {
        &mut *(pt_virt.as_mut_ptr::<x86_64::structures::paging::PageTable>())
    };
    let mut mapper = unsafe { x86_64::structures::paging::OffsetPageTable::new(page_table,x86_64::VirtAddr::new(phys_offset),)
    };
    let mut mapper = unsafe {
        x86_64::structures::paging::OffsetPageTable::new(page_table,x86_64::VirtAddr::new(phys_offset),)
    };

    // yeah better call the frame alloc man, 
    let mut alloc_guard = GLOBAL_FRAME_ALLOCATOR.lock();
    let allocator = match alloc_guard.as_mut() {
        Some(a) => a,
        None => {
            crate::serial_println!("[PF] No global frame allocator");
            return false;
        }
    };
    let result = crate::vm::handle_page_fault(fault_addr,error_code,&mut current.vma_list,&mut mapper,allocator,);
    match result {
        crate::vm::PageFaultResult::Resolved => { crate::serial_println!("[PF] Fault at {:?} resolved!", fault_addr);true }
        _ => {
            crate::serial_println!("[PF] Fault at {:?} NOT resolved", fault_addr);
            false
        }
    }
}
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}
extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::scheduler::SCHEDULER_TICKS.fetch_add(1, Ordering::Relaxed);

    let ticks = graphics::PIT_TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // Wake compositor ~60fps changed from 18Hz to 1000Hz.
    if ticks % 16 == 0 {
        graphics::CLOCK_TICK.store(true, core::sync::atomic::Ordering::Relaxed);
        if let Some(waker) = crate::gui::graphics::COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }
    }
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }

    let should_switch = {
        let mut sched = crate::scheduler::SCHEDULER.lock();
        sched.timer_tick()
    };

    if should_switch {
        crate::process::sys_yield();
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode:u8 = unsafe {port.read()};
    crate::task::keyboard::add_scancode(scancode);
    if SNAKE_ACTIVE.load(Ordering::Relaxed){
        SNAKE_SCANCODES.lock().push(scancode);
    }
    unsafe{
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x60);
    let packet = unsafe { port.read() };
    crate::task::mouse::add_packet_from_interrupt(packet);
    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}

// --- 5. Tests ---

#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
}