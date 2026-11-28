//! Provision immutable, typed arrays by aggregate element counts, not by
//! guessing the eventual length of a wrapped line. Readers take no locks.
//! Publishing or replacing a reference never copies a neighbour's payload.
use super::{Budget, pool};
use std::{
    fmt,
    ops::{Deref, Index, IndexMut, Range},
    sync::{Arc, Mutex, Weak},
};

#[cfg(test)]
pub(super) mod meter {
    use super::super::{Command, Hit, Scope};
    use skia_safe::Point;
    use std::{any::TypeId, cell::Cell};
    thread_local! { pub static BYTES:Cell<[usize;3]>=const{Cell::new([0;3])}; }
    pub fn write<T: 'static>(len: usize) {
        let t = TypeId::of::<T>();
        let index = if t == TypeId::of::<(Scope, Command)>() {
            0
        } else if t == TypeId::of::<Point>() || t == TypeId::of::<u16>() {
            1
        } else if t == TypeId::of::<Hit>() {
            2
        } else {
            return;
        };
        let mut bytes = BYTES.get();
        bytes[index] += len * size_of::<T>();
        BYTES.set(bytes);
    }
}
type Free<T> = Mutex<Vec<Arc<T>>>;
pub(super) struct Stock<T> {
    free: Option<Arc<Free<T>>>,
    capacity: usize,
}
impl<T> Default for Stock<T> {
    fn default() -> Self {
        Self {
            free: Default::default(),
            capacity: 0,
        }
    }
}
impl<T> Stock<T> {
    pub fn charge(n: u128, extra: u128, b: Budget) -> u128 {
        let n = n + (n * b.0 as u128).div_ceil(100);
        n * (size_of::<T>() as u128 + extra + 6 * size_of::<usize>() as u128)
    }
    pub fn reserve(
        &mut self,
        n: usize,
        mut make: impl FnMut() -> Result<T, String>,
    ) -> Result<(), String> {
        if n > self.capacity {
            let mut bin = self
                .free
                .get_or_insert_with(Default::default)
                .lock()
                .unwrap();
            pool::reserve(&mut bin, n)?;
            while self.capacity < n {
                bin.push(Arc::new(make()?));
                self.capacity += 1;
            }
        }
        Ok(())
    }
    pub fn issue(&self, write: impl FnOnce(&mut T)) -> Ref<T>
    where
        T: Recycle,
    {
        let free = self.free.as_ref().expect("admitted storage");
        let mut a = free
            .lock()
            .unwrap()
            .pop()
            .expect("reserved immutable version");
        write(Arc::get_mut(&mut a).expect("unissued slot"));
        Ref {
            value: Some(a),
            free: Arc::downgrade(free),
        }
    }
}
pub(super) trait Recycle {
    fn recycle(&mut self);
}
impl<T> Recycle for Vec<T> {
    fn recycle(&mut self) {
        self.clear()
    }
}
pub(super) struct Ref<T: Recycle> {
    value: Option<Arc<T>>,
    free: Weak<Free<T>>,
}
impl<T: Recycle> Clone for Ref<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            free: self.free.clone(),
        }
    }
}
impl<T: Recycle + fmt::Debug> fmt::Debug for Ref<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
impl<T: Recycle + PartialEq> PartialEq for Ref<T> {
    fn eq(&self, b: &Self) -> bool {
        self.same(b) || **self == **b
    }
}
impl<T: Recycle> Deref for Ref<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_deref().unwrap()
    }
}
impl<T: Recycle> Ref<T> {
    pub fn same(&self, b: &Self) -> bool {
        Arc::ptr_eq(self.value.as_ref().unwrap(), b.value.as_ref().unwrap())
    }
}
impl<T: Recycle> Drop for Ref<T> {
    fn drop(&mut self) {
        let mut a = self.value.take().unwrap();
        let Some(free) = self.free.upgrade() else {
            return;
        };
        // Serialize decrements, not reads. A last reference cannot race a clone.
        // Clear outgoing references *outside* the lock (possibly the same bank).
        let bin = free.lock().unwrap();
        if Arc::strong_count(&a) != 1 {
            drop(a);
            return;
        }
        drop(bin);
        Arc::get_mut(&mut a).unwrap().recycle();
        free.lock().unwrap().push(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simultaneous_final_readers_return_every_reserved_slot() {
        let mut stock = Stock::default();
        stock
            .reserve(4, || Ok(Vec::<usize>::with_capacity(1)))
            .unwrap();
        for _ in 0..32 {
            let a = std::array::from_fn::<_, 4, _>(|i| {
                stock.issue(|v| {
                    assert!(v.is_empty());
                    v.push(i);
                })
            });
            let b = a.clone();
            let start = std::sync::Barrier::new(2);
            let barrier = &start;
            std::thread::scope(|s| {
                s.spawn(move || {
                    barrier.wait();
                    drop(a);
                });
                barrier.wait();
                drop(b);
            });
        }
    }
}
#[derive(Clone)]
enum Slice<T> {
    Empty,
    Leaf(Ref<Vec<T>>),
    Fork(Ref<Fork<T>>),
}
struct Fork<T> {
    split: usize,
    sides: [Slice<T>; 2],
}
impl<T> Recycle for Fork<T> {
    fn recycle(&mut self) {
        self.sides = [Slice::Empty, Slice::Empty]
    }
}
impl<T> Slice<T> {
    fn chunk(&self, i: usize) -> &[T] {
        match self {
            Self::Empty => &[],
            Self::Leaf(v) => &v[i..],
            Self::Fork(n) => {
                if i < n.split {
                    n.sides[0].chunk(i)
                } else {
                    n.sides[1].chunk(i - n.split)
                }
            }
        }
    }
}
pub(super) struct Pages<T, const N: usize> {
    leaves: Stock<Vec<T>>,
    branches: Stock<Fork<T>>,
}
impl<T, const N: usize> Default for Pages<T, N> {
    fn default() -> Self {
        Self {
            leaves: Stock::default(),
            branches: Stock::default(),
        }
    }
}
impl<T: Clone + 'static, const N: usize> Pages<T, N> {
    pub fn charge(records: u128, items: u128, versions: u128, b: Budget) -> u128 {
        let full = items / N as u128;
        Stock::<Vec<T>>::charge(
            versions * (records.min(items) + full),
            (N * size_of::<T>()) as u128,
            b,
        ) + Stock::<Fork<T>>::charge(versions * full, 0, b)
    }
    pub fn reserve(
        &mut self,
        records: usize,
        items: usize,
        versions: usize,
        b: Budget,
    ) -> Result<(), String> {
        // There is at most one partial leaf per nonempty recording. Every
        // additional leaf consumed N items; a binary tree has leaves-1 forks.
        let full = items / N;
        self.leaves
            .reserve(b.capacity(versions * (records.min(items) + full)), || {
                let mut v = Vec::new();
                pool::reserve(&mut v, N)?;
                Ok(v)
            })?;
        self.branches.reserve(b.capacity(versions * full), || {
            Ok(Fork {
                split: 0,
                sides: [Slice::Empty, Slice::Empty],
            })
        })
    }
    fn store(&self, v: &[T]) -> Slice<T> {
        if v.is_empty() {
            return Slice::Empty;
        }
        if v.len() <= N {
            return Slice::Leaf(self.leaves.issue(|b| {
                #[cfg(test)]
                meter::write::<T>(v.len());
                b.extend_from_slice(v)
            }));
        }
        let split = (v.len().div_ceil(N) / 2) * N;
        Slice::Fork(self.branches.issue(|p| {
            p.split = split;
            p.sides = [self.store(&v[..split]), self.store(&v[split..])];
        }))
    }
    pub fn save(&self, v: &[T]) -> Frozen<T> {
        Frozen {
            len: v.len(),
            data: self.store(v),
        }
    }
    pub fn freeze(&self, b: &Buffer<T>) -> Buffer<T> {
        Buffer::Frozen(self.save(b.vec()))
    }
}
#[derive(Clone)]
pub(super) struct Frozen<T> {
    len: usize,
    data: Slice<T>,
}
impl<T> Frozen<T> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn at(&self, i: usize) -> &T {
        &self.data.chunk(i)[0]
    }
}
pub(super) enum Buffer<T> {
    Build(Vec<T>),
    Frozen(Frozen<T>),
}
impl<T> Default for Buffer<T> {
    fn default() -> Self {
        Self::Build(Vec::new())
    }
}
impl<T> Buffer<T> {
    pub fn vec(&self) -> &Vec<T> {
        match self {
            Self::Build(v) => v,
            _ => unreachable!("builder"),
        }
    }
    pub fn edit(&mut self) -> &mut Vec<T> {
        match self {
            Self::Build(v) => v,
            _ => unreachable!("immutable recording"),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Build(v) => v.len(),
            Self::Frozen(v) => v.len,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn capacity(&self) -> usize {
        self.vec().capacity()
    }
    pub fn push(&mut self, v: T)
    where
        T: 'static,
    {
        #[cfg(test)]
        meter::write::<T>(1);
        self.edit().push(v)
    }
    pub fn clear(&mut self) {
        self.edit().clear()
    }
    pub fn chunk(&self, r: Range<usize>) -> &[T] {
        if r.is_empty() {
            return &[];
        }
        match self {
            Self::Build(v) => &v[r],
            Self::Frozen(v) => {
                let s = v.data.chunk(r.start);
                &s[..s.len().min(r.len())]
            }
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        (0..self.len()).map(|i| &self[i])
    }
}
impl<T> Index<usize> for Buffer<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        match self {
            Self::Build(v) => &v[i],
            Self::Frozen(v) => &v.data.chunk(i)[0],
        }
    }
}
impl<T> IndexMut<usize> for Buffer<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        &mut self.edit()[i]
    }
}
impl<T: PartialEq> PartialEq for Buffer<T> {
    fn eq(&self, b: &Self) -> bool {
        match (self, b) {
            (Self::Build(l), Self::Build(r)) => l == r,
            _ => self.len() == b.len() && self.iter().eq(b.iter()),
        }
    }
}
impl<T: fmt::Debug> fmt::Debug for Buffer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
