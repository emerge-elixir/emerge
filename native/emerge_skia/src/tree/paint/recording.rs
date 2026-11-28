//! Local drawing/hit programs, and their immutable child order. Recording
//! uses exactly the same shape and inline emitters as the surrounding frame.
//! A sealed program owns its resources; no layout slot is a rendering input.
use super::*;
use pages::{Pages, Recycle, Ref, Stock};
mod admission;
mod materials;
mod selection;
pub(in crate::tree) use admission::{Sites, charge};
pub(super) use materials::{Swatch, Swatches};
use selection::Selection;

#[derive(Default)]
struct Payloads {
    op: Pages<(Scope, Command), 4>,
    id: Pages<u16, 32>,
    point: Pages<Point, 32>,
    hit: Pages<Hit, 4>,
    clip: Pages<(Rounded, Mask), 4>,
    plane: Pages<Plane, 2>,
    styles: Pages<Ref<Swatch>, 4>,
    drawings: Stock<Drawing>,
    targets: Stock<Hits>,
}
impl Recycle for Drawing {
    fn recycle(&mut self) {
        *self = Self::default()
    }
}
impl Recycle for Hits {
    fn recycle(&mut self) {
        *self = Self::default()
    }
}
impl Payloads {
    fn reserve(
        &mut self,
        n: usize,
        d: [usize; 8],
        h: [usize; 4],
        v: usize,
        b: Budget,
    ) -> Result<(), String> {
        self.op.reserve(n, d[0], v, b)?;
        self.id.reserve(n, d[1], v, b)?;
        self.point.reserve(n, d[2], v, b)?;
        self.hit.reserve(n, h[0], v, b)?;
        self.clip.reserve(2 * n, d[3] + h[1], v, b)?;
        self.plane.reserve(2 * n, d[6] + h[2], v, b)?;
        if d[4] != 0 {
            self.styles.reserve(n, n + d[4], v, b)?;
        }
        self.drawings
            .reserve(b.capacity(n * v), || Ok(Drawing::default()))?;
        self.targets
            .reserve(b.capacity(n * v), || Ok(Hits::default()))
    }
    fn drawing(&self, d: &Drawing) -> Option<Ref<Drawing>> {
        (!d.commands.is_empty()).then(|| {
            self.drawings.issue(|p| {
                debug_assert!(d.shadow_guards.is_empty());
                p.commands = self.op.freeze(&d.commands);
                p.ids = self.id.freeze(&d.ids);
                p.positions = self.point.freeze(&d.positions);
                p.clips.0 = self.clip.freeze(&d.clips.0);
                p.planes.0 = self.plane.freeze(&d.planes.0);
                p.materials.shared = self.styles.freeze(&d.materials.shared);
            })
        })
    }
    fn hits(&self, h: &Hits) -> Option<Ref<Hits>> {
        (!h.items.is_empty()).then(|| {
            self.targets.issue(|p| {
                debug_assert!(h.scrolls.is_empty());
                p.items = self.hit.freeze(&h.items);
                p.clips.0 = self.clip.freeze(&h.clips.0);
                p.planes.0 = self.plane.freeze(&h.planes.0);
            })
        })
    }
}

// An immutable midpoint tree of recorded ordinals. No array of this frame's
// visible handles. First exposure or an edit replaces just the search path.
#[derive(Clone, Default)]
struct Node<T: Clone> {
    children: [Option<Ref<Node<T>>>; 2],
    value: Option<T>,
}
impl<T: Clone> Recycle for Node<T> {
    fn recycle(&mut self) {
        self.value = None;
        self.children = [None, None]
    }
}
type Root<T> = Option<Ref<Node<T>>>;
fn get<T: Clone>(mut node: Option<&Node<T>>, mut n: usize, mut i: usize) -> Option<&T> {
    while let Some(a) = node {
        let m = n / 2;
        if m == i {
            return a.value.as_ref();
        };
        let right = i > m;
        node = a.children[usize::from(right)].as_deref();
        if right {
            i -= m + 1;
            n -= m + 1;
        } else {
            n = m;
        }
    }
    None
}
fn set<T: Clone + Default>(
    stock: &Stock<Node<T>>,
    node: &mut Root<T>,
    n: usize,
    i: usize,
    value: T,
) {
    debug_assert!(i < n);
    let mut a = node.as_deref().cloned().unwrap_or_default();
    let m = n / 2;
    if i == m {
        a.value = Some(value);
    } else if i < m {
        set(stock, &mut a.children[0], m, i, value);
    } else {
        set(stock, &mut a.children[1], n - m - 1, i - m - 1, value);
    }
    *node = Some(stock.issue(|p| *p = a));
}
fn visit<T: Clone, R>(
    node: Option<&Node<T>>,
    lo: usize,
    hi: usize,
    range: &Range<usize>,
    rev: bool,
    f: &mut impl FnMut(&T) -> Option<R>,
) -> Option<R> {
    if lo >= hi || range.start >= hi || range.end <= lo {
        return None;
    };
    let a = node?;
    let mid = lo + (hi - lo) / 2;
    let branch = |right: bool, f: &mut _| {
        let (l, h) = if right { (mid + 1, hi) } else { (lo, mid) };
        visit(
            a.children[usize::from(right)].as_deref(),
            l,
            h,
            range,
            rev,
            f,
        )
    };
    branch(rev, f)
        .or_else(|| {
            if range.contains(&mid) {
                a.value.as_ref().and_then(&mut *f)
            } else {
                None
            }
        })
        .or_else(|| branch(!rev, f))
}
mod local;
mod replay;
pub(super) use local::{Children, Local};
pub(super) use replay::Replay;
#[derive(Default, Clone)]
pub(in crate::tree) struct Window {
    pub key: Option<Key>,
    pub(in crate::tree) banded: bool,
    counted: bool, // in a sealed side: its ordinary order is reachable
    host: Option<Key>,
    floats: bool,
    shell: [Local; 2],
    pub view: scroll::View,
    root: [Root<Local>; 2],
    n: usize,
    ranges: [Selection; 2],
    clip: Rect,
    shift: [f32; 2],
    counts: [usize; 3],
    rows: [usize; 3],
    revision: [u64; 2],
    stamp: Option<([u64; 2], u64, bool)>,
    stationary: Option<([u64; 2], Rect)>,
}
impl Recycle for Window {
    fn recycle(&mut self) {
        *self = Self::default();
    }
}
impl Window {
    pub(in crate::tree) fn hosts(&self, k: Key) -> bool {
        self.host == Some(k)
    }
    pub(in crate::tree) fn stationary(&mut self, revision: [u64; 2], frame: Rect) -> bool {
        let next = Some((revision, frame));
        let same = self.stationary == next;
        self.stationary = next;
        same
    }
    pub(in crate::tree::paint) fn segment(&self, side: usize, slice: Segment) -> Children<'_> {
        Children {
            slice: Some(slice),
            floats: false,
            ..self.children(side)
        }
    }
    #[cfg(test)]
    pub(in crate::tree) fn shares_order(&self, b: &Self, side: usize) -> bool {
        self.root[side]
            .as_ref()
            .zip(b.root[side].as_ref())
            .is_some_and(|(x, y)| x.same(y))
    }
    fn same_version(&self, b: &Self, s: usize) -> bool {
        self.same(b, s)
            && match (&self.root[s], &b.root[s]) {
                (None, None) => true,
                (Some(l), Some(r)) => l.same(r),
                _ => false,
            }
    }
    fn same(&self, b: &Self, side: usize) -> bool {
        self.key == b.key
            && self.banded == b.banded
            && self.revision[side] == b.revision[side]
            && self.clip == b.clip
            && self.shift == b.shift
            && (if side == 1 {
                self.view == b.view
            } else {
                self.view.offset() == b.view.offset()
                    && self.view.hover == b.view.hover
                    && self.view.range == b.view.range
            })
    }
    pub(super) fn draw(
        &self,
        canvas: &skia_safe::Canvas,
        glyphs: &mut impl FnMut(&skia_safe::Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
        replay: Replay<'_>,
    ) -> Result<(), String> {
        let _restore = skia_safe::AutoCanvasRestore::guard(canvas, true);
        canvas.translate((self.shift[0], self.shift[1]));
        let replay = replay.translated(self.shift);
        if self.host == self.key {
            self.children(0).draw(canvas, glyphs, replay)
        } else {
            self.shell[0].draw_with(canvas, glyphs, replay.with_children(self.children(0)))
        }
    }
    pub(super) fn hit(
        &self,
        x: f32,
        y: f32,
        capture: Option<Attachment>,
        outer: [f32; 2],
    ) -> Option<(Hit, f32, f32)> {
        let clip = Rect {
            x: self.clip.x + outer[0],
            y: self.clip.y + outer[1],
            ..self.clip
        };
        let shift = [outer[0] + self.shift[0], outer[1] + self.shift[1]];
        if capture.is_none() && !clip.contains(x, y) {
            return None;
        }
        if self.host == self.key {
            self.children(1).hit(x, y, clip, capture, shift)
        } else {
            self.shell[1].hit_with(x, y, clip, capture, shift, self.children(1))
        }
    }
    fn children(&self, s: usize) -> Children<'_> {
        Children {
            root: self.root[s].as_deref(),
            bounds: sk(Rect::UNBOUNDED),
            n: self.n,
            selection: Some(&self.ranges[s]),
            slice: None,
            floats: self.floats,
        }
    }
}
mod bindings;
pub(super) use bindings::{Bindings, Tables};
#[derive(Default)]
pub(super) struct Cache {
    payloads: Payloads,
    nodes: Stock<Node<Local>>,
    pub maps: Bindings,
    #[cfg(test)]
    pub recorded: [usize; 2],
    #[cfg(test)]
    pub ordinals: usize,
    scratch: Drawing,
    targets: Hits,
    generation: u64,
    intervals: Pages<Range<usize>, 8>,
    runs: Vec<Range<usize>>,
    changes: [Vec<Range<usize>>; 2],
    work: Vec<(Key, [bool; 2], bool)>,
}
impl Cache {
    pub fn reserve(
        &mut self,
        sites: Sites,
        d: [usize; 8],
        h: [usize; 4],
        readers: usize,
        b: Budget,
    ) -> Result<(), String> {
        let Sites {
            owners: n, ports, ..
        } = sites;
        self.maps.reserve(sites, readers, b)?;
        self.intervals.reserve(2 * ports, 2 * n, readers + 2, b)?;
        for v in [&mut self.runs, &mut self.changes[0]] {
            pool::reserve(v, b.capacity(n.div_ceil(2) + 1))?;
        }
        pool::reserve(&mut self.changes[1], b.capacity(n.div_ceil(2) + 1))?;
        let mut scratch = d;
        scratch[4] = 0;
        scratch[5] = 0;
        self.scratch.reserve(scratch)?;
        pool::reserve(self.scratch.materials.shared.edit(), d[4])?;
        self.targets.reserve(h)?;
        let mut d = d;
        let mut h = h;
        d[0] += n;
        h[0] += n;
        self.payloads.reserve(n, d, h, readers + 2, b)?;
        pool::reserve(&mut self.work, b.capacity(n + 1))?;
        self.nodes
            .reserve(b.capacity(2 * (readers + 3) * (n + 1)), || {
                Ok(Node::default())
            })
    }
    // Compile into reserved local scratch using the ordinary emitters. The
    // shell builders (possibly waiting for a fit) keep their exact contents.
    fn swap_builders(&mut self, out: &mut Output, enter: bool) {
        out.packing_local = enter;
        out.fits.recording = None;
        std::mem::swap(&mut out.draw.build, &mut self.scratch);
        std::mem::swap(&mut out.hits.build, &mut self.targets);
    }
    pub fn standalone(&mut self, out: &mut Output, e: &Elements, l: &mut Layers, k: Key) {
        let missing = [0, 1].map(|s| !l.projected[k].saved[s].valid(e, l, k, s));
        if missing != [false; 2] {
            let projected = l.projected[k].projection;
            self.swap_builders(out, true);
            self.record(out, e, l, k, missing);
            self.swap_builders(out, false);
            l.projected[k].projection = projected;
        }
    }
    // An invalid hidden version can be the longest line in the document.
    // Retire it from the mutable order on the existing Compose job: the
    // replacement may spend the same admitted glyphs in a different row.
    pub fn retire(&self, w: &mut Window, i: usize, k: Key, outputs: Outputs) {
        for side in 0..2 {
            if (side == 0 && outputs == Outputs::Hit)
                || (side == 1 && outputs == Outputs::Draw)
                || i >= w.n
                || w.ranges[side].contains(i)
            {
                continue;
            }
            if get(w.root[side].as_deref(), w.n, i).is_some_and(|p| p.key.is_some_and(|a| a.0 == k))
            {
                set(&self.nodes, &mut w.root[side], w.n, i, Local::default());
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        out: &mut Output,
        e: &Elements,
        l: &mut Layers,
        port: Key,
        host: Key,
        (sequence, overlap): (u64, bool),
        w: &mut Window,
    ) -> bool {
        let kids = e.children(host);
        let ctx = l.painted(port);
        let stable = l.paint[port];
        let banded = !l.parts[host].independent_children()
            || (host != port && l.parts[host].facts().coupled);
        let reset = w.banded != banded || w.n != kids.len() || w.host != Some(host);
        if reset {
            *w = Window::default();
        }
        w.banded = banded;
        w.host = Some(host);
        w.floats = l.paragraphs.has_floats(host);
        w.key = Some(port);
        w.n = kids.len();
        w.view = l.scroll.view(port);
        let exposure = l.exposure(e, port);
        w.clip = Rect {
            x: exposure.child_clip.x + (ctx.offset[0] - exposure.offset[0]),
            y: exposure.child_clip.y + (ctx.offset[1] - exposure.offset[1]),
            ..exposure.child_clip
        };
        let [dx, dy] = if banded { [0.; 2] } else { w.view.offset() };
        w.shift = [
            (ctx.offset[0] - stable.offset[0]) - dx,
            (ctx.offset[1] - stable.offset[1]) - dy,
        ];
        let cover = !banded && overlap && selection::allows_coverage(e, l, port, w.shift);
        let stamp = (l.parts[host].ports.map(|p| p.revision), sequence, cover);
        if w.stamp == Some(stamp) {
            self.maps.update(w);
            return false;
        }
        let mut structure = reset || (banded && w.stamp.is_none_or(|s| s.0 != stamp.0));
        let mut next = w.ranges.clone();
        let mut changed = [false; 2];
        for side in 0..2 {
            self.changes[side].clear();
            let clean = w.stamp.is_some_and(|s| s.0[side] == stamp.0[side]);
            if w.stamp.is_none_or(|s| {
                s.1 != sequence || (side == 0 && (s.2 != cover || (cover && !clean)))
            }) {
                self.runs.clear();
                let ranges = l.scroll.ranges_rev(e, host, port);
                if side == 0 && cover {
                    let mut cover = coverage::Coverage::default();
                    for i in ranges.flat_map(Iterator::rev) {
                        #[cfg(test)]
                        {
                            self.ordinals += 1;
                        }
                        if !cover.hides_in(e, l, kids[i], None) {
                            if let Some(r) = self.runs.last_mut()
                                && r.start == i + 1
                            {
                                r.start = i;
                            } else {
                                self.runs.push(i..i + 1);
                            }
                        }
                    }
                } else {
                    self.runs.extend(ranges);
                }
                self.runs.reverse();
                next[side] = w.ranges[side].save(&self.runs, &self.intervals);
            }
            let prev = &w.ranges[side];
            prev.difference(&next[side], |removed| {
                for i in removed {
                    structure |= banded && l.parts[kids[i]].facts().coupled;
                    #[cfg(test)]
                    {
                        self.ordinals += 1;
                    }
                    if let Some(r) = get(w.root[side].as_deref(), w.n, i) {
                        let c = r.counts();
                        changed[side] |= c != [0; 3];
                        for (n, c) in w.rows.iter_mut().zip(c) {
                            *n -= c;
                        }
                    }
                }
            });
            if clean {
                next[side].difference(prev, |r| self.changes[side].push(r));
            } else {
                self.changes[side].extend(next[side].iter().cloned());
            }
        }
        self.swap_builders(out, true);
        let mut origin = [0.; 2];
        if host != port && !banded {
            let mut geometry = Some(Outputs::Both);
            for (s, shell) in w.shell.iter_mut().enumerate() {
                let old = shell.stamp;
                self.shell(out, e, l, host, s, true, shell, &mut geometry);
                origin = shell.origin;
                changed[s] |= old != shell.stamp && shell.own_counts != [0; 3];
            }
            out.partition = None;
            out.inline_base = None;
        }
        let changes = std::mem::take(&mut self.changes);
        let mut todo = changes
            .each_ref()
            .map(|rs| rs.iter().cloned().flatten().peekable());
        while let Some(i) = todo.iter_mut().filter_map(|it| it.peek().copied()).min() {
            #[cfg(test)]
            {
                self.ordinals += 1;
            }
            let k = kids[i];
            let requested = todo.each_mut().map(|it| it.peek() == Some(&i));
            let missing = std::array::from_fn(|side| {
                requested[side]
                    && get(w.root[side].as_deref(), w.n, i).is_none_or(|r| !r.valid(e, l, k, side))
            });
            let coupled = banded && l.parts[k].facts().coupled;
            if !coupled && missing != [false; 2] {
                self.record(out, e, l, k, missing);
            }
            for (side, it) in todo.iter_mut().enumerate() {
                if !requested[side] {
                    continue;
                }
                it.next();
                let mut row = get(w.root[side].as_deref(), w.n, i)
                    .cloned()
                    .unwrap_or_default();
                let before = row.counts();
                let visible = w.ranges[side].contains(i);
                if visible {
                    for (n, c) in w.rows.iter_mut().zip(before) {
                        *n -= c;
                    }
                }
                if coupled {
                    structure |= !visible;
                    if row.key.is_some() {
                        set(&self.nodes, &mut w.root[side], w.n, i, Local::default());
                        changed[side] |= before != [0; 3];
                    }
                    continue;
                }
                if missing[side] {
                    row = l.projected[k].saved[side].clone();
                    row.origin[0] -= origin[0];
                    row.origin[1] -= origin[1];
                    changed[side] |= before != [0; 3] || row.counts() != [0; 3];
                    set(&self.nodes, &mut w.root[side], w.n, i, row.clone());
                }
                changed[side] |= !visible && row.counts() != [0; 3];
                for (n, c) in w.rows.iter_mut().zip(row.counts()) {
                    *n += c;
                }
            }
        }
        self.swap_builders(out, false);
        for (side, changed) in changed.into_iter().enumerate() {
            if changed {
                self.generation += 1;
                w.revision[side] = self.generation;
            }
        }
        self.changes = changes;
        w.counts = std::array::from_fn(|i| {
            let shell = &w.shell[usize::from(i == 2)];
            shell.own_counts[i]
                + if host == port || banded || shell.marker {
                    w.rows[i]
                } else {
                    0
                }
        });
        w.ranges = next;
        w.stamp = Some(stamp);
        self.maps.update(w);
        structure
    }
}
