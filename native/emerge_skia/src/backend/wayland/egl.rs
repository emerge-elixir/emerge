use std::{ffi::CString, num::NonZeroU32, ptr::NonNull};

use glutin::{
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext},
    display::{Display, DisplayApiPreference, GlDisplay},
    prelude::GlSurface,
    surface::{Surface as GlutinSurface, SurfaceAttributesBuilder, WindowSurface},
};
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use skia_safe::gpu::{
    direct_contexts,
    gl::{FramebufferInfo, Interface},
};
use wayland_client::{Connection, Proxy, protocol::wl_surface};

use crate::{backend::skia_gpu::GlFrameSurface, renderer::SceneRenderer};

pub(super) struct GlEnv {
    pub(super) gl_surface: GlutinSurface<WindowSurface>,
    pub(super) gl_context: PossiblyCurrentContext,
    pub(super) frame_surface: GlFrameSurface,
    pub(super) renderer: SceneRenderer,
}

pub(super) fn create_gl_env(
    conn: &Connection,
    surface: &wl_surface::WlSurface,
    dimensions: (u32, u32),
) -> Result<GlEnv, String> {
    let raw_display_handle = raw_display_handle(conn)?;
    let raw_window_handle = raw_window_handle(surface)?;

    // SAFETY: the Wayland display handle comes from the live connection backend and
    // remains valid for the lifetime of the GL objects created from it.
    let gl_display = unsafe { Display::new(raw_display_handle, DisplayApiPreference::Egl) }
        .map_err(|err| format!("failed to create EGL display: {err}"))?;

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_transparency(true)
        .compatible_with_native_window(raw_window_handle)
        .build();

    // SAFETY: the raw window handle points at the live wl_surface backing the
    // SCTK window, which remains alive while the backend owns the window.
    let gl_config = unsafe { gl_display.find_configs(template) }
        .map_err(|err| format!("failed to enumerate EGL configs: {err}"))?
        .reduce(|accum, cfg| {
            if cfg.num_samples() < accum.num_samples() {
                cfg
            } else {
                accum
            }
        })
        .ok_or_else(|| "could not choose an EGL config".to_string())?;

    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(None))
        .build(Some(raw_window_handle));
    let fallback_context_attributes =
        ContextAttributesBuilder::new().build(Some(raw_window_handle));

    // SAFETY: the config/display/raw handles all originate from the current live
    // Wayland connection and surface, and remain valid while the backend owns them.
    let not_current_gl_context = unsafe {
        gl_display
            .create_context(&gl_config, &context_attributes)
            .or_else(|_| gl_display.create_context(&gl_config, &fallback_context_attributes))
    }
    .map_err(|err| format!("failed to create EGL context: {err}"))?;

    let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(dimensions.0.max(1)).unwrap(),
        NonZeroU32::new(dimensions.1.max(1)).unwrap(),
    );

    // SAFETY: the surface handle is the live wl_surface for this backend window.
    let gl_surface = unsafe { gl_display.create_window_surface(&gl_config, &attrs) }
        .map_err(|err| format!("could not create EGL window surface: {err}"))?;

    let gl_context = not_current_gl_context
        .make_current(&gl_surface)
        .map_err(|err| format!("could not make EGL context current: {err}"))?;

    gl::load_with(|symbol| gl_display.get_proc_address(CString::new(symbol).unwrap().as_c_str()));

    let interface = Interface::new_load_with(|name| {
        if name == "eglGetCurrentDisplay" {
            return std::ptr::null();
        }

        gl_display.get_proc_address(CString::new(name).unwrap().as_c_str())
    })
    .ok_or_else(|| "could not create Skia GL interface".to_string())?;

    let gr_context = direct_contexts::make_gl(interface, None)
        .ok_or_else(|| "make_gl failed: could not create Skia direct context".to_string())?;

    let fb_info = {
        let mut fboid: i32 = 0;

        // SAFETY: GL has been loaded for the current thread and a context is current.
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

        FramebufferInfo {
            fboid: fboid as u32,
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        }
    };

    let frame_surface = GlFrameSurface::new(
        (dimensions.0.max(1), dimensions.1.max(1)),
        fb_info,
        gr_context,
        gl_config.num_samples() as usize,
        gl_config.stencil_size() as usize,
    );

    Ok(GlEnv {
        gl_surface,
        gl_context,
        frame_surface,
        renderer: SceneRenderer::new(),
    })
}

pub(super) fn resize_gl_env(env: &mut GlEnv, dimensions: (u32, u32)) {
    env.gl_surface.resize(
        &env.gl_context,
        NonZeroU32::new(dimensions.0.max(1)).unwrap(),
        NonZeroU32::new(dimensions.1.max(1)).unwrap(),
    );
    env.frame_surface
        .resize((dimensions.0.max(1), dimensions.1.max(1)));
}

fn raw_display_handle(conn: &Connection) -> Result<RawDisplayHandle, String> {
    conn.backend()
        .display_handle()
        .map(|handle| handle.as_raw())
        .map_err(|err| format!("failed to get wayland display handle: {err}"))
}

fn raw_window_handle(surface: &wl_surface::WlSurface) -> Result<RawWindowHandle, String> {
    let ptr = NonNull::new(surface.id().as_ptr().cast())
        .ok_or_else(|| "failed to get wl_surface pointer".to_string())?;

    Ok(RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr)))
}
