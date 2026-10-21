use std::sync::Arc;

#[cfg(all(feature = "wayland", feature = "opengl"))]
use glutin::prelude::GlSurface;
#[cfg(all(feature = "wayland", feature = "opengl"))]
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::Window;
use wayland_client::Connection;

#[cfg(all(feature = "wayland", feature = "opengl"))]
use crate::video::VideoImportContext;
use crate::{
    RenderingApi,
    renderer::{RenderFrame, RenderState, RendererCacheConfig, SceneRenderer},
    video::{VideoCleanupResult, VideoRegistry},
};

#[cfg(all(feature = "wayland", feature = "opengl"))]
use super::egl::{GlEnv, create_gl_env, resize_gl_env};
#[cfg(all(feature = "wayland", feature = "vulkan"))]
use super::vulkan::WaylandVulkanEnv;
#[cfg(all(feature = "wayland", feature = "vulkan"))]
use crate::{
    backend::vulkan::{VulkanRendererReport, capabilities::DrmNodeId},
    video::VulkanVideoImportContext,
};

/// Owns the Wayland GPU renderer and brackets acquire, scene traversal, backend-controlled flush,
/// capture, and presentation. Raster presentation remains separate.
pub(super) enum RendererEnv {
    #[cfg(all(feature = "wayland", feature = "opengl"))]
    OpenGl(GlEnv),
    #[cfg(all(feature = "wayland", feature = "vulkan"))]
    Vulkan(WaylandVulkanEnv),
}

#[cfg(any(
    all(feature = "wayland", feature = "opengl"),
    all(feature = "wayland", feature = "vulkan"),
    test
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RendererEnvKind {
    #[cfg(all(feature = "wayland", feature = "opengl"))]
    OpenGl,
    #[cfg(all(feature = "wayland", feature = "vulkan"))]
    Vulkan,
}

pub(super) enum RendererVideoImportContext {
    #[cfg(all(feature = "wayland", feature = "opengl"))]
    OpenGl(VideoImportContext),
    #[cfg(all(feature = "wayland", feature = "vulkan"))]
    Vulkan(VulkanVideoImportContext),
}

pub(super) struct PresentOutcome {
    pub(super) submitted: bool,
    pub(super) capture: Option<(u32, u32, Vec<u8>)>,
}

#[cfg(any(
    all(feature = "wayland", feature = "opengl"),
    all(feature = "wayland", feature = "vulkan"),
    test
))]
impl RendererEnvKind {
    fn for_api(rendering_api: RenderingApi) -> Result<Self, String> {
        match rendering_api {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            RenderingApi::OpenGl => Ok(Self::OpenGl),
            #[cfg(not(all(feature = "wayland", feature = "opengl")))]
            RenderingApi::OpenGl => {
                Err("OpenGL Wayland rendering support is not available in this build".to_string())
            }
            RenderingApi::Auto => unreachable!("auto is resolved before Wayland startup"),
            RenderingApi::Raster => {
                Err("Wayland raster renderer is not implemented yet".to_string())
            }
            RenderingApi::Metal => Err("Wayland does not support Metal renderer".to_string()),
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            RenderingApi::Vulkan => Ok(Self::Vulkan),
            #[cfg(not(all(feature = "wayland", feature = "vulkan")))]
            RenderingApi::Vulkan => {
                Err("Vulkan rendering support is not available in this build".to_string())
            }
        }
    }

    #[cfg(all(feature = "wayland", feature = "opengl"))]
    fn supports_late_replacement(self, swap_buffers_nonblocking: bool) -> bool {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl => swap_buffers_nonblocking,
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan => false,
        }
    }

    #[cfg(any(
        all(feature = "wayland", feature = "opengl"),
        all(feature = "wayland", feature = "vulkan")
    ))]
    fn requests_frame_callback_before_render(self) -> bool {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl => true,
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan => false,
        }
    }
}

#[cfg(any(
    all(feature = "wayland", feature = "opengl"),
    all(feature = "wayland", feature = "vulkan")
))]
impl RendererEnv {
    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(_) => Ok(()),
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => env.shutdown(),
        }
    }

    pub(super) fn new(
        rendering_api: RenderingApi,
        conn: &Connection,
        window: &Window,
        dimensions: (u32, u32),
        renderer_cache_config: RendererCacheConfig,
        #[cfg(all(feature = "wayland", feature = "vulkan"))] compositor_device: Option<DrmNodeId>,
    ) -> Result<Self, String> {
        match RendererEnvKind::for_api(rendering_api)? {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            RendererEnvKind::OpenGl => {
                create_gl_env(conn, window.wl_surface(), dimensions, renderer_cache_config)
                    .map(Self::OpenGl)
            }
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            RendererEnvKind::Vulkan => WaylandVulkanEnv::new(
                conn,
                window,
                dimensions,
                renderer_cache_config,
                compositor_device,
            )
            .map(Self::Vulkan),
        }
    }

    pub(super) fn requests_frame_callback_before_render(&self) -> bool {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(_) => RendererEnvKind::OpenGl.requests_frame_callback_before_render(),
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(_) => RendererEnvKind::Vulkan.requests_frame_callback_before_render(),
        }
    }

    pub(super) fn supports_late_replacement(&self) -> bool {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => {
                RendererEnvKind::OpenGl.supports_late_replacement(env.swap_buffers_nonblocking)
            }
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => env.supports_late_replacement(),
        }
    }

    pub(super) fn can_skip_unchanged_visible_frame(
        &mut self,
        render_state: &RenderState,
        dimensions: (u32, u32),
    ) -> bool {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => env
                .renderer
                .can_skip_unchanged_visible_frame(render_state, dimensions),
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => env.renderer_mut().is_ok_and(|renderer| {
                renderer.can_skip_unchanged_visible_frame(render_state, dimensions)
            }),
        }
    }

    pub(super) fn render_frame<R>(
        &mut self,
        capture_requested: bool,
        draw: impl FnOnce(&mut SceneRenderer, &mut RenderFrame<'_>) -> R,
    ) -> Result<Option<R>, String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => {
                let result = {
                    let mut frame = env.frame_surface.frame();
                    draw(&mut env.renderer, &mut frame)
                };
                env.pending_capture = capture_requested
                    .then(|| env.frame_surface.capture_rgba_pixels())
                    .flatten();
                Ok(Some(result))
            }
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => env.render_frame(capture_requested, draw),
        }
    }

    pub(super) fn reap_video_cleanup(
        &mut self,
        registry: &Arc<VideoRegistry>,
        video_import_ctx: Option<&RendererVideoImportContext>,
    ) -> Result<VideoCleanupResult, String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => {
                let gl_context = match video_import_ctx {
                    Some(RendererVideoImportContext::OpenGl(context)) => Some(context),
                    #[cfg(all(feature = "wayland", feature = "vulkan"))]
                    Some(RendererVideoImportContext::Vulkan(_)) | None => None,
                    #[cfg(not(all(feature = "wayland", feature = "vulkan")))]
                    None => None,
                };
                let cleanup = env.renderer.reap_video_cleanup(registry, gl_context);
                if cleanup.resources_changed {
                    // Retiring an imported frame deletes raw external GL textures behind Ganesh.
                    // Restore its binding assumptions before any later UI-only redraw.
                    env.frame_surface.reset_context();
                }
                Ok(cleanup)
            }
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => {
                let active = video_import_ctx.is_some_and(|context| {
                    matches!(context, RendererVideoImportContext::Vulkan(_))
                });
                if !active {
                    return Ok(VideoCleanupResult::default());
                }
                match env.renderer_mut()?.reap_vulkan_video_cleanup(registry) {
                    Ok(cleanup) => Ok(cleanup),
                    Err(error) => {
                        if env.device()?.is_device_lost() {
                            env.mark_device_lost();
                        }
                        Err(error)
                    }
                }
            }
        }
    }

    #[cfg(all(feature = "wayland", feature = "vulkan"))]
    pub(super) fn vulkan_renderer_report(&self) -> Result<Option<VulkanRendererReport>, String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(_) => Ok(None),
            Self::Vulkan(env) => env
                .device()
                .map(|device| Some(VulkanRendererReport::for_device(device))),
        }
    }

    pub(super) fn initialize_video_import(&self) -> Result<RendererVideoImportContext, String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(_) => {
                VideoImportContext::new_current().map(RendererVideoImportContext::OpenGl)
            }
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => VulkanVideoImportContext::new(Arc::clone(env.device()?))
                .map(RendererVideoImportContext::Vulkan),
        }
    }

    pub(super) fn resize(&mut self, dimensions: (u32, u32)) -> Result<(), String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => resize_gl_env(env, dimensions),
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => env.resize(dimensions)?,
        }
        self.invalidate_visible_frame_fingerprint();
        Ok(())
    }

    pub(super) fn invalidate_visible_frame_fingerprint(&mut self) {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => env.renderer.invalidate_visible_frame_fingerprint(),
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => {
                if let Ok(renderer) = env.renderer_mut() {
                    renderer.invalidate_visible_frame_fingerprint();
                }
            }
        }
    }

    pub(super) fn present(&mut self) -> Result<PresentOutcome, String> {
        match self {
            #[cfg(all(feature = "wayland", feature = "opengl"))]
            Self::OpenGl(env) => {
                env.gl_surface
                    .swap_buffers(&env.gl_context)
                    .map_err(|err| err.to_string())?;
                Ok(PresentOutcome {
                    submitted: true,
                    capture: env.pending_capture.take(),
                })
            }
            #[cfg(all(feature = "wayland", feature = "vulkan"))]
            Self::Vulkan(env) => env.present().map(|outcome| PresentOutcome {
                submitted: outcome.submitted,
                capture: outcome
                    .capture
                    .map(|capture| (capture.width, capture.height, capture.pixels)),
            }),
        }
    }
}

#[cfg(not(any(
    all(feature = "wayland", feature = "opengl"),
    all(feature = "wayland", feature = "vulkan")
)))]
impl RendererEnv {
    pub(super) fn new(
        _rendering_api: RenderingApi,
        _conn: &Connection,
        _window: &Window,
        _dimensions: (u32, u32),
        _renderer_cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        Err("Wayland GPU rendering support is not available in this build".to_string())
    }

    pub(super) fn requests_frame_callback_before_render(&self) -> bool {
        false
    }

    pub(super) fn supports_late_replacement(&self) -> bool {
        false
    }

    pub(super) fn can_skip_unchanged_visible_frame(
        &mut self,
        _render_state: &RenderState,
        _dimensions: (u32, u32),
    ) -> bool {
        false
    }

    pub(super) fn render_frame<R>(
        &mut self,
        _capture_requested: bool,
        _draw: impl FnOnce(&mut SceneRenderer, &mut RenderFrame<'_>) -> R,
    ) -> Result<Option<R>, String> {
        Err("Wayland GPU rendering support is not available in this build".to_string())
    }

    pub(super) fn reap_video_cleanup(
        &mut self,
        _registry: &Arc<VideoRegistry>,
        _video_import_ctx: Option<&RendererVideoImportContext>,
    ) -> Result<VideoCleanupResult, String> {
        Ok(VideoCleanupResult::default())
    }

    pub(super) fn initialize_video_import(&self) -> Result<RendererVideoImportContext, String> {
        Err("Wayland GPU rendering support is not available in this build".to_string())
    }

    pub(super) fn resize(&mut self, _dimensions: (u32, u32)) -> Result<(), String> {
        Err("Wayland GPU rendering support is not available in this build".to_string())
    }

    pub(super) fn present(&mut self) -> Result<PresentOutcome, String> {
        Err("Wayland GPU rendering support is not available in this build".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_api_dispatch_keeps_opengl_and_feature_gates_vulkan() {
        #[cfg(all(feature = "wayland", feature = "opengl"))]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::OpenGl),
            Ok(RendererEnvKind::OpenGl)
        );
        #[cfg(not(all(feature = "wayland", feature = "opengl")))]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::OpenGl),
            Err("OpenGL Wayland rendering support is not available in this build".to_string())
        );
        #[cfg(all(feature = "wayland", feature = "vulkan"))]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::Vulkan),
            Ok(RendererEnvKind::Vulkan)
        );
        #[cfg(not(all(feature = "wayland", feature = "vulkan")))]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::Vulkan),
            Err("Vulkan rendering support is not available in this build".to_string())
        );
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::Raster),
            Err("Wayland raster renderer is not implemented yet".to_string())
        );
    }

    #[test]
    fn late_replacement_remains_gl_only() {
        #[cfg(all(feature = "wayland", feature = "opengl"))]
        assert!(RendererEnvKind::OpenGl.supports_late_replacement(true));
        #[cfg(all(feature = "wayland", feature = "opengl"))]
        assert!(!RendererEnvKind::OpenGl.supports_late_replacement(false));
    }

    #[test]
    fn vulkan_requests_a_frame_callback_only_after_nonblocking_acquisition() {
        #[cfg(all(feature = "wayland", feature = "opengl"))]
        assert!(RendererEnvKind::OpenGl.requests_frame_callback_before_render());
        #[cfg(all(feature = "wayland", feature = "vulkan"))]
        assert!(!RendererEnvKind::Vulkan.requests_frame_callback_before_render());
    }
}
