use std::sync::Arc;

#[cfg(feature = "wayland")]
use glutin::prelude::GlSurface;
use wayland_client::{Connection, protocol::wl_surface};

#[cfg(feature = "wayland")]
use crate::video::VideoImportContext;
use crate::{
    RenderingApi,
    renderer::{RenderFrame, RenderState, RendererCacheConfig, SceneRenderer},
    video::{VideoCleanupResult, VideoRegistry},
};

#[cfg(feature = "wayland")]
use super::egl::{GlEnv, create_gl_env, resize_gl_env};
#[cfg(feature = "wayland-vulkan")]
use super::vulkan::WaylandVulkanEnv;
#[cfg(feature = "wayland-vulkan")]
use crate::{
    backend::vulkan::{VulkanRendererReport, capabilities::DrmNodeId},
    video::VulkanVideoImportContext,
};

/// Owns the Wayland GPU renderer and brackets acquire, scene traversal, backend-controlled flush,
/// capture, and presentation. Raster presentation remains separate.
pub(super) enum RendererEnv {
    #[cfg(feature = "wayland")]
    OpenGl(GlEnv),
    #[cfg(feature = "wayland-vulkan")]
    Vulkan(WaylandVulkanEnv),
}

#[cfg(any(feature = "wayland", feature = "wayland-vulkan", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RendererEnvKind {
    #[cfg(feature = "wayland")]
    OpenGl,
    #[cfg(feature = "wayland-vulkan")]
    Vulkan,
}

pub(super) enum RendererVideoImportContext {
    #[cfg(feature = "wayland")]
    OpenGl(VideoImportContext),
    #[cfg(feature = "wayland-vulkan")]
    Vulkan(VulkanVideoImportContext),
}

pub(super) struct PresentOutcome {
    pub(super) submitted: bool,
    pub(super) capture: Option<(u32, u32, Vec<u8>)>,
}

#[cfg(any(feature = "wayland", feature = "wayland-vulkan", test))]
impl RendererEnvKind {
    fn for_api(rendering_api: RenderingApi) -> Result<Self, String> {
        match rendering_api {
            #[cfg(feature = "wayland")]
            RenderingApi::OpenGl => Ok(Self::OpenGl),
            #[cfg(not(feature = "wayland"))]
            RenderingApi::OpenGl => {
                Err("OpenGL Wayland rendering support is not available in this build".to_string())
            }
            RenderingApi::Auto => unreachable!("auto is resolved before Wayland startup"),
            RenderingApi::Raster => {
                Err("Wayland raster renderer is not implemented yet".to_string())
            }
            RenderingApi::Metal => Err("Wayland does not support Metal renderer".to_string()),
            #[cfg(feature = "wayland-vulkan")]
            RenderingApi::Vulkan => Ok(Self::Vulkan),
            #[cfg(not(feature = "wayland-vulkan"))]
            RenderingApi::Vulkan => {
                Err("Vulkan rendering support is not available in this build".to_string())
            }
        }
    }

    #[cfg(feature = "wayland")]
    fn supports_late_replacement(self, swap_buffers_nonblocking: bool) -> bool {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl => swap_buffers_nonblocking,
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan => false,
        }
    }

    #[cfg(any(feature = "wayland", feature = "wayland-vulkan"))]
    fn requests_frame_callback_before_render(self) -> bool {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl => true,
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan => false,
        }
    }
}

#[cfg(any(feature = "wayland", feature = "wayland-vulkan"))]
impl RendererEnv {
    pub(super) fn new(
        rendering_api: RenderingApi,
        conn: &Connection,
        surface: &wl_surface::WlSurface,
        dimensions: (u32, u32),
        renderer_cache_config: RendererCacheConfig,
        #[cfg(feature = "wayland-vulkan")] compositor_device: Option<DrmNodeId>,
    ) -> Result<Self, String> {
        match RendererEnvKind::for_api(rendering_api)? {
            #[cfg(feature = "wayland")]
            RendererEnvKind::OpenGl => {
                create_gl_env(conn, surface, dimensions, renderer_cache_config).map(Self::OpenGl)
            }
            #[cfg(feature = "wayland-vulkan")]
            RendererEnvKind::Vulkan => WaylandVulkanEnv::new(
                conn,
                surface,
                dimensions,
                renderer_cache_config,
                compositor_device,
            )
            .map(Self::Vulkan),
        }
    }

    pub(super) fn requests_frame_callback_before_render(&self) -> bool {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(_) => RendererEnvKind::OpenGl.requests_frame_callback_before_render(),
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(_) => RendererEnvKind::Vulkan.requests_frame_callback_before_render(),
        }
    }

    pub(super) fn supports_late_replacement(&self) -> bool {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(env) => {
                RendererEnvKind::OpenGl.supports_late_replacement(env.swap_buffers_nonblocking)
            }
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(env) => env.supports_late_replacement(),
        }
    }

    pub(super) fn can_skip_unchanged_visible_frame(
        &mut self,
        render_state: &RenderState,
        dimensions: (u32, u32),
    ) -> bool {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(env) => env
                .renderer
                .can_skip_unchanged_visible_frame(render_state, dimensions),
            #[cfg(feature = "wayland-vulkan")]
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
            #[cfg(feature = "wayland")]
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
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(env) => env.render_frame(capture_requested, draw),
        }
    }

    pub(super) fn reap_video_cleanup(
        &mut self,
        registry: &Arc<VideoRegistry>,
        video_import_ctx: Option<&RendererVideoImportContext>,
    ) -> Result<VideoCleanupResult, String> {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(env) => {
                let gl_context = match video_import_ctx {
                    Some(RendererVideoImportContext::OpenGl(context)) => Some(context),
                    #[cfg(feature = "wayland-vulkan")]
                    Some(RendererVideoImportContext::Vulkan(_)) | None => None,
                    #[cfg(not(feature = "wayland-vulkan"))]
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
            #[cfg(feature = "wayland-vulkan")]
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

    #[cfg(feature = "wayland-vulkan")]
    pub(super) fn vulkan_renderer_report(&self) -> Result<Option<VulkanRendererReport>, String> {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(_) => Ok(None),
            Self::Vulkan(env) => env
                .device()
                .map(|device| Some(VulkanRendererReport::for_device(device))),
        }
    }

    pub(super) fn initialize_video_import(&self) -> Result<RendererVideoImportContext, String> {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(_) => {
                VideoImportContext::new_current().map(RendererVideoImportContext::OpenGl)
            }
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(env) => VulkanVideoImportContext::new(Arc::clone(env.device()?))
                .map(RendererVideoImportContext::Vulkan),
        }
    }

    pub(super) fn resize(&mut self, dimensions: (u32, u32)) -> Result<(), String> {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(env) => resize_gl_env(env, dimensions),
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(env) => env.resize(dimensions)?,
        }
        self.invalidate_visible_frame_fingerprint();
        Ok(())
    }

    pub(super) fn invalidate_visible_frame_fingerprint(&mut self) {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(env) => env.renderer.invalidate_visible_frame_fingerprint(),
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(env) => {
                if let Ok(renderer) = env.renderer_mut() {
                    renderer.invalidate_visible_frame_fingerprint();
                }
            }
        }
    }

    pub(super) fn present(&mut self) -> Result<PresentOutcome, String> {
        match self {
            #[cfg(feature = "wayland")]
            Self::OpenGl(env) => {
                env.gl_surface
                    .swap_buffers(&env.gl_context)
                    .map_err(|err| err.to_string())?;
                Ok(PresentOutcome {
                    submitted: true,
                    capture: env.pending_capture.take(),
                })
            }
            #[cfg(feature = "wayland-vulkan")]
            Self::Vulkan(env) => env.present().map(|outcome| PresentOutcome {
                submitted: outcome.submitted,
                capture: outcome
                    .capture
                    .map(|capture| (capture.width, capture.height, capture.pixels)),
            }),
        }
    }
}

#[cfg(not(any(feature = "wayland", feature = "wayland-vulkan")))]
impl RendererEnv {
    pub(super) fn new(
        _rendering_api: RenderingApi,
        _conn: &Connection,
        _surface: &wl_surface::WlSurface,
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
        #[cfg(feature = "wayland")]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::OpenGl),
            Ok(RendererEnvKind::OpenGl)
        );
        #[cfg(not(feature = "wayland"))]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::OpenGl),
            Err("OpenGL Wayland rendering support is not available in this build".to_string())
        );
        #[cfg(feature = "wayland-vulkan")]
        assert_eq!(
            RendererEnvKind::for_api(RenderingApi::Vulkan),
            Ok(RendererEnvKind::Vulkan)
        );
        #[cfg(not(feature = "wayland-vulkan"))]
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
        #[cfg(feature = "wayland")]
        assert!(RendererEnvKind::OpenGl.supports_late_replacement(true));
        #[cfg(feature = "wayland")]
        assert!(!RendererEnvKind::OpenGl.supports_late_replacement(false));
    }

    #[test]
    fn vulkan_requests_a_frame_callback_only_after_nonblocking_acquisition() {
        #[cfg(feature = "wayland")]
        assert!(RendererEnvKind::OpenGl.requests_frame_callback_before_render());
        #[cfg(feature = "wayland-vulkan")]
        assert!(!RendererEnvKind::Vulkan.requests_frame_callback_before_render());
    }
}
