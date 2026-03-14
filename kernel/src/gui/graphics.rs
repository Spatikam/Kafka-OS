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
use super::window::{Window, WindowManager, AppState};
use crate::gui::buffer::FrameBufferDisplay;
use super::taskbar::{Taskbar, TaskbarAction};

use spin::Mutex;
use alloc::vec::Vec;

use crate::gui::geometry::Rect; //geometry module

use core::sync::atomic::{AtomicI32, Ordering, AtomicUsize, AtomicBool};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

// Global atomic coordinates that the Compositor will read
pub static MOUSE_X: AtomicI32 = AtomicI32::new(0);
pub static MOUSE_Y: AtomicI32 = AtomicI32::new(0);

// Assuming DAMAGE_QUEUE is accessible here
pub static DAMAGE_QUEUE: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

// We store raw global (x, y) physical screen coordinates here
//pub static CLICK_QUEUE: spin::Mutex<alloc::vec::Vec<(i32, i32)>> = spin::Mutex::new(alloc::vec::Vec::new());
pub static MOUSE_EVENTS: spin::Mutex<alloc::vec::Vec<RawMouse>> = spin::Mutex::new(alloc::vec::Vec::new());


/// Any task can call this when it changes pixels on the screen
pub fn report_damage(rect: Rect) {
    DAMAGE_QUEUE.lock().push(rect);
}

// The global alarm clock for the Compositor
pub static COMPOSITOR_WAKER: spin::Mutex<Option<Waker>> = spin::Mutex::new(None);

// The PIT fires about 18.2 times per second by default on x86
pub static PIT_TICKS: AtomicUsize = AtomicUsize::new(0);

// A flag to tell the Compositor "A second has passed, update the clock!"
pub static CLOCK_TICK: AtomicBool = AtomicBool::new(false);

// For Opening Apps and Using them
#[derive(Clone, Copy)]
pub enum AppRequest {
    Files,
    Terminal,
    Paint,
    Calculator,
    Snake,
}

pub static APP_REQUESTS: spin::Mutex<alloc::vec::Vec<AppRequest>> = spin::Mutex::new(alloc::vec::Vec::new());

pub struct WaitForDamage;

impl Future for WaitForDamage {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let queue = DAMAGE_QUEUE.lock(); // Adjust crate path as needed
        let clock_ticked = CLOCK_TICK.load(Ordering::Relaxed);
        let term_dirty = super::terminal::GUI_TERMINAL.lock().needs_redraw;   //random name for some reason.. 
        if queue.is_empty() && !clock_ticked && !term_dirty {
            // The queue is empty. Register our alarm clock and go to sleep!
            *COMPOSITOR_WAKER.lock() = Some(cx.waker().clone());
            Poll::Pending
        } else {
            // There is damage! Wake up immediately.
            Poll::Ready(())
        }
    }
}

pub async fn compositor_task(display: &mut FrameBufferDisplay, mut wm: WindowManager, mut taskbar: Taskbar, screen_width: i32, screen_height: i32) {
    loop {
        // 1.
        WaitForDamage.await;
        if CLOCK_TICK.swap(false, Ordering::Relaxed) {
            // This recalculates the RTC time and automatically pushes 
            // the new graphic to the DAMAGE_QUEUE!
            taskbar.tick();
        }

        // Removes closed Windows from display
        wm.windows.retain(|window| {
            if window.close_btn {
                // Before it dies, report its entire physical footprint as damaged!
                report_damage(Rect::new(
                    window.x, window.y, 
                    window.width as i32, window.height as i32
                ));
                
                // Return false to permanently delete it from the Vector and free the RAM
                false 
            } else {
                // Keep the window alive
                true 
            }
        });

        // --- APP LAUNCH PHASE ---
        let requests: Vec<AppRequest> = {
            let mut q = APP_REQUESTS.lock();
            let reqs = q.clone();
            q.clear();
            reqs
        };

        for req in requests {
            match req {
                AppRequest::Files => {
                    // Spawn a 400x300 window in the center of the screen
                    let mut file_win = Window::with_state(
                        100, 100, 400, 300, 
                        "Files", Rgb888::WHITE, taskbar.bpp,
                        AppState::FileExplorer {
                            current_path: alloc::string::String::new(),
                            displayed_entries: alloc::vec::Vec::new(),
                        }
                    );
                    file_win.render_file_explorer(); 
                    wm.add_window(file_win);
                },
                AppRequest::Terminal => {
                    let initial_terminal = {
                        let global = super::terminal::GUI_TERMINAL.lock();
                        global.clone()
                    };
                    let mut term_win = Window::with_state(
                        100, 100, 400, 300, 
                        "Terminal", Rgb888::BLACK, taskbar.bpp,
                        AppState::Terminal {
                            //terminal: super::terminal::GuiTerminal::new(),
                            terminal:initial_terminal,
                        }
                    );
                    // Draw the window background and the terminal text
                    term_win.render_internal_graphics();
                    let mut temp_state = core::mem::replace(&mut term_win.app_state, AppState::None);
                    
                    if let AppState::Terminal { ref mut terminal } = temp_state {
                        terminal.needs_full_redraw = true;
                        terminal.render_into_window(&mut term_win);
                        //term_win.app_state = AppState::Terminal { terminal };
                        term_win.app_state = AppState::Terminal { terminal: terminal.clone() };
                    }else{
                        term_win.app_state = temp_state;
                    }

                    //term_win.app_state = temp_state;

                    wm.add_window(term_win);
                },
                AppRequest::Calculator => {
                    let mut calc_win = Window::with_state(
                        150, 150, 200, 250, 
                        "Calculator", Rgb888::new(50, 50, 50), taskbar.bpp,
                        crate::gui::window::AppState::Calculator {
                            display: alloc::string::String::from("0"),
                            clear_on_next: false,
                        }
                    );
                    calc_win.render_calculator(); 
                    wm.add_window(calc_win);
                },
                AppRequest::Paint => {
                    let mut paint_win = Window::with_state(
                        120, 60, 270, 310,
                        "Paint", Rgb888::WHITE, taskbar.bpp,
                        AppState::Paint {
                            paint: super::paint::PaintApp::new(),
                        }
                    );
                    paint_win.render_internal_graphics();
                    let mut temp_state = core::mem::replace(&mut paint_win.app_state, AppState::None);
                    
                    if let AppState::Paint { ref mut paint } = temp_state {
                        paint.render_into_window(&mut paint_win);
                    }

                    paint_win.app_state = temp_state;

                    wm.add_window(paint_win);
                },
                AppRequest::Snake =>{
                    let mut snake_win = Window::with_state(
                        150,80,330,290,
                        "Snake",Rgb888::new(26,26,46),taskbar.bpp,
                        AppState::Snake{
                            snake:super::snake::SnakeGame::new()
                        }
                    );
                    snake_win.render_snake();
                    wm.add_window(snake_win);
                },
                _ => {} 
            }
        }

        let raw_events: Vec<RawMouse> = {
            let mut queue = MOUSE_EVENTS.lock();
            let evts = queue.clone();
            queue.clear();
            evts
        };

        // Route Clicks and Releases
        for event in raw_events {
            match event {
                RawMouse::Left (x, y) => {
                    if y < taskbar.height as i32 {
                        // The click belongs to the taskbar!
                        taskbar.send_event(UIEvent::MouseClick { 
                            x, y, button: event // Local X and Y are identical to global here
                        });

                        match taskbar.process_events() {
                            TaskbarAction::OpenPowerMenu => {
                                // Create a small 120x100 window right below the power button
                                let mut power_menu = Window::new(
                                    (taskbar.width - 120) as i32, taskbar.height as i32, 120, 100, 
                                    "Power Menu", Rgb888::new(40, 40, 40), taskbar.bpp, 
                                );
                                
                                // We will build this custom render function next!
                                power_menu.render_power_menu(); 
                                wm.add_window(power_menu);
                            },
                            TaskbarAction::OpenAppMenu => {
                                // Create a 150x120 window anchored to the left
                                let mut app_menu = Window::new(
                                    0, taskbar.height as i32, 
                                    150, 150, 
                                    "App Menu", Rgb888::new(40, 40, 40), taskbar.bpp
                                );
                                
                                app_menu.render_app_menu(); 
                                wm.add_window(app_menu);
                            },
                            TaskbarAction::None => {}
                        }
                        continue; // Skip the windows below!
                    }

                    let mut clicked_index = None;
                    for (i, window) in wm.windows.iter_mut().enumerate().rev() {
                        if x >= window.x && x < window.x + (window.width as i32) &&
                           y >= window.y && y < window.y + (window.height as i32) 
                        {
                            window.send_event(UIEvent::MouseClick { 
                                x: x - window.x, y: y - window.y, 
                                button: event
                            });
                            window.process_events();
                            clicked_index = Some(i);
                            break; 
                        }
                    }

                    if let Some(index) = clicked_index {
                        // Only move it if it isn't ALREADY the top window
                        if index != wm.windows.len() - 1 {
                            // Safely extract the window from the vector
                            let top_window = wm.windows.remove(index);
                            
                            // Report damage so the Compositor repaints the overlapping areas
                            report_damage(Rect::new(
                                top_window.x, top_window.y, 
                                top_window.width as i32, top_window.height as i32
                            ));
                            
                            // Push it to the back of the vector (which is the TOP of the screen)
                            wm.windows.push(top_window);
                        }
                    }
                },
                RawMouse::Left_Released (x, y)  => {
                    // Tell ALL windows to let go!
                    for window in &mut wm.windows {
                        window.is_dragging = false;
                    }
                },
                RawMouse::Left_Pressed (x, y) => {
                    for (i, window) in wm.windows.iter_mut().enumerate().rev() {
                        if x >= window.x && x < window.x + (window.width as i32) &&
                           y >= window.y && y < window.y + (window.height as i32) 
                        {
                            window.send_event(UIEvent::MouseClick { 
                                x: x - window.x, y: y - window.y, 
                                button: event
                            });
                            window.process_events();
                            break; 
                        }
                    }
                }
                _ => {} 
            }
        }


        if let Some(window) = wm.windows.last_mut() {
            // --- THE DRAG ENGINE ---
            // Grab the live atomic coordinates of the cursor
            let global_mouse_x = MOUSE_X.load(core::sync::atomic::Ordering::Relaxed) as i32;
            let global_mouse_y = MOUSE_Y.load(core::sync::atomic::Ordering::Relaxed) as i32;
            
            if window.is_dragging {
                // Calculate the new physical position using the local grab offset
                let new_x = (global_mouse_x - window.drag_x).clamp(0, screen_width - window.width as i32);
                let new_y = (global_mouse_y - window.drag_y).clamp(30, screen_height - window.height as i32);

                if new_x != window.x || new_y != window.y {
                    // 1. Report damage for the OLD position (Erases the trail)
                    if window.x > new_x {  
                        report_damage(Rect::new(
                            new_x + window.width as i32, window.y, window.x - new_x, window.height as i32
                        ));
                    } else if window.x < new_x {
                        report_damage(Rect::new(
                            window.x as i32, window.y, new_x - window.x, window.height as i32
                        ));
                    }

                    if window.y > new_y {  
                        report_damage(Rect::new(
                            window.x, new_y + window.height as i32, window.width as i32, window.y - new_y
                        ));
                    } else if window.y < new_y {
                        report_damage(Rect::new(
                            window.x, window.y as i32, window.width as i32, new_y - window.y
                        ));
                    }

                    // 2. Move the window
                    window.x = new_x;
                    window.y = new_y;

                    // 3. Report damage for the NEW position (Draws the window)
                    report_damage(Rect::new(
                        window.x, window.y, window.width as i32, window.height as i32
                    ));
                }
            }
        }
        
        /*if global_term_state.needs_redraw {
            let snapshot_cells = global_term_state.cells;
            let snapshot_row = global_term_state.row;
            let snapshot_col = global_term_state.col;
            let snapshot_fg = global_term_state.fg;
            let snapshot_full = global_term_state.needs_full_redraw;
            global_term_state.needs_redraw = false;
            global_term_state.needs_full_redraw = false;
            drop(global_term_state); // Release lock BEFORE rendering
            for window in &mut wm.windows {
                let mut temp_state = core::mem::replace(&mut window.app_state, AppState::None);

                if let AppState::Terminal { ref mut terminal } = temp_state {
                    terminal.cells = 
                    // Did the cursor move, OR was a clear/scroll triggered?
                    //if terminal.row != global_term_state.row || terminal.col != global_term_state.col || global_term_state.needs_full_redraw {
                    *terminal = global_term_state.clone();
                    
                    // RENDER TO THE REAL WINDOW
                    terminal.render_into_window(window);
                    
                    // Report damage exactly where the window is currently located!
                    report_damage(Rect::new(
                        window.x, window.y + 20, window.width as i32, window.height as i32 - 20
                    ));
                    
                    // Reset the global redraw flag now that it has been handled
                    //global_term_state.needs_full_redraw = false;
                    global_term_state.needs_redraw = false;
                    //crate::println!("Term loop {}", window.title);
                }
                window.app_state = temp_state;
            }
        }*/
        
        let mut global_term_state = super::terminal::GUI_TERMINAL.lock();
        if global_term_state.needs_redraw {
            // what if i drop the lock and proceed and then i guess in the fut, push it back again..
            let snapshot_cells = global_term_state.cells;
            let snapshot_row = global_term_state.row;
            let snapshot_col = global_term_state.col;
            let snapshot_fg = global_term_state.fg;
            let snapshot_full = global_term_state.needs_full_redraw;
            // that means reddraw i should keep it false.
            global_term_state.needs_redraw = false;  
            global_term_state.needs_full_redraw = false;
            drop(global_term_state); // Release lock BEFORE rendering

            for window in &mut wm.windows {
                let is_terminal = matches!(&window.app_state, AppState::Terminal { .. });
                if !is_terminal { continue; }
                let temp_state = core::mem::replace(&mut window.app_state, AppState::None);

                if let AppState::Terminal { mut terminal } = temp_state {
                    // Sync the local terminal from the snapshot
                    let old_row = terminal.row;
                    terminal.cells = snapshot_cells;
                    terminal.row = snapshot_row;
                    terminal.col = snapshot_col;
                    terminal.fg = snapshot_fg;
                    if snapshot_full  || old_row != snapshot_row{
                        terminal.needs_full_redraw = true;
                    }
                    // Render into the WM-owned window
                    terminal.render_into_window(window);
                    report_damage(Rect::new(
                        window.x, window.y + 20,
                        window.width as i32, window.height as i32 - 20,
                    ));
                    // Put state back
                    window.app_state = AppState::Terminal { terminal };
                }
            }
        }
        {
            let has_snake = wm.windows.iter().any(|w| w.title == "Snake");
            crate::interrupts::SNAKE_ACTIVE.store(has_snake, Ordering::Relaxed);
            let scancodes: Vec<u8> = {
                let mut q = crate::interrupts::SNAKE_SCANCODES.lock();
                let s = q.clone();
                q.clear();
                s
            };
            for window in &mut wm.windows {
                if let AppState::Snake { ref mut snake } = window.app_state {
                    let old_body_head = snake.body.first().copied();
                    let old_state = snake.state;
                    for &sc in &scancodes {
                        snake.on_key(sc);
                    }
                    
                    snake.tick();
                    let new_body_head = snake.body.first().copied();

                    if old_state != snake.state{
                        window.render_snake();
                    }else if old_body_head != new_body_head{
                        window.render_snake_partial();
                    }
                }
            }

        }

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

            let task_rect = Rect::new(0, 0, screen_width, 30);
            if task_rect.contains_rect(&damage) {
                fully_covered = true;
            } else {
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
                    display.blit_partial(&overlap, &window.buffer, window.width, window.x, window.y);
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
                        display.blit_partial(&overlap, &window.buffer, window.width, window.x, window.y);
                    }
                }
            }

            // Draw the Taskbar over everything else
            let taskbar_rect = Rect::new(0, 0, taskbar.width as i32, taskbar.height as i32);
            if let Some(overlap) = damage.intersection(&taskbar_rect) {
                display.blit_partial(&overlap, &taskbar.buffer, taskbar.width, 0, 0);
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
    //super::paint::init_paint(bpp);
    //let mut status = Window::new(550, 50, 200, 150, "Status", Rgb888::BLUE, bpp);
    //status.render_internal_graphics();
    //wm.add_window(status);
    //let mut status2 = Window::new(100, 50, 200, 150, "File", Rgb888::RED, bpp);
    //status2.render_internal_graphics();
    //wm.add_window(status2);
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

    // 2. Report initial damage so the cursor draws on the very first frame
    {
        let mut queue = DAMAGE_QUEUE.lock();
        queue.push(Rect::new(cursor_x, cursor_y, CURSOR_WIDTH as i32, CURSOR_HEIGHT as i32));
    }
    
    let mut left_button_was_down = false;

    while let Some(packet) = mouse_stream.next().await {
        
        if packet.left_btn && !left_button_was_down {
            // The user just clicked! Send the global coordinates to the sorter.
            //let mut clicks = CLICK_QUEUE.lock();
            //clicks.push((cursor_x, cursor_y));
            MOUSE_EVENTS.lock().push(RawMouse::Left(cursor_x, cursor_y));
            
            // Wake up the Compositor to process the click immediately!
            if let Some(waker) = COMPOSITOR_WAKER.lock().take() { waker.wake(); }
        } else if !packet.left_btn && left_button_was_down {
            MOUSE_EVENTS.lock().push(RawMouse::Left_Released(cursor_x, cursor_y));
            if let Some(waker) = COMPOSITOR_WAKER.lock().take() { waker.wake(); }
        } else if packet.left_btn && left_button_was_down {
            MOUSE_EVENTS.lock().push(RawMouse::Left_Pressed(cursor_x, cursor_y));
            if let Some(waker) = COMPOSITOR_WAKER.lock().take() { waker.wake(); }
        }

        if packet.left_btn && (packet.x != 0 || packet.y != 0) {
            if let Some(waker) = COMPOSITOR_WAKER.lock().take() {
                waker.wake();
            }
        }
        
        left_button_was_down = packet.left_btn;

        // 3. Create a damage box for the OLD position (tells Compositor to erase it)
        let old_rect = Rect::new(cursor_x, cursor_y, CURSOR_WIDTH as i32, CURSOR_HEIGHT as i32);

        // 4. Calculate new coordinates
        cursor_x = (cursor_x + ((packet.x as i32) / SENSITIVITY)).clamp(0, screen_width - CURSOR_WIDTH as i32);
        cursor_y = (cursor_y + ((packet.y as i32) / SENSITIVITY)).clamp(0, screen_height - CURSOR_HEIGHT as i32); 
        
        // 5. Update the global atomic coordinates for the Compositor
        MOUSE_X.store(cursor_x, Ordering::Relaxed);
        MOUSE_Y.store(cursor_y, Ordering::Relaxed);

        // Handle left button clicks for paint? idk man, pls work
        /*if packet.left_btn {
            if crate::gui::paint::point_in_paint_window(cursor_x, cursor_y) {
                crate::gui::paint::handle_paint_click(cursor_x, cursor_y);
            }
        }*/

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

// --------------------------------------------------------------------------
// INTERACTIVE MOUSE PIPELINE
// --------------------------------------------------------------------------
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RawMouse {
    Left(i32, i32),
    Left_Released(i32, i32),
    Left_Pressed(i32, i32),
    Right(i32, i32),
    Right_Released(i32, i32),
    Middle(i32, i32),
    Middle_Released(i32, i32),
}

#[derive(Copy, Clone, Debug)]
pub enum UIEvent {
    /// Fired when a mouse button is pressed down
    MouseClick { x: i32, y: i32, button: RawMouse },
    
    /// Fired when a mouse button is released
    MouseRelease { x: i32, y: i32, button: RawMouse },

    // Mouse Long Press
    //MousePressed { x: i32, y: i32, button: RawMouse },
    
    /// Fired when the mouse enters or moves across the component
    MouseMove { x: i32, y: i32 },
    
    /// (For later) Fired when a key is pressed and this component has focus
    KeyPress { char: char }, 
}
