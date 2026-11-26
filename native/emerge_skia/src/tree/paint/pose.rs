//! A pose changes coordinates, not the space of an already active clip.
//! Each boundary is recorded once, even when many descendants draw or listen.
use super::*;
use skia_safe::{Canvas, Matrix, Rect as SkRect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plane {
    pub matrix: Matrix,
    local: Matrix,  // inverse step in a Hits buffer, forward step in a Drawing
    outer: Rect,    // stationary rectangular clip in the parent's space
    device: SkRect, // conservative bounds for culling only
    pub stop: Mask,
    parent: u32,
}
impl Pose {
    fn matrix(self, frame: Rect) -> Matrix {
        let center = sk(frame).center();
        let mut m = Matrix::rotate_deg_pivot(self.degrees, center);
        if self.zoom != 1. {
            m.pre_scale((self.zoom, self.zoom), center);
        }
        m
    }
    // Descendant shapes may overflow an El. Don't intersect their ink with
    // the host's frame or the pre-transform viewport. This AABB is only an
    // early reject; the exact stationary rectangle is installed by enter().
    pub(in crate::tree) fn local_clip(self, frame: Rect, outer: Rect) -> Rect {
        if outer.width <= 0. || outer.height <= 0. {
            return Rect::default();
        }
        self.matrix(frame).invert().map_or(Rect::default(), |inv| {
            let r = inv.map_rect(sk(outer)).0.with_outset((1., 1.));
            Rect {
                x: r.left,
                y: r.top,
                width: r.width(),
                height: r.height(),
            }
        })
    }
}
impl Plane {
    pub fn bounds(self, local: SkRect) -> SkRect {
        let mut r = self.matrix.map_rect(local).0;
        if r.intersect(self.device) {
            r
        } else {
            SkRect::default()
        }
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
    pub fn branch(
        &mut self,
        parent: u32,
        stop: Mask,
        a: &Attrs,
        ctx: PaintContext,
        hits: bool,
    ) -> Option<u32> {
        if !a.posed() {
            return Some(parent);
        }
        if a.pose.zoom == 0. {
            return None;
        }
        let local = a.pose.matrix(ctx.frame);
        let outer = ctx.boundary;
        let (matrix, device) = self.get(parent).map_or((local, sk(outer)), |p| {
            (p.matrix * local, p.bounds(sk(outer)))
        });
        if !(ctx.zoom.is_finite()
            && ctx.zoom > 0.
            && matrix.is_finite()
            && matrix.map_rect(sk(ctx.frame)).0.is_finite())
        {
            return None;
        }
        // Never publish ink on a plane that cannot supply its local point.
        let inverse = local.invert()?;
        self.0.push(Plane {
            matrix,
            local: if hits { inverse } else { local },
            stop,
            parent,
            outer,
            device,
        });
        Some(self.0.len() as u32)
    }
    pub fn enter(&self, p: Plane, canvas: &Canvas, clips: &Clips, before: Mask) {
        let mut stop = OPEN;
        if let Some(parent) = self.get(p.parent) {
            self.enter(parent, canvas, clips, OPEN);
            stop = parent.stop;
        }
        canvas.clip_rect(sk(p.outer), None, false);
        clips.apply(p.stop, stop, canvas, 0, (0., 0.));
        if let Some(r) = clips.one(before) {
            r.apply(canvas, (0., 0.));
        }
        canvas.concat(&p.local);
    }
    pub fn point(
        &self,
        plane: u32,
        clips: &Clips,
        before: Mask,
        x: f32,
        y: f32,
    ) -> Option<(f32, f32, Mask)> {
        let Some(p) = self.get(plane) else {
            return Some((x, y, OPEN));
        };
        let (x, y, stop) = self.point(p.parent, clips, OPEN, x, y)?;
        if !p.outer.contains(x, y)
            || !clips.contains(p.stop, stop, x, y)
            || clips.one(before).is_some_and(|r| !r.contains(x, y))
        {
            return None;
        }
        let pt = p.local.map_point((x, y));
        Some((pt.x, pt.y, p.stop))
    }
    pub fn get(&self, id: u32) -> Option<Plane> {
        id.checked_sub(1).map(|i| self.0[i as usize])
    }
}
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Scope {
    pub mask: Mask,
    pub plane: u32,
    // A rounded inline continuation belongs to the wrapper, immediately
    // before its Text's pose. All continuations still share one Text plane.
    pub before: Mask,
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
