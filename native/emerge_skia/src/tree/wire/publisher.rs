//! Apply live edits against the last *accepted* declaration. A publication is
//! a pinned root and changed keys, never a wire message for layout to reapply.
use super::{batch::Batch, *};
use crate::tree::declaration::{Block, Mounts, Order, Pages, Record, Records};
use std::sync::Arc;

#[derive(Default, Clone, Copy)]
struct Totals {
    nodes: usize,
    raw: usize,
    glyphs: usize,
    texts: usize,
    images: usize,
    groups: usize,
    clip_nearby: usize,
    poses: usize,
    hosts: usize,
    words: usize,
    materials: usize,
    material_bytes: usize,
    rules: usize,
    scales: usize,
    sizes: text::Sizes,
}
impl Totals {
    fn count(&mut self, shape: Shape, a: &Attrs, raw: usize, add: bool) {
        let count = |n: &mut usize, v: usize| if add { *n += v } else { *n -= v };
        count(&mut self.nodes, 1);
        count(&mut self.raw, raw);
        count(&mut self.glyphs, a.glyph_count);
        count(&mut self.texts, usize::from(shape.uses_glyphs()));
        count(&mut self.images, usize::from(a.has_art()));
        count(&mut self.hosts, usize::from(shape.has_prose_sites()));
        count(&mut self.clip_nearby, usize::from(a.clip_nearby));
        count(&mut self.groups, usize::from(a.opacity.is_some()));
        count(&mut self.poses, usize::from(a.pose_declared));
        count(&mut self.materials, usize::from(a.material()));
        count(&mut self.material_bytes, a.material_bytes());
        count(&mut self.scales, usize::from(a.layout_scale.is_some()));
        count(&mut self.rules, usize::from(a.has_rules()));
    }
}
// A workspace is sent only on growth. Its arrays are provisioned against the
// declaration, not borrowed from a computing tree. In steady state a turn
// carries no computed stores. Two glyph generations can overlap in measure.
#[derive(Default, Clone, Copy)]
struct Capacity([usize; 13]);
impl Capacity {
    fn for_tree(t: Totals, keys: usize, tree: Subtree) -> Result<Self> {
        Ok(Self([
            keys,
            t.nodes,
            t.glyphs,
            t.words,
            t.images,
            t.materials,
            t.material_bytes,
            t.groups,
            t.poses * (1 + usize::from(tree.escapes != 0)),
            tree.escapes,
            tree.mounts,
            t.clip_nearby,
            t.materials
                + tree.extra_commands()?
                + t.images
                + 2 * t.groups
                + if t.groups != 0 { 2 * t.words } else { 0 }
                + if t.rules != 0 { 2 * t.texts } else { 0 },
        ]))
    }
    fn grow(&self, need: Self, b: Budget) -> Option<Self> {
        need.0.iter().zip(self.0).any(|(&n, old)| n > old).then(|| {
            Self(std::array::from_fn(|i| {
                if need.0[i] > self.0[i] {
                    b.capacity(need.0[i])
                } else {
                    self.0[i]
                }
            }))
        })
    }
    fn provision(self, quota: &storage::Quota) -> Result<Box<Tree>> {
        let [
            keys,
            owners,
            glyphs,
            words,
            images,
            materials,
            bytes,
            groups,
            poses,
            escapes,
            mounts,
            clip_nearby,
            extra,
        ] = self.0;
        // Conservative byte credits, including the builder, five readers,
        // secondary-map slots, glyph overlap and paragraph/clip scratch.
        // Keep the credit pinned with any sealed readers after a growth.
        let units = [
            4096, 4096, 160, 8192, 1536, 4096, 6, 128, 2048, 128, 128, 1024, 1536,
        ];
        let charge = self
            .0
            .iter()
            .zip(units)
            .try_fold(65536usize, |s, (n, u)| s.checked_add(n.checked_mul(u)?))
            .ok_or("live workspace exceeds storage limit")?;
        let grant = quota.reserve(charge)?;
        let mut t = Tree::for_session(0);
        t.workspace = Some(Arc::new(grant));
        let e = &mut t.elements;
        e.slots = Mounts::layout();
        e.attrs.version = Some(Default::default());
        e.slots.try_reserve(keys).map_err(pool::capacity_error)?;
        e.ids.try_reserve(owners).map_err(pool::capacity_error)?;
        pool::reserve(&mut e.touched, owners)?;
        t.layers.reserve(keys, owners)?;
        t.layers.text.ensure(2 * glyphs, owners, Budget(0))?;
        t.output
            .reserve_for(owners, glyphs, words, extra, groups + mounts, Budget(0))?;
        t.output.reserve_bands(escapes, groups, Budget(0))?;
        t.output
            .reserve_materials(keys, materials, bytes, words, clip_nearby, Budget(0))?;
        t.layers
            .paragraphs
            .reserve(keys, owners, words, Budget(0))?;
        e.images.reserve(keys, images, Budget(0))?;
        t.output.fits.reserve(images, Budget(0))?;
        t.output.reserve_poses(poses, Budget(0))?;
        let mut batch = Batch::default();
        batch.reserve(owners)?;
        t.wire.patch = Some(batch);
        Ok(Box::new(t))
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArtChange {
    Keep,
    Request,
    Withdraw,
}
#[derive(Clone, Copy)]
struct Change {
    key: Key,
    art: ArtChange,
    membership: bool,
}
pub(crate) struct Publication {
    _descriptors: storage::Grant,
    version: Records,
    root: Option<Key>,
    changes: Arc<[Change]>,
    removed: Arc<[Key]>,
    workspace: Option<Box<Tree>>,
    totals: Totals,
    scale: f32,
    strikes: Arc<text::Strikes>,
}
pub(crate) struct Publisher {
    elements: Elements,
    root: Option<Key>,
    batch: Batch,
    totals: Totals,
    capacity: Capacity,
    strikes: Arc<text::Strikes>,
    budget: Budget,
    pages: Pages,
    storage: storage::Storage,
}
/// Work supplied by the provisioner. Only the NIF writes the draft directory.
/// Its base stays pinned so removing an entry cannot reclaim a subtree here.
pub(crate) struct Draft {
    publication: Option<Publication>,
    resident_limit: usize,
    version: Records,
    pages: Pages,
    writes: Vec<(Key, Arc<Record>)>,
    cursor: usize,
    inserted: Vec<Key>,
    removed: Arc<[Key]>,
    media: media::Changes,
    defs: media::Definitions,
    growth: Option<Capacity>,
    root: Option<Key>,
    totals: Totals,
    strikes: Arc<text::Strikes>,
}
impl Draft {
    pub fn apply_slice(&mut self) -> bool {
        const RECORDS: usize = 32;
        let end = self.removed.len() + self.writes.len();
        for i in self.cursor..(self.cursor + RECORDS).min(end) {
            if i < self.removed.len() {
                self.version.set(self.removed[i], None, &mut self.pages);
            } else {
                let (k, r) = &self.writes[i - self.removed.len()];
                self.version.set(*k, Some(r.clone()), &mut self.pages);
            }
        }
        self.cursor = (self.cursor + RECORDS).min(end);
        self.cursor == end
    }
    pub fn check_extent(&self) -> Result<()> {
        if self.version.extent > self.resident_limit {
            Err(storage::LIMIT.into())
        } else {
            Ok(())
        }
    }
    pub fn publish(&mut self) -> Publication {
        debug_assert_eq!(self.cursor, self.removed.len() + self.writes.len());
        let mut p = self.publication.take().expect("single commit");
        p.version = self.version.clone();
        p
    }
}
impl Publisher {
    #[cfg(test)]
    pub fn new(percent: u16) -> Self {
        Self::with_storage(percent, Default::default())
    }
    pub fn with_storage(percent: u16, storage: storage::Storage) -> Self {
        let mut batch = Batch::default();
        batch.live = true;
        Self {
            storage,
            elements: Elements {
                attrs: declaration::Attributes::versioned(),
                ..Default::default()
            },
            root: None,
            batch,
            totals: Totals::default(),
            capacity: Default::default(),
            strikes: Default::default(),
            pages: Default::default(),
            budget: Budget(percent.into()),
        }
    }
    pub fn trim_if_pressured(&mut self) {
        if self.storage.trim_pages() {
            self.pages.trim();
        }
    }
    pub fn collected(&mut self) {
        self.pages.reclaim();
    }
    pub fn prepare(&mut self, rows: stream::Rows, scale: f32) -> Result<Draft> {
        let result = self.stage(rows, scale);
        // Retire this proof in its own turn, even on refusal. A fitting edit
        // after a large upload must not have to clear the upload's workset.
        self.batch.reset();
        result
    }
    fn stage(&mut self, mut rows: stream::Rows, scale: f32) -> Result<Draft> {
        let b = &mut self.batch;
        let e = &mut self.elements;
        rows.validate(b, e, self.root, self.budget)?;
        let media = media::Changes::for_batch(e, b, |r| rows.raw(r), &rows.defs)?;
        if media.extent > self.storage.media.single_limit() {
            return Err(storage::LIMIT.into());
        }
        let mut t = self.totals;
        // Withdraw first: a batch may replace every one of the 64 font sizes.
        for s in &b.entries {
            if let Some(k) = e.live(s.id) {
                let a = &e.attrs[k];
                t.count(s.shape, a, e.raw(k).len(), false);
                if let Some(n) = a.font_size {
                    t.sizes.remove(n);
                }
            }
        }
        let mut new = 0;
        for s in b.entries.iter_mut().filter(|s| s.attached) {
            let old = e.live(s.id);
            new += usize::from(old.is_none());
            if s.attrs
                .as_ref()
                .zip(old)
                .is_some_and(|((_, r), k)| e.raw(k) == rows.raw(r))
            {
                s.attrs = None;
            }
            let (a, raw) = s.attrs.as_ref().map_or_else(
                || {
                    let k = old.expect("kept input");
                    (&e.attrs[k], e.raw(k))
                },
                |(a, r)| (a, rows.raw(r)),
            );
            s.request_image = a.has_art()
                && old.is_none_or(|k| {
                    e.attrs[k].svg_expected != a.svg_expected
                        || e.attrs[k].image_src.differs(e.raw(k), a.image_src, raw)
                });
            t.count(s.shape, a, raw.len(), true);
            if let Some(n) = a.font_size {
                t.sizes.add(n)?;
            }
        }
        let tree = b.root_stats(e);
        if t.nodes > 1_000_000
            || media.bytes().saturating_add(
                t.raw + 18 * t.nodes + 8 * t.nodes.saturating_sub(1) + tree.mounts + 25,
            ) > MAX_BYTES
        {
            return Err("patched declarations exceed storage limit".into());
        }
        t.words = if t.hosts != 0 {
            b.paragraph_words(e, t.words)
        } else {
            0
        };
        // Reserve fresh generational keys without removing any accepted key.
        // Abandoning a draft just returns these unpublished reservations.
        let overlap = e.slots.len() + new;
        if self.budget.capacity(overlap) > (256usize << 20) / (size_of::<Mount>() + 128) {
            return Err("live occurrence index exceeds storage limit".into());
        }
        if overlap > e.slots.capacity() {
            e.slots
                .try_reserve(self.budget.capacity(overlap) - e.slots.len())
                .map_err(pool::capacity_error)?;
        }
        e.ids.try_reserve(new).map_err(pool::capacity_error)?;
        pool::reserve(&mut e.free_indices, e.slots.capacity())?;
        let demand = Capacity::for_tree(t, e.slots.capacity(), tree)?;
        let growth = self.capacity.grow(demand, self.budget);
        let workspace = growth
            .map(|c| c.provision(&self.storage.workspace))
            .transpose()?;
        let mut strikes = self.strikes.clone();
        b.prepare_domains(e, &mut strikes, scale, t.scales)?;
        b.commit_strikes(&mut strikes);
        const RECORD: usize = size_of::<Record>() + 32;
        const BLOCK: usize = size_of::<Block>() + 48;
        let order_size = |edges: Edges<'_, u64>| {
            size_of::<Order>() + 64 + size_of_val(edges.keys) + edges.tags.len()
        };
        let touched = b.entries.len() + b.changed_inputs().count();
        let descriptors = rows.document.reserve(256 + touched * 96)?;
        let extent = b
            .entries
            .iter()
            .filter(|s| s.attached)
            .map(|s| {
                s.attrs
                    .as_ref()
                    .map_or(0, |(_, r)| BLOCK + rows.raw(r).len())
                    + s.order.map_or(0, |o| order_size(b.orders.get(o)))
            })
            .sum::<usize>();
        let mut backing = rows.document.reserve(RECORD * touched + extent)?;
        let version = e.attrs.version.as_ref().unwrap().clone();
        let removed: Arc<[_]> = b
            .entries
            .iter()
            .filter(|s| !s.attached)
            .filter_map(|s| e.live(s.id))
            .collect();
        let high = e.slots.len() + e.free_indices.len();
        let indices = removed
            .iter()
            .rev()
            .map(|&k| e.slots[k].index)
            .chain(e.free_indices.iter().rev().copied())
            .chain(high..)
            .take(new)
            .collect::<Vec<_>>();
        let inserted = b
            .entries
            .iter()
            .filter(|s| s.attached && s.inserted)
            .zip(indices)
            .map(|(s, index)| {
                let k = e.slots.insert(Mount::new(s.id, s.shape, index));
                e.ids.insert(s.id, k);
                k
            })
            .collect();
        let mut writes = Vec::with_capacity(b.entries.len());
        let mut changes = Vec::with_capacity(b.entries.len());
        for s in b.entries.iter_mut().filter(|s| s.attached) {
            let k = e.ids[&s.id];
            let m = &e.slots[k];
            let old = version.get(k);
            let parent = s.parent.map(|p| e.ids[&p]);
            let inputs = s.inputs.unwrap_or(m.admitted);
            if s.attrs.is_none()
                && s.order.is_none()
                && old.is_some_and(|r| {
                    r.parent == parent
                        && r.slot == s.slot
                        && r.stats == s.stats
                        && r.inputs == inputs
                })
            {
                continue;
            }
            let block = s.attrs.take().map_or_else(
                || old.unwrap().block.clone(),
                |(attrs, r)| {
                    let raw = rows.take(&r);
                    let media = attrs
                        .image_src
                        .get(&raw)
                        .strip_prefix(&[4])
                        .and_then(|v| v.try_into().ok())
                        .map(|id| {
                            rows.defs
                                .get(id)
                                .or_else(|| e.media.find(id))
                                .expect("proven media")
                                .clone()
                        });
                    Arc::new(Block {
                        _grant: backing.split(BLOCK + raw.len()),
                        attrs,
                        raw,
                        media,
                    })
                },
            );
            let children = s.order.map_or_else(
                || old.unwrap().children.clone(),
                |order| {
                    let edges = b.orders.get(order);
                    Arc::new(Order {
                        keys: edges.keys.iter().map(|id| e.ids[id]).collect(),
                        tags: edges.tags.into(),
                        _grant: backing.split(order_size(edges)),
                    })
                },
            );
            let art = if s.request_image {
                ArtChange::Request
            } else if !block.attrs.has_art() && old.is_some_and(|r| r.block.attrs.has_art()) {
                ArtChange::Withdraw
            } else {
                ArtChange::Keep
            };
            let r = Record {
                _grant: backing.split(RECORD),
                id: s.id,
                shape: s.shape,
                parent,
                slot: s.slot,
                stats: s.stats,
                inputs,
                index: m.index,
                block,
                children,
            };
            writes.push((k, Arc::new(r)));
            changes.push(Change {
                key: k,
                art,
                membership: s.membership,
            });
        }
        for (id, inputs) in b.changed_inputs().filter(|(id, _)| b.get(*id).is_none()) {
            let k = e.ids[&id];
            let old = version.get(k).unwrap();
            let r = Record {
                _grant: backing.split(RECORD),
                id,
                shape: old.shape,
                parent: old.parent,
                slot: old.slot,
                stats: old.stats,
                index: old.index,
                block: old.block.clone(),
                children: old.children.clone(),
                inputs,
            };
            writes.push((k, Arc::new(r)));
            changes.push(Change {
                key: k,
                art: ArtChange::Keep,
                membership: false,
            });
        }
        let root = b.root.map(|id| e.ids[&id]);
        let publication = Some(Publication {
            _descriptors: descriptors,
            root,
            version: Records::default(),
            changes: changes.into(),
            removed: removed.clone(),
            workspace,
            totals: t,
            scale,
            strikes: strikes.clone(),
        });
        if let Err(error) = self.pages.reserve(
            removed
                .iter()
                .copied()
                .chain(writes.iter().map(|(k, _)| *k)),
            &mut rows.document,
        ) {
            for k in inserted {
                let m = e.slots.remove(k).unwrap();
                e.ids.remove(&m.id);
            }
            return Err(error);
        }
        let pages = std::mem::take(&mut self.pages);
        Ok(Draft {
            publication,
            resident_limit: self.storage.document.single_limit(),
            version,
            pages,
            writes,
            cursor: 0,
            inserted,
            removed,
            media,
            defs: rows.defs,
            growth,
            root,
            totals: t,
            strikes,
        })
    }
    /// Maintain only the writer's lookup and capacity proofs here. The NIF
    /// already applied the declaration; no consumer ever reapplies the wire.
    pub fn release(&mut self, d: Draft, committed: bool) {
        let e = &mut self.elements;
        self.pages = d.pages;
        if !committed {
            // There may never be another collection after a cancellation.
            // Drop private reserves before announcing credit, not on the next edit.
            self.pages.trim();
            for k in d.inserted {
                let m = e.slots.remove(k).unwrap();
                e.ids.remove(&m.id);
            }
            return;
        }
        for &k in d.removed.iter() {
            let m = e.slots.remove(k).unwrap();
            e.ids.remove(&m.id);
            e.free_indices.push(m.index);
        }
        e.free_indices
            .truncate(e.free_indices.len().saturating_sub(d.inserted.len()));
        e.topology += 1;
        for (k, r) in d.writes {
            let m = &mut e.slots[k];
            m.parent = r.parent;
            m.slot = r.slot;
            m.admitted = r.inputs;
            m.stats = r.stats;
        }
        d.media.install(&d.defs, e);
        e.attrs.version = Some(d.version);
        self.root = d.root;
        self.totals = d.totals;
        self.strikes = d.strikes;
        if let Some(g) = d.growth {
            self.capacity = g;
        }
    }
    #[cfg(test)]
    pub fn apply(&mut self, wire: &[u8], scale: f32) -> Result<Publication> {
        let mut stream = stream::Stream::bounded(wire.len(), &self.storage)?;
        for (i, part) in wire.chunks(stream::CHUNK).enumerate() {
            stream.push(i * stream::CHUNK, part)?;
        }
        let mut d = self.prepare(stream.finish()?, scale)?;
        while !d.apply_slice() {}
        if let Err(e) = d.check_extent() {
            self.release(d, false);
            return Err(e);
        }
        let p = d.publish();
        self.release(d, true);
        Ok(p)
    }
}
impl Publication {
    pub fn install(self, t: &mut Tree) {
        let rebuild = self.workspace.is_some();
        if let Some(mut workspace) = self.workspace {
            workspace
                .elements
                .images
                .inherit(&mut t.elements.images, &self.version);
            workspace.output.fits.advance_from(&t.output.fits);
            *t = *workspace;
        }
        let e = &mut t.elements;
        for &k in self.removed.iter() {
            if e.slots.contains_key(k) {
                t.layers.evict(k);
                e.evict(k);
            }
        }
        let old = e
            .attrs
            .version
            .replace(self.version.clone())
            .unwrap_or_default();
        e.topology += 1;
        // Backgrounds can disappear without detaching their hosts. Withdraw
        // subscriptions before adding any new ones within the reserved count.
        if !rebuild && e.images.active != 0 {
            for c in self.changes.iter().filter(|c| c.art == ArtChange::Withdraw) {
                e.withdraw_art(c.key);
            }
        }
        let mut bind = |c: Change| {
            let k = c.key;
            let r = self.version.get(k).unwrap();
            if !e.slots.contains_key(k) {
                e.slots.cache(k, Mount::new(r.id, r.shape, r.index));
                e.ids.insert(r.id, k);
            }
            let m = &mut e.slots[k];
            if (m.parent, m.slot) != (r.parent, r.slot) {
                m.change.old_parent.get_or_insert((m.parent, m.slot));
            }
            if !m.change.cold && m.change.before.is_none() {
                m.change.before = old.get(k).cloned();
            }
            m.parent = r.parent;
            m.slot = r.slot;
            m.stats = r.stats;
            m.admitted = r.inputs;
            m.change.membership |= c.membership;
            if c.art == ArtChange::Request
                || (rebuild && r.block.attrs.has_art() && !e.images.bound(k))
            {
                e.images.request(k);
            }
            e.touch(k);
        };
        if rebuild {
            self.version.each(|key, _| {
                bind(Change {
                    key,
                    art: ArtChange::Keep,
                    membership: false,
                })
            });
        }
        self.changes.iter().copied().for_each(bind);
        // A display resize may have rebased the prior turn to a different
        // scale. Restore this version's proofs before preparing another epoch.
        if !rebuild && t.layers.prepared_scale != self.scale {
            self.version.each(|k, r| e.slots[k].admitted = r.inputs);
        }
        t.output.cancel_fits();
        t.root = self.root;
        t.layers.prepared_scale = self.scale;
        t.layers.text.strikes = self.strikes;
        t.layers.text.sizes = self.totals.sizes;
        t.layers.scale_declarations = self.totals.scales;
        t.layers.text_count = self.totals.texts;
        t.layers.paragraphs.hosts = self.totals.hosts;
        t.layers.paragraphs.words = self.totals.words;
        e.images.active = self.totals.images;
    }
}
