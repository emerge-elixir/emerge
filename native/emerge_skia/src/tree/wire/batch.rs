//! A replacement proof, scoped to the old target. Only named, displaced or
//! summary-changing nodes enter this scratch. Closed interiors are not decoded.
use super::*;
mod domains;
use domains::Domains;
use std::hash::{BuildHasher, RandomState};

pub(super) struct Edit {
    pub id: u64,
    pub shape: Shape,
    pub parent: Option<u64>,
    pub depth: usize,
    pub stats: Subtree,
    pub attached: bool,
    pub inserted: bool,
    pub membership: bool,
    pub attrs: Option<(Attrs, Range<usize>)>,
    pub order: Option<Span<u64>>,
    next_in_bucket: usize,
    visited: bool,
    closed: bool,
    inputs: Option<scale::Inputs>,
}
#[derive(Default)]
pub(super) struct Batch {
    // Removing from an open-addressed HashMap leaves tombstones: reserving the
    // same high-water mark could reallocate on a fitting detach. These chains
    // live in the entries themselves. Zero is empty, other links are index+1.
    hash: RandomState,
    heads: Vec<usize>,
    pub entries: Vec<Edit>,
    pub orders: Pool<u64>,
    stack: Vec<(u64, Option<u64>, usize)>,
    post: Vec<usize>,
    displaced: Vec<Key>,
    pub victims: Vec<Key>,
    pub undo: Vec<(usize, capacity::Bound)>,
    pub root: Option<u64>,
    domains: Domains,
}
impl Batch {
    pub fn reserve(&mut self, n: usize) -> Result<()> {
        if n > self.heads.len() {
            let cap = n.next_power_of_two();
            pool::reserve(&mut self.heads, cap)?;
            self.heads.clear();
            self.heads.resize(cap, 0);
            for (i, s) in self.entries.iter_mut().enumerate() {
                let bucket = self.hash.hash_one(s.id) as usize & (cap - 1);
                s.next_in_bucket = std::mem::replace(&mut self.heads[bucket], i + 1);
            }
        }
        pool::reserve(&mut self.entries, n)?;
        pool::reserve(&mut self.stack, n)?;
        pool::reserve(&mut self.post, n)?;
        pool::reserve(&mut self.displaced, n)?;
        pool::reserve(&mut self.victims, n)?;
        pool::reserve(&mut self.undo, n)?;
        self.domains.reserve(n)?;
        self.orders.ensure(n, n, Budget(0))
    }
    fn reset(&mut self) {
        for s in self.entries.drain(..) {
            let bucket = self.hash.hash_one(s.id) as usize & (self.heads.len() - 1);
            self.heads[bucket] = 0;
            if let Some(span) = s.order {
                self.orders.release(span);
            }
        }
        self.stack.clear();
        self.post.clear();
        self.displaced.clear();
        self.victims.clear();
        self.undo.clear();
        self.domains.clear();
    }
    fn position(&self, id: u64) -> Option<usize> {
        let bucket = self.hash.hash_one(id) as usize & self.heads.len().wrapping_sub(1);
        let mut link = *self.heads.get(bucket)?;
        while link != 0 {
            let i = link - 1;
            let e = &self.entries[i];
            if e.id == id {
                return Some(i);
            }
            link = e.next_in_bucket;
        }
        None
    }
    pub fn get(&self, id: u64) -> Option<&Edit> {
        self.position(id).map(|i| &self.entries[i])
    }
    fn add(&mut self, id: u64, shape: Shape, b: Budget) -> Result<usize> {
        if id == 0 || self.position(id).is_some() {
            return Err("zero or duplicate element ID".into());
        }
        let i = self.entries.len();
        if i == self.entries.capacity() || i >= self.heads.len() {
            self.reserve(b.capacity(i + 1))?;
        }
        let bucket = self.hash.hash_one(id) as usize & (self.heads.len() - 1);
        self.entries.push(Edit {
            next_in_bucket: std::mem::replace(&mut self.heads[bucket], i + 1),
            id,
            shape,
            parent: None,
            depth: 0,
            stats: Subtree::default(),
            attached: false,
            inserted: false,
            membership: false,
            attrs: None,
            order: None,
            visited: false,
            closed: false,
            inputs: None,
        });
        Ok(i)
    }
    fn old(&mut self, e: &Elements, k: Key, b: Budget) -> Result<usize> {
        let m = &e.slots[k];
        if let Some(i) = self.position(m.id) {
            return Ok(i);
        }
        let i = self.add(m.id, m.shape, b)?;
        let s = &mut self.entries[i];
        s.stats = m.stats;
        s.parent = m.parent.map(|p| e.slots[p].id);
        s.depth = e.depth(k);
        s.attached = true;
        Ok(i)
    }
    fn order(
        &mut self,
        i: usize,
        len: usize,
        ids: impl Iterator<Item = u64>,
        budget: Budget,
    ) -> Result<()> {
        self.orders
            .ensure(self.orders.len() + len, self.entries.len(), budget)?;
        let span = self.orders.alloc(len);
        for (to, id) in self.orders.write(span).iter_mut().zip(ids) {
            *to = id;
        }
        self.entries[i].order = Some(span);
        Ok(())
    }
    fn definition(&mut self, e: &Elements, bytes: &[u8], r: &mut Reader, b: Budget) -> Result<()> {
        let id = r.id()?;
        let shape = Shape::from_tag(r.byte()?)?;
        let flags = r.byte()?;
        let old = e.live(id);
        if flags > 3 || (flags != 3 && old.is_none()) {
            return Err("invalid keep field or missing fresh declaration".into());
        }
        if old.is_some_and(|k| e.slots[k].shape != shape) {
            return Err("live occurrence changed kind".into());
        }
        let i = self.add(id, shape, b)?;
        if flags & 1 != 0 {
            let raw = r.block()?;
            self.entries[i].attrs = Some((attrs(raw, shape)?, range(bytes, raw)));
        }
        if flags & 2 != 0 {
            let children = r.children()?;
            shape.check_children(children.len() / 8)?;
            if r.count()? != 0 {
                return Err("Nearby is not in the first slice".into());
            }
            self.order(i, children.len() / 8, ids(children), b)?;
        }
        Ok(())
    }
    fn visit(&mut self, e: &Elements, b: Budget) -> Result<()> {
        while let Some((id, parent, depth)) = self.stack.pop() {
            let old = e.live(id);
            let i = match self.position(id) {
                Some(i) => i,
                None => self.old(e, old.ok_or("unknown kept occurrence")?, b)?,
            };
            let s = &self.entries[i];
            if s.visited {
                return Err("cycle or duplicate child".into());
            }
            // A kept root may move even when both its attrs and its children
            // are closed. Only this incoming edge changes site, not its interior.
            let attrs = s
                .attrs
                .as_ref()
                .map(|(a, _)| a)
                .or_else(|| old.map(|k| &e.attrs[k]));
            if let Some(a) = attrs {
                let host = || {
                    parent.map_or(s.shape, |p| {
                        self.get(p)
                            .map_or_else(|| e.slots[e.ids[&p]].shape, |edit| edit.shape)
                    })
                };
                if a.sizing.width.bound_px().is_some() && !host().allots_width() {
                    return Err("bounded fill width requires a root Row or a child of a Row".into());
                }
                if matches!(a.sizing.width, Length::Weighted(_)) && !host().allots_width() {
                    return Err(
                        "weighted fill width requires a root Row or a child of a Row".into(),
                    );
                }
                if a.sizing.height.weight().is_some() && !host().allots_height() {
                    return Err(
                        "height(fill()) requires a root Column or a child of a Column".into(),
                    );
                }
            }
            let s = &mut self.entries[i];
            s.visited = true;
            s.attached = true;
            s.inserted = old.is_none();
            s.parent = parent;
            s.depth = depth;
            self.post.push(i);
            if let Some(span) = s.order {
                let kids = self.orders.get(&span);
                let end = self.stack.len() + kids.len();
                pool::reserve(&mut self.stack, end)?;
                self.stack
                    .extend(kids.iter().map(|&c| (c, Some(id), depth + 1)));
                s.membership = kids
                    .iter()
                    .any(|&c| old.is_none() || e.live(c).is_none_or(|k| e.slots[k].parent != old));
            } else {
                s.closed = true;
                s.stats = e.slots[old.ok_or("keep requires live occurrence")?].stats;
            }
            if depth + s.stats.height > MAX_DEPTH {
                return Err(format!("tree depth exceeds {MAX_DEPTH}"));
            }
        }
        if self.entries.len() != self.post.len() {
            return Err("unreachable patch record".into());
        }
        Ok(())
    }
    fn within_target(&self, e: &Elements, target: Option<Key>) -> Result<()> {
        for s in &self.entries {
            let Some(mut k) = e.live(s.id) else { continue };
            while Some(k) != target {
                k = e.slots[k].parent.ok_or("occurrence outside patch target")?;
                if self.get(e.slots[k].id).is_some_and(|a| a.closed) {
                    return Err("overlapping retained branch".into());
                }
            }
        }
        Ok(())
    }
    fn fold(&mut self, e: &Elements) -> Result<()> {
        for &i in self.post.iter().rev() {
            let Some(order) = self.entries[i].order else {
                continue;
            };
            let children = self.orders.get(&order);
            let stats = Subtree::over(
                children
                    .iter()
                    .map(|id| self.get(*id).expect("visited child").stats),
            );
            self.entries[i].stats = stats;
            if let Some(k) = e.live(self.entries[i].id) {
                if children
                    .iter()
                    .copied()
                    .eq(e.children(k).iter().map(|&c| e.slots[c].id))
                {
                    self.entries[i].order = None;
                    self.orders.release(order);
                } else {
                    let end = self.displaced.len() + e.children(k).len();
                    pool::reserve(&mut self.displaced, end)?;
                    self.displaced.extend_from_slice(e.children(k));
                }
            }
        }
        Ok(())
    }
    fn replace_edge(
        &mut self,
        e: &Elements,
        parent: Key,
        target: Key,
        root: Option<u64>,
        b: Budget,
    ) -> Result<()> {
        let i = self.old(e, parent, b)?;
        let children = e.children(parent);
        let len = children.len() - usize::from(root.is_none());
        self.order(
            i,
            len,
            children.iter().filter_map(|&c| {
                if c == target {
                    root
                } else {
                    Some(e.slots[c].id)
                }
            }),
            b,
        )?;
        self.entries[i].membership = true;
        Ok(())
    }
    fn ancestors(&mut self, e: &Elements, mut parent: Option<Key>, b: Budget) -> Result<()> {
        while let Some(k) = parent {
            let m = &e.slots[k];
            let stats = if let Some(span) = self.get(m.id).and_then(|s| s.order) {
                Subtree::over(self.orders.get(&span).iter().map(|&id| {
                    self.get(id)
                        .map_or_else(|| e.slots[e.ids[&id]].stats, |s| s.stats)
                }))
            } else {
                Subtree::over(e.children(k).iter().map(|&c| {
                    self.get(e.slots[c].id)
                        .map_or(e.slots[c].stats, |s| s.stats)
                }))
            };
            if stats == m.stats {
                break;
            }
            let i = self.old(e, k, b)?;
            self.entries[i].stats = stats;
            parent = m.parent;
        }
        Ok(())
    }
    pub fn validate(
        &mut self,
        e: &Elements,
        previous: Option<Key>,
        bytes: &[u8],
        b: Budget,
    ) -> Result<()> {
        self.reset();
        let mut r = Reader(bytes);
        if r.take(5)? != b"EMRG\x0a" {
            return Err("expected EMRG version 10".into());
        }
        let target_id = r.id()?;
        let target = if target_id == 0 && previous.is_none() {
            None
        } else {
            Some(e.live(target_id).ok_or("unknown patch target")?)
        };
        let id = r.id()?;
        let root = (id != 0).then_some(id);
        let n = r.long()?;
        if n > r.0.len() / 10 || n > 1_000_000 {
            return Err("invalid record count".into());
        }
        if n > self.entries.capacity() {
            self.reserve(b.capacity(n))?;
        }
        for _ in 0..n {
            self.definition(e, bytes, &mut r, b)?;
        }
        if !r.0.is_empty() {
            return Err("trailing patch bytes".into());
        }
        let parent = target.and_then(|k| e.slots[k].parent);
        if let Some(id) = root {
            if self.stack.capacity() == 0 {
                self.reserve(1)?;
            }
            self.stack.push((
                id,
                parent.map(|k| e.slots[k].id),
                target.map_or(0, |k| e.depth(k)),
            ));
        }
        self.visit(e, b)?;
        self.within_target(e, target)?;
        self.fold(e)?;
        self.root = if parent.is_none() {
            root
        } else {
            previous.map(|k| e.slots[k].id)
        };
        if let Some(k) = target {
            if root != Some(target_id) {
                let end = self.displaced.len() + 1;
                pool::reserve(&mut self.displaced, end)?;
                self.displaced.push(k);
                if let Some(p) = parent {
                    self.replace_edge(e, p, k, root, b)?;
                }
            }
            if root
                .and_then(|id| self.get(id))
                .is_none_or(|s| s.stats != e.slots[k].stats)
            {
                self.ancestors(e, parent, b)?;
            }
        }
        // Claims are complete. Retire only actual absences, stopping at moved
        // subtrees even when an old owner was removed before its new owner.
        while let Some(k) = self.displaced.pop() {
            if self.get(e.slots[k].id).is_some() {
                continue;
            }
            let i = self.old(e, k, b)?;
            self.entries[i].attached = false;
            let end = self.displaced.len() + e.children(k).len();
            pool::reserve(&mut self.displaced, end)?;
            self.displaced.extend_from_slice(e.children(k));
        }
        Ok(())
    }
}
