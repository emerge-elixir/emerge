//! Actor-local pose overlay. A future Animation port supplies tracks, not a
//! succession of authored patches. Endpoints are acknowledged by the owner of
//! output progress; keeping a drawing or throwing it away cannot acknowledge.
use super::*;
use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub(crate) struct Endpoints(Arc<[Arc<OnceLock<Instant>>]>);
impl Endpoints {
    pub fn observe(&self, at: Instant) {
        for point in self.0.iter() {
            let _ = point.set(at);
        }
    }
}
#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))] // Construction belongs to the separate Animation port.
pub(crate) enum Property {
    OffsetX(f64, f64),
    Width(f64, f64),
    Background([u8; 4], [u8; 4]),
}
impl Property {
    fn apply(self, t: f64, a: &mut Attrs) {
        let lerp = |x, y| x + (y - x) * t;
        match self {
            Self::OffsetX(x, y) => a.offset[0] = lerp(x, y),
            Self::Width(x, y) => a.sizing.width = Length::Px(lerp(x, y)),
            Self::Background(x, y) => {
                a.background = Some(u32::from_be_bytes(std::array::from_fn(|i| {
                    lerp(f64::from(x[i]), f64::from(y[i])).round() as u8
                })))
            }
        }
    }
}
struct Track {
    key: Key,
    property: Property,
    start: Instant,
    duration: Duration,
    first: Option<Arc<OnceLock<Instant>>>,
    end: Arc<OnceLock<Instant>>,
    last: Option<f64>,
    block: Option<Arc<declaration::Block>>,
}
#[derive(Default)]
pub(crate) struct Motion(Vec<Track>);
impl Motion {
    #[cfg(test)]
    pub(in crate::tree) fn linear(
        &mut self,
        key: Key,
        property: Property,
        start: Instant,
        duration: Duration,
        first: bool,
    ) {
        self.0.push(Track {
            key,
            property,
            start,
            duration,
            first: first.then(Arc::default),
            end: Arc::default(),
            last: None,
            block: None,
        });
    }
    pub fn active(&self) -> bool {
        self.0.iter().any(|t| t.end.get().is_none())
    }
    pub fn sample(&mut self, tree: &mut Tree, at: Instant) -> bool {
        let mut changed = false;
        self.0.retain_mut(|t| {
            let e = &mut tree.elements;
            let Some(source) = e
                .attrs
                .version
                .as_ref()
                .and_then(|v| v.get(t.key))
                .map(|r| r.block.clone())
            else {
                return false;
            };
            let elapsed = t.first.as_ref().map_or_else(
                || at.saturating_duration_since(t.start),
                |f| {
                    f.get()
                        .map_or(Duration::ZERO, |s| at.saturating_duration_since(*s))
                },
            );
            let u = if t.first.as_ref().is_some_and(|p| p.get().is_none()) {
                0. // Even a zero-duration transition can require its initial pose.
            } else if t.duration.is_zero() {
                1.
            } else {
                (elapsed.as_secs_f64() / t.duration.as_secs_f64()).min(1.)
            };
            if t.last != Some(u)
                || t.block
                    .as_ref()
                    .is_none_or(|old| !Arc::ptr_eq(old, &source))
                || !e.attrs.poses.contains_key(&t.key)
            {
                let mut pose = source.attrs.clone();
                t.property.apply(u, &mut pose);
                let old = e.attrs.poses.insert(t.key, Arc::new(pose));
                if let Some(attrs) = old {
                    let raw = t.block.as_ref().expect("prior sampled block").clone();
                    e.slots[t.key]
                        .change
                        .pose_before
                        .get_or_insert((attrs, raw));
                } else if !e.slots[t.key].change.cold && e.slots[t.key].change.before.is_none() {
                    e.slots[t.key].change.pose_before =
                        Some((Arc::new(source.attrs.clone()), source.clone()));
                }
                e.touch(t.key);
                t.last = Some(u);
                t.block = Some(source);
                changed = true;
            }
            true // The terminal *value* stays resolved without an active clock.
        });
        changed
    }
    pub fn endpoints(&self) -> Endpoints {
        if self.0.is_empty() {
            return Endpoints::default();
        }
        Endpoints(
            self.0
                .iter()
                .flat_map(|t| {
                    t.first
                        .iter()
                        .filter(|p| t.last == Some(0.) && p.get().is_none())
                        .chain((t.last == Some(1.) && t.end.get().is_none()).then_some(&t.end))
                })
                .cloned()
                .collect(),
        )
    }
}
