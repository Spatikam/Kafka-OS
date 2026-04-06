// src/syscalls.rs

use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;
use core::arch::naked_asm;
use crate::gdt;

// Make sure your interrupts.rs exposes this as public!
// If it's a OnceCell, we might need .get() below.
use crate::task::keyboard::ScancodeStream; // Or wherever your queue lives

pub fn init() {
    unsafe {
        Efer::update(|flags| {
            flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS);
        });

        LStar::write(VirtAddr::new(syscall_handler as usize as u64));
        SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);

        Star::write(
            gdt::GDT.1.user_code_selector,
            gdt::GDT.1.user_data_selector,
            gdt::GDT.1.code_selector,
            gdt::GDT.1.kernel_data_selector,
        );
    }
    crate::println!("KERNEL: Syscalls initialized!");
}

static mut USER_STACK_SCRATCH: u64 = 0;
const SYSCALL_STACK_SIZE: usize = 4096 * 4;
static mut SYSCALL_STACK: [u8; SYSCALL_STACK_SIZE] = [0; SYSCALL_STACK_SIZE];

#[unsafe(naked)]
extern "C" fn syscall_handler() {
    unsafe {
        naked_asm!(
            "mov [{user_stack_scratch}], rsp",      // Save User Stack
            "lea rsp, [{syscall_stack} + {stack_size}]", // Switch to Kernel Stack
            
            "push r11", "push rcx", "push rbp", "push rdi", "push rsi", 
            "push rdx", "push r8",  "push r9",  "push r10", "push r12", 
            "push r13", "push r14", "push r15",

            // --- ARGUMENT SHUFFLE  ---
            // User (Sys V): RAX=ID, RDI=Arg1, RSI=Arg2
            // Rust (Fn):    RDI=ID, RSI=Arg1, RDX=Arg2
            "mov rdx, rsi",   // Arg2 -> RDX
            "mov rsi, rdi",   // Arg1 -> RSI
            "mov rdi, rax",   // ID   -> RDI
            
            "call {rust_handler}",

            "pop r15", "pop r14", "pop r13", "pop r12", "pop r10", 
            "pop r9",  "pop r8",  "pop rdx", "pop rsi", "pop rdi", 
            "pop rbp", "pop rcx", "pop r11",

            "mov rsp, [{user_stack_scratch}]",      // Restore User Stack
            "sysretq",

            user_stack_scratch = sym USER_STACK_SCRATCH,
            syscall_stack = sym SYSCALL_STACK,
            stack_size = const SYSCALL_STACK_SIZE,
            rust_handler = sym wrapped_syscall_handler,
        );
    }
}
static BRK_CURRENT:spin::Mutex<u64> = spin::Mutex::new(0x1000_0000);
#[unsafe(no_mangle)]
extern "C" fn wrapped_syscall_handler(id: u64, arg1: u64, arg2: u64) -> u64 {
    match id {
        // SYSCALL 1: PRINT STRING (Ptr, Len)
        1 => {
            let ptr = arg1 as *const u8;
            let len = arg2 as usize;
            // UNSAFE: We trust the user sent a valid pointer for now
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            let s = core::str::from_utf8(slice).unwrap_or("[Invalid UTF-8]");
            crate::print!("{}", s); // Print to Kernel VGA
            0
        }

        // SYSCALL 2: READ CHAR
        // Note: This requires your KEYBOARD_QUEUE to be accessible.
        // If it's intricate, we can return 0 for now to test printing first.
        2 => {
            0 
        }

        // SYSCALL 3: SHUTDOWN
        3 => {
            crate::println!("SYSCALL: Shutting down...");
            use x86_64::instructions::port::Port;
            let mut port = Port::new(0xf4);
            unsafe { port.write(0x10 as u32); }
            0
        }
        10 =>{
            let sched = crate::scheduler::SCHEDULER.lock();
            sched.current_pid().unwrap_or(0)
        }
        11 =>{
            crate::process::sys_yield();
            0
        }
        12 =>{
            crate::serial_println!("SYSCALL: Process exiting with code {}", arg1);
            let mut sched = crate::scheduler::SCHEDULER.lock();
            sched.exit_current(arg1);
            drop(sched);
            crate::println!("KERNEL:No more process halting");
            loop {x86_64::instructions::hlt();}
        }

        // UNKNOWN
        _ => {
            crate::println!("KERNEL: Unknown syscall: {}", id);
            u64::MAX
        }
    }
}