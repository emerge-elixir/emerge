//! One committed owner table plus a bounded, last-write-wins admission delta.
//! Reads expose the candidate view; publication explicitly commits it. No rollback
//! clone, superseded-attempt chain or second timing authority is kept here.
use std::collections::{HashMap, hash_map};
use std::hash::Hash;

#[derive(Clone, Debug)]
pub(super) struct Version(std::sync::Arc<()>);
impl Default for Version {
    fn default() -> Self {
        Self(std::sync::Arc::new(()))
    }
}
impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Version {
    fn changed(&mut self) {
        if std::sync::Arc::strong_count(&self.0) > 1 {
            self.0 = std::sync::Arc::new(());
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct AdmissionMap<K, V> {
    version: Version,
    committed: HashMap<K, V>,
    staged: HashMap<K, Option<V>>,
    len: usize,
}
impl<K, V> Default for AdmissionMap<K, V> {
    fn default() -> Self {
        Self {
            version: Version::default(),
            committed: HashMap::new(),
            staged: HashMap::new(),
            len: 0,
        }
    }
}
impl<K: Eq + Hash + Copy, V: Clone> AdmissionMap<K, V> {
    pub fn version(&self) -> Version {
        self.version.clone()
    }
    pub fn has_staged(&self) -> bool {
        !self.staged.is_empty()
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        match self.staged.get(key) {
            Some(value) => value.as_ref(),
            None => self.committed.get(key),
        }
    }
    pub fn committed(&self, key: &K) -> Option<&V> {
        self.committed.get(key)
    }
    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }
    pub fn insert(&mut self, key: K, value: V) {
        self.version.changed();
        if !self.contains_key(&key) {
            self.len += 1;
        }
        self.staged.insert(key, Some(value));
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let old = self.get(key)?.clone();
        self.version.changed();
        self.len -= 1;
        if self.committed.contains_key(key) {
            self.staged.insert(*key, None);
        } else {
            self.staged.remove(key);
        }
        Some(old)
    }
    /// Restore the original owner when an unpublished replacement is no longer desired.
    pub fn restore(&mut self, key: &K) {
        self.version.changed();
        let before = usize::from(self.contains_key(key));
        self.staged.remove(key);
        self.len = self.len - before + usize::from(self.committed.contains_key(key));
    }
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.version.changed();
        if !self.staged.contains_key(key) {
            let copy = self.committed.get(key)?.clone();
            self.staged.insert(*key, Some(copy));
        }
        self.staged.get_mut(key)?.as_mut()
    }
    pub fn iter(&self) -> ViewIter<'_, K, V> {
        ViewIter {
            base: self.committed.iter(),
            delta: self.staged.iter(),
            shadowed: &self.staged,
        }
    }
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(key, _)| key)
    }
    #[cfg(test)]
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        let removed: Vec<_> = self
            .iter()
            .filter_map(|(key, value)| (!keep(key, value)).then_some(*key))
            .collect();
        for key in removed {
            self.remove(&key);
        }
    }
    /// Drop only sparse proposals equal to their committed value.
    pub fn restore_unchanged(&mut self, same: impl Fn(&V, &V) -> bool) {
        let restored: Vec<_> = self
            .staged
            .iter()
            .filter_map(|(key, next)| {
                next.as_ref()
                    .zip(self.committed.get(key))
                    .filter(|(next, old)| same(next, old))
                    .map(|_| *key)
            })
            .collect();
        for key in restored {
            self.restore(&key);
        }
    }
    pub fn commit(&mut self) {
        for (key, value) in std::mem::take(&mut self.staged) {
            match value {
                Some(value) => {
                    self.committed.insert(key, value);
                }
                None => {
                    self.committed.remove(&key);
                }
            }
        }
        if self.committed.is_empty() {
            self.committed = HashMap::new();
        }
        debug_assert_eq!(self.len, self.committed.len());
    }
    #[cfg(test)]
    pub fn record_counts(&self) -> (usize, usize) {
        (self.committed.len(), self.staged.len())
    }
}

pub(super) struct ViewIter<'a, K, V> {
    base: hash_map::Iter<'a, K, V>,
    delta: hash_map::Iter<'a, K, Option<V>>,
    shadowed: &'a HashMap<K, Option<V>>,
}
impl<'a, K: Eq + Hash, V> Iterator for ViewIter<'a, K, V> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.base
            .find(|(key, _)| !self.shadowed.contains_key(key))
            .or_else(|| {
                self.delta
                    .find_map(|(key, value)| value.as_ref().map(|value| (key, value)))
            })
    }
}
#[cfg(test)]
impl<K: Eq + Hash + Copy, V: Clone> std::ops::Index<&K> for AdmissionMap<K, V> {
    type Output = V;
    fn index(&self, key: &K) -> &Self::Output {
        self.get(key).expect("test owner must exist")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn an_unpublished_replacement_can_restore_the_original_without_history() {
        let mut map = AdmissionMap::default();
        let original = Arc::new(1);
        map.insert(7, Arc::clone(&original));
        assert_eq!(map.record_counts(), (0, 1));
        map.commit();
        for n in 2..1000 {
            map.insert(7, Arc::new(n));
            assert_eq!(map.record_counts(), (1, 1));
        }
        assert!(Arc::ptr_eq(map.committed(&7).unwrap(), &original));
        map.restore(&7);
        assert!(Arc::ptr_eq(map.get(&7).unwrap(), &original));
        assert_eq!(map.record_counts(), (1, 0));
        assert_eq!(map.len(), 1);
    }
    #[test]
    fn cancellation_and_in_place_updates_leave_committed_values_untouched() {
        let mut map = AdmissionMap::default();
        map.insert(1, 10);
        map.insert(2, 20);
        map.commit();
        *map.get_mut(&1).unwrap() = 11;
        map.remove(&2);
        map.insert(3, 30);
        assert_eq!(map.committed(&1), Some(&10));
        assert_eq!(map.committed(&2), Some(&20));
        assert_eq!(
            map.iter().map(|(k, v)| (*k, *v)).collect::<HashMap<_, _>>(),
            HashMap::from([(1, 11), (3, 30)])
        );
        assert_eq!(map.len(), 2);
        map.commit();
        assert_eq!(map.record_counts(), (2, 0));
        map.retain(|_, _| false);
        assert!(map.is_empty());
        assert_eq!(map.record_counts(), (2, 2));
        map.commit();
        assert_eq!(map.record_counts(), (0, 0));
    }
}
