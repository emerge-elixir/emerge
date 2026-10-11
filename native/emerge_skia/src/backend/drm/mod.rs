//! DRM/KMS presenter dispatch.
//!
//! `core` owns KMS discovery/error policy. Raster uses mapped dumb buffers without GPU libraries;
//! OpenGL and Vulkan remain narrow concrete render owners.

mod core;
mod power;
mod raster;
mod session;
pub(crate) use core::outputs;
mod cursor_theme;
#[cfg(any(feature = "drm", test))]
mod first_frame;
#[cfg(feature = "drm-vulkan")]
pub mod functional_probe;
#[cfg(feature = "drm")]
mod gl;
#[cfg(feature = "drm-vulkan")]
mod vulkan;

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::Sender as StartupSender,
};

use crossbeam_channel::{Receiver, Sender};

use crate::{
    DrmCursorOverrideConfig, LatestFrameStore, RasterPresentKind, RenderingApi,
    actors::{EventMsg, RenderMsg, TreeMsg},
    assets::AssetConfig,
    backend::wake::BackendWake,
    cursor::SharedCursorState,
    events::CursorIcon,
    linux_wait::EventFd,
    native_log::NativeLogRelay,
    renderer::RendererCacheConfig,
    stats::RendererStatsCollector,
    video::VideoRegistry,
};

#[derive(Clone)]
pub(crate) struct DrmBackendWake {
    input_gate: Arc<AtomicBool>,
    power_supported: bool,
    presenter_wake: EventFd,
    input_wake: EventFd,
    control_tx: Sender<crate::runtime::config::ConfigRequest>,
}

impl DrmBackendWake {
    pub(crate) fn new(
        presenter_wake: EventFd,
        input_wake: EventFd,
        control_tx: Sender<crate::runtime::config::ConfigRequest>,
        input_gate: Arc<AtomicBool>,
        power_supported: bool,
    ) -> Self {
        Self {
            presenter_wake,
            input_wake,
            control_tx,
            input_gate,
            power_supported,
        }
    }
}

impl BackendWake for DrmBackendWake {
    fn input_enabled(&self) -> bool {
        self.input_gate.load(Ordering::Acquire)
    }

    fn direct_config_sender(&self) -> Option<Sender<crate::runtime::config::ConfigRequest>> {
        Some(self.control_tx.clone())
    }

    fn validate_config(&self, config: &crate::runtime::config::LiveConfig) -> Result<(), String> {
        if !self.power_supported
            && config
                .drm_power
                .as_ref()
                .is_some_and(|power| !power.display_on || power.brightness.is_some())
        {
            return Err(
                "DRM display/backlight control requires OpenGL or CPU raster presentation".into(),
            );
        }
        if config.title.is_some() || config.target_fps.is_some() {
            return Err("DRM title/frame-rate controls are unsupported".into());
        }
        Ok(())
    }

    fn request_stop(&self) {
        let _ = self.presenter_wake.signal();
        let _ = self.input_wake.signal();
    }

    fn request_redraw(&self) {
        let _ = self.presenter_wake.signal();
    }

    fn notify_video_frame(&self) {
        let _ = self.presenter_wake.signal();
    }
}

#[cfg_attr(not(feature = "drm"), allow(dead_code))]
#[derive(Clone)]
pub(crate) struct DrmRunConfig {
    pub(crate) power: crate::runtime::config::DrmPower,
    pub(crate) backlight: Option<String>,
    pub(crate) backlight_state: rustler::ResourceArc<crate::runtime::backlight::BacklightState>,
    pub(crate) requested_size: Option<(u32, u32)>,
    pub(crate) card_path: Option<String>,
    pub(crate) output: Option<String>,
    pub(crate) mode: Option<String>,
    #[cfg_attr(not(feature = "drm-vulkan"), allow(dead_code))]
    pub(crate) vulkan_drm_node: Option<String>,
    pub(crate) asset_config: AssetConfig,
    pub(crate) startup_retries: u32,
    pub(crate) cursor_overrides: Vec<DrmCursorOverrideConfig>,
    pub(crate) retry_interval_ms: u32,
    pub(crate) force_gpu_finish: bool,
    pub(crate) hw_cursor: bool,
    pub(crate) render_log: bool,
    pub(crate) renderer_stats_log: bool,
    pub(crate) rendering_api: RenderingApi,
    pub(crate) raster_present: RasterPresentKind,
    pub(crate) renderer_cache_config: RendererCacheConfig,
}

pub(crate) struct DrmBackendStartupInfo {
    pub(crate) prime_video_supported: bool,
    pub(crate) prime_video_formats: Vec<String>,
    #[cfg(feature = "vulkan")]
    pub(crate) vulkan_device: Option<crate::backend::vulkan::VulkanRendererReport>,
}

impl DrmBackendStartupInfo {
    pub(crate) fn raster() -> Self {
        Self {
            prime_video_supported: false,
            prime_video_formats: Vec::new(),
            #[cfg(feature = "vulkan")]
            vulkan_device: None,
        }
    }

    #[cfg_attr(not(feature = "drm"), allow(dead_code))]
    pub(crate) fn opengl(prime_video_supported: bool) -> Self {
        Self {
            prime_video_supported,
            prime_video_formats: if prime_video_supported {
                vec!["NV12".to_string(), "ABGR8888".to_string()]
            } else {
                Vec::new()
            },
            #[cfg(feature = "vulkan")]
            vulkan_device: None,
        }
    }
}

#[cfg_attr(not(feature = "drm"), allow(dead_code))]
#[derive(Clone)]
pub(crate) struct DrmRunContext {
    pub(crate) input_gate: Arc<AtomicBool>,
    pub(crate) control_rx: Receiver<crate::runtime::config::ConfigRequest>,
    pub(crate) presenter: Option<power::PresenterControl>,
    pub(crate) lifecycle: Arc<crate::runtime::lifecycle::Lifecycle>,
    pub(crate) startup_tx: StartupSender<Result<DrmBackendStartupInfo, String>>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) running_flag: Arc<AtomicBool>,
    pub(crate) presenter_wake: EventFd,
    pub(crate) input_wake: EventFd,
    pub(crate) tree_tx: Sender<TreeMsg>,
    pub(crate) render_rx: Receiver<RenderMsg>,
    pub(crate) cursor_icon_rx: Receiver<CursorIcon>,
    pub(crate) cursor_state: Arc<SharedCursorState>,
    pub(crate) event_tx: Sender<EventMsg>,
    pub(crate) screen_tx: Sender<(u32, u32)>,
    pub(crate) render_counter: Arc<AtomicU64>,
    pub(crate) native_log: Arc<NativeLogRelay>,
    pub(crate) stats: Option<Arc<RendererStatsCollector>>,
    pub(crate) latest_frame: Arc<LatestFrameStore>,
    pub(crate) video_registry: Arc<VideoRegistry>,
}

pub(crate) fn run(context: DrmRunContext, config: DrmRunConfig) {
    power::run(context, config);
}

fn run_presenter(context: DrmRunContext, config: DrmRunConfig) {
    match config.rendering_api {
        RenderingApi::Vulkan => {
            #[cfg(feature = "drm-vulkan")]
            vulkan::run(context, config);
            #[cfg(not(feature = "drm-vulkan"))]
            reject_unavailable_runner(
                context,
                "Vulkan rendering support is not available in this build".to_string(),
            );
        }
        RenderingApi::Raster if config.raster_present != RasterPresentKind::GpuUpload => {
            raster::run(context, config);
        }
        RenderingApi::Auto | RenderingApi::OpenGl | RenderingApi::Raster => {
            #[cfg(feature = "drm")]
            gl::run(context, config);
            #[cfg(not(feature = "drm"))]
            reject_unavailable_runner(
                context,
                "OpenGL DRM rendering support is not available in this build".to_string(),
            );
        }
        RenderingApi::Metal => reject_unavailable_runner(
            context,
            "rendering_api :metal is only supported with backend :macos".to_string(),
        ),
    }
}

fn reject_unavailable_runner(context: DrmRunContext, error: String) {
    let _ = context.startup_tx.send(Err(error));
    context.running_flag.store(false, Ordering::Release);
}

#[cfg(test)]
mod power_admission_tests {
    use super::*;
    use crate::runtime::config::{DrmPower, LiveConfig};
    #[test]
    fn unsupported_power_requests_are_rejected_before_admission() {
        for supported in [false, true] {
            let wake = DrmBackendWake::new(
                EventFd::new().unwrap(),
                EventFd::new().unwrap(),
                crossbeam_channel::bounded(1).0,
                Arc::default(),
                supported,
            );
            let mut config = LiveConfig {
                title: None,
                target_fps: None,
                input_mask: 0,
                drm_power: Some(DrmPower {
                    display_on: false,
                    brightness: Some(0.5),
                }),
            };
            assert_eq!(wake.validate_config(&config).is_ok(), supported);
            config.drm_power = Some(DrmPower {
                display_on: true,
                brightness: None,
            });
            assert!(wake.validate_config(&config).is_ok());
        }
    }
}
