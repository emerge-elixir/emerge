use super::*;

fn bottom(r: &Rect) -> f32 {
    (r.y + r.height).min(f32::MAX)
}

/// Float rectangles share one coordinate system for the duration of one
/// document (or standalone paragraph). Each side stacks in increasing Y.
#[derive(Default)]
pub(in crate::tree) struct Floats([Vec<Rect>; 2]);
impl Floats {
    pub fn reserve(&mut self, n: usize, b: Budget) -> Result<(), String> {
        for side in &mut self.0 {
            if n > side.capacity() {
                pool::reserve(side, b.capacity(n))?;
            }
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        for side in &mut self.0 {
            side.clear();
        }
    }
    pub fn bottom(&self) -> f32 {
        self.0
            .iter()
            .filter_map(|s| s.last())
            .map(bottom)
            .fold(0., f32::max)
    }
}
/// Search with the intrinsic demand, then exclude the box at its final offer.
#[derive(Clone, Copy)]
pub(in crate::tree) struct FloatSize {
    pub wanted: [f32; 2],
    pub outer: [f32; 2],
}
/// The caller works in local coordinates. Only float queries and insertions
/// cross this boundary; stored word fragments never depend on a viewport origin.
#[derive(Clone, Copy)]
pub(in crate::tree) struct Band {
    origin: [f32; 2],
    left: f32,
    pub right: f32,
    gap: f32,
}
impl Band {
    pub fn new(origin: [f32; 2], left: f32, inner: f32, gap: f32) -> Self {
        Self {
            origin,
            left,
            right: coordinate(left + inner),
            gap,
        }
    }
    /// An oversized item can overflow a clear line. Only a float that
    /// actually narrows its required band is a reason to wait.
    pub fn fits(self, width: f32, left: f32, right: f32) -> bool {
        width.min(self.right - self.left) <= right - left + EPS
    }
    pub fn limits(self, floats: &Floats, y: f32, height: f32) -> (f32, f32) {
        let y = coordinate(y + self.origin[1]);
        let active = |side: usize| {
            let floats = &floats.0[side];
            let start = floats.partition_point(|r| bottom(r) <= y);
            floats[start..]
                .iter()
                .take_while(move |r| r.y < y + height.max(1.))
        };
        let l = active(0)
            .map(|r| (r.x - self.origin[0] + r.width + self.gap).min(self.right))
            .fold(self.left, f32::max);
        let r = active(1)
            .map(|r| (r.x - self.origin[0] - self.gap).max(self.left))
            .fold(self.right, f32::min);
        (l, r.max(l))
    }
    pub fn next_bottom(self, floats: &Floats, y: f32) -> Option<f32> {
        floats
            .0
            .iter()
            .filter_map(|s| s.get(s.partition_point(|r| bottom(r) - self.origin[1] <= y + EPS)))
            .map(|r| bottom(r) - self.origin[1])
            .min_by(f32::total_cmp)
    }
    pub fn place(self, floats: &mut Floats, side: Alignment, y: f32, size: FloatSize) -> Rect {
        let right = side == Alignment::End;
        let mut y = floats.0[usize::from(right)]
            .last()
            .map_or(y, |r| y.max(bottom(r) - self.origin[1]));
        loop {
            let (l, r) = self.limits(floats, y, size.wanted[1]);
            if self.fits(size.wanted[0], l, r) {
                break;
            }
            let Some(b) = self.next_bottom(floats, y) else {
                break;
            };
            y = b;
        }
        let [width, height] = size.outer;
        let (l, r) = self.limits(floats, y, height);
        let rect = Rect {
            x: if right { (r - width).max(l) } else { l },
            y,
            width,
            height,
        };
        floats.0[usize::from(right)].push(Rect {
            x: coordinate(rect.x + self.origin[0]),
            y: coordinate(rect.y + self.origin[1]),
            ..rect
        });
        rect
    }
}
