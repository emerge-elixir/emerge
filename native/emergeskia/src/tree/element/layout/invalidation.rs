use indexmap::IndexMap;
use std::cmp::Reverse;

use crate::tree::element::Key;
use super::Phase;
 
#[derive(Default, Debug)]
struct WorkSet {
    roots: IndexMap<Key, usize>,
}

impl WorkSet {
    fn insert(&mut self, key: Key, depth: usize) {
        // shift_remove + insert ensures append.
        self.roots.shift_remove(&key);
        self.roots.insert(key, depth);
    }

    fn remove(&mut self, key: Key) {
        // shift_remove preserves relative order.
        self.roots.shift_remove(&key);
    }

    fn pop(&mut self) -> Option<Key> {
        self.roots.pop().map(|(key, _depth)| key)
    }

    fn sort(&mut self, descending: bool) {
        if descending {
            self.roots.sort_by_key(|_key, depth| *depth);
        } else {
            self.roots.sort_by_key(|_key, depth| Reverse(*depth));
        }
    }

    fn clear(&mut self) {
        self.roots.clear();
    }
}

#[derive(Debug)]
pub(super) struct Invalidation {
    phases: [WorkSet; Phase::COUNT],
}

impl Default for Invalidation {
    fn default() -> Self {
        Self {
            phases: std::array::from_fn(|_| WorkSet::default()),
        }
    }
}

impl Invalidation {
    pub(super) fn dirty(&mut self, phase: Phase, key: Key, depth: usize) {
        self.phases[phase.index()].insert(key, depth);
    }

    pub(super) fn clean(&mut self, phase: Phase, key: Key) {
        self.phases[phase.index()].remove(key);
    }

    pub(super) fn pop(&mut self, phase: Phase) -> Option<Key> {
        self.phases[phase.index()].pop()
    }

    pub(crate) fn sort(&mut self, phase: Phase) {
        self.phases[phase.index()].sort(phase == Phase::Measure);
    }

    pub(super) fn clear(&mut self) {
        self.phases.iter_mut().for_each(WorkSet::clear);
    }
}
