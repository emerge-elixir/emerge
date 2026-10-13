//! One submission grants one successor, independently of output readiness.
//!
//! Scene snapshots carry their own first-presentation tokens. A prediction can
//! request tree work but cannot anchor a transition or release a display buffer.
//! The dedicated capacity-one lane cannot be starved by a full mutation queue.
use std::{
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::{actors::TreeMsg, backend::wake::BackendWakeHandle};

const RETRY_INTERVAL: Duration = Duration::from_millis(16);

/// Identity belongs to an admitted transition, not its reusable node id.
#[derive(Clone, Debug, Default)]
pub struct PresentationAnchor(Arc<OnceLock<Instant>>);
impl PresentationAnchor {
    pub fn observed_at(&self) -> Option<Instant> {
        self.0.get().copied()
    }
    pub fn observe(&self, at: Instant) {
        let _ = self.0.set(at);
    }
}

#[derive(Debug)]
struct RequestTransport {
    tx: Sender<FrameRequest>,
    busy: AtomicBool,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationRequests(Arc<RequestTransport>);
impl AnimationRequests {
    pub(crate) fn channel() -> (Self, Receiver<FrameRequest>) {
        let (tx, rx) = bounded(1);
        (
            Self(Arc::new(RequestTransport {
                tx,
                busy: AtomicBool::new(false),
            })),
            rx,
        )
    }

    pub(crate) fn frame(
        &self,
        active: bool,
        anchors: Vec<PresentationAnchor>,
        wake: BackendWakeHandle,
    ) -> AnimationFrame {
        if !active && anchors.is_empty() {
            return AnimationFrame::default();
        }
        AnimationFrame(Some(Arc::new(Frame {
            active,
            anchors,
            requests: Arc::downgrade(&self.0),
            state: Mutex::new(RequestState::Fresh),
            wake,
        })))
    }
}

#[derive(Debug)]
enum RequestState {
    Fresh,
    Requested,
    RetryAt(Instant),
    Finished,
}

struct Frame {
    active: bool,
    anchors: Vec<PresentationAnchor>,
    // The actor owns the sender. A queued request must not retain its own
    // channel through a scene snapshot, including when the worker shuts down.
    requests: Weak<RequestTransport>,
    state: Mutex<RequestState>,
    wake: BackendWakeHandle,
}

/// Clone this with the pixels it describes, never with the latest scene on flip.
#[derive(Clone, Default)]
pub struct AnimationFrame(Option<Arc<Frame>>);
impl std::fmt::Debug for AnimationFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationFrame")
            .field("active", &self.active())
            .finish()
    }
}
impl AnimationFrame {
    pub fn active(&self) -> bool {
        self.0.as_ref().is_some_and(|frame| frame.active)
    }

    /// Real backend progress only: KMS flip, compositor callback, or delivery.
    /// This does not request another sample or imply that a buffer is reusable.
    pub fn observed(&self, at: Instant) {
        if let Some(frame) = &self.0 {
            frame.anchors.iter().for_each(|anchor| anchor.observe(at));
        }
    }

    fn request(&self, sample_at: Instant, now: Instant) -> Poll {
        let Some(frame) = &self.0 else {
            return Poll::Finished;
        };
        if !frame.active {
            return Poll::Finished;
        }
        let Some(requests) = frame.requests.upgrade() else {
            return Poll::Finished;
        };
        // The initial pose must reach an output opportunity before look-ahead.
        if frame
            .anchors
            .iter()
            .any(|anchor| anchor.observed_at().is_none())
        {
            return Poll::Waiting(None);
        }
        let mut state = frame
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match *state {
            RequestState::Finished => return Poll::Finished,
            RequestState::Requested => return Poll::Waiting(Some(now + RETRY_INTERVAL)),
            RequestState::RetryAt(at) if now < at => return Poll::Waiting(Some(at)),
            _ => {}
        }
        if requests.busy.swap(true, Ordering::AcqRel) {
            return Poll::Waiting(Some(now + RETRY_INTERVAL));
        }
        *state = RequestState::Requested;
        drop(state);
        let request = FrameRequest {
            frame: self.clone(),
            sample_at: sample_at.max(now),
            completed: false,
        };
        if let Err(error) = requests.tx.try_send(request) {
            // Busy reserves the only slot, so a send error means shutdown.
            error.into_inner().complete(RequestOutcome::Inactive);
            return Poll::Finished;
        }
        Poll::Waiting(Some(now + RETRY_INTERVAL))
    }
}

enum Poll {
    Finished,
    Waiting(Option<Instant>),
}

pub(crate) enum RequestOutcome {
    Scene,
    Inactive,
    RetryAt(Instant),
}

#[derive(Debug)]
pub(crate) struct FrameRequest {
    frame: AnimationFrame,
    sample_at: Instant,
    completed: bool,
}
impl FrameRequest {
    pub(crate) fn message(&self) -> TreeMsg {
        TreeMsg::SampleAnimation {
            sample_at: self.sample_at,
        }
    }

    pub(crate) fn complete(mut self, outcome: RequestOutcome) {
        self.finish(outcome);
    }

    fn finish(&mut self, outcome: RequestOutcome) {
        if self.completed {
            return;
        }
        self.completed = true;
        if let Some(frame) = &self.frame.0 {
            *frame
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = match outcome {
                RequestOutcome::Scene | RequestOutcome::Inactive => RequestState::Finished,
                RequestOutcome::RetryAt(at) => RequestState::RetryAt(at),
            };
            if let Some(requests) = frame.requests.upgrade() {
                requests.busy.store(false, Ordering::Release);
            }
            frame.wake.request_redraw();
        }
    }
}
impl Drop for FrameRequest {
    fn drop(&mut self) {
        // All paths acknowledge, including decode/layout errors and actor stop.
        self.finish(RequestOutcome::Inactive);
    }
}

/// Concrete presenters retain ownership of readiness, deadlines and buffers.
#[derive(Default)]
pub struct FrameProduction {
    pending: Option<(AnimationFrame, Instant)>,
    deadline: Option<Instant>,
}
impl FrameProduction {
    /// Call only for accepted normal work, not cursor commits or late replacements.
    pub fn submitted(&mut self, frame: AnimationFrame, sample_at: Instant) {
        self.pending = frame.active().then_some((frame, sample_at));
        self.deadline = None;
        self.poll(Instant::now());
    }

    /// Rebase a still-unspent bootstrap credit, without granting another one.
    pub fn progress(&mut self, sample_at: Instant) {
        if let Some((_, pending_sample_at)) = &mut self.pending {
            *pending_sample_at = (*pending_sample_at).max(sample_at);
        }
        self.poll(Instant::now());
    }

    pub fn poll(&mut self, now: Instant) {
        self.deadline = None;
        if let Some((frame, sample_at)) = &self.pending {
            match frame.request(*sample_at, now) {
                Poll::Finished => self.pending = None,
                Poll::Waiting(deadline) => self.deadline = deadline,
            }
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn reset(&mut self) {
        self.pending = None;
        self.deadline = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(requests: &AnimationRequests, anchors: Vec<PresentationAnchor>) -> AnimationFrame {
        requests.frame(true, anchors, BackendWakeHandle::noop())
    }

    #[test]
    fn one_successor_is_requested_before_slow_output_completes() {
        let (requests, rx) = AnimationRequests::channel();
        let t = Instant::now();
        let first = frame(&requests, vec![]);
        assert!(matches!(
            first.request(t + Duration::from_millis(85), t),
            Poll::Waiting(Some(_))
        ));
        let request = rx.try_recv().unwrap();
        assert_eq!(request.sample_at, t + Duration::from_millis(85));
        request.complete(RequestOutcome::Scene);
        first.observed(t + Duration::from_millis(85));
        assert!(matches!(first.request(t, t), Poll::Finished));
        assert!(
            rx.is_empty(),
            "completion must not grant a duplicate successor"
        );
    }

    #[test]
    fn initial_pose_waits_for_its_own_progress_not_an_older_frame() {
        let (requests, rx) = AnimationRequests::channel();
        let t = Instant::now();
        let old = frame(&requests, vec![]);
        let anchor = PresentationAnchor::default();
        let new = frame(&requests, vec![anchor.clone()]);
        old.observed(t);
        assert!(matches!(new.request(t, t), Poll::Waiting(None)));
        assert!(rx.is_empty());
        new.observed(t + Duration::from_millis(85));
        new.observed(t + Duration::from_secs(1));
        assert_eq!(anchor.observed_at(), Some(t + Duration::from_millis(85)));
        new.request(t, t + Duration::from_millis(85));
        assert_eq!(rx.len(), 1);
    }

    #[test]
    fn newer_submission_coalesces_behind_the_single_outstanding_request() {
        let (requests, rx) = AnimationRequests::channel();
        let t = Instant::now();
        let first = frame(&requests, vec![]);
        let next = frame(&requests, vec![]);
        first.request(t, t);
        let request = rx.try_recv().unwrap();
        assert!(matches!(next.request(t, t), Poll::Waiting(Some(_))));
        assert!(rx.is_empty());
        request.complete(RequestOutcome::Scene);
        next.request(t, t);
        assert_eq!(rx.len(), 1);
    }

    #[test]
    fn no_change_and_failure_acknowledgements_can_retry_without_another_flip() {
        let (requests, rx) = AnimationRequests::channel();
        let t = Instant::now();
        let frame = frame(&requests, vec![]);
        frame.request(t, t);
        rx.try_recv()
            .unwrap()
            .complete(RequestOutcome::RetryAt(t + RETRY_INTERVAL));
        assert!(matches!(frame.request(t, t), Poll::Waiting(Some(_))));
        assert!(rx.is_empty());
        frame.request(t, t + RETRY_INTERVAL);
        rx.try_recv().unwrap().complete(RequestOutcome::Inactive);
        assert!(matches!(frame.request(t, t), Poll::Finished));
    }

    #[test]
    fn disconnect_releases_queued_frame_and_anchor_ownership() {
        let (requests, rx) = AnimationRequests::channel();
        let frame = frame(&requests, vec![]);
        let weak = Arc::downgrade(frame.0.as_ref().unwrap());
        frame.request(Instant::now(), Instant::now());
        drop(frame);
        // Both ends leave with the actor; queued scenes cannot retain a sender.
        drop(requests);
        drop(rx);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn dropped_requests_and_disconnected_workers_do_not_leave_busy_credits() {
        let (requests, rx) = AnimationRequests::channel();
        let t = Instant::now();
        frame(&requests, vec![]).request(t, t);
        drop(rx.try_recv().unwrap());
        assert!(!requests.0.busy.load(Ordering::Acquire));
        drop(rx);
        assert!(matches!(
            frame(&requests, vec![]).request(t, t),
            Poll::Finished
        ));
        assert!(!requests.0.busy.load(Ordering::Acquire));
    }

    #[test]
    fn retained_frame_does_not_poll_a_stopped_actor() {
        let (requests, rx) = AnimationRequests::channel();
        let frame = frame(&requests, vec![]);
        frame.request(Instant::now(), Instant::now());
        drop(requests);
        drop(rx);
        assert!(matches!(
            frame.request(Instant::now(), Instant::now()),
            Poll::Finished
        ));
    }
}
