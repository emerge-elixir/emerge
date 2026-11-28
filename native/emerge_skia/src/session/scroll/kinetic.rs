//! Direct-touch release uses ingress time, not how long layout or dispatch
//! happened to take. Once running, sample elapsed time (never missed ticks).
use super::*;
const SAMPLE_WINDOW: Duration = Duration::from_millis(100);
const DECELERATION_RATE: f32 = 2.358_201_7;

#[derive(Clone, Copy)]
pub(in crate::session) struct Velocity {
    at: Instant,
    point: [f32; 2],
    last_move: Instant,
    velocity: [f32; 2],
}
impl Velocity {
    pub fn new(at: Instant, point: [f32; 2]) -> Self {
        Self {
            at,
            point,
            last_move: at,
            velocity: [0.; 2],
        }
    }
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub fn remap(&mut self, map: impl Fn([f32; 2]) -> [f32; 2]) {
        self.point = map(self.point);
        self.velocity = map(self.velocity);
    }
    pub fn moved(&mut self, at: Instant, point: [f32; 2]) {
        self.last_move = at;
        let dt = at.saturating_duration_since(self.at);
        if dt < Duration::from_millis(4) {
            return;
        }
        self.velocity = if dt > SAMPLE_WINDOW {
            [0.; 2]
        } else {
            std::array::from_fn(|i| (point[i] - self.point[i]) / dt.as_secs_f32())
        };
        self.point = point;
        self.at = at;
    }
    pub fn released(self, at: Instant) -> [f32; 2] {
        if at.saturating_duration_since(self.last_move) >= SAMPLE_WINDOW {
            [0.; 2]
        } else {
            self.velocity
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Fling {
    key: Attachment,
    epoch: u64,
    axis: usize,
    start: Instant,
    seconds: f32,
    distance: f32,
    previous: f32,
    sampled_at: Instant,
    pending: Option<u64>,
}
impl Fling {
    fn new(p: ScrollHit, axis: usize, speed: f32, start: Instant) -> Option<Self> {
        if !speed.is_finite() || speed.abs() < 500. {
            return None;
        }
        // Same finite velocity envelope/trajectory as main's adaptive drag.
        let v = speed.clamp(-6000., 6000.);
        let seconds = DECELERATION_RATE
            * 0.35
            * (v.abs() / (0.015 * 51_890.203 / 0.35)).powf(1. / (DECELERATION_RATE - 1.));
        Some(Self {
            key: p.key,
            epoch: p.view.axes[axis].epoch,
            axis,
            start,
            seconds,
            distance: v * seconds / DECELERATION_RATE,
            previous: 0.,
            sampled_at: start,
            pending: None,
        })
    }
    fn sample(self, now: Instant) -> (f32, bool) {
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / self.seconds).min(1.);
        (
            self.distance * (1. - (1. - t).powf(DECELERATION_RATE)),
            t >= 1.,
        )
    }
}
impl Driver {
    pub fn release(&mut self, s: &Scene, velocity: [f32; 2], now: Instant) {
        let Some(Capture::Pan {
            key,
            active: true,
            locked,
            ..
        }) = self.capture.take()
        else {
            return;
        };
        let Some(p) = s.hits.scroll_port(key) else {
            return;
        };
        let [dx, dy] = components(velocity, locked);
        let v = p.vector(dx, dy);
        let pos = self.axes(p).map(|p| p.pos);
        let axis = (0..2)
            .filter(|&i| {
                p.view.range[i] > 0. && (v[i] > 0. && pos[i] > 0. || v[i] < 0. && pos[i] < 1.)
            })
            .max_by(|&a, &b| v[a].abs().total_cmp(&v[b].abs()));
        self.fling = axis.and_then(|i| Fling::new(p, i, v[i], now));
    }
    pub(super) fn reconcile_fling(&mut self, s: &Scene) {
        if let Some(f) = self.fling.as_mut() {
            if let Some(p) = s.hits.scroll_port(f.key)
                && p.view.axes[f.axis].epoch == f.epoch
                && p.view.range[f.axis] > 0.
            {
                if f.pending
                    .is_some_and(|serial| p.view.axes[f.axis].input >= serial)
                {
                    f.pending = None;
                }
            } else {
                self.interrupt();
            }
        }
    }
    pub fn tick(&mut self, s: &Scene, cx: &Context<'_, '_>, now: Instant) -> Option<Instant> {
        let mut f = self.fling?;
        // No input timer while presentation is stalled. Otherwise use the
        // existing output cadence without consuming animation opportunity IDs.
        if f.pending.is_some() {
            return None;
        }
        let due = f.sampled_at + cx.owner.pipe.lock().ok()?.clock.period();
        if due > now {
            return Some(due);
        }
        self.interrupt();
        let p = s.hits.scroll_port(f.key)?;
        let (distance, done) = f.sample(now);
        let mut d = [0.; 2];
        d[f.axis] = distance - f.previous;
        let next = self.moved(p, d);
        if next == self.axes(p).map(|a| a.pos) {
            return None;
        }
        self.commit(p, next, cx);
        if !done && next[f.axis] > 0. && next[f.axis] < 1. {
            f.previous = distance;
            f.pending = Some(self.serial);
            f.sampled_at = now;
            self.fling = Some(f);
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_uses_real_time_and_a_paused_contact_has_no_release_velocity() {
        let t = Instant::now();
        let mut v = Velocity::new(t, [0., 60.]);
        v.moved(t + Duration::from_millis(1), [0., 55.]);
        assert_eq!(v.released(t + Duration::from_millis(2)), [0.; 2]);
        v.moved(t + Duration::from_millis(20), [0., 30.]);
        assert_eq!(v.released(t + Duration::from_millis(30)), [0., -1500.]);
        assert_eq!(v.released(t + Duration::from_millis(120)), [0.; 2]);
    }
}
