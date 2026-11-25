//! Live authoring owns the ID allocator. Layout pins an immutable version and
//! has only derived, key-addressed cache slots. Detached trees retain their
//! pooled encoding and synchronous reserve-before-commit path.
use super::*;
use slotmap::Key as _;
use std::{
    ops::{Index, IndexMut},
    sync::Arc,
};

#[derive(Debug)]
pub(super) struct Block {
    pub _grant: storage::Grant,
    pub attrs: Attrs,
    pub raw: Box<[u8]>,
    pub media: Option<Arc<crate::raster_assets::Media>>,
}
#[derive(Debug)]
pub(super) struct Order {
    pub keys: Box<[Key]>,
    pub _grant: storage::Grant,
}
impl std::ops::Deref for Order {
    type Target = [Key];
    fn deref(&self) -> &[Key] {
        &self.keys
    }
}
#[derive(Debug)]
pub(super) struct Record {
    pub _grant: storage::Grant,
    pub id: u64,
    pub shape: Shape,
    pub parent: Option<Key>,
    pub stats: Subtree,
    pub inputs: scale::Inputs,
    pub index: usize,
    pub block: Arc<Block>,
    pub children: Arc<Order>,
}
impl Record {
    // Each live occurrence owns one block and one order within this version.
    // A shared inline source is counted by digest in the separate media account.
    fn extent(&self) -> usize {
        self._grant.bytes() + self.block._grant.bytes() + self.children._grant.bytes()
    }
}
// Fixed fanout bounds the amount copied at every directory level. A shallow
// version clone does not clone a hash table or a wide owner's children.
#[derive(Clone)]
enum Page {
    Branch([Option<Arc<Resident>>; 16]),
    Leaf([Option<Arc<Record>>; 16]),
}
struct Resident {
    value: Page,
    _grant: storage::Grant,
}
#[derive(Default, Clone)]
pub(super) struct Records {
    root: Option<Arc<Resident>>,
    pub extent: usize,
}
impl Records {
    pub fn get(&self, k: Key) -> Option<&Arc<Record>> {
        let bits = k.data().as_ffi();
        let mut p = self.root.as_deref()?;
        for shift in (1..16).rev() {
            let Page::Branch(nodes) = &p.value else {
                return None;
            };
            p = nodes[((bits >> (4 * shift)) & 15) as usize].as_deref()?;
        }
        match &p.value {
            Page::Leaf(nodes) => nodes[(bits & 15) as usize].as_ref(),
            _ => None,
        }
    }
    pub fn set(&mut self, k: Key, value: Option<Arc<Record>>, pages: &mut Pages) {
        fn update(
            slot: &mut Option<Arc<Resident>>,
            bits: u64,
            shift: u32,
            value: Option<Arc<Record>>,
            pages: &mut Pages,
            extent: &mut usize,
        ) {
            if slot.as_mut().and_then(Arc::get_mut).is_none() {
                let mut fresh = pages.spare.pop().expect("reserved edit path");
                let p = Arc::get_mut(&mut fresh).unwrap();
                if slot.is_none() {
                    *extent += p._grant.bytes();
                }
                p.value = slot
                    .as_ref()
                    .map(|old| old.value.clone())
                    .unwrap_or_else(|| {
                        if shift == 0 {
                            Page::Leaf(Default::default())
                        } else {
                            Page::Branch(Default::default())
                        }
                    });
                if let Some(old) = slot.replace(fresh) {
                    pages.retired.push_back(old);
                }
            }
            let i = ((bits >> shift) & 15) as usize;
            let empty = match &mut Arc::get_mut(slot.as_mut().unwrap()).unwrap().value {
                Page::Leaf(v) => {
                    *extent -= v[i].as_ref().map_or(0, |r| r.extent());
                    *extent += value.as_ref().map_or(0, |r| r.extent());
                    v[i] = value;
                    v.iter().all(Option::is_none)
                }
                Page::Branch(v) => {
                    update(&mut v[i], bits, shift - 4, value, pages, extent);
                    v.iter().all(Option::is_none)
                }
            };
            if empty {
                let empty = slot.take().unwrap();
                *extent -= empty._grant.bytes();
                pages.spare.push(empty);
            }
        }
        update(
            &mut self.root,
            k.data().as_ffi(),
            60,
            value,
            pages,
            &mut self.extent,
        );
    }
    pub fn each(&self, mut visit: impl FnMut(Key, &Arc<Record>)) {
        fn walk(p: &Resident, bits: u64, f: &mut impl FnMut(Key, &Arc<Record>)) {
            match &p.value {
                Page::Leaf(v) => {
                    for (i, a) in v.iter().enumerate() {
                        if let Some(a) = a {
                            f(slotmap::KeyData::from_ffi(bits | i as u64).into(), a)
                        }
                    }
                }
                Page::Branch(v) => {
                    for (i, p) in v.iter().enumerate() {
                        if let Some(p) = p {
                            walk(p, (bits | i as u64) << 4, f)
                        }
                    }
                }
            }
        }
        if let Some(p) = &self.root {
            walk(p, 0, &mut visit)
        }
    }
}
/// Space for every distinct prefix written by a draft. Replaced paths return
/// parent first. Recycle a page only when neither a root nor a reader owns it.
/// The reclaimer runs on the provisioner, never in a NIF or the tree actor.
#[derive(Default)]
pub(super) struct Pages {
    spare: Vec<Arc<Resident>>,
    retired: std::collections::VecDeque<Arc<Resident>>,
    paths: Vec<u64>,
}
impl Pages {
    /// The actor has folded a prefix. Keep a small pool of clean, unique
    /// directory pages, not a second strong root retaining removed media.
    pub fn reclaim(&mut self) {
        for mut page in self.retired.drain(..) {
            if self.spare.len() < 64
                && let Some(p) = Arc::get_mut(&mut page)
            {
                p.value = Page::Leaf(Default::default());
                self.spare.push(page);
            }
        }
        self.spare.truncate(64);
    }
    pub fn trim(&mut self) {
        self.retired.clear();
        self.spare.clear();
    }
    pub fn reserve(
        &mut self,
        keys: impl Iterator<Item = Key>,
        quota: &mut storage::Allowance,
    ) -> Result<(), String> {
        self.paths.clear();
        self.paths.extend(keys.map(|k| k.data().as_ffi()));
        self.paths.sort_unstable();
        let n = if self.paths.is_empty() {
            0
        } else {
            16 + self
                .paths
                .windows(2)
                .map(|w| (63 - (w[0] ^ w[1]).leading_zeros().min(63)) as usize / 4)
                .sum::<usize>()
        };
        for _ in 0..self.retired.len().min(4 * n + 32) {
            let mut page = self.retired.pop_front().unwrap();
            if let Some(p) = Arc::get_mut(&mut page) {
                p.value = Page::Leaf(Default::default());
                if self.spare.len() < n + 32 {
                    self.spare.push(page);
                }
            } else {
                self.retired.push_back(page);
            }
        }
        self.retired.try_reserve(n).map_err(pool::capacity_error)?;
        pool::reserve(&mut self.spare, n)?;
        let charge = size_of::<Resident>() + 64;
        let mut grant = quota.reserve(charge * n.saturating_sub(self.spare.len()))?;
        self.spare.resize_with(self.spare.len().max(n), || {
            Arc::new(Resident {
                value: Page::Leaf(Default::default()),
                _grant: grant.split(charge),
            })
        });
        Ok(())
    }
}
#[derive(Default)]
pub(super) struct Attributes {
    local: SecondaryMap<Key, Attrs>,
    pub poses: HashMap<Key, Arc<Attrs>>,
    pub version: Option<Records>,
}
impl Attributes {
    pub fn versioned() -> Self {
        Self {
            version: Some(Records::default()),
            ..Self::default()
        }
    }
    pub fn get(&self, k: Key) -> Option<&Attrs> {
        if !self.poses.is_empty()
            && let Some(a) = self.poses.get(&k)
        {
            return Some(a);
        }
        match &self.version {
            Some(v) => Some(&v.get(k)?.block.attrs),
            None => self.local.get(k),
        }
    }
    pub fn insert(&mut self, k: Key, a: Attrs) {
        self.local.insert(k, a);
    }
    pub fn remove(&mut self, k: Key) {
        self.local.remove(k);
        self.poses.remove(&k);
    }
    pub fn try_set_capacity(&mut self, n: usize) -> Result<(), std::collections::TryReserveError> {
        if self.version.is_some() {
            Ok(())
        } else {
            self.local.try_set_capacity(n)
        }
    }
}
impl Index<Key> for Attributes {
    type Output = Attrs;
    fn index(&self, k: Key) -> &Attrs {
        self.get(k).expect("resident attributes")
    }
}
#[derive(Default)]
pub(super) struct Mounts {
    assigned: SlotMap<Key, Mount>,
    fixed: Option<SecondaryMap<Key, Mount>>,
}
impl Mounts {
    pub fn layout() -> Self {
        Self {
            fixed: Some(Default::default()),
            ..Default::default()
        }
    }
    pub fn len(&self) -> usize {
        self.fixed
            .as_ref()
            .map_or_else(|| self.assigned.len(), SecondaryMap::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn capacity(&self) -> usize {
        self.fixed
            .as_ref()
            .map_or_else(|| self.assigned.capacity(), SecondaryMap::capacity)
    }
    pub fn try_reserve(
        &mut self,
        additional: usize,
    ) -> Result<(), std::collections::TryReserveError> {
        match &mut self.fixed {
            Some(m) => m.try_set_capacity(m.len() + additional),
            None => self.assigned.try_reserve(additional),
        }
    }
    pub fn insert(&mut self, v: Mount) -> Key {
        self.assigned.insert(v)
    }
    pub fn cache(&mut self, k: Key, v: Mount) {
        self.fixed.as_mut().expect("layout slots").insert(k, v);
    }
    pub fn remove(&mut self, k: Key) -> Option<Mount> {
        match &mut self.fixed {
            Some(m) => m.remove(k),
            None => self.assigned.remove(k),
        }
    }
    pub fn get(&self, k: Key) -> Option<&Mount> {
        match &self.fixed {
            Some(m) => m.get(k),
            None => self.assigned.get(k),
        }
    }
    pub fn contains_key(&self, k: Key) -> bool {
        self.get(k).is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = (Key, &Mount)> {
        self.assigned
            .iter()
            .chain(self.fixed.iter().flat_map(|m| m.iter()))
    }
}
impl Index<Key> for Mounts {
    type Output = Mount;
    fn index(&self, k: Key) -> &Mount {
        self.get(k).expect("resident occurrence")
    }
}
impl IndexMut<Key> for Mounts {
    fn index_mut(&mut self, k: Key) -> &mut Mount {
        match &mut self.fixed {
            Some(m) => &mut m[k],
            None => &mut self.assigned[k],
        }
    }
}
