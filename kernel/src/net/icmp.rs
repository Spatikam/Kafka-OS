use alloc::vec::Vec;
use super::ipv4;

pub fn handle(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 8 { return None; }

    let icmp_type = payload[0];
    if icmp_type != 8 { return None; } // 8 = echo request, we only reply to this

    // Build echo reply: same data, type=0
    let mut reply = Vec::with_capacity(payload.len());
    reply.push(0x00);               // type = echo reply
    reply.push(0x00);               // code = 0
    reply.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    reply.extend_from_slice(&payload[4..]);        // copy ID + seq + data

    // Compute checksum over the whole ICMP reply
    let cksum = ipv4::checksum(&reply);
    reply[2] = (cksum >> 8) as u8;
    reply[3] = (cksum & 0xFF) as u8;
    Some(reply)
}