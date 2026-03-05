// kernel/src/pci.rs
// Phase 1: PCI Bus Enumeration for Kafka-OS
//
// This module scans the PCI bus to detect connected devices.
// Primary goal: find the Intel E1000 NIC (vendor 0x8086, device 0x100E)
// which QEMU exposes as a virtual network card.

use alloc::vec::Vec;
use x86_64::instructions::port::Port;
use crate::serial_println;

// ─── PCI Configuration Space Ports ───────────────────────
// All PCI config access goes through these two I/O ports.
// Write an address to CONFIG_ADDRESS, then read/write CONFIG_DATA.
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

// ─── Well-Known Vendor/Device IDs ────────────────────────
pub const VENDOR_INTEL: u16 = 0x8086;
pub const DEVICE_E1000: u16 = 0x100E; // 82540EM (QEMU default)

// ─── PCI Class Codes ─────────────────────────────────────
pub const CLASS_NETWORK: u8 = 0x02;
pub const SUBCLASS_ETHERNET: u8 = 0x00;

// ─── PCI Command Register Bits ───────────────────────────
pub const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
pub const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
pub const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;
pub const PCI_COMMAND_INTERRUPT_DISABLE: u16 = 1 << 10;

// ─── BAR (Base Address Register) Types ───────────────────
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarType {
    /// Memory-mapped I/O (most common for modern devices)
    Memory32 { base_address: u32 },
    /// 64-bit memory-mapped I/O
    Memory64 { base_address: u64 },
    /// Port-mapped I/O
    IoPort { base_address: u32 },
    /// BAR is not present / zero
    None,
}

// ─── PCI Device ──────────────────────────────────────────
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    pub header_type: u8,
    pub interrupt_line: u8,
    pub interrupt_pin: u8,
}

impl PciDevice {
    // ─── Raw Config Space Access ─────────────────────────

    /// Read a 32-bit value from PCI configuration space.
    ///
    /// The PCI config address format (32 bits):
    /// [31]    Enable bit (must be 1)
    /// [23:16] Bus number
    /// [15:11] Device number
    /// [10:8]  Function number
    /// [7:2]   Register offset (aligned to 4 bytes)
    /// [1:0]   Always 0
    pub fn config_read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        let address: u32 = (1u32 << 31)                    // Enable bit
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC);                    // Align to 4 bytes

        unsafe {
            let mut addr_port: Port<u32> = Port::new(PCI_CONFIG_ADDRESS);
            let mut data_port: Port<u32> = Port::new(PCI_CONFIG_DATA);
            addr_port.write(address);
            data_port.read()
        }
    }

    /// Write a 32-bit value to PCI configuration space.
    pub fn config_write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
        let address: u32 = (1u32 << 31)
            | ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC);

        unsafe {
            let mut addr_port: Port<u32> = Port::new(PCI_CONFIG_ADDRESS);
            let mut data_port: Port<u32> = Port::new(PCI_CONFIG_DATA);
            addr_port.write(address);
            data_port.write(value);
        }
    }

    /// Read a 16-bit value from PCI configuration space.
    pub fn config_read_u16(&self, offset: u8) -> u16 {
        let val = Self::config_read_u32(self.bus, self.device, self.function, offset & 0xFC);
        // Extract the correct 16-bit half based on offset alignment
        ((val >> ((offset as u32 & 2) * 8)) & 0xFFFF) as u16
    }

    /// Write a 16-bit value to PCI configuration space.
    pub fn config_write_u16(&self, offset: u8, value: u16) {
        let current = Self::config_read_u32(self.bus, self.device, self.function, offset & 0xFC);
        let shift = (offset as u32 & 2) * 8;
        let mask = !(0xFFFFu32 << shift);
        let new_val = (current & mask) | ((value as u32) << shift);
        Self::config_write_u32(self.bus, self.device, self.function, offset & 0xFC, new_val);
    }

    // ─── Device Properties ───────────────────────────────

    /// Read a Base Address Register (BAR0-BAR5).
    ///
    /// BARs tell us where the device's registers/memory are mapped.
    /// For the E1000, BAR0 contains the MMIO base address.
    pub fn read_bar(&self, bar_index: u8) -> BarType {
        if bar_index > 5 {
            return BarType::None;
        }

        let offset = 0x10 + (bar_index * 4);
        let bar_value = Self::config_read_u32(self.bus, self.device, self.function, offset);

        if bar_value == 0 {
            return BarType::None;
        }

        if bar_value & 1 == 1 {
            // I/O Space BAR
            BarType::IoPort {
                base_address: bar_value & 0xFFFFFFFC,
            }
        } else {
            // Memory Space BAR
            let bar_type = (bar_value >> 1) & 0x3;
            match bar_type {
                0 => {
                    // 32-bit memory BAR
                    BarType::Memory32 {
                        base_address: bar_value & 0xFFFFFFF0,
                    }
                }
                2 => {
                    // 64-bit memory BAR (spans two BARs)
                    let high = Self::config_read_u32(
                        self.bus,
                        self.device,
                        self.function,
                        offset + 4,
                    );
                    let addr = ((high as u64) << 32) | ((bar_value & 0xFFFFFFF0) as u64);
                    BarType::Memory64 {
                        base_address: addr,
                    }
                }
                _ => BarType::None,
            }
        }
    }

    /// Determine the size of a BAR's memory region.
    ///
    /// Works by: save BAR → write all 1s → read back → restore BAR.
    /// The pattern of writable bits tells us the size.
    pub fn bar_size(&self, bar_index: u8) -> u32 {
        let offset = 0x10 + (bar_index * 4);
        let original = Self::config_read_u32(self.bus, self.device, self.function, offset);

        // Write all 1s
        Self::config_write_u32(self.bus, self.device, self.function, offset, 0xFFFFFFFF);
        let size_mask = Self::config_read_u32(self.bus, self.device, self.function, offset);

        // Restore original value
        Self::config_write_u32(self.bus, self.device, self.function, offset, original);

        if size_mask == 0 {
            return 0;
        }

        // For memory BARs, mask the lower 4 bits; for I/O, mask lower 2
        let mask = if original & 1 == 1 {
            size_mask & 0xFFFFFFFC
        } else {
            size_mask & 0xFFFFFFF0
        };

        // Size = ~mask + 1 (two's complement trick)
        (!mask).wrapping_add(1)
    }

    // ─── Command Register Operations ─────────────────────

    /// Read the PCI command register.
    pub fn read_command(&self) -> u16 {
        self.config_read_u16(0x04)
    }

    /// Write the PCI command register.
    pub fn write_command(&self, value: u16) {
        self.config_write_u16(0x04, value);
    }

    /// Enable PCI bus mastering.
    ///
    /// This is REQUIRED for DMA — the NIC needs to read/write
    /// system memory directly to transfer packet data.
    pub fn enable_bus_mastering(&self) {
        let cmd = self.read_command();
        self.write_command(cmd | PCI_COMMAND_BUS_MASTER);
    }

    /// Enable memory space access (for MMIO).
    pub fn enable_memory_space(&self) {
        let cmd = self.read_command();
        self.write_command(cmd | PCI_COMMAND_MEMORY_SPACE);
    }

    /// Enable both bus mastering and memory space (common combo for NICs).
    pub fn enable_for_nic(&self) {
        let cmd = self.read_command();
        self.write_command(cmd | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_MEMORY_SPACE);
    }

    // ─── Display Helpers ─────────────────────────────────

    /// Human-readable device class name
    pub fn class_name(&self) -> &'static str {
        match (self.class_code, self.subclass) {
            (0x00, _) => "Unclassified",
            (0x01, 0x00) => "SCSI Storage",
            (0x01, 0x01) => "IDE Controller",
            (0x01, 0x06) => "SATA Controller",
            (0x02, 0x00) => "Ethernet Controller",
            (0x02, 0x80) => "Other Network Controller",
            (0x03, 0x00) => "VGA Controller",
            (0x04, _) => "Multimedia",
            (0x05, _) => "Memory Controller",
            (0x06, 0x00) => "Host Bridge",
            (0x06, 0x01) => "ISA Bridge",
            (0x06, 0x04) => "PCI-to-PCI Bridge",
            (0x06, _) => "Bridge Device",
            (0x0C, 0x03) => "USB Controller",
            _ => "Unknown",
        }
    }

    /// Check if this is a network controller
    pub fn is_network_controller(&self) -> bool {
        self.class_code == CLASS_NETWORK
    }

    /// Check if this is specifically an Intel E1000
    pub fn is_e1000(&self) -> bool {
        self.vendor_id == VENDOR_INTEL && self.device_id == DEVICE_E1000
    }
}

// ─── PCI Bus Scanner ─────────────────────────────────────

/// Scan the entire PCI bus and return all detected devices.
///
/// Iterates over all possible bus/device/function combinations.
/// Skips empty slots (vendor_id == 0xFFFF).
pub fn scan_pci_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for bus in 0u8..=255 {
        for device in 0u8..32 {
            scan_device(&mut devices, bus, device);
        }
    }

    devices
}

fn scan_device(devices: &mut Vec<PciDevice>, bus: u8, device: u8) {
    let vendor_device = PciDevice::config_read_u32(bus, device, 0, 0x00);
    let vendor_id = (vendor_device & 0xFFFF) as u16;

    if vendor_id == 0xFFFF {
        return; // No device in this slot
    }

    // Check function 0
    scan_function(devices, bus, device, 0);

    // Check header type for multi-function device
    let header_info = PciDevice::config_read_u32(bus, device, 0, 0x0C);
    let header_type = ((header_info >> 16) & 0xFF) as u8;

    if header_type & 0x80 != 0 {
        // Multi-function device: check remaining functions
        for function in 1u8..8 {
            let vd = PciDevice::config_read_u32(bus, device, function, 0x00);
            if (vd & 0xFFFF) as u16 != 0xFFFF {
                scan_function(devices, bus, device, function);
            }
        }
    }
}

fn scan_function(devices: &mut Vec<PciDevice>, bus: u8, device: u8, function: u8) {
    let vendor_device = PciDevice::config_read_u32(bus, device, function, 0x00);
    let vendor_id = (vendor_device & 0xFFFF) as u16;
    let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;

    if vendor_id == 0xFFFF {
        return;
    }

    let class_rev = PciDevice::config_read_u32(bus, device, function, 0x08);
    let class_code = ((class_rev >> 24) & 0xFF) as u8;
    let subclass = ((class_rev >> 16) & 0xFF) as u8;
    let prog_if = ((class_rev >> 8) & 0xFF) as u8;
    let revision = (class_rev & 0xFF) as u8;

    let header_info = PciDevice::config_read_u32(bus, device, function, 0x0C);
    let header_type = ((header_info >> 16) & 0xFF) as u8;

    let interrupt_info = PciDevice::config_read_u32(bus, device, function, 0x3C);
    let interrupt_line = (interrupt_info & 0xFF) as u8;
    let interrupt_pin = ((interrupt_info >> 8) & 0xFF) as u8;

    devices.push(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        revision,
        header_type: header_type & 0x7F, // Mask off multi-function bit
        interrupt_line,
        interrupt_pin,
    });
}

// ─── Convenience Functions ───────────────────────────────

/// Find the Intel E1000 NIC on the PCI bus.
/// Returns None if not found (e.g., QEMU launched without -device e1000).
pub fn find_e1000() -> Option<PciDevice> {
    scan_pci_bus()
        .into_iter()
        .find(|dev| dev.is_e1000())
}

/// Find all network controllers on the PCI bus.
pub fn find_network_controllers() -> Vec<PciDevice> {
    scan_pci_bus()
        .into_iter()
        .filter(|dev| dev.is_network_controller())
        .collect()
}

/// Print all PCI devices to serial output.
///
/// Call this from main.rs to verify PCI scanning works:
/// ```
/// pci::print_pci_devices();
/// ```
pub fn print_pci_devices() {
    let devices = scan_pci_bus();

    serial_println!("╔══════════════════════════════════════════════════════════════╗");
    serial_println!("║                    PCI Device Scan                          ║");
    serial_println!("╠══════════════════════════════════════════════════════════════╣");
    serial_println!("║ Bus:Dev.Fn │ Vendor:Device │ Class          │ IRQ           ║");
    serial_println!("╠══════════════════════════════════════════════════════════════╣");

    for dev in &devices {
        serial_println!(
            "║ {:02x}:{:02x}.{:01x}   │ {:04x}:{:04x}      │ {:<14} │ IRQ {}        ║",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class_name(),
            dev.interrupt_line
        );
    }

    serial_println!("╠══════════════════════════════════════════════════════════════╣");
    serial_println!("║ Total devices: {:<46}║", devices.len());
    serial_println!("╚══════════════════════════════════════════════════════════════╝");

    // Specifically highlight the E1000 if found
    if let Some(e1000) = devices.iter().find(|d| d.is_e1000()) {
        serial_println!();
        serial_println!("┌─────────────────────────────────────┐");
        serial_println!("│  ✓ E1000 NIC Found!                 │");
        serial_println!("│  Bus {:02x}, Device {:02x}, Function {:02x}    │",
            e1000.bus, e1000.device, e1000.function);

        match e1000.read_bar(0) {
            BarType::Memory32 { base_address } => {
                serial_println!("│  BAR0 (MMIO): 0x{:08X}            │", base_address);
                serial_println!("│  BAR0 Size:   {} KB               │",
                    e1000.bar_size(0) / 1024);
            }
            BarType::Memory64 { base_address } => {
                serial_println!("│  BAR0 (MMIO64): 0x{:016X}  │", base_address);
            }
            _ => {
                serial_println!("│  BAR0: unexpected type             │");
            }
        }

        serial_println!("│  IRQ Line: {}                       │", e1000.interrupt_line);
        serial_println!("│  Ready for Phase 2 (NIC Driver)     │");
        serial_println!("└─────────────────────────────────────┘");
    } else {
        serial_println!();
        serial_println!("⚠ E1000 NIC not found!");
        serial_println!("  Make sure QEMU is launched with:");
        serial_println!("  -netdev user,id=net0 -device e1000,netdev=net0");
    }
}