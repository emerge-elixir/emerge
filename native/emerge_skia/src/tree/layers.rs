use super::{
    paint::{self, Composition, Outputs, PaintContext},
    text::Run,
    *,
};
use super::{queue::Queue, text::TextStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    Context,
    Measure,
    Flow,
    Reflow,
    Place,
    PaintContext,
    Compose,
}
use Pass::*;
const PASSES: [Pass; 7] = [Context, Measure, Flow, Reflow, Place, PaintContext, Compose];
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Size {
    pub w: f32,
    pub h: f32,
}
#[derive(Clone, Copy, PartialEq)]
struct ContextValue {
    family: Family,
    size: f32,
    scale: f32,
}
#[derive(Clone, Copy, PartialEq)]
struct Width {
    outer: f32,
    inner: f32,
}

#[derive(Default)]
pub(super) struct Layers {
    jobs: [Queue; 7],
    context: SecondaryMap<Key, ContextValue>,
    measure: SecondaryMap<Key, Size>,
    // Glyph changes and intrinsic size changes have different readers.
    pub(super) runs: SecondaryMap<Key, Run>,
    pub(super) text: TextStore,
    pub(super) bounds: capacity::PayloadBounds,
    pub(super) owners: usize,
    pub(super) text_count: usize,
    pub(super) prepared_scale: f32,
    width: SecondaryMap<Key, Width>,
    fit: SecondaryMap<Key, Size>,
    offsets: SecondaryMap<Key, [f32; 2]>,
    pub frames: SecondaryMap<Key, Rect>,
    paint: SecondaryMap<Key, PaintContext>,
    pub(super) parts: SecondaryMap<Key, Composition>,
    pub(super) viewport: Rect,
    pub(super) scale: f32,
    #[cfg(test)]
    pub trace: Vec<(Pass, u64, Outputs)>,
}
fn changed<T: PartialEq>(map: &mut SecondaryMap<Key, T>, k: Key, value: T) -> bool {
    if map.get(k) == Some(&value) {
        false
    } else {
        map.insert(k, value);
        true
    }
}
impl Layers {
    pub fn reserve(&mut self, keys: usize, capacity: usize) -> Result<(), String> {
        for q in &mut self.jobs {
            q.reserve(keys, capacity)?
        }
        macro_rules! room {($($map:ident),+) => {$(self.$map.try_set_capacity(keys).map_err(pool::capacity_error)?;)+}}
        room!(
            context, measure, runs, width, fit, offsets, frames, paint, parts
        );
        #[cfg(test)]
        pool::reserve(&mut self.trace, 7 * capacity)?;
        self.bounds.reserve(capacity)?;
        self.owners = capacity;
        Ok(())
    }
    pub fn clear(&mut self) {
        for q in &mut self.jobs {
            q.clear()
        }
        self.context.clear();
        self.measure.clear();
        self.runs.clear();
        self.width.clear();
        self.fit.clear();
        self.offsets.clear();
        self.frames.clear();
        self.paint.clear();
        self.parts.clear();
        self.text.glyphs.clear();
        self.viewport = Rect::default();
        self.scale = 0.;
        #[cfg(test)]
        self.trace.clear();
    }
    // Invalidate the value by queuing its owner; keep the old result until comparison.
    pub fn invalidate(&mut self, p: Pass, k: Key, depth: usize) {
        let rank = if matches!(p, Measure | Reflow | Compose) {
            depth
        } else {
            usize::MAX - depth
        };
        self.jobs[p as usize].push(rank, k, Outputs::Both);
    }
    pub fn invalidate_composition(&mut self, k: Key, depth: usize, outputs: Outputs) {
        self.jobs[Compose as usize].push(depth, k, outputs);
    }
    pub fn mount(&mut self, k: Key, depth: usize) {
        for p in PASSES {
            self.invalidate(p, k, depth);
        }
    }
    fn enqueue(&mut self, p: Pass, k: Key, e: &Elements) {
        self.invalidate(p, k, e.slots[k].depth);
    }
    fn parent(&mut self, p: Pass, k: Key, e: &Elements) {
        if let Some(parent) = e.slots[k].parent {
            self.enqueue(p, parent, e);
        }
    }
    pub fn recompute(&mut self, e: &Elements, root: Option<Key>, w: f32, h: f32, scale: f32) {
        #[cfg(test)]
        self.trace.clear();
        let vp = Rect {
            x: 0.,
            y: 0.,
            width: w,
            height: h,
        };
        let Some(root) = root else {
            self.viewport = vp;
            return;
        };
        if self.scale != scale {
            self.scale = scale;
            self.enqueue(Context, root, e);
        }
        if vp != self.viewport {
            self.viewport = vp;
            self.enqueue(Flow, root, e);
            self.enqueue(PaintContext, root, e);
        }
        for pass in PASSES {
            while let Some((k, requested)) = self.jobs[pass as usize].pop() {
                if !e.slots.contains_key(k) {
                    continue;
                }
                #[cfg(test)]
                self.trace.push((pass, e.slots[k].id, requested));
                match pass {
                    Context => self.context(e, k),
                    Measure => self.measure(e, k),
                    Flow => self.flow(e, k),
                    Reflow => self.reflow(e, k),
                    Place => self.place(e, k),
                    PaintContext => self.paint_context(e, k),
                    Compose => {
                        if let Some(outputs) = paint::compose(
                            e,
                            k,
                            &self.paint[k],
                            self.runs.get(k),
                            &mut self.parts,
                            requested,
                        ) && let Some(p) = e.slots[k].parent
                        {
                            self.invalidate_composition(p, e.slots[p].depth, outputs)
                        }
                    }
                }
            }
        }
    }
    fn context(&mut self, e: &Elements, k: Key) {
        let a = &e.attrs[k];
        let inherited = e.slots[k].parent.map(|p| self.context[p]);
        let ctx = ContextValue {
            family: a.family.or(inherited.map(|p| p.family)).unwrap_or_default(),
            size: a
                .font_size
                .map(|v| v * self.scale)
                .or(inherited.map(|p| p.size))
                .unwrap_or(16.),
            scale: self.scale,
        };
        let scale_changed = self.context.get(k).is_none_or(|p| p.scale != ctx.scale);
        if changed(&mut self.context, k, ctx) {
            if e.slots[k].kind == Kind::Text || scale_changed {
                self.enqueue(Measure, k, e);
            }
            if scale_changed {
                for p in [Flow, Reflow, PaintContext] {
                    self.enqueue(p, k, e);
                }
            }
            for &c in e.children(k) {
                self.enqueue(Context, c, e);
            }
        }
    }
    fn measure(&mut self, e: &Elements, k: Key) {
        let c = self.context[k];
        let b = e.attrs[k].sizing;
        let children = e.children(k);
        let sizes = children.iter().map(|&k| self.measure[k]);
        let [gx, gy] = b.spacing.map(|v| v * c.scale);
        let gaps = children.len().saturating_sub(1) as f32;
        let content = match e.slots[k].kind {
            Kind::Text => {
                let run = self.runs.entry(k).unwrap().or_default();
                if (run.invalid
                    || run.revision == 0
                    || run.metrics.family != c.family
                    || run.metrics.size != c.size)
                    && self
                        .text
                        .measure(e.text(k), e.attrs[k].glyph_count, c.family, c.size, run)
                {
                    self.invalidate_composition(k, e.slots[k].depth, Outputs::Draw)
                }
                let len = e.attrs[k].glyph_count;
                self.bounds.set(e.slots[k].index, len, len, false);
                let run = &self.runs[k].metrics;
                Size {
                    w: run.width,
                    h: run.height,
                }
            }
            Kind::Column => sizes.fold(
                Size {
                    w: 0.,
                    h: gaps * gy,
                },
                |s, v| Size {
                    w: s.w.max(v.w),
                    h: s.h + v.h,
                },
            ),
            Kind::Wrap => sizes.fold(
                Size {
                    w: gaps * gx,
                    h: 0.,
                },
                |s, v| Size {
                    w: s.w + v.w,
                    h: s.h.max(v.h),
                },
            ),
            Kind::El | Kind::Empty => sizes.fold(Size::default(), |s, v| Size {
                w: s.w.max(v.w),
                h: s.h.max(v.h),
            }),
        };
        let [t, r, d, l] = b.padding.map(|v| v * c.scale);
        let m = Size {
            w: b.width.intrinsic(content.w + (l + r), c.scale),
            h: b.height.intrinsic(content.h + (t + d), c.scale),
        };
        if changed(&mut self.measure, k, m) {
            self.parent(Measure, k, e);
            self.enqueue(Flow, k, e);
            self.enqueue(Reflow, k, e);
        }
    }
    pub fn text_changed(&mut self, k: Key, depth: usize) {
        if let Some(r) = self.runs.get_mut(k) {
            r.invalid = true
        }
        self.invalidate(Measure, k, depth);
    }
    fn uses_available(e: &Elements, k: Key) -> bool {
        match e.attrs[k].sizing.width {
            Length::Fill => true,
            Length::Auto if e.slots[k].kind == Kind::Column => e
                .children(k)
                .iter()
                .any(|&c| e.attrs[c].sizing.width == Length::Fill),
            _ => false,
        }
    }
    fn flow(&mut self, e: &Elements, k: Key) {
        let b = e.attrs[k].sizing;
        let available = e.slots[k]
            .parent
            .map_or(self.viewport.width, |p| self.width[p].inner);
        let outer = if Self::uses_available(e, k) {
            available
        } else {
            self.measure[k].w
        };
        let [_, r, _, l] = b.padding.map(|v| v * self.scale);
        let inner = (outer - (r + l)).max(0.);
        let available_changed = self.width.get(k).is_none_or(|w| w.inner != inner);
        if changed(&mut self.width, k, Width { outer, inner }) {
            self.enqueue(Reflow, k, e);
        }
        if available_changed {
            for &c in e.children(k) {
                if matches!(e.attrs[c].sizing.width, Length::Fill)
                    || (e.slots[c].kind == Kind::Column && e.attrs[c].sizing.width == Length::Auto)
                {
                    self.enqueue(Flow, c, e);
                }
            }
        }
    }
    fn reflow(&mut self, e: &Elements, k: Key) {
        let n = &e.slots[k];
        let b = e.attrs[k].sizing;
        let [t, _, d, l] = b.padding.map(|v| v * self.scale);
        let [gx, gy] = b.spacing.map(|v| v * self.scale);
        let width = self.width[k];
        let (mut x, mut y, mut line_h, mut demand) = (0f32, 0f32, 0f32, 0f32);
        // Local line fitting uses only settled child extents. Store each offset
        // independently: a reorder can move children without changing our fit.
        for (i, &child) in e.children(k).iter().enumerate() {
            let s = self.fit[child];
            match n.kind {
                Kind::Wrap if i != 0 => {
                    if x + gx + s.w > width.inner {
                        y += line_h + gy;
                        x = 0.;
                        line_h = 0.;
                    } else {
                        x += gx;
                    }
                }
                Kind::Column if i != 0 => y += gy,
                _ => {}
            }
            if changed(&mut self.offsets, child, [l + x, t + y]) {
                self.enqueue(Place, child, e);
            }
            demand = demand.max(y + s.h);
            match n.kind {
                Kind::Wrap => {
                    x += s.w;
                    line_h = line_h.max(s.h);
                }
                Kind::Column => y += s.h,
                _ => {}
            }
        }
        let h = if n.kind == Kind::Text {
            self.measure[k].h
        } else {
            b.height.intrinsic(demand + t + d, self.scale)
        };
        if changed(&mut self.fit, k, Size { w: width.outer, h }) {
            self.parent(Reflow, k, e);
            self.enqueue(Place, k, e);
        }
    }
    fn place(&mut self, e: &Elements, k: Key) {
        let [x, y] = e.slots[k].parent.map_or([0.; 2], |p| {
            let base = self.frames[p];
            let [x, y] = self.offsets[k];
            [base.x + x, base.y + y]
        });
        let s = self.fit[k];
        let origin_changed = self.frames.get(k).is_none_or(|f| [f.x, f.y] != [x, y]);
        if changed(
            &mut self.frames,
            k,
            Rect {
                x,
                y,
                width: s.w,
                height: s.h,
            },
        ) {
            self.enqueue(PaintContext, k, e);
        }
        if origin_changed {
            for &child in e.children(k) {
                self.enqueue(Place, child, e);
            }
        }
    }
    fn paint_context(&mut self, e: &Elements, k: Key) {
        let parent = e.slots[k].parent;
        let a = &e.attrs[k];
        let f = self.frames[k];
        let color = a
            .color
            .or_else(|| parent.map(|p| self.paint[p].color))
            .unwrap_or(0xff);
        let [top, _, _, left] = a.sizing.padding;
        let next = PaintContext {
            frame: f,
            color,
            scale: self.scale,
            viewport: self.viewport,
            text_origin: [f.x + left * self.scale, f.y + top * self.scale],
        };
        let old = self.paint.get(k).copied();
        if old == Some(next) {
            return;
        }
        self.paint.insert(k, next);
        let viewport_changed = old.is_none_or(|p| p.viewport != next.viewport);
        let geometry_changed = viewport_changed || old.is_none_or(|p| p.frame != f);
        let color_changed = old.is_none_or(|p| p.color != color);
        let draw = (a.background.is_some() && geometry_changed)
            || (e.slots[k].kind == Kind::Text
                && (viewport_changed
                    || color_changed
                    || old.is_none_or(|p| p.text_origin != next.text_origin)));
        let hit = a.click && (geometry_changed || old.is_none_or(|p| p.scale != next.scale));
        if let Some(outputs) = Outputs::changed(draw, hit) {
            self.invalidate_composition(k, e.slots[k].depth, outputs);
        }
        if viewport_changed || color_changed {
            for &child in e.children(k) {
                if viewport_changed || e.attrs[child].color.is_none() {
                    self.enqueue(PaintContext, child, e);
                }
            }
        }
    }
}
