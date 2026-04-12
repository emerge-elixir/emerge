use std::sync::Arc;

pub(crate) trait BackendWake: Send + Sync {
    fn request_stop(&self);

    fn request_redraw(&self);

    fn notify_video_frame(&self);
}

#[derive(Clone)]
pub(crate) struct BackendWakeHandle {
    inner: Arc<dyn BackendWake>,
}

impl BackendWakeHandle {
    pub(crate) fn new<W>(wake: W) -> Self
    where
        W: BackendWake + 'static,
    {
        Self {
            inner: Arc::new(wake),
        }
    }

    pub(crate) fn noop() -> Self {
        Self::new(NoopWake)
    }

    pub(crate) fn request_stop(&self) {
        self.inner.request_stop();
    }

    pub(crate) fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    pub(crate) fn notify_video_frame(&self) {
        self.inner.notify_video_frame();
    }
}

impl Default for BackendWakeHandle {
    fn default() -> Self {
        Self::noop()
    }
}

impl std::panic::RefUnwindSafe for BackendWakeHandle {}

impl std::panic::UnwindSafe for BackendWakeHandle {}

#[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
pub(crate) struct WindowBackendStartupInfo {
    pub(crate) wake: BackendWakeHandle,
    pub(crate) prime_video_supported: bool,
}

#[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
pub(crate) type WindowBackendStartupResult = Result<WindowBackendStartupInfo, String>;

struct NoopWake;

impl BackendWake for NoopWake {
    fn request_stop(&self) {}

    fn request_redraw(&self) {}

    fn notify_video_frame(&self) {}
}
