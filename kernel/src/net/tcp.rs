// kernel/src/net/tcp.rs
// Transmission Control Protocol (TCP) for Kafka-OS
//
// TCP provides reliable, ordered, byte-stream delivery.
// This is a minimal client-side implementation sufficient for HTTP.
//
// Supports:
//   - Active open (connect to server)
//   - 3-way handshake (SYN → SYN-ACK → ACK)
//   - Data send/receive with sequence numbers
//   - Graceful close (FIN handshake)
//   - MSS negotiation
//
// Does NOT support (kept simple for an OS project):
//   - Passive open (listen/accept)
//   - Retransmission / congestion control
//   - Window scaling, SACK, timestamps
//   - Out-of-order reassembly
//   - Urgent data

use alloc::vec;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::{AtomicU32, Ordering};
use super::ip::{Ipv4Packet, PROTO_TCP};
use super::ethernet::{EthernetFrame, ETHERTYPE_IPV4};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TCP Flags (byte 13 of TCP header)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

const DEFAULT_MSS: u16 = 1460;
const DEFAULT_WINDOW: u16 = 65535;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Global State
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Ephemeral port counter (starts at 49152)
static NEXT_PORT: AtomicU32 = AtomicU32::new(49152);

/// Initial sequence number counter
static ISN_COUNTER: AtomicU32 = AtomicU32::new(100_000);

fn alloc_port() -> u16 {
    NEXT_PORT.fetch_add(1, Ordering::Relaxed) as u16
}

fn next_isn() -> u32 {
    ISN_COUNTER.fetch_add(64_000, Ordering::Relaxed)
}

/// Connection key: identifies a unique TCP connection.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug)]
struct ConnKey {
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
}

lazy_static! {
    /// Global TCP connection table.
    static ref CONNECTIONS: Mutex<BTreeMap<ConnKey, TcpConnection>> =
        Mutex::new(BTreeMap::new());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TCP Packet
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone)]
pub struct TcpPacket {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8, // In 32-bit words
    pub flags: u8,
    pub window: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
    pub mss: Option<u16>, // Parsed from options
    pub payload: Vec<u8>,
}

impl TcpPacket {
    /// Parse a TCP packet from raw bytes (IPv4 payload).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }

        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

        let data_offset = data[12] >> 4; // High nibble
        let flags = data[13];
        let window = u16::from_be_bytes([data[14], data[15]]);
        let checksum = u16::from_be_bytes([data[16], data[17]]);
        let urgent_ptr = u16::from_be_bytes([data[18], data[19]]);

        let header_len = (data_offset as usize) * 4;
        if data.len() < header_len {
            return None;
        }

        // Parse MSS option from TCP options (between byte 20 and header_len)
        let mss = parse_mss_option(&data[20..header_len]);

        let payload = data[header_len..].to_vec();

        Some(TcpPacket {
            src_port,
            dst_port,
            seq_num,
            ack_num,
            data_offset,
            flags,
            window,
            checksum,
            urgent_ptr,
            mss,
            payload,
        })
    }

    /// Serialize a TCP packet to bytes (without checksum — filled in later).
    fn serialize_header(&self, payload: &[u8], include_mss: bool) -> Vec<u8> {
        let data_offset: u8 = if include_mss { 6 } else { 5 }; // 24 or 20 bytes
        let header_len = (data_offset as usize) * 4;
        let mut buf = Vec::with_capacity(header_len + payload.len());

        buf.extend_from_slice(&self.src_port.to_be_bytes());
        buf.extend_from_slice(&self.dst_port.to_be_bytes());
        buf.extend_from_slice(&self.seq_num.to_be_bytes());
        buf.extend_from_slice(&self.ack_num.to_be_bytes());

        // Data offset (high nibble) | reserved (low nibble)
        buf.push(data_offset << 4);
        // Flags
        buf.push(self.flags);

        buf.extend_from_slice(&self.window.to_be_bytes());
        // Checksum placeholder
        buf.extend_from_slice(&0u16.to_be_bytes());
        // Urgent pointer
        buf.extend_from_slice(&0u16.to_be_bytes());

        // MSS option (only in SYN packets)
        if include_mss {
            buf.push(2);    // Kind = MSS
            buf.push(4);    // Length = 4
            buf.extend_from_slice(&DEFAULT_MSS.to_be_bytes());
        }

        // Payload
        buf.extend_from_slice(payload);
        buf
    }

    /// Check if specific flags are set.
    pub fn has_flag(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    fn flags_str(&self) -> alloc::string::String {
        let mut s = alloc::string::String::new();
        if self.has_flag(SYN) { s.push_str("SYN "); }
        if self.has_flag(ACK) { s.push_str("ACK "); }
        if self.has_flag(FIN) { s.push_str("FIN "); }
        if self.has_flag(RST) { s.push_str("RST "); }
        if self.has_flag(PSH) { s.push_str("PSH "); }
        s
    }
}

/// Parse the MSS option from TCP options bytes.
fn parse_mss_option(options: &[u8]) -> Option<u16> {
    let mut i = 0;
    while i < options.len() {
        match options[i] {
            0 => break,        // End of options
            1 => { i += 1; }   // NOP — skip
            2 => {
                // MSS: kind=2, length=4, value=2 bytes
                if i + 3 < options.len() && options[i + 1] == 4 {
                    return Some(u16::from_be_bytes([options[i + 2], options[i + 3]]));
                }
                return None;
            }
            _ => {
                // Unknown option: skip using length field
                if i + 1 < options.len() {
                    let len = options[i + 1] as usize;
                    if len < 2 { break; }
                    i += len;
                } else {
                    break;
                }
                continue;
            }
        }
    }
    None
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TCP State Machine
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TcpState {
    Closed,
    SynSent,      // We sent SYN, waiting for SYN-ACK
    Established,  // Connection open, data flows
    FinWait1,     // We sent FIN, waiting for ACK
    FinWait2,     // Our FIN was ACKed, waiting for server's FIN
    CloseWait,    // Server sent FIN, we ACKed, waiting for app to close
    LastAck,      // We sent FIN (after CloseWait), waiting for final ACK
    TimeWait,     // Both sides FINed, waiting before cleanup
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TCP Connection
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct TcpConnection {
    state: TcpState,
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,

    // Sequence tracking
    send_next: u32,   // Next seq number we'll use for sending
    send_unack: u32,  // Oldest unACKed seq number
    recv_next: u32,   // Next seq number we expect from peer

    // Receive buffer
    recv_buffer: Vec<u8>,

    // Peer's advertised window
    send_window: u16,

    // MSS (negotiated from peer's SYN-ACK)
    mss: u16,

    // Track if FIN received
    fin_received: bool,
    // Track if RST received
    rst_received: bool,
}

impl TcpConnection {
    fn new(local_port: u16, remote_ip: [u8; 4], remote_port: u16, isn: u32) -> Self {
        TcpConnection {
            state: TcpState::SynSent,
            local_port,
            remote_ip,
            remote_port,
            send_next: isn + 1, // SYN consumes one seq number
            send_unack: isn,
            recv_next: 0,
            recv_buffer: Vec::new(),
            send_window: DEFAULT_WINDOW,
            mss: DEFAULT_MSS,
            fin_received: false,
            rst_received: false,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal: Send a TCP Segment
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Build and send a full TCP segment through the stack.
///
/// Handles: TCP (with checksum) → IPv4 → Ethernet → NIC
fn send_tcp_segment(
    src_port: u16,
    dst_ip: [u8; 4],
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> Result<(), &'static str> {
    let our_ip = super::our_ip();
    let our_mac = super::mac_address().ok_or("NIC not initialized")?;

    let include_mss = flags & SYN != 0;

    // Build TCP header
    let tcp_pkt = TcpPacket {
        src_port,
        dst_port,
        seq_num: seq,
        ack_num: ack,
        data_offset: if include_mss { 6 } else { 5 },
        flags,
        window: DEFAULT_WINDOW,
        checksum: 0,
        urgent_ptr: 0,
        mss: if include_mss { Some(DEFAULT_MSS) } else { None },
        payload: Vec::new(),
    };

    let mut tcp_bytes = tcp_pkt.serialize_header(payload, include_mss);

    // Calculate TCP checksum (over pseudo-header + TCP segment)
    let tcp_len = tcp_bytes.len() as u16;
    let mut pseudo = Vec::with_capacity(12 + tcp_bytes.len());
    pseudo.extend_from_slice(&our_ip);
    pseudo.extend_from_slice(&dst_ip);
    pseudo.push(0);
    pseudo.push(PROTO_TCP);
    pseudo.extend_from_slice(&tcp_len.to_be_bytes());
    pseudo.extend_from_slice(&tcp_bytes);

    let checksum = Ipv4Packet::checksum(&pseudo);
    // Checksum is at bytes 16-17 of TCP header
    tcp_bytes[16] = (checksum >> 8) as u8;
    tcp_bytes[17] = (checksum & 0xFF) as u8;

    // Build IPv4 packet
    let ip_pkt = Ipv4Packet::new(our_ip, dst_ip, PROTO_TCP, tcp_bytes);
    let ip_bytes = ip_pkt.serialize();

    // Get destination MAC from ARP table (use gateway for all TCP)
    let gateway_ip = super::gateway_ip();
    let dst_mac = super::arp::ARP_TABLE
        .lock()
        .lookup(&gateway_ip)
        .ok_or("Gateway MAC not in ARP table — run ARP first")?;

    // Build Ethernet frame
    let frame = EthernetFrame::new(dst_mac, our_mac, ETHERTYPE_IPV4, ip_bytes);

    super::send_raw(&frame.serialize())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Incoming Packet Handler
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Handle an incoming TCP segment.
///
/// Called from the IP layer when protocol == 6.
/// Finds the matching connection and processes according to state.
pub fn handle_tcp_packet(ip_packet: &Ipv4Packet) {
    let tcp = match TcpPacket::parse(&ip_packet.payload) {
        Some(p) => p,
        None => {
            crate::serial_println!("[TCP] Failed to parse TCP segment");
            return;
        }
    };

    crate::serial_println!(
        "[TCP] {} {}:{} -> :{} seq={} ack={} len={}",
        tcp.flags_str(),
        Ipv4Packet::format_ip(&ip_packet.src_ip),
        tcp.src_port,
        tcp.dst_port,
        tcp.seq_num,
        tcp.ack_num,
        tcp.payload.len()
    );

    let key = ConnKey {
        local_port: tcp.dst_port,
        remote_ip: ip_packet.src_ip,
        remote_port: tcp.src_port,
    };

    let mut conns = CONNECTIONS.lock();
    let conn = match conns.get_mut(&key) {
        Some(c) => c,
        None => {
            // No matching connection — send RST if it's not a RST
            if !tcp.has_flag(RST) {
                crate::serial_println!("[TCP] No connection for port {} — ignoring", tcp.dst_port);
            }
            return;
        }
    };

    // Handle RST at any state
    if tcp.has_flag(RST) {
        crate::serial_println!("[TCP] Connection reset by peer");
        conn.state = TcpState::Closed;
        conn.rst_received = true;
        return;
    }

    match conn.state {
        TcpState::SynSent => {
            // Expecting SYN-ACK
            if tcp.has_flag(SYN) && tcp.has_flag(ACK) {
                // Verify they're ACKing our SYN
                if tcp.ack_num != conn.send_next {
                    crate::serial_println!("[TCP] SYN-ACK has wrong ack number");
                    return;
                }

                // Record peer's ISN
                conn.recv_next = tcp.seq_num + 1; // SYN consumes 1 seq
                conn.send_unack = tcp.ack_num;

                // Learn peer's MSS
                if let Some(mss) = tcp.mss {
                    conn.mss = mss;
                    crate::serial_println!("[TCP] Peer MSS: {}", mss);
                }
                conn.send_window = tcp.window;

                conn.state = TcpState::Established;
                crate::serial_println!("[TCP] Connection ESTABLISHED");

                // Send ACK to complete 3-way handshake
                // (drop lock temporarily to send)
                let (lp, rip, rp, sn, rn) = (
                    conn.local_port,
                    conn.remote_ip,
                    conn.remote_port,
                    conn.send_next,
                    conn.recv_next,
                );
                drop(conns);
                let _ = send_tcp_segment(lp, rip, rp, sn, rn, ACK, &[]);
            }
        }

        TcpState::Established => {
            // Update send_unack if they're ACKing our data
            if tcp.has_flag(ACK) {
                conn.send_unack = tcp.ack_num;
            }

            // Process received data
            let data_len = tcp.payload.len();
            if data_len > 0 {
                if tcp.seq_num == conn.recv_next {
                    // In-order data — buffer it
                    conn.recv_buffer.extend_from_slice(&tcp.payload);
                    conn.recv_next = conn.recv_next.wrapping_add(data_len as u32);

                    crate::serial_println!(
                        "[TCP] Buffered {} bytes (total: {})",
                        data_len,
                        conn.recv_buffer.len()
                    );
                } else {
                    crate::serial_println!(
                        "[TCP] Out-of-order: expected seq={}, got seq={} — dropping",
                        conn.recv_next,
                        tcp.seq_num
                    );
                }
            }

            // Handle FIN
            if tcp.has_flag(FIN) {
                conn.recv_next = conn.recv_next.wrapping_add(1); // FIN consumes 1 seq
                conn.fin_received = true;
                conn.state = TcpState::CloseWait;
                crate::serial_println!("[TCP] FIN received — server closing");
            }

            // Send ACK if we received data or FIN
            if data_len > 0 || tcp.has_flag(FIN) {
                let (lp, rip, rp, sn, rn) = (
                    conn.local_port,
                    conn.remote_ip,
                    conn.remote_port,
                    conn.send_next,
                    conn.recv_next,
                );
                drop(conns);
                let _ = send_tcp_segment(lp, rip, rp, sn, rn, ACK, &[]);
            }
        }

        TcpState::FinWait1 => {
            // We sent FIN, waiting for ACK (and possibly FIN)
            if tcp.has_flag(ACK) {
                conn.send_unack = tcp.ack_num;

                if tcp.has_flag(FIN) {
                    // Simultaneous close: ACK+FIN
                    conn.recv_next = conn.recv_next.wrapping_add(1);
                    conn.state = TcpState::TimeWait;
                    let (lp, rip, rp, sn, rn) = (
                        conn.local_port,
                        conn.remote_ip,
                        conn.remote_port,
                        conn.send_next,
                        conn.recv_next,
                    );
                    drop(conns);
                    let _ = send_tcp_segment(lp, rip, rp, sn, rn, ACK, &[]);
                } else {
                    conn.state = TcpState::FinWait2;
                }
            }
        }

        TcpState::FinWait2 => {
            // Waiting for server's FIN
            if tcp.has_flag(FIN) {
                conn.recv_next = conn.recv_next.wrapping_add(1);
                conn.state = TcpState::TimeWait;
                let (lp, rip, rp, sn, rn) = (
                    conn.local_port,
                    conn.remote_ip,
                    conn.remote_port,
                    conn.send_next,
                    conn.recv_next,
                );
                drop(conns);
                let _ = send_tcp_segment(lp, rip, rp, sn, rn, ACK, &[]);
            }
        }

        TcpState::LastAck => {
            // We sent FIN from CloseWait, waiting for ACK
            if tcp.has_flag(ACK) {
                conn.state = TcpState::Closed;
                crate::serial_println!("[TCP] Connection closed");
            }
        }

        TcpState::CloseWait => {
            // Server sent FIN, we haven't sent ours yet
            // Just ACK anything that comes in
            if tcp.has_flag(ACK) {
                conn.send_unack = tcp.ack_num;
            }
        }

        _ => {}
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Public API
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Connect to a remote host.
///
/// Performs the TCP 3-way handshake and returns the local port
/// as a connection handle.
///
/// ```
/// let port = tcp::tcp_connect([142, 250, 182, 78], 80)?;
/// ```
pub fn tcp_connect(remote_ip: [u8; 4], remote_port: u16) -> Result<u16, &'static str> {
    let local_port = alloc_port();
    let isn = next_isn();

    crate::serial_println!(
        "[TCP] Connecting :{} -> {}:{} (ISN={})",
        local_port,
        Ipv4Packet::format_ip(&remote_ip),
        remote_port,
        isn
    );

    // Create connection
    let key = ConnKey {
        local_port,
        remote_ip,
        remote_port,
    };
    let conn = TcpConnection::new(local_port, remote_ip, remote_port, isn);
    CONNECTIONS.lock().insert(key, conn);

    // Send SYN
    send_tcp_segment(local_port, remote_ip, remote_port, isn, 0, SYN, &[])?;
    crate::serial_println!("[TCP] SYN sent, waiting for SYN-ACK...");

    // Wait for SYN-ACK → Established
    for attempt in 0..500u32 {
        for _ in 0..500_000u32 {
            core::hint::spin_loop();
        }

        // Poll for packets
        while let Some(raw) = super::receive_raw() {
            super::process_packet(&raw);
        }

        let state = CONNECTIONS.lock().get(&key).map(|c| c.state);
        match state {
            Some(TcpState::Established) => {
                crate::serial_println!("[TCP] Handshake complete on attempt {}", attempt);
                return Ok(local_port);
            }
            Some(TcpState::Closed) => {
                CONNECTIONS.lock().remove(&key);
                return Err("Connection refused (RST)");
            }
            _ => {}
        }

        // Retransmit SYN after ~1 second
        if attempt == 100 || attempt == 300 {
            crate::serial_println!("[TCP] Retransmitting SYN...");
            let _ = send_tcp_segment(local_port, remote_ip, remote_port, isn, 0, SYN, &[]);
        }
    }

    CONNECTIONS.lock().remove(&key);
    Err("Connection timed out (no SYN-ACK)")
}

/// Send data on an established TCP connection.
///
/// `local_port` is the handle returned by tcp_connect.
pub fn tcp_send(local_port: u16, data: &[u8]) -> Result<(), &'static str> {
    // Get connection info
    let (remote_ip, remote_port, seq, ack, state, mss) = {
        let conns = CONNECTIONS.lock();
        let key_search = conns.iter().find(|(k, _)| k.local_port == local_port);
        match key_search {
            Some((_, conn)) => (
                conn.remote_ip,
                conn.remote_port,
                conn.send_next,
                conn.recv_next,
                conn.state,
                conn.mss as usize,
            ),
            None => return Err("No such connection"),
        }
    };

    if state != TcpState::Established {
        return Err("Connection not established");
    }

    // Send data in MSS-sized chunks
    let mut offset = 0;
    while offset < data.len() {
        let chunk_end = core::cmp::min(offset + mss, data.len());
        let chunk = &data[offset..chunk_end];
        let chunk_seq = seq.wrapping_add(offset as u32);

        send_tcp_segment(
            local_port,
            remote_ip,
            remote_port,
            chunk_seq,
            ack,
            PSH | ACK,
            chunk,
        )?;

        offset = chunk_end;
    }

    // Update send_next
    {
        let mut conns = CONNECTIONS.lock();
        let key_search = conns.iter_mut().find(|(k, _)| k.local_port == local_port);
        if let Some((_, conn)) = key_search {
            conn.send_next = seq.wrapping_add(data.len() as u32);
        }
    }

    crate::serial_println!("[TCP] Sent {} bytes", data.len());

    // Brief wait for ACK
    for _ in 0..50u32 {
        for _ in 0..200_000u32 {
            core::hint::spin_loop();
        }
        while let Some(raw) = super::receive_raw() {
            super::process_packet(&raw);
        }
    }

    Ok(())
}

/// Receive all data until the server closes the connection (FIN)
/// or timeout is reached.
///
/// Returns the accumulated data buffer.
pub fn tcp_receive_all(local_port: u16, timeout_iterations: u32) -> Result<Vec<u8>, &'static str> {
    crate::serial_println!("[TCP] Receiving data...");

    for _ in 0..timeout_iterations {
        for _ in 0..500_000u32 {
            core::hint::spin_loop();
        }

        // Poll NIC
        while let Some(raw) = super::receive_raw() {
            super::process_packet(&raw);
        }

        // Check connection state
        let (state, buf_len, fin, rst) = {
            let conns = CONNECTIONS.lock();
            let key_search = conns.iter().find(|(k, _)| k.local_port == local_port);
            match key_search {
                Some((_, conn)) => (
                    conn.state,
                    conn.recv_buffer.len(),
                    conn.fin_received,
                    conn.rst_received,
                ),
                None => return Err("No such connection"),
            }
        };

        if rst {
            return Err("Connection reset by peer");
        }

        // If server closed (FIN received), return all buffered data
        if fin || state == TcpState::CloseWait || state == TcpState::Closed {
            let mut conns = CONNECTIONS.lock();
            let key_search = conns.iter_mut().find(|(k, _)| k.local_port == local_port);
            if let Some((_, conn)) = key_search {
                let data = core::mem::take(&mut conn.recv_buffer);
                crate::serial_println!("[TCP] Received total: {} bytes", data.len());
                return Ok(data);
            }
        }
    }

    // Timeout — return whatever we have
    let mut conns = CONNECTIONS.lock();
    let key_search = conns.iter_mut().find(|(k, _)| k.local_port == local_port);
    match key_search {
        Some((_, conn)) => {
            let data = core::mem::take(&mut conn.recv_buffer);
            crate::serial_println!("[TCP] Timeout, returning {} bytes", data.len());
            Ok(data)
        }
        None => Err("No such connection"),
    }
}

/// Close a TCP connection gracefully.
///
/// Sends FIN and waits for the close handshake to complete.
pub fn tcp_close(local_port: u16) -> Result<(), &'static str> {
    let (remote_ip, remote_port, seq, ack, state) = {
        let conns = CONNECTIONS.lock();
        let key_search = conns.iter().find(|(k, _)| k.local_port == local_port);
        match key_search {
            Some((_, conn)) => (
                conn.remote_ip,
                conn.remote_port,
                conn.send_next,
                conn.recv_next,
                conn.state,
            ),
            None => return Err("No such connection"),
        }
    };

    match state {
        TcpState::Established => {
            // Active close: send FIN
            send_tcp_segment(local_port, remote_ip, remote_port, seq, ack, FIN | ACK, &[])?;
            {
                let mut conns = CONNECTIONS.lock();
                let key_search = conns.iter_mut().find(|(k, _)| k.local_port == local_port);
                if let Some((_, conn)) = key_search {
                    conn.send_next = seq.wrapping_add(1); // FIN consumes 1 seq
                    conn.state = TcpState::FinWait1;
                }
            }
            crate::serial_println!("[TCP] FIN sent (active close)");
        }
        TcpState::CloseWait => {
            // Passive close: server already sent FIN, we send ours
            send_tcp_segment(local_port, remote_ip, remote_port, seq, ack, FIN | ACK, &[])?;
            {
                let mut conns = CONNECTIONS.lock();
                let key_search = conns.iter_mut().find(|(k, _)| k.local_port == local_port);
                if let Some((_, conn)) = key_search {
                    conn.send_next = seq.wrapping_add(1);
                    conn.state = TcpState::LastAck;
                }
            }
            crate::serial_println!("[TCP] FIN sent (passive close)");
        }
        _ => {
            crate::serial_println!("[TCP] Close in state {:?} — cleaning up", state);
        }
    }

    // Wait for close handshake to complete
    for _ in 0..200u32 {
        for _ in 0..500_000u32 {
            core::hint::spin_loop();
        }
        while let Some(raw) = super::receive_raw() {
            super::process_packet(&raw);
        }

        let state = {
            let conns = CONNECTIONS.lock();
            let key_search = conns.iter().find(|(k, _)| k.local_port == local_port);
            key_search.map(|(_, c)| c.state)
        };

        match state {
            Some(TcpState::Closed) | Some(TcpState::TimeWait) | None => {
                break;
            }
            _ => {}
        }
    }

    // Remove connection
    let mut conns = CONNECTIONS.lock();
    let key = conns
        .keys()
        .find(|k| k.local_port == local_port)
        .copied();
    if let Some(k) = key {
        conns.remove(&k);
    }

    crate::serial_println!("[TCP] Connection closed and cleaned up");
    Ok(())
}