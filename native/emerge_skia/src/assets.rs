use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, TrySendError, bounded};
use resvg::usvg;
use sha2::{Digest, Sha256};
use skia_safe::{Data, codec::Codec};

use crate::actors::TreeMsg;
use crate::renderer::configure_asset_cache;

use crate::tree::attrs::{Background, ImageSource};
use crate::tree::element::ElementTree;

mod svg_cache;
pub use svg_cache::SvgCacheStats;
use svg_cache::{SvgTreeCache, estimated_font_bytes, estimated_tree_bytes};

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
    pub svg_tree_max_entries: u64,
    pub svg_tree_max_bytes: u64,
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
            svg_tree_max_entries: 64,
            svg_tree_max_bytes: 16 * 1024 * 1024,
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
    owner: std::sync::Weak<Mutex<AssetState>>,
    pub id: String,
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub encoded_bytes: u64,
    pub generation: u64,
    pub render_revision: Arc<AtomicU64>,
    pub epoch: u64,
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

const LOADING_INDICATOR_DELAY: Duration = Duration::from_millis(100);

#[derive(Default)]
struct AssetState {
    config: AssetConfig,
    sources: HashMap<ImageSource, AssetStatus>,
    // Some(deadline) is the silent grace period; None is a visible indicator.
    // Entries belong to active pending sources, not retained cache records.
    loading_indicators: HashMap<ImageSource, Option<Instant>>,
    records: HashMap<String, Arc<AssetRecord>>,
    pending_count: usize,
    status_generation: u64,
    epoch: u64,
    request_clock: u64,
    requests: HashMap<ImageSource, u64>,
    source_links: HashMap<ImageSource, (ResolvedAsset, u64)>,
    svg_cache: SvgTreeCache,
}

impl AssetState {
    fn clear_sources(&mut self) {
        if !self.sources.is_empty() || self.pending_count != 0 {
            self.status_generation = self.status_generation.wrapping_add(1);
        }
        self.sources.clear();
        self.loading_indicators.clear();
        self.records.clear();
        self.requests.clear();
        self.source_links.clear();
        self.pending_count = 0;
    }

    fn set_source_status(&mut self, source: ImageSource, status: AssetStatus) {
        if self.sources.get(&source) == Some(&status) {
            return;
        }

        if matches!(status, AssetStatus::Pending) {
            self.loading_indicators.insert(
                source.clone(),
                Some(Instant::now() + LOADING_INDICATOR_DELAY),
            );
        } else {
            self.loading_indicators.remove(&source);
        }

        if let AssetStatus::Ready(asset) = &status {
            self.request_clock = self.request_clock.wrapping_add(1);
            self.source_links
                .insert(source.clone(), (asset.clone(), self.request_clock));
            let limit = self
                .config
                .cache_max_entries
                .saturating_add(self.config.svg_tree_max_entries)
                .max(1);
            while self.source_links.len() as u64 > limit {
                let Some(oldest) = self
                    .source_links
                    .iter()
                    .min_by_key(|(_, (_, used))| used)
                    .map(|(source, _)| source.clone())
                else {
                    break;
                };
                self.source_links.remove(&oldest);
            }
        }
        if let Some(AssetStatus::Ready(previous)) = self.sources.insert(source, status)
            && !self.sources.values().any(
                |status| matches!(status, AssetStatus::Ready(asset) if asset.id == previous.id),
            )
        {
            self.records.remove(&previous.id);
        }
        self.status_generation = self.status_generation.wrapping_add(1);
    }
}

struct SvgFontEnvironment {
    epoch: u64,
    fonts: Arc<usvg::fontdb::Database>,
}

#[derive(Clone)]
pub struct AssetContext {
    state: Arc<Mutex<AssetState>>,
    tx: Arc<Mutex<Option<Sender<AssetMsg>>>>,
    renderer: Arc<crate::renderer::RendererAssetContext>,
    generation: Arc<AtomicU64>,
    // Serializes source parsing, not rendering or source-status lookups.
    svg_loader: Arc<Mutex<()>>,
    svg_fonts: Arc<Mutex<Option<SvgFontEnvironment>>>,
}

impl Default for AssetContext {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(AssetState::default())),
            tx: Arc::new(Mutex::new(None)),
            renderer: Arc::new(crate::renderer::RendererAssetContext::default()),
            generation: Arc::new(AtomicU64::new(1)),
            svg_loader: Arc::new(Mutex::new(())),
            svg_fonts: Arc::new(Mutex::new(None)),
        }
    }
}

pub struct AssetRuntime {
    tree_tx: Mutex<Option<Sender<TreeMsg>>>,
    context: AssetContext,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Default for AssetRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetRuntime {
    pub fn new() -> Self {
        Self {
            tree_tx: Mutex::new(None),
            context: AssetContext::default(),
            worker: Mutex::new(None),
        }
    }

    pub fn context(&self) -> AssetContext {
        self.context.clone()
    }

    pub fn enter(&self) -> AssetContextGuard {
        self.context.enter()
    }

    pub fn start(&self, tree_tx: Sender<TreeMsg>, log_render: bool) {
        self.stop_worker();
        if let Ok(mut target) = self.tree_tx.lock() {
            *target = Some(tree_tx.clone());
        }
        let (tx, rx) = bounded(4096);

        if let Ok(mut state) = self.context.state.lock() {
            state.clear_sources();
        }
        if let Ok(mut current_tx) = self.context.tx.lock() {
            *current_tx = Some(tx);
        }

        let context = self.context.clone();
        let state = Arc::clone(&context.state);
        let handle = thread::spawn(move || {
            let _context_guard = context.enter();
            let mut worker = Worker {
                tree_tx,
                state,
                log_render,
            };

            while let Ok(msg) = rx.recv() {
                match msg {
                    AssetMsg::Stop => break,
                    AssetMsg::Load(request) => worker.handle_load(request),
                }
            }
        });

        if let Ok(mut worker) = self.worker.lock() {
            *worker = Some(handle);
        }
    }

    pub fn stop(&self) {
        self.stop_worker();
        let _context_guard = self.enter();
        if let Ok(mut state) = self.context.state.lock() {
            state.clear_sources();
            state.epoch = state.epoch.wrapping_add(1);
            state.svg_cache = SvgTreeCache::default();
        }
        if let Ok(mut fonts) = self.context.svg_fonts.lock() {
            *fonts = None;
        }
        crate::renderer::clear_renderer_asset_context();
    }

    pub fn configure(&self, config: AssetConfig) {
        let _context_guard = self.enter();
        configure(config);
    }

    pub(crate) fn notify_font_metrics_changed(&self) {
        let target = self.tree_tx.lock().ok().and_then(|target| target.clone());
        if let Some(target) = target {
            let _context = self.enter();
            let msg = TreeMsg::FontMetricsChanged {
                generation: crate::renderer::live_font_cache_generation(),
            };
            if let Err(crossbeam_channel::TrySendError::Full(msg)) = target.try_send(msg) {
                let _ = target.send(msg);
            }
        }
    }
    fn stop_worker(&self) {
        if let Ok(mut target) = self.tree_tx.lock() {
            *target = None;
        }
        if let Ok(mut tx) = self.context.tx.lock()
            && let Some(tx) = tx.take()
        {
            let _ = tx.send(AssetMsg::Stop);
        }

        let handle = self.worker.lock().ok().and_then(|mut worker| worker.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

impl Drop for AssetRuntime {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

impl AssetContext {
    pub fn enter(&self) -> AssetContextGuard {
        let previous = CURRENT_ASSET_CONTEXT.with(|current| current.replace(Some(self.clone())));
        AssetContextGuard { previous }
    }
}

pub struct AssetContextGuard {
    previous: Option<AssetContext>,
}

impl Drop for AssetContextGuard {
    fn drop(&mut self) {
        CURRENT_ASSET_CONTEXT.with(|current| {
            current.replace(self.previous.take());
        });
    }
}

thread_local! {
    static CURRENT_ASSET_CONTEXT: RefCell<Option<AssetContext>> = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    static TEST_ASSET_CONTEXT: AssetContext = AssetContext::default();
}

fn current_context() -> AssetContext {
    CURRENT_ASSET_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_else(missing_current_context)
}

#[cfg(test)]
fn missing_current_context() -> AssetContext {
    TEST_ASSET_CONTEXT.with(Clone::clone)
}

#[cfg(not(test))]
fn missing_current_context() -> AssetContext {
    panic!("renderer asset access requires an active renderer asset context")
}

pub(crate) fn current_renderer_context() -> Arc<crate::renderer::RendererAssetContext> {
    current_context().renderer
}

enum AssetMsg {
    Load(LoadRequest),
    Stop,
}

struct LoadRequest {
    source: ImageSource,
    epoch: u64,
    token: u64,
    config: AssetConfig,
    require_record: bool,
}

struct Worker {
    tree_tx: Sender<TreeMsg>,
    state: Arc<Mutex<AssetState>>,
    log_render: bool,
}

pub fn configure(config: AssetConfig) {
    let context = current_context();
    let config = normalize_config(config);
    if let Ok(mut state) = context.state.lock() {
        state.clear_sources();
        state.epoch = state.epoch.wrapping_add(1);
        state.svg_cache = SvgTreeCache::default();
        state
            .svg_cache
            .configure(config.svg_tree_max_entries, config.svg_tree_max_bytes);
        state.svg_cache.stats.font_environment_generation = state.epoch;
        state.config = config.clone();
        state.status_generation = state.status_generation.wrapping_add(1);
    }
    crate::renderer::clear_cached_svg_pixels();
    configure_asset_cache(config.cache_max_entries, config.cache_max_bytes);
}

pub fn ensure_tree_sources(tree: &ElementTree) {
    let sources = collect_tree_sources(tree);
    let active_sources = sources.iter().cloned().collect::<HashSet<_>>();
    let context = current_context();
    if let Ok(mut state) = context.state.lock() {
        let removed_ids: HashSet<_> = state
            .sources
            .iter()
            .filter(|(source, _)| !active_sources.contains(*source))
            .filter_map(|(_, status)| match status {
                AssetStatus::Ready(asset) => Some(asset.id.clone()),
                _ => None,
            })
            .collect();
        let removed = state.sources.len();
        state
            .sources
            .retain(|source, _| active_sources.contains(source));
        state
            .requests
            .retain(|source, _| active_sources.contains(source));
        state
            .loading_indicators
            .retain(|source, _| active_sources.contains(source));
        if removed != state.sources.len() {
            state.status_generation = state.status_generation.wrapping_add(1);
        }
        let retained_ids: HashSet<_> = state
            .sources
            .values()
            .filter_map(|status| match status {
                AssetStatus::Ready(asset) => Some(asset.id.clone()),
                _ => None,
            })
            .collect();
        for id in removed_ids.difference(&retained_ids) {
            state.records.remove(id);
        }
        state.pending_count = state
            .sources
            .values()
            .filter(|status| matches!(status, AssetStatus::Pending))
            .count();
    }
    sources.iter().for_each(ensure_source);
}

pub fn snapshot_tree_sources(tree: &ElementTree) {
    let context = current_context();
    if context.tx.lock().is_ok_and(|tx| tx.is_some()) {
        collect_tree_sources(tree).iter().for_each(ensure_source);
    } else {
        snapshot_tree_sources_for_offscreen(tree);
    }
}

pub fn snapshot_tree_sources_for_offscreen(tree: &ElementTree) {
    // Metadata lookup may acquire asset state; resolve before taking that lock.
    let statuses: Vec<_> = collect_tree_sources(tree)
        .into_iter()
        .map(|source| {
            let status = snapshot_status_for_source(&source);
            (source, status)
        })
        .collect();
    if let Ok(mut state) = current_context().state.lock() {
        state.pending_count = 0;
        for (source, status) in statuses {
            state.set_source_status(source, status);
        }
    }
}

pub fn resolve_tree_sources_sync(
    tree: &ElementTree,
    timeout: Option<Duration>,
) -> Result<(), String> {
    let sources = collect_tree_sources(tree);

    let context = current_context();
    let state = Arc::clone(&context.state);

    let (config, epoch) = {
        let state = state
            .lock()
            .map_err(|_| "failed to lock asset state".to_string())?;
        (state.config.clone(), state.epoch)
    };

    let deadline = timeout.map(|duration| Instant::now() + duration);

    for source in sources {
        ensure_deadline(deadline)?;
        let status = load_source_in_epoch(&source, &config, epoch, true)
            .map_or(AssetStatus::Failed, AssetStatus::Ready);
        ensure_deadline(deadline)?;

        let mut state = state
            .lock()
            .map_err(|_| "failed to lock asset state".to_string())?;
        if state.epoch != epoch {
            return Err("asset configuration changed during load".into());
        }
        state.set_source_status(source, status);
        state.pending_count = 0;
    }

    Ok(())
}

pub fn ensure_source(source: &ImageSource) {
    if FRAME_ASSETS.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|frame| frame.matches_current())
    }) {
        return;
    }
    ensure_live_source(source);
}

// Only preparation/explicit live callers register inputs. Frozen frame lookups
// must not enqueue loads or repopulate source state after an epoch reset.
pub(crate) fn ensure_live_source(source: &ImageSource) {
    let context = current_context();
    if let ImageSource::Id(id) = source {
        let dimensions = asset_dimensions(id)
            .or_else(|| crate::renderer::retained_asset_metadata(id).map(|m| (m.width, m.height)));
        if let Ok(mut state) = context.state.lock() {
            if let Some((width, height)) = dimensions {
                if matches!(state.sources.get(source), Some(AssetStatus::Pending)) {
                    state.pending_count = state.pending_count.saturating_sub(1);
                }
                state.set_source_status(
                    source.clone(),
                    AssetStatus::Ready(ResolvedAsset {
                        id: id.clone(),
                        width,
                        height,
                    }),
                );
            } else if !state.sources.contains_key(source) {
                state.set_source_status(source.clone(), AssetStatus::Pending);
                state.pending_count += 1;
            }
        }
        return;
    }
    let (config, epoch, linked) = {
        let Ok(state) = context.state.lock() else {
            return;
        };
        if state.sources.contains_key(source) {
            return;
        }
        (
            state.config.clone(),
            state.epoch,
            state
                .source_links
                .get(source)
                .map(|(asset, _)| asset.clone()),
        )
    };
    // Validate the requested path under the current policy, even on cache hits.
    let cached = resolved_source_path(source, &config).ok().and_then(|path| {
        linked
            .and_then(|asset| {
                asset_dimensions(&asset.id)
                    .map(|(width, height)| ResolvedAsset {
                        width,
                        height,
                        ..asset.clone()
                    })
                    .or_else(|| {
                        crate::renderer::retained_asset_metadata(&asset.id).map(|m| ResolvedAsset {
                            id: m.id,
                            width: m.width,
                            height: m.height,
                        })
                    })
            })
            .or_else(|| {
                context
                    .state
                    .lock()
                    .ok()?
                    .svg_cache
                    .get_for_source(&path.display().to_string())
                    .map(|record| ResolvedAsset {
                        id: record.id.clone(),
                        width: record.width,
                        height: record.height,
                    })
            })
            .or_else(|| {
                crate::renderer::cached_asset_for_source(&path.display().to_string()).map(|m| {
                    ResolvedAsset {
                        id: m.id,
                        width: m.width,
                        height: m.height,
                    }
                })
            })
    });
    let request = {
        let Ok(mut state) = context.state.lock() else {
            return;
        };
        if state.epoch != epoch || state.sources.contains_key(source) {
            return;
        }
        let require_record = cached.is_none();
        state.set_source_status(
            source.clone(),
            cached.map_or(AssetStatus::Pending, AssetStatus::Ready),
        );
        if require_record {
            state.pending_count += 1;
        }
        new_load_request(&mut state, source.clone(), require_record)
    };
    send_load_request(&context, request);
}

fn new_load_request(
    state: &mut AssetState,
    source: ImageSource,
    require_record: bool,
) -> LoadRequest {
    state.request_clock = state.request_clock.wrapping_add(1);
    let token = state.request_clock;
    state.requests.insert(source.clone(), token);
    LoadRequest {
        source,
        token,
        require_record,
        epoch: state.epoch,
        config: state.config.clone(),
    }
}

fn send_load_request(context: &AssetContext, request: LoadRequest) {
    let tx = context.tx.lock().ok().and_then(|tx| tx.clone());
    if let Some(tx) = tx {
        // Asset I/O remains on the worker. Status/cache locks are not held here.
        if let Err(error) = tx.send(AssetMsg::Load(request))
            && let AssetMsg::Load(request) = error.0
            && let Ok(mut state) = context.state.lock()
            && state.requests.get(&request.source) == Some(&request.token)
        {
            state.requests.remove(&request.source);
            state.pending_count = state.pending_count.saturating_sub(usize::from(matches!(
                state.sources.get(&request.source),
                Some(AssetStatus::Pending)
            )));
            state.set_source_status(request.source, AssetStatus::Failed);
        }
    } else if let Ok(mut state) = context.state.lock() {
        state.requests.remove(&request.source);
    }
}

/// A retained pixel-only source needs parsing only if a requested variant misses.
pub(crate) fn request_asset_hydration(id: &str) {
    let context = current_context();
    let request = {
        let Ok(mut state) = context.state.lock() else {
            return;
        };
        let source = state.sources.iter().find_map(|(source, status)| {
            matches!(status, AssetStatus::Ready(asset) if asset.id == id).then(|| source.clone())
        });
        let Some(source) = source else {
            return;
        };
        if state.requests.contains_key(&source) {
            return;
        }
        new_load_request(&mut state, source, true)
    };
    crate::renderer::invalidate_asset_render_revision(id);
    send_load_request(&context, request);
}

fn resolved_source_path(source: &ImageSource, config: &AssetConfig) -> Result<PathBuf, String> {
    match source {
        ImageSource::Logical(path) => resolve_logical_path(path, config),
        ImageSource::RuntimePath(path) => resolve_runtime_path(path, config),
        ImageSource::Id(_) => Err("preloaded asset has no path".into()),
    }
}

pub(crate) fn current_epoch() -> u64 {
    current_context()
        .state
        .lock()
        .map(|state| state.epoch)
        .unwrap_or(0)
}

pub fn svg_cache_stats() -> SvgCacheStats {
    current_context()
        .state
        .lock()
        .map(|state| state.svg_cache.stats.clone())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn reset_svg_rasterization_count() {
    if let Ok(mut state) = current_context().state.lock() {
        state.svg_cache.stats.rasterizations = 0;
    }
}

pub(crate) fn record_svg_rasterization() {
    if let Ok(mut state) = current_context().state.lock() {
        state.svg_cache.stats.rasterizations += 1;
    }
}

/// One-shot paint deadline, serviced by the tree actor or the existing host tick.
/// Asset loading itself is never delayed and does not wait for this deadline.
pub fn next_loading_indicator_deadline() -> Option<Instant> {
    current_context()
        .state
        .lock()
        .ok()?
        .loading_indicators
        .values()
        .filter_map(|deadline| *deadline)
        .min()
}

pub(crate) fn advance_loading_indicators(now: Instant) -> bool {
    let context = current_context();
    let Ok(mut state) = context.state.lock() else {
        return false;
    };
    let changed = state
        .loading_indicators
        .values_mut()
        .fold(false, |changed, deadline| {
            if deadline.is_some_and(|at| at <= now) {
                *deadline = None;
                true
            } else {
                changed
            }
        });
    if changed {
        state.status_generation = state.status_generation.wrapping_add(1);
    }
    changed
}

pub(crate) fn loading_indicator_visible(source: &ImageSource) -> bool {
    if let Some(visible) = FRAME_ASSETS.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|frame| frame.matches_current())
            .map(|frame| frame.loading_indicators.contains(source))
    }) {
        return visible;
    }
    current_context()
        .state
        .lock()
        .is_ok_and(|state| state.loading_indicators.get(source) == Some(&None))
}

pub fn source_status(source: &ImageSource) -> Option<AssetStatus> {
    if let Some(status) = frame_source_status(source) {
        return status;
    }
    let context = current_context();
    let state = context.state.lock().ok()?;
    state.sources.get(source).cloned()
}

pub fn source_dimensions(source: &ImageSource) -> Option<(u32, u32)> {
    match source_status(source) {
        Some(AssetStatus::Ready(asset)) => Some((asset.width, asset.height)),
        _ => None,
    }
}

pub fn source_status_generation() -> u64 {
    if let Some(generation) = FRAME_ASSETS.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|frame| frame.matches_current())
            .map(|frame| frame.status_generation)
    }) {
        return generation;
    }
    let context = current_context();
    let Ok(state) = context.state.lock() else {
        return 0;
    };

    state.status_generation
}

pub(crate) fn has_scene_image_sources() -> bool {
    current_context()
        .state
        .lock()
        .map(|state| !state.records.is_empty() || state.svg_cache.stats.entries != 0)
        .unwrap_or(true)
}

pub(crate) fn record_belongs_to_current(record: &AssetRecord) -> bool {
    record
        .owner
        .ptr_eq(&Arc::downgrade(&current_context().state))
}

pub(crate) fn asset_record(id: &str) -> Option<Arc<AssetRecord>> {
    let context = current_context();
    let mut state = context.state.lock().ok()?;
    state
        .records
        .get(id)
        .cloned()
        .or_else(|| state.svg_cache.get(id))
}

pub(crate) fn asset_dimensions(id: &str) -> Option<(u32, u32)> {
    asset_record(id).map(|record| (record.width, record.height))
}

pub(crate) fn source_memory_snapshot() -> (usize, u64) {
    let context = current_context();
    let Ok(state) = context.state.lock() else {
        return (0, 0);
    };
    let bytes = state
        .records
        .values()
        .filter_map(|record| match &record.kind {
            AssetRecordKind::Raster(data) => Some(data.len() as u64),
            AssetRecordKind::Vector(_) => None,
        })
        .fold(0_u64, u64::saturating_add);
    (state.records.len(), bytes)
}

pub(crate) fn register_raster_asset(
    id: &str,
    source: &str,
    bytes: &[u8],
    decode_at_size: bool,
) -> Result<Arc<AssetRecord>, String> {
    register_raster_asset_in_epoch(id, source, bytes, decode_at_size, current_epoch())
}

fn register_raster_asset_in_epoch(
    id: &str,
    source: &str,
    bytes: &[u8],
    decode_at_size: bool,
    epoch: u64,
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
        owner: Arc::downgrade(&current_context().state),
        id: id.to_string(),
        source: source.to_string(),
        width,
        height,
        encoded_bytes: bytes.len() as u64,
        generation: generation_for_id(id),
        render_revision: Arc::new(AtomicU64::new(next_render_revision())),
        epoch,
        decode_at_size,
        kind: AssetRecordKind::Raster(data),
    })
}

pub(crate) fn register_vector_asset(
    id: &str,
    tree: usvg::Tree,
) -> Result<Arc<AssetRecord>, String> {
    register_vector_asset_in_epoch(id, id, tree, 0, current_epoch(), generation_for_id(id))
}

fn register_vector_asset_in_epoch(
    id: &str,
    source: &str,
    tree: usvg::Tree,
    encoded_bytes: u64,
    epoch: u64,
    generation: u64,
) -> Result<Arc<AssetRecord>, String> {
    register_asset_record(AssetRecord {
        owner: Arc::downgrade(&current_context().state),
        id: id.to_string(),
        source: source.to_string(),
        width: tree.size().width().ceil().max(1.0) as u32,
        height: tree.size().height().ceil().max(1.0) as u32,
        encoded_bytes,
        generation,
        render_revision: crate::renderer::retained_asset_metadata(id)
            .filter(|metadata| metadata.generation == generation)
            .and_then(|metadata| metadata.render_revision)
            .unwrap_or_else(|| Arc::new(AtomicU64::new(next_render_revision()))),
        epoch,
        decode_at_size: false,
        kind: AssetRecordKind::Vector(Arc::new(tree)),
    })
}

fn register_asset_record(record: AssetRecord) -> Result<Arc<AssetRecord>, String> {
    let charge = estimated_tree_bytes(&record);
    let record = Arc::new(record);
    let context = current_context();
    let mut state = context
        .state
        .lock()
        .map_err(|_| "failed to lock asset state".to_string())?;
    if record.epoch != state.epoch {
        return Err("asset configuration changed during load".into());
    }
    state.svg_cache.remove(&record.id);
    if matches!(record.kind, AssetRecordKind::Vector(_)) {
        state.svg_cache.insert(Arc::clone(&record), charge);
    }
    state.records.insert(record.id.clone(), Arc::clone(&record));
    Ok(record)
}

pub(crate) fn next_render_revision() -> u64 {
    current_context().generation.fetch_add(1, Ordering::Relaxed) + 1
}

fn generation_for_id(_id: &str) -> u64 {
    next_render_revision()
}

#[cfg(test)]
pub(crate) fn remove_asset_record(id: &str) {
    let context = current_context();
    if let Ok(mut state) = context.state.lock() {
        state.records.remove(id);
        state.svg_cache.remove(id);
    }
}

impl Worker {
    fn handle_load(&mut self, request: LoadRequest) {
        if !self.state.lock().is_ok_and(|state| {
            state.epoch == request.epoch
                && state.requests.get(&request.source) == Some(&request.token)
        }) {
            return;
        }
        let result = load_source_in_epoch(
            &request.source,
            &request.config,
            request.epoch,
            request.require_record,
        );
        self.complete_load(request, result);
    }

    fn complete_load(&mut self, request: LoadRequest, result: Result<ResolvedAsset, String>) {
        let updated = if let Ok(mut state) = self.state.lock() {
            if state.epoch != request.epoch {
                return;
            }
            if state.requests.get(&request.source) != Some(&request.token) {
                if let Ok(asset) = result
                    && !state.sources.values().any(|status| matches!(status, AssetStatus::Ready(ready) if ready.id == asset.id))
                { state.records.remove(&asset.id); }
                false
            } else {
                state.requests.remove(&request.source);
                if matches!(
                    state.sources.get(&request.source),
                    Some(AssetStatus::Pending)
                ) {
                    state.pending_count = state.pending_count.saturating_sub(1);
                }
                let status = result.map_or_else(
                    |reason| {
                        if self.log_render {
                            eprintln!(
                                "asset load failed source={:?} reason={reason}",
                                request.source
                            );
                        }
                        AssetStatus::Failed
                    },
                    AssetStatus::Ready,
                );
                state.set_source_status(request.source, status);
                true // record hydration can change drawing even when status did not change
            }
        } else {
            false
        };
        if updated {
            send_tree_update(&self.tree_tx, self.log_render);
        }
    }
}

pub(crate) fn collect_tree_sources(tree: &ElementTree) -> Vec<ImageSource> {
    tree_source_refs(tree)
        .map(|(source, _)| source)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}
fn tree_source_refs(
    tree: &ElementTree,
) -> impl Iterator<Item = (ImageSource, Option<crate::tree::element::NodeId>)> + '_ {
    tree.iter_nodes().flat_map(|element| {
        let attrs = &element.spec.declared;
        attrs
            .image_src
            .iter()
            .cloned()
            .map(move |source| {
                (
                    source,
                    (element.spec.kind == crate::tree::element::ElementKind::Image)
                        .then_some(element.id),
                )
            })
            .chain(
                attrs
                    .background
                    .iter()
                    .chain(
                        attrs
                            .mouse_over
                            .iter()
                            .chain(attrs.mouse_down.iter())
                            .chain(attrs.focused.iter())
                            .filter_map(|style| style.background.as_ref()),
                    )
                    .filter_map(background_image_source)
                    .map(|source| (source, None)),
            )
    })
}
#[derive(Clone, Debug)]
pub(crate) struct FrameSourceList {
    pub(crate) all: Vec<ImageSource>,
    pub(crate) measured: Vec<(crate::tree::element::NodeId, ImageSource)>,
}
impl FrameSourceList {
    pub(crate) fn capture(tree: &ElementTree) -> Self {
        let refs = tree_source_refs(tree).collect::<Vec<_>>();
        let all = refs
            .iter()
            .map(|(source, _)| source.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let measured = refs
            .into_iter()
            .filter_map(|(source, id)| id.map(|id| (id, source)))
            .collect();
        Self { all, measured }
    }
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

fn ensure_deadline(deadline: Option<Instant>) -> Result<(), String> {
    if let Some(deadline) = deadline
        && Instant::now() > deadline
    {
        return Err("asset preload timed out".to_string());
    }

    Ok(())
}

fn load_source_in_epoch(
    source: &ImageSource,
    config: &AssetConfig,
    epoch: u64,
    require_record: bool,
) -> Result<ResolvedAsset, String> {
    let context = current_context();
    let _loader = context
        .svg_loader
        .lock()
        .map_err(|_| "asset loader poisoned")?;
    if current_epoch() != epoch {
        return Err("asset configuration changed".into());
    }
    if let ImageSource::Id(id) = source {
        let (width, height) =
            asset_dimensions(id).ok_or_else(|| format!("unknown image id: {id}"))?;
        return Ok(ResolvedAsset {
            id: id.clone(),
            width,
            height,
        });
    }
    let path = resolved_source_path(source, config)?;
    let bytes =
        fs::read(&path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let id = canonical_asset_id(&bytes);
    if let Some(record) = asset_record(&id) {
        // A parsed-cache hit is also an active source reference once published.
        let mut state = context.state.lock().map_err(|_| "asset state poisoned")?;
        if state.epoch != epoch {
            return Err("asset configuration changed".into());
        }
        state.records.insert(id.clone(), Arc::clone(&record));
        return Ok(ResolvedAsset {
            id,
            width: record.width,
            height: record.height,
        });
    }
    let retained = crate::renderer::retained_asset_metadata(&id);
    if !require_record && let Some(metadata) = retained.as_ref() {
        return Ok(ResolvedAsset {
            id,
            width: metadata.width,
            height: metadata.height,
        });
    }
    let record = if path_is_svg(&path) {
        let fonts = svg_fonts(&context, epoch)?;
        let options = usvg::Options {
            fontdb: fonts,
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_data_nested(&bytes, &options)
            .map_err(|err| format!("failed to parse SVG {}: {err}", path.display()))?;
        if let Ok(mut state) = context.state.lock()
            && state.epoch == epoch
        {
            state.svg_cache.stats.parses += 1;
        }
        let generation = retained
            .map(|metadata| metadata.generation)
            .unwrap_or_else(|| generation_for_id(&id));
        register_vector_asset_in_epoch(
            &id,
            &path.display().to_string(),
            tree,
            bytes.len() as u64,
            epoch,
            generation,
        )?
    } else {
        let record = register_raster_asset_in_epoch(
            &id,
            &path.display().to_string(),
            &bytes,
            config.decode_at_size,
            epoch,
        )?;
        if !config.decode_at_size {
            crate::renderer::preload_raster_asset_original(&id)?;
        }
        record
    };
    Ok(ResolvedAsset {
        id,
        width: record.width,
        height: record.height,
    })
}

fn svg_fonts(context: &AssetContext, epoch: u64) -> Result<Arc<usvg::fontdb::Database>, String> {
    if let Some(environment) = context
        .svg_fonts
        .lock()
        .map_err(|_| "SVG font cache poisoned")?
        .as_ref()
        && environment.epoch == epoch
    {
        return Ok(Arc::clone(&environment.fonts));
    }
    // Caller holds the load serializer, never a cache/state lock during discovery.
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    let fonts = Arc::new(fonts);
    let font_faces = fonts.faces().count() as u64;
    let font_estimated_bytes = estimated_font_bytes(&fonts);
    let mut state = context.state.lock().map_err(|_| "asset state poisoned")?;
    if state.epoch != epoch {
        return Err("asset configuration changed".into());
    }
    state.svg_cache.stats.font_discoveries += 1;
    state.svg_cache.stats.font_faces = font_faces;
    state.svg_cache.stats.font_estimated_bytes = font_estimated_bytes;
    *context
        .svg_fonts
        .lock()
        .map_err(|_| "SVG font cache poisoned")? = Some(SvgFontEnvironment {
        epoch,
        fonts: Arc::clone(&fonts),
    });
    Ok(fonts)
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

#[cfg_attr(not(all(feature = "drm-core", target_os = "linux")), allow(dead_code))]
pub(crate) fn resolve_configured_path(path: &str, config: &AssetConfig) -> Result<PathBuf, String> {
    if Path::new(path).is_absolute() {
        resolve_runtime_path(path, config)
    } else {
        resolve_logical_path(path, config)
    }
}

#[cfg(test)]
mod tests;

/// One preparation's media facts. It owns no runtime and performs no hydration.
#[derive(Clone, Debug)]
pub(crate) struct FrameAssets {
    owner: std::sync::Weak<Mutex<AssetState>>,
    pub(crate) images: crate::renderer::ImageSnapshot,
    sources: HashMap<ImageSource, AssetStatus>,
    loading_indicators: HashSet<ImageSource>,
    status_generation: u64,
    measured: HashMap<crate::tree::element::NodeId, Option<(u32, u32)>>,
}
impl FrameAssets {
    pub(crate) fn capture(sources: &FrameSourceList) -> Self {
        let context = current_context();
        if sources.all.is_empty() {
            return Self {
                owner: Arc::downgrade(&context.state),
                images: Default::default(),
                sources: HashMap::new(),
                loading_indicators: HashSet::new(),
                measured: HashMap::new(),
                status_generation: 0,
            };
        }
        let state = context.state.lock().ok();
        let statuses: HashMap<_, _> = sources
            .all
            .iter()
            .map(|source| {
                let status = state
                    .as_ref()
                    .and_then(|state| state.sources.get(source))
                    .cloned()
                    .unwrap_or(AssetStatus::Pending);
                (source.clone(), status)
            })
            .collect();
        let records = statuses
            .iter()
            .filter_map(|(source, status)| {
                let id = match source {
                    ImageSource::Id(id) => Some(id),
                    _ => match status {
                        AssetStatus::Ready(asset) => Some(&asset.id),
                        _ => None,
                    },
                }?;
                let record = state.as_ref().and_then(|state| {
                    state
                        .records
                        .get(id)
                        .cloned()
                        .or_else(|| state.svg_cache.peek(id))
                });
                Some((id.clone(), record))
            })
            .collect();
        let images = crate::renderer::ImageSnapshot::capture_records(
            records,
            state.as_ref().map(|state| state.epoch),
            0,
        );
        let statuses: HashMap<_, _> = statuses
            .into_iter()
            .map(|(source, status)| {
                let id = match &source {
                    ImageSource::Id(id) => Some(id),
                    _ => match &status {
                        AssetStatus::Ready(asset) => Some(&asset.id),
                        _ => None,
                    },
                };
                let status = id
                    .and_then(|id| {
                        images.dimensions(id).map(|(width, height)| {
                            AssetStatus::Ready(ResolvedAsset {
                                id: id.clone(),
                                width,
                                height,
                            })
                        })
                    })
                    .unwrap_or(status);
                (source, status)
            })
            .collect();
        Self {
            owner: Arc::downgrade(&context.state),
            status_generation: state.as_ref().map_or(0, |state| state.status_generation),
            images,
            measured: sources
                .measured
                .iter()
                .map(|(id, source)| {
                    (
                        *id,
                        match statuses.get(source) {
                            Some(AssetStatus::Ready(asset)) => Some((asset.width, asset.height)),
                            _ => None,
                        },
                    )
                })
                .collect(),
            loading_indicators: statuses
                .iter()
                .filter(|(source, status)| {
                    matches!(status, AssetStatus::Pending)
                        && state.as_ref().is_some_and(|state| {
                            state.loading_indicators.get(*source) == Some(&None)
                        })
                })
                .map(|(source, _)| source.clone())
                .collect(),
            sources: statuses,
        }
    }
    pub(crate) fn matches_current(&self) -> bool {
        self.owner.ptr_eq(&Arc::downgrade(&current_context().state))
    }
    pub(crate) fn measure_invalidations(
        &self,
        other: Option<&Self>,
    ) -> Vec<crate::tree::element::NodeId> {
        self.measured
            .iter()
            .filter(|(id, size)| other.is_some_and(|old| old.measured.get(id) != Some(*size)))
            .map(|(id, _)| *id)
            .collect()
    }
    pub(crate) fn enter(self: &Arc<Self>) -> FrameAssetsGuard {
        FrameAssetsGuard {
            previous: FRAME_ASSETS.with(|slot| slot.replace(Some(Arc::clone(self)))),
            _images: self.images.enter(),
        }
    }
}
thread_local! {static FRAME_ASSETS:RefCell<Option<Arc<FrameAssets>>>=const {RefCell::new(None)};}
pub(crate) struct FrameAssetsGuard {
    previous: Option<Arc<FrameAssets>>,
    _images: crate::renderer::scene_images::ImageGuard,
}
impl Drop for FrameAssetsGuard {
    fn drop(&mut self) {
        FRAME_ASSETS.with(|slot| slot.replace(self.previous.take()));
    }
}
fn frame_source_status(source: &ImageSource) -> Option<Option<AssetStatus>> {
    FRAME_ASSETS.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|frame| frame.matches_current())
            .map(|frame| frame.sources.get(source).cloned())
    })
}
