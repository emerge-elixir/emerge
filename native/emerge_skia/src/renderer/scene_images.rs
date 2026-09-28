//! Scene-local image bindings. Source records are immutable; cache eviction may
//! remove lookup ownership but cannot substitute another image into an old scene.
use super::*;
use crate::render_scene::{RenderNode, RenderPaintLayerContentNode};
use std::cell::RefCell;

#[derive(Clone, Default)]
pub struct ImageSnapshot(Arc<HashMap<String, Binding>>, usize);

#[derive(Clone, Default)]
struct Binding {
    record: Option<Arc<crate::assets::AssetRecord>>,
    raster: Option<DecodedRaster>,
    vectors: HashMap<RenderedVectorKey, RenderedVectorVariant>,
    revision: u64,
}

impl Binding {
    fn metadata(&self, id: &str) -> Option<CachedAssetMetadata> {
        self.record
            .as_ref()
            .map(|record| CachedAssetMetadata {
                id: record.id.clone(),
                source: record.source.clone(),
                width: record.width,
                height: record.height,
                generation: record.generation,
                render_revision: Some(Arc::clone(&record.render_revision)),
                kind: match record.kind {
                    crate::assets::AssetRecordKind::Raster(_) => AssetKind::Raster,
                    crate::assets::AssetRecordKind::Vector(_) => AssetKind::Vector,
                },
            })
            .or_else(|| {
                self.raster
                    .as_ref()
                    .map(|raster| raster_metadata(id, raster))
            })
            .or_else(|| {
                self.vectors
                    .values()
                    .next()
                    .map(|variant| variant.metadata.clone())
            })
    }
}

impl std::fmt::Debug for ImageSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageSnapshot")
            .field("retention", &self.retention())
            .field("capture_node_visits", &self.capture_node_visits())
            .finish()
    }
}
impl PartialEq for ImageSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.len() == other.0.len()
                && self.0.iter().all(|(id, a)| {
                    other.0.get(id).is_some_and(|b| {
                        a.revision == b.revision
                            && match (&a.record, &b.record) {
                                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                                (None, None) => {
                                    a.raster.as_ref().map(|r| r.image.unique_id())
                                        == b.raster.as_ref().map(|r| r.image.unique_id())
                                        && a.vectors.len() == b.vectors.len()
                                        && a.vectors.iter().all(|(key, a)| {
                                            b.vectors.get(key).is_some_and(|b| {
                                                a.image.unique_id() == b.image.unique_id()
                                            })
                                        })
                                }
                                _ => false,
                            }
                    })
                }))
    }
}

/// Per-snapshot references/charges, not deduplicated global heap or GPU memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageSnapshotRetention {
    pub bindings: usize,
    pub source_records: usize,
    pub encoded_bytes: u64,
    pub vector_records: usize,
    pub cached_pixel_bytes: u64,
}

impl ImageSnapshot {
    pub fn capture(nodes: &[RenderNode]) -> Self {
        if nodes.is_empty()
            || (!crate::assets::has_scene_image_sources()
                && asset_context()
                    .pixel_cache
                    .lock()
                    .is_ok_and(|cache| cache.entries.is_empty() && cache.vectors.is_empty()))
        {
            return Self::default();
        }
        let visits = std::cell::Cell::new(0);
        let ids = image_ids(nodes, &visits).collect::<HashSet<_>>();
        let bindings = ids
            .into_iter()
            .map(|id| (id.to_owned(), crate::assets::asset_record(id)))
            .collect();
        Self::capture_records(bindings, None, visits.get())
    }
    // Caller may hold source state. Never acquire source state while holding the
    // pixel cache; capture only clones immutable references under these locks.
    pub(crate) fn capture_records(
        records: HashMap<String, Option<Arc<crate::assets::AssetRecord>>>,
        epoch: Option<u64>,
        visits: usize,
    ) -> Self {
        let context = asset_context();
        let cache = context.pixel_cache.lock().ok();
        let cache = cache
            .as_ref()
            .filter(|cache| epoch.is_none_or(|epoch| cache.epoch == epoch));
        let bindings = records
            .into_iter()
            .map(|(id, record)| {
                let mut binding = if record.is_some() {
                    Binding {
                        record,
                        ..Default::default()
                    }
                } else {
                    let generation = cache.and_then(|cache| {
                        cache
                            .entries
                            .get(&id)
                            .map(|entry| entry.source_generation)
                            .into_iter()
                            .chain(
                                cache
                                    .vectors
                                    .keys()
                                    .filter(|key| key.asset_id == id)
                                    .map(|key| key.generation),
                            )
                            .max()
                    });
                    Binding {
                        raster: cache.and_then(|cache| {
                            cache
                                .entries
                                .get(&id)
                                .filter(|entry| Some(entry.source_generation) == generation)
                                .cloned()
                        }),
                        vectors: cache
                            .map(|cache| {
                                cache
                                    .vectors
                                    .iter()
                                    .filter(|(key, _)| {
                                        key.asset_id == id && Some(key.generation) == generation
                                    })
                                    .map(|(key, value)| (key.clone(), value.clone()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        ..Default::default()
                    }
                };
                binding.revision = binding
                    .metadata(&id)
                    .and_then(|m| m.render_revision)
                    .map_or(0, |v| v.load(Ordering::Relaxed));
                (id, binding)
            })
            .collect();
        Self(Arc::new(bindings), visits)
    }
    pub(crate) fn dimensions(&self, id: &str) -> Option<(u32, u32)> {
        self.0.get(id)?.metadata(id).map(|m| (m.width, m.height))
    }
    pub(crate) fn referenced(&self, nodes: &[RenderNode]) -> Self {
        if self.0.is_empty() {
            return Self::default();
        }
        let visits = std::cell::Cell::new(0);
        let bindings = image_ids(nodes, &visits)
            .filter_map(|id| {
                self.0
                    .get(id)
                    .map(|binding| (id.to_owned(), binding.clone()))
            })
            .collect();
        Self(Arc::new(bindings), visits.get())
    }
    pub(crate) fn enter(&self) -> ImageGuard {
        enter(Some(self))
    }
    /// Actual render graph visits during capture, not a live-retention gauge.
    pub fn capture_node_visits(&self) -> usize {
        self.1
    }
    pub fn retention(&self) -> ImageSnapshotRetention {
        self.0
            .values()
            .fold(ImageSnapshotRetention::default(), |mut total, binding| {
                total.bindings += 1;
                if let Some(record) = &binding.record {
                    total.source_records += 1;
                    total.encoded_bytes = total.encoded_bytes.saturating_add(record.encoded_bytes);
                    total.vector_records += usize::from(matches!(
                        record.kind,
                        crate::assets::AssetRecordKind::Vector(_)
                    ));
                }
                total.cached_pixel_bytes = total
                    .cached_pixel_bytes
                    .saturating_add(binding.raster.as_ref().map_or(0, |r| r.bytes))
                    .saturating_add(binding.vectors.values().map(|v| v.bytes).sum::<u64>());
                total
            })
    }
}

// Stack traversal borrows cached render content, without cloning subtrees or
// allocating an iterator box for every primitive.
fn image_ids<'a>(
    nodes: &'a [RenderNode],
    visits: &'a std::cell::Cell<usize>,
) -> impl Iterator<Item = &'a str> {
    enum Branch<'a> {
        Nodes(&'a [RenderNode]),
        Content(&'a [RenderPaintLayerContentNode]),
    }
    let mut pending = vec![Branch::Nodes(nodes)];
    std::iter::from_fn(move || {
        loop {
            match pending.pop()? {
                Branch::Nodes(nodes) => {
                    let Some((node, rest)) = nodes.split_first() else {
                        continue;
                    };
                    visits.set(visits.get() + 1);
                    if !rest.is_empty() {
                        pending.push(Branch::Nodes(rest));
                    }
                    match node {
                        RenderNode::Primitive(DrawPrimitive::Image(_, _, _, _, id, _, _)) => {
                            return Some(id.as_str());
                        }
                        RenderNode::Primitive(_) => {}
                        RenderNode::PaintLayer(layer) => {
                            pending.push(Branch::Content(&layer.content.nodes))
                        }
                        RenderNode::ShadowPass { children }
                        | RenderNode::Clip { children, .. }
                        | RenderNode::RelaxedClip { children, .. }
                        | RenderNode::Transform { children, .. }
                        | RenderNode::Alpha { children, .. } => {
                            pending.push(Branch::Nodes(children))
                        }
                    }
                }
                Branch::Content(nodes) => {
                    let Some((node, rest)) = nodes.split_first() else {
                        continue;
                    };
                    visits.set(visits.get() + 1);
                    if !rest.is_empty() {
                        pending.push(Branch::Content(rest));
                    }
                    match node {
                        RenderPaintLayerContentNode::Own(run) => {
                            pending.push(Branch::Nodes(&run.nodes))
                        }
                        RenderPaintLayerContentNode::Child(layer) => {
                            pending.push(Branch::Content(&layer.content.nodes))
                        }
                        RenderPaintLayerContentNode::ShadowPass { children }
                        | RenderPaintLayerContentNode::Clip { children, .. }
                        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
                        | RenderPaintLayerContentNode::Transform { children, .. }
                        | RenderPaintLayerContentNode::Alpha { children, .. } => {
                            pending.push(Branch::Content(children))
                        }
                    }
                }
            }
        }
    })
}

thread_local! {static FRAME_IMAGES:RefCell<Option<ImageSnapshot>>=const {RefCell::new(None)};}
pub(crate) struct ImageGuard {
    previous: Option<ImageSnapshot>,
    _local: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Drop for ImageGuard {
    fn drop(&mut self) {
        FRAME_IMAGES.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}
/// Enter even for None: a nested manual scene must not borrow an outer binding.
pub(crate) fn enter(snapshot: Option<&ImageSnapshot>) -> ImageGuard {
    ImageGuard {
        previous: FRAME_IMAGES.with(|slot| slot.replace(snapshot.cloned())),
        _local: Default::default(),
    }
}
fn frozen<T>(id: &str, read: impl FnOnce(Option<&Binding>) -> Option<T>) -> Option<Option<T>> {
    FRAME_IMAGES.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|snapshot| read(snapshot.0.get(id)))
    })
}
pub(super) fn record(id: &str) -> Option<Arc<crate::assets::AssetRecord>> {
    frozen(id, |binding| binding.and_then(|b| b.record.clone()))
        .unwrap_or_else(|| crate::assets::asset_record(id))
}
pub(super) fn metadata(id: &str) -> Option<CachedAssetMetadata> {
    frozen(id, |binding| binding.and_then(|b| b.metadata(id)))
        .unwrap_or_else(|| retained_asset_metadata(id))
}
pub(super) fn revision(id: &str) -> Option<u64> {
    frozen(id, |binding| binding.map(|b| b.revision)).flatten()
}
pub(super) fn raster(id: &str) -> Option<Option<(Image, u32, u32)>> {
    frozen(id, |binding| {
        binding
            .and_then(|b| b.raster.as_ref())
            .map(|r| (r.image.clone(), r.source_width, r.source_height))
    })
}
pub(super) fn vector(key: &RenderedVectorKey) -> Option<Image> {
    frozen(&key.asset_id, |binding| {
        binding
            .and_then(|b| b.vectors.get(key))
            .map(|v| v.image.clone())
    })
    .flatten()
}
pub(super) fn request_hydration(id: &str) {
    let permitted = frozen(id, |binding| binding.and_then(|b| b.metadata(id)))
        .map(|captured| {
            captured
                .zip(retained_asset_metadata(id))
                .is_some_and(|(a, b)| {
                    a.generation == b.generation
                        && a.render_revision
                            .zip(b.render_revision)
                            .is_some_and(|(a, b)| Arc::ptr_eq(&a, &b))
                })
        })
        .unwrap_or(true);
    if permitted {
        crate::assets::request_asset_hydration(id);
    }
}

#[cfg(test)]
mod tests;
