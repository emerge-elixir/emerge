#![cfg_attr(
    test,
    allow(
        clippy::cloned_ref_to_slice_refs,
        clippy::field_reassign_with_default,
        clippy::needless_borrow,
        clippy::needless_lifetimes,
        clippy::nonminimal_bool,
        clippy::op_ref,
        clippy::redundant_pattern_matching,
        clippy::too_many_arguments
    )
)]

//! EmergeSkia NIF - Minimal Skia renderer for Elixir.
//!
//! This crate provides a Rustler NIF that exposes tree upload, layout,
//! rendering, and headless rasterization for Emerge.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use crossbeam_channel::unbounded;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError, bounded};

use rustler::{Atom, Binary, Encoder, Env, LocalPid, NewBinary, NifResult, ResourceArc, Term};
pub mod actors;
pub mod assets;
pub mod backend;
mod clipboard;
#[cfg(all(feature = "drm", target_os = "linux"))]
mod cursor;
mod debug_trace;
#[cfg(all(feature = "drm", target_os = "linux"))]
mod drm_input;
pub mod events;
pub mod input;
pub mod keys;
#[cfg(all(feature = "drm", target_os = "linux"))]
mod linux_wait;
mod native_log;
pub mod render_scene;
pub mod renderer;
mod runtime;
pub mod services;
pub mod stats;
pub mod tree;
mod video;

use actors::{EventMsg, RenderMsg, TreeMsg};
use assets::AssetConfig;
#[cfg(all(feature = "drm", target_os = "linux"))]
use backend::drm;
use backend::wake::BackendWakeHandle;
#[cfg(all(feature = "wayland", target_os = "linux"))]
use backend::wayland;
#[cfg(all(feature = "wayland", target_os = "linux"))]
use backend::wayland_config::WaylandConfig;
#[cfg(all(feature = "drm", target_os = "linux"))]
use cursor::{CursorState, SharedCursorState};
#[cfg(all(feature = "drm", target_os = "linux"))]
use drm_input::DrmInput;
use events::{CursorIcon, SpawnEventActorConfig, spawn_event_actor};
#[cfg(all(feature = "drm", target_os = "linux"))]
use linux_wait::EventFd;
use native_log::NativeLogRelay;
use renderer::clear_global_caches;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use renderer::set_render_log_enabled;
use runtime::tree_actor::{TreeActorConfig, spawn_tree_actor_with_initial_tree};
use stats::{LayoutCacheStats, RendererStatsCollector, RendererStatsSnapshot};
use std::time::Instant;
use tree::element::{ElementTree, NodeId};
use video::{VideoMode, VideoRegistry, VideoTargetResource, VideoWake};

type LayoutFrame<'a> = (Binary<'a>, f32, f32, f32, f32);
type LayoutFrames<'a> = Vec<LayoutFrame<'a>>;

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct StatsConfigureNif {
    enabled: bool,
}

#[derive(Clone, Copy, Debug, rustler::NifTaggedEnum)]
enum StatsCommandNif {
    Peek,
    Take,
    Reset,
    Configure(StatsConfigureNif),
}

#[derive(Clone, Debug, rustler::NifMap)]
struct StatsSnapshotNif {
    version: u64,
    kind: String,
    enabled: bool,
    window: StatsWindowNif,
    frames: StatsFrameSnapshotNif,
    timings: StatsTimingSnapshotNif,
    counters: StatsCounterSnapshotNif,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct StatsWindowNif {
    elapsed_ms: u64,
    reset_on_read: bool,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct StatsFrameSnapshotNif {
    fps: f64,
    display_fps: f64,
    display_frame_ms: f64,
    frame_count: u64,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct StatsTimingSnapshotNif {
    render: DurationStatsNif,
    present_submit: DurationStatsNif,
    layout: DurationStatsNif,
    refresh: DurationStatsNif,
    event_resolve: DurationStatsNif,
    patch_tree_process: DurationStatsNif,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct DurationStatsNif {
    count: u64,
    avg_ms: f64,
    min_ms: f64,
    max_ms: f64,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct StatsCounterSnapshotNif {
    layout_cache: LayoutCacheStatsNif,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct LayoutCacheStatsNif {
    intrinsic_measure_hits: u64,
    intrinsic_measure_misses: u64,
    intrinsic_measure_stores: u64,
    subtree_measure_hits: u64,
    subtree_measure_misses: u64,
    subtree_measure_stores: u64,
    resolve_hits: u64,
    resolve_misses: u64,
    resolve_stores: u64,
}

impl StatsSnapshotNif {
    fn disabled(kind: &'static str) -> Self {
        Self::from_snapshot(kind, false, false, &RendererStatsSnapshot::default())
    }

    fn from_snapshot(
        kind: &'static str,
        enabled: bool,
        reset_on_read: bool,
        snapshot: &RendererStatsSnapshot,
    ) -> Self {
        Self {
            version: 2,
            kind: kind.to_string(),
            enabled,
            window: StatsWindowNif {
                elapsed_ms: snapshot.window.as_millis() as u64,
                reset_on_read,
            },
            frames: StatsFrameSnapshotNif {
                fps: snapshot.fps,
                display_fps: snapshot.display_fps,
                display_frame_ms: snapshot.display_frame_ms,
                frame_count: snapshot.frame_count,
            },
            timings: StatsTimingSnapshotNif {
                render: DurationStatsNif::from(snapshot.render.clone()),
                present_submit: DurationStatsNif::from(snapshot.present_submit.clone()),
                layout: DurationStatsNif::from(snapshot.layout.clone()),
                refresh: DurationStatsNif::from(snapshot.refresh.clone()),
                event_resolve: DurationStatsNif::from(snapshot.event_resolve.clone()),
                patch_tree_process: DurationStatsNif::from(snapshot.patch_tree_process.clone()),
            },
            counters: StatsCounterSnapshotNif {
                layout_cache: LayoutCacheStatsNif::from(snapshot.layout_cache),
            },
        }
    }
}

impl From<stats::DurationStatsSnapshot> for DurationStatsNif {
    fn from(stats: stats::DurationStatsSnapshot) -> Self {
        Self {
            count: stats.count,
            avg_ms: stats.avg_ms,
            min_ms: stats.min_ms,
            max_ms: stats.max_ms,
        }
    }
}

impl From<LayoutCacheStats> for LayoutCacheStatsNif {
    fn from(stats: LayoutCacheStats) -> Self {
        Self {
            intrinsic_measure_hits: stats.intrinsic_measure_hits,
            intrinsic_measure_misses: stats.intrinsic_measure_misses,
            intrinsic_measure_stores: stats.intrinsic_measure_stores,
            subtree_measure_hits: stats.subtree_measure_hits,
            subtree_measure_misses: stats.subtree_measure_misses,
            subtree_measure_stores: stats.subtree_measure_stores,
            resolve_hits: stats.resolve_hits,
            resolve_misses: stats.resolve_misses,
            resolve_stores: stats.resolve_stores,
        }
    }
}

// ============================================================================
// Atoms
// ============================================================================

mod atoms {
    rustler::atoms! {
        ok,
        error,
    }
}

// ============================================================================
// NIF Resource
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendKind {
    #[cfg(feature = "macos")]
    Macos,
    #[cfg(all(feature = "wayland", target_os = "linux"))]
    Wayland,
    #[cfg(all(feature = "drm", target_os = "linux"))]
    Drm,
}

struct RendererResource {
    running_flag: Arc<AtomicBool>,
    backend_wake: BackendWakeHandle,
    stop_flag: Arc<AtomicBool>,
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    input_target: Arc<InputTargetRelay>,
    render_tx: RenderSender,
    video_registry: Arc<VideoRegistry>,
    video_wake: VideoWake,
    prime_video_supported: bool,
    native_log: Arc<NativeLogRelay>,
    stats: Option<Arc<RendererStatsCollector>>,
    close_signal_log: bool,
    log_render: bool,
    log_input: bool,
    handles: Mutex<Option<RendererHandles>>,
}

#[derive(Default)]
pub(crate) struct InputTargetRelay {
    target: Mutex<Option<LocalPid>>,
}

impl InputTargetRelay {
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    fn new(target: Option<LocalPid>) -> Self {
        Self {
            target: Mutex::new(target),
        }
    }

    fn set_target(&self, target: Option<LocalPid>) {
        let mut guard = self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = target;
    }

    fn send_running(&self) {
        let target = *self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(pid) = target {
            events::send_running_message(pid);
        }
    }

    #[cfg(all(feature = "wayland", target_os = "linux"))]
    fn send_close_requested(&self, close_signal_log: bool) {
        let target = *self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        log_close_signal(
            close_signal_log,
            "wayland_close",
            format!("relay target_present={}", target.is_some()),
        );

        if let Some(pid) = target {
            events::send_close_message(pid);
            log_close_signal(
                close_signal_log,
                "wayland_close",
                "relay send_close_message done",
            );
        }
    }
}

#[derive(Clone)]
pub(crate) struct RenderSender {
    tx: Sender<RenderMsg>,
    drop_rx: Receiver<RenderMsg>,
    log_render: bool,
}

impl RenderSender {
    fn send_latest(&self, msg: RenderMsg) {
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(msg)) => {
                let _ = self.drop_rx.try_recv();
                let _ = self.tx.try_send(msg);
                if self.log_render {
                    eprintln!("render queue overwrite");
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

/// Resource for holding an element tree (for layout/rendering).
struct TreeResource {
    tree: Mutex<ElementTree>,
    stats: Mutex<Option<Arc<RendererStatsCollector>>>,
}

struct TestHarnessHandles {
    proxy_handle: thread::JoinHandle<()>,
    tree_handle: thread::JoinHandle<()>,
    event_handle: thread::JoinHandle<()>,
}

#[derive(Default)]
struct RendererHandles {
    backend_handle: Option<thread::JoinHandle<()>>,
    input_handle: Option<thread::JoinHandle<()>>,
    tree_handle: Option<thread::JoinHandle<()>>,
    event_handle: Option<thread::JoinHandle<()>>,
    heartbeat_handle: Option<thread::JoinHandle<()>>,
}

struct ShutdownRuntimeContext {
    running_flag: Arc<AtomicBool>,
    backend_wake: BackendWakeHandle,
    stop_flag: Arc<AtomicBool>,
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    render_tx: RenderSender,
    close_signal_log: bool,
    log_render: bool,
    log_input: bool,
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
const RUNNING_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);

fn log_close_signal(enabled: bool, source: &'static str, message: impl Into<String>) {
    if enabled {
        let message = message.into();
        eprintln!("EmergeSkia native[{source}] {message}");
    }
}

struct TestHarnessResource {
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    render_rx: Receiver<RenderMsg>,
    tree_tap_rx: Receiver<TreeMsg>,
    base_instant: Mutex<Instant>,
    handles: Mutex<Option<TestHarnessHandles>>,
}

#[rustler::resource_impl]
impl rustler::Resource for RendererResource {}

#[rustler::resource_impl]
impl rustler::Resource for TreeResource {}

#[rustler::resource_impl]
impl rustler::Resource for TestHarnessResource {}

impl Drop for RendererResource {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl Drop for TestHarnessResource {
    fn drop(&mut self) {
        self.stop_inner();
    }
}

impl TestHarnessResource {
    fn stop_inner(&self) {
        let mut handles_guard = match self.handles.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };

        let Some(handles) = handles_guard.take() else {
            return;
        };

        drop(handles_guard);

        send_event(&self.event_tx, EventMsg::Stop, false);
        send_tree(&self.tree_tx, TreeMsg::Stop, false);

        let _ = handles.proxy_handle.join();
        let _ = handles.event_handle.join();
        let _ = handles.tree_handle.join();
        assets::stop();
        clear_global_caches();
        trim_process_allocator();
    }
}

impl RendererResource {
    fn stop(&self) {
        self.stop_inner();
    }

    fn stop_inner(&self) {
        let mut handles_guard = match self.handles.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let Some(handles) = handles_guard.take() else {
            return;
        };

        drop(handles_guard);

        let ctx = ShutdownRuntimeContext {
            running_flag: Arc::clone(&self.running_flag),
            backend_wake: self.backend_wake.clone(),
            stop_flag: Arc::clone(&self.stop_flag),
            tree_tx: self.tree_tx.clone(),
            event_tx: self.event_tx.clone(),
            render_tx: self.render_tx.clone(),
            close_signal_log: self.close_signal_log,
            log_render: self.log_render,
            log_input: self.log_input,
        };

        if self.running_flag.load(Ordering::Relaxed) {
            shutdown_renderer_runtime(ctx, handles);
        } else {
            thread::spawn(move || shutdown_renderer_runtime(ctx, handles));
        }
    }
}

fn shutdown_renderer_runtime(ctx: ShutdownRuntimeContext, mut handles: RendererHandles) {
    let ShutdownRuntimeContext {
        running_flag,
        backend_wake,
        stop_flag,
        tree_tx,
        event_tx,
        render_tx,
        close_signal_log,
        log_render,
        log_input,
    } = ctx;

    assets::stop();
    log_close_signal(close_signal_log, "nif_close", "shutdown begin");
    running_flag.store(false, Ordering::Relaxed);
    stop_flag.store(true, Ordering::Relaxed);
    send_tree(&tree_tx, TreeMsg::Stop, log_render);
    send_event(&event_tx, EventMsg::Stop, log_input);
    render_tx.send_latest(RenderMsg::Stop);

    backend_wake.request_stop();

    if let Some(handle) = handles.event_handle.take() {
        let _ = handle.join();
    }

    if let Some(handle) = handles.heartbeat_handle.take() {
        let _ = handle.join();
    }

    if let Some(handle) = handles.tree_handle.take() {
        let _ = handle.join();
    }

    if let Some(handle) = handles.input_handle.take() {
        let _ = handle.join();
    }

    if let Some(handle) = handles.backend_handle.take() {
        let _ = handle.join();
    }

    log_close_signal(close_signal_log, "nif_close", "shutdown end");
    clear_global_caches();
    trim_process_allocator();
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
fn backend_stats_label(backend: BackendKind) -> &'static str {
    match backend {
        #[cfg(feature = "macos")]
        BackendKind::Macos => "macos",
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        BackendKind::Wayland => "wayland",
        #[cfg(all(feature = "drm", target_os = "linux"))]
        BackendKind::Drm => "drm",
    }
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
fn spawn_running_heartbeat(
    running_flag: Arc<AtomicBool>,
    input_target: Arc<InputTargetRelay>,
    native_log: Arc<NativeLogRelay>,
    stats: Option<Arc<RendererStatsCollector>>,
    backend_label: &'static str,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut ticks = 0_u64;

        while running_flag.load(Ordering::Relaxed) {
            input_target.send_running();

            if let Some(stats) = stats.as_ref() {
                ticks = ticks.wrapping_add(1);

                if ticks.is_multiple_of(10) {
                    native_log.info(
                        "renderer_stats",
                        stats::format_renderer_stats_log(backend_label, &stats.snapshot()),
                    );
                }
            }

            thread::sleep(RUNNING_HEARTBEAT_INTERVAL);
        }
    })
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[cfg(not(test))]
fn trim_process_allocator() {
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(any(test, not(all(target_os = "linux", target_env = "gnu"))))]
fn trim_process_allocator() {}

fn send_tree(tree_tx: &Sender<TreeMsg>, msg: TreeMsg, log_render: bool) {
    match tree_tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking send");
            }
            crate::debug_trace::hover_trace!("tree_channel", "tree channel full, blocking send");
            let _ = tree_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn send_event(event_tx: &Sender<EventMsg>, msg: EventMsg, log_input: bool) {
    match event_tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_input {
                eprintln!("event channel full, blocking send");
            }
            crate::debug_trace::hover_trace!("event_channel", "event channel full, blocking send");
            let _ = event_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

// ============================================================================
// NIF Functions
// ============================================================================

#[derive(Clone, Debug)]
struct StartConfig {
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux"),
            feature = "macos"
        )),
        allow(dead_code)
    )]
    backend: BackendKind,
    #[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
    title: String,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    width: u32,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    height: u32,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    scroll_line_pixels: f32,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    asset_config: AssetConfig,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    drm_card: Option<String>,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    drm_startup_retries: u32,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    drm_retry_interval_ms: u32,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    drm_hw_cursor: bool,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    drm_cursor_overrides: Vec<DrmCursorOverrideConfig>,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    drm_input_log: bool,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    render_log: bool,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    close_signal_log: bool,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux"),
        )),
        allow(dead_code)
    )]
    stats_enabled: bool,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux"),
        )),
        allow(dead_code)
    )]
    renderer_stats_log: bool,
}

#[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct DrmCursorOverrideConfig {
    pub icon: CursorIcon,
    pub source: String,
    pub hotspot: (f32, f32),
}

#[derive(rustler::NifMap)]
struct StartOptsNif {
    backend: String,
    title: String,
    width: u32,
    height: u32,
    scroll_line_pixels: f32,
    drm_card: Option<String>,
    asset_sources: Vec<String>,
    asset_runtime_enabled: bool,
    asset_allowlist: Vec<String>,
    asset_follow_symlinks: bool,
    asset_max_file_size: u64,
    asset_extensions: Vec<String>,
    drm_cursor: Vec<DrmCursorOverrideNif>,
    drm_startup_retries: u32,
    drm_retry_interval_ms: u32,
    hw_cursor: bool,
    input_log: bool,
    render_log: bool,
    close_signal_log: bool,
    stats_enabled: bool,
    renderer_stats_log: bool,
}

#[derive(rustler::NifMap)]
struct DrmCursorOverrideNif {
    icon: String,
    source: String,
    hotspot_x: f32,
    hotspot_y: f32,
}

#[derive(rustler::NifMap)]
struct RenderTreeOffscreenOptsNif {
    width: u32,
    height: u32,
    scale: f32,
    sources: Vec<String>,
    runtime_enabled: bool,
    allowlist: Vec<String>,
    follow_symlinks: bool,
    max_file_size: u64,
    extensions: Vec<String>,
    asset_mode: String,
    asset_timeout_ms: u64,
}

fn start_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    #[cfg(feature = "macos")]
    if matches!(config.backend, BackendKind::Macos) {
        return Err(rustler::Error::Term(Box::new(
            "macOS uses the external host path; start it through EmergeSkia.start/1 with backend: :macos"
                .to_string(),
        )));
    }

    start_native_renderer_with_config(config, initial_log_target)
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn start_native_renderer_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    let running_flag = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let render_counter = Arc::new(AtomicU64::new(0));
    let input_target = Arc::new(InputTargetRelay::new(None));
    let native_log = Arc::new(NativeLogRelay::new(initial_log_target));

    #[cfg(all(feature = "drm", target_os = "linux"))]
    let log_input = matches!(config.backend, BackendKind::Drm) && config.drm_input_log;
    #[cfg(not(all(feature = "drm", target_os = "linux")))]
    let log_input = false;
    let log_render = config.render_log;
    let close_signal_log = config.close_signal_log;
    let renderer_stats = (config.stats_enabled || config.renderer_stats_log)
        .then(|| Arc::new(RendererStatsCollector::new()));
    let backend_label = backend_stats_label(config.backend);
    set_render_log_enabled(log_render);

    let (tree_tx, tree_rx) = bounded(512);
    let (event_tx, event_rx) = bounded(4096);
    let (render_tx, render_rx) = bounded(1);
    let render_sender = RenderSender {
        tx: render_tx,
        drop_rx: render_rx.clone(),
        log_render,
    };
    let (backend_cursor_tx, backend_cursor_rx) = unbounded();
    #[cfg(all(feature = "drm", target_os = "linux"))]
    let drm_cursor_state = Arc::new(SharedCursorState::new(CursorState {
        pos: (0.0, 0.0),
        visible: false,
    }));

    assets::start(tree_tx.clone(), log_render);

    #[cfg(all(feature = "wayland", target_os = "linux"))]
    let system_clipboard = matches!(config.backend, BackendKind::Wayland);
    #[cfg(not(all(feature = "wayland", target_os = "linux")))]
    let system_clipboard = false;
    let heartbeat_stats = if config.renderer_stats_log {
        renderer_stats.clone()
    } else {
        None
    };
    let mut handles = RendererHandles {
        heartbeat_handle: Some(spawn_running_heartbeat(
            Arc::clone(&running_flag),
            Arc::clone(&input_target),
            Arc::clone(&native_log),
            heartbeat_stats,
            backend_label,
        )),
        ..RendererHandles::default()
    };

    let initial_width = config.width;
    let initial_height = config.height;
    let release_tx = video::spawn_release_worker();
    let video_registry = Arc::new(VideoRegistry::new(release_tx));
    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    #[allow(unused_assignments)]
    let mut backend_wake = BackendWakeHandle::noop();
    #[cfg(not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )))]
    let backend_wake = BackendWakeHandle::noop();

    let (backend, prime_video_supported): (BackendKind, bool) = match config.backend {
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        BackendKind::Wayland => {
            let (proxy_tx, proxy_rx) = std::sync::mpsc::channel();
            let running_flag_clone = Arc::clone(&running_flag);
            let tree_tx_clone = tree_tx.clone();
            let event_tx_clone = event_tx.clone();
            let input_target_clone = Arc::clone(&input_target);
            let renderer_stats_clone = renderer_stats.clone();
            let video_registry_clone = Arc::clone(&video_registry);
            let wayland_config = WaylandConfig {
                title: config.title,
                width: config.width,
                height: config.height,
            };

            handles.backend_handle = Some(thread::spawn(move || {
                wayland::run(wayland::WaylandRunArgs {
                    config: wayland_config,
                    running_flag: running_flag_clone,
                    tree_tx: tree_tx_clone,
                    event_tx: event_tx_clone,
                    input_target: input_target_clone,
                    close_signal_log,
                    stats: renderer_stats_clone,
                    render_rx,
                    cursor_icon_rx: backend_cursor_rx,
                    video_registry: video_registry_clone,
                    proxy_tx,
                });
            }));

            let startup = match proxy_rx.recv() {
                Ok(Ok(startup)) => startup,
                Ok(Err(reason)) => {
                    shutdown_renderer_runtime(
                        ShutdownRuntimeContext {
                            running_flag: Arc::clone(&running_flag),
                            backend_wake: backend_wake.clone(),
                            stop_flag: Arc::clone(&stop_flag),
                            tree_tx: tree_tx.clone(),
                            event_tx: event_tx.clone(),
                            render_tx: render_sender.clone(),
                            close_signal_log,
                            log_render,
                            log_input,
                        },
                        std::mem::take(&mut handles),
                    );

                    return Err(rustler::Error::Term(Box::new(reason)));
                }
                Err(_) => {
                    shutdown_renderer_runtime(
                        ShutdownRuntimeContext {
                            running_flag: Arc::clone(&running_flag),
                            backend_wake: backend_wake.clone(),
                            stop_flag: Arc::clone(&stop_flag),
                            tree_tx: tree_tx.clone(),
                            event_tx: event_tx.clone(),
                            render_tx: render_sender.clone(),
                            close_signal_log,
                            log_render,
                            log_input,
                        },
                        std::mem::take(&mut handles),
                    );

                    return Err(rustler::Error::Term(Box::new(
                        "failed to receive backend startup info",
                    )));
                }
            };

            backend_wake = startup.wake.clone();

            handles.tree_handle = Some(runtime::tree_actor::spawn_tree_actor(
                tree_rx,
                TreeActorConfig {
                    render_sender: render_sender.clone(),
                    event_tx: event_tx.clone(),
                    render_counter: Arc::clone(&render_counter),
                    stats: renderer_stats.clone(),
                    log_input,
                    window_wake: startup.wake.clone(),
                    initial_width,
                    initial_height,
                },
            ));

            (BackendKind::Wayland, startup.prime_video_supported)
        }
        #[cfg(all(feature = "drm", target_os = "linux"))]
        BackendKind::Drm => {
            let presenter_wake = EventFd::new().map_err(|err| {
                rustler::Error::Term(Box::new(format!(
                    "creating DRM presenter wake failed: {err}"
                )))
            })?;
            let input_wake = EventFd::new().map_err(|err| {
                rustler::Error::Term(Box::new(format!("creating DRM input wake failed: {err}")))
            })?;
            backend_wake = BackendWakeHandle::new(drm::DrmBackendWake::new(
                presenter_wake.clone(),
                input_wake.clone(),
            ));

            let (screen_tx, screen_rx) = bounded(1);
            let (startup_tx, startup_rx) = std::sync::mpsc::channel();
            let event_tx_clone = event_tx.clone();
            let drm_cursor_state_for_input = Arc::clone(&drm_cursor_state);
            let stop_clone = Arc::clone(&stop_flag);
            let input_log = log_input;
            let drm_input_size = (initial_width, initial_height);
            let backend_wake_for_input = backend_wake.clone();
            let input_wake_for_input = input_wake.clone();
            let video_registry_clone = Arc::clone(&video_registry);

            handles.input_handle = Some(thread::spawn(move || {
                let mut input = DrmInput::new(
                    drm_input_size,
                    screen_rx,
                    event_tx_clone,
                    drm_cursor_state_for_input,
                    Arc::clone(&stop_clone),
                    backend_wake_for_input,
                    input_wake_for_input,
                    input_log,
                );
                input.run();
            }));

            let running_flag_clone = Arc::clone(&running_flag);
            let stop_for_thread = Arc::clone(&stop_flag);
            let render_counter_clone = Arc::clone(&render_counter);
            let tree_tx_clone = tree_tx.clone();
            let event_tx_clone = event_tx.clone();
            let drm_cursor_state_for_backend = Arc::clone(&drm_cursor_state);
            let native_log_for_backend = Arc::clone(&native_log);
            let renderer_stats_for_backend = renderer_stats.clone();
            let presenter_wake_for_backend = presenter_wake.clone();
            let input_wake_for_backend = input_wake.clone();
            let drm_config = drm::DrmRunConfig {
                requested_size: Some((config.width, config.height)),
                card_path: config.drm_card.clone(),
                asset_config: config.asset_config.clone(),
                startup_retries: config.drm_startup_retries,
                cursor_overrides: config.drm_cursor_overrides.clone(),
                retry_interval_ms: config.drm_retry_interval_ms,
                hw_cursor: config.drm_hw_cursor,
                render_log: log_render,
            };

            handles.backend_handle = Some(thread::spawn(move || {
                drm::run(
                    drm::DrmRunContext {
                        startup_tx,
                        stop: stop_for_thread,
                        running_flag: running_flag_clone,
                        presenter_wake: presenter_wake_for_backend,
                        input_wake: input_wake_for_backend,
                        tree_tx: tree_tx_clone,
                        render_rx,
                        cursor_icon_rx: backend_cursor_rx,
                        cursor_state: drm_cursor_state_for_backend,
                        event_tx: event_tx_clone,
                        screen_tx,
                        render_counter: render_counter_clone,
                        native_log: native_log_for_backend,
                        stats: renderer_stats_for_backend,
                        video_registry: video_registry_clone,
                    },
                    drm_config,
                );
            }));

            match startup_rx.recv() {
                Ok(Ok(())) => {}
                Ok(Err(reason)) => {
                    shutdown_renderer_runtime(
                        ShutdownRuntimeContext {
                            running_flag: Arc::clone(&running_flag),
                            backend_wake: backend_wake.clone(),
                            stop_flag: Arc::clone(&stop_flag),
                            tree_tx: tree_tx.clone(),
                            event_tx: event_tx.clone(),
                            render_tx: render_sender.clone(),
                            close_signal_log,
                            log_render,
                            log_input,
                        },
                        std::mem::take(&mut handles),
                    );

                    return Err(rustler::Error::Term(Box::new(reason)));
                }
                Err(_) => {
                    shutdown_renderer_runtime(
                        ShutdownRuntimeContext {
                            running_flag: Arc::clone(&running_flag),
                            backend_wake: backend_wake.clone(),
                            stop_flag: Arc::clone(&stop_flag),
                            tree_tx: tree_tx.clone(),
                            event_tx: event_tx.clone(),
                            render_tx: render_sender.clone(),
                            close_signal_log,
                            log_render,
                            log_input,
                        },
                        std::mem::take(&mut handles),
                    );

                    return Err(rustler::Error::Term(Box::new(
                        "failed to receive DRM backend startup info",
                    )));
                }
            }

            handles.tree_handle = Some(runtime::tree_actor::spawn_tree_actor(
                tree_rx,
                TreeActorConfig {
                    render_sender: render_sender.clone(),
                    event_tx: event_tx.clone(),
                    render_counter: Arc::clone(&render_counter),
                    stats: renderer_stats.clone(),
                    log_input,
                    window_wake: backend_wake.clone(),
                    initial_width,
                    initial_height,
                },
            ));

            (BackendKind::Drm, true)
        }
        #[cfg(feature = "macos")]
        BackendKind::Macos => unreachable!("macOS backend should return before runtime startup"),
    };

    handles.event_handle = Some(spawn_event_actor(SpawnEventActorConfig {
        event_rx,
        tree_tx: tree_tx.clone(),
        backend_cursor_tx: Some(backend_cursor_tx),
        backend_wake: backend_wake.clone(),
        scroll_line_pixels: config.scroll_line_pixels,
        log_render,
        system_clipboard,
        stats: renderer_stats.clone(),
    }));

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    let video_wake = match backend {
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        BackendKind::Wayland => VideoWake::new(backend_wake.clone()),
        #[cfg(all(feature = "drm", target_os = "linux"))]
        BackendKind::Drm => VideoWake::new(backend_wake.clone()),
        #[allow(unreachable_patterns)]
        _ => VideoWake::noop(),
    };
    #[cfg(not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )))]
    let video_wake = VideoWake::noop();

    let resource = RendererResource {
        running_flag,
        backend_wake,
        stop_flag,
        tree_tx,
        event_tx,
        input_target,
        render_tx: render_sender,
        video_registry,
        video_wake,
        prime_video_supported,
        native_log,
        stats: renderer_stats,
        close_signal_log,
        log_render,
        log_input,
        handles: Mutex::new(Some(handles)),
    };

    Ok(ResourceArc::new(resource))
}

#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
fn start_native_renderer_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    let _ = (config, initial_log_target);

    Err(rustler::Error::Term(Box::new(
        "no native window backend is compiled for this build".to_string(),
    )))
}

#[rustler::nif(schedule = "DirtyIo")]
fn start(
    env: Env,
    title: String,
    width: u32,
    height: u32,
) -> NifResult<ResourceArc<RendererResource>> {
    #[cfg(all(feature = "wayland", target_os = "linux"))]
    {
        start_with_config(
            StartConfig {
                backend: BackendKind::Wayland,
                title,
                width,
                height,
                scroll_line_pixels: input::SCROLL_LINE_PIXELS,
                asset_config: AssetConfig::default(),
                drm_card: None,
                drm_startup_retries: 40,
                drm_retry_interval_ms: 250,
                drm_hw_cursor: true,
                drm_cursor_overrides: Vec::new(),
                drm_input_log: false,
                render_log: false,
                close_signal_log: false,
                stats_enabled: false,
                renderer_stats_log: false,
            },
            Some(env.pid()),
        )
    }
    #[cfg(not(all(feature = "wayland", target_os = "linux")))]
    {
        let _ = (env, title, width, height);
        Err(rustler::Error::Term(Box::new(
            "Wayland backend not compiled; add :wayland to config :emerge, compiled_backends: [...]"
                .to_string(),
        )))
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn start_opts(env: Env, opts: StartOptsNif) -> NifResult<ResourceArc<RendererResource>> {
    let backend = opts.backend.to_lowercase();
    let backend =
        parse_backend_name(&backend).map_err(|reason| rustler::Error::Term(Box::new(reason)))?;
    let asset_config = AssetConfig {
        sources: opts.asset_sources,
        runtime_enabled: opts.asset_runtime_enabled,
        runtime_allowlist: opts.asset_allowlist,
        runtime_follow_symlinks: opts.asset_follow_symlinks,
        runtime_max_file_size: opts.asset_max_file_size,
        runtime_extensions: opts.asset_extensions,
    };
    let drm_cursor_overrides = parse_drm_cursor_overrides(opts.drm_cursor)
        .map_err(|reason| rustler::Error::Term(Box::new(reason)))?;

    start_with_config(
        StartConfig {
            backend,
            title: opts.title,
            width: opts.width,
            height: opts.height,
            scroll_line_pixels: opts.scroll_line_pixels,
            asset_config,
            drm_card: opts.drm_card,
            drm_startup_retries: opts.drm_startup_retries,
            drm_retry_interval_ms: opts.drm_retry_interval_ms,
            drm_hw_cursor: opts.hw_cursor,
            drm_cursor_overrides,
            drm_input_log: opts.input_log,
            render_log: opts.render_log,
            close_signal_log: opts.close_signal_log,
            stats_enabled: opts.stats_enabled,
            renderer_stats_log: opts.renderer_stats_log,
        },
        Some(env.pid()),
    )
}

#[rustler::nif(schedule = "DirtyIo")]
fn stop(renderer: ResourceArc<RendererResource>) -> Atom {
    renderer.stop();
    atoms::ok()
}

fn ensure_video_target_mode_supported(
    prime_video_supported: bool,
    mode: VideoMode,
) -> Result<(), String> {
    if matches!(mode, VideoMode::Prime) && !prime_video_supported {
        Err("prime video targets require a Prime-capable backend (:wayland or :drm)".to_string())
    } else {
        Ok(())
    }
}

#[rustler::nif]
fn video_target_new(
    renderer: ResourceArc<RendererResource>,
    id: String,
    width: u32,
    height: u32,
    mode: String,
) -> Result<ResourceArc<VideoTargetResource>, String> {
    let mode = VideoMode::parse(&mode)?;
    ensure_video_target_mode_supported(renderer.prime_video_supported, mode)?;

    let spec = video::VideoTargetSpec {
        id: id.clone(),
        width,
        height,
        mode,
    };
    renderer.video_registry.create_target(spec)?;

    Ok(ResourceArc::new(VideoTargetResource {
        id,
        _width: width,
        _height: height,
        _mode: mode,
        registry: Arc::clone(&renderer.video_registry),
        wake: renderer.video_wake.clone(),
    }))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_target_submit_prime(
    target: ResourceArc<VideoTargetResource>,
    desc: video::PrimeDesc,
) -> Result<bool, String> {
    let spec = target.registry.target_spec(&target.id)?;
    desc.validate_for_target(&target.id, spec.mode, spec.width, spec.height)?;
    target.registry.submit_prime(&target.id, desc.into())?;
    target.wake.notify();
    Ok(true)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_upload(renderer: ResourceArc<RendererResource>, data: Binary) -> Atom {
    let bytes = data.as_slice().to_vec();
    send_tree(
        &renderer.tree_tx,
        TreeMsg::UploadTree { bytes },
        renderer.log_render,
    );
    atoms::ok()
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_patch(renderer: ResourceArc<RendererResource>, data: Binary) -> Atom {
    let bytes = data.as_slice().to_vec();
    send_tree(
        &renderer.tree_tx,
        TreeMsg::PatchTree { bytes },
        renderer.log_render,
    );
    atoms::ok()
}

#[rustler::nif]
fn measure_text(text: String, font_size: f32) -> (f32, f32, f32, f32) {
    services::measure_text(&text, font_size)
}

/// Load a font from binary data and register it with a name.
///
/// - `name`: Family name to register (e.g., "my-font")
/// - `weight`: Font weight (100-900, 400=normal, 700=bold)
/// - `italic`: Whether this is an italic variant
/// - `data`: Binary font data (TTF file contents)
#[rustler::nif(schedule = "DirtyIo")]
fn load_font_nif(name: String, weight: u32, italic: bool, data: Binary) -> Result<bool, String> {
    services::load_font_bytes(&name, weight as u16, italic, data.as_slice())?;
    Ok(true)
}

#[rustler::nif(schedule = "DirtyIo")]
fn configure_assets_nif(
    _renderer: ResourceArc<RendererResource>,
    sources: Vec<String>,
    runtime_enabled: bool,
    allowlist: Vec<String>,
    follow_symlinks: bool,
    max_file_size: u64,
    extensions: Vec<String>,
) -> Atom {
    services::configure_assets(AssetConfig {
        sources,
        runtime_enabled,
        runtime_allowlist: allowlist,
        runtime_follow_symlinks: follow_symlinks,
        runtime_max_file_size: max_file_size,
        runtime_extensions: extensions,
    });
    atoms::ok()
}

#[rustler::nif]
fn is_running(renderer: ResourceArc<RendererResource>) -> bool {
    renderer.running_flag.load(Ordering::Relaxed)
}

// ============================================================================
// Input NIF Functions
// ============================================================================

/// Set the input event mask to filter which events are sent.
///
/// Mask bits:
/// - 0x01: Key events
/// - 0x02: Text input commit/preedit events
/// - 0x04: Cursor position events
/// - 0x08: Cursor button events
/// - 0x10: Cursor scroll events
/// - 0x20: Cursor enter/exit events
/// - 0x40: Resize events
/// - 0x80: Focus events
/// - 0xFF: All events
#[rustler::nif]
fn set_input_mask(renderer: ResourceArc<RendererResource>, mask: u32) -> Atom {
    send_event(
        &renderer.event_tx,
        EventMsg::SetInputMask(mask),
        renderer.log_input,
    );
    atoms::ok()
}

/// Set the target process to receive input events.
///
/// Input events are sent directly to the target process as
/// `{:emerge_skia_event, event}` messages.
#[rustler::nif]
fn set_input_target(
    env: Env<'_>,
    renderer: ResourceArc<RendererResource>,
    pid: Option<LocalPid>,
) -> Atom {
    renderer.input_target.set_target(pid);

    if let Some(target) = pid {
        events::send_running_message_in_env(env, target);
    }

    send_event(
        &renderer.event_tx,
        EventMsg::SetInputTarget(pid),
        renderer.log_input,
    );
    atoms::ok()
}

#[rustler::nif]
fn set_log_target(renderer: ResourceArc<RendererResource>, pid: Option<LocalPid>) -> Atom {
    renderer.native_log.set_target(pid);
    atoms::ok()
}

#[rustler::nif(name = "stats", schedule = "DirtyCpu")]
fn stats_nif<'a>(
    env: Env<'a>,
    resource: Term<'a>,
    command: StatsCommandNif,
) -> Result<Term<'a>, String> {
    if let Ok(renderer) = resource.decode::<ResourceArc<RendererResource>>() {
        return renderer_stats_snapshot(env, renderer, command);
    }

    if let Ok(tree) = resource.decode::<ResourceArc<TreeResource>>() {
        return tree_stats_snapshot(env, tree, command);
    }

    Err("unsupported stats resource".to_string())
}

fn renderer_stats_snapshot<'a>(
    env: Env<'a>,
    renderer: ResourceArc<RendererResource>,
    command: StatsCommandNif,
) -> Result<Term<'a>, String> {
    if matches!(command, StatsCommandNif::Configure(_)) {
        return Err("renderer stats are configured when the renderer starts".to_string());
    }

    let Some(stats) = renderer.stats.as_ref() else {
        return Ok(StatsSnapshotNif::disabled("renderer").encode(env));
    };

    match command {
        StatsCommandNif::Peek => {
            Ok(StatsSnapshotNif::from_snapshot("renderer", true, false, &stats.peek()).encode(env))
        }
        StatsCommandNif::Take => {
            Ok(StatsSnapshotNif::from_snapshot("renderer", true, true, &stats.take()).encode(env))
        }
        StatsCommandNif::Reset => {
            stats.reset();
            Ok(StatsSnapshotNif::from_snapshot("renderer", true, false, &stats.peek()).encode(env))
        }
        StatsCommandNif::Configure(_) => unreachable!(),
    }
}

fn tree_stats_snapshot<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    command: StatsCommandNif,
) -> Result<Term<'a>, String> {
    match command {
        StatsCommandNif::Configure(config) => {
            let next_stats = config
                .enabled
                .then(|| Arc::new(RendererStatsCollector::new()));

            {
                let mut stats_slot = tree_res
                    .stats
                    .lock()
                    .map_err(|_| "failed to lock tree stats".to_string())?;
                *stats_slot = next_stats.clone();
            }

            {
                let mut tree = tree_res
                    .tree
                    .lock()
                    .map_err(|_| "failed to lock tree".to_string())?;
                tree.set_layout_cache_stats_enabled(config.enabled);
            }

            if let Some(stats) = next_stats {
                Ok(StatsSnapshotNif::from_snapshot("tree", true, false, &stats.peek()).encode(env))
            } else {
                Ok(StatsSnapshotNif::disabled("tree").encode(env))
            }
        }
        StatsCommandNif::Peek | StatsCommandNif::Take | StatsCommandNif::Reset => {
            let stats = tree_res
                .stats
                .lock()
                .map_err(|_| "failed to lock tree stats".to_string())?
                .clone();

            let Some(stats) = stats else {
                return Ok(StatsSnapshotNif::disabled("tree").encode(env));
            };

            match command {
                StatsCommandNif::Peek => {
                    Ok(
                        StatsSnapshotNif::from_snapshot("tree", true, false, &stats.peek())
                            .encode(env),
                    )
                }
                StatsCommandNif::Take => {
                    Ok(
                        StatsSnapshotNif::from_snapshot("tree", true, true, &stats.take())
                            .encode(env),
                    )
                }
                StatsCommandNif::Reset => {
                    stats.reset();
                    Ok(
                        StatsSnapshotNif::from_snapshot("tree", true, false, &stats.peek())
                            .encode(env),
                    )
                }
                StatsCommandNif::Configure(_) => unreachable!(),
            }
        }
    }
}

// ============================================================================
// Raster NIF Functions
// ============================================================================

/// Render an encoded tree to an RGBA pixel buffer (synchronous, no window).
#[rustler::nif(schedule = "DirtyCpu")]
fn render_tree_to_pixels_nif<'a>(
    env: Env<'a>,
    data: Binary,
    opts: RenderTreeOffscreenOptsNif,
) -> Result<Binary<'a>, String> {
    let output = services::render_tree_to_pixels(data.as_slice(), offscreen_opts_from_nif(opts))?;

    let mut binary = NewBinary::new(env, output.len());
    binary.as_mut_slice().copy_from_slice(&output);

    Ok(binary.into())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn render_tree_to_png_nif<'a>(
    env: Env<'a>,
    data: Binary,
    opts: RenderTreeOffscreenOptsNif,
) -> Result<Binary<'a>, String> {
    let encoded = services::render_tree_to_png(data.as_slice(), offscreen_opts_from_nif(opts))?;

    let mut binary = NewBinary::new(env, encoded.len());
    binary.as_mut_slice().copy_from_slice(&encoded);

    Ok(binary.into())
}

fn offscreen_opts_from_nif(opts: RenderTreeOffscreenOptsNif) -> services::OffscreenRenderOptions {
    services::OffscreenRenderOptions {
        width: opts.width,
        height: opts.height,
        scale: opts.scale,
        asset_mode: opts.asset_mode,
        asset_timeout_ms: opts.asset_timeout_ms,
        asset_config: AssetConfig {
            sources: opts.sources,
            runtime_enabled: opts.runtime_enabled,
            runtime_allowlist: opts.allowlist,
            runtime_follow_symlinks: opts.follow_symlinks,
            runtime_max_file_size: opts.max_file_size,
            runtime_extensions: opts.extensions,
        },
    }
}

fn tree_lock_error() -> String {
    "failed to lock tree".to_string()
}

fn clone_tree_resource(tree_res: &TreeResource) -> Result<ElementTree, String> {
    let tree = tree_res.tree.lock().map_err(|_| tree_lock_error())?;
    Ok(tree.clone())
}

fn replace_tree_resource(tree_res: &TreeResource, tree: ElementTree) -> Result<(), String> {
    let mut guard = tree_res.tree.lock().map_err(|_| tree_lock_error())?;
    *guard = tree;
    Ok(())
}

fn upload_tree_resource(tree_res: &TreeResource, tree: ElementTree) -> Result<(), String> {
    let mut guard = tree_res.tree.lock().map_err(|_| tree_lock_error())?;
    guard.replace_with_uploaded(tree);
    Ok(())
}

fn encode_layout_frames<'a>(env: Env<'a>, tree: &ElementTree) -> LayoutFrames<'a> {
    tree.iter_node_pairs()
        .filter_map(|(id, element)| {
            if element.is_ghost() {
                return None;
            }

            element.layout.frame.map(|frame| {
                let id_bytes = id.to_be_bytes();
                let mut id_binary = NewBinary::new(env, id_bytes.len());
                id_binary.as_mut_slice().copy_from_slice(&id_bytes);
                (
                    id_binary.into(),
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                )
            })
        })
        .collect()
}

// ============================================================================
// Tree NIF Functions
// ============================================================================

/// Create a new empty tree resource.
#[rustler::nif]
fn tree_new() -> ResourceArc<TreeResource> {
    ResourceArc::new(TreeResource {
        tree: Mutex::new(ElementTree::new()),
        stats: Mutex::new(None),
    })
}

/// Upload a full tree from EMRG binary format.
/// Replaces any existing tree contents.
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_upload(tree_res: ResourceArc<TreeResource>, data: Binary) -> Result<bool, String> {
    let decoded = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;
    upload_tree_resource(&tree_res, decoded)?;
    Ok(true)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn tree_upload_roundtrip<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    data: Binary,
) -> Result<Binary<'a>, String> {
    let decoded = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;
    let encoded = encode_tree_binary(env, &decoded);
    upload_tree_resource(&tree_res, decoded)?;
    Ok(encoded)
}

/// Apply patches to an existing tree.
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_patch(tree_res: ResourceArc<TreeResource>, data: Binary) -> Result<bool, String> {
    let patches = tree::patch::decode_patches(data.as_slice()).map_err(|e| e.to_string())?;
    let mut tree = clone_tree_resource(&tree_res)?;
    tree::patch::apply_patches(&mut tree, patches)?;
    replace_tree_resource(&tree_res, tree)?;
    Ok(true)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn tree_patch_roundtrip<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    data: Binary,
) -> Result<Binary<'a>, String> {
    let patches = tree::patch::decode_patches(data.as_slice()).map_err(|e| e.to_string())?;
    let mut tree = clone_tree_resource(&tree_res)?;
    tree::patch::apply_patches(&mut tree, patches)?;
    let encoded = encode_tree_binary(env, &tree);
    replace_tree_resource(&tree_res, tree)?;
    Ok(encoded)
}

/// Get the number of nodes in the tree.
#[rustler::nif]
fn tree_node_count(tree_res: ResourceArc<TreeResource>) -> usize {
    if let Ok(tree) = tree_res.tree.lock() {
        tree.len()
    } else {
        0
    }
}

/// Check if the tree is empty.
#[rustler::nif]
fn tree_is_empty(tree_res: ResourceArc<TreeResource>) -> bool {
    if let Ok(tree) = tree_res.tree.lock() {
        tree.is_empty()
    } else {
        true
    }
}

/// Clear the tree.
#[rustler::nif]
fn tree_clear(tree_res: ResourceArc<TreeResource>) -> Atom {
    if let Ok(mut tree) = tree_res.tree.lock() {
        tree.clear();
    }
    atoms::ok()
}

/// Compute layout for the tree with the given constraints and scale factor.
/// Returns list of {id_bytes, x, y, width, height} tuples for all elements.
/// Scale is applied to all pixel-based attributes (px sizes, padding, spacing, etc.)
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_layout<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    width: f64,
    height: f64,
    scale: f64,
) -> Result<LayoutFrames<'a>, String> {
    let stats = tree_res
        .stats
        .lock()
        .map_err(|_| "failed to lock tree stats".to_string())?
        .clone();
    let mut tree = clone_tree_resource(&tree_res)?;
    tree.set_layout_cache_stats_enabled(
        stats
            .as_ref()
            .is_some_and(|stats| stats.layout_cache_enabled()),
    );
    let constraint = tree::layout::Constraint::new(width as f32, height as f32);
    let layout_started_at = Instant::now();
    tree::layout::layout_tree_default(&mut tree, constraint, scale as f32);
    if let Some(stats) = stats.as_ref() {
        stats.record_layout(layout_started_at.elapsed());
        stats.record_layout_cache(tree.layout_cache_stats());
    }
    let frames = encode_layout_frames(env, &tree);
    replace_tree_resource(&tree_res, tree)?;
    Ok(frames)
}

/// Round-trip EMRG binary: decode in Rust and re-encode.
#[rustler::nif(schedule = "DirtyCpu")]
fn tree_roundtrip<'a>(env: Env<'a>, data: Binary) -> Result<Binary<'a>, String> {
    let tree = tree::deserialize::decode_tree(data.as_slice()).map_err(|e| e.to_string())?;
    Ok(encode_tree_binary(env, &tree))
}

type HoverMsg<'a> = (Binary<'a>, bool);
type HoverMsgList<'a> = Vec<HoverMsg<'a>>;

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_new(width: u32, height: u32) -> Result<ResourceArc<TestHarnessResource>, String> {
    let (tree_tx, tree_rx_proxy) = bounded(512);
    let (tree_actor_tx, tree_actor_rx) = bounded(512);
    let (tree_tap_tx, tree_tap_rx) = bounded(4096);
    let (event_tx, event_rx) = bounded(4096);
    let (render_tx, render_rx) = bounded(8);
    let render_sender = RenderSender {
        tx: render_tx,
        drop_rx: render_rx.clone(),
        log_render: false,
    };
    let render_counter = Arc::new(AtomicU64::new(0));

    assets::start(tree_tx.clone(), false);

    let proxy_handle = thread::spawn(move || {
        while let Ok(msg) = tree_rx_proxy.recv() {
            let is_stop = matches!(msg, TreeMsg::Stop);
            let _ = tree_tap_tx.send(msg.clone());
            if tree_actor_tx.send(msg).is_err() || is_stop {
                break;
            }
        }
    });

    let event_handle = spawn_event_actor(SpawnEventActorConfig {
        event_rx,
        tree_tx: tree_tx.clone(),
        backend_cursor_tx: None,
        backend_wake: BackendWakeHandle::noop(),
        scroll_line_pixels: input::SCROLL_LINE_PIXELS,
        log_render: false,
        system_clipboard: false,
        stats: None,
    });
    let tree_handle = spawn_tree_actor_with_initial_tree(
        tree_actor_rx,
        TreeActorConfig {
            render_sender,
            event_tx: event_tx.clone(),
            render_counter,
            stats: None,
            log_input: false,
            window_wake: BackendWakeHandle::noop(),
            initial_width: width,
            initial_height: height,
        },
        ElementTree::new(),
    );

    Ok(ResourceArc::new(TestHarnessResource {
        tree_tx,
        event_tx,
        render_rx,
        tree_tap_rx,
        base_instant: Mutex::new(Instant::now()),
        handles: Mutex::new(Some(TestHarnessHandles {
            proxy_handle,
            tree_handle,
            event_handle,
        })),
    }))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn test_harness_upload(harness: ResourceArc<TestHarnessResource>, data: Binary) -> Atom {
    send_tree(
        &harness.tree_tx,
        TreeMsg::UploadTree {
            bytes: data.as_slice().to_vec(),
        },
        false,
    );
    atoms::ok()
}

#[rustler::nif(schedule = "DirtyCpu")]
fn test_harness_patch(harness: ResourceArc<TestHarnessResource>, data: Binary) -> Atom {
    send_tree(
        &harness.tree_tx,
        TreeMsg::PatchTree {
            bytes: data.as_slice().to_vec(),
        },
        false,
    );
    atoms::ok()
}

#[rustler::nif]
fn test_harness_cursor_pos(harness: ResourceArc<TestHarnessResource>, x: f64, y: f64) -> Atom {
    send_event(
        &harness.event_tx,
        EventMsg::InputEvent(input::InputEvent::CursorPos {
            x: x as f32,
            y: y as f32,
        }),
        false,
    );
    atoms::ok()
}

#[rustler::nif]
fn test_harness_animation_pulse(
    harness: ResourceArc<TestHarnessResource>,
    presented_ms: u64,
    predicted_ms: u64,
) -> Result<bool, String> {
    let base_instant = *harness
        .base_instant
        .lock()
        .map_err(|_| "failed to lock test harness clock".to_string())?;
    send_tree(
        &harness.tree_tx,
        TreeMsg::AnimationPulse {
            presented_at: base_instant + Duration::from_millis(presented_ms),
            predicted_next_present_at: base_instant + Duration::from_millis(predicted_ms),
        },
        false,
    );
    Ok(true)
}

#[rustler::nif]
fn test_harness_reset_clock(harness: ResourceArc<TestHarnessResource>) -> Atom {
    if let Ok(mut base_instant) = harness.base_instant.lock() {
        *base_instant = Instant::now();
    }
    atoms::ok()
}

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_await_render(
    harness: ResourceArc<TestHarnessResource>,
    timeout_ms: u64,
) -> Result<bool, String> {
    let timeout = Duration::from_millis(timeout_ms);

    match harness.render_rx.recv_timeout(timeout) {
        Ok(_) => {}
        Err(RecvTimeoutError::Timeout) => return Err("render timeout".to_string()),
        Err(RecvTimeoutError::Disconnected) => {
            return Err("render channel disconnected".to_string());
        }
    }

    while harness
        .render_rx
        .recv_timeout(Duration::from_millis(10))
        .is_ok()
    {}

    Ok(true)
}

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_drain_mouse_over_msgs<'a>(
    env: Env<'a>,
    harness: ResourceArc<TestHarnessResource>,
    timeout_ms: u64,
) -> HoverMsgList<'a> {
    let timeout = Duration::from_millis(timeout_ms);
    let mut flat = Vec::new();

    if let Ok(msg) = harness.tree_tap_rx.recv_timeout(timeout) {
        runtime::tree_actor::push_tree_message_flat(msg, &mut flat);
        while let Ok(msg) = harness.tree_tap_rx.recv_timeout(Duration::from_millis(10)) {
            runtime::tree_actor::push_tree_message_flat(msg, &mut flat);
        }
    }

    flat.into_iter()
        .filter_map(|msg| match msg {
            TreeMsg::SetMouseOverActive { element_id, active } => {
                Some(encode_hover_msg(env, &element_id, active))
            }
            _ => None,
        })
        .collect()
}

#[rustler::nif(schedule = "DirtyIo")]
fn test_harness_stop(harness: ResourceArc<TestHarnessResource>) -> Atom {
    harness.stop_inner();
    atoms::ok()
}

fn encode_hover_msg<'a>(env: Env<'a>, element_id: &NodeId, active: bool) -> HoverMsg<'a> {
    let id_bytes = element_id.to_be_bytes();
    let mut id_binary = NewBinary::new(env, id_bytes.len());
    id_binary.as_mut_slice().copy_from_slice(&id_bytes);
    (id_binary.into(), active)
}

fn encode_tree_binary<'a>(env: Env<'a>, tree: &ElementTree) -> Binary<'a> {
    let encoded = tree::serialize::encode_tree(tree);
    let mut binary = NewBinary::new(env, encoded.len());
    binary.as_mut_slice().copy_from_slice(&encoded);
    binary.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::RegistryRebuildPayload;
    use crate::events::test_support::AnimatedNearbyHitCase;
    use crate::input::InputEvent;
    use crate::tree::element::NodeId;
    use crossbeam_channel::RecvTimeoutError;

    struct LiveActorHarness {
        tree_tx: Sender<TreeMsg>,
        event_tx: Sender<EventMsg>,
        render_rx: Receiver<RenderMsg>,
        tree_tap_rx: Receiver<TreeMsg>,
        proxy_handle: thread::JoinHandle<()>,
        tree_handle: thread::JoinHandle<()>,
        event_handle: thread::JoinHandle<()>,
    }

    impl LiveActorHarness {
        fn new(width: u32, height: u32, initial_tree: ElementTree) -> Self {
            let (tree_tx, tree_rx_proxy) = bounded(512);
            let (tree_actor_tx, tree_actor_rx) = bounded(512);
            let (tree_tap_tx, tree_tap_rx) = bounded(4096);
            let (event_tx, event_rx) = bounded(4096);
            let (render_tx, render_rx) = bounded(8);
            let render_sender = RenderSender {
                tx: render_tx,
                drop_rx: render_rx.clone(),
                log_render: false,
            };
            let render_counter = Arc::new(AtomicU64::new(0));

            assets::start(tree_tx.clone(), false);

            let proxy_handle = thread::spawn(move || {
                while let Ok(msg) = tree_rx_proxy.recv() {
                    let is_stop = matches!(msg, TreeMsg::Stop);
                    let _ = tree_tap_tx.send(msg.clone());
                    if tree_actor_tx.send(msg).is_err() || is_stop {
                        break;
                    }
                }
            });

            let event_handle = spawn_event_actor(SpawnEventActorConfig {
                event_rx,
                tree_tx: tree_tx.clone(),
                backend_cursor_tx: None,
                backend_wake: BackendWakeHandle::noop(),
                scroll_line_pixels: input::SCROLL_LINE_PIXELS,
                log_render: false,
                system_clipboard: false,
                stats: None,
            });
            let tree_handle = spawn_tree_actor_with_initial_tree(
                tree_actor_rx,
                TreeActorConfig {
                    render_sender,
                    event_tx: event_tx.clone(),
                    render_counter,
                    stats: None,
                    log_input: false,
                    window_wake: BackendWakeHandle::noop(),
                    initial_width: width,
                    initial_height: height,
                },
                initial_tree,
            );

            Self {
                tree_tx,
                event_tx,
                render_rx,
                tree_tap_rx,
                proxy_handle,
                tree_handle,
                event_handle,
            }
        }

        fn send_tree(&self, msg: TreeMsg) {
            super::send_tree(&self.tree_tx, msg, false);
        }

        fn send_input(&self, event: crate::input::InputEvent) {
            super::send_event(&self.event_tx, EventMsg::InputEvent(event), false);
        }

        fn wait_for_render_settle(&self) {
            match self.render_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("expected render message"),
                Err(RecvTimeoutError::Disconnected) => panic!("render channel disconnected"),
            }

            while self
                .render_rx
                .recv_timeout(Duration::from_millis(15))
                .is_ok()
            {}
        }

        fn drain_set_mouse_over_active(&self, element_id: &NodeId) -> Vec<bool> {
            let mut msgs = Vec::new();
            while let Ok(msg) = self.tree_tap_rx.try_recv() {
                runtime::tree_actor::push_tree_message_flat(msg, &mut msgs);
            }

            msgs.into_iter()
                .filter_map(|msg| match msg {
                    TreeMsg::SetMouseOverActive {
                        element_id: id,
                        active,
                    } if &id == element_id => Some(active),
                    _ => None,
                })
                .collect()
        }

        fn stop(self) {
            super::send_event(&self.event_tx, EventMsg::Stop, false);
            super::send_tree(&self.tree_tx, TreeMsg::Stop, false);
            let _ = self.proxy_handle.join();
            let _ = self.event_handle.join();
            let _ = self.tree_handle.join();
            assets::stop();
            clear_global_caches();
            trim_process_allocator();
        }
    }

    struct SpawnedEventActorHarness {
        event_tx: Sender<EventMsg>,
        tree_rx: Receiver<TreeMsg>,
        handle: thread::JoinHandle<()>,
    }

    impl SpawnedEventActorHarness {
        fn new() -> Self {
            let (event_tx, event_rx) = bounded(4096);
            let (tree_tx, tree_rx) = bounded(4096);
            let handle = spawn_event_actor(SpawnEventActorConfig {
                event_rx,
                tree_tx,
                backend_cursor_tx: None,
                backend_wake: BackendWakeHandle::noop(),
                scroll_line_pixels: input::SCROLL_LINE_PIXELS,
                log_render: false,
                system_clipboard: false,
                stats: None,
            });

            Self {
                event_tx,
                tree_rx,
                handle,
            }
        }

        fn send_input(&self, event: InputEvent) {
            super::send_event(&self.event_tx, EventMsg::InputEvent(event), false);
        }

        fn send_rebuild(&self, rebuild: RegistryRebuildPayload) {
            super::send_event(&self.event_tx, EventMsg::RegistryUpdate { rebuild }, false);
        }

        fn wait_for_tree_msgs_quiet(&self) -> Vec<TreeMsg> {
            collect_tree_messages_until_quiet(&self.tree_rx)
        }

        fn stop(self) {
            super::send_event(&self.event_tx, EventMsg::Stop, false);
            let _ = self.handle.join();
        }
    }

    fn collect_tree_messages_until_quiet(rx: &Receiver<TreeMsg>) -> Vec<TreeMsg> {
        let mut out = Vec::new();

        if let Ok(msg) = rx.recv_timeout(Duration::from_millis(50)) {
            runtime::tree_actor::push_tree_message_flat(msg, &mut out);
            while let Ok(msg) = rx.recv_timeout(Duration::from_millis(10)) {
                runtime::tree_actor::push_tree_message_flat(msg, &mut out);
            }
        }

        out
    }

    #[test]
    fn shutdown_renderer_runtime_stops_and_joins_threads() {
        let running_flag = Arc::new(AtomicBool::new(true));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let backend_wake = BackendWakeHandle::noop();

        let (tree_tx, tree_rx) = bounded(1);
        let (event_tx, event_rx) = bounded(1);
        let (render_tx, render_rx) = bounded(1);
        let render_sender = RenderSender {
            tx: render_tx,
            drop_rx: render_rx.clone(),
            log_render: false,
        };

        let tree_stopped = Arc::new(AtomicBool::new(false));
        let event_stopped = Arc::new(AtomicBool::new(false));
        let backend_stopped = Arc::new(AtomicBool::new(false));
        let input_stopped = Arc::new(AtomicBool::new(false));

        let tree_handle = {
            let tree_stopped = Arc::clone(&tree_stopped);

            thread::spawn(move || {
                if matches!(tree_rx.recv(), Ok(TreeMsg::Stop)) {
                    tree_stopped.store(true, Ordering::Relaxed);
                }
            })
        };

        let event_handle = {
            let event_stopped = Arc::clone(&event_stopped);

            thread::spawn(move || {
                if matches!(event_rx.recv(), Ok(EventMsg::Stop)) {
                    event_stopped.store(true, Ordering::Relaxed);
                }
            })
        };

        let backend_handle = {
            let backend_stopped = Arc::clone(&backend_stopped);

            thread::spawn(move || {
                if matches!(render_rx.recv(), Ok(RenderMsg::Stop)) {
                    backend_stopped.store(true, Ordering::Relaxed);
                }
            })
        };

        let input_handle = {
            let input_stopped = Arc::clone(&input_stopped);
            let stop_flag = Arc::clone(&stop_flag);

            thread::spawn(move || {
                while !stop_flag.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(1));
                }

                input_stopped.store(true, Ordering::Relaxed);
            })
        };

        shutdown_renderer_runtime(
            ShutdownRuntimeContext {
                running_flag: Arc::clone(&running_flag),
                backend_wake: backend_wake.clone(),
                stop_flag: Arc::clone(&stop_flag),
                tree_tx: tree_tx.clone(),
                event_tx: event_tx.clone(),
                render_tx: render_sender.clone(),
                close_signal_log: false,
                log_render: false,
                log_input: false,
            },
            RendererHandles {
                backend_handle: Some(backend_handle),
                input_handle: Some(input_handle),
                tree_handle: Some(tree_handle),
                event_handle: Some(event_handle),
                heartbeat_handle: None,
            },
        );

        assert!(!running_flag.load(Ordering::Relaxed));
        assert!(stop_flag.load(Ordering::Relaxed));
        assert!(tree_stopped.load(Ordering::Relaxed));
        assert!(event_stopped.load(Ordering::Relaxed));
        assert!(backend_stopped.load(Ordering::Relaxed));
        assert!(input_stopped.load(Ordering::Relaxed));
    }

    #[test]
    fn send_registry_update_does_not_block_when_event_channel_is_full() {
        let (event_tx, event_rx) = bounded(1);
        event_tx.send(EventMsg::Stop).unwrap();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let handle = thread::spawn(move || {
            runtime::tree_actor::send_registry_update(
                &event_tx,
                RegistryRebuildPayload::default(),
                false,
            );
            let _ = done_tx.send(());
        });

        let completed = done_rx.recv_timeout(Duration::from_millis(100)).is_ok();

        if completed {
            assert!(matches!(event_rx.try_recv(), Ok(EventMsg::Stop)));
        }

        drop(event_rx);
        let _ = handle.join();

        assert!(
            completed,
            "registry update send should not block when event channel is full"
        );
    }

    #[test]
    fn video_target_new_rejects_prime_for_non_prime_backends() {
        let err = ensure_video_target_mode_supported(false, VideoMode::Prime)
            .expect_err("prime target should be rejected");

        assert_eq!(
            err,
            "prime video targets require a Prime-capable backend (:wayland or :drm)"
        );
    }

    #[test]
    fn video_target_new_accepts_prime_for_prime_capable_wayland_renderer() {
        assert!(ensure_video_target_mode_supported(true, VideoMode::Prime).is_ok());
    }

    #[test]
    fn parse_backend_name_rejects_removed_legacy_backend() {
        assert_eq!(
            parse_backend_name("wayland_legacy"),
            Err("backend :wayland_legacy has been removed; use :wayland".to_string())
        );
    }

    #[test]
    fn parse_backend_name_rejects_unsupported_backend() {
        assert_eq!(
            parse_backend_name("bogus"),
            Err("unsupported backend: bogus".to_string())
        );
    }

    #[cfg(not(feature = "drm"))]
    #[test]
    fn parse_backend_name_rejects_drm_when_not_compiled() {
        assert_eq!(
            parse_backend_name("drm"),
            Err(
                "DRM backend not compiled; add :drm to config :emerge, compiled_backends: [...]"
                    .to_string()
            )
        );
    }

    #[cfg(not(feature = "wayland"))]
    #[test]
    fn parse_backend_name_rejects_wayland_when_not_compiled() {
        assert_eq!(
            parse_backend_name("wayland"),
            Err(
                "Wayland backend not compiled; add :wayland to config :emerge, compiled_backends: [...]"
                    .to_string()
            )
        );
    }

    #[test]
    fn spawned_event_actor_harness_activates_hover_on_first_target_sample() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let probe = case.probe("newly_occupied_outside_host");
        let harness = SpawnedEventActorHarness::new();

        harness.send_rebuild(case.rebuild_at(0, false));
        let _ = harness.wait_for_tree_msgs_quiet();

        harness.send_input(InputEvent::CursorPos {
            x: probe.point.0,
            y: probe.point.1,
        });
        assert!(harness.wait_for_tree_msgs_quiet().is_empty());

        harness.send_rebuild(case.rebuild_at(500, false));
        let msgs = harness.wait_for_tree_msgs_quiet();

        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == case.target_id && *active
        )));

        harness.stop();
    }

    #[test]
    fn live_actor_harness_static_cursor_activates_on_first_target_sample() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let probe = case.probe("newly_occupied_outside_host");
        let first_target_sample = case
            .first_target_sample_ms(probe.label)
            .expect("probe should eventually hit target");
        let base = Instant::now();
        let harness = LiveActorHarness::new(
            case.constraint.max_width(0.0) as u32,
            case.constraint.max_height(0.0) as u32,
            case.source_tree(false),
        );

        harness.send_tree(TreeMsg::AnimationPulse {
            presented_at: base,
            predicted_next_present_at: base,
        });
        harness.wait_for_render_settle();
        let _ = harness.drain_set_mouse_over_active(&case.target_id);

        harness.send_input(input::InputEvent::CursorPos {
            x: probe.point.0,
            y: probe.point.1,
        });

        let mut activation_sample = None;

        for sample_ms in (50..=1000).step_by(50) {
            harness.send_tree(TreeMsg::AnimationPulse {
                presented_at: base + Duration::from_millis(sample_ms),
                predicted_next_present_at: base + Duration::from_millis(sample_ms),
            });
            harness.wait_for_render_settle();

            let activations = harness.drain_set_mouse_over_active(&case.target_id);
            if activation_sample.is_none() && activations.into_iter().any(|active| active) {
                activation_sample = Some(sample_ms);
            }
        }

        harness.stop();

        assert_eq!(activation_sample, Some(first_target_sample));
    }

    #[test]
    fn live_actor_harness_render_driven_pulses_activate_hover_without_tree_quiet_waits() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let probe = case.probe("newly_occupied_outside_host");
        let base = Instant::now();
        let harness = LiveActorHarness::new(
            case.constraint.max_width(0.0) as u32,
            case.constraint.max_height(0.0) as u32,
            case.source_tree(false),
        );

        harness.send_tree(TreeMsg::AnimationPulse {
            presented_at: base,
            predicted_next_present_at: base,
        });
        match harness.render_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => panic!("expected initial render"),
            Err(RecvTimeoutError::Disconnected) => panic!("render channel disconnected"),
        }

        harness.send_input(input::InputEvent::CursorPos {
            x: probe.point.0,
            y: probe.point.1,
        });

        let mut saw_activation = false;

        for sample_ms in (50..=1400).step_by(50) {
            harness.send_tree(TreeMsg::AnimationPulse {
                presented_at: base + Duration::from_millis(sample_ms),
                predicted_next_present_at: base + Duration::from_millis(sample_ms),
            });

            match harness.render_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => panic!("expected render for sample {sample_ms}"),
                Err(RecvTimeoutError::Disconnected) => panic!("render channel disconnected"),
            }

            saw_activation |= harness
                .drain_set_mouse_over_active(&case.target_id)
                .into_iter()
                .any(|active| active);
        }

        saw_activation |= collect_tree_messages_until_quiet(&harness.tree_tap_rx)
            .into_iter()
            .any(|msg| {
                matches!(
                    msg,
                    TreeMsg::SetMouseOverActive { element_id, active }
                        if element_id == case.target_id && active
                )
            });

        harness.stop();

        assert!(saw_activation);
    }
}

rustler::init!("Elixir.EmergeSkia.Native");

fn parse_drm_cursor_overrides(
    overrides: Vec<DrmCursorOverrideNif>,
) -> Result<Vec<DrmCursorOverrideConfig>, String> {
    overrides
        .into_iter()
        .map(|entry| {
            Ok(DrmCursorOverrideConfig {
                icon: parse_cursor_icon_name(&entry.icon)?,
                source: entry.source,
                hotspot: (entry.hotspot_x, entry.hotspot_y),
            })
        })
        .collect()
}

fn parse_cursor_icon_name(value: &str) -> Result<CursorIcon, String> {
    match value {
        "default" => Ok(CursorIcon::Default),
        "text" => Ok(CursorIcon::Text),
        "pointer" => Ok(CursorIcon::Pointer),
        other => Err(format!("unsupported DRM cursor icon: {other}")),
    }
}

fn parse_backend_name(value: &str) -> Result<BackendKind, String> {
    match value {
        #[cfg(feature = "macos")]
        "macos" => Ok(BackendKind::Macos),
        #[cfg(not(feature = "macos"))]
        "macos" => Err(
            "macOS backend not compiled; add :macos to config :emerge, compiled_backends: [...]"
                .to_string(),
        ),
        #[cfg(all(feature = "drm", target_os = "linux"))]
        "drm" => Ok(BackendKind::Drm),
        #[cfg(not(all(feature = "drm", target_os = "linux")))]
        "drm" => Err(
            "DRM backend not compiled; add :drm to config :emerge, compiled_backends: [...]"
                .to_string(),
        ),
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        "wayland" => Ok(BackendKind::Wayland),
        #[cfg(not(all(feature = "wayland", target_os = "linux")))]
        "wayland" => Err(
            "Wayland backend not compiled; add :wayland to config :emerge, compiled_backends: [...]"
                .to_string(),
        ),
        "wayland_legacy" => {
            Err("backend :wayland_legacy has been removed; use :wayland".to_string())
        }
        other => Err(format!("unsupported backend: {other}")),
    }
}
