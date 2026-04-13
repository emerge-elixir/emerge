use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq)]
pub struct DurationStatsSnapshot {
    pub count: u64,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RendererStatsSnapshot {
    pub window: Duration,
    pub fps: f64,
    pub display_fps: f64,
    pub display_frame_ms: f64,
    pub frame_count: u64,
    pub layout: DurationStatsSnapshot,
    pub event_resolve: DurationStatsSnapshot,
    pub patch_tree_process: DurationStatsSnapshot,
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
    layout: DurationStatsWindow,
    event_resolve: DurationStatsWindow,
    patch_tree_process: DurationStatsWindow,
}

impl RendererStatsWindow {
    fn new(started_at: Instant, last_display_interval_ns: Option<u64>) -> Self {
        Self {
            started_at,
            last_display_interval_ns,
            frame_count: 0,
            layout: DurationStatsWindow::default(),
            event_resolve: DurationStatsWindow::default(),
            patch_tree_process: DurationStatsWindow::default(),
        }
    }
}

pub struct RendererStatsCollector {
    window: Mutex<RendererStatsWindow>,
}

impl Default for RendererStatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl RendererStatsCollector {
    pub fn new() -> Self {
        Self {
            window: Mutex::new(RendererStatsWindow::new(Instant::now(), None)),
        }
    }

    pub fn record_frame_present(&self) {
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        window.frame_count = window.frame_count.saturating_add(1);
    }

    pub fn record_display_interval(&self, duration: Duration) {
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ns = duration.as_nanos().min(u128::from(u64::MAX)) as u64;
        window.last_display_interval_ns = Some(ns);
    }

    pub fn record_layout(&self, duration: Duration) {
        self.record_duration(duration, |window| &mut window.layout);
    }

    pub fn record_event_resolve(&self, duration: Duration) {
        self.record_duration(duration, |window| &mut window.event_resolve);
    }

    pub fn record_patch_tree_process(&self, duration: Duration) {
        self.record_duration(duration, |window| &mut window.patch_tree_process);
    }

    pub fn snapshot(&self) -> RendererStatsSnapshot {
        let now = Instant::now();
        let mut window = self
            .window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let elapsed = now.saturating_duration_since(window.started_at);

        let snapshot = RendererStatsSnapshot {
            window: elapsed,
            fps: if elapsed.is_zero() {
                0.0
            } else {
                window.frame_count as f64 / elapsed.as_secs_f64()
            },
            display_fps: window
                .last_display_interval_ns
                .map(|ns| 1_000_000_000.0 / ns as f64)
                .unwrap_or(0.0),
            display_frame_ms: window
                .last_display_interval_ns
                .map(|ns| ns as f64 / 1_000_000.0)
                .unwrap_or(0.0),
            frame_count: window.frame_count,
            layout: window.layout.snapshot(),
            event_resolve: window.event_resolve.snapshot(),
            patch_tree_process: window.patch_tree_process.snapshot(),
        };

        *window = RendererStatsWindow::new(now, window.last_display_interval_ns);
        snapshot
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
            "layout_ms_avg={:.3} layout_ms_min={:.3} layout_ms_max={:.3} layout_ms_count={} ",
            "event_resolve_ms_avg={:.3} event_resolve_ms_min={:.3} ",
            "event_resolve_ms_max={:.3} event_resolve_ms_count={} ",
            "patch_tree_actor_process_ms_avg={:.3} patch_tree_actor_process_ms_min={:.3} ",
            "patch_tree_actor_process_ms_max={:.3} patch_tree_actor_process_ms_count={}"
        ),
        backend_label,
        snapshot.window.as_millis(),
        snapshot.fps,
        snapshot.display_fps,
        snapshot.display_frame_ms,
        snapshot.frame_count,
        snapshot.layout.avg_ms,
        snapshot.layout.min_ms,
        snapshot.layout.max_ms,
        snapshot.layout.count,
        snapshot.event_resolve.avg_ms,
        snapshot.event_resolve.min_ms,
        snapshot.event_resolve.max_ms,
        snapshot.event_resolve.count,
        snapshot.patch_tree_process.avg_ms,
        snapshot.patch_tree_process.min_ms,
        snapshot.patch_tree_process.max_ms,
        snapshot.patch_tree_process.count,
    )
}

#[cfg(test)]
mod tests {
    use super::{format_renderer_stats_log, RendererStatsCollector};
    use std::time::Duration;

    #[test]
    fn snapshot_tracks_avg_min_max_and_resets_window() {
        let stats = RendererStatsCollector::new();

        stats.record_frame_present();
        stats.record_display_interval(Duration::from_millis(16));
        stats.record_frame_present();
        stats.record_layout(Duration::from_millis(2));
        stats.record_layout(Duration::from_millis(6));
        stats.record_event_resolve(Duration::from_millis(1));
        stats.record_patch_tree_process(Duration::from_millis(9));

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.frame_count, 2);
        assert_eq!(snapshot.display_frame_ms, 16.0);
        assert_eq!(snapshot.layout.count, 2);
        assert_eq!(snapshot.layout.min_ms, 2.0);
        assert_eq!(snapshot.layout.max_ms, 6.0);
        assert_eq!(snapshot.layout.avg_ms, 4.0);
        assert_eq!(snapshot.event_resolve.count, 1);
        assert_eq!(snapshot.patch_tree_process.count, 1);

        let reset_snapshot = stats.snapshot();
        assert_eq!(reset_snapshot.frame_count, 0);
        assert_eq!(reset_snapshot.display_frame_ms, 16.0);
        assert_eq!(reset_snapshot.layout.count, 0);
        assert_eq!(reset_snapshot.event_resolve.count, 0);
        assert_eq!(reset_snapshot.patch_tree_process.count, 0);
    }

    #[test]
    fn log_format_includes_all_stats_fields() {
        let stats = RendererStatsCollector::new();
        stats.record_frame_present();
        stats.record_display_interval(Duration::from_millis(16));
        stats.record_layout(Duration::from_millis(3));
        stats.record_event_resolve(Duration::from_millis(2));
        stats.record_patch_tree_process(Duration::from_millis(7));

        let message = format_renderer_stats_log("wayland", &stats.snapshot());

        assert!(message.contains("backend=wayland"));
        assert!(message.contains("fps="));
        assert!(message.contains("display_fps="));
        assert!(message.contains("display_frame_ms="));
        assert!(message.contains("frame_count=1"));
        assert!(message.contains("layout_ms_avg="));
        assert!(message.contains("layout_ms_min="));
        assert!(message.contains("layout_ms_max="));
        assert!(message.contains("layout_ms_count=1"));
        assert!(message.contains("event_resolve_ms_count=1"));
        assert!(message.contains("patch_tree_actor_process_ms_count=1"));
    }
}
