use super::{
    paint::{self, Composition, Outputs, PaintContext},
    text::Run,
    *,
};
use super::{
    queue::{Direction, WorkSet},
    text::TextStore,
};

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
impl Pass {
    fn direction(self) -> Direction {
        match self {
            Measure | Reflow | Compose => Direction::DeepFirst,
            Context | Flow | Place | PaintContext => Direction::ShallowFirst,
        }
    }
}
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
    jobs: [WorkSet; 7],
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
    offers: SecondaryMap<Key, f32>,
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
// Pass views expose only settled inputs and keyed writers. A shape cannot run
// a child early or install geometry, paint or hits on its own.
pub(super) struct Measuring<'a> {
    layers: &'a mut Layers,
    e: &'a Elements,
    k: Key,
    pub gap: [f32; 2],
}
impl Measuring<'_> {
    pub fn child(&self, c: Key) -> Size {
        self.layers.measure[c]
    }
    pub fn glyph_run(&mut self, text: &str) -> Size {
        let k = self.k;
        let l = &mut self.layers;
        let ctx = l.context[k];
        let len = self.e.attrs[k].glyph_count;
        let run = l.runs.entry(k).unwrap().or_default();
        let changed = (run.invalid
            || run.revision == 0
            || run.metrics.family != ctx.family
            || run.metrics.size != ctx.size)
            && l.text.measure(text, len, ctx.family, ctx.size, run);
        let size = Size {
            w: run.metrics.width,
            h: run.metrics.height,
        };
        if changed {
            l.invalidate_composition(k, self.e.slots[k].depth, Outputs::Draw)
        }
        l.bounds.set(self.e.slots[k].index, len, len, false);
        size
    }
}
pub(super) struct FlowCx<'a> {
    layers: &'a mut Layers,
    e: &'a Elements,
    pub policy: Length,
    pub inner: f32,
    pub inner_changed: bool,
    pub gap: f32,
}
impl FlowCx<'_> {
    pub fn measured_width(&self, c: Key) -> f32 {
        self.layers.measure[c].w
    }
    pub fn policy_of(&self, c: Key) -> Length {
        self.e.attrs[c].sizing.width
    }
    pub fn offer(&mut self, c: Key, width: f32) {
        if changed(&mut self.layers.offers, c, width) && self.e.slots[c].shape.uses_offer(self.e, c)
        {
            self.layers.enqueue(Flow, c, self.e)
        }
    }
}
pub(super) struct ReflowCx<'a> {
    layers: &'a mut Layers,
    e: &'a Elements,
    pub inner: f32,
    pub gap: [f32; 2],
    padding: [f32; 4],
    height: Length,
    pub measured_height: f32,
}
impl ReflowCx<'_> {
    pub fn child(&self, c: Key) -> Size {
        self.layers.fit[c]
    }
    pub fn offset(&mut self, c: Key, x: f32, y: f32) {
        if changed(
            &mut self.layers.offsets,
            c,
            [self.padding[3] + x, self.padding[0] + y],
        ) {
            self.layers.enqueue(Place, c, self.e)
        }
    }
    pub fn box_height(&self, demand: f32) -> f32 {
        self.height.intrinsic(
            demand + self.padding[0] + self.padding[2],
            self.layers.scale,
        )
    }
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
            context, measure, runs, width, offers, fit, offsets, frames, paint, parts
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
        self.offers.clear();
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
        self.jobs[p as usize].push(depth, k, Outputs::Both);
    }
    pub fn invalidate_composition(&mut self, k: Key, depth: usize, outputs: Outputs) {
        self.jobs[Compose as usize].push(depth, k, outputs);
    }
    pub fn mount(&mut self, k: Key, depth: usize) {
        for p in PASSES {
            self.invalidate(p, k, depth);
        }
    }
    /// Compare before overwriting the single child order. Equal per-ordinal
    /// measures don't need measuring again; offsets and stacking still read identity.
    pub fn invalidate_order(
        &mut self,
        e: &Elements,
        k: Key,
        next: impl Iterator<Item = Key>,
    ) -> bool {
        let node = &e.slots[k];
        let reads_offers = node
            .shape
            .reads_flow(e.attrs[k].sizing.width, FlowInput::Order);
        let (mut moved, mut measure, mut flow) = (false, false, false);
        for (&a, b) in e.children(k).iter().zip(next).filter(|(a, b)| **a != *b) {
            moved = true;
            let (old, new) = (self.measure.get(a), self.measure.get(b));
            measure |= old != new;
            flow |= reads_offers
                && (old.map(|m| m.w) != new.map(|m| m.w)
                    || e.attrs[a].sizing.width != e.attrs[b].sizing.width);
        }
        if measure {
            self.enqueue(Measure, k, e)
        }
        if flow {
            self.enqueue(Flow, k, e)
        }
        if moved {
            self.enqueue(Reflow, k, e);
            self.enqueue(Compose, k, e)
        }
        moved
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
            while let Some((k, requested)) = self.jobs[pass as usize].pop(pass.direction()) {
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
            if e.slots[k].shape.uses_glyphs() || scale_changed {
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
        let content = e.slots[k].shape.measure(
            e,
            k,
            &mut Measuring {
                layers: self,
                e,
                k,
                gap: b.spacing.map(|v| v * c.scale),
            },
        );
        let [t, r, d, l] = b.padding.map(|v| v * c.scale);
        let m = Size {
            w: b.width.intrinsic(content.w + (l + r), c.scale),
            h: b.height.intrinsic(content.h + (t + d), c.scale),
        };
        let width_changed = self.measure.get(k).is_none_or(|s| s.w != m.w);
        if changed(&mut self.measure, k, m) {
            self.parent(Measure, k, e);
            if width_changed {
                self.enqueue(Flow, k, e);
                if let Some(p) = e.slots[k].parent
                    && e.slots[p]
                        .shape
                        .reads_flow(e.attrs[p].sizing.width, FlowInput::ChildWidth(b.width))
                {
                    self.enqueue(Flow, p, e);
                }
            }
            self.enqueue(Reflow, k, e);
        }
    }
    pub fn text_changed(&mut self, k: Key, depth: usize) {
        if let Some(r) = self.runs.get_mut(k) {
            r.invalid = true
        }
        self.invalidate(Measure, k, depth);
    }
    fn flow(&mut self, e: &Elements, k: Key) {
        let b = e.attrs[k].sizing;
        let shape = e.slots[k].shape;
        let outer = if shape.uses_offer(e, k) {
            if e.slots[k].parent.is_some() {
                self.offers[k]
            } else {
                self.viewport.width
            }
        } else {
            self.measure[k].w
        };
        let [_, r, _, l] = b.padding.map(|v| v * self.scale);
        let inner = (outer - (r + l)).max(0.);
        let available_changed = self.width.get(k).is_none_or(|w| w.inner != inner);
        if changed(&mut self.width, k, Width { outer, inner }) {
            self.enqueue(Reflow, k, e);
        }
        shape.offer_widths(
            e,
            k,
            &mut FlowCx {
                inner,
                inner_changed: available_changed,
                policy: b.width,
                gap: b.spacing[0] * self.scale,
                layers: self,
                e,
            },
        );
    }
    fn reflow(&mut self, e: &Elements, k: Key) {
        let n = &e.slots[k];
        let b = e.attrs[k].sizing;
        let width = self.width[k];
        let h = n.shape.reflow(
            e,
            k,
            &mut ReflowCx {
                inner: width.inner,
                gap: b.spacing.map(|v| v * self.scale),
                padding: b.padding.map(|v| v * self.scale),
                height: b.height,
                measured_height: self.measure[k].h,
                layers: self,
                e,
            },
        );
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
        let shape = e.slots[k].shape;
        let clip = parent.map_or(self.viewport, |p| self.paint[p].child_clip);
        let next = PaintContext {
            frame: f,
            clip,
            child_clip: shape.child_clip(clip, f),
            color,
            scale: self.scale,
            text_origin: [f.x + left * self.scale, f.y + top * self.scale],
        };
        let old = self.paint.get(k).copied();
        if old == Some(next) {
            return;
        }
        self.paint.insert(k, next);
        let child_clip_changed = old.is_none_or(|p| p.child_clip != next.child_clip);
        let geometry_changed = old.is_none_or(|p| p.frame != f || p.clip != clip);
        let color_changed = old.is_none_or(|p| p.color != color);
        let draw = shape.reads_paint_context(a, old, next);
        let hit = a.click && (geometry_changed || old.is_none_or(|p| p.scale != next.scale));
        if let Some(outputs) = Outputs::changed(draw, hit) {
            self.invalidate_composition(k, e.slots[k].depth, outputs);
        }
        if child_clip_changed || color_changed {
            for &child in e.children(k) {
                if child_clip_changed || e.attrs[child].color.is_none() {
                    self.enqueue(PaintContext, child, e);
                }
            }
        }
    }
}
