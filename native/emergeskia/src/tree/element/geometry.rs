#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const DEFAULT: Point = Point { x: 0.0, y: 0.0 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub(crate) fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            origin: Point { x, y },
            size: Size { width, height },
        }
    }

    pub(crate) fn inside_padding(self, padding: f32) -> Rect {
        Rect {
            origin: Point {
                x: self.origin.x + padding,
                y: self.origin.y + padding,
            },
            size: Size {
                width: (self.size.width - padding * 2.0).max(0.0),
                height: (self.size.height - padding * 2.0).max(0.0),
            },
        }
    }
}
