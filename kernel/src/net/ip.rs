// kernel/src/net/ip.rs
// IPv4 Packet parsing and serialization
//
use alloc::vec::Vec;

/// IP Protocol numbers
pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8  = 6;
pub const PROTO_UDP: u8  = 17;

#[derive(Debug, Clone)]
pub struct Ipv4Packet {
    pub version: u8,
    pub ihl: u8,           // Internet Header Length (in 32-bit words)
    pub dscp_ecn: u8,
    pub total_length: u16,
    pub identification: u16,
    pub flags: u8,
    pub fragment_offset: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub header_checksum: u16,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub payload: Vec<u8>,
}

/// Global identification counter for outgoing packets
static IP_ID_COUNTER: core::sync::atomic::AtomicU16 =
    core::sync::atomic::AtomicU16::new(1);

fn next_ip_id() -> u16 {
    IP_ID_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

impl Ipv4Packet {
    /// Parse an IPv4 packet from raw bytes (Ethernet payload).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }

        let version = data[0] >> 4;
        if version != 4 {
            crate::serial_println!("[IP] Not IPv4 (version={})", version);
            return None;
        }

        let ihl = data[0] & 0x0F;
        let header_len = (ihl as usize) * 4;

        if data.len() < header_len {
            return None;
        }

        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let actual_len = core::cmp::min(total_length as usize, data.len());

        let mut src_ip = [0u8; 4];
        let mut dst_ip = [0u8; 4];
        src_ip.copy_from_slice(&data[12..16]);
        dst_ip.copy_from_slice(&data[16..20]);

        let flags_frag = u16::from_be_bytes([data[6], data[7]]);

        Some(Ipv4Packet {
            version,
            ihl,
            dscp_ecn: data[1],
            total_length,
            identification: u16::from_be_bytes([data[4], data[5]]),
            flags: (flags_frag >> 13) as u8,
            fragment_offset: flags_frag & 0x1FFF,
            ttl: data[8],
            protocol: data[9],
            header_checksum: u16::from_be_bytes([data[10], data[11]]),
            src_ip,
            dst_ip,
            payload: data[header_len..actual_len].to_vec(),
        })
    }

    /// Build a new IPv4 packet.
    pub fn new(src_ip: [u8; 4], dst_ip: [u8; 4], protocol: u8, payload: Vec<u8>) -> Self {
        let total_length = 20 + payload.len() as u16;
        Ipv4Packet {
            version: 4,
            ihl: 5, // No options = 5 words = 20 bytes
            dscp_ecn: 0,
            total_length,
            identification: next_ip_id(),
            flags: 0,
            fragment_offset: 0,
            ttl: 64,
            protocol,
            header_checksum: 0, // Filled during serialize
            src_ip,
            dst_ip,
            payload,
        }
    }

    /// Serialize to bytes (calculates checksum automatically).
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20 + self.payload.len());

        // Byte 0: Version (4) + IHL (5)
        buf.push((self.version << 4) | self.ihl);
        // Byte 1: DSCP/ECN
        buf.push(self.dscp_ecn);
        // Bytes 2-3: Total Length
        buf.extend_from_slice(&self.total_length.to_be_bytes());
        // Bytes 4-5: Identification
        buf.extend_from_slice(&self.identification.to_be_bytes());
        // Bytes 6-7: Flags + Fragment Offset
        let flags_frag = ((self.flags as u16) << 13) | self.fragment_offset;
        buf.extend_from_slice(&flags_frag.to_be_bytes());
        // Byte 8: TTL
        buf.push(self.ttl);
        // Byte 9: Protocol
        buf.push(self.protocol);
        // Bytes 10-11: Checksum placeholder (0)
        buf.extend_from_slice(&0u16.to_be_bytes());
        // Bytes 12-15: Source IP
        buf.extend_from_slice(&self.src_ip);
        // Bytes 16-19: Destination IP
        buf.extend_from_slice(&self.dst_ip);

        // Calculate and fill in the header checksum
        let checksum = Self::checksum(&buf[..20]);
        buf[10] = (checksum >> 8) as u8;
        buf[11] = (checksum & 0xFF) as u8;

        // Append payload
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Internet checksum (RFC 1071).
    /// Used for IP header, also reused for UDP/TCP pseudo-header.
    pub fn checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = 0;

        // Sum 16-bit words
        let mut i = 0;
        while i + 1 < data.len() {
            let word = ((data[i] as u32) << 8) | (data[i + 1] as u32);
            sum += word;
            i += 2;
        }

        // Handle odd byte
        if i < data.len() {
            sum += (data[i] as u32) << 8;
        }

        // Fold 32-bit sum to 16 bits
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        !sum as u16
    }

    /// Check if this packet is addressed to us.
    pub fn is_for_us(&self, our_ip: &[u8; 4]) -> bool {
        self.dst_ip == *our_ip || self.dst_ip == [255, 255, 255, 255]
    }

    /// Format an IP address for display.
    pub fn format_ip(ip: &[u8; 4]) -> alloc::string::String {
        alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
    }

    /// Protocol name for display.
    pub fn protocol_name(&self) -> &'static str {
        match self.protocol {
            PROTO_ICMP => "ICMP",
            PROTO_TCP => "TCP",
            PROTO_UDP => "UDP",
            _ => "Unknown",
        }
    }
}