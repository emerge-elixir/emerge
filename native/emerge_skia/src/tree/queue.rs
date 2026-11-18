use super::{Key, paint::Outputs};
use slotmap::SecondaryMap;
use std::collections::BinaryHeap;
#[derive(Default)]
pub(super) struct Queue {
    work: BinaryHeap<(usize, Key)>,
    queued: SecondaryMap<Key, Outputs>,
}
impl Queue {
    pub fn reserve(&mut self, keys: usize, occupancy: usize) -> Result<(), String> {
        self.queued
            .try_set_capacity(keys)
            .map_err(super::pool::capacity_error)?;
        if occupancy > self.work.capacity() {
            self.work
                .try_reserve_exact(occupancy - self.work.len())
                .map_err(super::pool::capacity_error)?
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.work.clear();
        self.queued.clear();
    }
    pub fn push(&mut self, rank: usize, k: Key, output: Outputs) {
        if let Some(old) = self.queued.get_mut(k) {
            if *old != output {
                *old = Outputs::Both
            }
        } else {
            self.queued.insert(k, output);
            self.work.push((rank, k));
        }
    }
    pub fn pop(&mut self) -> Option<(Key, Outputs)> {
        let (_, k) = self.work.pop()?;
        Some((k, self.queued.remove(k).expect("queued key")))
    }
}
