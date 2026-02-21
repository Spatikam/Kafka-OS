use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb888,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use crate::task::mouse::MousePacket;

pub struct Mouse {
    x: i32,
    y: i32,
    screen_width: i32,
    screen_height: i32,
}

impl Mouse {
    pub fn new(screen_width: i32, screen_height: i32) -> Self {
        Self {
            x: screen_width / 2,
            y: screen_height / 2,
            screen_width,
            screen_height,
        }
    }

    pub fn update(&mut self, packet: &MousePacket) {
        self.x = (self.x + packet.x as i32).clamp(0, self.screen_width - 1);
        self.y = (self.y - packet.y as i32).clamp(0, self.screen_height - 1);
    }

    pub fn position(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb888>,
    {
        // Simple 5x5 red square cursor for now
        Rectangle::new(self.position(), Size::new(5, 5))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::RED))
            .draw(target)
    }
}
