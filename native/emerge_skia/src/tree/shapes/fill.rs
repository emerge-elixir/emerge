//! Solve sum(child_length(unit)) = remaining outer pixels. A min/max tree is
//! piecewise either a fixed extent or a ray through zero. Find the interval
//! containing the shared unit, then divide its residual among the free rays.
//! No second child order, intermediate layout or per-child scratch.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) struct Slot<'a> {
    length: Length,
    content: f32,
    scale: f32,
    raw: &'a [u8],
}
impl<'a> Slot<'a> {
    pub(super) fn new(e: &'a Elements, k: Key, length: Length, content: f32) -> Self {
        Self {
            length,
            content,
            scale: e.slots[k].admitted.scale,
            raw: if matches!(length, Length::Formula(_)) {
                e.raw(k)
            } else {
                &[]
            },
        }
    }
    // The automatic minimum is already an outer size in device pixels. Use
    // the same bounded-share solver as Row without scaling that floor again.
    pub(super) fn line_fill(weight: f64, minimum: f32) -> Self {
        let length = if minimum == 0. {
            Length::Weighted(weight)
        } else {
            Length::BoundedFill {
                weight,
                bounds: Bounds {
                    floor: f64::from(minimum),
                    cap: f64::INFINITY,
                },
            }
        };
        Self {
            length,
            content: minimum,
            scale: 1.,
            raw: &[],
        }
    }
    pub(super) fn basis(self) -> f32 {
        self.content
    }
    fn eval(self, share: impl Fn(f64) -> f32) -> f32 {
        self.length
            .allotted(self.raw, self.content, self.scale, share)
    }
    fn active(self, w: f64, r: &mut Region) -> Term {
        if let Length::Formula(f) = self.length {
            f.eval(
                self.raw,
                Term::Px(self.content),
                |v| Term::Px(scale::pixels(v, self.scale)),
                Term::Weight,
                |min, a, b| r.select(min, a, b),
            )
        } else if let Some(b) = self.length.bounds() {
            let b = b.device(self.scale);
            let above_floor = r.select(false, Term::Weight(w), Term::Px(b.floor));
            r.select(true, above_floor, Term::Px(b.cap))
        } else {
            Term::Weight(w)
        }
    }
}

#[derive(Clone, Copy)]
enum Term {
    Px(f32),
    Weight(f64),
}
// Within [left,right), every comparison made at probe picks the same term.
struct Region {
    probe: f64,
    left: f64,
    right: f64,
}
impl Region {
    fn select(&mut self, min: bool, a: Term, b: Term) -> Term {
        use Term::*;
        let smaller_a = match (a, b) {
            (Px(x), Px(y)) => x <= y,
            (Weight(x), Weight(y)) => x <= y,
            (Weight(w), Px(px)) => self.below(w, px),
            (Px(px), Weight(w)) => !self.below(w, px),
        };
        if smaller_a == min { a } else { b }
    }
    fn below(&mut self, w: f64, px: f32) -> bool {
        // sqrt(px / weight) without overflow for any f32 size / finite f64 weight.
        let at = f64::from(px).abs().sqrt() / w.sqrt();
        if self.probe < at {
            self.right = self.right.min(at);
            true
        } else {
            self.left = self.left.max(at);
            false
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum FillShares {
    Device(f32),
    Relative { largest: f64, pixels_per_unit: f64 },
    // sqrt(unit) at a boundary of a flat or exhausted interval.
    Boundary(f64),
}
impl FillShares {
    pub(super) fn split<'a>(
        policy: Length,
        inner: f32,
        gap: f32,
        children: &[Key],
        slot: impl Fn(Key) -> Slot<'a>,
    ) -> Option<Self> {
        if !policy.definite() {
            return None;
        }
        Self::line(inner, gap, children, slot)
    }
    pub(super) fn line<'a>(
        inner: f32,
        gap: f32,
        children: &[Key],
        slot: impl Fn(Key) -> Slot<'a>,
    ) -> Option<Self> {
        let (mut fixed, mut total, mut largest) = (0., 0., 0f64);
        let mut device_weights = true;
        let mut bounded = false;
        for &c in children {
            let s = slot(c);
            if let Some(w) = s.length.weight() {
                largest = largest.max(w);
                let w = w as f32;
                total += w;
                device_weights &= w.is_normal();
                bounded |= s.length.bounds().is_some() || matches!(s.length, Length::Formula(_));
            } else {
                fixed += s.content;
            }
        }
        if largest == 0. {
            return None;
        }
        let gaps = gap * children.len().saturating_sub(1) as f32;
        let room = (inner - fixed - gaps).max(0.);
        // Preserve ordinary f32 sum/divide/multiply rounding when no bound binds.
        let unit = room / total;
        let initial = if device_weights
            && total.is_finite()
            && (unit.is_normal() || room == 0.)
            && (unit * largest as f32).is_finite()
        {
            Self::Device(unit)
        } else {
            let units: f64 = children
                .iter()
                .filter_map(|&c| slot(c).length.weight())
                .map(|w| w / largest)
                .sum();
            Self::Relative {
                largest,
                pixels_per_unit: f64::from(room) / units,
            }
        };
        if !bounded {
            return Some(initial);
        }
        Some(initial.redistribute(f64::from(room), || children.iter().map(|&c| slot(c))))
    }
    fn share(self, w: f64) -> f32 {
        match self {
            Self::Device(unit) => {
                if (w as f32).is_normal() {
                    unit * w as f32
                } else {
                    (f64::from(unit) * w) as f32
                }
            }
            Self::Boundary(root) => (root * w.sqrt()).powi(2) as f32,
            Self::Relative {
                pixels_per_unit: 0.,
                ..
            } => 0.,
            Self::Relative {
                largest,
                pixels_per_unit,
            } => (pixels_per_unit * (w / largest)) as f32,
        }
    }
    pub(super) fn offer(self, s: Slot<'_>) -> Option<f32> {
        s.length.weight().map(|_| s.eval(|w| self.share(w)))
    }
    fn sqrt_unit(self) -> f64 {
        match self {
            Self::Boundary(root) => root,
            Self::Device(u) => f64::from(u).sqrt(),
            Self::Relative {
                largest,
                pixels_per_unit,
            } => pixels_per_unit.sqrt() / largest.sqrt(),
        }
    }
    fn redistribute<'a, I>(self, room: f64, slots: impl Fn() -> I) -> Self
    where
        I: Iterator<Item = Slot<'a>>,
    {
        let mut probe = self.sqrt_unit();
        let (mut low, mut high) = (0, f64::INFINITY.to_bits());
        let mut first = true;
        loop {
            let mut free = Weights::default();
            let mut locked = Charges::default();
            let mut floors = Charges::default();
            let mut region = Region {
                probe,
                left: 0.,
                right: f64::INFINITY,
            };
            let mut changed = false;
            for s in slots() {
                let Some(w) = s.length.weight() else { continue };
                if first {
                    changed |= self.share(w) != s.eval(|v| self.share(v));
                    floors.add(f64::from(s.eval(|_| 0.)));
                }
                match s.active(w, &mut region) {
                    Term::Px(p) => locked.add(f64::from(p)),
                    Term::Weight(w) => free.add(w),
                }
            }
            let Region { left, right, .. } = region;
            if first {
                if floors.remaining(room) <= 0. {
                    return Self::Boundary(0.);
                }
                if !changed {
                    return self;
                }
            }
            let residual = locked.remaining(room);
            let next = free.split(residual.max(0.));
            let target = next.map_or(f64::INFINITY, Self::sqrt_unit);
            if residual >= 0. {
                if let Some(unit) = next {
                    if (left..=right).contains(&target) {
                        return unit;
                    }
                } else if residual == 0. || right.is_infinite() {
                    return Self::Boundary(left);
                }
            }
            if residual >= 0. && target > probe {
                low = right.to_bits();
            } else {
                high = left.to_bits();
            }
            if high.saturating_sub(low) <= 1 {
                return Self::Boundary(f64::from_bits(low.min(high)));
            }
            let key = target.to_bits();
            // Try the analytic target once, then bisect positive f64 bit keys:
            // at most 64 bisections, independent of child count or weights.
            probe = f64::from_bits(if first && key >= low && key < high {
                key
            } else {
                low.midpoint(high)
            });
            first = false;
        }
    }
}

// Don't lose a small bound next to a viewport-sized one. The residual can
// be much smaller than either room or the sum of charges.
#[derive(Default)]
struct Charges {
    sum: f64,
    error: f64,
}
impl Charges {
    fn add(&mut self, px: f64) {
        let next = self.sum + px;
        self.error += if self.sum >= px {
            (self.sum - next) + px
        } else {
            (px - next) + self.sum
        };
        self.sum = next;
    }
    fn remaining(&self, room: f64) -> f64 {
        (room - self.sum) - self.error
    }
}

// Positive weights, renormalized to the largest *free* weight. Subtracting
// frozen weights from a total would lose tiny survivors to cancellation.
#[derive(Default)]
struct Weights {
    largest: f64,
    units: f64,
}
impl Weights {
    fn add(&mut self, w: f64) {
        if w > self.largest {
            self.units *= self.largest / w;
            self.largest = w;
        }
        self.units += w / self.largest;
    }
    fn split(&self, room: f64) -> Option<FillShares> {
        (self.largest > 0.).then(|| FillShares::Relative {
            largest: self.largest,
            pixels_per_unit: room / self.units,
        })
    }
}
