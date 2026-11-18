//! Solve sum(clamp(weight × unit)) = remaining outer pixels. All shapes read
//! the same scalar result; there is no second child order or per-child scratch.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) enum FillShares {
    Device(f32),
    Relative { largest: f64, pixels_per_unit: f64 },
    // sqrt(unit) at a boundary of a flat or exhausted interval.
    Boundary(f64),
}
#[derive(Clone, Copy)]
enum Limit {
    Cap(f32),
    Floor(f32),
}
impl FillShares {
    pub(super) fn split(
        policy: Length,
        inner: f32,
        gap: f32,
        children: &[Key],
        length: impl Fn(Key) -> Length,
        basis: impl Fn(Key) -> f32,
        scale: impl Fn(Key) -> f32,
    ) -> Option<Self> {
        if !policy.definite() {
            return None;
        }
        let (mut fixed, mut total, mut largest) = (0., 0., 0f64);
        let mut device_weights = true;
        let mut bounded = false;
        for &c in children {
            let l = length(c);
            if let Some(w) = l.weight() {
                largest = largest.max(w);
                let w = w as f32;
                total += w;
                device_weights &= w.is_normal();
                bounded |= l.bound_px().is_some();
            } else {
                fixed += basis(c);
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
                .filter_map(|&c| length(c).weight())
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
        Some(initial.redistribute(f64::from(room), || {
            children.iter().filter_map(|&c| {
                let l = length(c);
                let w = l.weight()?;
                let bound = match l {
                    Length::Capped { px, .. } => Some(Limit::Cap(scale::pixels(px, scale(c)))),
                    Length::Floored { px, .. } => Some(Limit::Floor(scale::pixels(px, scale(c)))),
                    _ => None,
                };
                Some((w, bound))
            })
        }))
    }
    fn share(self, w: f64) -> f32 {
        match self {
            Self::Device(unit) => unit * w as f32,
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
    pub(super) fn offer(self, l: Length, scale: impl FnOnce() -> f32) -> Option<f32> {
        Some(l.resolve(self.share(l.weight()?), scale))
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
    fn redistribute<I>(self, room: f64, fills: impl Fn() -> I) -> Self
    where
        I: Iterator<Item = (f64, Option<Limit>)>,
    {
        // Between successive limit/weight breakpoints, the equation is linear:
        // room = sum(locked pixel sizes) + unit * sum(free weights). Search
        // for that interval, then divide the actual residual, not a rounded
        // trial extent. sqrt(unit) orders the breakpoints without overflowing
        // or underflowing even for a subnormal weight and a maximum f32 cap.
        let mut probe = self.sqrt_unit();
        let (mut low, mut high) = (0, f64::INFINITY.to_bits());
        let mut first = true;
        loop {
            let mut free = Weights::default();
            let mut locked = Charges::default();
            let mut floors = Charges::default();
            let (mut left, mut right) = (0f64, f64::INFINITY);
            let mut changed = false;
            for (w, bound) in fills() {
                if let Some(bound) = bound {
                    let px = match bound {
                        Limit::Cap(px) | Limit::Floor(px) => px,
                    };
                    let at = f64::from(px).abs().sqrt() / w.sqrt();
                    let past = at <= probe;
                    if past {
                        left = left.max(at);
                    } else {
                        right = right.min(at);
                    }
                    let frozen = match bound {
                        Limit::Cap(_) => {
                            changed |= first && self.share(w) > px;
                            past
                        }
                        Limit::Floor(_) => {
                            if first {
                                floors.add(f64::from(px));
                                changed |= self.share(w) < px;
                            }
                            !past
                        }
                    };
                    if frozen {
                        locked.add(f64::from(px));
                        continue;
                    }
                }
                free.add(w);
            }
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
                    return Self::Boundary(left); // flat interval, including all caps exhausted
                }
            }
            if residual >= 0. && target > probe {
                low = right.to_bits();
            } else {
                high = left.to_bits();
            }
            // Includes floors exhausting the host (high = 0). Otherwise,
            // after one trial of the analytic target, halve the positive f64
            // bit interval. At most 64 bisections, independent of child count.
            if high.saturating_sub(low) <= 1 {
                return Self::Boundary(f64::from_bits(low.min(high)));
            }
            let key = target.to_bits();
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
