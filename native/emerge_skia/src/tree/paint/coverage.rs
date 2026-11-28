//! Front-to-back proof for a single ordinary sibling group. Possible ink is
//! an upper bound; only an opaque, pixel-aligned interior adds coverage. Do
//! not union disjoint covers into a bounding rectangle (it would fill holes).
use super::*;

#[derive(Default)]
pub(super) struct Coverage {
    stationary: Option<Rect>,
    interiors: [Option<Rect>; 4],
    cursor: usize,
}
impl Coverage {
    pub(super) fn within(scope: Scope, clips: &Clips) -> Option<Self> {
        if scope.plane != 0 || scope.before != OPEN || scope.guard != Coords::default() {
            return None;
        }
        let stationary = clips.one(scope.mask);
        if stationary.is_some_and(|r| r.radii != [0.; 4] || clips.parent(scope.mask) != OPEN) {
            return None;
        }
        Some(Self {
            stationary: stationary.map(|r| r.rect),
            ..Self::default()
        })
    }
    pub(super) fn hides(&mut self, e: &Elements, l: &Layers, k: Key) -> bool {
        self.hides_in(e, l, k, l.projection(k))
    }
    pub(super) fn hides_in(
        &mut self,
        e: &Elements,
        l: &Layers,
        k: Key,
        projected: Option<&Projected>,
    ) -> bool {
        let a = &e.attrs[k];
        let p = l.parts[k].at(projected);
        if a.posed()
            || a.opacity.is_some()
            || e.slots[k].stats.mounts != 0
            || p.ports[0].children != 0
            || e.slots[k].shape == Shape::Paragraph
            || l.paint[k].bars
            || p.art
            || p.paint.text.is_some()
            || a.decor.shadows.len != 0
        {
            self.interiors.fill(None);
            return false;
        }
        let fill = p.paint.fill;
        let possible = if p.paint.decorated {
            projected.map_or(l.paint[k], |p| p.ctx).frame
        } else if let Some((r, _)) = fill {
            r
        } else {
            return false;
        };
        if self.interiors.iter().flatten().any(|r| {
            r.x <= possible.x.floor()
                && r.y <= possible.y.floor()
                && r.x + r.width >= (possible.x + possible.width).ceil()
                && r.y + r.height >= (possible.y + possible.height).ceil()
        }) {
            return true;
        }
        if !p.paint.decorated
            && let Some((r, c)) = fill
            && c & 255 == 255
        {
            // Even an axis-aligned scroll mask can have partially covered
            // edge pixels. Only its full-pixel interior is an occluder.
            let r = self
                .stationary
                .map_or(r, |clip| r.intersection(clip).unwrap_or_default());
            let (x, y) = (r.x.ceil(), r.y.ceil());
            let width = (r.x + r.width).floor() - x;
            let height = (r.y + r.height).floor() - y;
            if width > 0. && height > 0. {
                self.interiors[self.cursor] = Some(Rect {
                    x,
                    y,
                    width,
                    height,
                });
                self.cursor = (self.cursor + 1) % self.interiors.len();
            }
        }
        false
    }
}
