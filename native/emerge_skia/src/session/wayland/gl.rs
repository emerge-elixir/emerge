//! EGL owns the wl_buffer attach/commit. Each context is current on exactly
//! one thread. The presenter borrows only resolved, pinned share-group textures.
use super::{
    Owner, Ping,
    gpu::ProbeError,
    handles::*,
    textures::{self, Producer, Renderer},
};
use crate::session::capture::Rect;
use glutin::{
    api::egl::{context::PossiblyCurrentContext, display::Display, surface::Surface as EglSurface},
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, GlContext, NotCurrentGlContext},
    display::GlDisplay,
    prelude::GlSurface,
    surface::{SurfaceAttributesBuilder, SwapInterval, WindowSurface},
};
use skia_safe::{
    AlphaType, BlendMode, ColorType, ImageInfo, Paint, Surface,
    gpu::{
        self, SurfaceOrigin, backend_render_targets, direct_contexts,
        gl::{FramebufferInfo, Interface},
    },
};
use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::Window};
use std::sync::Arc;
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
fn ganesh(display: &Display) -> Result<gpu::DirectContext, String> {
    let interface = Interface::new_load_with(|name| {
        if name == "eglGetCurrentDisplay" {
            std::ptr::null()
        } else {
            display.get_proc_address(&CString::new(name).expect("GL symbol"))
        }
    })
    .ok_or("invalid Ganesh GL interface")?;
    direct_contexts::make_gl(interface, None).ok_or_else(|| "failed to initialize Ganesh GL".into())
}
struct Offscreen {
    context: gpu::DirectContext,
    _egl: PossiblyCurrentContext,
}
impl Renderer for Offscreen {
    type Storage = ();
    fn allocate(&mut self, (w, h): Size) -> Result<(Surface, ()), String> {
        let info = ImageInfo::new(
            (w as i32, h as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        gpu::surfaces::render_target(
            &mut self.context,
            gpu::Budgeted::Yes,
            &info,
            4,
            SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        )
        .map(|s| (s, ()))
        .ok_or_else(|| "cannot allocate 4x GL render target".into())
    }
    fn finish(&mut self, s: &mut Surface, _: &()) -> Result<textures::Image, String> {
        let mut t = gpu::surfaces::get_backend_texture(
            s,
            skia_safe::surface::BackendHandleAccess::FlushRead,
        )
        .ok_or("unshareable GL render target")?;
        self.context
            .flush_and_submit_surface(s, gpu::SyncCpu::Yes)
            .map_err(driver_error)?;
        if self.context.abandoned() {
            return Err("GL drawing context lost".into());
        }
        gpu::backend_textures::gl_texture_parameters_modified(&mut t);
        Ok(textures::Image::Gl(t))
    }
}
impl Gl {
    pub(super) fn new(
        conn: &Connection,
        win: &Window,
        size: Size,
        owner: &Arc<Owner>,
        ping: &Ping,
    ) -> Result<(Self, Producer), ProbeError> {
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
        let attrs = ContextAttributesBuilder::new()
            .with_context_api(egl.context_api())
            .with_sharing(&egl)
            .build(None);
        // SAFETY: live context and config from this same EGL display/share group.
        let draw = unsafe { display.create_context(&config, &attrs) }.map_err(driver_error)?;
        let mut context = ganesh(&display)?;
        if context.max_surface_sample_count_for_color_type(ColorType::RGBA8888) < 4 {
            return Err(ProbeError::Unavailable(
                "RGBA8 4x hardware multisampling is unavailable".into(),
            ));
        }
        let producer = Producer::start(owner.clone(), ping.clone(), move || {
            let egl = draw.make_current_surfaceless().map_err(driver_error)?;
            Ok(Offscreen {
                context: ganesh(&display)?,
                _egl: egl,
            })
        })?;
        let mut id = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut id) };
        let fb = FramebufferInfo {
            fboid: id as u32,
            format: gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        };
        let samples = config.num_samples() as usize;
        let stencil = config.stencil_size() as usize;
        Ok((
            Self {
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
            },
            producer,
        ))
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
    fn borrow(&mut self, t: &gpu::BackendTexture) -> Result<skia_safe::Image, String> {
        gpu::images::borrow_texture_from(
            &mut self.context,
            t,
            SurfaceOrigin::TopLeft,
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        )
        .ok_or_else(|| "cannot sample shared GL texture".into())
    }
    pub(super) fn read(
        &mut self,
        t: &gpu::BackendTexture,
        (x, y, w, h): Rect,
    ) -> Result<Vec<u8>, String> {
        let image = self.borrow(t)?;
        let info = ImageInfo::new(
            (w as i32, h as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        );
        let stride = w as usize * 4;
        let mut rgba = vec![0; stride * h as usize];
        image
            .read_pixels_with_context(
                &mut self.context,
                &info,
                &mut rgba,
                stride,
                (x as i32, y as i32),
                skia_safe::image::CachingHint::Disallow,
            )
            .then_some(rgba)
            .ok_or_else(|| "GL readback failed".into())
    }
    pub(super) fn submit(&mut self, t: &gpu::BackendTexture, size: Size) -> Result<(), String> {
        self.resize(size)?;
        let image = self.borrow(t)?;
        let mut paint = Paint::default();
        paint.set_blend_mode(BlendMode::Src);
        self.surface
            .canvas()
            .draw_image(image, (0, 0), Some(&paint));
        // Release the reader only when this context has finished the final blit.
        // The producer context can be drawing a different slot meanwhile.
        if self
            .context
            .flush_and_submit_surface(&mut self.surface, gpu::SyncCpu::Yes)
            .is_err()
            || self.context.abandoned()
        {
            return Err("GL context lost".into());
        }
        self.drawable.swap_buffers(&self.egl).map_err(driver_error)
    }
}
