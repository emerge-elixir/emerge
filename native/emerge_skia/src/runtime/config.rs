//! Revisioned live-control submission. Applied means the supported controls have
//! reached their owners, never that a frame was presented or KMS completed.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crossbeam_channel::Sender;
use rustler::{LocalPid, OwnedEnv};

use crate::actors::EventMsg;

#[derive(Clone, Debug, rustler::NifMap)]
pub(crate) struct LiveConfig {
    pub title: Option<String>,
    pub target_fps: Option<u32>,
    pub input_mask: u32,
    pub drm_power: Option<DrmPower>,
}

#[derive(Clone, Debug, PartialEq, rustler::NifMap)]
pub(crate) struct DrmPower {
    pub display_on: bool,
    pub brightness: Option<f64>,
}

impl DrmPower {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self
            .brightness
            .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err("brightness must be finite and in 0..1 or nil".into());
        }
        Ok(())
    }
}

impl LiveConfig {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.target_fps == Some(0) {
            return Err("target_fps must be positive or nil".into());
        }
        if let Some(power) = &self.drm_power {
            power.validate()?;
        }
        if self.input_mask & !0xff != 0 {
            return Err("input mask contains unknown event categories".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, rustler::NifMap)]
pub(crate) struct Status {
    pub session_id: u64,
    pub desired_revision: u64,
    pub pending_revision: Option<u64>,
    pub applied_revision: Option<u64>,
    pub failed_revision: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ConfigControl {
    state: Mutex<Status>,
    stop: Arc<AtomicBool>,
}

impl ConfigControl {
    pub(crate) fn new(session_id: u64, stop: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(Status {
                session_id,
                ..Status::default()
            }),
            stop,
        })
    }

    /// Queue acceptance and revision publication are serialized. A full/stopped
    /// actor queue does not change desired intent or consume a revision.
    pub(crate) fn submit(
        self: &Arc<Self>,
        config: LiveConfig,
        target: CompletionTarget,
        event_tx: &Sender<EventMsg>,
    ) -> Result<(u64, u64), String> {
        self.submit_to(config, target, |request| {
            event_tx
                .try_send(EventMsg::Configure(request))
                .map_err(|error| match error {
                    crossbeam_channel::TrySendError::Full(_) => {
                        "renderer control queue is full".into()
                    }
                    crossbeam_channel::TrySendError::Disconnected(_) => {
                        "renderer event actor stopped".into()
                    }
                })
        })
    }

    pub(crate) fn submit_to(
        self: &Arc<Self>,
        config: LiveConfig,
        target: CompletionTarget,
        enqueue: impl FnOnce(ConfigRequest) -> Result<(), String>,
    ) -> Result<(u64, u64), String> {
        config.validate()?;
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.stop.load(Ordering::Acquire) {
            return Err("renderer is stopping".into());
        }
        let revision = state
            .desired_revision
            .checked_add(1)
            .ok_or("config revision exhausted")?;
        let token = (state.session_id, revision);
        let request = ConfigRequest {
            config: Arc::new(config),
            token,
            control: Arc::clone(self),
            completion: Arc::new(Completion {
                target,
                sent: AtomicBool::new(false),
                input_done: AtomicBool::new(false),
                output_done: AtomicBool::new(false),
            }),
        };
        enqueue(request)?;
        state.desired_revision = revision;
        state.pending_revision = Some(revision);
        state.failed_revision = None;
        state.error = None;
        Ok(token)
    }

    pub(crate) fn status(&self) -> Status {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if self.stop.load(Ordering::Acquire)
            && let Some(revision) = state.pending_revision.take()
        {
            state.failed_revision = Some(revision);
            state.error = Some("renderer is stopping".into());
        }
        state
    }
}

pub(crate) enum CompletionTarget {
    Beam(LocalPid),
    #[cfg(test)]
    Test(Sender<((u64, u64), Result<(), String>)>),
}

struct Completion {
    target: CompletionTarget,
    sent: AtomicBool,
    input_done: AtomicBool,
    output_done: AtomicBool,
}

#[derive(Clone)]
pub struct ConfigRequest {
    pub(crate) config: Arc<LiveConfig>,
    token: (u64, u64),
    control: Arc<ConfigControl>,
    completion: Arc<Completion>,
}

impl ConfigRequest {
    pub(crate) fn finish_input(&self, result: Result<(), String>) {
        self.finish_part(
            &self.completion.input_done,
            &self.completion.output_done,
            result,
        );
    }

    #[cfg(any(all(feature = "drm", target_os = "linux"), test))]
    pub(crate) fn finish_output(&self, result: Result<(), String>) {
        self.finish_part(
            &self.completion.output_done,
            &self.completion.input_done,
            result,
        );
    }

    fn finish_part(&self, own: &AtomicBool, other: &AtomicBool, result: Result<(), String>) {
        if result.is_err() {
            self.finish(result);
        } else {
            own.store(true, Ordering::SeqCst);
            if other.load(Ordering::SeqCst) {
                self.finish(Ok(()));
            }
        }
    }

    pub(crate) fn apply(
        &self,
        operation: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let state = self.control.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.control.stop.load(Ordering::Acquire) {
            return Err("renderer is stopping".into());
        }
        if state.pending_revision != Some(self.token.1) {
            return Err("config superseded".into());
        }
        operation()
    }

    /// Only call on native owner threads, never a BEAM scheduler. Queue rejection
    /// in the NIF returns its error directly and does not send a completion.
    pub(crate) fn finish(&self, mut result: Result<(), String>) {
        let mut state = self.control.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.completion.sent.swap(true, Ordering::AcqRel) {
            return;
        }
        if state.pending_revision == Some(self.token.1) {
            if self.control.stop.load(Ordering::Acquire) {
                result = Err("renderer is stopping".into());
            }
            state.pending_revision = None;
            match &result {
                Ok(()) => state.applied_revision = Some(self.token.1),
                Err(reason) => {
                    state.failed_revision = Some(self.token.1);
                    state.error = Some(reason.clone());
                }
            }
        } else {
            result = Err("config superseded".into());
        }
        drop(state);
        match &self.completion.target {
            CompletionTarget::Beam(pid) => {
                let _ = OwnedEnv::new().send_and_clear(pid, |env| {
                    use rustler::Encoder;
                    let result = match &result {
                        Ok(()) => crate::atoms::ok().encode(env),
                        Err(reason) => (crate::atoms::error(), reason).encode(env),
                    };
                    (crate::atoms::emerge_skia_config(), self.token, result).encode(env)
                });
            }
            #[cfg(test)]
            CompletionTarget::Test(sender) => {
                let _ = sender.send((self.token, result));
            }
        }
    }
}

#[cfg(any(all(feature = "wayland", target_os = "linux"), test))]
mod inbox;
#[cfg(any(all(feature = "wayland", target_os = "linux"), test))]
pub(crate) use inbox::ConfigInbox;

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::{Receiver, bounded, unbounded};

    fn config() -> LiveConfig {
        LiveConfig {
            title: None,
            target_fps: None,
            input_mask: 1,
            drm_power: None,
        }
    }
    type Replies = Receiver<((u64, u64), Result<(), String>)>;
    fn submit(control: &Arc<ConfigControl>, tx: &Sender<EventMsg>) -> Replies {
        let (reply, rx) = unbounded();
        control
            .submit(config(), CompletionTarget::Test(reply), tx)
            .unwrap();
        rx
    }
    fn request(rx: &Receiver<EventMsg>) -> ConfigRequest {
        match rx.recv().unwrap() {
            EventMsg::Configure(request) => request,
            _ => panic!("expected config"),
        }
    }
    #[test]
    fn application_waits_for_owners_and_stale_revisions_cannot_mutate_or_complete_new_work() {
        let control = ConfigControl::new(7, Arc::new(AtomicBool::new(false)));
        let (tx, rx) = bounded(2);
        let first = submit(&control, &tx);
        let old = request(&rx);
        assert_eq!(control.status().pending_revision, Some(1));
        assert_eq!(control.status().applied_revision, None);
        old.apply(|| Ok(())).unwrap(); // Input applied, backend not yet applied.
        assert_eq!(control.status().applied_revision, None);
        let second = submit(&control, &tx);
        let latest = request(&rx);
        assert!(old.apply(|| panic!("stale side effect")).is_err());
        old.finish(Ok(()));
        assert!(first.recv().unwrap().1.is_err());
        assert_eq!(control.status().pending_revision, Some(2));
        latest.apply(|| Ok(())).unwrap();
        latest.finish(Ok(()));
        latest.finish(Err("duplicate".into()));
        assert_eq!(second.recv().unwrap(), ((7, 2), Ok(())));
        assert!(second.try_recv().is_err());
        assert_eq!(control.status().applied_revision, Some(2));
        assert_eq!(control.status().pending_revision, None);
    }
    #[test]
    fn queue_rejection_and_invalid_values_do_not_advance_desired_revision() {
        let control = ConfigControl::new(9, Arc::new(AtomicBool::new(false)));
        let (tx, _rx) = bounded(1);
        let _first = submit(&control, &tx);
        let (reply, _) = unbounded();
        assert!(
            control
                .submit(config(), CompletionTarget::Test(reply), &tx)
                .is_err()
        );
        assert_eq!(control.status().desired_revision, 1);
        assert!(
            LiveConfig {
                target_fps: Some(0),
                ..config()
            }
            .validate()
            .is_err()
        );
        assert!(
            LiveConfig {
                input_mask: 256,
                ..config()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn mailbox_is_latest_only_and_closure_fails_pending_work() {
        let control = ConfigControl::new(3, Arc::new(AtomicBool::new(false)));
        let (tx, rx) = bounded(2);
        let (inbox, owner) = ConfigInbox::pair();
        let old_reply = submit(&control, &tx);
        inbox.submit(request(&rx)).unwrap();
        let new_reply = submit(&control, &tx);
        inbox.submit(request(&rx)).unwrap();
        assert!(old_reply.recv().unwrap().1.is_err());
        drop(owner);
        assert!(new_reply.recv().unwrap().1.is_err());
        assert_eq!(control.status().failed_revision, Some(2));
        assert_eq!(control.status().pending_revision, None);
        let _reply = submit(&control, &tx);
        assert!(inbox.submit(request(&rx)).is_err());
    }
    #[test]
    fn owner_failure_is_terminal_and_retry_preserves_last_confirmed_revision() {
        let control = ConfigControl::new(5, Arc::new(AtomicBool::new(false)));
        let (tx, rx) = bounded(2);
        let first = submit(&control, &tx);
        request(&rx).finish(Ok(()));
        assert!(first.recv().unwrap().1.is_ok());
        let failed = submit(&control, &tx);
        let work = request(&rx);
        work.finish(work.apply(|| Err("owner failure".into())));
        assert!(failed.recv().unwrap().1.is_err());
        let status = control.status();
        assert_eq!(status.applied_revision, Some(1));
        assert_eq!(status.failed_revision, Some(2));
        assert_eq!(status.error.as_deref(), Some("owner failure"));
        assert_eq!(status.pending_revision, None);
        let retry = submit(&control, &tx);
        request(&rx).finish(Ok(()));
        assert!(retry.recv().unwrap().1.is_ok());
        assert_eq!(control.status().applied_revision, Some(3));
        assert_eq!(control.status().failed_revision, None);
        assert_eq!(control.status().error, None);
    }

    #[test]
    fn disconnected_queues_and_revision_exhaustion_reject_without_acceptance() {
        let control = ConfigControl::new(6, Arc::new(AtomicBool::new(false)));
        let (tx, rx) = bounded(1);
        drop(rx);
        let (reply, _) = unbounded();
        assert!(
            control
                .submit(config(), CompletionTarget::Test(reply), &tx)
                .is_err()
        );
        assert_eq!(control.status().desired_revision, 0);
        let (tx, _rx) = bounded(1);
        control.state.lock().unwrap().desired_revision = u64::MAX;
        let (reply, _) = unbounded();
        assert_eq!(
            control.submit(config(), CompletionTarget::Test(reply), &tx),
            Err("config revision exhausted".into())
        );
        assert_eq!(control.status().desired_revision, u64::MAX);
        assert_eq!(control.status().pending_revision, None);
    }

    #[test]
    fn stopped_and_failed_sessions_never_report_pending_as_applied() {
        let stop = Arc::new(AtomicBool::new(false));
        let control = ConfigControl::new(4, Arc::clone(&stop));
        let (tx, rx) = bounded(2);
        let reply = submit(&control, &tx);
        let request = request(&rx);
        stop.store(true, Ordering::Release);
        assert_eq!(control.status().failed_revision, Some(1));
        assert_eq!(control.status().pending_revision, None);
        assert!(request.apply(|| panic!("stopped side effect")).is_err());
        request.finish(Ok(()));
        assert!(reply.recv().unwrap().1.is_err());
        assert_eq!(control.status().applied_revision, None);
    }
}

#[cfg(test)]
mod power_tests;
