//! Sparse subscriptions. Rebinding queues at most one job per occurrence.
//! A completion publishes a whole immutable source (pixels *and* dimensions).
use super::*;
use crate::raster_assets::{Request, Source};

#[derive(Default)]
struct Binding {
    source: Source,
    generation: u64,
    queued: Option<usize>,
}
#[derive(Default)]
pub(super) struct Images {
    slots: SecondaryMap<Key, Binding>,
    queue: Vec<Key>,
    generation: u64,
    pub active: usize,
}
impl Images {
    pub fn reserve(&mut self, keys: usize, active: usize, budget: Budget) -> Result<(), String> {
        if active != 0 {
            self.slots
                .try_set_capacity(keys)
                .map_err(pool::capacity_error)?;
            // The commit cancels removed subscriptions before inserting new ones.
            if active > self.queue.capacity() {
                pool::reserve(&mut self.queue, budget.capacity(active))?;
            }
        }
        Ok(())
    }
    pub fn request(&mut self, k: Key) {
        self.generation += 1;
        let b = self.slots.entry(k).unwrap().or_default();
        b.generation = self.generation;
        if b.queued.is_none() {
            b.queued = Some(self.queue.len());
            self.queue.push(k);
        }
    }
    pub fn remove(&mut self, k: Key) {
        if let Some(i) = self.slots.remove(k).and_then(|b| b.queued) {
            self.queue.swap_remove(i);
            if let Some(&moved) = self.queue.get(i) {
                self.slots[moved].queued = Some(i);
            }
        }
    }
    pub fn source(&self, k: Key) -> &Source {
        &self.slots[k].source
    }
}
pub(crate) enum Ticket {
    Open(Key, u64),
    Fit(paint::fits::Job),
}
impl Tree {
    pub(crate) fn needs_images(&self) -> bool {
        !self.elements.images.queue.is_empty()
    }
    pub(crate) fn waiting_for_pixels(&self) -> bool {
        self.output.fits.waiting()
    }
    pub(crate) fn next_image(&mut self) -> Option<(Ticket, Request)> {
        let e = &mut self.elements;
        let Some(k) = e.images.queue.pop() else {
            return self
                .output
                .fits
                .next()
                .map(|(job, r)| (Ticket::Fit(job), r));
        };
        let b = &mut e.images.slots[k];
        b.queued = None;
        let ticket = Ticket::Open(k, b.generation);
        let name = std::str::from_utf8(e.attrs[k].image_src.get(e.raw(k)))
            .expect("validated logical name");
        Some((ticket, Request::Open(name.to_owned())))
    }
    pub(crate) fn complete_image(&mut self, ticket: &Ticket, source: Source) -> bool {
        let e = &mut self.elements;
        let (k, generation) = match ticket {
            Ticket::Open(k, generation) => (*k, *generation),
            Ticket::Fit(job) => {
                return match self.output.fits.complete(job, source) {
                    Ok(ready) => ready,
                    Err(record) => {
                        // A metadata-valid file can still fail pixel decode. Only
                        // the visible, still-bound occurrences read that failure.
                        for k in self.output.fits.users(&record) {
                            if let Some(b) = e.images.slots.get_mut(k)
                                && matches!(&b.source, Source::Sized(r) if r == &record)
                            {
                                let old = std::mem::replace(&mut b.source, Source::Failed);
                                e.slots[k].change.image.get_or_insert(old);
                                e.touch(k);
                            }
                        }
                        self.output.cancel_fits();
                        true
                    }
                };
            }
        };
        let Some(b) = e.images.slots.get_mut(k) else {
            return false;
        };
        if b.generation != generation || b.source == source {
            return false;
        }
        self.output.cancel_fits();
        // Keep the actual bound image during a replacement. Never infer the
        // next file's dimensions from the last frame or a partially read file.
        let old = std::mem::replace(&mut b.source, source);
        e.slots[k].change.image.get_or_insert(old);
        e.touch(k);
        true
    }
}
