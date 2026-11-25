//! Keep the existing v25 payload. No samples for unported collectors (in
//! particular no GPU timer, video, DRM, paint-layer cache or scanout claims).
use super::*;
include!("stats/schema.rs");

pub(super) struct Samples {
    start: Instant,
    count: u64,
    interval_ms: Option<f64>,
    submit: DurationStatsNif,
}
impl Samples {
    pub(super) fn new() -> Self {
        Self {
            start: Instant::now(),
            count: 0,
            interval_ms: None,
            submit: Default::default(),
        }
    }
    pub(super) fn submitted(&mut self, elapsed: Duration) {
        self.count += 1;
        let v = elapsed.as_secs_f64() * 1000.;
        let t = &mut self.submit;
        t.count += 1;
        t.avg_ms += (v - t.avg_ms) / t.count as f64;
        t.min_ms = if t.count == 1 { v } else { t.min_ms.min(v) };
        t.max_ms = t.max_ms.max(v);
    }
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub(super) fn output_mode(&mut self, millihertz: i32) {
        self.interval_ms = (millihertz > 0).then(|| 1_000_000. / f64::from(millihertz));
    }
    fn reset(&mut self) {
        *self = Self {
            interval_ms: self.interval_ms,
            ..Self::new()
        };
    }
}
pub(super) fn snapshot(
    l: &mut Display,
    owner: &Owner,
    cmd: StatsCommandNif,
) -> Result<StatsSnapshotNif, String> {
    if matches!(cmd, StatsCommandNif::Configure(_)) {
        return Err("renderer stats are configured when the renderer starts".into());
    }
    let mut snap = StatsSnapshotNif {
        version: STATS_SCHEMA_VERSION,
        kind: "renderer".into(),
        enabled: l.stats.is_some(),
        rendering_api: Some(RenderingApi {
            requested: owner.api.clone(),
            selected: owner
                .selected
                .get()
                .unwrap_or(&Selected::Raster)
                .name()
                .into(),
        }),
        ..Default::default()
    };
    snap.counters.renderer_cache.disabled_reason = Some("renderer cache unavailable".into());
    if let Some(s) = &mut l.stats {
        if matches!(cmd, StatsCommandNif::Reset) {
            s.reset();
        }
        let elapsed = s.start.elapsed();
        snap.window.elapsed_ms = elapsed.as_millis() as u64;
        snap.frames.frame_count = s.count;
        snap.frames.fps = if elapsed.is_zero() {
            0.
        } else {
            s.count as f64 / elapsed.as_secs_f64()
        };
        if let Some(ms) = s.interval_ms {
            snap.frames.display_frame_ms = ms;
            snap.frames.display_fps = 1000. / ms;
        }
        snap.timings.present_submit = s.submit;
        if matches!(cmd, StatsCommandNif::Take) {
            snap.window.reset_on_read = true;
            s.reset();
        }
    }
    Ok(snap)
}
