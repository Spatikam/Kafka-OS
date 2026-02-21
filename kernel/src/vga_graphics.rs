use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb888, RgbColor, Bgr888},
    prelude::*,
    primitives::{Rectangle,PrimitiveStyle},
};
use x86_64::{PhysAddr, VirtAddr};
use core::convert::TryInto;

const VGA_WIDTH: usize = 320;
const VGA_HEIGHT: usize = 200;
const VGA_BUFFER_ADDR: u64 = 0xA0000;

/// A simple color structure for VGA Mode 13h (which uses a 256-color palette).
/// For simplicity, we'll map common colors to the default VGA palette indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VgaColor(pub u8);

impl PixelColor for VgaColor {
    type Raw = ();
}

impl VgaColor {
    pub const BLACK: Self = VgaColor(0);
    pub const BLUE: Self = VgaColor(1);
    pub const GREEN: Self = VgaColor(2);
    pub const CYAN: Self = VgaColor(3);
    pub const RED: Self = VgaColor(4);
    pub const MAGENTA: Self = VgaColor(5);
    pub const BROWN: Self = VgaColor(6);
    pub const LIGHT_GRAY: Self = VgaColor(7);
    pub const DARK_GRAY: Self = VgaColor(8);
    pub const LIGHT_BLUE: Self = VgaColor(9);
    pub const LIGHT_GREEN: Self = VgaColor(10);
    pub const LIGHT_CYAN: Self = VgaColor(11);
    pub const LIGHT_RED: Self = VgaColor(12);
    pub const LIGHT_MAGENTA: Self = VgaColor(13);
    pub const YELLOW: Self = VgaColor(14);
    pub const WHITE: Self = VgaColor(15);
    
    // Helper to approximate RGB to closest VGA color (very basic)
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        // This is a naive mapping. A real implementation would use a best-fit algorithm against the palette.
        // For now, we just threshold to the 16 standard colors.
        let r_bit = if r > 128 { 4 } else { 0 };
        let g_bit = if g > 128 { 2 } else { 0 };
        let b_bit = if b > 128 { 1 } else { 0 };
        let bright_bit = if r > 200 || g > 200 || b > 200 { 8 } else { 0 };
        
        VgaColor(r_bit | g_bit | b_bit | bright_bit)
    }
}

impl From<Rgb888> for VgaColor {
    fn from(c: Rgb888) -> Self {
        Self::from_rgb(c.r(), c.g(), c.b())
    }
}

pub struct VgaGraphics {
    buffer: &'static mut [u8],
}

impl VgaGraphics {
    /// Creates a new VgaGraphics instance.
    /// 
    /// # Safety
    /// This function is unsafe because it creates a mutable slice to physical memory.
    /// The caller must ensure that `physical_memory_offset` is correct and that
    /// no other references to this memory exist.
    pub unsafe fn new(physical_memory_offset: VirtAddr) -> Self {
        let virt_addr = physical_memory_offset + VGA_BUFFER_ADDR;
        let ptr = virt_addr.as_mut_ptr::<u8>();
        let buffer = core::slice::from_raw_parts_mut(ptr, VGA_WIDTH * VGA_HEIGHT);
        
        Self { buffer }
    }

    pub fn clear(&mut self, color: VgaColor) {
        for byte in self.buffer.iter_mut() {
            *byte = color.0;
        }
    }

    pub fn draw_pixel(&mut self, x: usize, y: usize, color: VgaColor) {
        if x < VGA_WIDTH && y < VGA_HEIGHT {
            let offset = y * VGA_WIDTH + x;
            self.buffer[offset] = color.0;
        }
    }

    pub fn read_pixel(&self, x: usize, y: usize) -> VgaColor {
        if x < VGA_WIDTH && y < VGA_HEIGHT {
            let offset = y * VGA_WIDTH + x;
            VgaColor(self.buffer[offset])
        } else {
            VgaColor::BLACK
        }
    }
}

impl OriginDimensions for VgaGraphics {
    fn size(&self) -> Size {
        Size::new(VGA_WIDTH as u32, VGA_HEIGHT as u32)
    }
}

impl DrawTarget for VgaGraphics {
    type Color = VgaColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            let x = coord.x as usize;
            let y = coord.y as usize;
            self.draw_pixel(x, y, color);
        }
        Ok(())
    }
}

use spin::Mutex;
use conquer_once::spin::OnceCell;

pub static WRITER: OnceCell<Mutex<VgaGraphics>> = OnceCell::uninit();

pub fn init_graphics(physical_memory_offset: VirtAddr) {
    let graphics = unsafe { VgaGraphics::new(physical_memory_offset) };
    WRITER.init_once(|| Mutex::new(graphics));
}

use crate::task::mouse::MouseStream; 
use futures_util::stream::StreamExt;
pub async fn run_gui(mut graphics: VgaGraphics) {
    let mut mouse_stream = MouseStream::new(); 
    let mut cursor_x: i32 = 160;
    let mut cursor_y: i32 = 100;
    const SENSITIVITY: i32 = 2; 
    graphics.clear(VgaColor::BLUE);   // the screen would be blue
    Rectangle::new(Point::new(20, 20), Size::new(80, 80)).into_styled(PrimitiveStyle::with_fill(VgaColor::RED)).draw(&mut graphics).unwrap(); // a square i guess
    let mut cursor_bg = [[VgaColor::BLUE; 3]; 3];   // pixels 
    // here basically inorder to avoid the pixel eating.
    for dy in 0..3 {
        for dx in 0..3 {
            cursor_bg[dy][dx] = graphics.read_pixel((cursor_x + dx as i32) as usize, (cursor_y + dy as i32) as usize);
        }
    }
    while let Some(packet) = mouse_stream.next().await {
        for dy in 0..3 {
            for dx in 0..3 {
                graphics.draw_pixel((cursor_x + dx as i32) as usize, (cursor_y + dy as i32) as usize, cursor_bg[dy][dx]);
            }
        }
        cursor_x += (packet.x as i32) / SENSITIVITY;
        cursor_y += (packet.y as i32) / SENSITIVITY; 
        cursor_x = cursor_x.clamp(0, 319 - 3);  // setting it as  3x3 pixel so yeah
        cursor_y = cursor_y.clamp(0, 199 - 3);
        for dy in 0..3 {
            for dx in 0..3 {
                cursor_bg[dy][dx] = graphics.read_pixel((cursor_x + dx as i32) as usize, (cursor_y + dy as i32) as usize);
            }
        }
        Rectangle::new(Point::new(cursor_x, cursor_y), Size::new(3, 3)).into_styled(PrimitiveStyle::with_fill(VgaColor::WHITE)).draw(&mut graphics).unwrap();
    }
}