use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use crate::task::mouse::MousePacket;
use crate::task::mouse::MouseStream; 
use futures_util::stream::StreamExt;
use super::window::{Window, WindowManager};
use crate::gui::buffer::FrameBufferDisplay;

pub async fn activate_mouse(display: &mut FrameBufferDisplay, screen_width: i32, screen_height: i32) {
    let mut mouse_stream = MouseStream::new(); 
    let mut cursor_x: i32 = screen_width/2;
    let mut cursor_y: i32 = screen_height/2;
    const SENSITIVITY: i32 = 2;
    const BPP: usize = 4; // Bytes per pixel of the display

    let mut wm = WindowManager::new(screen_width as u32, screen_height as u32);
    wm.add_window(Window::new(100, 100, 400, 300, "Terminal", Rgb888::BLACK));
    wm.add_window(Window::new(550, 50, 200, 150, "Status", Rgb888::BLUE));
    wm.draw_windows(display).ok();

    let mut cursor_bg = [[Rgb888::new(0, 128, 128); 10]; 10];
    let mut saved_bg: [u8; 10 * 10 * BPP] = [0; 10 * 10 * BPP];

    display.save_patch(cursor_x as usize, cursor_y as usize, 10, 10, &mut saved_bg);
    
    while let Some(packet) = mouse_stream.next().await {
        display.restore_patch(cursor_x as usize, cursor_y as usize, 10, 10, &saved_bg);

        cursor_x = (cursor_x + ((packet.x as i32) / SENSITIVITY)).clamp(0, screen_width - 10);
        cursor_y = (cursor_y + ((packet.y as i32) / SENSITIVITY)).clamp(0, screen_height - 10); 

        display.save_patch(cursor_x as usize, cursor_y as usize, 10, 10, &mut saved_bg);

        // Simple 10x10 red square cursor for now
        Rectangle::new(Point::new(cursor_x, cursor_y),Size::new(10, 10))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(display);
    }
}