use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Size,
    pixelcolor::{Rgb888, RgbColor},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use super::geometry::Rect;
use super::window::Window;

/// A display driver that wraps the raw framebuffer.
pub struct FrameBufferDisplay {
    framebuffer: &'static mut [u8],
    pub info: FrameBufferInfo,
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
    /*pub fn clear(&mut self, color: Rgb888) {
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
    }*/

    /// Copies a specific intersecting rectangle from a window's private 
    /// buffer directly to the physical screen.
    pub fn blit_partial(&mut self, overlap: &Rect, window: &Window) {
        let bpp = self.info.bytes_per_pixel;
        let stride = self.info.stride;

        for row_offset in 0..overlap.height {
            // 1. Calculate the absolute Y coordinate on the physical screen
            let screen_y = (overlap.y + row_offset) as usize;
            
            // 2. Calculate the local Y coordinate inside the window's private buffer
            // (Where does this overlap start relative to the window's top-left corner?)
            let win_local_y = ((overlap.y + row_offset) - window.y) as usize;

            // 3. Calculate X coordinates and the total bytes to copy per row
            let screen_x = overlap.x as usize;
            let win_local_x = (overlap.x - window.x) as usize;
            let copy_width_bytes = (overlap.width as usize) * bpp;

            // 4. Calculate starting and ending array indices for the physical screen
            let screen_start = (screen_y * stride + screen_x) * bpp;
            let screen_end = screen_start + copy_width_bytes;

            // 5. Calculate starting and ending array indices for the window's buffer
            // (Windows don't have a stride, they just use their own width)
            let win_start = (win_local_y * (window.width as usize) + win_local_x) * bpp;
            let win_end = win_start + copy_width_bytes;

            // 6. Safely copy the slice row-by-row
            if screen_end <= self.framebuffer.len() && win_end <= window.buffer.len() {
                self.framebuffer[screen_start..screen_end]
                    .copy_from_slice(&window.buffer[win_start..win_end]);
            }
        }
    }

    /// A helper function to fill a damaged Rect with a solid background color
    pub fn fill_rect(&mut self, rect: &Rect, color: embedded_graphics::pixelcolor::Rgb888) {
        use embedded_graphics::prelude::*;
        use embedded_graphics::primitives::{Rectangle, PrimitiveStyle};
        
        let point = embedded_graphics::geometry::Point::new(rect.x, rect.y);
        let size = embedded_graphics::geometry::Size::new(rect.width as u32, rect.height as u32);
        
        // We can just use embedded-graphics for the solid background fill
        let _ = Rectangle::new(point, size)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(self);
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
