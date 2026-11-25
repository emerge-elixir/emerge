//! A leaf's local mask chain ends where its stationary ancestors begin.
//! Keep that split with the drawing/hits; a device bounding box is not a mask.
use super::*;
use skia_safe::{Canvas, Matrix, Rect as SkRect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plane {
    pub matrix: Matrix,
    pub outer: Rect,
    pub stop: Mask,
}
impl Plane {
    pub fn new(a: &Attrs, ctx: PaintContext, stop: Mask) -> Self {
        let center = sk(ctx.frame).center();
        let mut matrix = Matrix::rotate_deg_pivot(a.pose.degrees, center);
        if a.pose.zoom != 1. {
            matrix.pre_scale((a.pose.zoom, a.pose.zoom), center);
        }
        Self {
            matrix,
            outer: ctx.clip,
            stop,
        }
    }
    pub fn enter(self, canvas: &Canvas, clips: &Clips) {
        canvas.clip_rect(sk(self.outer), None, false);
        clips.apply(self.stop, OPEN, canvas, 0, (0., 0.));
        canvas.concat(&self.matrix);
    }
    pub fn bounds(self, local: SkRect) -> SkRect {
        let mut r = self.matrix.map_rect(local).0;
        if r.intersect(sk(self.outer)) {
            r
        } else {
            SkRect::default()
        }
    }
    pub fn inverse(self) -> Option<Self> {
        Some(Self {
            matrix: self.matrix.invert()?,
            ..self
        })
    }
}

/// Local lengths of a device-pixel allowance along the two sampling axes.
/// Unlike mapping an AABB back to local coordinates this also works under shear.
pub(super) fn allowance(matrix: Matrix, device: f32) -> (f32, f32) {
    let pad = |x: f32, y: f32| {
        let length = x.hypot(y);
        if length > 0. && length.is_finite() {
            let v = device / length;
            if v.is_finite() {
                return v;
            }
        }
        0.
    };
    (
        pad(matrix.scale_x(), matrix.skew_y()),
        pad(matrix.skew_x(), matrix.scale_y()),
    )
}

#[derive(Default, PartialEq)]
pub(super) struct Planes(pub Vec<Plane>);
impl Planes {
    pub fn add(&mut self, plane: Option<Plane>) -> u32 {
        plane.map_or(0, |p| {
            self.0.push(p);
            self.0.len() as u32
        })
    }
    pub fn get(&self, id: u32) -> Option<Plane> {
        id.checked_sub(1).map(|i| self.0[i as usize])
    }
}
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Scope {
    pub mask: Mask,
    pub plane: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_bleed_uses_the_transformed_unit_axes_not_their_bounding_box() {
        for (m, dx, dy) in [
            (Matrix::rotate_deg(45.), 1., 1.),
            (Matrix::rotate_deg(30.) * Matrix::scale((2., 0.5)), 0.5, 2.),
            (Matrix::scale((2., 2.)), 0.5, 0.5),
            (Matrix::skew((0., 0.75)), 0.8, 1.),
            (Matrix::scale((0., f32::INFINITY)), 0., 0.),
        ] {
            let (x, y) = allowance(m, 1.);
            assert!((x - dx).abs() < 1e-5 && (y - dy).abs() < 1e-5);
        }
    }
}
