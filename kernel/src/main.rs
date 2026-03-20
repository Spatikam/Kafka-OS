#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(naked_functions)]
#![test_runner(blog_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use blog_os::elf_loader::ElfLoader;
use blog_os::gdt;
use blog_os::net::pci;
use blog_os::println;
use blog_os::task::mouse::MouseStream;
use blog_os::task::{Task, executor::Executor, keyboard, mouse};
use bootloader_api::config::Mapping;
use bootloader_api::{BootInfo, BootloaderConfig, entry_point};
use core::arch::{asm, naked_asm};
use core::panic::PanicInfo;
use futures_util::StreamExt;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::structures::paging::Translate;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags as Flags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};
/*
use embedded_graphics::{
    pixelcolor::Rgb888,
    prelude::*,
};*/
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};

use blog_os::process;

use blog_os::gui::buffer::FrameBufferDisplay;
use blog_os::gui::{
    graphics::{activate_mouse, compositor_task, setup_desktop},
    window::{Window, WindowManager},
};

static RAM_DISK: &[u8] = include_bytes!("../disk.tar");

// Global Display Driver (Unsafe access required)
static mut GUI_DISPLAY: Option<FrameBufferDisplay> = None;

const BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.frame_buffer.minimum_framebuffer_height = Some(1080);
    config.frame_buffer.minimum_framebuffer_width = Some(1920);
    config.kernel_stack_size = 512 * 1024; //increase the stack size. i think we need to keeep on increasing this stuff unless we stop adding stuff so yeah
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn process_a() {
    loop {
        for _ in 0..1000000 {
            unsafe {
                core::arch::asm!("nop");
            }
        }
        crate::process::sys_yield();
    }
}

fn process_b() {
    loop {
        for _ in 0..1000000 {
            unsafe {
                core::arch::asm!("nop");
            }
        }
        crate::process::sys_yield();
    }
}

fn process_shell() {
    crate::println!("\n -> DEBUG Shell Process Started");
    let mut executor = Executor::new();
    executor.spawn(Task::new(blog_os::cli::run()));
    executor.run();
}

static mut USER_ENTRY: u64 = 0;
static mut USER_STACK: u64 = 0;
static mut USER_PAGE_TABLE: Option<PhysFrame> = None;

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use blog_os::allocator;
    use blog_os::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    blog_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset.into_option().unwrap());
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    unsafe {
        blog_os::vga_buffer::init_vga_offset(phys_mem_offset.as_u64());
    }

    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_regions) };
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    // --- FRAMEBUFFER (replaces VGA fixed **) ---
    // boot_info.framebuffer is now available here for GUI work
    // let fb = boot_info.framebuffer.as_mut().unwrap();
    // ------------------------------------------

    blog_os::fs::FILESYSTEM.init_once(|| Mutex::new(blog_os::fs::OverlayFileSystem::new(RAM_DISK)));
    // === RTL8139 INIT ===
    println!("\n Initializing RTL8139...");
    let mut pci_bus = pci::Pci::new();
    if let Some(mut rtl) = pci::RtlDevice::find(&mut pci_bus) {
        println!("Found RTL8139 at I/O Base: {:#x}", rtl.io_base);
        rtl.enable_bus_mastering(&mut pci_bus);
        let io = rtl.io_base as u16;
        let buf_phys = mapper
            .translate_addr(VirtAddr::from_ptr(unsafe {
                core::ptr::addr_of!(RX_BUFFER) as *const u8
            }))
            .expect("RX_BUFFER not mapped");
        pci::reset_rtl8139(io);
        pci::set_imr_isr(io);
        pci::init_recive_buffer(io, buf_phys.as_u64() as *const u8);
        pci::init_rcr(io);
        pci::enable_reciver(io);
        println!(
            "RTL8139 ready. MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            OUR_MAC[0], OUR_MAC[1], OUR_MAC[2], OUR_MAC[3], OUR_MAC[4], OUR_MAC[5]
        );
        println!(
            "IP: {}.{}.{}.{}",
            OUR_IP[0], OUR_IP[1], OUR_IP[2], OUR_IP[3]
        );
        blog_os::net::set_mac_address(OUR_MAC);
        blog_os::net::set_ip_address(OUR_IP);
    } else {
        println!("RTL8139 not found — network disabled.");
    }
    // === END RTL8139 INIT ===

    // --- INIT GUI ---
    if let Some(framebuffer) = boot_info.framebuffer.as_ref() {
        unsafe {
            // Reconstruct FrameBuffer to get mutable access to the slice
            // CAUTION: This assumes we have exclusive access.
            let info = framebuffer.info();
            let ptr = framebuffer.buffer().as_ptr() as *mut u8;
            let len = framebuffer.buffer().len();
            let slice = core::slice::from_raw_parts_mut(ptr, len);

            GUI_DISPLAY = Some(FrameBufferDisplay::new(slice, info));
        }
        crate::println!("GUI Display Initialized!");
    } else {
        crate::println!("No Framebuffer found! GUI Disabled.");
    }
    // ----------------

    let ptr = &raw mut GUI_DISPLAY;
    let (width, height) = unsafe {
        if let Some(display) = &*ptr {
            (display.size().width as i32, display.size().height as i32)
        } else {
            (1080, 720)
        }
    };
    //crate::println!("width:{width}, height: {height}");
    unsafe {
        if let Some(display) = &mut *ptr {
            // Prepare the off-screen windows in standard RAM
            let wm = setup_desktop(width, height, display.info.bytes_per_pixel);

            // Start the asynchronous task scheduler
            let mut exec = Executor::new();

            exec.spawn(Task::new(blog_os::cli::run()));

            // Spawn the mouse input task
            exec.spawn(Task::new(activate_mouse(width, height)));

            // Spawn the Taskbar
            let taskbar =
                blog_os::gui::taskbar::Taskbar::new(width as u32, display.info.bytes_per_pixel);

            // Spawn the Compositor
            exec.spawn(Task::new(compositor_task(
                display, wm, taskbar, width, height,
            )));

            exec.run();
        }
    }

    #[cfg(test)]
    test_main();

    crate::println!("\n[SCHEDULER] Spawning Processes...");

    let mut p0 = process::Process::new(
        0,
        "Boot",
        4096,
        false,
        &mut frame_allocator,
        phys_mem_offset,
    );
    let mut p1 = process::Process::new(
        1,
        "ProcA",
        4096,
        false,
        &mut frame_allocator,
        phys_mem_offset,
    );
    p1.init_stack(process_a as u64);
    let mut p2 = process::Process::new(
        2,
        "ProcB",
        4096,
        false,
        &mut frame_allocator,
        phys_mem_offset,
    );
    p2.init_stack(process_b as u64);
    let mut p3 = process::Process::new(
        3,
        "Shell",
        32768,
        false,
        &mut frame_allocator,
        phys_mem_offset,
    );
    p3.init_stack(process_shell as u64);

    let mut p_user = process::Process::new(
        5,
        "UserApp",
        32768,
        true,
        &mut frame_allocator,
        phys_mem_offset,
    );
    let fs = blog_os::fs::TarFileSystem::new(RAM_DISK);
    let elf_data_unaligned = fs.read_file("test_app").expect("Could not find test_app");
    use alloc::vec::Vec;
    let elf_data = Vec::from(elf_data_unaligned);

    let entry_point = p_user.load_elf(
        &elf_data,
        &mut mapper,
        &mut frame_allocator,
        phys_mem_offset,
    );
    let user_stack_top = p_user.allocate_user_stack(
        0x5000_0000,
        20 * 1024,
        &mut mapper,
        &mut frame_allocator,
        phys_mem_offset,
    );
    crate::println!("User App loaded at Entry Point: {:#x}", entry_point);

    unsafe {
        USER_ENTRY = entry_point;
        USER_STACK = user_stack_top;
        USER_PAGE_TABLE = Some(p_user.page_table);
    }

    fn user_mode_wrapper() {
        unsafe {
            let entry = USER_ENTRY;
            let stack = USER_STACK;
            let user_page_table = USER_PAGE_TABLE.expect("User page table not set!");
            use x86_64::registers::control::Cr3;
            let flags = Cr3::read().1;
            Cr3::write(user_page_table, flags);
            crate::println!("Wrapper: Jumping to User Mode at {:#x}", entry);
            if entry == 0 {
                blog_os::process::jump_to_userspace(0x10001580, stack);
            } else {
                blog_os::process::jump_to_userspace(entry, stack);
            }
        }
    }
    p_user.init_stack(user_mode_wrapper as u64);

    {
        let mut sched = blog_os::scheduler::SCHEDULER.lock();
        sched.add_process(p0);
        sched.add_process(p1);
        sched.add_process(p2);
        sched.add_process(p3);
        sched.add_process(p_user);
    }

    crate::println!("[SCHEDULER] Starting Multitasking...");

    for _ in 0..100000 {
        unsafe {
            core::arch::asm!("nop");
        }
    }

    x86_64::instructions::interrupts::enable();

    loop {
        x86_64::instructions::hlt();
    }
}
const OUR_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const OUR_IP: [u8; 4] = [10, 0, 2, 15];
static mut RX_BUFFER: [u8; 8192 + 16] = [0; 8192 + 16];

pub unsafe fn test_user_mode(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    fs: &blog_os::fs::TarFileSystem,
) {
    use alloc::vec::Vec;
    use x86_64::structures::paging::PageTableFlags;

    let user_code_selector = gdt::GDT.1.user_code_selector.0;
    let user_data_selector = gdt::GDT.1.user_data_selector.0;

    crate::println!("Preparing to jump to user mode...");

    let data_unaligned = fs
        .read_file("./test_app")
        .expect("Failed to find ./test_app");
    let data_aligned = data_unaligned.to_vec();
    let mut loader = blog_os::elf_loader::ElfLoader::new(&data_aligned, mapper, frame_allocator);
    let entry_point = loader.load();

    let stack_addr = 0x5000_0000u64;
    let stack_page = Page::containing_address(VirtAddr::new(stack_addr));
    let frame = frame_allocator.allocate_frame().unwrap();
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    mapper
        .map_to(stack_page, frame, flags, frame_allocator)
        .unwrap()
        .flush();

    let user_stack_top = stack_addr + 4096;
    crate::println!("Stack mapped at {:#x}", user_stack_top);

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
        data_sel = in(reg) user_data_selector,
        stack_ptr = in(reg) user_stack_top,
        code_sel = in(reg) user_code_selector,
        entry = in(reg) entry_point,
        options(noreturn),
    );
}

#[unsafe(naked)]
extern "C" fn user_mode_function() {
    unsafe {
        naked_asm!("mov rax, 1", "mov rdi, 0xDEAD", "syscall", "2:", "jmp 2b",);
    }
}

async fn async_number() -> u32 {
    42
}

async fn example_task() {
    let number = async_number().await;
    println!("async number: {}", number);
}

async fn test_mouse() {
    let mut mouse_stream = MouseStream::new();
    crate::println!("Move the mouse to see coordinates!");
    while let Some(packet) = mouse_stream.next().await {
        if packet.x != 0 || packet.y != 0 {
            crate::println!(
                "Mouse: X={}, Y={}, Click={}",
                packet.x,
                packet.y,
                packet.left_btn
            );
        }
    }
}

pub async fn print_mouse_packets() {
    let mut mouse_stream = MouseStream::new();
    let mut x_pos = 40;
    let mut y_pos = 12;

    crate::println!("Waiting for mouse movement...");
    while let Some(packet) = mouse_stream.next().await {
        if packet.x != 0 || packet.y != 0 {
            x_pos = (x_pos as i32 + packet.x as i32).clamp(0, 79) as usize;
            y_pos = (y_pos as i32 - packet.y as i32).clamp(0, 24) as usize;
            crate::println!(
                "Mouse Packet: dx={}, dy={} -> Pos({}, {})",
                packet.x,
                packet.y,
                x_pos,
                y_pos
            );
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    blog_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    blog_os::test_panic_handler(info)
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
