// src/gui/notepad.rs

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{MonoTextStyle, ascii::FONT_8X13},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use pc_keyboard::{DecodedKey, KeyCode};
use spin::Mutex;

use super::geometry::Rect;
use super::graphics::{COMPOSITOR_WAKER, report_damage};
use super::window::Window;

// ─── Window Geometry ─────────────────────────────────────────────
pub const NOTEPAD_WIN_X: i32 = 160;
pub const NOTEPAD_WIN_Y: i32 = 60;
pub const NOTEPAD_WIN_W: u32 = 580;
pub const NOTEPAD_WIN_H: u32 = 440;

// ─── Layout Constants ────────────────────────────────────────────
const CHAR_W: i32 = 8;
const CHAR_H: i32 = 13;
const TITLE_BAR_H: i32 = 20;
const STATUS_BAR_H: i32 = 18;
const GUTTER_W: i32 = 40; // 5-char line numbers
const PAD: i32 = 2;

// Derived: visible text area
const TEXT_X: i32 = GUTTER_W;
const TEXT_Y: i32 = TITLE_BAR_H;
const TEXT_AREA_W: i32 = NOTEPAD_WIN_W as i32 - GUTTER_W;
const TEXT_AREA_H: i32 = NOTEPAD_WIN_H as i32 - TITLE_BAR_H - STATUS_BAR_H;
const VISIBLE_COLS: usize = (TEXT_AREA_W / CHAR_W) as usize;
const VISIBLE_ROWS: usize = (TEXT_AREA_H / CHAR_H) as usize;

// ─── Colors ──────────────────────────────────────────────────────
const BG_COLOR: Rgb888 = Rgb888::new(30, 30, 30);
const TEXT_COLOR: Rgb888 = Rgb888::new(220, 220, 220);
const GUTTER_BG: Rgb888 = Rgb888::new(40, 40, 40);
const GUTTER_FG: Rgb888 = Rgb888::new(120, 120, 120);
const CURSOR_COLOR: Rgb888 = Rgb888::WHITE;
const SELECT_BG: Rgb888 = Rgb888::new(50, 80, 140);
const STATUS_BG: Rgb888 = Rgb888::new(0, 100, 180);
const STATUS_FG: Rgb888 = Rgb888::WHITE;
const TITLE_BG: Rgb888 = Rgb888::new(50, 50, 50);
const TITLE_FG: Rgb888 = Rgb888::WHITE;
const BTN_CLOSE: Rgb888 = Rgb888::new(230, 70, 70);
const BTN_MAX: Rgb888 = Rgb888::new(70, 200, 70);
const BTN_MIN: Rgb888 = Rgb888::new(230, 200, 50);

// so basically this decides what the notepad is doing right now
// editing = normal typing, save/open prompts = asking for filename in status bar
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Editing,
    SavePrompt,
    OpenPrompt,
}

// first i guess i will try for selection  using no_std
#[derive(Clone, Copy, PartialEq)] //standard alloc 
struct Selection {
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
}

impl Selection {
    /// Returns (from_row, from_col, to_row, to_col) in document order
    fn ordered(&self) -> (usize, usize, usize, usize) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_row, self.start_col, self.end_row, self.end_col)
        } else {
            (self.end_row, self.end_col, self.start_row, self.start_col)
        }
    }

    fn is_empty(&self) -> bool {
        self.start_row == self.end_row && self.start_col == self.end_col
    }
}

// I might test out with undo
#[derive(Clone)]
struct Snapshot {
    lines: Vec<Vec<u8>>,
    cursor_row: usize,
    cursor_col: usize,
}
const MAX_UNDO: usize = 50; // for now i will set something like this 

// next part is that I have to decide which all properties that a notepad can have (like scrolling, stuff like that)
pub struct NotepadState {
    lines: Vec<Vec<u8>>, // for now .
    cursor_row: usize,
    cursor_col: usize,
    scroll_row: usize, // for scrolling
    scroll_col: usize,
    selection: Option<Selection>, // this would mean like if i want to select a particular text after writing or while reading
    clipboard: Vec<u8>,
    pub mode: Mode,
    prompt_buf: String,
    filename: Option<String>,
    modified: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
}

impl NotepadState {
    pub const fn new() -> Self {
        Self {
            lines: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            selection: None,
            clipboard: Vec::new(),
            mode: Mode::Editing,
            prompt_buf: String::new(),
            filename: None,
            modified: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn init(&mut self) {
        self.lines = vec![Vec::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.selection = None;
        self.mode = Mode::Editing;
        self.prompt_buf = String::new();
        self.filename = None;
        self.modified = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    // i guess I can add some undo functions here which would be helpful
    fn push_undo(&mut self) {
        let snap = Snapshot {
            lines: self.lines.clone(), // clone it and pass
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        };
        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snap);
        self.redo_stack.clear(); // wipe redo history since we made a new change
    }

    fn undo(&mut self) {
        if let Some(snap) = self.undo_stack.pop() {
            let redo = Snapshot {
                lines: self.lines.clone(),
                cursor_row: self.cursor_row,
                cursor_col: self.cursor_col,
            };
            self.redo_stack.push(redo);
            self.lines = snap.lines;
            self.cursor_row = snap.cursor_row;
            self.cursor_col = snap.cursor_col;
            self.clamp_cursor();
            self.ensure_visible();
            self.modified = true;
        }
    }

    // i guess if someone wants to redo it, it will be something like
    fn redo(&mut self) {
        if let Some(snap) = self.redo_stack.pop() {
            let undo = Snapshot {
                lines: self.lines.clone(),
                cursor_row: self.cursor_row,
                cursor_col: self.cursor_col,
            };
            self.undo_stack.push(undo);
            self.lines = snap.lines;
            self.cursor_row = snap.cursor_row;
            self.cursor_col = snap.cursor_col;
            self.clamp_cursor();
            self.ensure_visible();
            self.modified = true;
        }
    }

    // so i assume all the notepads do have the line length, so we can define something like this
    fn line_len(&self, row: usize) -> usize {
        if row < self.lines.len() {
            self.lines[row].len()
        } else {
            0
        }
    }

    // and i guess i want to determine the total lines of the thing
    fn total_lines(&self) -> usize {
        self.lines.len()
    }

    // now comes the cursor
    fn clamp_cursor(&mut self) {
        if self.cursor_row >= self.total_lines() {
            self.cursor_row = self.total_lines().saturating_sub(1);
        }
        let len = self.line_len(self.cursor_row);
        if self.cursor_col > len {
            self.cursor_col = len;
        }
    }

    // to make sure it is visible
    fn ensure_visible(&mut self) {
        // i guess we will start with vertical scroll
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        }
        if self.cursor_row >= self.scroll_row + VISIBLE_ROWS {
            self.scroll_row = self.cursor_row - VISIBLE_ROWS + 1;
        }
        // Horizontal scroll
        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        }
        if self.cursor_col >= self.scroll_col + VISIBLE_COLS {
            self.scroll_col = self.cursor_col - VISIBLE_COLS + 1;
        }
    }

    //Selection stuff
    fn start_or_extend_selection(&mut self, shift: bool) {
        if shift {
            if self.selection.is_none() {
                self.selection = Some(Selection {
                    start_row: self.cursor_row,
                    start_col: self.cursor_col,
                    end_row: self.cursor_row,
                    end_col: self.cursor_col,
                });
            }
        } else {
            self.selection = None;
        }
    }

    fn update_selection_end(&mut self) {
        if let Some(sel) = self.selection.as_mut() {
            sel.end_row = self.cursor_row;
            sel.end_col = self.cursor_col;
        }
    }

    fn select_all(&mut self) {
        let last_row = self.total_lines().saturating_sub(1);
        let last_col = self.line_len(last_row);
        self.selection = Some(Selection {
            start_row: 0,
            start_col: 0,
            end_row: last_row,
            end_col: last_col,
        });
        self.cursor_row = last_row;
        self.cursor_col = last_col;
        self.ensure_visible();
    }

    fn get_selected_text(&self) -> Vec<u8> {
        let sel = match &self.selection {
            Some(s) if !s.is_empty() => s,
            _ => return Vec::new(),
        };
        let (r1, c1, r2, c2) = sel.ordered();
        let mut result = Vec::new();

        for r in r1..=r2 {
            if r >= self.lines.len() {
                break;
            }
            let line = &self.lines[r];
            let start = if r == r1 { c1.min(line.len()) } else { 0 };
            let end = if r == r2 {
                c2.min(line.len())
            } else {
                line.len()
            };
            if start <= end && end <= line.len() {
                result.extend_from_slice(&line[start..end]);
            }
            if r < r2 {
                result.push(b'\n');
            }
        }
        result
    }

    fn delete_selection(&mut self) {
        let sel = match self.selection.take() {
            Some(s) if !s.is_empty() => s,
            _ => return,
        };
        self.push_undo();
        let (r1, c1, r2, c2) = sel.ordered();

        if r1 == r2 {
            // Single line selection
            let c_start = c1.min(self.lines[r1].len());
            let c_end = c2.min(self.lines[r1].len());
            self.lines[r1].drain(c_start..c_end);
        } else {
            // Multi-line: keep prefix of first line + suffix of last line
            let first_prefix: Vec<u8> = self.lines[r1][..c1.min(self.lines[r1].len())].to_vec();
            let last_suffix: Vec<u8> = if r2 < self.lines.len() {
                self.lines[r2][c2.min(self.lines[r2].len())..].to_vec()
            } else {
                Vec::new()
            };
            // Remove lines r1+1..=r2
            let drain_end = (r2 + 1).min(self.lines.len());
            self.lines.drain((r1 + 1)..drain_end);
            // Combine
            self.lines[r1] = first_prefix;
            self.lines[r1].extend_from_slice(&last_suffix);
        }
        self.cursor_row = r1;
        self.cursor_col = c1;
        self.clamp_cursor();
        self.ensure_visible();
        self.modified = true;
    }

    //
    // will try to implement the text editing part
    fn insert_char(&mut self, ch: u8) {
        self.push_undo();
        if self.cursor_col > self.lines[self.cursor_row].len() {
            self.cursor_col = self.lines[self.cursor_row].len();
        }
        self.lines[self.cursor_row].insert(self.cursor_col, ch);
        self.cursor_col += 1;
        self.modified = true;
        self.ensure_visible();
    }

    fn insert_newline(&mut self) {
        self.push_undo();
        let rest = self.lines[self.cursor_row].split_off(self.cursor_col);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, rest);
        self.cursor_col = 0;
        self.modified = true;
        self.ensure_visible();
    }

    fn backspace(&mut self) {
        if self.selection.is_some() {
            self.delete_selection();
            return;
        }
        self.push_undo();
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            self.lines[self.cursor_row].remove(self.cursor_col);
            self.modified = true;
        } else if self.cursor_row > 0 {
            // Merge with previous line
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].extend_from_slice(&current);
            self.modified = true;
        }
        self.ensure_visible();
    }

    fn delete(&mut self) {
        if self.selection.is_some() {
            self.delete_selection();
            return;
        }
        self.push_undo();
        if self.cursor_col < self.lines[self.cursor_row].len() {
            self.lines[self.cursor_row].remove(self.cursor_col);
            self.modified = true;
        } else if self.cursor_row + 1 < self.total_lines() {
            // Merge next line into current
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].extend_from_slice(&next);
            self.modified = true;
        }
    }

    // ok so tab is basically just 4 spaces, nothing fancy
    fn insert_tab(&mut self) {
        for _ in 0..4 {
            self.insert_char(b' ');
        }
    }

    //
    // i will try to implement copy, paste, cut.
    fn copy(&mut self) {
        let text = self.get_selected_text();
        if !text.is_empty() {
            self.clipboard = text;
        }
    }

    fn cut(&mut self) {
        self.copy();
        self.delete_selection();
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        if self.selection.is_some() {
            self.delete_selection();
        }
        self.push_undo();
        let clip = self.clipboard.clone();
        for &byte in &clip {
            if byte == b'\n' {
                let rest = self.lines[self.cursor_row].split_off(self.cursor_col);
                self.cursor_row += 1;
                self.lines.insert(self.cursor_row, rest);
                self.cursor_col = 0;
            } else {
                self.lines[self.cursor_row].insert(self.cursor_col, byte);
                self.cursor_col += 1;
            }
        }
        self.modified = true;
        self.ensure_visible();
    }

    //
    // this would be specific to cursor.
    fn move_left(&mut self, shift: bool) {
        self.start_or_extend_selection(shift);
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
        }
        self.ensure_visible();
        self.update_selection_end();
    }

    fn move_right(&mut self, shift: bool) {
        self.start_or_extend_selection(shift);
        if self.cursor_col < self.line_len(self.cursor_row) {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.total_lines() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
        self.ensure_visible();
        self.update_selection_end();
    }

    fn move_up(&mut self, shift: bool) {
        self.start_or_extend_selection(shift);
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_cursor();
        }
        self.ensure_visible();
        self.update_selection_end();
    }

    fn move_down(&mut self, shift: bool) {
        self.start_or_extend_selection(shift);
        if self.cursor_row + 1 < self.total_lines() {
            self.cursor_row += 1;
            self.clamp_cursor();
        }
        self.ensure_visible();
        self.update_selection_end();
    }

    fn move_home(&mut self, shift: bool) {
        self.start_or_extend_selection(shift);
        self.cursor_col = 0;
        self.ensure_visible();
        self.update_selection_end();
    }

    fn move_end(&mut self, shift: bool) {
        self.start_or_extend_selection(shift);
        self.cursor_col = self.line_len(self.cursor_row);
        self.ensure_visible();
        self.update_selection_end();
    }

    // ── File I/O ─────────────────────────────────────────────────
    // here i will define some FILE I/O operations which we need to do
    // this would be linking with the fs.rs so yeah
    fn save_file(&mut self) {
        let name = match &self.filename {
            Some(n) => n.clone(),
            None => return, // should not happen, prompt sets it first
        };
        // Build file content
        let mut content = Vec::new();
        for (i, line) in self.lines.iter().enumerate() {
            content.extend_from_slice(line);
            if i + 1 < self.lines.len() {
                content.push(b'\n');
            }
        }
        if let Some(fs) = crate::fs::FILESYSTEM.get() {
            fs.lock().write_file(&name, &content);
        }
        self.modified = false;
    }

    fn open_file(&mut self) {
        let name = match &self.filename {
            Some(n) => n.clone(),
            None => return,
        };
        if let Some(fs) = crate::fs::FILESYSTEM.get() {
            if let Some(data) = fs.lock().read_file(&name) {
                self.lines.clear();
                let mut current_line = Vec::new();
                for &byte in &data {
                    if byte == b'\n' {
                        self.lines.push(current_line.clone());
                        current_line.clear();
                    } else {
                        current_line.push(byte);
                    }
                }
                self.lines.push(current_line);
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.scroll_row = 0;
                self.scroll_col = 0;
                self.selection = None;
                self.modified = false;
                self.undo_stack.clear();
                self.redo_stack.clear();
            }
        }
    }

    fn new_file(&mut self) {
        self.push_undo();
        self.lines = vec![Vec::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_row = 0;
        self.scroll_col = 0;
        self.selection = None;
        self.filename = None;
        self.modified = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
    // this is the big one, draws the entire notepad into the window buffer
    fn render_full(&self, window: &mut Window) {
        //  Text area background
        let _ = Rectangle::new(
            Point::new(0, TITLE_BAR_H),
            Size::new(
                NOTEPAD_WIN_W,
                (NOTEPAD_WIN_H as i32 - TITLE_BAR_H - STATUS_BAR_H) as u32,
            ),
        )
        .into_styled(PrimitiveStyle::with_fill(BG_COLOR))
        .draw(window);

        // 2. Gutter background
        let _ = Rectangle::new(
            Point::new(0, TITLE_BAR_H),
            Size::new(
                GUTTER_W as u32,
                (NOTEPAD_WIN_H as i32 - TITLE_BAR_H - STATUS_BAR_H) as u32,
            ),
        )
        .into_styled(PrimitiveStyle::with_fill(GUTTER_BG))
        .draw(window);

        // Render visible lines
        for vr in 0..VISIBLE_ROWS {
            let doc_row = self.scroll_row + vr;
            self.render_line(vr, doc_row, window);
        }
        //  Render cursor
        self.render_cursor(window);

        //  Render title bar (with buttons)
        self.render_title_bar(window);

        // Render status bar
        self.render_status_bar(window);
    }
    // so yeah when i tested out initially it was bull shit.Perf sucked..
    // fast render — only redraws the rows that actually changed + cursor + status bar
    // much faster than render_full, used on every keypress
    fn render_fast(&self, prev_cursor_row: usize, prev_cursor_col: usize, window: &mut Window) {
        let prev_vis = prev_cursor_row as i32 - self.scroll_row as i32;
        let cur_vis = self.cursor_row as i32 - self.scroll_row as i32;

        // always redraw the previous cursor row (erases old cursor + old text)
        if prev_vis >= 0 && prev_vis < VISIBLE_ROWS as i32 {
            self.render_line(prev_vis as usize, prev_cursor_row, window);
        }

        // redraw current cursor row if its a different row
        if cur_vis != prev_vis && cur_vis >= 0 && cur_vis < VISIBLE_ROWS as i32 {
            self.render_line(cur_vis as usize, self.cursor_row, window);
        }

        // draw the cursor
        self.render_cursor(window);

        // update status bar
        self.render_status_bar(window);
    }

    // renders a single line — line number on the left, text on the right
    // also handles selection highlighting per character
    fn render_line(&self, vis_row: usize, doc_row: usize, window: &mut Window) {
        let py = TITLE_BAR_H + (vis_row as i32) * CHAR_H;

        // clear the entire row first (gutter + text area)
        let _ = Rectangle::new(Point::new(0, py), Size::new(NOTEPAD_WIN_W, CHAR_H as u32))
            .into_styled(PrimitiveStyle::with_fill(BG_COLOR))
            .draw(window);

        // gutter bg for this row
        let _ = Rectangle::new(Point::new(0, py), Size::new(GUTTER_W as u32, CHAR_H as u32))
            .into_styled(PrimitiveStyle::with_fill(GUTTER_BG))
            .draw(window);

        // if this row doesnt exist in the document, just leave it blank
        if doc_row >= self.total_lines() {
            return;
        }
        // draw the line number (right-aligned in the gutter)
        let line_num = doc_row + 1;
        let num_str = format_line_num(line_num);
        let style_gutter = MonoTextStyle::new(&FONT_8X13, GUTTER_FG);
        let _ = Text::new(&num_str, Point::new(PAD, py + CHAR_H - 1), style_gutter).draw(window);

        // now draw each character in the line
        let line = &self.lines[doc_row];
        let style_text = MonoTextStyle::new(&FONT_8X13, TEXT_COLOR);
        let style_sel = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);

        for vc in 0..VISIBLE_COLS {
            let doc_col = self.scroll_col + vc;
            if doc_col >= line.len() {
                break;
            }
            let px = TEXT_X + (vc as i32) * CHAR_W;
            let ch = line[doc_col];
            // skip non-printable characters
            if ch < 0x20 || ch > 0x7e {
                continue;
            }

            let in_selection = self.is_in_selection(doc_row, doc_col);
            if in_selection {
                // paint the selection background behind the character
                let _ = Rectangle::new(Point::new(px, py), Size::new(CHAR_W as u32, CHAR_H as u32))
                    .into_styled(PrimitiveStyle::with_fill(SELECT_BG))
                    .draw(window);
            }

            // draw the actual character
            let mut buf = [0u8; 1];
            buf[0] = ch;
            if let Ok(s) = core::str::from_utf8(&buf) {
                let sty = if in_selection { style_sel } else { style_text };
                let _ = Text::new(s, Point::new(px, py + CHAR_H - 1), sty).draw(window);
            }
        }
    }

    // helper to check if a given (row, col) falls inside the current selection
    fn is_in_selection(&self, row: usize, col: usize) -> bool {
        let sel = match &self.selection {
            Some(s) if !s.is_empty() => s,
            _ => return false,
        };
        let (r1, c1, r2, c2) = sel.ordered();
        if row < r1 || row > r2 {
            return false;
        }
        if r1 == r2 {
            return col >= c1 && col < c2;
        }
        if row == r1 {
            return col >= c1;
        }
        if row == r2 {
            return col < c2;
        }
        true // middle rows fully selected
    }

    fn render_cursor(&self, window: &mut Window) {
        if self.mode != Mode::Editing {
            return;
        }
        let vr = self.cursor_row as i32 - self.scroll_row as i32;
        let vc = self.cursor_col as i32 - self.scroll_col as i32;
        if vr < 0 || vr >= VISIBLE_ROWS as i32 || vc < 0 || vc >= VISIBLE_COLS as i32 {
            return;
        }
        let px = TEXT_X + vc * CHAR_W;
        let py = TITLE_BAR_H + vr * CHAR_H;
        // Draw a 2px wide cursor line
        let _ = Rectangle::new(Point::new(px, py), Size::new(2, CHAR_H as u32))
            .into_styled(PrimitiveStyle::with_fill(CURSOR_COLOR))
            .draw(window);
    }

    fn render_title_bar(&self, window: &mut Window) {
        // title bar background
        let _ = Rectangle::new(Point::zero(), Size::new(NOTEPAD_WIN_W, TITLE_BAR_H as u32))
            .into_styled(PrimitiveStyle::with_fill(TITLE_BG))
            .draw(window); // was accidentally Window (capital W) before, fixed it

        // title text — shows filename + modified indicator
        let title = match &self.filename {
            Some(name) => {
                let mut t = String::from("Notepad - ");
                t.push_str(name);
                if self.modified {
                    t.push('*');
                }
                t
            }
            None => {
                let mut t = String::from("Notepad - Untitled");
                if self.modified {
                    t.push('*');
                }
                t
            }
        };
        let style = MonoTextStyle::new(&FONT_8X13, TITLE_FG);
        let _ = Text::new(&title, Point::new(8, 15), style).draw(window);

        // Window control buttons (top-right)
        let btn_size = 12u32;
        let btn_y = 4i32;
        let btn_gap = 4i32;

        // Close button (rightmost)
        let close_x = NOTEPAD_WIN_W as i32 - btn_size as i32 - btn_gap;
        let _ = Rectangle::new(Point::new(close_x, btn_y), Size::new(btn_size, btn_size))
            .into_styled(PrimitiveStyle::with_fill(BTN_CLOSE))
            .draw(window);
        // little x on the close button
        let style_x = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        let _ = Text::new("x", Point::new(close_x + 2, btn_y + 11), style_x).draw(window);

        // Maximize button
        let max_x = close_x - btn_size as i32 - btn_gap;
        let _ = Rectangle::new(Point::new(max_x, btn_y), Size::new(btn_size, btn_size))
            .into_styled(PrimitiveStyle::with_fill(BTN_MAX))
            .draw(window);

        // Minimize button
        let min_x = max_x - btn_size as i32 - btn_gap;
        let _ = Rectangle::new(Point::new(min_x, btn_y), Size::new(btn_size, btn_size))
            .into_styled(PrimitiveStyle::with_fill(BTN_MIN))
            .draw(window);

        // Border under title bar
        let _ = Rectangle::new(Point::new(0, TITLE_BAR_H - 1), Size::new(NOTEPAD_WIN_W, 1))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(80, 80, 80)))
            .draw(window);
    }

    // the bottom bar — shows line/col info when editing, or the filename prompt when saving/opening
    fn render_status_bar(&self, window: &mut Window) {
        let sy = NOTEPAD_WIN_H as i32 - STATUS_BAR_H;

        // status bar background
        let _ = Rectangle::new(
            Point::new(0, sy),
            Size::new(NOTEPAD_WIN_W, STATUS_BAR_H as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(STATUS_BG))
        .draw(window);

        let style = MonoTextStyle::new(&FONT_8X13, STATUS_FG);

        match self.mode {
            Mode::SavePrompt => {
                // when saving, show the prompt in the status bar
                let mut msg = String::from("Save as: ");
                msg.push_str(&self.prompt_buf);
                msg.push('_'); // fake blinking cursor lol
                let _ = Text::new(&msg, Point::new(8, sy + 14), style).draw(window);
            }
            Mode::OpenPrompt => {
                // same thing but for opening files
                let mut msg = String::from("Open file: ");
                msg.push_str(&self.prompt_buf);
                msg.push('_');
                let _ = Text::new(&msg, Point::new(8, sy + 14), style).draw(window);
            }
            Mode::Editing => {
                // normal mode — show line number, column, and total lines on the left
                let info =
                    format_status(self.cursor_row + 1, self.cursor_col + 1, self.total_lines());
                let _ = Text::new(&info, Point::new(8, sy + 14), style).draw(window);

                // some helpful shortcuts on the right side
                let hint = "Ctrl+S:Save Ctrl+O:Open Esc:Close";
                let hint_x = NOTEPAD_WIN_W as i32 - (hint.len() as i32) * CHAR_W - 8;
                let _ = Text::new(hint, Point::new(hint_x.max(200), sy + 14), style).draw(window);
            }
        }
    }
}

// just pads the number so it looks nice in the gutter
fn format_line_num(n: usize) -> String {
    if n < 10 {
        alloc::format!("   {}", n)
    } else if n < 100 {
        alloc::format!("  {}", n)
    } else if n < 1000 {
        alloc::format!(" {}", n)
    } else {
        alloc::format!("{}", n)
    }
}

// ─── Helper: format status bar info ─────────────────────────────
fn format_status(row: usize, col: usize, total: usize) -> String {
    alloc::format!("Ln {}, Col {} | {} lines", row, col, total)
}
//i can call API here so yeah linking all together..
pub static NOTEPAD_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static NOTEPAD_STATE: Mutex<NotepadState> = Mutex::new(NotepadState::new());
pub static NOTEPAD_WINDOW: Mutex<Option<Window>> = Mutex::new(None);

/// Opens the notepad (called from cli.rs when user types "notepad")
pub fn open_notepad(bpp: usize) {
    if NOTEPAD_ACTIVE.load(Ordering::Relaxed) {
        return; // already open, dont open another one
    }

    let mut state = NOTEPAD_STATE.lock();
    state.init();

    let mut win = Window::new(
        NOTEPAD_WIN_X,
        NOTEPAD_WIN_Y,
        NOTEPAD_WIN_W,
        NOTEPAD_WIN_H,
        "Notepad",
        BG_COLOR,
        bpp,
    );

    // render the full notepad into the window buffer
    state.render_full(&mut win);

    *NOTEPAD_WINDOW.lock() = Some(win);
    NOTEPAD_ACTIVE.store(true, Ordering::Relaxed);

    // tell the compositor to paint this area
    report_damage(Rect::new(
        NOTEPAD_WIN_X,
        NOTEPAD_WIN_Y,
        NOTEPAD_WIN_W as i32,
        NOTEPAD_WIN_H as i32,
    ));

    if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
        waker.wake();
    }
}

// Opens the notepad with a file already loaded i mean something like notepad jvk.txt
pub fn open_notepad_with_file(bpp: usize, filename: &str) {
    if NOTEPAD_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let mut state = NOTEPAD_STATE.lock();
    state.init();
    state.filename = Some(String::from(filename));
    state.open_file();

    let mut win = Window::new(
        NOTEPAD_WIN_X,
        NOTEPAD_WIN_Y,
        NOTEPAD_WIN_W,
        NOTEPAD_WIN_H,
        "Notepad",
        BG_COLOR,
        bpp,
    );

    state.render_full(&mut win);

    *NOTEPAD_WINDOW.lock() = Some(win);
    NOTEPAD_ACTIVE.store(true, Ordering::Relaxed);

    report_damage(Rect::new(
        NOTEPAD_WIN_X,
        NOTEPAD_WIN_Y,
        NOTEPAD_WIN_W as i32,
        NOTEPAD_WIN_H as i32,
    ));

    if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
        waker.wake();
    }
}

//Closes the notepad — clears the window and damages the area so the compositor repaints the background
pub fn close_notepad() {
    NOTEPAD_ACTIVE.store(false, Ordering::Relaxed);

    // drop the window
    *NOTEPAD_WINDOW.lock() = None;

    // damage the area to repaint background (teal will show through)
    report_damage(Rect::new(
        NOTEPAD_WIN_X,
        NOTEPAD_WIN_Y,
        NOTEPAD_WIN_W as i32,
        NOTEPAD_WIN_H as i32,
    ));

    if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
        waker.wake();
    }
}

//quick check if notepad is currently open
pub fn is_active() -> bool {
    NOTEPAD_ACTIVE.load(Ordering::Relaxed)
}

//his is the main entry point for keyboard input when notepad is active
// cli.rs calls this instead of processing keys itself
pub fn handle_key(key: DecodedKey, ctrl: bool, shift: bool) {
    if !is_active() {
        return;
    }
    let mut state = NOTEPAD_STATE.lock();

    let prev_row = state.cursor_row;
    let prev_col = state.cursor_col;
    let prev_scroll = state.scroll_row;
    let prev_lines = state.total_lines(); // track if lines were added/removed

    match state.mode {
        Mode::SavePrompt => {
            handle_prompt_key(&mut state, key, Mode::SavePrompt);
        }
        Mode::OpenPrompt => {
            handle_prompt_key(&mut state, key, Mode::OpenPrompt);
        }
        Mode::Editing => {
            handle_editing_key(&mut state, key, ctrl, shift);
        }
    }

    let mut win_guard = NOTEPAD_WINDOW.lock();
    if let Some(window) = win_guard.as_mut() {
        // full redraw if scroll changed, lines added/removed, undo/redo, paste, etc
        if state.scroll_row != prev_scroll || state.total_lines() != prev_lines {
            state.render_full(window);
            // this is a fucking huge issue, i will redraw  the thing again here
            report_damage(Rect::new(
                NOTEPAD_WIN_X,
                NOTEPAD_WIN_Y,
                NOTEPAD_WIN_W as i32,
                NOTEPAD_WIN_H as i32,
            ));
        } else {
            state.render_fast(prev_row, prev_col, window);
            let row1 = prev_row as i32 - state.scroll_row as i32;
            let row2 = state.cursor_row as i32 - state.scroll_row as i32;
            let min_vr = row1.min(row2).max(0);
            let max_vr = row1.max(row2).min(VISIBLE_ROWS as i32 - 1);

            let y_start = NOTEPAD_WIN_Y + TITLE_BAR_H + min_vr * CHAR_H;
            let y_end = NOTEPAD_WIN_Y + TITLE_BAR_H + (max_vr + 1) * CHAR_H;
            let h = y_end - y_start;
            report_damage(Rect::new(NOTEPAD_WIN_X, y_start, NOTEPAD_WIN_W as i32, h));
            report_damage(Rect::new(
                NOTEPAD_WIN_X,
                NOTEPAD_WIN_Y,
                NOTEPAD_WIN_W as i32,
                NOTEPAD_WIN_H as i32,
            ));
        }
    }
    drop(win_guard);
    drop(state);
    if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
        waker.wake();
    }
}
// handles keyboard input during normal editing mode
// ctrl combos for save/open/undo/redo/copy/cut/paste, regular typing, arrow keys etc
fn handle_editing_key(state: &mut NotepadState, key: DecodedKey, ctrl: bool, shift: bool) {
    match key {
        DecodedKey::Unicode(ch) => {
            if ctrl {
                // ctrl + key shortcuts
                match ch {
                    's' | 'S' => {
                        if state.filename.is_some() {
                            state.save_file();
                        } else {
                            // no filename yet, ask for one
                            state.mode = Mode::SavePrompt;
                            state.prompt_buf.clear();
                        }
                    }
                    'o' | 'O' => {
                        state.mode = Mode::OpenPrompt;
                        state.prompt_buf.clear();
                    }
                    'n' | 'N' => {
                        state.new_file();
                    }
                    'z' | 'Z' => {
                        state.undo();
                    }
                    'y' | 'Y' => {
                        state.redo();
                    }
                    'a' | 'A' => {
                        state.select_all();
                    }
                    'c' | 'C' => {
                        state.copy();
                    }
                    'x' | 'X' => {
                        state.cut();
                    }
                    'v' | 'V' => {
                        state.paste();
                    }
                    _ => {}
                }
            } else {
                // normal key presses
                match ch {
                    '\n' => {
                        if state.selection.is_some() {
                            state.delete_selection();
                        }
                        state.insert_newline();
                    }
                    '\x08' => {
                        state.backspace();
                    }
                    '\t' => {
                        if state.selection.is_some() {
                            state.delete_selection();
                        }
                        state.insert_tab();
                    }
                    '\x1B' => {
                        // Escape — handled in cli.rs (close notepad)
                    }
                    c if (c as u8) >= 0x20 && (c as u8) <= 0x7e => {
                        if state.selection.is_some() {
                            state.delete_selection();
                        }
                        state.insert_char(c as u8);
                    }
                    _ => {}
                }
            }
        }
        DecodedKey::RawKey(key_code) => match key_code {
            KeyCode::ArrowLeft => state.move_left(shift),
            KeyCode::ArrowRight => state.move_right(shift),
            KeyCode::ArrowUp => state.move_up(shift),
            KeyCode::ArrowDown => state.move_down(shift),
            KeyCode::Home => state.move_home(shift),
            KeyCode::End => state.move_end(shift),
            KeyCode::Delete => state.delete(),
            KeyCode::Escape => {
                // handled in cli.rs to avoid lock issues
            }
            _ => {}
        },
    }
}

// handles keyboard input when we're in the save/open filename prompt
// basically just a mini text input in the status bar
fn handle_prompt_key(state: &mut NotepadState, key: DecodedKey, mode: Mode) {
    match key {
        DecodedKey::Unicode(ch) => match ch {
            '\n' => {
                // user pressed enter — confirm the filename
                let name = state.prompt_buf.clone();
                if !name.is_empty() {
                    state.filename = Some(name);
                    match mode {
                        Mode::SavePrompt => state.save_file(),
                        Mode::OpenPrompt => state.open_file(),
                        _ => {}
                    }
                }
                state.mode = Mode::Editing;
                state.prompt_buf.clear();
            }
            '\x08' => {
                // backspace in the prompt
                state.prompt_buf.pop();
            }
            '\x1B' => {
                // escape — cancel the prompt and go back to editing
                state.mode = Mode::Editing;
                state.prompt_buf.clear();
            }
            c if (c as u8) >= 0x20 && (c as u8) <= 0x7e => {
                state.prompt_buf.push(c);
            }
            _ => {}
        },
        DecodedKey::RawKey(KeyCode::Escape) => {
            state.mode = Mode::Editing;
            state.prompt_buf.clear();
        }
        _ => {}
    }
}

pub fn get_bpp() -> usize {
    /*let win_guard = super::terminal::TERMINAL_WINDOW.lock();
    if let Some(window) = win_guard.as_ref() {
        window.bpp
    } else {
        3 // default fallback, since, we three sincked to 3 so yeah it should work !!
    }*/
    3
}
