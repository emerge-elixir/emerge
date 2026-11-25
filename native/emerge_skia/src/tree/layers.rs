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
mod document;
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
    pub min_h: f32,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Heights {
    pub fit: f32,
    pub min: f32,
}
impl From<f32> for Heights {
    fn from(h: f32) -> Self {
        Self { fit: h, min: h }
    }
}
impl Heights {
    pub fn max(self, rhs: Self) -> Self {
        Self {
            fit: self.fit.max(rhs.fit),
            min: self.min.max(rhs.min),
        }
    }
    pub fn map(self, f: impl Fn(f32) -> f32) -> Self {
        Self {
            fit: f(self.fit),
            min: f(self.min),
        }
    }
}
#[derive(Clone, Copy)]
struct Fit {
    size: Size,
    // Natural cross offer for a WrappedRow, before fitting its full stack.
    row_offer: f32,
}
impl Fit {
    fn width_at(fits: &SecondaryMap<Key, Self>, e: &Elements, k: Key, edge: Constraint) -> f32 {
        let fit = fits[k].size;
        if !e.slots[k].shape.packs_width(e.attrs[k].sizing.width) {
            return fit.w;
        }
        let height = edge.resolve_height(e, k, fit.h);
        shapes::column_width(e, k, height, |c| fits[c].size)
    }
}
impl Size {
    pub fn heights(self) -> Heights {
        Heights {
            fit: self.h,
            min: self.min_h,
        }
    }
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
    pub(super) fn resolve_height(self, e: &Elements, k: Key, fit: f32) -> f32 {
        let policy = e.attrs[k].sizing.height;
        self.for_height(policy).resolve(e, k, policy, fit)
    }
    pub(super) fn resolve(self, e: &Elements, k: Key, policy: Length, fit: f32) -> f32 {
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
enum Placement {
    Box {
        offset: [f32; 2],
        height: Constraint,
    },
    // Reflow already resolved this rectangle relative to its document or
    // paragraph. Switching coordinate systems matters even at (0,0).
    Prose(Key),
}
impl Placement {
    fn for_height(self, policy: Length) -> Self {
        match self {
            Self::Box { offset, height } => Self::Box {
                offset,
                height: height.for_height(policy),
            },
            site => site,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct LayoutContext {
    inputs: Inputs,
    // Unlike strike resources, alignment is resolved only after the mutation interval.
    text_align: Alignment,
}
impl LayoutContext {
    fn text_takes_offer(self, width: Length) -> bool {
        width == Length::Auto && self.text_align != Alignment::Start
    }
}

#[derive(Default)]
pub(super) struct Layers {
    jobs: [WorkSet; 7],
    context: SecondaryMap<Key, LayoutContext>,
    measure: SecondaryMap<Key, Demand>,
    pub(super) intrinsic: intrinsic::Intrinsic,
    // Glyph changes and intrinsic size changes have different readers.
    pub(super) runs: SecondaryMap<Key, Run>,
    pub(super) text: TextStore,
    pub(super) paragraphs: paragraph::Paragraphs,
    pub(super) bounds: capacity::PayloadBounds,
    pub(super) owners: usize,
    pub(super) text_count: usize,
    pub(super) prepared_scale: f32,
    width: SecondaryMap<Key, Width>,
    offers: SecondaryMap<Key, WidthOffer>,
    // Only independent, declared Column groups needed by an early packed width.
    height_shares: SecondaryMap<Key, shapes::FillShares>,
    fit: SecondaryMap<Key, Fit>,
    placement: SecondaryMap<Key, Placement>,
    pub frames: SecondaryMap<Key, Rect>,
    pub(super) paint: SecondaryMap<Key, PaintContext>,
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
    pub fn scale(&self) -> f32 {
        self.layers.context[self.k].inputs.scale
    }
    pub fn allocated_image_height(&self) -> f32 {
        self.layers
            .known_height_edge(self.e, self.k)
            .expect("admitted height-driven Image")
            .resolve_height(self.e, self.k, 0.)
    }
    pub fn glyph_run(&mut self, text: &str) -> Demand {
        let k = self.k;
        let l = &mut self.layers;
        let ctx = l.context[k].inputs;
        let len = self.e.attrs[k].glyph_count;
        let run = l.runs.entry(k).unwrap().or_default();
        let font_size = ctx.font_size();
        let changed = (run.invalid
            || run.revision == 0
            || run.metrics.face != ctx.face
            || run.metrics.size != font_size)
            && l.text.measure(text, len, ctx.face, font_size, run);
        let size = Demand {
            min_w: run.metrics.width,
            w: run.metrics.width,
            h: run.metrics.height,
        };
        if changed {
            l.invalidate_composition(k, self.e.depth(k), Outputs::Draw);
            if let Some(p) = self.e.inline_host(k) {
                l.enqueue(Reflow, p, self.e);
            }
        }
        l.bounds.set(self.e.slots[k].index, len, len, false);
        size
    }
}
pub(super) struct FlowCx<'a> {
    layers: &'a mut Layers,
    e: &'a Elements,
    pub scale: f32,
    pub inner: f32,
    offer_children: bool,
}
impl FlowCx<'_> {
    pub fn measured_width(&self, c: Key) -> f32 {
        self.layers.measure[c].w
    }
    pub fn line_measure(&self, c: Key) -> f32 {
        let measured = self.layers.measure[c];
        if self.e.attrs[c].sizing.width.weight().is_some() {
            measured.min_w
        } else if self.e.slots[c].shape.requests_fill_width(self.e, c) {
            // An auto Column can claim the line. Non-left Text takes its assigned
            // box; it does not ask to break the line or redistribute its peers.
            self.inner
        } else {
            measured.w
        }
    }
    pub fn offer_inner_width(&mut self, parent: Key) {
        if self.offer_children {
            for &c in self.e.children(parent) {
                self.layers
                    .offer_width(self.e, c, Constraint::Available(self.inner), false);
            }
        }
    }
    pub fn offer_intrinsic_width(&mut self, c: Key) {
        let width = self.measured_width(c).min(self.inner);
        self.offer_available_width(c, width);
    }
    pub fn offer_available_width(&mut self, c: Key, width: f32) {
        self.layers
            .offer_width(self.e, c, Constraint::Available(width), false);
    }
    pub fn assign_width(&mut self, c: Key, width: f32, break_before: bool) {
        self.layers
            .offer_width(self.e, c, Constraint::Assigned(width), break_before);
    }
}
pub(super) struct ReflowCx<'a> {
    layers: &'a mut Layers,
    pub gap: [f32; 2],
    padding: [f32; 4],
    height: Length,
    scale: f32,
    pub fixed_basis: bool,
    pub measured_height: f32,
    pub row_offer: f32,
}
impl ReflowCx<'_> {
    pub fn child(&self, c: Key) -> Size {
        self.layers.fit[c].size
    }
    pub fn break_before(&self, c: Key) -> bool {
        self.layers.offers[c].break_before
    }
    pub fn inner_width(&self, k: Key) -> f32 {
        self.layers.width[k].inner
    }
    pub fn inner_height(&self, outer: f32) -> f32 {
        (outer - (self.padding[0] + self.padding[2])).max(0.)
    }
    pub fn measured_height(&self, c: Key) -> f32 {
        self.layers.measure[c].h
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
    height: f32,
    width: f32,
    pub scale: f32,
    pub row_offer: f32,
    translated: bool,
}
impl PlaceCx<'_> {
    pub fn place_site(&mut self, host: Key, c: Key) {
        self.layers
            .offer_edge(self.e, c, Placement::Prose(host), self.translated);
    }
    pub fn child(&self, c: Key) -> Size {
        self.layers.fit[c].size
    }
    pub fn break_before(&self, c: Key) -> bool {
        self.layers.offers[c].break_before
    }
    pub fn measured_height(&self, c: Key) -> f32 {
        self.layers.measure[c].h
    }
    /// Local area for child placement, not a paint or hit clip.
    pub fn padded_area(&self, style: BoxStyle) -> Rect {
        let [top, right, bottom, left] = style.padding.map(|v| pixels(v, self.scale));
        Rect {
            x: left,
            y: top,
            width: (self.width - (left + right)).max(0.),
            height: (self.height - (top + bottom)).max(0.),
        }
    }
    pub fn declared_shares(&self, k: Key) -> Option<shapes::FillShares> {
        self.layers.height_shares.get(k).copied()
    }
    pub fn width_at(&self, c: Key, edge: Constraint) -> f32 {
        Fit::width_at(&self.layers.fit, self.e, c, edge)
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
            context,
            measure,
            runs,
            width,
            offers,
            height_shares,
            fit,
            placement,
            frames,
            paint,
            parts
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
        if measure || (moved && node.shape == Shape::WrappedColumn) {
            self.enqueue(Measure, k, e)
        }
        if moved
            && node.shape == Shape::Column
            && (self.has_height_group(k) || (membership && Self::has_group_reader(e, k)))
        {
            self.enqueue(Context, k, e);
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
        self.paragraphs.evict(k);
        if let Some(run) = self.runs.remove(k) {
            self.text.release(run);
        }
        self.context.remove(k);
        self.measure.remove(k);
        self.width.remove(k);
        self.offers.remove(k);
        self.height_shares.remove(k);
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
    pub(super) fn packed_width_reader(&mut self, e: &Elements, k: Key) {
        if let Some(p) = e.slots[k].parent {
            if e.slots[p].shape.reads_prose_box(e, k) {
                self.enqueue(Reflow, p, e);
            } else if e.slots[p].shape.reads_placed_width(e, k) {
                self.enqueue(Place, p, e);
            }
        }
    }
    // A wrapping parent plans the width of an auto Column before flowing
    // that Column. Its direct children's fill presence affects that width.
    pub(super) fn auto_width_reader(&mut self, e: &Elements, k: Key) {
        if matches!(e.slots[k].shape, Shape::Column | Shape::TextColumn)
            && e.attrs[k].sizing.width == Length::Auto
        {
            match e.parent_shape(k) {
                Some(Shape::WrappedRow) => self.parent(Flow, k, e),
                Some(Shape::WrappedColumn) => self.parent(Measure, k, e),
                _ => (),
            }
        }
    }
    fn reads_width_offer(&self, e: &Elements, k: Key) -> bool {
        let shape = e.slots[k].shape;
        shape.requests_fill_width(e, k)
            || (shape == Shape::Paragraph && !e.attrs[k].sizing.width.definite())
            || (shape == Shape::Text
                && self
                    .context
                    .get(k)
                    .is_some_and(|c| c.text_takes_offer(e.attrs[k].sizing.width)))
    }
    fn resolved_width(&self, e: &Elements, k: Key, edge: Constraint) -> f32 {
        let measured = self.measure[k].w;
        if e.slots[k].shape == Shape::Paragraph && !e.attrs[k].sizing.width.definite() {
            match edge {
                Constraint::Natural => measured,
                Constraint::Available(w) | Constraint::Assigned(w) => e.attrs[k]
                    .sizing
                    .width
                    .resolve(measured.min(w), || e.slots[k].admitted.scale),
            }
        } else if self.reads_width_offer(e, k) {
            edge.resolve(e, k, e.attrs[k].sizing.width, measured)
        } else {
            measured
        }
    }
    fn offer_width(&mut self, e: &Elements, k: Key, axis: Constraint, break_before: bool) {
        let next = WidthOffer { axis, break_before };
        let old = self.offers.get(k).copied();
        if old != Some(next) {
            self.offers.insert(k, next);
            if old.is_none_or(|p| p.axis != axis) && self.reads_width_offer(e, k) {
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
        let edge = e
            .inline_host(k)
            .map_or(Placement::Box { offset, height }, Placement::Prose);
        self.offer_edge(e, k, edge, translated);
    }
    fn placed_site(&self, host: Key, child: Key) -> Rect {
        let mut r = self.paragraphs.site(child);
        r.x += self.frames[host].x;
        r.y += self.frames[host].y;
        r
    }
    fn offer_edge(&mut self, e: &Elements, k: Key, edge: Placement, translated: bool) {
        let translated = translated
            || matches!(edge,Placement::Prose(host) if self.frames.get(k) != Some(&self.placed_site(host,k)));
        if self.placement.get(k) == Some(&edge) && !translated {
            return;
        }
        let old = self.placement.insert(k, edge);
        let policy = e.attrs[k].sizing.height;
        // Compare what the element reads. Losing an assigned extent matters
        // even if the new policy ignores changes in available height.
        if translated || old.is_none_or(|p| p.for_height(policy) != edge.for_height(policy)) {
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
        if self.context.get(root).map(|c| c.inputs) != Some(e.slots[root].admitted) {
            self.enqueue(Context, root, e);
        }
        // Supply the incoming edge, even on promotion at unchanged viewport
        // size. The viewport offers space, not alignment: the root offset is
        // always zero. Its own child placement still reads its declaration.
        self.offer_width(e, root, Constraint::Available(w), false);
        self.offer_placement(e, root, [0.; 2], Constraint::Available(h), false);
        if vp != self.viewport {
            if vp.height != self.viewport.height && Self::reads_early_height(e, root) {
                self.enqueue(Measure, root, e);
            }
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
    pub(super) fn reads_early_height(e: &Elements, k: Key) -> bool {
        let b = e.attrs[k].sizing;
        match e.slots[k].shape {
            Shape::Image => b.width_from_height(),
            Shape::WrappedColumn => {
                !matches!(b.width, Length::Px(_)) && b.height.weight().is_some()
            }
            _ => false,
        }
    }
    pub(super) fn invalidate_early_heights(&mut self, e: &Elements, p: Key) {
        for &c in e.children(p) {
            if Self::reads_early_height(e, c) {
                self.enqueue(Measure, c, e);
            }
        }
    }
    pub(super) fn has_height_group(&self, k: Key) -> bool {
        self.height_shares.contains_key(k)
    }
    pub(super) fn has_group_reader(e: &Elements, p: Key) -> bool {
        matches!(e.attrs[p].sizing.height, Length::Px(_))
            && e.children(p).iter().any(|&c| {
                e.slots[c]
                    .shape
                    .reads_group_height(e.attrs[c].sizing, e.attrs[p].sizing)
            })
    }
    fn known_height_edge(&self, e: &Elements, k: Key) -> Option<Constraint> {
        let Some(p) = e.slots[k].parent else {
            return Some(Constraint::Available(self.viewport.height));
        };
        let style = e.attrs[p].sizing;
        if let Some(px) = e.slots[p].shape.cross_height_px(style) {
            let s = e.slots[p].admitted.scale;
            Some(Constraint::Available(
                (pixels(px, s) - (pixels(style.padding[0], s) + pixels(style.padding[2], s)))
                    .max(0.),
            ))
        } else {
            self.height_shares
                .get(p)?
                .assigned_height(e, k)
                .map(Constraint::Assigned)
        }
    }
    fn context(&mut self, e: &Elements, k: Key) {
        if e.slots[k].shape == Shape::Column {
            let shares = shapes::FillShares::declared_column(e, k);
            if shares != self.height_shares.get(k).copied() {
                if let Some(value) = shares {
                    self.height_shares.insert(k, value);
                } else {
                    self.height_shares.remove(k);
                }
                self.invalidate_early_heights(e, k);
            }
        }
        let ctx = e.slots[k].admitted;
        let text_align = e.attrs[k]
            .text_align
            .or_else(|| e.slots[k].parent.map(|p| self.context[p].text_align))
            .unwrap_or_default();
        let old = self.context.get(k).copied();
        let old_scale = old.map(|c| c.inputs.scale);
        let scale_changed = old_scale != Some(ctx.scale);
        let inputs_changed = old.is_none_or(|c| c.inputs != ctx);
        let next = LayoutContext {
            inputs: ctx,
            text_align,
        };
        if changed(&mut self.context, k, next) {
            if e.slots[k].shape == Shape::Paragraph
                && old.is_none_or(|p| p.text_align != text_align)
            {
                self.enqueue(Reflow, k, e);
            }
            if let Some(prev) = old
                && e.slots[k].shape == Shape::Text
                && prev.text_align != text_align
            {
                self.enqueue(PaintContext, k, e);
                let width = e.attrs[k].sizing.width;
                if prev.text_takes_offer(width) != next.text_takes_offer(width)
                    && !e.parent_shape(k).is_some_and(Shape::allots_width)
                {
                    self.enqueue(Flow, k, e);
                    if let Some(p) = e.slots[k].parent
                        && e.slots[p]
                            .shape
                            .measures_packed_width(e.attrs[p].sizing.width)
                    {
                        self.enqueue(Measure, p, e);
                    }
                }
            }
            if (e.slots[k].shape.uses_glyphs() && inputs_changed) || scale_changed {
                self.enqueue(Measure, k, e);
            }
            if scale_changed {
                if e.attrs[k].sizing.padding != [0.; 4]
                    && let Some(p) = e.inline_host(k)
                {
                    self.enqueue(Reflow, p, e);
                }
                if e.slots[k]
                    .shape
                    .cross_height_px(e.attrs[k].sizing)
                    .is_some()
                {
                    self.invalidate_early_heights(e, k);
                }
                // The parent's offer reads the bound at the child's scale,
                // even if the child's intrinsic box remains exactly the same.
                if let Some(old) = old_scale {
                    let moved = |l: Length| {
                        matches!(l, Length::Formula(_))
                            || matches!(l, Length::BoundedFill { bounds, .. } if bounds.device(old) != bounds.device(ctx.scale))
                    };
                    if let Some(p) = e.slots[k].parent
                        && e.slots[p].shape == Shape::WrappedColumn
                    {
                        if moved(e.attrs[k].sizing.width)
                            && !matches!(e.attrs[p].sizing.width, Length::Px(_))
                        {
                            self.enqueue(Measure, p, e);
                        }
                        if e.attrs[k].height_floor.is_some() {
                            self.enqueue(Reflow, p, e);
                            if !matches!(e.attrs[p].sizing.width, Length::Px(_)) {
                                self.enqueue(Measure, p, e);
                            }
                        }
                    }
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
                if inputs_changed || e.attrs[c].text_align.is_none() {
                    self.enqueue(Context, c, e);
                }
            }
        }
    }
    fn measure(&mut self, e: &Elements, k: Key) {
        let c = self.context[k].inputs;
        let b = e.attrs[k].sizing.without_weights();
        let shape = e.slots[k].shape;
        let memo = (!shape.uses_glyphs() && shape != Shape::Image).then(|| {
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
        // Keep the site-independent demand in the memo, including on a closed move.
        let mut m = if shape.measures_packed_width(b.width) {
            shapes::column_demand(
                e,
                k,
                m,
                self.known_height_edge(e, k),
                |c| self.measure[c],
                |c, inner| self.resolved_width(e, c, Constraint::Available(inner)),
            )
        } else {
            m
        };
        let floor = e.attrs[k]
            .line_floor
            .filter(|_| e.parent_shape(k) == Some(Shape::WrappedRow));
        m.min_w = wrap_minimum(floor, m.w, m.min_w, || c.scale);
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
                // A side float searches for a band with its *measured*
                // dimensions before placing the reflowed box. Its actual fit
                // and even the document's aggregate measure can stay equal.
                if e.is_prose_float(k) {
                    self.parent(Reflow, k, e);
                }
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
        let scale = self.context[k].inputs.scale;
        let outer = self.resolved_width(e, k, self.offers[k].axis);
        let [_, r, _, l] = b.padding.map(|v| pixels(v, scale));
        let inner = if shape.packs_width(b.width) {
            // Child offers use the single-column basis, never the packed width.
            let one = e
                .children(k)
                .iter()
                .map(|c| self.measure[*c].w)
                .fold(0., f32::max);
            (b.width.resolve(one + (r + l), || scale) - (r + l)).max(0.)
        } else {
            (outer - (r + l)).max(0.)
        };
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
                scale,
                layers: self,
                e,
            },
        );
    }
    fn reflow(&mut self, e: &Elements, k: Key) {
        let n = &e.slots[k];
        if n.shape == Shape::Paragraph
            && e.parent_shape(k) == Some(Shape::TextColumn)
            && e.float_side(k).is_none()
        {
            // Only the document knows this paragraph's incoming bands. Its
            // descendants have settled; delegate the single line solve to it.
            self.paragraphs.invalidate_lines(k);
            self.parent(Reflow, k, e);
            return;
        }
        let b = e.attrs[k].sizing;
        let width = self.width[k];
        let scale = self.context[k].inputs.scale;
        let mut cx = ReflowCx {
            gap: b.spacing.map(|v| pixels(v, scale)),
            padding: b.padding.map(|v| pixels(v, scale)),
            height: b.height,
            scale,
            fixed_basis: !b.height.fits_content_height(),
            measured_height: self.measure[k].h,
            row_offer: 0.,
            layers: self,
        };
        let heights = n.shape.reflow(e, k, &mut cx);
        let row_offer = cx.row_offer;
        // Even at equal fit, a Column's shares or a definite WrappedRow's
        // line origins may need updating against the final placed height.
        let h = heights.fit;
        let fit = Size {
            w: width.outer,
            h,
            min_h: heights.min,
        };
        let old = self
            .fit
            .insert(
                k,
                Fit {
                    size: fit,
                    row_offer,
                },
            )
            .map(|f| f.size);
        if n.shape.allots_height()
            || matches!(
                n.shape,
                Shape::WrappedRow | Shape::Paragraph | Shape::TextColumn
            )
            || old != Some(fit)
        {
            self.enqueue(Place, k, e);
        }
        if n.shape.packs_width(b.width) {
            self.packed_width_reader(e, k);
        }
        if old != Some(fit)
            && let Some(p) = n.parent
        {
            let height_changed = old.is_none_or(|s| s.h != h);
            match e.slots[p].shape {
                host if host.offers_inner_height() => {
                    if old.is_none_or(|s| s.min_h != fit.min_h || s.h != h)
                        && e.attrs[p].sizing.height.fits_content_height()
                    {
                        self.enqueue(Reflow, p, e);
                    }
                    if (height_changed && e.alignment(k, 1) != Alignment::Start)
                        || (old.is_none_or(|s| s.w != fit.w) && host.reads_placed_width(e, k))
                    {
                        self.enqueue(Place, p, e);
                    }
                }
                Shape::WrappedColumn => {
                    self.enqueue(Reflow, p, e);
                    self.enqueue(Place, p, e);
                }
                Shape::Column => {
                    if old.is_none_or(|s| s.w != fit.w || height_changed)
                        && Shape::Column.reads_placed_width(e, k)
                    {
                        self.enqueue(Place, p, e);
                    }
                    if old.is_none_or(|s| s.min_h != fit.min_h)
                        && e.attrs[p].sizing.height.fits_content_height()
                    {
                        self.enqueue(Reflow, p, e);
                    }
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
        let Fit { size: s, row_offer } = self.fit[k];
        let scale = self.context[k].inputs.scale;
        let rect = match self.placement[k] {
            Placement::Prose(host) => self.placed_site(host, k),
            Placement::Box { offset, height } => Rect {
                x: origin[0] + offset[0],
                y: origin[1] + offset[1],
                width: Fit::width_at(&self.fit, e, k, height),
                height: height.resolve_height(e, k, s.h),
            },
        };
        let origin_changed = self
            .frames
            .get(k)
            .is_none_or(|f| [f.x, f.y] != [rect.x, rect.y]);
        let Rect { width, height, .. } = rect;
        if changed(&mut self.frames, k, rect) {
            self.enqueue(PaintContext, k, e);
        }
        e.slots[k].shape.place_children(
            e,
            k,
            &mut PlaceCx {
                layers: self,
                e,
                height,
                width,
                scale,
                row_offer,
                translated: origin_changed,
            },
        );
    }
    fn paint_context(&mut self, e: &Elements, k: Key) {
        let parent = e.slots[k].parent;
        let a = &e.attrs[k];
        let inherited = parent.map(|p| self.paint[p]);
        let scale = self.context[k].inputs.scale;
        let offset = std::array::from_fn(|i| {
            inherited.map_or(0., |p| p.offset[i]) + pixels(a.offset[i], scale)
        });
        let mut f = self.frames[k];
        f.x += offset[0];
        f.y += offset[1];
        let gradient = if a.font_gradient.len != 0 {
            Some((k, e.slots[k].ink_revision))
        } else if a.color.is_some() {
            None
        } else {
            inherited.and_then(|p| p.gradient)
        };
        let color = if gradient.is_some() {
            0xff
        } else {
            a.color
                .or_else(|| inherited.map(|p| p.color))
                .unwrap_or(0xff)
        };
        let rules = [a.underline, a.strike]
            .into_iter()
            .enumerate()
            .fold(0u8, |mask, (i, b)| {
                mask | (u8::from(
                    b.unwrap_or_else(|| inherited.is_some_and(|p| p.rules & (1 << i) != 0)),
                ) << i)
            });
        let shape = e.slots[k].shape;
        let clip = parent.map_or(self.viewport, |p| self.paint[p].child_clip);
        // A borrowed media shape turns with the leaf. Do not rotate the
        // intersection with its ancestors (in particular a linear host clip).
        let borrowed =
            parent.filter(|&p| a.posed() && !a.image_host_clip && e.attrs[p].clips_media());
        let outer = borrowed.map_or(clip, |p| {
            let host = self.paint[p];
            e.slots[p].shape.child_clip(host.clip, host.frame)
        });
        let ctx = self.context[k];
        let next = PaintContext {
            frame: f,
            clip: outer,
            child_clip: if a.posed() {
                borrowed.map_or(f, |p| {
                    let host = self.paint[p];
                    e.attrs[p].decor.inner_rect(host.frame, host.scale)
                })
            } else if shape == Shape::Image {
                // A leaf's content is the sample, not another layout box.
                clip.media_clip(inherited)
            } else if a.clips_media() {
                clip.intersection(a.decor.inner_rect(f, scale))
                    .unwrap_or_default()
            } else {
                shape.child_clip(clip, f)
            },
            color,
            gradient,
            rules,
            offset,
            scale: ctx.inputs.scale,
            text_align: ctx.text_align,
        };
        let old = self.paint.get(k).copied();
        if old == Some(next) {
            return;
        }
        self.paint.insert(k, next);
        if let Some(p) = e.inline_host(k) {
            let hit = a.click && old.is_none_or(|ctx| ctx.scale != next.scale || ctx.frame != f);
            self.invalidate_composition(
                p,
                e.depth(p),
                if hit { Outputs::Both } else { Outputs::Draw },
            );
        }
        let child_clip_changed = old.is_none_or(|p| p.child_clip != next.child_clip);
        let geometry_changed = old.is_none_or(|p| p.frame != f || p.clip != outer)
            || (a.posed() && child_clip_changed);
        let color_changed = old.is_none_or(|p| p.color != color || p.gradient != gradient);
        let inputs_changed = old.is_none_or(|p| p.rules != rules || p.offset != offset);
        let draw = shape == Shape::Paragraph
            || a.decor.active()
            || shape.reads_paint_context(a, old, next);
        let hit = (a.click || shape == Shape::Paragraph || a.decor.radius != [0.; 4])
            && (geometry_changed || old.is_none_or(|p| p.scale != next.scale));
        if let Some(outputs) = Outputs::changed(draw, hit) {
            self.invalidate_composition(k, e.depth(k), outputs);
        }
        let outer_clip_changed = old.is_none_or(|p| p.clip != clip);
        let frame_changed = old.is_none_or(|p| p.frame != f);
        if child_clip_changed
            || color_changed
            || inputs_changed
            || outer_clip_changed
            || frame_changed
        {
            for &child in e.children(k) {
                if child_clip_changed
                    || inputs_changed
                    || (color_changed && e.attrs[child].color.is_none())
                    || (outer_clip_changed && e.slots[child].shape == Shape::Image)
                    || (frame_changed && e.attrs[child].posed())
                {
                    self.enqueue(PaintContext, child, e);
                }
            }
        }
    }
}
