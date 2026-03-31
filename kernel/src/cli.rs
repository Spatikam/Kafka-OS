// src/cli.rs
use crate::QemuExitCode;
use crate::exit_qemu;
use crate::gui::notepad;
use crate::gui::terminal::{self, COLOR_CYAN, COLOR_GREEN, COLOR_RED, COLOR_WHITE, COLOR_YELLOW};
use crate::task::keyboard::ScancodeStream;
use crate::{tprint, tprintln};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use futures_util::stream::StreamExt;
use pc_keyboard::{DecodedKey, HandleControl, KeyCode, Keyboard, ScancodeSet1, layouts};

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

    // we need to track these ourselves for notepad shortcuts, would help use to trigger some func.
    let mut ctrl_pressed = false;
    let mut shift_pressed = false;

    print_prompt(&cwd);

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
                            execute_command(&input_buffer, &mut cwd).await;
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
async fn yield_now() {
    struct YieldOnce(bool);
    impl core::future::Future for YieldOnce {
        type Output = ();
        fn poll(
            mut self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context,
        ) -> core::task::Poll<()> {
            if self.0 {
                core::task::Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                core::task::Poll::Pending
            }
        }
    }
    YieldOnce(false).await
}

async fn execute_command(input: &str, cwd: &mut String) {
    let mut parts = input.trim().split_whitespace();
    let command = match parts.next() {
        Some(s) => s,
        None => return,
    };
    let mut args = parts;

    terminal::set_terminal_color(COLOR_GREEN);

    match command {
        "help" => tprintln!(
            "Commands: help version clear echo ls cd cat calc kafkafetch ifconfig ping shutdown touch write rm ps notepad"
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
                None => {
                    tprintln!("Usage: calc <n> <op> <n>");
                    return;
                }
            };
            let op = match args.next() {
                Some(s) => s,
                None => {
                    tprintln!("Error: Missing operator.");
                    return;
                }
            };
            let num2_str = match args.next() {
                Some(s) => s,
                None => {
                    tprintln!("Usage: calc <n> <op> <n>");
                    return;
                }
            };
            let num1: i64 = match num1_str.parse() {
                Ok(n) => n,
                Err(_) => {
                    tprintln!("Error: '{}' NaN", num1_str);
                    return;
                }
            };
            let num2: i64 = match num2_str.parse() {
                Ok(n) => n,
                Err(_) => {
                    tprintln!("Error: '{}' NaN", num2_str);
                    return;
                }
            };
            match op {
                "+" => tprintln!("Result: {}", num1 + num2),
                "-" => tprintln!("Result: {}", num1 - num2),
                "*" => tprintln!("Result: {}", num1 * num2),
                "/" => {
                    if num2 == 0 {
                        tprintln!("Error: Division by zero!");
                    } else {
                        tprintln!("Result: {}", num1 / num2);
                    }
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
                None => {
                    tprintln!("Usage: touch <filename>");
                    return;
                }
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
                None => {
                    tprintln!("Usage: write <file> <text>");
                    return;
                }
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
                None => {
                    tprintln!("Usage: rm <filename>");
                    return;
                }
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
        "notepad" => {
            let bpp = notepad::get_bpp();
            if let Some(filename) = args.next() {
                tprintln!("Opening {}", filename);
                notepad::open_notepad_with_file(bpp, filename);
            } else {
                tprintln!("Opening Notepad...");
                notepad::open_notepad(bpp);
            }
        }
        "ifconfig" => {
            if let (Some(mac), Some(ip)) =
                (crate::net::get_mac_address(), crate::net::get_ip_address())
            {
                let rx_count = crate::net::get_rx_count();
                let tx_count = crate::net::get_tx_count();
                terminal::set_terminal_color(COLOR_CYAN);
                tprintln!("eth0: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>  mtu 1500");
                terminal::set_terminal_color(COLOR_YELLOW);
                tprintln!(
                    "        inet {}.{}.{}.{}  netmask 255.255.255.0  broadcast 10.0.2.255",
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                );
                tprintln!(
                    "        ether {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}  txqueuelen 1000",
                    mac[0],
                    mac[1],
                    mac[2],
                    mac[3],
                    mac[4],
                    mac[5]
                );
                tprintln!("        RX packets {}  bytes {}", rx_count, rx_count * 64);
                tprintln!("        TX packets {}  bytes {}", tx_count, tx_count * 64);
            } else {
                terminal::set_terminal_color(COLOR_RED);
                tprintln!("eth0: network not initialized");
            }
        }

        "ping" => {
            let target = match args.next() {
                Some(s) => s,
                None => {
                    tprintln!("Usage: ping <ip>");
                    return;
                }
            };
            let parts: alloc::vec::Vec<&str> = target.split('.').collect();
            if parts.len() != 4 {
                tprintln!("Error: invalid IP '{}'", target);
                return;
            }
            let mut ip = [0u8; 4];
            let mut parse_ok = true;
            for (i, p) in parts.iter().enumerate() {
                match p.parse::<u8>() {
                    Ok(n) => ip[i] = n,
                    Err(_) => {
                        parse_ok = false;
                        break;
                    }
                }
            }
            if !parse_ok {
                tprintln!("Error: invalid IP '{}'", target);
                return;
            }

            tprintln!("PING {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
            yield_now().await;

            // drain stale packets
            for _ in 0..10 {
                if let Some(io) = crate::net::get_io_base() {
                    unsafe {
                        let rx = &*core::ptr::addr_of!(crate::net::pci::RX_BUFFER);
                        crate::net::pci::rtl8139_handler(io, &rx.0);
                    }
                }
                yield_now().await;
            }

            // ARP resolve
            if crate::net::arp::lookup(ip).is_none() {
                crate::net::arp::send_request(ip);
                for _ in 0..200 {
                    if let Some(io) = crate::net::get_io_base() {
                        unsafe {
                            let rx = &*core::ptr::addr_of!(crate::net::pci::RX_BUFFER);
                            crate::net::pci::rtl8139_handler(io, &rx.0);
                        }
                    }
                    yield_now().await;
                    if crate::net::arp::lookup(ip).is_some() {
                        break;
                    }
                }
            }

            tprintln!("ARP resolved, starting ping...");
            yield_now().await;

            for seq in 1u16..=4 {
                crate::net::icmp::take_ping_reply();
                crate::net::icmp::send_ping(ip, seq);
                let mut got_reply = false;
                for _ in 0..2000 {
                    if let Some(io) = crate::net::get_io_base() {
                        unsafe {
                            let rx = &*core::ptr::addr_of!(crate::net::pci::RX_BUFFER);
                            crate::net::pci::rtl8139_handler(io, &rx.0);
                        }
                    }
                    yield_now().await;
                    if let Some((reply_seq, reply_ip)) = crate::net::icmp::take_ping_reply() {
                        if reply_seq == seq {
                            tprintln!(
                                "Reply from {}.{}.{}.{}: seq={}",
                                reply_ip[0],
                                reply_ip[1],
                                reply_ip[2],
                                reply_ip[3],
                                reply_seq
                            );
                            got_reply = true;
                            break;
                        }
                    }
                }
                if !got_reply {
                    tprintln!("Request timeout for seq={}", seq);
                }
                yield_now().await;
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
