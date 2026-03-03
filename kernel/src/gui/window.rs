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

use crate::gui::graphics::{UIEvent, RawMouse, APP_REQUESTS};
use crate::{exit_qemu,QemuExitCode};

use core::convert::Infallible;

// For Application Specific Parameters
pub enum AppState {
    None, // For basic windows
    FileExplorer {
        current_path: alloc::string::String,
        displayed_entries: alloc::vec::Vec<alloc::string::String>,
    },
    Terminal {
        terminal: super::terminal::GuiTerminal,
    },
    Calculator {
        display: alloc::string::String,
        operand1: i32,
        operator: Option<char>,
        new_input: bool, // True if we just pressed +, -, etc.
    },
}

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
    pub app_state: AppState,
}

impl Window {
    pub fn new(x: i32, y: i32, width: u32, height: u32, title: &str, color: Rgb888, bpp: usize) -> Self {
        Self {
            x,
            y, //cmp::min(30, y),
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
            app_state: AppState::None,
        }
    }
    // Constructor For Application Specific Windows
    pub fn with_state(x: i32, y: i32, width: u32, height: u32, title: &str, color: Rgb888, bpp: usize, app_state: AppState) -> Self {
        let mut window = Self::new(x, y, width, height, title, color, bpp);
        window.app_state = app_state;

        window
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
        let events = core::mem::take(&mut self.event_queue);

        // Drain clears the inbox so we don't process the same click twice
        for event in events {
            match event {
                UIEvent::MouseClick { x, y, button } => {
                    match button {
                        RawMouse::Left (raw_x, raw_y) => {
                            //crate::println!("Window '{}' clicked at local {}, {}", self.title, x, y);
                            
                            if self.title == "Power Menu" {
                                // Did they click inside the content area (below the title bar)?
                                if y > 20 {
                                    if y >= 25 && y < 50 {
                                        crate::println!("Sleep selected");
                                        self.close_btn = true; // Auto-close the menu
                                    } else if y >= 50 && y < 75 {
                                        crate::println!("Restart selected");
                                        self.close_btn = true;
                                    } else if y >= 75 && y <= 100 {
                                        crate::println!("Shutdown selected! Goodbye.");
                                        self.close_btn = true;
                                        exit_qemu(QemuExitCode::Success);
                                    }
                                }
                            }
                            if self.title == "App Menu" {
                                if y > 20 {
                                    if y >= 25 && y < 50 {
                                        crate::println!("Launching Terminal...");
                                        APP_REQUESTS.lock().push(super::graphics::AppRequest::Terminal);
                                        //super::terminal::init_terminal(bpp);
                                        self.close_btn = true; // Auto-close the menu
                                    } else if y >= 50 && y < 75 {
                                        crate::println!("Launching Files...");
                                        APP_REQUESTS.lock().push(super::graphics::AppRequest::Files);
                                        self.close_btn = true;
                                    } else if y >= 75 && y <= 100 {
                                        crate::println!("Launching Settings...");
                                        self.close_btn = true;
                                    } else if y >= 100 && y <= 125 { // Assuming this coordinate range
                                        crate::println!("Launching Calculator...");
                                        APP_REQUESTS.lock().push(crate::gui::graphics::AppRequest::Calculator);
                                        self.close_btn = true;
                                    }
                                }
                            }
                            if self.title == "Files" {
                                let mut needs_redraw = false;
                                // 1. Lock the state, modify the path, but DO NOT draw here!
                                if let AppState::FileExplorer { ref mut current_path, ref displayed_entries } = self.app_state {
                                    if y >= 30 && y <= 45 && x >= 10 && x <= 120 {
                                        // Back Button Clicked
                                        let mut parts: alloc::vec::Vec<&str> = current_path.split_terminator('/').collect();
                                        parts.pop();
                                        if parts.is_empty() {
                                            *current_path = alloc::string::String::new();
                                        } else {
                                            *current_path = parts.join("/") + "/";
                                        }
                                        needs_redraw = true; // Signal that we want to redraw
                                    } 
                                    else if y >= 55 {
                                        // Row Clicked
                                        let row_index = ((y - 55) / 20) as usize;
                                        if row_index < displayed_entries.len() {
                                            let clicked_item = &displayed_entries[row_index];
                                            if clicked_item.ends_with('/') {
                                                current_path.push_str(clicked_item);
                                                needs_redraw = true; // Signal that we want to redraw
                                            } else {
                                                crate::println!("Selected File: /{}{}", current_path, clicked_item);
                                            }
                                        }
                                    }
                                } // THE LOCK IS RELEASED HERE!

                                // 2. Call the draw function safely outside the lock!
                                if needs_redraw {
                                    self.render_file_explorer();
                                }
                            }

                            if self.title == "Calculator" {
                                let mut needs_redraw = false;

                                if let AppState::Calculator { ref mut display, ref mut operand1, ref mut operator, ref mut new_input } = self.app_state {
                                    
                                    // Did they click inside the grid area?
                                    if y >= 70 && y <= 230 && x >= 10 && x <= 190 {
                                        // Calculate exactly which button in the grid was clicked!
                                        let col = (x - 10) / 45;
                                        let row = (y - 70) / 40;
                                        
                                        if col < 4 && row < 4 {
                                            let labels = [
                                                '7', '8', '9', '/',
                                                '4', '5', '6', '*',
                                                '1', '2', '3', '-',
                                                'C', '0', '=', '+'
                                            ];
                                            let btn = labels[(row * 4 + col) as usize];

                                            match btn {
                                                '0'..='9' => {
                                                    if *new_input {
                                                        display.clear();
                                                        *new_input = false;
                                                    }
                                                    display.push(btn);
                                                },
                                                '+' | '-' | '*' | '/' => {
                                                    // Parse the current display string into an integer
                                                    if let Ok(val) = display.parse::<i32>() {
                                                        *operand1 = val;
                                                    }
                                                    *operator = Some(btn);
                                                    *new_input = true;
                                                },
                                                '=' => {
                                                    if let (Ok(val2), Some(op)) = (display.parse::<i32>(), *operator) {
                                                        let result = match op {
                                                            '+' => *operand1 + val2,
                                                            '-' => *operand1 - val2,
                                                            '*' => *operand1 * val2,
                                                            '/' => if val2 != 0 { *operand1 / val2 } else { 0 }, // Prevent divide by zero!
                                                            _ => 0,
                                                        };
                                                        *display = alloc::format!("{}", result);
                                                        *new_input = true;
                                                        *operator = None;
                                                    }
                                                },
                                                'C' => {
                                                    *display = alloc::string::String::from("0");
                                                    *operand1 = 0;
                                                    *operator = None;
                                                    *new_input = true;
                                                },
                                                _ => {}
                                            }
                                            needs_redraw = true;
                                        }
                                    }
                                } // Lock released

                                if needs_redraw {
                                    self.render_calculator();
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

    // Power Menu
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

    // Applications Menu
    pub fn render_app_menu(&mut self) {
        // 1. Draw standard background/borders
        self.render_internal_graphics();

        // 2. Draw 3 Placeholder Apps
        let style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        
        Text::new("Terminal", Point::new(10, 40), style).draw(self).unwrap();
        Text::new("Files", Point::new(10, 65), style).draw(self).unwrap();
        Text::new("Settings", Point::new(10, 90), style).draw(self).unwrap();
        Text::new("Calculator", Point::new(10, 115), style).draw(self).unwrap();
    }

    // File Explorer
    pub fn render_file_explorer(&mut self) {// 1. READ PHASE: Get a clone of the current path so we don't lock `self`
        let current_path = match &self.app_state {
            AppState::FileExplorer { current_path, .. } => current_path.clone(),
            _ => alloc::string::String::new(), // Failsafe
        };

        // 2. DRAW PHASE (Static Elements): We can freely pass `self` here!
        self.render_internal_graphics();
        let text_style = MonoTextStyle::new(&FONT_8X13, Rgb888::BLACK);
        
        Text::new("[ .. Go Back ]", Point::new(10, 40), text_style).draw(self).unwrap();
        Text::new(&alloc::format!("Path: /{}", current_path), Point::new(140, 40), text_style).draw(self).unwrap();
        Text::new("----------------------------------------", Point::new(10, 50), text_style).draw(self).unwrap();

        // 3. COMPUTE PHASE: Build the new list of entries in a temporary variable
        let mut new_entries = alloc::vec::Vec::new();
        
        if let Some(fs_mutex) = crate::fs::FILESYSTEM.get() {
            let fs = fs_mutex.lock();
            for path in fs.list_files() {
                if path.starts_with(&current_path) && path != current_path {
                    let remainder = &path[current_path.len()..];
                    if let Some(slash_idx) = remainder.find('/') {
                        let folder_name = alloc::string::String::from(&remainder[..=slash_idx]);
                        if !new_entries.contains(&folder_name) {
                            new_entries.push(folder_name);
                        }
                    } else {
                        new_entries.push(alloc::string::String::from(remainder));
                    }
                }
            }
        }

        // 4. DRAW PHASE (Dynamic Elements): Draw the entries we just computed
        let mut y_offset = 65;
        for entry in &new_entries {
            if entry.ends_with('/') {
                // Folder (Yellow)
                Rectangle::new(Point::new(10, y_offset - 10), Size::new(10, 10))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(255, 255, 0)))
                    .draw(self).unwrap();
            } else {
                // File (Blue)
                Rectangle::new(Point::new(10, y_offset - 10), Size::new(10, 10))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::BLUE))
                    .draw(self).unwrap();
            }
            Text::new(entry, Point::new(30, y_offset), text_style).draw(self).unwrap();
            y_offset += 20;
        }

        // 5. UPDATE PHASE: Now that all drawing is done, save the new list to our state!
        if let AppState::FileExplorer { ref mut displayed_entries, .. } = self.app_state {
            *displayed_entries = new_entries;
        }

        // Report damage
        crate::gui::graphics::report_damage(crate::gui::geometry::Rect::new(
            self.x, self.y, self.width as i32, self.height as i32
        ));
    }

    pub fn render_calculator(&mut self) {
        self.render_internal_graphics(); // Draws the border and title bar

        let text_style = MonoTextStyle::new(&FONT_8X13, Rgb888::BLACK);
        let btn_style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);

        // 1. Draw the "LCD Display" (White box at the top)
        Rectangle::new(Point::new(10, 30), Size::new(180, 30))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::WHITE))
            .draw(self).unwrap();

        // Safely fetch the current display string from the state
        let display_text = match &self.app_state {
            AppState::Calculator { display, .. } => display.clone(),
            _ => alloc::string::String::from("Error"),
        };

        Text::new(&display_text, Point::new(15, 50), text_style).draw(self).unwrap();

        // 2. Draw the 4x4 Button Grid
        let labels = [
            "7", "8", "9", "/",
            "4", "5", "6", "*",
            "1", "2", "3", "-",
            "C", "0", "=", "+"
        ];

        let mut idx = 0;
        for row in 0..4 {
            for col in 0..4 {
                let bx = 10 + (col * 45); // 45 pixels wide per button
                let by = 70 + (row * 40); // 40 pixels tall per button

                // Draw button background
                Rectangle::new(Point::new(bx, by), Size::new(40, 35))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(80, 80, 80)))
                    .draw(self).unwrap();

                // Draw button text
                Text::new(labels[idx], Point::new(bx + 15, by + 22), btn_style).draw(self).unwrap();
                idx += 1;
            }
        }

        crate::gui::graphics::report_damage(crate::gui::geometry::Rect::new(
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
}
