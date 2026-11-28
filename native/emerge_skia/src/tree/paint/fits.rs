//! Work for one candidate command stream. Coalesce visible uses before issuing
//! any decode. Resolve the samples once, before raster replay or GPU sealing.
use super::*;
use crate::raster_assets::{BitmapRef, Record, Request, Source, covers};
use skia_safe::ISize;
use std::sync::OnceLock;
mod readers;

// Removing a bucket can leave a tombstone. At most half the table's usable
// capacity may be live, so even complete source/attachment turnover can
// rehash in place. Call on admitted extent growth, not on every deletion.
fn reserve_index<K: Eq + std::hash::Hash, V>(
    map: &mut HashMap<K, V>,
    n: usize,
) -> Result<(), String> {
    map.try_reserve(n.saturating_mul(2).saturating_sub(map.len()))
        .map_err(pool::capacity_error)
}

// Filled on the tree owner before a frame can be sealed. Holding frames need
// neither a mutable cache nor a lookup in the latest compressed-source record.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Bound {
    pub source: Source,
    // Keep xywh: subtracting edges to recover a crop width can move a strict
    // sampling boundary by one texel after scaling.
    pub src: Rect,
}
pub(super) type Slot = pages::Ref<OnceLock<Bound>>;
impl pages::Recycle for OnceLock<Bound> {
    fn recycle(&mut self) {
        self.take();
    }
}
struct Draft {
    owner: Key,
    record: Record,
    size: ISize,
    src: Rect,
    slot: Slot,
}

struct Entry {
    record: Record,
    target: ISize,
    pixels: Option<BitmapRef>,
}
pub(crate) struct Job {
    epoch: u64,
    record: Record,
    target: ISize,
}
#[derive(Default)]
pub(in crate::tree) struct Fits {
    index: HashMap<usize, usize>,
    entries: Vec<Entry>,
    users: Vec<(usize, Key)>,
    epoch: u64,
    cursor: usize,
    missing: usize,
    slots: pages::Stock<OnceLock<Bound>>,
    readers: readers::Readers,
    pub(super) tracking: bool,
    drafts: Vec<Draft>,
    submitted: usize,
    pub(super) recording: Option<Key>,
    #[cfg(test)]
    pub(in crate::tree) additions: usize,
}
impl Fits {
    pub(super) fn recording_charge(n: u128, v: u128, b: Budget) -> u128 {
        readers::Readers::charge((n * (100 + b.0 as u128)).div_ceil(100))
            + pages::Stock::<OnceLock<Bound>>::charge(n * v, 0, b)
            + n * size_of::<Draft>() as u128 * (100 + b.0 as u128).div_ceil(100)
    }
    pub(super) fn reserve_versions(&mut self, n: usize, v: usize, b: Budget) -> Result<(), String> {
        if n > self.drafts.capacity() {
            let cap = b.capacity(n);
            self.readers.reserve(cap)?;
            pool::reserve(&mut self.drafts, cap)?;
        }
        self.slots
            .reserve(b.capacity(n * v), || Ok(OnceLock::new()))
    }
    pub(in crate::tree) fn abandon(&mut self) -> impl Iterator<Item = Key> + '_ {
        self.submitted = 0;
        self.drafts.drain(..).map(|d| d.owner)
    }
    pub(in crate::tree) fn has_drafts(&self) -> bool {
        !self.drafts.is_empty()
    }
    pub(super) fn new_drafts(&self) -> bool {
        self.submitted != self.drafts.len()
    }
    pub(super) fn sample(&mut self, k: Key, r: &Record, size: ISize, src: Rect) -> Option<Slot> {
        if self.recording.is_some() {
            let slot = self.slots.issue(|_| ());
            self.drafts.push(Draft {
                owner: k,
                record: r.clone(),
                size,
                src,
                slot: slot.clone(),
            });
            Some(slot)
        } else {
            self.add(k, r, size);
            None
        }
    }
    pub(in crate::tree) fn advance_from(&mut self, old: &Self) {
        self.epoch = old.epoch + 1;
    }
    pub(in crate::tree) fn reserve(&mut self, count: usize, budget: Budget) -> Result<(), String> {
        if count > self.entries.capacity() {
            let cap = budget.capacity(count);
            reserve_index(&mut self.index, cap)?;
            pool::reserve(&mut self.entries, cap)?;
        }
        if count > self.users.capacity() {
            pool::reserve(&mut self.users, budget.capacity(count))?;
        }
        Ok(())
    }
    pub(in crate::tree) fn users(&self, r: &Record) -> impl Iterator<Item = Key> + '_ {
        let id = r.id();
        self.users
            .iter()
            .filter_map(move |&(i, k)| (i == id).then_some(k))
    }
    pub(super) fn check_reader(&mut self, k: Key, e: &Elements) {
        self.readers.check(k, e);
    }
    pub(super) fn forget_reader(&mut self, k: Key) {
        self.readers.forget(k);
    }
    pub(super) fn reopen_readers(&mut self, mut invalidate: impl FnMut(Key)) -> bool {
        if self.missing == 0 || !self.tracking {
            return false;
        }
        let mut changed = false;
        for e in &self.entries {
            if e.pixels.is_none() {
                changed |= self
                    .readers
                    .reopen(e.record.id(), self.epoch, &mut invalidate);
            }
        }
        changed
    }
    pub(super) fn unbound(&self) -> bool {
        !self.entries.is_empty()
    }
    pub(super) fn reset(&mut self) {
        // The index has room for every admitted image. Reset the previous
        // visible requests, not a hash table sized for the whole document.
        for entry in self.entries.drain(..) {
            self.index.remove(&entry.record.id());
        }
        self.users.clear();
        self.cursor = 0;
        self.submitted = 0;
        self.missing = 0;
        self.epoch += 1;
    }
    pub(super) fn add(&mut self, k: Key, r: &Record, size: ISize) {
        #[cfg(test)]
        {
            self.additions += 1;
        }
        self.readers.preparing(k, self.epoch);
        self.users.push((r.id(), k));
        let i = *self.index.entry(r.id()).or_insert_with(|| {
            self.entries.push(Entry {
                record: r.clone(),
                target: size,
                pixels: None,
            });
            self.entries.len() - 1
        });
        let t = &mut self.entries[i].target;
        t.width = t.width.max(size.width);
        t.height = t.height.max(size.height);
    }
    pub(super) fn finish_targets(&mut self) {
        for i in self.submitted..self.drafts.len() {
            let d = &self.drafts[i];
            self.add(d.owner, &d.record.clone(), d.size);
        }
        self.submitted = self.drafts.len();
        self.cursor = 0;
        self.missing = self
            .entries
            .iter_mut()
            .map(|e| {
                e.pixels = e.record.current(e.target);
                usize::from(e.pixels.is_none())
            })
            .sum();
    }
    pub(in crate::tree) fn waiting(&self) -> bool {
        self.missing != 0 || self.new_drafts()
    }
    pub(in crate::tree) fn next(&mut self) -> Option<(Job, Request)> {
        while self.cursor < self.entries.len() {
            let i = self.cursor;
            self.cursor += 1;
            let e = &self.entries[i];
            if e.pixels.is_none() {
                return Some((
                    Job {
                        epoch: self.epoch,
                        record: e.record.clone(),
                        target: e.target,
                    },
                    Request::Fit(e.record.clone(), e.target),
                ));
            }
        }
        None
    }
    pub(in crate::tree) fn complete(&mut self, job: &Job, result: Source) -> Result<bool, Record> {
        let Some(&i) = self.index.get(&job.record.id()) else {
            return Ok(false);
        };
        let e = &mut self.entries[i];
        // A new preparation may explicitly request the same immutable bytes
        // at the same size. Use an already-running successful decode rather
        // than starving it under unrelated edits. Never adopt an old failure.
        if e.pixels.is_some()
            || e.target != job.target
            || (job.epoch != self.epoch && !matches!(result, Source::Ready(_)))
        {
            return Ok(false);
        }
        if let Source::Ready(pixels) = result
            && covers(pixels.image().dimensions(), e.target)
        {
            e.record.publish(&pixels);
            e.pixels = Some(pixels);
            self.missing -= 1;
            Ok(true)
        } else {
            Err(e.record.clone())
        }
    }
    pub(super) fn bind(&mut self, drawing: &mut Drawing) {
        if self.entries.is_empty() || self.waiting() {
            return;
        }
        if self.tracking {
            for &(id, k) in &self.users {
                self.readers.bound(k, id, self.epoch);
            }
        }
        for d in self.drafts.drain(..) {
            let b = self.entries[self.index[&d.record.id()]]
                .pixels
                .as_ref()
                .expect("all fits ready");
            d.slot
                .set(Bound {
                    src: image::bound_crop(d.src, b, d.record.size()),
                    source: Source::Ready(b.clone()),
                })
                .expect("bind each recording once");
        }
        for (_, cmd) in drawing.commands.edit() {
            if let Command::Image(sample) = cmd
                && let image::Pixels::Direct(b) = &mut sample.pixels
                && let Source::Sized(r) = &b.source
            {
                let e = &self.entries[self.index[&r.id()]];
                let bitmap = e.pixels.as_ref().expect("all fitted inputs settled");
                b.src = image::bound_crop(b.src, bitmap, r.size());
                b.source = Source::Ready(bitmap.clone());
            }
        }
        self.reset();
    }
}
