//! The selector knows neither the device nor how a buffer is released. Taking
//! a bundle claims that exact frame; completing a newer one never mutates it.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Id {
    pub generation: u64,
    pub serial: u64,
}
pub(super) struct Frame<T> {
    pub id: Id,
    pub earliest: u64,
    pub value: T,
}
impl<T> Frame<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Frame<U> {
        Frame {
            id: self.id,
            earliest: self.earliest,
            value: f(self.value),
        }
    }
}
#[derive(Default)]
pub(super) struct Latest<T> {
    generation: u64,
    high: u64,
    ready: Option<Frame<T>>,
    opportunity: u64,
    future: Option<Frame<T>>,
}
impl<T> Latest<T> {
    pub fn new() -> Self {
        Self {
            generation: 0,
            high: 0,
            ready: None,
            opportunity: 1,
            future: None,
        }
    }
    pub fn reset(&mut self, generation: u64) -> [Option<Frame<T>>; 2] {
        self.generation = generation;
        self.high = 0;
        self.opportunity = 1;
        [self.take(), self.future.take()]
    }
    pub fn advance(&mut self, opportunity: u64) -> Option<Frame<T>> {
        self.opportunity = self.opportunity.max(opportunity);
        if self
            .future
            .as_ref()
            .is_some_and(|f| f.earliest <= self.opportunity)
        {
            let next = self.future.take().unwrap();
            return self.offer(next);
        }
        None
    }
    /// The returned lease is retired outside the handoff lock. A late
    /// completion cannot wind back even an already-claimed frame.
    pub fn offer(&mut self, frame: Frame<T>) -> Option<Frame<T>> {
        if frame.id.generation != self.generation || frame.id.serial <= self.high {
            return Some(frame);
        }
        if frame.earliest > self.opportunity {
            // At most one opportunity of look-ahead. Never evict the only
            // eligible target with a pose reserved for its successor.
            if frame.earliest != self.opportunity + 1
                || self
                    .future
                    .as_ref()
                    .is_some_and(|f| f.id.serial >= frame.id.serial)
            {
                return Some(frame);
            }
            return self.future.replace(frame);
        }
        self.high = frame.id.serial;
        self.ready.replace(frame)
    }
    pub fn take(&mut self) -> Option<Frame<T>> {
        self.ready.take()
    }
    pub fn pending(&self) -> bool {
        self.ready.is_some()
    }
    #[cfg(any(
        test,
        feature = "stage-probes",
        all(target_os = "linux", feature = "wayland", feature = "vulkan")
    ))]
    pub fn retry(&mut self, frame: Frame<T>) -> Option<Frame<T>> {
        if frame.id.generation == self.generation
            && frame.id.serial == self.high
            && self.ready.is_none()
        {
            self.ready = Some(frame);
            None
        } else {
            Some(frame)
        }
    }
}
/// Draw admission is independent of submission. Two credits allow a ready
/// target and one replacement per handoff, not a queue of missed animation ticks.
pub(super) struct Cadence {
    interval: Option<Duration>,
    last: Option<Instant>,
    credits: u8,
}
impl Cadence {
    pub fn new(interval: Option<Duration>) -> Self {
        Self {
            interval,
            last: None,
            credits: 2,
        }
    }
    pub fn set(&mut self, interval: Option<Duration>) {
        self.interval = interval;
    }
    pub fn advance(&mut self, successor_started: bool) {
        self.credits = 2 - u8::from(successor_started);
    }
    pub fn due(&self, now: Instant) -> Option<Instant> {
        (self.credits != 0).then(|| self.last.zip(self.interval).map_or(now, |(t, d)| t + d))
    }
    pub fn start(&mut self, now: Instant) {
        debug_assert!(self.credits != 0);
        self.credits -= 1;
        self.last = Some(now);
    }
}

#[cfg(test)]
#[path = "presentation/tests.rs"]
mod adapter_tests;
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(generation: u64, serial: u64) -> Frame<(u64, u64)> {
        Frame {
            id: Id { generation, serial },
            earliest: 1,
            value: (serial * 10, serial),
        }
    }
    #[test]
    fn only_completed_targets_compete_and_a_claim_is_immutable() {
        let mut ready = Latest::new();
        ready.reset(1);
        assert!(ready.offer(frame(1, 2)).is_none());
        // C hasn't finished: the presenter can claim B, without a render join.
        let b = ready.take().unwrap();
        ready.offer(frame(1, 3));
        assert_eq!(b.value, (20, 2));
        assert_eq!(ready.retry(b).unwrap().id.serial, 2); // definitely not submitted
        assert_eq!(ready.offer(frame(1, 4)).unwrap().value, (30, 3));
        assert_eq!(ready.take().unwrap().value, (40, 4));
        assert!(ready.offer(frame(1, 3)).is_some());
        assert!(!ready.pending());
        ready.reset(2);
        assert!(ready.offer(frame(1, 5)).is_some());
        ready.offer(frame(2, 1));
        let held = ready.take().unwrap();
        assert!(ready.retry(held).is_none());
        assert_eq!(ready.take().unwrap().value, (10, 1));
    }
    #[test]
    fn missed_draw_periods_do_not_accumulate_credits_or_catch_up_samples() {
        let t = Instant::now();
        let interval = Duration::from_millis(10);
        let mut clock = Cadence::new(Some(interval));
        clock.start(t);
        assert_eq!(clock.due(t), Some(t + interval));
        clock.start(t + interval * 5);
        assert_eq!(clock.due(t + interval * 10), None);
        clock.advance(false);
        assert_eq!(clock.due(t + interval * 5), Some(t + interval * 6));
        clock.set(None);
        assert_eq!(clock.due(t + interval * 5), Some(t + interval * 5));
    }
}
