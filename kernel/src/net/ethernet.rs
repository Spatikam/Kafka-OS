use alloc::vec::Vec;

pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHERTYPE_IPV4: u16 = 0x0800;

// NO #[repr(C, packed)] — not needed, we parse from raw bytes manually
pub struct EthernetFrame {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
}

pub enum EtherPayload<'a> {
    Arp(&'a [u8]),
    Ipv4(&'a [u8]),
    Unknown(u16),
}

pub fn parse(raw: &[u8]) -> Option<(EthernetFrame, EtherPayload)> {
    if raw.len() < 14 {
        return None;
    }

    let mut dst = [0u8; 6];
    let mut src = [0u8; 6];
    dst.copy_from_slice(&raw[0..6]);
    src.copy_from_slice(&raw[6..12]);
    let ethertype = u16::from_be_bytes([raw[12], raw[13]]);

    let frame = EthernetFrame {
        dst,
        src,
        ethertype,
    };
    let payload = &raw[14..];

    let kind = match ethertype {
        ETHERTYPE_ARP => EtherPayload::Arp(payload),
        ETHERTYPE_IPV4 => EtherPayload::Ipv4(payload),
        other => EtherPayload::Unknown(other),
    };
    Some((frame, kind))
}

pub fn build(dst: [u8; 6], src: [u8; 6], ethertype: u16, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.extend_from_slice(&dst);
    frame.extend_from_slice(&src);
    frame.extend_from_slice(&ethertype.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}
