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

pub struct Window {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub title: String,
    // Content data could go here, for now just a background color
    pub color: Rgb888,
}

impl Window {
    pub fn new(x: i32, y: i32, width: u32, height: u32, title: &str, color: Rgb888) -> Self {
        Self {
            x,
            y,
            width,
            height,
            title: String::from(title),
            color,
        }
    }

    pub fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
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
    }
}

pub struct WindowManager {
    windows: Vec<Window>,
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

    pub fn draw_windows<D>(&self, target: &mut D) -> Result<(), D::Error>
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
    }
}
