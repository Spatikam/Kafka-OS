/*kernel/src/net/arp.rs
Address Resolution Protocol (ARP)

ARP resolves IPv4 addresses to MAC addresses on the local network.

ARP packet layout (inside Ethernet payload):
┌────────────┬────────────┬────────┬──────────┬───────────┐
│ HW Type    │ Proto Type │ HW/Pro │ Opcode   │ Addresses │
│ 2 bytes    │ 2 bytes    │ Len 2B │ 2 bytes  │ variable  │
└────────────┴────────────┴────────┴──────────┴───────────┘

For Ethernet + IPv4:
  Sender MAC (6) + Sender IP (4) + Target MAC (6) + Target IP (4) = 20 bytes
  Total ARP payload = 8 (header) + 20 (addresses) = 28 bytes*/

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;

use super::ethernet::{self, EthernetFrame, ETHERTYPE_ARP, BROADCAST_MAC};

/// ARP opcodes
const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16   = 2;

/// Hardware type: Ethernet
const HW_TYPE_ETHERNET: u16 = 1;
/// Protocol type: IPv4
const PROTO_TYPE_IPV4: u16 = 0x0800;

lazy_static! {
    /// Global ARP cache: IP address → MAC address
    pub static ref ARP_TABLE: Mutex<ArpTable> = Mutex::new(ArpTable::new());
}

/*
Declaring a B tree would be the best i guess for implementing an Arp. 
some of the things i have to add 
a) Insert. 
b) for the lookup of MAC address for a specific IP i guess.
c) for debugging what i will do is nothing but print the all the entries. (so it would be easy )
*/

pub struct ArpTable {
    entries: BTreeMap<[u8; 4], [u8; 6]>,
}

impl ArpTable {
    pub fn new() -> Self {
        ArpTable {
            entries: BTreeMap::new(),
        }
    }

    /// Add or update an ARP entry.
    pub fn insert(&mut self, ip: [u8; 4], mac: [u8; 6]) {
        crate::serial_println!(
            "[ARP] Cache update: {}.{}.{}.{} -> {}",
            ip[0], ip[1], ip[2], ip[3],
            EthernetFrame::format_mac(&mac)
        );
        self.entries.insert(ip, mac);
    }

    /// Look up a MAC address for an IP.
    pub fn lookup(&self, ip: &[u8; 4]) -> Option<[u8; 6]> {
        self.entries.get(ip).copied()
    }

    // i would basically print all the ARP tables (note that we can invoke it if and only if we add to  cli for verifyinng)
    pub fn print_table(&self) {
        crate::serial_println!("┌─── ARP Table ─────────────────────────────────┐");
        if self.entries.is_empty() {
            crate::serial_println!("│ (empty)                                       │");
        }
        for (ip, mac) in &self.entries {
            crate::serial_println!(
                "│ {}.{}.{}.{}\t -> {}  │",
                ip[0], ip[1], ip[2], ip[3],
                EthernetFrame::format_mac(mac)
            );
        }
        crate::serial_println!("└───────────────────────────────────────────────┘");
    }
}

/*
  In this i  have defined major of ARP PACKET 
  that would basically carry
*/
#[derive(Debug, Clone)]
pub struct ArpPacket {
    pub hardware_type: u16,
    pub protocol_type: u16,
    pub hw_len: u8,
    pub proto_len: u8,
    pub operation: u16,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

impl ArpPacket {
    /// Parse an ARP packet from raw bytes (Ethernet payload).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 28 {
            return None;
        }

        let mut sender_mac = [0u8; 6];
        let mut sender_ip = [0u8; 4];
        let mut target_mac = [0u8; 6];
        let mut target_ip = [0u8; 4];

        sender_mac.copy_from_slice(&data[8..14]);
        sender_ip.copy_from_slice(&data[14..18]);
        target_mac.copy_from_slice(&data[18..24]);
        target_ip.copy_from_slice(&data[24..28]);

        Some(ArpPacket {
            hardware_type: u16::from_be_bytes([data[0], data[1]]),
            protocol_type: u16::from_be_bytes([data[2], data[3]]),
            hw_len: data[4],
            proto_len: data[5],
            operation: u16::from_be_bytes([data[6], data[7]]),
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        })
    }

    // packets to bit conversion.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(28);
        buf.extend_from_slice(&self.hardware_type.to_be_bytes());
        buf.extend_from_slice(&self.protocol_type.to_be_bytes());
        buf.push(self.hw_len);
        buf.push(self.proto_len);
        buf.extend_from_slice(&self.operation.to_be_bytes());
        buf.extend_from_slice(&self.sender_mac);
        buf.extend_from_slice(&self.sender_ip);
        buf.extend_from_slice(&self.target_mac);
        buf.extend_from_slice(&self.target_ip);
        buf
    }

    // creating a request here, so that we know where is what !! and how it is being accessed.
    pub fn new_request(our_mac: [u8; 6], our_ip: [u8; 4], target_ip: [u8; 4]) -> Self {
        ArpPacket {
            hardware_type: HW_TYPE_ETHERNET,
            protocol_type: PROTO_TYPE_IPV4,
            hw_len: 6,
            proto_len: 4,
            operation: ARP_REQUEST,
            sender_mac: our_mac,
            sender_ip: our_ip,
            target_mac: [0x00; 6], // Unknown — that's what we're asking
            target_ip,
        }
    }
    pub fn new_reply(
        our_mac: [u8; 6],
        our_ip: [u8; 4],
        target_mac: [u8; 6],
        target_ip: [u8; 4],
    ) -> Self {
        ArpPacket {
            hardware_type: HW_TYPE_ETHERNET,
            protocol_type: PROTO_TYPE_IPV4,
            hw_len: 6,
            proto_len: 4,
            operation: ARP_REPLY,
            sender_mac: our_mac,
            sender_ip: our_ip,
            target_mac,
            target_ip,
        }
    }
    pub fn is_request(&self) -> bool {
        self.operation == ARP_REQUEST
    }
    pub fn is_reply(&self) -> bool {
        self.operation == ARP_REPLY
    }
}

/*
  Here i guess we can handle two major operations .
  a ) being send the arp_request thing/
  b ) handling the incomping packets 
*/

pub fn send_arp_request(our_mac: [u8; 6], our_ip: [u8; 4], target_ip: [u8; 4]) {
    crate::serial_println!(
        "[ARP] Sending request: Who has {}.{}.{}.{}? Tell {}.{}.{}.{}",
        target_ip[0], target_ip[1], target_ip[2], target_ip[3],
        our_ip[0], our_ip[1], our_ip[2], our_ip[3]
    );

    let arp = ArpPacket::new_request(our_mac, our_ip, target_ip);
    let frame = EthernetFrame::new(
        BROADCAST_MAC,  // ARP requests go to broadcast
        our_mac,
        ETHERTYPE_ARP,
        arp.serialize(),
    );

    if let Err(e) = super::send_raw(&frame.serialize()) {
        crate::serial_println!("[ARP] Failed to send request: {}", e);
    }
}
pub fn handle_arp_packet(data: &[u8], our_mac: [u8; 6], our_ip: [u8; 4]) {
    let arp = match ArpPacket::parse(data) {
        Some(p) => p,
        None => {
            crate::serial_println!("[ARP] Failed to parse ARP packet");
            return;
        }
    };
    crate::serial_println!(
        "[ARP] {} from {} ({}.{}.{}.{}) -> target {}.{}.{}.{}",
        if arp.is_request() { "REQUEST" } else { "REPLY" },
        EthernetFrame::format_mac(&arp.sender_mac),
        arp.sender_ip[0], arp.sender_ip[1], arp.sender_ip[2], arp.sender_ip[3],
        arp.target_ip[0], arp.target_ip[1], arp.target_ip[2], arp.target_ip[3]
    );

    // Always learn the sender's MAC (even from requests)
    ARP_TABLE.lock().insert(arp.sender_ip, arp.sender_mac);

    if arp.is_request() && arp.target_ip == our_ip {
        // Someone is asking for our MAC (reply man come on)
        crate::serial_println!("[ARP] Request is for us! Sending reply...");
        let reply = ArpPacket::new_reply(our_mac, our_ip, arp.sender_mac, arp.sender_ip);
        let frame = EthernetFrame::new(
            arp.sender_mac,  // Unicast reply back to requester
            our_mac,
            ETHERTYPE_ARP,
            reply.serialize(),
        );
        if let Err(e) = super::send_raw(&frame.serialize()) {
            crate::serial_println!("[ARP] Failed to send reply: {}", e);
        }
    }
}