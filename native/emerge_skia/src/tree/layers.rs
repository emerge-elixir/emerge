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
use scale::{Inputs, pixels};
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
/// Intrinsic outer sizes in device pixels. A wrapping descendant may fit
/// at a smaller width than its preferred (single-line) measurement.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(super) struct Demand {
    pub w: f32,
    pub h: f32,
    pub min_w: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct WidthOffer {
    axis: Constraint,
    break_before: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct Width {
    outer: f32,
    inner: f32,
    new_edge: bool,
}

/// Input on an axis, in device pixels. An available extent is not a cap;
/// an assigned extent already includes the child's main-axis policy.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Constraint {
    Natural,
    Available(f32),
    Assigned(f32),
}
impl Constraint {
    fn for_height(self, policy: Length) -> Self {
        match self {
            Self::Available(_) if policy.weight().is_none() => Self::Natural,
            other => other,
        }
    }
    fn resolve(self, e: &Elements, k: Key, policy: Length, fit: f32) -> f32 {
        match self {
            Self::Natural => fit,
            Self::Available(n) => policy.at_offer(e, k, fit, n),
            Self::Assigned(n) => n,
        }
    }
}

/// The offset is relative to the incoming coordinate system. A zero
/// constraint is an actual size, distinct from using natural fit.
#[derive(Clone, Copy, PartialEq)]
struct Placement {
    offset: [f32; 2],
    height: Constraint,
}

#[derive(Default)]
pub(super) struct Layers {
    jobs: [WorkSet; 7],
    context: SecondaryMap<Key, Inputs>,
    measure: SecondaryMap<Key, Demand>,
    pub(super) intrinsic: intrinsic::Intrinsic,
    // Glyph changes and intrinsic size changes have different readers.
    pub(super) runs: SecondaryMap<Key, Run>,
    pub(super) text: TextStore,
    pub(super) bounds: capacity::PayloadBounds,
    pub(super) owners: usize,
    pub(super) text_count: usize,
    pub(super) prepared_scale: f32,
    width: SecondaryMap<Key, Width>,
    offers: SecondaryMap<Key, WidthOffer>,
    fit: SecondaryMap<Key, Size>,
    placement: SecondaryMap<Key, Placement>,
    pub frames: SecondaryMap<Key, Rect>,
    paint: SecondaryMap<Key, PaintContext>,
    pub(super) parts: SecondaryMap<Key, Composition>,
    pub(super) viewport: Rect,
    pub(super) scale_declarations: usize,
    #[cfg(test)]
    pub trace: Vec<(Pass, u64, Outputs)>,
    #[cfg(test)]
    pub invalidation_passes: usize,
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
    pub fn child(&self, c: Key) -> Demand {
        self.layers.measure[c]
    }
    pub fn glyph_run(&mut self, text: &str) -> Demand {
        let k = self.k;
        let l = &mut self.layers;
        let ctx = l.context[k];
        let len = self.e.attrs[k].glyph_count;
        let run = l.runs.entry(k).unwrap().or_default();
        let font_size = ctx.font_size();
        let changed = (run.invalid
            || run.revision == 0
            || run.metrics.family != ctx.family
            || run.metrics.size != font_size)
            && l.text.measure(text, len, ctx.family, font_size, run);
        let size = Demand {
            min_w: run.metrics.width,
            w: run.metrics.width,
            h: run.metrics.height,
        };
        if changed {
            l.invalidate_composition(k, self.e.depth(k), Outputs::Draw)
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
    pub offer_children: bool,
    pub gap: f32,
}
impl FlowCx<'_> {
    pub fn measured_width(&self, c: Key) -> f32 {
        self.layers.measure[c].w
    }
    pub fn line_measure(&self, c: Key) -> f32 {
        let measured = self.layers.measure[c];
        if self.e.attrs[c].sizing.width.weight().is_some() {
            measured.min_w
        } else if self.e.slots[c].shape.uses_offer(self.e, c) {
            // An auto Column with a fill child consumes the available width.
            self.inner
        } else {
            measured.w
        }
    }
    pub fn offer(&mut self, c: Key, width: Constraint) {
        self.layers.offer_width(self.e, c, width, false);
    }
    pub fn assign_width(&mut self, c: Key, width: f32, break_before: bool) {
        self.layers
            .offer_width(self.e, c, Constraint::Assigned(width), break_before);
    }
}
pub(super) struct ReflowCx<'a> {
    layers: &'a mut Layers,
    e: &'a Elements,
    pub gap: [f32; 2],
    padding: [f32; 4],
    height: Length,
    scale: f32,
    pub fixed_basis: bool,
    pub measured_height: f32,
}
impl ReflowCx<'_> {
    pub fn child(&self, c: Key) -> Size {
        self.layers.fit[c]
    }
    pub fn break_before(&self, c: Key) -> bool {
        self.layers.offers[c].break_before
    }
    pub fn inner_height(&self, outer: f32) -> f32 {
        (outer - (self.padding[0] + self.padding[2])).max(0.)
    }
    pub fn offset(&mut self, c: Key, x: f32, y: f32, height: Constraint) {
        self.layers.offer_placement(
            self.e,
            c,
            [self.padding[3] + x, self.padding[0] + y],
            height,
            false,
        );
    }
    pub fn column_basis(&self, c: Key) -> f32 {
        if self.e.attrs[c].sizing.height.weight().is_some() {
            self.layers.measure[c].h
        } else {
            self.child(c).h
        }
    }
    pub fn box_height(&self, demand: f32) -> f32 {
        if self.fixed_basis {
            // A fixed pixel extent does not depend on wrapping.
            self.measured_height
        } else {
            // Width-dependent content may differ from its pre-wrap measurement.
            self.height
                .resolve(demand + self.padding[0] + self.padding[2], || self.scale)
        }
    }
}
pub(super) struct PlaceCx<'a> {
    layers: &'a mut Layers,
    e: &'a Elements,
    pub height: f32,
    pub scale: f32,
    translated: bool,
}
impl PlaceCx<'_> {
    pub fn child(&self, c: Key) -> Size {
        self.layers.fit[c]
    }
    pub fn break_before(&self, c: Key) -> bool {
        self.layers.offers[c].break_before
    }
    pub fn measured_height(&self, c: Key) -> f32 {
        self.layers.measure[c].h
    }
    pub fn padded_height_offer(&self, style: BoxStyle) -> ([f32; 2], f32) {
        let [top, _, bottom, left] = style.padding.map(|v| pixels(v, self.scale));
        ([left, top], (self.height - (top + bottom)).max(0.))
    }
    pub fn place(&mut self, c: Key, offset: [f32; 2], height: Constraint) {
        self.layers
            .offer_placement(self.e, c, offset, height, self.translated);
    }
    pub fn translate_children(&mut self, children: &[Key]) {
        if self.translated {
            for &c in children {
                self.layers.enqueue(Place, c, self.e);
            }
        }
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
            context, measure, runs, width, offers, fit, placement, frames, paint, parts
        );
        #[cfg(test)]
        pool::reserve(&mut self.trace, 7 * capacity)?;
        self.bounds.reserve(capacity)?;
        self.intrinsic.reserve(capacity)?;
        self.owners = capacity;
        Ok(())
    }
    // Invalidate the value by queuing its owner; keep the old result until comparison.
    pub fn invalidate(&mut self, p: Pass, k: Key, depth: usize) {
        self.jobs[p as usize].push(depth, k, Outputs::Both);
    }
    pub fn invalidate_composition(&mut self, k: Key, depth: usize, outputs: Outputs) {
        self.jobs[Compose as usize].push(depth, k, outputs);
    }
    #[cfg(test)]
    pub fn worksets_empty(&self) -> bool {
        self.jobs.iter().all(WorkSet::is_empty)
    }
    pub fn mount(&mut self, k: Key, depth: usize) {
        for p in PASSES {
            self.invalidate(p, k, depth);
        }
    }
    /// Compare the saved input with the final order. A saved edge can name
    /// an evicted generation; never resolve it as a current attachment.
    pub fn invalidate_order(&mut self, e: &Elements, k: Key, old_order: &[Key], membership: bool) {
        let node = &e.slots[k];
        let per_line = node.shape == Shape::WrappedRow;
        let reads_offers = !per_line
            && node
                .shape
                .reads_flow(e.attrs[k].sizing.width, FlowInput::Order);
        let next = e.children(k);
        let different_len = old_order.len() != next.len();
        let (mut moved, mut measure, mut flow) = (different_len, different_len, different_len);
        for (&a, &b) in old_order.iter().zip(next).filter(|(a, b)| a != b) {
            moved = true;
            let (old, new) = (self.measure.get(a), self.measure.get(b));
            measure |= old != new;
            flow |= reads_offers
                && (old.map(|m| m.w) != new.map(|m| m.w)
                    || e.attrs.get(a).is_none_or(|v| {
                        !v.sizing
                            .width
                            .same_split(e.raw(a), e.attrs[b].sizing.width, e.raw(b))
                    }));
        }
        if measure {
            self.enqueue(Measure, k, e)
        }
        if moved && membership {
            // An incoming edge needs an offer even if our inner width is unchanged.
            if let Some(width) = self.width.get_mut(k) {
                width.new_edge = true;
            }
            flow = true;
            self.auto_width_reader(e, k);
        }
        // Even equal-width siblings can exchange a first-on-line edge.
        if flow || (moved && per_line) {
            self.enqueue(Flow, k, e)
        }
        if moved {
            if node.shape.offers_inner_height() {
                // Place supplies the new edge. A permutation of the same
                // children doesn't change the tallest basis.
                self.enqueue(Place, k, e);
                if (different_len || membership) && e.attrs[k].sizing.height.fits_content_height() {
                    self.enqueue(Reflow, k, e);
                }
            } else {
                self.enqueue(Reflow, k, e);
            }
            self.enqueue(Compose, k, e)
        }
    }
    pub fn reattached_container(&mut self, k: Key, e: &Elements) {
        if let Some(width) = self.width.get_mut(k) {
            width.new_edge = true;
        }
        self.enqueue(Measure, k, e);
        self.enqueue(Compose, k, e);
    }
    pub fn evict(&mut self, k: Key) {
        if let Some(run) = self.runs.remove(k) {
            self.text.release(run);
        }
        self.context.remove(k);
        self.measure.remove(k);
        self.width.remove(k);
        self.offers.remove(k);
        self.fit.remove(k);
        self.placement.remove(k);
        self.frames.remove(k);
        self.paint.remove(k);
        self.parts.remove(k);
    }
    fn enqueue(&mut self, p: Pass, k: Key, e: &Elements) {
        self.invalidate(p, k, e.depth(k));
    }
    fn parent(&mut self, p: Pass, k: Key, e: &Elements) {
        if let Some(parent) = e.slots[k].parent {
            self.enqueue(p, parent, e);
        }
    }
    // A wrapping parent plans the width of an auto Column before flowing
    // that Column. Its direct children's fill presence affects that width.
    pub(super) fn auto_width_reader(&mut self, e: &Elements, k: Key) {
        if e.slots[k].shape == Shape::Column
            && e.attrs[k].sizing.width == Length::Auto
            && e.parent_shape(k) == Some(Shape::WrappedRow)
        {
            self.parent(Flow, k, e);
        }
    }
    fn offer_width(&mut self, e: &Elements, k: Key, axis: Constraint, break_before: bool) {
        let next = WidthOffer { axis, break_before };
        let old = self.offers.get(k).copied();
        if old != Some(next) {
            self.offers.insert(k, next);
            if old.is_none_or(|p| p.axis != axis) && e.slots[k].shape.uses_offer(e, k) {
                self.enqueue(Flow, k, e);
            }
            if old.is_some_and(|p| p.break_before) != break_before
                && e.parent_shape(k) == Some(Shape::WrappedRow)
            {
                self.parent(Reflow, k, e);
            }
        }
    }
    fn offer_placement(
        &mut self,
        e: &Elements,
        k: Key,
        offset: [f32; 2],
        height: Constraint,
        translated: bool,
    ) {
        let edge = Placement { offset, height };
        if self.placement.get(k) == Some(&edge) && !translated {
            return;
        }
        let old = self.placement.insert(k, edge);
        let policy = e.attrs[k].sizing.height;
        // Compare what the element reads. Losing an assigned extent matters
        // even if the new policy ignores changes in available height.
        if translated
            || old.is_none_or(|p| {
                p.offset != offset || p.height.for_height(policy) != height.for_height(policy)
            })
        {
            self.enqueue(Place, k, e);
        }
    }
    pub fn recompute(&mut self, e: &Elements, root: Option<Key>, w: f32, h: f32) {
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
        if self.context.get(root) != Some(&e.slots[root].admitted) {
            self.enqueue(Context, root, e);
        }
        // Supply the incoming edge, even on promotion at unchanged viewport
        // size. There is no synthetic mount or separate root length resolver.
        self.offer_width(e, root, Constraint::Available(w), false);
        self.offer_placement(e, root, [0.; 2], Constraint::Available(h), false);
        if vp != self.viewport {
            self.viewport = vp;
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
                    Place => {
                        let base = e.slots[k].parent.map_or(vp, |p| self.frames[p]);
                        self.place(e, k, [base.x, base.y]);
                    }
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
                            self.invalidate_composition(p, e.depth(p), outputs)
                        }
                    }
                }
            }
        }
    }
    fn context(&mut self, e: &Elements, k: Key) {
        let ctx = e.slots[k].admitted;
        let old_scale = self.context.get(k).map(|p| p.scale);
        let scale_changed = old_scale != Some(ctx.scale);
        if changed(&mut self.context, k, ctx) {
            if e.slots[k].shape.uses_glyphs() || scale_changed {
                self.enqueue(Measure, k, e);
            }
            if scale_changed {
                // The parent's offer reads the bound at the child's scale,
                // even if the child's intrinsic box remains exactly the same.
                if let Some(old) = old_scale {
                    let moved = |l: Length| {
                        matches!(l, Length::Formula(_))
                            || matches!(l, Length::BoundedFill { bounds, .. } if bounds.device(old) != bounds.device(ctx.scale))
                    };
                    let b = e.attrs[k].sizing;
                    self.invalidate_offer_readers(
                        e,
                        k,
                        moved(b.width).then_some(FlowInput::ChildSplit),
                        moved(b.height),
                    );
                }
                if e.slots[k].shape.offers_inner_height() {
                    self.enqueue(Place, k, e); // scaled padding/gap, even at equal fit
                }
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
        let b = e.attrs[k].sizing.without_weights();
        let shape = e.slots[k].shape;
        let memo = (!shape.uses_glyphs()).then(|| {
            self.intrinsic.lookup(
                shape,
                b,
                c.scale,
                e.children(k).iter().map(|c| self.measure[*c]),
            )
        });
        let m = if let Some((_, Some(value))) = memo {
            value
        } else {
            let content = shape.measure(
                e,
                k,
                &mut Measuring {
                    layers: self,
                    e,
                    k,
                    gap: b.spacing.map(|v| pixels(v, c.scale)),
                },
            );
            let [t, r, d, l] = b.padding.map(|v| pixels(v, c.scale));
            let m = Demand {
                min_w: b.width.resolve(content.min_w + (l + r), || c.scale),
                w: b.width.resolve(content.w + (l + r), || c.scale),
                h: b.height.resolve(content.h + (t + d), || c.scale),
            };
            if let Some((slot, _)) = memo {
                self.intrinsic.store(
                    slot,
                    shape,
                    b,
                    c.scale,
                    e.children(k).iter().map(|c| self.measure[*c]),
                    m,
                );
            }
            m
        };
        // Only an explicit floor replaces the intrinsic minimum. Keep the
        // site-independent demand in the memo, including on a closed move.
        let mut m = m;
        if let Some(bounds) = e.attrs[k].line_floor
            && e.parent_shape(k) == Some(Shape::WrappedRow)
        {
            m.min_w = bounds.resolve(m.w, c.scale);
        }
        let old = self.measure.get(k).copied();
        if changed(&mut self.measure, k, m) {
            self.parent(Measure, k, e);
            if old.is_none_or(|s| s.w != m.w) {
                self.enqueue(Flow, k, e);
                if let Some(p) = e.slots[k].parent
                    && e.slots[p].shape.reads_flow(
                        e.attrs[p].sizing.width,
                        FlowInput::ChildWidth(e.attrs[k].sizing.width),
                    )
                {
                    self.enqueue(Flow, p, e);
                }
            }
            if old.is_none_or(|s| s.min_w != m.min_w)
                && e.attrs[k].sizing.width.weight().is_some()
                && e.parent_shape(k) == Some(Shape::WrappedRow)
            {
                self.parent(Flow, k, e);
            }
            // Column's natural demand charges a fill child's measured basis,
            // independent of the child's width-dependent fit.
            if old.is_none_or(|s| s.h != m.h)
                && e.attrs[k].sizing.height.weight().is_some()
                && let Some(p) = e.slots[k].parent
                && e.slots[p].shape.allots_height()
                && e.attrs[p].sizing.height.fits_content_height()
            {
                self.enqueue(Reflow, p, e);
            }
            if old.is_none_or(|s| s.w != m.w || s.h != m.h) {
                self.enqueue(Reflow, k, e);
            }
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
        let scale = self.context[k].scale;
        let outer = if shape.uses_offer(e, k) {
            self.offers[k]
                .axis
                .resolve(e, k, b.width, self.measure[k].w)
        } else {
            self.measure[k].w
        };
        let [_, r, _, l] = b.padding.map(|v| pixels(v, scale));
        let inner = (outer - (r + l)).max(0.);
        let old = self.width.insert(
            k,
            Width {
                outer,
                inner,
                new_edge: false,
            },
        );
        if old.is_none_or(|w| w.outer != outer || w.inner != inner) {
            self.enqueue(Reflow, k, e);
        }
        let offer_children = old.is_none_or(|w| w.inner != inner || w.new_edge);
        shape.offer_widths(
            e,
            k,
            &mut FlowCx {
                inner,
                offer_children,
                policy: b.width,
                gap: pixels(b.spacing[0], scale),
                layers: self,
                e,
            },
        );
    }
    fn reflow(&mut self, e: &Elements, k: Key) {
        let n = &e.slots[k];
        let b = e.attrs[k].sizing;
        let width = self.width[k];
        let scale = self.context[k].scale;
        let h = n.shape.reflow(
            e,
            k,
            &mut ReflowCx {
                gap: b.spacing.map(|v| pixels(v, scale)),
                padding: b.padding.map(|v| pixels(v, scale)),
                height: b.height,
                scale,
                fixed_basis: !b.height.fits_content_height(),
                measured_height: self.measure[k].h,
                layers: self,
                e,
            },
        );
        // Even at equal fit, a Column's shares or a definite WrappedRow's
        // line origins may need updating against the final placed height.
        let fit = Size { w: width.outer, h };
        let old = self.fit.insert(k, fit);
        if n.shape.allots_height()
            || (n.shape == Shape::WrappedRow && b.height.definite())
            || old != Some(fit)
        {
            self.enqueue(Place, k, e);
        }
        if old != Some(fit)
            && let Some(p) = n.parent
        {
            let height_changed = old.is_none_or(|s| s.h != h);
            match e.slots[p].shape {
                host if host.offers_inner_height() => {
                    if height_changed && e.attrs[p].sizing.height.fits_content_height() {
                        self.enqueue(Reflow, p, e);
                    }
                    // A Row also reads widths to place the following sibling.
                    if host.allots_width() && old.is_none_or(|s| s.w != fit.w) {
                        self.enqueue(Place, p, e);
                    }
                }
                Shape::Column => {
                    if height_changed {
                        if e.attrs[p].sizing.height.fits_content_height()
                            && b.height.weight().is_none()
                        {
                            self.enqueue(Reflow, p, e);
                        }
                        if e.attrs[p].sizing.height.definite()
                            && (b.height.weight().is_none() || b.height.reads_fill_content())
                        {
                            self.enqueue(Place, p, e);
                        }
                    }
                }
                _ => self.enqueue(Reflow, p, e),
            }
        }
    }
    fn place(&mut self, e: &Elements, k: Key, origin: [f32; 2]) {
        let s = self.fit[k];
        let scale = self.context[k].scale;
        let edge = self.placement[k];
        let [x, y] = [origin[0] + edge.offset[0], origin[1] + edge.offset[1]];
        let policy = e.attrs[k].sizing.height;
        let height = edge.height.for_height(policy).resolve(e, k, policy, s.h);
        let origin_changed = self.frames.get(k).is_none_or(|f| [f.x, f.y] != [x, y]);
        if changed(
            &mut self.frames,
            k,
            Rect {
                x,
                y,
                width: s.w,
                height,
            },
        ) {
            self.enqueue(PaintContext, k, e);
        }
        e.slots[k].shape.place_children(
            e,
            k,
            &mut PlaceCx {
                layers: self,
                e,
                height,
                scale,
                translated: origin_changed,
            },
        );
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
        let scale = self.context[k].scale;
        let next = PaintContext {
            frame: f,
            clip,
            child_clip: shape.child_clip(clip, f),
            color,
            scale,
            text_origin: [f.x + pixels(left, scale), f.y + pixels(top, scale)],
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
            self.invalidate_composition(k, e.depth(k), outputs);
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
