use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use x86_64::{VirtAddr, PhysAddr,structures::paging::{FrameAllocator, Mapper, OffsetPageTable, Page, PageTable,PageTableFlags, PhysFrame, Size4KiB,},registers::control::{Cr2, Cr3},};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VmProt: u8 {
        const READ    = 0b001;
        const WRITE   = 0b010;
        const EXECUTE = 0b100;
    }
}

bitflags::bitflags! {
    /// VMA type flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VmFlags: u8 {
        const ANONYMOUS = 0b0001;
        const FILE      = 0b0010;
        const COW       = 0b0100;
        const SHARED    = 0b1000;
    }
}
#[derive(Debug, Clone)]
pub struct VmArea {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub prot: VmProt,
    pub flags: VmFlags,
    pub ref_count: u64,
}

impl VmArea {
    pub fn new(start: VirtAddr, end: VirtAddr, prot: VmProt, flags: VmFlags) -> Self {
        Self {
            start,
            end,
            prot,
            flags,
            ref_count: 1,
        }
    }
    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.start && addr < self.end
    }
    pub fn page_count(&self) -> u64 {
        (self.end.as_u64() - self.start.as_u64()) / 4096
    }
}
const MAX_FRAMES: usize = 32768;
static FRAME_REF_COUNTS: Mutex<FrameRefCounts> = Mutex::new(FrameRefCounts::new());

struct FrameRefCounts {
    counts: [u16; MAX_FRAMES],
    base_addr: u64,
    initialized: bool,
}

impl FrameRefCounts {
    const fn new() -> Self {
        Self {
            counts: [0; MAX_FRAMES],
            base_addr: 0,
            initialized: false,
        }
    }

    fn init(&mut self, base: u64) {
        self.base_addr = base;
        self.initialized = true;
    }

    fn frame_index(&self, phys_addr: u64) -> Option<usize> {
        if !self.initialized {
            return None;
        }
        if phys_addr < self.base_addr {
            return None;
        }
        let idx = ((phys_addr - self.base_addr) / 4096) as usize;
        if idx < MAX_FRAMES { Some(idx) } else { None }
    }

    fn increment(&mut self, phys_addr: u64) {
        if let Some(idx) = self.frame_index(phys_addr) {
            self.counts[idx] = self.counts[idx].saturating_add(1);
        }
    }

    fn decrement(&mut self, phys_addr: u64) -> u16 {
        if let Some(idx) = self.frame_index(phys_addr) {
            self.counts[idx] = self.counts[idx].saturating_sub(1);
            self.counts[idx]
        } else {
            0
        }
    }

    fn get(&self, phys_addr: u64) -> u16 {
        if let Some(idx) = self.frame_index(phys_addr) {
            self.counts[idx]
        } else {
            0
        }
    }
}
pub fn init_ref_counts(base_phys_addr: u64) {
    let mut refs = FRAME_REF_COUNTS.lock();
    refs.init(base_phys_addr);
    crate::serial_println!("[VM] Frame ref counts initialized, base: {:#x}", base_phys_addr);
}

pub fn frame_ref_increment(phys_addr: u64) {
    FRAME_REF_COUNTS.lock().increment(phys_addr);
}
pub fn frame_ref_decrement(phys_addr: u64) -> u16 {
    FRAME_REF_COUNTS.lock().decrement(phys_addr)
}
pub fn frame_ref_get(phys_addr: u64) -> u16 {
    FRAME_REF_COUNTS.lock().get(phys_addr)
}
pub fn find_vma(vma_list: &[VmArea], addr: VirtAddr) -> Option<usize> {
    vma_list.iter().position(|vma| vma.contains(addr))
}
pub fn insert_vma(vma_list: &mut Vec<VmArea>, vma: VmArea) {
    let pos = vma_list.iter().position(|v| v.start > vma.start).unwrap_or(vma_list.len());
    vma_list.insert(pos, vma);
}
pub fn remove_vma_range(vma_list: &mut Vec<VmArea>, start: VirtAddr, end: VirtAddr) -> Vec<VmArea> {
    let mut removed = Vec::new();
    vma_list.retain(|vma| {
        if vma.start < end && vma.end > start {
            removed.push(vma.clone());
            false
        } else {
            true
        }
    });
    removed
}
pub fn pte_flags_from_vma(vma: &VmArea) -> PageTableFlags {
    let mut flags = PageTableFlags::PRESENT;
    if vma.prot.contains(VmProt::WRITE) && !vma.flags.contains(VmFlags::COW) {
        flags |= PageTableFlags::WRITABLE;
    }
    flags |= PageTableFlags::USER_ACCESSIBLE;

    // NX bit: set NO_EXECUTE if the region is not executable
    if !vma.prot.contains(VmProt::EXECUTE) {
        flags |= PageTableFlags::NO_EXECUTE;
    }

    flags
}
#[inline]
pub fn invalidate_page(addr: VirtAddr) {
    unsafe {
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) addr.as_u64(),
            options(nostack, preserves_flags)
        );
    }
}
#[inline]
pub fn flush_tlb() {
    let (frame, flags) = Cr3::read();
    unsafe { Cr3::write(frame, flags); }
}
#[derive(Debug)]
pub enum PageFaultResult {
    //Fault was resolved (page mapped). Resume execution.
    Resolved,
    // The process should be killed (SIGSEGV).
    SegmentationFault,
    //No VMA list available (kernel fault or process without VMAs).
    NoVmaList,
}
pub fn handle_page_fault(fault_addr: VirtAddr,error_code: x86_64::structures::idt::PageFaultErrorCode,vma_list: &mut Vec<VmArea>,mapper: &mut OffsetPageTable<'_>,frame_allocator: &mut impl FrameAllocator<Size4KiB>,) -> PageFaultResult {
    let is_write = error_code.contains(x86_64::structures::idt::PageFaultErrorCode::CAUSED_BY_WRITE);
    let is_present = error_code.contains(x86_64::structures::idt::PageFaultErrorCode::PROTECTION_VIOLATION);
    let is_user = error_code.contains(x86_64::structures::idt::PageFaultErrorCode::USER_MODE);

    crate::serial_println!("[VM] Page fault at {:#x} | write={} present={} user={}",fault_addr.as_u64(), is_write, is_present, is_user);

    // Step 1: Find VMA
    let vma_idx = match find_vma(vma_list, fault_addr) {
        Some(idx) => idx,
        None => { crate::serial_println!("[VM] No VMA for address {:#x} → SIGSEGV", fault_addr.as_u64()); return PageFaultResult::SegmentationFault;}
    };

    let vma = &vma_list[vma_idx];

    // Step 2: Permission check — write to a non-writable, non-CoW VMA
    if is_write && !vma.prot.contains(VmProt::WRITE) && !vma.flags.contains(VmFlags::COW) {
        crate::serial_println!("[VM] Write to non-writable VMA at {:#x} → SIGSEGV",fault_addr.as_u64());
        return PageFaultResult::SegmentationFault;
    }

    let page: Page<Size4KiB> = Page::containing_address(fault_addr);
    if is_write && is_present && vma.flags.contains(VmFlags::COW) {
        return cow_resolve(page, vma_idx, vma_list, mapper, frame_allocator);
    }
    if vma.flags.contains(VmFlags::ANONYMOUS) && !is_present {
        return demand_allocate(page, vma, mapper, frame_allocator);
    }

    // If we get here, the fault is not resolvable
    crate::serial_println!("[VM] Unresolvable fault at {:#x}, error={:?}",fault_addr.as_u64(), error_code);
    PageFaultResult::SegmentationFault
}

fn demand_allocate(page: Page<Size4KiB>,vma: &VmArea,mapper: &mut OffsetPageTable<'_>,frame_allocator: &mut impl FrameAllocator<Size4KiB>,) -> PageFaultResult {
    // Allocate a fresh frame
    let frame = match frame_allocator.allocate_frame() {
        Some(f) => f,
        None => {
            crate::serial_println!("[VM] Out of physical memory during demand paging!");
            return PageFaultResult::SegmentationFault;
        }
    };
    zero_frame(frame);

    let flags = pte_flags_from_vma(vma);

    // Map the page
    unsafe {
        match mapper.map_to(page, frame, flags, frame_allocator) {
            Ok(flush) => flush.flush(),
            Err(e) => {
                crate::serial_println!("[VM] Failed to map demand page: {:?}", e);
                return PageFaultResult::SegmentationFault;
            }
        }
    }
    frame_ref_increment(frame.start_address().as_u64());
    crate::serial_println!( "[VM] Demand paged: {:#x} → frame {:#x}",page.start_address().as_u64(),frame.start_address().as_u64());
    PageFaultResult::Resolved
}

fn cow_resolve(page: Page<Size4KiB>,vma_idx: usize,vma_list: &mut Vec<VmArea>,mapper: &mut OffsetPageTable<'_>,frame_allocator: &mut impl FrameAllocator<Size4KiB>,) -> PageFaultResult {
    use x86_64::structures::paging::mapper::TranslateResult;
    let old_frame = match mapper.translate_page(page) {
        Ok(frame) => frame,
        Err(_) => {
            crate::serial_println!("[VM] CoW fault but page not mapped?!");
            return PageFaultResult::SegmentationFault;
        }
    };

    let old_phys = old_frame.start_address().as_u64();
    let ref_count = frame_ref_get(old_phys);

    if ref_count <= 1 {
        let vma = &mut vma_list[vma_idx];

        // Remove CoW flag — this page is now exclusively ours
        vma.flags.remove(VmFlags::COW);

        let mut new_flags = pte_flags_from_vma(vma);
        new_flags |= PageTableFlags::WRITABLE;

        unsafe {
            // Unmap and remap with new flags
            if let Ok((_, flush)) = mapper.unmap(page) {
                flush.flush();
            }
            match mapper.map_to(page, old_frame, new_flags, frame_allocator) {
                Ok(flush) => flush.flush(),
                Err(e) => {
                    crate::serial_println!("[VM] CoW in-place remap failed: {:?}", e);
                    return PageFaultResult::SegmentationFault;
                }
            }
        }

        crate::serial_println!( "[VM] CoW resolved in-place: {:#x} (ref_count was {})",page.start_address().as_u64(), ref_count);
    } else {
        // Shared: allocate a new frame and copy the data
        let new_frame = match frame_allocator.allocate_frame() {
            Some(f) => f,
            None => {crate::serial_println!("[VM] Out of memory during CoW copy!");return PageFaultResult::SegmentationFault;}
        };

        // Copy 4 KiB from old frame to new frame 
        copy_frame(old_frame, new_frame);
        frame_ref_decrement(old_phys);

        // Unmap old, map new with writable flags
        let vma = &mut vma_list[vma_idx];
        let mut new_flags = pte_flags_from_vma(vma);
        new_flags |= PageTableFlags::WRITABLE;

        unsafe {
            if let Ok((_, flush)) = mapper.unmap(page) {
                flush.flush();
            }
            match mapper.map_to(page, new_frame, new_flags, frame_allocator) {
                Ok(flush) => flush.flush(),
                Err(e) => {crate::serial_println!("[VM] CoW copy remap failed: {:?}", e);return PageFaultResult::SegmentationFault;}
            }
        }
        // Track the new frame
        frame_ref_increment(new_frame.start_address().as_u64());

        crate::serial_println!("[VM] CoW copied: {:#x} → new frame {:#x} (old ref_count: {})",page.start_address().as_u64(),new_frame.start_address().as_u64(),ref_count);
    }

    PageFaultResult::Resolved
}
fn zero_frame(frame: PhysFrame) {
    let phys_addr = frame.start_address().as_u64();
    let offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    if offset == 0 {
        crate::serial_println!("[VM] WARNING: PHYS_MEM_OFFSET not set, cannot zero frame");
        return;
    }
    let virt = (offset + phys_addr) as *mut u8;
    unsafe {
        core::ptr::write_bytes(virt, 0, 4096);
    }
}

/// Copy 4 KiB from one physical frame to another.
fn copy_frame(src: PhysFrame, dst: PhysFrame) {
    let offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    if offset == 0 {
        crate::serial_println!("[VM] WARNING: PHYS_MEM_OFFSET not set, cannot copy frame");
        return;
    }
    let src_virt = (offset + src.start_address().as_u64()) as *const u8;
    let dst_virt = (offset + dst.start_address().as_u64()) as *mut u8;
    // SAFETY: Both frames are valid, dst is freshly allocated and not aliased.
    unsafe {
        core::ptr::copy_nonoverlapping(src_virt, dst_virt, 4096);
    }
}
static PHYS_MEM_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Set the physical memory offset. Call once during boot.
pub fn set_phys_mem_offset(offset: u64) {
    PHYS_MEM_OFFSET.store(offset, Ordering::Relaxed);
    crate::serial_println!("[VM] Physical memory offset set to {:#x}", offset);
}

/// Get the physical memory offset.
pub fn phys_mem_offset() -> u64 {
    PHYS_MEM_OFFSET.load(Ordering::Relaxed)
}

pub fn cow_fork_vmas(parent_vmas: &mut Vec<VmArea>) -> Vec<VmArea> {
    let mut child_vmas = Vec::new();

    for vma in parent_vmas.iter_mut() {
        if vma.prot.contains(VmProt::WRITE) {
            // Mark as CoW in the parent
            vma.flags.insert(VmFlags::COW);
            vma.ref_count += 1;
        }

        // Clone for child (same CoW flags)
        let mut child_vma = vma.clone();
        child_vma.ref_count = vma.ref_count;
        child_vmas.push(child_vma);
    }

    child_vmas
}
// CoW is dead until fork exists. Fix the remap when wiring up fork
pub unsafe fn mark_pages_readonly_for_cow(
    vma_list: &[VmArea],
    mapper: &mut OffsetPageTable<'_>,
) {
    for vma in vma_list {
        if !vma.flags.contains(VmFlags::COW) {
            continue;
        }

        let start_page: Page<Size4KiB> = Page::containing_address(vma.start);
        let end_page: Page<Size4KiB> = Page::containing_address(vma.end - 1u64);

        for page in Page::range_inclusive(start_page, end_page) {
            if let Ok(frame) = mapper.translate_page(page) {
                let phys = frame.start_address().as_u64();
                let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
                if !vma.prot.contains(VmProt::EXECUTE) {
                    flags |= PageTableFlags::NO_EXECUTE;
                }
                if let Ok((_, flush)) = mapper.unmap(page) {
                    flush.flush();
                }
                frame_ref_increment(phys);
            }
        }
    }
}