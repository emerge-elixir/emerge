//! Typed handles into initialized storage. Moving a payload during compaction
//! changes only its block metadata; direct readers resolve their owner's handle.
use slotmap::{SlotMap, new_key_type};
use std::{marker::PhantomData, ops::Range};
new_key_type! {struct Block;}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Span<T>(Option<Block>, PhantomData<T>);
#[derive(Default)]
pub(super) struct Pool<T> {
    data: Vec<T>,
    blocks: SlotMap<Block, Range<usize>>,
    holes: Vec<Range<usize>>,
    scratch: Vec<(usize, Block)>,
    used: usize,
}
// Headroom is spent, not maintained as a free-space percentage.
#[derive(Clone, Copy)]
pub(super) struct Budget(pub usize);
impl Default for Budget {
    fn default() -> Self {
        Self(80)
    }
}
impl Budget {
    pub fn capacity(self, n: usize) -> usize {
        // Use wider arithmetic on 32-bit targets. An unrepresentable bound
        // becomes a failed try_reserve, never a wrapped reserve request.
        usize::try_from(n as u128 + (n as u128 * self.0 as u128).div_ceil(100))
            .unwrap_or(usize::MAX)
    }
}
pub(super) fn capacity_error(_: std::collections::TryReserveError) -> String {
    "tree workspace capacity exceeded".into()
}
pub(super) fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<(), String> {
    if n > v.capacity() {
        v.try_reserve_exact(n - v.len()).map_err(capacity_error)?
    }
    Ok(())
}
impl<T: Copy + Default> Pool<T> {
    pub fn len(&self) -> usize {
        self.used
    }
    pub fn ensure(&mut self, slots: usize, blocks: usize, budget: Budget) -> Result<(), String> {
        if blocks > self.blocks.capacity() {
            self.blocks
                .try_reserve(budget.capacity(blocks) - self.blocks.len())
                .map_err(capacity_error)?
        }
        if blocks + 1 > self.holes.capacity() {
            reserve(&mut self.holes, budget.capacity(blocks).saturating_add(1))?
        }
        if blocks > self.scratch.capacity() {
            reserve(&mut self.scratch, budget.capacity(blocks))?
        }
        if slots > self.data.len() {
            let end = budget.capacity(slots);
            let start = self.data.len();
            reserve(&mut self.data, end)?;
            self.data.resize(end, T::default());
            self.vacate(start..end);
        }
        Ok(())
    }
    fn vacate(&mut self, mut span: Range<usize>) {
        let i = self.holes.partition_point(|h| h.start < span.start);
        if i < self.holes.len() && self.holes[i].start == span.end {
            span.end = self.holes.remove(i).end
        }
        if i != 0 && self.holes[i - 1].end == span.start {
            self.holes[i - 1].end = span.end
        } else {
            self.holes.insert(i, span)
        }
    }
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.holes.clear();
        self.used = 0;
        if !self.data.is_empty() {
            self.holes.push(0..self.data.len())
        }
    }
    pub fn alloc(&mut self, n: usize) -> Span<T> {
        if n == 0 {
            return Span::default();
        }
        let i = self
            .holes
            .iter()
            .position(|h| h.len() >= n)
            .unwrap_or_else(|| {
                assert!(
                    n <= self.data.len() - self.used,
                    "capacity was not reserved"
                );
                self.scratch.clear();
                self.scratch
                    .extend(self.blocks.iter().map(|(k, r)| (r.start, k)));
                self.scratch.sort_unstable();
                let mut cursor = 0;
                for &(_, k) in &self.scratch {
                    let r = &mut self.blocks[k];
                    self.data.copy_within(r.clone(), cursor);
                    let end = cursor + r.len();
                    *r = cursor..end;
                    cursor = end;
                }
                self.holes.clear();
                self.holes.push(cursor..self.data.len());
                0
            });
        let h = &mut self.holes[i];
        let start = h.start;
        h.start += n;
        if h.start == h.end {
            self.holes.remove(i);
        }
        self.used += n;
        Span(Some(self.blocks.insert(start..start + n)), PhantomData)
    }
    pub fn insert(&mut self, values: &[T]) -> Span<T> {
        let s = self.alloc(values.len());
        self.write(s).copy_from_slice(values);
        s
    }
    fn range(&self, span: Span<T>) -> Range<usize> {
        span.0.map_or(0..0, |b| self.blocks[b].clone())
    }
    pub fn get(&self, span: &Span<T>) -> &[T] {
        &self.data[self.range(*span)]
    }
    pub fn write(&mut self, span: Span<T>) -> &mut [T] {
        let r = self.range(span);
        &mut self.data[r]
    }
    pub fn release(&mut self, span: Span<T>) {
        if let Some(b) = span.0 {
            let hole = self.blocks.remove(b).expect("live payload");
            self.used -= hole.len();
            self.vacate(hole)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn last_slot_and_contiguous_reuse() {
        let mut p = Pool::default();
        p.ensure(100, 8, Budget(80)).unwrap();
        assert_eq!(p.data.len(), 180);
        let a = p.insert(&[1u8; 70]);
        let b = p.insert(&[2; 60]);
        let c = p.insert(&[3; 50]);
        p.release(a);
        p.release(c);
        let replacement = p.insert(&[4; 120]);
        assert_eq!(p.get(&b), [2; 60]);
        assert_eq!(p.get(&replacement), [4; 120]);
        assert_eq!(p.len(), 180);
    }
}
