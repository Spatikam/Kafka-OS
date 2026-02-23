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
                    pixel[0] = ((r as u16 + g as u16 + b as u16) / 3) as u8;
                }
                other => panic!("Unknown pixel format: {:?}", other),
            }
        }
    }
    
    /// Saves a patch of the screen into a byte buffer, clipping at screen edges.
    pub fn save_patch(&self, x: usize, y: usize, width: usize, height: usize, dest: &mut [u8]) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;

        for row in 0..height {
            let screen_y = y + row;
            // Clip if it goes off the bottom of the screen
            if screen_y >= self.info.height { break; } 

            // Shrink the copied width if it goes off the right edge
            let draw_width = core::cmp::min(width, self.info.width.saturating_sub(x));
            if draw_width == 0 { continue; }

            let screen_start = (screen_y * stride + x) * bpp;
            let screen_end = screen_start + (draw_width * bpp);

            let dest_start = row * width * bpp;
            let dest_end = dest_start + (draw_width * bpp);

            dest[dest_start..dest_end].copy_from_slice(&self.framebuffer[screen_start..screen_end]);
        }
    }

    /// Restores a patch of the screen from a byte buffer, clipping at screen edges.
    pub fn restore_patch(&mut self, x: usize, y: usize, width: usize, height: usize, source: &[u8]) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;

        for row in 0..height {
            let screen_y = y + row;
            if screen_y >= self.info.height { break; }

            let draw_width = core::cmp::min(width, self.info.width.saturating_sub(x));
            if draw_width == 0 { continue; }

            let screen_start = (screen_y * stride + x) * bpp;
            let screen_end = screen_start + (draw_width * bpp);

            let source_start = row * width * bpp;
            let source_end = source_start + (draw_width * bpp);

            self.framebuffer[screen_start..screen_end].copy_from_slice(&source[source_start..source_end]);
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
