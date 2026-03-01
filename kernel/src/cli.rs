// src/cli.rs
use crate::{tprint, tprintln};
use crate::task::keyboard::ScancodeStream;
use crate::exit_qemu;
use crate::QemuExitCode;
use crate::gui::terminal::{self, COLOR_CYAN, COLOR_YELLOW, COLOR_GREEN, COLOR_WHITE, COLOR_RED};
use crate::gui::notepad;
use futures_util::stream::StreamExt;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1, KeyCode};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

pub async fn run() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );

    let mut input_buffer = String::new();
    let mut history: Vec<String> = Vec::new();
    let mut history_index: usize = 0;

    // we need to track these ourselves for notepad shortcuts, would help use to trigger some func.
    let mut ctrl_pressed = false;
    let mut shift_pressed = false;

    print_prompt();

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            // track modifier keys from the raw event
            let is_down = key_event.state == pc_keyboard::KeyState::Down;
            match key_event.code {
                KeyCode::LControl | KeyCode::RControl => {
                    ctrl_pressed = is_down;
                }
                KeyCode::LShift | KeyCode::RShift => {
                    shift_pressed = is_down;
                }
                _ => {}
            }

            // always process key event (keeps keyboard decoder state in sync)
            if let Some(key) = keyboard.process_keyevent(key_event) {

                
                // if notepad is open, send all keys there instead of the terminal  // yeah this is a bit of conflict
                // for now if this is open, we can't access the terminal.. we kinda need to do some kind of sharing.
                // so when we make the stuff dynammic, i will reroute this.. 
                if notepad::is_active() {
                    // check for escape to close notepad
                    let is_escape = matches!(
                        key,
                        DecodedKey::Unicode('\x1B') | DecodedKey::RawKey(KeyCode::Escape)
                    );

                    if is_escape {
                        // only close if we're in editing mode (not typing a filename)
                        let in_prompt = {
                            let state = notepad::NOTEPAD_STATE.lock();
                            state.mode != crate::gui::notepad::Mode::Editing
                        };
                        if in_prompt {
                            // let notepad handle it (cancel the prompt)
                            notepad::handle_key(key, ctrl_pressed, shift_pressed);
                        } else {
                            notepad::close_notepad();
                        }
                    } else {
                        notepad::handle_key(key, ctrl_pressed, shift_pressed);
                    }
                    continue; 
                }
                match key {
                    DecodedKey::Unicode(character) => match character {
                        '\n' => {
                            tprintln!();
                            execute_command(&input_buffer);

                            if !input_buffer.is_empty() {
                                history.push(input_buffer.clone());
                            }
                            history_index = history.len();
                            input_buffer.clear();

                            print_prompt();
                        }
                        '\x08' => {
                            if input_buffer.pop().is_some() {
                                tprint!("\x08");
                            }
                        }
                        c => {
                            input_buffer.push(c);
                            terminal::set_terminal_color(COLOR_YELLOW);
                            tprint!("{}", c);
                        }
                    },
                    DecodedKey::RawKey(key) => match key {
                        KeyCode::ArrowUp => {
                            if history_index > 0 {
                                for _ in 0..input_buffer.len() {
                                    tprint!("\x08");
                                }
                                history_index -= 1;
                                input_buffer = history[history_index].clone();
                                terminal::set_terminal_color(COLOR_YELLOW);
                                tprint!("{}", input_buffer);
                            }
                        }
                        KeyCode::ArrowDown => {
                            if history_index < history.len() {
                                for _ in 0..input_buffer.len() {
                                    tprint!("\x08");
                                }
                                history_index += 1;
                                if history_index == history.len() {
                                    input_buffer.clear();
                                } else {
                                    input_buffer = history[history_index].clone();
                                    terminal::set_terminal_color(COLOR_YELLOW);
                                    tprint!("{}", input_buffer);
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}

fn print_prompt() {
    terminal::set_terminal_color(COLOR_GREEN);
    tprint!("KafkaSH");
    terminal::set_terminal_color(COLOR_CYAN);
    tprint!("> ");
    terminal::set_terminal_color(COLOR_YELLOW);
}

fn execute_command(input: &str) {
    let mut parts = input.trim().split_whitespace();
    let command = match parts.next() {
        Some(s) => s,
        None => return,
    };
    let mut args = parts;

    terminal::set_terminal_color(COLOR_GREEN);

    match command {
        "help" => tprintln!(
            "Commands: help version clear echo ls cat calc kafkafetch shutdown touch write rm ps"
        ),

        "version" => tprintln!("KafkaOS v0.1.0"),

        "clear" => {
            terminal::clear_terminal();
        }

        "echo" => {
            for arg in args {
                tprint!("{} ", arg);
            }
            tprintln!();
        }

        "ls" => {
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                let fs_mut = fs.lock();
                let files = fs_mut.list_files();
                tprintln!("Files in the DISK:");
                for file in files {
                    tprintln!(" - {}", file);
                }
            } else {
                tprintln!("File system not initialized");
            }
        }

        "cat" => {
            if let Some(filename) = args.next() {
                if let Some(fs) = crate::fs::FILESYSTEM.get() {
                    match fs.lock().read_file(filename) {
                        Some(data) => {
                            if let Ok(text) = core::str::from_utf8(&data) {
                                tprintln!("{}", text);
                            } else {
                                tprintln!("(Binary file)");
                            }
                        }
                        None => tprintln!("File not found: {}", filename),
                    }
                } else {
                    tprintln!("Error: Filesystem not initialized");
                }
            } else {
                tprintln!("Usage: cat <filename>");
            }
        }

        "calc" => {
            let num1_str = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: calc <n> <op> <n>"); return; }
            };
            let op = match args.next() {
                Some(s) => s,
                None => { tprintln!("Error: Missing operator."); return; }
            };
            let num2_str = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: calc <n> <op> <n>"); return; }
            };
            let num1: i64 = match num1_str.parse() {
                Ok(n) => n,
                Err(_) => { tprintln!("Error: '{}' NaN", num1_str); return; }
            };
            let num2: i64 = match num2_str.parse() {
                Ok(n) => n,
                Err(_) => { tprintln!("Error: '{}' NaN", num2_str); return; }
            };
            match op {
                "+" => tprintln!("Result: {}", num1 + num2),
                "-" => tprintln!("Result: {}", num1 - num2),
                "*" => tprintln!("Result: {}", num1 * num2),
                "/" => {
                    if num2 == 0 { tprintln!("Error: Division by zero!"); }
                    else { tprintln!("Result: {}", num1 / num2); }
                }
                "%" => tprintln!("Result: {}", num1 % num2),
                _ => tprintln!("Error: Unknown op '{}'", op),
            }
        }

        "ps" => {
            tprintln!("Process Status Report");
            // NOTE: print_process_list uses println! internally,
            // its output goes to serial. Update it later if needed.
            let sched = crate::scheduler::SCHEDULER.lock();
            sched.print_process_list(3);
        }

        "kafkafetch" => print_kafkafetch(),

        "shutdown" => {
            tprintln!("Shutting down...");
            exit_qemu(QemuExitCode::Success);
        }

        "touch" => {
            let filename = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: touch <filename>"); return; }
            };
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                fs.lock().write_file(filename, &[]);
                tprintln!("File created: {}", filename);
            }
        }

        "write" => {
            let filename = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: write <file> <text>"); return; }
            };
            let mut text = String::new();
            for arg in args {
                text.push_str(arg);
                text.push(' ');
            }
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                fs.lock().write_file(filename, text.as_bytes());
                tprintln!("Wrote to file: {}", filename);
            }
        }

        "rm" => {
            let filename = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: rm <filename>"); return; }
            };
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                match fs.lock().remove_files(filename) {
                    Ok(_) => tprintln!("Deleted: {}", filename),
                    Err(e) => tprintln!("Error: {}", e),
                }
            }
        }
        "notepad" => {
            let bpp = notepad::get_bpp();
            if let Some(filename) = args.next(){
                tprintln!("Opening {}",filename);
                notepad::open_notepad_with_file(bpp, filename);
            }else{
                tprintln!("Opening Notepad...");
                notepad::open_notepad(bpp);
            }
        }

        "" => {}
        unknown => tprintln!("Unknown command: '{}'", unknown),
    }
}

fn print_kafkafetch() {
    let proc_count = crate::scheduler::SCHEDULER.lock().process_count();
    let uptime = crate::interrupts::uptime_seconds();

    terminal::set_terminal_color(COLOR_CYAN);
    tprintln!("  _  __      __ _");
    tprintln!(" | |/ /__ _ / _| | ____ _");
    tprintln!(" | ' // _` | |_| |/ / _` |");
    tprintln!(" | . \\ (_| |  _|   < (_| |");
    tprintln!(" |_|\\_\\__,_|_| |_|\\_\\__,_|");

    terminal::set_terminal_color(COLOR_YELLOW);
    tprintln!();
    tprintln!(" OS:      KafkaOS v0.1.0");
    tprintln!(" Kernel:  Rst Microkernel");
    tprintln!(" Shell:   KafkaSH");
    tprintln!(" Procs:   {} Active Tasks", proc_count);
    tprintln!(" Memory:  100 MB Heap");
    tprintln!(" Uptime:  {:.2}s", uptime);

    terminal::set_terminal_color(COLOR_GREEN);
    tprintln!();
}