//! Draw inputs already compiled against one fitted bitmap. A larger demand
//! must re-invite just this source's readers to the coalesced request. This is
//! a dependency list, not another workset; scrolling never queries it when the
//! requested pixels already fit.
use super::*;

struct Reader {
    source: usize,
    prev: Option<Key>,
    next: Option<Key>,
    prepared: u64,
}
#[derive(Default)]
pub(super) struct Readers {
    heads: HashMap<usize, Key>,
    sites: HashMap<Key, Reader>,
}
impl Readers {
    pub fn reserve(&mut self, n: usize) -> Result<(), String> {
        reserve_index(&mut self.heads, n)?;
        reserve_index(&mut self.sites, n)
    }
    pub fn charge(n: u128) -> u128 {
        // Half occupancy for turnover, plus controls and rounding up to a
        // power-of-two bucket array at the 7/8 load factor (both directories).
        4 * (2 * n) * (size_of::<(Key, Reader)>() + size_of::<(usize, Key)>() + 2) as u128
    }
    pub fn forget(&mut self, k: Key) {
        let Some(old) = self.sites.remove(&k) else {
            return;
        };
        if let Some(next) = old.next {
            self.sites.get_mut(&next).unwrap().prev = old.prev;
        }
        if let Some(prev) = old.prev {
            self.sites.get_mut(&prev).unwrap().next = old.next;
        } else if let Some(next) = old.next {
            *self.heads.get_mut(&old.source).unwrap() = next;
        } else {
            self.heads.remove(&old.source);
        }
    }
    pub fn check(&mut self, k: Key, e: &Elements) {
        if self.sites.get(&k).is_some_and(|r| {
            !e.images.bound(k)
                || !matches!(e.images.source(k), Source::Sized(s) if s.id()==r.source)
        }) {
            self.forget(k);
        }
    }
    pub fn preparing(&mut self, k: Key, epoch: u64) {
        if let Some(r) = self.sites.get_mut(&k) {
            r.prepared = epoch;
        }
    }
    pub fn bound(&mut self, k: Key, source: usize, prepared: u64) {
        if self.sites.get(&k).is_some_and(|r| r.source == source) {
            return;
        }
        self.forget(k);
        let next = self.heads.insert(source, k);
        if let Some(n) = next {
            self.sites.get_mut(&n).unwrap().prev = Some(k);
        }
        self.sites.insert(
            k,
            Reader {
                source,
                prev: None,
                next,
                prepared,
            },
        );
    }
    pub fn reopen(&mut self, source: usize, epoch: u64, mut invalidate: impl FnMut(Key)) -> bool {
        let mut changed = false;
        while let Some(&k) = self.heads.get(&source) {
            if self.sites[&k].prepared != epoch {
                invalidate(k);
                changed = true;
            }
            self.forget(k);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_fitted_sources_and_remounting_readers_spends_only_admitted_buckets() {
        let n = 112;
        let mut fits = Fits::default();
        fits.reserve(n, Budget(0)).unwrap();
        fits.reserve_versions(n, 3, Budget(0)).unwrap();
        let capacities = |f: &Fits| {
            [
                f.index.capacity(),
                f.readers.heads.capacity(),
                f.readers.sites.capacity(),
            ]
        };
        let admitted = capacities(&fits);
        let key =
            |g: u64, i| slotmap::KeyData::from_ffi(((2 * g + 1) << 32) | (i as u64 + 1)).into();
        for g in 1..=100u64 {
            for i in 0..n {
                if g > 1 {
                    fits.readers.forget(key(g - 1, i));
                }
                let source = (g as usize * n + i + 1) * 64;
                fits.readers.bound(key(g, i), source, g);
                // A fresh coalesced set each time, as after Fits::reset.
                fits.index.insert(source, i);
            }
            for i in 0..n {
                fits.index.remove(&((g as usize * n + i + 1) * 64));
            }
            fits.reserve(n, Budget(0)).unwrap();
            fits.reserve_versions(n, 3, Budget(0)).unwrap();
            let actual = capacities(&fits);
            assert!(
                actual
                    .into_iter()
                    .zip(admitted)
                    .all(|(used, paid)| used <= paid),
                "at generation {g}: {actual:?} exceeds {admitted:?}"
            );
        }
    }
}
