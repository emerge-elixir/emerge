use super::attrs::Attrs;
use super::element::Frame;
use super::geometry::{ClipShape, Rect, point_hits_clip};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2 {
    pub xx: f32,
    pub yx: f32,
    pub xy: f32,
    pub yy: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Affine2 {
    fn default() -> Self {
        Self::identity()
    }
}

impl Affine2 {
    pub const fn identity() -> Self {
        Self {
            xx: 1.0,
            yx: 0.0,
            xy: 0.0,
            yy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn is_identity(self) -> bool {
        self == Self::identity()
    }

    pub const fn translation(dx: f32, dy: f32) -> Self {
        Self {
            xx: 1.0,
            yx: 0.0,
            xy: 0.0,
            yy: 1.0,
            tx: dx,
            ty: dy,
        }
    }

    pub const fn scale(x: f32, y: f32) -> Self {
        Self {
            xx: x,
            yx: 0.0,
            xy: 0.0,
            yy: y,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn rotation_degrees(deg: f32) -> Self {
        let rad = deg.to_radians();
        let (sin, cos) = rad.sin_cos();
        Self {
            xx: cos,
            yx: sin,
            xy: -sin,
            yy: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn mul(self, other: Self) -> Self {
        Self {
            xx: self.xx * other.xx + self.xy * other.yx,
            yx: self.yx * other.xx + self.yy * other.yx,
            xy: self.xx * other.xy + self.xy * other.yy,
            yy: self.yx * other.xy + self.yy * other.yy,
            tx: self.xx * other.tx + self.xy * other.ty + self.tx,
            ty: self.yx * other.tx + self.yy * other.ty + self.ty,
        }
    }

    pub fn map_point(self, point: Point) -> Point {
        Point {
            x: self.xx * point.x + self.xy * point.y + self.tx,
            y: self.yx * point.x + self.yy * point.y + self.ty,
        }
    }

    pub fn map_rect_aabb(self, rect: Rect) -> Rect {
        let top_left = self.map_point(Point {
            x: rect.x,
            y: rect.y,
        });
        let top_right = self.map_point(Point {
            x: rect.x + rect.width,
            y: rect.y,
        });
        let bottom_left = self.map_point(Point {
            x: rect.x,
            y: rect.y + rect.height,
        });
        let bottom_right = self.map_point(Point {
            x: rect.x + rect.width,
            y: rect.y + rect.height,
        });

        let min_x = top_left
            .x
            .min(top_right.x)
            .min(bottom_left.x)
            .min(bottom_right.x);
        let max_x = top_left
            .x
            .max(top_right.x)
            .max(bottom_left.x)
            .max(bottom_right.x);
        let min_y = top_left
            .y
            .min(top_right.y)
            .min(bottom_left.y)
            .min(bottom_right.y);
        let max_y = top_left
            .y
            .max(top_right.y)
            .max(bottom_left.y)
            .max(bottom_right.y);

        Rect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }

    pub fn inverse(self) -> Option<Self> {
        let det = self.xx * self.yy - self.xy * self.yx;
        if det.abs() <= f32::EPSILON {
            return None;
        }

        let inv_det = 1.0 / det;
        let xx = self.yy * inv_det;
        let yx = -self.yx * inv_det;
        let xy = -self.xy * inv_det;
        let yy = self.xx * inv_det;
        let tx = -(xx * self.tx + xy * self.ty);
        let ty = -(yx * self.tx + yy * self.ty);

        Some(Self {
            xx,
            yx,
            xy,
            yy,
            tx,
            ty,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InteractionClip {
    pub local_clip: ClipShape,
    pub screen_bounds: Rect,
    pub screen_to_local: Option<Affine2>,
}

impl InteractionClip {
    pub fn new(local_clip: ClipShape, local_to_screen: Affine2) -> Self {
        Self {
            local_clip,
            screen_bounds: local_to_screen.map_rect_aabb(local_clip.rect),
            screen_to_local: local_to_screen.inverse(),
        }
    }

    pub fn contains_screen(&self, x: f32, y: f32) -> bool {
        if !self.screen_bounds.contains(x, y) {
            return false;
        }

        let Some(screen_to_local) = self.screen_to_local else {
            return false;
        };
        let local = screen_to_local.map_point(Point { x, y });
        point_hits_clip(self.local_clip, local.x, local.y)
    }
}

pub fn element_transform(frame: Frame, attrs: &Attrs) -> Affine2 {
    let move_x = attrs.move_x.unwrap_or(0.0) as f32;
    let move_y = attrs.move_y.unwrap_or(0.0) as f32;
    let rotate = attrs.rotate.unwrap_or(0.0) as f32;
    let scale = attrs.scale.unwrap_or(1.0) as f32;
    let center_x = frame.x + frame.width / 2.0;
    let center_y = frame.y + frame.height / 2.0;

    let mut transform = Affine2::identity();

    if move_x != 0.0 || move_y != 0.0 {
        transform = transform.mul(Affine2::translation(move_x, move_y));
    }

    if rotate != 0.0 || (scale - 1.0).abs() > f32::EPSILON {
        transform = transform
            .mul(Affine2::translation(center_x, center_y))
            .mul(Affine2::rotation_degrees(rotate))
            .mul(Affine2::scale(scale, scale))
            .mul(Affine2::translation(-center_x, -center_y));
    }

    transform
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;

    #[test]
    fn affine_inverse_round_trips_points() {
        let transform = Affine2::translation(12.0, -5.0)
            .mul(Affine2::translation(30.0, 20.0))
            .mul(Affine2::rotation_degrees(30.0))
            .mul(Affine2::scale(1.5, 1.5))
            .mul(Affine2::translation(-30.0, -20.0));
        let inverse = transform.inverse().expect("transform should invert");
        let point = Point { x: 42.0, y: 17.0 };

        let mapped = transform.map_point(point);
        let round_trip = inverse.map_point(mapped);

        assert!((round_trip.x - point.x).abs() < 0.001);
        assert!((round_trip.y - point.y).abs() < 0.001);
    }

    #[test]
    fn interaction_transform_keeps_center_fixed_under_rotation_and_scale() {
        let mut attrs = Attrs::default();
        attrs.move_x = Some(15.0);
        attrs.move_y = Some(-10.0);
        attrs.rotate = Some(45.0);
        attrs.scale = Some(2.0);

        let frame = Frame {
            x: 20.0,
            y: 40.0,
            width: 80.0,
            height: 20.0,
            content_width: 80.0,
            content_height: 20.0,
        };
        let transform = element_transform(frame, &attrs);
        let center = Point {
            x: frame.x + frame.width / 2.0,
            y: frame.y + frame.height / 2.0,
        };
        let mapped = transform.map_point(center);

        assert!((mapped.x - (center.x + 15.0)).abs() < 0.001);
        assert!((mapped.y - (center.y - 10.0)).abs() < 0.001);
    }
}
