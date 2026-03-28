use alloc::vec::Vec;

pub enum IpPayload<'a> {
    Icmp(&'a [u8]),
    Tcp(&'a [u8]),
    Unknown(u8),
}

pub struct IpHeader {
    pub src: [u8; 4],
    pub dst: [u8; 4],
    pub protocol: u8,
    pub header_len: usize,
}

pub fn parse(raw: &[u8]) -> Option<(IpHeader, IpPayload)> {
    if raw.len() < 20 {
        return None;
    }

    let ihl = (raw[0] & 0x0F) as usize * 4; // header length in bytes
    let protocol = raw[9];
    let mut src = [0u8; 4];
    src.copy_from_slice(&raw[12..16]);
    let mut dst = [0u8; 4];
    dst.copy_from_slice(&raw[16..20]);

    let header = IpHeader {
        src,
        dst,
        protocol,
        header_len: ihl,
    };
    let payload = &raw[ihl..];

    let kind = match protocol {
        1 => IpPayload::Icmp(payload),
        6 => IpPayload::Tcp(payload),
        _ => IpPayload::Unknown(protocol),
    };
    Some((header, kind))
}

pub fn build(src: [u8; 4], dst: [u8; 4], protocol: u8, payload: &[u8]) -> Vec<u8> {
    let total_len = (20 + payload.len()) as u16;
    let mut pkt = Vec::with_capacity(20 + payload.len());

    pkt.push(0x45); // version=4, IHL=5
    pkt.push(0x00); // DSCP/ECN
    pkt.extend_from_slice(&total_len.to_be_bytes()); // total length
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ID
    pkt.extend_from_slice(&0u16.to_be_bytes()); // flags/fragment
    pkt.push(64); // TTL
    pkt.push(protocol); // protocol
    pkt.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    pkt.extend_from_slice(&src);
    pkt.extend_from_slice(&dst);
    pkt.extend_from_slice(payload);

    // Fill in checksum
    let cksum = checksum(&pkt[..20]);
    pkt[10] = (cksum >> 8) as u8;
    pkt[11] = (cksum & 0xFF) as u8;
    pkt
}

pub fn checksum(data: &[u8]) -> u16 {
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
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
