//! A pass orders work by depth, not by sibling key. Each nonempty depth is a
//! stack of mounts; its links double as the generational membership map.
//! Depth is fixed during compute. Wire mutation never seeds a workset.
use super::{Key, MAX_DEPTH, paint::Outputs, pool};
use slotmap::SecondaryMap;

const BITS: usize = u64::BITS as usize;

#[derive(Clone, Copy)]
pub(super) enum Direction {
    ShallowFirst,
    DeepFirst,
}

struct Entry {
    next: Option<Key>,
    outputs: Outputs,
}

#[derive(Default)]
pub(super) struct WorkSet {
    heads: Vec<Option<Key>>,
    queued: SecondaryMap<Key, Entry>,
    active: [u64; (MAX_DEPTH + 1).div_ceil(BITS)],
    // Both extremes are valid whenever queued is nonempty.
    shallow: usize,
    deep: usize,
}

impl WorkSet {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }
    pub fn reserve(&mut self, keys: usize, occupancy: usize) -> Result<(), String> {
        self.queued
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        // N mounts can occupy at most N consecutive tree depths. Link space is
        // shared across buckets, so a fitting wide -> deep replacement still fits.
        let depths = occupancy.min(MAX_DEPTH + 1);
        if depths > self.heads.len() {
            pool::reserve(&mut self.heads, depths)?;
            self.heads.resize(depths, None);
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        // A full replacement can discard pending work. A drained pass has
        // nothing to reset; never sweep retained key slots or empty buckets.
        while self.pop(Direction::ShallowFirst).is_some() {}
    }

    pub fn push(&mut self, depth: usize, k: Key, outputs: Outputs) {
        let empty = self.queued.is_empty();
        let Some(slot) = self.queued.entry(k) else {
            return;
        };
        let entry = slot.or_insert_with(|| {
            let next = self.heads[depth].replace(k);
            if next.is_none() {
                self.active[depth / BITS] |= 1 << (depth % BITS);
                (self.shallow, self.deep) = if empty {
                    (depth, depth)
                } else {
                    (self.shallow.min(depth), self.deep.max(depth))
                };
            }
            Entry { next, outputs }
        });
        if entry.outputs != outputs {
            entry.outputs = Outputs::Both;
        }
    }

    pub fn pop(&mut self, direction: Direction) -> Option<(Key, Outputs)> {
        if self.queued.is_empty() {
            return None;
        }
        let depth = match direction {
            Direction::ShallowFirst => self.shallow,
            Direction::DeepFirst => self.deep,
        };
        let head = &mut self.heads[depth];
        let k = head.expect("active depth has a head");
        let entry = self.queued.remove(k).expect("queued mount");
        *head = entry.next;
        if head.is_none() {
            self.active[depth / BITS] &= !(1 << (depth % BITS));
            if !self.queued.is_empty() {
                // Only an emptied extreme needs a scan. Skip whole empty
                // words; popping another sibling never takes this path.
                let first = self.shallow / BITS;
                let mut words = self.active[first..=self.deep / BITS]
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|&(_, w)| w != 0);
                match direction {
                    Direction::ShallowFirst => {
                        let (i, w) = words.next().expect("another active depth");
                        self.shallow = (first + i) * BITS + w.trailing_zeros() as usize;
                    }
                    Direction::DeepFirst => {
                        let (i, w) = words.next_back().expect("another active depth");
                        self.deep = (first + i) * BITS + (BITS - 1 - w.leading_zeros() as usize);
                    }
                }
            }
        }
        Some((k, entry.outputs))
    }
}

#[cfg(test)]
mod tests;
