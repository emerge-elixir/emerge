use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, TrySendError, bounded};
use resvg::usvg;
use sha2::{Digest, Sha256};
use skia_safe::{Data, codec::Codec};

use crate::actors::TreeMsg;
use crate::renderer::insert_vector_asset;
use crate::renderer::{configure_asset_cache, insert_raster_asset_with_policy};
use crate::tree::attrs::{Background, ImageSource};
use crate::tree::element::ElementTree;

#[derive(Clone, Debug)]
pub struct AssetConfig {
    pub sources: Vec<String>,
    pub runtime_enabled: bool,
    pub runtime_allowlist: Vec<String>,
    pub runtime_follow_symlinks: bool,
    pub runtime_max_file_size: u64,
    pub runtime_extensions: Vec<String>,
    pub cache_max_entries: u64,
    pub cache_max_bytes: u64,
    pub decode_at_size: bool,
}

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            sources: vec!["priv".to_string()],
            runtime_enabled: false,
            runtime_allowlist: Vec::new(),
            runtime_follow_symlinks: false,
            runtime_max_file_size: 25_000_000,
            runtime_extensions: vec![
                ".png".to_string(),
                ".jpg".to_string(),
                ".jpeg".to_string(),
                ".webp".to_string(),
                ".gif".to_string(),
                ".bmp".to_string(),
                ".svg".to_string(),
            ],
            cache_max_entries: 256,
            cache_max_bytes: 256 * 1024 * 1024,
            decode_at_size: false,
        }
    }
}

#[derive(Clone)]
pub(crate) enum AssetRecordKind {
    Raster(Data),
    Vector(Arc<usvg::Tree>),
}

#[derive(Clone)]
pub(crate) struct AssetRecord {
    pub id: String,
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub encoded_bytes: u64,
    pub generation: u64,
    pub decode_at_size: bool,
    pub kind: AssetRecordKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAsset {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetStatus {
    Pending,
    Ready(ResolvedAsset),
    Failed,
}

#[derive(Default)]
struct AssetState {
    config: AssetConfig,
    sources: HashMap<ImageSource, AssetStatus>,
    records: HashMap<String, Arc<AssetRecord>>,
    pending_count: usize,
    status_generation: u64,
}

impl AssetState {
    fn clear_sources(&mut self) {
        if !self.sources.is_empty() || self.pending_count != 0 {
            self.status_generation = self.status_generation.wrapping_add(1);
        }
        self.sources.clear();
        #[cfg(not(test))]
        self.records.clear();
        self.pending_count = 0;
    }

    fn set_source_status(&mut self, source: ImageSource, status: AssetStatus) {
        if self.sources.get(&source) == Some(&status) {
            return;
        }

        self.sources.insert(source, status);
        self.status_generation = self.status_generation.wrapping_add(1);
    }
}

struct Global {
    state: Arc<Mutex<AssetState>>,
    tx: Option<Sender<AssetMsg>>,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(AssetState::default())),
            tx: None,
        }
    }
}

enum AssetMsg {
    Ensure(ImageSource),
    HydrateCached(ImageSource, String),
    Stop,
}

struct Worker {
    tree_tx: Sender<TreeMsg>,
    state: Arc<Mutex<AssetState>>,
    log_render: bool,
}

static GLOBAL: OnceLock<Mutex<Global>> = OnceLock::new();
static ASSET_GENERATION: AtomicU64 = AtomicU64::new(1);

fn global() -> &'static Mutex<Global> {
    GLOBAL.get_or_init(|| Mutex::new(Global::default()))
}

pub fn start(tree_tx: Sender<TreeMsg>, log_render: bool) {
    let (tx, rx) = bounded(4096);

    let mut guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if let Some(existing) = guard.tx.take() {
        let _ = existing.send(AssetMsg::Stop);
    }

    let state = Arc::clone(&guard.state);

    if let Ok(mut state_guard) = state.lock() {
        state_guard.clear_sources();
    }

    guard.tx = Some(tx.clone());
    drop(guard);

    thread::spawn(move || {
        let mut worker = Worker {
            tree_tx,
            state,
            log_render,
        };

        while let Ok(msg) = rx.recv() {
            match msg {
                AssetMsg::Stop => break,
                AssetMsg::Ensure(source) => worker.handle_ensure(source),
                AssetMsg::HydrateCached(source, cached_id) => {
                    worker.handle_hydrate_cached(source, &cached_id);
                }
            }
        }
    });
}

pub fn stop() {
    let mut guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if let Some(tx) = guard.tx.take() {
        let _ = tx.send(AssetMsg::Stop);
    }

    if let Ok(mut state) = guard.state.lock() {
        state.clear_sources();
    }
}

pub fn configure(config: AssetConfig) {
    configure_asset_cache(config.cache_max_entries, config.cache_max_bytes);

    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if let Ok(mut state) = guard.state.lock() {
        state.config = normalize_config(config);
        state.clear_sources();
        state.status_generation = state.status_generation.wrapping_add(1);
    }
}

pub fn ensure_tree_sources(tree: &ElementTree) {
    let sources = collect_tree_sources(tree);
    let active_sources = sources.iter().cloned().collect::<HashSet<_>>();

    if let Ok(guard) = global().lock()
        && let Ok(mut state) = guard.state.lock()
    {
        let removed_ids = state
            .sources
            .iter()
            .filter(|(source, _)| !active_sources.contains(*source))
            .filter_map(|(_, status)| match status {
                AssetStatus::Ready(asset) => Some(asset.id.clone()),
                AssetStatus::Pending | AssetStatus::Failed => None,
            })
            .collect::<HashSet<_>>();

        state
            .sources
            .retain(|source, _| active_sources.contains(source));
        state.pending_count = state
            .sources
            .values()
            .filter(|status| matches!(status, AssetStatus::Pending))
            .count();

        #[cfg(not(test))]
        {
            let retained_ids = state
                .sources
                .values()
                .filter_map(|status| match status {
                    AssetStatus::Ready(asset) => Some(asset.id.clone()),
                    AssetStatus::Pending | AssetStatus::Failed => None,
                })
                .collect::<HashSet<_>>();
            removed_ids
                .iter()
                .filter(|id| !retained_ids.contains(*id))
                .for_each(|id| {
                    state.records.remove(id);
                });
        }
        #[cfg(test)]
        let _ = removed_ids;
    }

    sources.iter().for_each(ensure_source);
}

pub fn snapshot_tree_sources(tree: &ElementTree) {
    let sources = collect_tree_sources(tree);

    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if guard.tx.is_some() {
        drop(guard);
        sources.iter().for_each(ensure_source);
        return;
    }

    if let Ok(mut state) = guard.state.lock() {
        state.pending_count = 0;
        sources.iter().for_each(|source| {
            state.set_source_status(source.clone(), snapshot_status_for_source(source))
        });
    }
}

pub fn snapshot_tree_sources_for_offscreen(tree: &ElementTree) {
    let sources = collect_tree_sources(tree);

    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if let Ok(mut state) = guard.state.lock() {
        state.pending_count = 0;
        sources.iter().for_each(|source| {
            state.set_source_status(source.clone(), snapshot_status_for_source(source))
        });
    }
}

pub fn resolve_tree_sources_sync(
    tree: &ElementTree,
    timeout: Option<Duration>,
) -> Result<(), String> {
    let sources = collect_tree_sources(tree);

    let guard = global()
        .lock()
        .map_err(|_| "failed to lock asset global state".to_string())?;

    let state = Arc::clone(&guard.state);
    drop(guard);

    let config = state
        .lock()
        .map_err(|_| "failed to lock asset state".to_string())?
        .config
        .clone();

    let deadline = timeout.map(|duration| Instant::now() + duration);

    for source in sources {
        ensure_deadline(deadline)?;
        let status = blocking_status_for_source(&source, &config);
        ensure_deadline(deadline)?;

        let mut state = state
            .lock()
            .map_err(|_| "failed to lock asset state".to_string())?;
        state.set_source_status(source, status);
        state.pending_count = 0;
    }

    Ok(())
}

pub fn ensure_source(source: &ImageSource) {
    if let ImageSource::Id(id) = source {
        let dimensions = asset_dimensions(id);
        if let Ok(guard) = global().lock()
            && let Ok(mut state) = guard.state.lock()
        {
            match dimensions {
                Some((width, height)) => {
                    if matches!(state.sources.get(source), Some(AssetStatus::Pending))
                        && state.pending_count > 0
                    {
                        state.pending_count -= 1;
                    }
                    state.set_source_status(
                        source.clone(),
                        AssetStatus::Ready(ResolvedAsset {
                            id: id.clone(),
                            width,
                            height,
                        }),
                    );
                }
                None if !state.sources.contains_key(source) => {
                    state.set_source_status(source.clone(), AssetStatus::Pending);
                    state.pending_count = state.pending_count.saturating_add(1);
                }
                None => {}
            }
        }
        return;
    }

    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    let mut queued_msg = None;

    if let Ok(mut state) = guard.state.lock() {
        match state.sources.get(source) {
            Some(AssetStatus::Pending)
            | Some(AssetStatus::Ready(_))
            | Some(AssetStatus::Failed) => {}
            None => {
                if let Some(asset) = resolved_asset_from_raster_cache(source, &state.config) {
                    let cached_id = asset.id.clone();
                    state.set_source_status(source.clone(), AssetStatus::Ready(asset));
                    queued_msg = Some(AssetMsg::HydrateCached(source.clone(), cached_id));
                } else {
                    state.set_source_status(source.clone(), AssetStatus::Pending);
                    state.pending_count = state.pending_count.saturating_add(1);
                    queued_msg = Some(AssetMsg::Ensure(source.clone()));
                }
            }
        }
    }

    let Some(msg) = queued_msg else {
        return;
    };

    if let Some(tx) = guard.tx.as_ref() {
        match tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(msg)) => {
                let _ = tx.send(msg);
            }
            Err(TrySendError::Disconnected(AssetMsg::Ensure(_))) => {
                if let Ok(mut state) = guard.state.lock()
                    && matches!(state.sources.get(source), Some(AssetStatus::Pending))
                {
                    if state.pending_count > 0 {
                        state.pending_count -= 1;
                    }
                    state.set_source_status(source.clone(), AssetStatus::Failed);
                }
            }
            Err(TrySendError::Disconnected(AssetMsg::HydrateCached(_, _) | AssetMsg::Stop)) => {}
        }
    }
}

fn resolved_asset_from_raster_cache(
    source: &ImageSource,
    config: &AssetConfig,
) -> Option<ResolvedAsset> {
    let path = match source {
        ImageSource::Logical(logical) => resolve_logical_path(logical, config).ok()?,
        ImageSource::RuntimePath(path) => resolve_runtime_path(path, config).ok()?,
        ImageSource::Id(_) => return None,
    };
    let (id, width, height) =
        crate::renderer::cached_raster_asset_for_source(&path.display().to_string())?;
    Some(ResolvedAsset { id, width, height })
}

pub fn source_status(source: &ImageSource) -> Option<AssetStatus> {
    let guard = global().lock().ok()?;
    let state = guard.state.lock().ok()?;
    state.sources.get(source).cloned()
}

pub fn source_dimensions(source: &ImageSource) -> Option<(u32, u32)> {
    match source_status(source) {
        Some(AssetStatus::Ready(asset)) => Some((asset.width, asset.height)),
        _ => None,
    }
}

pub fn source_status_generation() -> u64 {
    let Ok(guard) = global().lock() else {
        return 0;
    };
    let Ok(state) = guard.state.lock() else {
        return 0;
    };

    state.status_generation
}

pub(crate) fn asset_record(id: &str) -> Option<Arc<AssetRecord>> {
    let guard = global().lock().ok()?;
    let state = guard.state.lock().ok()?;
    state.records.get(id).cloned()
}

pub(crate) fn asset_dimensions(id: &str) -> Option<(u32, u32)> {
    asset_record(id).map(|record| (record.width, record.height))
}

pub(crate) fn source_memory_snapshot() -> (usize, u64) {
    let Ok(global) = global().lock() else {
        return (0, 0);
    };
    let Ok(state) = global.state.lock() else {
        return (0, 0);
    };
    let bytes = state
        .records
        .values()
        .map(|record| record.encoded_bytes)
        .fold(0_u64, u64::saturating_add);
    (state.records.len(), bytes)
}

pub(crate) fn register_raster_asset(
    id: &str,
    source: &str,
    bytes: &[u8],
    decode_at_size: bool,
) -> Result<Arc<AssetRecord>, String> {
    let data = Data::new_copy(bytes);
    let codec = Codec::from_data(data.clone())
        .ok_or_else(|| "failed to decode image metadata".to_string())?;
    let dimensions = codec.dimensions();
    let width = u32::try_from(dimensions.width)
        .ok()
        .filter(|width| *width > 0)
        .ok_or_else(|| "invalid raster image width".to_string())?;
    let height = u32::try_from(dimensions.height)
        .ok()
        .filter(|height| *height > 0)
        .ok_or_else(|| "invalid raster image height".to_string())?;

    register_asset_record(AssetRecord {
        id: id.to_string(),
        source: source.to_string(),
        width,
        height,
        encoded_bytes: bytes.len() as u64,
        generation: generation_for_id(id),
        decode_at_size,
        kind: AssetRecordKind::Raster(data),
    })
}

pub(crate) fn register_vector_asset(
    id: &str,
    tree: usvg::Tree,
) -> Result<Arc<AssetRecord>, String> {
    let width = tree.size().width().ceil().max(1.0) as u32;
    let height = tree.size().height().ceil().max(1.0) as u32;

    register_asset_record(AssetRecord {
        id: id.to_string(),
        source: id.to_string(),
        width,
        height,
        encoded_bytes: 0,
        generation: generation_for_id(id),
        decode_at_size: false,
        kind: AssetRecordKind::Vector(Arc::new(tree)),
    })
}

fn register_asset_record(record: AssetRecord) -> Result<Arc<AssetRecord>, String> {
    let record = Arc::new(record);
    let guard = global()
        .lock()
        .map_err(|_| "failed to lock asset global state".to_string())?;
    let mut state = guard
        .state
        .lock()
        .map_err(|_| "failed to lock asset state".to_string())?;
    state.records.insert(record.id.clone(), Arc::clone(&record));
    Ok(record)
}

fn generation_for_id(id: &str) -> u64 {
    id.strip_prefix("img_")
        .and_then(|hex| hex.get(..16))
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .filter(|generation| *generation != 0)
        .unwrap_or_else(|| ASSET_GENERATION.fetch_add(1, Ordering::Relaxed) + 1)
}

#[cfg(test)]
pub(crate) fn remove_asset_record(id: &str) {
    if let Ok(guard) = global().lock()
        && let Ok(mut state) = guard.state.lock()
    {
        state.records.remove(id);
    }
}

impl Worker {
    fn handle_ensure(&mut self, source: ImageSource) {
        let config = match self.state.lock() {
            Ok(state) => state.config.clone(),
            Err(_) => return,
        };

        let result = self.load_source(&source, &config);

        let updated = if let Ok(mut state) = self.state.lock() {
            let was_pending = matches!(state.sources.get(&source), Some(AssetStatus::Pending));
            if !was_pending {
                if let Ok(asset) = result {
                    let still_referenced = state.sources.values().any(|status| {
                        matches!(status, AssetStatus::Ready(ready) if ready.id == asset.id)
                    });
                    if !still_referenced {
                        state.records.remove(&asset.id);
                    }
                }
                false
            } else {
                if state.pending_count > 0 {
                    state.pending_count -= 1;
                }

                match result {
                    Ok(asset) => {
                        state.set_source_status(source.clone(), AssetStatus::Ready(asset));
                    }
                    Err(reason) => {
                        if self.log_render {
                            eprintln!("asset load failed source={source:?} reason={reason}");
                        }
                        state.set_source_status(source.clone(), AssetStatus::Failed);
                    }
                }
                true
            }
        } else {
            false
        };

        if updated {
            send_tree_update(&self.tree_tx, self.log_render);
        }
    }

    fn handle_hydrate_cached(&mut self, source: ImageSource, cached_id: &str) {
        let config = match self.state.lock() {
            Ok(state) => state.config.clone(),
            Err(_) => return,
        };
        let result = self.load_source(&source, &config);

        let updated = if let Ok(mut state) = self.state.lock() {
            let still_using_cached = matches!(
                state.sources.get(&source),
                Some(AssetStatus::Ready(asset)) if asset.id == cached_id
            );

            match (still_using_cached, result) {
                (true, Ok(asset)) => {
                    if asset.id != cached_id {
                        state.set_source_status(source.clone(), AssetStatus::Ready(asset));
                    }
                    true
                }
                (true, Err(_)) => false,
                (false, Ok(asset)) => {
                    let still_referenced = state.sources.values().any(|status| {
                        matches!(status, AssetStatus::Ready(ready) if ready.id == asset.id)
                    });
                    if !still_referenced {
                        state.records.remove(&asset.id);
                    }
                    false
                }
                (false, Err(_)) => false,
            }
        } else {
            false
        };

        if updated {
            send_tree_update(&self.tree_tx, self.log_render);
        }
    }

    fn load_source(
        &mut self,
        source: &ImageSource,
        config: &AssetConfig,
    ) -> Result<ResolvedAsset, String> {
        load_source(source, config)
    }
}

fn collect_tree_sources(tree: &ElementTree) -> Vec<ImageSource> {
    tree.iter_nodes()
        .flat_map(|element| {
            element
                .spec
                .declared
                .image_src
                .iter()
                .cloned()
                .chain(
                    element
                        .spec
                        .declared
                        .background
                        .iter()
                        .filter_map(background_image_source),
                )
                .chain(
                    element
                        .spec
                        .declared
                        .mouse_over
                        .iter()
                        .filter_map(|mouse_over| mouse_over.background.as_ref())
                        .filter_map(background_image_source),
                )
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

fn background_image_source(background: &Background) -> Option<ImageSource> {
    match background {
        Background::Image { source, .. } => Some(source.clone()),
        _ => None,
    }
}

fn snapshot_status_for_source(source: &ImageSource) -> AssetStatus {
    match source {
        ImageSource::Id(id) => {
            asset_dimensions(id).map_or(AssetStatus::Pending, |(width, height)| {
                AssetStatus::Ready(ResolvedAsset {
                    id: id.clone(),
                    width,
                    height,
                })
            })
        }
        ImageSource::Logical(_) | ImageSource::RuntimePath(_) => AssetStatus::Pending,
    }
}

fn blocking_status_for_source(source: &ImageSource, config: &AssetConfig) -> AssetStatus {
    load_source(source, config).map_or(AssetStatus::Failed, AssetStatus::Ready)
}

fn ensure_deadline(deadline: Option<Instant>) -> Result<(), String> {
    if let Some(deadline) = deadline
        && Instant::now() > deadline
    {
        return Err("asset preload timed out".to_string());
    }

    Ok(())
}

fn load_source(source: &ImageSource, config: &AssetConfig) -> Result<ResolvedAsset, String> {
    match source {
        ImageSource::Id(id) => {
            let (width, height) =
                asset_dimensions(id).ok_or_else(|| format!("unknown image id: {id}"))?;
            Ok(ResolvedAsset {
                id: id.clone(),
                width,
                height,
            })
        }
        ImageSource::Logical(logical) => {
            let path = resolve_logical_path(logical, config)?;
            load_path(&path, config)
        }
        ImageSource::RuntimePath(path) => {
            let resolved = resolve_runtime_path(path, config)?;
            load_path(&resolved, config)
        }
    }
}

fn load_path(path: &Path, config: &AssetConfig) -> Result<ResolvedAsset, String> {
    let bytes =
        fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;

    let id = canonical_asset_id(&bytes);

    let (width, height) = match asset_dimensions(&id) {
        Some((w, h)) => (w, h),
        None if path_is_svg(path) => load_svg_asset(path, &id, &bytes)?,
        None => insert_raster_asset_with_policy(
            &id,
            &path.display().to_string(),
            &bytes,
            config.decode_at_size,
        )?,
    };

    Ok(ResolvedAsset { id, width, height })
}

fn load_svg_asset(path: &Path, id: &str, bytes: &[u8]) -> Result<(u32, u32), String> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();

    let tree = usvg::Tree::from_data_nested(bytes, &options)
        .map_err(|err| format!("failed to parse SVG {}: {err}", path.display()))?;

    let (width, height) = svg_dimensions(&tree).unwrap_or((64, 64));
    insert_vector_asset(id, tree)
        .map(|_| (width, height))
        .map_err(|reason| format!("failed to cache SVG {}: {reason}", path.display()))
}

fn svg_dimensions(tree: &usvg::Tree) -> Option<(u32, u32)> {
    positive_dimensions(tree.size().width(), tree.size().height())
}

fn positive_dimensions(width: f32, height: f32) -> Option<(u32, u32)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }

    Some((width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32))
}

fn path_is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
}

pub(crate) fn resolve_logical_path(logical: &str, config: &AssetConfig) -> Result<PathBuf, String> {
    let relative = logical_asset_relative_path(logical)?;

    if config.sources.is_empty() {
        return Err("asset sources are empty".to_string());
    }

    for source in &config.sources {
        let source_root = PathBuf::from(source);
        let candidate = source_root.join(&relative);
        if fs::metadata(&candidate)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
        {
            return Ok(candidate);
        }
    }

    Err(format!("logical asset not found: {logical}"))
}

fn send_tree_update(tree_tx: &Sender<TreeMsg>, log_render: bool) {
    match tree_tx.try_send(TreeMsg::AssetStateChanged) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking asset update send");
            }
            let _ = tree_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn normalize_config(config: AssetConfig) -> AssetConfig {
    let mut normalized = config;
    normalized.sources = normalized
        .sources
        .into_iter()
        .map(|path| expand_path(path.as_str()))
        .collect();
    normalized.runtime_allowlist = normalized
        .runtime_allowlist
        .into_iter()
        .map(|path| expand_path(path.as_str()))
        .collect();
    normalized.runtime_extensions = normalized
        .runtime_extensions
        .into_iter()
        .map(|ext| ext.to_lowercase())
        .collect();
    normalized
}

fn logical_asset_relative_path(logical: &str) -> Result<PathBuf, String> {
    let trimmed = logical.trim();
    let without_prefix = trimmed.trim_start_matches('/');
    if without_prefix.is_empty() {
        return Err(format!("logical asset path is empty: {logical}"));
    }

    let mut out = PathBuf::new();
    for component in Path::new(without_prefix).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => out.push(segment),
            Component::ParentDir => {
                return Err(format!(
                    "logical asset path may not contain '..': {logical}"
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("logical asset path must be relative: {logical}"));
            }
        }
    }

    if out.as_os_str().is_empty() {
        return Err(format!("logical asset path is empty: {logical}"));
    }

    Ok(out)
}

fn canonical_asset_id(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    format!("img_{digest:x}")
}

pub(crate) fn resolve_runtime_path(path: &str, config: &AssetConfig) -> Result<PathBuf, String> {
    if !config.runtime_enabled {
        return Err(format!("runtime paths disabled: {path}"));
    }

    let expanded = PathBuf::from(expand_path(path));
    let metadata = fs::metadata(&expanded)
        .map_err(|err| format!("failed to stat runtime path {}: {err}", expanded.display()))?;

    if !metadata.is_file() {
        return Err(format!(
            "runtime path is not a file: {}",
            expanded.display()
        ));
    }

    let extension = expanded
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_lowercase()))
        .unwrap_or_default();

    if !config
        .runtime_extensions
        .iter()
        .any(|allowed| allowed == &extension)
    {
        return Err(format!(
            "runtime extension not allowed: {}",
            expanded.display()
        ));
    }

    if metadata.len() > config.runtime_max_file_size {
        return Err(format!(
            "runtime file too large: {} ({} > {})",
            expanded.display(),
            metadata.len(),
            config.runtime_max_file_size
        ));
    }

    let resolved = if config.runtime_follow_symlinks {
        fs::canonicalize(&expanded).map_err(|err| {
            format!(
                "failed to canonicalize runtime path {}: {err}",
                expanded.display()
            )
        })?
    } else {
        if path_has_symlink_component(&expanded)? {
            return Err(format!("symlink not allowed: {}", expanded.display()));
        }
        expanded
    };

    if config.runtime_allowlist.is_empty() {
        return Err("runtime allowlist is empty".to_string());
    }

    let mut allowed = false;
    for root in &config.runtime_allowlist {
        let root_path = PathBuf::from(root);
        let root_path = if config.runtime_follow_symlinks {
            fs::canonicalize(&root_path).unwrap_or(root_path)
        } else {
            root_path
        };

        if resolved.starts_with(&root_path) {
            allowed = true;
            break;
        }
    }

    if !allowed {
        return Err(format!(
            "runtime path not allowlisted: {}",
            resolved.display()
        ));
    }

    Ok(resolved)
}

fn path_has_symlink_component(path: &Path) -> Result<bool, String> {
    let mut current = PathBuf::new();

    for component in path.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|err| format!("failed to lstat {}: {err}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }

    Ok(false)
}

fn expand_path(path: &str) -> String {
    if path == "~" {
        return std::env::var("HOME").unwrap_or_else(|_| path.to_string());
    }

    if let Some(suffix) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(suffix).display().to_string();
    }

    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return candidate.display().to_string();
    }

    match std::env::current_dir() {
        Ok(cwd) => cwd.join(candidate).display().to_string(),
        Err(_) => candidate.display().to_string(),
    }
}

#[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
pub(crate) fn resolve_configured_path(path: &str, config: &AssetConfig) -> Result<PathBuf, String> {
    if Path::new(path).is_absolute() {
        resolve_runtime_path(path, config)
    } else {
        resolve_logical_path(path, config)
    }
}
