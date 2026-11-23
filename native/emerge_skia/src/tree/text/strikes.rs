//! Only actually used family/size pairs need device metrics. Keep a small
//! warm set for zooming back, never the history of every scale of a viewport.
use super::*;
use crate::tree::{pool, scale::Inputs};
use std::collections::HashMap;

const TABLE_BYTES: usize = 64 * 1024 * 1024;
const WARM: usize = 8;
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::tree) struct StrikeKey(Face, u32);
impl StrikeKey {
    pub(super) fn new(face: Face, size: f32) -> Self {
        Self(face, size.to_bits())
    }
}
impl From<Inputs> for StrikeKey {
    fn from(i: Inputs) -> Self {
        Self::new(i.face, i.font_size())
    }
}
pub(in crate::tree) type StrikeDelta = (Option<StrikeKey>, Option<StrikeKey>);
#[derive(Default)]
pub(in crate::tree) struct Strikes {
    entries: HashMap<StrikeKey, (Strike, usize)>,
    warm: Vec<StrikeKey>,
    bytes: usize,
}
impl Strikes {
    fn bytes(s: &Strike) -> usize {
        s.widths.capacity() * size_of::<f32>() + s.bounds.capacity() * size_of::<skia_safe::Rect>()
    }
    fn forget(&mut self, key: StrikeKey) {
        let (strike, refs) = self.entries.remove(&key).expect("resident strike");
        debug_assert_eq!(refs, 0);
        self.bytes -= Self::bytes(&strike);
    }
    pub(in crate::tree) fn prepare(
        &mut self,
        deltas: &[StrikeDelta],
        added: &mut Vec<StrikeKey>,
    ) -> Result<(), String> {
        self.prepare_within(deltas, added, TABLE_BYTES)
    }
    fn prepare_within(
        &mut self,
        deltas: &[StrikeDelta],
        added: &mut Vec<StrikeKey>,
        limit: usize,
    ) -> Result<(), String> {
        if deltas.is_empty() {
            return Ok(());
        }
        pool::reserve(&mut self.warm, WARM)?;
        added.clear();
        let result = deltas
            .iter()
            .filter_map(|&(_, new)| new)
            .try_for_each(|key| {
                if !self.entries.contains_key(&key) {
                    self.entries.try_reserve(1).map_err(pool::capacity_error)?;
                    let strike = Strike::new(key.0, f32::from_bits(key.1), |required| {
                        // Under pressure only: evict truly optional history, not a
                        // warm key promised to another reader in this same batch.
                        while self.bytes + required > limit {
                            let i = self
                                .warm
                                .iter()
                                .position(|k| !deltas.iter().any(|&(_, new)| new == Some(*k)))
                                .ok_or("font strike workspace capacity exceeded")?;
                            let victim = self.warm.remove(i);
                            self.forget(victim);
                        }
                        Ok(())
                    })?;
                    self.bytes += Self::bytes(&strike);
                    self.entries.insert(key, (strike, 0));
                    added.push(key);
                }
                Ok(())
            });
        if result.is_err() {
            for key in added.drain(..) {
                self.forget(key);
            }
        }
        result
    }
    pub(in crate::tree) fn commit(&mut self, deltas: &[StrikeDelta]) {
        // Claim before releasing: moving the last reader or swapping two
        // families cannot evict a strike another text is about to acquire.
        for key in deltas.iter().filter_map(|&(_, new)| new) {
            let entry = self.entries.get_mut(&key).expect("prepared strike");
            if entry.1 == 0
                && let Some(i) = self.warm.iter().position(|v| *v == key)
            {
                self.warm.swap_remove(i);
            }
            entry.1 += 1;
        }
        for key in deltas.iter().filter_map(|&(old, _)| old) {
            let entry = self
                .entries
                .get_mut(&key)
                .expect("previously accepted strike");
            entry.1 -= 1;
            if entry.1 == 0 {
                if self.warm.len() == WARM {
                    let victim = self.warm.remove(0);
                    self.forget(victim);
                }
                self.warm.push(key);
            }
        }
    }
    pub(super) fn get(&self, key: StrikeKey) -> &Strike {
        &self.entries[&key].0
    }
    #[cfg(test)]
    pub(in crate::tree) fn resident(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_partial_cold_reserve_cannot_leak_or_evict_live_or_promised_tables() {
        let mut s = Strikes::default();
        let [a, b, c, d] = [12., 14., 16., 18.].map(|n| StrikeKey::new(Face::default(), n));
        let mut added = Vec::new();
        s.prepare(&[(None, Some(a))], &mut added).unwrap();
        s.commit(&[(None, Some(a))]);
        let table = s.bytes;
        let limit = 2 * table + table / 10; // two tables plus temporary glyph IDs
        s.prepare_within(&[(Some(a), Some(b))], &mut added, limit)
            .unwrap();
        s.commit(&[(Some(a), Some(b))]);
        let pending = [(Some(b), Some(c)), (None, Some(a))];
        assert!(s.prepare_within(&pending, &mut added, limit).is_err()); // cannot evict a
        assert_eq!(s.resident(), 2);
        assert!(
            s.prepare_within(&[(None, Some(c)), (None, Some(d))], &mut added, limit)
                .is_err()
        ); // evict a, prepare c, then reject d
        assert_eq!(s.resident(), 1);
        assert_eq!(s.bytes, table);
        assert_eq!(s.entries[&b].1, 1);
        s.prepare_within(&[(Some(b), Some(d))], &mut added, limit)
            .unwrap();
        s.commit(&[(Some(b), Some(d))]);
        assert_eq!(s.entries[&d].1, 1);
    }
}
