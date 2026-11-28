//! A recording references its gradient's owner even when it inherits the
//! gradient from outside its subtree. Skia borrows contiguous bytes; neither
//! compiling another span nor replaying a page joins or duplicates them.
use super::*;
use crate::tree::decoration::Bytes;

#[derive(Default, PartialEq)]
pub(in crate::tree::paint) struct Swatch {
    style: styled::Material,
    raw: Option<Ref<Vec<u8>>>,
}
impl Recycle for Swatch {
    fn recycle(&mut self) {
        self.raw = None;
    }
}
impl Swatch {
    pub(in crate::tree::paint) fn parts(&self) -> (styled::Material, &[u8]) {
        (self.style, self.raw.as_deref().map_or(&[], Vec::as_slice))
    }
    pub(in crate::tree::paint) fn matches(&self, e: &Elements, k: Key) -> bool {
        let mut start = 0;
        let mut same = true;
        let (_, saved) = self.parts();
        let style = styled::Material::from_owner(e, k, |span| {
            if span.len == 0 {
                return Bytes::default();
            }
            let b = Bytes {
                start,
                len: span.len,
            };
            start += b.len;
            same &= saved.get(b.start as usize..start as usize) == Some(span.get(e.raw(k)));
            b
        });
        same && style == self.style
    }
}
#[derive(Default)]
pub(in crate::tree::paint) struct Swatches {
    versions: Stock<Swatch>,
    sizes: Vec<Stock<Vec<u8>>>,
}
impl Swatches {
    pub fn reserve(
        &mut self,
        styles: usize,
        bytes: usize,
        versions: usize,
        b: Budget,
    ) -> Result<(), String> {
        self.versions
            .reserve(b.capacity(versions * styles), || Ok(Swatch::default()))?;
        if bytes == 0 {
            return Ok(());
        }
        let levels = 1 + bytes.next_power_of_two().ilog2() as usize;
        pool::reserve(&mut self.sizes, levels)?;
        self.sizes
            .resize_with(self.sizes.len().max(levels), Stock::default);
        for (i, stock) in self.sizes[..levels].iter_mut().enumerate() {
            let cap = 1usize << i;
            let count = styles.min(bytes / (cap / 2 + 1));
            stock.reserve(b.capacity(versions * count), || {
                let mut v = Vec::new();
                pool::reserve(&mut v, cap)?;
                Ok(v)
            })?;
        }
        Ok(())
    }
    pub fn resolve(&self, saved: &mut Option<Ref<Swatch>>, e: &Elements, k: Key) -> Ref<Swatch> {
        if let Some(value) = saved
            && value.style.scale == e.slots[k].admitted.scale
        {
            return value.clone();
        }
        let len = e.attrs[k].material_bytes();
        let mut style = None;
        let raw = (len != 0).then(|| {
            self.sizes[len.next_power_of_two().ilog2() as usize].issue(|raw| {
                style = Some(styled::Material::from_owner(e, k, |s| {
                    if s.len == 0 {
                        return Bytes::default();
                    }
                    let start = raw.len() as u32;
                    raw.extend_from_slice(s.get(e.raw(k)));
                    Bytes { start, len: s.len }
                }));
            })
        });
        let version = self.versions.issue(|m| {
            m.raw = raw;
            m.style =
                style.unwrap_or_else(|| styled::Material::from_owner(e, k, |_| Bytes::default()));
        });
        *saved = Some(version.clone());
        version
    }
}
