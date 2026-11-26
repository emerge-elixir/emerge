//! A non-fill charge can ask for an intrinsic height **at a known local
//! width**. This is a Measure product, not an early Flow or a Reflow job.
//! Width-dependent measures use the same length, group and line algorithms
//! as ordinary offers. Flow publishes their edges; Reflow consumes their
//! heights and fragments. Neither pass sends work to an earlier layer.
use super::*;

/// Heights not already supplied by declaration-only Context. A measured
/// share is assigned; a row's cross extent is merely available space.
#[derive(Clone, Copy, Default)]
pub(super) enum HeightOffers {
    #[default]
    Independent,
    Available(f32),
    Group(shapes::FillShares),
    Declared(shapes::FillShares),
    Prose,
}
impl HeightOffers {
    pub(super) fn for_child(self, l: &Layers, e: &Elements, c: Key) -> Option<f32> {
        let a = &e.attrs[c];
        if !e.slots[c].stats.own.height_reader || a.sizing.height.weight().is_none() {
            return None;
        }
        let basis = l.local_measure[c];
        match self {
            Self::Independent => None,
            Self::Available(h) => {
                let local_h = if a.sizing.height.reads_fill_content() && e.is_row_charge(c) {
                    l.constrained_fit(c).map_or(basis.h, |f| f.local.h)
                } else {
                    basis.h
                };
                Some(Constraint::Available(h).resolve_height(e, c, local_h))
            }
            Self::Declared(s) => s.assigned_height(e, c),
            Self::Group(shares) => shares.height(e, c, l.slot_basis(e, c)),
            Self::Prose if e.is_prose_float(c) => {
                Some(document::float_edge(e, c, basis).resolve_height(e, c, basis.h))
            }
            Self::Prose => None,
        }
    }
}
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Basis {
    Independent,
    Offered,
}
impl Basis {
    pub(super) fn fits(self, l: &Layers) -> &SecondaryMap<Key, Fit> {
        match self {
            Self::Independent => &l.constrained.fits,
            Self::Offered => &l.fit,
        }
    }
    pub(super) fn width(self, l: &Layers, k: Key) -> Width {
        match self {
            Self::Independent => l.constrained.inputs[k].width,
            Self::Offered => l.width[k],
        }
    }
    pub(super) fn break_before(self, l: &Layers, c: Key) -> bool {
        match self {
            Self::Independent => l.constrained.inputs[c].edge.break_before,
            Self::Offered => l.offers[c].break_before,
        }
    }
    pub(super) fn record(self, l: &mut Layers, k: Key, fit: Fit) {
        match self {
            Self::Independent => l.constrained.fits.insert(k, fit),
            Self::Offered => l.fit.insert(k, fit),
        };
    }
}
#[derive(Clone, Copy)]
struct Input {
    width: Width,
    edge: WidthOffer,
    valid: bool,
    // Fixed-height branches can contribute a number without visiting their descendants.
    child_edges: bool,
    // A fixed-height prose box can supply a scalar slot without laying out
    // its children. Ordinary Reflow must still produce its sites and ink.
    defer_prose: bool,
}
#[derive(Default)]
pub(super) struct Constrained {
    inputs: SecondaryMap<Key, Input>,
    pub(super) fits: SecondaryMap<Key, Fit>,
    marked: SecondaryMap<Key, u64>,
    epoch: u64,
    collecting: bool,
}
impl Constrained {
    pub(super) fn reserve(&mut self, n: usize) -> Result<(), String> {
        self.inputs
            .try_set_capacity(n)
            .map_err(pool::capacity_error)?;
        self.fits
            .try_set_capacity(n)
            .map_err(pool::capacity_error)?;
        self.marked
            .try_set_capacity(n)
            .map_err(pool::capacity_error)
    }
    pub(super) fn evict(&mut self, k: Key) {
        self.inputs.remove(k);
        self.fits.remove(k);
        self.marked.remove(k);
    }
    pub(super) fn begin(&mut self, active: bool) {
        self.epoch += 1;
        self.collecting = active || !self.inputs.is_empty();
    }
    pub(super) fn discard(&mut self, k: Key) {
        if let Some(input) = self.inputs.get_mut(k) {
            input.valid = false;
        }
    }
    pub(super) fn finish_measure(&mut self) {
        self.collecting = false;
    }
    pub(super) fn accepts(&self, k: Key, width: Width) -> bool {
        self.inputs.get(k).is_some_and(|i| {
            i.valid
                && i.width.outer == width.outer
                && i.width.inner == width.inner
                && i.width.child_height == width.child_height
        })
    }
}
impl Layers {
    pub(in crate::tree) fn refresh_child_offers(&mut self, e: &Elements, k: Key) {
        if let Some(w) = self.width.get_mut(k) {
            w.new_edge = true;
        }
        self.enqueue(Flow, k, e);
    }
    pub(super) fn child_heights(&self, e: &Elements, k: Key, outer: Option<f32>) -> HeightOffers {
        let m = &e.slots[k];
        if !m.stats.children.height_reader || self.context[k].inline_host.is_some() {
            return HeightOffers::Independent;
        }
        let b = e.attrs[k].sizing;
        let s = m.admitted.scale;
        let inner =
            outer.map(|h| (h - (pixels(b.padding[0], s) + pixels(b.padding[2], s))).max(0.));
        if m.shape.passes_cross_height() {
            inner
                .or(self.context[k].inner_offer)
                .or_else(|| self.natural_cross.get(k).copied())
                .map_or(HeightOffers::Independent, HeightOffers::Available)
        } else if m.shape == Shape::Column {
            if let Some(h) = inner
                && Some(h) != self.context[k].inner_offer
                && b.height.definite()
            {
                if let Some(s) = shapes::FillShares::declared_column(e, k, Some(h)) {
                    return HeightOffers::Declared(s);
                }
                return shapes::FillShares::measured_column(e, k, h, |c| self.slot_basis(e, c))
                    .map_or(HeightOffers::Independent, HeightOffers::Group);
            }
            self.height_shares
                .get(k)
                .copied()
                .map(HeightOffers::Declared)
                .or_else(|| {
                    self.measured_shares
                        .get(k)
                        .copied()
                        .map(HeightOffers::Group)
                })
                .unwrap_or_default()
        } else if m.shape.has_prose_sites() {
            HeightOffers::Prose
        } else {
            HeightOffers::Independent
        }
    }
    // Evaluate a subtree's intrinsic width function at a proved height. The
    // only recursive edges are filling consumers. This reads scalar Measure
    // products, never builds fragments, chooses new row breaks or fits a tree.
    pub(super) fn projected_local_width(&self, e: &Elements, k: Key, h: f32) -> f32 {
        let m = &e.slots[k];
        let a = &e.attrs[k];
        let b = a.sizing;
        let local = self.local_measure[k];
        if m.shape == Shape::Image && b.width_from_height() {
            return shapes::image_width_at_height(e, k, h).unwrap_or(local.w);
        }
        if m.shape.measures_packed_width(b.width) {
            return self
                .packed_demand(
                    e,
                    k,
                    self.single_column(e, k),
                    Some(Constraint::Assigned(h)),
                )
                .w;
        }
        if !m.stats.children.height_reader
            || matches!(b.width, Length::Px(_))
            || !(m.shape.passes_cross_height() || m.shape == Shape::Column)
        {
            return local.w;
        }
        let s = m.admitted.scale;
        let inner_h = (h - (pixels(b.padding[0], s) + pixels(b.padding[2], s))).max(0.);
        if self.context[k]
            .inner_offer
            .or_else(|| self.natural_cross.get(k).copied())
            == Some(inner_h)
        {
            return local.w;
        }
        let hosts = self.child_heights(e, k, Some(h));
        let widths = e
            .children(k)
            .iter()
            .map(|&c| self.child_demand(e, k, c, hosts).w);
        let content = if m.shape.allots_width() {
            widths.sum::<f32>()
                + pixels(b.spacing[0], s) * e.children(k).len().saturating_sub(1) as f32
        } else {
            widths.fold(0., f32::max)
        };
        let w = b.width.resolve(
            content + (pixels(b.padding[1], s) + pixels(b.padding[3], s)),
            || s,
        );
        if matches!(m.shape, Shape::Row | Shape::Column) && b.height.weight().is_some() {
            w.max(local.min_w)
        } else {
            w
        }
    }
    fn unallotted_slot(&self, e: &Elements, c: Key) -> f32 {
        if e.attrs[c].sizing.height.weight().is_some() {
            self.local_measure[c].h
        } else {
            self.measure[c].h
        }
    }
    pub(super) fn slot_basis(&self, e: &Elements, c: Key) -> f32 {
        self.slot_heights
            .get(c)
            .copied()
            .unwrap_or_else(|| self.unallotted_slot(e, c))
    }
    // A cross-width consumer must not read a previous fit. Fixed-width hosts
    // supply the actual offer. In a content-width Column, a sibling whose
    // width reads that very Column contributes its *intrinsic* height to the
    // width calculation. Its actual height still allocates the placed group.
    fn slot_width(&self, e: &Elements, k: Key, c: Key) -> Option<Constraint> {
        let b = e.attrs[c].sizing;
        if !e.slots[c].stats.own.reflows_height || e.fit_reads_height_offer(c) {
            return None;
        }
        if e.slots[k].shape.allots_width()
            && e.attrs[c].sizing.width.weight().is_none()
            && !(e.slots[k].shape == Shape::WrappedRow
                && e.slots[c].shape.requests_fill_width(e, c))
        {
            return Some(Constraint::Assigned(self.local_measure[c].w));
        }
        if !self.reads_width_offer(e, c) {
            return Some(Constraint::Natural);
        }
        if !e.slots[k].shape.allots_width()
            && let Length::Px(n) = e.attrs[k].sizing.width
        {
            return Some(Constraint::Available(self.single_inner(
                e,
                k,
                pixels(n, e.slots[k].admitted.scale),
            )));
        }
        // Only the wrapper's height is requested. Its outside width may
        // change, but all contributing child heights are independent of it.
        (!e.slots[c].stats.own.width_sensitive_fit
            && b.height.weight().is_none()
            && !e.slots[c].shape.has_prose_sites())
        .then_some(Constraint::Natural)
    }
    pub(super) fn measure_cross_slots(&mut self, e: &Elements, k: Key) -> f32 {
        e.children(k)
            .iter()
            .map(|&c| {
                self.slot_width(e, k, c)
                    .map(|edge| self.intrinsic_fit(e, c, edge).size.h)
                    .unwrap_or(self.measure[c].h)
            })
            .fold(0., f32::max)
    }
    pub(super) fn measure_column_slots(&mut self, e: &Elements, k: Key) {
        let style = e.attrs[k].sizing;
        let needed = e.slots[k].stats.children.height_reader
            && !self.has_height_group(k)
            && Self::has_group_reader(e, k);
        if needed {
            for &c in e.children(k) {
                let h = if !e.slots[c].shape.independent_height_slot(&e.attrs[c])
                    && let Some(edge) = self.slot_width(e, k, c)
                {
                    let f = self.intrinsic_fit(e, c, edge);
                    if e.attrs[c].sizing.height.weight().is_some() {
                        f.local.h
                    } else {
                        f.size.h
                    }
                } else {
                    self.unallotted_slot(e, c)
                };
                self.slot_heights.insert(c, h);
            }
        }
        let inner = self.context[k].inner_offer.or_else(|| {
            if !needed || !style.height.reads_fill_content() {
                return None;
            }
            let edge = self.known_height_edge(e, k)?;
            let scale = e.slots[k].admitted.scale;
            let pad = pixels(style.padding[0], scale) + pixels(style.padding[2], scale);
            let gap = pixels(style.spacing[1], scale);
            let sum = e
                .children(k)
                .iter()
                .map(|&c| self.slot_basis(e, c))
                .reduce(|a, b| a + gap + b)
                .unwrap_or(0.);
            let basis = style.height.resolve(sum + pad, || scale);
            Some((edge.resolve_height(e, k, basis) - pad).max(0.))
        });
        let shares = inner
            .filter(|_| needed && style.height.definite())
            .and_then(|h| shapes::FillShares::measured_column(e, k, h, |c| self.slot_basis(e, c)));
        let old = if let Some(s) = shares {
            self.measured_shares.insert(k, s)
        } else {
            self.measured_shares.remove(k)
        };
        if shares != old {
            self.refresh_child_offers(e, k);
        }
    }
    pub(super) fn uses_single_column(e: &Elements, k: Key) -> bool {
        let w = e.attrs[k].sizing.width;
        e.slots[k].shape.packs_width(w)
            || (e.slots[k].shape == Shape::WrappedColumn
                && w.reads_fill_content()
                && e.slots[k].stats.children.width_sensitive_slot)
    }
    pub(super) fn single_inner(&self, e: &Elements, k: Key, w: f32) -> f32 {
        let b = e.attrs[k].sizing;
        let s = e.slots[k].admitted.scale;
        (w - (pixels(b.padding[1], s) + pixels(b.padding[3], s))).max(0.)
    }
    pub(super) fn single_column(&self, e: &Elements, k: Key) -> Demand {
        let b = e.attrs[k].sizing;
        let s = e.slots[k].admitted.scale;
        let pad = pixels(b.padding[1], s) + pixels(b.padding[3], s);
        let one = e
            .children(k)
            .iter()
            .map(|c| self.measure[*c].w)
            .fold(0., f32::max);
        Demand {
            w: b.width.resolve(one + pad, || s),
            ..self.local_measure[k]
        }
    }
    fn pack_slot_fit(&self, e: &Elements, c: Key, inner: f32) -> (Size, Size) {
        let offer = Constraint::Available(inner);
        let width = self.width_for(e, c, offer, None);
        if let Some(f) = self.use_fit(c, width) {
            return (f.local, f.size);
        }
        let h = self.local_measure[c].h;
        let local = Size {
            w: width.outer,
            h,
            min_h: h,
        };
        (local, local.turned(e.attrs[c].rotation))
    }
    pub(super) fn packed_demand(
        &self,
        e: &Elements,
        k: Key,
        single: Demand,
        edge: Option<Constraint>,
    ) -> Demand {
        let inner = self.single_inner(e, k, single.w);
        let fit = |c| self.pack_slot_fit(e, c, inner);
        shapes::column_demand(e, k, single, edge, fit, |c, edge| {
            let (local, size) = fit(c);
            let a = &e.attrs[c];
            let h = edge.resolve_height(e, c, local.h);
            let w = if matches!(edge, Constraint::Assigned(_)) && e.slots[c].stats.own.height_reader
            {
                self.resolved_width(
                    e,
                    c,
                    Constraint::Available(inner),
                    self.projected_local_width(e, c, h),
                )
            } else {
                local.w
            };
            [
                if a.rotation.mixes_axes() {
                    a.rotation.extent(w, h).0
                } else if e.slots[c].shape.packs_width(a.sizing.width)
                    && !matches!(edge, Constraint::Assigned(_))
                {
                    self.packed_demand(e, c, self.single_column(e, c), Some(edge))
                        .w
                } else {
                    w
                },
                self.measure[c].min_w.min(size.w),
            ]
        })
    }
    pub(super) fn invalidate_constraints(&mut self, e: &Elements, mut k: Key) {
        if !self.constrained.collecting {
            return;
        }
        // The first changed input on each path suffices, even if its visible
        // intrinsic extents compare equal (e.g. different word breaks).
        loop {
            if self.constrained.marked.insert(k, self.constrained.epoch)
                == Some(self.constrained.epoch)
            {
                break;
            }
            if let Some(i) = self.constrained.inputs.get_mut(k) {
                i.valid = false;
            }
            let Some(p) = e.slots[k].parent else { break };
            if e.measures_at_width(p, k) {
                self.invalidate(Measure, p, e.depth(p));
            }
            k = p;
        }
    }
    fn measure_at(&mut self, e: &Elements, k: Key, edge: WidthOffer) -> Fit {
        let width = self.width_for(e, k, edge.axis, edge.projected_height);
        if self.constrained.accepts(k, width) {
            self.constrained.inputs[k].edge = edge;
            return self.constrained.fits[k];
        }
        let shape = e.slots[k].shape;
        let in_document = self.in_document_flow(e, k);
        let b = e.attrs[k].sizing;
        let defer_prose =
            shape.has_prose_sites() && matches!(b.height, Length::Px(_)) && !in_document;
        let child_edges = in_document
            || (!defer_prose && (shape.has_prose_sites() || e.slots[k].stats.own.reflows_height))
            || shape.packs_width(b.width);
        self.constrained.inputs.insert(
            k,
            Input {
                width,
                edge,
                valid: true,
                child_edges,
                defer_prose,
            },
        );
        if child_edges {
            self.child_widths(e, k, width.inner, true, Basis::Independent);
        }
        let f = if !child_edges {
            Fit::from_heights(e, k, width.outer, self.local_measure[k].h.into(), 0.)
        } else if in_document {
            self.paragraphs.invalidate_lines(k);
            Fit::from_heights(e, k, width.outer, self.local_measure[k].h.into(), 0.) // document supplies the bands
        } else {
            self.box_fit(e, k, width, Basis::Independent)
        };
        self.constrained.fits.insert(k, f);
        f
    }
    pub(super) fn constrained_fit(&self, k: Key) -> Option<Fit> {
        self.constrained
            .inputs
            .get(k)
            .filter(|i| i.valid)
            .and_then(|_| self.constrained.fits.get(k))
            .copied()
    }
    pub(super) fn intrinsic_fit(&mut self, e: &Elements, k: Key, axis: Constraint) -> Fit {
        self.measure_at(
            e,
            k,
            WidthOffer {
                axis,
                break_before: false,
                projected_height: None,
            },
        )
    }
    pub(super) fn use_fit(&self, k: Key, width: Width) -> Option<Fit> {
        (self.constrained.accepts(k, width) && !self.constrained.inputs[k].defer_prose)
            .then(|| self.constrained.fits[k])
    }
    pub(super) fn child_widths(
        &mut self,
        e: &Elements,
        k: Key,
        inner: f32,
        offer_children: bool,
        basis: Basis,
    ) {
        if basis == Basis::Offered
            && self.constrained.accepts(k, self.width[k])
            && self.constrained.inputs[k].child_edges
        {
            for &c in e.children(k) {
                let edge = self.constrained.inputs[c].edge;
                self.offer_width(e, c, edge);
            }
            return;
        }
        let heights = self.child_heights(e, k, basis.width(self, k).child_height);
        e.slots[k].shape.offer_widths(
            e,
            k,
            &mut FlowCx {
                layers: self,
                e,
                host: k,
                heights,
                inner,
                offer_children,
                scale: e.slots[k].admitted.scale,
                basis,
            },
        );
    }
}
impl FlowCx<'_> {
    pub(in crate::tree) fn offer_pack(&mut self, k: Key) -> bool {
        let e = self.e;
        if !e.slots[k].stats.children.height_reader {
            return false;
        }
        if !self.offer_children {
            return true;
        }
        let edge = self
            .basis
            .width(self.layers, k)
            .child_height
            .map(Constraint::Assigned)
            .or_else(|| self.layers.known_height_edge(e, k));
        let Some(h) =
            shapes::column_height(e, k, edge, |c| self.layers.pack_slot_fit(e, c, self.inner))
        else {
            return false;
        };
        let mut groups = shapes::ColumnGroups::new(e, k, h);
        while let Some((line, shares)) =
            groups.next(|c| self.layers.pack_slot_fit(e, c, self.inner))
        {
            for &c in line {
                let height = if e.slots[c].stats.children.height_reader
                    && e.attrs[c].sizing.height.weight().is_some()
                {
                    let slot = self.layers.pack_slot_fit(e, c, self.inner);
                    match shapes::pack_edge(e, c, slot, shares) {
                        Constraint::Assigned(h) => Some(h),
                        _ => None,
                    }
                } else {
                    None
                };
                self.offer_at(
                    c,
                    WidthOffer {
                        axis: Constraint::Available(self.inner),
                        break_before: false,
                        projected_height: height,
                    },
                );
            }
        }
        true
    }
    pub(super) fn offer(&mut self, k: Key, axis: Constraint, break_before: bool) {
        self.offer_at(
            k,
            WidthOffer {
                axis,
                break_before,
                projected_height: self.heights.for_child(self.layers, self.e, k),
            },
        );
    }
    fn offer_at(&mut self, k: Key, edge: WidthOffer) {
        match self.basis {
            Basis::Offered => self.layers.offer_width(self.e, k, edge),
            Basis::Independent => {
                self.layers.measure_at(self.e, k, edge);
            }
        }
    }
}
impl Fit {
    pub(super) fn from_heights(e: &Elements, k: Key, w: f32, h: Heights, row_offer: f32) -> Self {
        let local = Size {
            w,
            h: h.fit,
            min_h: h.min,
        };
        Fit {
            size: local.turned(e.attrs[k].rotation),
            local,
            row_offer,
        }
    }
}
