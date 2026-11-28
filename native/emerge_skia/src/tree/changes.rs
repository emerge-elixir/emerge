//! One mutation interval may contain many accepted wire batches. Save the first
//! replaced input, not a history of edits. Only refresh seeds the pass worksets.
use super::*;
use paint::Outputs;

#[derive(Default, Debug)]
pub(super) struct Change {
    listed: Option<usize>,
    pub cold: bool,
    pub reattached: bool,
    pub membership: bool,
    pub old_parent: Option<(Option<Key>, Option<Slot>)>,
    pub image: Option<crate::raster_assets::Source>,
    pub image_draft: bool,
    pub layout_before: Option<rotation::Dependencies>,
    pub attrs: Option<(Attrs, Span<u8>)>,
    pub order: Option<OrderSpan<Key>>,
    pub before: Option<std::sync::Arc<declaration::Record>>,
    pub pose_before: Option<(std::sync::Arc<Attrs>, std::sync::Arc<declaration::Block>)>,
}
impl Mount {
    pub fn new(id: u64, shape: Shape, index: usize) -> Self {
        Self {
            id,
            shape,
            index,
            parent: None,
            slot: None,
            depths: Cell::new((u64::MAX, [0; 2])),
            stats: Subtree::default(),
            admitted: scale::Inputs::default(),
            children: OrderSpan::default(),
            bytes: Span::default(),
            detached: None,
            attachment: 0,
            ink_revision: 0,
            change: Change {
                cold: true,
                ..Change::default()
            },
        }
    }
}
impl Elements {
    pub fn touch(&mut self, k: Key) {
        if self.slots[k].change.listed.is_none() {
            self.slots[k].change.listed = Some(self.touched.len());
            self.touched.push(k);
        }
    }
    fn unlist(&mut self, k: Key) {
        if let Some(i) = self.slots[k].change.listed.take() {
            self.touched.swap_remove(i);
            if let Some(&moved) = self.touched.get(i) {
                self.slots[moved].change.listed = Some(i);
            }
        }
    }
    pub fn detach(&mut self, k: Key) {
        self.unlist(k);
        self.images.remove(k);
        self.slots[k].change.image = None;
        if self.slots[k].detached.is_none() {
            self.slots[k].detached = Some(self.detached.len());
            self.detached.push(k);
        }
    }
    pub fn uncache(&mut self, k: Key) {
        if let Some(i) = self.slots[k].detached.take() {
            self.detached.swap_remove(i);
            if let Some(&moved) = self.detached.get(i) {
                self.slots[moved].detached = Some(i);
            }
        }
    }
    pub fn replace_bytes(&mut self, k: Key) {
        let m = &mut self.slots[k];
        let span = std::mem::take(&mut m.bytes);
        if !m.change.cold && m.change.attrs.is_none() {
            m.change.attrs = Some((self.attrs[k].clone(), span));
        } else {
            self.bytes.release(span);
        }
    }
    pub fn replace_order(&mut self, k: Key) {
        let m = &mut self.slots[k];
        let span = std::mem::take(&mut m.children);
        if !m.change.cold && m.change.order.is_none() {
            m.change.order = Some(span);
        } else {
            self.orders.release(span);
        }
    }
    pub fn evict(&mut self, k: Key) {
        self.unlist(k);
        self.uncache(k);
        self.images.remove(k);
        let m = self.slots.remove(k).unwrap();
        self.ids.remove(&m.id);
        self.attrs.remove(k);
        self.bytes.release(m.bytes);
        self.orders.release(m.children);
        if let Some((_, s)) = m.change.attrs {
            self.bytes.release(s);
        }
        if let Some(s) = m.change.order {
            self.orders.release(s);
        }
        if self.attrs.version.is_none() {
            self.free_indices.push(m.index);
        }
    }
}
impl Layers {
    pub fn invalidate_changes(
        &mut self,
        e: &mut Elements,
        root: Option<Key>,
        out: &mut paint::Output,
    ) {
        self.begin_measure_inputs(e, root);
        #[cfg(test)]
        if !e.touched.is_empty() {
            self.invalidation_passes += 1;
        }
        while let Some(k) = e.touched.pop() {
            out.prepare_material(e, k);
            let depth = e.depth(k);
            let m = &mut e.slots[k];
            debug_assert!(m.detached.is_none());
            let change = std::mem::take(&mut m.change);
            let moved = change
                .old_parent
                .is_some_and(|(p, s)| p != m.parent || s.is_some() != m.slot.is_some());
            let role = m.slot;
            let slot_change = change
                .old_parent
                .and_then(|(_, s)| s.zip(role))
                .filter(|(a, b)| a != b);
            if change.image_draft {
                self.invalidate_composition(k, depth, Outputs::Draw);
            }
            if let Some((old, next)) = slot_change {
                // Front and behind share the same two-axis anchor.
                if !matches!(
                    (old, next),
                    (Slot::Front, Slot::Behind) | (Slot::Behind, Slot::Front)
                ) {
                    self.enqueue(Pass::Place, k, e);
                }
                let height =
                    old.host_height() != next.host_height() && e.slots[k].stats.reads_anchor_height;
                if height || (old.host_width() != next.host_width() && self.reads_width_offer(e, k))
                {
                    self.enqueue(Pass::Flow, k, e);
                }
                if height {
                    self.enqueue(Pass::Context, k, e);
                    self.enqueue(Pass::Measure, k, e);
                }
                if old.escapes() != next.escapes() {
                    self.enqueue(Pass::PaintContext, k, e);
                    self.invalidate_composition(k, depth, Outputs::Both);
                }
            }
            if moved
                && change
                    .old_parent
                    .is_some_and(|(_, s)| s.is_some() || role.is_some())
            {
                self.invalidate_composition(k, depth, Outputs::Both);
            }
            if (moved || change.reattached)
                && (Self::reads_early_height(e, k)
                    || (e.slots[k].stats.reads_anchor_height
                        && (role.is_some() || change.old_parent.is_some_and(|(_, s)| s.is_some())))
                    || e.attrs[k].line_floor.is_some()
                    || change
                        .before
                        .as_ref()
                        .is_some_and(|r| r.block.attrs.line_floor.is_some())
                    || change
                        .attrs
                        .as_ref()
                        .is_some_and(|(old, _)| old.line_floor.is_some()))
            {
                // The new edge may consume a different wrapped minimum even
                // if scale, preferred width and every child stayed the same.
                self.enqueue(Pass::Measure, k, e);
            }
            let shared = change
                .before
                .as_ref()
                .zip(e.attrs.version.as_ref().and_then(|v| v.get(k)));
            let old_layout = change
                .layout_before
                .or_else(|| shared.map(|(r, _)| r.stats.own));
            let new_layout = e.slots[k].stats.own;
            if old_layout.is_some_and(|old| old.late_nonfill != new_layout.late_nonfill)
                && let Some(p) = e.layout_parent(k)
                && e.slots[p].shape.allots_width()
            {
                // The source of the split charge changed, possibly behind an
                // equal intrinsic width and an attrs-only descendant edit.
                self.enqueue(Pass::Measure, p, e);
                self.enqueue(Pass::Flow, p, e);
            }
            if old_layout.is_some_and(|old| {
                old.reflows_height != new_layout.reflows_height
                    || old.fit_after_share != new_layout.fit_after_share
                    || old.width_sensitive_fit != new_layout.width_sensitive_fit
                    || old.width_sensitive_slot != new_layout.width_sensitive_slot
            }) && let Some(p) = e.layout_parent(k)
                && e.slots[p].stats.own.constrained_measure
            {
                self.enqueue(Pass::Measure, p, e);
                if e.slots[p].shape == Shape::WrappedColumn {
                    self.enqueue(Pass::Flow, p, e);
                }
            }
            if old_layout.is_some_and(|old| old.height_reader != new_layout.height_reader) {
                self.enqueue(Pass::Context, k, e);
                if role.is_some_and(Slot::host_height) {
                    self.enqueue(Pass::Measure, k, e);
                    self.enqueue(Pass::Flow, k, e);
                }
                if let Some(p) = e.layout_parent(k)
                    && e.slots[p].shape == Shape::Column
                {
                    self.enqueue(Pass::Context, p, e);
                }
                if matches!(e.slots[k].shape, Shape::Row | Shape::Column)
                    && e.attrs[k].sizing.height.weight().is_some()
                {
                    self.enqueue(Pass::Measure, k, e);
                }
            }
            let shared_order =
                shared.is_some_and(|(a, b)| !std::sync::Arc::ptr_eq(&a.children, &b.children));
            if e.slots[k].stats.tab_stops != 0
                && (change.cold
                    || change.reattached
                    || moved
                    || shared_order
                    || change.order.is_some()
                    || slot_change.is_some())
            {
                self.navigation_changed = true;
            }
            if let Some(p) = e.inline_host(k) {
                let old_children = change
                    .order
                    .map(|s| e.orders.get(s).children())
                    .or_else(|| {
                        shared
                            .filter(|_| shared_order)
                            .map(|(r, _)| r.children.edges().children())
                    });
                if change.cold
                    || change.reattached
                    || moved
                    || old_children.is_some_and(|old| old != e.children(k))
                {
                    self.enqueue(Pass::Reflow, p, e);
                }
            }
            if !change.cold && e.attrs[k].has_art() {
                let source = e.images.source(k);
                if e.slots[k].shape == Shape::Image
                    && (change.reattached
                        || change
                            .image
                            .as_ref()
                            .is_some_and(|s| s.dimensions() != source.dimensions()))
                {
                    self.enqueue(Pass::Measure, k, e);
                    self.enqueue(Pass::Reflow, k, e);
                    if e.attrs[k].sizing.width_from_height() {
                        self.enqueue(Pass::Place, k, e);
                        self.packed_width_reader(e, k);
                    }
                    if let Some(p) = e.layout_parent(k).filter(|p| self.has_height_group(*p)) {
                        self.enqueue(Pass::Context, p, e);
                    }
                }
                if change.reattached || change.image.as_ref().is_some_and(|s| s != source) {
                    self.invalidate_composition(k, depth, Outputs::Draw);
                }
            }
            if moved || change.reattached {
                self.outer_window_changed(e, k);
            }
            if change.cold {
                self.mount(k, e);
            } else if change.reattached {
                // A cached container's children may have been computed in a
                // different attachment. Check their current measures/offers;
                // a leaf's intrinsic inputs are already compared below.
                if !e.children(k).is_empty() {
                    self.reattached_container(k, e);
                }
                if let Some(part) = self.parts.get_mut(k) {
                    part.reattach();
                }
                for p in [
                    Pass::Context,
                    Pass::Flow,
                    Pass::Reflow,
                    Pass::Place,
                    Pass::PaintContext,
                ] {
                    self.enqueue(p, k, e);
                }
                self.invalidate_composition(k, depth, Outputs::Hit);
            } else if moved {
                // A paragraph leaving document flow no longer reads sibling floats.
                if e.slots[k].shape == Shape::Paragraph {
                    self.enqueue(Pass::Reflow, k, e);
                }
                // Ordinary fit is site-independent. The new constraints and origin
                // can still differ even when width and scale are unchanged.
                if let Some(part) = self.parts.get_mut(k) {
                    part.reattach();
                }
                for p in [Pass::Context, Pass::Flow, Pass::Place, Pass::PaintContext] {
                    self.enqueue(p, k, e);
                }
            }
            if let Some((old, source)) = &change.pose_before {
                self.invalidate_attrs(e, k, old, &source.raw);
            }
            if let Some((before, next)) = shared {
                if !std::sync::Arc::ptr_eq(&before.block, &next.block) {
                    let (a, b) = (&before.block, &next.block);
                    if a.attrs
                        .font_gradient
                        .differs(&a.raw, b.attrs.font_gradient, &b.raw)
                    {
                        e.slots[k].ink_revision += 1;
                    }
                    if change.pose_before.is_none() {
                        self.invalidate_attrs(e, k, &a.attrs, &a.raw);
                    }
                }
                if shared_order {
                    self.invalidate_order(e, k, before.children.edges(), change.membership);
                }
            }
            if let Some((before, span)) = change.attrs {
                if before.font_gradient.differs(
                    e.bytes.get(&span),
                    e.attrs[k].font_gradient,
                    e.raw(k),
                ) {
                    e.slots[k].ink_revision += 1;
                }
                self.invalidate_attrs(e, k, &before, e.bytes.get(&span));
                e.bytes.release(span);
            }
            if let Some(span) = change.order {
                self.invalidate_order(e, k, e.orders.get(span), change.membership);
                e.orders.release(span);
            }
        }
    }
    fn invalidate_attrs(&mut self, e: &Elements, k: Key, old: &Attrs, old_raw: &[u8]) {
        let next = &e.attrs[k];
        let depth = e.depth(k);
        let shape = e.slots[k].shape;
        self.navigation_changed |= old.actions.focusable() != next.actions.focusable()
            || (e.slots[k].stats.tab_stops != 0
                && (old.actions != next.actions
                    || old.autofocus != next.autofocus
                    || old.pose != next.pose
                    || old.scroll.port() != next.scroll.port()
                    || old.decor.border != next.decor.border));
        if old.scroll != next.scroll
            || (next.scroll.port() && old.sizing.padding != next.sizing.padding)
        {
            if old.scroll.enabled != next.scroll.enabled {
                self.scroll.forget_order(e, k);
                self.invalidate_composition(k, depth, Outputs::Both);
                self.enqueue(Pass::Place, k, e);
            }
            if !next.scroll.tracked() {
                self.scroll.remove(k);
            }
            if old.scroll.enabled != next.scroll.enabled
                || old.sizing.padding != next.sizing.padding
            {
                self.scroll.resize(k);
            }
            self.enqueue(Pass::PaintContext, k, e);
        }
        let mut invalidate = |pass| self.enqueue(pass, k, e);
        if old.family != next.family
            || old.weight != next.weight
            || old.italic != next.italic
            || old.font_size != next.font_size
            || old.text_align != next.text_align
            || old.layout_scale != next.layout_scale
        {
            invalidate(Pass::Context)
        }
        let (a, b) = (old.sizing.without_weights(), next.sizing.without_weights());
        let parent = e.parent_shape(k);
        let align_changed = old.align != next.align;
        if shape == Shape::Paragraph
            && (old.align[0] != next.align[0] || old.text_align != next.text_align)
        {
            invalidate(Pass::Reflow);
        }
        if e.slots[k].slot.is_some() && (align_changed || a.height != b.height) {
            invalidate(Pass::Place);
        }
        if align_changed && shape == Shape::El {
            invalidate(Pass::Place);
        }
        if old.rotation != next.rotation {
            // A quarter-turned root swaps incoming offers. A turn elsewhere
            // changes the parent-facing reservation, not descendant policies.
            for pass in [
                Pass::Context,
                Pass::Measure,
                Pass::Flow,
                Pass::Reflow,
                Pass::Place,
            ] {
                invalidate(pass);
            }
        }
        if old.image_axes != next.image_axes {
            invalidate(Pass::Measure);
            invalidate(Pass::Reflow);
        }
        if a != b || (parent == Some(Shape::WrappedRow) && old.line_floor != next.line_floor) {
            invalidate(Pass::Measure)
        }
        let gaps = shape
            .main_axis()
            .map(|i| (old.main_spacing(i), next.main_spacing(i)));
        let charge_changed = gaps.is_some_and(|(a, b)| a.charge() != b.charge());
        if gaps.is_some_and(|(a, b)| a != b) {
            invalidate(Pass::Place);
        }
        let width_gaps = FlowInput::HorizontalGaps {
            authored: a.spacing[0] != b.spacing[0],
            charge: charge_changed,
        };
        if a.width != b.width
            || a.padding[1] != b.padding[1]
            || a.padding[3] != b.padding[3]
            || shape.reads_flow(b.width, width_gaps)
        {
            invalidate(Pass::Flow)
        }
        if shape.offers_inner_height() {
            let vertical_padding = a.padding[0] != b.padding[0] || a.padding[2] != b.padding[2];
            if a.height != b.height || vertical_padding {
                invalidate(Pass::Reflow)
            }
            if vertical_padding || a.padding[3] != b.padding[3] {
                invalidate(Pass::Place)
            }
        } else if a.height != b.height || a.spacing != b.spacing || a.padding != b.padding {
            invalidate(Pass::Reflow)
        }
        let clip_changed =
            old.decor.radius != next.decor.radius || old.decor.border != next.decor.border;
        if clip_changed || old.pose != next.pose || old.clip_nearby != next.clip_nearby {
            self.outer_window_changed(e, k);
        }
        if clip_changed {
            for &c in e.children(k) {
                if e.attrs[c].posed() {
                    self.enqueue(Pass::PaintContext, c, e);
                }
            }
        }
        if clip_changed
            || old.clip_nearby != next.clip_nearby
            || old.pose != next.pose
            || (next.posed() && old.image_host_clip != next.image_host_clip)
            || old.color != next.color
            || old
                .font_gradient
                .differs(old_raw, next.font_gradient, e.raw(k))
            || old.underline != next.underline
            || old.strike != next.strike
            || old.offset != next.offset
        {
            self.enqueue(Pass::PaintContext, k, e)
        }
        let paint_changed = old.clip_nearby != next.clip_nearby
            || old.has_art() != next.has_art()
            || old.image_fit != next.image_fit
            || old.opacity != next.opacity
            || old.pose != next.pose
            || shape.paint_attrs_changed(old, next)
            || old
                .svg_gradient
                .differs(old_raw, next.svg_gradient, e.raw(k))
            || old.decor.differs(old_raw, next.decor, e.raw(k));
        let output = Outputs::changed(
            paint_changed,
            old.actions != next.actions
                || old.clip_nearby != next.clip_nearby
                || clip_changed
                || old.pose != next.pose
                || ((next.actions.any() || e.slots[k].slot.is_some_and(Slot::escapes))
                    && next.posed()
                    && old.image_host_clip != next.image_host_clip),
        );
        if let Some(outputs) = output {
            self.invalidate_composition(k, depth, outputs)
        }
        if shape.uses_glyphs() && &old_raw[old.text.clone()] != e.text(k).as_bytes() {
            self.text_changed(k, e)
        }
        if let Some(p) = e.inline_host(k) {
            let item_y_changed = parent == Some(Shape::Paragraph)
                && old.align[1].unwrap_or_default() != next.align[1].unwrap_or_default();
            if item_y_changed
                || a.padding != b.padding
                || old.rotation != next.rotation
                || (shape == Shape::Text && &old_raw[old.text.clone()] != e.text(k).as_bytes())
            {
                self.enqueue(Pass::Reflow, p, e);
            }
            if let Some(outputs) = output {
                self.invalidate_composition(p, e.depth(p), outputs);
            }
        }
        if old.align[0] != next.align[0]
            && parent.is_some_and(Shape::has_prose_sites)
            && Self::reads_early_height(e, k)
        {
            self.enqueue(Pass::Measure, k, e);
            self.enqueue(Pass::Place, k, e);
        }
        if old.align[0] != next.align[0] && parent == Some(Shape::Paragraph) {
            if old.float_side().is_some() != next.float_side().is_some() {
                self.enqueue(Pass::Context, k, e);
            }
            let p = e.slots[k].parent.unwrap();
            self.enqueue(Pass::Reflow, p, e);
            self.enqueue(Pass::Place, k, e);
        }
        if old.rotation != next.rotation
            && let Some(p) = e.layout_parent(k)
        {
            // Equal unallotted fits needn't give equal occupied slots (e.g.
            // a square intrinsic box with a cross-axis fill after a turn).
            let pass = if e.slots[p].shape.reads_prose_box(e, k) {
                Pass::Reflow
            } else {
                Pass::Place
            };
            self.enqueue(pass, p, e);
        }
        if old.align[0] != next.align[0] && parent == Some(Shape::TextColumn) {
            let p = e.slots[k].parent.unwrap();
            self.enqueue(Pass::Flow, p, e);
            self.enqueue(Pass::Reflow, p, e);
        }
        if align_changed
            && let Some(p) = e.layout_parent(k)
            && !e.slots[p].shape.main_axis().is_some_and(|i| {
                e.attrs[p].main_spacing(i) == MainSpacing::Even
                    && old.align[1 - i] == next.align[1 - i]
            })
        {
            self.enqueue(Pass::Place, p, e);
        }
        // The single-column basis in Flow may stay equal while the policy
        // starts, stops or limits packing of the actual columns in Place.
        if a.width != b.width && (shape.packs_width(a.width) || shape.packs_width(b.width)) {
            self.enqueue(Pass::Place, k, e);
            self.packed_width_reader(e, k);
        }
        let raw = e.raw(k);
        let height = !old
            .sizing
            .height
            .same_split(old_raw, next.sizing.height, raw);
        if e.slots[k].stats.own.height_reader
            && (shape.passes_cross_height() || shape == Shape::Column)
            && (height
                || a.height != b.height
                || a.padding[0] != b.padding[0]
                || a.padding[2] != b.padding[2])
        {
            self.enqueue(Pass::Context, k, e);
        }
        if shape == Shape::Image
            && (a.width_from_height() || b.width_from_height())
            && (a != b || old.image_axes != next.image_axes)
        {
            self.enqueue(Pass::Place, k, e);
            self.packed_width_reader(e, k);
        }
        if a.height != b.height
            && e.slots[k].slot.is_some_and(Slot::host_height)
            && e.slots[k].stats.own.height_reader
        {
            self.enqueue(Pass::Context, k, e);
            self.enqueue(Pass::Flow, k, e);
        }
        if shape.cross_height_px(a) != shape.cross_height_px(b)
            || (shape.cross_height_px(b).is_some()
                && (a.padding[0] != b.padding[0] || a.padding[2] != b.padding[2]))
        {
            self.invalidate_early_heights(e, k);
        }
        if shape.offers_inner_height()
            && a.padding[1] != b.padding[1]
            && (gaps.is_some_and(|(_, b)| b == MainSpacing::Even)
                || e.children(k)
                    .iter()
                    .any(|&c| e.alignment(c, 0) != Alignment::Start))
        {
            self.enqueue(Pass::Place, k, e);
        }
        let width = !old.sizing.width.same_split(old_raw, next.sizing.width, raw);
        let split_height = parent.is_some_and(Shape::allots_height);
        if shape == Shape::Column
            && (a.height != b.height
                || a.width != b.width
                || charge_changed
                || a.padding[0] != b.padding[0]
                || a.padding[2] != b.padding[2])
            && (self.has_height_group(k) || Self::has_group_reader(e, k))
        {
            self.enqueue(Pass::Context, k, e);
        }
        if parent == Some(Shape::Column)
            && let Some(p) = e.slots[k].parent
            && (matches!(e.attrs[p].sizing.height, Length::Px(_))
                || e.slots[p].stats.own.height_reader)
        {
            // Equal intrinsic widths can still add or remove a reader of the
            // Column's height shares. Compare the policies used by the allocator.
            let reads = |style| shape.reads_group_height(style, e.attrs[p].sizing);
            let slot_changed = height
                || old.rotation != next.rotation
                || (next.rotation.mixes_axes() && a.width != b.width)
                || a.height != b.height
                || old.layout_scale != next.layout_scale
                || (shape == Shape::Image
                    && (old.image_axes != next.image_axes
                        || a.width != b.width
                        || a.padding != b.padding));
            if slot_changed && e.slots[p].stats.children.height_reader && !self.has_height_group(p)
            {
                self.enqueue(Pass::Measure, p, e);
                self.refresh_child_offers(e, p);
            }
            if reads(old) != reads(next)
                || (slot_changed && (self.has_height_group(p) || Self::has_group_reader(e, p)))
            {
                self.enqueue(Pass::Context, p, e);
            }
        }
        if height && parent == Some(Shape::Column) && e.slots[k].stats.children.height_reader {
            self.enqueue(Pass::Context, k, e);
        }
        if height
            && Self::reads_early_height(e, k)
            && (a.height != b.height
                || old.sizing.height.reads_fill_content()
                || next.sizing.height.reads_fill_content()
                || parent == Some(Shape::Column))
        {
            self.enqueue(Pass::Measure, k, e);
        }
        if parent == Some(Shape::WrappedColumn)
            && let Some(p) = e.slots[k].parent
        {
            if e.slots[p].stats.children.height_reader
                && (height
                    || old.height_floor != next.height_floor
                    || a.height != b.height
                    || old.rotation != next.rotation)
            {
                self.enqueue(Pass::Measure, p, e);
            }
            if old.height_floor != next.height_floor
                || a.height.weight().is_some() != b.height.weight().is_some()
            {
                self.enqueue(Pass::Reflow, p, e);
            }
            if !matches!(e.attrs[p].sizing.width, Length::Px(_))
                && (shape.has_intrinsic_height(old) != shape.has_intrinsic_height(next)
                    || old.height_floor != next.height_floor
                    || a.height.weight().is_some() != b.height.weight().is_some()
                    || a.width != b.width
                    || (width
                        && (next.sizing.width.reads_fill_content()
                            || old.sizing.width.reads_fill_content())))
            {
                self.enqueue(Pass::Measure, p, e);
            }
        }
        if matches!(shape, Shape::Column | Shape::TextColumn)
            && parent == Some(Shape::WrappedRow)
            && (a.width == Length::Auto) != (b.width == Length::Auto)
            && let Some(p) = e.slots[k].parent
        {
            self.enqueue(Pass::Flow, p, e);
        }
        // When fitting its content, a Column charges measured fill bases
        // and reflowed nonfills. The choice can change at two equal fits,
        // including when the Column's own height is a content-limited fill.
        if split_height
            && a.height.weight().is_some() != b.height.weight().is_some()
            && let Some(p) = e.slots[k].parent
            && e.attrs[p].sizing.height.fits_content_height()
        {
            self.enqueue(Pass::Reflow, p, e);
        }
        if width
            && (old.sizing.width.reads_fill_content() || next.sizing.width.reads_fill_content())
            && !parent.is_some_and(Shape::allots_width)
        {
            self.enqueue(Pass::Flow, k, e);
        }
        let parent_input = width.then(|| {
            if old.sizing.width.weight().is_some() != next.sizing.width.weight().is_some() {
                FlowInput::ChildFill
            } else {
                FlowInput::ChildSplit
            }
        });
        // On the cross axis a changed numeric weight alone is not an input.
        let height = height
            && (a.height != b.height
                || old.sizing.height.reads_fill_content()
                || next.sizing.height.reads_fill_content()
                || split_height);
        self.invalidate_offer_readers(e, k, parent_input, height);
    }
    // A Column reads shares; a WrappedRow reads occupied heights to advance
    // lines. At other sites only the element reads its incoming height offer.
    pub(super) fn invalidate_offer_readers(
        &mut self,
        e: &Elements,
        k: Key,
        width: Option<FlowInput>,
        height: bool,
    ) {
        if width.is_none() && !height {
            return;
        }
        let parent = e.layout_parent(k);
        if height {
            if e.slots[k].shape == Shape::Paragraph && e.is_prose_float(k) {
                // Mixed policies can change whether wrapped content grows a
                // floating paragraph without changing its intrinsic clamp.
                self.enqueue(Pass::Reflow, k, e);
            }
            if let Some(p) = parent.filter(|&p| {
                e.slots[p].shape.offers_inner_height() && e.alignment(k, 1) != Alignment::Start
            }) {
                self.enqueue(Pass::Place, p, e);
            }
            if e.attrs[k].rotation.mixes_axes()
                || e.slots[k].shape.packs_width(e.attrs[k].sizing.width)
                || (e.slots[k].shape == Shape::Image && e.attrs[k].sizing.width_from_height())
            {
                // A new height can move even a top-aligned child's horizontal origin.
                self.packed_width_reader(e, k);
            }
            if let Some(p) = parent.filter(|&p| e.slots[p].shape.reads_prose_box(e, k)) {
                self.enqueue(Pass::Reflow, p, e);
            } else if let Some(p) = parent.filter(|&p| e.slots[p].shape == Shape::WrappedRow) {
                // The resolved outer box can move later lines at equal fit.
                let pass = if e.attrs[p].sizing.height.fits_content_height() {
                    Pass::Reflow
                } else {
                    Pass::Place
                };
                self.enqueue(pass, p, e);
            } else if let Some(p) = parent.filter(|&p| e.slots[p].shape.allots_height()) {
                if e.attrs[p].sizing.height.definite() || e.slots[p].shape == Shape::WrappedColumn {
                    self.enqueue(Pass::Place, p, e);
                }
            } else {
                self.enqueue(Pass::Place, k, e);
            }
        }
        if let Some(p) = parent
            && width
                .is_some_and(|input| e.slots[p].shape.reads_flow(e.attrs[p].sizing.width, input))
        {
            self.enqueue(Pass::Flow, p, e);
            if width == Some(FlowInput::ChildFill) {
                self.auto_width_reader(e, p);
            }
        }
    }
}
