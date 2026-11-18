//! EGL owns the wl_buffer attach/commit. Keep every GL/Skia object on this thread.
use super::{gpu::ProbeError, handles::*};
use glutin::{
    api::egl::{context::PossiblyCurrentContext, display::Display, surface::Surface as EglSurface},
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext},
    display::GlDisplay,
    prelude::GlSurface,
    surface::{SurfaceAttributesBuilder, SwapInterval, WindowSurface},
};
use skia_safe::{
    ColorType, Surface,
    gpu::{
        self, SurfaceOrigin, backend_render_targets, direct_contexts,
        gl::{FramebufferInfo, Interface},
    },
};
use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::Window};
use std::{
    ffi::{CStr, CString},
    num::NonZeroU32,
    sync::OnceLock,
};
use wayland_client::Connection;
type Size = (u32, u32);
pub(super) struct Gl {
    // Drop in this order, with the EGL context still current.
    surface: Surface,
    context: gpu::DirectContext,
    drawable: EglSurface<WindowSurface>,
    egl: PossiblyCurrentContext,
    fb: FramebufferInfo,
    samples: usize,
    stencil: usize,
    size: Size,
    _window: Window,
    _connection: Connection,
}
fn wrap(
    ctx: &mut gpu::DirectContext,
    fb: FramebufferInfo,
    samples: usize,
    stencil: usize,
    (w, h): Size,
) -> Result<Surface, String> {
    let target = backend_render_targets::make_gl((w as i32, h as i32), samples, stencil, fb);
    gpu::surfaces::wrap_backend_render_target(
        ctx,
        &target,
        SurfaceOrigin::BottomLeft,
        ColorType::RGBA8888,
        None,
        None,
    )
    .ok_or_else(|| "cannot wrap EGL framebuffer".into())
}
fn dims(size: Size) -> Result<(NonZeroU32, NonZeroU32), String> {
    NonZeroU32::new(size.0)
        .zip(NonZeroU32::new(size.1))
        .ok_or_else(|| "empty EGL drawable".into())
}
fn driver_error(e: impl std::fmt::Display) -> String {
    format!("EGL: {e}")
}
impl Gl {
    pub(super) fn new(conn: &Connection, win: &Window, size: Size) -> Result<Self, ProbeError> {
        let dh = raw_display_handle(conn)?;
        let wh = raw_window_handle(win.wl_surface())?;
        // SAFETY: both the SCTK Window owner (not only its proxy) and the display
        // connection outlive all EGL objects, also on partial construction failure.
        let display = unsafe { Display::new(dh) }.map_err(driver_error)?;
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .compatible_with_native_window(wh)
            .build();
        let configs = unsafe { display.find_configs(template) }.map_err(driver_error)?;
        let config = configs
            .filter(|c| {
                c.alpha_size() == 8
                    && !c.float_pixels()
                    && matches!(
                        c.color_buffer_type(),
                        Some(glutin::config::ColorBufferType::Rgb {
                            r_size: 8,
                            g_size: 8,
                            b_size: 8
                        })
                    )
            })
            .min_by_key(GlConfig::num_samples) // MSAA is on our retained color surface.
            .ok_or_else(|| {
                ProbeError::Unavailable("no compatible RGBA EGL window config".into())
            })?;
        let attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(None))
            .build(Some(wh));
        let desktop = ContextAttributesBuilder::new().build(Some(wh));
        let not_current = unsafe {
            display
                .create_context(&config, &attrs)
                .or_else(|_| display.create_context(&config, &desktop))
        }
        .map_err(driver_error)?;
        let (w, h) = dims(size)?;
        let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(wh, w, h);
        let drawable =
            unsafe { display.create_window_surface(&config, &attrs) }.map_err(driver_error)?;
        let egl = not_current.make_current(&drawable).map_err(driver_error)?;
        // gl::load_with writes process-global function pointers. Pin EGL and only
        // initialize once, even when different viewports start concurrently.
        static BINDINGS: OnceLock<Result<libloading::Library, String>> = OnceLock::new();
        let proc = |s: &str| display.get_proc_address(&CString::new(s).expect("GL symbol"));
        BINDINGS
            .get_or_init(|| {
                let lib =
                    unsafe { libloading::Library::new("libEGL.so.1") }.map_err(driver_error)?;
                gl::load_with(proc);
                Ok(lib)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        fn string(which: u32) -> Result<String, String> {
            let p = unsafe { gl::GetString(which) };
            if p.is_null() {
                return Err("missing GL context identity".into());
            }
            Ok(unsafe { CStr::from_ptr(p.cast()) }
                .to_string_lossy()
                .into_owned())
        }
        let name = string(gl::RENDERER)?;
        let lower = name.to_ascii_lowercase();
        if [
            "llvmpipe",
            "softpipe",
            "swrast",
            "swiftshader",
            "software rasterizer",
        ]
        .iter()
        .any(|s| lower.contains(s))
        {
            return Err(ProbeError::Unavailable(format!(
                "software GL renderer {name}"
            )));
        }
        let device = display.device().map_err(driver_error)?;
        if device.extensions().contains("EGL_MESA_device_software") {
            return Err(ProbeError::Unavailable(format!(
                "software EGL device {name}"
            )));
        }
        let node = device
            .drm_render_device_node_path()
            .or(device.drm_device_node_path())
            .ok_or_else(|| ProbeError::Fatal(format!("unable to verify hardware for {name}")))?;
        eprintln!(
            "Wayland OpenGL hardware: {} / {name} / {} / {node:?}",
            string(gl::VENDOR)?,
            string(gl::VERSION)?
        );
        let _ = drawable.set_swap_interval(&egl, SwapInterval::DontWait);
        let interface = Interface::new_load_with(|name| {
            if name == "eglGetCurrentDisplay" {
                std::ptr::null()
            } else {
                proc(name)
            }
        })
        .ok_or("invalid Ganesh GL interface".to_string())?;
        let mut context = direct_contexts::make_gl(interface, None)
            .ok_or("failed to initialize Ganesh GL".to_string())?;
        let mut id = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut id) };
        let fb = FramebufferInfo {
            fboid: id as u32,
            format: gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        };
        let samples = config.num_samples() as usize;
        let stencil = config.stencil_size() as usize;
        Ok(Self {
            surface: wrap(&mut context, fb, samples, stencil, size)?,
            context,
            drawable,
            egl,
            fb,
            samples,
            stencil,
            size,
            _window: win.clone(),
            _connection: conn.clone(),
        })
    }
    pub(super) fn resize(&mut self, size: Size) -> Result<(), String> {
        if self.size != size {
            let (w, h) = dims(size)?;
            self.drawable.resize(&self.egl, w, h);
            self.surface = wrap(&mut self.context, self.fb, self.samples, self.stencil, size)?;
            self.size = size;
        }
        Ok(())
    }
    pub(super) fn context(&mut self) -> &mut gpu::DirectContext {
        &mut self.context
    }
    pub(super) fn surface(&mut self) -> &mut Surface {
        &mut self.surface
    }
    pub(super) fn swap(&mut self) -> Result<(), String> {
        self.context.flush_surface(&mut self.surface);
        if !self.context.submit(gpu::SyncCpu::No) || self.context.abandoned() {
            return Err("GL context lost".into());
        }
        self.drawable.swap_buffers(&self.egl).map_err(driver_error)
    }
}
