//! Shared intrinsic box values, independent of mount identity. Collision or payload
//! pressure is a miss, never evidence of equality. A slot owns a snapshot of
//! the settled child sizes, not an occurrence or another copy of child order.
use super::{layers::Demand, *};
use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy)]
struct Entry {
    shape: Shape,
    style: BoxStyle,
    scale: f32,
    children: Span<Demand>,
    value: Demand,
}
#[derive(Default)]
pub(super) struct Intrinsic {
    slots: Vec<Option<Entry>>,
    inputs: Pool<Demand>,
    #[cfg(test)]
    pub misses: usize,
}
impl Intrinsic {
    pub fn reserve(&mut self, n: usize) -> Result<(), String> {
        let cap = 2 * n + 1;
        pool::reserve(&mut self.slots, cap)?;
        self.slots.resize(cap, None);
        self.inputs.ensure(n, cap, Budget(0))
    }
    fn slot(
        &self,
        shape: Shape,
        style: BoxStyle,
        scale: f32,
        children: impl Iterator<Item = Demand>,
    ) -> usize {
        let mut h = DefaultHasher::new();
        (shape as u8, scale.to_bits()).hash(&mut h);
        for length in [style.width, style.height] {
            std::mem::discriminant(&length).hash(&mut h);
            match length {
                Length::Px(px) => px.to_bits().hash(&mut h),
                Length::BoundedContent(bounds) | Length::BoundedFill { bounds, .. } => {
                    (bounds.floor.to_bits(), bounds.cap.to_bits()).hash(&mut h)
                }
                _ => (),
            }
        }
        for n in style.padding.into_iter().chain(style.spacing) {
            n.to_bits().hash(&mut h);
        }
        for s in children {
            (s.w.to_bits(), s.h.to_bits(), s.min_w.to_bits()).hash(&mut h);
        }
        h.finish() as usize % self.slots.len()
    }
    pub fn lookup(
        &self,
        shape: Shape,
        style: BoxStyle,
        scale: f32,
        children: impl Iterator<Item = Demand> + Clone,
    ) -> (usize, Option<Demand>) {
        let i = self.slot(shape, style, scale, children.clone());
        (
            i,
            self.slots[i]
                .filter(|s| {
                    s.shape == shape
                        && s.style == style
                        && s.scale == scale
                        && self.inputs.get(&s.children).iter().copied().eq(children)
                })
                .map(|s| s.value),
        )
    }
    pub fn store(
        &mut self,
        i: usize,
        shape: Shape,
        style: BoxStyle,
        scale: f32,
        children: impl ExactSizeIterator<Item = Demand>,
        value: Demand,
    ) {
        #[cfg(test)]
        {
            self.misses += 1;
        }
        if let Some(old) = self.slots[i].take() {
            self.inputs.release(old.children);
        }
        if children.len() <= self.inputs.capacity() - self.inputs.len() {
            let inputs = self.inputs.alloc(children.len());
            for (to, from) in self.inputs.write(inputs).iter_mut().zip(children) {
                *to = from;
            }
            self.slots[i] = Some(Entry {
                shape,
                style,
                scale,
                children: inputs,
                value,
            });
        }
    }
}
