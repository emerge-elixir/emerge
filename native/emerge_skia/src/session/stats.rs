//! Session-local, opt-in wall-clock measurements. Only fixed-size samples cross
//! the stats lock. No tree walks, driver queries or waiting for another stage.
use super::*;
use std::ops::Range;
include!("stats/schema.rs");
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod callback;
mod report;
#[cfg(test)]
mod tests;
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(super) use callback::Callback;

const PERIOD: Duration = Duration::from_secs(5);
const SLOW: Duration = Duration::from_millis(4);

macro_rules! timings {
    ($($id:ident => $field:ident : $label:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub(super) enum Metric { $($id),+ }
        impl StatsTimingSnapshotNif {
            fn record(&mut self, metric: Metric, d: Duration) {
                match metric { $(Metric::$id => &mut self.$field),+ }.add(d);
            }
            fn rows(&self) -> impl Iterator<Item = (Metric, &'static str, DurationStatsNif)> {
                [$((Metric::$id, $label, self.$field)),+].into_iter()
            }
        }
    };
}
timings! {
    Render => render: "render",
    Draw => render_draw: "render draw",
    Flush => render_flush: "render flush",
    GpuFlush => render_gpu_flush: "render gpu flush",
    Submit => render_submit: "render submit",
    Present => present_submit: "present submit",
    CallbackTotal => pipeline: "pipeline submit->frame callback",
    TreeStart => pipeline_submit_to_tree_start: "pipeline submit->tree",
    Tree => pipeline_tree: "pipeline tree",
    Queue => pipeline_render_queue: "pipeline render queue",
    Delivered => pipeline_submit_to_swap: "pipeline submit->handoff",
    CallbackWait => pipeline_swap_to_frame_callback: "pipeline handoff->frame callback",
    Layout => layout: "layout",
    Refresh => refresh: "refresh",
    Event => event_resolve: "event resolve",
    Patch => patch_tree_process: "patch tree actor",
}
impl DurationStatsNif {
    fn add(&mut self, elapsed: Duration) {
        let ms = elapsed.as_secs_f64() * 1000.;
        self.count += 1;
        self.avg_ms += (ms - self.avg_ms) / self.count as f64;
        self.min_ms = if self.count == 1 {
            ms
        } else {
            self.min_ms.min(ms)
        };
        self.max_ms = self.max_ms.max(ms);
    }
}
#[derive(Clone, Copy, Default)]
struct Counts {
    timings: StatsTimingSnapshotNif,
    pipeline: StatsPipelineSnapshotNif,
    reused: u64,
    edits: [DurationStatsNif; 3],
}
// Accumulate NIF slices on the ticket; only accepted publications contribute a
// sample. The submitting NIF never needs a stats lock or a reply from the tree.
#[derive(Clone, Copy, Default)]
pub(super) struct EditTimes {
    // An empty (route-only) edit has no wire slices to time.
    pub decode: Option<Duration>,
    pub prepare: Duration,
    pub apply: Duration,
}

// Profile one producer turn locally, then record it once. No stats locks
// between Skia flush and submit and no additional GPU synchronization.
#[derive(Default)]
pub(super) struct DrawTimes {
    pub began: Option<Instant>,
    pub draw: Option<Duration>,
    pub flush: Option<Duration>,
    pub gpu_flush: Option<Duration>,
    pub submit: Option<Duration>,
}
impl DrawTimes {
    pub fn mark(&self) -> Option<Instant> {
        self.began.map(|_| Instant::now())
    }
    pub fn time<T>(
        &mut self,
        field: fn(&mut Self) -> &mut Option<Duration>,
        work: impl FnOnce() -> T,
    ) -> T {
        let start = self.mark();
        let result = work();
        *field(self) = start.map(|t| t.elapsed());
        result
    }
}
struct Window {
    start: Instant,
    counts: Counts,
    // Gauges survive :take/:reset. They aren't samples from the new window.
    interval: Option<Duration>,
    layout: [usize; 3],
    packed: [usize; 3],
    last_slow: [Option<Instant>; 2],
}
impl Window {
    fn new(interval: Option<Duration>) -> Self {
        Self {
            start: Instant::now(),
            counts: Counts::default(),
            interval,
            layout: [0; 3],
            packed: [0; 3],
            last_slow: [None; 2],
        }
    }
    fn close(&mut self, cmd: StatsCommandNif, now: Instant) -> Reading {
        let discard = matches!(cmd, StatsCommandNif::Reset);
        let reset = discard || matches!(cmd, StatsCommandNif::Take);
        let reading = Reading {
            elapsed: if discard {
                Duration::ZERO
            } else {
                now.duration_since(self.start)
            },
            counts: if discard {
                Counts::default()
            } else {
                self.counts
            },
            interval: self.interval,
            layout: self.layout,
            packed: self.packed,
        };
        if reset {
            self.start = now;
            self.counts = Counts::default();
        }
        reading
    }
}
#[derive(Default)]
struct Reading {
    elapsed: Duration,
    counts: Counts,
    interval: Option<Duration>,
    layout: [usize; 3],
    packed: [usize; 3],
}
impl Reading {
    fn frames(&self) -> StatsFrameSnapshotNif {
        let count = self.counts.timings.present_submit.count;
        let hz = |d: Duration| {
            if d.is_zero() {
                0.
            } else {
                1. / d.as_secs_f64()
            }
        };
        StatsFrameSnapshotNif {
            frame_count: count,
            fps: count as f64 * hz(self.elapsed),
            display_fps: self.interval.map_or(0., hz),
            display_frame_ms: self.interval.map_or(0., |d| d.as_secs_f64() * 1000.),
        }
    }
}
struct Enabled {
    window: Mutex<Window>,
    logging: bool,
    assets: std::sync::OnceLock<crate::raster_assets::Usage>,
}
pub(super) struct Collector(Option<Box<Enabled>>);
impl Collector {
    pub(super) fn new(enabled: bool, logging: bool, interval: Option<Duration>) -> Self {
        Self((enabled || logging).then(|| {
            Box::new(Enabled {
                window: Mutex::new(Window::new(interval)),
                logging,
                assets: Default::default(),
            })
        }))
    }
    fn update<T>(&self, f: impl FnOnce(&mut Window) -> T) -> Option<T> {
        self.0
            .as_ref()
            .map(|s| f(&mut s.window.lock().unwrap_or_else(|p| p.into_inner())))
    }
    pub(super) fn now(&self) -> Option<Instant> {
        self.0.as_ref().map(|_| Instant::now())
    }
    pub(super) fn measure<T>(&self, metric: Metric, work: impl FnOnce() -> T) -> T {
        let start = self.now();
        let result = work();
        if let Some(t) = start {
            self.record(metric, t.elapsed());
        }
        result
    }
    fn record(&self, metric: Metric, d: Duration) {
        self.update(|w| w.counts.timings.record(metric, d));
    }
    pub(super) fn draw_times(&self) -> DrawTimes {
        DrawTimes {
            began: self.now(),
            ..Default::default()
        }
    }
    pub(super) fn edit(&self, at: Instant, spans: EditTimes) {
        let wait = at.elapsed();
        self.update(|w| {
            w.counts.timings.record(Metric::TreeStart, wait);
            let [decode, prepare, apply] = &mut w.counts.edits;
            if let Some(d) = spans.decode {
                decode.add(d);
            }
            prepare.add(spans.prepare);
            apply.add(spans.apply);
        });
    }
    pub(super) fn interval(&self, interval: Option<Duration>) {
        self.update(|w| w.interval = interval);
    }
    pub(super) fn assets(&self, loader: &mut crate::raster_assets::Loader) {
        if let Some(s) = &self.0
            && s.logging
        {
            s.assets.get_or_init(|| loader.usage());
        }
    }
    pub(super) fn overwrite(&self) {
        self.update(|w| w.counts.pipeline.render_queue_overwrites += 1);
    }
    pub(super) fn constructed(
        &self,
        tree: &Tree,
        scene: &crate::tree::Scene,
        started: Instant,
        queued: Instant,
    ) {
        self.update(|w| {
            let c = &mut w.counts;
            c.timings
                .record(Metric::Tree, queued.duration_since(started));
            c.pipeline.scenes_constructed += 1;
            w.packed = scene.packed_counts();
            w.layout = tree.layout_entries();
        });
    }
    fn reading(&self, cmd: StatsCommandNif) -> Reading {
        self.update(|w| w.close(cmd, Instant::now()))
            .unwrap_or_default()
    }
    fn log_slow(&self, stage: usize, d: Duration) -> bool {
        self.0.as_ref().is_some_and(|s| s.logging)
            && d >= SLOW
            && self.update(|w| {
                let now = Instant::now();
                if w.last_slow[stage]
                    .is_some_and(|t| now.duration_since(t) < Duration::from_secs(1))
                {
                    return false;
                }
                w.last_slow[stage] = Some(now);
                true
            }) == Some(true)
    }
}
impl Owner {
    fn info(&self, source: &str, message: impl FnOnce() -> String) {
        let target = *self.log.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(pid) = target {
            let _ = OwnedEnv::new()
                .send_and_clear(&pid, |_| (emerge_skia_log(), info(), source, message()));
        }
    }
    pub(super) fn drawn(&self, bundle: &Bundle, d: DrawTimes, reused: bool) {
        let Some(start) = d.began else { return };
        let elapsed = start.elapsed();
        self.stats.update(|w| {
            w.counts.timings.record(Metric::Render, elapsed);
            for (metric, span) in [
                (Metric::Draw, d.draw),
                (Metric::Flush, d.flush),
                (Metric::GpuFlush, d.gpu_flush),
                (Metric::Submit, d.submit),
            ] {
                if let Some(time) = span {
                    w.counts.timings.record(metric, time);
                }
            }
            if let Some(q) = bundle.queued {
                w.counts
                    .timings
                    .record(Metric::Queue, start.duration_since(q));
            }
            w.counts.reused += u64::from(reused);
        });
        if self.stats.log_slow(0, elapsed) {
            self.slow_frame(bundle, "render", elapsed, Some(&d));
        }
    }
    pub(super) fn handed_off(&self, bundle: &Bundle, handoff: &Range<Instant>) {
        let elapsed = handoff.end.duration_since(handoff.start);
        self.stats.update(|w| {
            w.counts.timings.record(Metric::Present, elapsed);
            if let Some(t) = bundle.accepted {
                w.counts
                    .timings
                    .record(Metric::Delivered, handoff.end.duration_since(t));
            }
        });
        if self.stats.log_slow(1, elapsed) {
            self.slow_frame(bundle, "present", elapsed, None);
        }
    }
    fn slow_frame(&self, b: &Bundle, stage: &str, d: Duration, draw: Option<&DrawTimes>) {
        self.info("renderer_slow_frame", || {
            let [commands, glyphs, hits] = b.scene.packed_counts();
            let mut msg = format!("slow {stage} frame\n  backend: {}\n  {stage}: {:.3} ms\n  scene: revision={} serial={} commands={commands} glyphs={glyphs} hits={hits}", self.backend, d.as_secs_f64()*1000., b.revision, b.id.serial);
            if let Some(t) = draw {
                use std::fmt::Write;
                for (label,time) in [("draw",t.draw),("flush",t.flush),("gpu_flush",t.gpu_flush),("submit",t.submit)] {
                    if let Some(v) = time { write!(msg,"\n    {label}: {:.3} ms", v.as_secs_f64()*1000.).unwrap(); }
                }
            }
            msg
        });
    }
}
impl Display {
    pub(super) fn next_report(logging: bool) -> Option<Instant> {
        logging.then(|| Instant::now() + PERIOD)
    }
    // The output loop services this deadline even when there are no edits,
    // drawable targets or frame callbacks. It does not grant draw credit.
    pub(super) fn report_due(&mut self, owner: &Owner) -> Option<Instant> {
        let due = self.next_report.as_mut()?;
        let now = Instant::now();
        if now >= *due {
            *due = now + PERIOD;
            let r = owner.stats.reading(StatsCommandNif::Take);
            owner.info("renderer_stats", || report::format(owner, &r));
        }
        Some(*due)
    }
}
pub(super) fn snapshot(owner: &Owner, cmd: StatsCommandNif) -> Result<StatsSnapshotNif, String> {
    if matches!(cmd, StatsCommandNif::Configure(_)) {
        return Err("renderer stats are configured when the renderer starts".into());
    }
    let r = owner.stats.reading(cmd);
    let enabled = owner.stats.0.is_some();
    let mut snap = StatsSnapshotNif {
        version: STATS_SCHEMA_VERSION,
        kind: "renderer".into(),
        enabled,
        rendering_api: Some(RenderingApi {
            requested: owner.api.clone(),
            selected: owner
                .selected
                .get()
                .unwrap_or(&Selected::Raster)
                .name()
                .into(),
        }),
        window: StatsWindowNif {
            elapsed_ms: r.elapsed.as_millis() as u64,
            reset_on_read: enabled && matches!(cmd, StatsCommandNif::Take),
        },
        frames: r.frames(),
        timings: r.counts.timings,
        ..Default::default()
    };
    snap.counters.pipeline = r.counts.pipeline;
    snap.counters.renderer_cache.disabled_reason = Some("renderer cache unavailable".into());
    Ok(snap)
}
