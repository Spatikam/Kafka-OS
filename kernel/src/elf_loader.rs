// src/elf_loader.rs
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, Mapper, Size4KiB, FrameAllocator}};
use xmas_elf::{ElfFile, program::{Type, ProgramHeader}};

// FIX: Move from 0x400000 to 0x1000_0000 so we don't collide with the Kernel
const LOAD_OFFSET: u64 = 0x1000_0000;

pub struct ElfLoader<'a, M, F> {
    elf: ElfFile<'a>,
    mapper: &'a mut M,
    allocator: &'a mut F,
}

impl<'a, M, F> ElfLoader<'a, M, F>
where
    M: Mapper<Size4KiB>,
    F: FrameAllocator<Size4KiB>,
{
    pub fn new(data: &'a [u8], mapper: &'a mut M, allocator: &'a mut F) -> Self {
        Self {
            elf: ElfFile::new(data).expect("Invalid ELF file"),
            mapper,
            allocator,
        }
    }

    // src/elf_loader.rs First time in emacs.. hehe will be fun so

    pub fn load(&mut self) -> u64 {
        for ph in self.elf.program_iter() {
            if let ProgramHeader::Ph64(header) = ph {
                if header.get_type().unwrap() == Type::Load {
                    
                    let virt_start = header.virtual_addr + LOAD_OFFSET;
                    let mem_size = header.mem_size;
                    let file_size = header.file_size;
                    let file_offset = header.offset;

                    let start_page = Page::containing_address(VirtAddr::new(virt_start));
                    let end_page = Page::containing_address(VirtAddr::new(virt_start + mem_size));

                    for page in Page::range_inclusive(start_page, end_page) {
                        
                        // --- THE FIX: FORCE UNMAP FIRST ---
                        // If the page exists (bootloader mapping), kill it.
                        if self.mapper.translate_page(page).is_ok() {
                            self.mapper.unmap(page).expect("failed to unmap").1.flush();
                        }

                        // Now map it fresh with OUR rules
                        let frame = self.allocator.allocate_frame().unwrap();
                        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
                        
                        unsafe {
                            self.mapper.map_to(page, frame, flags, self.allocator)
                                .unwrap()
                                .flush();
                        }
                    }

                    // Copy the Data
                    let dest = virt_start as *mut u8;
                    unsafe {
                        core::ptr::write_bytes(dest, 0, mem_size as usize);
                        let data_start = self.elf.input.as_ptr().add(file_offset as usize);
                        core::ptr::copy_nonoverlapping(data_start, dest, file_size as usize);
                    }
                }
            }
        }
        
        self.elf.header.pt2.entry_point() + LOAD_OFFSET
    }
        
}
