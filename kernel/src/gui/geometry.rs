#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
    // fix for the tearing when wrting it so yeah 
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x&& other.y >= self.y && (other.x + other.width) <= (self.x + self.width) && (other.y + other.height) <= (self.y + self.height)
    }

    /// Calculates the exact overlapping rectangle between two rects.
    /// Returns None if they do not overlap at all.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        
        let self_x2 = self.x + self.width;
        let self_y2 = self.y + self.height;
        let other_x2 = other.x + other.width;
        let other_y2 = other.y + other.height;

        let x2 = self_x2.min(other_x2);
        let y2 = self_y2.min(other_y2);

        if x1 < x2 && y1 < y2 {
            Some(Rect {
                x: x1,
                y: y1,
                width: x2 - x1,
                height: y2 - y1,
            })
        } else {
            None
        }
    }
}