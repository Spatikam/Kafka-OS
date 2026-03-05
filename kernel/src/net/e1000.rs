// kernel/src/net/e1000.rs
// Intel E1000 (82540EM) NIC Driver for Kafka-OS
//
// This driver talks to QEMU's virtual E1000 NIC via MMIO registers
// and DMA descriptor rings for packet TX/RX.
//
// Reference: Intel PCI/PCI-X Family of Gigabit Ethernet Controllers
//            Software Developer's Manual (SDM)

use alloc::vec::Vec;
use core::ptr;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// E1000 Register Offsets (from the Intel SDM)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

// Device Control & Status
const REG_CTRL: u32    = 0x0000;  // Device Control
const REG_STATUS: u32  = 0x0008;  // Device Status

// Interrupt Control
const REG_ICR: u32     = 0x00C0;  // Interrupt Cause Read
const REG_IMS: u32     = 0x00D0;  // Interrupt Mask Set
const REG_IMC: u32     = 0x00D8;  // Interrupt Mask Clear

// Receive Registers
const REG_RCTL: u32    = 0x0100;  // Receive Control
const REG_RDBAL: u32   = 0x2800;  // RX Descriptor Base Address Low
const REG_RDBAH: u32   = 0x2804;  // RX Descriptor Base Address High
const REG_RDLEN: u32   = 0x2808;  // RX Descriptor Length (bytes)
const REG_RDH: u32     = 0x2810;  // RX Descriptor Head
const REG_RDT: u32     = 0x2818;  // RX Descriptor Tail

// Transmit Registers
const REG_TCTL: u32    = 0x0400;  // Transmit Control
const REG_TIPG: u32    = 0x0410;  // Transmit Inter-Packet Gap
const REG_TDBAL: u32   = 0x3800;  // TX Descriptor Base Address Low
const REG_TDBAH: u32   = 0x3804;  // TX Descriptor Base Address High
const REG_TDLEN: u32   = 0x3808;  // TX Descriptor Length (bytes)
const REG_TDH: u32     = 0x3810;  // TX Descriptor Head
const REG_TDT: u32     = 0x3818;  // TX Descriptor Tail

// MAC Address
const REG_RAL: u32     = 0x5400;  // Receive Address Low
const REG_RAH: u32     = 0x5404;  // Receive Address High

// Multicast Table Array (128 entries)
const REG_MTA_BASE: u32 = 0x5200;

// Control Register Bits

const CTRL_SLU: u32   = 1 << 6;   // Set Link Up
const CTRL_RST: u32   = 1 << 26;  // Device Reset


// Receive Control Register Bits
const RCTL_EN: u32         = 1 << 1;   // Receiver Enable
const RCTL_BAM: u32        = 1 << 15;  // Broadcast Accept Mode
const RCTL_LBM_NONE: u32   = 0 << 6;   // No Loopback
const RCTL_BSIZE_2048: u32  = 0 << 16;  // Buffer Size = 2048 bytes
const RCTL_SECRC: u32      = 1 << 26;  // Strip Ethernet CRC
const RCTL_UPE: u32        = 1 << 3;   // Unicast Promiscuous Enable


// Transmit Control Register Bits
const TCTL_EN: u32         = 1 << 1;   // Transmit Enable
const TCTL_PSP: u32        = 1 << 3;   // Pad Short Packets
const TCTL_CT_SHIFT: u32   = 4;        // Collision Threshold
const TCTL_COLD_SHIFT: u32 = 12;       // Collision Distance


// TX Descriptor Command & Status Bits
const TDESC_CMD_EOP: u8   = 1 << 0;  // End of Packet
const TDESC_CMD_IFCS: u8  = 1 << 1;  // Insert FCS/CRC
const TDESC_CMD_RS: u8    = 1 << 3;  // Report Status
const TDESC_STA_DD: u8    = 1 << 0;  // Descriptor Done

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// RX Descriptor Status Bits
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const RDESC_STA_DD: u8    = 1 << 0;  // Descriptor Done
const RDESC_STA_EOP: u8   = 1 << 1;  // End of Packet

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Descriptor Ring Sizes
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

//Number of RX descriptors (must be multiple of 8, min 8)
const NUM_RX_DESC: usize = 32;
/// Number of TX descriptors (must be multiple of 8, min 8)  // for now keeping it as 8, i guess we would have to increase when we write a driver 
const NUM_TX_DESC: usize = 8;
/// Size of each packet buffer
const PACKET_BUFFER_SIZE: usize = 2048;
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct RxDescriptor {
    pub buffer_addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

/// Transmit Descriptor — 16 bytes, hardware-defined layout.
///
/// We fill in `buffer_addr`, `length`, and `cmd`, then advance
/// the tail pointer to tell the NIC to send it.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TxDescriptor {
    pub buffer_addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// DMA Buffer — tracks both physical and virtual addresses
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A DMA-capable buffer with known physical address.
///
/// The NIC needs physical addresses for DMA, but we need
/// virtual addresses to read/write the data from the kernel.
#[derive(Clone, Copy)]
struct DmaBuffer {
    /// Physical address (what the NIC sees)
    phys: u64,
    /// Virtual address (what we use to read/write)
    virt: *mut u8,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// E1000 Driver
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct E1000 {
    /// Base virtual address of MMIO register space (BAR0 mapped)
    mmio_base: usize,
    /// Our MAC address (read from hardware)
    mac_address: [u8; 6],

    // ── RX State ──
    rx_descs_virt: *mut RxDescriptor,
    rx_descs_phys: u64,
    rx_buffers: [DmaBuffer; NUM_RX_DESC],
    rx_cur: usize,

    // ── TX State ──
    tx_descs_virt: *mut TxDescriptor,
    tx_descs_phys: u64,
    tx_buffers: [DmaBuffer; NUM_TX_DESC],
    tx_cur: usize,
}

// Safety: E1000 is only accessed through a Mutex<Option<E1000>> in mod.rs
unsafe impl Send for E1000 {}

/// Helper: allocate a physical frame and return both addresses.
///
/// Uses your BootInfoFrameAllocator to get a 4KiB page,
/// then calculates the virtual address via phys_mem_offset.
fn alloc_dma_frame(
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> DmaBuffer {
    let frame: PhysFrame = frame_allocator
        .allocate_frame()
        .expect("[E1000] Failed to allocate DMA frame");

    let phys = frame.start_address().as_u64();
    let virt = super::phys_to_virt(phys);

    // Zero out the page
    unsafe {
        ptr::write_bytes(virt as *mut u8, 0, 4096);
    }

    DmaBuffer {
        phys,
        virt: virt as *mut u8,
    }
}

impl E1000 {
    /// Initialize the E1000 NIC.
    ///
    /// `mmio_base`: Virtual address of BAR0 (already mapped by bootloader)
    /// `frame_allocator`: Your BootInfoFrameAllocator for DMA buffers
    pub unsafe fn new(
        mmio_base: usize,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Self {
        // Allocate descriptor rings (each needs a full page, 16-byte aligned)
        let rx_ring = alloc_dma_frame(frame_allocator);
        let tx_ring = alloc_dma_frame(frame_allocator);

        // Allocate packet buffers for RX (one page each = 4096 bytes, plenty for 2048-byte packets)
        let null_buf = DmaBuffer { phys: 0, virt: ptr::null_mut() };
        let mut rx_buffers = [null_buf; NUM_RX_DESC];
        for buf in rx_buffers.iter_mut() {
            *buf = alloc_dma_frame(frame_allocator);
        }

        // Allocate packet buffers for TX
        let mut tx_buffers = [null_buf; NUM_TX_DESC];
        for buf in tx_buffers.iter_mut() {
            *buf = alloc_dma_frame(frame_allocator);
        }

        let mut nic = E1000 {
            mmio_base,
            mac_address: [0u8; 6],
            rx_descs_virt: rx_ring.virt as *mut RxDescriptor,
            rx_descs_phys: rx_ring.phys,
            rx_buffers,
            rx_cur: 0,
            tx_descs_virt: tx_ring.virt as *mut TxDescriptor,
            tx_descs_phys: tx_ring.phys,
            tx_buffers,
            tx_cur: 0,
        };

        crate::serial_println!("[E1000] Resetting device...");
        nic.reset();

        crate::serial_println!("[E1000] Reading MAC address...");
        nic.read_mac_address();

        crate::serial_println!("[E1000] Initializing RX ring ({} descriptors)...", NUM_RX_DESC);
        nic.init_rx();

        crate::serial_println!("[E1000] Initializing TX ring ({} descriptors)...", NUM_TX_DESC);
        nic.init_tx();

        crate::serial_println!("[E1000] Setting link up...");
        nic.link_up();

        crate::serial_println!("[E1000] Enabling interrupts...");
        nic.enable_interrupts();

        // Read and display link status
        let status = nic.read_reg(REG_STATUS);
        let link_up = (status & 0x02) != 0;
        let speed = match (status >> 6) & 0x03 {
            0 => "10 Mb/s",
            1 => "100 Mb/s",
            2 | 3 => "1000 Mb/s",
            _ => "unknown",
        };
        crate::serial_println!("[E1000] Link: {} | Speed: {}",
            if link_up { "UP" } else { "DOWN" }, speed);

        nic
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Register Access
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn read_reg(&self, reg: u32) -> u32 {
        unsafe { ptr::read_volatile((self.mmio_base + reg as usize) as *const u32) }
    }

    fn write_reg(&self, reg: u32, value: u32) {
        unsafe { ptr::write_volatile((self.mmio_base + reg as usize) as *mut u32, value) }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Initialization
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn reset(&self) {
        // Disable interrupts first
        self.write_reg(REG_IMC, 0xFFFFFFFF);

        // Disable RX and TX
        self.write_reg(REG_RCTL, 0);
        self.write_reg(REG_TCTL, 0);

        // Trigger device reset
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | CTRL_RST);

        // Wait for reset to complete (spin — QEMU is fast)
        for _ in 0..100_000 {
            core::hint::spin_loop();
        }

        // Disable interrupts again (reset re-enables them)
        self.write_reg(REG_IMC, 0xFFFFFFFF);

        // Clear any pending interrupts
        self.read_reg(REG_ICR);

        crate::serial_println!("[E1000] Device reset complete");
    }

    fn read_mac_address(&mut self) {
        // QEMU pre-loads RAL/RAH with a MAC (typically 52:54:00:xx:xx:xx)
        let low = self.read_reg(REG_RAL);
        let high = self.read_reg(REG_RAH);

        self.mac_address[0] = (low & 0xFF) as u8;
        self.mac_address[1] = ((low >> 8) & 0xFF) as u8;
        self.mac_address[2] = ((low >> 16) & 0xFF) as u8;
        self.mac_address[3] = ((low >> 24) & 0xFF) as u8;
        self.mac_address[4] = (high & 0xFF) as u8;
        self.mac_address[5] = ((high >> 8) & 0xFF) as u8;

        // so yeah initially i was not able to get the thing inorder to validate.
        // that means something like reset is the bug 
        self.write_reg(REG_RAL,low);
        self.write_reg(REG_RAH,(high & 0xFFFF) | (1<<31)); // (31 bits should be good)

        crate::serial_println!(
            "[E1000] MAC: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac_address[0], self.mac_address[1], self.mac_address[2],
            self.mac_address[3], self.mac_address[4], self.mac_address[5]
        );
    }

    fn init_rx(&mut self) {
        unsafe {
            for i in 0..NUM_RX_DESC {
                let desc = &mut *self.rx_descs_virt.add(i);
                desc.buffer_addr = self.rx_buffers[i].phys;
                desc.length = 0;
                desc.checksum = 0;
                desc.status = 0;
                desc.errors = 0;
                desc.special = 0;
            }
        }

        // Point NIC at the RX descriptor ring (physical address!)
        self.write_reg(REG_RDBAL, (self.rx_descs_phys & 0xFFFFFFFF) as u32);
        self.write_reg(REG_RDBAH, (self.rx_descs_phys >> 32) as u32);

        // Ring size in bytes
        let ring_size = (NUM_RX_DESC * core::mem::size_of::<RxDescriptor>()) as u32;
        self.write_reg(REG_RDLEN, ring_size);

        // Head = 0 (NIC writes here), Tail = last descriptor
        self.write_reg(REG_RDH, 0);
        self.write_reg(REG_RDT, (NUM_RX_DESC - 1) as u32);
        self.rx_cur = 0;

        // Clear multicast table
        for i in 0..128u32 {
            self.write_reg(REG_MTA_BASE + (i * 4), 0);
        }

        // Enable receiver
        self.write_reg(
            REG_RCTL,
            RCTL_EN | RCTL_BAM | RCTL_UPE | RCTL_LBM_NONE | RCTL_BSIZE_2048 | RCTL_SECRC,
        );

        crate::serial_println!(
            "[E1000] RX ring at phys 0x{:X}, {} descs x {} byte buffers",
            self.rx_descs_phys, NUM_RX_DESC, PACKET_BUFFER_SIZE
        );
    }

    fn init_tx(&mut self) {
        unsafe {
            for i in 0..NUM_TX_DESC {
                let desc = &mut *self.tx_descs_virt.add(i);
                desc.buffer_addr = self.tx_buffers[i].phys;
                desc.length = 0;
                desc.cso = 0;
                desc.cmd = 0;
                desc.status = TDESC_STA_DD; // Mark done so first send() works
                desc.css = 0;
                desc.special = 0;
            }
        }

        // Point NIC at the TX descriptor ring
        self.write_reg(REG_TDBAL, (self.tx_descs_phys & 0xFFFFFFFF) as u32);
        self.write_reg(REG_TDBAH, (self.tx_descs_phys >> 32) as u32);

        let ring_size = (NUM_TX_DESC * core::mem::size_of::<TxDescriptor>()) as u32;
        self.write_reg(REG_TDLEN, ring_size);

        self.write_reg(REG_TDH, 0);
        self.write_reg(REG_TDT, 0);
        self.tx_cur = 0;

        // Enable transmitter
        self.write_reg(
            REG_TCTL,
            TCTL_EN | TCTL_PSP | (15 << TCTL_CT_SHIFT) | (64 << TCTL_COLD_SHIFT),
        );

        // Inter-Packet Gap: IPGT=10, IPGR1=10, IPGR2=10 (standard)
        self.write_reg(REG_TIPG, 10 | (10 << 10) | (10 << 20));

        crate::serial_println!(
            "[E1000] TX ring at phys 0x{:X}, {} descs",
            self.tx_descs_phys, NUM_TX_DESC
        );
    }

    fn link_up(&self) {
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | CTRL_SLU);
    }

    fn enable_interrupts(&self) {
        // Enable: LSC + RXO + RXDMT0 + RXT0
        self.write_reg(REG_IMS, 0x1F6DC);
        self.read_reg(REG_ICR); // Clear pending
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Packet Transmission
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Send a raw Ethernet frame.
    ///
    /// `data` must be a complete Ethernet frame (dst + src + ethertype + payload).
    /// The NIC appends the CRC automatically (IFCS flag).
    pub fn send(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > PACKET_BUFFER_SIZE {
            return Err("Packet too large for buffer");
        }
        if data.len() < 14 {
            return Err("Packet too small (need at least Ethernet header)");
        }

        unsafe {
            let desc = &mut *self.tx_descs_virt.add(self.tx_cur);

            // Wait for descriptor to become available
            let mut timeout = 100_000u32;
            while (ptr::read_volatile(&desc.status) & TDESC_STA_DD) == 0 {
                core::hint::spin_loop();
                timeout -= 1;
                if timeout == 0 {
                    return Err("TX descriptor timeout");
                }
            }

            // Copy packet data into TX buffer
            let buf = self.tx_buffers[self.tx_cur].virt;
            ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());

            // Configure descriptor
            desc.length = data.len() as u16;
            desc.cmd = TDESC_CMD_EOP | TDESC_CMD_IFCS | TDESC_CMD_RS;
            desc.status = 0;

            // Advance tail — tells the NIC "go send this!"
            let old_cur = self.tx_cur;
            self.tx_cur = (self.tx_cur + 1) % NUM_TX_DESC;
            self.write_reg(REG_TDT, self.tx_cur as u32);

            crate::serial_println!(
                "[E1000] TX: {} bytes on desc {}",
                data.len(), old_cur
            );
        }

        Ok(())
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Packet Reception
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Try to receive a packet.
    ///
    /// Returns `Some(Vec<u8>)` with raw Ethernet frame, or `None` if empty.
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        unsafe {
            let desc = &mut *self.rx_descs_virt.add(self.rx_cur);

            // Check DD (Descriptor Done) bit
            //let status = ptr::read_volatile(&desc.status);
            let status = ptr::read_volatile(ptr::addr_of!(desc.status)); // inorder to match.
            if (status & RDESC_STA_DD) == 0 {
                return None;
            }

            //let length = ptr::read_volatile(&desc.length) as usize;
            let length = ptr::read_volatile(ptr::addr_of!(desc.length)) as usize;
            if length == 0 || length > PACKET_BUFFER_SIZE {
                self.reset_rx_descriptor(self.rx_cur);
                return None;
            }

            // Copy packet data out of DMA buffer
            let buf = self.rx_buffers[self.rx_cur].virt;
            let mut packet = Vec::with_capacity(length);
            packet.set_len(length);
            ptr::copy_nonoverlapping(buf, packet.as_mut_ptr(), length);

            crate::serial_println!(
                "[E1000] RX: {} bytes on desc {}",
                length, self.rx_cur
            );

            // Reset descriptor for reuse
            self.reset_rx_descriptor(self.rx_cur);

            Some(packet)
        }
    }

    fn reset_rx_descriptor(&mut self, index: usize) {
        unsafe {
            let desc = &mut *self.rx_descs_virt.add(index);
            desc.status = 0;
            desc.length = 0;
            desc.errors = 0;
        }
        let old_cur = self.rx_cur;
        self.rx_cur = (self.rx_cur + 1) % NUM_RX_DESC;
        self.write_reg(REG_RDT, old_cur as u32);
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Interrupt Handling
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Handle an E1000 interrupt. Call from your IRQ 11 handler.
    pub fn handle_interrupt(&mut self) -> u32 {
        let cause = self.read_reg(REG_ICR);

        if cause & 0x04 != 0 {
            let status = self.read_reg(REG_STATUS);
            let link_up = (status & 0x02) != 0;
            crate::serial_println!(
                "[E1000] IRQ: Link {}",
                if link_up { "UP" } else { "DOWN" }
            );
        }

        cause
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Public Getters
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    pub fn mac(&self) -> [u8; 6] {
        self.mac_address
    }

    pub fn link_is_up(&self) -> bool {
        (self.read_reg(REG_STATUS) & 0x02) != 0
    }

    /// Print debug info about NIC state.
    pub fn debug_status(&self) {
        let status = self.read_reg(REG_STATUS);
        let ctrl = self.read_reg(REG_CTRL);
        let rdh = self.read_reg(REG_RDH);
        let rdt = self.read_reg(REG_RDT);
        let tdh = self.read_reg(REG_TDH);
        let tdt = self.read_reg(REG_TDT);

        crate::serial_println!("┌─── E1000 Debug ───────────────────┐");
        crate::serial_println!("│ STATUS:  0x{:08X}               │", status);
        crate::serial_println!("│ CTRL:    0x{:08X}               │", ctrl);
        crate::serial_println!("│ Link:    {}                      │",
            if status & 0x02 != 0 { "UP  " } else { "DOWN" });
        crate::serial_println!("│ RX Head: {:3}  Tail: {:3}           │", rdh, rdt);
        crate::serial_println!("│ TX Head: {:3}  Tail: {:3}           │", tdh, tdt);
        crate::serial_println!("│ RX Cur:  {:3}                      │", self.rx_cur);
        crate::serial_println!("│ TX Cur:  {:3}                      │", self.tx_cur);
        crate::serial_println!("└───────────────────────────────────┘");
    }
}