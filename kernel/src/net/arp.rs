use alloc::vec::Vec;

// ARP opcodes
const ARP_REQUEST: u16 = 1;
const ARP_REPLY:   u16 = 2;

#[repr(C, packed)]
pub struct ArpPacket {
    pub htype:  u16,     // 0x0001 = Ethernet
    pub ptype:  u16,     // 0x0800 = IPv4
    pub hlen:   u8,      // 6
    pub plen:   u8,      // 4
    pub opcode: u16,     // 1=request, 2=reply
    pub src_mac: [u8; 6],
    pub src_ip:  [u8; 4],
    pub dst_mac: [u8; 6],
    pub dst_ip:  [u8; 4],
}                        // = 28 bytes total

pub fn handle(payload: &[u8], our_mac: [u8; 6], our_ip: [u8; 4]) -> Option<Vec<u8>> {
    if payload.len() < 28 { return None; }

    let opcode = u16::from_be_bytes([payload[6], payload[7]]);
    if opcode != 1 { return None; } // only handle requests

    let mut target_ip = [0u8; 4];
    target_ip.copy_from_slice(&payload[24..28]);

    if target_ip != our_ip { return None; }

    let mut src_mac = [0u8; 6];
    let mut src_ip  = [0u8; 4];
    src_mac.copy_from_slice(&payload[8..14]);
    src_ip.copy_from_slice(&payload[14..18]);

    // Build ARP reply
    let mut reply = Vec::with_capacity(28);
    reply.extend_from_slice(&0x0001u16.to_be_bytes()); // htype
    reply.extend_from_slice(&0x0800u16.to_be_bytes()); // ptype
    reply.push(6);                                      // hlen
    reply.push(4);                                      // plen
    reply.extend_from_slice(&ARP_REPLY.to_be_bytes());  // opcode
    reply.extend_from_slice(&our_mac);                  // sender MAC = us
    reply.extend_from_slice(&our_ip);                   // sender IP  = us
    reply.extend_from_slice(&src_mac);                  // target MAC = requester
    reply.extend_from_slice(&src_ip);                   // target IP  = requester

    Some(reply)
}