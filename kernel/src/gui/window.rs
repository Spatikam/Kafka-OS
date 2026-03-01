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

use crate::gui::graphics::{UIEvent, RawMouse};
use crate::{exit_qemu,QemuExitCode};

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
    pub event_queue: Vec<UIEvent>, // Event Queue for event handling
    pub close_btn: bool, // Close Button Implementation
    pub is_dragging: bool, // is Window being dragged Windows
    pub drag_x: i32,
    pub drag_y: i32,
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
            event_queue: Vec::new(),
            close_btn: false,
            is_dragging: false,
            drag_x: x,
            drag_y: y,
        }
    }

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

        // Drawing the Close Button
        Rectangle::new(Point::new(self.width as i32 - 20, 0), Size::new(20, 20))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
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

    pub fn send_event(&mut self, event: UIEvent) {
        self.event_queue.push(event);
    }

    // Processing the Interactive UI Elements of the Window
    pub fn process_events(&mut self) {
        // Drain clears the inbox so we don't process the same click twice
        for event in self.event_queue.drain(..) {
            match event {
                UIEvent::MouseClick { x, y, button } => {
                    match button {
                        RawMouse::Left (raw_x, raw_y) => {
                            //crate::println!("Window '{}' clicked at local {}, {}", self.title, x, y);
                            // --- THE POWER MENU HIT-TESTING ---
                            if self.title == "Power" {
                                // Did they click inside the content area (below the title bar)?
                                if y > 20 {
                                    if y >= 25 && y < 50 {
                                        crate::println!("Sleep selected");
                                        // self.should_close = true; // Close the menu when clicked
                                    } else if y >= 50 && y < 75 {
                                        crate::println!("Restart selected");
                                    } else if y >= 75 && y <= 100 {
                                        crate::println!("Shutdown selected! Goodbye.");
                                        exit_qemu(QemuExitCode::Success);
                                    }
                                }
                            }
                            if self.width as i32 - 20 <= x && x <= self.width as i32 - 2 && 1 <= y && y <= 19 {
                                self.close_btn = true;
                            } else if 0 <= x && x <= self.width as i32 - 21 && 0 <= y && y <= 20 {
                                self.is_dragging = true;
                                self.drag_x = x;
                                self.drag_y = y;
                            }
                        },
                        _ => {}
                    }
                },
                _ => {} // Ignore other events for now
            }
        }
    }

    pub fn render_power_menu(&mut self) {
        // 1. Draw the standard background and border
        self.render_internal_graphics();

        // 2. Draw the 3 Menu Options
        let style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        
        // Sleep (y = 40)
        Text::new("Sleep", Point::new(10, 40), style).draw(self).unwrap();
        // Restart (y = 65)
        Text::new("Restart", Point::new(10, 65), style).draw(self).unwrap();
        // Shutdown (y = 90)
        Text::new("Shutdown", Point::new(10, 90), style).draw(self).unwrap();
        
        // Note: render_internal_graphics already reported the damage, 
        // so the screen will perfectly update when this spawns.
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
}
