//! Work for one candidate command stream. Coalesce visible uses before issuing
//! any decode. Resolve the samples once, before raster replay or GPU sealing.
use super::*;
use crate::raster_assets::{BitmapRef, Record, Request, Source, covers};
use skia_safe::ISize;

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
}
impl Fits {
    pub(in crate::tree) fn advance_from(&mut self, old: &Self) {
        self.epoch = old.epoch + 1;
    }
    pub(in crate::tree) fn reserve(&mut self, count: usize, budget: Budget) -> Result<(), String> {
        if count > self.index.capacity() {
            self.index
                .try_reserve(budget.capacity(count) - self.index.len())
                .map_err(pool::capacity_error)?;
        }
        if count > self.entries.capacity() {
            pool::reserve(&mut self.entries, budget.capacity(count))?;
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
    pub(super) fn unbound(&self) -> bool {
        !self.entries.is_empty()
    }
    pub(super) fn reset(&mut self) {
        self.index.clear();
        self.entries.clear();
        self.users.clear();
        self.cursor = 0;
        self.missing = 0;
        self.epoch += 1;
    }
    pub(super) fn add(&mut self, k: Key, r: &Record, size: ISize) {
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
        self.missing != 0
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
        for (_, cmd) in &mut drawing.commands {
            if let Command::Image(sample) = cmd
                && let Source::Sized(r) = &sample.source
            {
                let e = &self.entries[self.index[&r.id()]];
                let bitmap = e.pixels.as_ref().expect("all fitted inputs settled");
                sample.bind(bitmap, r.size());
            }
        }
        self.reset();
    }
}
