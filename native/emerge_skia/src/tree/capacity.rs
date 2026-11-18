//! Aggregate text storage bounds. A local edit updates one leaf and its log N
//! ancestors; it never walks the document to size a calculation. Multiple edits
//! before a draw replace their outstanding demands rather than accumulating them.
use super::pool::reserve;
#[derive(Clone, Copy, Default)]
pub(super) struct Bound {
    live_or_next: usize,
    desired: usize,
    overlap: usize,
    dirty_overlap: usize,
}
impl Bound {
    fn join(a: Self, b: Self) -> Self {
        Self {
            live_or_next: a.live_or_next + b.live_or_next,
            desired: a.desired + b.desired,
            overlap: a.overlap.max(b.overlap),
            dirty_overlap: a.dirty_overlap.max(b.dirty_overlap),
        }
    }
}
#[derive(Default)]
pub(super) struct PayloadBounds {
    nodes: Vec<Bound>,
    base: usize,
    pub context_dirty: bool,
}
impl PayloadBounds {
    pub fn reserve(&mut self, capacity: usize) -> Result<(), String> {
        let base = capacity.max(1).next_power_of_two();
        if base > self.base {
            reserve(&mut self.nodes, 2 * base)?;
            self.nodes.resize(2 * base, Bound::default());
            self.nodes.copy_within(self.base..2 * self.base, base);
            self.nodes[base + self.base..].fill(Bound::default());
            for k in (1..base).rev() {
                self.nodes[k] = Bound::join(self.nodes[2 * k], self.nodes[2 * k + 1]);
            }
            self.base = base;
        }
        Ok(())
    }
    pub fn reset(&mut self) {
        self.nodes.fill(Bound::default());
        self.context_dirty = false;
    }
    pub fn leaf(&self, i: usize) -> Bound {
        self.nodes[self.base + i]
    }
    pub fn set(&mut self, i: usize, old: usize, next: usize, dirty: bool) {
        self.replace(
            i,
            Bound {
                live_or_next: old.max(next),
                desired: next,
                overlap: old.min(next),
                dirty_overlap: if dirty { old.min(next) } else { 0 },
            },
        )
    }
    pub fn replace(&mut self, i: usize, value: Bound) {
        let mut k = self.base + i;
        self.nodes[k] = value;
        while k > 1 {
            k /= 2;
            self.nodes[k] = Bound::join(self.nodes[2 * k], self.nodes[2 * k + 1])
        }
    }
    pub fn peak(&self) -> usize {
        let b = self.nodes[1];
        b.live_or_next
            + if self.context_dirty {
                b.overlap
            } else {
                b.dirty_overlap
            }
    }
    pub fn context_peak(&self) -> usize {
        let b = self.nodes[1];
        b.live_or_next + b.overlap
    }
    pub fn desired(&self) -> usize {
        self.nodes[1].desired
    }
}
