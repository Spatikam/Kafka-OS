// kernel/src/net/udp.rs
// User Datagram Protocol (UDP)
//
// UDP Header (8 bytes):
// ┌───────────────┬───────────────┐
// │ Source Port    │ Dest Port     │
// │ 2 bytes       │ 2 bytes       │
// ├───────────────┼───────────────┤
// │ Length         │ Checksum      │
// │ 2 bytes       │ 2 bytes       │
// └───────────────┴───────────────┘
//
// UDP is simple: no connection, no handshake, no retransmission.
// Perfect for DNS (port 53), DHCP (ports 67/68), NTP, etc.

use alloc::vec::Vec;
use super::ip::{Ipv4Packet, PROTO_UDP};
use super::ethernet::{EthernetFrame, ETHERTYPE_IPV4};

#[derive(Debug, Clone)]
pub struct UdpPacket {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
    pub payload: Vec<u8>,
}

impl UdpPacket {
    /// Parse a UDP packet from raw bytes (IPv4 payload).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let length = u16::from_be_bytes([data[4], data[5]]);
        let checksum = u16::from_be_bytes([data[6], data[7]]);

        let payload_end = core::cmp::min(length as usize, data.len());
        let payload = data[8..payload_end].to_vec();

        Some(UdpPacket {
            src_port,
            dst_port,
            length,
            checksum,
            payload,
        })
    }

    /// Build a new UDP packet.
    pub fn new(src_port: u16, dst_port: u16, payload: Vec<u8>) -> Self {
        let length = 8 + payload.len() as u16;
        UdpPacket {
            src_port,
            dst_port,
            length,
            checksum: 0, // Optional for UDP over IPv4
            payload,
        }
    }

    /// Serialize to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + self.payload.len());
        buf.extend_from_slice(&self.src_port.to_be_bytes());
        buf.extend_from_slice(&self.dst_port.to_be_bytes());
        buf.extend_from_slice(&self.length.to_be_bytes());
        buf.extend_from_slice(&self.checksum.to_be_bytes()); // 0 = no checksum
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Serialize with UDP checksum (uses pseudo-header).
    /// Some servers require a valid checksum even for UDP.
    pub fn serialize_with_checksum(&self, src_ip: &[u8; 4], dst_ip: &[u8; 4]) -> Vec<u8> {
        let mut buf = self.serialize();

        // Build pseudo-header for checksum calculation
        let mut pseudo = Vec::with_capacity(12 + buf.len());
        pseudo.extend_from_slice(src_ip);
        pseudo.extend_from_slice(dst_ip);
        pseudo.push(0); // Zero
        pseudo.push(PROTO_UDP); // Protocol
        pseudo.extend_from_slice(&self.length.to_be_bytes());
        pseudo.extend_from_slice(&buf);

        let checksum = Ipv4Packet::checksum(&pseudo);
        // If checksum is 0, use 0xFFFF (RFC 768)
        let checksum = if checksum == 0 { 0xFFFF } else { checksum };

        buf[6] = (checksum >> 8) as u8;
        buf[7] = (checksum & 0xFF) as u8;

        buf
    }
}

/// Send a UDP packet to a destination IP:port.
///
/// This handles the full stack: UDP → IPv4 → Ethernet → NIC.
/// Uses the ARP table to resolve the destination MAC.
/// For anything outside our subnet, sends to the gateway.
pub fn send_udp(
    src_port: u16,
    dst_ip: [u8; 4],
    dst_port: u16,
    payload: Vec<u8>,
) -> Result<(), &'static str> {
    let our_mac = super::mac_address().ok_or("NIC not initialized")?;
    let our_ip = super::our_ip();

    // Build UDP packet
    let udp = UdpPacket::new(src_port, dst_port, payload);
    let udp_bytes = udp.serialize_with_checksum(&our_ip, &dst_ip);

    // Build IPv4 packet
    let ip = Ipv4Packet::new(our_ip, dst_ip, PROTO_UDP, udp_bytes);
    let ip_bytes = ip.serialize();

    // Determine destination MAC:
    // In QEMU user-mode networking, everything goes through the gateway (10.0.2.2)
    // since we're on a /24 NAT network. The gateway handles routing.
    //let gateway_ip = super::GATEWAY_IP;  // this is hardocded for now, i guess i will have to add a=dhcp.
    let gateway_ip = super::gateway_ip();

    // Look up gateway MAC in ARP table
    let dst_mac = match super::arp::ARP_TABLE.lock().lookup(&gateway_ip) {
        Some(mac) => mac,
        None => {
            // Need to ARP for the gateway first
            crate::serial_println!("[UDP] Gateway MAC unknown, sending ARP request...");
            super::arp::send_arp_request(our_mac, our_ip, gateway_ip);

            // Wait for ARP reply
            for _ in 0..100u32 {
                for _ in 0..500_000u32 {
                    core::hint::spin_loop();
                }
                // Poll for packets
                while let Some(raw) = super::receive_raw() {
                    super::process_packet(&raw);
                }
                if let Some(mac) = super::arp::ARP_TABLE.lock().lookup(&gateway_ip) {
                    break;
                }
            }

            // Try again
            super::arp::ARP_TABLE
                .lock()
                .lookup(&gateway_ip)
                .ok_or("Could not resolve gateway MAC via ARP")?
        }
    };

    // Build Ethernet frame
    let frame = EthernetFrame::new(dst_mac, our_mac, ETHERTYPE_IPV4, ip_bytes);

    crate::serial_println!(
        "[UDP] Sending {}:{} -> {}:{} ({} bytes payload)",
        Ipv4Packet::format_ip(&our_ip), src_port,
        Ipv4Packet::format_ip(&dst_ip), dst_port,
        udp.payload.len()
    );

    // Send!
    super::send_raw(&frame.serialize())
}

/// Handle an incoming UDP packet.
///
/// Called from the IP layer when protocol == 17.
/// Currently logs the packet; specific port handlers (DNS, DHCP)
/// will be added as we build more protocols.
pub fn handle_udp_packet(ip_packet: &Ipv4Packet) {
    let udp = match UdpPacket::parse(&ip_packet.payload) {
        Some(p) => p,
        None => {
            crate::serial_println!("[UDP] Failed to parse UDP packet");
            return;
        }
    };

    crate::serial_println!(
        "[UDP] {}:{} -> {}:{} ({} bytes)",
        Ipv4Packet::format_ip(&ip_packet.src_ip), udp.src_port,
        Ipv4Packet::format_ip(&ip_packet.dst_ip), udp.dst_port,
        udp.payload.len()
    );

    // Dispatch to specific handlers based on port
    match udp.dst_port {
        68 => {
            // DHCP client port (Phase 5)
            crate::serial_println!("[UDP] DHCP response received (not yet handled)");
        }
        _ => {
            // Store in a receive buffer for DNS and other callers
            let mut rx = UDP_RX_BUFFER.lock();
            rx.push(UdpRxPacket {
                src_ip: ip_packet.src_ip,
                src_port: udp.src_port,
                dst_port: udp.dst_port,
                payload: udp.payload,
            });
        }
    }
}

/// A received UDP packet with source info.
#[derive(Debug, Clone)]
pub struct UdpRxPacket {
    pub src_ip: [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: Vec<u8>,
}

use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    /// Buffer for received UDP packets that haven't been consumed yet.
    /// DNS, NTP, etc. poll this buffer for their responses.
    pub static ref UDP_RX_BUFFER: Mutex<Vec<UdpRxPacket>> = Mutex::new(Vec::new());
}

/// Wait for a UDP packet on a specific port (with timeout).
///
/// Returns the first matching packet, or None on timeout.
pub fn receive_udp(port: u16, timeout_iterations: u32) -> Option<UdpRxPacket> {
    for _ in 0..timeout_iterations {
        // Small delay
        for _ in 0..500_000u32 {
            core::hint::spin_loop();
        }

        // Poll NIC for new packets
        while let Some(raw) = super::receive_raw() {
            super::process_packet(&raw);
        }

        // Check buffer for our port
        let mut rx = UDP_RX_BUFFER.lock();
        if let Some(idx) = rx.iter().position(|p| p.dst_port == port) {
            return Some(rx.remove(idx));
        }
    }

    None
}