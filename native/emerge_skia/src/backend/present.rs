use std::time::{Duration, Instant};

pub const MIN_PLAUSIBLE_FRAME_INTERVAL: Duration = Duration::from_millis(4);
pub const MAX_PLAUSIBLE_FRAME_INTERVAL: Duration = Duration::from_millis(100);

pub fn plausible_frame_interval(interval: Duration) -> bool {
    interval >= MIN_PLAUSIBLE_FRAME_INTERVAL && interval <= MAX_PLAUSIBLE_FRAME_INTERVAL
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameIntervalEstimator {
    estimated_frame_interval: Duration,
}

impl FrameIntervalEstimator {
    pub fn new(initial_frame_interval: Duration) -> Self {
        Self {
            estimated_frame_interval: initial_frame_interval,
        }
    }

    pub fn observe_interval(&mut self, interval: Duration) -> bool {
        if plausible_frame_interval(interval) {
            self.estimated_frame_interval = interval;
            true
        } else {
            false
        }
    }

    pub fn predict_next_present_after(&self, presented_at: Instant) -> Instant {
        presented_at + self.estimated_frame_interval
    }

    pub fn estimated_frame_interval(&self) -> Duration {
        self.estimated_frame_interval
    }
}

impl Default for FrameIntervalEstimator {
    fn default() -> Self {
        Self::new(Duration::from_millis(16))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentPredictionState {
    last_present_at: Option<Instant>,
    frame_interval: FrameIntervalEstimator,
}

impl PresentPredictionState {
    pub fn new(initial_frame_interval: Duration) -> Self {
        Self {
            last_present_at: None,
            frame_interval: FrameIntervalEstimator::new(initial_frame_interval),
        }
    }

    pub fn observe_present(&mut self, presented_at: Instant) -> Instant {
        if let Some(last_present_at) = self.last_present_at {
            let observed = presented_at.saturating_duration_since(last_present_at);
            self.frame_interval.observe_interval(observed);
        }

        self.last_present_at = Some(presented_at);
        self.predict_next_present_after(presented_at)
    }

    pub fn predict_next_present_after(&self, presented_at: Instant) -> Instant {
        self.frame_interval.predict_next_present_after(presented_at)
    }

    pub fn estimated_frame_interval(&self) -> Duration {
        self.frame_interval.estimated_frame_interval()
    }

    pub fn last_present_at(&self) -> Option<Instant> {
        self.last_present_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimator_accepts_only_plausible_intervals() {
        let mut estimator = FrameIntervalEstimator::new(Duration::from_millis(16));

        assert!(!estimator.observe_interval(Duration::from_millis(3)));
        assert_eq!(
            estimator.estimated_frame_interval(),
            Duration::from_millis(16)
        );

        assert!(estimator.observe_interval(Duration::from_millis(8)));
        assert_eq!(
            estimator.estimated_frame_interval(),
            Duration::from_millis(8)
        );

        assert!(!estimator.observe_interval(Duration::from_millis(101)));
        assert_eq!(
            estimator.estimated_frame_interval(),
            Duration::from_millis(8)
        );
    }

    #[test]
    fn present_prediction_updates_from_observed_present_times() {
        let mut state = PresentPredictionState::new(Duration::from_millis(16));
        let first = Instant::now();

        assert_eq!(
            state.observe_present(first),
            first + Duration::from_millis(16)
        );

        let second = first + Duration::from_millis(12);
        assert_eq!(
            state.observe_present(second),
            second + Duration::from_millis(12)
        );
    }

    #[test]
    fn present_prediction_rejects_implausible_present_times() {
        let mut state = PresentPredictionState::new(Duration::from_millis(16));
        let first = Instant::now();
        state.observe_present(first);

        let second = first + Duration::from_millis(250);
        assert_eq!(
            state.observe_present(second),
            second + Duration::from_millis(16)
        );
    }
}
