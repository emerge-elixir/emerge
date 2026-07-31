use std::{
    ops::Index,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{
    render_scene::RenderSceneSummary,
    renderer::{
        RenderDrawTimings, RenderImageDrawProfile, RenderShadowDrawProfile, RenderTimings,
        RendererCacheFrameStats, RendererCachePaintLayerFrameStats,
    },
};

pub const SLOW_RENDER_STAGE_THRESHOLD: Duration = Duration::from_millis(4);
pub const SLOW_PRESENT_SUBMIT_THRESHOLD: Duration = Duration::from_millis(4);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutCacheStats {
    pub intrinsic_measure_hits: u64,
    pub intrinsic_measure_misses: u64,
    pub intrinsic_measure_stores: u64,
    pub subtree_measure_hits: u64,
    pub subtree_measure_misses: u64,
    pub subtree_measure_stores: u64,
    pub resolve_hits: u64,
    pub resolve_misses: u64,
    pub resolve_stores: u64,
}

impl LayoutCacheStats {
    fn increment(counter: &mut u64) {
        *counter = counter.saturating_add(1);
    }

    fn add_counter(counter: &mut u64, value: u64) {
        *counter = counter.saturating_add(value);
    }

    pub fn add(&mut self, other: Self) {
        Self::add_counter(
            &mut self.intrinsic_measure_hits,
            other.intrinsic_measure_hits,
        );
        Self::add_counter(
            &mut self.intrinsic_measure_misses,
            other.intrinsic_measure_misses,
        );
        Self::add_counter(
            &mut self.intrinsic_measure_stores,
            other.intrinsic_measure_stores,
        );
        Self::add_counter(&mut self.subtree_measure_hits, other.subtree_measure_hits);
        Self::add_counter(
            &mut self.subtree_measure_misses,
            other.subtree_measure_misses,
        );
        Self::add_counter(
            &mut self.subtree_measure_stores,
            other.subtree_measure_stores,
        );
        Self::add_counter(&mut self.resolve_hits, other.resolve_hits);
        Self::add_counter(&mut self.resolve_misses, other.resolve_misses);
        Self::add_counter(&mut self.resolve_stores, other.resolve_stores);
    }

    pub fn record_intrinsic_measure_hit(&mut self) {
        Self::increment(&mut self.intrinsic_measure_hits);
    }

    pub fn record_intrinsic_measure_miss(&mut self) {
        Self::increment(&mut self.intrinsic_measure_misses);
    }

    pub fn record_intrinsic_measure_store(&mut self) {
        Self::increment(&mut self.intrinsic_measure_stores);
    }

    pub fn record_subtree_measure_hit(&mut self) {
        Self::increment(&mut self.subtree_measure_hits);
    }

    pub fn record_subtree_measure_miss(&mut self) {
        Self::increment(&mut self.subtree_measure_misses);
    }

    pub fn record_subtree_measure_store(&mut self) {
        Self::increment(&mut self.subtree_measure_stores);
    }

    pub fn record_resolve_hit(&mut self) {
        Self::increment(&mut self.resolve_hits);
    }

    pub fn record_resolve_miss(&mut self) {
        Self::increment(&mut self.resolve_misses);
    }

    pub fn record_resolve_store(&mut self) {
        Self::increment(&mut self.resolve_stores);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DurationStatsSnapshot {
    pub count: u64,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererTimingMetric {
    Render,
    RenderDraw,
    RenderFlush,
    RenderGpuFlush,
    RenderSubmit,
    PresentSubmit,
    HeadlessPrimePrepare,
    HeadlessPrimeRetarget,
    HeadlessPrimeFenceExport,
    HeadlessPrimeGpuFinish,
    HeadlessPrimeExportMetadata,
    Pipeline,
    PipelineSubmitToTreeStart,
    PipelineTree,
    PipelineRenderQueue,
    PipelineSubmitToSwap,
    PipelineSwapToFrameCallback,
    VideoSubmitToImport,
    VideoSubmitToRelease,
    VideoRetireFence,
    VideoSubmitToPresent,
    DrmForcedGpuFinishBeforeSwap,
    DrmForcedGpuFinishAfterSwap,
    DrmGpuQueueCompletion,
    DrmEglSwapBuffers,
    DrmGbmLockFrontBuffer,
    DrmFramebufferLookup,
    DrmPreparedToCommit,
    DrmPreviousFlipToCommit,
    DrmAtomicCommitIoctl,
    DrmCommitToKernelPageFlip,
    DrmKernelPageFlipInterval,
    DrmPageFlipDispatchDelay,
    DrmCommitToPageFlip,
    Layout,
    Refresh,
    EventResolve,
    PatchTreeProcess,
    PatchTreeDecode,
    PatchTreeApply,
    PatchTreeAnimationSync,
    PatchTreePrepare,
    PatchTreeLayout,
    PatchTreeRefresh,
    PatchTreeRefreshTraversal,
    PatchTreeRefreshRegistryPost,
}

impl RendererTimingMetric {
    pub const COUNT: usize = 46;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Render,
        Self::RenderDraw,
        Self::RenderFlush,
        Self::RenderGpuFlush,
        Self::RenderSubmit,
        Self::PresentSubmit,
        Self::HeadlessPrimePrepare,
        Self::HeadlessPrimeRetarget,
        Self::HeadlessPrimeFenceExport,
        Self::HeadlessPrimeGpuFinish,
        Self::HeadlessPrimeExportMetadata,
        Self::Pipeline,
        Self::PipelineSubmitToTreeStart,
        Self::PipelineTree,
        Self::PipelineRenderQueue,
        Self::PipelineSubmitToSwap,
        Self::PipelineSwapToFrameCallback,
        Self::VideoSubmitToImport,
        Self::VideoSubmitToRelease,
        Self::VideoRetireFence,
        Self::VideoSubmitToPresent,
        Self::DrmForcedGpuFinishBeforeSwap,
        Self::DrmForcedGpuFinishAfterSwap,
        Self::DrmGpuQueueCompletion,
        Self::DrmEglSwapBuffers,
        Self::DrmGbmLockFrontBuffer,
        Self::DrmFramebufferLookup,
        Self::DrmPreparedToCommit,
        Self::DrmPreviousFlipToCommit,
        Self::DrmAtomicCommitIoctl,
        Self::DrmCommitToKernelPageFlip,
        Self::DrmKernelPageFlipInterval,
        Self::DrmPageFlipDispatchDelay,
        Self::DrmCommitToPageFlip,
        Self::Layout,
        Self::Refresh,
        Self::EventResolve,
        Self::PatchTreeProcess,
        Self::PatchTreeDecode,
        Self::PatchTreeApply,
        Self::PatchTreeAnimationSync,
        Self::PatchTreePrepare,
        Self::PatchTreeLayout,
        Self::PatchTreeRefresh,
        Self::PatchTreeRefreshTraversal,
        Self::PatchTreeRefreshRegistryPost,
    ];

    #[inline]
    const fn index(self) -> usize {
        self as usize
    }

    fn log_label(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::RenderDraw => "render draw",
            Self::RenderFlush => "render flush",
            Self::RenderGpuFlush => "render gpu flush",
            Self::RenderSubmit => "render submit",
            Self::PresentSubmit => "present submit",
            Self::HeadlessPrimePrepare => "headless PRIME prepare",
            Self::HeadlessPrimeRetarget => "headless PRIME retarget",
            Self::HeadlessPrimeFenceExport => "headless PRIME fence export",
            Self::HeadlessPrimeGpuFinish => "headless PRIME GPU finish fallback",
            Self::HeadlessPrimeExportMetadata => "headless PRIME export metadata",
            Self::Pipeline => "pipeline submit->frame callback",
            Self::PipelineSubmitToTreeStart => "pipeline submit->tree",
            Self::PipelineTree => "pipeline tree",
            Self::PipelineRenderQueue => "pipeline render queue",
            Self::PipelineSubmitToSwap => "pipeline submit->swap",
            Self::PipelineSwapToFrameCallback => "pipeline swap->frame callback",
            Self::VideoSubmitToImport => "video submit->import",
            Self::VideoSubmitToRelease => "video submit->lease release",
            Self::VideoRetireFence => "video retired fence",
            Self::VideoSubmitToPresent => "video submit->page flip",
            Self::DrmForcedGpuFinishBeforeSwap => "drm forced GPU finish before swap",
            Self::DrmForcedGpuFinishAfterSwap => "drm forced GPU finish after swap",
            Self::DrmGpuQueueCompletion => "drm GPU queue completion span",
            Self::DrmEglSwapBuffers => "drm eglSwapBuffers",
            Self::DrmGbmLockFrontBuffer => "drm GBM lock front buffer",
            Self::DrmFramebufferLookup => "drm framebuffer lookup",
            Self::DrmPreparedToCommit => "drm prepared->atomic commit",
            Self::DrmPreviousFlipToCommit => "drm previous kernel flip->next commit",
            Self::DrmAtomicCommitIoctl => "drm atomic commit ioctl",
            Self::DrmCommitToKernelPageFlip => "drm atomic commit->kernel page flip",
            Self::DrmKernelPageFlipInterval => "drm kernel page flip interval",
            Self::DrmPageFlipDispatchDelay => "drm kernel page flip->event dispatch",
            Self::DrmCommitToPageFlip => "drm atomic commit->event processed",
            Self::Layout => "layout",
            Self::Refresh => "refresh",
            Self::EventResolve => "event resolve",
            Self::PatchTreeProcess => "patch tree actor",
            Self::PatchTreeDecode => "patch tree decode",
            Self::PatchTreeApply => "patch tree apply",
            Self::PatchTreeAnimationSync => "patch tree animation sync",
            Self::PatchTreePrepare => "patch tree prepare attrs",
            Self::PatchTreeLayout => "patch tree layout",
            Self::PatchTreeRefresh => "patch tree refresh",
            Self::PatchTreeRefreshTraversal => "patch tree refresh traversal",
            Self::PatchTreeRefreshRegistryPost => "patch tree refresh registry post",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RendererTimingSnapshots {
    values: [DurationStatsSnapshot; RendererTimingMetric::COUNT],
}

impl Default for RendererTimingSnapshots {
    fn default() -> Self {
        Self {
            values: [DurationStatsSnapshot::default(); RendererTimingMetric::COUNT],
        }
    }
}

impl Index<RendererTimingMetric> for RendererTimingSnapshots {
    type Output = DurationStatsSnapshot;

    fn index(&self, metric: RendererTimingMetric) -> &Self::Output {
        &self.values[metric.index()]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererStatsSnapshot {
    pub window: Duration,
    pub fps: f64,
    pub display_fps: f64,
    pub display_frame_ms: f64,
    pub frame_count: u64,
    pub timings: RendererTimingSnapshots,
    pub video_pipeline: VideoPipelineStatsSnapshot,
    pub layout_cache: LayoutCacheStats,
    pub renderer_cache: RendererCacheStatsSnapshot,
}

impl RendererStatsSnapshot {
    pub fn timing(&self, metric: RendererTimingMetric) -> &DurationStatsSnapshot {
        &self.timings[metric]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoPipelineStatsSnapshot {
    pub submitted: u64,
    pub inactive_dropped: u64,
    pub pending_replaced: u64,
    pub pending_taken: u64,
    pub imported: u64,
    pub leases_released: u64,
    pub retired_fences_created: u64,
    pub retired_fences_released: u64,
    pub acquire_fences_received: u64,
    pub acquire_server_waits_queued: u64,
    pub acquire_client_wait_fallbacks: u64,
    pub acquire_wait_timeouts: u64,
    pub acquire_wait_errors: u64,
    pub primary_prepared: u64,
    pub video_primary_prepared: u64,
    pub stale_prepared: u64,
    pub stale_video_prepared: u64,
    pub gbm_no_free: u64,
    pub primary_commit_attempts: u64,
    pub primary_commit_ebusy: u64,
    pub primary_committed: u64,
    pub primary_presented: u64,
    pub video_primary_presented: u64,
    pub page_flip_events: u64,
    pub page_flip_sequence_steps: u64,
    pub missed_vblanks: u64,
    pub current_pending: u64,
    pub current_direct_imports: u64,
    pub current_retired_imports: u64,
    pub max_retired_imports: u64,
    pub current_prepared: u64,
    pub current_in_flight: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererCacheStatsSnapshot {
    pub paint_layer: RendererCachePaintLayerStatsSnapshot,
}

impl RendererCacheStatsSnapshot {
    pub fn is_empty(&self) -> bool {
        self.paint_layer.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererCachePaintLayerStatsSnapshot {
    pub candidates: u64,
    pub visible_candidates: u64,

    pub suppressed_by_parent: u64,
    pub bypassed_low_value: u64,
    pub admitted: u64,
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
    pub stale_evictions: u64,
    pub rejected: u64,
    pub current_entries: u64,
    pub current_bytes: u64,
    pub current_gpu_payloads: u64,
    pub current_cpu_payloads: u64,
    pub evicted_bytes: u64,
    pub stale_evicted_bytes: u64,
    pub gpu_payload_stores: u64,
    pub cpu_payload_stores: u64,
    pub cached_image_draws: u64,
    pub composited_payload_pixels: u64,
    pub composited_visible_pixels: u64,
    pub hit_payload_pixels: u64,
    pub hit_visible_pixels: u64,
    pub store_payload_pixels: u64,
    pub store_visible_pixels: u64,
    pub prepare_successes: u64,
    pub prepare_failures: u64,
    pub direct_fallbacks_after_admission: u64,
    pub rejected_ineligible: u64,
    pub rejected_admission: u64,
    pub rejected_oversized: u64,
    pub rejected_payload_budget: u64,
    pub rejected_fractional_placement: u64,
    pub rejected_unsupported_transform: u64,
    pub prepare: DurationStatsSnapshot,
    pub draw_hit: DurationStatsSnapshot,
}

impl RendererCachePaintLayerStatsSnapshot {
    pub fn is_empty(&self) -> bool {
        self.candidates == 0
            && self.visible_candidates == 0
            && self.suppressed_by_parent == 0
            && self.bypassed_low_value == 0
            && self.admitted == 0
            && self.hits == 0
            && self.misses == 0
            && self.stores == 0
            && self.evictions == 0
            && self.stale_evictions == 0
            && self.rejected == 0
            && self.current_entries == 0
            && self.current_bytes == 0
            && self.current_gpu_payloads == 0
            && self.current_cpu_payloads == 0
            && self.evicted_bytes == 0
            && self.stale_evicted_bytes == 0
            && self.gpu_payload_stores == 0
            && self.cpu_payload_stores == 0
            && self.cached_image_draws == 0
            && self.composited_payload_pixels == 0
            && self.composited_visible_pixels == 0
            && self.hit_payload_pixels == 0
            && self.hit_visible_pixels == 0
            && self.store_payload_pixels == 0
            && self.store_visible_pixels == 0
            && self.prepare_successes == 0
            && self.prepare_failures == 0
            && self.direct_fallbacks_after_admission == 0
            && self.rejected_ineligible == 0
            && self.rejected_admission == 0
            && self.rejected_oversized == 0
            && self.rejected_payload_budget == 0
            && self.rejected_fractional_placement == 0
            && self.rejected_unsupported_transform == 0
            && self.prepare.count == 0
            && self.draw_hit.count == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsFamilies {
    pub timings: bool,
    pub layout_cache: bool,
}

impl StatsFamilies {
    pub fn all_current() -> Self {
        Self {
            timings: true,
            layout_cache: true,
        }
    }
}

#[derive(Default)]
struct DurationStatsWindow {
    count: u64,
    total_ns: u128,
    min_ns: Option<u64>,
    max_ns: u64,
}

impl DurationStatsWindow {
    fn record(&mut self, duration: Duration) {
        let ns = duration.as_nanos().min(u128::from(u64::MAX)) as u64;
        self.count = self.count.saturating_add(1);
        self.total_ns = self.total_ns.saturating_add(u128::from(ns));
        self.min_ns = Some(self.min_ns.map(|current| current.min(ns)).unwrap_or(ns));
        self.max_ns = self.max_ns.max(ns);
    }

    fn record_many(&mut self, total: Duration, count: u64) {
        if count == 0 {
            return;
        }
        let total_ns = total.as_nanos();
        let avg_ns = (total_ns / u128::from(count)).min(u128::from(u64::MAX)) as u64;
        self.count = self.count.saturating_add(count);
        self.total_ns = self.total_ns.saturating_add(total_ns);
        self.min_ns = Some(
            self.min_ns
                .map(|current| current.min(avg_ns))
                .unwrap_or(avg_ns),
        );
        self.max_ns = self.max_ns.max(avg_ns);
    }

    fn snapshot(&self) -> DurationStatsSnapshot {
        if self.count == 0 {
            return DurationStatsSnapshot {
                count: 0,
                avg_ms: 0.0,
                min_ms: 0.0,
                max_ms: 0.0,
            };
        }

        DurationStatsSnapshot {
            count: self.count,
            avg_ms: self.total_ns as f64 / self.count as f64 / 1_000_000.0,
            min_ms: self.min_ns.unwrap_or(0) as f64 / 1_000_000.0,
            max_ms: self.max_ns as f64 / 1_000_000.0,
        }
    }
}

struct RendererTimingWindows {
    values: [DurationStatsWindow; RendererTimingMetric::COUNT],
}

impl Default for RendererTimingWindows {
    fn default() -> Self {
        Self {
            values: std::array::from_fn(|_| DurationStatsWindow::default()),
        }
    }
}

impl RendererTimingWindows {
    #[inline]
    fn record(&mut self, metric: RendererTimingMetric, duration: Duration) {
        self.values[metric.index()].record(duration);
    }

    fn snapshot(&self) -> RendererTimingSnapshots {
        RendererTimingSnapshots {
            values: std::array::from_fn(|index| self.values[index].snapshot()),
        }
    }
}

#[derive(Default)]
struct VideoPipelineStatsWindow {
    submitted: u64,
    inactive_dropped: u64,
    pending_replaced: u64,
    pending_taken: u64,
    imported: u64,
    leases_released: u64,
    retired_fences_created: u64,
    retired_fences_released: u64,
    acquire_fences_received: u64,
    acquire_server_waits_queued: u64,
    acquire_client_wait_fallbacks: u64,
    acquire_wait_timeouts: u64,
    acquire_wait_errors: u64,
    primary_prepared: u64,
    video_primary_prepared: u64,
    stale_prepared: u64,
    stale_video_prepared: u64,
    gbm_no_free: u64,
    primary_commit_attempts: u64,
    primary_commit_ebusy: u64,
    primary_committed: u64,
    primary_presented: u64,
    video_primary_presented: u64,
    page_flip_events: u64,
    page_flip_sequence_steps: u64,
    missed_vblanks: u64,
    current_pending: u64,
    current_direct_imports: u64,
    current_retired_imports: u64,
    max_retired_imports: u64,
    current_prepared: u64,
    current_in_flight: u64,
}

impl VideoPipelineStatsWindow {
    fn snapshot(&self) -> VideoPipelineStatsSnapshot {
        VideoPipelineStatsSnapshot {
            submitted: self.submitted,
            inactive_dropped: self.inactive_dropped,
            pending_replaced: self.pending_replaced,
            pending_taken: self.pending_taken,
            imported: self.imported,
            leases_released: self.leases_released,
            retired_fences_created: self.retired_fences_created,
            retired_fences_released: self.retired_fences_released,
            acquire_fences_received: self.acquire_fences_received,
            acquire_server_waits_queued: self.acquire_server_waits_queued,
            acquire_client_wait_fallbacks: self.acquire_client_wait_fallbacks,
            acquire_wait_timeouts: self.acquire_wait_timeouts,
            acquire_wait_errors: self.acquire_wait_errors,
            primary_prepared: self.primary_prepared,
            video_primary_prepared: self.video_primary_prepared,
            stale_prepared: self.stale_prepared,
            stale_video_prepared: self.stale_video_prepared,
            gbm_no_free: self.gbm_no_free,
            primary_commit_attempts: self.primary_commit_attempts,
            primary_commit_ebusy: self.primary_commit_ebusy,
            primary_committed: self.primary_committed,
            primary_presented: self.primary_presented,
            video_primary_presented: self.video_primary_presented,
            page_flip_events: self.page_flip_events,
            page_flip_sequence_steps: self.page_flip_sequence_steps,
            missed_vblanks: self.missed_vblanks,
            current_pending: self.current_pending,
            current_direct_imports: self.current_direct_imports,
            current_retired_imports: self.current_retired_imports,
            max_retired_imports: self.max_retired_imports,
            current_prepared: self.current_prepared,
            current_in_flight: self.current_in_flight,
        }
    }

    fn copy_gauges_from(&mut self, previous: &Self) {
        self.current_pending = previous.current_pending;
        self.current_direct_imports = previous.current_direct_imports;
        self.current_retired_imports = previous.current_retired_imports;
        self.max_retired_imports = previous.current_retired_imports;
        self.current_prepared = previous.current_prepared;
        self.current_in_flight = previous.current_in_flight;
    }
}

struct RendererStatsWindow {
    started_at: Instant,
    last_display_interval_ns: Option<u64>,
    frame_count: u64,
    timings: RendererTimingWindows,
    video_pipeline: VideoPipelineStatsWindow,
    layout_cache: LayoutCacheStats,
    renderer_cache: RendererCacheStatsWindow,
}

impl RendererStatsWindow {
    fn new(started_at: Instant, last_display_interval_ns: Option<u64>) -> Self {
        Self {
            started_at,
            last_display_interval_ns,
            frame_count: 0,
            timings: RendererTimingWindows::default(),
            video_pipeline: VideoPipelineStatsWindow::default(),
            layout_cache: LayoutCacheStats::default(),
            renderer_cache: RendererCacheStatsWindow::default(),
        }
    }

    fn snapshot(&self, now: Instant) -> RendererStatsSnapshot {
        let elapsed = now.saturating_duration_since(self.started_at);

        RendererStatsSnapshot {
            window: elapsed,
            fps: if elapsed.is_zero() {
                0.0
            } else {
                self.frame_count as f64 / elapsed.as_secs_f64()
            },
            display_fps: self
                .last_display_interval_ns
                .map(|ns| 1_000_000_000.0 / ns as f64)
                .unwrap_or(0.0),
            display_frame_ms: self
                .last_display_interval_ns
                .map(|ns| ns as f64 / 1_000_000.0)
                .unwrap_or(0.0),
            frame_count: self.frame_count,
            timings: self.timings.snapshot(),
            video_pipeline: self.video_pipeline.snapshot(),
            layout_cache: self.layout_cache,
            renderer_cache: self.renderer_cache.snapshot(),
        }
    }
}

#[derive(Default)]
struct RendererCacheStatsWindow {
    paint_layer: RendererCachePaintLayerStatsWindow,
}

impl RendererCacheStatsWindow {
    fn record(&mut self, stats: RendererCacheFrameStats) {
        self.paint_layer.record(stats.paint_layer);
    }

    fn snapshot(&self) -> RendererCacheStatsSnapshot {
        RendererCacheStatsSnapshot {
            paint_layer: self.paint_layer.snapshot(),
        }
    }
}

#[derive(Default)]
struct RendererCachePaintLayerStatsWindow {
    candidates: u64,
    visible_candidates: u64,

    suppressed_by_parent: u64,
    bypassed_low_value: u64,
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
    cached_image_draws: u64,
    composited_payload_pixels: u64,
    composited_visible_pixels: u64,
    hit_payload_pixels: u64,
    hit_visible_pixels: u64,
    store_payload_pixels: u64,
    store_visible_pixels: u64,
    prepare_successes: u64,
    prepare_failures: u64,
    direct_fallbacks_after_admission: u64,
    rejected_ineligible: u64,
    rejected_admission: u64,
    rejected_oversized: u64,
    rejected_payload_budget: u64,
    rejected_fractional_placement: u64,
    rejected_unsupported_transform: u64,
    prepare: DurationStatsWindow,
    draw_hit: DurationStatsWindow,
}

impl RendererCachePaintLayerStatsWindow {
    fn record(&mut self, stats: RendererCachePaintLayerFrameStats) {
        self.candidates = self.candidates.saturating_add(stats.candidates);
        self.visible_candidates = self
            .visible_candidates
            .saturating_add(stats.visible_candidates);
        self.suppressed_by_parent = self
            .suppressed_by_parent
            .saturating_add(stats.suppressed_by_parent);
        self.bypassed_low_value = self
            .bypassed_low_value
            .saturating_add(stats.bypassed_low_value);
        self.admitted = self.admitted.saturating_add(stats.admitted);
        self.hits = self.hits.saturating_add(stats.hits);
        self.misses = self.misses.saturating_add(stats.misses);
        self.stores = self.stores.saturating_add(stats.stores);
        self.evictions = self.evictions.saturating_add(stats.evictions);
        self.stale_evictions = self.stale_evictions.saturating_add(stats.stale_evictions);
        self.rejected = self.rejected.saturating_add(stats.rejected);
        self.current_entries = stats.current_entries;
        self.current_bytes = stats.current_bytes;
        self.current_gpu_payloads = stats.current_gpu_payloads;
        self.current_cpu_payloads = stats.current_cpu_payloads;
        self.evicted_bytes = self.evicted_bytes.saturating_add(stats.evicted_bytes);
        self.stale_evicted_bytes = self
            .stale_evicted_bytes
            .saturating_add(stats.stale_evicted_bytes);
        self.gpu_payload_stores = self
            .gpu_payload_stores
            .saturating_add(stats.gpu_payload_stores);
        self.cpu_payload_stores = self
            .cpu_payload_stores
            .saturating_add(stats.cpu_payload_stores);
        self.cached_image_draws = self
            .cached_image_draws
            .saturating_add(stats.cached_image_draws);
        self.composited_payload_pixels = self
            .composited_payload_pixels
            .saturating_add(stats.composited_payload_pixels);
        self.composited_visible_pixels = self
            .composited_visible_pixels
            .saturating_add(stats.composited_visible_pixels);
        self.hit_payload_pixels = self
            .hit_payload_pixels
            .saturating_add(stats.hit_payload_pixels);
        self.hit_visible_pixels = self
            .hit_visible_pixels
            .saturating_add(stats.hit_visible_pixels);
        self.store_payload_pixels = self
            .store_payload_pixels
            .saturating_add(stats.store_payload_pixels);
        self.store_visible_pixels = self
            .store_visible_pixels
            .saturating_add(stats.store_visible_pixels);
        self.prepare_successes = self
            .prepare_successes
            .saturating_add(stats.prepare_successes);
        self.prepare_failures = self.prepare_failures.saturating_add(stats.prepare_failures);
        self.direct_fallbacks_after_admission = self
            .direct_fallbacks_after_admission
            .saturating_add(stats.direct_fallbacks_after_admission);
        self.rejected_ineligible = self
            .rejected_ineligible
            .saturating_add(stats.rejected_ineligible);
        self.rejected_admission = self
            .rejected_admission
            .saturating_add(stats.rejected_admission);
        self.rejected_oversized = self
            .rejected_oversized
            .saturating_add(stats.rejected_oversized);
        self.rejected_payload_budget = self
            .rejected_payload_budget
            .saturating_add(stats.rejected_payload_budget);
        self.rejected_fractional_placement = self
            .rejected_fractional_placement
            .saturating_add(stats.rejected_fractional_placement);
        self.rejected_unsupported_transform = self
            .rejected_unsupported_transform
            .saturating_add(stats.rejected_unsupported_transform);

        if stats.prepare_successes > 0 {
            self.prepare
                .record_many(stats.prepare_time, stats.prepare_successes);
        }

        if stats.hits > 0 {
            self.draw_hit.record_many(stats.draw_hit_time, stats.hits);
        }
    }

    fn snapshot(&self) -> RendererCachePaintLayerStatsSnapshot {
        RendererCachePaintLayerStatsSnapshot {
            candidates: self.candidates,
            visible_candidates: self.visible_candidates,
            suppressed_by_parent: self.suppressed_by_parent,
            bypassed_low_value: self.bypassed_low_value,
            admitted: self.admitted,
            hits: self.hits,
            misses: self.misses,
            stores: self.stores,
            evictions: self.evictions,
            stale_evictions: self.stale_evictions,
            rejected: self.rejected,
            current_entries: self.current_entries,
            current_bytes: self.current_bytes,
            current_gpu_payloads: self.current_gpu_payloads,
            current_cpu_payloads: self.current_cpu_payloads,
            evicted_bytes: self.evicted_bytes,
            stale_evicted_bytes: self.stale_evicted_bytes,
            gpu_payload_stores: self.gpu_payload_stores,
            cpu_payload_stores: self.cpu_payload_stores,
            cached_image_draws: self.cached_image_draws,
            composited_payload_pixels: self.composited_payload_pixels,
            composited_visible_pixels: self.composited_visible_pixels,
            hit_payload_pixels: self.hit_payload_pixels,
            hit_visible_pixels: self.hit_visible_pixels,
            store_payload_pixels: self.store_payload_pixels,
            store_visible_pixels: self.store_visible_pixels,
            prepare_successes: self.prepare_successes,
            prepare_failures: self.prepare_failures,
            direct_fallbacks_after_admission: self.direct_fallbacks_after_admission,
            rejected_ineligible: self.rejected_ineligible,
            rejected_admission: self.rejected_admission,
            rejected_oversized: self.rejected_oversized,
            rejected_payload_budget: self.rejected_payload_budget,
            rejected_fractional_placement: self.rejected_fractional_placement,
            rejected_unsupported_transform: self.rejected_unsupported_transform,
            prepare: self.prepare.snapshot(),
            draw_hit: self.draw_hit.snapshot(),
        }
    }
}

pub struct RendererStatsCollector {
    window: Mutex<RendererStatsWindow>,
    families: StatsFamilies,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineLayoutQueueTiming {
    pub render_queued_at: Instant,
    pub pipeline_submitted_at: Option<Instant>,
    pub pipeline_render_queued_at: Option<Instant>,
}

pub fn earliest_pipeline_instant(left: Option<Instant>, right: Option<Instant>) -> Option<Instant> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left <= right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub fn record_pipeline_layout_queued(
    stats: Option<&RendererStatsCollector>,
    current_pipeline_submitted_at: Option<Instant>,
    current_pipeline_render_queued_at: Option<Instant>,
    pipeline_submitted_at: Option<Instant>,
    tree_started_at: Option<Instant>,
    render_queued_at: Instant,
) -> PipelineLayoutQueueTiming {
    let pipeline_render_queued_at = pipeline_submitted_at
        .map(|_| render_queued_at)
        .or(current_pipeline_render_queued_at);

    if let (Some(stats), Some(tree_started_at), Some(render_queued_at)) = (
        stats,
        tree_started_at,
        pipeline_submitted_at.map(|_| render_queued_at),
    ) {
        stats.record_pipeline_tree(tree_started_at, render_queued_at);
    }

    PipelineLayoutQueueTiming {
        render_queued_at,
        pipeline_submitted_at: earliest_pipeline_instant(
            current_pipeline_submitted_at,
            pipeline_submitted_at,
        ),
        pipeline_render_queued_at,
    }
}

impl Default for RendererStatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl RendererStatsCollector {
    pub fn new() -> Self {
        Self::with_families(StatsFamilies::all_current())
    }

    pub fn with_families(families: StatsFamilies) -> Self {
        Self {
            window: Mutex::new(RendererStatsWindow::new(Instant::now(), None)),
            families,
        }
    }

    pub fn layout_cache_enabled(&self) -> bool {
        self.families.layout_cache
    }

    pub fn record_frame_present(&self) {
        if !self.families.timings {
            return;
        }

        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.frame_count = window.frame_count.saturating_add(1);
    }

    pub fn record_display_interval(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ns = duration.as_nanos().min(u128::from(u64::MAX)) as u64;
        window.last_display_interval_ns = Some(ns);
    }

    pub fn record_render(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::Render, duration);
    }

    pub fn record_render_draw(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::RenderDraw, duration);
    }

    pub fn record_render_flush(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::RenderFlush, duration);
    }

    pub fn record_render_gpu_flush(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::RenderGpuFlush, duration);
    }

    pub fn record_render_submit(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::RenderSubmit, duration);
    }

    pub fn record_render_timings(&self, render_duration: Duration, timings: &RenderTimings) {
        self.record_render(render_duration);
        self.record_render_draw(timings.draw);
        self.record_render_flush(timings.flush);
        self.record_render_gpu_flush(timings.gpu_flush);
        self.record_render_submit(timings.submit);
        if let Some(renderer_cache) = timings.renderer_cache.as_deref() {
            self.record_renderer_cache(*renderer_cache);
        }
    }

    pub fn record_present_submit(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PresentSubmit, duration);
    }

    pub fn record_headless_prime_timings(
        &self,
        prepare: Duration,
        retarget: Duration,
        fence_export: Option<Duration>,
        gpu_finish_fallback: Option<Duration>,
        export_metadata: Duration,
    ) {
        self.record_timing(RendererTimingMetric::HeadlessPrimePrepare, prepare);
        self.record_timing(RendererTimingMetric::HeadlessPrimeRetarget, retarget);
        if let Some(fence_export) = fence_export {
            self.record_timing(RendererTimingMetric::HeadlessPrimeFenceExport, fence_export);
        }
        if let Some(gpu_finish_fallback) = gpu_finish_fallback {
            self.record_timing(
                RendererTimingMetric::HeadlessPrimeGpuFinish,
                gpu_finish_fallback,
            );
        }
        self.record_timing(
            RendererTimingMetric::HeadlessPrimeExportMetadata,
            export_metadata,
        );
    }

    pub fn record_video_submitted(&self, replaced_pending: bool) {
        self.update_video_pipeline(|video| {
            video.submitted = video.submitted.saturating_add(1);
            if replaced_pending {
                video.pending_replaced = video.pending_replaced.saturating_add(1);
            } else {
                video.current_pending = video.current_pending.saturating_add(1);
            }
        });
    }

    pub fn record_video_inactive_drop(&self) {
        self.update_video_pipeline(|video| {
            video.inactive_dropped = video.inactive_dropped.saturating_add(1);
        });
    }

    pub fn record_video_pending_taken(&self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.update_video_pipeline(|video| {
            video.pending_taken = video.pending_taken.saturating_add(count);
            video.current_pending = video.current_pending.saturating_sub(count);
        });
    }

    pub fn record_video_imported(&self, submit_to_import: Duration) {
        self.update_video_pipeline(|video| {
            video.imported = video.imported.saturating_add(1);
        });
        self.record_timing(RendererTimingMetric::VideoSubmitToImport, submit_to_import);
    }

    pub fn record_video_lease_released(&self, submit_to_release: Duration) {
        self.update_video_pipeline(|video| {
            video.leases_released = video.leases_released.saturating_add(1);
        });
        self.record_timing(
            RendererTimingMetric::VideoSubmitToRelease,
            submit_to_release,
        );
    }

    pub fn record_video_acquire_fence_received(&self) {
        self.update_video_pipeline(|video| {
            video.acquire_fences_received = video.acquire_fences_received.saturating_add(1);
        });
    }

    pub fn record_video_acquire_server_wait_queued(&self) {
        self.update_video_pipeline(|video| {
            video.acquire_server_waits_queued = video.acquire_server_waits_queued.saturating_add(1);
        });
    }

    pub fn record_video_acquire_client_wait_fallback(&self) {
        self.update_video_pipeline(|video| {
            video.acquire_client_wait_fallbacks =
                video.acquire_client_wait_fallbacks.saturating_add(1);
        });
    }

    pub fn record_video_acquire_wait_timeout(&self) {
        self.update_video_pipeline(|video| {
            video.acquire_wait_timeouts = video.acquire_wait_timeouts.saturating_add(1);
        });
    }

    pub fn record_video_acquire_wait_error(&self) {
        self.update_video_pipeline(|video| {
            video.acquire_wait_errors = video.acquire_wait_errors.saturating_add(1);
        });
    }

    pub fn record_video_retired_fence_created(&self, current_depth: usize) {
        let current_depth = u64::try_from(current_depth).unwrap_or(u64::MAX);
        self.update_video_pipeline(|video| {
            video.retired_fences_created = video.retired_fences_created.saturating_add(1);
            video.current_retired_imports = current_depth;
            video.max_retired_imports = video.max_retired_imports.max(current_depth);
        });
    }

    pub fn record_video_retired_fence_released(&self, retired_for: Duration) {
        self.update_video_pipeline(|video| {
            video.retired_fences_released = video.retired_fences_released.saturating_add(1);
            video.current_retired_imports = video.current_retired_imports.saturating_sub(1);
        });
        self.record_timing(RendererTimingMetric::VideoRetireFence, retired_for);
    }

    pub fn set_video_import_gauges(&self, direct_imports: usize, retired_imports: usize) {
        let direct_imports = u64::try_from(direct_imports).unwrap_or(u64::MAX);
        let retired_imports = u64::try_from(retired_imports).unwrap_or(u64::MAX);
        self.update_video_pipeline(|video| {
            video.current_direct_imports = direct_imports;
            video.current_retired_imports = retired_imports;
            video.max_retired_imports = video.max_retired_imports.max(retired_imports);
        });
    }

    pub fn record_drm_forced_gpu_finish_before_swap(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmForcedGpuFinishBeforeSwap, duration);
    }

    pub fn record_drm_forced_gpu_finish_after_swap(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmForcedGpuFinishAfterSwap, duration);
    }

    pub fn record_drm_gpu_queue_completion(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmGpuQueueCompletion, duration);
    }

    pub fn record_drm_egl_swap_buffers(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmEglSwapBuffers, duration);
    }

    pub fn record_drm_gbm_lock_front_buffer(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmGbmLockFrontBuffer, duration);
    }

    pub fn record_drm_framebuffer_lookup(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmFramebufferLookup, duration);
    }

    pub fn record_drm_prepared_to_commit(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmPreparedToCommit, duration);
    }

    pub fn record_drm_previous_flip_to_commit(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmPreviousFlipToCommit, duration);
    }

    pub fn record_drm_atomic_commit_ioctl(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmAtomicCommitIoctl, duration);
    }

    pub fn record_drm_commit_to_kernel_page_flip(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmCommitToKernelPageFlip, duration);
    }

    pub fn record_drm_kernel_page_flip_interval(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmKernelPageFlipInterval, duration);
    }

    pub fn record_drm_page_flip_dispatch_delay(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::DrmPageFlipDispatchDelay, duration);
    }

    pub fn record_drm_page_flip_sequence(&self, sequence_delta: Option<u32>) {
        self.update_video_pipeline(|video| {
            video.page_flip_events = video.page_flip_events.saturating_add(1);
            if let Some(delta) = sequence_delta {
                let delta = u64::from(delta);
                video.page_flip_sequence_steps =
                    video.page_flip_sequence_steps.saturating_add(delta);
                video.missed_vblanks = video.missed_vblanks.saturating_add(delta.saturating_sub(1));
            }
        });
    }

    pub fn record_drm_primary_prepared(&self, contains_new_video: bool) {
        self.update_video_pipeline(|video| {
            video.primary_prepared = video.primary_prepared.saturating_add(1);
            if contains_new_video {
                video.video_primary_prepared = video.video_primary_prepared.saturating_add(1);
            }
            video.current_prepared = 1;
        });
    }

    pub fn record_drm_stale_prepared(&self, contained_new_video: bool) {
        self.update_video_pipeline(|video| {
            video.stale_prepared = video.stale_prepared.saturating_add(1);
            if contained_new_video {
                video.stale_video_prepared = video.stale_video_prepared.saturating_add(1);
            }
            video.current_prepared = 0;
        });
    }

    pub fn record_drm_gbm_no_free_buffer(&self) {
        self.update_video_pipeline(|video| {
            video.gbm_no_free = video.gbm_no_free.saturating_add(1);
        });
    }

    pub fn record_drm_primary_commit_attempt(&self) {
        self.update_video_pipeline(|video| {
            video.primary_commit_attempts = video.primary_commit_attempts.saturating_add(1);
        });
    }

    pub fn record_drm_primary_commit_ebusy(&self) {
        self.update_video_pipeline(|video| {
            video.primary_commit_ebusy = video.primary_commit_ebusy.saturating_add(1);
        });
    }

    pub fn record_drm_primary_committed(&self) {
        self.update_video_pipeline(|video| {
            video.primary_committed = video.primary_committed.saturating_add(1);
            video.current_prepared = 0;
            video.current_in_flight = 1;
        });
    }

    pub fn record_drm_primary_presented(
        &self,
        contained_new_video: bool,
        commit_to_page_flip: Duration,
        video_submit_to_present: Option<Duration>,
    ) {
        self.update_video_pipeline(|video| {
            video.primary_presented = video.primary_presented.saturating_add(1);
            if contained_new_video {
                video.video_primary_presented = video.video_primary_presented.saturating_add(1);
            }
            video.current_in_flight = 0;
        });
        self.record_timing(
            RendererTimingMetric::DrmCommitToPageFlip,
            commit_to_page_flip,
        );
        if let Some(duration) = video_submit_to_present {
            self.record_timing(RendererTimingMetric::VideoSubmitToPresent, duration);
        }
    }

    pub fn record_pipeline(&self, submitted_at: Instant, presented_at: Instant) {
        self.record_timing_span(RendererTimingMetric::Pipeline, submitted_at, presented_at);
    }

    pub fn record_pipeline_submit_to_tree_start(
        &self,
        submitted_at: Instant,
        tree_started_at: Instant,
    ) {
        self.record_timing_span(
            RendererTimingMetric::PipelineSubmitToTreeStart,
            submitted_at,
            tree_started_at,
        );
    }

    pub fn record_pipeline_tree(&self, tree_started_at: Instant, render_queued_at: Instant) {
        self.record_timing_span(
            RendererTimingMetric::PipelineTree,
            tree_started_at,
            render_queued_at,
        );
    }

    pub fn record_pipeline_render_queue(
        &self,
        render_queued_at: Instant,
        render_received_at: Instant,
    ) {
        self.record_timing_span(
            RendererTimingMetric::PipelineRenderQueue,
            render_queued_at,
            render_received_at,
        );
    }

    pub fn record_pipeline_submit_to_swap(&self, submitted_at: Instant, swap_done_at: Instant) {
        self.record_timing_span(
            RendererTimingMetric::PipelineSubmitToSwap,
            submitted_at,
            swap_done_at,
        );
    }

    pub fn record_pipeline_swap_to_frame_callback(
        &self,
        swap_done_at: Instant,
        presented_at: Instant,
    ) {
        self.record_timing_span(
            RendererTimingMetric::PipelineSwapToFrameCallback,
            swap_done_at,
            presented_at,
        );
    }

    pub fn record_pipeline_draw_started(
        &self,
        render_queued_at: Option<Instant>,
        draw_started_at: Instant,
    ) {
        if let Some(render_queued_at) = render_queued_at {
            self.record_pipeline_render_queue(render_queued_at, draw_started_at);
        }
    }

    pub fn record_pipeline_presented(
        &self,
        submitted_at: Option<Instant>,
        swap_done_at: Instant,
        presented_at: Instant,
    ) {
        if let Some(submitted_at) = submitted_at {
            self.record_pipeline_submit_to_swap(submitted_at, swap_done_at);
            self.record_pipeline(submitted_at, presented_at);
            self.record_pipeline_swap_to_frame_callback(swap_done_at, presented_at);
        }
    }

    pub fn record_layout(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::Layout, duration);
    }

    pub fn record_refresh(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::Refresh, duration);
    }

    pub fn record_event_resolve(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::EventResolve, duration);
    }

    pub fn record_patch_tree_process(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeProcess, duration);
    }

    pub fn record_patch_tree_decode(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeDecode, duration);
    }

    pub fn record_patch_tree_apply(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeApply, duration);
    }

    pub fn record_patch_tree_animation_sync(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeAnimationSync, duration);
    }

    pub fn record_patch_tree_prepare(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreePrepare, duration);
    }

    pub fn record_patch_tree_layout(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeLayout, duration);
    }

    pub fn record_patch_tree_refresh(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeRefresh, duration);
    }

    pub fn record_patch_tree_refresh_traversal(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeRefreshTraversal, duration);
    }

    pub fn record_patch_tree_refresh_registry_post(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PatchTreeRefreshRegistryPost, duration);
    }

    pub fn record_layout_cache(&self, stats: LayoutCacheStats) {
        if !self.families.layout_cache {
            return;
        }

        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.layout_cache.add(stats);
    }

    pub fn record_renderer_cache(&self, stats: RendererCacheFrameStats) {
        if !self.families.timings {
            return;
        }

        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.renderer_cache.record(stats);
    }

    pub fn peek(&self) -> RendererStatsSnapshot {
        let now = Instant::now();
        let window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.snapshot(now)
    }

    pub fn snapshot(&self) -> RendererStatsSnapshot {
        self.take()
    }

    pub fn take(&self) -> RendererStatsSnapshot {
        let now = Instant::now();
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = window.snapshot(now);
        let last_display_interval_ns = window.last_display_interval_ns;
        let previous = std::mem::replace(
            &mut *window,
            RendererStatsWindow::new(now, last_display_interval_ns),
        );
        window
            .video_pipeline
            .copy_gauges_from(&previous.video_pipeline);
        snapshot
    }

    pub fn reset(&self) {
        let now = Instant::now();
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let last_display_interval_ns = window.last_display_interval_ns;
        let previous = std::mem::replace(
            &mut *window,
            RendererStatsWindow::new(now, last_display_interval_ns),
        );
        window
            .video_pipeline
            .copy_gauges_from(&previous.video_pipeline);
    }

    fn update_video_pipeline(&self, update: impl FnOnce(&mut VideoPipelineStatsWindow)) {
        if !self.families.timings {
            return;
        }

        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut window.video_pipeline);
    }

    #[inline]
    fn record_timing_span(&self, metric: RendererTimingMetric, start: Instant, end: Instant) {
        self.record_timing(metric, end.saturating_duration_since(start));
    }

    #[inline]
    fn record_timing(&self, metric: RendererTimingMetric, duration: Duration) {
        if !self.families.timings {
            return;
        }

        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.timings.record(metric, duration);
    }
}

pub fn format_renderer_stats_log(
    backend_label: &str,
    rendering_api_label: &str,
    snapshot: &RendererStatsSnapshot,
) -> String {
    let timing_lines = RendererTimingMetric::ALL
        .into_iter()
        .map(|metric| format_duration_stat_line(metric.log_label(), snapshot.timing(metric)))
        .collect::<Vec<_>>()
        .join("\n");
    let frame_clock_label = match backend_label {
        "wayland" => "display mode",
        "headless" => "target cadence",
        _ => "display",
    };

    let mut message = format!(
        concat!(
            "renderer stats\n",
            "  window\n",
            "    backend: {}\n",
            "    rendering_api: {}\n",
            "    duration: {} ms\n",
            "    frames: {}\n",
            "    fps: {:.1}\n",
            "    {}: {:.1} fps ({:.3} ms/frame)\n",
            "\n",
            "  timings\n",
            "{}\n",
            "\n",
            "  layout cache\n",
            "    intrinsic measure: hits={} misses={} stores={}\n",
            "    subtree measure:   hits={} misses={} stores={}\n",
            "    resolve:           hits={} misses={} stores={}"
        ),
        backend_label,
        rendering_api_label,
        snapshot.window.as_millis(),
        snapshot.frame_count,
        snapshot.fps,
        frame_clock_label,
        snapshot.display_fps,
        snapshot.display_frame_ms,
        timing_lines,
        snapshot.layout_cache.intrinsic_measure_hits,
        snapshot.layout_cache.intrinsic_measure_misses,
        snapshot.layout_cache.intrinsic_measure_stores,
        snapshot.layout_cache.subtree_measure_hits,
        snapshot.layout_cache.subtree_measure_misses,
        snapshot.layout_cache.subtree_measure_stores,
        snapshot.layout_cache.resolve_hits,
        snapshot.layout_cache.resolve_misses,
        snapshot.layout_cache.resolve_stores,
    );

    let video = snapshot.video_pipeline;
    let per_second = |count: u64| {
        if snapshot.window.is_zero() {
            0.0
        } else {
            count as f64 / snapshot.window.as_secs_f64()
        }
    };
    message.push_str(&format!(
        concat!(
            "\n\n  video pipeline\n",
            "    registry: submitted={} ({:.1}/s) inactive_dropped={} replaced_pending={} taken={} current_pending={}\n",
            "    imports: imported={} ({:.1}/s) current_direct={} retired={} max_retired={} fences={}/{}\n",
            "    acquire: received={} server_queued={} client_fallback={} timeouts={} errors={}\n",
            "    leases: released={} ({:.1}/s)\n",
            "    drm: prepared={} video_prepared={} stale={} stale_video={} no_free_gbm={}\n",
            "    present: commit_attempts={} committed={} ebusy={} presented={} ({:.1}/s) video_presented={} ({:.1}/s) prepared_now={} in_flight_now={}\n",
            "    kms: flip_events={} sequence_steps={} missed_vblanks={}"
        ),
        video.submitted,
        per_second(video.submitted),
        video.inactive_dropped,
        video.pending_replaced,
        video.pending_taken,
        video.current_pending,
        video.imported,
        per_second(video.imported),
        video.current_direct_imports,
        video.current_retired_imports,
        video.max_retired_imports,
        video.retired_fences_created,
        video.retired_fences_released,
        video.acquire_fences_received,
        video.acquire_server_waits_queued,
        video.acquire_client_wait_fallbacks,
        video.acquire_wait_timeouts,
        video.acquire_wait_errors,
        video.leases_released,
        per_second(video.leases_released),
        video.primary_prepared,
        video.video_primary_prepared,
        video.stale_prepared,
        video.stale_video_prepared,
        video.gbm_no_free,
        video.primary_commit_attempts,
        video.primary_committed,
        video.primary_commit_ebusy,
        video.primary_presented,
        per_second(video.primary_presented),
        video.video_primary_presented,
        per_second(video.video_primary_presented),
        video.current_prepared,
        video.current_in_flight,
        video.page_flip_events,
        video.page_flip_sequence_steps,
        video.missed_vblanks,
    ));

    message.push_str("\n\n  renderer cache\n");
    let paint_layer = combined_renderer_cache_snapshot(&snapshot.renderer_cache);
    message.push_str(&format_renderer_cache_kind_line(
        "paint_layer",
        &paint_layer,
        snapshot.frame_count,
    ));

    message
}

fn combined_renderer_cache_snapshot(
    stats: &RendererCacheStatsSnapshot,
) -> RendererCachePaintLayerStatsSnapshot {
    stats.paint_layer.clone()
}

fn combined_renderer_cache_frame_stats(
    stats: &RendererCacheFrameStats,
) -> RendererCachePaintLayerFrameStats {
    stats.paint_layer
}

fn format_renderer_cache_kind_line(
    label: &str,
    stats: &RendererCachePaintLayerStatsSnapshot,
    frame_count: u64,
) -> String {
    let unknown_payloads = stats.current_entries.saturating_sub(
        stats
            .current_gpu_payloads
            .saturating_add(stats.current_cpu_payloads),
    );
    let per_frame = |count: u64| {
        if frame_count == 0 {
            0.0
        } else {
            count as f64 / frame_count as f64
        }
    };
    let ratio = |numerator: u64, denominator: u64| {
        if denominator == 0 {
            0.0
        } else {
            numerator as f64 / denominator as f64
        }
    };
    let mut message = format!(
        "    {}\n      activity: candidates={} visible={} suppressed_by_parent={} bypassed_low_value={} admitted={} hits={} misses={} stores={} evictions={} stale_evictions={} rejected={}\n",
        label,
        stats.candidates,
        stats.visible_candidates,
        stats.suppressed_by_parent,
        stats.bypassed_low_value,
        stats.admitted,
        stats.hits,
        stats.misses,
        stats.stores,
        stats.evictions,
        stats.stale_evictions,
        stats.rejected,
    );
    message.push_str(&format!(
        concat!(
            "      per_frame: candidates={:.2} visible={:.2} hits={:.2} misses={:.2} stores={:.2} rejected={:.2}\n",
            "      resident: entries={} bytes={} payloads={{gpu={} cpu={} unknown={}}}\n",
            "      store_payloads: gpu={} cpu={} evicted_bytes={} stale_evicted_bytes={}\n",
            "      composition: cached_image_draws={} payload_pixels={} visible_pixels={} waste={:.2} hit_payload_pixels={} hit_visible_pixels={} store_payload_pixels={} store_visible_pixels={} hit_waste={:.2} store_waste={:.2}\n",
            "      prepare: success={} failure={} avg={:.3} ms count={}\n",
            "      fallback_after_admit={} rejections={{ineligible={} admission={} oversized={} budget={} fractional_placement={} unsupported_transform={}}}\n",
            "      hit_draw: avg={:.3} ms count={}\n"
        ),
        per_frame(stats.candidates),
        per_frame(stats.visible_candidates),
        per_frame(stats.hits),
        per_frame(stats.misses),
        per_frame(stats.stores),
        per_frame(stats.rejected),
        stats.current_entries,
        stats.current_bytes,
        stats.current_gpu_payloads,
        stats.current_cpu_payloads,
        unknown_payloads,
        stats.gpu_payload_stores,
        stats.cpu_payload_stores,
        stats.evicted_bytes,
        stats.stale_evicted_bytes,
        stats.cached_image_draws,
        stats.composited_payload_pixels,
        stats.composited_visible_pixels,
        ratio(stats.composited_payload_pixels, stats.composited_visible_pixels),
        stats.hit_payload_pixels,
        stats.hit_visible_pixels,
        stats.store_payload_pixels,
        stats.store_visible_pixels,
        ratio(stats.hit_payload_pixels, stats.hit_visible_pixels),
        ratio(stats.store_payload_pixels, stats.store_visible_pixels),
        stats.prepare_successes,
        stats.prepare_failures,
        stats.prepare.avg_ms,
        stats.prepare.count,
        stats.direct_fallbacks_after_admission,
        stats.rejected_ineligible,
        stats.rejected_admission,
        stats.rejected_oversized,
        stats.rejected_payload_budget,
        stats.rejected_fractional_placement,
        stats.rejected_unsupported_transform,
        stats.draw_hit.avg_ms,
        stats.draw_hit.count,
    ));
    message
}

pub fn format_slow_render_frame_log(
    backend_label: &str,
    timings: &RenderTimings,
    scene: RenderSceneSummary,
) -> String {
    let mut message = format!(
        concat!(
            "slow render frame\n",
            "  backend: {}\n",
            "  slow stages: {}\n",
            "  timings: render={:.3} ms draw={:.3} ms flush={:.3} ms gpu_flush={:.3} ms submit={:.3} ms\n",
            "  scene: {}"
        ),
        backend_label,
        slow_render_stage_labels(timings).join(", "),
        duration_ms(timings.total),
        duration_ms(timings.draw),
        duration_ms(timings.flush),
        duration_ms(timings.gpu_flush),
        duration_ms(timings.submit),
        scene
    );

    if let Some(detail) = timings.draw_detail.as_ref() {
        message.push('\n');
        message.push_str(&format_render_draw_detail(timings.draw, detail));
    }

    message.push('\n');
    message.push_str(&format_renderer_cache_frame_detail(
        timings.renderer_cache.as_deref(),
    ));

    message
}

fn format_renderer_cache_frame_detail(stats: Option<&RendererCacheFrameStats>) -> String {
    let Some(stats) = stats else {
        return "  renderer cache: no candidates".to_string();
    };

    let mut message = String::from("  renderer cache\n");
    let paint_layer = combined_renderer_cache_frame_stats(stats);
    message.push_str(&format_renderer_cache_kind_frame_line(
        "paint_layer",
        &paint_layer,
    ));
    message
}

fn format_renderer_cache_kind_frame_line(
    label: &str,
    stats: &RendererCachePaintLayerFrameStats,
) -> String {
    let unknown_payloads = stats.current_entries.saturating_sub(
        stats
            .current_gpu_payloads
            .saturating_add(stats.current_cpu_payloads),
    );
    let ratio = |numerator: u64, denominator: u64| {
        if denominator == 0 {
            0.0
        } else {
            numerator as f64 / denominator as f64
        }
    };
    let mut message = format!(
        "    {}\n      activity: candidates={} visible={} suppressed_by_parent={} bypassed_low_value={} admitted={} hits={} misses={} stores={} evictions={} stale_evictions={} rejected={}\n",
        label,
        stats.candidates,
        stats.visible_candidates,
        stats.suppressed_by_parent,
        stats.bypassed_low_value,
        stats.admitted,
        stats.hits,
        stats.misses,
        stats.stores,
        stats.evictions,
        stats.stale_evictions,
        stats.rejected,
    );
    message.push_str(&format!(
        concat!(
            "      resident: entries={} bytes={} payloads={{gpu={} cpu={} unknown={}}}\n",
            "      store_payloads: gpu={} cpu={} evicted_bytes={} stale_evicted_bytes={}\n",
            "      composition: cached_image_draws={} payload_pixels={} visible_pixels={} waste={:.2} hit_payload_pixels={} hit_visible_pixels={} store_payload_pixels={} store_visible_pixels={} hit_waste={:.2} store_waste={:.2}\n",
            "      prepare: success={} failure={} time={:.3} ms\n",
            "      fallback_after_admit={} rejections={{ineligible={} admission={} oversized={} budget={} fractional_placement={} unsupported_transform={}}}\n",
            "      hit_draw: time={:.3} ms\n"
        ),
        stats.current_entries,
        stats.current_bytes,
        stats.current_gpu_payloads,
        stats.current_cpu_payloads,
        unknown_payloads,
        stats.gpu_payload_stores,
        stats.cpu_payload_stores,
        stats.evicted_bytes,
        stats.stale_evicted_bytes,
        stats.cached_image_draws,
        stats.composited_payload_pixels,
        stats.composited_visible_pixels,
        ratio(stats.composited_payload_pixels, stats.composited_visible_pixels),
        stats.hit_payload_pixels,
        stats.hit_visible_pixels,
        stats.store_payload_pixels,
        stats.store_visible_pixels,
        ratio(stats.hit_payload_pixels, stats.hit_visible_pixels),
        ratio(stats.store_payload_pixels, stats.store_visible_pixels),
        stats.prepare_successes,
        stats.prepare_failures,
        duration_ms(stats.prepare_time),
        stats.direct_fallbacks_after_admission,
        stats.rejected_ineligible,
        stats.rejected_admission,
        stats.rejected_oversized,
        stats.rejected_payload_budget,
        stats.rejected_fractional_placement,
        stats.rejected_unsupported_transform,
        duration_ms(stats.draw_hit_time),
    ));
    message
}

pub fn format_slow_present_frame_log(
    backend_label: &str,
    present_submit: Duration,
    scene: RenderSceneSummary,
) -> String {
    format!(
        concat!(
            "slow present frame\n",
            "  backend: {}\n",
            "  present submit: {:.3} ms\n",
            "  scene: {}"
        ),
        backend_label,
        duration_ms(present_submit),
        scene
    )
}

pub fn render_frame_has_slow_stage(timings: &RenderTimings) -> bool {
    timings.total >= SLOW_RENDER_STAGE_THRESHOLD
        || timings.draw >= SLOW_RENDER_STAGE_THRESHOLD
        || timings.gpu_flush >= SLOW_RENDER_STAGE_THRESHOLD
        || timings.submit >= SLOW_RENDER_STAGE_THRESHOLD
}

fn slow_render_stage_labels(timings: &RenderTimings) -> Vec<&'static str> {
    [
        (timings.total >= SLOW_RENDER_STAGE_THRESHOLD, "render"),
        (timings.draw >= SLOW_RENDER_STAGE_THRESHOLD, "draw"),
        (
            timings.gpu_flush >= SLOW_RENDER_STAGE_THRESHOLD,
            "gpu_flush",
        ),
        (timings.submit >= SLOW_RENDER_STAGE_THRESHOLD, "submit"),
    ]
    .into_iter()
    .filter_map(|(slow, label)| slow.then_some(label))
    .collect()
}

fn format_render_draw_detail(draw: Duration, detail: &RenderDrawTimings) -> String {
    let mut message = format!(
        concat!(
            "  draw detail: clear={:.3} ms clips={:.3} ms relaxed_clips={:.3} ms ",
            "transforms={:.3} ms alphas={:.3} ms rects={:.3} ms rounded_rects={:.3} ms ",
            "borders={:.3} ms shadows={:.3} ms inset_shadows={:.3} ms texts={:.3} ms ",
            "gradients={:.3} ms images={:.3} ms videos={:.3} ms placeholders={:.3} ms ",
            "unattributed={:.3} ms"
        ),
        duration_ms(detail.clear),
        duration_ms(detail.clips),
        duration_ms(detail.relaxed_clips),
        duration_ms(detail.transforms),
        duration_ms(detail.alphas),
        duration_ms(detail.rects),
        duration_ms(detail.rounded_rects),
        duration_ms(detail.borders),
        duration_ms(detail.shadows),
        duration_ms(detail.inset_shadows),
        duration_ms(detail.texts),
        duration_ms(detail.gradients),
        duration_ms(detail.images),
        duration_ms(detail.videos),
        duration_ms(detail.image_placeholders),
        duration_ms(detail.unattributed(draw))
    );

    if !detail.clip_detail.is_empty() {
        message.push('\n');
        message.push_str(&format_clip_draw_detail(detail));
    }

    if !detail.border_detail.is_empty() {
        message.push('\n');
        message.push_str(&format_border_draw_detail(detail));
    }

    if !detail.layer_detail.is_empty() {
        message.push('\n');
        message.push_str(&format_layer_draw_detail(detail));
    }

    for (index, shadow) in detail.shadow_details.iter().enumerate() {
        message.push('\n');
        message.push_str(&format_shadow_draw_detail(index, shadow));
    }

    for (index, image) in detail.image_details.iter().enumerate() {
        message.push('\n');
        message.push_str(&format_image_draw_detail(index, image));
    }

    message
}

fn format_clip_draw_detail(detail: &RenderDrawTimings) -> String {
    let clip = detail.clip_detail;
    format!(
        concat!(
            "  clip detail: scopes={} relaxed_scopes={} empty_scopes={} ",
            "rect_shapes={} rounded_shapes={} shadow_escape_reapplications={}"
        ),
        clip.clip_scopes,
        clip.relaxed_clip_scopes,
        clip.empty_clip_scopes,
        clip.rect_shapes,
        clip.rounded_shapes,
        clip.shadow_escape_reapplications
    )
}

fn format_border_draw_detail(detail: &RenderDrawTimings) -> String {
    let border = detail.border_detail;
    format!(
        concat!(
            "  border detail: total={} solid={} dashed={} dotted={} uniform_width={} ",
            "asymmetric_width={} zero_radius={} rounded={} path_clip_candidates={} ",
            "max_width={:.1} max_area={:.0}"
        ),
        border.total,
        border.solid,
        border.dashed,
        border.dotted,
        border.uniform_width,
        border.asymmetric_width,
        border.zero_radius,
        border.rounded,
        border.path_clip_candidates,
        border.max_width,
        border.max_area
    )
}

fn format_layer_draw_detail(detail: &RenderDrawTimings) -> String {
    let layer = detail.layer_detail;
    format!(
        concat!(
            "  layer detail: alpha_layers={} alpha_children={} max_alpha_children={} ",
            "tint_layers={} tint_area_px={} max_tint_area_px={}"
        ),
        layer.alpha_layers,
        layer.alpha_children,
        layer.max_alpha_children,
        layer.tinted_image_layers,
        layer.tinted_image_area_px,
        layer.max_tinted_image_area_px
    )
}

fn format_shadow_draw_detail(index: usize, shadow: &RenderShadowDrawProfile) -> String {
    format!(
        concat!(
            "  shadow[{}]: path={:?} rect={:.1},{:.1} {:.1}x{:.1} offset={:.1},{:.1} ",
            "blur={:.1} size={:.1} radius={:.1} color=0x{:08X} total={:.3} ms ",
            "prepare={:.3} ms clip={:.3} ms draw={:.3} ms"
        ),
        index,
        shadow.path,
        shadow.rect_x,
        shadow.rect_y,
        shadow.rect_width,
        shadow.rect_height,
        shadow.offset_x,
        shadow.offset_y,
        shadow.blur,
        shadow.size,
        shadow.radius,
        shadow.color,
        duration_ms(shadow.total),
        duration_ms(shadow.prepare),
        duration_ms(shadow.clip),
        duration_ms(shadow.draw)
    )
}

fn format_image_draw_detail(index: usize, image: &RenderImageDrawProfile) -> String {
    format!(
        concat!(
            "  image[{}]: id={} kind={:?} fit={:?} tint={} tint_layer={} source={}x{} draw={}x{} ",
            "total={:.3} ms lookup={:.3} ms fit={:.3} ms vector_cache_lookup={:.3} ms ",
            "vector_cache_hit={} vector_rasterize={:.3} ms vector_cache_store={:.3} ms ",
            "draw={:.3} ms"
        ),
        index,
        image.image_id,
        image.kind,
        image.fit,
        image.tinted,
        image.tint_layer_used,
        image.source_width,
        image.source_height,
        image.draw_width,
        image.draw_height,
        duration_ms(image.total),
        duration_ms(image.asset_lookup),
        duration_ms(image.fit_compute),
        duration_ms(image.vector_cache_lookup),
        image
            .vector_cache_hit
            .map_or("n/a".to_string(), |hit| hit.to_string()),
        duration_ms(image.vector_rasterize),
        duration_ms(image.vector_cache_store),
        duration_ms(image.draw)
    )
}

fn format_duration_stat_line(label: &str, stats: &DurationStatsSnapshot) -> String {
    if stats.count == 0 {
        format!("    {label}: no samples (count=0)")
    } else {
        format!(
            "    {label}: avg={:.3} ms min={:.3} ms max={:.3} ms count={}",
            stats.avg_ms, stats.min_ms, stats.max_ms, stats.count
        )
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::{
        LayoutCacheStats, RendererStatsCollector, RendererTimingMetric, format_renderer_stats_log,
        format_slow_present_frame_log, format_slow_render_frame_log, record_pipeline_layout_queued,
        render_frame_has_slow_stage,
    };
    use crate::{
        render_scene::{DrawPrimitive, RenderNode, RenderScene},
        renderer::{
            RenderBorderDrawSummary, RenderClipDrawSummary, RenderDrawTimings,
            RenderImageAssetKind, RenderImageDrawProfile, RenderLayerDrawSummary,
            RenderShadowDrawPath, RenderShadowDrawProfile, RenderTimings, RendererCacheFrameStats,
            RendererCachePaintLayerFrameStats,
        },
    };
    use std::time::{Duration, Instant};

    #[test]
    fn snapshot_tracks_avg_min_max_and_resets_window() {
        let stats = RendererStatsCollector::new();

        stats.record_frame_present();
        stats.record_display_interval(Duration::from_millis(16));
        stats.record_frame_present();
        stats.record_render(Duration::from_millis(4));
        stats.record_render_draw(Duration::from_millis(3));
        stats.record_render_flush(Duration::from_millis(1));
        stats.record_render_gpu_flush(Duration::from_millis(1));
        stats.record_render_submit(Duration::from_millis(0));
        stats.record_present_submit(Duration::from_millis(1));
        let pipeline_submitted_at = Instant::now();
        stats.record_pipeline(
            pipeline_submitted_at,
            pipeline_submitted_at + Duration::from_millis(13),
        );
        stats.record_pipeline_submit_to_tree_start(
            pipeline_submitted_at,
            pipeline_submitted_at + Duration::from_millis(1),
        );
        stats.record_pipeline_tree(
            pipeline_submitted_at + Duration::from_millis(1),
            pipeline_submitted_at + Duration::from_millis(5),
        );
        stats.record_pipeline_render_queue(
            pipeline_submitted_at + Duration::from_millis(5),
            pipeline_submitted_at + Duration::from_millis(7),
        );
        stats.record_pipeline_submit_to_swap(
            pipeline_submitted_at,
            pipeline_submitted_at + Duration::from_millis(9),
        );
        stats.record_pipeline_swap_to_frame_callback(
            pipeline_submitted_at + Duration::from_millis(9),
            pipeline_submitted_at + Duration::from_millis(13),
        );
        stats.record_layout(Duration::from_millis(2));
        stats.record_layout(Duration::from_millis(6));
        stats.record_refresh(Duration::from_millis(1));
        stats.record_refresh(Duration::from_millis(3));
        stats.record_event_resolve(Duration::from_millis(1));
        stats.record_patch_tree_process(Duration::from_millis(9));
        stats.record_renderer_cache(RendererCacheFrameStats {
            paint_layer: RendererCachePaintLayerFrameStats {
                candidates: 6,
                visible_candidates: 5,
                admitted: 2,
                hits: 1,
                misses: 1,
                stores: 2,
                evictions: 1,
                rejected: 1,
                current_entries: 2,
                current_bytes: 640,
                current_gpu_payloads: 1,
                current_cpu_payloads: 1,
                evicted_bytes: 128,
                gpu_payload_stores: 1,
                cpu_payload_stores: 1,
                prepare_successes: 2,
                rejected_ineligible: 1,
                prepare_time: Duration::from_micros(50),
                draw_hit_time: Duration::from_micros(10),
                ..RendererCachePaintLayerFrameStats::default()
            },
        });
        stats.record_layout_cache(LayoutCacheStats {
            resolve_hits: 5,
            subtree_measure_hits: 3,
            ..LayoutCacheStats::default()
        });

        let peek_snapshot = stats.peek();
        assert_eq!(peek_snapshot.frame_count, 2);
        assert_eq!(peek_snapshot.layout_cache.resolve_hits, 5);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.frame_count, 2);
        assert_eq!(snapshot.display_frame_ms, 16.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::Render).count, 1);
        assert_eq!(snapshot.timing(RendererTimingMetric::Render).avg_ms, 4.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::RenderDraw).count, 1);
        assert_eq!(
            snapshot.timing(RendererTimingMetric::RenderDraw).avg_ms,
            3.0
        );
        assert_eq!(snapshot.timing(RendererTimingMetric::RenderFlush).count, 1);
        assert_eq!(
            snapshot.timing(RendererTimingMetric::RenderFlush).avg_ms,
            1.0
        );
        assert_eq!(
            snapshot.timing(RendererTimingMetric::RenderGpuFlush).count,
            1
        );
        assert_eq!(
            snapshot.timing(RendererTimingMetric::RenderGpuFlush).avg_ms,
            1.0
        );
        assert_eq!(snapshot.timing(RendererTimingMetric::RenderSubmit).count, 1);
        assert_eq!(
            snapshot.timing(RendererTimingMetric::RenderSubmit).avg_ms,
            0.0
        );
        assert_eq!(
            snapshot.timing(RendererTimingMetric::PresentSubmit).count,
            1
        );
        assert_eq!(
            snapshot.timing(RendererTimingMetric::PresentSubmit).avg_ms,
            1.0
        );
        assert_eq!(snapshot.timing(RendererTimingMetric::Pipeline).count, 1);
        assert_eq!(snapshot.timing(RendererTimingMetric::Pipeline).avg_ms, 13.0);
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineSubmitToTreeStart)
                .count,
            1
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineSubmitToTreeStart)
                .avg_ms,
            1.0
        );
        assert_eq!(snapshot.timing(RendererTimingMetric::PipelineTree).count, 1);
        assert_eq!(
            snapshot.timing(RendererTimingMetric::PipelineTree).avg_ms,
            4.0
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineRenderQueue)
                .count,
            1
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineRenderQueue)
                .avg_ms,
            2.0
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineSubmitToSwap)
                .count,
            1
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineSubmitToSwap)
                .avg_ms,
            9.0
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineSwapToFrameCallback)
                .count,
            1
        );
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PipelineSwapToFrameCallback)
                .avg_ms,
            4.0
        );
        assert_eq!(snapshot.timing(RendererTimingMetric::Layout).count, 2);
        assert_eq!(snapshot.timing(RendererTimingMetric::Layout).min_ms, 2.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::Layout).max_ms, 6.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::Layout).avg_ms, 4.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::Refresh).count, 2);
        assert_eq!(snapshot.timing(RendererTimingMetric::Refresh).min_ms, 1.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::Refresh).max_ms, 3.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::Refresh).avg_ms, 2.0);
        assert_eq!(snapshot.timing(RendererTimingMetric::EventResolve).count, 1);
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::PatchTreeProcess)
                .count,
            1
        );
        assert_eq!(snapshot.layout_cache.resolve_hits, 5);
        assert_eq!(snapshot.layout_cache.subtree_measure_hits, 3);
        assert_eq!(snapshot.renderer_cache.paint_layer.candidates, 6);
        assert_eq!(snapshot.renderer_cache.paint_layer.visible_candidates, 5);
        assert_eq!(snapshot.renderer_cache.paint_layer.current_entries, 2);
        assert_eq!(snapshot.renderer_cache.paint_layer.current_bytes, 640);
        assert_eq!(snapshot.renderer_cache.paint_layer.current_gpu_payloads, 1);
        assert_eq!(snapshot.renderer_cache.paint_layer.current_cpu_payloads, 1);
        assert_eq!(snapshot.renderer_cache.paint_layer.evicted_bytes, 128);
        assert_eq!(snapshot.renderer_cache.paint_layer.gpu_payload_stores, 1);
        assert_eq!(snapshot.renderer_cache.paint_layer.cpu_payload_stores, 1);
        assert_eq!(snapshot.renderer_cache.paint_layer.prepare_successes, 2);
        assert_eq!(snapshot.renderer_cache.paint_layer.prepare.count, 2);
        assert_eq!(snapshot.renderer_cache.paint_layer.prepare.avg_ms, 0.025);
        assert_eq!(snapshot.renderer_cache.paint_layer.draw_hit.count, 1);
        assert_eq!(snapshot.renderer_cache.paint_layer.draw_hit.avg_ms, 0.01);
        let reset_snapshot = stats.snapshot();
        assert_eq!(reset_snapshot.frame_count, 0);
        assert_eq!(reset_snapshot.display_frame_ms, 16.0);
        assert_eq!(reset_snapshot.timing(RendererTimingMetric::Render).count, 0);
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::RenderDraw)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::RenderFlush)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::RenderGpuFlush)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::RenderSubmit)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PresentSubmit)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot.timing(RendererTimingMetric::Pipeline).count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PipelineSubmitToTreeStart)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PipelineTree)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PipelineRenderQueue)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PipelineSubmitToSwap)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PipelineSwapToFrameCallback)
                .count,
            0
        );
        assert_eq!(reset_snapshot.timing(RendererTimingMetric::Layout).count, 0);
        assert_eq!(
            reset_snapshot.timing(RendererTimingMetric::Refresh).count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::EventResolve)
                .count,
            0
        );
        assert_eq!(
            reset_snapshot
                .timing(RendererTimingMetric::PatchTreeProcess)
                .count,
            0
        );
        assert_eq!(reset_snapshot.layout_cache.resolve_hits, 0);
        assert_eq!(reset_snapshot.renderer_cache.paint_layer.candidates, 0);
        assert_eq!(reset_snapshot.renderer_cache.paint_layer.candidates, 0);
    }

    #[test]
    fn pipeline_helpers_record_layout_draw_and_present_spans() {
        let stats = RendererStatsCollector::new();
        let submitted_at = Instant::now();
        let tree_started_at = submitted_at + Duration::from_millis(2);
        let render_queued_at = submitted_at + Duration::from_millis(8);
        let draw_started_at = submitted_at + Duration::from_millis(10);
        let swap_done_at = submitted_at + Duration::from_millis(11);
        let presented_at = submitted_at + Duration::from_millis(18);

        let timing = record_pipeline_layout_queued(
            Some(&stats),
            None,
            None,
            Some(submitted_at),
            Some(tree_started_at),
            render_queued_at,
        );
        assert_eq!(timing.pipeline_submitted_at, Some(submitted_at));
        assert_eq!(timing.pipeline_render_queued_at, Some(render_queued_at));

        let retained = record_pipeline_layout_queued(
            None,
            timing.pipeline_submitted_at,
            timing.pipeline_render_queued_at,
            None,
            None,
            submitted_at + Duration::from_millis(20),
        );
        assert_eq!(retained.pipeline_submitted_at, Some(submitted_at));
        assert_eq!(retained.pipeline_render_queued_at, Some(render_queued_at));

        stats.record_pipeline_draw_started(timing.pipeline_render_queued_at, draw_started_at);
        stats.record_pipeline_presented(timing.pipeline_submitted_at, swap_done_at, presented_at);

        let snapshot = stats.snapshot();
        assert_timing(&snapshot, RendererTimingMetric::PipelineTree, 6.0);
        assert_timing(&snapshot, RendererTimingMetric::PipelineRenderQueue, 2.0);
        assert_timing(&snapshot, RendererTimingMetric::PipelineSubmitToSwap, 11.0);
        assert_timing(&snapshot, RendererTimingMetric::Pipeline, 18.0);
        assert_timing(
            &snapshot,
            RendererTimingMetric::PipelineSwapToFrameCallback,
            7.0,
        );
    }

    fn assert_timing(
        snapshot: &super::RendererStatsSnapshot,
        metric: RendererTimingMetric,
        expected_avg_ms: f64,
    ) {
        let timing = snapshot.timing(metric);
        assert_eq!(timing.count, 1);
        assert!((timing.avg_ms - expected_avg_ms).abs() < 0.001);
    }

    #[test]
    fn log_format_includes_all_stats_fields() {
        let stats = RendererStatsCollector::new();
        stats.record_frame_present();
        stats.record_display_interval(Duration::from_millis(16));
        stats.record_render(Duration::from_millis(3));
        stats.record_render_draw(Duration::from_millis(2));
        stats.record_render_flush(Duration::from_millis(1));
        stats.record_render_gpu_flush(Duration::from_millis(1));
        stats.record_render_submit(Duration::from_millis(0));
        stats.record_present_submit(Duration::from_millis(1));
        stats.record_headless_prime_timings(
            Duration::from_millis(1),
            Duration::from_millis(2),
            Some(Duration::from_millis(3)),
            Some(Duration::from_millis(4)),
            Duration::from_millis(5),
        );
        let pipeline_submitted_at = Instant::now();
        stats.record_pipeline(
            pipeline_submitted_at,
            pipeline_submitted_at + Duration::from_millis(18),
        );
        stats.record_pipeline_submit_to_tree_start(
            pipeline_submitted_at,
            pipeline_submitted_at + Duration::from_millis(2),
        );
        stats.record_pipeline_tree(
            pipeline_submitted_at + Duration::from_millis(2),
            pipeline_submitted_at + Duration::from_millis(8),
        );
        stats.record_pipeline_render_queue(
            pipeline_submitted_at + Duration::from_millis(8),
            pipeline_submitted_at + Duration::from_millis(10),
        );
        stats.record_pipeline_submit_to_swap(
            pipeline_submitted_at,
            pipeline_submitted_at + Duration::from_millis(11),
        );
        stats.record_pipeline_swap_to_frame_callback(
            pipeline_submitted_at + Duration::from_millis(11),
            pipeline_submitted_at + Duration::from_millis(18),
        );
        stats.record_video_submitted(false);
        stats.record_video_pending_taken(1);
        stats.record_video_imported(Duration::from_millis(2));
        stats.record_video_acquire_fence_received();
        stats.record_video_acquire_server_wait_queued();
        stats.record_video_acquire_client_wait_fallback();
        stats.record_video_acquire_wait_timeout();
        stats.record_video_acquire_wait_error();
        stats.record_video_retired_fence_created(1);
        stats.record_video_retired_fence_released(Duration::from_millis(4));
        stats.record_video_lease_released(Duration::from_millis(12));
        stats.set_video_import_gauges(1, 0);
        stats.record_drm_forced_gpu_finish_before_swap(Duration::from_millis(5));
        stats.record_drm_forced_gpu_finish_after_swap(Duration::from_millis(1));
        stats.record_drm_gpu_queue_completion(Duration::from_millis(9));
        stats.record_drm_egl_swap_buffers(Duration::from_millis(2));
        stats.record_drm_gbm_lock_front_buffer(Duration::from_millis(3));
        stats.record_drm_framebuffer_lookup(Duration::from_millis(1));
        stats.record_drm_prepared_to_commit(Duration::from_millis(4));
        stats.record_drm_previous_flip_to_commit(Duration::from_millis(6));
        stats.record_drm_atomic_commit_ioctl(Duration::from_millis(1));
        stats.record_drm_commit_to_kernel_page_flip(Duration::from_millis(14));
        stats.record_drm_kernel_page_flip_interval(Duration::from_millis(20));
        stats.record_drm_page_flip_dispatch_delay(Duration::from_millis(2));
        stats.record_drm_page_flip_sequence(None);
        stats.record_drm_page_flip_sequence(Some(2));
        stats.record_drm_primary_prepared(true);
        stats.record_drm_primary_commit_attempt();
        stats.record_drm_primary_commit_ebusy();
        stats.record_drm_primary_commit_attempt();
        stats.record_drm_primary_committed();
        stats.record_drm_primary_presented(
            true,
            Duration::from_millis(17),
            Some(Duration::from_millis(19)),
        );
        stats.record_layout(Duration::from_millis(3));
        stats.record_refresh(Duration::from_millis(1));
        stats.record_event_resolve(Duration::from_millis(2));
        stats.record_patch_tree_process(Duration::from_millis(7));
        stats.record_renderer_cache(RendererCacheFrameStats {
            paint_layer: RendererCachePaintLayerFrameStats {
                candidates: 8,
                visible_candidates: 7,
                admitted: 3,
                hits: 1,
                misses: 2,
                stores: 2,
                evictions: 1,
                rejected: 2,
                current_entries: 2,
                current_bytes: 768,
                current_gpu_payloads: 1,
                current_cpu_payloads: 1,
                evicted_bytes: 128,
                gpu_payload_stores: 1,
                cpu_payload_stores: 1,
                prepare_successes: 2,
                direct_fallbacks_after_admission: 1,
                rejected_ineligible: 1,
                rejected_payload_budget: 1,
                prepare_time: Duration::from_micros(90),
                draw_hit_time: Duration::from_micros(12),
                ..RendererCachePaintLayerFrameStats::default()
            },
        });
        stats.record_layout_cache(LayoutCacheStats {
            resolve_hits: 11,
            ..LayoutCacheStats::default()
        });

        let message = format_renderer_stats_log("wayland", "auto (opengl)", &stats.snapshot());

        assert!(message.starts_with("renderer stats\n"));
        assert!(message.contains("  window\n"));
        assert!(message.contains("    backend: wayland\n"));
        assert!(message.contains("    rendering_api: auto (opengl)\n"));
        assert!(message.contains("    frames: 1\n"));
        assert!(message.contains("    fps: "));
        assert!(message.contains("    display mode: "));
        assert!(message.contains("  timings\n"));
        assert!(message.contains("    render: avg=3.000 ms min=3.000 ms max=3.000 ms count=1"));
        assert!(message.contains("    render draw: avg=2.000 ms"));
        assert!(message.contains("    render flush: avg=1.000 ms"));
        assert!(message.contains("    render gpu flush: avg=1.000 ms"));
        assert!(message.contains("    render submit: avg=0.000 ms"));
        assert!(message.contains("    present submit: avg=1.000 ms"));
        assert!(message.contains("    headless PRIME prepare: avg=1.000 ms"));
        assert!(message.contains("    headless PRIME retarget: avg=2.000 ms"));
        assert!(message.contains("    headless PRIME fence export: avg=3.000 ms"));
        assert!(message.contains("    headless PRIME GPU finish fallback: avg=4.000 ms"));
        assert!(message.contains("    headless PRIME export metadata: avg=5.000 ms"));
        assert!(message.contains("    pipeline submit->frame callback: avg=18.000 ms"));
        assert!(message.contains("    pipeline submit->tree: avg=2.000 ms"));
        assert!(message.contains("    pipeline tree: avg=6.000 ms"));
        assert!(message.contains("    pipeline render queue: avg=2.000 ms"));
        assert!(message.contains("    pipeline submit->swap: avg=11.000 ms"));
        assert!(message.contains("    pipeline swap->frame callback: avg=7.000 ms"));
        assert!(message.contains("    video submit->import: avg=2.000 ms"));
        assert!(message.contains("    video submit->lease release: avg=12.000 ms"));
        assert!(message.contains("    video retired fence: avg=4.000 ms"));
        assert!(message.contains("    video submit->page flip: avg=19.000 ms"));
        assert!(message.contains("    drm forced GPU finish before swap: avg=5.000 ms"));
        assert!(message.contains("    drm forced GPU finish after swap: avg=1.000 ms"));
        assert!(message.contains("    drm GPU queue completion span: avg=9.000 ms"));
        assert!(message.contains("    drm eglSwapBuffers: avg=2.000 ms"));
        assert!(message.contains("    drm GBM lock front buffer: avg=3.000 ms"));
        assert!(message.contains("    drm framebuffer lookup: avg=1.000 ms"));
        assert!(message.contains("    drm prepared->atomic commit: avg=4.000 ms"));
        assert!(message.contains("    drm previous kernel flip->next commit: avg=6.000 ms"));
        assert!(message.contains("    drm atomic commit ioctl: avg=1.000 ms"));
        assert!(message.contains("    drm atomic commit->kernel page flip: avg=14.000 ms"));
        assert!(message.contains("    drm kernel page flip interval: avg=20.000 ms"));
        assert!(message.contains("    drm kernel page flip->event dispatch: avg=2.000 ms"));
        assert!(message.contains("    drm atomic commit->event processed: avg=17.000 ms"));
        assert!(message.contains("    layout: avg=3.000 ms"));
        assert!(message.contains("    refresh: avg=1.000 ms"));
        assert!(message.contains("    event resolve: avg=2.000 ms"));
        assert!(message.contains("    patch tree actor: avg=7.000 ms"));
        assert!(message.contains("  layout cache\n"));
        assert!(message.contains("    intrinsic measure: hits=0 misses=0 stores=0"));
        assert!(message.contains("    subtree measure:   hits=0 misses=0 stores=0"));
        assert!(message.contains("    resolve:           hits=11 misses=0 stores=0"));
        assert!(message.contains("  video pipeline\n"));
        assert!(message.contains("registry: submitted=1"));
        assert!(message.contains("imports: imported=1"));
        assert!(
            message.contains(
                "acquire: received=1 server_queued=1 client_fallback=1 timeouts=1 errors=1"
            )
        );
        assert!(message.contains("leases: released=1"));
        assert!(message.contains("drm: prepared=1 video_prepared=1"));
        assert!(message.contains("present: commit_attempts=2 committed=1 ebusy=1 presented=1"));
        assert!(message.contains("kms: flip_events=2 sequence_steps=2 missed_vblanks=1"));
        assert!(message.contains("  renderer cache\n"));
        assert!(message.contains("    paint_layer\n"));
        assert!(message.contains(
            "activity: candidates=8 visible=7 suppressed_by_parent=0 bypassed_low_value=0 admitted=3 hits=1 misses=2 stores=2 evictions=1 stale_evictions=0 rejected=2"
        ));
        assert!(!message.contains("layers: selected="));
        assert!(!message.contains("      layer_groups:\n"));
        assert!(
            message.contains("per_frame: candidates=8.00 visible=7.00 hits=1.00 misses=2.00 stores=2.00 rejected=2.00")
        );
        assert!(message.contains("resident: entries=2 bytes=768 payloads={gpu=1 cpu=1 unknown=0}"));
        assert!(
            message.contains("store_payloads: gpu=1 cpu=1 evicted_bytes=128 stale_evicted_bytes=0")
        );
        assert!(message.contains("prepare: success=2 failure=0 avg=0.045 ms count=2"));
        assert!(message.contains("fallback_after_admit=1 rejections="));
        assert!(message.contains("hit_draw: avg=0.012 ms count=1"));
        assert!(!message.contains("    shell\n"));
        assert!(!message.contains("    moving_paint_layer\n"));
    }

    #[test]
    fn log_format_includes_empty_paint_layer_renderer_cache() {
        let stats = RendererStatsCollector::new();
        stats.record_frame_present();

        let message = format_renderer_stats_log("wayland", "auto (opengl)", &stats.snapshot());

        assert!(message.contains("  renderer cache\n"));
        assert!(message.contains("    paint_layer\n"));
        assert!(
            message
                .contains("activity: candidates=0 visible=0 suppressed_by_parent=0 bypassed_low_value=0 admitted=0 hits=0 misses=0 stores=0")
        );
        assert!(!message.contains("    shell\n"));
        assert!(!message.contains("    moving_paint_layer\n"));
    }

    #[test]
    fn slow_render_frame_log_includes_timing_split_and_scene_summary() {
        let scene = RenderScene {
            nodes: vec![
                RenderNode::Clip {
                    clips: Vec::new(),
                    children: vec![RenderNode::Primitive(DrawPrimitive::TextWithFont(
                        0.0,
                        0.0,
                        "slow".to_string(),
                        14.0,
                        0xFFFFFFFF,
                        "default".to_string(),
                        400,
                        false,
                    ))],
                },
                RenderNode::Primitive(DrawPrimitive::Shadow(
                    0.0, 0.0, 10.0, 10.0, 0.0, 1.0, 8.0, 0.0, 4.0, 0x00000080,
                )),
            ],
        };
        let timings = RenderTimings {
            total: Duration::from_micros(10_250),
            draw: Duration::from_micros(750),
            draw_detail: Some(RenderDrawTimings {
                clear: Duration::from_micros(100),
                shadows: Duration::from_micros(100),
                texts: Duration::from_micros(200),
                clip_detail: RenderClipDrawSummary {
                    clip_scopes: 1,
                    rect_shapes: 1,
                    ..RenderClipDrawSummary::default()
                },
                border_detail: RenderBorderDrawSummary {
                    total: 2,
                    solid: 1,
                    dashed: 1,
                    uniform_width: 1,
                    asymmetric_width: 1,
                    rounded: 1,
                    path_clip_candidates: 2,
                    max_width: 3.0,
                    max_area: 120.0,
                    ..RenderBorderDrawSummary::default()
                },
                layer_detail: RenderLayerDrawSummary {
                    alpha_layers: 1,
                    alpha_children: 2,
                    max_alpha_children: 2,
                    tinted_image_layers: 1,
                    tinted_image_area_px: 2_304,
                    max_tinted_image_area_px: 2_304,
                },
                shadow_details: vec![RenderShadowDrawProfile {
                    path: RenderShadowDrawPath::MaskFilter,
                    rect_x: 0.0,
                    rect_y: 0.0,
                    rect_width: 10.0,
                    rect_height: 10.0,
                    offset_x: 0.0,
                    offset_y: 1.0,
                    blur: 8.0,
                    size: 0.0,
                    radius: 4.0,
                    color: 0x00000080,
                    total: Duration::from_micros(100),
                    prepare: Duration::from_micros(10),
                    clip: Duration::from_micros(20),
                    draw: Duration::from_micros(70),
                }],
                image_details: vec![RenderImageDrawProfile {
                    image_id: "asset-1".to_string(),
                    kind: RenderImageAssetKind::Vector,
                    fit: crate::tree::attrs::ImageFit::Contain,
                    tinted: true,
                    tint_layer_used: true,
                    source_width: 24,
                    source_height: 24,
                    draw_width: 48,
                    draw_height: 48,
                    total: Duration::from_micros(250),
                    asset_lookup: Duration::from_micros(10),
                    fit_compute: Duration::from_micros(5),
                    vector_cache_lookup: Duration::from_micros(15),
                    vector_cache_hit: Some(false),
                    vector_rasterize: Duration::from_micros(200),
                    vector_cache_store: Duration::from_micros(5),
                    draw: Duration::from_micros(15),
                }],
                ..RenderDrawTimings::default()
            }),
            flush: Duration::from_micros(9_500),
            gpu_flush: Duration::from_micros(9_250),
            submit: Duration::from_micros(250),
            renderer_cache: None,
        };

        assert!(render_frame_has_slow_stage(&timings));

        let message = format_slow_render_frame_log("wayland", &timings, scene.summary());

        assert!(message.starts_with("slow render frame\n"));
        assert!(message.contains("  backend: wayland\n"));
        assert!(message.contains("  slow stages: render, gpu_flush\n"));
        assert!(message.contains(
            "  timings: render=10.250 ms draw=0.750 ms flush=9.500 ms gpu_flush=9.250 ms submit=0.250 ms\n"
        ));
        assert!(message.contains("nodes=3 primitives=2"));
        assert!(message.contains("clips=1"));
        assert!(message.contains("shadows=1"));
        assert!(message.contains("texts=1"));
        assert!(message.contains("text_bytes=4"));
        assert!(message.contains("draw detail: clear=0.100 ms"));
        assert!(message.contains("shadows=0.100 ms"));
        assert!(message.contains("texts=0.200 ms"));
        assert!(message.contains("unattributed=0.350 ms"));
        assert!(message.contains(
            "clip detail: scopes=1 relaxed_scopes=0 empty_scopes=0 rect_shapes=1 rounded_shapes=0 shadow_escape_reapplications=0"
        ));
        assert!(message.contains(
            "border detail: total=2 solid=1 dashed=1 dotted=0 uniform_width=1 asymmetric_width=1 zero_radius=0 rounded=1 path_clip_candidates=2"
        ));
        assert!(message.contains(
            "layer detail: alpha_layers=1 alpha_children=2 max_alpha_children=2 tint_layers=1 tint_area_px=2304 max_tint_area_px=2304"
        ));
        assert!(message.contains(
            "shadow[0]: path=MaskFilter rect=0.0,0.0 10.0x10.0 offset=0.0,1.0 blur=8.0 size=0.0 radius=4.0 color=0x00000080"
        ));
        assert!(message.contains("prepare=0.010 ms clip=0.020 ms draw=0.070 ms"));
        assert!(message.contains(
            "image[0]: id=asset-1 kind=Vector fit=Contain tint=true tint_layer=true source=24x24 draw=48x48"
        ));
        assert!(message.contains("vector_cache_hit=false"));
        assert!(message.contains("vector_rasterize=0.200 ms"));
        assert!(message.contains("renderer cache: no candidates"));
    }

    #[test]
    fn slow_render_frame_log_includes_renderer_cache_frame_stats() {
        let scene = RenderScene {
            nodes: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                0.0, 0.0, 10.0, 10.0, 0xFFFFFFFF,
            ))],
        };
        let timings = RenderTimings {
            total: Duration::from_micros(5_000),
            draw: Duration::from_micros(500),
            flush: Duration::from_micros(4_500),
            gpu_flush: Duration::from_micros(4_400),
            submit: Duration::from_micros(100),
            renderer_cache: Some(Box::new(RendererCacheFrameStats {
                paint_layer: RendererCachePaintLayerFrameStats {
                    candidates: 1,
                    visible_candidates: 1,
                    admitted: 1,
                    hits: 1,
                    current_entries: 1,
                    current_bytes: 4096,
                    current_gpu_payloads: 1,
                    draw_hit_time: Duration::from_micros(9),
                    ..RendererCachePaintLayerFrameStats::default()
                },
            })),
            ..RenderTimings::default()
        };

        let message = format_slow_render_frame_log("wayland", &timings, scene.summary());

        assert!(message.contains("  renderer cache\n"));
        assert!(message.contains("    paint_layer\n"));
        assert!(
            message
                .contains("activity: candidates=1 visible=1 suppressed_by_parent=0 bypassed_low_value=0 admitted=1 hits=1 misses=0 stores=0")
        );
        assert!(
            message.contains("resident: entries=1 bytes=4096 payloads={gpu=1 cpu=0 unknown=0}")
        );
        assert!(message.contains("hit_draw: time=0.009 ms"));
    }

    #[test]
    fn slow_present_frame_log_includes_present_duration_and_scene_summary() {
        let scene = RenderScene {
            nodes: vec![RenderNode::Primitive(DrawPrimitive::Rect(
                0.0, 0.0, 10.0, 10.0, 0xFFFFFFFF,
            ))],
        };

        let message =
            format_slow_present_frame_log("wayland", Duration::from_micros(8_250), scene.summary());

        assert!(message.starts_with("slow present frame\n"));
        assert!(message.contains("  backend: wayland\n"));
        assert!(message.contains("  present submit: 8.250 ms\n"));
        assert!(message.contains("nodes=1 primitives=1"));
        assert!(message.contains("rects=1"));
    }
}
