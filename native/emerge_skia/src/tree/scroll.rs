//! Layout boxes stay unscrolled. A port projects its live fraction onto the
//! latest range; the display gets only an immutable receipt and geometry.
use super::*;
mod visibility;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
pub(super) use visibility::{BandPart, Facts, Query, Reach, delegated, order_port};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Proposal {
    Fraction(f64),
    Target { token: u64, id: u64 },
    Pixels(f64), // older EMRG pixel-offset attributes, not Scroll.scroll_x/y
}
impl Proposal {
    fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Target { token: a, .. }, Self::Target { token: b, .. }) => a == b,
            _ => self == other,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Spec {
    pub enabled: [bool; 2],
    pub position: [Option<Proposal>; 2],
    pub emit: bool,
}
impl Spec {
    pub fn port(self) -> bool {
        self.enabled != [false; 2]
    }
    pub fn align(self, axis: usize, alignment: Alignment, room: f32, occupied: f32) -> f32 {
        let offset = alignment.offset(room, occupied);
        if self.enabled[axis] {
            offset.max(0.)
        } else {
            offset
        }
    }
    pub fn tracked(self) -> bool {
        self.port() || self.position != [None; 2]
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Progress {
    pub pos: f64,
    pub epoch: u64, // an app replacement or enable/disable supersedes old input
    pub input: u64, // native receipt; installations can trail multiple dispatches
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct View {
    pub axes: [Progress; 2],
    pub range: [f32; 2],
    pub emit: bool,
    pub hover: Option<usize>,
}
impl View {
    pub(super) fn offset(self) -> [f32; 2] {
        std::array::from_fn(|i| (self.axes[i].pos * f64::from(self.range[i])) as f32)
    }
}

// Only emitted callbacks have expectations. Expire from the head; repeated
// samples on the stationary axis (of a biaxial callback) use one entry.
#[derive(Default)]
struct Echoes(VecDeque<(Instant, f64)>);
impl Echoes {
    fn prune(&mut self, now: Instant) {
        while self
            .0
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= Duration::from_secs(5))
        {
            self.0.pop_front();
        }
    }
    fn record(&mut self, now: Instant, value: f64) {
        self.prune(now);
        if let Some((at, previous)) = self.0.back_mut()
            && *previous == value
        {
            *at = now;
        } else {
            self.0.push_back((now, value));
        }
    }
    fn consume(&mut self, now: Instant, value: f64) -> bool {
        self.prune(now);
        if let Some(i) = self.0.iter().position(|(_, v)| *v == value) {
            self.0.drain(..=i);
            true
        } else {
            false
        }
    }
}
/// Coalesce movements, not callback expectations. The actor takes ownership
/// before processing; no frame, Elixir term or tree is borrowed by dispatch.
#[derive(Default)]
pub(crate) struct Update {
    latest: [Option<Progress>; 2],
    pub hover: Option<Option<usize>>,
    echoes: [Echoes; 2],
}
impl Update {
    pub fn record(&mut self, axes: [Progress; 2], moved: [bool; 2], callback: Option<Instant>) {
        for (i, p) in axes.into_iter().enumerate() {
            if moved[i] {
                self.latest[i] = Some(p);
            }
            if let Some(at) = callback {
                self.echoes[i].record(at, p.pos);
            }
        }
    }
}
#[derive(Default)]
struct State {
    visible_children: visibility::Window,
    recording: paint::Window,
    view: View,
    spec: Spec,
    read_range: bool,
    echoes: [Echoes; 2],
}
impl State {
    fn forget_order(&mut self) {
        self.visible_children = Default::default();
        self.recording = Default::default();
    }
}
#[derive(Default)]
pub(super) struct Ports {
    states: HashMap<Key, State>,
    entries: SecondaryMap<Key, visibility::Entry>,
    #[cfg(test)]
    pub exhaustive: bool,
    #[cfg(test)]
    pub counts: visibility::Counts,
    serial: u64,
}
impl Ports {
    pub fn reserve(&mut self, n: usize, keys: usize) -> Result<(), String> {
        if n != 0 {
            self.entries
                .try_set_capacity(keys)
                .map_err(pool::capacity_error)?;
        }
        self.states
            .try_reserve(n.saturating_sub(self.states.len()))
            .map_err(pool::capacity_error)
    }
    pub fn remove(&mut self, k: Key) {
        self.states.remove(&k);
    }
    pub fn evict(&mut self, k: Key) {
        self.remove(k);
        self.entries.remove(k);
    }
    pub fn inherit(&mut self, old: &mut Self, version: &declaration::Records) {
        self.serial = old.serial;
        self.states.extend(
            old.states
                .drain()
                .filter(|(k, _)| {
                    version
                        .get(*k)
                        .is_some_and(|r| r.block.attrs.scroll.tracked())
                })
                .map(|(k, mut s)| {
                    s.read_range = true;
                    s.visible_children = Default::default();
                    s.recording = Default::default();
                    (k, s)
                }),
        );
    }
    pub fn resize(&mut self, k: Key) {
        if let Some(s) = self.states.get_mut(&k) {
            s.read_range = true;
        }
    }
    pub fn recorded(&self, k: Key) -> Option<&paint::Window> {
        self.states
            .get(&k)
            .map(|s| &s.recording)
            .filter(|w| w.key.is_some())
    }
    pub fn recording(&mut self, k: Key) -> Option<&mut paint::Window> {
        self.states.get_mut(&k).map(|s| &mut s.recording)
    }
    pub fn view(&self, k: Key) -> View {
        self.states.get(&k).map_or(View::default(), |s| s.view)
    }
    pub fn settle(
        &mut self,
        e: &Elements,
        k: Key,
        local: Rect,
        frames: &SecondaryMap<Key, Rect>,
        scale: f32,
    ) -> View {
        let spec = e.attrs[k].scroll;
        if !spec.tracked() {
            return View::default();
        }
        let state = self.states.entry(k).or_default();
        let [t, r, b, l] = e.attrs[k].sizing.padding.map(|v| scale::pixels(v, scale));
        if std::mem::take(&mut state.read_range) || state.spec.enabled != spec.enabled {
            // Only the changed scrollport reads its direct children. A nested
            // viewport contributes its box rather than its own overflow.
            #[cfg(test)]
            {
                self.counts.range_reads += e.children(k).len();
            }
            state.view.range =
                e.children(k)
                    .iter()
                    .map(|&c| frames[c])
                    .fold([0f32; 2], |max, f| {
                        let overflow = [
                            ((f.x - local.x) + f.width) + r - local.width,
                            ((f.y - local.y) + f.height) + b - local.height,
                        ];
                        std::array::from_fn(|i| {
                            if spec.enabled[i] {
                                max[i].max(coordinate(overflow[i]))
                            } else {
                                0.
                            }
                        })
                    });
        }
        for i in 0..2 {
            let range = f64::from(state.view.range[i]);
            let p = &mut state.view.axes[i];
            let fresh = spec.position[i]
                .filter(|v| !state.spec.position[i].is_some_and(|old| old.same(*v)));
            let jump = if !spec.enabled[i] {
                None
            } else {
                match fresh {
                    Some(Proposal::Fraction(v))
                        if state.echoes[i].0.is_empty()
                            || !state.echoes[i].consume(Instant::now(), v) =>
                    {
                        Some(v)
                    }
                    Some(Proposal::Target { id, .. }) => target(e, k, id, i).map(|c| {
                        let f = frames[c];
                        fraction([f.x - local.x - l, f.y - local.y - t][i] as f64, range)
                    }),
                    Some(Proposal::Pixels(v)) => Some(fraction(v * f64::from(scale), range)),
                    _ => None,
                }
            };
            // An equivalent proposal (including a target already at this
            // edge) must not cancel a gesture on the installed epoch.
            if state.spec.enabled[i] != spec.enabled[i] || jump.is_some_and(|v| v != p.pos) {
                self.serial += 1;
                p.epoch = self.serial;
                p.pos = jump.unwrap_or(0.);
            }
        }
        state.view.emit = spec.emit;
        state.spec = spec;
        state.view
    }
}
fn fraction(offset: f64, range: f64) -> f64 {
    if range > 0. {
        (offset / range).clamp(0., 1.)
    } else {
        0.
    }
}
fn target(e: &Elements, host: Key, id: u64, axis: usize) -> Option<Key> {
    let child = e.live(id)?;
    let mut k = child;
    loop {
        k = e.layout_parent(k)?;
        if k == host {
            return Some(child);
        }
        if e.attrs[k].scroll.enabled[axis] {
            return None;
        }
    }
}
impl Tree {
    pub(crate) fn scroll_input(&mut self, attachment: Attachment, update: Update) -> bool {
        let k = attachment.0;
        if !self
            .elements
            .slots
            .get(k)
            .is_some_and(|m| m.attachment == attachment.1 && m.detached.is_none())
        {
            return false;
        }
        let Some(s) = self.layers.scroll.states.get_mut(&k) else {
            return false;
        };
        let mut changed = false;
        if let Some(hover) = update.hover
            && hover != s.view.hover
        {
            s.view.hover = hover;
            changed = true;
        }
        for (i, echoes) in update.echoes.into_iter().enumerate() {
            for (at, value) in echoes.0 {
                s.echoes[i].record(at, value);
            }
            if let Some(p) = update.latest[i]
                && p.epoch == s.view.axes[i].epoch
                && p.input > s.view.axes[i].input
            {
                s.view.axes[i] = p;
                changed = true;
            }
        }
        if changed {
            if self.output.fits.has_drafts() {
                self.output.cancel_fits(&mut self.elements);
            }
            self.layers.enqueue(Pass::PaintContext, k, &self.elements);
            if self.layers.scroll.recorded(k).is_some() {
                self.layers.enqueue(Pass::Project, k, &self.elements);
            } else {
                self.layers
                    .invalidate_composition(k, self.elements.depth(k), paint::Outputs::Both);
            }
        }
        changed
    }
}
