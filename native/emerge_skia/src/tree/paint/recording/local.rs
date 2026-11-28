//! Separate own ink from the order below it. Compiling a changed parent may
//! change an edge without copying a sibling's commands, positions or hits.
use super::*;
#[derive(Clone, Copy, PartialEq)]
struct SpanInput {
    paragraph: Attachment,
    revision: u64,
    size: [f32; 2],
}
fn span_input(e: &Elements, l: &Layers, k: Key) -> Option<SpanInput> {
    let h = e.layout_parent(k)?;
    if e.slots[h].shape != Shape::Paragraph || e.float_side(k).is_some() {
        return None;
    }
    let f = l.paint[h].frame;
    Some(SpanInput {
        paragraph: Attachment(h, e.slots[h].attachment),
        revision: l.paragraphs.revision(h),
        size: [f.width, f.height],
    })
}
#[derive(Default, Clone)]
pub(in crate::tree::paint) struct Local {
    pub(in crate::tree::paint) key: Option<Attachment>,
    pub(super) stamp: u64,
    pub(super) origin: [f32; 2],
    pub(super) drawing: Option<Ref<Drawing>>,
    pub(super) hits: Option<Ref<Hits>>,
    span: Option<SpanInput>,
    floats: bool,
    pub(super) floating: bool,
    own: Option<(u64, bool)>,
    order: Root<Local>,
    n: usize,
    counts: [usize; 3],
    pub(super) own_counts: [usize; 3],
    extent: opacity::Extent,
    child_bounds: skia_safe::Rect,
    pub(super) marker: bool,
}
impl Recycle for Local {
    fn recycle(&mut self) {
        *self = Self::default();
    }
}
impl Local {
    pub(super) fn same_program(&self, b: &Self) -> bool {
        fn same<T: Recycle>(a: &Option<Ref<T>>, b: &Option<Ref<T>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => a.same(b),
                (None, None) => true,
                _ => false,
            }
        }
        self.key == b.key
            && self.origin == b.origin
            && self.span == b.span
            && self.n == b.n
            && self.floats == b.floats
            && self.marker == b.marker
            && same(&self.order, &b.order)
            && same(&self.drawing, &b.drawing)
            && same(&self.hits, &b.hits)
    }
    pub(in crate::tree::paint) fn hit(
        &self,
        x: f32,
        y: f32,
        clip: Rect,
        capture: Option<Attachment>,
        shift: [f32; 2],
    ) -> Option<(Hit, f32, f32)> {
        self.hit_with(x, y, clip, capture, shift, self.children())
    }
    pub(super) fn draw(
        &self,
        canvas: &skia_safe::Canvas,
        glyphs: &mut impl FnMut(&skia_safe::Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
        replay: Replay<'_>,
    ) -> Result<(), String> {
        self.draw_with(canvas, glyphs, replay.with_children(self.children()))
    }
    pub(in crate::tree::paint) fn valid(&self, e: &Elements, l: &Layers, k: Key, s: usize) -> bool {
        self.key == Some(Attachment(k, e.slots[k].attachment))
            && self.stamp == l.parts[k].ports[s].revision
            && self.span == span_input(e, l, k)
    }
    pub(in crate::tree::paint) fn placed_bounds(&self) -> skia_safe::Rect {
        let Some(draw) = self.drawing.as_ref() else {
            return Default::default();
        };
        let local = self.extent.resolved(draw, self.child_bounds);
        let placed = local.with_offset((self.origin[0], self.origin[1]));
        if !placed.is_finite() || (!local.is_empty() && placed.is_empty()) {
            sk(Rect::UNBOUNDED)
        } else {
            placed
        }
    }
    pub(in crate::tree::paint) fn counts(&self) -> [usize; 3] {
        self.counts
    }
    pub(in crate::tree::paint) fn retire(&mut self, epoch: u64) {
        self.key = None;
        self.order = None;
        if self.own.is_none_or(|(s, split)| s != epoch || !split) {
            self.own = None;
            self.drawing = None;
            self.hits = None;
        }
    }
    fn children(&self) -> Children<'_> {
        Children {
            root: self.order.as_deref(),
            bounds: self.child_bounds,
            n: self.n,
            selection: None,
            slice: None,
            floats: self.floats,
        }
    }
    pub(super) fn draw_with(
        &self,
        canvas: &skia_safe::Canvas,
        glyphs: &mut impl FnMut(&skia_safe::Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
        replay: Replay<'_>,
    ) -> Result<(), String> {
        if let Some(d) = &self.drawing {
            let _save = skia_safe::AutoCanvasRestore::guard(canvas, true);
            canvas.translate((self.origin[0], self.origin[1]));
            replay_inner(canvas, d, replay.translated(self.origin), glyphs)?;
        }
        Ok(())
    }
    pub(super) fn hit_with(
        &self,
        x: f32,
        y: f32,
        mut view: Rect,
        capture: Option<Attachment>,
        shift: [f32; 2],
        children: Children<'_>,
    ) -> Option<(Hit, f32, f32)> {
        let hits = self.hits.as_ref()?;
        let (dx, dy) = (self.origin[0] + shift[0], self.origin[1] + shift[1]);
        view.x -= dx;
        view.y -= dy;
        let screen = (x, y);
        let (x, y) = (x - dx, y - dy);
        hits.items.iter().rev().find_map(|h| {
            let (x, y) = if let Some(key) = capture {
                if h.kind != HitKind::Children && (h.key != key || !h.actions.any()) {
                    return None;
                }
                let plane = hits.planes.get(h.scope.plane);
                let rect = plane.map_or(sk(h.rect), |p| p.bounds(sk(h.rect)));
                if !rect.intersects(sk(view)) {
                    return None;
                }
                let p = plane.map_or(Some((x, y).into()), |p| {
                    Some(p.matrix.invert()?.map_point((x, y)))
                })?;
                (p.x, p.y)
            } else {
                let (x, y, stop) = hits.planes.point(
                    h.scope.plane,
                    &hits.clips,
                    h.scope.before,
                    x,
                    y,
                    &Tables::EMPTY,
                )?;
                if !(h.rect.contains(x, y) && hits.clips.contains(h.scope.mask, stop, x, y)) {
                    return None;
                }
                (x, y)
            };
            if h.kind == HitKind::Children {
                let r = hits.planes.get(h.scope.plane).map_or(Some(view), |p| {
                    let r = p.matrix.invert()?.map_rect(sk(view)).0;
                    Some(Rect {
                        x: r.left,
                        y: r.top,
                        width: r.width(),
                        height: r.height(),
                    })
                })?;
                let r = r.intersection(h.rect)?;
                if h.scope.plane == 0 {
                    children.hit(
                        screen.0,
                        screen.1,
                        Rect {
                            x: r.x + dx,
                            y: r.y + dy,
                            ..r
                        },
                        capture,
                        [dx, dy],
                    )
                } else {
                    children.hit(x, y, r, capture, [0.; 2])
                }
            } else if h.scope.plane == 0 {
                // Keep the original sample through translation-only edges. The
                // public local coordinate subtracts the f32 placed origin in f64.
                let mut h = *h;
                h.origin[0] += dx;
                h.origin[1] += dy;
                Some((h, screen.0, screen.1))
            } else {
                Some((*h, x, y))
            }
        })
    }
}
#[derive(Clone, Copy)]
pub(in crate::tree::paint) struct Children<'a> {
    pub(in crate::tree::paint) bounds: skia_safe::Rect,
    pub(super) root: Option<&'a Node<Local>>,
    pub(super) n: usize,
    pub(super) selection: Option<&'a Selection>,
    pub(super) floats: bool,
    pub(super) slice: Option<Segment>,
}
impl Children<'_> {
    fn find<R>(self, rev: bool, mut f: impl FnMut(&Local) -> Option<R>) -> Option<R> {
        let mut pass = |floats: bool| {
            let mut walk = |r: &Range<usize>| {
                let r = self.slice.map_or(r.clone(), |s| {
                    r.start.max(s.first as usize)..r.end.min(s.end as usize)
                });
                visit(self.root, 0, self.n, &r, rev, &mut |row| {
                    if self
                        .slice
                        .is_none_or(|s| s.floating.is_none_or(|v| v == row.floating))
                        && (!self.floats || row.floating == floats)
                    {
                        f(row)
                    } else {
                        None
                    }
                })
            };
            match self.selection {
                None => walk(&(0..self.n)),
                Some(s) if rev => s.within(self.slice).rev().find_map(walk),
                Some(s) => s.within(self.slice).find_map(walk),
            }
        };
        pass(!rev).or_else(|| if self.floats { pass(rev) } else { None })
    }
    pub(super) fn draw(
        self,
        canvas: &skia_safe::Canvas,
        glyphs: &mut impl FnMut(&skia_safe::Canvas, GlyphPaintRun, &Paint) -> Result<(), String>,
        replay: Replay<'_>,
    ) -> Result<(), String> {
        self.find(false, |r| {
            r.draw_with(canvas, glyphs, replay.with_children(r.children()))
                .err()
        })
        .map_or(Ok(()), Err)
    }
    pub(in crate::tree::paint) fn hit(
        self,
        x: f32,
        y: f32,
        view: Rect,
        capture: Option<Attachment>,
        shift: [f32; 2],
    ) -> Option<(Hit, f32, f32)> {
        self.find(true, |r| {
            r.hit_with(x, y, view, capture, shift, r.children())
        })
    }
}
fn order(
    kids: &[Key],
    l: &Layers,
    side: usize,
    origin: [f32; 2],
    stock: &Stock<Node<Local>>,
) -> (Root<Local>, [usize; 3], skia_safe::Rect) {
    if kids.is_empty() {
        return (None, [0; 3], Default::default());
    }
    let mid = kids.len() / 2;
    let k = kids[mid];
    let mut row = l.projected[k].saved[side].clone();
    row.origin[0] -= origin[0];
    row.origin[1] -= origin[1];
    let (a, left, mut bounds) = order(&kids[..mid], l, side, origin, stock);
    let (b, right, other) = order(&kids[mid + 1..], l, side, origin, stock);
    bounds.join(row.placed_bounds());
    bounds.join(other);
    let counts = std::array::from_fn(|i| left[i] + row.counts[i] + right[i]);
    let root = (counts != [0; 3]).then(|| {
        stock.issue(|v| {
            v.children = [a, b];
            v.value = Some(row);
        })
    });
    (root, counts, bounds)
}
impl Cache {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn shell(
        &mut self,
        out: &mut Output,
        e: &Elements,
        l: &mut Layers,
        k: Key,
        s: usize,
        split: bool,
        row: &mut Local,
        geometry: &mut Option<Outputs>,
    ) {
        let p = l.parts[k];
        let key = Some(Attachment(k, e.slots[k].attachment));
        let f = l.paint[k].frame;
        let own = Some((
            if split {
                p.own_epoch[s]
            } else {
                p.ports[s].revision
            },
            split,
        ));
        let span = span_input(e, l, k);
        let origin = span.map_or(f, |s| l.paint[s.paragraph.0].frame);
        if row.own != own || row.span != span || (row.marker != p.ports[s].nonempty && split) {
            if let Some(outputs) = geometry.take() {
                out.inline_base = span.map(|input| {
                    let h = input.paragraph.0;
                    (
                        inline::Span::at(l, h, k),
                        l.inline_record_geometry(e, k, h, outputs),
                    )
                });
                if span.is_none() {
                    l.record_geometry(e, k, [f.x, f.y], None, !split, outputs);
                }
            }
            out.fits.recording = Some(k);
            out.partition = split.then_some(k);
            out.marker = false;
            if s == 0 {
                out.draw.build.clear();
                out.index.begin(true);
                out.pack(e, Some(k), l, s);
                // Complete local bounds, not the layer surface for this scroll sample.
                row.extent = out.draw.build.extent(0..out.draw.build.commands.len()).1;
                row.drawing = self.payloads.drawing(&out.draw.build);
            } else {
                out.hits.build.clear();
                out.pack(e, Some(k), l, s);
                row.hits = self.payloads.hits(&out.hits.build);
            }
            row.marker = out.marker;
            row.own = own;
            row.own_counts = [
                row.drawing
                    .as_ref()
                    .map_or(0, |d| d.commands.len() - usize::from(row.marker)),
                row.drawing.as_ref().map_or(0, |d| d.ids.len()),
                row.hits
                    .as_ref()
                    .map_or(0, |h| h.items.len() - usize::from(row.marker)),
            ];
            #[cfg(test)]
            {
                self.recorded[s] += 1;
            }
        }
        row.key = key;
        row.stamp = p.ports[s].revision;
        row.span = span;
        row.origin = [origin.x, origin.y];
        row.floats = l.paragraphs.has_floats(k);
        row.floating = e.float_side(k).is_some();
    }
    pub(super) fn record(
        &mut self,
        out: &mut Output,
        e: &Elements,
        l: &mut Layers,
        root: Key,
        missing: [bool; 2],
    ) {
        self.work.push((root, missing, false));
        while let Some((k, requested, leave)) = self.work.pop() {
            let p = l.parts[k];
            let kids = e.children(k);
            // Only the immediate host can lend the bitmap its media mask.
            let grouped = span_input(e, l, k).is_some()
                || e.order(k).tags.contains(&Slot::Behind)
                || (p.media_subtree && kids.iter().any(|&c| e.slots[c].shape == Shape::Image));
            let split = !kids.is_empty() && !grouped;
            let needed =
                std::array::from_fn(|s| requested[s] && !l.projected[k].saved[s].valid(e, l, k, s));
            if needed == [false; 2] {
                continue;
            }
            if !leave && split {
                self.work.push((k, needed, true));
                self.work
                    .extend(kids.iter().rev().map(|&c| (c, needed, false)));
                continue;
            }
            let f = l.paint[k].frame;
            let origin = [f.x, f.y];
            let mut geometry = Outputs::changed(needed[0], needed[1]);
            for (s, need) in needed.into_iter().enumerate() {
                if !need {
                    continue;
                }
                let mut row = std::mem::take(&mut l.projected[k].saved[s]);
                self.shell(out, e, l, k, s, split, &mut row, &mut geometry);
                row.n = if split { kids.len() } else { 0 };
                let (root, counts, bounds) = if split {
                    order(kids, l, s, origin, &self.nodes)
                } else {
                    (None, [0; 3], Default::default())
                };
                row.child_bounds = bounds;
                row.order = root;
                row.counts = row.own_counts;
                if row.marker {
                    for (n, c) in row.counts.iter_mut().zip(counts) {
                        *n += c;
                    }
                }
                l.projected[k].saved[s] = row;
            }
        }
        out.partition = None;
        out.inline_base = None;
    }
}
