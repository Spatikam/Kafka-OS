// src/interrupts.rs
use crate::{gdt, hlt_loop, print, println};
use crate::task::keyboard::WAKER;
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crossbeam_queue::ArrayQueue;
use core::sync::atomic::{AtomicU64,Ordering};
use crate::gui::graphics;

pub static TICKS:AtomicU64=AtomicU64::new(0);


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
    ticks as f64 / 18.2 
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
    crate::serial_println!("====== EXCEPTION: PAGE FAULT ======");
    crate::serial_println!("Accessed Address : {:?}", Cr2::read());
    crate::serial_println!("Error Code       : {:?}", error_code);
    crate::serial_println!("{:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

// --- 4. Hardware Interrupt Handlers ---

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // print!("."); // Uncomment to see timer ticks
    TICKS.fetch_add(1,Ordering::Relaxed);

    // Increment the heartbeat
    let ticks = graphics::PIT_TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    
    // Every ~1 second (18 ticks), ring the alarm clock!
    if ticks % 18 == 0 {
        graphics::CLOCK_TICK.store(true, core::sync::atomic::Ordering::Relaxed);
        
        // Wake up the Compositor!
        if let Some(waker) = crate::gui::graphics::COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }
    }
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
    crate::process::sys_yield();
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode:u8 = unsafe {port.read()};
    crate::task::keyboard::add_scancode(scancode);
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