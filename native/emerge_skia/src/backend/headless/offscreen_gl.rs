use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, OwnedFd},
        raw::c_void,
    },
    ptr,
    sync::Arc,
    time::{Duration, Instant},
};

use gbm::{BufferObject, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat};
use glutin_egl_sys::egl;
use glutin_egl_sys::egl::types::{
    EGLAttrib, EGLConfig, EGLContext, EGLDeviceEXT, EGLDisplay, EGLSurface, EGLenum, EGLint,
};
use libloading::Library;
use skia_safe::gpu::{
    SurfaceOrigin, direct_contexts,
    gl::{FramebufferInfo, Interface},
};

use video_interop::{AcquireSync, SyncFile, egl::NativeFenceFunctions};

use crate::{
    backend::skia_gpu::GlFrameSurface,
    renderer::{RenderState, RendererCacheConfig, SceneRenderer},
};

use super::{
    HeadlessPrimeExport, HeadlessPrimeTimings, HeadlessRgbaFrame, PrimeObjectMeta, PrimePlaneMeta,
};

const EGL_PLATFORM_SURFACELESS_MESA: EGLenum = 0x31DD;
const EGL_WIDTH: EGLenum = 0x3057;
const EGL_HEIGHT: EGLenum = 0x3056;
const EGL_LINUX_DMA_BUF_EXT: EGLenum = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: EGLenum = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: EGLenum = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: EGLenum = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: EGLenum = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: EGLenum = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: EGLenum = 0x3444;
const EGL_DEVICE_EXT: EGLenum = 0x322C;
const EGL_DRM_DEVICE_FILE_EXT: EGLint = 0x3233;
const EGL_DRM_RENDER_NODE_FILE_EXT: EGLint = 0x3377;
const DRM_FORMAT_MOD_LINEAR: u64 = 0;
const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

type GlEglImageTargetTexture2DOes = unsafe extern "system" fn(u32, *const c_void);

struct PrimeExportState {
    gbm: GbmDevice<File>,
    image_target_texture_2d_oes: GlEglImageTargetTexture2DOes,
    sync_mode: PrimeSyncMode,
    deferred_sync_destroy: DeferredCleanupQueue<DeferredProducerSync>,
    max_in_flight: usize,
    next_release_id: u64,
    available: Vec<PrimeFrameSlot>,
    in_flight: HashMap<u64, PrimeFrameSlot>,
}

#[derive(Clone, Copy, Debug)]
enum PrimeSyncMode {
    Explicit(NativeFenceFunctions),
    ImplicitFallback,
}

struct PrimeSynchronization {
    acquire_sync: AcquireSync,
    owned_fence: Option<OwnedFd>,
    fence_export: Option<Duration>,
    gpu_finish_fallback: Option<Duration>,
}

struct PrimeSyncContext<'a> {
    mode: &'a mut PrimeSyncMode,
    deferred_sync_destroy: &'a mut DeferredCleanupQueue<DeferredProducerSync>,
    egl: &'a egl::Egl,
    display: EGLDisplay,
}

struct DeferredProducerSync {
    functions: NativeFenceFunctions,
    handle: video_interop::egl::SyncHandle,
}

struct DeferredCleanupQueue<T> {
    pending: Vec<T>,
}

impl<T> Default for DeferredCleanupQueue<T> {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
        }
    }
}

impl<T> DeferredCleanupQueue<T> {
    fn push(&mut self, value: T) {
        self.pending.push(value);
    }

    fn retry_with(&mut self, mut retry: impl FnMut(T) -> Result<(), T>) {
        self.pending = std::mem::take(&mut self.pending)
            .into_iter()
            .filter_map(|value| retry(value).err())
            .collect();
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn into_pending(self) -> Vec<T> {
        self.pending
    }
}

struct PrimeFrameSlot {
    surface: ExportSurface,
    fds: Vec<OwnedFd>,
    object_sizes: Vec<u64>,
    acquire_fence: Option<OwnedFd>,
}

pub(super) struct GlHeadlessRenderer {
    state: EglHeadlessState,
    renderer: SceneRenderer,
    width: u32,
    height: u32,
    prime: Option<PrimeExportState>,
    terminal_error: Option<String>,
}

struct EglHeadlessState {
    _egl_lib: Arc<Library>,
    egl: egl::Egl,
    display: EGLDisplay,
    context: EGLContext,
    surface: EGLSurface,
    frame_surface: Option<GlFrameSurface>,
    // Unrecoverable sync handles remain owned until eglTerminate destroys the display.
    final_sync_handles: Vec<video_interop::egl::SyncHandle>,
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

        Ok(Self::from_state(
            state,
            width,
            height,
            renderer_cache_config,
        ))
    }

    pub(super) fn new_prime(
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
        max_in_flight: u32,
    ) -> Result<Self, String> {
        let dimensions = (width.max(1), height.max(1));
        let (egl_lib, egl) = load_egl()?;
        let candidates = display_candidates(&egl);
        if candidates.is_empty() {
            return Err("headless PRIME could not find an EGL display candidate".to_string());
        }

        let mut errors = Vec::new();
        for candidate in candidates {
            match try_create_egl_state(Arc::clone(&egl_lib), egl.clone(), candidate, dimensions) {
                Ok(state) => {
                    let mut renderer =
                        Self::from_state(state, width, height, renderer_cache_config);
                    match renderer.enable_prime_export(max_in_flight) {
                        Ok(()) => return Ok(renderer),
                        Err(err) => errors.push(err),
                    }
                }
                Err(err) => errors.push(err),
            }
        }

        Err(format!(
            "headless PRIME startup failed for every EGL/GBM candidate: {}",
            errors.join("; ")
        ))
    }

    fn from_state(
        state: EglHeadlessState,
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
    ) -> Self {
        Self {
            state,
            renderer: SceneRenderer::with_cache_config(renderer_cache_config),
            width,
            height,
            prime: None,
            terminal_error: None,
        }
    }

    fn enable_prime_export(&mut self, max_in_flight: u32) -> Result<(), String> {
        self.make_current()?;
        let image_target_texture_2d_oes = load_egl_proc::<GlEglImageTargetTexture2DOes>(
            &self.state.egl,
            "glEGLImageTargetTexture2DOES",
        )?;
        let prime_device = open_prime_gbm_device(
            &self.state.egl,
            self.state.display,
            image_target_texture_2d_oes,
            (self.width.max(1), self.height.max(1)),
        );
        if let Some(frame_surface) = self.state.frame_surface.as_mut() {
            frame_surface.reset_context();
        }
        let (gbm, probe) = prime_device?;
        let probe =
            create_prime_frame_slot_from_surface(&self.state.egl, self.state.display, probe)?;
        let sync_mode = select_prime_sync_mode(&mut self.state)?;
        eprintln!(
            "headless PRIME acquire synchronization: {}",
            match sync_mode {
                PrimeSyncMode::Explicit(_) => "explicit EGL native fence",
                PrimeSyncMode::ImplicitFallback => "implicit glFinish fallback",
            }
        );
        self.prime = Some(PrimeExportState {
            gbm,
            image_target_texture_2d_oes,
            sync_mode,
            deferred_sync_destroy: DeferredCleanupQueue::default(),
            max_in_flight: max_in_flight.max(1) as usize,
            next_release_id: 1,
            available: vec![probe],
            in_flight: HashMap::new(),
        });
        Ok(())
    }

    pub(super) fn render_binary(
        &mut self,
        state: &RenderState,
    ) -> Result<HeadlessRgbaFrame, String> {
        self.make_current()?;

        let frame_surface = self
            .state
            .frame_surface
            .as_mut()
            .ok_or_else(|| "headless GL surface already destroyed".to_string())?;
        let mut frame = frame_surface.frame();
        let timings = self.renderer.render(&mut frame, state);

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

    pub(super) fn render_prime(
        &mut self,
        state: &RenderState,
    ) -> Result<Option<HeadlessPrimeExport>, String> {
        let prepare_started_at = Instant::now();
        if let Some(error) = self.terminal_error.clone() {
            if self.make_current().is_ok() {
                self.retry_deferred_prime_sync_destroy();
            }
            return Err(error);
        }
        if let Err(error) = self.make_current() {
            self.terminal_error = Some(error.clone());
            return Err(error);
        }
        self.retry_deferred_prime_sync_destroy();
        let dimensions = (self.width.max(1), self.height.max(1));
        let prime = self
            .prime
            .as_mut()
            .ok_or_else(|| "headless GL PRIME export was not initialized".to_string())?;
        if prime.in_flight.len() >= prime.max_in_flight {
            return Ok(None);
        }

        let mut slot = match prime.available.pop() {
            Some(slot) => slot,
            None => {
                let slot = create_prime_frame_slot(
                    &self.state.egl,
                    self.state.display,
                    &prime.gbm,
                    prime.image_target_texture_2d_oes,
                    dimensions,
                );
                if let Some(frame_surface) = self.state.frame_surface.as_mut() {
                    frame_surface.reset_context();
                }
                match slot {
                    Ok(slot) => slot,
                    Err(error) => {
                        self.terminal_error = Some(error.clone());
                        return Err(error);
                    }
                }
            }
        };
        let release_id = prime.next_release_id;
        prime.next_release_id = prime.next_release_id.wrapping_add(1).max(1);
        let Some(frame_surface) = self.state.frame_surface.as_mut() else {
            prime.available.push(slot);
            let error = "headless GL surface already destroyed".to_string();
            self.terminal_error = Some(error.clone());
            return Err(error);
        };
        let prepare = prepare_started_at.elapsed();
        debug_assert!(slot.acquire_fence.is_none());
        match render_prime_frame(
            frame_surface,
            &mut self.renderer,
            state,
            dimensions,
            release_id,
            PrimeSyncContext {
                mode: &mut prime.sync_mode,
                deferred_sync_destroy: &mut prime.deferred_sync_destroy,
                egl: &self.state.egl,
                display: self.state.display,
            },
            &mut slot,
        ) {
            Ok(mut export) => {
                export.prime_timings.prepare = prepare;
                prime.in_flight.insert(release_id, slot);
                Ok(Some(export))
            }
            Err(error) => {
                prime.available.push(slot);
                self.terminal_error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub(super) fn release_prime(&mut self, release_id: u64) {
        let Some(prime) = self.prime.as_mut() else {
            return;
        };
        if let Some(mut frame) = prime.in_flight.remove(&release_id) {
            // The exported fence stays valid for the complete lease lifetime.
            close_slot_acquire_fence(&mut frame.acquire_fence);
            prime.available.push(frame);
        }
        if self.make_current().is_ok() {
            self.retry_deferred_prime_sync_destroy();
        }
    }

    pub(super) fn terminal_prime_shutdown_ready(&self) -> bool {
        prime_terminal_shutdown_ready(
            self.terminal_error.is_some(),
            self.prime.as_ref().map_or(0, |prime| prime.in_flight.len()),
        )
    }

    fn retry_deferred_prime_sync_destroy(&mut self) {
        let Some(prime) = self.prime.as_mut() else {
            return;
        };
        if prime.deferred_sync_destroy.is_empty() {
            return;
        }
        prime.deferred_sync_destroy.retry_with(|deferred| {
            unsafe { deferred.functions.destroy(deferred.handle) }.map_err(|error| {
                DeferredProducerSync {
                    functions: deferred.functions,
                    handle: error.handle,
                }
            })
        });
    }

    fn make_current(&self) -> Result<(), String> {
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
        Ok(())
    }
}

impl Drop for GlHeadlessRenderer {
    fn drop(&mut self) {
        let context_current = self.make_current().is_ok();
        if let Some(frame_surface) = self.state.frame_surface.take() {
            if context_current {
                drop(frame_surface);
            } else {
                frame_surface.abandon();
            }
        }

        if let Some(mut prime) = self.prime.take() {
            if context_current {
                prime.deferred_sync_destroy.retry_with(|deferred| {
                    unsafe { deferred.functions.destroy(deferred.handle) }.map_err(|error| {
                        DeferredProducerSync {
                            functions: deferred.functions,
                            handle: error.handle,
                        }
                    })
                });
            }
            self.state.final_sync_handles.extend(
                prime
                    .deferred_sync_destroy
                    .into_pending()
                    .into_iter()
                    .map(|deferred| deferred.handle),
            );
            let frames = prime
                .available
                .drain(..)
                .chain(prime.in_flight.drain().map(|(_release_id, frame)| frame));
            for frame in frames {
                if context_current {
                    destroy_prime_frame(&self.state.egl, self.state.display, frame);
                } else {
                    abandon_prime_frame(&self.state.egl, self.state.display, frame);
                }
            }
        }
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
            final_sync_handles: Vec::new(),
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

fn close_slot_acquire_fence(fence: &mut Option<OwnedFd>) {
    drop(fence.take());
}

fn prime_terminal_shutdown_ready(terminal: bool, in_flight_leases: usize) -> bool {
    terminal && in_flight_leases == 0
}

fn select_prime_sync_mode(state: &mut EglHeadlessState) -> Result<PrimeSyncMode, String> {
    if std::env::var_os("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC").is_some() {
        return gl::Finish::is_loaded()
            .then_some(PrimeSyncMode::ImplicitFallback)
            .ok_or_else(|| {
                "headless PRIME cannot prove GPU completion because glFinish is unavailable"
                    .to_string()
            });
    }

    let functions = unsafe {
        NativeFenceFunctions::load_with(|name| {
            let symbol = CString::new(name).expect("EGL symbol");
            let proc = state.egl.GetProcAddress(symbol.as_ptr()) as *const c_void;
            if !proc.is_null() {
                return proc;
            }
            state
                ._egl_lib
                .get::<*const c_void>(symbol.as_bytes_with_nul())
                .map(|loaded| *loaded)
                .unwrap_or(ptr::null())
        })
    };
    let egl_extensions = query_egl_string(&state.egl, state.display, egl::EXTENSIONS as EGLint);
    let functions = functions.select_producer(&egl_extensions);
    if !gl::Finish::is_loaded() {
        return Err(
            "headless PRIME cannot prove GPU completion because glFinish is unavailable"
                .to_string(),
        );
    }
    let Some(functions) = functions.filter(|_| gl::Flush::is_loaded()) else {
        return Ok(PrimeSyncMode::ImplicitFallback);
    };

    let mut mode = PrimeSyncMode::Explicit(functions);
    let mut deferred_sync_destroy = DeferredCleanupQueue::default();
    let synchronization = synchronize_prime_frame(
        &mut mode,
        &mut deferred_sync_destroy,
        &state.egl,
        state.display,
    );
    if let Err(error) = synchronization {
        state.final_sync_handles.extend(
            deferred_sync_destroy
                .into_pending()
                .into_iter()
                .map(|deferred| deferred.handle),
        );
        return Err(error);
    }
    let synchronization = synchronization.expect("checked successful synchronization");
    drop(synchronization.owned_fence);
    if !matches!(synchronization.acquire_sync, AcquireSync::SyncFile(_)) {
        debug_assert!(matches!(mode, PrimeSyncMode::ImplicitFallback));
    }
    Ok(mode)
}

fn query_egl_string(egl: &egl::Egl, display: EGLDisplay, name: EGLint) -> String {
    let value = unsafe { egl.QueryString(display, name) };
    if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    }
}

enum ExplicitSyncAttemptError<T> {
    Recoverable(String),
    Terminal { reason: String, deferred: T },
}

enum ExplicitSyncFailureAction<T> {
    Downgrade(String),
    Poison { reason: String, deferred: T },
}

fn explicit_sync_failure_action<T>(
    error: ExplicitSyncAttemptError<T>,
) -> ExplicitSyncFailureAction<T> {
    match error {
        ExplicitSyncAttemptError::Recoverable(reason) => {
            ExplicitSyncFailureAction::Downgrade(reason)
        }
        ExplicitSyncAttemptError::Terminal { reason, deferred } => {
            ExplicitSyncFailureAction::Poison { reason, deferred }
        }
    }
}

fn synchronize_prime_frame(
    mode: &mut PrimeSyncMode,
    deferred_sync_destroy: &mut DeferredCleanupQueue<DeferredProducerSync>,
    egl: &egl::Egl,
    display: EGLDisplay,
) -> Result<PrimeSynchronization, String> {
    let functions = match *mode {
        PrimeSyncMode::Explicit(functions) => functions,
        PrimeSyncMode::ImplicitFallback => {
            let duration = checked_gl_finish(egl)?;
            return Ok(PrimeSynchronization {
                acquire_sync: AcquireSync::Implicit,
                owned_fence: None,
                fence_export: None,
                gpu_finish_fallback: Some(duration),
            });
        }
    };

    let started_at = Instant::now();
    match try_export_native_fence(functions, display) {
        Ok(fence) => {
            let sync = AcquireSync::SyncFile(SyncFile {
                acquire_fence_fd: fence.as_raw_fd(),
            });
            Ok(PrimeSynchronization {
                acquire_sync: sync,
                owned_fence: Some(fence),
                fence_export: Some(started_at.elapsed()),
                gpu_finish_fallback: None,
            })
        }
        Err(error) => match explicit_sync_failure_action(error) {
            ExplicitSyncFailureAction::Downgrade(reason) => {
                eprintln!(
                    "headless PRIME explicit synchronization failed; permanently using glFinish fallback: {reason}"
                );
                *mode = PrimeSyncMode::ImplicitFallback;
                let duration = checked_gl_finish(egl)?;
                Ok(PrimeSynchronization {
                    acquire_sync: AcquireSync::Implicit,
                    owned_fence: None,
                    fence_export: None,
                    gpu_finish_fallback: Some(duration),
                })
            }
            ExplicitSyncFailureAction::Poison { reason, deferred } => {
                deferred_sync_destroy.push(deferred);
                Err(reason)
            }
        },
    }
}

fn try_export_native_fence(
    functions: NativeFenceFunctions,
    display: EGLDisplay,
) -> Result<OwnedFd, ExplicitSyncAttemptError<DeferredProducerSync>> {
    let _ = take_gl_errors();
    let handle = unsafe { functions.create_export_fence(display.cast_mut().cast()) }
        .map_err(|error| ExplicitSyncAttemptError::Recoverable(error.to_string()))?;
    unsafe { gl::Flush() };
    let gl_errors = take_gl_errors();
    if !gl_errors.is_empty() {
        unsafe { functions.destroy(handle) }.map_err(|error| {
            terminal_sync_destroy_error(functions, error, "after glFlush error")
        })?;
        return Err(ExplicitSyncAttemptError::Recoverable(format!(
            "glFlush reported GL errors {}",
            format_gl_errors(&gl_errors)
        )));
    }

    let duplicated = match unsafe { functions.duplicate_fence(&handle) } {
        Ok(fence) => fence,
        Err(error) => {
            unsafe { functions.destroy(handle) }.map_err(|destroy_error| {
                terminal_sync_destroy_error(
                    functions,
                    destroy_error,
                    "after fence duplication error",
                )
            })?;
            return Err(ExplicitSyncAttemptError::Recoverable(error.to_string()));
        }
    };
    if let Err(error) = unsafe { functions.destroy(handle) } {
        drop(duplicated);
        return Err(terminal_sync_destroy_error(functions, error, ""));
    }
    Ok(duplicated)
}

fn terminal_sync_destroy_error(
    functions: NativeFenceFunctions,
    error: video_interop::egl::DestroyError,
    context: &str,
) -> ExplicitSyncAttemptError<DeferredProducerSync> {
    let suffix = if context.is_empty() {
        String::new()
    } else {
        format!(" {context}")
    };
    ExplicitSyncAttemptError::Terminal {
        reason: format!("headless PRIME EGL sync destruction failed{suffix}: {error}"),
        deferred: DeferredProducerSync {
            functions,
            handle: error.handle,
        },
    }
}

fn checked_gl_finish(egl: &egl::Egl) -> Result<Duration, String> {
    let _ = unsafe { egl.GetError() };
    let _ = take_gl_errors();
    let started_at = Instant::now();
    unsafe { gl::Finish() };
    let duration = started_at.elapsed();
    let gl_errors = take_gl_errors();
    let egl_error = unsafe { egl.GetError() };
    if !gl_errors.is_empty() || egl_error != egl::SUCCESS as EGLint {
        return Err(format!(
            "headless PRIME GPU finish fallback could not prove completion: gl_errors={} egl_error=0x{egl_error:x}",
            format_gl_errors(&gl_errors)
        ));
    }
    Ok(duration)
}

fn render_prime_frame(
    frame_surface: &mut GlFrameSurface,
    renderer: &mut SceneRenderer,
    state: &RenderState,
    dimensions: (u32, u32),
    release_id: u64,
    sync_context: PrimeSyncContext<'_>,
    slot: &mut PrimeFrameSlot,
) -> Result<HeadlessPrimeExport, String> {
    let retarget_started_at = Instant::now();
    frame_surface.retarget(
        dimensions,
        FramebufferInfo {
            fboid: slot.surface.fbo,
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        },
        prime_export_surface_origin(),
    )?;
    let retarget = retarget_started_at.elapsed();

    let timings = {
        let mut frame = frame_surface.frame();
        renderer.render(&mut frame, state)
    };
    let synchronization = synchronize_prime_frame(
        sync_context.mode,
        sync_context.deferred_sync_destroy,
        sync_context.egl,
        sync_context.display,
    )?;
    slot.acquire_fence = synchronization.owned_fence;

    let export_metadata_started_at = Instant::now();
    let modifier = modifier_to_option(slot.surface.bo.modifier().into());
    let plane_count = slot.surface.bo.plane_count().max(1);
    let objects = slot
        .fds
        .iter()
        .zip(slot.object_sizes.iter().copied())
        .map(|(fd, size)| PrimeObjectMeta {
            fd: fd.as_raw_fd(),
            size,
            modifier,
        })
        .collect::<Vec<_>>();
    let planes = (0..plane_count)
        .map(|plane| PrimePlaneMeta {
            object_index: plane,
            pitch: slot.surface.bo.stride_for_plane(plane as i32),
            offset: u64::from(slot.surface.bo.offset(plane as i32)),
        })
        .collect::<Vec<_>>();

    Ok(HeadlessPrimeExport {
        release_id,
        width: dimensions.0,
        height: dimensions.1,
        format: slot.surface.bo.format() as u32,
        objects,
        planes,
        acquire_sync: synchronization.acquire_sync,
        timings,
        prime_timings: HeadlessPrimeTimings {
            prepare: Duration::ZERO,
            retarget,
            fence_export: synchronization.fence_export,
            gpu_finish_fallback: synchronization.gpu_finish_fallback,
            export_metadata: export_metadata_started_at.elapsed(),
        },
    })
}

fn create_prime_frame_slot(
    egl: &egl::Egl,
    display: EGLDisplay,
    gbm: &GbmDevice<File>,
    image_target_texture_2d_oes: GlEglImageTargetTexture2DOes,
    dimensions: (u32, u32),
) -> Result<PrimeFrameSlot, String> {
    let surface = create_gbm_export_frame_surface(
        egl,
        display,
        gbm,
        image_target_texture_2d_oes,
        dimensions,
    )?;
    create_prime_frame_slot_from_surface(egl, display, surface)
}

fn create_prime_frame_slot_from_surface(
    egl: &egl::Egl,
    display: EGLDisplay,
    surface: ExportSurface,
) -> Result<PrimeFrameSlot, String> {
    let plane_count = surface.bo.plane_count().max(1);
    let fds = match (0..plane_count)
        .map(|plane| {
            if plane_count == 1 {
                surface.bo.fd()
            } else {
                surface.bo.fd_for_plane(plane as i32)
            }
            .map_err(|err| format!("gbm_bo_get_fd failed for plane {plane}: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(fds) => fds,
        Err(err) => {
            destroy_export_surface(egl, display, surface);
            return Err(err);
        }
    };
    let object_sizes = match fds.iter().map(dma_buf_size).collect::<Result<Vec<_>, _>>() {
        Ok(sizes) => sizes,
        Err(err) => {
            destroy_export_surface(egl, display, surface);
            return Err(err);
        }
    };

    Ok(PrimeFrameSlot {
        surface,
        fds,
        object_sizes,
        acquire_fence: None,
    })
}

struct ExportSurface {
    texture: u32,
    fbo: u32,
    image: egl::types::EGLImageKHR,
    bo: BufferObject<()>,
}

fn create_gbm_export_frame_surface(
    egl: &egl::Egl,
    display: EGLDisplay,
    gbm: &GbmDevice<File>,
    image_target_texture_2d_oes: GlEglImageTargetTexture2DOes,
    dimensions: (u32, u32),
) -> Result<ExportSurface, String> {
    let bo = gbm
        .create_buffer_object::<()>(
            dimensions.0,
            dimensions.1,
            GbmFormat::Abgr8888,
            prime_export_buffer_flags(),
        )
        .map_err(|err| format!("gbm_bo_create failed for linear headless PRIME frame: {err}"))?;
    validate_linear_export_modifier(bo.modifier().into())?;
    let plane_count = bo.plane_count().max(1);
    if plane_count != 1 {
        return Err(format!(
            "headless PRIME output currently requires a single-plane ABGR8888 buffer; GBM allocated {plane_count} planes"
        ));
    }
    let import_fd = bo
        .fd()
        .map_err(|err| format!("gbm_bo_get_fd failed for headless PRIME import: {err}"))?;
    let image = create_dma_buf_image(egl, display, &bo, import_fd.as_raw_fd(), dimensions)?;
    drop(import_fd);

    let _ = take_gl_errors();
    let mut texture = 0_u32;
    let mut fbo = 0_u32;
    let status = unsafe {
        gl::GenTextures(1, &mut texture);
        gl::BindTexture(gl::TEXTURE_2D, texture);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        image_target_texture_2d_oes(gl::TEXTURE_2D, image as *const c_void);
        gl::GenFramebuffers(1, &mut fbo);
        gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
        gl::FramebufferTexture2D(
            gl::FRAMEBUFFER,
            gl::COLOR_ATTACHMENT0,
            gl::TEXTURE_2D,
            texture,
            0,
        );
        let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        gl::BindTexture(gl::TEXTURE_2D, 0);
        status
    };
    let gl_errors = take_gl_errors();
    if texture == 0 || fbo == 0 || status != gl::FRAMEBUFFER_COMPLETE || !gl_errors.is_empty() {
        let surface = ExportSurface {
            texture,
            fbo,
            image,
            bo,
        };
        destroy_export_surface(egl, display, surface);
        return Err(format!(
            "headless PRIME framebuffer setup failed: texture={texture} fbo={fbo} status=0x{status:x} gl_errors={}; egl_error={}",
            format_gl_errors(&gl_errors),
            egl_error(egl)
        ));
    }

    Ok(ExportSurface {
        texture,
        fbo,
        image,
        bo,
    })
}

fn create_dma_buf_image(
    egl: &egl::Egl,
    display: EGLDisplay,
    bo: &BufferObject<()>,
    fd: i32,
    dimensions: (u32, u32),
) -> Result<egl::types::EGLImageKHR, String> {
    let modifier = modifier_to_option(bo.modifier().into());
    let mut attrs = vec![
        EGL_WIDTH as EGLint,
        dimensions.0 as EGLint,
        EGL_HEIGHT as EGLint,
        dimensions.1 as EGLint,
        EGL_LINUX_DRM_FOURCC_EXT as EGLint,
        bo.format() as EGLint,
        EGL_DMA_BUF_PLANE0_FD_EXT as EGLint,
        fd,
        EGL_DMA_BUF_PLANE0_OFFSET_EXT as EGLint,
        bo.offset(0) as EGLint,
        EGL_DMA_BUF_PLANE0_PITCH_EXT as EGLint,
        bo.stride_for_plane(0) as EGLint,
    ];
    if let Some(modifier) = modifier {
        attrs.extend([
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT as EGLint,
            (modifier & 0xffff_ffff) as EGLint,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT as EGLint,
            (modifier >> 32) as EGLint,
        ]);
    }
    attrs.push(egl::NONE as EGLint);

    let image = if egl.CreateImageKHR.is_loaded() {
        unsafe {
            egl.CreateImageKHR(
                display,
                egl::NO_CONTEXT,
                EGL_LINUX_DMA_BUF_EXT,
                ptr::null(),
                attrs.as_ptr(),
            )
        }
    } else if egl.CreateImage.is_loaded() {
        let core_attrs = attrs
            .iter()
            .map(|attribute| *attribute as EGLAttrib)
            .collect::<Vec<_>>();
        unsafe {
            egl.CreateImage(
                display,
                egl::NO_CONTEXT,
                EGL_LINUX_DMA_BUF_EXT,
                ptr::null(),
                core_attrs.as_ptr(),
            ) as egl::types::EGLImageKHR
        }
    } else {
        return Err("neither eglCreateImageKHR nor eglCreateImage is available".to_string());
    };

    if image == egl::NO_IMAGE_KHR {
        return Err(format!(
            "eglCreateImage dma-buf import failed for headless PRIME frame: {}",
            egl_error(egl)
        ));
    }

    Ok(image)
}

fn destroy_export_surface(egl: &egl::Egl, display: EGLDisplay, surface: ExportSurface) {
    destroy_image(egl, display, surface.image);
    unsafe {
        if surface.fbo != 0 {
            gl::DeleteFramebuffers(1, &surface.fbo);
        }
        if surface.texture != 0 {
            gl::DeleteTextures(1, &surface.texture);
        }
    }
    drop(surface.bo);
}

fn destroy_image(egl: &egl::Egl, display: EGLDisplay, image: egl::types::EGLImageKHR) {
    unsafe {
        if egl.DestroyImageKHR.is_loaded() {
            let _ = egl.DestroyImageKHR(display, image);
        } else if egl.DestroyImage.is_loaded() {
            let _ = egl.DestroyImage(display, image as egl::types::EGLImage);
        }
    }
}

fn abandon_prime_frame(egl: &egl::Egl, display: EGLDisplay, frame: PrimeFrameSlot) {
    destroy_image(egl, display, frame.surface.image);
    drop(frame.surface.bo);
    drop(frame.fds);
}

fn destroy_prime_frame(egl: &egl::Egl, display: EGLDisplay, frame: PrimeFrameSlot) {
    destroy_export_surface(egl, display, frame.surface);
    drop(frame.fds);
}

fn dma_buf_size(fd: &OwnedFd) -> Result<u64, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(format!(
            "fstat failed for exported headless PRIME dma-buf: {}",
            std::io::Error::last_os_error()
        ));
    }

    let size = unsafe { stat.assume_init() }.st_size;
    u64::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| format!("exported headless PRIME dma-buf reported invalid size {size}"))
}

fn modifier_to_option(modifier: u64) -> Option<u64> {
    (modifier != DRM_FORMAT_MOD_INVALID).then_some(modifier)
}

fn prime_export_buffer_flags() -> BufferObjectFlags {
    // This output is consumed through EGL DMA-BUF import rather than direct scanout. Requiring a
    // linear BO avoids relying on implicit, driver-specific tiling metadata when GBM cannot report
    // an explicit modifier for a legacy gbm_bo_create allocation.
    BufferObjectFlags::RENDERING | BufferObjectFlags::LINEAR
}

fn prime_export_surface_origin() -> SurfaceOrigin {
    // DMA-BUF video consumers use top-left image coordinates. The ordinary GL window surfaces use
    // bottom-left framebuffer coordinates, but exporting that orientation flips the video frame.
    SurfaceOrigin::TopLeft
}

fn validate_linear_export_modifier(modifier: u64) -> Result<(), String> {
    if matches!(modifier, DRM_FORMAT_MOD_LINEAR | DRM_FORMAT_MOD_INVALID) {
        Ok(())
    } else {
        Err(format!(
            "GBM returned non-linear modifier {modifier:#018x} for a linear headless PRIME buffer"
        ))
    }
}

fn open_prime_gbm_device(
    egl: &egl::Egl,
    display: EGLDisplay,
    image_target_texture_2d_oes: GlEglImageTargetTexture2DOes,
    dimensions: (u32, u32),
) -> Result<(GbmDevice<File>, ExportSurface), String> {
    let mut errors = Vec::new();
    for path in gbm_device_candidates(egl, display) {
        let file = match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                errors.push(format!("{path}: open failed: {err}"));
                continue;
            }
        };
        let device = match GbmDevice::new(file) {
            Ok(device) => device,
            Err(err) => {
                errors.push(format!("{path}: gbm_create_device failed: {err}"));
                continue;
            }
        };

        match create_gbm_export_frame_surface(
            egl,
            display,
            &device,
            image_target_texture_2d_oes,
            dimensions,
        ) {
            Ok(probe) => return Ok((device, probe)),
            Err(err) => errors.push(format!("{path}: PRIME export probe failed: {err}")),
        }
    }

    Err(format!(
        "could not find a GBM device compatible with the EGL display for headless PRIME output ({})",
        errors.join("; ")
    ))
}

fn gbm_device_candidates(egl: &egl::Egl, display: EGLDisplay) -> Vec<String> {
    let mut paths = egl_display_device_paths(egl, display);
    for path in discovered_dri_device_paths().into_iter().chain(
        (128..=143)
            .map(|node| format!("/dev/dri/renderD{node}"))
            .chain((0..=7).map(|node| format!("/dev/dri/card{node}"))),
    ) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn discovered_dri_device_paths() -> Vec<String> {
    let mut paths = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            let suffix = name
                .strip_prefix("renderD")
                .or_else(|| name.strip_prefix("card"))?;
            suffix
                .chars()
                .all(|character| character.is_ascii_digit())
                .then(|| entry.path().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn egl_display_device_paths(egl: &egl::Egl, display: EGLDisplay) -> Vec<String> {
    if !egl.QueryDisplayAttribEXT.is_loaded() || !egl.QueryDeviceStringEXT.is_loaded() {
        return Vec::new();
    }

    let mut device_attrib: EGLAttrib = 0;
    if unsafe {
        egl.QueryDisplayAttribEXT(
            display,
            EGL_DEVICE_EXT as EGLint,
            &mut device_attrib as *mut EGLAttrib,
        )
    } == egl::FALSE
    {
        return Vec::new();
    }

    let device = device_attrib as EGLDeviceEXT;
    [EGL_DRM_RENDER_NODE_FILE_EXT, EGL_DRM_DEVICE_FILE_EXT]
        .into_iter()
        .filter_map(|name| unsafe {
            let path = egl.QueryDeviceStringEXT(device, name);
            (!path.is_null()).then(|| CStr::from_ptr(path).to_string_lossy().into_owned())
        })
        .collect()
}

fn load_egl_proc<T>(egl: &egl::Egl, name: &str) -> Result<T, String>
where
    T: Copy,
{
    let symbol = CString::new(name).expect("EGL symbol");
    let ptr = unsafe { egl.GetProcAddress(symbol.as_ptr()) } as *const c_void;
    if ptr.is_null() {
        return Err(format!("{name} is not available"));
    }
    Ok(unsafe { std::mem::transmute_copy::<*const c_void, T>(&ptr) })
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

fn take_gl_errors() -> Vec<u32> {
    std::iter::from_fn(|| {
        let error = unsafe { gl::GetError() };
        (error != gl::NO_ERROR).then_some(error)
    })
    .take(16)
    .collect()
}

fn format_gl_errors(errors: &[u32]) -> String {
    if errors.is_empty() {
        "none".to_string()
    } else {
        errors
            .iter()
            .map(|error| format!("0x{error:x}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsRawFd, FromRawFd};

    use super::*;

    #[test]
    fn slot_acquire_fence_closes_exactly_once_before_reuse() {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        let raw = fds[0];
        let mut fence = Some(unsafe { OwnedFd::from_raw_fd(raw) });
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };

        close_slot_acquire_fence(&mut fence);
        assert!(fence.is_none());
        assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
        close_slot_acquire_fence(&mut fence);
        assert!(fence.is_none());
        assert!(write.as_raw_fd() >= 0);
    }

    #[test]
    fn explicit_sync_failure_policy_downgrades_only_recoverable_errors() {
        assert!(matches!(
            explicit_sync_failure_action(ExplicitSyncAttemptError::<u8>::Recoverable(
                "duplicate failed".to_string()
            )),
            ExplicitSyncFailureAction::Downgrade(reason) if reason == "duplicate failed"
        ));
        assert!(matches!(
            explicit_sync_failure_action(ExplicitSyncAttemptError::Terminal {
                reason: "destroy failed".to_string(),
                deferred: 7_u8,
            }),
            ExplicitSyncFailureAction::Poison { reason, deferred }
                if reason == "destroy failed" && deferred == 7
        ));
    }

    #[test]
    fn deferred_cleanup_retries_without_losing_failed_values() {
        let mut cleanup = DeferredCleanupQueue::default();
        cleanup.push(1_u8);
        cleanup.push(2_u8);

        cleanup.retry_with(|value| (value == 1).then_some(()).ok_or(value));
        assert_eq!(cleanup.pending, [2]);
        cleanup.retry_with(|_value| Ok(()));
        assert!(cleanup.is_empty());
    }

    #[test]
    fn terminal_shutdown_waits_for_every_in_flight_lease() {
        assert!(!prime_terminal_shutdown_ready(false, 0));
        assert!(!prime_terminal_shutdown_ready(true, 2));
        assert!(!prime_terminal_shutdown_ready(true, 1));
        assert!(prime_terminal_shutdown_ready(true, 0));
    }

    #[test]
    fn prime_export_uses_interoperable_linear_render_buffers() {
        let flags = prime_export_buffer_flags();

        assert!(flags.contains(BufferObjectFlags::RENDERING));
        assert!(flags.contains(BufferObjectFlags::LINEAR));
        assert!(!flags.contains(BufferObjectFlags::SCANOUT));
    }

    #[test]
    fn prime_export_rejects_non_linear_buffer_modifiers() {
        assert!(validate_linear_export_modifier(DRM_FORMAT_MOD_LINEAR).is_ok());
        assert!(validate_linear_export_modifier(DRM_FORMAT_MOD_INVALID).is_ok());
        assert!(validate_linear_export_modifier(1).is_err());
    }

    #[test]
    fn prime_export_uses_top_left_video_orientation() {
        assert_eq!(prime_export_surface_origin(), SurfaceOrigin::TopLeft);
    }
}
