
///* SNK
///  PCI device configuration
///  I have referred these articles for the implementation
///  ref 001: https://wiki.osdev.org/PCI#Configuration_Space_Access_Mechanism_#1
///  */
use crate::{print, util::bit_manipulation::{GetBits, SetBits}};
use x86_64::{addr, instructions::port::Port};
extern crate alloc;
pub static mut TX_PHYS_ADDR: u32 = 0;
pub static mut TX_BUFFERS: [[u8; 1792]; 4] = [[0; 1792]; 4];
pub static mut TX_SLOT: usize = 0;
static mut RX_OFFSET: u16 = 0;  

pub struct Pci {
    config_port: Port<u32>,
    data_port: Port<u32>,
}

impl Pci {
    pub fn new() -> Self {
        Self {
            config_port: unsafe { Port::new(0xCF8) },
            data_port: unsafe { Port::new(0xCFC) },
        }
    }

    pub fn config_read(&mut self, bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
        let mut address: u32 = offset as u32;
        assert_eq!(offset & 0b11, 0, "PCI reads must be 4-byte aligned");

        address.set_bits(8, 3, func as u32);   // 3 bits for function
        address.set_bits(11, 5, slot as u32);  // 5 bits for slot
        address.set_bits(16, 8, bus as u32);   // 8 bits for bus
        address.set_bit(31, true);              // Enable bit

        unsafe {
            self.config_port.write(address);    // Write address to 0xCF8
            self.data_port.read()               // Read data from 0xCFC
        }
    }

    pub fn config_write(&mut self, bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
        let mut address: u32 = offset as u32;
        assert_eq!(offset & 0b11, 0, "PCI writes must be 4-byte aligned");

        address.set_bits(8, 3, func as u32);   // 3 bits for function
        address.set_bits(11, 5, slot as u32);  // 5 bits for slot
        address.set_bits(16, 8, bus as u32);   // 8 bits for bus
        address.set_bit(31, true);             // Enable bit

        unsafe {
            self.config_port.write(address);   // Tell PCI which register we want
            self.data_port.write(value);       // Write the data to it
        }
    }




}

#[derive(Debug)]
pub struct PciAddress {
    pub bus: u8,
    pub slot: u8,
}
#[derive(Debug)]
pub struct RtlDevice {
    pub bus: u8,
    pub slot: u8,
    pub io_base: u32,
}
impl PciAddress {
    pub fn new(bus: u8, slot: u8) -> Self {
        Self { bus, slot }
    }

    pub fn read_register(&self, pci: &mut Pci, register: u8) -> u32 {
    pci.config_read(self.bus, self.slot, 0, (register * 4))  
}
}
impl RtlDevice {
    pub fn find(pci: &mut Pci) -> Option<Self> {
        // Scan all slots on bus 0 for RTL8139
        for slot in 0..32 {
            let device = PciAddress::new(0, slot);
            let vendor_device = device.read_register(pci, 0);
            
            let vendor_id = vendor_device & 0xFFFF;
            let device_id = (vendor_device >> 16) & 0xFFFF;
            
            // Check if this is RTL8139 (Vendor: 0x10EC, Device: 0x8139)
            if vendor_id == 0x10EC && device_id == 0x8139 {
                // Read BAR0 to get I/O base address
                let bar0 = device.read_register(pci, 4);
                let io_base = bar0 & 0xFFFFFFFC;  // Mask out lower bits
                
                return Some(RtlDevice {
                    bus: 0,
                    slot,
                    io_base,
                });
            }
        }
        None
    }
}
#[derive(Debug)]
pub struct BarInfo {
    pub bar_number: u8,
    pub address: u32,
    pub is_io: bool,
}

impl RtlDevice {
    pub fn read_bars(&self, pci: &mut Pci) -> alloc::vec::Vec<BarInfo> {
        use alloc::vec::Vec;
        
        let device = PciAddress::new(self.bus, self.slot);
        let mut bars = Vec::new();
        
        for i in 0..6 {
            let base_address = device.read_register(pci, 4 + i);
            
            if base_address != 0 {
                let addr = base_address & 0xFFFFFFF0;
                let is_io = base_address & 0x1 == 1;
                
                bars.push(BarInfo {
                    bar_number: i as u8,
                    address: addr,
                    is_io,
                });
            }
        }
        
        bars
    }
}

impl RtlDevice {
    pub fn read_mac_address(&self) -> [u8; 6] {
        use x86_64::instructions::port::Port;
        
        let mut mac = [0u8; 6];
        
        unsafe {
            // Read 6 bytes from I/O base (MAC is at offset 0x00-0x05)
            for i in 0..6 {
                let mut port = Port::<u8>::new(self.io_base as u16 + i);
                mac[i as usize] = port.read();
            }
        }
        
        mac
    }
}

impl RtlDevice {
    pub fn enable_bus_mastering(&self, pci: &mut Pci) {
        let device = PciAddress::new(self.bus, self.slot);
        let mut command = device.read_register(pci, 1); 
        
        command |= (1 << 2); 
        
        // You'll need a config_write method in your Pci struct:
        pci.config_write(self.bus, self.slot, 0, 0x04, command);
    }
}
//  ref :: http://wiki.osdev.org/RTL8139#Registers
// https://tungdam.medium.com/linux-network-ring-buffers-cea7ead0b8e8

///**
///  so we need to enble the pci bus mastering for this device 
///  this allows nic to perform dma
////*/

//outportb(ioaddr + 0x52, 0x0); it write  a byte to an I/O
// port


//Send 0x00 to the CONFIG_1 register (0x52) to set the LWAKE + LWPTN to active high. this should essentially *power on* the device.
pub fn power_on_device(io_addr:u16){
    let port_addr = io_addr+0x52;
    let mut config_port = Port::<u8>::new(port_addr);
    unsafe {
        config_port.write(0x00);
    }
}

/// Software resettt!

///*
/// Next, we should do a software reset to clear the RX and TX buffers and set everything back to defaults. 
/// Do this to eliminate the possibility of there still being garbage left in the buffers or registers on power on.
///  */
/// 
/// ref: https://wiki.osdev.org/RTL8139#PCI_Bus_Mastering

pub fn reset_rtl8139(io_addr:u16){
    let cmd_reg_addr=io_addr+0x37;
    let mut cmd_port = Port::<u8>::new(cmd_reg_addr);
    unsafe {
        // send the reset
        cmd_port.write(0x10);

        // so we use while loop  pool RST bit 0x10 untill it becomesss 0
        while (cmd_port.read()&0x10)!=0 {
            core::hint::spin_loop();
        }
    }
   crate::println!("RTL8139  reset done")
}


pub fn init_recive_buffer(io_addr:u16,rx_buffer_ptr:*const u8){
  let rbstart_addr = io_addr+0x30;
  let mut rbstart_port =Port::<u32>::new(rbstart_addr);

  unsafe {
    let phy_addr= rx_buffer_ptr as u32;
    rbstart_port.write(phy_addr);
  }
}

pub fn set_imr_isr(io_addr:u16){
    let imr_addr = io_addr+0x3C;
    let mut imr_port =Port::<u16>::new(imr_addr);

    unsafe{
        imr_port.write(0x0005);
    }

}

pub fn init_rcr(io_addr:u16){
    let rcr_reg_addr=io_addr+0x44;
    let mut  rcr_port = Port::<u32>::new(rcr_reg_addr);
     unsafe {
        let config_bits :u32= 0xF|(1<<7);
        rcr_port.write(config_bits);
     }
}

pub fn enable_reciver(io_addr:u16){
    let cmd_reg_addr = io_addr+0x37;
    let mut  cmd_port=Port::<u8>::new(cmd_reg_addr);
   
unsafe {
    let command:u8 =0x0C;
    cmd_port.write(command);
}
print!("RTL8139 is now liveee!")
}
// In a real driver, you'd store current_offset in your RtlDevice struct
static mut CURRENT_OFFSET: u16 = 0;

pub fn receive_packet(io_addr: u16, rx_buffer: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    unsafe {
        let offset = RX_OFFSET as usize;
        
        let status = (rx_buffer[offset] as u16) | ((rx_buffer[offset + 1] as u16) << 8);
        let length = (rx_buffer[offset + 2] as u16) | ((rx_buffer[offset + 3] as u16) << 8);

        if length == 0xFFF0 || length < 4 { return None; }

        let packet_len = (length - 4) as usize; // strip CRC
        let data_start = offset + 4;

        let mut packet = alloc::vec::Vec::with_capacity(packet_len);
        packet.extend_from_slice(&rx_buffer[data_start..data_start + packet_len]);

        // Advance ring buffer offset, 4-byte aligned, wrapping at 8192
        RX_OFFSET = ((RX_OFFSET + length + 4 + 3) & !3) % 8192;

        // Tell NIC we consumed the data
        let mut capr_port = Port::<u16>::new(io_addr + 0x38);
        capr_port.write(RX_OFFSET.wrapping_sub(0x10));

        Some(packet)
    }
}

//// ref :https://wiki.osdev.org/RTL8139#ISR_Handler

const ROK:u16=0x01;
const TOK:u16=0x04;

pub fn rtl8139_handler(io_addr: u16, rx_buffer: &[u8]) {  // ← add rx_buffer param
    let mut isr_port = Port::<u16>::new(io_addr + 0x3E);
    unsafe {
        let status = isr_port.read();
        isr_port.write(ROK | TOK);

        if (status & TOK) != 0 {
            crate::print!("transmit Ok");
        }
        if (status & ROK) != 0 {
            receive_packet(io_addr, rx_buffer);  // ← pass it through
        }
    }
}

// ref :https://wiki.osdev.org/RTL8139#Transmitting_Packets

///*
/// 
/// The transmit start registers are each 32 bits long, and are in I/O offsets 
/// 0x20, 0x24, 0x28 and 0x2C.
/// 
/// The transmit status/command registers are also each 32 bits long and are
///  in I/O offsets 0x10, 0x14, 0x18 and 0x1C. 
///  */


pub fn transmit_packet(io_addr: u16, data: &[u8]) {
    unsafe {
        let slot = TX_SLOT % 4;
        let tsad = [0x20u16, 0x24, 0x28, 0x2C];
        let tsd  = [0x10u16, 0x14, 0x18, 0x1C];

        TX_BUFFERS[slot][..data.len()].copy_from_slice(data);

        // ✅ Use the pre-computed physical address
        let phys_addr = TX_PHYS_ADDR + (slot as u32 * 1792);

        let mut addr_port = Port::<u32>::new(io_addr + tsad[slot]);
        addr_port.write(phys_addr);

        let mut status_port = Port::<u32>::new(io_addr + tsd[slot]);
        status_port.write(data.len() as u32 & 0x1FFF);

        loop {
            if status_port.read() & (1 << 15) != 0 { break; }
            core::hint::spin_loop();
        }

        TX_SLOT += 1;
    }
}

//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------
//------------------------------------------------------------------------------------------------------------------------------------------------------


