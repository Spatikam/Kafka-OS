// kernel/src/net/mod.rs
// Kafka-OS Networking Stack
//
// Phase 2: E1000 NIC Driver          ✓
// Phase 3: Ethernet + ARP            ✓
// Phase 4: IPv4 + UDP + DNS          ✓
// Phase 5: DHCP Client               ✓
// Phase 6: TCP + HTTP                ✓
// Phase 7: Applications              (next)

pub mod e1000;
pub mod ethernet;
pub mod arp;
pub mod ip;
pub mod udp;
pub mod dns;
pub mod dhcp;
pub mod tcp;

use alloc::vec::Vec;
use spin::Mutex;
use lazy_static::lazy_static;
use ethernet::{EthernetFrame, ETHERTYPE_ARP, ETHERTYPE_IPV4};
use ip::Ipv4Packet;

lazy_static! {
    pub static ref NIC: Mutex<Option<e1000::E1000>> = Mutex::new(None);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Network Configuration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

static OUR_IP: Mutex<[u8; 4]> = Mutex::new([10, 0, 2, 15]);
static GATEWAY: Mutex<[u8; 4]> = Mutex::new([10, 0, 2, 2]);
static DNS_SERVER: Mutex<[u8; 4]> = Mutex::new([10, 0, 2, 3]);

pub const DEFAULT_GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
pub const DEFAULT_DNS_IP: [u8; 4] = [10, 0, 2, 3];
pub const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
pub const DNS_IP: [u8; 4] = [10, 0, 2, 3];

static PHYS_MEM_OFFSET: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

pub fn set_phys_mem_offset(offset: u64) {
    PHYS_MEM_OFFSET.store(offset, core::sync::atomic::Ordering::SeqCst);
}
pub fn phys_mem_offset() -> u64 {
    PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::SeqCst)
}
pub fn phys_to_virt(phys: u64) -> u64 {
    phys_mem_offset() + phys
}
pub fn virt_to_phys(virt: u64) -> u64 {
    virt - phys_mem_offset()
}

pub fn our_ip() -> [u8; 4] { *OUR_IP.lock() }
pub fn set_our_ip(ip: [u8; 4]) { *OUR_IP.lock() = ip; }
pub fn gateway_ip() -> [u8; 4] { *GATEWAY.lock() }
pub fn set_gateway_ip(ip: [u8; 4]) { *GATEWAY.lock() = ip; }
pub fn dns_ip() -> [u8; 4] { *DNS_SERVER.lock() }
pub fn set_dns_ip(ip: [u8; 4]) { *DNS_SERVER.lock() = ip; }

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Initialization
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn init(
    frame_allocator: &mut impl x86_64::structures::paging::FrameAllocator<
        x86_64::structures::paging::Size4KiB,
    >,
    phys_offset: x86_64::VirtAddr,
) {
    set_phys_mem_offset(phys_offset.as_u64());

    let pci_device = match crate::pci::find_e1000() {
        Some(dev) => dev,
        None => {
            crate::serial_println!("[NET] No E1000 NIC found - networking disabled");
            return;
        }
    };

    crate::serial_println!("[NET] Initializing E1000 NIC...");
    pci_device.enable_for_nic();

    let mmio_phys = match pci_device.read_bar(0) {
        crate::pci::BarType::Memory32 { base_address } => base_address as u64,
        crate::pci::BarType::Memory64 { base_address } => base_address,
        other => {
            crate::serial_println!("[NET] Unexpected BAR0 type: {:?}", other);
            return;
        }
    };

    let irq = pci_device.interrupt_line;
    crate::serial_println!("[NET] BAR0 MMIO Physical: 0x{:X}", mmio_phys);
    crate::serial_println!("[NET] IRQ Line: {}", irq);

    let mmio_virt = phys_to_virt(mmio_phys);
    crate::serial_println!("[NET] BAR0 MMIO Virtual:  0x{:X}", mmio_virt);

    let nic = unsafe { e1000::E1000::new(mmio_virt as usize, frame_allocator) };

    let mac = nic.mac();
    crate::serial_println!(
        "[NET] MAC Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
    crate::serial_println!("[NET] E1000 NIC initialized successfully!");
    *NIC.lock() = Some(nic);
    crate::serial_println!("[NET] Ready to send and receive packets!");
}

pub fn print_config() {
    let ip = our_ip();
    let gw = gateway_ip();
    let dns = dns_ip();
    let mac = mac_address().unwrap_or([0; 6]);
    crate::serial_println!("┌─── Network Configuration ────────────────────┐");
    crate::serial_println!("│ MAC:     {}             │", EthernetFrame::format_mac(&mac));
    crate::serial_println!("│ IP:      {:<15}                │", Ipv4Packet::format_ip(&ip));
    crate::serial_println!("│ Gateway: {:<15}                │", Ipv4Packet::format_ip(&gw));
    crate::serial_println!("│ DNS:     {:<15}                │", Ipv4Packet::format_ip(&dns));
    crate::serial_println!("└──────────────────────────────────────────────┘");
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Raw Packet I/O
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn send_raw(data: &[u8]) -> Result<(), &'static str> {
    let mut nic_lock = NIC.lock();
    match nic_lock.as_mut() {
        Some(nic) => nic.send(data),
        None => Err("NIC not initialized"),
    }
}

pub fn receive_raw() -> Option<Vec<u8>> {
    let mut nic_lock = NIC.lock();
    match nic_lock.as_mut() {
        Some(nic) => nic.receive(),
        None => None,
    }
}

pub fn mac_address() -> Option<[u8; 6]> {
    let nic_lock = NIC.lock();
    nic_lock.as_ref().map(|nic| nic.mac())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Packet Processing
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn process_packet(raw: &[u8]) {
    let frame = match EthernetFrame::parse(raw) {
        Some(f) => f,
        None => {
            crate::serial_println!("[NET] Malformed frame ({} bytes)", raw.len());
            return;
        }
    };

    let our_mac = match mac_address() {
        Some(m) => m,
        None => return,
    };

    if !frame.is_for_us(&our_mac) {
        return;
    }

    match frame.ethertype {
        ETHERTYPE_ARP => {
            arp::handle_arp_packet(&frame.payload, our_mac, our_ip());
        }
        ETHERTYPE_IPV4 => {
            handle_ipv4_packet(&frame.payload);
        }
        _ => {}
    }
}

fn handle_ipv4_packet(data: &[u8]) {
    let ip_packet = match Ipv4Packet::parse(data) {
        Some(p) => p,
        None => {
            crate::serial_println!("[IP] Failed to parse IPv4 packet");
            return;
        }
    };

    let our_ip_addr = our_ip();
    if !ip_packet.is_for_us(&our_ip_addr) && our_ip_addr != [0, 0, 0, 0] {
        return;
    }

    match ip_packet.protocol {
        ip::PROTO_UDP => {
            crate::serial_println!(
                "[IP] UDP {} -> {} ({} bytes)",
                Ipv4Packet::format_ip(&ip_packet.src_ip),
                Ipv4Packet::format_ip(&ip_packet.dst_ip),
                ip_packet.payload.len()
            );
            udp::handle_udp_packet(&ip_packet);
        }
        ip::PROTO_TCP => {
            // TCP handles its own logging
            tcp::handle_tcp_packet(&ip_packet);
        }
        ip::PROTO_ICMP => {
            crate::serial_println!("[ICMP] Ping received (not yet handled)");
        }
        _ => {
            crate::serial_println!("[IP] Unknown protocol: {}", ip_packet.protocol);
        }
    }
}

pub fn poll_packets() -> usize {
    let mut count = 0;
    while let Some(raw) = receive_raw() {
        process_packet(&raw);
        count += 1;
    }
    count
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: DHCP (Phase 5)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn test_dhcp() {
    crate::serial_println!();
    crate::serial_println!("╔══════════════════════════════════════════════╗");
    crate::serial_println!("║    DHCP Test — Dynamic IP Assignment!       ║");
    crate::serial_println!("╚══════════════════════════════════════════════╝");

    match dhcp::discover() {
        Some(config) => {
            set_our_ip(config.ip_address);
            set_gateway_ip(config.gateway);
            set_dns_ip(config.dns_server);
            crate::serial_println!();
            crate::serial_println!("╔══════════════════════════════════════════════╗");
            crate::serial_println!("║  ✓ DHCP Configuration Received!             ║");
            crate::serial_println!("║  IP:      {}                  ║", Ipv4Packet::format_ip(&config.ip_address));
            crate::serial_println!("║  Mask:    {}              ║", Ipv4Packet::format_ip(&config.subnet_mask));
            crate::serial_println!("║  Gateway: {}                   ║", Ipv4Packet::format_ip(&config.gateway));
            crate::serial_println!("║  DNS:     {}                   ║", Ipv4Packet::format_ip(&config.dns_server));
            crate::serial_println!("║  Lease:   {} seconds                       ║", config.lease_time);
            crate::serial_println!("║  IP ASSIGNED BY DHCP!                        ║");
            crate::serial_println!("╚══════════════════════════════════════════════╝");
        }
        None => {
            crate::serial_println!("[DHCP] Failed — using defaults");
        }
    }
    print_config();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: ARP (Phase 3)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn test_arp() {
    let our_mac = match mac_address() {
        Some(m) => m,
        None => { crate::serial_println!("[ARP TEST] NIC not initialized!"); return; }
    };

    let our_ip = our_ip();
    let target_ip = gateway_ip();

    crate::serial_println!("╔══════════════════════════════════════════════╗");
    crate::serial_println!("║          ARP Test — First Packet!           ║");
    crate::serial_println!("╚══════════════════════════════════════════════╝");

    arp::send_arp_request(our_mac, our_ip, target_ip);
    crate::serial_println!("[ARP TEST] Waiting for reply...");

    let mut got_reply = false;
    for attempt in 0..200u32 {
        for _ in 0..1_000_000u32 { core::hint::spin_loop(); }
        while let Some(raw) = receive_raw() { process_packet(&raw); }
        if arp::ARP_TABLE.lock().lookup(&target_ip).is_some() {
            got_reply = true;
            crate::serial_println!("[ARP TEST] Reply on attempt {}", attempt);
            break;
        }
        if attempt % 50 == 49 {
            arp::send_arp_request(our_mac, our_ip, target_ip);
        }
    }

    if got_reply {
        let gw_mac = arp::ARP_TABLE.lock().lookup(&target_ip).unwrap();
        crate::serial_println!("║  ✓ Gateway MAC: {}  ║", EthernetFrame::format_mac(&gw_mac));
    } else {
        crate::serial_println!("║  ✗ No ARP reply (timeout)  ║");
    }
    arp::ARP_TABLE.lock().print_table();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: DNS (Phase 4)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub fn test_dns() {
    crate::serial_println!();
    crate::serial_println!("╔══════════════════════════════════════════════╗");
    crate::serial_println!("║     DNS Test — Resolving a Domain Name!     ║");
    crate::serial_println!("╚══════════════════════════════════════════════╝");

    match dns::resolve("google.com") {
        Some(ip) => {
            crate::serial_println!("║  ✓ google.com -> {}  ║", Ipv4Packet::format_ip(&ip));
        }
        None => {
            crate::serial_println!("║  ✗ DNS resolution failed  ║");
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Test: TCP + HTTP (Phase 6)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Test TCP by making an HTTP GET request to example.com.
///
/// This exercises the full stack:
///   DNS resolve → TCP handshake → HTTP GET → receive → close
///
/// Call from main.rs:
/// ```
/// blog_os::net::test_tcp_http();
/// ```
pub fn test_tcp_http() {
    crate::serial_println!();
    crate::serial_println!("╔══════════════════════════════════════════════╗");
    crate::serial_println!("║   TCP+HTTP Test — Fetching a Web Page!      ║");
    crate::serial_println!("╚══════════════════════════════════════════════╝");

    // Step 1: Resolve example.com
    crate::serial_println!("[HTTP] Resolving example.com...");
    let server_ip = match dns::resolve("example.com") {
        Some(ip) => {
            crate::serial_println!("[HTTP] example.com -> {}", Ipv4Packet::format_ip(&ip));
            ip
        }
        None => {
            crate::serial_println!("[HTTP] DNS resolution failed!");
            return;
        }
    };

    // Step 2: TCP connect to port 80
    crate::serial_println!("[HTTP] Connecting to {}:80...", Ipv4Packet::format_ip(&server_ip));
    let local_port = match tcp::tcp_connect(server_ip, 80) {
        Ok(port) => {
            crate::serial_println!("[HTTP] Connected! (local port {})", port);
            port
        }
        Err(e) => {
            crate::serial_println!("[HTTP] Connection failed: {}", e);
            return;
        }
    };

    // Step 3: Send HTTP/1.0 GET request
    // HTTP/1.0 closes the connection after the response (simpler than 1.1)
    let request = b"GET / HTTP/1.0\r\nHost: example.com\r\nAccept: */*\r\n\r\n";

    crate::serial_println!("[HTTP] Sending GET request ({} bytes)...", request.len());
    if let Err(e) = tcp::tcp_send(local_port, request) {
        crate::serial_println!("[HTTP] Send failed: {}", e);
        let _ = tcp::tcp_close(local_port);
        return;
    }

    // Step 4: Receive the HTTP response
    crate::serial_println!("[HTTP] Waiting for response...");
    let response = match tcp::tcp_receive_all(local_port, 500) {
        Ok(data) => data,
        Err(e) => {
            crate::serial_println!("[HTTP] Receive failed: {}", e);
            let _ = tcp::tcp_close(local_port);
            return;
        }
    };

    // Step 5: Close the connection
    let _ = tcp::tcp_close(local_port);

    // Step 6: Display results
    if response.is_empty() {
        crate::serial_println!("[HTTP] Empty response!");
        return;
    }

    crate::serial_println!();
    crate::serial_println!("╔══════════════════════════════════════════════╗");
    crate::serial_println!("║  ✓ HTTP Response Received! ({:5} bytes)     ║", response.len());
    crate::serial_println!("╠══════════════════════════════════════════════╣");

    // Convert to string and print first lines
    let response_str = core::str::from_utf8(&response).unwrap_or("<binary data>");

    // Print the first 500 characters (or less)
    let preview_len = core::cmp::min(response_str.len(), 500);
    let preview = &response_str[..preview_len];

    // Print line by line
    for line in preview.lines().take(15) {
        crate::serial_println!("║ {}", line);
    }

    if response_str.len() > preview_len {
        crate::serial_println!("║ ... ({} more bytes)", response_str.len() - preview_len);
    }

    crate::serial_println!("╠══════════════════════════════════════════════╣");
    crate::serial_println!("║                                              ║");
    crate::serial_println!("║  YOUR OS JUST FETCHED A WEB PAGE!           ║");
    crate::serial_println!("║  FROM THE REAL INTERNET!                    ║");
    crate::serial_println!("║                                              ║");
    crate::serial_println!("╚══════════════════════════════════════════════╝");
}