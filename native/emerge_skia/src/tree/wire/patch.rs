use super::{batch::Batch, *};

// Occupied storage includes detached slots and the first inputs replaced since
// recompute. Live declarations alone are subject to the wire limits.
struct Demand {
    resident: usize,
    live: usize,
    live_bytes: usize,
    raw: usize,
    edges: usize,
    texts: usize,
    new_glyphs: usize,
    new_ids: usize,
    sizes: text::Sizes,
}
impl Demand {
    fn new(t: &Tree) -> Self {
        let e = &t.elements;
        Self {
            resident: e.slots.len(),
            live: e.attached_count(),
            live_bytes: e.live_bytes,
            raw: e.bytes.len(),
            edges: e.child_pool.len(),
            texts: t.layers.text_count,
            new_glyphs: 0,
            new_ids: 0,
            sizes: t.layers.text.sizes,
        }
    }
    fn discard(&mut self, e: &Elements, k: Key) {
        let m = &e.slots[k];
        self.resident -= 1;
        self.raw -= e.raw(k).len()
            + m.change
                .attrs
                .as_ref()
                .map_or(0, |(_, s)| e.bytes.get(s).len());
        self.edges -=
            e.children(k).len() + m.change.order.map_or(0, |s| e.child_pool.get(&s).len());
    }
}
fn set_bound(
    l: &mut Layers,
    undo: &mut Vec<(usize, capacity::Bound)>,
    i: usize,
    old: usize,
    next: usize,
    dirty: bool,
) {
    undo.push((i, l.bounds.leaf(i)));
    l.bounds.set(i, old, next, dirty);
}
impl Tree {
    pub fn patch(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_BYTES {
            return Err("patch exceeds storage limit".into());
        }
        if bytes.is_empty() {
            return Ok(());
        }
        // Move the reusable scratch out only to split its borrow from the tree;
        // neither patch bytes nor a tree clone survives this synchronous call.
        let mut b = self.wire.patch.take().unwrap_or_default();
        let result = b
            .validate(&self.elements, &mut self.wire.upload, bytes, self.budget)
            .and_then(|()| {
                if b.entries.is_empty() {
                    return Ok(());
                }
                let dirty = self.layers.bounds.context_dirty;
                match self.admit(&mut b, bytes) {
                    Ok(d) => {
                        self.commit(&mut b, bytes, d);
                        Ok(())
                    }
                    Err(err) => {
                        for (i, old) in b.undo.drain(..).rev() {
                            self.layers.bounds.replace(i, old);
                        }
                        self.layers.bounds.context_dirty = dirty;
                        Err(err)
                    }
                }
            });
        self.wire.patch = Some(b);
        result
    }
    fn admit(&mut self, b: &mut Batch, bytes: &[u8]) -> Result<Demand> {
        let mut d = Demand::new(self);
        let e = &self.elements;
        let l = &mut self.layers;
        // Withdraw old declarations first: all 64 font sizes may be replaced in one batch.
        for s in &b.entries {
            if let Some(&k) = e.ids.get(&s.id)
                && e.slots[k].detached.is_none()
            {
                d.live -= 1;
                d.live_bytes -= e.raw(k).len();
                d.texts -= usize::from(e.slots[k].shape.uses_glyphs());
                if let Some(size) = e.attrs[k].font_size {
                    d.sizes.remove(size);
                }
            }
        }
        for s in &mut b.entries {
            let mut existing = e.ids.get(&s.id).copied();
            let old_glyphs = existing
                .and_then(|k| l.runs.get(k))
                .map_or(0, |r| l.text.glyphs.get(&r.span).len());
            if !s.attached {
                if let Some(k) = existing {
                    set_bound(l, &mut b.undo, e.slots[k].index, old_glyphs, 0, false);
                }
                continue;
            }
            if let Some(k) = existing
                && e.slots[k].shape != s.shape
            {
                b.victims.push(k);
                d.discard(e, k);
                set_bound(l, &mut b.undo, e.slots[k].index, 0, 0, false);
                existing = None;
            }
            if let Some(k) = existing
                && let Some((_, raw)) = &s.attrs
                && e.raw(k) == &bytes[raw.clone()]
            {
                s.attrs = None;
            }
            let (a, len) = s.attrs.as_ref().map_or_else(
                || {
                    let k = existing.unwrap();
                    (&e.attrs[k], e.raw(k).len())
                },
                |(a, r)| (a, r.len()),
            );
            d.live += 1;
            d.live_bytes += len;
            d.texts += usize::from(s.shape.uses_glyphs());
            if let Some(size) = a.font_size {
                d.sizes.add(size)?;
            }
            if let Some(k) = existing {
                let m = &e.slots[k];
                if let Some((a, r)) = &s.attrs {
                    if m.change.cold || m.change.attrs.is_some() {
                        d.raw -= e.raw(k).len();
                    }
                    d.raw += r.len();
                    if a.family != e.attrs[k].family || a.font_size != e.attrs[k].font_size {
                        l.bounds.context_dirty = true;
                    }
                }
                if let Some(span) = s.order {
                    if m.change.cold || m.change.order.is_some() {
                        d.edges -= e.children(k).len();
                    }
                    d.edges += b.orders.get(&span).len();
                }
                if s.inserted
                    || (s.shape.uses_glyphs()
                        && s.attrs.as_ref().is_some_and(|(a, r)| {
                            e.text(k).as_bytes() != &bytes[r.clone()][a.text.clone()]
                        }))
                {
                    set_bound(l, &mut b.undo, m.index, old_glyphs, a.glyph_count, true);
                }
            } else {
                d.resident += 1;
                d.new_ids += 1;
                d.raw += len;
                d.edges += b.orders.get(&s.order.unwrap()).len();
                d.new_glyphs += a.glyph_count;
            }
        }
        if d.live > 1_000_000
            || d.live_bytes + 17 * d.live + 8 * d.live.saturating_sub(1) + 9 > MAX_BYTES
        {
            return Err("patched declarations exceed storage limit".into());
        }
        // Reclaim only under pressure; never scan unrelated attached mounts.
        if d.resident > l.owners
            || d.raw > e.bytes.capacity()
            || d.edges > e.child_pool.capacity()
            || l.bounds.peak() + d.new_glyphs > l.text.glyphs.capacity()
        {
            let start = b.victims.len();
            // At most one victim per old slot, including incompatible generations above.
            pool::reserve(&mut b.victims, e.slots.len())?;
            for &k in &e.detached {
                if b.get(e.slots[k].id).is_none_or(|s| !s.attached) {
                    b.victims.push(k);
                }
            }
            for s in &b.entries {
                if !s.attached
                    && let Some(&k) = e.ids.get(&s.id)
                    && e.slots[k].detached.is_none()
                {
                    b.victims.push(k);
                }
            }
            pool::reserve(&mut b.undo, b.entries.len() + b.victims.len() - start)?;
            for &k in &b.victims[start..] {
                d.discard(e, k);
                set_bound(l, &mut b.undo, e.slots[k].index, 0, 0, false);
            }
        }
        let peak = l.bounds.peak() + d.new_glyphs;
        let desired = l.bounds.desired() + d.new_glyphs;
        self.reserve_owners(d.resident)?;
        // Removing keys may leave hash tombstones, not insertion capacity.
        self.elements
            .ids
            .try_reserve(d.new_ids)
            .map_err(pool::capacity_error)?;
        b.reserve(self.layers.owners)?;
        self.elements
            .bytes
            .ensure(d.raw, 2 * d.resident, self.budget)?;
        self.elements
            .child_pool
            .ensure(d.edges, 2 * d.resident, self.budget)?;
        self.layers.text.ensure(peak, d.resident, self.budget)?;
        self.output.reserve_for(d.live, desired, self.budget)?;
        self.layers
            .text
            .provision(d.sizes, self.layers.prepared_scale, d.texts != 0)?;
        Ok(d)
    }
    fn commit(&mut self, b: &mut Batch, bytes: &[u8], d: Demand) {
        let e = &mut self.elements;
        for &k in &b.victims {
            self.layers.evict(k);
            e.evict(k);
        }
        // Claim final keys (including forward references) and make all reclaimed
        // spans available before installing any larger declaration or child order.
        for s in &b.entries {
            if !s.attached {
                if let Some(&k) = e.ids.get(&s.id) {
                    e.detach(k);
                }
                continue;
            }
            let k = if let Some(&k) = e.ids.get(&s.id) {
                if s.inserted {
                    e.uncache(k);
                    e.slots[k].change.reattached = true;
                    e.slots[k].attachment += 1;
                }
                k
            } else {
                let index = e.free_indices.pop().unwrap_or(e.slots.len());
                let k = e.slots.insert(Mount::new(s.id, s.shape, index));
                e.ids.insert(s.id, k);
                self.layers
                    .bounds
                    .set(index, 0, s.attrs.as_ref().unwrap().0.glyph_count, true);
                k
            };
            if s.attrs.is_some() {
                e.replace_bytes(k);
            }
            if s.order.is_some() {
                e.slots[k].change.membership |= s.membership;
                e.replace_order(k);
            }
            if s.attrs.is_some() || s.order.is_some() || s.inserted {
                e.touch(k);
            }
        }
        for s in &mut b.entries {
            if !s.attached {
                continue;
            }
            let k = e.ids[&s.id];
            if s.inserted {
                e.slots[k].depth = s.depth;
                e.slots[k].parent = s.parent.map(|id| e.ids[&id]);
            }
            if let Some((a, r)) = s.attrs.take() {
                e.slots[k].bytes = e.bytes.insert(&bytes[r]);
                e.attrs.insert(k, a);
            }
            if let Some(order) = s.order {
                let cs = b.orders.get(&order);
                let span = e.child_pool.alloc(cs.len());
                for (to, id) in e.child_pool.write(span).iter_mut().zip(cs) {
                    *to = e.ids[id];
                }
                e.slots[k].children = span;
            }
        }
        e.live_bytes = d.live_bytes;
        self.layers.text_count = d.texts;
    }
}
