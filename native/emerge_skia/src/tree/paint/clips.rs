//! Only rounded boundaries need more than the intersected device rectangle.
//! The prepared side owns its clip chain: no candidate references a live mount.
use super::*;
use skia_safe::{Canvas, ClipOp, RRect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rounded {
    pub rect: Rect,
    pub radii: [f32; 4],
}
impl Rounded {
    pub fn new(rect: Rect, radii: [f32; 4]) -> Option<Self> {
        let cap = rect.width.min(rect.height).max(0.) * 0.5;
        let radii = radii.map(|r| r.clamp(0., cap));
        radii
            .iter()
            .any(|&r| r > 0.)
            .then_some(Self { rect, radii })
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        let r = self.rect;
        let sides = [x - r.x, r.x + r.width - x, y - r.y, r.y + r.height - y];
        if sides.iter().any(|v| *v < 0.) {
            return false;
        }
        [(0, 2), (1, 2), (1, 3), (0, 3)]
            .into_iter()
            .zip(self.radii)
            .all(|((ix, iy), radius)| {
                let dx = (radius - sides[ix]).max(0.);
                let dy = (radius - sides[iy]).max(0.);
                dx * dx + dy * dy <= radius * radius
            })
    }
    fn apply(self, canvas: &Canvas) {
        let r = self.rect;
        let rr = RRect::new_rect_radii(
            skia_safe::Rect::from_xywh(r.x, r.y, r.width, r.height),
            &self.radii.map(|v| Point::new(v, v)),
        );
        canvas.clip_rrect(rr, ClipOp::Intersect, true);
    }
}
impl crate::tree::decoration::Decor {
    pub(in crate::tree) fn radii(self, scale: f32) -> [f32; 4] {
        self.radius.map(|v| scale::pixels(v, scale))
    }
    pub(in crate::tree) fn inner_rect(self, r: Rect, scale: f32) -> Rect {
        let [t, ri, b, l] = self.border.map(|v| scale::pixels(v, scale));
        Rect {
            x: r.x + l,
            y: r.y + t,
            width: (r.width - l - ri).max(0.),
            height: (r.height - t - b).max(0.),
        }
    }
    pub(super) fn rounded_inner(self, r: Rect, scale: f32) -> Option<Rounded> {
        if self.radius == [0.; 4] {
            return None;
        }
        let [t, ri, b, l] = self.border.map(|v| scale::pixels(v, scale));
        let corners = self.radii(scale);
        let inset = [l.max(t), ri.max(t), ri.max(b), l.max(b)];
        Rounded::new(
            self.inner_rect(r, scale),
            std::array::from_fn(|i| (corners[i] - inset[i]).max(0.)),
        )
    }
}
pub(super) type Mask = u32;
pub(super) const OPEN: Mask = 0;
#[derive(Default, PartialEq)]
pub(super) struct Clips(Vec<(Rounded, Mask)>);
impl Clips {
    pub fn reserve(&mut self, n: usize, b: Budget) -> Result<(), String> {
        if n > self.0.capacity() {
            pool::reserve(&mut self.0, b.capacity(n))?;
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.0.clear()
    }
    pub fn add(&mut self, parent: Mask, shape: Option<Rounded>) -> Mask {
        match shape {
            Some(r) => {
                self.0.push((r, parent));
                self.0.len() as Mask
            }
            None => parent,
        }
    }
    pub fn contains(&self, mut mask: Mask, x: f32, y: f32) -> bool {
        while mask != OPEN {
            let (r, next) = self.0[mask as usize - 1];
            if !r.contains(x, y) {
                return false;
            }
            mask = next;
        }
        true
    }
    fn apply(&self, mask: Mask, canvas: &Canvas) {
        if mask != OPEN {
            let (r, prev) = self.0[mask as usize - 1];
            self.apply(prev, canvas);
            r.apply(canvas);
        }
    }
    pub fn within<T>(&self, mask: Mask, canvas: &Canvas, f: impl FnOnce() -> T) -> T {
        if mask == OPEN {
            return f();
        }
        canvas.save();
        self.apply(mask, canvas);
        let result = f();
        canvas.restore();
        result
    }
}
