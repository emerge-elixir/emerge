use std::sync::Mutex;

use rustler::LocalPid;

use crate::events::send_log_event;

#[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Default)]
pub struct NativeLogRelay {
    target: Mutex<Option<LocalPid>>,
}

impl NativeLogRelay {
    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    pub fn new(target: Option<LocalPid>) -> Self {
        Self {
            target: Mutex::new(target),
        }
    }

    pub fn set_target(&self, target: Option<LocalPid>) {
        let mut guard = self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = target;
    }

    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    pub fn log(&self, level: NativeLogLevel, source: &'static str, message: impl Into<String>) {
        let target = *self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(pid) = target {
            send_log_event(pid, level, source, &message.into());
        }
    }

    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    pub fn info(&self, source: &'static str, message: impl Into<String>) {
        self.log(NativeLogLevel::Info, source, message);
    }

    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    pub fn warning(&self, source: &'static str, message: impl Into<String>) {
        self.log(NativeLogLevel::Warning, source, message);
    }

    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    pub fn error(&self, source: &'static str, message: impl Into<String>) {
        self.log(NativeLogLevel::Error, source, message);
    }
}
