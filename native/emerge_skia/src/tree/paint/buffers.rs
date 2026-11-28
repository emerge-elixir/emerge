//! One mutable builder and a fixed bank of immutable readers. Publishing swaps
//! vector headers, not their contents. Detached raster needs only one reader;
//! live renderers can keep an older installation while preparing its successor.
use super::{Budget, pool};
use std::{array, sync::Arc};

pub(super) trait Packed<const N: usize>: Default {
    fn capacities(&self) -> [usize; N];
    fn reserve(&mut self, capacities: [usize; N]) -> Result<(), String>;
    fn clear(&mut self);
}
struct Slot<T> {
    value: Arc<T>,
    // A pinned reader can't be grown during edit admission. Reserve its
    // replacement now, not when the next fitting layout needs the builder.
    growth: Option<T>,
}
impl<T: Default> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: Arc::default(),
            growth: None,
        }
    }
}
pub(super) struct Buffers<T, const N: usize> {
    pub build: T,
    slots: Box<[Slot<T>]>,
    current: usize,
}
impl<T: Packed<N>, const N: usize> Default for Buffers<T, N> {
    fn default() -> Self {
        Self::new(1)
    }
}
impl<T: Packed<N>, const N: usize> Buffers<T, N> {
    pub fn new(count: usize) -> Self {
        Self {
            build: T::default(),
            slots: (0..count).map(|_| Slot::default()).collect(),
            current: 0,
        }
    }
    pub fn readers(&self) -> usize {
        self.slots.len()
    }
    pub fn current(&self) -> &Arc<T> {
        &self.slots[self.current].value
    }
    pub fn reserve(&mut self, required: [usize; N], budget: Budget) -> Result<(), String> {
        let old = self.build.capacities();
        let caps = array::from_fn(|i| {
            if required[i] > old[i] {
                budget.capacity(required[i])
            } else {
                old[i]
            }
        });
        self.build.reserve(caps)?;
        for slot in &mut self.slots {
            let fits = slot
                .value
                .capacities()
                .into_iter()
                .zip(caps)
                .all(|(have, need)| have >= need);
            if !fits {
                if let Some(value) = Arc::get_mut(&mut slot.value) {
                    if let Some(reserved) = &mut slot.growth {
                        // Don't change the contents of the current installation.
                        reserved.reserve(caps)?;
                    } else {
                        value.reserve(caps)?;
                    }
                } else {
                    slot.growth.get_or_insert_with(T::default).reserve(caps)?;
                }
            }
        }
        Ok(())
    }
    pub fn available(&self) -> bool {
        self.slots.iter().any(|s| Arc::strong_count(&s.value) == 1)
    }
    pub fn publish(&mut self) {
        // Only this owner issues handles. Once unique, a slot cannot gain a
        // reader until we publish it ourselves. No Weak handles escape.
        let (index, slot) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| Arc::strong_count(&s.value) == 1)
            .expect("reserved frame slot");
        let value = Arc::get_mut(&mut slot.value).expect("exclusive frame builder");
        if let Some(reserved) = slot.growth.take() {
            *value = reserved;
        }
        std::mem::swap(value, &mut self.build);
        self.build.clear();
        self.current = index;
        self.reclaim();
    }
    pub fn reclaim(&mut self) {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if i != self.current
                && let Some(value) = Arc::get_mut(&mut slot.value)
            {
                value.clear();
            }
        }
    }
}

impl Packed<8> for super::Drawing {
    fn capacities(&self) -> [usize; 8] {
        let [styles, bytes] = self.materials.capacities();
        [
            self.commands.capacity(),
            self.ids.capacity(),
            self.positions.capacity(),
            self.clips.capacity(),
            styles,
            bytes,
            self.planes.0.capacity(),
            self.shadow_guards.capacity(),
        ]
    }
    fn reserve(
        &mut self,
        [commands, ids, pos, clips, styles, bytes, planes, guards]: [usize; 8],
    ) -> Result<(), String> {
        pool::reserve(&mut self.shadow_guards, guards)?;
        pool::reserve(&mut self.planes.0, planes)?;
        pool::reserve(&mut self.commands, commands)?;
        pool::reserve(&mut self.ids, ids)?;
        pool::reserve(&mut self.positions, pos)?;
        self.clips.reserve(clips, Budget(0))?;
        self.materials.reserve(styles, bytes, Budget(0))
    }
    fn clear(&mut self) {
        super::Drawing::clear(self)
    }
}
impl Packed<4> for super::Hits {
    fn capacities(&self) -> [usize; 4] {
        [
            self.items.capacity(),
            self.clips.capacity(),
            self.planes.0.capacity(),
            self.scrolls.capacity(),
        ]
    }
    fn reserve(&mut self, [items, clips, planes, scrolls]: [usize; 4]) -> Result<(), String> {
        self.scrolls
            .try_reserve(scrolls.saturating_sub(self.scrolls.len()))
            .map_err(pool::capacity_error)?;
        pool::reserve(&mut self.planes.0, planes)?;
        pool::reserve(&mut self.items, items)?;
        self.clips.reserve(clips, Budget(0))
    }
    fn clear(&mut self) {
        // HashMap::clear can touch the capacity of every admitted port,
        // even if the old sealed stream contained just one wheel region.
        if self.scrolls.capacity() <= self.items.len() {
            self.scrolls.clear();
        } else if !self.scrolls.is_empty() {
            for h in self.items.iter().filter(|h| h.kind == super::HitKind::Port) {
                self.scrolls.remove(&h.key);
            }
        }
        self.items.clear();
        self.clips.clear();
        self.planes.0.clear();
    }
}
