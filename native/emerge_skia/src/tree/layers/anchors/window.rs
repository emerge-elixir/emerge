//! Intersection of logical viewports in the anchor's axes. The common case
//! is just intersecting rectangles. A turn or rounded corner needs the actual
//! shape, not its enclosing box (which would keep a detached menu alive).
use super::*;

type Pair = [f64; 2];
fn dot(a: Pair, b: Pair) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

// A vertical line in anchor space, expressed in the window's axes:
// local(x, y) = origin + x * dx + y * dy. The inner box starts at (0,0).
pub(super) struct Window {
    origin: Pair,
    dx: Pair,
    dy: Pair,
    size: Pair,
    radii: [f64; 4],
}
impl Window {
    pub(super) fn new(m: Matrix, r: Rect, radii: [f32; 4]) -> Self {
        Self {
            origin: [
                f64::from(m.translate_x()) - f64::from(r.x),
                f64::from(m.translate_y()) - f64::from(r.y),
            ],
            dx: [m.scale_x().into(), m.skew_y().into()],
            dy: [m.skew_x().into(), m.scale_y().into()],
            size: [r.width.into(), r.height.into()],
            radii: radii.map(f64::from),
        }
    }
    fn corner(&self, p: Pair) -> (Pair, f64) {
        let right = p[0] > self.size[0] * 0.5;
        let bottom = p[1] > self.size[1] * 0.5;
        let r = self.radii[match (right, bottom) {
            (false, false) => 0,
            (true, false) => 1,
            (true, true) => 2,
            (false, true) => 3,
        }];
        let u = std::array::from_fn(|i| {
            p[i] - if [right, bottom][i] {
                self.size[i] - r
            } else {
                r
            }
        });
        let cut = r > 0.
            && (if right { u[0] > 0. } else { u[0] < 0. })
            && (if bottom { u[1] > 0. } else { u[1] < 0. });
        (u, if cut { r } else { 0. })
    }
    fn contains_origin(&self) -> bool {
        let p = self.origin;
        let (u, r) = self.corner(p);
        (0..2).all(|i| p[i] >= 0. && p[i] < self.size[i]) && (r == 0. || dot(u, u) <= r * r)
    }
    // The box's enclosing interval on each anchor axis. Invert only the two
    // affine rows in f64: a tiny box far from the origin still has a width.
    fn bounds(&self) -> Option<[Pair; 2]> {
        let [a, c] = self.dx;
        let [b, d] = self.dy;
        let det = a * d - b * c;
        if det == 0.
            || !det.is_finite()
            || !self.origin.iter().all(|v| v.is_finite())
            || !self.size.iter().all(|&v| v > 0. && v.is_finite())
        {
            return None;
        }
        let axes = [[d / det, -b / det], [-c / det, a / det]];
        Some(axes.map(|v| {
            let t = -dot(v, self.origin);
            let [x, y] = [v[0] * self.size[0], v[1] * self.size[1]];
            [t + x.min(0.) + y.min(0.), t + x.max(0.) + y.max(0.)]
        }))
    }
    // A vertical chord has two endpoints. Their supporting line slopes tell
    // on which side of x two disjoint chords could meet. If the line misses
    // this window altogether, its center gives that direction directly.
    fn section(&self, x: f64) -> Result<[End; 2], bool> {
        let p = std::array::from_fn::<_, 2, _>(|i| self.origin[i] + x * self.dx[i]);
        let center_right = || {
            let [u, v] = std::array::from_fn(|i| self.size[i] * 0.5 - p[i]);
            (u * self.dy[1] - v * self.dy[0]) / (self.dx[0] * self.dy[1] - self.dx[1] * self.dy[0])
                > 0.
        };
        let mut ends = [End::flat(f64::NEG_INFINITY), End::flat(f64::INFINITY)];
        for (i, &v) in p.iter().enumerate() {
            let dy = self.dy[i];
            if dy == 0. {
                if v <= 0. || v >= self.size[i] {
                    return Err(center_right());
                }
                continue;
            }
            let sign = usize::from(dy < 0.);
            let walls = [0., self.size[i]];
            for (j, end) in ends.iter_mut().enumerate() {
                let y = (walls[j ^ sign] - v) / dy;
                if (j == 0 && y > end.y) || (j == 1 && y < end.y) {
                    *end = End {
                        y,
                        slope: -self.dx[i] / dy,
                    };
                }
            }
        }
        if ends[0].y >= ends[1].y {
            return Err(center_right());
        }
        for (j, end) in ends.iter_mut().enumerate() {
            let (u, r) = self.corner(std::array::from_fn(|i| p[i] + end.y * self.dy[i]));
            if r == 0. || dot(u, u) < r * r {
                continue;
            }
            // Intersect the line with the circular corner in its own axes.
            let norm = dot(self.dy, self.dy);
            let cross = u[0] * self.dy[1] - u[1] * self.dy[0];
            let disc = r * r * norm - cross * cross;
            if disc <= 0. {
                return Err(center_right());
            }
            let along = if j == 0 { -disc.sqrt() } else { disc.sqrt() };
            let t = (along - dot(u, self.dy)) / norm;
            let normal = std::array::from_fn(|i| u[i] + t * self.dy[i]);
            end.y += t;
            end.slope = -dot(normal, self.dx) / along;
        }
        Ok(ends)
    }
}
#[derive(Clone, Copy)]
struct End {
    y: f64,
    slope: f64,
}
impl End {
    fn flat(y: f64) -> Self {
        Self { y, slope: 0. }
    }
}
struct Guard<'a> {
    window: Window,
    parent: Option<&'a Guard<'a>>,
}
#[derive(Clone, Copy)]
pub(super) struct Area<'a> {
    bounds: [Pair; 2],
    guards: Option<&'a Guard<'a>>,
    point: bool,
}
impl Area<'_> {
    pub(super) fn new(f: Rect) -> Self {
        Self {
            bounds: [[0., f.width.into()], [0., f.height.into()]],
            guards: None,
            point: f.width == 0. || f.height == 0.,
        }
    }
    pub(super) fn within(self, w: Window, then: impl FnOnce(Area<'_>) -> bool) -> bool {
        let Some(bounds) = w.bounds() else {
            return false;
        };
        if self.point {
            return w.contains_origin() && then(self);
        }
        let bounds = std::array::from_fn(|i| {
            [
                self.bounds[i][0].max(bounds[i][0]),
                self.bounds[i][1].min(bounds[i][1]),
            ]
        });
        if bounds.iter().any(|[lo, hi]| lo >= hi) {
            return false;
        }
        let rect = w.radii == [0.; 4]
            && ((w.dx[1] == 0. && w.dy[0] == 0.) || (w.dx[0] == 0. && w.dy[1] == 0.));
        let guard = Guard {
            window: w,
            parent: self.guards,
        };
        then(Area {
            bounds,
            guards: if rect { self.guards } else { Some(&guard) },
            point: false,
        })
    }
    pub(super) fn visible(self) -> bool {
        let Some(first) = self.guards else {
            return true;
        };
        let [[left, right], [top, bottom]] = self.bounds;
        let mut range = left.abs().to_bits()..right.to_bits(); // canonical +0, x in [0, width]
        let mut x = (left + right) * 0.5;
        // The intersection's lower edge is convex, its upper edge concave.
        // Each failed slice eliminates one half of the representable x values;
        // at most 64 probes, only for rounded/turned partial intersections.
        while x > f64::from_bits(range.start) && x < f64::from_bits(range.end) {
            let (mut lower, mut upper) = (End::flat(top), End::flat(bottom));
            let mut miss = None;
            for g in std::iter::successors(Some(first), |g| g.parent) {
                match g.window.section(x) {
                    Ok([lo, hi]) => {
                        if lo.y > lower.y {
                            lower = lo;
                        }
                        if hi.y < upper.y {
                            upper = hi;
                        }
                    }
                    Err(right) => {
                        miss = Some(right);
                        break;
                    }
                }
            }
            let right = if let Some(right) = miss {
                right
            } else if lower.y < upper.y {
                return true;
            } else if upper.slope == lower.slope {
                return false;
            } else {
                upper.slope > lower.slope
            };
            if right {
                range.start = x.to_bits();
            } else {
                range.end = x.to_bits();
            }
            x = f64::from_bits(range.start + (range.end - range.start) / 2);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn area(w: f32, h: f32) -> Area<'static> {
        Area::new(Rect {
            width: w,
            height: h,
            ..Rect::default()
        })
    }
    #[test]
    fn tilted_bounds_and_edge_contact_are_not_area_intersections() {
        for (offset, expected) in [(13., false), (10., false), (9.999, true)] {
            let diamond = Window {
                origin: [5., offset],
                dx: [1., 1.],
                dy: [-1., 1.],
                size: [10.; 2],
                radii: [0.; 4],
            };
            assert_eq!(area(1., 1.).within(diamond, |a| a.visible()), expected);
        }
    }
    #[test]
    fn curved_windows_need_one_common_patch_not_two_individual_intersections() {
        // Radius-five windows centered at (1,1) and (7,9) touch at (4,5).
        // Each has substantial area in the anchor, but no *common* area.
        for (dx, visible) in [(0., false), (-0.001, true), (0.001, false)] {
            let first = Window {
                origin: [4., 4.],
                dx: [1., 0.],
                dy: [0., 1.],
                size: [10.; 2],
                radii: [5.; 4],
            };
            let turned = Window {
                origin: [14., -2. - dx],
                dx: [0., 1.],
                dy: [-1., 0.],
                size: [10.; 2],
                radii: [5.; 4],
            };
            assert_eq!(
                area(20., 20.).within(first, |a| a.within(turned, |a| a.visible())),
                visible
            );
        }
    }
    #[test]
    fn empty_attachments_use_the_origin_in_the_actual_window() {
        for (point, expected) in [
            ([0., 0.], false),
            ([2., 2.], true),
            ([5., 0.], true),
            ([10., 5.], false),
        ] {
            let window = Window {
                origin: point,
                dx: [1., 0.],
                dy: [0., 1.],
                size: [10.; 2],
                radii: [5.; 4],
            };
            assert_eq!(area(0., 0.).within(window, |a| a.visible()), expected);
        }
    }
}
