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
    Project,
}
use Pass::*;
mod anchors;
mod constrained;
mod document;
mod projection;
use constrained::{Basis, HeightOffers};
use scale::{Inputs, pixels};
impl Pass {
    fn direction(self) -> Direction {
        match self {
            Measure | Reflow | Compose => Direction::DeepFirst,
            Context | Flow | Place | PaintContext | Project => Direction::ShallowFirst,
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
    size: Size,  // unallotted occupied extents for the parent
    local: Size, // resolved on the authored axes before rotation
    // Natural cross offer for a WrappedRow, before fitting its full stack.
    row_offer: f32,
}
impl Fit {
    fn local_width_at(
        fits: &SecondaryMap<Key, Self>,
        e: &Elements,
        k: Key,
        edge: Constraint,
    ) -> f32 {
        let fit = fits[k].local;
        let style = e.attrs[k].sizing;
        if e.slots[k].shape == Shape::Image && style.width_from_height() {
            return shapes::image_width_at_height(e, k, edge.resolve_height(e, k, fit.h))
                .unwrap_or(fit.w);
        }
        if !e.slots[k].shape.packs_width(style.width) {
            return fit.w;
        }
        let height = edge.resolve_height(e, k, fit.h);
        shapes::column_width(
            e,
            k,
            height,
            |c| (fits[c].local, fits[c].size),
            |c, edge| Self::width_at(fits, e, c, edge),
        )
    }
    fn local_at(fits: &SecondaryMap<Key, Self>, e: &Elements, k: Key, edge: Constraint) -> Size {
        let local = fits[k].local;
        Size {
            w: Self::local_width_at(fits, e, k, edge),
            h: edge.resolve_height(e, k, local.h),
            ..local
        }
    }
    fn width_at(fits: &SecondaryMap<Key, Self>, e: &Elements, k: Key, edge: Constraint) -> f32 {
        if e.attrs[k].rotation.mixes_axes() {
            Self::local_at(fits, e, k, edge)
                .turned(e.attrs[k].rotation)
                .w
        } else {
            Self::local_width_at(fits, e, k, edge)
        }
    }
}
impl Size {
    fn turned(self, r: Rotation) -> Self {
        let (w, h) = r.extent(self.w, self.h);
        Self {
            w,
            h,
            min_h: r.extent(self.w, self.min_h).1,
        }
    }
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
    // A projected width can be offered through more than one filling wrapper.
    projected_height: Option<f32>,
}
#[derive(Clone, Copy, PartialEq)]
struct Width {
    outer: f32,
    inner: f32,
    new_edge: bool,
    child_height: Option<f32>,
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
    Occupied(Key), // float or document block: site is the parent-facing slot
    Inline(Key),   // span or its Text: site is the unturned local frame
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
    // Inline text is measured but placed by its paragraph, not in a block slot.
    inline_host: Option<Key>,
    // Only definite inherited offers on branches with an early width projection.
    inner_offer: Option<f32>,
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
    jobs: [WorkSet; Project as usize + 1],
    // Later layout ranks share a single membership/link table. Each dirty
    // occurrence has one entry, irrespective of the number of dirty phases.
    deferred: Option<WorkSet<u8>>,
    rank: u16,
    constrained: constrained::Constrained,
    context: SecondaryMap<Key, LayoutContext>,
    measure: SecondaryMap<Key, Demand>,
    // Retain both sides of a rotation boundary, not a second layout tree.
    local_measure: SecondaryMap<Key, Demand>,
    local_frames: SecondaryMap<Key, Rect>,
    pub(super) scroll: scroll::Ports,
    pub(super) navigation_changed: bool,
    pub(super) focus_ports_changed: bool,
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
    // Unallotted single-row cross basis, never the height of a wrapped stack.
    natural_cross: SecondaryMap<Key, f32>,
    // Width measurement only. Placement allocates from the final child fits.
    measured_shares: SecondaryMap<Key, shapes::FillShares>,
    slot_heights: SecondaryMap<Key, f32>,
    fit: SecondaryMap<Key, Fit>,
    placement: SecondaryMap<Key, Placement>,
    pub frames: SecondaryMap<Key, Rect>,
    pub(super) paint: SecondaryMap<Key, PaintContext>,
    pub(super) projected: SecondaryMap<Key, paint::ProjectionSlot>,
    pub(super) edge_epoch: u64,
    pub(super) parts: SecondaryMap<Key, Composition>,
    pub(super) viewport: Rect,
    pub(super) scale_declarations: usize,
    #[cfg(test)]
    draining: Option<Pass>,
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
    heights: HeightOffers,
}
impl Measuring<'_> {
    pub fn child(&self, c: Key) -> Demand {
        self.layers.child_demand(self.e, self.k, c, self.heights)
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
            if let Some(p) = l.context[k].inline_host {
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
    host: Key,
    heights: HeightOffers,
    basis: Basis,
}
impl FlowCx<'_> {
    pub fn measured_width(&self, c: Key) -> f32 {
        if self.e.attrs[c].sizing.width.weight().is_some() {
            // The fill's content operand is local, not the occupied extent.
            self.heights.for_child(self.layers, self.e, c).map_or_else(
                || self.layers.local_measure[c].w,
                |h| self.layers.projected_local_width(self.e, c, h),
            )
        } else {
            self.layers
                .child_demand(self.e, self.host, c, self.heights)
                .w
        }
    }
    pub fn line_measure(&self, c: Key) -> f32 {
        if self.e.attrs[c].sizing.width.weight().is_some() {
            self.layers.local_measure[c].min_w
        } else if self.e.slots[c].shape.requests_fill_width(self.e, c) {
            // An auto Column can claim the line. Non-left Text takes its assigned
            // box; it does not ask to break the line or redistribute its peers.
            self.inner
        } else {
            self.measured_width(c)
        }
    }
    pub fn offer_inner_width(&mut self, parent: Key) {
        if self.offer_children {
            for &c in self.e.children(parent) {
                self.offer(c, Constraint::Available(self.inner), false);
            }
        }
    }
    pub fn offer_intrinsic_width(&mut self, c: Key) {
        let width = self.layers.local_measure[c].w.min(self.inner);
        self.offer_available_width(c, width);
    }
    pub fn offer_available_width(&mut self, c: Key, width: f32) {
        self.offer(c, Constraint::Available(width), false);
    }
    pub fn assign_width(&mut self, c: Key, width: f32, break_before: bool) {
        self.offer(c, Constraint::Assigned(width), break_before);
    }
}
pub(super) struct ReflowCx<'a> {
    layers: &'a mut Layers,
    basis: Basis,
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
        self.basis.fits(self.layers)[c].size
    }
    pub fn break_before(&self, c: Key) -> bool {
        self.basis.break_before(self.layers, c)
    }
    pub fn inner_width(&self, k: Key) -> f32 {
        self.basis.width(self.layers, k).inner
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
        let edge = if self.layers.context[c].inline_host == Some(host) {
            let inline = Placement::Inline(host);
            // The Text and El can have different pivots. Supply both sites
            // even when the wrapper's bounds alone compare equal.
            if let Some(source) = self.e.span_source(c).filter(|&source| source != c) {
                self.layers
                    .offer_edge(self.e, source, inline, self.translated);
            }
            inline
        } else {
            Placement::Occupied(host)
        };
        self.layers.offer_edge(self.e, c, edge, self.translated);
    }
    pub fn child(&self, c: Key) -> Size {
        self.layers.fit[c].size
    }
    pub fn break_before(&self, c: Key) -> bool {
        self.layers.offers[c].break_before
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
    pub fn local_child(&self, k: Key) -> Size {
        self.layers.fit[k].local
    }
    pub fn intrinsic_turn_height(&self, k: Key) -> Option<f32> {
        let a = &self.e.attrs[k];
        // The peer inside this card fits at the width left by its projected
        // art. Its later lines cannot increase the height that chose that
        // width. Use the same evaluated cap for the occupied slot and advance.
        if self.e.slots[k].stats.own.fit_after_share
            && a.rotation.mixes_axes()
            && a.sizing.width.weight().is_none()
            && a.sizing.height.reads_fill_content()
        {
            self.layers.offers[k].projected_height
        } else {
            None
        }
    }
    pub fn local_height(&self, k: Key) -> f32 {
        self.layers.local_measure[k].h
    }
    pub fn occupied_at(&self, c: Key, edge: Constraint) -> Size {
        Fit::local_at(&self.layers.fit, self.e, c, edge).turned(self.e.attrs[c].rotation)
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
        for q in &mut self.jobs[..Project as usize] {
            q.reserve(keys, capacity)?
        }
        macro_rules! room {($($map:ident),+) => {$(self.$map.try_set_capacity(keys).map_err(pool::capacity_error)?;)+}}
        room!(
            context,
            measure,
            local_measure,
            local_frames,
            runs,
            width,
            offers,
            height_shares,
            natural_cross,
            measured_shares,
            slot_heights,
            fit,
            placement,
            frames,
            paint,
            parts
        );
        #[cfg(test)]
        pool::reserve(&mut self.trace, (Project as usize + 1) * capacity)?;
        self.bounds.reserve(capacity)?;
        self.constrained.reserve(keys)?;
        self.intrinsic.reserve(capacity)?;
        self.owners = capacity;
        Ok(())
    }
    pub fn reserve_ranks(&mut self, keys: usize, mounts: usize) -> Result<(), String> {
        if mounts != 0 {
            self.deferred
                .get_or_insert_default()
                .reserve(keys, mounts.min(MAX_DEPTH) + 1)?;
        }
        Ok(())
    }
    // Queue an already classified phase in the active rank (or a paint phase).
    pub(super) fn invalidate(&mut self, p: Pass, k: Key, depth: usize) {
        #[cfg(test)]
        assert!(
            self.draining
                .is_none_or(|current| p as usize >= current as usize),
            "{p:?} enqueued while draining {:?} in rank {}",
            self.draining,
            self.rank
        );
        self.jobs[p as usize].push(depth, k, Outputs::Both);
    }
    pub fn invalidate_composition(&mut self, k: Key, depth: usize, outputs: Outputs) {
        if let Some(p) = self.parts.get_mut(k) {
            for (s, n) in p.own_epoch.iter_mut().enumerate() {
                if outputs.includes(s) {
                    *n += 1;
                }
            }
        }
        self.jobs[Compose as usize].push(depth, k, outputs);
    }
    #[cfg(test)]
    pub fn worksets_empty(&self) -> bool {
        self.deferred.as_ref().is_none_or(WorkSet::is_empty)
            && self.jobs.iter().all(WorkSet::is_empty)
    }
    pub(super) fn begin_measure_inputs(&mut self, e: &Elements, root: Option<Key>) {
        self.constrained.begin(root.is_some_and(|k| {
            let s = e.slots[k].stats;
            s.own.constrained_measure || s.mounted_constraints
        }));
    }
    pub fn mount(&mut self, k: Key, e: &Elements) {
        for p in PASSES {
            self.enqueue(p, k, e);
        }
    }
    /// Compare the saved input with the final order. A saved edge can name
    /// an evicted generation; never resolve it as a current attachment.
    pub fn invalidate_order(
        &mut self,
        e: &Elements,
        k: Key,
        old: Edges<'_, Key>,
        membership: bool,
    ) {
        let new = e.order(k);
        if !old.nearby().eq(new.nearby())
            && let Some(sides) = Composition::mount_order_change(&self.parts, k, old, new)
        {
            self.invalidate_composition(k, e.depth(k), sides);
        }
        let old_order = old.children();
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
        if moved {
            self.scroll.forget_order(e, k);
        }
        if moved && e.attrs[k].scroll.port() {
            self.scroll.resize(k);
            self.enqueue(PaintContext, k, e);
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
        self.scroll.forget_order(e, k);
        if let Some(width) = self.width.get_mut(k) {
            width.new_edge = true;
        }
        self.enqueue(Measure, k, e);
        self.enqueue(Compose, k, e);
    }
    pub fn evict(&mut self, k: Key) {
        self.scroll.evict(k);
        self.projected.remove(k);
        self.constrained.evict(k);
        self.paragraphs.evict(k);
        if let Some(run) = self.runs.remove(k) {
            self.text.release(run);
        }
        self.context.remove(k);
        self.measure.remove(k);
        self.local_measure.remove(k);
        self.local_frames.remove(k);
        self.width.remove(k);
        self.offers.remove(k);
        self.height_shares.remove(k);
        self.natural_cross.remove(k);
        self.measured_shares.remove(k);
        self.slot_heights.remove(k);
        self.fit.remove(k);
        self.placement.remove(k);
        self.frames.remove(k);
        self.paint.remove(k);
        self.parts.remove(k);
    }
    pub(super) fn enqueue(&mut self, p: Pass, k: Key, e: &Elements) {
        let [depth, rank] = e.depths(k);
        if (p as usize) < PaintContext as usize && rank != self.rank {
            #[cfg(any(test, debug_assertions))]
            assert!(rank > self.rank, "layout sent to a settled host");
            self.deferred.as_mut().expect("reserved mount ranks").push(
                usize::from(rank),
                k,
                1 << p as usize,
            );
        } else {
            if matches!(p, Measure | Flow | Reflow) {
                self.invalidate_constraints(e, k);
            }
            self.invalidate(p, k, usize::from(depth));
        }
    }
    fn parent(&mut self, p: Pass, k: Key, e: &Elements) {
        if let Some(parent) = e.layout_parent(k) {
            self.enqueue(p, parent, e);
        }
    }
    pub(super) fn packed_width_reader(&mut self, e: &Elements, k: Key) {
        if let Some(p) = e.layout_parent(k) {
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
    pub(super) fn reads_width_offer(&self, e: &Elements, k: Key) -> bool {
        let shape = e.slots[k].shape;
        shape.requests_fill_width(e, k)
            || (shape == Shape::Paragraph && !e.attrs[k].sizing.width.definite())
            || (shape == Shape::Text
                && self
                    .context
                    .get(k)
                    .is_some_and(|c| c.text_takes_offer(e.attrs[k].sizing.width)))
    }
    fn resolved_width(&self, e: &Elements, k: Key, edge: Constraint, measured: f32) -> f32 {
        // A Row's non-fill charge is an occupied width, not a constraint on
        // the paragraph, aligned text or container in the local frame.
        if matches!(edge, Constraint::Assigned(_))
            && e.attrs[k].rotation.mixes_axes()
            && e.attrs[k].sizing.width.weight().is_none()
        {
            return measured;
        }
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
    fn offer_width(&mut self, e: &Elements, k: Key, next: WidthOffer) {
        let WidthOffer {
            axis, break_before, ..
        } = next;
        let old = self.offers.get(k).copied();
        if old != Some(next) {
            self.offers.insert(k, next);
            if old.is_none_or(|p| p.projected_height != next.projected_height)
                || (old.is_none_or(|p| p.axis != axis) && self.reads_width_offer(e, k))
            {
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
        self.offer_edge(e, k, Placement::Box { offset, height }, translated);
    }
    fn placed_site(&self, host: Key, child: Key) -> Rect {
        let mut r = self.paragraphs.site(child);
        r.x = coordinate(r.x + self.local_frames[host].x);
        r.y = coordinate(r.y + self.local_frames[host].y);
        r
    }
    fn offer_edge(&mut self, e: &Elements, k: Key, edge: Placement, translated: bool) {
        let translated = translated
            || match edge {
                Placement::Inline(host) => {
                    self.local_frames.get(k) != Some(&self.placed_site(host, k))
                }
                Placement::Occupied(host) => self.frames.get(k) != Some(&self.placed_site(host, k)),
                Placement::Box { .. } => false,
            };
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
    pub fn recompute(
        &mut self,
        e: &Elements,
        root: Option<Key>,
        w: f32,
        h: f32,
        out: &mut paint::Output,
    ) {
        #[cfg(test)]
        {
            self.trace.clear();
            self.paragraphs.frame += 1;
            out.reset_work();
        }
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
        let swap = e.attrs[root].rotation.swaps_axes();
        let (local_w, local_h) = if swap { (h, w) } else { (w, h) };
        self.offer_width(
            e,
            root,
            WidthOffer {
                axis: Constraint::Available(local_w),
                break_before: false,
                projected_height: None,
            },
        );
        self.offer_placement(e, root, [0.; 2], Constraint::Available(local_h), false);
        if vp != self.viewport {
            if local_h != self.root_height(e, root) {
                if Self::reads_early_height(e, root) {
                    self.enqueue(Measure, root, e);
                }
                if e.slots[root].stats.own.height_reader
                    && e.attrs[root].sizing.height.weight().is_some()
                {
                    self.enqueue(Context, root, e);
                    if e.attrs[root].sizing.height.reads_fill_content() {
                        self.enqueue(Measure, root, e);
                    }
                }
            }
            self.viewport = vp;
            self.outer_window_changed(e, root);
            self.enqueue(PaintContext, root, e);
        }
        // Only layout depends on the host having finished Place. Painting
        // and composition still run once in ordinary ownership-depth order.
        loop {
            for &pass in &PASSES[..PaintContext as usize] {
                self.drain(e, pass, out);
            }
            let Some(rank) = self.deferred.as_ref().and_then(WorkSet::first_depth) else {
                break;
            };
            self.rank = rank as u16;
            self.begin_measure_inputs(e, Some(root));
            while let Some((k, mask)) = self.deferred.as_mut().and_then(|set| set.pop_at(rank)) {
                for &p in &PASSES[..PaintContext as usize] {
                    if mask & (1 << p as usize) != 0 {
                        self.enqueue(p, k, e);
                    }
                }
            }
        }
        for pass in [PaintContext, Compose, Project] {
            self.drain(e, pass, out);
        }
        self.rank = 0;
    }
    pub(super) fn drain(&mut self, e: &Elements, pass: Pass, out: &mut paint::Output) {
        #[cfg(test)]
        assert!(
            self.jobs[..pass as usize].iter().all(WorkSet::is_empty),
            "work sent back before {pass:?}"
        );
        if pass == Flow {
            self.constrained.finish_measure();
        }
        #[cfg(test)]
        {
            self.draining = Some(pass);
        }
        while let Some((k, requested)) = self.jobs[pass as usize].pop(pass.direction()) {
            if !e.slots.contains_key(k) {
                continue;
            }
            #[cfg(test)]
            {
                assert!((pass as usize) >= PaintContext as usize || self.rank == e.depths(k)[1]);
                self.trace.push((pass, e.slots[k].id, requested));
            }
            match pass {
                Context => self.context(e, k),
                Measure => self.measure(e, k),
                Flow => self.flow(e, k),
                Reflow => self.reflow(e, k),
                Place => {
                    let base = e.slots[k]
                        .parent
                        .map_or(self.viewport, |p| self.local_frames[p]);
                    self.place(e, k, [base.x, base.y]);
                }
                PaintContext => self.paint_context(e, k),
                Compose => {
                    if let Some(outputs) = paint::compose(self, e, k, requested)
                        && let Some(p) = e.slots[k].parent
                    {
                        if let Some(slot) = self.projected.get_mut(k) {
                            slot.retire(outputs, self.parts[k]);
                        }
                        if let Some(port) = scroll::order_port(e, p)
                            && let Some(i) = self.scroll.ordinal(k)
                            && let Some(w) = self.scroll.recording(port)
                            && w.key == Some(port)
                        {
                            out.retire_local(w, i, k, outputs);
                        }
                        self.jobs[Compose as usize].push(e.depth(p), p, outputs);
                    }
                    if !self.scroll.exhaustive() && self.paint[k].scrollport {
                        self.enqueue(Project, k, e);
                    }
                }
                Project => self.project(e, k, out),
            }
        }
        #[cfg(test)]
        {
            self.draining = None;
        }
    }
    #[cfg(test)]
    pub fn flow_width(&self, k: Key) -> f32 {
        self.width[k].outer
    }
    pub(super) fn reads_early_height(e: &Elements, k: Key) -> bool {
        let b = e.attrs[k].sizing;
        e.slots[k].shape.projects_height(&e.attrs[k])
            || (e.slots[k].shape == Shape::WrappedColumn
                && !matches!(b.width, Length::Px(_))
                && b.height.weight().is_some())
    }
    pub(super) fn invalidate_early_heights(&mut self, e: &Elements, p: Key) {
        for &c in e.children(p) {
            if e.slots[c].stats.children.height_reader
                && e.attrs[c].sizing.height.weight().is_some()
            {
                self.enqueue(Context, c, e);
            }
            if Self::reads_early_height(e, c) {
                self.enqueue(Measure, c, e);
            }
        }
    }
    pub(super) fn has_height_group(&self, k: Key) -> bool {
        self.height_shares.contains_key(k)
    }
    pub(super) fn has_group_reader(e: &Elements, p: Key) -> bool {
        (matches!(e.attrs[p].sizing.height, Length::Px(_)) || e.slots[p].stats.own.height_reader)
            && e.children(p)
                .iter()
                .any(|&c| e.reads_column_share(c, e.attrs[p].sizing))
    }
    fn root_height(&self, e: &Elements, k: Key) -> f32 {
        if e.attrs[k].rotation.swaps_axes() {
            self.viewport.width
        } else {
            self.viewport.height
        }
    }
    fn known_height_edge(&self, e: &Elements, k: Key) -> Option<Constraint> {
        let Some(p) = e.slots[k].parent else {
            return Some(Constraint::Available(self.root_height(e, k)));
        };
        if let Some(slot) = e.slots[k].slot {
            return slot
                .host_height()
                .then(|| Constraint::Available(self.local_frames[p].height));
        }
        // A span's declared height contributes demand, not an allocation for its Text.
        if self.context[p].inline_host.is_some() {
            return None;
        }
        let style = e.attrs[p].sizing;
        if e.slots[p].shape == Shape::TextColumn && !e.is_prose_float(k) {
            return Some(document::BLOCK_HEIGHT);
        }
        if let Some(px) = e.slots[p].shape.cross_height_px(style) {
            let s = e.slots[p].admitted.scale;
            Some(Constraint::Available(
                (pixels(px, s) - (pixels(style.padding[0], s) + pixels(style.padding[2], s)))
                    .max(0.),
            ))
        } else if e.slots[p].shape.passes_cross_height() && e.slots[k].stats.own.height_reader {
            self.context.get(p)?.inner_offer.map(Constraint::Available)
        } else {
            self.height_shares
                .get(p)?
                .assigned_height(e, k)
                .map(Constraint::Assigned)
        }
    }
    fn independent_inner(&self, e: &Elements, k: Key) -> Option<f32> {
        let m = &e.slots[k];
        let b = e.attrs[k].sizing;
        let s = m.admitted.scale;
        let outer = match b.height {
            Length::Px(px) => pixels(px, s),
            h if m.stats.own.height_reader && h.weight().is_some() => {
                let edge = self.known_height_edge(e, k)?;
                if !h.independent_fill() && !matches!(edge, Constraint::Assigned(_)) {
                    return None;
                }
                edge.resolve_height(e, k, 0.)
            }
            _ => return None,
        };
        Some((outer - (pixels(b.padding[0], s) + pixels(b.padding[2], s))).max(0.))
    }
    fn context(&mut self, e: &Elements, k: Key) {
        let shape = e.slots[k].shape;
        let inline_host = e.inline_host(k);
        let inner = if inline_host.is_none()
            && (shape == Shape::Column
                || (shape.passes_cross_height() && e.slots[k].stats.own.height_reader))
        {
            self.independent_inner(e, k)
        } else {
            None
        };
        if shape == Shape::Column {
            let shares = shapes::FillShares::declared_column(e, k, inner);
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
        let height_changed = old.and_then(|c| c.inner_offer) != inner;
        let site_changed = old.is_some_and(|c| c.inline_host != inline_host);
        let next = LayoutContext {
            inputs: ctx,
            inline_host,
            inner_offer: inner,
            text_align,
        };
        if changed(&mut self.context, k, next) {
            if site_changed {
                if e.slots[k].stats.own.height_reader {
                    self.enqueue(Measure, k, e);
                }
                self.enqueue(Place, k, e);
                self.enqueue(PaintContext, k, e);
            }
            if height_changed
                && shape.passes_cross_height()
                && e.slots[k].stats.children.height_reader
            {
                self.refresh_child_offers(e, k);
            }
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
                    if let Some(p) = e.layout_parent(k)
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
                    && let Some(p) = inline_host
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
                    if let Some(p) = e.layout_parent(k)
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
            for &c in e.owned(k) {
                if inputs_changed
                    || (site_changed && shape == Shape::El && e.slots[c].shape == Shape::Text)
                    || (old.is_none_or(|p| p.text_align != text_align)
                        && e.attrs[c].text_align.is_none())
                {
                    self.enqueue(Context, c, e);
                }
                if height_changed && e.slots[c].slot.is_none() && e.slots[c].stats.own.height_reader
                {
                    self.enqueue(Context, c, e);
                    if Self::reads_early_height(e, c) {
                        self.enqueue(Measure, c, e);
                    }
                }
            }
        }
    }
    fn child_demand(&self, e: &Elements, p: Key, c: Key, heights: HeightOffers) -> Demand {
        let mut m = self.measure[c];
        if e.is_row_charge(c)
            && let Some(fit) = self.constrained_fit(c)
        {
            m.w = fit.size.w;
        }
        let a = &e.attrs[c];
        let host = e.slots[p].shape;
        let local = self.local_measure[c];
        let content_column = host == Shape::Column
            && matches!(
                e.attrs[p].sizing.width,
                Length::Content | Length::BoundedContent(_)
            );
        if content_column && a.rotation.mixes_axes() && a.sizing.height.weight().is_some() {
            // A Column does not grow its content width to a turned child's
            // empty allocation. The occupied slot still advances and clips normally.
            m.w = a.rotation.extent(local.w, local.h).0;
            return m;
        }
        if let Some(h) = heights.for_child(self, e, c) {
            m.w = a.rotation.extent(self.projected_local_width(e, c, h), h).0;
        }
        if content_column && e.slots[c].shape == Shape::Image && a.sizing.width_from_height() {
            m.w = m.w.max(local.min_w);
        }
        m
    }
    fn measure(&mut self, e: &Elements, k: Key) {
        let c = self.context[k].inputs;
        let b = e.attrs[k].sizing.without_weights();
        let shape = e.slots[k].shape;
        let cross = (shape.passes_cross_height()
            && self.context[k].inline_host.is_none()
            && e.slots[k].stats.children.height_reader
            && self.context[k].inner_offer.is_none())
        .then(|| {
            let pad = pixels(b.padding[0], c.scale) + pixels(b.padding[2], c.scale);
            let one = self.measure_cross_slots(e, k);
            let natural = b.height.resolve(one + pad, || c.scale);
            let outer = self
                .known_height_edge(e, k)
                .map_or(natural, |edge| edge.resolve_height(e, k, natural));
            (outer - pad).max(0.)
        });
        let old_cross = match cross {
            Some(h) => self.natural_cross.insert(k, h),
            None => self.natural_cross.remove(k),
        };
        if old_cross != cross {
            self.refresh_child_offers(e, k);
        }
        if e.slots[k].stats.children.late_nonfill && shape.allots_width() {
            for &c in e.children(k) {
                if e.is_row_charge(c) {
                    let old = self.constrained.fits.get(c).map(|f| f.size.w);
                    let w = self
                        .intrinsic_fit(e, c, Constraint::Assigned(self.local_measure[c].w))
                        .size
                        .w;
                    if old != Some(w) {
                        self.enqueue(Flow, k, e);
                    }
                }
            }
        }
        if shape == Shape::Column {
            self.measure_column_slots(e, k);
        }
        let memo = (!shape.uses_glyphs()
            && shape != Shape::Image
            && cross.is_none()
            && !e.slots[k].stats.children.height_reader
            && !e.slots[k].stats.children.late_nonfill)
            .then(|| {
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
            let heights = self.child_heights(e, k, None);
            let content = shape.measure(
                e,
                k,
                &mut Measuring {
                    layers: self,
                    e,
                    k,
                    heights,
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
        if shape == Shape::WrappedColumn
            && (shape.measures_packed_width(b.width) || e.slots[k].stats.children.height_reader)
        {
            if b.height.limits_column_height() || e.slots[k].stats.children.height_reader {
                let inner = self.single_inner(e, k, m.w);
                let known_inner =
                    matches!(b.width, Length::Px(_)) || Self::uses_single_column(e, k);
                for &child in e.children(k) {
                    if e.slots[child].stats.own.reflows_height
                        && !e.fit_reads_height_offer(child)
                        && (known_inner || !self.reads_width_offer(e, child))
                    {
                        self.intrinsic_fit(e, child, Constraint::Available(inner));
                    }
                }
            }
            if e.slots[k].stats.children.height_reader {
                self.refresh_child_offers(e, k);
            }
        }
        let mut m = if shape.measures_packed_width(b.width) {
            self.packed_demand(e, k, m, self.known_height_edge(e, k))
        } else {
            m
        };
        let edge = if shape.projects_height(&e.attrs[k]) {
            if e.is_prose_float(k) {
                Some(document::float_edge(e, k, m))
            } else {
                self.known_height_edge(e, k)
            }
        } else {
            None
        };
        if shape == Shape::Image
            && b.width_from_height()
            && let Some(edge) = edge
            && let Some(w) = shapes::image_width_at_height(e, k, edge.resolve_height(e, k, m.h))
        {
            m.w = w;
        }
        // A height-filled linear wrapper keeps its intrinsic minimum when a
        // projected child shrinks. El instead follows the child's final width.
        if matches!(shape, Shape::Row | Shape::Column)
            && e.slots[k].stats.own.height_reader
            && b.height.weight().is_some()
        {
            m.w = m.w.max(m.min_w);
        }
        let floor = e.attrs[k]
            .line_floor
            .filter(|_| e.parent_shape(k) == Some(Shape::WrappedRow));
        m.min_w = wrap_minimum(floor, m.w, m.min_w, || c.scale);
        let rotation = e.attrs[k].rotation;
        let old_local = self.local_measure.insert(k, m);
        if old_local.is_none_or(|s| s.w != m.w) {
            self.enqueue(Flow, k, e);
        }
        if old_local.is_none_or(|s| s.h != m.h) {
            self.enqueue(Reflow, k, e);
            // An unsplit Column offers this fill its local natural height,
            // even if a quarter turn hides that change on the occupied axis.
            if rotation.mixes_axes()
                && b.height.weight().is_some()
                && let Some(p) = e.layout_parent(k)
                && e.slots[p].shape == Shape::Column
                && !e.attrs[p].sizing.height.definite()
            {
                self.enqueue(Place, p, e);
            }
        }
        if rotation.mixes_axes() {
            if b.width.weight().is_some()
                && old_local.is_none_or(|s| s.w != m.w || s.min_w != m.min_w)
                && e.parent_shape(k).is_some_and(Shape::allots_width)
            {
                self.parent(Flow, k, e);
            }
            let h = if b.height.weight().is_some() {
                edge.map_or(m.h, |edge| edge.resolve_height(e, k, m.h))
            } else {
                m.h
            };
            let (w, h) = rotation.extent(m.w, h);
            m = Demand {
                w,
                h,
                min_w: rotation.extent(m.min_w, m.h).0,
            };
        }
        let old = self.measure.get(k).copied();
        if changed(&mut self.measure, k, m) {
            self.parent(Measure, k, e);
            if old.is_none_or(|s| s.w != m.w) {
                self.enqueue(Flow, k, e);
                if let Some(p) = e.layout_parent(k)
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
                && let Some(p) = e.layout_parent(k)
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
    pub fn text_changed(&mut self, k: Key, e: &Elements) {
        if let Some(r) = self.runs.get_mut(k) {
            r.invalid = true
        }
        self.enqueue(Measure, k, e);
    }
    fn width_for(&self, e: &Elements, k: Key, edge: Constraint, height: Option<f32>) -> Width {
        let b = e.attrs[k].sizing;
        let scale = self.context[k].inputs.scale;
        let outer = if matches!(b.width, Length::Px(_)) {
            // A pixel width needs no incoming offer, including before its Row's split.
            self.local_measure[k].w
        } else {
            self.resolved_width(
                e,
                k,
                edge,
                height.map_or_else(
                    || self.local_measure[k].w,
                    |h| self.projected_local_width(e, k, h),
                ),
            )
        };
        let [_, r, _, l] = b.padding.map(|v| pixels(v, scale));
        let inner = if Self::uses_single_column(e, k) {
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
        Width {
            outer,
            inner,
            new_edge: false,
            child_height: height.filter(|_| {
                e.slots[k].stats.children.height_reader
                    && (e.slots[k].shape.passes_cross_height() || e.slots[k].shape.allots_height())
            }),
        }
    }
    fn flow(&mut self, e: &Elements, k: Key) {
        if let Some(slot) = e.slots[k].slot {
            let p = e.slots[k].parent.expect("mounted root");
            let h = self.known_height_edge(e, k).filter(|_| {
                e.slots[k].stats.own.height_reader && e.attrs[k].sizing.height.weight().is_some()
            });
            self.offers.insert(
                k,
                WidthOffer {
                    axis: if slot.host_width() {
                        Constraint::Available(self.local_frames[p].width)
                    } else {
                        Constraint::Natural
                    },
                    break_before: false,
                    projected_height: h
                        .map(|edge| edge.resolve_height(e, k, self.local_measure[k].h)),
                },
            );
        }
        let edge = self.offers[k];
        let width = self.width_for(e, k, edge.axis, edge.projected_height);
        let Width { outer, inner, .. } = width;
        // The shared fragments belong to the width that produced them. A
        // different ordinary solve retires, rather than overwrites, that proof.
        if !self.constrained.accepts(k, width) {
            self.constrained.discard(k);
        }
        let old = self.width.insert(k, width);
        if old.is_none_or(|w| w.outer != outer || w.inner != inner) {
            self.enqueue(Reflow, k, e);
        }
        let offer_children = old
            .is_none_or(|w| w.inner != inner || w.new_edge || w.child_height != width.child_height);
        self.child_widths(e, k, inner, offer_children, Basis::Offered);
    }
    fn in_document_flow(&self, e: &Elements, k: Key) -> bool {
        e.slots[k].shape == Shape::Paragraph
            && e.parent_shape(k) == Some(Shape::TextColumn)
            && !e.attrs[k].rotation.mixes_axes()
            && e.float_side(k).is_none()
    }
    fn box_fit(&mut self, e: &Elements, k: Key, width: Width, basis: Basis) -> Fit {
        let b = e.attrs[k].sizing;
        let scale = self.context[k].inputs.scale;
        let mut cx = ReflowCx {
            basis,
            gap: b.spacing.map(|v| pixels(v, scale)),
            padding: b.padding.map(|v| pixels(v, scale)),
            height: b.height,
            scale,
            fixed_basis: !b.height.fits_content_height(),
            measured_height: self.local_measure[k].h,
            row_offer: 0.,
            layers: self,
        };
        let heights = e.slots[k].shape.reflow(e, k, &mut cx);
        Fit::from_heights(e, k, width.outer, heights, cx.row_offer)
    }
    fn reflow(&mut self, e: &Elements, k: Key) {
        let n = &e.slots[k];
        let b = e.attrs[k].sizing;
        let width = self.width[k];
        let known = self.use_fit(k, width);
        if self.in_document_flow(e, k)
            && n.parent
                .is_none_or(|p| self.use_fit(p, self.width[p]).is_none())
        {
            self.constrained.discard(k);
            self.paragraphs.invalidate_lines(k);
            self.parent(Reflow, k, e);
            return;
        }
        let next = known.unwrap_or_else(|| self.box_fit(e, k, width, Basis::Offered));
        let Fit {
            size: fit, local, ..
        } = next;
        let h = fit.h;
        let previous = self.fit.insert(k, next);
        let old = previous.map(|f| f.size);
        if n.shape.allots_height()
            || matches!(
                n.shape,
                Shape::WrappedRow | Shape::Paragraph | Shape::TextColumn
            )
            || old != Some(fit)
            || previous.is_none_or(|p| p.local != local)
        {
            self.enqueue(Place, k, e);
        }
        if e.attrs[k].rotation.mixes_axes()
            && b.height.reads_fill_content()
            && previous.is_none_or(|f| f.local.h != local.h)
            && e.parent_shape(k).is_some_and(Shape::allots_height)
        {
            // Vertical sharing reads local content, not the rotated slot.
            self.parent(Place, k, e);
        }
        if n.shape.packs_width(b.width)
            || (e.attrs[k].rotation.mixes_axes() && previous.is_none_or(|f| f.local != local))
        {
            self.packed_width_reader(e, k);
        }
        if old != Some(fit)
            && let Some(p) = e.layout_parent(k)
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
        #[cfg(test)]
        if e.slots[k].shape == Shape::Paragraph {
            self.paragraphs.assert_final_width(k, self.width[k].inner);
        }
        let row_offer = self.fit[k].row_offer;
        let scale = self.context[k].inputs.scale;
        let rotation = e.attrs[k].rotation;
        let placed = |size: Size, [x, y]: [f32; 2]| {
            let local = Rect {
                x: coordinate(origin[0] + x),
                y: coordinate(origin[1] + y),
                width: size.w,
                height: size.h,
            };
            (rotation.slot(local), local)
        };
        let role = e.slots[k].slot;
        let (slot, mut local) = if let Some(slot) = role {
            let host = self.local_frames[e.slots[k].parent.expect("mounted root")];
            let height = if slot.host_height() {
                Constraint::Available(host.height)
            } else {
                Constraint::Natural
            };
            let size = Fit::local_at(&self.fit, e, k, height);
            placed(size, slot.offset(host, size.turned(rotation), &e.attrs[k]))
        } else {
            match self.placement[k] {
                Placement::Inline(host) => {
                    let local = self.placed_site(host, k);
                    (rotation.about_center(local), local)
                }
                Placement::Occupied(host) => {
                    let slot = self.placed_site(host, k);
                    let local = if rotation.mixes_axes() {
                        let edge = if e.is_prose_float(k) {
                            document::float_edge(e, k, self.local_measure[k])
                        } else {
                            document::BLOCK_HEIGHT
                        };
                        let size = Fit::local_at(&self.fit, e, k, edge);
                        Rect {
                            width: size.w,
                            height: size.h,
                            ..slot
                        }
                    } else {
                        slot
                    };
                    (slot, local)
                }
                Placement::Box { offset, height } => {
                    placed(Fit::local_at(&self.fit, e, k, height), offset)
                }
            }
        };
        if rotation.mixes_axes()
            && (role.is_some() || !matches!(self.placement[k], Placement::Inline(_)))
        {
            local.x = coordinate(local.x + (slot.width - local.width) * 0.5);
            local.y = coordinate(local.y + (slot.height - local.height) * 0.5);
        }
        let old = self.local_frames.insert(k, local);
        let origin_changed = old.is_none_or(|f| [f.x, f.y] != [local.x, local.y]);
        let Rect { width, height, .. } = local;
        if old != Some(local) {
            let dw = old.is_none_or(|f| f.width != width);
            let dh = old.is_none_or(|f| f.height != height);
            for (slot, c) in e.nearby(k) {
                if dw && slot.host_width() && self.reads_width_offer(e, c) {
                    self.enqueue(Flow, c, e);
                }
                if dh && slot.host_height() && e.slots[c].stats.reads_anchor_height {
                    for p in [Context, Measure, Flow] {
                        self.enqueue(p, c, e);
                    }
                }
                if origin_changed || slot.reads_placement(dw, dh, &e.attrs[c]) {
                    self.enqueue(Place, c, e);
                }
            }
        }
        if changed(&mut self.frames, k, slot) || old != Some(local) {
            if e.attrs[k].scroll.port() {
                self.scroll.resize(k);
            }
            if let Some(p) = e.layout_parent(k).filter(|&p| e.attrs[p].scroll.port()) {
                self.scroll.resize(p);
                self.enqueue(PaintContext, p, e);
            }
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
        let content = parent.and_then(|p| {
            if self.scroll.exhaustive() {
                None
            } else if e.slots[k].slot.is_none() && e.attrs[p].scroll.port() {
                Some(p)
            } else {
                self.paint[p].content
            }
        });
        let previous = self.scroll.view(k);
        let view = self
            .scroll
            .settle(e, k, self.local_frames[k], &self.frames, scale);
        if e.slots[k].stats.tab_stops != 0
            && (view.range != previous.range
                || view.emit != previous.emit
                || view.axes.map(|p| p.epoch) != previous.axes.map(|p| p.epoch))
        {
            self.focus_ports_changed = true;
        }
        if view != previous {
            let draw = view.range != previous.range || view.offset() != previous.offset();
            self.invalidate_composition(
                k,
                e.depth(k),
                if draw { Outputs::Both } else { Outputs::Hit },
            );
        }
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
        let ctx = self.context[k];
        let escaping = e.slots[k].slot.is_some_and(Slot::escapes);
        let next = paint::PaintContext {
            content,
            color,
            gradient,
            rules,
            scale,
            content_scroll: view.offset(),
            scrollport: a.scroll.port(),
            bars: view.range != [0.; 2],
            transformed: a.posed() || inherited.is_some_and(|p| p.transformed),
            zoom: inherited.map_or(1., |p| p.zoom) * a.pose.zoom.abs(),
            text_align: ctx.text_align,
            ..Default::default()
        }
        .geometry(
            e,
            k,
            self.local_frames[k],
            self.viewport,
            inherited,
            content.is_none(),
        );
        let (f, clip, outer, offset) = (next.frame, next.clip, next.boundary, next.offset);
        let old = self.paint.get(k).copied();
        if old == Some(next) {
            return;
        }
        self.paint.insert(k, next);
        if old.is_some_and(|p| p.frame != f || p.content != next.content) {
            self.outer_window_changed(e, k);
        }
        if self.scroll.recorded(k).is_some()
            && old.is_some_and(|p| {
                PaintContext {
                    content_scroll: next.content_scroll,
                    ..p
                } == next
            })
        {
            self.enqueue(Project, k, e);
            return;
        }
        if e.slots[k].stats.tab_stops != 0
            && old.is_none_or(|p| p.frame != f || p.zoom != next.zoom)
        {
            self.navigation_changed = true;
        }
        let content_edge = old.and_then(|p| p.content) != content;
        if content_edge {
            // Initialize storage when a key enters scroll content, not up to a
            // distant ordinal on its first reveal. None is not a projection.
            if content.is_some() {
                self.jobs[Project as usize].prepare_slot(k);
                self.projected.insert(k, Default::default());
                #[cfg(test)]
                {
                    self.scroll.counts.content_slots += 1;
                }
            } else {
                self.projected.remove(k);
            }
        }
        if content_edge
            || (content.is_some() && old.is_none_or(|p| p.frame != f || p.zoom != next.zoom))
        {
            self.invalidate_composition(k, e.depth(k), Outputs::Both);
        }
        if let Some(p) = ctx.inline_host {
            let hit =
                a.actions.any() && old.is_none_or(|ctx| ctx.scale != next.scale || ctx.frame != f);
            self.invalidate_composition(
                p,
                e.depth(p),
                if hit { Outputs::Both } else { Outputs::Draw },
            );
        }
        let child_clip_changed = old.is_none_or(|p| p.child_clip != next.child_clip);
        let geometry_changed = old
            .is_none_or(|p| p.frame != f || p.clip != clip || p.boundary != outer)
            || (a.posed() && child_clip_changed);
        let color_changed = old.is_none_or(|p| p.color != color || p.gradient != gradient);
        let space_changed =
            old.is_none_or(|p| p.transformed != next.transformed || p.zoom != next.zoom);
        let inputs_changed =
            space_changed || old.is_none_or(|p| p.rules != rules || p.offset != offset);
        let port_changed =
            old.is_none_or(|p| p.scrollport != next.scrollport || p.bars != next.bars);
        let port_changed = (next.scrollport || old.is_some_and(|p| p.scrollport))
            && (port_changed
                || geometry_changed
                || child_clip_changed
                || old.is_none_or(|p| p.scale != next.scale || p.escape_clip != next.escape_clip));
        let guarded = a.clip_nearby && e.slots[k].stats.escapes > 0;
        let draw = port_changed
            || guarded
            || a.posed()
            || (space_changed && (a.background.is_some() || a.has_art()))
            || shape == Shape::Paragraph
            || a.decor.active()
            || shape.reads_paint_context(a, old, next);
        let hit = port_changed
            || (guarded
                || escaping
                || a.posed()
                || a.actions.any()
                || shape == Shape::Paragraph
                || a.decor.radius != [0.; 4])
                && (geometry_changed || old.is_none_or(|p| p.scale != next.scale));
        if let Some(outputs) = Outputs::changed(draw, hit) {
            self.invalidate_composition(k, e.depth(k), outputs);
        }
        let outer_clip_changed = old.is_none_or(|p| p.clip != clip);
        let frame_changed = old.is_none_or(|p| p.frame != f);
        let escape_clip_changed = old.is_none_or(|p| p.escape_clip != next.escape_clip);
        let content_changed = old.is_none_or(|p| {
            (self.scroll.exhaustive() && p.content_scroll != next.content_scroll)
                || p.scrollport != next.scrollport
        });
        if child_clip_changed
            || escape_clip_changed
            || color_changed
            || inputs_changed
            || outer_clip_changed
            || frame_changed
            || content_edge
        {
            for &child in e.owned(k) {
                if content_edge
                    || (content_changed && e.slots[child].slot.is_none())
                    || escape_clip_changed
                    || child_clip_changed
                    || inputs_changed
                    || (color_changed && e.attrs[child].color.is_none())
                    || (outer_clip_changed && e.slots[child].shape == Shape::Image)
                    || (frame_changed && e.attrs[child].posed())
                {
                    self.enqueue(PaintContext, child, e);
                }
            }
        } else if content_changed {
            // A position-only update need not even inspect host-owned mounts.
            for &child in e.children(k) {
                self.enqueue(PaintContext, child, e);
            }
        }
    }
}
