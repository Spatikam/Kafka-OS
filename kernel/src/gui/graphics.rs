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

pub async fn activate_mouse<T: DrawTarget<Color = Rgb888>>(display: &mut T, screen_width: i32, screen_height: i32) {
    let mut mouse_stream = MouseStream::new(); 
    let mut cursor_x: i32 = screen_width/2;
    let mut cursor_y: i32 = screen_height/2;
    const SENSITIVITY: i32 = 2;

    let mut wm = WindowManager::new(screen_width as u32, screen_height as u32);
    wm.add_window(Window::new(100, 100, 400, 300, "Terminal", Rgb888::BLACK));
    wm.add_window(Window::new(550, 50, 200, 150, "Status", Rgb888::BLUE));
    wm.draw_windows(display).ok();

    while let Some(packet) = mouse_stream.next().await {
        cursor_x = (cursor_x + ((packet.x as i32) / SENSITIVITY)).clamp(0, screen_width - 1);
        cursor_y = (cursor_y + ((packet.y as i32) / SENSITIVITY)).clamp(0, screen_height - 1); 

        // Simple 10x10 red square cursor for now
        Rectangle::new(Point::new(cursor_x, cursor_y),Size::new(10, 10))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(display);
    }
}