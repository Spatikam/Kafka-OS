use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{Triangle, Rectangle, Circle, PrimitiveStyle, PrimitiveStyleBuilder, Line},
};
use crate::task::mouse::MousePacket;
use crate::task::mouse::MouseStream; 
use futures_util::stream::StreamExt;
use super::window::{Window, WindowManager};
use crate::gui::buffer::FrameBufferDisplay;

pub async fn activate_gui(display: &mut FrameBufferDisplay, screen_width: i32, screen_height: i32) {
    let mut mouse_stream = MouseStream::new(); 
    let mut cursor_x: i32 = screen_width/2;
    let mut cursor_y: i32 = screen_height/2;
    const SENSITIVITY: i32 = 1;
    const BPP: usize = 4; // Bytes per pixel of the display
    const CURSOR_WIDTH: usize = 17;
    const CURSOR_HEIGHT: usize = 21;

    let mut wm = WindowManager::new(screen_width as u32, screen_height as u32);
    wm.add_window(Window::new(100, 100, 400, 300, "Terminal", Rgb888::BLACK));
    wm.add_window(Window::new(550, 50, 200, 150, "Status", Rgb888::BLUE));
    wm.draw_windows(display).ok();

    //let mut cursor_bg = [[Rgb888::new(0, 128, 128); 10]; 10];
    let mut saved_bg: [u8; CURSOR_WIDTH * CURSOR_HEIGHT * BPP] = [0; CURSOR_WIDTH * CURSOR_HEIGHT * BPP];

    display.save_patch(cursor_x as usize, cursor_y as usize, CURSOR_WIDTH, CURSOR_HEIGHT, &mut saved_bg);
    
    while let Some(packet) = mouse_stream.next().await {
        display.restore_patch(cursor_x as usize, cursor_y as usize, CURSOR_WIDTH, CURSOR_HEIGHT, &saved_bg);

        cursor_x = (cursor_x + ((packet.x as i32) / SENSITIVITY)).clamp(0, screen_width - CURSOR_WIDTH as i32);
        cursor_y = (cursor_y + ((packet.y as i32) / SENSITIVITY)).clamp(0, screen_height - CURSOR_HEIGHT as i32); 

        display.save_patch(cursor_x as usize, cursor_y as usize, CURSOR_WIDTH, CURSOR_HEIGHT, &mut saved_bg);

        // Simple 10x10 red square cursor for now
        /*Rectangle::new(Point::new(cursor_x, cursor_y),Size::new(10, 10))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(display);
        */
        draw_cursor(display, cursor_x, cursor_y).ok();
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
    Line::new(Point::new(x, y), Point::new(x, y + 20))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 1))
        .draw(target)?;
    Line::new(Point::new(x, y), Point::new(x + 16, y + 12))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 1))
        .draw(target)?;
    Line::new(Point::new(x + 6, y + 12), Point::new(x, y + 20))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 1))
        .draw(target)?;
    Line::new(Point::new(x + 6, y + 12), Point::new(x + 16, y + 12))
        .into_styled(PrimitiveStyle::with_stroke(Rgb888::WHITE, 1))
        .draw(target)?;
    Ok(())
}