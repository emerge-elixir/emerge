use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

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
            "backend={} ",
            "window_ms={} ",
            "fps={:.1} ",
            "display_fps={:.1} ",
            "display_frame_ms={:.3} ",
            "frame_count={} ",
            "render_ms_avg={:.3} render_ms_min={:.3} render_ms_max={:.3} render_ms_count={} ",
            "present_submit_ms_avg={:.3} present_submit_ms_min={:.3} ",
            "present_submit_ms_max={:.3} present_submit_ms_count={} ",
            "layout_ms_avg={:.3} layout_ms_min={:.3} layout_ms_max={:.3} layout_ms_count={} ",
            "refresh_ms_avg={:.3} refresh_ms_min={:.3} refresh_ms_max={:.3} refresh_ms_count={} ",
            "event_resolve_ms_avg={:.3} event_resolve_ms_min={:.3} ",
            "event_resolve_ms_max={:.3} event_resolve_ms_count={} ",
            "patch_tree_actor_process_ms_avg={:.3} patch_tree_actor_process_ms_min={:.3} ",
            "patch_tree_actor_process_ms_max={:.3} patch_tree_actor_process_ms_count={} ",
            "layout_cache_intrinsic_measure_hits={} layout_cache_intrinsic_measure_misses={} ",
            "layout_cache_intrinsic_measure_stores={} ",
            "layout_cache_subtree_measure_hits={} layout_cache_subtree_measure_misses={} ",
            "layout_cache_subtree_measure_stores={} ",
            "layout_cache_resolve_hits={} layout_cache_resolve_misses={} ",
            "layout_cache_resolve_stores={}"
        ),
        backend_label,
        snapshot.window.as_millis(),
        snapshot.fps,
        snapshot.display_fps,
        snapshot.display_frame_ms,
        snapshot.frame_count,
        snapshot.render.avg_ms,
        snapshot.render.min_ms,
        snapshot.render.max_ms,
        snapshot.render.count,
        snapshot.present_submit.avg_ms,
        snapshot.present_submit.min_ms,
        snapshot.present_submit.max_ms,
        snapshot.present_submit.count,
        snapshot.layout.avg_ms,
        snapshot.layout.min_ms,
        snapshot.layout.max_ms,
        snapshot.layout.count,
        snapshot.refresh.avg_ms,
        snapshot.refresh.min_ms,
        snapshot.refresh.max_ms,
        snapshot.refresh.count,
        snapshot.event_resolve.avg_ms,
        snapshot.event_resolve.min_ms,
        snapshot.event_resolve.max_ms,
        snapshot.event_resolve.count,
        snapshot.patch_tree_process.avg_ms,
        snapshot.patch_tree_process.min_ms,
        snapshot.patch_tree_process.max_ms,
        snapshot.patch_tree_process.count,
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

#[cfg(test)]
mod tests {
    use super::{LayoutCacheStats, RendererStatsCollector, format_renderer_stats_log};
    use std::time::Duration;

    #[test]
    fn snapshot_tracks_avg_min_max_and_resets_window() {
        let stats = RendererStatsCollector::new();

        stats.record_frame_present();
        stats.record_display_interval(Duration::from_millis(16));
        stats.record_frame_present();
        stats.record_render(Duration::from_millis(4));
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

        assert!(message.contains("backend=wayland"));
        assert!(message.contains("fps="));
        assert!(message.contains("display_fps="));
        assert!(message.contains("display_frame_ms="));
        assert!(message.contains("frame_count=1"));
        assert!(message.contains("render_ms_avg="));
        assert!(message.contains("render_ms_count=1"));
        assert!(message.contains("present_submit_ms_avg="));
        assert!(message.contains("present_submit_ms_count=1"));
        assert!(message.contains("layout_ms_avg="));
        assert!(message.contains("layout_ms_min="));
        assert!(message.contains("layout_ms_max="));
        assert!(message.contains("layout_ms_count=1"));
        assert!(message.contains("refresh_ms_avg="));
        assert!(message.contains("refresh_ms_min="));
        assert!(message.contains("refresh_ms_max="));
        assert!(message.contains("refresh_ms_count=1"));
        assert!(message.contains("event_resolve_ms_count=1"));
        assert!(message.contains("patch_tree_actor_process_ms_count=1"));
        assert!(message.contains("layout_cache_resolve_hits=11"));
        assert!(message.contains("layout_cache_subtree_measure_hits="));
    }
}
