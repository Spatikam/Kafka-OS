use super::ethernet::build as build_ethernet;
use alloc::vec::Vec;
use spin::Mutex;

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

static PING_REPLY: spin::Mutex<Option<(u16, [u8; 4])>> = spin::Mutex::new(None);

pub fn handle_icmp_packet(src_ip: [u8; 4], data: &[u8]) {
    if data.len() < 8 {
        return;
    }
    let icmp_type = data[0];
    let seq = u16::from_be_bytes([data[6], data[7]]);
    if icmp_type == 0 {
        *PING_REPLY.lock() = Some((seq, src_ip));
    }
}

pub fn take_ping_reply() -> Option<(u16, [u8; 4])> {
    PING_REPLY.lock().take()
}

pub fn send_ping(dst_ip: [u8; 4], seq: u16) {
    let src_ip = crate::net::get_ip_address().unwrap_or([10, 0, 2, 15]);
    let src_mac = crate::net::get_mac_address().unwrap_or([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let dst_mac = crate::net::arp::lookup(dst_ip).unwrap_or([0x52, 0x55, 0x0a, 0x00, 0x02, 0x02]);

    // ICMP echo request: type=8, code=0, id=0xABCD, seq, payload
    let payload = b"kafkaos-ping";
    let mut icmp = Vec::with_capacity(8 + payload.len());
    icmp.push(8); // type: echo request
    icmp.push(0); // code
    icmp.extend_from_slice(&[0u8; 2]); // checksum placeholder
    icmp.extend_from_slice(&0xABCDu16.to_be_bytes()); // identifier
    icmp.extend_from_slice(&seq.to_be_bytes()); // sequence
    icmp.extend_from_slice(payload);

    let cksum = checksum(&icmp);
    icmp[2] = (cksum >> 8) as u8;
    icmp[3] = (cksum & 0xFF) as u8;

    let ip_pkt = crate::net::ipv4::build(src_ip, dst_ip, 1, &icmp);
    let frame = build_ethernet(dst_mac, src_mac, 0x0800, &ip_pkt);

    if let Some(io) = crate::net::get_io_base() {
        crate::net::pci::transmit_packet(io, &frame);
        crate::net::increment_tx();
    }
}
