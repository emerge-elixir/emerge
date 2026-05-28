use std::{ffi::CString, os::raw::c_void, ptr, sync::Arc};

use glutin_egl_sys::egl;
use glutin_egl_sys::egl::types::{
    EGLConfig, EGLContext, EGLDeviceEXT, EGLDisplay, EGLSurface, EGLenum, EGLint,
};
use libloading::Library;
use skia_safe::gpu::{
    direct_contexts,
    gl::{FramebufferInfo, Interface},
};

use crate::{
    backend::skia_gpu::GlFrameSurface,
    renderer::{RenderState, RendererCacheConfig, SceneRenderer},
};

use super::HeadlessRgbaFrame;

const EGL_PLATFORM_SURFACELESS_MESA: EGLenum = 0x31DD;

pub(super) struct GlHeadlessRenderer {
    state: EglHeadlessState,
    renderer: SceneRenderer,
    width: u32,
    height: u32,
}

struct EglHeadlessState {
    _egl_lib: Arc<Library>,
    egl: egl::Egl,
    display: EGLDisplay,
    context: EGLContext,
    surface: EGLSurface,
    frame_surface: Option<GlFrameSurface>,
}

impl GlHeadlessRenderer {
    pub(super) fn new(
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        let dimensions = (width.max(1), height.max(1));
        let (egl_lib, egl) = load_egl()?;
        let state = create_egl_state(egl_lib, egl, dimensions)?;

        Ok(Self {
            state,
            renderer: SceneRenderer::with_cache_config(renderer_cache_config),
            width,
            height,
        })
    }

    pub(super) fn render(&mut self, state: &RenderState) -> Result<HeadlessRgbaFrame, String> {
        if unsafe {
            self.state.egl.MakeCurrent(
                self.state.display,
                self.state.surface,
                self.state.surface,
                self.state.context,
            )
        } == egl::FALSE
        {
            return Err(format!(
                "headless GL eglMakeCurrent failed: {}",
                egl_error(&self.state.egl)
            ));
        }

        let frame_surface = self
            .state
            .frame_surface
            .as_mut()
            .ok_or_else(|| "headless GL surface already destroyed".to_string())?;
        let mut frame = frame_surface.frame();
        let timings = self.renderer.render(&mut frame, state);
        drop(frame);

        let Some((width, height, data)) = frame_surface.capture_rgba_pixels() else {
            return Err("headless GL readback failed".to_string());
        };

        Ok(HeadlessRgbaFrame {
            width: width.min(self.width.max(1)),
            height: height.min(self.height.max(1)),
            data,
            timings,
        })
    }
}

impl Drop for EglHeadlessState {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .egl
                .MakeCurrent(self.display, self.surface, self.surface, self.context);
        }
        drop(self.frame_surface.take());
        unsafe {
            self.egl.MakeCurrent(
                self.display,
                egl::NO_SURFACE,
                egl::NO_SURFACE,
                egl::NO_CONTEXT,
            );
            if self.surface != egl::NO_SURFACE {
                self.egl.DestroySurface(self.display, self.surface);
            }
            if self.context != egl::NO_CONTEXT {
                self.egl.DestroyContext(self.display, self.context);
            }
            if self.display != egl::NO_DISPLAY {
                self.egl.Terminate(self.display);
            }
        }
    }
}

fn create_egl_state(
    egl_lib: Arc<Library>,
    egl: egl::Egl,
    dimensions: (u32, u32),
) -> Result<EglHeadlessState, String> {
    let candidates = display_candidates(&egl);
    if candidates.is_empty() {
        return Err("headless GL could not find an EGL display candidate".to_string());
    }

    let mut errors = Vec::new();
    for candidate in candidates {
        match try_create_egl_state(Arc::clone(&egl_lib), egl.clone(), candidate, dimensions) {
            Ok(state) => return Ok(state),
            Err(err) => errors.push(err),
        }
    }

    Err(format!("headless GL startup failed: {}", errors.join("; ")))
}

fn try_create_egl_state(
    egl_lib: Arc<Library>,
    egl: egl::Egl,
    candidate: DisplayCandidate,
    dimensions: (u32, u32),
) -> Result<EglHeadlessState, String> {
    let display = candidate.display;
    if display == egl::NO_DISPLAY {
        return Err(format!("{} returned EGL_NO_DISPLAY", candidate.label));
    }

    let mut major: EGLint = 0;
    let mut minor: EGLint = 0;
    if unsafe { egl.Initialize(display, &mut major, &mut minor) } == egl::FALSE {
        return Err(format!(
            "{} eglInitialize failed: {}",
            candidate.label,
            egl_error(&egl)
        ));
    }

    let result = init_on_display(&egl, display, dimensions)
        .map(|(context, surface, frame_surface)| EglHeadlessState {
            _egl_lib: egl_lib,
            egl: egl.clone(),
            display,
            context,
            surface,
            frame_surface: Some(frame_surface),
        })
        .map_err(|err| format!("{} {err}", candidate.label));

    if result.is_err() {
        unsafe {
            egl.Terminate(display);
        }
    }

    result
}

fn init_on_display(
    egl: &egl::Egl,
    display: EGLDisplay,
    dimensions: (u32, u32),
) -> Result<(EGLContext, EGLSurface, GlFrameSurface), String> {
    if unsafe { egl.BindAPI(egl::OPENGL_ES_API) } == egl::FALSE {
        return Err(format!("eglBindAPI failed: {}", egl_error(egl)));
    }

    let config = choose_config(egl, display)?;
    let context_attribs: [EGLint; 3] = [
        egl::CONTEXT_CLIENT_VERSION as EGLint,
        2,
        egl::NONE as EGLint,
    ];
    let context =
        unsafe { egl.CreateContext(display, config, egl::NO_CONTEXT, context_attribs.as_ptr()) };
    if context == egl::NO_CONTEXT {
        return Err(format!("eglCreateContext failed: {}", egl_error(egl)));
    }

    let pbuffer_attribs: [EGLint; 5] = [
        egl::WIDTH as EGLint,
        dimensions.0 as EGLint,
        egl::HEIGHT as EGLint,
        dimensions.1 as EGLint,
        egl::NONE as EGLint,
    ];
    let surface = unsafe { egl.CreatePbufferSurface(display, config, pbuffer_attribs.as_ptr()) };
    if surface == egl::NO_SURFACE {
        unsafe {
            egl.DestroyContext(display, context);
        }
        return Err(format!(
            "eglCreatePbufferSurface failed: {}",
            egl_error(egl)
        ));
    }

    if unsafe { egl.MakeCurrent(display, surface, surface, context) } == egl::FALSE {
        unsafe {
            egl.DestroySurface(display, surface);
            egl.DestroyContext(display, context);
        }
        return Err(format!("eglMakeCurrent failed: {}", egl_error(egl)));
    }

    match create_frame_surface(egl, dimensions) {
        Ok(frame_surface) => Ok((context, surface, frame_surface)),
        Err(err) => {
            unsafe {
                egl.MakeCurrent(display, egl::NO_SURFACE, egl::NO_SURFACE, egl::NO_CONTEXT);
                egl.DestroySurface(display, surface);
                egl.DestroyContext(display, context);
            }
            Err(err)
        }
    }
}

fn create_frame_surface(egl: &egl::Egl, dimensions: (u32, u32)) -> Result<GlFrameSurface, String> {
    gl::load_with(|symbol| unsafe {
        let symbol = CString::new(symbol).expect("GL symbol");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    });

    let interface = Interface::new_load_with(|name| unsafe {
        if name == "eglGetCurrentDisplay" {
            return ptr::null();
        }
        let symbol = CString::new(name).expect("egl symbol");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    })
    .ok_or_else(|| "could not create Skia GL interface".to_string())?;

    let gr_context = direct_contexts::make_gl(interface, None)
        .ok_or_else(|| "make_gl failed: could not create Skia direct context".to_string())?;

    let fb_info = {
        let mut fboid: i32 = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

        FramebufferInfo {
            fboid: fboid as u32,
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        }
    };

    GlFrameSurface::try_new(dimensions, fb_info, gr_context, 0, 0)
}

fn choose_config(egl: &egl::Egl, display: EGLDisplay) -> Result<EGLConfig, String> {
    let config_attribs: [EGLint; 13] = [
        egl::SURFACE_TYPE as EGLint,
        egl::PBUFFER_BIT as EGLint,
        egl::RENDERABLE_TYPE as EGLint,
        egl::OPENGL_ES2_BIT as EGLint,
        egl::RED_SIZE as EGLint,
        8,
        egl::GREEN_SIZE as EGLint,
        8,
        egl::BLUE_SIZE as EGLint,
        8,
        egl::ALPHA_SIZE as EGLint,
        8,
        egl::NONE as EGLint,
    ];

    let mut config: EGLConfig = ptr::null();
    let mut num_configs: EGLint = 0;
    if unsafe {
        egl.ChooseConfig(
            display,
            config_attribs.as_ptr(),
            &mut config,
            1,
            &mut num_configs,
        )
    } == egl::FALSE
        || num_configs == 0
    {
        return Err(format!("eglChooseConfig failed: {}", egl_error(egl)));
    }

    Ok(config)
}

#[derive(Clone, Copy)]
struct DisplayCandidate {
    label: &'static str,
    display: EGLDisplay,
}

fn display_candidates(egl: &egl::Egl) -> Vec<DisplayCandidate> {
    device_display_candidates(egl)
        .into_iter()
        .chain(surfaceless_display_candidates(egl))
        .chain(default_display_candidate(egl))
        .collect()
}

fn device_display_candidates(egl: &egl::Egl) -> Vec<DisplayCandidate> {
    if !egl.QueryDevicesEXT.is_loaded() || !egl.GetPlatformDisplayEXT.is_loaded() {
        return Vec::new();
    }

    let mut device_count: EGLint = 0;
    if unsafe { egl.QueryDevicesEXT(0, ptr::null_mut(), &mut device_count) } == egl::FALSE
        || device_count <= 0
    {
        return Vec::new();
    }

    let mut devices = vec![ptr::null(); device_count as usize];
    let mut returned_count: EGLint = 0;
    if unsafe { egl.QueryDevicesEXT(device_count, devices.as_mut_ptr(), &mut returned_count) }
        == egl::FALSE
    {
        return Vec::new();
    }

    devices
        .into_iter()
        .take(returned_count.max(0) as usize)
        .enumerate()
        .map(|(index, device)| DisplayCandidate {
            label: match index {
                0 => "EGL device display 0",
                1 => "EGL device display 1",
                2 => "EGL device display 2",
                _ => "EGL device display",
            },
            display: unsafe {
                egl.GetPlatformDisplayEXT(
                    egl::PLATFORM_DEVICE_EXT,
                    device as EGLDeviceEXT as *mut c_void,
                    ptr::null(),
                )
            },
        })
        .collect()
}

fn surfaceless_display_candidates(egl: &egl::Egl) -> Vec<DisplayCandidate> {
    let ext_candidate = egl
        .GetPlatformDisplayEXT
        .is_loaded()
        .then(|| DisplayCandidate {
            label: "EGL surfaceless display EXT",
            display: unsafe {
                egl.GetPlatformDisplayEXT(
                    EGL_PLATFORM_SURFACELESS_MESA,
                    ptr::null_mut(),
                    ptr::null(),
                )
            },
        });

    let khr_candidate = egl
        .GetPlatformDisplay
        .is_loaded()
        .then(|| DisplayCandidate {
            label: "EGL surfaceless display KHR",
            display: unsafe {
                egl.GetPlatformDisplay(EGL_PLATFORM_SURFACELESS_MESA, ptr::null_mut(), ptr::null())
            },
        });

    ext_candidate.into_iter().chain(khr_candidate).collect()
}

fn default_display_candidate(egl: &egl::Egl) -> Option<DisplayCandidate> {
    egl.GetDisplay.is_loaded().then(|| DisplayCandidate {
        label: "EGL default display",
        display: unsafe { egl.GetDisplay(egl::DEFAULT_DISPLAY) },
    })
}

fn load_egl() -> Result<(Arc<Library>, egl::Egl), String> {
    let lib = Arc::new(
        unsafe { Library::new("libEGL.so.1") }
            .map_err(|e| format!("headless GL failed to load libEGL: {e}"))?,
    );
    let get_proc = unsafe {
        lib.get::<unsafe extern "system" fn(*const std::ffi::c_char) -> *const c_void>(
            b"eglGetProcAddress\0",
        )
        .map_err(|e| format!("headless GL failed to load eglGetProcAddress: {e}"))?
    };

    let egl_lib = Arc::clone(&lib);
    let egl = egl::Egl::load_with(|name| unsafe {
        let symbol = CString::new(name).expect("egl symbol");
        let ptr = get_proc(symbol.as_ptr());
        if !ptr.is_null() {
            return ptr;
        }
        let raw = format!("{name}\0");
        egl_lib
            .get::<*const c_void>(raw.as_bytes())
            .map(|s| *s)
            .unwrap_or(ptr::null())
    });

    Ok((lib, egl))
}

fn egl_error(egl: &egl::Egl) -> String {
    format!("0x{:x}", unsafe { egl.GetError() })
}
