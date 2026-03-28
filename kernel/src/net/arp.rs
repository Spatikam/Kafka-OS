use alloc::vec::Vec;
use spin::Mutex;

const ARP_REQUEST: u16 = 1;
const ARP_REPLY: u16 = 2;

// Cache: (IP -> MAC). QEMU gateway is always 52:55:0a:00:02:02
static ARP_CACHE: Mutex<Option<([u8; 4], [u8; 6])>> = Mutex::new(None);

pub fn cache_reply(ip: [u8; 4], mac: [u8; 6]) {
    *ARP_CACHE.lock() = Some((ip, mac));
}

pub fn lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    if let Some((cached_ip, mac)) = *ARP_CACHE.lock() {
        if cached_ip == ip {
            return Some(mac);
        }
    }
    // QEMU SLIRP gateway MAC is always this
    if ip == [10, 0, 2, 2] {
        return Some([0x52, 0x55, 0x0a, 0x00, 0x02, 0x02]);
    }
    None
}

pub fn send_request(target_ip: [u8; 4]) {
    let src_mac = crate::net::get_mac_address().unwrap_or([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let src_ip = crate::net::get_ip_address().unwrap_or([10, 0, 2, 15]);

    let mut pkt = Vec::with_capacity(28);
    pkt.extend_from_slice(&0x0001u16.to_be_bytes()); // htype ethernet
    pkt.extend_from_slice(&0x0800u16.to_be_bytes()); // ptype IPv4
    pkt.push(6);
    pkt.push(4); // hlen, plen
    pkt.extend_from_slice(&ARP_REQUEST.to_be_bytes());
    pkt.extend_from_slice(&src_mac);
    pkt.extend_from_slice(&src_ip);
    pkt.extend_from_slice(&[0u8; 6]); // target MAC unknown
    pkt.extend_from_slice(&target_ip);

    let frame = crate::net::ethernet::build([0xff; 6], src_mac, 0x0806, &pkt);
    if let Some(io) = crate::net::get_io_base() {
        crate::net::pci::transmit_packet(io, &frame);
    }
}

pub fn handle(payload: &[u8], our_mac: [u8; 6], our_ip: [u8; 4]) -> Option<Vec<u8>> {
    if payload.len() < 28 {
        return None;
    }
    let opcode = u16::from_be_bytes([payload[6], payload[7]]);

    // cache any ARP reply we see
    if opcode == ARP_REPLY {
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&payload[8..14]);
        let mut ip = [0u8; 4];
        ip.copy_from_slice(&payload[14..18]);
        cache_reply(ip, mac);
        return None;
    }

    if opcode != ARP_REQUEST {
        return None;
    }
    let mut target_ip = [0u8; 4];
    target_ip.copy_from_slice(&payload[24..28]);
    if target_ip != our_ip {
        return None;
    }

    let mut src_mac = [0u8; 6];
    src_mac.copy_from_slice(&payload[8..14]);
    let mut src_ip = [0u8; 4];
    src_ip.copy_from_slice(&payload[14..18]);

    let mut reply = Vec::with_capacity(28);
    reply.extend_from_slice(&0x0001u16.to_be_bytes());
    reply.extend_from_slice(&0x0800u16.to_be_bytes());
    reply.push(6);
    reply.push(4);
    reply.extend_from_slice(&ARP_REPLY.to_be_bytes());
    reply.extend_from_slice(&our_mac);
    reply.extend_from_slice(&our_ip);
    reply.extend_from_slice(&src_mac);
    reply.extend_from_slice(&src_ip);
    Some(reply)
}
