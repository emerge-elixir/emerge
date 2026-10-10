use super::ConfigRequest;
use std::sync::{Arc, Mutex};

// The window owns the receiver. Closing it rejects later producers and reports
// the single pending request without relying on another frame or a scene queue.
#[derive(Default)]
struct InboxState {
    pending: Option<ConfigRequest>,
    closed: bool,
}
#[derive(Clone, Default)]
pub(crate) struct ConfigInbox(Arc<Mutex<InboxState>>);
pub(crate) struct ConfigReceiver(ConfigInbox);

impl ConfigInbox {
    pub(crate) fn pair() -> (Self, ConfigReceiver) {
        let inbox = Self::default();
        (inbox.clone(), ConfigReceiver(inbox))
    }

    pub(crate) fn submit(&self, request: ConfigRequest) -> Result<(), String> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed {
            return Err("window control owner stopped".into());
        }
        let previous = state.pending.replace(request);
        drop(state);
        if let Some(previous) = previous {
            previous.finish(Err("config superseded".into()));
        }
        Ok(())
    }
}

impl ConfigReceiver {
    pub(crate) fn take(&self) -> Option<ConfigRequest> {
        self.0
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .take()
    }
}

impl Drop for ConfigReceiver {
    fn drop(&mut self) {
        let mut state = self.0.0.lock().unwrap_or_else(|p| p.into_inner());
        state.closed = true;
        let pending = state.pending.take();
        drop(state);
        if let Some(request) = pending {
            request.finish(Err("window control owner stopped".into()));
        }
    }
}
