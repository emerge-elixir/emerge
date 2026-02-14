use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::SystemTime;

use crossbeam_channel::{Sender, TrySendError, bounded};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::actors::TreeMsg;
use crate::renderer::{image_dimensions, insert_image};
use crate::tree::attrs::{Background, ImageSource};
use crate::tree::element::ElementTree;

#[derive(Clone, Debug)]
pub struct AssetConfig {
    pub manifest_path: String,
    pub runtime_enabled: bool,
    pub runtime_allowlist: Vec<String>,
    pub runtime_follow_symlinks: bool,
    pub runtime_max_file_size: u64,
    pub runtime_extensions: Vec<String>,
}

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            manifest_path: "priv/static/cache_manifest.json".to_string(),
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
            ],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedAsset {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub enum AssetStatus {
    Pending,
    Ready(ResolvedAsset),
    Failed,
}

struct AssetState {
    config: AssetConfig,
    config_revision: u64,
    sources: HashMap<ImageSource, AssetStatus>,
    pending_count: usize,
}

impl Default for AssetState {
    fn default() -> Self {
        Self {
            config: AssetConfig::default(),
            config_revision: 0,
            sources: HashMap::new(),
            pending_count: 0,
        }
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
    Stop,
}

struct ManifestCache {
    path: String,
    mtime: SystemTime,
    latest: HashMap<String, String>,
    root: PathBuf,
}

struct Worker {
    tree_tx: Sender<TreeMsg>,
    state: Arc<Mutex<AssetState>>,
    log_render: bool,
    seen_revision: u64,
    manifest_cache: Option<ManifestCache>,
}

static GLOBAL: OnceLock<Mutex<Global>> = OnceLock::new();

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
        state_guard.sources.clear();
        state_guard.pending_count = 0;
    }

    guard.tx = Some(tx.clone());
    drop(guard);

    thread::spawn(move || {
        let mut worker = Worker {
            tree_tx,
            state,
            log_render,
            seen_revision: 0,
            manifest_cache: None,
        };

        while let Ok(msg) = rx.recv() {
            match msg {
                AssetMsg::Stop => break,
                AssetMsg::Ensure(source) => worker.handle_ensure(source),
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
        state.sources.clear();
        state.pending_count = 0;
    }
}

pub fn configure(config: AssetConfig) {
    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    if let Ok(mut state) = guard.state.lock() {
        state.config = normalize_config(config);
        state.config_revision = state.config_revision.saturating_add(1);
        state.sources.clear();
        state.pending_count = 0;
    }
}

pub fn ensure_tree_sources(tree: &ElementTree) {
    for element in tree.nodes.values() {
        if let Some(source) = element.attrs.image_src.as_ref() {
            ensure_source(source);
        }

        if let Some(Background::Image { source, .. }) = element.attrs.background.as_ref() {
            ensure_source(source);
        }

        if let Some(mouse_over) = element.attrs.mouse_over.as_ref()
            && let Some(Background::Image { source, .. }) = mouse_over.background.as_ref()
        {
            ensure_source(source);
        }
    }
}

pub fn ensure_source(source: &ImageSource) {
    if let ImageSource::Id(id) = source
        && let Some((width, height)) = image_dimensions(id)
    {
        if let Ok(guard) = global().lock()
            && let Ok(mut state) = guard.state.lock()
        {
            state.sources.insert(
                source.clone(),
                AssetStatus::Ready(ResolvedAsset {
                    id: id.clone(),
                    width,
                    height,
                }),
            );
        }
        return;
    }

    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };

    let mut should_queue = false;

    if let Ok(mut state) = guard.state.lock() {
        match state.sources.get(source) {
            Some(AssetStatus::Pending)
            | Some(AssetStatus::Ready(_))
            | Some(AssetStatus::Failed) => {}
            None => {
                state.sources.insert(source.clone(), AssetStatus::Pending);
                state.pending_count = state.pending_count.saturating_add(1);
                should_queue = true;
            }
        }
    }

    if !should_queue {
        return;
    }

    if let Some(tx) = guard.tx.as_ref() {
        match tx.try_send(AssetMsg::Ensure(source.clone())) {
            Ok(()) => {}
            Err(TrySendError::Full(msg)) => {
                let _ = tx.send(msg);
            }
            Err(TrySendError::Disconnected(_)) => {
                if let Ok(mut state) = guard.state.lock() {
                    if matches!(state.sources.get(source), Some(AssetStatus::Pending)) {
                        if state.pending_count > 0 {
                            state.pending_count -= 1;
                        }
                        state.sources.insert(source.clone(), AssetStatus::Failed);
                    }
                }
            }
        }
    }
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

pub fn has_pending_assets() -> bool {
    let guard = match global().lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };

    match guard.state.lock() {
        Ok(state) => state.pending_count > 0,
        Err(_) => false,
    }
}

impl Worker {
    fn handle_ensure(&mut self, source: ImageSource) {
        let (config, revision) = match self.state.lock() {
            Ok(state) => (state.config.clone(), state.config_revision),
            Err(_) => return,
        };

        if revision != self.seen_revision {
            self.seen_revision = revision;
            self.manifest_cache = None;
        }

        let result = self.load_source(&source, &config);

        if let Ok(mut state) = self.state.lock() {
            let was_pending = matches!(state.sources.get(&source), Some(AssetStatus::Pending));
            if was_pending && state.pending_count > 0 {
                state.pending_count -= 1;
            }

            match result {
                Ok(asset) => {
                    state
                        .sources
                        .insert(source.clone(), AssetStatus::Ready(asset));
                }
                Err(reason) => {
                    if self.log_render {
                        eprintln!("asset load failed source={source:?} reason={reason}");
                    }
                    state.sources.insert(source.clone(), AssetStatus::Failed);
                }
            }
        }

        send_tree_update(&self.tree_tx, self.log_render);
    }

    fn load_source(
        &mut self,
        source: &ImageSource,
        config: &AssetConfig,
    ) -> Result<ResolvedAsset, String> {
        match source {
            ImageSource::Id(id) => {
                let (width, height) =
                    image_dimensions(id).ok_or_else(|| format!("unknown image id: {id}"))?;
                Ok(ResolvedAsset {
                    id: id.clone(),
                    width,
                    height,
                })
            }
            ImageSource::Logical(logical) => {
                let path = self.resolve_logical_path(logical, config)?;
                self.load_path(&path)
            }
            ImageSource::RuntimePath(path) => {
                let resolved = resolve_runtime_path(path, config)?;
                self.load_path(&resolved)
            }
        }
    }

    fn load_path(&self, path: &Path) -> Result<ResolvedAsset, String> {
        let bytes =
            fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;

        let id = canonical_image_id(&bytes);

        let (width, height) = match image_dimensions(&id) {
            Some((w, h)) => (w, h),
            None => insert_image(&id, &bytes)?,
        };

        Ok(ResolvedAsset { id, width, height })
    }

    fn resolve_logical_path(
        &mut self,
        logical: &str,
        config: &AssetConfig,
    ) -> Result<PathBuf, String> {
        let manifest_path = PathBuf::from(&config.manifest_path);

        let metadata = fs::metadata(&manifest_path)
            .map_err(|err| format!("failed to stat manifest {}: {err}", manifest_path.display()))?;

        let mtime = metadata.modified().map_err(|err| {
            format!(
                "failed to read manifest mtime {}: {err}",
                manifest_path.display()
            )
        })?;

        let reload = self
            .manifest_cache
            .as_ref()
            .is_none_or(|cache| cache.path != config.manifest_path || cache.mtime != mtime);

        if reload {
            let bytes = fs::read(&manifest_path).map_err(|err| {
                format!("failed to read manifest {}: {err}", manifest_path.display())
            })?;

            let parsed: Value = serde_json::from_slice(&bytes).map_err(|err| {
                format!(
                    "failed to parse manifest {}: {err}",
                    manifest_path.display()
                )
            })?;

            let latest_obj = parsed
                .get("latest")
                .and_then(Value::as_object)
                .ok_or_else(|| "manifest missing latest map".to_string())?;

            let mut latest = HashMap::new();
            for (key, value) in latest_obj {
                if let Some(path) = value.as_str() {
                    latest.insert(key.clone(), path.to_string());
                }
            }

            self.manifest_cache = Some(ManifestCache {
                path: config.manifest_path.clone(),
                mtime,
                latest,
                root: manifest_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(".")),
            });
        }

        let cache = self
            .manifest_cache
            .as_ref()
            .ok_or_else(|| "manifest cache unavailable".to_string())?;

        for candidate in logical_candidates(logical) {
            if let Some(digested) = cache.latest.get(&candidate) {
                return Ok(cache.root.join(digested));
            }
        }

        Err(format!("logical asset not found in manifest: {logical}"))
    }
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

fn logical_candidates(path: &str) -> Vec<String> {
    let trimmed = path.trim_start_matches('/').to_string();
    let mut out = vec![trimmed.clone()];
    let prefixed = format!("/{trimmed}");
    if prefixed != trimmed {
        out.push(prefixed);
    }
    out
}

fn canonical_image_id(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    format!("img_{digest:x}")
}

fn resolve_runtime_path(path: &str, config: &AssetConfig) -> Result<PathBuf, String> {
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
