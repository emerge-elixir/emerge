use super::*;
pub(super) struct Record<'a> {
    pub id: u64,
    pub shape: Shape,
    pub attrs: Attrs,
    pub raw: &'a [u8],
    pub children: &'a [u8],
}
pub(super) fn record<'a>(r: &mut Reader<'a>) -> Result<Record<'a>> {
    let id = r.id()?;
    let shape = Shape::from_tag(r.byte()?)?;
    let raw = r.block()?;
    let attrs = attrs(raw, shape)?;
    let children = r.children()?;
    shape.check_children(children.len() / 8)?;
    if r.count()? != 0 {
        return Err("Nearby is not in the first slice".into());
    }
    Ok(Record {
        id,
        shape,
        attrs,
        raw,
        children,
    })
}
#[derive(Default)]
struct Link {
    edges: Range<usize>,
    parent: Option<u64>,
    depth: Option<usize>,
}
#[derive(Default)]
pub(super) struct Proof {
    links: HashMap<u64, Link>,
    stack: Vec<u64>,
}
#[derive(Default)]
pub(super) struct Requirements {
    pub root: Option<u64>,
    raw: usize,
    glyphs: usize,
    longest: usize,
    texts: usize,
    sizes: text::Sizes,
}
impl Proof {
    pub fn take_parent_depth(&mut self, id: u64) -> (Option<u64>, usize) {
        let p = self.links.remove(&id).expect("validated ID");
        (p.parent, p.depth.unwrap())
    }
    pub fn read<'a>(
        &mut self,
        data: &'a [u8],
        budget: Budget,
    ) -> Result<(Reader<'a>, usize, Requirements)> {
        let mut r = Reader(data);
        if r.take(5)? != b"EMRG\x09" {
            return Err("expected EMRG version 9".into());
        }
        let n = r.long()?;
        if n > r.0.len() / 19 {
            return Err("invalid node count".into());
        }
        if data.len() > MAX_BYTES || n > 1_000_000 {
            return Err("upload exceeds storage limit".into());
        }
        let req = self.validate(r.0, n, budget)?;
        Ok((r, n, req))
    }
    fn validate(&mut self, bytes: &[u8], n: usize, budget: Budget) -> Result<Requirements> {
        self.links.clear();
        self.stack.clear();
        if n > self.links.capacity() {
            self.links
                .try_reserve(budget.capacity(n))
                .map_err(pool::capacity_error)?
        }
        if n > self.stack.capacity() {
            pool::reserve(&mut self.stack, budget.capacity(n))?
        }
        let mut r = Reader(bytes);
        let mut out = Requirements::default();
        for _ in 0..n {
            let rec = record(&mut r)?;
            out.root.get_or_insert(rec.id);
            if self
                .links
                .insert(
                    rec.id,
                    Link {
                        edges: range(bytes, rec.children),
                        ..Default::default()
                    },
                )
                .is_some()
            {
                return Err("duplicate element ID".into());
            }
            if let Some(size) = rec.attrs.font_size {
                out.sizes.add(size)?
            }
            out.raw += rec.raw.len();
            out.glyphs += rec.attrs.glyph_count;
            out.longest = out.longest.max(rec.attrs.glyph_count);
            out.texts += usize::from(rec.shape.uses_glyphs());
        }
        if !r.0.is_empty() {
            return Err("trailing upload bytes".into());
        }
        let mut visited = 0;
        if let Some(id) = out.root {
            self.links.get_mut(&id).unwrap().depth = Some(0);
            self.stack.push(id)
        }
        while let Some(id) = self.stack.pop() {
            let link = &self.links[&id];
            visited += 1;
            let depth = link.depth.unwrap() + 1;
            for c in ids(&bytes[link.edges.clone()]) {
                if depth > MAX_DEPTH {
                    return Err(format!("tree depth exceeds {MAX_DEPTH}"));
                }
                let p = self.links.get_mut(&c).ok_or("unknown child ID")?;
                if p.depth.replace(depth).is_some() {
                    return Err("cycle or duplicate child".into());
                }
                p.parent = Some(id);
                self.stack.push(c)
            }
        }
        if visited != n {
            return Err("unreachable element".into());
        }
        Ok(out)
    }
}
fn mount_id(slots: &mut SlotMap<Key, Mount>, ids: &mut HashMap<u64, Key>, id: u64) -> Key {
    *ids.entry(id)
        .or_insert_with(|| slots.insert(Mount::new(id, Shape::Empty, 0)))
}
impl Tree {
    pub(super) fn reserve_owners(&mut self, n: usize) -> Result<()> {
        let cap = self.budget.capacity(n);
        let e = &mut self.elements;
        if n > e.slots.capacity() {
            e.slots
                .try_reserve(cap - e.slots.len())
                .map_err(pool::capacity_error)?
        }
        if n > e.ids.capacity() {
            e.ids
                .try_reserve(cap - e.ids.len())
                .map_err(pool::capacity_error)?
        }
        let key_cap = e.slots.capacity();
        e.attrs
            .try_set_capacity(key_cap)
            .map_err(pool::capacity_error)?;
        let owners = if n > self.layers.owners {
            cap
        } else {
            self.layers.owners
        };
        pool::reserve(&mut e.touched, owners)?;
        pool::reserve(&mut e.detached, owners)?;
        pool::reserve(&mut e.free_indices, owners)?;
        self.layers.reserve(key_cap, owners)
    }
    pub fn upload(&mut self, data: &[u8]) -> Result<()> {
        self.upload_scaled(data, 1.)
    }
    pub(crate) fn upload_scaled(&mut self, data: &[u8], scale: f32) -> Result<()> {
        let (mut r, n, req) = self.wire.upload.read(data, self.budget)?;
        self.reserve_owners(n)?;
        self.wire
            .patch
            .get_or_insert_with(Default::default)
            .reserve(self.layers.owners)?;
        self.elements
            .bytes
            .ensure(2 * req.raw, 2 * n, self.budget)?;
        self.elements
            .child_pool
            .ensure(2 * n.saturating_sub(1), 2 * n, self.budget)?;
        self.layers
            .text
            .ensure(req.glyphs + req.longest, n, self.budget)?;
        self.output.reserve_for(n, req.glyphs, self.budget)?;
        self.layers
            .text
            .provision(req.sizes, scale, req.texts != 0)?;
        // No fallible admission or heap work after this point.
        self.clear();
        self.layers.clear();
        let e = &mut self.elements;
        self.layers.bounds.reset();
        e.live_bytes = req.raw;
        self.layers.text_count = req.texts;
        self.layers.prepared_scale = scale;
        // Validate topology using borrowed wire offsets, then decode records directly
        // into mounts. Forward references claim their final generational key here.
        for index in 0..n {
            let rec = record(&mut r).expect("validated record");
            let (parent, depth) = self.wire.upload.take_parent_depth(rec.id);
            let k = mount_id(&mut e.slots, &mut e.ids, rec.id);
            let span = e.child_pool.alloc(rec.children.len() / 8);
            for (to, id) in e.child_pool.write(span).iter_mut().zip(ids(rec.children)) {
                *to = mount_id(&mut e.slots, &mut e.ids, id)
            }
            let parent = parent.map(|id| mount_id(&mut e.slots, &mut e.ids, id));
            e.slots[k] = Mount {
                depth,
                parent,
                children: span,
                bytes: e.bytes.insert(rec.raw),
                ..Mount::new(rec.id, rec.shape, index)
            };
            self.layers
                .bounds
                .set(index, 0, rec.attrs.glyph_count, true);
            e.attrs.insert(k, rec.attrs);
            e.touch(k)
        }
        self.root = req.root.map(|id| e.ids[&id]);
        Ok(())
    }
}
