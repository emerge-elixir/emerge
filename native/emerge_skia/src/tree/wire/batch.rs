//! Sparse transaction overlay. IDs refer to the preceding commands in this
//! batch. Only affected child lists are materialized, in reusable typed scratch.
use super::*;
use std::collections::HashSet;

pub(super) struct Edit {
    pub id: u64,
    pub shape: Shape,
    pub parent: Option<u64>,
    pub depth: usize,
    pub attached: bool,
    pub inserted: bool,
    pub membership: bool,
    pub attrs: Option<(Attrs, Range<usize>)>,
    pub order: Option<Span<u64>>,
}
#[derive(Default)]
pub(super) struct Batch {
    index: HashMap<u64, usize>,
    pub entries: Vec<Edit>,
    pub orders: Pool<u64>,
    children: Vec<u64>,
    stack: Vec<u64>,
    seen: HashSet<u64>,
    pub victims: Vec<Key>,
    pub undo: Vec<(usize, capacity::Bound)>,
}
impl Batch {
    pub fn reserve(&mut self, n: usize) -> Result<()> {
        if n > self.index.capacity() {
            self.index
                .try_reserve(n - self.index.len())
                .map_err(pool::capacity_error)?;
        }
        if n > self.seen.capacity() {
            self.seen
                .try_reserve(n - self.seen.len())
                .map_err(pool::capacity_error)?;
        }
        pool::reserve(&mut self.entries, n)?;
        pool::reserve(&mut self.children, n)?;
        pool::reserve(&mut self.stack, n)?;
        pool::reserve(&mut self.victims, n)?;
        pool::reserve(&mut self.undo, n)?;
        self.orders.ensure(n, n, Budget(0))
    }
    fn room(&mut self, n: usize, b: Budget) -> Result<()> {
        if n > self.entries.capacity() {
            self.reserve(b.capacity(n))?;
        }
        Ok(())
    }
    pub fn get(&self, id: u64) -> Option<&Edit> {
        self.index.get(&id).map(|&i| &self.entries[i])
    }
    fn claim(&mut self, id: u64, shape: Shape, b: Budget) -> Result<usize> {
        if let Some(&i) = self.index.get(&id) {
            return Ok(i);
        }
        self.room(self.entries.len() + 1, b)?;
        if self.index.len() == self.index.capacity() {
            self.index.try_reserve(1).map_err(pool::capacity_error)?;
        }
        let i = self.entries.len();
        self.entries.push(Edit {
            id,
            shape,
            parent: None,
            depth: 0,
            attached: false,
            inserted: false,
            membership: false,
            attrs: None,
            order: None,
        });
        self.index.insert(id, i);
        Ok(i)
    }
    fn attached(&self, e: &Elements, id: u64) -> bool {
        self.get(id).map_or_else(
            || {
                e.ids
                    .get(&id)
                    .is_some_and(|&k| e.slots[k].detached.is_none())
            },
            |s| s.attached,
        )
    }
    fn active(&mut self, e: &Elements, id: u64, b: Budget) -> Result<usize> {
        if let Some(&i) = self.index.get(&id) {
            return self.entries[i]
                .attached
                .then_some(i)
                .ok_or_else(|| "unknown patch target".into());
        }
        let &k = e.ids.get(&id).ok_or("unknown patch target")?;
        let m = &e.slots[k];
        if m.detached.is_some() {
            return Err("unknown patch target".into());
        }
        let i = self.claim(id, m.shape, b)?;
        let s = &mut self.entries[i];
        s.attached = true;
        s.depth = m.depth;
        s.parent = m.parent.map(|p| e.slots[p].id);
        Ok(i)
    }
    fn load_children(&mut self, e: &Elements, i: usize, b: Budget) -> Result<()> {
        self.children.clear();
        let s = &self.entries[i];
        if let Some(span) = s.order {
            self.children.extend_from_slice(self.orders.get(&span));
        } else {
            let keys = e.children(e.ids[&s.id]);
            if keys.len() > self.children.capacity() {
                pool::reserve(&mut self.children, b.capacity(keys.len()))?;
            }
            self.children.extend(keys.iter().map(|&k| e.slots[k].id));
        }
        Ok(())
    }
    fn store_children(&mut self, i: usize, b: Budget) -> Result<()> {
        if let Some(s) = self.entries[i].order.take() {
            self.orders.release(s);
        }
        self.orders.ensure(
            self.orders.len() + self.children.len(),
            self.entries.len(),
            b,
        )?;
        let span = self.orders.insert(&self.children);
        self.entries[i].order = Some(span);
        Ok(())
    }
    fn insert(
        &mut self,
        e: &Elements,
        proof: &mut upload::Proof,
        p: usize,
        pos: usize,
        wire: (&[u8], &[u8]),
        b: Budget,
    ) -> Result<()> {
        self.entries[p].shape.check_children(1)?;
        let (whole, subtree) = wire;
        let (mut r, n, req) = proof.read(subtree, b)?;
        let root = req.root.ok_or("empty inserted subtree")?;
        let parent = self.entries[p].id;
        let base = self.entries[p].depth + 1;
        for _ in 0..n {
            let rec = upload::record(&mut r)?;
            if self.attached(e, rec.id) {
                return Err("inserted element ID already attached".into());
            }
            let (owner, relative) = proof.take_parent_depth(rec.id);
            let depth = base + relative;
            if depth > MAX_DEPTH {
                return Err(format!("tree depth exceeds {MAX_DEPTH}"));
            }
            let i = self.claim(rec.id, rec.shape, b)?;
            let s = &mut self.entries[i];
            s.attached = true;
            s.inserted = true;
            s.membership = true;
            s.shape = rec.shape;
            s.parent = owner.or(Some(parent));
            s.depth = depth;
            s.attrs = Some((rec.attrs, range(whole, rec.raw)));
            self.children.clear();
            let count = rec.children.len() / 8;
            if count > self.children.capacity() {
                pool::reserve(&mut self.children, b.capacity(count))?;
            }
            self.children.extend(ids(rec.children));
            self.store_children(i, b)?;
        }
        self.load_children(e, p, b)?;
        let need = self.children.len() + 1;
        if need > self.children.capacity() {
            pool::reserve(&mut self.children, b.capacity(need))?;
        }
        self.entries[p].membership = true;
        self.children.insert(pos.min(self.children.len()), root);
        self.store_children(p, b)
    }
    fn remove(&mut self, e: &Elements, id: u64, b: Budget) -> Result<()> {
        // Patch.diff can remove both an ancestor and its descendants.
        if !self.attached(e, id) {
            return Ok(());
        }
        let i = self.active(e, id, b)?;
        let parent = self.entries[i]
            .parent
            .ok_or("root removal is not supported")?;
        let p = self.active(e, parent, b)?;
        self.load_children(e, p, b)?;
        self.entries[p].membership = true;
        self.children.retain(|&c| c != id);
        self.store_children(p, b)?;
        self.stack.push(id);
        while let Some(id) = self.stack.pop() {
            let i = self.active(e, id, b)?;
            self.load_children(e, i, b)?;
            self.entries[i].attached = false;
            let end = self.stack.len() + self.children.len();
            if end > self.stack.capacity() {
                pool::reserve(&mut self.stack, b.capacity(end))?;
            }
            self.stack.extend_from_slice(&self.children);
        }
        Ok(())
    }
    fn reorder(&mut self, e: &Elements, i: usize, next: &[u8], b: Budget) -> Result<()> {
        self.load_children(e, i, b)?;
        if self.children.len() != next.len() / 8 {
            return Err("only same-parent reorder is supported".into());
        }
        self.seen.clear();
        if self.children.len() > self.seen.capacity() {
            self.seen
                .try_reserve(b.capacity(self.children.len()))
                .map_err(pool::capacity_error)?;
        }
        self.seen.extend(self.children.iter().copied());
        self.children.clear();
        for id in ids(next) {
            if !self.seen.remove(&id) {
                return Err("only same-parent reorder is supported".into());
            }
            self.children.push(id);
        }
        self.store_children(i, b)
    }
    pub fn validate(
        &mut self,
        e: &Elements,
        proof: &mut upload::Proof,
        bytes: &[u8],
        b: Budget,
    ) -> Result<()> {
        for s in self.entries.drain(..) {
            self.index.remove(&s.id);
            if let Some(o) = s.order {
                self.orders.release(o);
            }
        }
        self.stack.clear();
        self.victims.clear();
        self.undo.clear();
        let mut r = Reader(bytes);
        while !r.0.is_empty() {
            let tag = r.byte()?;
            match tag {
                1 => {
                    let i = self.active(e, r.id()?, b)?;
                    let raw = r.block()?;
                    self.entries[i].attrs =
                        Some((attrs(raw, self.entries[i].shape)?, range(bytes, raw)));
                }
                2 => {
                    let i = self.active(e, r.id()?, b)?;
                    self.reorder(e, i, r.children()?, b)?;
                }
                3 => {
                    let id = r.id()?;
                    if id == 0 {
                        return Err("root insertion is not supported".into());
                    }
                    let p = self.active(e, id, b)?;
                    let pos = r.count()?;
                    self.insert(e, proof, p, pos, (bytes, r.block()?), b)?;
                }
                4 => self.remove(e, r.id()?, b)?,
                _ => return Err(format!("unsupported patch {tag} in first slice")),
            }
        }
        for s in &self.entries {
            if s.attached
                && let Some(o) = s.order
            {
                let n = self.orders.get(&o).len();
                if n > u16::MAX as usize {
                    return Err("too many children".into());
                }
                s.shape.check_children(n)?;
            }
        }
        Ok(())
    }
}
