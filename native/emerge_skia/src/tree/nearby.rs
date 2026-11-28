//! One ownership order. Layout borrows only the ordinary prefix. A tag in
//! the mounted suffix describes the edge, never the identity of its root.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum Slot {
    #[default]
    Behind = 1,
    Above,
    Right,
    Below,
    Left,
    Front,
}
impl Slot {
    pub fn decode(tag: u8) -> Result<Self, String> {
        match tag {
            1 => Ok(Self::Behind),
            2 => Ok(Self::Above),
            3 => Ok(Self::Right),
            4 => Ok(Self::Below),
            5 => Ok(Self::Left),
            6 => Ok(Self::Front),
            _ => Err(format!("invalid Nearby slot {tag}")),
        }
    }
    pub fn escapes(self) -> bool {
        self != Self::Behind
    }
    pub fn host_width(self) -> bool {
        !matches!(self, Self::Left | Self::Right)
    }
    pub fn host_height(self) -> bool {
        !matches!(self, Self::Above | Self::Below)
    }
    pub fn offset(self, host: Rect, size: layers::Size, a: &Attrs) -> [f32; 2] {
        let [x, y] = a.align.map(Option::unwrap_or_default);
        [
            match self {
                Self::Left => -size.w,
                Self::Right => host.width,
                _ => x.offset(host.width, size.w),
            },
            match self {
                Self::Above => -size.h,
                Self::Below => host.height,
                _ => y.offset(host.height, size.h),
            },
        ]
    }
    pub fn reads_placement(self, dw: bool, dh: bool, a: &Attrs) -> bool {
        let [x, y] = a.align.map(Option::unwrap_or_default);
        (dw && (self == Self::Right || (self.host_width() && x != Alignment::Start)))
            || (dh
                && (self == Self::Below
                    || (self.host_height()
                        && (a.sizing.height.weight().is_some() || y != Alignment::Start))))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Edges<'a, T> {
    pub keys: &'a [T],
    pub tags: &'a [Slot],
}
impl<'a, T: Copy> Edges<'a, T> {
    pub fn children(self) -> &'a [T] {
        &self.keys[..self.keys.len() - self.tags.len()]
    }
    pub fn nearby(self) -> impl DoubleEndedIterator<Item = (Slot, T)> + 'a {
        self.tags
            .iter()
            .copied()
            .zip(self.keys[self.children().len()..].iter().copied())
    }
    pub fn owned(self) -> impl Iterator<Item = (Option<Slot>, T)> + 'a {
        self.children()
            .iter()
            .map(|&c| (None, c))
            .chain(self.nearby().map(|(s, c)| (Some(s), c)))
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct OrderSpan<T> {
    keys: Span<T>,
    tags: Span<Slot>,
}
#[derive(Default)]
pub(super) struct Orders<T> {
    pub keys: Pool<T>,
    pub tags: Pool<Slot>,
}
impl<T: Copy + Default> Orders<T> {
    pub fn ensure(
        &mut self,
        keys: usize,
        tags: usize,
        blocks: usize,
        b: Budget,
    ) -> Result<(), String> {
        self.keys.ensure(keys, blocks, b)?;
        if tags != 0 {
            self.tags.ensure(tags, blocks.min(tags), b)?;
        }
        Ok(())
    }
    pub fn get(&self, s: OrderSpan<T>) -> Edges<'_, T> {
        Edges {
            keys: self.keys.get(&s.keys),
            tags: self.tags.get(&s.tags),
        }
    }
    pub fn release(&mut self, s: OrderSpan<T>) {
        self.keys.release(s.keys);
        self.tags.release(s.tags);
    }
    pub fn insert(
        &mut self,
        len: usize,
        keys: impl Iterator<Item = T>,
        mounted: usize,
        tags: impl Iterator<Item = Slot>,
    ) -> OrderSpan<T> {
        let s = OrderSpan {
            keys: self.keys.alloc(len),
            tags: self.tags.alloc(mounted),
        };
        self.keys
            .write(s.keys)
            .iter_mut()
            .zip(keys)
            .for_each(|(to, k)| *to = k);
        self.tags
            .write(s.tags)
            .iter_mut()
            .zip(tags)
            .for_each(|(to, tag)| *to = tag);
        s
    }
}
