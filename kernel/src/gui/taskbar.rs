use super::graphics::RawMouse;
use crate::gui::graphics::UIEvent; // Adjust imports as needed
use crate::gui::rtc::{DateTime, RTC};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::Infallible;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{MonoTextStyle, ascii::FONT_8X13},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Arc, Line, PrimitiveStyle, Rectangle},
    text::Text,
};

pub struct Taskbar {
    pub width: u32,
    pub height: u32,
    pub bpp: usize,
    pub buffer: Vec<u8>,
    pub event_queue: Vec<UIEvent>,
    pub last_minute: u8, // NEW: Remember the last drawn minute
    pub minimized_labels: Vec<(String, usize)>, // (title, window index)
}

pub enum TaskbarAction {
    None,
    OpenPowerMenu,
    OpenAppMenu,
    RestoreWindow(usize),
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
            minimized_labels: Vec::new(),
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
            let crappy_bug_man = self.minimized_labels.clone();
            self.draw_minimized_windows(&crappy_bug_man);
        }
    }

    /// Reads the Taskbar's inbox and returns an action for the Compositor
    pub fn process_events(&mut self) -> TaskbarAction {
        let mut action = TaskbarAction::None;

        for event in self.event_queue.drain(..) {
            if let UIEvent::MouseClick { x, y, button } = event {
                if let RawMouse::Left(..) = button {
                    // MATH CHECK: Did they click the right-side Power Button?
                    if x >= (self.width as i32 - 25) {
                        //crate::println!("Taskbar: Power Button Clicked!");
                        action = TaskbarAction::OpenPowerMenu;
                    } else if x <= 80 {
                        //crate::println!("Taskbar: App Menu Clicked!");
                        action = TaskbarAction::OpenAppMenu;
                    } else {
                        let mut btn_x = 90i32;
                        for (title, win_idx) in &self.minimized_labels {
                            let label_len = if title.len() > 8 { 10 } else { title.len() };
                            let label_width = (label_len as i32 * 8) + 12;
                            if x >= btn_x && x < btn_x + label_width && y >= 4 && y <= 26 {
                                action = TaskbarAction::RestoreWindow(*win_idx);
                                break;
                            }
                            btn_x += label_width + 4;
                        }
                    }
                }
            }
        }

        action
    }
}

// --- boilerplate DrawTarget ---
impl OriginDimensions for Taskbar {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for Taskbar {
    type Color = Rgb888;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
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
                    if self.bpp == 4 {
                        self.buffer[index + 3] = 0;
                    }
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

        let date_str = format!("{}, {:02} {} {}", weekday, time.day, month_str, time.year);
        let time_str = format!("{:02}:{:02}", time.hour, time.minute);

        // 1. Base Background (Dark Gray)
        Rectangle::new(Point::zero(), self.size())
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(30, 30, 30)))
            .draw(self)
            .unwrap();

        let text_style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);

        // 2. Apps Menu (Left)
        Text::new("KafkaOS", Point::new(10, 20), text_style)
            .draw(self)
            .unwrap();

        // 3. Time & Date (Center)
        //let text_width = date_str.len() as u32 * 8;
        let center_x = (self.width / 2) - ((date_str.len() as u32 * 8) / 2);
        Text::new(&date_str, Point::new(center_x as i32, 12), text_style)
            .draw(self)
            .unwrap();
        //let text_width = time_str.len() as u32 * 8;
        let center_x = (self.width / 2) - ((time_str.len() as u32 * 8) / 2);
        Text::new(&time_str, Point::new(center_x as i32, 27), text_style)
            .draw(self)
            .unwrap();

        //let power_btn_x = (self.width - 70) as i32;
        Line::new(
            Point::new(self.width as i32 - 15, 3),
            Point::new(self.width as i32 - 15, 15),
        )
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::RED, 3))
        .draw(self)
        .unwrap();

        Arc::new(
            Point::new(self.width as i32 - 25, 7),
            20,
            -60.0.deg(),
            300.0.deg(),
        )
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::RED, 3))
        .draw(self)
        .unwrap();

        // Report damage to the Compositor
        super::graphics::report_damage(crate::gui::geometry::Rect::new(
            0,
            0,
            self.width as i32,
            self.height as i32,
        ));
    }
    /// Draw minimized window labels on the taskbar and store their positions
    pub fn draw_minimized_windows(&mut self, titles: &[(String, usize)]) {
        self.minimized_labels = titles.iter().cloned().collect();

        if titles.is_empty() {
            return;
        }

        let btn_style = MonoTextStyle::new(&FONT_8X13, Rgb888::new(200, 200, 200));
        let mut x_offset = 90i32; // Start after "KafkaOS" label

        for (title, _idx) in titles {
            // Truncate long titles to 8 chars
            let label: String = if title.len() > 8 {
                let mut s = String::from(&title[..8]);
                s.push_str("..");
                s
            } else {
                title.clone()
            };

            let label_width = (label.len() as u32 * 8) + 12;

            // Draw button background
            Rectangle::new(Point::new(x_offset, 4), Size::new(label_width, 22))
                .into_styled(PrimitiveStyle::with_fill(Rgb888::new(60, 60, 70)))
                .draw(self)
                .unwrap();

            // Draw border
            Rectangle::new(Point::new(x_offset, 4), Size::new(label_width, 22))
                .into_styled(PrimitiveStyle::with_stroke(Rgb888::new(100, 100, 120), 1))
                .draw(self)
                .unwrap();

            // Draw label text
            Text::new(&label, Point::new(x_offset + 6, 20), btn_style)
                .draw(self)
                .unwrap();

            x_offset += label_width as i32 + 4; // gap between buttons
        }

        // Report damage so the taskbar repaints
        super::graphics::report_damage(crate::gui::geometry::Rect::new(
            0,
            0,
            self.width as i32,
            self.height as i32,
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
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "Unk",
    }
}
