use std::{
    ops::Index,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{
    render_scene::RenderSceneSummary,
    renderer::{
        RenderDrawTimings, RenderImageDrawProfile, RenderShadowDrawProfile, RenderTimings,
        RendererCacheFrameStats, RendererCacheKindFrameStats,
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
    Pipeline,
    PipelineSubmitToTreeStart,
    PipelineTree,
    PipelineRenderQueue,
    PipelineSubmitToSwap,
    PipelineSwapToFrameCallback,
    Layout,
    Refresh,
    EventResolve,
    PatchTreeProcess,
}

impl RendererTimingMetric {
    pub const COUNT: usize = 16;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Render,
        Self::RenderDraw,
        Self::RenderFlush,
        Self::RenderGpuFlush,
        Self::RenderSubmit,
        Self::PresentSubmit,
        Self::Pipeline,
        Self::PipelineSubmitToTreeStart,
        Self::PipelineTree,
        Self::PipelineRenderQueue,
        Self::PipelineSubmitToSwap,
        Self::PipelineSwapToFrameCallback,
        Self::Layout,
        Self::Refresh,
        Self::EventResolve,
        Self::PatchTreeProcess,
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
            Self::Pipeline => "pipeline submit->frame callback",
            Self::PipelineSubmitToTreeStart => "pipeline submit->tree",
            Self::PipelineTree => "pipeline tree",
            Self::PipelineRenderQueue => "pipeline render queue",
            Self::PipelineSubmitToSwap => "pipeline submit->swap",
            Self::PipelineSwapToFrameCallback => "pipeline swap->frame callback",
            Self::Layout => "layout",
            Self::Refresh => "refresh",
            Self::EventResolve => "event resolve",
            Self::PatchTreeProcess => "patch tree actor",
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
    pub layout_cache: LayoutCacheStats,
    pub renderer_cache: RendererCacheStatsSnapshot,
}

impl RendererStatsSnapshot {
    pub fn timing(&self, metric: RendererTimingMetric) -> &DurationStatsSnapshot {
        &self.timings[metric]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererCacheStatsSnapshot {
    pub noop: RendererCacheKindStatsSnapshot,
    pub clean_subtree: RendererCacheKindStatsSnapshot,
}

impl RendererCacheStatsSnapshot {
    pub fn is_empty(&self) -> bool {
        self.noop.is_empty() && self.clean_subtree.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererCacheKindStatsSnapshot {
    pub candidates: u64,
    pub visible_candidates: u64,
    pub suppressed_by_parent: u64,
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
    pub prepare_successes: u64,
    pub prepare_failures: u64,
    pub direct_fallbacks_after_admission: u64,
    pub rejected_ineligible: u64,
    pub rejected_admission: u64,
    pub rejected_oversized: u64,
    pub rejected_payload_budget: u64,
    pub prepare: DurationStatsSnapshot,
    pub draw_hit: DurationStatsSnapshot,
}

impl RendererCacheKindStatsSnapshot {
    pub fn is_empty(&self) -> bool {
        self.candidates == 0
            && self.visible_candidates == 0
            && self.suppressed_by_parent == 0
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
            && self.prepare_successes == 0
            && self.prepare_failures == 0
            && self.direct_fallbacks_after_admission == 0
            && self.rejected_ineligible == 0
            && self.rejected_admission == 0
            && self.rejected_oversized == 0
            && self.rejected_payload_budget == 0
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

struct RendererStatsWindow {
    started_at: Instant,
    last_display_interval_ns: Option<u64>,
    frame_count: u64,
    timings: RendererTimingWindows,
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
            layout_cache: self.layout_cache,
            renderer_cache: self.renderer_cache.snapshot(),
        }
    }
}

#[derive(Default)]
struct RendererCacheStatsWindow {
    noop: RendererCacheKindStatsWindow,
    clean_subtree: RendererCacheKindStatsWindow,
}

impl RendererCacheStatsWindow {
    fn record(&mut self, stats: RendererCacheFrameStats) {
        self.noop.record(stats.noop);
        self.clean_subtree.record(stats.clean_subtree);
    }

    fn snapshot(&self) -> RendererCacheStatsSnapshot {
        RendererCacheStatsSnapshot {
            noop: self.noop.snapshot(),
            clean_subtree: self.clean_subtree.snapshot(),
        }
    }
}

#[derive(Default)]
struct RendererCacheKindStatsWindow {
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
    prepare: DurationStatsWindow,
    draw_hit: DurationStatsWindow,
}

impl RendererCacheKindStatsWindow {
    fn record(&mut self, stats: RendererCacheKindFrameStats) {
        self.candidates = self.candidates.saturating_add(stats.candidates);
        self.visible_candidates = self
            .visible_candidates
            .saturating_add(stats.visible_candidates);
        self.suppressed_by_parent = self
            .suppressed_by_parent
            .saturating_add(stats.suppressed_by_parent);
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

        if stats.stores > 0 {
            self.prepare.record(stats.prepare_time);
        }

        if stats.hits > 0 {
            self.draw_hit.record(stats.draw_hit_time);
        }
    }

    fn snapshot(&self) -> RendererCacheKindStatsSnapshot {
        RendererCacheKindStatsSnapshot {
            candidates: self.candidates,
            visible_candidates: self.visible_candidates,
            suppressed_by_parent: self.suppressed_by_parent,
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
            prepare_successes: self.prepare_successes,
            prepare_failures: self.prepare_failures,
            direct_fallbacks_after_admission: self.direct_fallbacks_after_admission,
            rejected_ineligible: self.rejected_ineligible,
            rejected_admission: self.rejected_admission,
            rejected_oversized: self.rejected_oversized,
            rejected_payload_budget: self.rejected_payload_budget,
            prepare: self.prepare.snapshot(),
            draw_hit: self.draw_hit.snapshot(),
        }
    }
}

pub struct RendererStatsCollector {
    window: Mutex<RendererStatsWindow>,
    families: StatsFamilies,
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

    pub fn record_present_submit(&self, duration: Duration) {
        self.record_timing(RendererTimingMetric::PresentSubmit, duration);
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

        *window = RendererStatsWindow::new(now, window.last_display_interval_ns);
        snapshot
    }

    pub fn reset(&self) {
        let now = Instant::now();
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *window = RendererStatsWindow::new(now, window.last_display_interval_ns);
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

pub fn format_renderer_stats_log(backend_label: &str, snapshot: &RendererStatsSnapshot) -> String {
    let timing_lines = RendererTimingMetric::ALL
        .into_iter()
        .map(|metric| format_duration_stat_line(metric.log_label(), snapshot.timing(metric)))
        .collect::<Vec<_>>()
        .join("\n");

    let mut message = format!(
        concat!(
            "renderer stats\n",
            "  window\n",
            "    backend: {}\n",
            "    duration: {} ms\n",
            "    frames: {}\n",
            "    fps: {:.1}\n",
            "    display: {:.1} fps ({:.3} ms/frame)\n",
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
        snapshot.window.as_millis(),
        snapshot.frame_count,
        snapshot.fps,
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

    message.push_str("\n\n  renderer cache\n");
    if !snapshot.renderer_cache.noop.is_empty() {
        message.push_str(&format_renderer_cache_kind_line(
            "noop",
            &snapshot.renderer_cache.noop,
            snapshot.frame_count,
        ));
    }
    message.push_str(&format_renderer_cache_kind_line(
        "clean_subtree",
        &snapshot.renderer_cache.clean_subtree,
        snapshot.frame_count,
    ));

    message
}

fn format_renderer_cache_kind_line(
    label: &str,
    stats: &RendererCacheKindStatsSnapshot,
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
    format!(
        concat!(
            "    {}\n",
            "      activity: candidates={} visible={} suppressed_by_parent={} admitted={} hits={} misses={} stores={} evictions={} stale_evictions={} rejected={}\n",
            "      per_frame: candidates={:.2} visible={:.2} hits={:.2} misses={:.2} stores={:.2} rejected={:.2}\n",
            "      resident: entries={} bytes={} payloads={{gpu={} cpu={} unknown={}}}\n",
            "      store_payloads: gpu={} cpu={} evicted_bytes={} stale_evicted_bytes={}\n",
            "      prepare: success={} failure={} avg={:.3} ms count={}\n",
            "      fallback_after_admit={} rejections={{ineligible={} admission={} oversized={} budget={}}}\n",
            "      hit_draw: avg={:.3} ms count={}\n"
        ),
        label,
        stats.candidates,
        stats.visible_candidates,
        stats.suppressed_by_parent,
        stats.admitted,
        stats.hits,
        stats.misses,
        stats.stores,
        stats.evictions,
        stats.stale_evictions,
        stats.rejected,
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
        stats.prepare_successes,
        stats.prepare_failures,
        stats.prepare.avg_ms,
        stats.prepare.count,
        stats.direct_fallbacks_after_admission,
        stats.rejected_ineligible,
        stats.rejected_admission,
        stats.rejected_oversized,
        stats.rejected_payload_budget,
        stats.draw_hit.avg_ms,
        stats.draw_hit.count,
    )
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
    if !stats.noop.is_empty() {
        message.push_str(&format_renderer_cache_kind_frame_line("noop", &stats.noop));
    }
    message.push_str(&format_renderer_cache_kind_frame_line(
        "clean_subtree",
        &stats.clean_subtree,
    ));
    message
}

fn format_renderer_cache_kind_frame_line(
    label: &str,
    stats: &RendererCacheKindFrameStats,
) -> String {
    let unknown_payloads = stats.current_entries.saturating_sub(
        stats
            .current_gpu_payloads
            .saturating_add(stats.current_cpu_payloads),
    );
    format!(
        concat!(
            "    {}\n",
            "      activity: candidates={} visible={} suppressed_by_parent={} admitted={} hits={} misses={} stores={} evictions={} stale_evictions={} rejected={}\n",
            "      resident: entries={} bytes={} payloads={{gpu={} cpu={} unknown={}}}\n",
            "      store_payloads: gpu={} cpu={} evicted_bytes={} stale_evicted_bytes={}\n",
            "      prepare: success={} failure={} time={:.3} ms\n",
            "      fallback_after_admit={} rejections={{ineligible={} admission={} oversized={} budget={}}}\n",
            "      hit_draw: time={:.3} ms\n"
        ),
        label,
        stats.candidates,
        stats.visible_candidates,
        stats.suppressed_by_parent,
        stats.admitted,
        stats.hits,
        stats.misses,
        stats.stores,
        stats.evictions,
        stats.stale_evictions,
        stats.rejected,
        stats.current_entries,
        stats.current_bytes,
        stats.current_gpu_payloads,
        stats.current_cpu_payloads,
        unknown_payloads,
        stats.gpu_payload_stores,
        stats.cpu_payload_stores,
        stats.evicted_bytes,
        stats.stale_evicted_bytes,
        stats.prepare_successes,
        stats.prepare_failures,
        duration_ms(stats.prepare_time),
        stats.direct_fallbacks_after_admission,
        stats.rejected_ineligible,
        stats.rejected_admission,
        stats.rejected_oversized,
        stats.rejected_payload_budget,
        duration_ms(stats.draw_hit_time),
    )
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
        format_slow_present_frame_log, format_slow_render_frame_log, render_frame_has_slow_stage,
    };
    use crate::{
        render_scene::{DrawPrimitive, RenderNode, RenderScene},
        renderer::{
            RenderBorderDrawSummary, RenderClipDrawSummary, RenderDrawTimings,
            RenderImageAssetKind, RenderImageDrawProfile, RenderLayerDrawSummary,
            RenderShadowDrawPath, RenderShadowDrawProfile, RenderTimings, RendererCacheFrameStats,
            RendererCacheKindFrameStats,
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
            noop: RendererCacheKindFrameStats {
                candidates: 2,
                visible_candidates: 1,
                admitted: 1,
                hits: 1,
                misses: 1,
                stores: 1,
                current_entries: 1,
                current_bytes: 128,
                current_cpu_payloads: 1,
                cpu_payload_stores: 1,
                prepare_successes: 1,
                prepare_time: Duration::from_micros(20),
                draw_hit_time: Duration::from_micros(10),
                ..RendererCacheKindFrameStats::default()
            },
            clean_subtree: RendererCacheKindFrameStats {
                candidates: 4,
                visible_candidates: 4,
                admitted: 1,
                stores: 1,
                evictions: 1,
                current_entries: 1,
                current_bytes: 512,
                current_gpu_payloads: 1,
                evicted_bytes: 128,
                gpu_payload_stores: 1,
                prepare_successes: 1,
                prepare_time: Duration::from_micros(30),
                ..RendererCacheKindFrameStats::default()
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
        assert_eq!(snapshot.renderer_cache.noop.candidates, 2);
        assert_eq!(snapshot.renderer_cache.noop.visible_candidates, 1);
        assert_eq!(snapshot.renderer_cache.noop.cpu_payload_stores, 1);
        assert_eq!(snapshot.renderer_cache.noop.prepare_successes, 1);
        assert_eq!(snapshot.renderer_cache.noop.prepare.count, 1);
        assert_eq!(snapshot.renderer_cache.noop.prepare.avg_ms, 0.02);
        assert_eq!(snapshot.renderer_cache.noop.draw_hit.count, 1);
        assert_eq!(snapshot.renderer_cache.noop.draw_hit.avg_ms, 0.01);
        assert_eq!(snapshot.renderer_cache.clean_subtree.candidates, 4);
        assert_eq!(snapshot.renderer_cache.clean_subtree.visible_candidates, 4);
        assert_eq!(snapshot.renderer_cache.clean_subtree.stores, 1);
        assert_eq!(snapshot.renderer_cache.clean_subtree.evictions, 1);
        assert_eq!(snapshot.renderer_cache.clean_subtree.current_entries, 1);
        assert_eq!(snapshot.renderer_cache.clean_subtree.current_bytes, 512);
        assert_eq!(
            snapshot.renderer_cache.clean_subtree.current_gpu_payloads,
            1
        );
        assert_eq!(
            snapshot.renderer_cache.clean_subtree.current_cpu_payloads,
            0
        );
        assert_eq!(snapshot.renderer_cache.clean_subtree.evicted_bytes, 128);
        assert_eq!(snapshot.renderer_cache.clean_subtree.gpu_payload_stores, 1);
        assert_eq!(snapshot.renderer_cache.clean_subtree.prepare_successes, 1);
        assert_eq!(snapshot.renderer_cache.clean_subtree.prepare.count, 1);
        assert_eq!(snapshot.renderer_cache.clean_subtree.prepare.avg_ms, 0.03);

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
        assert_eq!(reset_snapshot.renderer_cache.noop.candidates, 0);
        assert_eq!(reset_snapshot.renderer_cache.clean_subtree.candidates, 0);
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
        stats.record_layout(Duration::from_millis(3));
        stats.record_refresh(Duration::from_millis(1));
        stats.record_event_resolve(Duration::from_millis(2));
        stats.record_patch_tree_process(Duration::from_millis(7));
        stats.record_renderer_cache(RendererCacheFrameStats {
            noop: RendererCacheKindFrameStats {
                candidates: 3,
                visible_candidates: 2,
                admitted: 1,
                hits: 1,
                misses: 1,
                stores: 1,
                rejected: 1,
                current_entries: 1,
                current_bytes: 256,
                current_cpu_payloads: 1,
                cpu_payload_stores: 1,
                prepare_successes: 1,
                rejected_ineligible: 1,
                prepare_time: Duration::from_micros(40),
                draw_hit_time: Duration::from_micros(12),
                ..RendererCacheKindFrameStats::default()
            },
            clean_subtree: RendererCacheKindFrameStats {
                candidates: 5,
                visible_candidates: 5,
                admitted: 2,
                misses: 1,
                stores: 1,
                evictions: 1,
                rejected: 1,
                current_entries: 1,
                current_bytes: 512,
                current_gpu_payloads: 1,
                evicted_bytes: 128,
                gpu_payload_stores: 1,
                prepare_successes: 1,
                direct_fallbacks_after_admission: 1,
                rejected_payload_budget: 1,
                prepare_time: Duration::from_micros(50),
                ..RendererCacheKindFrameStats::default()
            },
        });
        stats.record_layout_cache(LayoutCacheStats {
            resolve_hits: 11,
            ..LayoutCacheStats::default()
        });

        let message = format_renderer_stats_log("wayland", &stats.snapshot());

        assert!(message.starts_with("renderer stats\n"));
        assert!(message.contains("  window\n"));
        assert!(message.contains("    backend: wayland\n"));
        assert!(message.contains("    frames: 1\n"));
        assert!(message.contains("    fps: "));
        assert!(message.contains("    display: "));
        assert!(message.contains("  timings\n"));
        assert!(message.contains("    render: avg=3.000 ms min=3.000 ms max=3.000 ms count=1"));
        assert!(message.contains("    render draw: avg=2.000 ms"));
        assert!(message.contains("    render flush: avg=1.000 ms"));
        assert!(message.contains("    render gpu flush: avg=1.000 ms"));
        assert!(message.contains("    render submit: avg=0.000 ms"));
        assert!(message.contains("    present submit: avg=1.000 ms"));
        assert!(message.contains("    pipeline submit->frame callback: avg=18.000 ms"));
        assert!(message.contains("    pipeline submit->tree: avg=2.000 ms"));
        assert!(message.contains("    pipeline tree: avg=6.000 ms"));
        assert!(message.contains("    pipeline render queue: avg=2.000 ms"));
        assert!(message.contains("    pipeline submit->swap: avg=11.000 ms"));
        assert!(message.contains("    pipeline swap->frame callback: avg=7.000 ms"));
        assert!(message.contains("    layout: avg=3.000 ms"));
        assert!(message.contains("    refresh: avg=1.000 ms"));
        assert!(message.contains("    event resolve: avg=2.000 ms"));
        assert!(message.contains("    patch tree actor: avg=7.000 ms"));
        assert!(message.contains("  layout cache\n"));
        assert!(message.contains("    intrinsic measure: hits=0 misses=0 stores=0"));
        assert!(message.contains("    subtree measure:   hits=0 misses=0 stores=0"));
        assert!(message.contains("    resolve:           hits=11 misses=0 stores=0"));
        assert!(message.contains("  renderer cache\n"));
        assert!(message.contains("    noop\n"));
        assert!(message.contains(
            "activity: candidates=3 visible=2 suppressed_by_parent=0 admitted=1 hits=1 misses=1 stores=1 evictions=0 stale_evictions=0 rejected=1"
        ));
        assert!(message.contains(
            "per_frame: candidates=3.00 visible=2.00 hits=1.00 misses=1.00 stores=1.00 rejected=1.00"
        ));
        assert!(message.contains("resident: entries=1 bytes=256 payloads={gpu=0 cpu=1 unknown=0}"));
        assert!(
            message.contains("store_payloads: gpu=0 cpu=1 evicted_bytes=0 stale_evicted_bytes=0")
        );
        assert!(message.contains("    clean_subtree\n"));
        assert!(message.contains(
            "activity: candidates=5 visible=5 suppressed_by_parent=0 admitted=2 hits=0 misses=1 stores=1 evictions=1 stale_evictions=0 rejected=1"
        ));
        assert!(message.contains(
            "per_frame: candidates=5.00 visible=5.00 hits=0.00 misses=1.00 stores=1.00 rejected=1.00"
        ));
        assert!(message.contains("resident: entries=1 bytes=512 payloads={gpu=1 cpu=0 unknown=0}"));
        assert!(
            message.contains("store_payloads: gpu=1 cpu=0 evicted_bytes=128 stale_evicted_bytes=0")
        );
        assert!(message.contains("prepare: success=1 failure=0 avg=0.050 ms count=1"));
        assert!(message.contains("prepare: success=1 failure=0 avg=0.040 ms count=1"));
        assert!(message.contains(
            "fallback_after_admit=1 rejections={ineligible=0 admission=0 oversized=0 budget=1}"
        ));
        assert!(message.contains("hit_draw: avg=0.012 ms count=1"));
    }

    #[test]
    fn log_format_includes_empty_clean_subtree_renderer_cache() {
        let stats = RendererStatsCollector::new();
        stats.record_frame_present();

        let message = format_renderer_stats_log("wayland", &stats.snapshot());

        assert!(message.contains("  renderer cache\n"));
        assert!(message.contains("    clean_subtree\n"));
        assert!(
            message
                .contains("activity: candidates=0 visible=0 suppressed_by_parent=0 admitted=0 hits=0 misses=0 stores=0")
        );
        assert!(!message.contains("    noop\n"));
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
                clean_subtree: RendererCacheKindFrameStats {
                    candidates: 1,
                    visible_candidates: 1,
                    admitted: 1,
                    hits: 1,
                    current_entries: 1,
                    current_bytes: 4096,
                    current_gpu_payloads: 1,
                    draw_hit_time: Duration::from_micros(9),
                    ..RendererCacheKindFrameStats::default()
                },
                ..RendererCacheFrameStats::default()
            })),
            ..RenderTimings::default()
        };

        let message = format_slow_render_frame_log("wayland", &timings, scene.summary());

        assert!(message.contains("  renderer cache\n"));
        assert!(message.contains("    clean_subtree\n"));
        assert!(
            message
                .contains("activity: candidates=1 visible=1 suppressed_by_parent=0 admitted=1 hits=1 misses=0 stores=0")
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
