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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayerPayloadPlacement {
    Fixed,
    ScrollMoving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayerPayloadStorage {
    Gpu,
    Cpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaintLayerPayloadKey {
    pub stable_id: u64,
    pub placement: PaintLayerPayloadPlacement,
    pub content_hash: u64,
    pub dependency_hash: u64,
    pub width_px: u32,
    pub height_px: u32,
    pub scale_bits: u32,
    pub resource_generation: u64,
}

impl PaintLayerPayloadKey {
    pub fn fixed(
        stable_id: u64,
        content_hash: u64,
        dependency_hash: u64,
        width_px: u32,
        height_px: u32,
        scale_bits: u32,
        resource_generation: u64,
    ) -> Self {
        Self {
            stable_id,
            placement: PaintLayerPayloadPlacement::Fixed,
            content_hash,
            dependency_hash,
            width_px,
            height_px,
            scale_bits,
            resource_generation,
        }
    }

    pub fn scroll_moving(
        stable_id: u64,
        content_hash: u64,
        width_px: u32,
        height_px: u32,
        scale_bits: u32,
        resource_generation: u64,
    ) -> Self {
        Self {
            stable_id,
            placement: PaintLayerPayloadPlacement::ScrollMoving,
            content_hash,
            dependency_hash: 0,
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
    pub fixed_entries: u64,
    pub moving_entries: u64,
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
        self.evict_stale(frame_index)
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

    pub fn try_store(
        &mut self,
        key: PaintLayerPayloadKey,
        payload: P,
        bytes: u64,
        storage: PaintLayerPayloadStorage,
    ) -> Result<Vec<u64>, PaintLayerPayloadStoreRejection> {
        self.try_reserve_store(bytes)?;
        self.store_reserved(key, payload, bytes, storage)
    }

    pub fn try_reserve_store(&mut self, bytes: u64) -> Result<(), PaintLayerPayloadStoreRejection> {
        if bytes > self.config.max_entry_bytes || bytes > self.config.max_bytes {
            return Err(PaintLayerPayloadStoreRejection::OversizedEntry);
        }

        if self.new_payloads_remaining == 0 {
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
                match entry.key.placement {
                    PaintLayerPayloadPlacement::Fixed => stats.fixed_entries += 1,
                    PaintLayerPayloadPlacement::ScrollMoving => stats.moving_entries += 1,
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

    fn evict_stale(&mut self, frame_index: u64) -> Vec<u64> {
        let stale_keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                frame_index.saturating_sub(entry.last_seen_frame) > self.config.max_stale_frames
            })
            .map(|(key, _)| *key)
            .collect();

        stale_keys
            .into_iter()
            .filter_map(|key| self.entries.remove(&key))
            .map(|entry| {
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
                entry.bytes
            })
            .collect()
    }

    fn oldest_entry_key(&self) -> Option<PaintLayerPayloadKey> {
        self.entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_used_frame)
            .map(|(key, _)| *key)
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
        PaintLayerPayloadKey::scroll_moving(stable_id, content_hash, 100, 40, 1.0f32.to_bits(), 9)
    }

    fn fixed_key(dependency_hash: u64) -> PaintLayerPayloadKey {
        PaintLayerPayloadKey::fixed(1, 10, dependency_hash, 800, 600, 1.0f32.to_bits(), 9)
    }

    #[test]
    fn moving_payload_key_excludes_placement() {
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
    fn fixed_key_includes_dynamic_child_dependency_hash() {
        assert_ne!(fixed_key(1), fixed_key(2));
    }

    #[test]
    fn byte_budget_and_entry_budget_are_shared_across_layer_placements() {
        let mut cache = PaintLayerPayloadCache::with_config(config());
        cache.begin_frame(1);

        assert_eq!(
            cache.try_store(fixed_key(1), "fixed", 120, PaintLayerPayloadStorage::Gpu,),
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
                .try_store(fixed_key(1), "a", 10, PaintLayerPayloadStorage::Gpu)
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
    fn stale_eviction_uses_last_seen_frame() {
        let mut cache = PaintLayerPayloadCache::with_config(config());
        let key = moving_key(3, 1);
        cache.begin_frame(1);
        cache
            .try_store(key, "moving", 10, PaintLayerPayloadStorage::Gpu)
            .expect("store should succeed");
        cache.begin_frame(3);
        cache.mark_seen(&key);

        assert_eq!(cache.begin_frame(5), Vec::<u64>::new());
        assert_eq!(cache.begin_frame(6), vec![10]);
        assert_eq!(cache.stats().entries, 0);
    }
}
