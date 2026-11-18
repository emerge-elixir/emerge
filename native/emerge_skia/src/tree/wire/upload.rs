use super::*;
struct Record<'a> {
    id: u64,
    shape: Shape,
    attrs: Attrs,
    raw: &'a [u8],
    children: &'a [u8],
}
fn record<'a>(r: &mut Reader<'a>) -> Result<Record<'a>> {
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
struct Requirements {
    root: Option<u64>,
    raw: usize,
    glyphs: usize,
    longest: usize,
    texts: usize,
    sizes: text::Sizes,
}
impl Proof {
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
    *ids.entry(id).or_insert_with(|| {
        slots.insert(Mount {
            id,
            shape: Shape::Empty,
            parent: None,
            depth: 0,
            index: 0,
            bytes: Default::default(),
            children: Default::default(),
        })
    })
}
impl Tree {
    pub fn upload(&mut self, data: &[u8]) -> Result<()> {
        self.upload_scaled(data, 1.)
    }
    pub(crate) fn upload_scaled(&mut self, data: &[u8], scale: f32) -> Result<()> {
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
        let req = self.wire.upload.validate(r.0, n, self.budget)?;
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
        e.bytes.ensure(req.raw, n, self.budget)?;
        e.child_pool.ensure(n.saturating_sub(1), n, self.budget)?;
        let owners = if n > self.layers.owners {
            cap
        } else {
            self.layers.owners
        };
        self.layers.reserve(key_cap, owners)?;
        self.layers
            .text
            .ensure(req.glyphs + req.longest, req.texts, self.budget)?;
        self.output.reserve_for(n, req.glyphs, self.budget)?;
        self.wire.patch.prepare(key_cap, owners)?;
        self.layers
            .text
            .provision(req.sizes, scale, req.texts != 0)?;
        // No fallible admission or heap work after this point.
        self.clear();
        self.layers.clear();
        let e = &mut self.elements;
        self.layers.bounds.reset(n);
        self.layers.text_count = req.texts;
        self.layers.prepared_scale = scale;
        // Validate topology using borrowed wire offsets, then decode records directly
        // into mounts. Forward references claim their final generational key here.
        for index in 0..n {
            let rec = record(&mut r).expect("validated record");
            let p = &self.wire.upload.links[&rec.id];
            let k = mount_id(&mut e.slots, &mut e.ids, rec.id);
            let span = e.child_pool.alloc(rec.children.len() / 8);
            for (to, id) in e.child_pool.write(span).iter_mut().zip(ids(rec.children)) {
                *to = mount_id(&mut e.slots, &mut e.ids, id)
            }
            let parent = p.parent.map(|id| mount_id(&mut e.slots, &mut e.ids, id));
            let depth = p.depth.unwrap();
            e.slots[k] = Mount {
                id: rec.id,
                shape: rec.shape,
                depth,
                parent,
                index,
                children: span,
                bytes: e.bytes.insert(rec.raw),
            };
            self.layers
                .bounds
                .set(index, 0, rec.attrs.glyph_count, true);
            e.attrs.insert(k, rec.attrs);
            self.layers.mount(k, depth)
        }
        self.root = req.root.map(|id| e.ids[&id]);
        Ok(())
    }
}
