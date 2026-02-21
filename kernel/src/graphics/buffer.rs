use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Size,
    pixelcolor::{Rgb888, RgbColor},
    prelude::*,
};

/// A display driver that wraps the raw framebuffer.
pub struct FrameBufferDisplay {
    framebuffer: &'static mut [u8],
    info: FrameBufferInfo,
}

impl FrameBufferDisplay {
    /// Creates a new display driver from the bootloader's framebuffer.
    /// Creates a new display driver from the raw framebuffer slice and info.
    pub fn new(buffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        
        Self {
            framebuffer: buffer,
            info,
        }
    }

    /// Clears the screen with a specific color.
    pub fn clear(&mut self, color: Rgb888) {
        for pixel in self.framebuffer.chunks_exact_mut(self.info.bytes_per_pixel) {
            let (r, g, b) = (color.r(), color.g(), color.b());
            
            match self.info.pixel_format {
                PixelFormat::Rgb => {
                    pixel[0] = r;
                    pixel[1] = g;
                    pixel[2] = b;
                }
                PixelFormat::Bgr => {
                    pixel[0] = b;
                    pixel[1] = g;
                    pixel[2] = r;
                }
                PixelFormat::U8 => {
                    // Grayscale fallback
                    pixel[0] = (r as u16 + g as u16 + b as u16 / 3) as u8;
                }
                other => panic!("Unknown pixel format: {:?}", other),
            }
        }
    }
}

impl OriginDimensions for FrameBufferDisplay {
    fn size(&self) -> Size {
        Size::new(self.info.width as u32, self.info.height as u32)
    }
}

impl DrawTarget for FrameBufferDisplay {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels.into_iter() {
            // Check if point is within bounds
            if point.x >= 0 && point.x < self.info.width as i32 &&
               point.y >= 0 && point.y < self.info.height as i32 
            {
                // Calculate byte offset
                let pixel_offset = (point.y as usize * self.info.stride) + point.x as usize;
                let byte_offset = pixel_offset * self.info.bytes_per_pixel;

                // Write color
                let pixel_buffer = &mut self.framebuffer[byte_offset..];
                let (r, g, b) = (color.r(), color.g(), color.b());

                match self.info.pixel_format {
                    PixelFormat::Rgb => {
                        pixel_buffer[0] = r;
                        pixel_buffer[1] = g;
                        pixel_buffer[2] = b;
                    }
                    PixelFormat::Bgr => {
                        pixel_buffer[0] = b;
                        pixel_buffer[1] = g;
                        pixel_buffer[2] = r;
                    }
                    PixelFormat::U8 => {
                        pixel_buffer[0] = ((r as u16 + g as u16 + b as u16) / 3) as u8;
                    }
                     _ => {} // Ignore unknown formats for now
                }
            }
        }
        Ok(())
    }
}
