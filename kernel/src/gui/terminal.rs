// src/gui/terminal.rs
//
// GUI Terminal Emulator for KafkaOS — Compositor-Compatible
// v3: Only re-renders changed rows for speed. No flicker.

use core::fmt;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{MonoTextStyle, ascii::FONT_8X13},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use spin::Mutex;

use super::geometry::Rect;
use super::graphics::{COMPOSITOR_WAKER, report_damage};
use super::window::Window;

// kind of sets eveyrthing up
pub const TERM_WIN_X: i32 = 100;
pub const TERM_WIN_Y: i32 = 100;
pub const TERM_WIN_W: u32 = 400;
pub const TERM_WIN_H: u32 = 300;

//would handle geome
const CHAR_WIDTH: i32 = 8;
const CHAR_HEIGHT: i32 = 13;
const TITLE_BAR_H: i32 = 20;
const PAD: i32 = 4;

const TERM_COLS: usize = 48;
const TERM_ROWS: usize = 20;

#[derive(Copy, Clone, PartialEq)]
pub struct TermCell {
    ch: u8,
    fg: u8,
}

impl TermCell {
    const fn blank() -> Self {
        Self {
            ch: b' ',
            fg: COLOR_GREEN,
        }
    }
}

pub const COLOR_GREEN: u8 = 0;
pub const COLOR_CYAN: u8 = 1;
pub const COLOR_YELLOW: u8 = 2;
pub const COLOR_WHITE: u8 = 3;
pub const COLOR_RED: u8 = 4;

fn palette(idx: u8) -> Rgb888 {
    match idx {
        COLOR_GREEN => Rgb888::new(0, 255, 0),
        COLOR_CYAN => Rgb888::new(0, 255, 255),
        COLOR_YELLOW => Rgb888::new(255, 255, 0),
        COLOR_WHITE => Rgb888::WHITE,
        COLOR_RED => Rgb888::new(255, 80, 80),
        _ => Rgb888::new(0, 255, 0),
    }
}

#[derive(Clone)]
pub struct GuiTerminal {
    pub cells: [[TermCell; TERM_COLS]; TERM_ROWS],
    pub col: usize,
    pub row: usize,
    pub fg: u8,
    pub prev_row: usize,
    pub needs_full_redraw: bool,
    pub needs_redraw: bool,
}

impl GuiTerminal {
    pub const fn new() -> Self {
        Self {
            cells: [[TermCell::blank(); TERM_COLS]; TERM_ROWS],
            col: 0,
            row: 0,
            fg: COLOR_GREEN,
            prev_row: 0,
            needs_full_redraw: true,
            needs_redraw: false,
        }
    }

    pub fn set_fg(&mut self, color: u8) {
        self.fg = color;
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            0x08 => self.backspace(),
            byte => {
                if self.col >= TERM_COLS {
                    self.new_line();
                }
                self.cells[self.row][self.col] = TermCell {
                    ch: byte,
                    fg: self.fg,
                };
                self.col += 1;
            }
        }
    }

    fn new_line(&mut self) {
        self.col = 0;
        if self.row >= TERM_ROWS - 1 {
            self.scroll_up();
        } else {
            self.row += 1;
        }
    }

    fn scroll_up(&mut self) {
        for r in 1..TERM_ROWS {
            self.cells[r - 1] = self.cells[r];
        }
        self.cells[TERM_ROWS - 1] = [TermCell::blank(); TERM_COLS];
        self.needs_full_redraw = true;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.cells[self.row][self.col] = TermCell::blank();
        }
    }

    pub fn clear(&mut self) {
        self.cells = [[TermCell::blank(); TERM_COLS]; TERM_ROWS];
        self.col = 0;
        self.row = 0;
        self.needs_full_redraw = true;
    }

    // renderiing it row by row i guess, at max i can avoid the tear
    fn render_row(&self, r: usize, window: &mut Window) {
        let text_x = PAD;
        let base_y = TITLE_BAR_H + CHAR_HEIGHT;
        let py_top = base_y + (r as i32) * CHAR_HEIGHT - CHAR_HEIGHT + 2;

        let _ = Rectangle::new(
            Point::new(text_x, py_top),
            Size::new((TERM_COLS as i32 * CHAR_WIDTH) as u32, CHAR_HEIGHT as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(Rgb888::BLACK))
        .draw(window);

        // Draw only non-space characters in this row
        let mut char_buf = [0u8; 1];
        for c in 0..TERM_COLS {
            let cell = &self.cells[r][c];
            if cell.ch != b' ' {
                char_buf[0] = cell.ch;
                if let Ok(s) = core::str::from_utf8(&char_buf) {
                    let style = MonoTextStyle::new(&FONT_8X13, palette(cell.fg));
                    let px = text_x + (c as i32) * CHAR_WIDTH;
                    let py = base_y + (r as i32) * CHAR_HEIGHT;
                    let _ = Text::new(s, Point::new(px, py), style).draw(window);
                }
            }
        }
    }
    fn render_cursor(&self, window: &mut Window) {
        if self.col < TERM_COLS && self.row < TERM_ROWS {
            let text_x = PAD;
            let base_y = TITLE_BAR_H + CHAR_HEIGHT;
            let cx = text_x + (self.col as i32) * CHAR_WIDTH;
            let cy = base_y + (self.row as i32) * CHAR_HEIGHT - CHAR_HEIGHT + 2;
            let _ = Rectangle::new(
                Point::new(cx, cy),
                Size::new(CHAR_WIDTH as u32, CHAR_HEIGHT as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(palette(self.fg)))
            .draw(window);
        }
    }
    pub fn render_into_window(&mut self, window: &mut Window) {
        if self.needs_full_redraw {
            // Scroll or clear: redraw all rows
            for r in 0..TERM_ROWS {
                self.render_row(r, window);
            }
            self.render_cursor(window);
            self.needs_full_redraw = false;
            self.prev_row = self.row;
            return;
        }
        self.render_row(self.row, window);

        if self.prev_row != self.row && self.prev_row < TERM_ROWS {
            self.render_row(self.prev_row, window);
        }

        self.render_cursor(window);
        self.prev_row = self.row;
    }
}

impl fmt::Write for GuiTerminal {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

//declared it globally
pub static GUI_TERMINAL: Mutex<GuiTerminal> = Mutex::new(GuiTerminal::new());

/*
pub static TERMINAL_WINDOW: Mutex<Option<Window>> = Mutex::new(None);

pub fn init_terminal(bpp: usize) {
    let mut win = Window::new(
        TERM_WIN_X, TERM_WIN_Y,
        TERM_WIN_W, TERM_WIN_H,
        "Terminal", Rgb888::BLACK, bpp,
    );
    win.render_internal_graphics();
    *TERMINAL_WINDOW.lock() = Some(win);
}
*/

pub fn _tprint(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut term = GUI_TERMINAL.lock();
        term.write_fmt(args).unwrap();
        term.needs_redraw = true;

        //drop(term);

        /*let mut win_guard = TERMINAL_WINDOW.lock();
        if let Some(window) = win_guard.as_mut() {
            term.render_into_window(window);
        }
        drop(win_guard);
        drop(term);

        // Only damage the text area, not the title bar
        report_damage(Rect::new(
            TERM_WIN_X,
            TERM_WIN_Y + TITLE_BAR_H,
            TERM_WIN_W as i32,
            TERM_WIN_H as i32 - TITLE_BAR_H,
        ));

        if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }
        */
    });
    if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
        waker.wake();
    }
}

pub fn set_terminal_color(fg: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        GUI_TERMINAL.lock().set_fg(fg);
    });
}

pub fn clear_terminal() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut term = GUI_TERMINAL.lock();
        term.clear();
        term.needs_redraw = true;

        drop(term);

        if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }

        /*
        let mut win_guard = TERMINAL_WINDOW.lock();
        if let Some(window) = win_guard.as_mut() {
            term.render_into_window(window);
        }
        drop(win_guard);
        drop(term);

        report_damage(Rect::new(
            TERM_WIN_X, TERM_WIN_Y,
            TERM_WIN_W as i32, TERM_WIN_H as i32,
        ));

        if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }*/
    });
}

// will be used in cli.rs
#[macro_export]
macro_rules! tprint {
    ($($arg:tt)*) => ({
        $crate::gui::terminal::_tprint(format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! tprintln {
    ()            => ($crate::tprint!("\n"));
    ($($arg:tt)*) => ($crate::tprint!("{}\n", format_args!($($arg)*)));
}
