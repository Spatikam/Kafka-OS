use super::tcp;
use alloc::string::String;
use alloc::vec::Vec;

async fn yield_now() {
    struct Y(bool);
    impl core::future::Future for Y {
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
    Y(false).await
}

fn poll_rx() {
    if let Some(io) = crate::net::get_io_base() {
        unsafe {
            let rx = &*core::ptr::addr_of!(crate::net::pci::RX_BUFFER);
            crate::net::pci::rtl8139_handler(io, &rx.0);
        }
    }
}

pub async fn get(host: &str, path: &str, ip: [u8; 4]) -> Option<Vec<u8>> {
    tcp::reset();

    // Drain any leftover packets from previous connection
    for _ in 0..10 {
        poll_rx();
        yield_now().await;
    }

    // ARP warm-up — only if not already known
    if crate::net::arp::lookup(ip).is_none() {
        crate::net::arp::send_request(ip);
        for _ in 0..50 {
            poll_rx();
            yield_now().await;
            if crate::net::arp::lookup(ip).is_some() {
                break;
            }
        }
    }

    tcp::connect(ip, 80);

    // Wait for ESTABLISHED
    let mut connected = false;
    for _ in 0..400 {
        poll_rx();
        yield_now().await;
        match tcp::get_state() {
            tcp::State::Established => {
                connected = true;
                break;
            }
            tcp::State::Done | tcp::State::Closed => break,
            _ => {}
        }
    }

    if !connected {
        crate::println!(
            "HTTP: TCP connect timeout (state={})",
            tcp::get_state_name()
        );
        tcp::reset();
        return None;
    }

    crate::println!("HTTP: connected, sending request");

    let request = alloc::format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path,
        host
    );
    tcp::send_data(request.as_bytes());

    // Wait for response
    for _ in 0..2000 {
        poll_rx();
        yield_now().await;
        if tcp::is_done() {
            break;
        }
    }

    tcp::close();

    // Drain after close
    for _ in 0..50 {
        poll_rx();
        yield_now().await;
    }

    tcp::reset();

    let data = tcp::take_rx_data();
    if data.is_empty() {
        crate::println!("HTTP: no data received");
        None
    } else {
        Some(data)
    }
}

pub fn strip_headers(response: &[u8]) -> &[u8] {
    for i in 0..response.len().saturating_sub(3) {
        if &response[i..i + 4] == b"\r\n\r\n" {
            return &response[i + 4..];
        }
    }
    response
}

pub fn strip_html(html: &[u8]) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut last_was_space = false;

    for &b in html {
        match b {
            b'<' => {
                in_tag = true;
            }
            b'>' => {
                in_tag = false;
                out.push('\n');
                last_was_space = false;
            }
            _ if in_tag => {}
            b'\n' | b'\r' => {
                if !last_was_space {
                    out.push('\n');
                    last_was_space = true;
                }
            }
            b' ' | b'\t' => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            c => {
                out.push(c as char);
                last_was_space = false;
            }
        }
    }
    out
}
