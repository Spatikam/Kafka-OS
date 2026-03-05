/*kernel/src/net/dns.rs
Minimal DNS Client

DNS uses UDP port 53. We send a query to QEMU's DNS server (10.0.2.3)
and parse the response to get an IP address.

DNS Packet Format (simplified):
┌────────────────────────────────────┐
│ Header (12 bytes)                  │
│   ID, Flags, QCount, ACount, etc.  │
├────────────────────────────────────┤
│ Question Section                   │
│   Name (encoded), Type, Class      │
├────────────────────────────────────┤
│ Answer Section (in response)       │
│   Name, Type, Class, TTL, Data     │
└────────────────────────────────────┘*/

use alloc::string::String;
use alloc::vec::Vec;

/// DNS record types
const TYPE_A: u16     = 1;   // IPv4 address
const CLASS_IN: u16   = 1;   // Internet class

/// DNS header flags
const FLAG_RD: u16    = 0x0100; // Recursion Desired
const FLAG_QR: u16    = 0x8000; // Query Response (set in replies)

/// Build a DNS query for an A record (IPv4 address).
///
/// `domain` should be like "google.com" or "example.org"
fn build_dns_query(domain: &str, transaction_id: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);

    // ── Header (12 bytes) ──
    buf.extend_from_slice(&transaction_id.to_be_bytes()); // ID
    buf.extend_from_slice(&FLAG_RD.to_be_bytes());        // Flags: recursion desired
    buf.extend_from_slice(&1u16.to_be_bytes());           // QDCOUNT: 1 question
    buf.extend_from_slice(&0u16.to_be_bytes());           // ANCOUNT: 0
    buf.extend_from_slice(&0u16.to_be_bytes());           // NSCOUNT: 0
    buf.extend_from_slice(&0u16.to_be_bytes());           // ARCOUNT: 0

    // ── Question Section ──
    // Encode domain name: "google.com" → [6]google[3]com[0]
    for label in domain.split('.') {
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0); // Root label (end of name)

    buf.extend_from_slice(&TYPE_A.to_be_bytes());   // QTYPE: A record
    buf.extend_from_slice(&CLASS_IN.to_be_bytes()); // QCLASS: Internet

    buf
}

/// Parse a DNS response and extract IPv4 addresses from A records.
fn parse_dns_response(data: &[u8]) -> Option<Vec<[u8; 4]>> {
    if data.len() < 12 {
        return None;
    }

    let flags = u16::from_be_bytes([data[2], data[3]]);
    if flags & FLAG_QR == 0 {
        // Not a response
        return None;
    }

    // Check RCODE (lower 4 bits of flags)
    let rcode = flags & 0x000F;
    if rcode != 0 {
        crate::serial_println!("[DNS] Server returned error code: {}", rcode);
        return None;
    }

    let _qd_count = u16::from_be_bytes([data[4], data[5]]);
    let an_count = u16::from_be_bytes([data[6], data[7]]);

    if an_count == 0 {
        crate::serial_println!("[DNS] No answers in response");
        return None;
    }

    // Skip the question section to get to answers
    let mut offset = 12;

    // Skip question(s): each has a name + type(2) + class(2)
    for _ in 0.._qd_count {
        offset = skip_dns_name(data, offset)?;
        offset += 4; // Skip QTYPE + QCLASS
    }

    // Parse answer section
    let mut addresses = Vec::new();

    for _ in 0..an_count {
        if offset >= data.len() {
            break;
        }

        // Skip name (may be a pointer)
        offset = skip_dns_name(data, offset)?;

        if offset + 10 > data.len() {
            break;
        }

        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let _rclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        let _ttl = u32::from_be_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        offset += 10;

        if rtype == TYPE_A && rdlength == 4 && offset + 4 <= data.len() {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&data[offset..offset + 4]);
            addresses.push(ip);
        }

        offset += rdlength;
    }

    if addresses.is_empty() {
        None
    } else {
        Some(addresses)
    }
}

/// Skip a DNS name in the packet (handles both labels and compression pointers).
fn skip_dns_name(data: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        if offset >= data.len() {
            return None;
        }

        let len = data[offset] as usize;

        if len == 0 {
            // End of name
            return Some(offset + 1);
        }

        if len & 0xC0 == 0xC0 {
            // Compression pointer (2 bytes) — we just skip it
            return Some(offset + 2);
        }

        // Regular label: skip length + label bytes
        offset += 1 + len;
    }
}
pub fn resolve(domain: &str) -> Option<[u8; 4]> {
    //let dns_server = super::DNS_IP; // 10.0.2.3 ehh hardcoded, will have to add a dynammic one, by adding dhcp
    let dns_server = super::dns_ip();  // fixed !! no harcoded crap !! 
    let src_port = 12345u16; // Arbitrary source port
    let transaction_id = 0xABCDu16;

    crate::serial_println!("[DNS] Resolving '{}'...", domain);
    let query = build_dns_query(domain, transaction_id);

    if let Err(e) = super::udp::send_udp(src_port, dns_server, 53, query) {
        crate::serial_println!("[DNS] Failed to send query: {}", e);
        return None;
    }
    crate::serial_println!("[DNS] Waiting for response from {}...",
        super::ip::Ipv4Packet::format_ip(&dns_server));
    let response = super::udp::receive_udp(src_port, 200)?;
    if response.src_ip != dns_server || response.src_port != 53 {
        crate::serial_println!("[DNS] Unexpected response source");
        return None;
    }
    let addresses = parse_dns_response(&response.payload)?;

    if let Some(ip) = addresses.first() {
        crate::serial_println!(
            "[DNS] {} -> {}",
            domain,
            super::ip::Ipv4Packet::format_ip(ip)
        );
        Some(*ip)
    } else {
        crate::serial_println!("[DNS] No A records found for '{}'", domain);
        None
    }
}