// src/gui/paint.rs
//
// GUI Paint Application for KafkaOS — Compositor-Compatible
// A basic drawing canvas with color palette and mouse-driven painting.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use spin::Mutex;

use super::geometry::Rect;
use super::graphics::{COMPOSITOR_WAKER, report_damage};
use super::window::Window;

// ── Window layout constants ─────────────────────────────────────────
pub const PAINT_WIN_X: i32 = 120;
pub const PAINT_WIN_Y: i32 = 60;
pub const PAINT_WIN_W: u32 = 270; // SET WINDOW WIDTH HERE
pub const PAINT_WIN_H: u32 = 310; // SET WINDOW HEIGHT HERE

const TITLE_BAR_H: i32 = 20;
const PALETTE_BAR_H: i32 = 20;

/// Canvas dimensions (pixels). Sits below the title bar + palette bar.
const CANVAS_W: usize = PAINT_WIN_W as usize; // 270
const CANVAS_H: usize = PAINT_WIN_H as usize - TITLE_BAR_H as usize - PALETTE_BAR_H as usize; // 310 - 40 = 270 yeah i need to calculate ts

/// The Y offset where the canvas starts inside the window.
const CANVAS_Y_OFFSET: i32 = TITLE_BAR_H + PALETTE_BAR_H;

// ── Palette colours ─────────────────────────────────────────────────
const PALETTE: &[Rgb888] = &[
    Rgb888::BLACK,
    Rgb888::WHITE,
    Rgb888::new(255, 0, 0),     // Red
    Rgb888::new(0, 200, 0),     // Green
    Rgb888::new(0, 100, 255),   // Blue
    Rgb888::new(255, 255, 0),   // Yellow
    Rgb888::new(255, 128, 0),   // Orange
    Rgb888::new(160, 0, 210),   // Purple
    Rgb888::new(0, 200, 200),   // Cyan
    Rgb888::new(255, 105, 180), // Pink
];

/// Width of each colour swatch in the palette bar.
const SWATCH_W: i32 = (PAINT_WIN_W as i32) / (PALETTE.len() as i32);

// ── Canvas pixel buffer ─────────────────────────────────────────────
// A full 2-D array would be huge on the stack, so we use a flat buffer
// addressed as  [y * CANVAS_W + x].
// Because `const fn` in `no_std` doesn't allow heap allocation we
// initialise via a const array (every pixel starts as white).

const CANVAS_SIZE: usize = CANVAS_W * CANVAS_H;

/// Compact representation: each pixel is stored as (R, G, B).
#[derive(Copy, Clone, PartialEq)]
struct CanvasPixel {
    r: u8,
    g: u8,
    b: u8,
}

impl CanvasPixel {
    const fn white() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
        }
    }

    fn from_rgb888(c: Rgb888) -> Self {
        Self {
            r: c.r(),
            g: c.g(),
            b: c.b(),
        }
    }

    const fn to_rgb888(self) -> Rgb888 {
        Rgb888::new(self.r, self.g, self.b)
    }
}

// ── Paint ap3plication state ─────────────────────────────────────────
pub struct PaintApp {
    //canvas: [CanvasPixel; CANVAS_SIZE],
    canvas: alloc::vec::Vec<CanvasPixel>,
    brush_color: CanvasPixel,
    brush_radius: i32, // 0 = single pixel, 1 = 3×3, 2 = 5×5 …
    needs_full_redraw: bool,
}

impl PaintApp {
    pub fn new() -> Self {
        Self {
            //canvas: [CanvasPixel::white(); CANVAS_SIZE],
            canvas: alloc::vec![CanvasPixel::white(); CANVAS_SIZE],
            brush_color: CanvasPixel::from_rgb888(Rgb888::BLACK),
            brush_radius: 1,
            needs_full_redraw: true,
        }
    }

    // ── Public API ──────────────────────────────────────────────────

    /// Set the brush colour by palette index.
    pub fn set_color(&mut self, idx: usize) {
        if idx < PALETTE.len() {
            self.brush_color = CanvasPixel::from_rgb888(PALETTE[idx]);
        }
    }

    /// Set the brush radius (0 = 1px, 1 = 3px, 2 = 5px …).
    pub fn set_radius(&mut self, r: i32) {
        self.brush_radius = r;
    }

    /// Paint at canvas-local coordinates (cx, cy) with the current brush.
    pub fn paint(&mut self, cx: i32, cy: i32) {
        let r = self.brush_radius;
        for dy in -r..=r {
            for dx in -r..=r {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && (px as usize) < CANVAS_W && (py as usize) < CANVAS_H {
                    self.canvas[py as usize * CANVAS_W + px as usize] = self.brush_color;
                }
            }
        }
    }

    /// Clear the entire canvas back to white.
    pub fn clear(&mut self) {
        self.canvas = [CanvasPixel::white(); CANVAS_SIZE].to_vec();
        self.needs_full_redraw = true;
    }

    // ── Rendering ───────────────────────────────────────────────────

    /// Render the palette bar into the window.
    fn render_palette(&self, window: &mut Window) {
        for (i, &color) in PALETTE.iter().enumerate() {
            let x = i as i32 * SWATCH_W;
            let _ = Rectangle::new(
                Point::new(x, TITLE_BAR_H),
                Size::new(SWATCH_W as u32, PALETTE_BAR_H as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(window);

            // Draw a thin border around the active swatch so the user
            // can see which colour is selected.
            if CanvasPixel::from_rgb888(color) == self.brush_color {
                let _ = Rectangle::new(
                    Point::new(x, TITLE_BAR_H),
                    Size::new(SWATCH_W as u32, PALETTE_BAR_H as u32),
                )
                .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 2))
                .draw(window);
            }
        }
    }

    /// Render the full canvas into the window buffer. Called when
    /// `needs_full_redraw` is set (initial draw, clear, etc.).
    fn render_full_canvas(&self, window: &mut Window) {
        for y in 0..CANVAS_H {
            for x in 0..CANVAS_W {
                let px = self.canvas[y * CANVAS_W + x];
                let _ = Rectangle::new(
                    Point::new(x as i32, CANVAS_Y_OFFSET + y as i32),
                    Size::new(1, 1),
                )
                .into_styled(PrimitiveStyle::with_fill(px.to_rgb888()))
                .draw(window);
            }
        }
    }

    /// Render a small patch of the canvas around (cx, cy), used after a
    /// paint stroke so we don't have to redraw the entire canvas.
    fn render_patch(&self, cx: i32, cy: i32, window: &mut Window) {
        let r = self.brush_radius;
        for dy in -r..=r {
            for dx in -r..=r {
                let px = cx + dx;
                let py = cy + dy;
                if px >= 0 && py >= 0 && (px as usize) < CANVAS_W && (py as usize) < CANVAS_H {
                    let pixel = self.canvas[py as usize * CANVAS_W + px as usize];
                    let _ = Rectangle::new(Point::new(px, CANVAS_Y_OFFSET + py), Size::new(1, 1))
                        .into_styled(PrimitiveStyle::with_fill(pixel.to_rgb888()))
                        .draw(window);
                }
            }
        }
    }

    /// Main render entry point. Decides between full and incremental draw.
    pub fn render_into_window(&mut self, window: &mut Window) {
        self.render_palette(window);

        if self.needs_full_redraw {
            self.render_full_canvas(window);
            self.needs_full_redraw = false;
        }
        // When painting incrementally the caller should use render_patch
        // right after paint() for best performance.
    }
}

// ── Global statics ──────────────────────────────────────────────────
//pub static PAINT_APP: Mutex<PaintApp> = Mutex::new(PaintApp::new());
pub static PAINT_APP: Mutex<Option<PaintApp>> = Mutex::new(None);
pub static PAINT_WINDOW: Mutex<Option<Window>> = Mutex::new(None);

// ── Initialisation ──────────────────────────────────────────────────

/// Create the paint window and render its initial chrome.
/// Call this from `setup_desktop()` in graphics.rs.
pub fn init_paint(bpp: usize) {
    let mut win = Window::new(
        PAINT_WIN_X,
        PAINT_WIN_Y,
        PAINT_WIN_W,
        PAINT_WIN_H,
        "Paint",
        Rgb888::WHITE,
        bpp,
    );
    win.render_internal_graphics();

    // Draw the initial palette + blank canvas into the window buffer
    // {
    //     let mut app = PAINT_APP.lock();
    //     app.render_into_window(&mut win);
    // }
    {
    let mut app_guard = PAINT_APP.lock();
    *app_guard = Some(PaintApp::new());
    let app = app_guard.as_mut().unwrap();
    app.render_into_window(&mut win);
}

    *PAINT_WINDOW.lock() = Some(win);
}

// ── Mouse interaction helpers ───────────────────────────────────────

/// Call this from mouse event handling code when the left button is
/// held down and the cursor is over the paint window.
///
/// `screen_x` / `screen_y` are absolute screen coordinates.
pub fn handle_paint_click(screen_x: i32, screen_y: i32) {
    // Convert screen coords → window-local coords
    let local_x = screen_x - PAINT_WIN_X;
    let local_y = screen_y - PAINT_WIN_Y;

    // Check if click is in the palette bar
    if local_y >= TITLE_BAR_H
        && local_y < CANVAS_Y_OFFSET
        && local_x >= 0
        && local_x < PAINT_WIN_W as i32
    {
        let idx = (local_x / SWATCH_W) as usize;
        x86_64::instructions::interrupts::without_interrupts(|| {
            // let mut app = PAINT_APP.lock();
            let mut guard = PAINT_APP.lock();
            let app = guard.as_mut().unwrap();
            app.set_color(idx);

            // Re-render palette to update the selection indicator
            let mut win_guard = PAINT_WINDOW.lock();
            if let Some(window) = win_guard.as_mut() {
                app.render_palette(window);
            }
            drop(win_guard);
            drop(app);

            report_damage(Rect::new(
                PAINT_WIN_X,
                PAINT_WIN_Y + TITLE_BAR_H,
                PAINT_WIN_W as i32,
                PALETTE_BAR_H,
            ));

            if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
                waker.wake();
            }
        });
        return;
    }

    // Check if click is on the canvas
    let canvas_x = local_x;
    let canvas_y = local_y - CANVAS_Y_OFFSET;

    if canvas_x >= 0 && canvas_x < CANVAS_W as i32 && canvas_y >= 0 && canvas_y < CANVAS_H as i32 {
        x86_64::instructions::interrupts::without_interrupts(|| {
            //let mut app = PAINT_APP.lock();
            let mut guard = PAINT_APP.lock();
            let app = guard.as_mut().unwrap();
            app.paint(canvas_x, canvas_y);
            let r = app.brush_radius + 1;

            let mut win_guard = PAINT_WINDOW.lock();
            if let Some(window) = win_guard.as_mut() {
                app.render_patch(canvas_x, canvas_y, window);
            }
            drop(win_guard);
            drop(app);

            // Only damage the brush-sized area that changed
            report_damage(Rect::new(
                PAINT_WIN_X + canvas_x - r,
                PAINT_WIN_Y + CANVAS_Y_OFFSET + canvas_y - r,
                r * 2 + 1,
                r * 2 + 1,
            ));

            if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
                waker.wake();
            }
        });
    }
}

/// Clear the paint canvas and trigger a full redraw.
pub fn clear_paint() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        //let mut app = PAINT_APP.lock();
        let mut guard = PAINT_APP.lock();
        let app = guard.as_mut().unwrap();
        app.clear();

        let mut win_guard = PAINT_WINDOW.lock();
        if let Some(window) = win_guard.as_mut() {
            app.render_into_window(window);
        }
        drop(win_guard);
        drop(app);

        report_damage(Rect::new(
            PAINT_WIN_X,
            PAINT_WIN_Y,
            PAINT_WIN_W as i32,
            PAINT_WIN_H as i32,
        ));

        if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }
    });
}

/// Returns `true` if the given screen coordinate falls within the
/// paint window's bounds (useful for hit-testing in mouse handler).
pub fn point_in_paint_window(screen_x: i32, screen_y: i32) -> bool {
    screen_x >= PAINT_WIN_X
        && screen_x < PAINT_WIN_X + PAINT_WIN_W as i32
        && screen_y >= PAINT_WIN_Y
        && screen_y < PAINT_WIN_Y + PAINT_WIN_H as i32
}
