//! EmergeSkia NIF - Minimal Skia renderer for Elixir.
//!
//! This crate provides a Rustler NIF that exposes tree upload, layout,
//! rendering, and headless rasterization for Emerge.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Sender as CleanupSender, channel as cleanup_channel},
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

use rustler::{
    Atom, Binary, Decoder, Encoder, Env, LocalPid, NewBinary, NifResult, ResourceArc, Term,
};
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
pub mod paint_layer_payload_cache;
pub mod render_scene;
pub mod renderer;
pub mod runtime;
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
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use renderer::set_render_log_enabled;
use renderer::{RendererCacheConfig, RendererPaintLayerCacheConfig, clear_global_caches};
use runtime::tree_actor::{TreeActorConfig, spawn_tree_actor_with_initial_tree};
use stats::{
    LayoutCacheStats, RendererStatsCollector, RendererStatsSnapshot, RendererTimingMetric,
};
use std::time::Instant;
use tree::element::{ElementTree, NodeId};
use video::{
    CanonicalSubmitError, VideoConsumerSessionResource, VideoMode, VideoRegistry,
    VideoTargetResource, VideoWake,
};

type LayoutFrame<'a> = (Binary<'a>, f32, f32, f32, f32);
type LayoutFrames<'a> = Vec<LayoutFrame<'a>>;

/// Bump whenever the public `EmergeSkia.stats/2` payload shape changes.
const STATS_SCHEMA_VERSION: u64 = 19;

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
    rendering_api: Option<RenderingApiInfoNif>,
    window: StatsWindowNif,
    frames: StatsFrameSnapshotNif,
    timings: StatsTimingSnapshotNif,
    drm: StatsDrmSnapshotNif,
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
    render_draw: DurationStatsNif,
    render_flush: DurationStatsNif,
    render_gpu_flush: DurationStatsNif,
    render_submit: DurationStatsNif,
    present_submit: DurationStatsNif,
    pipeline: DurationStatsNif,
    pipeline_submit_to_tree_start: DurationStatsNif,
    pipeline_tree: DurationStatsNif,
    pipeline_render_queue: DurationStatsNif,
    pipeline_submit_to_swap: DurationStatsNif,
    pipeline_swap_to_frame_callback: DurationStatsNif,
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
struct StatsDrmSnapshotNif {
    forced_gpu_finish_before_swap: DurationStatsNif,
    forced_gpu_finish_after_swap: DurationStatsNif,
    gpu_queue_completion: DurationStatsNif,
    egl_swap_buffers: DurationStatsNif,
    gbm_lock_front_buffer: DurationStatsNif,
    framebuffer_lookup: DurationStatsNif,
    prepared_to_commit: DurationStatsNif,
    previous_flip_to_commit: DurationStatsNif,
    atomic_commit_ioctl: DurationStatsNif,
    commit_to_kernel_page_flip: DurationStatsNif,
    kernel_page_flip_interval: DurationStatsNif,
    page_flip_dispatch_delay: DurationStatsNif,
    commit_to_event_processed: DurationStatsNif,
    page_flip_events: u64,
    page_flip_sequence_steps: u64,
    missed_vblanks: u64,
}

#[derive(Clone, Debug, rustler::NifMap)]
struct StatsCounterSnapshotNif {
    layout_cache: LayoutCacheStatsNif,
    renderer_cache: RendererCacheStatsNif,
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

#[derive(Clone, Debug, rustler::NifMap)]
struct RendererCacheStatsNif {
    enabled: bool,
    disabled_reason: Option<String>,
    paint_layer: RendererCachePaintLayerStatsNif,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct RendererCachePaintLayerStatsNif {
    candidates: u64,
    visible_candidates: u64,
    suppressed_by_parent: u64,
    admitted: u64,
    hits: u64,
    misses: u64,
    stores: u64,
    evictions: u64,
    stale_evictions: u64,
    rejected: u64,
    current_entries: u64,
    current_bytes: u64,
    current_gpu_payloads: u64,
    current_cpu_payloads: u64,
    evicted_bytes: u64,
    stale_evicted_bytes: u64,
    gpu_payload_stores: u64,
    cpu_payload_stores: u64,
    prepare_successes: u64,
    prepare_failures: u64,
    direct_fallbacks_after_admission: u64,
    rejected_ineligible: u64,
    rejected_admission: u64,
    rejected_oversized: u64,
    rejected_payload_budget: u64,
    rejected_fractional_placement: u64,
    rejected_unsupported_transform: u64,
    prepare: DurationStatsNif,
    draw_hit: DurationStatsNif,
}

impl StatsSnapshotNif {
    fn from_snapshot(
        kind: &'static str,
        enabled: bool,
        reset_on_read: bool,
        rendering_api: Option<RenderingApiInfoNif>,
        renderer_cache_status: RendererCacheStatus,
        snapshot: &RendererStatsSnapshot,
    ) -> Self {
        let timing = |metric| DurationStatsNif::from(*snapshot.timing(metric));

        Self {
            version: STATS_SCHEMA_VERSION,
            kind: kind.to_string(),
            enabled,
            rendering_api,
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
                render: timing(RendererTimingMetric::Render),
                render_draw: timing(RendererTimingMetric::RenderDraw),
                render_flush: timing(RendererTimingMetric::RenderFlush),
                render_gpu_flush: timing(RendererTimingMetric::RenderGpuFlush),
                render_submit: timing(RendererTimingMetric::RenderSubmit),
                present_submit: timing(RendererTimingMetric::PresentSubmit),
                pipeline: timing(RendererTimingMetric::Pipeline),
                pipeline_submit_to_tree_start: timing(
                    RendererTimingMetric::PipelineSubmitToTreeStart,
                ),
                pipeline_tree: timing(RendererTimingMetric::PipelineTree),
                pipeline_render_queue: timing(RendererTimingMetric::PipelineRenderQueue),
                pipeline_submit_to_swap: timing(RendererTimingMetric::PipelineSubmitToSwap),
                pipeline_swap_to_frame_callback: timing(
                    RendererTimingMetric::PipelineSwapToFrameCallback,
                ),
                layout: timing(RendererTimingMetric::Layout),
                refresh: timing(RendererTimingMetric::Refresh),
                event_resolve: timing(RendererTimingMetric::EventResolve),
                patch_tree_process: timing(RendererTimingMetric::PatchTreeProcess),
            },
            drm: StatsDrmSnapshotNif {
                forced_gpu_finish_before_swap: timing(
                    RendererTimingMetric::DrmForcedGpuFinishBeforeSwap,
                ),
                forced_gpu_finish_after_swap: timing(
                    RendererTimingMetric::DrmForcedGpuFinishAfterSwap,
                ),
                gpu_queue_completion: timing(RendererTimingMetric::DrmGpuQueueCompletion),
                egl_swap_buffers: timing(RendererTimingMetric::DrmEglSwapBuffers),
                gbm_lock_front_buffer: timing(RendererTimingMetric::DrmGbmLockFrontBuffer),
                framebuffer_lookup: timing(RendererTimingMetric::DrmFramebufferLookup),
                prepared_to_commit: timing(RendererTimingMetric::DrmPreparedToCommit),
                previous_flip_to_commit: timing(RendererTimingMetric::DrmPreviousFlipToCommit),
                atomic_commit_ioctl: timing(RendererTimingMetric::DrmAtomicCommitIoctl),
                commit_to_kernel_page_flip: timing(RendererTimingMetric::DrmCommitToKernelPageFlip),
                kernel_page_flip_interval: timing(RendererTimingMetric::DrmKernelPageFlipInterval),
                page_flip_dispatch_delay: timing(RendererTimingMetric::DrmPageFlipDispatchDelay),
                commit_to_event_processed: timing(RendererTimingMetric::DrmCommitToPageFlip),
                page_flip_events: snapshot.video_pipeline.page_flip_events,
                page_flip_sequence_steps: snapshot.video_pipeline.page_flip_sequence_steps,
                missed_vblanks: snapshot.video_pipeline.missed_vblanks,
            },
            counters: StatsCounterSnapshotNif {
                layout_cache: LayoutCacheStatsNif::from(snapshot.layout_cache),
                renderer_cache: RendererCacheStatsNif::from_snapshot(
                    snapshot.renderer_cache.clone(),
                    renderer_cache_status,
                ),
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

impl RendererCacheStatsNif {
    fn from_snapshot(
        stats: stats::RendererCacheStatsSnapshot,
        status: RendererCacheStatus,
    ) -> Self {
        Self {
            enabled: status.enabled,
            disabled_reason: status.disabled_reason.map(ToString::to_string),
            paint_layer: RendererCachePaintLayerStatsNif::from(stats.paint_layer),
        }
    }
}

impl From<stats::RendererCachePaintLayerStatsSnapshot> for RendererCachePaintLayerStatsNif {
    fn from(stats: stats::RendererCachePaintLayerStatsSnapshot) -> Self {
        Self {
            candidates: stats.candidates,
            visible_candidates: stats.visible_candidates,
            suppressed_by_parent: stats.suppressed_by_parent,
            admitted: stats.admitted,
            hits: stats.hits,
            misses: stats.misses,
            stores: stats.stores,
            evictions: stats.evictions,
            stale_evictions: stats.stale_evictions,
            rejected: stats.rejected,
            current_entries: stats.current_entries,
            current_bytes: stats.current_bytes,
            current_gpu_payloads: stats.current_gpu_payloads,
            current_cpu_payloads: stats.current_cpu_payloads,
            evicted_bytes: stats.evicted_bytes,
            stale_evicted_bytes: stats.stale_evicted_bytes,
            gpu_payload_stores: stats.gpu_payload_stores,
            cpu_payload_stores: stats.cpu_payload_stores,
            prepare_successes: stats.prepare_successes,
            prepare_failures: stats.prepare_failures,
            direct_fallbacks_after_admission: stats.direct_fallbacks_after_admission,
            rejected_ineligible: stats.rejected_ineligible,
            rejected_admission: stats.rejected_admission,
            rejected_oversized: stats.rejected_oversized,
            rejected_payload_budget: stats.rejected_payload_budget,
            rejected_fractional_placement: stats.rejected_fractional_placement,
            rejected_unsupported_transform: stats.rejected_unsupported_transform,
            prepare: DurationStatsNif::from(stats.prepare),
            draw_hit: DurationStatsNif::from(stats.draw_hit),
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
        emerge_skia_frame,
        caller_owned,
        transferred,
        released,
        per_buffer,
        implicit,
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
    Headless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderingApi {
    Auto,
    OpenGl,
    Raster,
    Metal,
    Vulkan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RasterPresentKind {
    Auto,
    GpuUpload,
    Cpu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RenderingApiConfig {
    kind: RenderingApi,
    raster_present: RasterPresentKind,
    raster_present_configured: bool,
}

impl Default for RenderingApiConfig {
    fn default() -> Self {
        Self {
            kind: RenderingApi::Auto,
            raster_present: RasterPresentKind::Auto,
            raster_present_configured: false,
        }
    }
}

#[derive(Clone, Debug, rustler::NifMap)]
struct RenderingApiInfoNif {
    requested: String,
    selected: String,
}

#[derive(Clone, Debug, rustler::NifMap)]
struct RendererCapabilitiesNif {
    gpu: bool,
    renderer_cache: bool,
    screenshot: bool,
    raster_present: Vec<String>,
    prime_video: bool,
}

#[derive(Clone, Debug, rustler::NifMap)]
struct RendererInfoNif {
    backend: String,
    rendering_api: RenderingApiInfoNif,
    capabilities: RendererCapabilitiesNif,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RendererCacheStatus {
    enabled: bool,
    disabled_reason: Option<&'static str>,
}

impl RendererCacheStatus {
    fn enabled() -> Self {
        Self {
            enabled: true,
            disabled_reason: None,
        }
    }

    fn disabled(reason: &'static str) -> Self {
        Self {
            enabled: false,
            disabled_reason: Some(reason),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RendererRuntimeInfo {
    backend: BackendKind,
    requested_rendering_api: RenderingApi,
    selected_rendering_api: RenderingApi,
    raster_present: RasterPresentKind,
    renderer_cache: RendererCacheStatus,
    screenshot_supported: bool,
    prime_video_supported: bool,
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
    latest_frame: Arc<LatestFrameStore>,
    info: RendererRuntimeInfo,
    close_signal_log: bool,
    log_render: bool,
    log_input: bool,
    cleanup_dispatcher: CleanupDispatcher,
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
                let mut msg = msg;
                if let Ok(dropped) = self.drop_rx.try_recv() {
                    msg.absorb_pipeline_submitted_at(&dropped);
                }
                let _ = self.tx.try_send(msg);
                if self.log_render {
                    eprintln!("render queue overwrite");
                }
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LatestFrameSnapshot {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub sequence: u64,
    pub pixels: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct LatestFrameStore {
    sequence: AtomicU64,
    frame: Mutex<Option<LatestFrameSnapshot>>,
}

impl LatestFrameStore {
    pub(crate) fn publish_rgba(&self, width: u32, height: u32, scale: f32, pixels: Vec<u8>) {
        let sequence = self
            .sequence
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        let mut guard = self
            .frame
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(LatestFrameSnapshot {
            width,
            height,
            scale,
            sequence,
            pixels,
        });
    }

    fn latest(&self) -> Option<LatestFrameSnapshot> {
        self.frame
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
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

type CleanupTask = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
pub(crate) struct CleanupDispatcher {
    primary: CleanupSender<CleanupTask>,
    fallback: CleanupSender<CleanupTask>,
}

impl CleanupDispatcher {
    pub(crate) fn start() -> Result<Self, String> {
        let primary = Self::start_worker("emerge_skia_cleanup")?;
        let fallback = Self::start_worker("emerge_skia_cleanup_fallback")?;
        Ok(Self { primary, fallback })
    }

    fn start_worker(name: &str) -> Result<CleanupSender<CleanupTask>, String> {
        let (sender, receiver) = cleanup_channel::<CleanupTask>();
        thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                while let Ok(task) = receiver.recv() {
                    if catch_unwind(AssertUnwindSafe(task)).is_err() {
                        eprintln!("EmergeSkia native cleanup task panicked; aborting");
                        std::process::abort();
                    }
                }
            })
            .map_err(|error| format!("failed to start {name}: {error}"))?;
        Ok(sender)
    }

    pub(crate) fn dispatch(&self, task: CleanupTask) {
        let task = match self.primary.send(task) {
            Ok(()) => return,
            Err(error) => error.0,
        };

        if self.fallback.send(task).is_err() {
            eprintln!("both EmergeSkia native cleanup workers stopped; aborting");
            std::process::abort();
        }
    }

    #[cfg(test)]
    fn from_senders(
        primary: CleanupSender<CleanupTask>,
        fallback: CleanupSender<CleanupTask>,
    ) -> Self {
        Self { primary, fallback }
    }
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
    cleanup_dispatcher: CleanupDispatcher,
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
        self.video_registry.close_admission();
        let registry = Arc::clone(&self.video_registry);
        let wake = self.video_wake.clone();
        let shutdown = self.take_shutdown_for_drop();

        self.cleanup_dispatcher.dispatch(Box::new(move || {
            registry.close();
            wake.notify();
            if let Some((ctx, handles)) = shutdown
                && let Err(error) = shutdown_renderer_runtime(ctx, handles)
            {
                eprintln!("renderer drop shutdown failed: {error}");
            }
        }));
    }
}

impl Drop for TestHarnessResource {
    fn drop(&mut self) {
        let handles = match self.handles.get_mut() {
            Ok(handles) => handles,
            Err(poisoned) => poisoned.into_inner(),
        }
        .take();
        let Some(handles) = handles else {
            return;
        };
        let tree_tx = self.tree_tx.clone();
        let event_tx = self.event_tx.clone();

        self.cleanup_dispatcher.dispatch(Box::new(move || {
            stop_test_harness_runtime(tree_tx, event_tx, handles);
        }));
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
        stop_test_harness_runtime(self.tree_tx.clone(), self.event_tx.clone(), handles);
    }
}

fn stop_test_harness_runtime(
    tree_tx: Sender<TreeMsg>,
    event_tx: Sender<EventMsg>,
    handles: TestHarnessHandles,
) {
    send_event(&event_tx, EventMsg::Stop, false);
    send_tree(&tree_tx, TreeMsg::Stop, false);

    let _ = handles.proxy_handle.join();
    let _ = handles.event_handle.join();
    let _ = handles.tree_handle.join();
    assets::stop();
    clear_global_caches();
    trim_process_allocator();
}

impl RendererResource {
    fn stop(&self) -> Result<(), String> {
        self.video_registry.close();
        self.video_wake.notify();
        self.stop_inner()
    }

    fn stop_inner(&self) -> Result<(), String> {
        let mut handles_guard = match self.handles.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let Some(handles) = handles_guard.take() else {
            return Ok(());
        };

        drop(handles_guard);
        shutdown_renderer_runtime(self.shutdown_context(), handles)
    }

    fn shutdown_context(&self) -> ShutdownRuntimeContext {
        ShutdownRuntimeContext {
            running_flag: Arc::clone(&self.running_flag),
            backend_wake: self.backend_wake.clone(),
            stop_flag: Arc::clone(&self.stop_flag),
            tree_tx: self.tree_tx.clone(),
            event_tx: self.event_tx.clone(),
            render_tx: self.render_tx.clone(),
            close_signal_log: self.close_signal_log,
            log_render: self.log_render,
            log_input: self.log_input,
        }
    }

    fn take_shutdown_for_drop(&mut self) -> Option<(ShutdownRuntimeContext, RendererHandles)> {
        let handles = match self.handles.get_mut() {
            Ok(handles) => handles,
            Err(poisoned) => poisoned.into_inner(),
        }
        .take()?;
        Some((self.shutdown_context(), handles))
    }
}

fn shutdown_renderer_runtime(
    ctx: ShutdownRuntimeContext,
    mut handles: RendererHandles,
) -> Result<(), String> {
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

    let mut join_failures = Vec::new();
    join_runtime_thread("event", handles.event_handle.take(), &mut join_failures);
    join_runtime_thread(
        "heartbeat",
        handles.heartbeat_handle.take(),
        &mut join_failures,
    );
    join_runtime_thread("tree", handles.tree_handle.take(), &mut join_failures);
    join_runtime_thread("input", handles.input_handle.take(), &mut join_failures);
    join_runtime_thread("backend", handles.backend_handle.take(), &mut join_failures);

    log_close_signal(close_signal_log, "nif_close", "shutdown end");
    clear_global_caches();
    trim_process_allocator();

    if join_failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "renderer thread join failures: {}",
            join_failures.join(", ")
        ))
    }
}

fn join_runtime_thread(
    name: &'static str,
    handle: Option<thread::JoinHandle<()>>,
    failures: &mut Vec<String>,
) {
    if let Some(handle) = handle
        && let Err(panic) = handle.join()
    {
        let detail = panic
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string());
        failures.push(format!("{name}: {detail}"));
    }
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
fn backend_stats_label(backend: BackendKind) -> &'static str {
    backend.as_str()
}

impl BackendKind {
    fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "macos")]
            BackendKind::Macos => "macos",
            #[cfg(all(feature = "wayland", target_os = "linux"))]
            BackendKind::Wayland => "wayland",
            #[cfg(all(feature = "drm", target_os = "linux"))]
            BackendKind::Drm => "drm",
            BackendKind::Headless => "headless",
        }
    }
}

impl RenderingApi {
    fn as_str(self) -> &'static str {
        match self {
            RenderingApi::Auto => "auto",
            RenderingApi::OpenGl => "opengl",
            RenderingApi::Raster => "raster",
            RenderingApi::Metal => "metal",
            RenderingApi::Vulkan => "vulkan",
        }
    }

    fn has_gpu(self) -> bool {
        matches!(
            self,
            RenderingApi::OpenGl | RenderingApi::Metal | RenderingApi::Vulkan
        )
    }
}

impl RasterPresentKind {
    fn as_str(self) -> &'static str {
        match self {
            RasterPresentKind::Auto => "auto",
            RasterPresentKind::GpuUpload => "gpu_upload",
            RasterPresentKind::Cpu => "cpu",
        }
    }
}

impl RendererRuntimeInfo {
    fn rendering_api_nif(self) -> RenderingApiInfoNif {
        RenderingApiInfoNif {
            requested: self.requested_rendering_api.as_str().to_string(),
            selected: self.selected_rendering_api.as_str().to_string(),
        }
    }

    fn renderer_label(self) -> String {
        format!(
            "{} ({})",
            self.requested_rendering_api.as_str(),
            self.selected_rendering_api.as_str()
        )
    }

    fn to_nif(self) -> RendererInfoNif {
        let _requested_raster_present = self.raster_present.as_str();

        RendererInfoNif {
            backend: self.backend.as_str().to_string(),
            rendering_api: self.rendering_api_nif(),
            capabilities: RendererCapabilitiesNif {
                gpu: self.selected_rendering_api.has_gpu(),
                renderer_cache: self.renderer_cache.enabled,
                screenshot: self.screenshot_supported,
                raster_present: raster_present_capabilities(self.backend)
                    .into_iter()
                    .map(ToString::to_string)
                    .collect(),
                prime_video: self.prime_video_supported,
            },
        }
    }
}

fn raster_present_capabilities(backend: BackendKind) -> Vec<&'static str> {
    match backend {
        #[cfg(feature = "macos")]
        BackendKind::Macos => Vec::new(),
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        BackendKind::Wayland => vec!["gpu_upload", "cpu"],
        #[cfg(all(feature = "drm", target_os = "linux"))]
        BackendKind::Drm => vec!["gpu_upload"],
        BackendKind::Headless => Vec::new(),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn selected_rendering_api_for_config(config: RenderingApiConfig) -> RenderingApi {
    match config.kind {
        RenderingApi::Auto => RenderingApi::OpenGl,
        explicit => explicit,
    }
}

fn renderer_cache_status(
    selected_rendering_api: RenderingApi,
    config: RendererCacheConfig,
) -> RendererCacheStatus {
    if config.enabled {
        RendererCacheStatus::enabled()
    } else if matches!(selected_rendering_api, RenderingApi::Raster) {
        RendererCacheStatus::disabled("raster_renderer")
    } else {
        RendererCacheStatus::disabled("configured_disabled")
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
    rendering_api_label: String,
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
                        stats::format_renderer_stats_log(
                            backend_label,
                            &rendering_api_label,
                            &stats.snapshot(),
                        ),
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
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux"),
            feature = "macos"
        )),
        allow(dead_code)
    )]
    rendering_api: RenderingApiConfig,
    requested_rendering_api: RenderingApi,
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
    drm_force_gpu_finish: bool,
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
    #[cfg_attr(
        not(any(all(feature = "wayland", target_os = "linux"),)),
        allow(dead_code)
    )]
    renderer_animation_log: bool,
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux"),
        )),
        allow(dead_code)
    )]
    renderer_cache_config: RendererCacheConfig,
    renderer_cache_enabled_configured: bool,
    headless: HeadlessConfig,
}

#[derive(Clone)]
struct HeadlessConfig {
    target: Option<LocalPid>,
    mode: String,
    pixel_format: String,
    bw1_polarity: String,
    target_fps: Option<u32>,
    frame_message: String,
    prime: HeadlessPrimeConfig,
}

#[derive(Clone, Debug)]
struct HeadlessPrimeConfig {
    max_in_flight: u32,
    on_backpressure: String,
}

impl std::fmt::Debug for HeadlessConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessConfig")
            .field("target", &self.target.is_some())
            .field("mode", &self.mode)
            .field("pixel_format", &self.pixel_format)
            .field("bw1_polarity", &self.bw1_polarity)
            .field("target_fps", &self.target_fps)
            .field("frame_message", &self.frame_message)
            .field("prime", &self.prime)
            .finish()
    }
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            target: None,
            mode: "binary".to_string(),
            pixel_format: "rgba8888".to_string(),
            bw1_polarity: "one_is_black".to_string(),
            target_fps: None,
            frame_message: "emerge_skia_frame".to_string(),
            prime: HeadlessPrimeConfig {
                max_in_flight: 2,
                on_backpressure: "drop_new".to_string(),
            },
        }
    }
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
    rendering_api: RenderingApiConfigNif,
    headless: HeadlessConfigNif,
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
    drm_force_gpu_finish: bool,
    hw_cursor: bool,
    input_log: bool,
    render_log: bool,
    close_signal_log: bool,
    stats_enabled: bool,
    renderer_stats_log: bool,
    renderer_animation_log: bool,
    renderer_cache: RendererCacheConfigNif,
}

#[derive(Clone, Debug, rustler::NifMap)]
struct RenderingApiConfigNif {
    kind: String,
    raster_present: String,
    raster_present_configured: bool,
}

#[derive(Clone, rustler::NifMap)]
struct HeadlessConfigNif {
    target: Option<LocalPid>,
    mode: String,
    pixel_format: String,
    bw1_polarity: String,
    target_fps: Option<u32>,
    frame_message: String,
    prime: HeadlessPrimeConfigNif,
}

#[derive(Clone, rustler::NifMap)]
struct HeadlessPrimeConfigNif {
    max_in_flight: u32,
    on_backpressure: String,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct RendererCacheConfigNif {
    enabled: bool,
    enabled_configured: bool,
    max_new_payloads_per_frame: u32,
    paint_layer: RendererPaintLayerCacheConfigNif,
}

#[derive(Clone, Copy, Debug, rustler::NifMap)]
struct RendererPaintLayerCacheConfigNif {
    max_entries: u64,
    max_bytes: u64,
    max_entry_bytes: u64,
    min_visible_before_store: u64,
    max_stale_frames: u64,
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

#[derive(Clone, Debug, rustler::NifMap)]
struct ScreenshotOptsNif {
    pixel_format: String,
    scale: f32,
    region_x: Option<u32>,
    region_y: Option<u32>,
    region_width: Option<u32>,
    region_height: Option<u32>,
    timeout_ms: u64,
    background: String,
    png_compression: String,
}

fn start_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    ensure_rendering_api_supported(config.backend, config.rendering_api)
        .map_err(|reason| rustler::Error::Term(Box::new(reason)))?;

    if matches!(config.backend, BackendKind::Headless) {
        return backend::headless::start_renderer_with_config(config, initial_log_target);
    }

    #[cfg(feature = "macos")]
    if matches!(config.backend, BackendKind::Macos) {
        return Err(rustler::Error::Term(Box::new(
            "macOS uses the external host path; start it through EmergeSkia.start/1 with backend: :macos"
                .to_string(),
        )));
    }

    start_native_renderer_with_config(config, initial_log_target)
}

fn renderer_cache_config_from_nif(
    config: RendererCacheConfigNif,
) -> Result<RendererCacheConfig, String> {
    let max_entries = usize::try_from(config.paint_layer.max_entries).map_err(|_| {
        "renderer_cache.paint_layer.max_entries does not fit this backend".to_string()
    })?;

    Ok(RendererCacheConfig {
        enabled: config.enabled,
        max_new_payloads_per_frame: config.max_new_payloads_per_frame,
        paint_layer: RendererPaintLayerCacheConfig {
            max_entries,
            max_bytes: config.paint_layer.max_bytes,
            max_entry_bytes: config.paint_layer.max_entry_bytes,
            min_visible_before_store: config.paint_layer.min_visible_before_store,
            max_stale_frames: config.paint_layer.max_stale_frames,
        },
    })
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn auto_raster_fallback_config(config: &StartConfig) -> Option<StartConfig> {
    if !matches!(config.rendering_api.kind, RenderingApi::Auto) {
        return None;
    }

    let mut fallback = config.clone();
    fallback.rendering_api.kind = RenderingApi::Raster;
    if !fallback.renderer_cache_enabled_configured {
        fallback.renderer_cache_config.enabled = false;
    }
    Some(fallback)
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn start_auto_raster_fallback_or_error(
    fallback: Option<StartConfig>,
    initial_log_target: Option<LocalPid>,
    reason: String,
) -> NifResult<ResourceArc<RendererResource>> {
    match fallback {
        Some(config) => {
            eprintln!("OpenGL backend startup failed; falling back to raster: {reason}");
            start_native_renderer_with_config(config, initial_log_target)
        }
        None => Err(rustler::Error::Term(Box::new(reason))),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn start_native_renderer_with_config(
    config: StartConfig,
    initial_log_target: Option<LocalPid>,
) -> NifResult<ResourceArc<RendererResource>> {
    let auto_raster_fallback = auto_raster_fallback_config(&config);
    let fallback_log_target = initial_log_target;
    let running_flag = Arc::new(AtomicBool::new(true));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let render_counter = Arc::new(AtomicU64::new(0));
    let input_target = Arc::new(InputTargetRelay::new(None));
    let native_log = Arc::new(NativeLogRelay::new(initial_log_target));
    let latest_frame = Arc::new(LatestFrameStore::default());

    #[cfg(all(feature = "drm", target_os = "linux"))]
    let log_input = matches!(config.backend, BackendKind::Drm) && config.drm_input_log;
    #[cfg(not(all(feature = "drm", target_os = "linux")))]
    let log_input = false;
    let log_render = config.render_log;
    let close_signal_log = config.close_signal_log;
    let renderer_stats = (config.stats_enabled || config.renderer_stats_log)
        .then(|| Arc::new(RendererStatsCollector::new()));
    let backend_label = backend_stats_label(config.backend);
    let selected_rendering_api = selected_rendering_api_for_config(config.rendering_api);
    let renderer_cache =
        renderer_cache_status(selected_rendering_api, config.renderer_cache_config);
    let rendering_api_label = RendererRuntimeInfo {
        backend: config.backend,
        requested_rendering_api: config.requested_rendering_api,
        selected_rendering_api,
        raster_present: config.rendering_api.raster_present,
        renderer_cache,
        screenshot_supported: true,
        prime_video_supported: false,
    }
    .renderer_label();
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
    let release_tx = video::spawn_release_worker()
        .map_err(|error| rustler::Error::Term(Box::new(error.to_string())))?;
    let cleanup_dispatcher =
        CleanupDispatcher::start().map_err(|error| rustler::Error::Term(Box::new(error)))?;

    let mut handles = RendererHandles {
        heartbeat_handle: Some(spawn_running_heartbeat(
            Arc::clone(&running_flag),
            Arc::clone(&input_target),
            Arc::clone(&native_log),
            heartbeat_stats,
            backend_label,
            rendering_api_label,
        )),
        ..RendererHandles::default()
    };

    let initial_width = config.width;
    let initial_height = config.height;
    let video_registry = Arc::new(VideoRegistry::new(
        release_tx,
        cleanup_dispatcher.clone(),
        renderer_stats.clone(),
    ));
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
            let native_log_clone = Arc::clone(&native_log);
            let renderer_stats_clone = renderer_stats.clone();
            let renderer_stats_log = config.renderer_stats_log;
            let renderer_animation_log = config.renderer_animation_log;
            let renderer_cache_config = config.renderer_cache_config;
            let latest_frame_clone = Arc::clone(&latest_frame);
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
                    render_log: log_render,
                    stats: renderer_stats_clone,
                    renderer_stats_log,
                    renderer_animation_log,
                    rendering_api: selected_rendering_api,
                    raster_present: config.rendering_api.raster_present,
                    renderer_cache_config,
                    latest_frame: latest_frame_clone,
                    native_log: native_log_clone,
                    render_rx,
                    cursor_icon_rx: backend_cursor_rx,
                    video_registry: video_registry_clone,
                    proxy_tx,
                });
            }));

            let startup = match proxy_rx.recv() {
                Ok(Ok(startup)) => startup,
                Ok(Err(reason)) => {
                    let _ = shutdown_renderer_runtime(
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

                    return start_auto_raster_fallback_or_error(
                        auto_raster_fallback,
                        fallback_log_target,
                        reason,
                    );
                }
                Err(_) => {
                    let _ = shutdown_renderer_runtime(
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

                    return start_auto_raster_fallback_or_error(
                        auto_raster_fallback,
                        fallback_log_target,
                        "failed to receive backend startup info".to_string(),
                    );
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
            let latest_frame_for_backend = Arc::clone(&latest_frame);
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
                force_gpu_finish: config.drm_force_gpu_finish,
                hw_cursor: config.drm_hw_cursor,
                render_log: log_render,
                renderer_stats_log: config.renderer_stats_log,
                rendering_api: selected_rendering_api,
                raster_present: config.rendering_api.raster_present,
                renderer_cache_config: config.renderer_cache_config,
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
                        latest_frame: latest_frame_for_backend,
                        video_registry: video_registry_clone,
                    },
                    drm_config,
                );
            }));

            match startup_rx.recv() {
                Ok(Ok(())) => {}
                Ok(Err(reason)) => {
                    let _ = shutdown_renderer_runtime(
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

                    return start_auto_raster_fallback_or_error(
                        auto_raster_fallback,
                        fallback_log_target,
                        reason,
                    );
                }
                Err(_) => {
                    let _ = shutdown_renderer_runtime(
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

                    return start_auto_raster_fallback_or_error(
                        auto_raster_fallback,
                        fallback_log_target,
                        "failed to receive DRM backend startup info".to_string(),
                    );
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

            (
                BackendKind::Drm,
                matches!(selected_rendering_api, RenderingApi::OpenGl),
            )
        }
        #[cfg(feature = "macos")]
        BackendKind::Macos => unreachable!("macOS backend should return before runtime startup"),
        BackendKind::Headless => {
            unreachable!("headless backend should return before native startup")
        }
    };

    handles.event_handle = Some(spawn_event_actor(SpawnEventActorConfig {
        event_rx,
        tree_tx: tree_tx.clone(),
        backend_cursor_tx: Some(backend_cursor_tx),
        backend_wake: backend_wake.clone(),
        scroll_line_pixels: config.scroll_line_pixels,
        log_render,
        native_log: Arc::clone(&native_log),
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
        latest_frame,
        info: RendererRuntimeInfo {
            backend,
            requested_rendering_api: config.requested_rendering_api,
            selected_rendering_api,
            raster_present: config.rendering_api.raster_present,
            renderer_cache,
            screenshot_supported: true,
            prime_video_supported,
        },
        close_signal_log,
        log_render,
        log_input,
        cleanup_dispatcher,
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
                rendering_api: RenderingApiConfig::default(),
                requested_rendering_api: RenderingApi::Auto,
                title,
                width,
                height,
                scroll_line_pixels: input::SCROLL_LINE_PIXELS,
                asset_config: AssetConfig::default(),
                drm_card: None,
                drm_startup_retries: 40,
                drm_retry_interval_ms: 250,
                drm_force_gpu_finish: false,
                drm_hw_cursor: true,
                drm_cursor_overrides: Vec::new(),
                drm_input_log: false,
                render_log: false,
                close_signal_log: false,
                stats_enabled: false,
                renderer_stats_log: false,
                renderer_animation_log: false,
                renderer_cache_config: RendererCacheConfig::default(),
                renderer_cache_enabled_configured: false,
                headless: HeadlessConfig::default(),
            },
            Some(env.pid()),
        )
    }
    #[cfg(not(all(feature = "wayland", target_os = "linux")))]
    {
        let _ = (env, title, width, height);
        Err(rustler::Error::Term(Box::new(
            "Wayland backend is not compiled; add :wayland to config :emerge, compiled_backends: [...]"
                .to_string(),
        )))
    }
}

#[rustler::nif(schedule = "DirtyIo")]
fn start_opts(env: Env, opts: StartOptsNif) -> NifResult<ResourceArc<RendererResource>> {
    let backend = opts.backend.to_lowercase();
    let backend =
        parse_backend_name(&backend).map_err(|reason| rustler::Error::Term(Box::new(reason)))?;
    let rendering_api = parse_rendering_api_config(opts.rendering_api)
        .map_err(|reason| rustler::Error::Term(Box::new(reason)))?;
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
    let renderer_cache_enabled_configured = opts.renderer_cache.enabled_configured;
    let renderer_cache_config = renderer_cache_config_from_nif(opts.renderer_cache)
        .map_err(|reason| rustler::Error::Term(Box::new(reason)))?;

    start_with_config(
        StartConfig {
            backend,
            rendering_api,
            requested_rendering_api: rendering_api.kind,
            title: opts.title,
            width: opts.width,
            height: opts.height,
            scroll_line_pixels: opts.scroll_line_pixels,
            asset_config,
            drm_card: opts.drm_card,
            drm_startup_retries: opts.drm_startup_retries,
            drm_retry_interval_ms: opts.drm_retry_interval_ms,
            drm_force_gpu_finish: opts.drm_force_gpu_finish,
            drm_hw_cursor: opts.hw_cursor,
            drm_cursor_overrides,
            drm_input_log: opts.input_log,
            render_log: opts.render_log,
            close_signal_log: opts.close_signal_log,
            stats_enabled: opts.stats_enabled,
            renderer_stats_log: opts.renderer_stats_log,
            renderer_animation_log: opts.renderer_animation_log,
            renderer_cache_config,
            renderer_cache_enabled_configured,
            headless: HeadlessConfig {
                target: opts.headless.target,
                mode: opts.headless.mode,
                pixel_format: opts.headless.pixel_format,
                bw1_polarity: opts.headless.bw1_polarity,
                target_fps: opts.headless.target_fps,
                frame_message: opts.headless.frame_message,
                prime: HeadlessPrimeConfig {
                    max_in_flight: opts.headless.prime.max_in_flight,
                    on_backpressure: opts.headless.prime.on_backpressure,
                },
            },
        },
        Some(env.pid()),
    )
}

#[rustler::nif(schedule = "DirtyIo")]
fn stop(renderer: ResourceArc<RendererResource>) -> Result<Atom, String> {
    renderer.stop()?;
    Ok(atoms::ok())
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

#[rustler::nif(schedule = "DirtyCpu")]
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
    let incarnation = renderer.video_registry.create_target(spec)?;

    Ok(ResourceArc::new(VideoTargetResource {
        id,
        renderer_epoch: renderer.video_registry.renderer_epoch,
        incarnation,
        _width: width,
        _height: height,
        _mode: mode,
        registry: Arc::clone(&renderer.video_registry),
        wake: renderer.video_wake.clone(),
        cleanup_dispatcher: renderer.cleanup_dispatcher.clone(),
    }))
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_target_submit_prime(
    target: ResourceArc<VideoTargetResource>,
    desc: video::PrimeDesc,
) -> Result<bool, String> {
    let spec = target.registry.target_spec(&target.id)?;
    desc.validate_for_target(&target.id, spec.mode, spec.width, spec.height)?;
    let submitted =
        target
            .registry
            .submit_prime_exact(&target.id, target.incarnation, desc.into())?;
    if matches!(submitted, video::VideoSubmitResult::Queued) {
        target.wake.notify();
        Ok(true)
    } else {
        Ok(false)
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_consumer_session_open(
    target: ResourceArc<VideoTargetResource>,
    width: u32,
    height: u32,
    fourcc: u32,
    modifier: Term<'_>,
) -> Result<ResourceArc<VideoConsumerSessionResource>, String> {
    if target.renderer_epoch != target.registry.renderer_epoch {
        return Err("stale video renderer epoch".to_string());
    }
    let modifier_policy = decode_stream_modifier_policy(modifier)?;
    let stream_id = target.registry.open_stream(
        &target.id,
        target.incarnation,
        width,
        height,
        fourcc,
        modifier_policy,
    )?;
    target.wake.notify();
    Ok(ResourceArc::new(VideoConsumerSessionResource::new(
        &target, stream_id,
    )))
}

fn decode_stream_modifier_policy(term: Term<'_>) -> Result<video::StreamModifierPolicy, String> {
    if let Ok(atom) = term.decode::<Atom>() {
        if atom == atoms::per_buffer() {
            return Ok(video::StreamModifierPolicy::PerBuffer);
        }
        if atom == atoms::implicit() {
            return Ok(video::StreamModifierPolicy::Implicit);
        }
        return Err("unsupported video stream modifier policy".to_string());
    }

    term.decode::<u64>()
        .map(video::StreamModifierPolicy::Explicit)
        .map_err(|_| {
            "video stream modifier policy must be :per_buffer, :implicit, or u64".to_string()
        })
}

fn decode_video_frame(term: Term<'_>) -> Result<video_interop::Frame<'_>, (Atom, String)> {
    video_interop::Frame::decode(term).map_err(|error| {
        (
            atoms::caller_owned(),
            format!("invalid VideoInterop.Frame: {error:?}"),
        )
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_consumer_session_submit(
    session: ResourceArc<VideoConsumerSessionResource>,
    frame: Term<'_>,
) -> Result<Atom, (Atom, String)> {
    let frame = decode_video_frame(frame)?;
    if session.renderer_epoch != session.registry.renderer_epoch {
        return Err((
            atoms::caller_owned(),
            "stale video renderer epoch".to_string(),
        ));
    }
    if session.is_closed() {
        return Err((
            atoms::caller_owned(),
            "video consumer stream is closed".to_string(),
        ));
    }
    let prepared = frame
        .prepare_cloexec()
        .map_err(|error| (atoms::caller_owned(), error.to_string()))?;
    match session.registry.submit_canonical(
        &session.id,
        session.incarnation,
        session.stream_id,
        prepared,
    ) {
        Ok(video::VideoSubmitResult::Queued) => {
            session.wake.notify();
            Ok(atoms::transferred())
        }
        Ok(video::VideoSubmitResult::DroppedInactive) => Ok(atoms::released()),
        Err(CanonicalSubmitError::CallerOwned(reason)) => Err((atoms::caller_owned(), reason)),
        Err(CanonicalSubmitError::Transferred(reason)) => Err((atoms::transferred(), reason)),
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_consumer_decode_for_test(frame: Term<'_>) -> Result<Atom, (Atom, String)> {
    let _frame = decode_video_frame(frame)?;
    Ok(atoms::caller_owned())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn video_consumer_session_close(session: ResourceArc<VideoConsumerSessionResource>) -> Atom {
    session.close();
    atoms::ok()
}

#[rustler::nif]
fn headless_prime_release_backend_token(
    backend_token: ResourceArc<backend::headless::HeadlessPrimeBackendToken>,
) -> Atom {
    backend::headless::release_backend_token(backend_token);
    atoms::ok()
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_upload(renderer: ResourceArc<RendererResource>, data: Binary) -> Atom {
    let submitted_at = Instant::now();
    let bytes = data.as_slice().to_vec();
    send_tree(
        &renderer.tree_tx,
        TreeMsg::UploadTree {
            bytes,
            submitted_at: Some(submitted_at),
        },
        renderer.log_render,
    );
    atoms::ok()
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_patch(renderer: ResourceArc<RendererResource>, data: Binary) -> Atom {
    let submitted_at = Instant::now();
    let bytes = data.as_slice().to_vec();
    send_tree(
        &renderer.tree_tx,
        TreeMsg::PatchTree {
            bytes,
            submitted_at: Some(submitted_at),
        },
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

#[rustler::nif]
fn renderer_info(renderer: ResourceArc<RendererResource>) -> Result<RendererInfoNif, String> {
    Ok(renderer.info.to_nif())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_capture_pixels<'a>(
    env: Env<'a>,
    renderer: ResourceArc<RendererResource>,
    opts: ScreenshotOptsNif,
) -> Result<Binary<'a>, String> {
    if !renderer.info.screenshot_supported {
        return Err("screenshot capture is not supported for headless PRIME output".to_string());
    }
    let capture = capture_latest_frame(renderer.latest_frame.latest(), &opts)?;
    let pixels = convert_screenshot_pixels(&capture, &opts.pixel_format)?;
    let mut binary = NewBinary::new(env, pixels.len());
    binary.as_mut_slice().copy_from_slice(&pixels);
    Ok(binary.into())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_capture_png<'a>(
    env: Env<'a>,
    renderer: ResourceArc<RendererResource>,
    opts: ScreenshotOptsNif,
) -> Result<Binary<'a>, String> {
    if !renderer.info.screenshot_supported {
        return Err("screenshot capture is not supported for headless PRIME output".to_string());
    }
    let capture = capture_latest_frame(renderer.latest_frame.latest(), &opts)?;
    let encoded = services::encode_rgba_png(capture.width, capture.height, &capture.pixels)?;
    let mut binary = NewBinary::new(env, encoded.len());
    binary.as_mut_slice().copy_from_slice(&encoded);
    Ok(binary.into())
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

    let snapshot = |enabled, reset_on_read, snapshot: RendererStatsSnapshot| {
        StatsSnapshotNif::from_snapshot(
            "renderer",
            enabled,
            reset_on_read,
            Some(renderer.info.rendering_api_nif()),
            renderer.info.renderer_cache,
            &snapshot,
        )
        .encode(env)
    };

    let Some(stats) = renderer.stats.as_ref() else {
        return Ok(snapshot(false, false, RendererStatsSnapshot::default()));
    };

    match command {
        StatsCommandNif::Peek => Ok(snapshot(true, false, stats.peek())),
        StatsCommandNif::Take => Ok(snapshot(true, true, stats.take())),
        StatsCommandNif::Reset => {
            stats.reset();
            Ok(snapshot(true, false, stats.peek()))
        }
        StatsCommandNif::Configure(_) => unreachable!(),
    }
}

fn capture_latest_frame(
    frame: Option<LatestFrameSnapshot>,
    opts: &ScreenshotOptsNif,
) -> Result<LatestFrameSnapshot, String> {
    let _timeout_ms = opts.timeout_ms;
    let _png_compression = opts.png_compression.as_str();

    if opts.scale != 1.0 {
        return Err("screenshot scale values other than 1.0 are not implemented yet".to_string());
    }

    if opts.background != "transparent" {
        return Err("screenshot background currently only supports :transparent".to_string());
    }

    let frame = frame.ok_or_else(|| "no presented frame is available yet".to_string())?;
    let _sequence = frame.sequence;
    let _frame_scale = frame.scale;

    crop_screenshot_frame(frame, opts)
}

fn crop_screenshot_frame(
    frame: LatestFrameSnapshot,
    opts: &ScreenshotOptsNif,
) -> Result<LatestFrameSnapshot, String> {
    let region = match (
        opts.region_x,
        opts.region_y,
        opts.region_width,
        opts.region_height,
    ) {
        (None, None, None, None) => return Ok(frame),
        (Some(x), Some(y), Some(width), Some(height)) => (x, y, width, height),
        _ => return Err("screenshot region must include x, y, width, and height".to_string()),
    };

    let (x, y, width, height) = region;
    if width == 0 || height == 0 {
        return Err("screenshot region width and height must be positive".to_string());
    }
    if x.checked_add(width).is_none_or(|right| right > frame.width)
        || y.checked_add(height)
            .is_none_or(|bottom| bottom > frame.height)
    {
        return Err("screenshot region is outside the latest frame".to_string());
    }

    let source_stride = usize::try_from(frame.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "latest frame dimensions are too large".to_string())?;
    let dest_stride = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "screenshot region dimensions are too large".to_string())?;
    let x_offset = usize::try_from(x)
        .ok()
        .and_then(|x| x.checked_mul(4))
        .ok_or_else(|| "screenshot region is too large".to_string())?;
    let start_row = usize::try_from(y).map_err(|_| "screenshot region is too large".to_string())?;
    let row_count =
        usize::try_from(height).map_err(|_| "screenshot region is too large".to_string())?;

    let pixels = (0..row_count)
        .flat_map(|row| {
            let start = (start_row + row) * source_stride + x_offset;
            let end = start + dest_stride;
            frame.pixels[start..end].to_vec()
        })
        .collect();

    Ok(LatestFrameSnapshot {
        width,
        height,
        pixels,
        ..frame
    })
}

fn convert_screenshot_pixels(
    capture: &LatestFrameSnapshot,
    pixel_format: &str,
) -> Result<Vec<u8>, String> {
    match pixel_format {
        "rgba8888" => Ok(capture.pixels.clone()),
        "rgb888" => Ok(capture
            .pixels
            .chunks_exact(4)
            .flat_map(|rgba| [rgba[0], rgba[1], rgba[2]])
            .collect()),
        other => Err(format!(
            "screenshot pixel_format {other} is not implemented yet; supported formats are :rgba8888 and :rgb888"
        )),
    }
}

fn tree_stats_snapshot<'a>(
    env: Env<'a>,
    tree_res: ResourceArc<TreeResource>,
    command: StatsCommandNif,
) -> Result<Term<'a>, String> {
    let tree_snapshot = |enabled, reset_on_read, snapshot: RendererStatsSnapshot| {
        StatsSnapshotNif::from_snapshot(
            "tree",
            enabled,
            reset_on_read,
            None,
            RendererCacheStatus::disabled("tree_resource"),
            &snapshot,
        )
        .encode(env)
    };

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
                Ok(tree_snapshot(true, false, stats.peek()))
            } else {
                Ok(tree_snapshot(
                    false,
                    false,
                    RendererStatsSnapshot::default(),
                ))
            }
        }
        StatsCommandNif::Peek | StatsCommandNif::Take | StatsCommandNif::Reset => {
            let stats = tree_res
                .stats
                .lock()
                .map_err(|_| "failed to lock tree stats".to_string())?
                .clone();

            let Some(stats) = stats else {
                return Ok(tree_snapshot(
                    false,
                    false,
                    RendererStatsSnapshot::default(),
                ));
            };

            match command {
                StatsCommandNif::Peek => Ok(tree_snapshot(true, false, stats.peek())),
                StatsCommandNif::Take => Ok(tree_snapshot(true, true, stats.take())),
                StatsCommandNif::Reset => {
                    stats.reset();
                    Ok(tree_snapshot(true, false, stats.peek()))
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
    let mut tree = tree_res.tree.lock().map_err(|_| tree_lock_error())?;
    tree::patch::apply_patches(&mut tree, patches)?;
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
        native_log: Arc::new(NativeLogRelay::default()),
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

    let cleanup_dispatcher = CleanupDispatcher::start()?;

    Ok(ResourceArc::new(TestHarnessResource {
        tree_tx,
        event_tx,
        render_rx,
        tree_tap_rx,
        base_instant: Mutex::new(Instant::now()),
        cleanup_dispatcher,
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
            submitted_at: Some(Instant::now()),
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
            submitted_at: Some(Instant::now()),
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
            trace: None,
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
                native_log: Arc::new(NativeLogRelay::default()),
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
                native_log: Arc::new(NativeLogRelay::default()),
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
        )
        .expect("runtime threads should join cleanly");

        assert!(!running_flag.load(Ordering::Relaxed));
        assert!(stop_flag.load(Ordering::Relaxed));
        assert!(tree_stopped.load(Ordering::Relaxed));
        assert!(event_stopped.load(Ordering::Relaxed));
        assert!(backend_stopped.load(Ordering::Relaxed));
        assert!(input_stopped.load(Ordering::Relaxed));
    }

    #[test]
    fn shutdown_renderer_runtime_reports_thread_panics() {
        let running_flag = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (tree_tx, _tree_rx) = bounded(1);
        let (event_tx, _event_rx) = bounded(1);
        let (render_tx, render_rx) = bounded(1);
        let backend_handle = thread::spawn(|| panic!("forced backend panic"));

        let error = shutdown_renderer_runtime(
            ShutdownRuntimeContext {
                running_flag,
                backend_wake: BackendWakeHandle::noop(),
                stop_flag,
                tree_tx,
                event_tx,
                render_tx: RenderSender {
                    tx: render_tx,
                    drop_rx: render_rx,
                    log_render: false,
                },
                close_signal_log: false,
                log_render: false,
                log_input: false,
            },
            RendererHandles {
                backend_handle: Some(backend_handle),
                ..RendererHandles::default()
            },
        )
        .unwrap_err();

        assert!(error.contains("backend: forced backend panic"));
    }

    #[test]
    fn cleanup_dispatcher_uses_persistent_fallback_when_primary_is_closed() {
        let (primary, primary_rx) = cleanup_channel::<CleanupTask>();
        drop(primary_rx);
        let fallback = CleanupDispatcher::start_worker("emerge_skia_cleanup_test_fallback")
            .expect("fallback cleanup worker should start");
        let dispatcher = CleanupDispatcher::from_senders(primary, fallback);
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();

        dispatcher.dispatch(Box::new(move || {
            let _ = completed_tx.send(());
        }));

        assert_eq!(completed_rx.recv_timeout(Duration::from_secs(1)), Ok(()));
    }

    #[test]
    fn renderer_resource_stop_blocks_until_threads_join_even_after_running_flag_cleared() {
        let running_flag = Arc::new(AtomicBool::new(false));
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
        let (release_tx, _release_rx) = bounded(1);

        let tree_stopped = Arc::new(AtomicBool::new(false));
        let event_stopped = Arc::new(AtomicBool::new(false));
        let backend_stopped = Arc::new(AtomicBool::new(false));

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

        let (backend_release_tx, backend_release_rx) = bounded::<()>(1);
        let (backend_stop_seen_tx, backend_stop_seen_rx) = bounded::<()>(1);
        let backend_handle = {
            let backend_stopped = Arc::clone(&backend_stopped);

            thread::spawn(move || {
                if matches!(render_rx.recv(), Ok(RenderMsg::Stop)) {
                    let _ = backend_stop_seen_tx.send(());
                    let _ = backend_release_rx.recv();
                    backend_stopped.store(true, Ordering::Relaxed);
                }
            })
        };

        let cleanup_dispatcher = CleanupDispatcher::start().expect("start cleanup dispatcher");
        let resource = Arc::new(RendererResource {
            running_flag: Arc::clone(&running_flag),
            backend_wake: backend_wake.clone(),
            stop_flag: Arc::clone(&stop_flag),
            tree_tx,
            event_tx,
            input_target: Arc::new(InputTargetRelay::default()),
            render_tx: render_sender,
            video_registry: Arc::new(VideoRegistry::new(
                release_tx,
                cleanup_dispatcher.clone(),
                None,
            )),
            video_wake: VideoWake::noop(),
            prime_video_supported: false,
            native_log: Arc::new(NativeLogRelay::default()),
            stats: None,
            latest_frame: Arc::new(LatestFrameStore::default()),
            info: RendererRuntimeInfo {
                backend: BackendKind::Headless,
                requested_rendering_api: RenderingApi::Auto,
                selected_rendering_api: RenderingApi::OpenGl,
                raster_present: RasterPresentKind::Auto,
                renderer_cache: RendererCacheStatus::enabled(),
                screenshot_supported: true,
                prime_video_supported: false,
            },
            close_signal_log: false,
            log_render: false,
            log_input: false,
            cleanup_dispatcher,
            handles: Mutex::new(Some(RendererHandles {
                backend_handle: Some(backend_handle),
                input_handle: None,
                tree_handle: Some(tree_handle),
                event_handle: Some(event_handle),
                heartbeat_handle: None,
            })),
        });

        let (stop_done_tx, stop_done_rx) = bounded::<()>(1);
        let stop_handle = {
            let resource = Arc::clone(&resource);

            thread::spawn(move || {
                resource.stop().expect("renderer should stop cleanly");
                let _ = stop_done_tx.send(());
            })
        };

        assert_eq!(
            backend_stop_seen_rx.recv_timeout(Duration::from_secs(1)),
            Ok(())
        );
        assert!(matches!(
            stop_done_rx.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));

        let _ = backend_release_tx.send(());
        assert_eq!(stop_done_rx.recv_timeout(Duration::from_secs(1)), Ok(()));
        let _ = stop_handle.join();

        assert!(!running_flag.load(Ordering::Relaxed));
        assert!(stop_flag.load(Ordering::Relaxed));
        assert!(tree_stopped.load(Ordering::Relaxed));
        assert!(event_stopped.load(Ordering::Relaxed));
        assert!(backend_stopped.load(Ordering::Relaxed));
        assert!(
            resource
                .handles
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none()
        );
    }

    #[test]
    fn send_registry_update_waits_for_channel_capacity_instead_of_dropping() {
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

        assert!(
            done_rx.recv_timeout(Duration::from_millis(20)).is_err(),
            "registry update send should wait while the event channel is full"
        );
        assert!(matches!(event_rx.try_recv(), Ok(EventMsg::Stop)));
        assert!(
            done_rx.recv_timeout(Duration::from_millis(100)).is_ok(),
            "registry update send should complete once capacity is available"
        );
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::RegistryUpdate { .. })
        ));

        let _ = handle.join();
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
    fn parse_rendering_api_config_accepts_nested_raster_present() {
        let config = parse_rendering_api_config(RenderingApiConfigNif {
            kind: "raster".to_string(),
            raster_present: "cpu".to_string(),
            raster_present_configured: true,
        })
        .expect("valid backend renderer config");

        assert_eq!(config.kind, RenderingApi::Raster);
        assert_eq!(config.raster_present, RasterPresentKind::Cpu);
        assert!(config.raster_present_configured);
    }

    #[test]
    fn parse_rendering_api_config_rejects_unknown_present_mode() {
        let err = parse_rendering_api_config(RenderingApiConfigNif {
            kind: "raster".to_string(),
            raster_present: "bogus".to_string(),
            raster_present_configured: true,
        })
        .expect_err("unknown present mode should be rejected");

        assert!(err.contains("unsupported rendering_api raster present mode"));
    }

    #[cfg(all(feature = "wayland", target_os = "linux"))]
    #[test]
    fn rendering_api_matrix_allows_wayland_auto_and_gl() {
        assert!(
            ensure_rendering_api_supported(BackendKind::Wayland, RenderingApiConfig::default())
                .is_ok()
        );
        assert!(
            ensure_rendering_api_supported(
                BackendKind::Wayland,
                RenderingApiConfig {
                    kind: RenderingApi::OpenGl,
                    raster_present: RasterPresentKind::Auto,
                    raster_present_configured: false,
                }
            )
            .is_ok()
        );
    }

    #[cfg(all(feature = "wayland", target_os = "linux"))]
    #[test]
    fn rendering_api_matrix_allows_wayland_raster() {
        assert!(
            ensure_rendering_api_supported(
                BackendKind::Wayland,
                RenderingApiConfig {
                    kind: RenderingApi::Raster,
                    raster_present: RasterPresentKind::Cpu,
                    raster_present_configured: true,
                },
            )
            .is_ok()
        );
    }

    #[cfg(all(feature = "drm", target_os = "linux"))]
    #[test]
    fn rendering_api_matrix_rejects_metal_on_drm() {
        let err = ensure_rendering_api_supported(
            BackendKind::Drm,
            RenderingApiConfig {
                kind: RenderingApi::Metal,
                raster_present: RasterPresentKind::Auto,
                raster_present_configured: false,
            },
        )
        .expect_err("metal should be rejected on drm");

        assert_eq!(
            err,
            "rendering_api :metal is only supported with backend :macos"
        );
    }

    #[test]
    fn rendering_api_matrix_allows_headless_auto_and_raster() {
        assert!(
            ensure_rendering_api_supported(BackendKind::Headless, RenderingApiConfig::default())
                .is_ok()
        );
        assert!(
            ensure_rendering_api_supported(
                BackendKind::Headless,
                RenderingApiConfig {
                    kind: RenderingApi::Raster,
                    raster_present: RasterPresentKind::Auto,
                    raster_present_configured: false,
                },
            )
            .is_ok()
        );
    }

    #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
    #[test]
    fn rendering_api_matrix_allows_headless_gl_on_linux() {
        assert!(
            ensure_rendering_api_supported(
                BackendKind::Headless,
                RenderingApiConfig {
                    kind: RenderingApi::OpenGl,
                    raster_present: RasterPresentKind::Auto,
                    raster_present_configured: false,
                },
            )
            .is_ok()
        );
    }

    #[cfg(not(all(target_os = "linux", feature = "headless-opengl")))]
    #[test]
    fn rendering_api_matrix_rejects_headless_gl_without_feature() {
        let err = ensure_rendering_api_supported(
            BackendKind::Headless,
            RenderingApiConfig {
                kind: RenderingApi::OpenGl,
                raster_present: RasterPresentKind::Auto,
                raster_present_configured: false,
            },
        )
        .expect_err("headless OpenGL should require the headless-opengl feature");

        assert!(err.contains("not available for backend :headless in this build"));
    }

    #[test]
    fn rendering_api_matrix_rejects_headless_metal_and_vulkan() {
        let metal_err = ensure_rendering_api_supported(
            BackendKind::Headless,
            RenderingApiConfig {
                kind: RenderingApi::Metal,
                raster_present: RasterPresentKind::Auto,
                raster_present_configured: false,
            },
        )
        .expect_err("metal should be rejected on headless");
        assert_eq!(
            metal_err,
            "rendering_api :metal is only supported with backend :macos"
        );

        let vulkan_err = ensure_rendering_api_supported(
            BackendKind::Headless,
            RenderingApiConfig {
                kind: RenderingApi::Vulkan,
                raster_present: RasterPresentKind::Auto,
                raster_present_configured: false,
            },
        )
        .expect_err("vulkan should be rejected on headless");
        assert_eq!(vulkan_err, "rendering_api :vulkan is not implemented yet");
    }

    #[cfg(feature = "macos")]
    #[test]
    fn rendering_api_matrix_rejects_gl_and_raster_present_on_macos() {
        let gl_err = ensure_rendering_api_supported(
            BackendKind::Macos,
            RenderingApiConfig {
                kind: RenderingApi::OpenGl,
                raster_present: RasterPresentKind::Auto,
                raster_present_configured: false,
            },
        )
        .expect_err("gl should be rejected on macOS");
        assert_eq!(
            gl_err,
            "rendering_api :opengl is not supported with backend :macos"
        );

        let raster_present_err = ensure_rendering_api_supported(
            BackendKind::Macos,
            RenderingApiConfig {
                kind: RenderingApi::Raster,
                raster_present: RasterPresentKind::Cpu,
                raster_present_configured: true,
            },
        )
        .expect_err("raster present options should be rejected on macOS");
        assert_eq!(
            raster_present_err,
            "rendering_api raster present options are only supported with backend :wayland or :drm"
        );
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
                "DRM backend is not compiled; add :drm to config :emerge, compiled_backends: [...]"
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
                "Wayland backend is not compiled; add :wayland to config :emerge, compiled_backends: [...]"
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
            trace: None,
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
                trace: None,
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
            trace: None,
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
                trace: None,
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

fn parse_rendering_api_config(config: RenderingApiConfigNif) -> Result<RenderingApiConfig, String> {
    let kind = parse_rendering_api(&config.kind)?;
    let raster_present = parse_raster_present_kind(&config.raster_present)?;

    Ok(RenderingApiConfig {
        kind,
        raster_present,
        raster_present_configured: config.raster_present_configured,
    })
}

fn parse_rendering_api(value: &str) -> Result<RenderingApi, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(RenderingApi::Auto),
        "opengl" => Ok(RenderingApi::OpenGl),
        "raster" => Ok(RenderingApi::Raster),
        "metal" => Ok(RenderingApi::Metal),
        "vulkan" => Ok(RenderingApi::Vulkan),
        other => Err(format!(
            "unsupported rendering_api kind: {other}; expected auto, opengl, raster, metal, or vulkan"
        )),
    }
}

fn parse_raster_present_kind(value: &str) -> Result<RasterPresentKind, String> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(RasterPresentKind::Auto),
        "gpu_upload" => Ok(RasterPresentKind::GpuUpload),
        "cpu" => Ok(RasterPresentKind::Cpu),
        other => Err(format!(
            "unsupported rendering_api raster present mode: {other}; expected auto, gpu_upload, or cpu"
        )),
    }
}

fn ensure_rendering_api_supported(
    backend: BackendKind,
    config: RenderingApiConfig,
) -> Result<(), String> {
    let _raster_present = config.raster_present;

    match backend {
        #[cfg(feature = "macos")]
        BackendKind::Macos => ensure_macos_rendering_api_supported(config),
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        BackendKind::Wayland => ensure_wayland_rendering_api_supported(config),
        #[cfg(all(feature = "drm", target_os = "linux"))]
        BackendKind::Drm => ensure_drm_rendering_api_supported(config),
        BackendKind::Headless => ensure_headless_rendering_api_supported(config),
    }
}

#[cfg(feature = "macos")]
fn ensure_macos_rendering_api_supported(config: RenderingApiConfig) -> Result<(), String> {
    match config.kind {
        RenderingApi::Auto | RenderingApi::Metal | RenderingApi::Raster
            if !config.raster_present_configured =>
        {
            Ok(())
        }
        RenderingApi::OpenGl => {
            Err("rendering_api :opengl is not supported with backend :macos".to_string())
        }
        RenderingApi::Vulkan => {
            Err("rendering_api :vulkan is not supported with backend :macos".to_string())
        }
        RenderingApi::Auto | RenderingApi::Metal | RenderingApi::Raster => Err(
            "rendering_api raster present options are only supported with backend :wayland or :drm"
                .to_string(),
        ),
    }
}

#[cfg(all(feature = "wayland", target_os = "linux"))]
fn ensure_wayland_rendering_api_supported(config: RenderingApiConfig) -> Result<(), String> {
    match config.kind {
        RenderingApi::Auto | RenderingApi::OpenGl | RenderingApi::Raster => Ok(()),
        RenderingApi::Metal => {
            Err("rendering_api :metal is only supported with backend :macos".to_string())
        }
        RenderingApi::Vulkan => Err("rendering_api :vulkan is not implemented yet".to_string()),
    }
}

#[cfg(all(feature = "drm", target_os = "linux"))]
fn ensure_drm_rendering_api_supported(config: RenderingApiConfig) -> Result<(), String> {
    match config.kind {
        RenderingApi::Auto | RenderingApi::OpenGl | RenderingApi::Raster => Ok(()),
        RenderingApi::Metal => {
            Err("rendering_api :metal is only supported with backend :macos".to_string())
        }
        RenderingApi::Vulkan => Err("rendering_api :vulkan is not implemented yet".to_string()),
    }
}

fn ensure_headless_rendering_api_supported(config: RenderingApiConfig) -> Result<(), String> {
    match config.kind {
        RenderingApi::Auto | RenderingApi::Raster => Ok(()),
        #[cfg(all(target_os = "linux", feature = "headless-opengl"))]
        RenderingApi::OpenGl => Ok(()),
        #[cfg(not(all(target_os = "linux", feature = "headless-opengl")))]
        RenderingApi::OpenGl => Err(
            "rendering_api :opengl is not available for backend :headless in this build"
                .to_string(),
        ),
        RenderingApi::Metal => {
            Err("rendering_api :metal is only supported with backend :macos".to_string())
        }
        RenderingApi::Vulkan => Err("rendering_api :vulkan is not implemented yet".to_string()),
    }
}

fn parse_backend_name(value: &str) -> Result<BackendKind, String> {
    match value {
        #[cfg(feature = "macos")]
        "macos" => Ok(BackendKind::Macos),
        #[cfg(not(feature = "macos"))]
        "macos" => Err(
            "macOS backend is not compiled; add :macos to config :emerge, compiled_backends: [...]"
                .to_string(),
        ),
        "headless" => Ok(BackendKind::Headless),
        #[cfg(all(feature = "drm", target_os = "linux"))]
        "drm" => Ok(BackendKind::Drm),
        #[cfg(not(all(feature = "drm", target_os = "linux")))]
        "drm" => Err(
            "DRM backend is not compiled; add :drm to config :emerge, compiled_backends: [...]"
                .to_string(),
        ),
        #[cfg(all(feature = "wayland", target_os = "linux"))]
        "wayland" => Ok(BackendKind::Wayland),
        #[cfg(not(all(feature = "wayland", target_os = "linux")))]
        "wayland" => Err(
            "Wayland backend is not compiled; add :wayland to config :emerge, compiled_backends: [...]"
                .to_string(),
        ),
        "wayland_legacy" => {
            Err("backend :wayland_legacy has been removed; use :wayland".to_string())
        }
        other => Err(format!("unsupported backend: {other}")),
    }
}
