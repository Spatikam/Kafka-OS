/*kernel/src/net/ethernet.rs
Ethernet II Frame parsing and serialization

An Ethernet frame looks like:
┌──────────┬──────────┬───────────┬─────────────────┐
│ Dst MAC  │ Src MAC  │ EtherType │ Payload (46-1500)│
│ 6 bytes  │ 6 bytes  │ 2 bytes   │                  │
└──────────┴──────────┴───────────┴─────────────────┘
CRC is stripped by the NIC (we set SECRC in RCTL).*/

use alloc::vec::Vec;

/// EtherType constants
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16  = 0x0806;
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

/// Broadcast MAC address (FF:FF:FF:FF:FF:FF)
pub const BROADCAST_MAC: [u8; 6] = [0xFF; 6];

/// Parsed Ethernet II frame.
#[derive(Debug, Clone)]
pub struct EthernetFrame {
    pub dst_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16,
    pub payload: Vec<u8>,
}

impl EthernetFrame {
    /// Parse raw bytes into an Ethernet frame.
    /// Returns None if the data is too short.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 14 {
            return None;
        }

        let mut dst_mac = [0u8; 6];
        let mut src_mac = [0u8; 6];
        dst_mac.copy_from_slice(&data[0..6]);
        src_mac.copy_from_slice(&data[6..12]);
        let ethertype = u16::from_be_bytes([data[12], data[13]]);
        let payload = data[14..].to_vec();

        Some(EthernetFrame {
            dst_mac,
            src_mac,
            ethertype,
            payload,
        })
    }

    /// Build a new Ethernet frame.
    pub fn new(dst_mac: [u8; 6], src_mac: [u8; 6], ethertype: u16, payload: Vec<u8>) -> Self {
        EthernetFrame {
            dst_mac,
            src_mac,
            ethertype,
            payload,
        }
    }

    /// Serialize the frame into bytes ready to send to the NIC.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(14 + self.payload.len());
        buf.extend_from_slice(&self.dst_mac);
        buf.extend_from_slice(&self.src_mac);
        buf.extend_from_slice(&self.ethertype.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Check if this frame is addressed to us or is a broadcast.
    pub fn is_for_us(&self, our_mac: &[u8; 6]) -> bool {
        self.dst_mac == *our_mac || self.dst_mac == BROADCAST_MAC
    }

    /// Format MAC address as string for debug output.
    pub fn format_mac(mac: &[u8; 6]) -> alloc::string::String {
        alloc::format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        )
    }
}