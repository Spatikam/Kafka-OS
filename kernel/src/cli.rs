// src/cli.rs
use crate::{tprint, tprintln};
use crate::task::keyboard::ScancodeStream;
use crate::exit_qemu;
use crate::QemuExitCode;
use crate::gui::terminal::{self, COLOR_CYAN, COLOR_YELLOW, COLOR_GREEN, COLOR_WHITE, COLOR_RED};
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
    let mut cwd = String::from("/");

    print_prompt(&cwd);

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => match character {
                        '\n' => {
                            tprintln!();
                            execute_command(&input_buffer, &mut cwd);

                            if !input_buffer.is_empty() {
                                history.push(input_buffer.clone());
                            }
                            history_index = history.len();
                            input_buffer.clear();

                            print_prompt(&cwd);
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

fn print_prompt(cwd: &str) {
    terminal::set_terminal_color(COLOR_GREEN);
    tprint!("KafkaSH");
    terminal::set_terminal_color(COLOR_WHITE);
    tprint!(":{}", cwd);
    terminal::set_terminal_color(COLOR_CYAN);
    tprint!("> ");
    terminal::set_terminal_color(COLOR_YELLOW);
}

fn execute_command(input: &str, cwd: &mut String) {
    let mut parts = input.trim().split_whitespace();
    let command = match parts.next() {
        Some(s) => s,
        None => return,
    };
    let mut args = parts;

    terminal::set_terminal_color(COLOR_GREEN);

    match command {
        "help" => tprintln!(
            "Commands: help version clear echo ls cd cat calc kafkafetch shutdown touch write rm ps"
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

        "cd" => {
            let target = match args.next() {
                Some(s) => s,
                None => {
                    *cwd = String::from("/");
                    return;
                }
            };

            if target == "/" {
                *cwd = String::from("/");
            } else if target == ".." {
                if *cwd != "/" {
                    let trimmed = cwd.trim_end_matches('/');
                    if let Some(pos) = trimmed.rfind('/') {
                        if pos == 0 {
                            *cwd = String::from("/");
                        } else {
                            *cwd = String::from(&trimmed[..pos]);
                            if !cwd.ends_with('/') {
                                cwd.push('/');
                            }
                        }
                    }
                }
            } else {
                let new_path = if target.starts_with('/') {
                    let mut p = String::from(target);
                    if !p.ends_with('/') {
                        p.push('/');
                    }
                    p
                } else {
                    let mut p = cwd.clone();
                    p.push_str(target);
                    if !p.ends_with('/') {
                        p.push('/');
                    }
                    p
                };

                if let Some(fs) = crate::fs::FILESYSTEM.get() {
                    let fs_lock = fs.lock();
                    let files = fs_lock.list_files();
                    let prefix = new_path.trim_start_matches('/');
                    let exists = files.iter().any(|f| f.starts_with(prefix));
                    if exists {
                        *cwd = new_path;
                    } else {
                        tprintln!("No such directory: {}", target);
                    }
                }
            }
        }

        "ls" => {
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                let fs_lock = fs.lock();
                let files = fs_lock.list_files();
                let prefix = cwd.trim_start_matches('/');

                let mut entries: Vec<String> = Vec::new();

                for file in &files {
                    let relative = if prefix.is_empty() {
                        file.as_str()
                    } else if let Some(rest) = file.strip_prefix(prefix) {
                        rest
                    } else {
                        continue;
                    };

                    let entry = if let Some(slash_pos) = relative.find('/') {
                        format!("{}/", &relative[..slash_pos])
                    } else {
                        String::from(relative)
                    };

                    if !entry.is_empty() && !entries.contains(&entry) {
                        entries.push(entry);
                    }
                }

                if entries.is_empty() {
                    tprintln!("(empty)");
                } else {
                    for entry in &entries {
                        if entry.ends_with('/') {
                            terminal::set_terminal_color(COLOR_CYAN);
                        } else {
                            terminal::set_terminal_color(COLOR_WHITE);
                        }
                        tprintln!("  {}", entry);
                    }
                }
            } else {
                tprintln!("File system not initialized");
            }
        }

        "cat" => {
            if let Some(filename) = args.next() {
                let full_path = if filename.starts_with('/') {
                    String::from(filename.trim_start_matches('/'))
                } else {
                    let prefix = cwd.trim_start_matches('/');
                    format!("{}{}", prefix, filename)
                };

                if let Some(fs) = crate::fs::FILESYSTEM.get() {
                    match fs.lock().read_file(&full_path) {
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
            let full_path = if filename.starts_with('/') {
                String::from(filename.trim_start_matches('/'))
            } else {
                let prefix = cwd.trim_start_matches('/');
                format!("{}{}", prefix, filename)
            };
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                fs.lock().write_file(&full_path, &[]);
                tprintln!("File created: {}", filename);
            }
        }

        "write" => {
            let filename = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: write <file> <text>"); return; }
            };
            let full_path = if filename.starts_with('/') {
                String::from(filename.trim_start_matches('/'))
            } else {
                let prefix = cwd.trim_start_matches('/');
                format!("{}{}", prefix, filename)
            };
            let mut text = String::new();
            for arg in args {
                text.push_str(arg);
                text.push(' ');
            }
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                fs.lock().write_file(&full_path, text.as_bytes());
                tprintln!("Wrote to file: {}", filename);
            }
        }

        "rm" => {
            let filename = match args.next() {
                Some(s) => s,
                None => { tprintln!("Usage: rm <filename>"); return; }
            };
            let full_path = if filename.starts_with('/') {
                String::from(filename.trim_start_matches('/'))
            } else {
                let prefix = cwd.trim_start_matches('/');
                format!("{}{}", prefix, filename)
            };
            if let Some(fs) = crate::fs::FILESYSTEM.get() {
                match fs.lock().remove_files(&full_path) {
                    Ok(_) => tprintln!("Deleted: {}", filename),
                    Err(e) => tprintln!("Error: {}", e),
                }
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