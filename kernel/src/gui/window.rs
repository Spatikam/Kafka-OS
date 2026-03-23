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

use crate::gui::graphics::{UIEvent, RawMouse, APP_REQUESTS, report_damage};
use crate::{exit_qemu,QemuExitCode};
//use crate::gui::paint::{PAINT_APP};
use core::convert::Infallible;

use crate::gui::geometry::Rect; //geometry module

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
        clear_on_next: bool,
    },
    Paint {
        paint: super::paint::PaintApp,
    },
    Snake {
        snake: super::snake::SnakeGame,
    }
}
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ResizeEdge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
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
    pub is_minimized:bool,
    pub was_minimized: bool,
    pub is_dragging: bool, // is Window being dragged Windows
    pub drag_x: i32,
    pub drag_y: i32,
    pub is_resizing:bool,
    pub resize_edge:Option<ResizeEdge>,
    pub min_width:u32,
    pub min_height:u32,

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
            is_minimized:false,
            was_minimized: false,
            is_dragging: false,
            drag_x: x,
            drag_y: y,
            is_resizing:false,
            resize_edge:None,
            min_width:80,
            min_height:60,
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

        //addition of Minimize
        Rectangle::new(Point::new(self.width as i32 - 40, 0), Size::new(20, 20))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(200, 180, 0)))
            .draw(self)
            .unwrap();

        Rectangle::new(Point::new(self.width as i32 - 36, 13), Size::new(12, 2))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::BLACK))
            .draw(self)
            .unwrap();
        // Drawing the Close Button
        Rectangle::new(Point::new(self.width as i32 - 20, 0), Size::new(20, 20))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(self)
            .unwrap();
        let close_style = MonoTextStyle::new(&FONT_8X13,Rgb888::WHITE);
        Text::new("X",Point::new(self.width as i32 - 15,14),close_style).draw(self).unwrap();


        // Draw Border
        Rectangle::new(Point::zero(), Size::new(self.width, self.height))
            .into_styled(PrimitiveStyle::with_stroke(Rgb888::BLACK, 1))
            .draw(self)
            .unwrap();

        // Tell the Compositor that this window's area on the physical screen is damaged
        report_damage(Rect::new(
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
                            } else if self.title == "App Menu" {
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
                                        crate::println!("Launching Paint...");
                                        APP_REQUESTS.lock().push(super::graphics::AppRequest::Paint);
                                        self.close_btn = true;
                                    } else if y >= 100 && y <= 125 { // Assuming this coordinate range
                                        crate::println!("Launching Calculator...");
                                        APP_REQUESTS.lock().push(crate::gui::graphics::AppRequest::Calculator);
                                        self.close_btn = true;
                                    }
                                    else if y >= 125 && y <=150{
                                        crate::println!("Launching Snake gAME");
                                        APP_REQUESTS.lock().push(crate::gui::graphics::AppRequest::Snake);
                                        self.close_btn = true;
                                    }
                                }
                            } else if self.title == "Files" {
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
                            } else if self.title == "Paint" {
                                super::paint::handle_paint_click(raw_x, raw_y, self.x, self.y, self);
                                //self.render_paint();

                            } else if self.title == "Calculator" {
                                let mut needs_redraw = false;

                                if let AppState::Calculator { ref mut display, ref mut clear_on_next } = self.app_state {
                                    
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
                                                    if *clear_on_next || display == "0" || display == "Error" || display == "Div by 0" {
                                                        display.clear();
                                                        *clear_on_next = false;
                                                    }
                                                    display.push(btn);
                                                },
                                                '+' | '-' | '*' | '/' => {
                                                    *clear_on_next = false;
                                                    
                                                    // Smart UX: If they click '+' then '-', replace the '+' instead of crashing
                                                    if let Some(last_char) = display.chars().last() {
                                                        if "+-*/".contains(last_char) {
                                                            display.pop(); 
                                                        }
                                                    }
                                                    display.push(btn);
                                                },
                                                '=' => {
                                                    match evaluate_expression(display) {
                                                        Ok(result) => *display = alloc::format!("{}", result),
                                                        Err(e) => *display = alloc::string::String::from(e),
                                                    }
                                                    *clear_on_next = true;
                                                },
                                                'C' => {
                                                    *display = alloc::string::String::from("0");
                                                    *clear_on_next = false;
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
                            }else if self.title == "Snake"{
                                if let AppState::Snake {ref mut snake} = self.app_state{
                                    if snake.state == super::snake::GameState::GameOver{
                                        snake.reset();
                                        self.render_snake();
                                    }
                                }
                            }

                            if self.width as i32 - 20 <= x && x <= self.width as i32 - 2 && 1 <= y && y <= 19 {
                                self.close_btn = true;
                            }else if self.width as i32 - 40 <= x  &&  x< self.width as i32 - 20 && 1 <= y && y <= 19{
                                self.is_minimized = true;
                            }else if 0 <= x && x <= self.width as i32 - 21 && 0 <= y && y <= 20 {
                                self.is_dragging = true;
                                self.drag_x = x;
                                self.drag_y = y;
                            }
                        },
                        RawMouse::Left_Pressed (raw_x, raw_y) => {
                            if self.title == "Paint" {
                                super::paint::handle_paint_click(raw_x, raw_y, self.x, self.y, self);
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
        Text::new("Paint", Point::new(10, 90), style).draw(self).unwrap();
        Text::new("Calculator", Point::new(10, 115), style).draw(self).unwrap();
        Text::new("Snake",Point::new(10,140),style).draw(self).unwrap();
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
        report_damage(Rect::new(
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

        report_damage(Rect::new(
            self.x, self.y, self.width as i32, self.height as i32
        ));
    }

    pub fn render_paint(&mut self) {
        let mut global_paint_state = super::paint::PAINT_APP.lock();  

        if global_paint_state.needs_redraw {
            //for window in &mut wm.windows {
            let mut temp_state = core::mem::replace(&mut self.app_state, AppState::None);

            if let AppState::Paint { ref mut paint } = temp_state {

                *paint = global_paint_state.clone();
                
                // RENDER TO THE REAL WINDOW
                paint.render_into_window(self);
                
                // Report damage exactly where the window is currently located!
                report_damage(Rect::new(
                    self.x, self.y + 20, self.width as i32, self.height as i32 - 20
                ));
                
                // Reset the global redraw flag now that it has been handled
                //global_term_state.needs_full_redraw = false;
                //crate::println!("Term loop {}", window.title);
            }
            self.app_state = temp_state;

            global_paint_state.needs_redraw = false;
            //}
        }
    }
    // ============================================================
// PASTE THESE INTO window.rs, replacing the existing
// render_snake() and render_snake_partial() methods on Window.
// ============================================================

    pub fn render_snake(&mut self) {
        self.render_internal_graphics();

        let text_style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        let dim_style  = MonoTextStyle::new(&FONT_8X13, Rgb888::new(120, 120, 140));
        let bg_color   = Rgb888::new(26, 26, 46);

        let game_y = 20i32;   // below title bar
        let game_w = self.width;
        let game_h = self.height - 20;

        // Fill game background
        Rectangle::new(Point::new(0, game_y), Size::new(game_w, game_h))
            .into_styled(PrimitiveStyle::with_fill(bg_color))
            .draw(self)
            .unwrap();

        // Clone game data to avoid borrow conflicts
        let (body, food, score, high_score, state, direction) = match &self.app_state {
            AppState::Snake { snake } => (
                snake.body.clone(),
                snake.food,
                snake.score,
                snake.high_score,
                snake.state,
                snake.direction,
            ),
            _ => return,
        };

        let cell = 16u32;
        let grid_y = game_y + 20; // below score bar

        // ── Score bar ───────────────────────────────────────────
        let score_text = alloc::format!("Score: {}  Hi: {}", score, high_score);
        Text::new(&score_text, Point::new(5, game_y + 15), text_style)
            .draw(self)
            .unwrap();

        // Speed indicator on the right
        let speed = match score / 50 {
            0 => "SPD: *",
            1 => "SPD: **",
            2 => "SPD: ***",
            _ => "SPD: ****",
        };
        Text::new(speed, Point::new(self.width as i32 - 80, game_y + 15), dim_style)
            .draw(self)
            .unwrap();

        // ── Play area border ────────────────────────────────────
        let grid_w = 20u32 * cell;
        let grid_h = 15u32 * cell;
        Rectangle::new(
            Point::new(0, grid_y - 1),
            Size::new(grid_w + 2, grid_h + 2),
        )
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::new(60, 60, 90), 1))
        .draw(self)
        .unwrap();

        // ── Food (bright red with a small highlight) ────────────
        let fx = (food.x as u32 * cell + 1) as i32;
        let fy = grid_y + (food.y as u32 * cell + 1) as i32;
        // Outer
        Rectangle::new(Point::new(fx, fy), Size::new(cell - 2, cell - 2))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(220, 30, 30)))
            .draw(self)
            .unwrap();
        // Inner highlight (2x2 bright spot)
        Rectangle::new(Point::new(fx + 2, fy + 2), Size::new(4, 4))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(255, 120, 120)))
            .draw(self)
            .unwrap();

        // ── Snake body (gradient: bright head → dim tail) ───────
        let body_len = body.len().max(1);
        for (i, seg) in body.iter().enumerate() {
            let sx = (seg.x as u32 * cell + 1) as i32;
            let sy = grid_y + (seg.y as u32 * cell + 1) as i32;

            if i == 0 {
                // ── Head: bright green with direction indicator ──
                Rectangle::new(Point::new(sx, sy), Size::new(cell - 2, cell - 2))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(50, 255, 50)))
                    .draw(self)
                    .unwrap();

                // Draw eyes based on direction
                let (eye1, eye2) = match direction {
                    super::snake::Direction::Right => (
                        Point::new(sx + 9, sy + 3),
                        Point::new(sx + 9, sy + 9),
                    ),
                    super::snake::Direction::Left => (
                        Point::new(sx + 3, sy + 3),
                        Point::new(sx + 3, sy + 9),
                    ),
                    super::snake::Direction::Up => (
                        Point::new(sx + 3, sy + 3),
                        Point::new(sx + 9, sy + 3),
                    ),
                    super::snake::Direction::Down => (
                        Point::new(sx + 3, sy + 9),
                        Point::new(sx + 9, sy + 9),
                    ),
                };
                // Eyes: 3x3 dark squares
                Rectangle::new(eye1, Size::new(3, 3))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(10, 10, 10)))
                    .draw(self)
                    .unwrap();
                Rectangle::new(eye2, Size::new(3, 3))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(10, 10, 10)))
                    .draw(self)
                    .unwrap();
            } else {
                // ── Body: gradient from bright green to dark green ──
                let brightness = 200u32.saturating_sub(i as u32 * 140 / body_len as u32) as u8;
                let g = brightness;
                let r = brightness / 8; // slight warmth
                Rectangle::new(Point::new(sx, sy), Size::new(cell - 2, cell - 2))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(r, g, 20)))
                    .draw(self)
                    .unwrap();
            }
        }

        // ── State overlays ──────────────────────────────────────
        match state {
            super::snake::GameState::GameOver => {
                // Semi-dark overlay box
                let overlay_y = grid_y + 80;
                Rectangle::new(Point::new(30, overlay_y - 15), Size::new(260, 65))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(15, 15, 30)))
                    .draw(self)
                    .unwrap();
                Rectangle::new(Point::new(30, overlay_y - 15), Size::new(260, 65))
                    .into_styled(PrimitiveStyle::with_stroke(Rgb888::new(220, 50, 50), 1))
                    .draw(self)
                    .unwrap();

                let gameover_style = MonoTextStyle::new(&FONT_8X13, Rgb888::new(255, 60, 60));
                Text::new("GAME OVER", Point::new(100, overlay_y), gameover_style)
                    .draw(self)
                    .unwrap();

                let final_score = alloc::format!("Final Score: {}", score);
                Text::new(&final_score, Point::new(95, overlay_y + 16), text_style)
                    .draw(self)
                    .unwrap();

                Text::new("ENTER to restart", Point::new(85, overlay_y + 35), dim_style)
                    .draw(self)
                    .unwrap();
            }
            super::snake::GameState::Paused => {
                let overlay_y = grid_y + 90;
                Rectangle::new(Point::new(60, overlay_y - 15), Size::new(200, 45))
                    .into_styled(PrimitiveStyle::with_fill(Rgb888::new(15, 15, 30)))
                    .draw(self)
                    .unwrap();
                Rectangle::new(Point::new(60, overlay_y - 15), Size::new(200, 45))
                    .into_styled(PrimitiveStyle::with_stroke(Rgb888::new(100, 100, 200), 1))
                    .draw(self)
                    .unwrap();

                let pause_style = MonoTextStyle::new(&FONT_8X13, Rgb888::new(150, 150, 255));
                Text::new("PAUSED", Point::new(120, overlay_y), pause_style)
                    .draw(self)
                    .unwrap();
                Text::new("P / ESC to resume", Point::new(82, overlay_y + 20), dim_style)
                    .draw(self)
                    .unwrap();
            }
            _ => {}
        }

        report_damage(Rect::new(
            self.x,
            self.y,
            self.width as i32,
            self.height as i32,
        ));
    }

    pub fn render_snake_partial(&mut self) {
        let (moved, last_tail, head, old_head, food, score, high_score, state, direction, body_len, just_ate) =
            match &self.app_state {
                AppState::Snake { snake } => (
                    snake.moved,
                    snake.last_tail,
                    snake.body.first().copied(),
                    snake.body.get(1).copied(),
                    snake.food,
                    snake.score,
                    snake.high_score,
                    snake.state,
                    snake.direction,
                    snake.body.len(),
                    snake.just_ate,
                ),
                _ => return,
            };

        if state != super::snake::GameState::Playing {
            self.render_snake();
            return;
        }

        if !moved {
            return;
        }

        let cell = 16u32;
        let grid_y = 40i32; // 20 (title bar) + 20 (score area)
        let bg_color = Rgb888::new(26, 26, 46);
        let text_style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        let dim_style  = MonoTextStyle::new(&FONT_8X13, Rgb888::new(120, 120, 140));

        // Track the damage bounding box (only repaint what changed)
        let mut min_x = self.width as i32;
        let mut min_y = self.height as i32;
        let mut max_x = 0i32;
        let mut max_y = 0i32;

        // Helper closure-style: expand damage region
        macro_rules! expand_damage {
            ($px:expr, $py:expr, $pw:expr, $ph:expr) => {
                let px = $px;
                let py = $py;
                if px < min_x { min_x = px; }
                if py < min_y { min_y = py; }
                if px + $pw > max_x { max_x = px + $pw; }
                if py + $ph > max_y { max_y = py + $ph; }
            };
        }

        // 1. Erase old tail
        if let Some(tail) = last_tail {
            let tx = (tail.x as u32 * cell) as i32;
            let ty = grid_y + (tail.y as u32 * cell) as i32;
            Rectangle::new(Point::new(tx, ty), Size::new(cell, cell))
                .into_styled(PrimitiveStyle::with_fill(bg_color))
                .draw(self)
                .unwrap();
            expand_damage!(tx, ty, cell as i32, cell as i32);
        }

        // 2. Draw new head with eyes
        if let Some(h) = head {
            let hx = (h.x as u32 * cell + 1) as i32;
            let hy = grid_y + (h.y as u32 * cell + 1) as i32;
            Rectangle::new(Point::new(hx, hy), Size::new(cell - 2, cell - 2))
                .into_styled(PrimitiveStyle::with_fill(Rgb888::new(50, 255, 50)))
                .draw(self)
                .unwrap();

            // Eyes
            let (eye1, eye2) = match direction {
                super::snake::Direction::Right => (Point::new(hx + 9, hy + 3), Point::new(hx + 9, hy + 9)),
                super::snake::Direction::Left  => (Point::new(hx + 3, hy + 3), Point::new(hx + 3, hy + 9)),
                super::snake::Direction::Up    => (Point::new(hx + 3, hy + 3), Point::new(hx + 9, hy + 3)),
                super::snake::Direction::Down  => (Point::new(hx + 3, hy + 9), Point::new(hx + 9, hy + 9)),
            };
            Rectangle::new(eye1, Size::new(3, 3))
                .into_styled(PrimitiveStyle::with_fill(Rgb888::new(10, 10, 10)))
                .draw(self)
                .unwrap();
            Rectangle::new(eye2, Size::new(3, 3))
                .into_styled(PrimitiveStyle::with_fill(Rgb888::new(10, 10, 10)))
                .draw(self)
                .unwrap();
            expand_damage!(hx - 1, hy - 1, cell as i32, cell as i32);
        }

        // 3. Repaint old head as body segment (gradient index 1)
        if let Some(oh) = old_head {
            let ox = (oh.x as u32 * cell + 1) as i32;
            let oy = grid_y + (oh.y as u32 * cell + 1) as i32;
            let brightness = 200u32.saturating_sub(140 / body_len.max(1) as u32) as u8;
            Rectangle::new(Point::new(ox, oy), Size::new(cell - 2, cell - 2))
                .into_styled(PrimitiveStyle::with_fill(Rgb888::new(brightness / 8, brightness, 20)))
                .draw(self)
                .unwrap();
            expand_damage!(ox - 1, oy - 1, cell as i32, cell as i32);
        }

        // 4. Redraw food (in case tail erased it, or new food spawned)
        let fx = (food.x as u32 * cell + 1) as i32;
        let fy = grid_y + (food.y as u32 * cell + 1) as i32;
        Rectangle::new(Point::new(fx, fy), Size::new(cell - 2, cell - 2))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(220, 30, 30)))
            .draw(self)
            .unwrap();
        Rectangle::new(Point::new(fx + 2, fy + 2), Size::new(4, 4))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(255, 120, 120)))
            .draw(self)
            .unwrap();
        expand_damage!(fx - 1, fy - 1, cell as i32, cell as i32);

        
        if just_ate {
            Rectangle::new(Point::new(0, 20), Size::new(self.width, 20))
                .into_styled(PrimitiveStyle::with_fill(bg_color))
                .draw(self)
                .unwrap();
            let score_text = alloc::format!("Score: {}  Hi: {}", score, high_score);
            Text::new(&score_text, Point::new(5, 35), text_style)
                .draw(self)
                .unwrap();
            let speed = match score / 50 {
                0 => "SPD: *",
                1 => "SPD: **",
                2 => "SPD: ***",
                _ => "SPD: ****",
            };
            Text::new(speed, Point::new(self.width as i32 - 80, 35), dim_style)
                .draw(self)
                .unwrap();
            expand_damage!(0, 20, self.width as i32, 20);
        }

        // 6. Report ONLY the damaged region instead of the whole window
        if max_x > min_x && max_y > min_y {
            report_damage(Rect::new(
                self.x + min_x,
                self.y + min_y,
                max_x - min_x,
                max_y - min_y,
            ));
        }
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

// Calculator Functions
fn precedence(op: char) -> u8 {
    match op {
        '+' | '-' => 1,
        '*' | '/' => 2,
        _ => 0,
    }
}

fn apply_op(a: i32, b: i32, op: char) -> Result<i32, &'static str> {
    match op {
        '+' => Ok(a + b),
        '-' => Ok(a - b),
        '*' => Ok(a * b),
        '/' => if b == 0 { Err("Div by 0") } else { Ok(a / b) },
        _ => Err("Invalid op"),
    }
}

fn evaluate_expression(expr: &str) -> Result<i32, &'static str> {
    let mut nums: alloc::vec::Vec<i32> = alloc::vec::Vec::new();
    let mut ops: alloc::vec::Vec<char> = alloc::vec::Vec::new();
    
    let mut current_num = 0;
    let mut parsing_num = false;

    for c in expr.chars() {
        if c.is_ascii_digit() {
            // Build multi-digit numbers (e.g., '1' then '2' becomes 12)
            current_num = current_num * 10 + (c as i32 - 48);
            parsing_num = true;
        } else if "+-*/".contains(c) {
            if !parsing_num { return Err("Syntax Error"); }
            nums.push(current_num);
            current_num = 0;
            parsing_num = false;
            
            // Resolve previous operations if they have higher or equal precedence
            while let Some(&top_op) = ops.last() {
                if precedence(top_op) >= precedence(c) {
                    let op = ops.pop().unwrap();
                    let b = nums.pop().unwrap();
                    let a = nums.pop().unwrap();
                    nums.push(apply_op(a, b, op)?);
                } else {
                    break;
                }
            }
            ops.push(c);
        }
    }
    
    // Push the very last number
    if parsing_num {
        nums.push(current_num);
    } else {
        return Err("Syntax Error");
    }

    // Resolve any remaining operations in the stacks
    while let Some(op) = ops.pop() {
        if nums.len() < 2 { return Err("Syntax Error"); }
        let b = nums.pop().unwrap();
        let a = nums.pop().unwrap();
        nums.push(apply_op(a, b, op)?);
    }

    if nums.len() == 1 {
        Ok(nums[0])
    } else {
        Err("Parse Error")
    }
}
