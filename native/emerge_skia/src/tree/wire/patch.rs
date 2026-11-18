use super::*;
#[derive(Default)]
struct Edit {
    attrs: Option<(Attrs, Range<usize>)>,
    order: Option<Range<usize>>,
    previous_bound: Option<capacity::Bound>,
}
#[derive(Default)]
pub(super) struct Batch {
    edits: SecondaryMap<Key, Edit>,
    keys: Vec<Key>,
    seen: SecondaryMap<Key, u64>,
    sequence: u64,
}
impl Batch {
    pub fn prepare(&mut self, keys: usize, capacity: usize) -> Result<()> {
        self.edits.clear();
        self.keys.clear();
        self.seen.clear();
        self.edits
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        self.seen
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        pool::reserve(&mut self.keys, capacity)
    }
    fn validate(&mut self, e: &Elements, bytes: &[u8]) -> Result<()> {
        for k in self.keys.drain(..) {
            self.edits.remove(k);
        }
        let mut r = Reader(bytes);
        while !r.0.is_empty() {
            let tag = r.byte()?;
            if !matches!(tag, 1 | 2) {
                return Err(format!("unsupported patch {tag} in first slice"));
            }
            let k = *e.ids.get(&r.id()?).ok_or("unknown patch target")?;
            if !self.edits.contains_key(k) {
                self.keys.push(k);
                self.edits.insert(k, Edit::default());
            }
            if tag == 1 {
                let raw = r.block()?;
                self.edits[k].attrs = Some((attrs(raw, e.slots[k].kind)?, range(bytes, raw)))
            } else {
                let next = r.children()?;
                self.sequence += 1;
                if next.len() / 8 != e.children(k).len() {
                    return Err("only same-parent reorder is supported".into());
                }
                for id in ids(next) {
                    let &c = e.ids.get(&id).ok_or("unknown child ID")?;
                    if e.slots[c].parent != Some(k)
                        || self.seen.insert(c, self.sequence) == Some(self.sequence)
                    {
                        return Err("only same-parent reorder is supported".into());
                    }
                }
                self.edits[k].order = Some(range(bytes, next));
            }
        }
        Ok(())
    }
}
fn invalidate_readers(l: &mut Layers, e: &Elements, k: Key, next: &Attrs, raw: &[u8]) {
    let old = &e.attrs[k];
    let depth = e.slots[k].depth;
    let mut invalidate = |pass| l.invalidate(pass, k, depth);
    if old.family != next.family || old.font_size != next.font_size {
        invalidate(Pass::Context)
    }
    let (a, b) = (old.sizing, next.sizing);
    if a != b {
        invalidate(Pass::Measure)
    }
    if a.width != b.width || a.padding[1] != b.padding[1] || a.padding[3] != b.padding[3] {
        invalidate(Pass::Flow)
    }
    if a.height != b.height || a.spacing != b.spacing || a.padding != b.padding {
        invalidate(Pass::Reflow)
    }
    if old.color != next.color
        || (e.slots[k].kind == Kind::Text
            && (a.padding[0] != b.padding[0] || a.padding[3] != b.padding[3]))
    {
        invalidate(Pass::PaintContext)
    }
    if let Some(outputs) =
        paint::Outputs::changed(old.background != next.background, old.click != next.click)
    {
        l.invalidate_composition(k, depth, outputs)
    }
    if e.text(k).as_bytes() != &raw[next.text.clone()] {
        l.text_changed(k, depth)
    }
    if (a.width == Length::Fill) != (b.width == Length::Fill)
        && let Some(p) = e.slots[k].parent
        && e.slots[p].kind == Kind::Column
        && e.attrs[p].sizing.width == Length::Auto
    {
        l.invalidate(Pass::Flow, p, e.slots[p].depth)
    }
}
impl Tree {
    pub fn patch(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_BYTES {
            return Err("patch exceeds storage limit".into());
        }
        let batch = &mut self.wire.patch;
        let e = &mut self.elements;
        batch.validate(e, bytes)?;
        // All records (including shadowed records in the batch) have been checked.
        // Only the last declaration/order per mount can be observed by a reader.
        let mut sizes_changed = false;
        let mut sizes = self.layers.text.sizes;
        let size_changes = || {
            batch.keys.iter().filter_map(|&k| {
                let old = e.attrs[k].font_size;
                let next = batch.edits[k].attrs.as_ref()?.0.font_size;
                (old != next).then_some((old, next))
            })
        };
        for (old, _) in size_changes() {
            sizes_changed = true;
            if let Some(size) = old {
                sizes.remove(size)
            }
        }
        if sizes_changed {
            for (_, next) in size_changes() {
                if let Some(size) = next {
                    sizes.add(size)?
                }
            }
        }
        let old_context_dirty = self.layers.bounds.context_dirty;
        let mut total = e.bytes.len();
        for &k in &batch.keys {
            let edit = &mut batch.edits[k];
            if let Some((next, r)) = &edit.attrs {
                if e.raw(k) == &bytes[r.clone()] {
                    edit.attrs = None;
                    continue;
                }
                total = total - e.raw(k).len() + r.len();
                // A color/listener/box edit does not require another glyph span.
                // Keep any earlier, still pending text demand in that case.
                if e.slots[k].kind == Kind::Text
                    && e.text(k).as_bytes() != &bytes[r.clone()][next.text.clone()]
                {
                    let old = self
                        .layers
                        .runs
                        .get(k)
                        .map_or(0, |r| self.layers.text.glyphs.get(&r.span).len());
                    let i = e.slots[k].index;
                    edit.previous_bound = Some(self.layers.bounds.leaf(i));
                    self.layers.bounds.set(i, old, next.glyph_count, true)
                }
                if e.attrs[k].family != next.family || e.attrs[k].font_size != next.font_size {
                    self.layers.bounds.context_dirty = true;
                }
            }
        }
        let n = e.slots.len();
        let admit = (|| {
            if total > MAX_BYTES {
                return Err("patched declarations exceed storage limit".into());
            }
            e.bytes.ensure(total, n, self.budget)?;
            if n != 0 {
                self.layers.text.ensure(
                    self.layers.bounds.peak(),
                    self.layers.text_count,
                    self.budget,
                )?;
                self.output
                    .reserve_for(n, self.layers.bounds.desired(), self.budget)?;
            }
            if sizes_changed {
                self.layers.text.provision(
                    sizes,
                    self.layers.prepared_scale,
                    self.layers.text_count != 0,
                )?
            }
            Ok(())
        })();
        if let Err(err) = admit {
            for &k in &batch.keys {
                if let Some(before) = batch.edits[k].previous_bound {
                    self.layers.bounds.replace(e.slots[k].index, before)
                }
            }
            self.layers.bounds.context_dirty = old_context_dirty;
            return Err(err);
        }
        for &k in &batch.keys {
            if let Some((next, r)) = &batch.edits[k].attrs {
                invalidate_readers(&mut self.layers, e, k, next, &bytes[r.clone()]);
                e.bytes.release(std::mem::take(&mut e.slots[k].bytes));
            }
        }
        for k in batch.keys.drain(..) {
            let edit = batch.edits.remove(k).unwrap();
            if let Some((next, r)) = edit.attrs {
                e.slots[k].bytes = e.bytes.insert(&bytes[r]);
                e.attrs.insert(k, next);
            }
            if let Some(r) = edit.order {
                let next = &bytes[r];
                if ids(next).eq(e.children(k).iter().map(|&c| e.slots[c].id)) {
                    continue;
                }
                for (to, id) in e
                    .child_pool
                    .write(e.slots[k].children)
                    .iter_mut()
                    .zip(ids(next))
                {
                    *to = e.ids[&id]
                }
                for p in [Pass::Measure, Pass::Reflow, Pass::Compose] {
                    self.layers.invalidate(p, k, e.slots[k].depth)
                }
            }
        }
        Ok(())
    }
}
