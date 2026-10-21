use crate::runtime::config::{ConfigRequest, LiveConfig};
use std::sync::Arc;

pub(crate) trait BackendWake: Send + Sync {
    fn input_enabled(&self) -> bool {
        true
    }

    fn direct_config_sender(&self) -> Option<crossbeam_channel::Sender<ConfigRequest>> {
        None
    }

    fn validate_config(&self, config: &LiveConfig) -> Result<(), String> {
        if config.title.is_some() || config.target_fps.is_some() || config.drm_power.is_some() {
            return Err("this backend does not support the requested live controls".into());
        }
        Ok(())
    }

    // Called on the input owner, never on a NIF scheduler. Input-only backends
    // have no further owner work; window backends override with a control lane.
    fn configure(&self, request: ConfigRequest) -> Result<(), String> {
        self.validate_config(&request.config)?;
        request.finish(request.apply(|| Ok(())));
        Ok(())
    }

    fn request_stop(&self);

    fn request_redraw(&self);

    fn notify_video_frame(&self);

    fn set_frame_rate(&self, _fps: Option<u32>) -> Result<(), String> {
        Err("this backend does not support live frame caps".into())
    }

    fn set_title(&self, _title: String) -> Result<(), String> {
        Err("this backend does not support window titles".into())
    }
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

    pub(crate) fn input_enabled(&self) -> bool {
        self.inner.input_enabled()
    }

    pub(crate) fn direct_config_sender(&self) -> Option<crossbeam_channel::Sender<ConfigRequest>> {
        self.inner.direct_config_sender()
    }

    pub(crate) fn validate_config(&self, config: &LiveConfig) -> Result<(), String> {
        if config.drm_power.is_some() && self.direct_config_sender().is_none() {
            return Err("DRM power controls require a DRM owner".into());
        }
        self.inner.validate_config(config)
    }

    pub(crate) fn configure(&self, request: ConfigRequest) -> Result<(), String> {
        self.inner.configure(request)
    }

    pub(crate) fn set_frame_rate(&self, fps: Option<u32>) -> Result<(), String> {
        self.inner.set_frame_rate(fps)
    }

    pub(crate) fn set_title(&self, title: String) -> Result<(), String> {
        self.inner.set_title(title)
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
    pub(crate) prime_video_formats: Vec<String>,
    #[cfg(feature = "vulkan")]
    pub(crate) vulkan_device: Option<crate::backend::vulkan::VulkanRendererReport>,
}

#[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
pub(crate) type WindowBackendStartupResult = Result<WindowBackendStartupInfo, String>;

struct NoopWake;

impl BackendWake for NoopWake {
    fn request_stop(&self) {}

    fn request_redraw(&self) {}

    fn notify_video_frame(&self) {}
}
