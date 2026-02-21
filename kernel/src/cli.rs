// src/cli.rs
use crate::println;
use crate::print;
use crate::task::keyboard::ScancodeStream;
use crate::vga_buffer::{self,Color};
use crate::exit_qemu;
use crate::QemuExitCode;
use futures_util::stream::StreamExt;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1,KeyCode};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
pub async fn run() {
    //crate::println!(" -> [Debug] Inside cli::run()! The Shell is ALIVE!");
    crate::print!("KafkaSH> ");
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore);
    
    // HISTORY STORAGE
    let mut input_buffer = String::new();
    let mut history: Vec<String> = Vec::new();
    let mut history_index = 0; // Points to the "next" slot (end of list)

    // Initial Prompt
    crate::vga_buffer::set_color(crate::vga_buffer::Color::Cyan, crate::vga_buffer::Color::Black);
    print!("> ");
    crate::vga_buffer::set_color(crate::vga_buffer::Color::Yellow, crate::vga_buffer::Color::Black);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => {
                        match character {
                            '\n' => {
                                println!();
                                // Execute
                                execute_command(&input_buffer);
                                
                                // Save to History (if not empty)
                                if !input_buffer.is_empty() {
                                    history.push(input_buffer.clone());
                                }
                                
                                // Reset everything
                                history_index = history.len(); // Reset index to the bottom
                                input_buffer.clear();
                                
                                // New Prompt
                                crate::vga_buffer::set_color(crate::vga_buffer::Color::Cyan, crate::vga_buffer::Color::Black);
                                print!("> ");
                                crate::vga_buffer::set_color(crate::vga_buffer::Color::Yellow, crate::vga_buffer::Color::Black);
                            }
                            '\x08' => { // Backspace
                                if input_buffer.pop().is_some() {
                                    print!("\x08 \x08");
                                }
                            }
                            c => {
                                print!("{}", c);
                                input_buffer.push(c);
                            }
                        }
                    }
                    DecodedKey::RawKey(key) => {
                        match key {
                            KeyCode::ArrowUp => {
                                if history_index > 0 {
                                    // 1. Visually clear current line
                                    for _ in 0..input_buffer.len() {
                                        print!("\x08");
                                    }
                                    
                                    // 2. Move index back
                                    history_index -= 1;
                                    
                                    // 3. Load history
                                    input_buffer = history[history_index].clone();
                                    
                                    // 4. Print it
                                    print!("{}", input_buffer);
                                }
                            }
                            KeyCode::ArrowDown => {
                                if history_index < history.len() {
                                    // 1. Visually clear current line
                                    for _ in 0..input_buffer.len() {
                                        print!("\x08");
                                    }

                                    // 2. Move index forward
                                    history_index += 1;

                                    // 3. Load history OR clear if at bottom
                                    if history_index == history.len() {
                                        input_buffer.clear();
                                    } else {
                                        input_buffer = history[history_index].clone();
                                        print!("{}", input_buffer);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn execute_command(input: &str) {
    let mut parts = input.trim().split_whitespace();
    let command = match parts.next() {
        Some(s) => s,
        None => return, // No command entered
    };

    let mut args = parts; // The rest of the iterator is the arguments

    match command {
        "help" => println!("Available commands: help, version, clear, echo <text>, ls, cat <file>, calc <num> <op> <num>, kafkafetch, shutdown"),
        
        "version" => println!("KafkaOS v0.1.0"),
        
        "clear" => {
            crate::vga_buffer::clear_screen();
        },

        "echo" => {
            for arg in args {
                print!("{} ", arg);
            }
            println!(); 
        },

        // --- FILESYSTEM COMMANDS ---
        "ls" => {
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                let fs_mut = fs.lock();
                let files = fs_mut.list_files();
                println!("Files in the DISK:");
                for file in files {
                    crate::println!(" - {}", file);
                }
            } else {
                println!("File system not initialized");
            }
        },

        "cat" => {
            if let Some(filename) = args.next() {
                if let Some(fs) = crate::fs::FILESYSTEM.get() {
                    match fs.lock().read_file(filename) {
                        Some(data) => {
                            if let Ok(text) = core::str::from_utf8(&data) {
                                println!("{}", text);
                            } else {
                                println!("(Binary file - cannot display content)");
                            }
                        },
                        None => println!("File not found: {}", filename),
                    }
                } else {
                    println!("Error: Filesystem not initialized");
                }
            } else {
                println!("Usage: cat <filename>");
            }
        },

        // --- CALCULATOR ---
        "calc" => {
            let num1_str = match args.next() {
                Some(s) => s,
                None => { println!("Usage: calc <num> <op> <num>"); return; }
            };
            let op = match args.next() {
                Some(s) => s,
                None => { println!("Error: Missing operator."); return; }
            };
            let num2_str = match args.next() {
                Some(s) => s,
                None => { println!("Usage: calc <num> <op> <num>"); return; }
            };

            let num1: i64 = match num1_str.parse() {
                Ok(n) => n,
                Err(_) => { println!("Error: '{}' is not a valid number", num1_str); return; }
            };
            let num2: i64 = match num2_str.parse() {
                Ok(n) => n,
                Err(_) => { println!("Error: '{}' is not a valid number", num2_str); return; }
            };

            match op {
                "+" => println!("Result: {}", num1 + num2),
                "-" => println!("Result: {}", num1 - num2),
                "*" => println!("Result: {}", num1 * num2),
                "/" => {
                    if num2 == 0 { println!("Error: Division by zero!"); } 
                    else { println!("Result: {}", num1 / num2); }
                },
                "%" => println!("Result: {}", num1 % num2),
                _ => println!("Error: Unknown operator '{}'", op),
            }
        },

        "ps" =>{
            crate::println!("Process Status Report");
            let sched = crate::scheduler::SCHEDULER.lock();
            sched.print_process_list(3);
        },

        // --- EXTRAS ---
        "kafkafetch" => {
             // Assuming you have this function defined elsewhere in cli.rs
              print_kafkafetch(); 
             //println!("(KafkaFetch logo would go here)");
        },

        "shutdown" => {
             // Assuming you have this helper, or use the crate::exit_qemu logic
             // system_shutdown_qemu();
             println!("Shutting down...");
             //crate::process::exit_qemu(crate::process::QemuExitCode::Success);
             exit_qemu(QemuExitCode::Success);
        },
        "touch" =>{
            let filename = match args.next(){
                Some(s) =>s,
                None => {crate::println!("Usage: touch <filename>");return;}
            };
            if let Some(fs) = crate::fs::FILESYSTEM.get(){
                fs.lock().write_file(filename,&[]);
                crate::println!("File created :{}",filename);
            }
        },
        "write" =>{
            let filename = match args.next(){
                Some(s) =>s,
                None => {crate::println!("Usage: touch <filename>");return;}
            };
            let mut text = String::new();
            for arg in args {
                text.push_str(arg);
                text.push(' ');
            }
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                // Convert string to bytes and write
                fs.lock().write_file(filename, text.as_bytes());
                crate::println!("Wrote to file: {}", filename);
            }

        },

        "rm" =>{
            let filename = match args.next(){
                Some(s) =>s,
                None => {crate::println!("Usage: touch <filename>");return;}
            };
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                match fs.lock().remove_files(filename) {
                    Ok(_) => crate::println!("File deleted: {}", filename),
                    Err(e) => crate::println!("Error: {}", e),
                }
            }

        },
        // --- CATCH ALL ---
        "" => {}, // Ignore empty Enter keys
        unknown => println!("Unknown command: '{}'", unknown),
    }
}
fn print_kafkafetch() {
    // 1. Define the Logo (Must be fixed width for alignment to look good!)
    let proc_count = crate::scheduler::SCHEDULER.lock().process_count();
    let uptime = crate::interrupts::uptime_seconds();
    let logo_lines = [
        "    _  __      __ _          ",
        "   | |/ /__ _ / _| | ____ _  ",
        "   | ' // _` | |_| |/ / _` | ",
        "   | . \\ (_| |  _|   < (_| | ",
        "   |_|\\_\\__,_|_| |_|\\_\\__,_| ",
    ];
    let stats_lines:Vec<String> = alloc::vec![
        format!("OS:      KafkaOS v0.1.0"),
        format!("Kernel:  Rst Microkernel"),
        format!("Shell:   KafkaSH (PID 3)"),
        format!("Procs:   {} Active Tasks", proc_count),
        format!("Memory:  100 MB Heap (Allocated)"),
        format!("Uptime:  {:.2} seconds", uptime),
    ];

    // 3. Determine how many lines to print (max of logo or stats)
    let num_lines = if logo_lines.len() > stats_lines.len() { 
        logo_lines.len() 
    } else { 
        stats_lines.len() 
    };

    // 4. Print them side-by-side
    for i in 0..num_lines {
        // --- PRINT LOGO PART ---
        crate::vga_buffer::set_color(crate::vga_buffer::Color::Cyan, crate::vga_buffer::Color::Black);
        
        if i < logo_lines.len() {
            print!("{}", logo_lines[i]);
        } else {
            // If logo runs out, print equivalent spaces (approx 29 chars based on logo above)
            print!("                             "); 
        }

        // --- PRINT SEPARATOR ---
        print!("   "); 

        // --- PRINT STATS PART ---
        crate::vga_buffer::set_color(crate::vga_buffer::Color::Yellow, crate::vga_buffer::Color::Black);
        
        if i < stats_lines.len() {
            println!("{}", stats_lines[i]);
        } else {
            // If stats run out, just print a newline
            println!(); 
        }
    }

    // Reset color at the end
    crate::vga_buffer::set_color(crate::vga_buffer::Color::Yellow, crate::vga_buffer::Color::Black);
    println!(); // Extra margin at bottom
}

fn system_shutdown_qemu(){
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0xf4);
    println!("Shutting down KafkaOS...");
    unsafe{
        port.write(0x10 as u32);
    }
    println!("Shutdown failed on emulator");
    loop{
        x86_64::instructions::hlt();
    }
}