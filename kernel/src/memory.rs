use bootloader_api::info::{MemoryRegions, MemoryRegionKind};
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB},
};

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        let level_4_table = active_level_4_table(physical_memory_offset);
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    unsafe { &mut *page_table_ptr }
}

pub struct EmptyFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        None
    }
}

pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryRegions,
    region_idx:usize,
    next_addr:u64,
}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_map: &'static MemoryRegions) -> Self {
        let mut alloc = BootInfoFrameAllocator{memory_map,region_idx:0,next_addr:0};
        alloc.seek_region_start();
        alloc
    }
    fn seek_region_start(&mut self) {
        if self.region_idx < self.memory_map.len() {
            let start = self.memory_map[self.region_idx].start;
            self.next_addr = (start + 0xfff) & !0xfff; // align up to 4 KiB
        }
    }

}
unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while self.region_idx < self.memory_map.len() {
            let region = &self.memory_map[self.region_idx];
            if region.kind == MemoryRegionKind::Usable && self.next_addr + 4096 <= region.end {
                let frame = PhysFrame::containing_address(PhysAddr::new(self.next_addr));
                self.next_addr += 4096;
                return Some(frame);
            }
            self.region_idx += 1;
            self.seek_region_start();
        }
        None
    }
}

pub unsafe fn create_new_page_table(
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    physical_memory_offset: VirtAddr,
    is_user: bool,
) -> PhysFrame {
    let frame = frame_allocator.allocate_frame().expect("no frame");
    let phys_addr = frame.start_address();
    let virt_addr = physical_memory_offset + phys_addr.as_u64();
    let page_table_ptr: *mut PageTable = virt_addr.as_mut_ptr();
    let new_table = &mut *page_table_ptr;
    new_table.zero();
    let active_table = active_level_4_table(physical_memory_offset);

    // yeah For now I will I guess copy the ALL entries (0-511) for both kernel and user tables.
    // With bootloader 0.11, kernel lives in lower half (entries 0-255),
    // so user tables need those too or IDT/kernel code vanishes on CR3 switch!
    for i in 0..512 {
        new_table[i] = active_table[i].clone();
    }

    if is_user {
        crate::serial_println!("[MEMORY] Creating USER Table. Copied ALL kernel mappings.");
    } else {
        crate::serial_println!("[MEMORY] Creating KERNEL Table. Copying ALL.");
    }
    frame
}

use spin::Mutex;

pub static GLOBAL_FRAME_ALLOCATOR: Mutex<Option<BootInfoFrameAllocator>> = Mutex::new(None);
pub static GLOBAL_PHYS_MEM_OFFSET: core::sync::atomic::AtomicU64 =core::sync::atomic::AtomicU64::new(0);

pub fn set_global_frame_allocator(alloc: BootInfoFrameAllocator) {
    *GLOBAL_FRAME_ALLOCATOR.lock() = Some(alloc);
}

pub fn set_global_phys_offset(offset: u64) {
    GLOBAL_PHYS_MEM_OFFSET.store(offset, core::sync::atomic::Ordering::Relaxed);
}

pub fn get_global_phys_offset() -> u64 {
    GLOBAL_PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed)
}
