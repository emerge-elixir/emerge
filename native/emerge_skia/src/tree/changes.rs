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
    pub old_parent: Option<Option<Key>>,
    pub attrs: Option<(Attrs, Span<u8>)>,
    pub order: Option<Span<Key>>,
}
impl Mount {
    pub fn new(id: u64, shape: Shape, index: usize) -> Self {
        Self {
            id,
            shape,
            index,
            parent: None,
            depth: Cell::new((0, 0)),
            stats: Subtree::default(),
            admitted: scale::Inputs::default(),
            children: Span::default(),
            bytes: Span::default(),
            detached: None,
            attachment: 0,
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
            self.child_pool.release(span);
        }
    }
    pub fn evict(&mut self, k: Key) {
        self.unlist(k);
        self.uncache(k);
        let m = self.slots.remove(k).unwrap();
        self.ids.remove(&m.id);
        self.attrs.remove(k);
        self.bytes.release(m.bytes);
        self.child_pool.release(m.children);
        if let Some((_, s)) = m.change.attrs {
            self.bytes.release(s);
        }
        if let Some(s) = m.change.order {
            self.child_pool.release(s);
        }
        self.free_indices.push(m.index);
    }
}
impl Layers {
    pub fn invalidate_changes(&mut self, e: &mut Elements) {
        #[cfg(test)]
        if !e.touched.is_empty() {
            self.invalidation_passes += 1;
        }
        while let Some(k) = e.touched.pop() {
            let depth = e.depth(k);
            let m = &mut e.slots[k];
            debug_assert!(m.detached.is_none());
            let change = std::mem::take(&mut m.change);
            if change.cold {
                self.mount(k, depth);
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
                    self.invalidate(p, k, depth);
                }
                self.invalidate_composition(k, depth, Outputs::Hit);
            } else if change.old_parent.is_some_and(|p| p != m.parent) {
                if let Some(part) = self.parts.get_mut(k) {
                    part.reattach();
                }
                for p in [Pass::Context, Pass::Flow, Pass::Place, Pass::PaintContext] {
                    self.invalidate(p, k, depth);
                }
            }
            if let Some((before, span)) = change.attrs {
                self.invalidate_attrs(e, k, &before, e.bytes.get(&span));
                e.bytes.release(span);
            }
            if let Some(span) = change.order {
                self.invalidate_order(e, k, e.child_pool.get(&span), change.membership);
                e.child_pool.release(span);
            }
        }
    }
    fn invalidate_attrs(&mut self, e: &Elements, k: Key, old: &Attrs, old_raw: &[u8]) {
        let next = &e.attrs[k];
        let depth = e.depth(k);
        let shape = e.slots[k].shape;
        let mut invalidate = |pass| self.invalidate(pass, k, depth);
        if old.family != next.family
            || old.font_size != next.font_size
            || old.layout_scale != next.layout_scale
        {
            invalidate(Pass::Context)
        }
        let (a, b) = (old.sizing.without_weights(), next.sizing.without_weights());
        if a != b {
            invalidate(Pass::Measure)
        }
        if a.width != b.width
            || a.padding[1] != b.padding[1]
            || a.padding[3] != b.padding[3]
            || (a.spacing[0] != b.spacing[0] && shape.reads_flow(b.width, FlowInput::HorizontalGap))
        {
            invalidate(Pass::Flow)
        }
        if a.height != b.height || a.spacing != b.spacing || a.padding != b.padding {
            invalidate(Pass::Reflow)
        }
        if old.color != next.color || shape.paint_context_attrs_changed(old, next) {
            invalidate(Pass::PaintContext)
        }
        if let Some(outputs) = Outputs::changed(
            shape.paint_attrs_changed(old, next),
            old.click != next.click,
        ) {
            self.invalidate_composition(k, depth, outputs)
        }
        if shape.uses_glyphs() && &old_raw[old.text.clone()] != e.text(k).as_bytes() {
            self.text_changed(k, depth)
        }
        let raw = e.raw(k);
        let width = !old.sizing.width.same_split(old_raw, next.sizing.width, raw);
        let height = !old
            .sizing
            .height
            .same_split(old_raw, next.sizing.height, raw);
        if old.sizing.height.reads_fill_content() != next.sizing.height.reads_fill_content() {
            self.invalidate(Pass::Reflow, k, depth);
            if let Some(p) = e.slots[k].parent {
                self.invalidate(Pass::Reflow, p, e.depth(p));
            }
        }
        if e.slots[k].parent.is_none() {
            if width
                && (old.sizing.width.reads_fill_content() || next.sizing.width.reads_fill_content())
            {
                self.invalidate(Pass::Flow, k, depth);
            }
            if height
                && (old.sizing.height.reads_fill_content()
                    || next.sizing.height.reads_fill_content())
            {
                self.invalidate(Pass::Place, k, depth);
            }
        }
        self.invalidate_split(e, k, width, height);
    }
    // The splitter can change a sibling's offer or offset even if neither
    // the host frame nor this child's intrinsic box changes.
    pub(super) fn invalidate_split(&mut self, e: &Elements, k: Key, width: bool, height: bool) {
        if !width && !height {
            return;
        }
        let Some(p) = e.slots[k].parent else { return };
        let shape = e.slots[p].shape;
        let b = e.attrs[p].sizing;
        if height && shape.allots_height() && b.height.definite() {
            self.invalidate(Pass::Place, p, e.depth(p));
        }
        if width && shape.reads_flow(b.width, FlowInput::ChildFill) {
            self.invalidate(Pass::Flow, p, e.depth(p));
        }
    }
}
