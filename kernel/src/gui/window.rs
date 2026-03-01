use alloc::string::String;
use alloc::vec::Vec;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    geometry::Size,
    mono_font::{ascii::FONT_8X13, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};

use core::convert::Infallible;

pub struct Window {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub color: Rgb888,
    pub bpp: usize,
    pub buffer: Vec<u8>, // The private memory canvas (4 bytes per pixel for ARGB/XRGB)
}

impl Window {
    pub fn new(x: i32, y: i32, width: u32, height: u32, title: &str, color: Rgb888, bpp: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
            title: String::from(title),
            color,
            bpp,
            buffer: alloc::vec![0; (width * height * bpp as u32) as usize], // Allocate the exact amount of RAM needed for this specific window
        }
    }

    /*pub fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb888>,
    {
        // Draw Window Body
        Rectangle::new(Point::new(self.x, self.y), Size::new(self.width, self.height))
            .into_styled(PrimitiveStyle::with_fill(self.color))
            .draw(target)?;

        // Draw Title Bar
        let title_bar_height = 20;
        Rectangle::new(Point::new(self.x, self.y), Size::new(self.width, title_bar_height))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(50, 50, 50)))
            .draw(target)?;

        // Draw Title Text
        let style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        Text::new(&self.title, Point::new(self.x + 5, self.y + 15), style)
            .draw(target)?;

        // Draw Border
        Rectangle::new(Point::new(self.x, self.y), Size::new(self.width, self.height))
            .into_styled(PrimitiveStyle::with_stroke(Rgb888::BLACK, 1))
            .draw(target)?;

        Ok(())
    }*/

    /// Draws the initial window frame, title bar, and background into RAM
    pub fn render_internal_graphics(&mut self) {
        // Draw directly to SELF!
        // Draw Window Body
        Rectangle::new(Point::zero(), self.size())
            .into_styled(PrimitiveStyle::with_fill(self.color))
            .draw(self)
            .unwrap();

        // Draw Title Bar
        let title_bar_height = 20;
        Rectangle::new(Point::zero(), Size::new(self.width, title_bar_height))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(50, 50, 50)))
            .draw(self)
            .unwrap();

        // Draw Title Text
        let title_text = self.title.clone();
        let style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        Text::new(&title_text, Point::new(5, 15), style)
            .draw(self)
            .unwrap();

        // Draw Border
        Rectangle::new(Point::zero(), Size::new(self.width, self.height))
            .into_styled(PrimitiveStyle::with_stroke(Rgb888::BLACK, 1))
            .draw(self)
            .unwrap();

        // Tell the Compositor that this window's area on the physical screen is damaged
        super::graphics::report_damage(crate::gui::geometry::Rect::new(
            self.x, self.y, self.width as i32, self.height as i32
        ));
    }
}

impl OriginDimensions for Window {
    fn size(&self) -> Size {
        // We return the window's internal width and height
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for Window {
    type Color = Rgb888;
    // Infallible means writing to RAM can never "fail" the way writing to a disk might
    type Error = Infallible; 

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let bounds = self.bounding_box();

        for Pixel(coord, color) in pixels.into_iter() {
            // 1. Discard any pixels that fall outside the window's boundaries
            if bounds.contains(coord) {
                let x = coord.x as u32;
                let y = coord.y as u32;
                
                // 2. Calculate the flat array index (3 bytes per pixel)
                // Notice we use self.width, NOT the screen's stride!
                let index = ((y * self.width + x) * self.bpp as u32) as usize;   // logger says its 3 reverted it to 3.

                // 3. Write the color channels to the Vec buffer safely
                if index + 2 < self.buffer.len() {
                    self.buffer[index] = color.b();
                    self.buffer[index + 1] = color.g();
                    self.buffer[index + 2] = color.r();
                    if self.bpp == 4 {
                        self.buffer[index + 3] = 0; // Padding/Alpha byte
                    }
                }
            }
        }
        Ok(())
    }
} 

pub struct WindowManager {
    pub windows: Vec<Window>,
    screen_width: u32,
    screen_height: u32,
}

impl WindowManager {
    pub fn new(screen_width: u32, screen_height: u32) -> Self {
        Self {
            windows: Vec::new(),
            screen_width,
            screen_height,
        }
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows.push(window);
    }

    /*pub fn draw_windows<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb888>,
    {
        // Draw background
        target.clear(Rgb888::new(0, 128, 128))?; // Teal desktop

        // Draw all windows
        for window in &self.windows {
            window.draw(target)?;
        }
        Ok(())
    }*/
}

