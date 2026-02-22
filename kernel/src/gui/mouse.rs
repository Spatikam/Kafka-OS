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

pub struct Mouse {
    x: i32,
    y: i32,
    screen_width: i32,
    screen_height: i32,
    mouse_stream: MouseStream,
    SENSITIVITY: i32,
}

impl Mouse {
    pub fn new(screen_width: i32, screen_height: i32) -> Self {
        Self {
            x: screen_width / 2,
            y: screen_height / 2,
            screen_width,
            screen_height,
            mouse_stream: MouseStream::new(),
            SENSITIVITY: 2,
        }
    }

    pub fn update(&mut self, packet: &MousePacket) {
        self.x = ((self.x + packet.x as i32) / self.SENSITIVITY).clamp(0, self.screen_width - 1);
        self.y = ((self.y - packet.y as i32) / self.SENSITIVITY).clamp(0, self.screen_height - 1);
    }

    pub fn position(&mut self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn draw<D>(&mut self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb888>,
    {
        // Simple 5x5 red square cursor for now
        Rectangle::new(self.position(), Size::new(5, 5))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(target)
    }

    pub async fn start<D>(&mut self, display: &mut D) 
    where
        D: DrawTarget<Color = Rgb888>,
    {
        while let Some(packet) = self.mouse_stream.next().await {
            self.update(&packet);
            self.draw(display).ok();
        }
    }
}

    