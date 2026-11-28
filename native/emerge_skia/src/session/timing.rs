//! Monotonic opportunity predictions, not wire edits or simulated display
//! feedback. Without active tracks this clock never arms a timer.
use super::*;
pub(super) const HEADLESS_PERIOD: Duration = Duration::from_nanos(16_666_667);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tick {
    pub output_generation: u64,
    pub opportunity_id: u64,
    pub sample_at: Instant,
}
#[derive(Default)]
pub(super) struct Clock {
    generation: u64,
    opportunity: u64,
    period: Option<Duration>,
    anchor: Option<Instant>,
    pending: Option<Tick>,
    sampled: Option<Tick>,
    observed: Option<Instant>,
    cost: Duration,
}
impl Clock {
    pub fn reset(&mut self, generation: u64) {
        self.generation = generation;
        self.opportunity = 1;
        self.pending = None;
        self.anchor = None;
        self.observed = None;
    }
    pub fn period(&self) -> Duration {
        self.period.unwrap_or(HEADLESS_PERIOD)
    }
    pub fn cadence(&mut self, d: Duration) {
        self.period = Some(d);
        self.pending = None;
    }
    pub fn opportunity(&self) -> u64 {
        self.opportunity
    }
    pub fn progress(&mut self, at: Instant, work: Option<Duration>) {
        self.opportunity += 1;
        self.observed = Some(at);
        self.pending = None;
        if let Some(d) = work {
            self.cost = if self.cost.is_zero() {
                d
            } else {
                self.cost.mul_f32(0.75) + d.mul_f32(0.25)
            };
        }
    }
    /// Rebase on the actual output clock when available. Handoff and frame
    /// callback times are not evidence of physical scanout.
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub fn feedback(&mut self, at: Instant) {
        self.anchor = Some(at);
        self.pending = None;
    }
    pub fn poll(
        &mut self,
        active: bool,
        now: Instant,
        draw_at: Option<Instant>,
    ) -> (Option<Tick>, Option<Instant>) {
        let Some((due, period)) = active.then_some(draw_at).flatten().zip(self.period) else {
            self.pending = None;
            return (None, None);
        };
        if self.sampled.is_some_and(|t| {
            t.output_generation == self.generation && t.opportunity_id == self.opportunity
        }) {
            return (None, None);
        }
        let lead = self.cost.min(period);
        // Layout can run *before* the next admissible draw. Charging the
        // entire lead after that deadline can turn a 60 Hz cap into 30 Hz.
        let threshold = (now + lead).max(due);
        // Pick one next useful interval, even after a long stall; never enqueue
        // the missed poses. Don't move a still-useful prediction just because
        // the worker woke a little late to prepare it.
        if self
            .pending
            .is_none_or(|p| (due > now && p.sample_at < due) || p.sample_at + period <= now)
        {
            let predicted = if let Some(anchor) = self.anchor {
                // A window has an output phase estimate from its callbacks.
                let elapsed = threshold.saturating_duration_since(anchor);
                let step = period.as_nanos().max(1);
                let count = elapsed.as_nanos().div_ceil(step).max(1);
                anchor + Duration::from_nanos(u64::try_from(count * step).unwrap_or(u64::MAX))
            } else {
                // Headless has no refresh grid to miss: predict from its last
                // delivery. Do not round a few microseconds up to another period.
                (self.observed.unwrap_or(now) + period).max(threshold)
            };
            let at = self
                .sampled
                .map_or(predicted, |p| predicted.max(p.sample_at));
            self.pending = Some(Tick {
                output_generation: self.generation,
                opportunity_id: self.opportunity,
                sample_at: at,
            });
        }
        let tick = self.pending.unwrap();
        let wake = tick.sample_at - lead;
        if now < wake {
            return (None, Some(wake));
        }
        self.sampled = self.pending.take();
        (Some(tick), None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_caps_round_up_even_at_subnanosecond_intervals() {
        assert_eq!(interval(Some(60)).unwrap(), Some(HEADLESS_PERIOD));
        assert_eq!(
            interval(Some(u32::MAX)).unwrap(),
            Some(Duration::from_nanos(1))
        );
        assert_eq!(interval(None).unwrap(), None);
        assert!(interval(Some(0)).is_err());
    }
    #[test]
    fn a_cap_costs_one_period_not_two_when_layout_can_start_before_drawing() {
        let period = HEADLESS_PERIOD;
        let last_draw = Instant::now();
        let lead = Duration::from_micros(200);
        let handed = last_draw + lead / 4;
        let mut c = Clock::default();
        c.reset(1);
        c.cadence(period);
        c.progress(handed, Some(lead));
        let due = last_draw + period;
        let predicted = handed + period;
        let wake = predicted - lead;
        assert!(wake < due);
        assert_eq!(c.poll(true, handed, Some(due)), (None, Some(wake)));
        assert_eq!(
            c.poll(true, wake, Some(due)).0.unwrap().sample_at,
            predicted
        );
    }
    #[test]
    fn predictions_replace_missed_time_and_disarm_without_credit_or_tracks() {
        let start = Instant::now();
        for hz in [60., 120.] {
            let period = Duration::from_secs_f64(1. / hz);
            let mut c = Clock::default();
            c.reset(1);
            c.cadence(period);
            assert_eq!(
                c.poll(true, start, Some(start)),
                (None, Some(start + period))
            );
            let one = c.poll(true, start + period, Some(start)).0.unwrap();
            assert_eq!(one.sample_at, start + period);
            assert_eq!(c.poll(true, start + 10 * period, None), (None, None));
            c.progress(start + 10 * period, Some(period / 4));
            let cap = start + 20 * period;
            let wake = c.poll(true, start + 10 * period, Some(cap)).1.unwrap();
            assert_eq!(wake, cap - period / 4);
            let skipped = c.poll(true, wake, Some(cap)).0.unwrap();
            assert!(skipped.sample_at >= cap);
            assert_eq!(skipped.opportunity_id, one.opportunity_id + 1);
            c.reset(2);
            c.cadence(period / 2);
            let (_, next) = c.poll(true, start, Some(start));
            assert!(next.unwrap() >= skipped.sample_at - period / 2);
            assert_eq!(c.poll(false, start, Some(start)), (None, None));
            assert!(c.pending.is_none());
        }
    }
}
