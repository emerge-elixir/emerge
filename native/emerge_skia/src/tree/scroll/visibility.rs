//! Leaf-first reach in unscrolled paint coordinates. The ordinal range tree
//! is implicit in the *existing* child slice: its midpoint holds the union of
//! that slice. No copied keys, per-port Vec, fixed pitch or second dirty set.
use super::*;
use paint::{Composition, PaintContext};
use skia_safe::Rect as Box2;

#[derive(Clone, Copy, Default, PartialEq)]
enum Extent {
    #[default]
    Empty,
    Box(Box2),
    Unknown,
}
impl Extent {
    fn checked(r: Box2) -> Self {
        if !r.is_finite() {
            Self::Unknown
        } else if r.is_empty() {
            Self::Empty
        } else {
            Self::Box(r)
        }
    }
    fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Empty, x) | (x, Self::Empty) => x,
            (Self::Box(mut a), Self::Box(b)) => {
                a.join(b);
                Self::Box(a)
            }
            _ => Self::Unknown,
        }
    }
    fn map(self, f: impl FnOnce(Box2) -> Box2) -> Self {
        match self {
            Self::Box(r) => match Self::checked(f(r)) {
                // These inputs had area before the transform. At a distant
                // pivot their edges may round together, yet separate again
                // once Project uses the scrolled origin. This isn't a miss.
                Self::Empty => Self::Unknown,
                mapped => mapped,
            },
            other => other,
        }
    }
    fn intersects(self, window: Box2) -> bool {
        match self {
            Self::Empty => false,
            Self::Unknown => true,
            Self::Box(r) => r.intersects(window),
        }
    }
}
/// Includes local, behind and exported contributions. Ordinary host clips may
/// discard some of these, never more than these. Shadows use a separate strip.
#[derive(Clone, Copy, Default, PartialEq)]
pub(in crate::tree) struct Reach {
    ink: Extent,
    hit: Extent,
    shadow: Extent,
}
impl Reach {
    pub(in crate::tree) fn union(self, b: Self) -> Self {
        Self {
            ink: self.ink.union(b.ink),
            hit: self.hit.union(b.hit),
            shadow: self.shadow.union(b.shadow),
        }
    }
    pub(in crate::tree) fn side_changed(self, other: Self, side: usize) -> bool {
        if side == 0 {
            self.ink != other.ink || self.shadow != other.shadow
        } else {
            self.hit != other.hit
        }
    }
    fn body(self) -> Extent {
        self.ink.union(self.hit)
    }
    pub(in crate::tree) fn compose(
        e: &Elements,
        k: Key,
        ctx: PaintContext,
        overhang: f32,
        ordinary: Self,
        mounts: Self,
    ) -> Self {
        let a = &e.attrs[k];
        let shape = e.slots[k].shape;
        let f = ctx.frame;
        let raw = Box2::from_xywh(f.x, f.y, f.width, f.height);
        // A narrow positive layout box can collapse to coincident f32 edges
        // far down a document, then regain its width near the scrolled origin.
        let frame = if f.width > 0. && f.height > 0. && raw.is_empty() {
            Extent::Unknown
        } else {
            Extent::checked(raw)
        };
        let pixel = 1. / ctx.zoom;
        let body = if shape == Shape::Paragraph || shape == Shape::Text || a.has_art() {
            // The paragraph clips ordinary continuations; its floating/escaping
            // children contribute separately. Include AA and fitted bitmap bleed.
            let pad = overhang + 2. * pixel;
            frame.union(Extent::checked(raw.with_outset((pad, pad))))
        } else if a.background.is_some() || a.decor.active() || ctx.bars {
            frame
        } else {
            Extent::Empty
        };
        let shadow = if a.decor.shadows.len == 0 {
            Extent::Empty
        } else {
            let pad = (4. * scale::pixels(a.decor.max_shadow, ctx.scale) + 2.) * pixel.max(1.);
            frame.union(Extent::checked(raw.with_outset((pad, pad))))
        };
        let hit = if a.click
            || shape == Shape::Paragraph
            || ctx.bars
            || e.slots[k].slot.is_some_and(Slot::escapes)
        {
            frame
        } else {
            Extent::Empty
        };
        let children = if ctx.scrollport {
            // Its ordinary branches (including their escapes) cannot cross the
            // stationary inner box, except ordinary shadows on the free axis.
            Self {
                ink: frame,
                hit: frame,
                shadow: if ordinary.shadow == Extent::Empty {
                    Extent::Empty
                } else {
                    Extent::Unknown
                },
            }
        } else {
            ordinary
        };
        let mut reach = Self {
            ink: body,
            hit,
            shadow,
        }
        .union(children)
        .union(mounts);
        if a.posed() {
            let m = a.pose.matrix(f);
            let mapped = |r: Box2| m.map_rect(r).0;
            reach = Self {
                ink: reach.ink.map(mapped),
                hit: reach.hit.map(mapped),
                shadow: reach.shadow.map(mapped),
            };
        }
        reach
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(in crate::tree) struct Facts {
    pub reach: Reach,
    pub active: [bool; 2],
    pub exports: [bool; 2],
}
impl Facts {
    fn union(self, b: Self) -> Self {
        Self {
            reach: self.reach.union(b.reach),
            active: std::array::from_fn(|i| self.active[i] || b.active[i]),
            exports: std::array::from_fn(|i| self.exports[i] || b.exports[i]),
        }
    }
}
fn edges(r: Box2, axis: usize) -> (f32, f32) {
    if axis == 0 {
        (r.left, r.right)
    } else {
        (r.top, r.bottom)
    }
}
#[derive(Clone, Copy, Default)]
struct Summary {
    facts: Facts,
    ordered: [bool; 2],
    // The common interval on the other axis certifies that a single run is enough.
    common: Box2,
}
impl Summary {
    fn leaf(facts: Facts) -> Self {
        match facts.reach.body() {
            Extent::Box(r) if facts.reach.shadow == Extent::Empty => Self {
                facts,
                ordered: [true; 2],
                common: r,
            },
            _ => Self {
                facts,
                ..Self::default()
            },
        }
    }
    fn then(self, other: Self) -> Self {
        let ordered = std::array::from_fn(|i| {
            self.ordered[i]
                && other.ordered[i]
                && matches!((self.facts.reach.body(), other.facts.reach.body()),(Extent::Box(l),Extent::Box(r)) if edges(l,i).1 <= edges(r,i).0)
        });
        Self {
            facts: self.facts.union(other.facts),
            ordered,
            common: Box2::new(
                self.common.left.max(other.common.left),
                self.common.top.max(other.common.top),
                self.common.right.min(other.common.right),
                self.common.bottom.min(other.common.bottom),
            ),
        }
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(super) struct Run {
    pub first: usize,
    pub end: usize,
}
impl Run {
    fn nonempty(self) -> bool {
        self.first < self.end
    }
}
#[derive(Clone, Copy, Default)]
pub(super) struct Entry {
    ordinal: usize,
    summary: Summary,
    previous: Run,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Window {
    order: Option<(Key, usize)>,
    query: Option<Query>,
    last: Run, // A monotone list uses only this pair. Extra runs link backwards by ordinal.
    multiple: bool,
}
fn delegated(e: &Elements, port: Key) -> Option<Key> {
    if e.slots[port].shape != Shape::El {
        return None;
    }
    let &child = e.children(port).first()?;
    (!e.attrs[child].scroll.port()
        && matches!(
            e.slots[child].shape,
            Shape::Column
                | Shape::Row
                | Shape::WrappedColumn
                | Shape::WrappedRow
                | Shape::TextColumn
                | Shape::Paragraph
        ))
    .then_some(child)
}
pub(in crate::tree) fn order_port(e: &Elements, k: Key) -> Option<Key> {
    if e.attrs[k].scroll.port() {
        delegated(e, k).is_none().then_some(k)
    } else {
        e.layout_parent(k)
            .filter(|&p| e.attrs[p].scroll.port() && delegated(e, p) == Some(k))
    }
}
#[derive(Clone, Copy, PartialEq)]
pub(in crate::tree) struct Query {
    body: Box2,
    shadow: Box2,
}
impl Query {
    pub(in crate::tree) fn new(
        e: &Elements,
        (port, p): (Key, PaintContext),
        (host, h): (Key, PaintContext),
        stable: PaintContext,
    ) -> Self {
        // An explicit Nearby guard isn't an ordinary box clip. Conversely, an
        // ancestor Row can clip ordinary boxes without clipping their exports.
        let r = match (p.child_clip, p.content_escape) {
            (a, b) if a.width > 0. && a.height > 0. && b.width > 0. && b.height > 0. => {
                paragraph::union(a, b)
            }
            (a, _) if a.width > 0. && a.height > 0. => a,
            (_, b) => b,
        };
        // Round the stationary device edges outwards *before* shifting them
        // into the unscrolled plane (not the fractional row edges afterwards).
        let mut body = if r.width <= 0. || r.height <= 0. {
            Box2::default()
        } else if p.transformed {
            Box2::from_xywh(r.x, r.y, r.width, r.height).with_outset((2. / p.zoom, 2. / p.zoom))
        } else {
            Box2::new(
                r.x.floor(),
                r.y.floor(),
                (r.x + r.width).ceil(),
                (r.y + r.height).ceil(),
            )
        };
        let inner = e.attrs[port].decor.inner_rect(p.frame, p.scale);
        let axes = e.attrs[port].scroll.enabled;
        let mut shadow = Box2::new(
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        );
        if axes[0] {
            shadow.left = inner.x - 1. / p.zoom;
            shadow.right = inner.x + inner.width + 1. / p.zoom;
        }
        if axes[1] {
            shadow.top = inner.y - 1. / p.zoom;
            shadow.bottom = inner.y + inner.height + 1. / p.zoom;
        }
        if port != host && e.attrs[host].posed() {
            let local = e.attrs[host].pose.local_clip(
                h.frame,
                Rect {
                    x: body.left,
                    y: body.top,
                    width: body.width(),
                    height: body.height(),
                },
            );
            body = Box2::from_xywh(local.x, local.y, local.width, local.height);
            shadow = Box2::new(
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::INFINITY,
            );
        }
        let delta = std::array::from_fn::<_, 2, _>(|i| {
            stable.offset[i] - h.offset[i]
                + if port == host {
                    h.content_scroll[i]
                } else {
                    0.
                }
        });
        // If translating the query cannot even preserve pixel edges, don't
        // use the rounded or collapsed rectangle as a rejection proof.
        if delta
            .iter()
            .any(|d| !d.is_finite() || d.abs() >= (1 << 23) as f32)
        {
            let all = Box2::new(
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::INFINITY,
            );
            return Self {
                body: all,
                shadow: all,
            };
        }
        body.offset((delta[0], delta[1]));
        shadow.offset((delta[0], delta[1]));
        Self { body, shadow }
    }
    fn admits(self, reach: Reach) -> bool {
        reach.body().intersects(self.body) || reach.shadow.intersects(self.shadow)
    }
}
#[cfg(test)]
#[derive(Default, Debug, Clone, Copy)]
pub(in crate::tree) struct Counts {
    pub content_slots: usize,
    pub range_reads: usize,
    pub compose_reads: [usize; 2],
    pub fact_roots: usize,
    pub builds: usize,
    pub refits: usize,
    pub probes: usize,
    pub refreshes: usize,
    pub rewrites: usize,
}

// Refit just the path to the changed ordinal. Build a new certificate only
// after an order change, using final placement and child composition results.
fn fit(
    entries: &mut SecondaryMap<Key, Entry>,
    parts: &SecondaryMap<Key, Composition>,
    kids: &[Key],
    base: usize,
    changed: Option<usize>,
) -> Option<Summary> {
    if kids.is_empty() {
        return None;
    }
    let mid = kids.len() / 2;
    let key = kids[mid];
    if changed.is_some_and(|i| i < base || i >= base + kids.len()) {
        return Some(entries[key].summary);
    }
    let left = fit(entries, parts, &kids[..mid], base, changed);
    let right = fit(entries, parts, &kids[mid + 1..], base + mid + 1, changed);
    let own = Summary::leaf(parts[key].facts());
    let start = left.map_or(own, |l| l.then(own));
    let summary = right.map_or(start, |r| start.then(r));
    let entry = entries.entry(key).unwrap().or_default();
    entry.ordinal = base + mid;
    entry.summary = summary;
    Some(summary)
}
impl Ports {
    pub(in crate::tree) fn exhaustive(&self) -> bool {
        #[cfg(test)]
        {
            self.exhaustive
        }
        #[cfg(not(test))]
        {
            false
        }
    }
    pub(in crate::tree) fn forget_order(&mut self, e: &Elements, k: Key) {
        if let Some(s) = self.states.get_mut(&k) {
            s.visible_children = Window::default();
        }
        if let Some(s) = e.layout_parent(k).and_then(|p| self.states.get_mut(&p))
            && s.visible_children.order.is_some_and(|(host, _)| host == k)
        {
            s.visible_children = Window::default();
        }
    }
    pub(in crate::tree) fn facts(
        &mut self,
        e: &Elements,
        k: Key,
        parts: &SecondaryMap<Key, Composition>,
    ) -> Option<Facts> {
        if self.exhaustive() {
            return None;
        }
        let port = order_port(e, k)?;
        let kids = e.children(k);
        let w = &mut self.states.get_mut(&port)?.visible_children;
        if w.order != Some((k, kids.len())) {
            fit(&mut self.entries, parts, kids, 0, None);
            *w = Window {
                order: Some((k, kids.len())),
                ..Window::default()
            };
            #[cfg(test)]
            {
                self.counts.builds += 1;
                self.counts.probes += kids.len();
            }
        }
        #[cfg(test)]
        {
            self.counts.fact_roots += usize::from(!kids.is_empty());
        }
        Some(
            kids.get(kids.len() / 2)
                .map_or(Facts::default(), |&c| self.entries[c].summary.facts),
        )
    }
    pub(in crate::tree) fn refit(
        &mut self,
        e: &Elements,
        k: Key,
        parts: &SecondaryMap<Key, Composition>,
        old: Facts,
        new: Facts,
    ) {
        if old == new {
            return;
        }
        let Some((host, port)) = e
            .layout_parent(k)
            .and_then(|h| Some((h, order_port(e, h)?)))
        else {
            return;
        };
        let Some(s) = self.states.get_mut(&port) else {
            return;
        };
        let kids = e.children(host);
        let w = &mut s.visible_children;
        if w.order != Some((host, kids.len())) {
            return;
        }
        let Some(i) = self
            .entries
            .get(k)
            .map(|x| x.ordinal)
            .filter(|&i| kids.get(i) == Some(&k))
        else {
            return;
        };
        fit(&mut self.entries, parts, kids, 0, Some(i));
        if old.reach != new.reach {
            w.query = None;
        }
        #[cfg(test)]
        {
            self.counts.refits += 1;
        }
    }
    pub(in crate::tree) fn children_rev<'a>(
        &'a self,
        e: &'a Elements,
        k: Key,
    ) -> impl Iterator<Item = &'a Key> {
        let kids = e.children(k);
        let w = self
            .states
            .get(&k)
            .filter(|s| s.visible_children.order.is_some_and(|(host, _)| host == k))
            .or_else(|| e.layout_parent(k).and_then(|p| self.states.get(&p)))
            .map(|s| s.visible_children)
            .filter(|w| w.order == Some((k, kids.len())) && w.query.is_some());
        let last = w.map_or(
            Run {
                first: 0,
                end: kids.len(),
            },
            |w| w.last,
        );
        let multiple = w.is_some_and(|w| w.multiple);
        std::iter::successors(last.nonempty().then_some(last), move |r| {
            multiple
                .then(|| self.entries[kids[r.first]].previous)
                .filter(|r| r.nonempty())
        })
        .flat_map(|r| kids[r.first..r.end].iter().rev())
    }
    pub(in crate::tree) fn select(
        &mut self,
        e: &Elements,
        host: Key,
        port: Key,
        parts: &SecondaryMap<Key, Composition>,
        query: Query,
    ) {
        let kids = e.children(host);
        // Delegation can switch at equal geometry (e.g. an El gains a kept
        // Column). Never query the previous order's ordinal certificates.
        if self.states[&port].visible_children.order != Some((host, kids.len())) {
            self.facts(e, host, parts);
        }
        let w = &mut self
            .states
            .get_mut(&port)
            .expect("settled scrollport")
            .visible_children;
        if w.query == Some(query) {
            return;
        }
        w.query = Some(query);
        let mut probes = 0;
        let summary = kids.get(kids.len() / 2).map(|&k| self.entries[k].summary);
        let axis = summary.and_then(|s| {
            (0..2).find(|&i| {
                let (lo, hi) = edges(s.common, 1 - i);
                let (q0, q1) = edges(query.body, 1 - i);
                s.ordered[i] && lo < q1 && hi > q0 && lo < hi
            })
        });
        let (last, multiple, changed_links) =
            if summary.is_none_or(|s| !query.admits(s.facts.reach)) {
                (Run::default(), false, false)
            } else if let Some(axis) = axis {
                let (lo, hi) = edges(query.body, axis);
                let mut edge = |i, n| {
                    probes += 1;
                    let Extent::Box(r) = parts[kids[i]].facts().reach.body() else {
                        unreachable!("ordered finite interval")
                    };
                    if n {
                        edges(r, axis).1 <= lo
                    } else {
                        edges(r, axis).0 < hi
                    }
                };
                let first = cut(kids.len(), w.last.first, |i| edge(i, true));
                let end = cut(kids.len(), w.last.end, |i| edge(i, false));
                (Run { first, end }, false, false)
            } else {
                let mut runs = Runs {
                    entries: &mut self.entries,
                    parts,
                    kids,
                    query,
                    last: Run::default(),
                    multiple: false,
                    changed_links: false,
                    probes: 0,
                };
                runs.visit(0, kids.len());
                probes += runs.probes;
                (runs.last, runs.multiple, runs.changed_links)
            };
        if w.last != last || w.multiple != multiple || (multiple && changed_links) {
            w.last = last;
            w.multiple = multiple;
            #[cfg(test)]
            {
                self.counts.rewrites += 1;
            }
        }
        #[cfg(test)]
        {
            self.counts.refreshes += 1;
            self.counts.probes += 1 + probes;
        }
        let _ = probes;
    }
}
// Two nearby crossed edges are cheaper than a search; cap stepping for page
// jumps and keyed targets, including reverse scroll and variable heights.
fn cut(n: usize, saved: usize, mut before: impl FnMut(usize) -> bool) -> usize {
    let mut i = saved.min(n);
    for _ in 0..2 {
        if i > 0 && !before(i - 1) {
            i -= 1;
        } else if i < n && before(i) {
            i += 1;
        } else {
            return i;
        }
    }
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if before(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}
struct Runs<'a> {
    entries: &'a mut SecondaryMap<Key, Entry>,
    parts: &'a SecondaryMap<Key, Composition>,
    kids: &'a [Key],
    query: Query,
    last: Run,
    multiple: bool,
    changed_links: bool,
    probes: usize,
}
impl Runs<'_> {
    fn visit(&mut self, first: usize, end: usize) {
        if first == end {
            return;
        }
        let mid = first + (end - first) / 2;
        let k = self.kids[mid];
        self.probes += 1;
        if !self.query.admits(self.entries[k].summary.facts.reach) {
            return;
        }
        self.visit(first, mid);
        self.probes += 1;
        if self.query.admits(self.parts[k].facts().reach) {
            if self.last.nonempty() && self.last.end == mid {
                self.last.end += 1;
            } else {
                self.multiple |= self.last.nonempty();
                if self.entries[k].previous != self.last {
                    self.entries[k].previous = self.last;
                    self.changed_links = true;
                }
                self.last = Run {
                    first: mid,
                    end: mid + 1,
                };
            }
        }
        self.visit(mid + 1, end);
    }
}
