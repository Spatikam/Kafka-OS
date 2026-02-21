// src/task/mouse.rs
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::stream::Stream;
use futures_util::task::AtomicWaker;
use futures_util::StreamExt;
use core::pin::Pin;
use core::task::{Context, Poll};
use x86_64::instructions::port::Port;

// A queue to store mouse events so the GUI can read them later
static MOUSE_QUEUE: OnceCell<ArrayQueue<MousePacket>> = OnceCell::uninit();
static MOUSE_WAKER: AtomicWaker = AtomicWaker::new();

#[derive(Debug, Clone, Copy)]
pub struct MousePacket {
    pub x: i16,      // I guess  i will assign it as left.
    pub y: i16,      // scroll up and down.
    pub left_btn: bool,
    pub right_btn: bool,
}

// implementation of ps2
pub fn init() {
    let mut data_port = Port::<u8>::new(0x60);
    let mut status_port = Port::<u8>::new(0x64);
    let mut command_port = Port::<u8>::new(0x64);

    unsafe {
        command_port.write(0xA8);

        // i guess enabling  the inerruots  for Mouse
        command_port.write(0x20); // Read Config Byte
        let mut status = data_port.read();
        status |= 2; // Set bit 1 (Enable Mouse IRQ 12)
        command_port.write(0x60); 
        data_port.write(status);
        command_port.write(0xD4);
        data_port.write(0xF4); // 0xF4 = "Enable Data Reporting"
        
        // let it read here 
        let _ack = data_port.read();
    }
    
    //  the event queue (Size 100) for now here size set as 100
    MOUSE_QUEUE.try_init_once(|| ArrayQueue::new(100)).expect("MouseQueue init failed");
}

// updated the thing to i16Interrupt Handler (IRQ 12)
pub fn add_packet_from_interrupt(packet_byte: u8) {
    static mut BUFFER: [u8; 3] = [0; 3];
    static mut INDEX: usize = 0;

    unsafe {
        // Guard against out-of-sync packets
        if INDEX == 0 && (packet_byte == 0xFA || (packet_byte & 0x08) == 0) {
            return; 
        }
        BUFFER[INDEX] = packet_byte;
        INDEX += 1;
        if INDEX == 3 {
            let flags = BUFFER[0];
            let mut x = BUFFER[1] as i16;
            let mut y = BUFFER[2] as i16; 
            // Bit 4 (0x10) is X sign, Bit 5 (0x20) is Y sign
            if (flags & 0x10) != 0 {
                x -= 256; 
            }
            if (flags & 0x20) != 0 {
                y -= 256; 
            }
            y = -y;
            let packet = MousePacket {
                x,
                y, 
                left_btn: (flags & 0b0000_0001) != 0,
                right_btn: (flags & 0b0000_0010) != 0,
            };
            if let Some(queue) = MOUSE_QUEUE.get() {
                if queue.push(packet).is_ok() {
                    MOUSE_WAKER.wake(); 
                }
            }
            INDEX = 0; // Reset for next packet
        }
    }
}

/// Non-blocking: returns a packet if one is queued, otherwise None.
/// Use this from synchronous (non-async) contexts like the GUI draw loop.
pub fn try_get_packet() -> Option<MousePacket> {
    MOUSE_QUEUE.get().and_then(|q| q.pop())
}

pub struct MouseStream {
    _private: (),
}

impl MouseStream {
    pub fn new() -> Self {
        init(); // Auto-init hardware when stream is created
        Self { _private: () }
    }
}

impl Stream for MouseStream {
    type Item = MousePacket;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<MousePacket>> {
        let queue = MOUSE_QUEUE.get().expect("Mouse queue not init");

        if let Some(packet) = queue.pop() {
            return Poll::Ready(Some(packet));
        }

        MOUSE_WAKER.register(cx.waker());
        match queue.pop() {
            Some(packet) => {
                MOUSE_WAKER.take();
                Poll::Ready(Some(packet))
            }
            None => Poll::Pending,
        }
    }
}