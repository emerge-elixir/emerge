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
    minimum: f32, // additional outer constraint supplied by the group, in device pixels
    scale: f32,
    raw: &'a [u8],
}
impl<'a> Slot<'a> {
    pub(super) fn new(e: &'a Elements, k: Key, length: Length, content: f32) -> Self {
        Self {
            length,
            content,
            minimum: 0.,
            scale: e.slots[k].admitted.scale,
            raw: if matches!(length, Length::Formula(_)) {
                e.raw(k)
            } else {
                &[]
            },
        }
    }
    // A fill can grow from this measurement; a non-fill charges it as-is.
    pub(super) fn with_basis(mut self, outer: f32) -> Self {
        if self.length.weight().is_some() {
            self.minimum = outer;
        } else {
            self.content = outer;
        }
        self
    }
    fn eval(self, share: impl Fn(f64) -> f32) -> f32 {
        self.length
            .allotted(self.raw, self.content, self.scale, share)
            .max(self.minimum)
    }
    fn active(self, w: f64, r: &mut Region) -> Term {
        let term = if let Length::Formula(f) = self.length {
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
        };
        if self.minimum > 0. {
            r.select(false, term, Term::Px(self.minimum))
        } else {
            term
        }
    }
}

impl<'e> Mounted<'e, Children> {
    pub(super) fn width_slot(&self, cx: &FlowCx<'_>, c: Key) -> Slot<'e> {
        Slot::new(
            self.elements,
            c,
            self.elements.attrs[c].sizing.width,
            cx.measured_width(c),
        )
    }
}

impl FlowCx<'_> {
    /// The caller has already chosen group membership and the child measurements.
    pub(super) fn assign_group<'e>(
        &mut self,
        children: &[Key],
        available: Option<f32>,
        charge: f32,
        break_before: bool,
        slot: impl Fn(&Self, Key) -> Slot<'e>,
    ) {
        let split = FillShares::group(available, charge, children, |c| slot(self, c));
        for (i, &c) in children.iter().enumerate() {
            let s = slot(self, c);
            let width = split
                .and_then(|shares| shares.offer(s))
                .unwrap_or(s.content);
            self.assign_width(c, width, i == 0 && break_before);
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

#[derive(Clone, Copy, PartialEq)]
pub(in crate::tree) enum FillShares {
    Device(f32),
    Relative { largest: f64, pixels_per_unit: f64 },
    // sqrt(unit) at a boundary of a flat or exhausted interval.
    Boundary(f64),
}
impl FillShares {
    pub(super) fn group<'a>(
        available: Option<f32>,
        gap: f32,
        children: &[Key],
        slot: impl Fn(Key) -> Slot<'a>,
    ) -> Option<Self> {
        let inner = available?;
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
                bounded |= s.minimum > 0.
                    || s.length.bounds().is_some()
                    || matches!(s.length, Length::Formula(_));
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
    // A fixed-height Column can compute this group before width Flow when
    // all slots are pixels or fills that do not read content. Use exactly
    // the same solver and device rounding as the final Place group.
    fn declared_slot(e: &Elements, c: Key) -> Slot<'_> {
        let h = e.attrs[c].sizing.height;
        let basis = h.resolve(0., || e.slots[c].admitted.scale);
        Slot::new(e, c, h, basis)
    }
    pub(in crate::tree) fn declared_column(e: &Elements, k: Key) -> Option<Self> {
        let b = e.attrs[k].sizing;
        let Length::Px(px) = b.height else {
            return None;
        };
        let kids = e.children(k);
        if !kids
            .iter()
            .any(|&c| e.slots[c].shape.reads_group_height(e.attrs[c].sizing, b))
            || !kids
                .iter()
                .all(|&c| e.attrs[c].sizing.height.declared_height_slot())
        {
            return None;
        }
        let s = e.slots[k].admitted.scale;
        let inner = (scale::pixels(px, s)
            - (scale::pixels(b.padding[0], s) + scale::pixels(b.padding[2], s)))
        .max(0.);
        let charge = scale::pixels(e.attrs[k].main_spacing(linear::Y).charge(), s);
        Self::group(Some(inner), charge, kids, |c| Self::declared_slot(e, c))
    }
    pub(in crate::tree) fn assigned_height(self, e: &Elements, c: Key) -> Option<f32> {
        self.offer(Self::declared_slot(e, c))
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
