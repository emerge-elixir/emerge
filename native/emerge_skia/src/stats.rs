use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{
    render_scene::RenderSceneSummary,
    renderer::{RenderDrawTimings, RenderImageDrawProfile, RenderShadowDrawProfile, RenderTimings},
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DurationStatsSnapshot {
    pub count: u64,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererStatsSnapshot {
    pub window: Duration,
    pub fps: f64,
    pub display_fps: f64,
    pub display_frame_ms: f64,
    pub frame_count: u64,
    pub render: DurationStatsSnapshot,
    pub render_draw: DurationStatsSnapshot,
    pub render_flush: DurationStatsSnapshot,
    pub render_gpu_flush: DurationStatsSnapshot,
    pub render_submit: DurationStatsSnapshot,
    pub present_submit: DurationStatsSnapshot,
    pub layout: DurationStatsSnapshot,
    pub refresh: DurationStatsSnapshot,
    pub event_resolve: DurationStatsSnapshot,
    pub patch_tree_process: DurationStatsSnapshot,
    pub layout_cache: LayoutCacheStats,
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

struct RendererStatsWindow {
    started_at: Instant,
    last_display_interval_ns: Option<u64>,
    frame_count: u64,
    render: DurationStatsWindow,
    render_draw: DurationStatsWindow,
    render_flush: DurationStatsWindow,
    render_gpu_flush: DurationStatsWindow,
    render_submit: DurationStatsWindow,
    present_submit: DurationStatsWindow,
    layout: DurationStatsWindow,
    refresh: DurationStatsWindow,
    event_resolve: DurationStatsWindow,
    patch_tree_process: DurationStatsWindow,
    layout_cache: LayoutCacheStats,
}

impl RendererStatsWindow {
    fn new(started_at: Instant, last_display_interval_ns: Option<u64>) -> Self {
        Self {
            started_at,
            last_display_interval_ns,
            frame_count: 0,
            render: DurationStatsWindow::default(),
            render_draw: DurationStatsWindow::default(),
            render_flush: DurationStatsWindow::default(),
            render_gpu_flush: DurationStatsWindow::default(),
            render_submit: DurationStatsWindow::default(),
            present_submit: DurationStatsWindow::default(),
            layout: DurationStatsWindow::default(),
            refresh: DurationStatsWindow::default(),
            event_resolve: DurationStatsWindow::default(),
            patch_tree_process: DurationStatsWindow::default(),
            layout_cache: LayoutCacheStats::default(),
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
            render: self.render.snapshot(),
            render_draw: self.render_draw.snapshot(),
            render_flush: self.render_flush.snapshot(),
            render_gpu_flush: self.render_gpu_flush.snapshot(),
            render_submit: self.render_submit.snapshot(),
            present_submit: self.present_submit.snapshot(),
            layout: self.layout.snapshot(),
            refresh: self.refresh.snapshot(),
            event_resolve: self.event_resolve.snapshot(),
            patch_tree_process: self.patch_tree_process.snapshot(),
            layout_cache: self.layout_cache,
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
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.render);
    }

    pub fn record_render_draw(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.render_draw);
    }

    pub fn record_render_flush(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.render_flush);
    }

    pub fn record_render_gpu_flush(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.render_gpu_flush);
    }

    pub fn record_render_submit(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.render_submit);
    }

    pub fn record_present_submit(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.present_submit);
    }

    pub fn record_layout(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.layout);
    }

    pub fn record_refresh(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.refresh);
    }

    pub fn record_event_resolve(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.event_resolve);
    }

    pub fn record_patch_tree_process(&self, duration: Duration) {
        if !self.families.timings {
            return;
        }

        self.record_duration(duration, |window| &mut window.patch_tree_process);
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

    fn record_duration(
        &self,
        duration: Duration,
        metric: impl FnOnce(&mut RendererStatsWindow) -> &mut DurationStatsWindow,
    ) {
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        metric(&mut window).record(duration);
    }
}

pub fn format_renderer_stats_log(backend_label: &str, snapshot: &RendererStatsSnapshot) -> String {
    format!(
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
            "{}\n",
            "{}\n",
            "{}\n",
            "{}\n",
            "{}\n",
            "{}\n",
            "{}\n",
            "{}\n",
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
        format_duration_stat_line("render", &snapshot.render),
        format_duration_stat_line("render draw", &snapshot.render_draw),
        format_duration_stat_line("render flush", &snapshot.render_flush),
        format_duration_stat_line("render gpu flush", &snapshot.render_gpu_flush),
        format_duration_stat_line("render submit", &snapshot.render_submit),
        format_duration_stat_line("present submit", &snapshot.present_submit),
        format_duration_stat_line("layout", &snapshot.layout),
        format_duration_stat_line("refresh", &snapshot.refresh),
        format_duration_stat_line("event resolve", &snapshot.event_resolve),
        format_duration_stat_line("patch tree actor", &snapshot.patch_tree_process),
        snapshot.layout_cache.intrinsic_measure_hits,
        snapshot.layout_cache.intrinsic_measure_misses,
        snapshot.layout_cache.intrinsic_measure_stores,
        snapshot.layout_cache.subtree_measure_hits,
        snapshot.layout_cache.subtree_measure_misses,
        snapshot.layout_cache.subtree_measure_stores,
        snapshot.layout_cache.resolve_hits,
        snapshot.layout_cache.resolve_misses,
        snapshot.layout_cache.resolve_stores,
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

fn format_shadow_draw_detail(index: usize, shadow: &RenderShadowDrawProfile) -> String {
    format!(
        concat!(
            "  shadow[{}]: rect={:.1},{:.1} {:.1}x{:.1} offset={:.1},{:.1} ",
            "blur={:.1} size={:.1} radius={:.1} color=0x{:08X} total={:.3} ms ",
            "prepare={:.3} ms clip={:.3} ms draw={:.3} ms"
        ),
        index,
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
            "  image[{}]: id={} kind={:?} fit={:?} tint={} source={}x{} draw={}x{} ",
            "total={:.3} ms lookup={:.3} ms fit={:.3} ms vector_cache_lookup={:.3} ms ",
            "vector_cache_hit={} vector_rasterize={:.3} ms vector_cache_store={:.3} ms ",
            "draw={:.3} ms"
        ),
        index,
        image.image_id,
        image.kind,
        image.fit,
        image.tinted,
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
        LayoutCacheStats, RendererStatsCollector, format_renderer_stats_log,
        format_slow_present_frame_log, format_slow_render_frame_log, render_frame_has_slow_stage,
    };
    use crate::{
        render_scene::{DrawPrimitive, RenderNode, RenderScene},
        renderer::{
            RenderDrawTimings, RenderImageAssetKind, RenderImageDrawProfile,
            RenderShadowDrawProfile, RenderTimings,
        },
    };
    use std::time::Duration;

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
        stats.record_layout(Duration::from_millis(2));
        stats.record_layout(Duration::from_millis(6));
        stats.record_refresh(Duration::from_millis(1));
        stats.record_refresh(Duration::from_millis(3));
        stats.record_event_resolve(Duration::from_millis(1));
        stats.record_patch_tree_process(Duration::from_millis(9));
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
        assert_eq!(snapshot.render.count, 1);
        assert_eq!(snapshot.render.avg_ms, 4.0);
        assert_eq!(snapshot.render_draw.count, 1);
        assert_eq!(snapshot.render_draw.avg_ms, 3.0);
        assert_eq!(snapshot.render_flush.count, 1);
        assert_eq!(snapshot.render_flush.avg_ms, 1.0);
        assert_eq!(snapshot.render_gpu_flush.count, 1);
        assert_eq!(snapshot.render_gpu_flush.avg_ms, 1.0);
        assert_eq!(snapshot.render_submit.count, 1);
        assert_eq!(snapshot.render_submit.avg_ms, 0.0);
        assert_eq!(snapshot.present_submit.count, 1);
        assert_eq!(snapshot.present_submit.avg_ms, 1.0);
        assert_eq!(snapshot.layout.count, 2);
        assert_eq!(snapshot.layout.min_ms, 2.0);
        assert_eq!(snapshot.layout.max_ms, 6.0);
        assert_eq!(snapshot.layout.avg_ms, 4.0);
        assert_eq!(snapshot.refresh.count, 2);
        assert_eq!(snapshot.refresh.min_ms, 1.0);
        assert_eq!(snapshot.refresh.max_ms, 3.0);
        assert_eq!(snapshot.refresh.avg_ms, 2.0);
        assert_eq!(snapshot.event_resolve.count, 1);
        assert_eq!(snapshot.patch_tree_process.count, 1);
        assert_eq!(snapshot.layout_cache.resolve_hits, 5);
        assert_eq!(snapshot.layout_cache.subtree_measure_hits, 3);

        let reset_snapshot = stats.snapshot();
        assert_eq!(reset_snapshot.frame_count, 0);
        assert_eq!(reset_snapshot.display_frame_ms, 16.0);
        assert_eq!(reset_snapshot.render.count, 0);
        assert_eq!(reset_snapshot.render_draw.count, 0);
        assert_eq!(reset_snapshot.render_flush.count, 0);
        assert_eq!(reset_snapshot.render_gpu_flush.count, 0);
        assert_eq!(reset_snapshot.render_submit.count, 0);
        assert_eq!(reset_snapshot.present_submit.count, 0);
        assert_eq!(reset_snapshot.layout.count, 0);
        assert_eq!(reset_snapshot.refresh.count, 0);
        assert_eq!(reset_snapshot.event_resolve.count, 0);
        assert_eq!(reset_snapshot.patch_tree_process.count, 0);
        assert_eq!(reset_snapshot.layout_cache.resolve_hits, 0);
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
        stats.record_layout(Duration::from_millis(3));
        stats.record_refresh(Duration::from_millis(1));
        stats.record_event_resolve(Duration::from_millis(2));
        stats.record_patch_tree_process(Duration::from_millis(7));
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
        assert!(message.contains("    layout: avg=3.000 ms"));
        assert!(message.contains("    refresh: avg=1.000 ms"));
        assert!(message.contains("    event resolve: avg=2.000 ms"));
        assert!(message.contains("    patch tree actor: avg=7.000 ms"));
        assert!(message.contains("  layout cache\n"));
        assert!(message.contains("    intrinsic measure: hits=0 misses=0 stores=0"));
        assert!(message.contains("    subtree measure:   hits=0 misses=0 stores=0"));
        assert!(message.contains("    resolve:           hits=11 misses=0 stores=0"));
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
                shadow_details: vec![RenderShadowDrawProfile {
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
            "shadow[0]: rect=0.0,0.0 10.0x10.0 offset=0.0,1.0 blur=8.0 size=0.0 radius=4.0 color=0x00000080"
        ));
        assert!(message.contains("prepare=0.010 ms clip=0.020 ms draw=0.070 ms"));
        assert!(message.contains(
            "image[0]: id=asset-1 kind=Vector fit=Contain tint=true source=24x24 draw=48x48"
        ));
        assert!(message.contains("vector_cache_hit=false"));
        assert!(message.contains("vector_rasterize=0.200 ms"));
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
