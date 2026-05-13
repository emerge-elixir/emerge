use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintLayerPayloadCacheConfig {
    pub max_entries: usize,
    pub max_bytes: u64,
    pub max_entry_bytes: u64,
    pub max_stale_frames: u64,
    pub max_new_payloads_per_frame: u32,
}

impl Default for PaintLayerPayloadCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 512,
            max_bytes: 512 * 1024 * 1024,
            max_entry_bytes: 128 * 1024 * 1024,
            max_stale_frames: 120,
            max_new_payloads_per_frame: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayerPayloadStorage {
    Gpu,
    Cpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaintLayerPayloadKey {
    pub stable_id: u64,
    pub content_hash: u64,
    pub width_px: u32,
    pub height_px: u32,
    pub scale_bits: u32,
    pub resource_generation: u64,
}

impl PaintLayerPayloadKey {
    pub fn new(
        stable_id: u64,
        content_hash: u64,
        width_px: u32,
        height_px: u32,
        scale_bits: u32,
        resource_generation: u64,
    ) -> Self {
        Self {
            stable_id,
            content_hash,
            width_px,
            height_px,
            scale_bits,
            resource_generation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintLayerPayloadCacheEntry<P> {
    pub key: PaintLayerPayloadKey,
    pub payload: P,
    pub bytes: u64,
    pub storage: PaintLayerPayloadStorage,
    pub last_used_frame: u64,
    pub last_seen_frame: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaintLayerPayloadCacheStats {
    pub entries: u64,
    pub bytes: u64,
    pub gpu_payloads: u64,
    pub cpu_payloads: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayerPayloadStoreRejection {
    OversizedEntry,
    PayloadBudget,
}

#[derive(Clone, Debug)]
pub struct PaintLayerPayloadCache<P> {
    entries: HashMap<PaintLayerPayloadKey, PaintLayerPayloadCacheEntry<P>>,
    total_bytes: u64,
    frame_index: u64,
    new_payloads_remaining: u32,
    config: PaintLayerPayloadCacheConfig,
}

impl<P> Default for PaintLayerPayloadCache<P> {
    fn default() -> Self {
        Self::with_config(PaintLayerPayloadCacheConfig::default())
    }
}

impl<P> PaintLayerPayloadCache<P> {
    pub fn with_config(config: PaintLayerPayloadCacheConfig) -> Self {
        Self {
            entries: HashMap::new(),
            total_bytes: 0,
            frame_index: 0,
            new_payloads_remaining: config.max_new_payloads_per_frame,
            config,
        }
    }

    pub fn begin_frame(&mut self, frame_index: u64) -> Vec<u64> {
        self.frame_index = frame_index;
        self.new_payloads_remaining = self.config.max_new_payloads_per_frame;
        Vec::new()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_bytes = 0;
        self.new_payloads_remaining = self.config.max_new_payloads_per_frame;
    }

    pub fn config(&self) -> PaintLayerPayloadCacheConfig {
        self.config
    }

    pub fn get(&mut self, key: &PaintLayerPayloadKey) -> Option<&P> {
        let entry = self.entries.get_mut(key)?;
        entry.last_used_frame = self.frame_index;
        entry.last_seen_frame = self.frame_index;
        Some(&entry.payload)
    }

    pub fn mark_seen(&mut self, key: &PaintLayerPayloadKey) -> bool {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_seen_frame = self.frame_index;
            return true;
        }
        false
    }

    pub fn contains_key(&self, key: &PaintLayerPayloadKey) -> bool {
        self.entries.contains_key(key)
    }

    pub fn try_store(
        &mut self,
        key: PaintLayerPayloadKey,
        payload: P,
        bytes: u64,
        storage: PaintLayerPayloadStorage,
    ) -> Result<Vec<u64>, PaintLayerPayloadStoreRejection> {
        self.try_reserve_store(key, bytes)?;
        self.store_reserved(key, payload, bytes, storage)
    }

    pub fn try_reserve_store(
        &mut self,
        key: PaintLayerPayloadKey,
        bytes: u64,
    ) -> Result<(), PaintLayerPayloadStoreRejection> {
        if bytes > self.config.max_entry_bytes || bytes > self.config.max_bytes {
            return Err(PaintLayerPayloadStoreRejection::OversizedEntry);
        }

        if self.config.max_entries == 0
            || self.new_payloads_remaining == 0
            || self.store_would_evict_current_frame_payload(key, bytes)
        {
            return Err(PaintLayerPayloadStoreRejection::PayloadBudget);
        }
        self.new_payloads_remaining -= 1;
        Ok(())
    }

    pub fn store_reserved(
        &mut self,
        key: PaintLayerPayloadKey,
        payload: P,
        bytes: u64,
        storage: PaintLayerPayloadStorage,
    ) -> Result<Vec<u64>, PaintLayerPayloadStoreRejection> {
        if bytes > self.config.max_entry_bytes || bytes > self.config.max_bytes {
            return Err(PaintLayerPayloadStoreRejection::OversizedEntry);
        }

        if let Some(existing) = self.entries.remove(&key) {
            self.total_bytes = self.total_bytes.saturating_sub(existing.bytes);
        }

        self.total_bytes = self.total_bytes.saturating_add(bytes);
        self.entries.insert(
            key,
            PaintLayerPayloadCacheEntry {
                key,
                payload,
                bytes,
                storage,
                last_used_frame: self.frame_index,
                last_seen_frame: self.frame_index,
            },
        );

        Ok(self.evict_if_needed())
    }

    pub fn entries(&self) -> impl Iterator<Item = &PaintLayerPayloadCacheEntry<P>> {
        self.entries.values()
    }

    pub fn stats(&self) -> PaintLayerPayloadCacheStats {
        self.entries.values().fold(
            PaintLayerPayloadCacheStats {
                entries: self.entries.len() as u64,
                bytes: self.total_bytes,
                ..PaintLayerPayloadCacheStats::default()
            },
            |mut stats, entry| {
                match entry.storage {
                    PaintLayerPayloadStorage::Gpu => stats.gpu_payloads += 1,
                    PaintLayerPayloadStorage::Cpu => stats.cpu_payloads += 1,
                }
                stats
            },
        )
    }

    fn evict_if_needed(&mut self) -> Vec<u64> {
        let mut evicted = Vec::new();
        while self.entries.len() > self.config.max_entries
            || self.total_bytes > self.config.max_bytes
        {
            let Some(oldest_key) = self.oldest_entry_key() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest_key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
                evicted.push(entry.bytes);
            }
        }
        evicted
    }

    fn oldest_entry_key(&self) -> Option<PaintLayerPayloadKey> {
        self.entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_used_frame)
            .map(|(key, _)| *key)
    }

    fn store_would_evict_current_frame_payload(
        &self,
        store_key: PaintLayerPayloadKey,
        store_bytes: u64,
    ) -> bool {
        let existing_bytes = self
            .entries
            .get(&store_key)
            .map(|entry| entry.bytes)
            .unwrap_or(0);
        let mut projected_len =
            self.entries.len() + usize::from(!self.entries.contains_key(&store_key));
        let mut projected_bytes = self
            .total_bytes
            .saturating_sub(existing_bytes)
            .saturating_add(store_bytes);

        if projected_len <= self.config.max_entries && projected_bytes <= self.config.max_bytes {
            return false;
        }

        let mut victims: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| **key != store_key)
            .collect();
        victims.sort_by_key(|(_, entry)| entry.last_used_frame);

        for (_, victim) in victims {
            if projected_len <= self.config.max_entries && projected_bytes <= self.config.max_bytes
            {
                return false;
            }
            if victim.last_used_frame == self.frame_index {
                return true;
            }
            projected_len = projected_len.saturating_sub(1);
            projected_bytes = projected_bytes.saturating_sub(victim.bytes);
        }

        projected_len > self.config.max_entries || projected_bytes > self.config.max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> PaintLayerPayloadCacheConfig {
        PaintLayerPayloadCacheConfig {
            max_entries: 3,
            max_bytes: 300,
            max_entry_bytes: 200,
            max_stale_frames: 2,
            max_new_payloads_per_frame: 2,
        }
    }

    fn moving_key(stable_id: u64, content_hash: u64) -> PaintLayerPayloadKey {
        PaintLayerPayloadKey::new(stable_id, content_hash, 100, 40, 1.0f32.to_bits(), 9)
    }

    #[test]
    fn payload_key_excludes_placement() {
        let first_placement = moving_key(7, 44);
        let second_placement = moving_key(7, 44);

        assert_eq!(first_placement, second_placement);
    }

    #[test]
    fn payload_key_includes_size_scale_resource_and_content() {
        let base = moving_key(7, 44);
        let different_content = moving_key(7, 45);
        let different_resource = PaintLayerPayloadKey {
            resource_generation: 10,
            ..base
        };
        let different_scale = PaintLayerPayloadKey {
            scale_bits: 2.0f32.to_bits(),
            ..base
        };
        let different_size = PaintLayerPayloadKey {
            width_px: 101,
            ..base
        };

        assert_ne!(base, different_content);
        assert_ne!(base, different_resource);
        assert_ne!(base, different_scale);
        assert_ne!(base, different_size);
    }

    #[test]
    fn byte_budget_and_entry_budget_are_shared_across_layer_payloads() {
        let mut cache = PaintLayerPayloadCache::with_config(config());
        cache.begin_frame(1);

        assert_eq!(
            cache.try_store(moving_key(1, 1), "a", 120, PaintLayerPayloadStorage::Gpu,),
            Ok(Vec::new())
        );
        assert_eq!(
            cache.try_store(
                moving_key(2, 1),
                "moving-a",
                120,
                PaintLayerPayloadStorage::Gpu,
            ),
            Ok(Vec::new())
        );

        cache.begin_frame(2);
        let evicted = cache
            .try_store(
                moving_key(3, 1),
                "moving-b",
                120,
                PaintLayerPayloadStorage::Gpu,
            )
            .expect("store should fit entry budget and evict for byte budget");

        assert_eq!(evicted, vec![120]);
        assert_eq!(cache.stats().entries, 2);
        assert_eq!(cache.stats().bytes, 240);
    }

    #[test]
    fn payload_budget_limits_new_stores_per_frame() {
        let mut cache = PaintLayerPayloadCache::with_config(config());
        cache.begin_frame(1);

        assert!(
            cache
                .try_store(moving_key(1, 1), "a", 10, PaintLayerPayloadStorage::Gpu)
                .is_ok()
        );
        assert!(
            cache
                .try_store(moving_key(2, 1), "b", 10, PaintLayerPayloadStorage::Gpu)
                .is_ok()
        );
        assert_eq!(
            cache.try_store(moving_key(3, 1), "c", 10, PaintLayerPayloadStorage::Gpu),
            Err(PaintLayerPayloadStoreRejection::PayloadBudget)
        );
    }

    #[test]
    fn store_reservation_rejects_when_it_would_evict_payload_used_this_frame() {
        let mut cache = PaintLayerPayloadCache::with_config(PaintLayerPayloadCacheConfig {
            max_new_payloads_per_frame: 4,
            ..config()
        });
        let first = moving_key(1, 1);
        let second = moving_key(2, 1);
        let third = moving_key(3, 1);

        cache.begin_frame(1);
        cache
            .try_store(first, "first", 100, PaintLayerPayloadStorage::Cpu)
            .unwrap();
        cache
            .try_store(second, "second", 100, PaintLayerPayloadStorage::Cpu)
            .unwrap();
        cache
            .try_store(third, "third", 100, PaintLayerPayloadStorage::Cpu)
            .unwrap();

        cache.begin_frame(2);
        assert_eq!(cache.get(&first), Some(&"first"));
        assert_eq!(cache.get(&second), Some(&"second"));
        assert_eq!(cache.get(&third), Some(&"third"));
        assert_eq!(
            cache.try_reserve_store(moving_key(4, 1), 100),
            Err(PaintLayerPayloadStoreRejection::PayloadBudget)
        );
        assert_eq!(cache.get(&first), Some(&"first"));
    }

    #[test]
    fn unseen_payloads_remain_until_budget_eviction() {
        let mut cache = PaintLayerPayloadCache::with_config(config());
        let key = moving_key(3, 1);
        cache.begin_frame(1);
        cache
            .try_store(key, "moving", 10, PaintLayerPayloadStorage::Gpu)
            .expect("store should succeed");
        cache.begin_frame(3);
        cache.mark_seen(&key);

        assert_eq!(cache.begin_frame(5), Vec::<u64>::new());
        assert_eq!(cache.begin_frame(600), Vec::<u64>::new());
        assert_eq!(cache.stats().entries, 1);
    }
}
