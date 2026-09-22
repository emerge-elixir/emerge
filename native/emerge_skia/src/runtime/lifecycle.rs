//! Session health is distinct from cleanup completion. A stopped worker is not a reusable output.
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, rustler::NifMap)]
pub(crate) struct Failure {
    pub reason: String,
    pub scope: String,
    pub recovery: String,
}

#[derive(Clone, Debug, rustler::NifMap)]
pub(crate) struct Status {
    pub session_id: u64,
    pub state: String,
    pub failure: Option<Failure>,
    pub cleanup_complete: bool,
}

#[derive(Default)]
struct Completion {
    result: Option<Result<(), String>>,
    failure: Option<Failure>,
    quarantined: bool,
}

pub(crate) struct Lifecycle {
    pub id: u64,
    pub running: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
    cleanup_started: AtomicBool,
    completion: Mutex<Completion>,
    changed: Condvar,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            id: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            running: Arc::new(AtomicBool::new(true)),
            stop: Arc::new(AtomicBool::new(false)),
            cleanup_started: AtomicBool::new(false),
            completion: Mutex::new(Completion::default()),
            changed: Condvar::new(),
        }
    }
}

impl Lifecycle {
    /// Safe in a resource down/drop callback: only atomic stores. The monitor wakes periodically.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.running.store(false, Ordering::Release);
    }

    pub fn begin_cleanup(&self) -> bool {
        self.request_stop();
        !self.cleanup_started.swap(true, Ordering::AcqRel)
    }

    pub fn fail(&self, reason: impl Into<String>) {
        let mut state = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        state.failure.get_or_insert_with(|| Failure {
            reason: reason.into(),
            scope: "session".into(),
            recovery: "after_cleanup".into(),
        });
        drop(state);
        self.request_stop();
    }

    pub fn quarantine(&self, reason: impl Into<String>) {
        self.quarantine_scoped("process", reason);
    }

    #[cfg(all(feature = "drm-core", target_os = "linux"))]
    pub fn quarantine_output(&self, reason: impl Into<String>) {
        self.quarantine_scoped("output", reason);
    }

    fn quarantine_scoped(&self, scope: &str, reason: impl Into<String>) {
        let mut state = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        state.quarantined = true;
        state.failure = Some(Failure {
            reason: reason.into(),
            scope: scope.into(),
            recovery: "vm_restart".into(),
        });
        drop(state);
        self.request_stop();
        self.changed.notify_all();
    }

    pub fn complete(&self, result: Result<(), String>) {
        if let Err(error) = &result {
            self.quarantine(error.clone());
        }
        let mut state = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        state.result.get_or_insert(result);
        self.changed.notify_all();
    }

    pub fn status(&self) -> Result<Status, String> {
        let state = self
            .completion
            .try_lock()
            .map_err(|_| "renderer status is busy")?;
        Ok(Status {
            session_id: self.id,
            state: if state.quarantined {
                "quarantined"
            } else if state.result.is_some() {
                "stopped"
            } else if self.stop.load(Ordering::Acquire) {
                "stopping"
            } else {
                "running"
            }
            .into(),
            failure: state.failure.clone(),
            cleanup_complete: matches!(state.result, Some(Ok(()))) && !state.quarantined,
        })
    }

    pub fn wait(&self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or("invalid shutdown timeout")?;
        let mut state = self.completion.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.quarantined {
                return Err(state
                    .failure
                    .as_ref()
                    .map(|f| f.reason.clone())
                    .unwrap_or_else(|| "renderer is quarantined".into()));
            }
            if let Some(result) = &state.result {
                return result.clone();
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // A caller timeout does not free resources or start another cleanup job.
                return Err("renderer cleanup is still pending (timeout)".into());
            }
            state = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completion_is_shared_by_repeated_waiters() {
        let lifecycle = Lifecycle::default();
        assert!(lifecycle.begin_cleanup());
        assert!(!lifecycle.begin_cleanup());
        assert!(lifecycle.wait(Duration::ZERO).is_err());
        lifecycle.complete(Ok(()));
        assert!(lifecycle.wait(Duration::ZERO).is_ok());
        assert!(lifecycle.wait(Duration::ZERO).is_ok());
        assert!(lifecycle.status().unwrap().cleanup_complete);
    }
    #[test]
    fn failure_survives_clean_teardown_and_quarantine_is_sticky() {
        let lifecycle = Lifecycle::default();
        lifecycle.fail("tree stopped");
        lifecycle.fail("another worker stopped");
        lifecycle.complete(Ok(()));
        assert_eq!(
            lifecycle.status().unwrap().failure.unwrap().reason,
            "tree stopped"
        );
        lifecycle.quarantine("uncertain GPU ownership");
        lifecycle.complete(Ok(()));
        assert!(!lifecycle.status().unwrap().cleanup_complete);
        assert!(lifecycle.wait(Duration::ZERO).is_err());
    }
}
