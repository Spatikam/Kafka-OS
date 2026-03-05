use alloc::vec::Vec;
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
    pub fn subtract(&self, other: &Rect) -> Vec<Rect> {
        let mut result = Vec::new();

        // If no overlap, return self unchanged
        let overlap = match self.intersection(other) {
            Some(o) => o,
            None => {
                result.push(*self);
                return result;
            }
        };

        let self_right = self.x + self.width;
        let self_bottom = self.y + self.height;
        let ovr_right = overlap.x + overlap.width;
        let ovr_bottom = overlap.y + overlap.height;

        // Top strip
        if overlap.y > self.y {
            result.push(Rect::new(self.x, self.y, self.width, overlap.y - self.y));
        }
        // Bottom strip
        if ovr_bottom < self_bottom {
            result.push(Rect::new(self.x, ovr_bottom, self.width, self_bottom - ovr_bottom));
        }
        // Left strip (between top and bottom of overlap)
        if overlap.x > self.x {
            result.push(Rect::new(self.x, overlap.y, overlap.x - self.x, overlap.height));
        }
        // Right strip (between top and bottom of overlap)
        if ovr_right < self_right {
            result.push(Rect::new(ovr_right, overlap.y, self_right - ovr_right, overlap.height));
        }

        result
    }
}