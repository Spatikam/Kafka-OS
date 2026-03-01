use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Triangle, Rectangle, Circle, PrimitiveStyle, PrimitiveStyleBuilder, Line, Polyline},
};
use crate::task::mouse::MousePacket;
use crate::task::mouse::MouseStream; 
use futures_util::stream::StreamExt;
use super::window::{Window, WindowManager};
use crate::gui::buffer::FrameBufferDisplay;

use spin::Mutex;
use alloc::vec::Vec;

use crate::gui::geometry::Rect; //geometry module

use core::sync::atomic::{AtomicI32, Ordering};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

// Global atomic coordinates that the Compositor will read
pub static MOUSE_X: AtomicI32 = AtomicI32::new(0);
pub static MOUSE_Y: AtomicI32 = AtomicI32::new(0);

// Assuming DAMAGE_QUEUE is accessible here
pub static DAMAGE_QUEUE: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

/// Any task can call this when it changes pixels on the screen
pub fn report_damage(rect: Rect) {
    DAMAGE_QUEUE.lock().push(rect);
}

// The global alarm clock for the Compositor
pub static COMPOSITOR_WAKER: spin::Mutex<Option<Waker>> = spin::Mutex::new(None);

pub struct WaitForDamage;

impl Future for WaitForDamage {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let queue = DAMAGE_QUEUE.lock(); // Adjust crate path as needed
        
        if queue.is_empty() {
            // The queue is empty. Register our alarm clock and go to sleep!
            *COMPOSITOR_WAKER.lock() = Some(cx.waker().clone());
            Poll::Pending
        } else {
            // There is damage! Wake up immediately.
            Poll::Ready(())
        }
    }
}

pub async fn compositor_task(display: &mut FrameBufferDisplay, wm: WindowManager) {
    loop {
        // 1.
        WaitForDamage.await;

        // 2. Extract all damage rects and instantly unlock the queue
        // so other tasks aren't blocked from reporting new damage.
        let damage_rects: Vec<Rect> = {
            let mut queue = DAMAGE_QUEUE.lock();
            let rects = queue.clone();
            queue.clear();
            rects
        };

        // 3. Process every damaged rectangle
        for damage in damage_rects {

            // ── FLICKER FIX ─────────────────────────────────────────
            // Check if the damage rect is fully inside any window.
            // If so, skip the background fill — the window blit will
            // fully cover it, so we avoid the teal flash (flicker).
            let mut fully_covered = false;

            // Check against regular windows (Status, etc.)
            for window in &wm.windows {
                let win_rect = Rect::new(
                    window.x, window.y,
                    window.width as i32, window.height as i32,
                );
                if win_rect.contains_rect(&damage) {
                    fully_covered = true;
                    break;
                }
            }

            // Check against the Terminal window
            if !fully_covered {
                let win_guard = crate::gui::terminal::TERMINAL_WINDOW.lock();
                if let Some(window) = win_guard.as_ref() {
                    let win_rect = Rect::new(
                        window.x, window.y,
                        window.width as i32, window.height as i32,
                    );
                    if win_rect.contains_rect(&damage) {
                        fully_covered = true;
                    }
                }
            }
            if !fully_covered{
                let win_guard = crate::gui::notepad::NOTEPAD_WINDOW.lock();
                if let Some(window) = win_guard.as_ref(){
                    let win_rect = Rect::new(
                        window.x,window.y, window.width as i32, window.height as i32,
                    );
                    if win_rect.contains_rect(&damage){
                        fully_covered = true;
                    }
                }
            }
            if !fully_covered {
                display.fill_rect(&damage, Rgb888::new(0, 128, 128)); // A nice teal background
            }
            for window in &wm.windows {
                let win_rect = Rect::new(
                    window.x, window.y, 
                    window.width as i32, window.height as i32
                );
                // have to eliminate the area so yeah this should do 
                if let Some(overlap) = damage.intersection(&win_rect) {
                    display.blit_partial(&overlap, window);
                }
            }
            {
                let win_guard = crate::gui::terminal::TERMINAL_WINDOW.lock();
                if let Some(window) = win_guard.as_ref() {
                    let win_rect = Rect::new(
                        window.x, window.y,
                        window.width as i32, window.height as i32,
                    );
                    if let Some(overlap) = damage.intersection(&win_rect) {
                        // Subtract the notepad area — only blit visible parts
                        if crate::gui::notepad::is_active() {
                            let np_guard = crate::gui::notepad::NOTEPAD_WINDOW.lock();
                            let visible_parts = if let Some(np) = np_guard.as_ref() {
                                let np_rect = Rect::new(
                                    np.x, np.y,
                                    np.width as i32, np.height as i32,
                                );
                                overlap.subtract(&np_rect)
                            } else {
                                alloc::vec![overlap]
                            };
                            drop(np_guard);
                            for part in visible_parts {
                                display.blit_partial(&part, window);
                            }
                        } else {
                            display.blit_partial(&overlap, window);
                        }
                    }
                }
            }
            // Notepad window (on top of terminal)  this is just for now
            // I guess we have to change this when we bring the dynammic window sizing.
            {
                let win_guard = crate::gui::notepad::NOTEPAD_WINDOW.lock();
                if let Some(window) = win_guard.as_ref() {
                    let win_rect = Rect::new(
                        window.x, window.y,
                        window.width as i32, window.height as i32,
                    );
                    if let Some(overlap) = damage.intersection(&win_rect) {
                        display.blit_partial(&overlap, window);
                    }
                }
            }


            // LAYER 3: Draw the Mouse Cursor on top
            // (Assuming you have global atomic variables for mouse coordinates)
            let mouse_x = MOUSE_X.load(core::sync::atomic::Ordering::Relaxed) as i32;
            let mouse_y = MOUSE_Y.load(core::sync::atomic::Ordering::Relaxed) as i32;
            let mouse_rect = Rect::new(mouse_x, mouse_y, 17, 21); // Your cursor dimensions

            if let Some(_overlap) = damage.intersection(&mouse_rect) {
                // The mouse is caught in the damage zone, so we must redraw it
                draw_cursor(display, mouse_x, mouse_y).ok();
            }
        }
    }
}

pub fn setup_desktop(screen_width: i32, screen_height: i32, bpp: usize) -> WindowManager {

    let mut wm = WindowManager::new(screen_width as u32, screen_height as u32);
    super::terminal::init_terminal(bpp);
    let mut status = Window::new(550, 50, 200, 150, "Status", Rgb888::BLUE, bpp);
    status.render_internal_graphics();
    wm.add_window(status);
    report_damage(Rect::new(0, 0, screen_width, screen_height));

    wm
}


pub async fn activate_mouse(screen_width: i32, screen_height: i32) {
    let mut mouse_stream = MouseStream::new(); 

    // 1. Initialize Starting Position
    let mut cursor_x: i32 = screen_width/2;
    let mut cursor_y: i32 = screen_height/2;

    MOUSE_X.store(cursor_x, Ordering::Relaxed);
    MOUSE_Y.store(cursor_y, Ordering::Relaxed);

    const SENSITIVITY: i32 = 1;
    //const BPP: usize = 4; // Bytes per pixel of the display
    const CURSOR_WIDTH: usize = 17;
    const CURSOR_HEIGHT: usize = 21;

    //let mut cursor_bg = [[Rgb888::new(0, 128, 128); 10]; 10];
    //let mut saved_bg: [u8; CURSOR_WIDTH * CURSOR_HEIGHT * BPP] = [0; CURSOR_WIDTH * CURSOR_HEIGHT * BPP];

    //display.save_patch(cursor_x as usize, cursor_y as usize, CURSOR_WIDTH, CURSOR_HEIGHT, &mut saved_bg);
    
    // 2. Report initial damage so the cursor draws on the very first frame
    {
        let mut queue = DAMAGE_QUEUE.lock();
        queue.push(Rect::new(cursor_x, cursor_y, CURSOR_WIDTH as i32, CURSOR_HEIGHT as i32));
    }
    
    while let Some(packet) = mouse_stream.next().await {
        //display.restore_patch(cursor_x as usize, cursor_y as usize, CURSOR_WIDTH, CURSOR_HEIGHT, &saved_bg);

        // 3. Create a damage box for the OLD position (tells Compositor to erase it)
        let old_rect = Rect::new(cursor_x, cursor_y, CURSOR_WIDTH as i32, CURSOR_HEIGHT as i32);

        // 4. Calculate new coordinates
        cursor_x = (cursor_x + ((packet.x as i32) / SENSITIVITY)).clamp(0, screen_width - CURSOR_WIDTH as i32);
        cursor_y = (cursor_y + ((packet.y as i32) / SENSITIVITY)).clamp(0, screen_height - CURSOR_HEIGHT as i32); 
        
        // 5. Update the global atomic coordinates for the Compositor
        MOUSE_X.store(cursor_x, Ordering::Relaxed);
        MOUSE_Y.store(cursor_y, Ordering::Relaxed);

        // 6. Create a damage box for the NEW position (tells Compositor to draw it)
        let new_rect = Rect::new(cursor_x, cursor_y, CURSOR_WIDTH as i32, CURSOR_HEIGHT as i32);

        // 7. Push both boxes to the damage queue
        {
            let mut queue = DAMAGE_QUEUE.lock();
            queue.push(old_rect);
            queue.push(new_rect);
        }

        // 8. Wake up the Compositor!
        if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
            waker.wake();
        }

        //display.save_patch(cursor_x as usize, cursor_y as usize, CURSOR_WIDTH, CURSOR_HEIGHT, &mut saved_bg);
        //draw_cursor(display, cursor_x, cursor_y).ok();
    }
}

pub fn draw_cursor<D>(target: &mut D, x: i32, y: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb888>,
{
    Triangle::new(Point::new(x, y), Point::new(x + 6, y + 12), Point::new(x, y + 20))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::BLACK))
        .draw(target)?;
    Triangle::new(Point::new(x, y), Point::new(x + 6, y + 12), Point::new(x + 16, y + 12))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::BLACK))
        .draw(target)?;
    let points: [Point; 5] = [
        Point::new(x, y),
        Point::new(x, y + 20),
        Point::new(x + 6, y + 12),
        Point::new(x + 16, y + 12),
        Point::new(x, y),
    ];
    Polyline::new(&points)
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 1))
        .draw(target)?;
    Ok(())
}