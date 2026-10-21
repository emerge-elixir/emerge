//! Live frame caps and admission deadlines, independent of rendering and delivery.
//! A bounded wake lane is separate from coalesced scene queues. Zero means no
//! explicit cap; backend animation pacing remains a separate policy.
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct FrameRateControl {
    fps: Arc<AtomicU32>,
    wake: Sender<()>,
}
impl FrameRateControl {
    pub(crate) fn new(fps: Option<u32>) -> (Self, Receiver<()>) {
        let (wake, rx) = bounded(1);
        (
            Self {
                fps: Arc::new(AtomicU32::new(fps.unwrap_or(0))),
                wake,
            },
            rx,
        )
    }
    pub(crate) fn set(&self, fps: Option<u32>) -> Result<(), String> {
        if fps == Some(0) {
            return Err("target_fps must be a positive integer or nil".into());
        }
        self.fps.store(fps.unwrap_or(0), Ordering::Release);
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => Err("renderer control channel stopped".into()),
        }
    }
    fn interval(&self) -> Option<Duration> {
        let fps = self.fps.load(Ordering::Acquire);
        (fps != 0).then(|| Duration::from_nanos(1_000_000_000u64.div_ceil(u64::from(fps))))
    }
    #[cfg(all(feature = "wayland", target_os = "linux"))]
    pub(crate) fn is_capped(&self) -> bool {
        self.fps.load(Ordering::Acquire) != 0
    }

    pub(crate) fn animation_interval(&self) -> Duration {
        self.interval().unwrap_or(Duration::from_millis(16))
    }
}

pub(crate) struct FramePacer {
    pub(crate) control: FrameRateControl,
    last_started: Option<Instant>,
}
impl FramePacer {
    pub(crate) fn new(control: FrameRateControl) -> Self {
        Self {
            control,
            last_started: None,
        }
    }
    pub(crate) fn started(&mut self, at: Instant) {
        self.last_started = Some(at);
    }
    pub(crate) fn render_at(&self, now: Instant) -> Instant {
        match (self.last_started, self.control.interval()) {
            (Some(last), Some(interval)) => (last + interval).max(now),
            _ => now,
        }
    }
    /// Only arm a cap deadline for work that the presenter can actually submit.
    /// Compositor callbacks, acquire retries and input keep their own wakeups.
    #[cfg(any(all(feature = "wayland", target_os = "linux"), test))]
    pub(crate) fn dispatch_timeout(
        &self,
        work_ready: bool,
        now: Instant,
        idle: Duration,
    ) -> Duration {
        if work_ready {
            self.render_at(now).saturating_duration_since(now).min(idle)
        } else {
            idle
        }
    }

    pub(crate) fn animation_at(&self, now: Instant) -> Instant {
        self.last_started
            .map(|last| last + self.control.animation_interval())
            .unwrap_or(now)
            .max(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_caps_rebase_from_actual_start_without_catchup_or_render_time_drift() {
        let (control, _rx) = FrameRateControl::new(Some(10));
        let mut pacer = FramePacer::new(control.clone());
        let t = Instant::now();
        assert_eq!(pacer.render_at(t), t);
        pacer.started(t);
        assert_eq!(
            pacer.render_at(t + Duration::from_millis(30)),
            t + Duration::from_millis(100)
        );
        control.set(Some(2)).unwrap();
        assert_eq!(
            pacer.render_at(t + Duration::from_millis(30)),
            t + Duration::from_millis(500)
        );
        control.set(Some(100)).unwrap();
        assert_eq!(
            pacer.render_at(t + Duration::from_millis(30)),
            t + Duration::from_millis(30)
        );
        pacer.started(t + Duration::from_secs(1));
        assert_eq!(
            pacer.render_at(t + Duration::from_secs(1)),
            t + Duration::from_millis(1010)
        );
        control.set(None).unwrap();
        assert_eq!(pacer.render_at(t), t);
        assert_eq!(control.animation_interval(), Duration::from_millis(16));
    }
    #[test]
    fn dispatch_deadlines_only_wake_for_presentable_work_and_rebase_live() {
        let (control, _rx) = FrameRateControl::new(Some(10));
        let mut pacer = FramePacer::new(control.clone());
        let now = Instant::now();
        let idle = Duration::from_millis(250);
        pacer.started(now);
        assert_eq!(
            pacer.dispatch_timeout(true, now, idle),
            Duration::from_millis(100)
        );
        assert_eq!(pacer.dispatch_timeout(false, now, idle), idle);
        control.set(Some(1)).unwrap();
        assert_eq!(pacer.dispatch_timeout(true, now, idle), idle);
        assert_eq!(
            pacer.dispatch_timeout(true, now + Duration::from_millis(950), idle),
            Duration::from_millis(50)
        );
        control.set(None).unwrap();
        assert_eq!(pacer.dispatch_timeout(true, now, idle), Duration::ZERO);
        assert_eq!(pacer.dispatch_timeout(false, now, idle), idle);
        // A missed deadline admits once; it never replays a backlog of frames.
        control.set(Some(30)).unwrap();
        let later = now + Duration::from_secs(10);
        assert_eq!(pacer.dispatch_timeout(true, later, idle), Duration::ZERO);
        pacer.started(later);
        assert!(pacer.dispatch_timeout(true, later, idle) > Duration::ZERO);
    }

    #[test]
    fn wake_lane_coalesces_and_rejects_closed_renderers_and_zero() {
        let (control, rx) = FrameRateControl::new(None);
        control.set(Some(60)).unwrap();
        assert_eq!(
            control.interval().unwrap(),
            Duration::from_nanos(16_666_667)
        );
        control.set(Some(1)).unwrap();
        assert_eq!(rx.len(), 1);
        assert!(control.set(Some(0)).is_err());
        assert_eq!(control.animation_interval(), Duration::from_secs(1));
        drop(rx);
        assert!(control.set(Some(30)).is_err());
    }
}
