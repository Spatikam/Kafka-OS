use crate::net::ethernet::build as eth_build;
use crate::net::ipv4;
use alloc::vec::Vec;
use spin::Mutex;

static TCP_STATE: Mutex<TcpConn> = Mutex::new(TcpConn::new());

// Increments each connection so SLIRP never sees TIME_WAIT collision
static SRC_PORT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(49152);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Closed,
    SynSent,
    Established,
    FinWait,
    Done,
}

pub struct TcpConn {
    pub state: State,
    pub src_port: u16,
    pub dst_port: u16,
    pub dst_ip: [u8; 4],
    pub seq: u32,
    pub ack: u32,
}

impl TcpConn {
    pub const fn new() -> Self {
        Self {
            state: State::Closed,
            src_port: 0,
            dst_port: 0,
            dst_ip: [0; 4],
            seq: 0,
            ack: 0,
        }
    }
}

static RX_DATA: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static CONN_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn reset() {
    CONN_DONE.store(false, core::sync::atomic::Ordering::SeqCst);
    RX_DATA.lock().clear();
    let mut c = TCP_STATE.lock();
    *c = TcpConn::new();
    // NOTE: do NOT reset SRC_PORT — it keeps incrementing across connections
}

pub fn connect(dst_ip: [u8; 4], dst_port: u16) {
    CONN_DONE.store(false, core::sync::atomic::Ordering::SeqCst);
    RX_DATA.lock().clear();

    // fresh port every connection — avoids SLIRP TIME_WAIT
    let src_port = SRC_PORT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

    {
        let mut c = TCP_STATE.lock();
        c.state = State::SynSent;
        c.src_port = src_port;
        c.dst_port = dst_port;
        c.dst_ip = dst_ip;
        c.seq = 0x12345678;
        c.ack = 0;
    }
    send_flags(0x02); // SYN
}

pub fn send_data(data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let (seq, ack, src_port, dst_port, dst_ip) = {
        let c = TCP_STATE.lock();
        (c.seq, c.ack, c.src_port, c.dst_port, c.dst_ip)
    };
    send_tcp_packet(dst_ip, src_port, dst_port, seq, ack, 0x18, data);
    let mut c = TCP_STATE.lock();
    c.seq = c.seq.wrapping_add(data.len() as u32);
}

pub fn close() {
    {
        let c = TCP_STATE.lock();
        if c.state == State::Done || c.state == State::Closed {
            return;
        }
    }
    send_flags(0x11); // FIN+ACK
    let mut c = TCP_STATE.lock();
    c.state = State::FinWait;
}

pub fn get_state() -> State {
    TCP_STATE.lock().state
}

pub fn get_state_name() -> &'static str {
    match TCP_STATE.lock().state {
        State::Closed => "Closed",
        State::SynSent => "SynSent",
        State::Established => "Established",
        State::FinWait => "FinWait",
        State::Done => "Done",
    }
}

pub fn take_rx_data() -> Vec<u8> {
    core::mem::take(&mut *RX_DATA.lock())
}

pub fn is_done() -> bool {
    CONN_DONE.load(core::sync::atomic::Ordering::SeqCst)
}

pub fn handle_packet(src_ip: [u8; 4], tcp_data: &[u8]) {
    if tcp_data.len() < 20 {
        return;
    }

    let src_port = u16::from_be_bytes([tcp_data[0], tcp_data[1]]);
    let dst_port = u16::from_be_bytes([tcp_data[2], tcp_data[3]]);
    let seq = u32::from_be_bytes([tcp_data[4], tcp_data[5], tcp_data[6], tcp_data[7]]);
    let ack_num = u32::from_be_bytes([tcp_data[8], tcp_data[9], tcp_data[10], tcp_data[11]]);
    let data_off = ((tcp_data[12] >> 4) as usize) * 4;
    let flags = tcp_data[13];

    crate::println!(
        "TCP rx: {}.{}.{}.{}:{} flags={:#x}",
        src_ip[0],
        src_ip[1],
        src_ip[2],
        src_ip[3],
        src_port,
        flags
    );

    let payload = if data_off <= tcp_data.len() {
        &tcp_data[data_off..]
    } else {
        &[]
    };

    let mut c = TCP_STATE.lock();

    // ← KEY FIX: drop everything when we're not in an active connection
    if c.state == State::Closed {
        return;
    }

    if c.dst_ip != src_ip || c.src_port != dst_port {
        return;
    }

    match c.state {
        State::SynSent => {
            if flags & 0x12 == 0x12 {
                c.ack = seq.wrapping_add(1);
                c.seq = ack_num;
                c.state = State::Established;
                drop(c);
                send_flags(0x10);
                crate::println!("TCP: ESTABLISHED");
            } else if flags & 0x04 != 0 {
                crate::println!("TCP: RST received, connection refused");
                TCP_STATE.lock().state = State::Closed;
            }
        }
        State::Established => {
            if !payload.is_empty() {
                let new_ack = seq.wrapping_add(payload.len() as u32);
                c.ack = new_ack;
                drop(c);
                RX_DATA.lock().extend_from_slice(payload);
                send_flags(0x10);
            }
            if flags & 0x01 != 0 {
                let mut c2 = TCP_STATE.lock();
                c2.ack = c2.ack.wrapping_add(1);
                c2.state = State::Done;
                drop(c2);
                send_flags(0x11);
                CONN_DONE.store(true, core::sync::atomic::Ordering::SeqCst);
                crate::println!("TCP: FIN received, DONE");
            }
        }
        State::FinWait => {
            if flags & 0x01 != 0 {
                c.state = State::Done;
                drop(c);
                CONN_DONE.store(true, core::sync::atomic::Ordering::SeqCst);
                crate::println!("TCP: FinWait FIN received, DONE");
            }
        }
        State::Done => {
            // ignore everything — connection is over
            return;
        }
        _ => {}
    }
}
fn send_flags(flags: u8) {
    let (seq, ack, src_port, dst_port, dst_ip) = {
        let c = TCP_STATE.lock();
        (c.seq, c.ack, c.src_port, c.dst_port, c.dst_ip)
    };
    send_tcp_packet(dst_ip, src_port, dst_port, seq, ack, flags, &[]);
}

fn send_tcp_packet(
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    data: &[u8],
) {
    let src_ip = crate::net::get_ip_address().unwrap_or([10, 0, 2, 15]);
    let src_mac = crate::net::get_mac_address().unwrap_or([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let dst_mac = crate::net::arp::lookup(dst_ip).unwrap_or([0x52, 0x55, 0x0a, 0x00, 0x02, 0x02]);

    let mut tcp = Vec::with_capacity(20 + data.len());
    tcp.extend_from_slice(&src_port.to_be_bytes());
    tcp.extend_from_slice(&dst_port.to_be_bytes());
    tcp.extend_from_slice(&seq.to_be_bytes());
    tcp.extend_from_slice(&ack.to_be_bytes());
    tcp.push(0x50);
    tcp.push(flags);
    tcp.extend_from_slice(&0xFFFFu16.to_be_bytes());
    tcp.extend_from_slice(&[0u8; 2]); // checksum placeholder
    tcp.extend_from_slice(&[0u8; 2]); // urgent pointer
    tcp.extend_from_slice(data);

    let tcp_len = tcp.len() as u16;
    let mut pseudo = Vec::with_capacity(12 + tcp.len());
    pseudo.extend_from_slice(&src_ip);
    pseudo.extend_from_slice(&dst_ip);
    pseudo.push(0);
    pseudo.push(6);
    pseudo.extend_from_slice(&tcp_len.to_be_bytes());
    pseudo.extend_from_slice(&tcp);
    let cksum = ipv4::checksum(&pseudo);
    tcp[16] = (cksum >> 8) as u8;
    tcp[17] = (cksum & 0xFF) as u8;

    let ip_pkt = ipv4::build(src_ip, dst_ip, 6, &tcp);
    let frame = eth_build(dst_mac, src_mac, 0x0800, &ip_pkt);

    if let Some(io) = crate::net::get_io_base() {
        crate::net::pci::transmit_packet(io, &frame);
        crate::net::increment_tx();
    }
}
