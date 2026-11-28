//! A pose changes coordinates, not the space of an already active clip.
//! Each boundary is recorded once, even when many descendants draw or listen.
use super::*;
use skia_safe::{Canvas, Matrix, Rect as SkRect};

/// Logical box clip and the clip a directly hosted bitmap may borrow. An
/// intervening owner caps that half-pixel allowance at the previous box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Aperture {
    pub body: Rect,
    pub media: Rect,
}
impl Aperture {
    pub fn fixed(r: Rect) -> Self {
        Self { body: r, media: r }
    }
    pub fn children(ctx: PaintContext) -> Self {
        Self {
            body: ctx.child_clip,
            media: ctx.child_clip.media_clip(Some(ctx)),
        }
    }
    pub fn intersect(self, prev: Option<Self>, pass: bool) -> Self {
        prev.map_or(self, |p| Self {
            body: self.body.intersection(p.body).unwrap_or_default(),
            media: self
                .media
                .intersection(if pass { p.media } else { p.body })
                .unwrap_or_default(),
        })
    }
    pub fn shifted(self, x: f32, y: f32) -> Self {
        Self {
            body: self.body.shifted(x, y),
            media: self.media.shifted(x, y),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plane {
    pub matrix: Matrix,
    local: Matrix,   // inverse step in a Hits buffer, forward step in a Drawing
    outer: Aperture, // stationary rectangular clip in the parent's space
    device: SkRect,  // conservative bounds for culling only
    pub stop: Mask,
    parent: u32,
    scroll: Option<Key>,
    pub(super) moving: bool,
}
impl Pose {
    pub(in crate::tree) fn matrix(self, frame: Rect) -> Matrix {
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
    pub fn visible(self, r: Rect) -> Option<Rect> {
        let b = self.matrix.invert()?.map_rect(self.device).0;
        r.intersection(Rect {
            x: b.left,
            y: b.top,
            width: b.width(),
            height: b.height(),
        })
    }
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
pub(super) struct Planes(pub Buffer<Plane>);
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
        let enclosing = self.get(parent);
        let (matrix, device) = enclosing.map_or((local, sk(outer)), |p| {
            (p.matrix * local, p.bounds(sk(outer)))
        });
        let moving = enclosing.is_some_and(|p| p.moving);
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
            scroll: None,
            moving,
            outer: Aperture::fixed(outer),
            device: if moving { sk(Rect::UNBOUNDED) } else { device },
        });
        Some(self.0.len() as u32)
    }
    // A port whose content exports across bands cannot be drawn as one
    // contiguous call. Every reference crossing that stationary boundary
    // uses the same keyed displacement, including references in later bands.
    pub fn content(&mut self, parent: u32, stop: Mask, outer: Aperture, k: Key) -> u32 {
        let matrix = self
            .get(parent)
            .map_or_else(Matrix::new_identity, |p| p.matrix);
        self.0.push(Plane {
            matrix,
            local: Matrix::new_identity(),
            outer,
            device: sk(Rect::UNBOUNDED),
            stop,
            parent,
            scroll: Some(k),
            moving: true,
        });
        self.0.len() as u32
    }
    pub fn translated_point(
        &self,
        mut id: u32,
        (x, y): (f32, f32),
        tables: &Tables,
    ) -> Option<(f32, f32, [f32; 2])> {
        self.get(id)?.scroll?;
        let mut delta = [0.; 2];
        while let Some(p) = self.get(id) {
            let Some(k) = p.scroll else {
                let inv = self.installed(id, tables, 1)?.matrix.invert()?;
                let pt = inv.map_point((x, y));
                return Some((pt.x, pt.y, delta));
            };
            let v = tables.get(k, 1)?.view.offset();
            delta[0] -= v[0];
            delta[1] -= v[1];
            id = p.parent;
        }
        Some((x, y, delta))
    }
    pub fn installed(&self, id: u32, tables: &Tables, side: usize) -> Option<Plane> {
        let mut p = self.get(id)?;
        if p.moving {
            let parent = self.installed(p.parent, tables, side);
            p.device = parent.map_or(sk(p.outer.body), |a| a.bounds(sk(p.outer.body)));
            let base = self
                .get(p.parent)
                .map_or_else(Matrix::new_identity, |a| a.matrix);
            let current = parent.map_or(base, |a| a.matrix);
            let mut x = current.translate_x() - base.translate_x();
            let mut y = current.translate_y() - base.translate_y();
            if let Some(k) = p.scroll {
                let [sx, sy] = tables.get(k, side)?.view.offset();
                x -= current.scale_x() * sx + current.skew_x() * sy;
                y -= current.skew_y() * sx + current.scale_y() * sy;
            }
            p.matrix.post_translate((x, y));
        }
        Some(p)
    }
    // Ordinary boxes intersect a logical rectangle. A translation, unlike
    // an authored pose, does not turn that rectangle into a hard device clip:
    // a rotated fill still antialiases its newly exposed edge inside the mask.
    fn limits(p: Plane, prev: Option<Aperture>, tables: &Tables) -> (Rect, Option<Aperture>) {
        let area = p.outer.intersect(prev, false);
        let outer = area.body;
        let next = if let Some(k) = p.scroll {
            let [x, y] = tables.get(k, 0).expect("sealed boundary").view.offset();
            Some(area.shifted(x, y))
        } else {
            prev.map(|_| {
                if outer.width <= 0. || outer.height <= 0. {
                    return Aperture::fixed(Rect::default());
                }
                let r = p
                    .local
                    .invert()
                    .unwrap()
                    .map_rect(sk(outer))
                    .0
                    .with_outset((1., 1.));
                Aperture::fixed(Rect {
                    x: r.left,
                    y: r.top,
                    width: r.width(),
                    height: r.height(),
                })
            })
        };
        (outer, next)
    }
    pub fn logical(&self, id: u32, limit: Option<Aperture>, tables: &Tables) -> Option<Aperture> {
        let Some(p) = self.get(id) else {
            return limit;
        };
        Self::limits(p, self.logical(p.parent, limit, tables), tables).1
    }
    pub fn enter(
        &self,
        p: Plane,
        canvas: &Canvas,
        clips: &Clips,
        before: Mask,
        tables: &Tables,
        limit: Option<Aperture>,
    ) -> Option<Aperture> {
        let parent = self.get(p.parent);
        let mut stop = OPEN;
        let mut prev = limit;
        if let Some(parent) = parent {
            prev = self.enter(parent, canvas, clips, OPEN, tables, limit);
            stop = parent.stop;
        }
        let (outer, next) = Self::limits(p, prev, tables);
        if p.scroll.is_none() {
            canvas.clip_rect(sk(outer), None, false);
        }
        clips.apply(p.stop, stop, canvas, 0, (0., 0.));
        if let Some(r) = clips.one(before) {
            r.apply(canvas, (0., 0.));
        }
        if let Some(k) = p.scroll {
            let [x, y] = tables
                .get(k, 0)
                .expect("sealed scroll boundary")
                .view
                .offset();
            canvas.translate((-x, -y));
        } else {
            canvas.concat(&p.local);
        }
        next
    }
    pub fn point(
        &self,
        plane: u32,
        clips: &Clips,
        before: Mask,
        x: f32,
        y: f32,
        tables: &Tables,
    ) -> Option<(f32, f32, Mask)> {
        let Some(p) = self.get(plane) else {
            return Some((x, y, OPEN));
        };
        let (x, y, stop) = self.point(p.parent, clips, OPEN, x, y, tables)?;
        if !p.outer.body.contains(x, y)
            || !clips.contains(p.stop, stop, x, y)
            || clips.one(before).is_some_and(|r| !r.contains(x, y))
        {
            return None;
        }
        if let Some(k) = p.scroll {
            let [dx, dy] = tables.get(k, 1)?.view.offset();
            return Some((x + dx, y + dy, p.stop));
        }
        let pt = p.local.map_point((x, y));
        Some((pt.x, pt.y, p.stop))
    }
    pub fn get(&self, id: u32) -> Option<Plane> {
        id.checked_sub(1).map(|i| self.0[i as usize])
    }
}
/// A stationary clip chain plus the coordinate breaks at which it applies.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Coords {
    pub mask: Mask,
    pub plane: u32,
}
impl Coords {
    pub fn clip(self, draw: &Drawing, canvas: &Canvas, tables: &Tables) {
        let matrix = canvas.local_to_device();
        let plane = draw.planes.get(self.plane);
        if let Some(p) = plane
            && let Some(r) = draw
                .planes
                .enter(p, canvas, &draw.clips, OPEN, tables, None)
        {
            canvas.clip_rect(sk(r.body), None, false);
        }
        draw.clips.apply(
            self.mask,
            plane.map_or(OPEN, |p| p.stop),
            canvas,
            0,
            (0., 0.),
        );
        canvas.set_matrix(&matrix);
    }
}
// An ordinary shadow drops box clips, but not the active axes of an enclosing
// port. Each strip stays in its establishing plane, even under a descendant pose.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ShadowGuard {
    pub parent: u32,
    pub plane: u32,
    pub inner: Rounded,
    pub axes: [bool; 2],
}
impl ShadowGuard {
    pub fn apply(mut link: u32, draw: &Drawing, canvas: &Canvas, tables: &Tables) {
        if link == 0 {
            return;
        }
        let base = canvas.local_to_device();
        while link != 0 {
            let guard = draw.shadow_guards[link as usize - 1];
            if let Some(p) = draw.planes.installed(guard.plane, tables, 0) {
                canvas.concat(&p.matrix);
            }
            if guard.axes == [true, true] {
                guard.inner.apply(canvas, (0., 0.));
            } else if let Some(mut strip) = canvas.local_clip_bounds() {
                let r = sk(guard.inner.rect);
                if guard.axes[0] {
                    (strip.left, strip.right) = (r.left, r.right);
                }
                if guard.axes[1] {
                    (strip.top, strip.bottom) = (r.top, r.bottom);
                }
                canvas.clip_rect(strip, None, false);
            }
            canvas.set_matrix(&base);
            link = guard.parent;
        }
    }
}
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Scope {
    pub shadow: u32,
    pub guard: Coords, // outer shadows on an escaped branch keep its whole guard
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
