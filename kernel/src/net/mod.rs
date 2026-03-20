pub mod pci;
pub mod ethernet;
pub mod arp;
pub mod ipv4;
pub mod icmp;




use spin::Mutex;
use lazy_static::lazy_static;
use core::sync::atomic::{AtomicU64, Ordering};
lazy_static! {
    static ref MAC_ADDRESS: Mutex<Option<[u8; 6]>> = Mutex::new(None);
    static ref IP_ADDRESS: Mutex<Option<[u8; 4]>> = Mutex::new(None);
    
}
static RX_PACKETS: AtomicU64 = AtomicU64::new(0);
static TX_PACKETS: AtomicU64 = AtomicU64::new(0);

pub fn set_mac_address(mac: [u8; 6]) {
    *MAC_ADDRESS.lock() = Some(mac);
}

pub fn set_ip_address(ip: [u8; 4]) {
    *IP_ADDRESS.lock() = Some(ip);
}

pub fn get_mac_address() -> Option<[u8; 6]> {
    *MAC_ADDRESS.lock()
}

pub fn get_ip_address() -> Option<[u8; 4]> {
    *IP_ADDRESS.lock()
}
pub fn increment_rx() {
    RX_PACKETS.fetch_add(1, Ordering::Relaxed);
}

pub fn increment_tx() {
    TX_PACKETS.fetch_add(1, Ordering::Relaxed);
}

pub fn get_rx_count() -> u64 {
    RX_PACKETS.load(Ordering::Relaxed)
}

pub fn get_tx_count() -> u64 {
    TX_PACKETS.load(Ordering::Relaxed)
}