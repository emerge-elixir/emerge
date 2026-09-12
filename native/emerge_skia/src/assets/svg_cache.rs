//! Renderer-local parsed vectors. Pixel retention is independently budgeted.
use super::{AssetRecord, AssetRecordKind};
use resvg::usvg;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) struct SvgTreeCache {
    entries: HashMap<String, Entry>,
    clock: u64,
    pub stats: SvgCacheStats,
}

struct Entry {
    record: Arc<AssetRecord>,
    charge: u64,
    used: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SvgCacheStats {
    pub entries: u64,
    pub estimated_bytes: u64,
    pub max_entries: u64,
    pub max_bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub parses: u64,
    pub font_discoveries: u64,
    pub font_faces: u64,
    pub font_estimated_bytes: u64,
    pub font_environment_generation: u64,
    pub rasterizations: u64,
}

impl Default for SvgTreeCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            stats: SvgCacheStats {
                max_entries: 64,
                max_bytes: 16 * 1024 * 1024,
                ..SvgCacheStats::default()
            },
        }
    }
}

impl SvgTreeCache {
    pub fn get(&mut self, id: &str) -> Option<Arc<AssetRecord>> {
        self.clock = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(id) {
            entry.used = self.clock;
            self.stats.hits += 1;
            Some(Arc::clone(&entry.record))
        } else {
            self.stats.misses += 1;
            None
        }
    }

    pub fn get_for_source(&mut self, source: &str) -> Option<Arc<AssetRecord>> {
        let id = self
            .entries
            .iter()
            .find(|(_, entry)| entry.record.source == source)
            .map(|(id, _)| id.clone())?;
        self.get(&id)
    }

    pub fn remove(&mut self, id: &str) {
        if let Some(entry) = self.entries.remove(id) {
            self.stats.estimated_bytes = self.stats.estimated_bytes.saturating_sub(entry.charge);
        }
        self.stats.entries = self.entries.len() as u64;
    }

    pub fn insert(&mut self, record: Arc<AssetRecord>, charge: u64) {
        self.remove(&record.id);
        if self.stats.max_entries == 0 || charge > self.stats.max_bytes {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.insert(
            record.id.clone(),
            Entry {
                record,
                charge,
                used: self.clock,
            },
        );
        self.stats.estimated_bytes = self.stats.estimated_bytes.saturating_add(charge);
        self.stats.entries = self.entries.len() as u64;
        self.evict();
    }

    pub fn configure(&mut self, max_entries: u64, max_bytes: u64) {
        self.stats.max_entries = max_entries;
        self.stats.max_bytes = max_bytes;
        self.evict();
    }

    fn evict(&mut self) {
        while self.stats.entries > self.stats.max_entries
            || self.stats.estimated_bytes > self.stats.max_bytes
        {
            let Some(id) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.remove(&id);
            self.stats.evictions += 1;
        }
    }
}

/// Estimated tree-owned storage, not an allocator/RSS measurement. A per-node
/// allowance covers private usvg layout/style fields; variable public buffers
/// are charged additionally. Shared fonts are owned by the font environment.
/// Subroots include masks, clips, patterns, nested SVGs and flattened text.
pub(super) fn estimated_tree_bytes(record: &AssetRecord) -> u64 {
    let AssetRecordKind::Vector(tree) = &record.kind else {
        return 0;
    };
    fn group_charge(group: &usvg::Group, seen: &mut HashSet<usize>) -> u64 {
        if !seen.insert(group as *const _ as usize) {
            return 0;
        }
        let filters = group
            .filters()
            .iter()
            .filter(|filter| seen.insert(Arc::as_ptr(filter) as usize))
            .map(|filter| {
                256 + filter.id().len() as u64
                    + filter
                        .primitives()
                        .iter()
                        .map(|primitive| {
                            use usvg::filter::{Kind, TransferFunction};
                            let variable = match primitive.kind() {
                                Kind::ConvolveMatrix(matrix) => {
                                    matrix.matrix().data().len() as u64 * 4
                                }
                                Kind::ComponentTransfer(transfer) => [
                                    transfer.func_r(),
                                    transfer.func_g(),
                                    transfer.func_b(),
                                    transfer.func_a(),
                                ]
                                .into_iter()
                                .map(|function| match function {
                                    TransferFunction::Table(values)
                                    | TransferFunction::Discrete(values) => values.len() as u64 * 4,
                                    _ => 0,
                                })
                                .sum(),
                                Kind::Merge(merge) => merge
                                    .inputs()
                                    .iter()
                                    .map(|input| match input {
                                        usvg::filter::Input::Reference(name) => {
                                            64 + name.len() as u64
                                        }
                                        _ => 64,
                                    })
                                    .sum(),
                                _ => 0,
                            };
                            1024 + primitive.result().len() as u64 + variable
                        })
                        .sum::<u64>()
            })
            .sum::<u64>();
        1024 + filters
            + group.id().len() as u64
            + group
                .children()
                .iter()
                .map(|node| {
                    let own = match node {
                        usvg::Node::Group(group) => group_charge(group, seen),
                        usvg::Node::Path(path) => {
                            let data = path.data();
                            let buffer = if seen.insert(data as *const _ as usize) {
                                (data.points().len() * 8 + data.verbs().len()) as u64
                            } else {
                                0
                            };
                            let paints = path
                                .fill()
                                .map(|fill| fill.paint())
                                .into_iter()
                                .chain(path.stroke().map(|stroke| stroke.paint()))
                                .map(|paint| match paint {
                                    usvg::Paint::LinearGradient(gradient)
                                        if seen.insert(Arc::as_ptr(gradient) as usize) =>
                                    {
                                        256 + gradient.id().len() as u64
                                            + gradient.stops().len() as u64 * 32
                                    }
                                    usvg::Paint::RadialGradient(gradient)
                                        if seen.insert(Arc::as_ptr(gradient) as usize) =>
                                    {
                                        256 + gradient.id().len() as u64
                                            + gradient.stops().len() as u64 * 32
                                    }
                                    _ => 0, // pattern groups are visited by subroots
                                })
                                .sum::<u64>();
                            1024 + path.id().len() as u64
                                + buffer
                                + paints
                                + path
                                    .stroke()
                                    .and_then(|stroke| stroke.dasharray())
                                    .map_or(0, |array| array.len() as u64 * 4)
                        }
                        usvg::Node::Text(text) => {
                            1024 + text.id().len() as u64
                                + ((text.dx().len() + text.dy().len() + text.rotate().len()) * 4)
                                    as u64
                                + text
                                    .chunks()
                                    .iter()
                                    .map(|chunk| {
                                        chunk.text().len() as u64 * 32
                                            + chunk.spans().len() as u64 * 512
                                    })
                                    .sum::<u64>()
                        }
                        usvg::Node::Image(image) => {
                            let bytes = match image.kind() {
                                usvg::ImageKind::JPEG(bytes)
                                | usvg::ImageKind::PNG(bytes)
                                | usvg::ImageKind::GIF(bytes)
                                | usvg::ImageKind::WEBP(bytes) => {
                                    if seen.insert(bytes.as_ptr() as usize) {
                                        bytes.len() as u64
                                    } else {
                                        0
                                    }
                                }
                                usvg::ImageKind::SVG(_) => 0, // visited by subroots
                            };
                            1024 + bytes
                        }
                    };
                    let mut subroots = 0;
                    node.subroots(|root| subroots += group_charge(root, seen));
                    own + subroots
                })
                .sum::<u64>()
    }
    let mut seen = HashSet::new();
    group_charge(tree.root(), &mut seen).saturating_add(std::mem::size_of::<usvg::Tree>() as u64)
}

/// Estimate the shared database once, without reading any unloaded font files.
pub(super) fn estimated_font_bytes(fonts: &usvg::fontdb::Database) -> u64 {
    let mut seen = HashSet::new();
    std::mem::size_of::<usvg::fontdb::Database>() as u64
        + fonts
            .faces()
            .map(|face| {
                use usvg::fontdb::Source;
                let (path_bytes, data) = match &face.source {
                    Source::Binary(data) => (0, Some(data)),
                    Source::File(path) => (path.as_os_str().len() as u64, None),
                    Source::SharedFile(path, data) => (path.as_os_str().len() as u64, Some(data)),
                };
                let buffer = data
                    .filter(|data| seen.insert(Arc::as_ptr(data) as *const () as usize))
                    .map_or(0, |data| data.as_ref().as_ref().len() as u64);
                256 + path_bytes
                    + buffer
                    + face.post_script_name.len() as u64
                    + face
                        .families
                        .iter()
                        .map(|(name, _)| 32 + name.len() as u64)
                        .sum::<u64>()
            })
            .sum::<u64>()
}
