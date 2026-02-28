use alloc::string::String;
use alloc::vec::Vec;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii::FONT_8X13, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use core::convert::Infallible;
use crate::gui::graphics::UIEvent; // Adjust imports as needed
use alloc::format;
use crate::gui::rtc::{RTC,DateTime};


pub struct Taskbar {
    pub width: u32,
    pub height: u32,
    pub bpp: usize,
    pub buffer: Vec<u8>,
    pub event_queue: Vec<UIEvent>,
    pub last_minute: u8, // NEW: Remember the last drawn minute
}

impl Taskbar {
    pub fn new(screen_width: u32, bpp: usize) -> Self {
        let height = 30; // 30 pixels tall
        Self {
            width: screen_width,
            height,
            bpp,
            buffer: alloc::vec![0; (screen_width * height * bpp as u32) as usize],
            event_queue: Vec::new(),
            last_minute: 60, // impossible minute to force update on boot
        }
    }
    
    pub fn send_event(&mut self, event: UIEvent) {
        self.event_queue.push(event);
    }

    pub fn tick(&mut self) {
        let mut rtc = RTC::new();
        let mut time = rtc.read_datetime();

        // --- Apply Indian Standard Time (GMT +5:30) ---
        time.apply_timezone_offset(5, 30);

        // ONLY render if the minute has actually changed!
        if time.minute != self.last_minute {
            self.last_minute = time.minute;
            self.render_internal_graphics(&time); 
        }
    }
}

// --- boilerplate DrawTarget ---
impl OriginDimensions for Taskbar {
    fn size(&self) -> Size { Size::new(self.width, self.height) }
}

impl DrawTarget for Taskbar {
    type Color = Rgb888;
    type Error = Infallible; 

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where I: IntoIterator<Item = Pixel<Self::Color>> {
        let bounds = self.bounding_box();
        for Pixel(coord, color) in pixels.into_iter() {
            if bounds.contains(coord) {
                let x = coord.x as u32;
                let y = coord.y as u32;
                let index = ((y * self.width + x) * self.bpp as u32) as usize; 

                if index + 2 < self.buffer.len() {
                    self.buffer[index] = color.b();
                    self.buffer[index + 1] = color.g();
                    self.buffer[index + 2] = color.r();
                    if self.bpp == 4 { self.buffer[index + 3] = 0; }
                }
            }
        }
        Ok(())
    }
}

impl Taskbar {
    pub fn render_internal_graphics(&mut self, time: &DateTime) {

        // --- Fetch the Real Hardware Time ---
        let mut rtc = RTC::new();
        //let time = rtc.read_datetime();
        //let time_string = format!("{:02}:{:02}", time.hour, time.minute);
        let weekday = get_weekday(time.year, time.month, time.day);
        let month_str = get_month_name(time.month);
        
        let display_str = format!(
            "{}, {:02} {} {} {:02}:{:02}", 
            weekday, time.day, month_str, time.year, time.hour, time.minute
        );

        // 1. Base Background (Dark Gray)
        Rectangle::new(Point::zero(), self.size())
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(30, 30, 30)))
            .draw(self).unwrap();

        let text_style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);

        // 2. Apps Menu (Left)
        Text::new("KafkaOS", Point::new(10, 20), text_style)
            .draw(self).unwrap();

        // 3. Time & Date (Center)
        let text_width = display_str.len() as u32 * 8;
        let center_x = (self.width / 2) - (text_width / 2);
        Text::new(&display_str, Point::new(center_x as i32, 20), text_style)
            .draw(self).unwrap();

        // 4. Power Button (Right)
        let power_btn_x = (self.width - 70) as i32;
        Rectangle::new(Point::new(power_btn_x, 0), Size::new(70, 30))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(self).unwrap();
            
        Text::new("Power", Point::new(power_btn_x + 15, 20), text_style)
            .draw(self).unwrap();

        // Report damage to the Compositor
        super::graphics::report_damage(crate::gui::geometry::Rect::new(
            0, 0, self.width as i32, self.height as i32
        ));
    }
}

// Sakamoto's algorithm to calculate the day of the week
fn get_weekday(year: u16, month: u8, day: u8) -> &'static str {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    if month < 3 {
        y -= 1;
    }
    let dow = (y + y / 4 - y / 100 + y / 400 + t[(month - 1) as usize] as u16 + day as u16) % 7;
    
    match dow {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        _ => "Unknown",
    }
}

fn get_month_name(month: u8) -> &'static str {
    match month {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "Unk",
    }
}