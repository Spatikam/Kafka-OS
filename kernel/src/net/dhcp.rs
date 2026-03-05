/*kernel/src/net/dhcp.rs
DHCP Client for Kafka-OS

DHCP (Dynamic Host Configuration Protocol) automatically assigns:  refer some good blogs for this i reffered, mosly wiki, stackover and gemini for theory
- IP address
- Subnet mask
- Gateway (router)
- DNS server

DHCP uses UDP: client port 68, server port 67.
reference mod : OSDEVWIKI. DHCP, DORA MODEL.

  1. Discover  (client → broadcast)  "I need an IP"
  2. Offer     (server → client)     "Here's 10.0.2.15"
  3. Request   (client → broadcast)  "I'll take 10.0.2.15"
  4. ACK       (server → client)     "Confirmed, it's yours"

In QEMU user-mode networking: (QEMU NETWORK CONFIGURATION MODULE IN OFFICIAL DOCS, ITS THERE YOU GUYS CAN CHECK THAT OUT)
  DHCP server lives at 10.0.2.2
  Assigned IP is always 10.0.2.15
  Gateway is 10.0.2.2
  DNS is 10.0.2.3*/ 


use alloc::vec; // type and macro lol, they need fucking both !! 
use alloc::vec::Vec;
use super::ethernet::{EthernetFrame, ETHERTYPE_IPV4, BROADCAST_MAC};
use super::ip::{Ipv4Packet, PROTO_UDP};
use super::udp::UdpPacket;

// DHCP ports
const DHCP_SERVER_PORT: u16 = 67;
const DHCP_CLIENT_PORT: u16 = 68;

// DHCP message types (Option 53)
const DHCP_DISCOVER: u8 = 1;
const DHCP_OFFER: u8    = 2;
const DHCP_REQUEST: u8  = 3;
const DHCP_DECLINE: u8  = 4;
const DHCP_ACK: u8      = 5;
const DHCP_NAK: u8      = 6;
const DHCP_RELEASE: u8  = 7;

// DHCP magic cookie (RFC 2131)
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

// Transaction ID (arbitrary, used to match responses)
const TRANSACTION_ID: [u8; 4] = [0xCA, 0xFE, 0xBA, 0xBE];

/// Result of a successful DHCP exchange.
#[derive(Debug, Clone)]
pub struct DhcpConfig {
    pub ip_address: [u8; 4],
    pub subnet_mask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns_server: [u8; 4],
    pub dhcp_server: [u8; 4],
    pub lease_time: u32,
}

impl DhcpConfig {
    fn new() -> Self {
        DhcpConfig {
            ip_address: [0; 4],
            subnet_mask: [255, 255, 255, 0],
            gateway: [0; 4],
            dns_server: [0; 4],
            dhcp_server: [0; 4],
            lease_time: 0,
        }
    }
}
// mainly packet discovery part
fn build_dhcp_discover(our_mac: [u8; 6]) -> Vec<u8> {
    let mut pkt = vec![0u8; 300];
    pkt[0] = 1;        // op: BOOTREQUEST
    pkt[1] = 1;        // htype: Ethernet
    pkt[2] = 6;        // hlen: MAC address length
    pkt[3] = 0;        // hops

    // Transaction ID
    pkt[4..8].copy_from_slice(&TRANSACTION_ID);

    // secs = 0, flags = 0x8000 
    pkt[10] = 0x80;
    pkt[11] = 0x00;

    // ciaddr, yiaddr, siaddr, giaddr = 0 (we don't have an IP yet)

    // Client MAC address (chaddr, bytes 28-33)
    pkt[28..34].copy_from_slice(&our_mac);
    pkt[236..240].copy_from_slice(&MAGIC_COOKIE);

    let mut opt_offset = 240;

    // Option 53: DHCP Message Type = Discover
    pkt[opt_offset] = 53;
    pkt[opt_offset + 1] = 1;
    pkt[opt_offset + 2] = DHCP_DISCOVER;
    opt_offset += 3;

    // Option 55: Parameter Request List
    // Ask for: subnet mask, router, DNS, lease time
    pkt[opt_offset] = 55;
    pkt[opt_offset + 1] = 4; // Length
    pkt[opt_offset + 2] = 1;  // Subnet Mask
    pkt[opt_offset + 3] = 3;  // Router
    pkt[opt_offset + 4] = 6;  // DNS Server
    pkt[opt_offset + 5] = 51; // Lease Time
    opt_offset += 6;

    // Option 255: End
    pkt[opt_offset] = 255;

    pkt
}

// same but for request partr.
fn build_dhcp_request(our_mac: [u8; 6], offered_ip: [u8; 4], server_ip: [u8; 4]) -> Vec<u8> {
    let mut pkt = vec![0u8; 300];
    pkt[0] = 1;        // op: BOOTREQUEST
    pkt[1] = 1;        // htype: Ethernet
    pkt[2] = 6;        // hlen
    pkt[3] = 0;        // hops

    // Same transaction ID
    pkt[4..8].copy_from_slice(&TRANSACTION_ID);

    // Broadcast flag
    pkt[10] = 0x80;

    // Client MAC
    pkt[28..34].copy_from_slice(&our_mac);

    // ── DHCP Options ──
    pkt[236..240].copy_from_slice(&MAGIC_COOKIE);

    let mut opt_offset = 240;

    // Option 53: DHCP Message Type = Request
    pkt[opt_offset] = 53;
    pkt[opt_offset + 1] = 1;
    pkt[opt_offset + 2] = DHCP_REQUEST;
    opt_offset += 3;

    // Option 50: Requested IP Address
    pkt[opt_offset] = 50;
    pkt[opt_offset + 1] = 4;
    pkt[opt_offset + 2..opt_offset + 6].copy_from_slice(&offered_ip);
    opt_offset += 6;

    // Option 54: DHCP Server Identifier
    pkt[opt_offset] = 54;
    pkt[opt_offset + 1] = 4;
    pkt[opt_offset + 2..opt_offset + 6].copy_from_slice(&server_ip);
    opt_offset += 6;

    // Option 55: Parameter Request List
    pkt[opt_offset] = 55;
    pkt[opt_offset + 1] = 4;
    pkt[opt_offset + 2] = 1;  // Subnet Mask
    pkt[opt_offset + 3] = 3;  // Router
    pkt[opt_offset + 4] = 6;  // DNS
    pkt[opt_offset + 5] = 51; // Lease Time
    opt_offset += 6;

    // Option 255: End
    pkt[opt_offset] = 255;

    pkt
}

// Parse a DHCP response (Offer or ACK) from raw BOOTP/DHCP payload.
fn parse_dhcp_response(data: &[u8]) -> Option<(u8, DhcpConfig)> {
    if data.len() < 240 {
        return None;
    }

    // if it's a BOOTREPLY
    if data[0] != 2 {
        return None;
    }

    // if transaction ID matches
    if data[4..8] != TRANSACTION_ID {
        return None;
    }

    // if magic cookie
    if data[236..240] != MAGIC_COOKIE {
        return None;
    }

    let mut config = DhcpConfig::new();

    // "Your" IP address (yiaddr) at offset 16
    config.ip_address.copy_from_slice(&data[16..20]);

    // Server IP (siaddr) at offset 20
    let mut siaddr = [0u8; 4];
    siaddr.copy_from_slice(&data[20..24]);

    // Parse DHCP options starting at offset 240
    let mut msg_type: u8 = 0;
    let mut i = 240;

    while i < data.len() {
        if data[i] == 255 {
            break; // End option
        }
        if data[i] == 0 {
            i += 1; // Pad option
            continue;
        }

        if i + 1 >= data.len() {
            break;
        }

        let option_code = data[i];
        let option_len = data[i + 1] as usize;

        if i + 2 + option_len > data.len() {
            break;
        }

        let option_data = &data[i + 2..i + 2 + option_len];

        match option_code {
            1 if option_len >= 4 => {
                // Subnet Mask
                config.subnet_mask.copy_from_slice(&option_data[..4]);
            }
            3 if option_len >= 4 => {
                // Router (Gateway)
                config.gateway.copy_from_slice(&option_data[..4]);
            }
            6 if option_len >= 4 => {
                // DNS Server
                config.dns_server.copy_from_slice(&option_data[..4]);
            }
            51 if option_len >= 4 => {
                // Lease Time
                config.lease_time = u32::from_be_bytes([
                    option_data[0],
                    option_data[1],
                    option_data[2],
                    option_data[3],
                ]);
            }
            53 if option_len >= 1 => {
                // DHCP Message Type
                msg_type = option_data[0];
            }
            54 if option_len >= 4 => {
                // DHCP Server Identifier
                config.dhcp_server.copy_from_slice(&option_data[..4]);
            }
            _ => {} // Ignore unknown options
        }

        i += 2 + option_len;
    }

    // If server identifier wasn't in options, use siaddr
    if config.dhcp_server == [0; 4] && siaddr != [0; 4] {
        config.dhcp_server = siaddr;
    }

    Some((msg_type, config))
}
fn send_dhcp_broadcast(our_mac: [u8; 6], dhcp_payload: Vec<u8>) -> Result<(), &'static str> {
    // Build UDP packet (port 68 → 67)
    let udp = UdpPacket::new(DHCP_CLIENT_PORT, DHCP_SERVER_PORT, dhcp_payload);
    let udp_bytes = udp.serialize(); // No checksum needed for DHCP

    // Build IPv4 packet (0.0.0.0 → 255.255.255.255)  for now ... 
    let ip = Ipv4Packet::new(
        [0, 0, 0, 0],          // We don't have an IP yet
        [255, 255, 255, 255],   // Broadcast
        PROTO_UDP,
        udp_bytes,
    );
    let ip_bytes = ip.serialize();
    // for f sake write the same format, the ref error is annoying as hell.
    let frame = EthernetFrame::new(
        BROADCAST_MAC,
        our_mac,
        ETHERTYPE_IPV4,
        ip_bytes,
    );
    super::send_raw(&frame.serialize())
}
// basically nothing but DORA exchange.
pub fn discover() -> Option<DhcpConfig> {
    let our_mac = super::mac_address()?;

    // Temporarily set our IP to 0.0.0.0 so we accept all incoming packets
    let old_ip = super::our_ip();
    super::set_our_ip([0, 0, 0, 0]);

    crate::serial_println!("[DHCP] Step 1/4: Sending DISCOVER...");

    // ── Step 1: Send DHCP Discover ──
    let discover_pkt = build_dhcp_discover(our_mac);
    if let Err(e) = send_dhcp_broadcast(our_mac, discover_pkt) {
        crate::serial_println!("[DHCP] Failed  bro !! to send DISCOVER: {}", e);
        super::set_our_ip(old_ip);
        return None;
    }

    // ── Step 2: Wait for DHCP Offer ──
    crate::serial_println!("[DHCP]: Waiting dhcp off...");
    let offer_config = wait_for_dhcp_message(DHCP_OFFER, 300)?;
    crate::serial_println!(
        "[DHCP]: Received OFFER: IP={} Gateway={} DNS={} Server={}",
        Ipv4Packet::format_ip(&offer_config.ip_address),
        Ipv4Packet::format_ip(&offer_config.gateway),
        Ipv4Packet::format_ip(&offer_config.dns_server),
        Ipv4Packet::format_ip(&offer_config.dhcp_server),
    );

    crate::serial_println!("[DHCP] : Sending REQUEST for {}...",
        Ipv4Packet::format_ip(&offer_config.ip_address));

    let request_pkt = build_dhcp_request(
        our_mac,
        offer_config.ip_address,
        offer_config.dhcp_server,
    );
    if let Err(e) = send_dhcp_broadcast(our_mac, request_pkt) {
        crate::serial_println!("[DHCP] Failed to send REQUEST: {}", e);
        super::set_our_ip(old_ip);
        return None;
    }

    
    crate::serial_println!("[DHCP] : Waiting for ACK...");
    let ack_config = wait_for_dhcp_message(DHCP_ACK, 300)?;
    crate::serial_println!("[DHCP] Got ACK! Configuration confirmed.");
    super::set_our_ip(ack_config.ip_address);
    Some(ack_config)
}
fn wait_for_dhcp_message(expected_type: u8, timeout_iterations: u32) -> Option<DhcpConfig> {
    for _ in 0..timeout_iterations {
        // Small delay
        for _ in 0..500_000u32 {
            core::hint::spin_loop();
        }

        // Poll for incoming packets
        while let Some(raw) = super::receive_raw() {
            // Parse the Ethernet frame
            let frame = match super::ethernet::EthernetFrame::parse(&raw) {
                Some(f) => f,
                None => continue,
            };

            // We only care about IPv4 frames
            if frame.ethertype != ETHERTYPE_IPV4 {
                // Still process ARP etc through normal path
                super::process_packet(&raw);
                continue;
            }

            // Parse IPv4
            let ip_pkt = match Ipv4Packet::parse(&frame.payload) {
                Some(p) => p,
                None => continue,
            };

            // We only care about UDP
            if ip_pkt.protocol != PROTO_UDP {
                continue;
            }

            // Parse UDP
            let udp_pkt = match UdpPacket::parse(&ip_pkt.payload) {
                Some(p) => p,
                None => continue,
            };

            // We only care about packets to port 68 (DHCP client)
            if udp_pkt.dst_port != DHCP_CLIENT_PORT {
                // Not DHCP — let normal processing handle it
                super::process_packet(&raw);
                continue;
            }

            // Try to parse as DHCP
            if let Some((msg_type, config)) = parse_dhcp_response(&udp_pkt.payload) {
                let type_name = match msg_type {
                    DHCP_OFFER => "OFFER",
                    DHCP_ACK => "ACK",
                    DHCP_NAK => "NAK",
                    _ => "UNKNOWN",
                };
                crate::serial_println!("[DHCP] Received,good job {}", type_name);

                if msg_type == DHCP_NAK {
                    crate::serial_println!("[DHCP]:Not fit for the server dumb ass !!");
                    return None;
                }

                if msg_type == expected_type {
                    return Some(config);
                }
            }
        }
    }
    crate::serial_println!("[DHCP] Timeout waiting for response");
    None
}