use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::raw::c_void;
use std::ptr;
use std::sync::mpsc::Sender as StartupSender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

mod cursor_theme;

use drm::Device as BasicDevice;
use drm::control::{
    self, AtomicCommitFlags, Device as ControlDevice, FbCmd2Flags, PlaneType, ResourceHandles,
    atomic, connector, crtc, encoder, framebuffer, plane, property,
};
use drm::{ClientCapability, DriverCapability};
use gbm::{
    AsRaw, BufferObject, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat,
    Modifier as GbmModifier, Surface,
};
use glutin_egl_sys::egl;
use glutin_egl_sys::egl::types::{EGLConfig, EGLContext, EGLDisplay, EGLSurface, EGLenum, EGLint};
use libloading::Library;
use skia_safe::{Paint, Rect, gpu::gl::FramebufferInfo};

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::RasterPresentKind;
use crate::actors::{EventMsg, RenderMsg, TreeMsg};
use crate::assets::AssetConfig;
use crate::backend::raster::{RasterBackend, RasterConfig};
use crate::backend::skia_gpu::GlFrameSurface;
use crate::backend::wake::BackendWake;
use crate::cursor::{CursorState, SharedCursorState};
use crate::events::CursorIcon;
use crate::input::InputEvent;
use crate::linux_wait::{EventFd, poll_fds};
use crate::native_log::NativeLogRelay;
use crate::renderer::{RenderState, RenderTimings, RendererCacheConfig, SceneRenderer};
use crate::stats::{
    RendererStatsCollector, format_slow_render_frame_log, render_frame_has_slow_stage,
};
use crate::video::{VideoImportContext, VideoRegistry};
use crate::{DrmCursorOverrideConfig, LatestFrameStore, RenderingApi};

use self::cursor_theme::{CURSOR_PLANE_SIZE, CursorVisual, DrmCursorTheme};

const EGL_PLATFORM_GBM_KHR: EGLenum = 0x31D7;
const EGL_OPENGL_ES3_BIT_KHR: EGLint = 0x0040;
const RENDER_PROFILE_INTERVAL: Duration = Duration::from_secs(1);
const GL_QUERY_COUNTER_BITS_EXT: gl::types::GLenum = 0x8864;
const GL_QUERY_RESULT_EXT: gl::types::GLenum = 0x8866;
const GL_QUERY_RESULT_AVAILABLE_EXT: gl::types::GLenum = 0x8867;
const GL_TIME_ELAPSED_EXT: gl::types::GLenum = 0x88BF;
const GL_GPU_DISJOINT_EXT: gl::types::GLenum = 0x8FBB;

#[derive(Clone)]
pub(crate) struct DrmBackendWake {
    presenter_wake: EventFd,
    input_wake: EventFd,
}

impl DrmBackendWake {
    pub(crate) fn new(presenter_wake: EventFd, input_wake: EventFd) -> Self {
        Self {
            presenter_wake,
            input_wake,
        }
    }
}

impl BackendWake for DrmBackendWake {
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

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> i32 {
        self.0.as_raw_fd()
    }
}

impl BasicDevice for Card {}
impl ControlDevice for Card {}

struct EglState {
    egl: egl::Egl,
    _egl_lib: Library,
    display: EGLDisplay,
    _context: EGLContext,
    surface: EGLSurface,
}

type GlGenQueriesExt = unsafe extern "system" fn(gl::types::GLsizei, *mut gl::types::GLuint);
type GlDeleteQueriesExt = unsafe extern "system" fn(gl::types::GLsizei, *const gl::types::GLuint);
type GlBeginQueryExt = unsafe extern "system" fn(gl::types::GLenum, gl::types::GLuint);
type GlEndQueryExt = unsafe extern "system" fn(gl::types::GLenum);
type GlGetQueryivExt =
    unsafe extern "system" fn(gl::types::GLenum, gl::types::GLenum, *mut gl::types::GLint);
type GlGetQueryObjectuivExt =
    unsafe extern "system" fn(gl::types::GLuint, gl::types::GLenum, *mut gl::types::GLuint);
type GlGetQueryObjectui64vExt =
    unsafe extern "system" fn(gl::types::GLuint, gl::types::GLenum, *mut gl::types::GLuint64);

#[derive(Clone, Copy)]
struct GpuQueueTimerApi {
    gen_queries: GlGenQueriesExt,
    delete_queries: GlDeleteQueriesExt,
    begin_query: GlBeginQueryExt,
    end_query: GlEndQueryExt,
    get_query_iv: GlGetQueryivExt,
    get_query_object_uiv: GlGetQueryObjectuivExt,
    get_query_object_ui64v: GlGetQueryObjectui64vExt,
}

struct PendingGpuQueueTimerSample {
    query: gl::types::GLuint,
    render_version: u64,
    ended_at: Instant,
    cpu_render: Duration,
    cpu_draw: Duration,
    cpu_flush: Duration,
    cpu_gpu_flush: Duration,
    cpu_submit: Duration,
    cache_hits: u64,
    cached_image_draws: u64,
}

struct GpuQueueTimer {
    api: Option<GpuQueueTimerApi>,
    active_query: Option<gl::types::GLuint>,
    pending: Option<PendingGpuQueueTimerSample>,
    log_results: bool,
    logged_disjoint: bool,
}

impl GpuQueueTimerApi {
    fn load(egl: &egl::Egl) -> Result<Self, String> {
        if !gl_has_extension("GL_EXT_disjoint_timer_query") {
            return Err("GL_EXT_disjoint_timer_query is not advertised".to_string());
        }

        macro_rules! load_ext {
            ($symbol:literal, $ty:ty) => {{
                let symbol = CString::new($symbol).expect("static GL extension symbol");
                let pointer = unsafe { egl.GetProcAddress(symbol.as_ptr()) } as *const c_void;
                if pointer.is_null() {
                    return Err(format!("{} is unavailable", $symbol));
                }
                // SAFETY: EGL returns the function pointer for the exact extension symbol and
                // the signature comes directly from GL_EXT_disjoint_timer_query.
                unsafe { std::mem::transmute::<*const c_void, $ty>(pointer) }
            }};
        }

        let api = Self {
            gen_queries: load_ext!("glGenQueriesEXT", GlGenQueriesExt),
            delete_queries: load_ext!("glDeleteQueriesEXT", GlDeleteQueriesExt),
            begin_query: load_ext!("glBeginQueryEXT", GlBeginQueryExt),
            end_query: load_ext!("glEndQueryEXT", GlEndQueryExt),
            get_query_iv: load_ext!("glGetQueryivEXT", GlGetQueryivExt),
            get_query_object_uiv: load_ext!("glGetQueryObjectuivEXT", GlGetQueryObjectuivExt),
            get_query_object_ui64v: load_ext!("glGetQueryObjectui64vEXT", GlGetQueryObjectui64vExt),
        };

        let mut counter_bits = 0;
        unsafe {
            (api.get_query_iv)(
                GL_TIME_ELAPSED_EXT,
                GL_QUERY_COUNTER_BITS_EXT,
                &mut counter_bits,
            );
        }
        if counter_bits <= 0 {
            return Err("GL_TIME_ELAPSED_EXT exposes zero counter bits".to_string());
        }

        Ok(api)
    }
}

impl GpuQueueTimer {
    fn new(egl: &egl::Egl, enabled: bool, log_results: bool, native_log: &NativeLogRelay) -> Self {
        if !enabled {
            return Self {
                api: None,
                active_query: None,
                pending: None,
                log_results,
                logged_disjoint: false,
            };
        }

        match GpuQueueTimerApi::load(egl) {
            Ok(api) => {
                native_log.info(
                    "drm",
                    "sampled asynchronous GPU queue completion-span profiling enabled via GL_EXT_disjoint_timer_query (at most one frame per second)",
                );
                Self {
                    api: Some(api),
                    active_query: None,
                    pending: None,
                    log_results,
                    logged_disjoint: false,
                }
            }
            Err(err) => {
                native_log.warning(
                    "drm",
                    format!("GPU queue completion-span profiling unavailable: {err}"),
                );
                Self {
                    api: None,
                    active_query: None,
                    pending: None,
                    log_results,
                    logged_disjoint: false,
                }
            }
        }
    }

    fn poll(&mut self, stats: Option<&RendererStatsCollector>, native_log: &NativeLogRelay) {
        let (Some(api), Some(pending)) = (self.api, self.pending.as_ref()) else {
            return;
        };

        let mut available = 0;
        unsafe {
            (api.get_query_object_uiv)(
                pending.query,
                GL_QUERY_RESULT_AVAILABLE_EXT,
                &mut available,
            );
        }
        if available == 0 {
            return;
        }

        let mut disjoint = 0;
        unsafe {
            gl::GetIntegerv(GL_GPU_DISJOINT_EXT, &mut disjoint);
        }

        let pending = self
            .pending
            .take()
            .expect("pending GPU query was checked above");
        if disjoint != 0 {
            unsafe {
                (api.delete_queries)(1, &pending.query);
            }
            if !self.logged_disjoint {
                native_log.warning(
                    "drm",
                    "discarded a GPU queue completion-span sample because GL_GPU_DISJOINT_EXT was set",
                );
                self.logged_disjoint = true;
            }
            return;
        }

        let mut elapsed_ns = 0;
        unsafe {
            (api.get_query_object_ui64v)(pending.query, GL_QUERY_RESULT_EXT, &mut elapsed_ns);
            (api.delete_queries)(1, &pending.query);
        }
        let elapsed = Duration::from_nanos(elapsed_ns);
        if let Some(stats) = stats {
            stats.record_drm_gpu_queue_completion(elapsed);
        }
        if self.log_results {
            let query_result_age = pending.ended_at.elapsed();
            native_log.info(
                "renderer_gpu_queue",
                format!(
                    "GPU queue completion-span sample\n  render_version: {}\n  queue_completion_span: {:.3} ms\n  query_result_age: {:.3} ms\n  CPU: render={:.3} ms draw_recording={:.3} ms flush={:.3} ms gpu_flush={:.3} ms submit={:.3} ms\n  cache: hits={} image_draws={}",
                    pending.render_version,
                    elapsed.as_secs_f64() * 1_000.0,
                    query_result_age.as_secs_f64() * 1_000.0,
                    pending.cpu_render.as_secs_f64() * 1_000.0,
                    pending.cpu_draw.as_secs_f64() * 1_000.0,
                    pending.cpu_flush.as_secs_f64() * 1_000.0,
                    pending.cpu_gpu_flush.as_secs_f64() * 1_000.0,
                    pending.cpu_submit.as_secs_f64() * 1_000.0,
                    pending.cache_hits,
                    pending.cached_image_draws,
                ),
            );
        }
    }

    fn begin_sample(&mut self, native_log: &NativeLogRelay) {
        let Some(api) = self.api else {
            return;
        };
        if self.active_query.is_some() || self.pending.is_some() {
            return;
        }

        // GL_EXT_disjoint_timer_query specifies that querying this flag clears stale state.
        let mut ignored_disjoint = 0;
        unsafe {
            gl::GetIntegerv(GL_GPU_DISJOINT_EXT, &mut ignored_disjoint);
        }

        let mut query = 0;
        unsafe {
            (api.gen_queries)(1, &mut query);
        }
        if query == 0 {
            self.api = None;
            native_log.warning(
                "drm",
                "GPU queue completion-span profiling disabled after glGenQueriesEXT returned zero",
            );
            return;
        }

        unsafe {
            (api.begin_query)(GL_TIME_ELAPSED_EXT, query);
        }
        self.active_query = Some(query);
    }

    fn end_sample(&mut self, render_version: u64, timings: &RenderTimings) {
        let (Some(api), Some(query)) = (self.api, self.active_query.take()) else {
            return;
        };

        unsafe {
            (api.end_query)(GL_TIME_ELAPSED_EXT);
        }
        let paint_layer = timings
            .renderer_cache
            .as_deref()
            .map(|cache| cache.paint_layer)
            .unwrap_or_default();
        self.pending = Some(PendingGpuQueueTimerSample {
            query,
            render_version,
            ended_at: Instant::now(),
            cpu_render: timings.total,
            cpu_draw: timings.draw,
            cpu_flush: timings.flush,
            cpu_gpu_flush: timings.gpu_flush,
            cpu_submit: timings.submit,
            cache_hits: paint_layer.hits,
            cached_image_draws: paint_layer.cached_image_draws,
        });
    }
}

impl Drop for GpuQueueTimer {
    fn drop(&mut self) {
        let Some(api) = self.api else {
            return;
        };
        if let Some(query) = self.active_query.take() {
            unsafe {
                (api.end_query)(GL_TIME_ELAPSED_EXT);
                (api.delete_queries)(1, &query);
            }
        }
        if let Some(pending) = self.pending.take() {
            unsafe {
                (api.delete_queries)(1, &pending.query);
            }
        }
    }
}

fn gl_has_extension(expected: &str) -> bool {
    let mut count = 0;
    unsafe {
        gl::GetIntegerv(gl::NUM_EXTENSIONS, &mut count);
    }
    (0..count).any(|index| {
        let value = unsafe { gl::GetStringi(gl::EXTENSIONS, index as gl::types::GLuint) };
        if value.is_null() {
            return false;
        }
        unsafe { CStr::from_ptr(value.cast()) }.to_bytes() == expected.as_bytes()
    })
}

struct CursorPlane {
    commit: CursorPlaneCommit,
    bo: BufferObject<()>,
}

struct PreparedPrimaryFrame {
    generation: u64,
    render_version: u64,
    pipeline_submitted_at: Option<Instant>,
    pipeline_swap_done_at: Option<Instant>,
    bo: BufferObject<()>,
    fb: framebuffer::Handle,
    video_sync_succeeded: bool,
    video_needs_cleanup: bool,
    imported_video_frames: usize,
    newest_video_submitted_at: Option<Instant>,
    prepared_at: Instant,
    atomic_commit_submitted_at: Option<Instant>,
    atomic_commit_monotonic_at: Option<Duration>,
    present_submit_duration: Duration,
}

struct CurrentPrimaryFrame {
    generation: u64,
    render_version: u64,
    bo: BufferObject<()>,
    fb: framebuffer::Handle,
}

struct SubmittedCursorState {
    version: Option<u64>,
    visible: bool,
    icon: CursorIcon,
}

struct InFlightCommit {
    primary: Option<PreparedPrimaryFrame>,
    cursor: Option<SubmittedCursorState>,
    emit_animation_pulse: bool,
}

const FOLLOW_UP_PRIMARY_WINDOW: Duration = Duration::from_millis(4);

struct DrmPresentState {
    mode_frame_interval: Duration,
}

impl DrmPresentState {
    fn new(mode_frame_interval: Duration) -> Self {
        Self {
            mode_frame_interval,
        }
    }

    fn observe_present(&mut self, presented_at: Instant) -> Instant {
        presented_at + self.mode_frame_interval
    }

    fn mode_frame_interval(&self) -> Duration {
        self.mode_frame_interval
    }
}

#[derive(Clone)]
struct CursorPlaneCommit {
    handle: plane::Handle,
    props: Arc<HashMap<String, property::Info>>,
    fb: framebuffer::Handle,
    size: (u32, u32),
}

impl CursorPlane {
    fn commit(&self) -> &CursorPlaneCommit {
        &self.commit
    }

    fn write_visual(&mut self, visual: &CursorVisual) -> Result<(), String> {
        self.bo
            .write(visual.plane_bgra())
            .map_err(|err| format!("failed to write cursor bo: {err}"))
    }
}

fn open_card(card_path: Option<&str>) -> Result<Card, String> {
    let card_path = card_path.unwrap_or("/dev/dri/card0");

    let fd = OpenOptions::new()
        .read(true)
        .write(true)
        .open(card_path)
        .map_err(|e| format!("failed to open {card_path}: {e}"))?;

    Ok(Card(fd))
}

fn sleep_with_stop(stop: &Arc<AtomicBool>, duration: Duration) {
    let deadline = Instant::now() + duration;

    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }

        std::thread::sleep((deadline - now).min(Duration::from_millis(25)));
    }
}

fn release_master_lock(card: &Card) {
    if let Err(err) = card.release_master_lock() {
        eprintln!("DRM master release failed: {err}");
    }
}

fn handle_startup_failure_with_card(
    card: &Card,
    startup_tx: &mut Option<StartupSender<Result<(), String>>>,
    running_flag: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    retries_remaining: &mut u32,
    retry_interval: Duration,
    message: String,
) -> bool {
    release_master_lock(card);

    handle_startup_failure(
        startup_tx,
        running_flag,
        stop,
        retries_remaining,
        retry_interval,
        message,
    )
}

fn handle_startup_failure(
    startup_tx: &mut Option<StartupSender<Result<(), String>>>,
    running_flag: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    retries_remaining: &mut u32,
    retry_interval: Duration,
    message: String,
) -> bool {
    if startup_tx.is_none() {
        eprintln!("DRM backend unavailable: {message}");
        sleep_with_stop(stop, retry_interval);

        if stop.load(Ordering::Relaxed) {
            running_flag.store(false, Ordering::Relaxed);
            return true;
        }

        return false;
    }

    if *retries_remaining == 0 {
        let final_message = format!("DRM backend unavailable: {message}");
        eprintln!("{final_message}");

        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Err(final_message));
        }

        running_flag.store(false, Ordering::Relaxed);
        return true;
    }

    *retries_remaining -= 1;
    eprintln!(
        "DRM backend unavailable: {message} (retrying, {} attempts left)",
        *retries_remaining
    );
    sleep_with_stop(stop, retry_interval);

    if stop.load(Ordering::Relaxed) {
        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Err("DRM startup aborted".to_string()));
        }

        running_flag.store(false, Ordering::Relaxed);
        return true;
    }

    false
}

fn mode_blob_id(mode_blob: &property::Value<'static>) -> Option<u64> {
    match mode_blob {
        property::Value::Blob(blob) if *blob != 0 => Some(*blob),
        _ => None,
    }
}

fn destroy_mode_blob(card: &Card, blob_id: Option<u64>) {
    if let Some(blob_id) = blob_id {
        let _ = card.destroy_property_blob(blob_id);
    }
}

fn destroy_framebuffers(card: &Card, framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>) {
    for (_, framebuffer) in framebuffer_cache.drain() {
        let _ = card.destroy_framebuffer(framebuffer);
    }
}

fn destroy_session_resources(
    card: &Card,
    cursor_plane: Option<CursorPlane>,
    framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>,
    mode_blob_id: Option<u64>,
) {
    if let Some(cursor_plane) = cursor_plane {
        let _ = card.destroy_framebuffer(cursor_plane.commit.fb);
    }

    destroy_framebuffers(card, framebuffer_cache);
    destroy_mode_blob(card, mode_blob_id);
}

#[allow(clippy::too_many_arguments)]
fn teardown_drm_output(
    card: &Card,
    connector: connector::Handle,
    crtc_handle: crtc::Handle,
    plane: plane::Handle,
    con_props: &HashMap<String, property::Info>,
    crtc_props: &HashMap<String, property::Info>,
    plane_props: &HashMap<String, property::Info>,
    cursor_plane: Option<&CursorPlaneCommit>,
) -> Result<(), String> {
    let mut req = atomic::AtomicModeReq::new();

    if let Some(cursor_plane) = cursor_plane {
        if let Ok(fb_handle) = prop_handle(&cursor_plane.props, "FB_ID") {
            req.add_property(
                cursor_plane.handle,
                fb_handle,
                property::Value::Framebuffer(None),
            );
        }

        if let Ok(crtc_prop) = prop_handle(&cursor_plane.props, "CRTC_ID") {
            req.add_property(cursor_plane.handle, crtc_prop, property::Value::CRTC(None));
        }
    }

    req.add_property(
        plane,
        prop_handle(plane_props, "FB_ID")?,
        property::Value::Framebuffer(None),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    req.add_property(
        connector,
        prop_handle(con_props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    req.add_property(
        crtc_handle,
        prop_handle(crtc_props, "ACTIVE")?,
        property::Value::Boolean(false),
    );

    if let Ok(mode_handle) = prop_handle(crtc_props, "MODE_ID") {
        req.add_property(crtc_handle, mode_handle, property::Value::Blob(0));
    }

    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
        .map_err(|e| format!("tearing down DRM output failed: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn cleanup_active_session(
    card: &Card,
    connector: connector::Handle,
    crtc_handle: crtc::Handle,
    plane: plane::Handle,
    con_props: &HashMap<String, property::Info>,
    crtc_props: &HashMap<String, property::Info>,
    plane_props: &HashMap<String, property::Info>,
    cursor_plane: Option<CursorPlane>,
    framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>,
    mode_blob_id: Option<u64>,
) {
    if let Err(err) = teardown_drm_output(
        card,
        connector,
        crtc_handle,
        plane,
        con_props,
        crtc_props,
        plane_props,
        cursor_plane.as_ref().map(CursorPlane::commit),
    ) {
        eprintln!("DRM teardown failed: {err}");
    }

    destroy_session_resources(card, cursor_plane, framebuffer_cache, mode_blob_id);
    release_master_lock(card);
}

fn mode_distance(mode: &control::Mode, requested: (u32, u32)) -> i64 {
    let (width, height) = mode.size();
    let dx = width as i64 - requested.0 as i64;
    let dy = height as i64 - requested.1 as i64;
    dx * dx + dy * dy
}

fn mode_area(mode: &control::Mode) -> i64 {
    let (width, height) = mode.size();
    width as i64 * height as i64
}

fn mode_is_preferred(mode: &control::Mode) -> bool {
    mode.mode_type().contains(control::ModeTypeFlags::PREFERRED)
}

fn preferred_size(modes: &[control::Mode]) -> Option<(u32, u32)> {
    modes
        .iter()
        .find(|mode| mode_is_preferred(mode))
        .map(|mode| {
            let (width, height) = mode.size();
            (width as u32, height as u32)
        })
}

fn choose_mode(
    modes: &[control::Mode],
    requested: Option<(u32, u32)>,
) -> Result<control::Mode, String> {
    let first = modes
        .first()
        .cloned()
        .ok_or_else(|| "connector has no modes".to_string())?;

    let target_size = requested.or_else(|| preferred_size(modes));
    let mut best = first;
    let mut best_score = score_mode(&best, target_size);

    for mode in modes.iter().skip(1) {
        let score = score_mode(mode, target_size);
        if score < best_score {
            best = *mode;
            best_score = score;
        }
    }

    Ok(best)
}

fn score_mode(mode: &control::Mode, target_size: Option<(u32, u32)>) -> (i64, i32, i32, i64) {
    let distance = target_size
        .map(|size| mode_distance(mode, size))
        .unwrap_or(0);
    let refresh = -(mode.vrefresh() as i32);
    let preferred = if mode_is_preferred(mode) { 0 } else { 1 };
    let area = -mode_area(mode);
    (distance, refresh, preferred, area)
}

fn mode_refresh_hz(mode: &control::Mode) -> f64 {
    precise_mode_refresh_hz(
        mode.clock(),
        mode.hsync().2,
        mode.vsync().2,
        mode.vscan(),
        mode.flags(),
    )
    .unwrap_or_else(|| mode.vrefresh().max(1) as f64)
}

fn mode_frame_interval(mode: &control::Mode) -> Duration {
    frame_interval_for_refresh_hz(mode_refresh_hz(mode))
}

fn frame_interval_for_refresh_hz(refresh_hz: f64) -> Duration {
    Duration::from_secs_f64(1.0 / refresh_hz.max(1.0))
}

fn precise_mode_refresh_hz(
    clock_khz: u32,
    htotal: u16,
    vtotal: u16,
    vscan: u16,
    flags: control::ModeFlags,
) -> Option<f64> {
    if clock_khz == 0 || htotal == 0 || vtotal == 0 {
        return None;
    }

    let mut refresh_hz = clock_khz as f64 * 1_000.0 / htotal as f64 / vtotal as f64;
    if flags.contains(control::ModeFlags::INTERLACE) {
        refresh_hz *= 2.0;
    }
    if flags.contains(control::ModeFlags::DBLSCAN) {
        refresh_hz /= 2.0;
    }
    refresh_hz /= vscan.max(1) as f64;

    refresh_hz
        .is_finite()
        .then_some(refresh_hz)
        .filter(|hz| *hz > 0.0)
}

fn first_connected_connector(
    card: &Card,
    resources: &ResourceHandles,
    requested: Option<(u32, u32)>,
) -> Result<
    (
        connector::Handle,
        control::Mode,
        crtc::Handle,
        encoder::Handle,
    ),
    String,
> {
    let mut last_error = None;

    for handle in resources.connectors() {
        let info = card
            .get_connector(*handle, false)
            .map_err(|e| format!("failed to read connector {handle:?}: {e}"))?;

        if info.state() != connector::State::Connected {
            continue;
        }

        let mode = match choose_mode(info.modes(), requested) {
            Ok(mode) => mode,
            Err(err) => {
                last_error = Some(format!("connector {handle:?} {err}"));
                continue;
            }
        };

        match pick_encoder_and_crtc(card, resources, &info) {
            Ok((encoder, crtc)) => return Ok((*handle, mode, crtc, encoder)),
            Err(err) => last_error = Some(err),
        }
    }

    if let Some(err) = last_error {
        Err(err)
    } else {
        Err("no connected DRM connectors found".into())
    }
}

fn pick_encoder_and_crtc(
    card: &Card,
    resources: &ResourceHandles,
    connector_info: &connector::Info,
) -> Result<(encoder::Handle, crtc::Handle), String> {
    let mut encoder_handles = Vec::new();

    if let Some(current_encoder) = connector_info.current_encoder() {
        encoder_handles.push(current_encoder);
    }

    for encoder_handle in connector_info.encoders() {
        if !encoder_handles.contains(encoder_handle) {
            encoder_handles.push(*encoder_handle);
        }
    }

    for encoder_handle in encoder_handles {
        let encoder_info = card
            .get_encoder(encoder_handle)
            .map_err(|e| format!("failed to read encoder {encoder_handle:?}: {e}"))?;

        if let Some(crtc_handle) = encoder_info.crtc() {
            return Ok((encoder_handle, crtc_handle));
        }

        if let Some(crtc_handle) = resources
            .filter_crtcs(encoder_info.possible_crtcs())
            .first()
            .copied()
        {
            return Ok((encoder_handle, crtc_handle));
        }
    }

    Err(format!(
        "connector {:?} has no usable encoder/CRTC pair",
        connector_info.handle()
    ))
}

fn is_primary_plane(card: &Card, plane: plane::Handle) -> Result<bool, String> {
    let props = card
        .get_properties(plane)
        .map_err(|e| format!("failed to get plane properties: {e}"))?;
    for (&id, &val) in props.iter() {
        let info = card
            .get_property(id)
            .map_err(|e| format!("failed to read property info: {e}"))?;
        if info
            .name()
            .to_str()
            .map(|name| name == "type")
            .unwrap_or(false)
        {
            return Ok(val == u64::from(PlaneType::Primary as u32));
        }
    }
    Ok(false)
}

fn is_cursor_plane(card: &Card, plane: plane::Handle) -> Result<bool, String> {
    let props = card
        .get_properties(plane)
        .map_err(|e| format!("failed to get plane properties: {e}"))?;
    for (&id, &val) in props.iter() {
        let info = card
            .get_property(id)
            .map_err(|e| format!("failed to read property info: {e}"))?;
        if info
            .name()
            .to_str()
            .map(|name| name == "type")
            .unwrap_or(false)
        {
            return Ok(val == u64::from(PlaneType::Cursor as u32));
        }
    }
    Ok(false)
}

fn find_primary_plane(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<plane::Handle, String> {
    let planes = card
        .plane_handles()
        .map_err(|e| format!("could not list planes: {e}"))?;
    let mut compatible = Vec::new();
    let mut primary = Vec::new();

    for plane in planes {
        let info = card
            .get_plane(plane)
            .map_err(|e| format!("failed to read plane info: {e}"))?;
        let compatible_crtcs = resources.filter_crtcs(info.possible_crtcs());
        if !compatible_crtcs.contains(&crtc_handle) {
            continue;
        }
        compatible.push(plane);
        if is_primary_plane(card, plane)? {
            primary.push(plane);
        }
    }

    primary
        .first()
        .copied()
        .or_else(|| compatible.first().copied())
        .ok_or_else(|| "no compatible planes found".to_string())
}

fn find_cursor_plane(
    card: &Card,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
) -> Result<Option<plane::Handle>, String> {
    let planes = card
        .plane_handles()
        .map_err(|e| format!("could not list planes: {e}"))?;
    let mut compatible = Vec::new();

    for plane in planes {
        let info = card
            .get_plane(plane)
            .map_err(|e| format!("failed to read plane info: {e}"))?;
        let compatible_crtcs = resources.filter_crtcs(info.possible_crtcs());
        if !compatible_crtcs.contains(&crtc_handle) {
            continue;
        }
        if is_cursor_plane(card, plane)? {
            compatible.push(plane);
        }
    }

    Ok(compatible.first().copied())
}

fn prop_handle(
    props: &HashMap<String, property::Info>,
    name: &str,
) -> Result<property::Handle, String> {
    props
        .get(name)
        .map(|info| info.handle())
        .ok_or_else(|| format!("missing property {name}"))
}

fn create_cursor_plane<T: AsFd>(
    card: &Card,
    gbm_device: &GbmDevice<T>,
    resources: &ResourceHandles,
    crtc_handle: crtc::Handle,
    theme: &DrmCursorTheme,
) -> Result<Option<CursorPlane>, String> {
    let Some(handle) = find_cursor_plane(card, resources, crtc_handle)? else {
        return Ok(None);
    };
    let props = card
        .get_properties(handle)
        .and_then(|props| props.as_hashmap(card))
        .map_err(|e| format!("failed to read cursor plane properties: {e}"))?;
    let props = Arc::new(props);

    let size = CURSOR_PLANE_SIZE;
    let mut bo = gbm_device
        .create_buffer_object(
            size.0,
            size.1,
            GbmFormat::Argb8888,
            BufferObjectFlags::CURSOR | BufferObjectFlags::WRITE | BufferObjectFlags::LINEAR,
        )
        .map_err(|e| format!("failed to create cursor bo: {e}"))?;

    bo.write(theme.cursor(CursorIcon::Default).plane_bgra())
        .map_err(|e| format!("failed to write cursor bo: {e}"))?;

    let fb = card
        .add_framebuffer(&bo, 32, 32)
        .map_err(|e| format!("failed to create cursor fb: {e}"))?;

    Ok(Some(CursorPlane {
        commit: CursorPlaneCommit {
            handle,
            props,
            fb,
            size,
        },
        bo,
    }))
}

fn cursor_plane_position(
    cursor: CursorState,
    plane_size: (u32, u32),
    hotspot: (f32, f32),
    screen_size: (u32, u32),
) -> Option<(i64, i64)> {
    if !cursor.visible {
        return None;
    }

    let (screen_w, screen_h) = screen_size;
    let min_x = -(plane_size.0 as i64) + 1;
    let min_y = -(plane_size.1 as i64) + 1;
    let max_x = screen_w.saturating_sub(1) as i64;
    let max_y = screen_h.saturating_sub(1) as i64;
    let x = (cursor.pos.0 - hotspot.0).round() as i64;
    let y = (cursor.pos.1 - hotspot.1).round() as i64;
    let x = x.clamp(min_x, max_x);
    let y = y.clamp(min_y, max_y);
    Some((x, y))
}

fn add_cursor_plane_properties(
    req: &mut atomic::AtomicModeReq,
    crtc_handle: crtc::Handle,
    plane: &CursorPlaneCommit,
    cursor: CursorState,
    visual: &CursorVisual,
    screen_size: (u32, u32),
) -> Result<(), String> {
    if let Some((x, y)) = cursor_plane_position(cursor, plane.size, visual.hotspot(), screen_size) {
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "FB_ID")?,
            property::Value::Framebuffer(Some(plane.fb)),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_ID")?,
            property::Value::CRTC(Some(crtc_handle)),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_X")?,
            property::Value::SignedRange(x),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_Y")?,
            property::Value::SignedRange(y),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_W")?,
            property::Value::UnsignedRange(plane.size.0 as u64),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_H")?,
            property::Value::UnsignedRange(plane.size.1 as u64),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_X")?,
            property::Value::UnsignedRange(0),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_Y")?,
            property::Value::UnsignedRange(0),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_W")?,
            property::Value::UnsignedRange((plane.size.0 as u64) << 16),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "SRC_H")?,
            property::Value::UnsignedRange((plane.size.1 as u64) << 16),
        );
    } else {
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "FB_ID")?,
            property::Value::Framebuffer(None),
        );
        req.add_property(
            plane.handle,
            prop_handle(&plane.props, "CRTC_ID")?,
            property::Value::CRTC(None),
        );
    }

    Ok(())
}

fn send_animation_pulse(
    tree_tx: &Sender<TreeMsg>,
    presented_at: Instant,
    predicted_next_present_at: Instant,
    log_render: bool,
) -> bool {
    let msg = TreeMsg::AnimationPulse {
        presented_at,
        predicted_next_present_at,
        trace: None,
    };

    match tree_tx.try_send(msg) {
        Ok(()) => true,
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking send");
            }
            tree_tx.send(msg).is_ok()
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

fn send_present_timing(
    event_tx: &Sender<EventMsg>,
    presented_at: Instant,
    predicted_next_present_at: Instant,
) {
    let msg = EventMsg::PresentTiming {
        presented_at,
        predicted_next_present_at,
    };

    match event_tx.try_send(msg) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}

fn record_drm_pipeline_scene_received(
    stats: Option<&RendererStatsCollector>,
    pipeline_render_queued_at: Option<Instant>,
    received_at: Instant,
) {
    if let Some(stats) = stats {
        stats.record_pipeline_draw_started(pipeline_render_queued_at, received_at);
    }
}

fn record_drm_pipeline_swap_done(
    stats: Option<&RendererStatsCollector>,
    pipeline_submitted_at: Option<Instant>,
    swap_done_at: Instant,
) {
    if let (Some(stats), Some(submitted_at)) = (stats, pipeline_submitted_at) {
        stats.record_pipeline_submit_to_swap(submitted_at, swap_done_at);
    }
}

fn record_drm_display_interval(stats: Option<&RendererStatsCollector>, frame_interval: Duration) {
    if let Some(stats) = stats {
        stats.record_display_interval(frame_interval);
    }
}

fn record_drm_pipeline_presented(
    stats: Option<&RendererStatsCollector>,
    pipeline_submitted_at: Option<Instant>,
    pipeline_swap_done_at: Option<Instant>,
    presented_at: Instant,
) {
    let Some(stats) = stats else {
        return;
    };

    if let Some(submitted_at) = pipeline_submitted_at {
        stats.record_pipeline(submitted_at, presented_at);
    }

    if let Some(swap_done_at) = pipeline_swap_done_at {
        stats.record_pipeline_swap_to_frame_callback(swap_done_at, presented_at);
    }
}

fn should_defer_cursor_only_commit(
    submit_primary: bool,
    submit_cursor: bool,
    follow_up_primary_until: Option<Instant>,
    now: Instant,
) -> bool {
    submit_cursor
        && !submit_primary
        && follow_up_primary_until
            .map(|deadline| now < deadline)
            .unwrap_or(false)
}

fn drm_session_mode_changed(
    current_dimensions: (u32, u32),
    next_dimensions: (u32, u32),
    current_frame_interval: Duration,
    next_frame_interval: Duration,
) -> bool {
    next_dimensions != current_dimensions || next_frame_interval != current_frame_interval
}

fn should_consider_unchanged_primary_skip(
    commit_in_flight: bool,
    primary_dirty: bool,
    video_sync_required: bool,
    hw_cursor_enabled: bool,
    cursor_visible: bool,
    animate: bool,
) -> bool {
    !commit_in_flight
        && primary_dirty
        && !video_sync_required
        && !animate
        && (hw_cursor_enabled || !cursor_visible)
}

fn should_prepare_primary(
    desired_generation: u64,
    committed_generation: u64,
    in_flight_generation: Option<u64>,
    prepared_generation: Option<u64>,
) -> bool {
    desired_generation != committed_generation
        && in_flight_generation != Some(desired_generation)
        && prepared_generation.is_none()
}

fn should_submit_prepared_primary(
    commit_in_flight: bool,
    committed_generation: u64,
    prepared_generation: Option<u64>,
) -> bool {
    !commit_in_flight
        && prepared_generation
            .map(|generation| generation != committed_generation)
            .unwrap_or(false)
}

fn should_schedule_video_cleanup(
    submitted_frame_needs_cleanup: bool,
    newer_frame_completed_video_sync: bool,
) -> bool {
    // A successful sync for a newer primary has already polled the submitted frame's retired
    // imports and carries forward any remaining cleanup requirement. A failed sync has not.
    submitted_frame_needs_cleanup && !newer_frame_completed_video_sync
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{RendererStatsSnapshot, RendererTimingMetric};

    fn assert_timing(
        snapshot: &RendererStatsSnapshot,
        metric: RendererTimingMetric,
        expected_avg_ms: f64,
    ) {
        let timing = snapshot.timing(metric);
        assert_eq!(timing.count, 1);
        assert!((timing.avg_ms - expected_avg_ms).abs() < 0.001);
    }

    #[test]
    fn drm_present_state_predicts_from_mode_interval_after_sparse_primary_flip() {
        let start = Instant::now();
        let mut present = DrmPresentState::new(Duration::from_millis(16));

        let first_predicted = present.observe_present(start);
        assert_eq!(first_predicted, start + Duration::from_millis(16));

        let second_presented = start + Duration::from_millis(33);
        let second_predicted = present.observe_present(second_presented);
        assert_eq!(
            second_predicted,
            second_presented + Duration::from_millis(16)
        );
    }

    #[test]
    fn drm_display_stats_use_mode_interval_after_sparse_primary_flip() {
        let start = Instant::now();
        let mode_interval = frame_interval_for_refresh_hz(60.0);
        let mut present = DrmPresentState::new(mode_interval);
        let stats = RendererStatsCollector::new();

        let sparse_presented = start + Duration::from_millis(33);
        let predicted_next_present_at = present.observe_present(sparse_presented);
        assert_eq!(predicted_next_present_at, sparse_presented + mode_interval);
        record_drm_display_interval(Some(&stats), present.mode_frame_interval());

        let snapshot = stats.snapshot();
        assert!((snapshot.display_frame_ms - mode_interval.as_secs_f64() * 1_000.0).abs() < 0.001);
        assert!((snapshot.display_fps - 60.0).abs() < 0.001);
    }

    #[test]
    fn drm_display_stats_seed_mode_interval_without_presented_frames() {
        let stats = RendererStatsCollector::new();
        stats.record_display_interval(frame_interval_for_refresh_hz(30.0));
        let _ = stats.snapshot();

        let mode_interval = frame_interval_for_refresh_hz(60.0);
        record_drm_display_interval(Some(&stats), mode_interval);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.frame_count, 0);
        assert!((snapshot.display_frame_ms - mode_interval.as_secs_f64() * 1_000.0).abs() < 0.001);
        assert!((snapshot.display_fps - 60.0).abs() < 0.001);
    }

    #[test]
    fn precise_mode_refresh_uses_drm_timing_fields() {
        let flags = control::ModeFlags::empty();
        let refresh = precise_mode_refresh_hz(54_000, 1_125, 800, 0, flags).unwrap();
        assert!((refresh - 60.0).abs() < 0.001);
    }

    #[test]
    fn precise_mode_refresh_applies_interlace_double_scan_and_vscan() {
        let interlaced =
            precise_mode_refresh_hz(54_000, 1_125, 800, 0, control::ModeFlags::INTERLACE).unwrap();
        assert!((interlaced - 120.0).abs() < 0.001);

        let double_scan =
            precise_mode_refresh_hz(54_000, 1_125, 800, 0, control::ModeFlags::DBLSCAN).unwrap();
        assert!((double_scan - 30.0).abs() < 0.001);

        let vscan =
            precise_mode_refresh_hz(54_000, 1_125, 800, 2, control::ModeFlags::empty()).unwrap();
        assert!((vscan - 30.0).abs() < 0.001);
    }

    #[test]
    fn precise_mode_refresh_rejects_missing_timing_fields() {
        assert!(precise_mode_refresh_hz(0, 1_125, 800, 0, control::ModeFlags::empty()).is_none());
        assert!(precise_mode_refresh_hz(54_000, 0, 800, 0, control::ModeFlags::empty()).is_none());
        assert!(
            precise_mode_refresh_hz(54_000, 1_125, 0, 0, control::ModeFlags::empty()).is_none()
        );
    }

    #[test]
    fn defer_cursor_only_commit_requires_active_follow_up_window() {
        let now = Instant::now();
        assert!(should_defer_cursor_only_commit(
            false,
            true,
            Some(now + FOLLOW_UP_PRIMARY_WINDOW),
            now,
        ));
    }

    #[test]
    fn defer_cursor_only_commit_never_blocks_primary_work() {
        let now = Instant::now();
        assert!(!should_defer_cursor_only_commit(
            true,
            true,
            Some(now + FOLLOW_UP_PRIMARY_WINDOW),
            now,
        ));
    }

    #[test]
    fn defer_cursor_only_commit_expires_with_deadline() {
        let now = Instant::now();
        assert!(!should_defer_cursor_only_commit(
            false,
            true,
            Some(now),
            now,
        ));
        assert!(!should_defer_cursor_only_commit(false, true, None, now));
    }

    #[test]
    fn drm_session_mode_change_detects_same_size_refresh_change() {
        assert!(drm_session_mode_changed(
            (1024, 600),
            (1024, 600),
            frame_interval_for_refresh_hz(60.0),
            frame_interval_for_refresh_hz(50.0),
        ));
        assert!(!drm_session_mode_changed(
            (1024, 600),
            (1024, 600),
            frame_interval_for_refresh_hz(60.0),
            frame_interval_for_refresh_hz(60.0),
        ));
        assert!(drm_session_mode_changed(
            (1024, 600),
            (1280, 720),
            frame_interval_for_refresh_hz(60.0),
            frame_interval_for_refresh_hz(60.0),
        ));
    }

    #[test]
    fn drm_pipeline_helpers_record_render_queue_swap_and_present_spans() {
        let stats = RendererStatsCollector::new();
        let submitted_at = Instant::now();
        let render_queued_at = submitted_at + Duration::from_millis(8);
        let render_received_at = submitted_at + Duration::from_millis(10);
        let swap_done_at = submitted_at + Duration::from_millis(11);
        let presented_at = submitted_at + Duration::from_millis(18);

        record_drm_pipeline_scene_received(
            Some(&stats),
            Some(render_queued_at),
            render_received_at,
        );
        record_drm_pipeline_swap_done(Some(&stats), Some(submitted_at), swap_done_at);
        record_drm_pipeline_presented(
            Some(&stats),
            Some(submitted_at),
            Some(swap_done_at),
            presented_at,
        );

        let snapshot = stats.snapshot();
        assert_timing(&snapshot, RendererTimingMetric::PipelineRenderQueue, 2.0);
        assert_timing(&snapshot, RendererTimingMetric::PipelineSubmitToSwap, 11.0);
        assert_timing(&snapshot, RendererTimingMetric::Pipeline, 18.0);
        assert_timing(
            &snapshot,
            RendererTimingMetric::PipelineSwapToFrameCallback,
            7.0,
        );
    }

    #[test]
    fn unchanged_primary_skip_requires_idle_dirty_primary_with_cursor_coverage() {
        assert!(should_consider_unchanged_primary_skip(
            false, true, false, true, true, false,
        ));
        assert!(!should_consider_unchanged_primary_skip(
            false, true, false, true, true, true,
        ));
        assert!(!should_consider_unchanged_primary_skip(
            true, true, false, true, true, false,
        ));
        assert!(!should_consider_unchanged_primary_skip(
            false, false, false, true, true, false,
        ));
        assert!(!should_consider_unchanged_primary_skip(
            false, true, false, false, true, false,
        ));
        assert!(should_consider_unchanged_primary_skip(
            false, true, false, false, false, false,
        ));
        assert!(!should_consider_unchanged_primary_skip(
            false, true, true, true, true, false,
        ));
    }

    #[test]
    fn primary_preparation_pipelines_one_bounded_staged_generation() {
        assert!(should_prepare_primary(3, 1, Some(2), None));
        assert!(!should_prepare_primary(2, 1, Some(2), None));
        assert!(!should_prepare_primary(3, 1, Some(2), Some(3)));
        assert!(!should_prepare_primary(4, 1, Some(2), Some(3)));
        assert!(!should_prepare_primary(3, 3, None, None));
    }

    #[test]
    fn staged_primary_submits_after_previous_flip_even_when_newer_work_exists() {
        assert!(should_submit_prepared_primary(false, 2, Some(3)));
        assert!(!should_submit_prepared_primary(true, 2, Some(3)));
        assert!(!should_submit_prepared_primary(false, 3, Some(3)));
        assert!(!should_submit_prepared_primary(false, 2, None));
    }

    #[test]
    fn successful_newer_video_sync_supersedes_older_cleanup_wakeup() {
        assert!(should_schedule_video_cleanup(true, false));
        assert!(!should_schedule_video_cleanup(true, true));
        assert!(!should_schedule_video_cleanup(false, false));
    }

    #[test]
    fn cursor_plane_position_clamps_visible_cursor_to_screen_bounds() {
        let position = cursor_plane_position(
            CursorState {
                pos: (-20.0, 200.0),
                visible: true,
            },
            (64, 64),
            (7.0, 2.0),
            (128, 128),
        );

        assert_eq!(position, Some((-27, 127)));
    }

    #[test]
    fn cursor_plane_position_accounts_for_hotspot_offset() {
        let position = cursor_plane_position(
            CursorState {
                pos: (40.0, 24.0),
                visible: true,
            },
            (64, 64),
            (7.0, 2.0),
            (128, 128),
        );

        assert_eq!(position, Some((33, 22)));
    }

    #[test]
    fn cursor_plane_position_returns_none_when_hidden() {
        let position = cursor_plane_position(
            CursorState {
                pos: (10.0, 20.0),
                visible: false,
            },
            (64, 64),
            (11.5, 11.5),
            (128, 128),
        );

        assert_eq!(position, None);
    }
}

fn add_plane_properties(
    req: &mut atomic::AtomicModeReq,
    plane: plane::Handle,
    plane_props: &HashMap<String, property::Info>,
    crtc_handle: crtc::Handle,
    fb: framebuffer::Handle,
) -> Result<(), String> {
    req.add_property(
        plane,
        prop_handle(plane_props, "FB_ID")?,
        property::Value::Framebuffer(Some(fb)),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_ID")?,
        property::Value::CRTC(Some(crtc_handle)),
    );
    Ok(())
}

fn add_plane_geometry(
    req: &mut atomic::AtomicModeReq,
    plane: plane::Handle,
    plane_props: &HashMap<String, property::Info>,
    mode: &control::Mode,
) -> Result<(), String> {
    let (width, height) = mode.size();
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_X")?,
        property::Value::UnsignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_Y")?,
        property::Value::UnsignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_W")?,
        property::Value::UnsignedRange((width as u64) << 16),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "SRC_H")?,
        property::Value::UnsignedRange((height as u64) << 16),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_X")?,
        property::Value::SignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_Y")?,
        property::Value::SignedRange(0),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_W")?,
        property::Value::UnsignedRange(width as u64),
    );
    req.add_property(
        plane,
        prop_handle(plane_props, "CRTC_H")?,
        property::Value::UnsignedRange(height as u64),
    );
    Ok(())
}

fn is_ebusy(err: &str) -> bool {
    err.contains("Device or resource busy") || err.contains("EBUSY")
}

fn monotonic_now() -> Option<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0
        || time.tv_sec < 0
        || time.tv_nsec < 0
    {
        return None;
    }
    Some(Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}

fn duration_since(later: Duration, earlier: Duration) -> Option<Duration> {
    later.checked_sub(earlier)
}

#[derive(Debug)]
struct EglDiagnostics {
    version: String,
    vendor: String,
    client_apis: String,
    native_visual_id: Option<EGLint>,
    min_swap_interval: Option<EGLint>,
    max_swap_interval: Option<EGLint>,
    surface_size: (Option<EGLint>, Option<EGLint>),
    native_fence_sync: bool,
    fence_sync: bool,
    wait_sync: bool,
    buffer_age: bool,
}

fn egl_query_string(egl: &egl::Egl, display: EGLDisplay, name: EGLint) -> String {
    let value = unsafe { egl.QueryString(display, name) };
    if value.is_null() {
        return "unavailable".to_string();
    }
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

fn egl_config_attribute(
    egl: &egl::Egl,
    display: EGLDisplay,
    config: EGLConfig,
    name: EGLint,
) -> Option<EGLint> {
    let mut value = 0;
    (unsafe { egl.GetConfigAttrib(display, config, name, &mut value) } != egl::FALSE)
        .then_some(value)
}

fn egl_surface_attribute(
    egl: &egl::Egl,
    display: EGLDisplay,
    surface: EGLSurface,
    name: EGLint,
) -> Option<EGLint> {
    let mut value = 0;
    (unsafe { egl.QuerySurface(display, surface, name, &mut value) } != egl::FALSE).then_some(value)
}

fn egl_error_code(egl: &egl::Egl) -> u32 {
    unsafe { egl.GetError() as u32 }
}

fn egl_config_diagnostics(egl: &egl::Egl, display: EGLDisplay, config: EGLConfig) -> String {
    let attribute = |name| egl_config_attribute(egl, display, config, name);
    format!(
        "id={:?} visual={:#010x} rgba={:?}/{:?}/{:?}/{:?} surface_type={:#x} renderable_type={:#x}",
        attribute(egl::CONFIG_ID as EGLint),
        attribute(egl::NATIVE_VISUAL_ID as EGLint).unwrap_or_default(),
        attribute(egl::RED_SIZE as EGLint),
        attribute(egl::GREEN_SIZE as EGLint),
        attribute(egl::BLUE_SIZE as EGLint),
        attribute(egl::ALPHA_SIZE as EGLint),
        attribute(egl::SURFACE_TYPE as EGLint).unwrap_or_default(),
        attribute(egl::RENDERABLE_TYPE as EGLint).unwrap_or_default(),
    )
}

fn gl_string(name: gl::types::GLenum) -> String {
    let value = unsafe { gl::GetString(name) };
    if value.is_null() {
        return "unavailable".to_string();
    }
    unsafe { CStr::from_ptr(value.cast()) }
        .to_string_lossy()
        .into_owned()
}

fn load_egl() -> Result<(Library, egl::Egl), String> {
    let lib = unsafe { Library::new("libEGL.so.1") }
        .map_err(|e| format!("failed to load libEGL: {e}"))?;
    let get_proc = unsafe {
        lib.get::<unsafe extern "system" fn(*const std::ffi::c_char) -> *const c_void>(
            b"eglGetProcAddress\0",
        )
        .map_err(|e| format!("failed to load eglGetProcAddress: {e}"))?
    };

    let egl = egl::Egl::load_with(|name| unsafe {
        let symbol = CString::new(name).expect("egl symbol");
        let ptr = get_proc(symbol.as_ptr());
        if !ptr.is_null() {
            return ptr;
        }
        let raw = format!("{name}\0");
        lib.get::<*const c_void>(raw.as_bytes())
            .map(|s| *s)
            .unwrap_or(ptr::null())
    });

    Ok((lib, egl))
}

fn egl_get_platform_display(egl: &egl::Egl, display_ptr: *mut c_void) -> EGLDisplay {
    if egl.GetPlatformDisplayEXT.is_loaded() {
        unsafe { egl.GetPlatformDisplayEXT(EGL_PLATFORM_GBM_KHR, display_ptr, ptr::null()) }
    } else if egl.GetPlatformDisplay.is_loaded() {
        unsafe { egl.GetPlatformDisplay(EGL_PLATFORM_GBM_KHR, display_ptr, ptr::null()) }
    } else {
        unsafe { egl.GetDisplay(display_ptr as egl::EGLNativeDisplayType) }
    }
}

fn init_egl(
    egl: &egl::Egl,
    gbm_device_ptr: *mut c_void,
    gbm_surface_ptr: *mut c_void,
) -> Result<(EGLDisplay, EGLContext, EGLSurface, EglDiagnostics), String> {
    let display = egl_get_platform_display(egl, gbm_device_ptr);
    if display == egl::NO_DISPLAY {
        return Err("failed to get EGL display".to_string());
    }

    let mut major: EGLint = 0;
    let mut minor: EGLint = 0;
    if unsafe { egl.Initialize(display, &mut major, &mut minor) } == egl::FALSE {
        return Err("failed to initialize EGL".to_string());
    }

    if unsafe { egl.BindAPI(egl::OPENGL_ES_API) } == egl::FALSE {
        return Err("failed to bind EGL OpenGL ES API".to_string());
    }

    // EGL_NATIVE_VISUAL_ID is a queried config property, not a portable
    // eglChooseConfig filter. Ask EGL for every RGB8/ES3 window candidate, then select
    // the config whose native visual exactly matches the XRGB8888 GBM surface. This is
    // the selection pattern used by kmscube, SDL KMSDRM, and mpv.
    let config_attribs: [EGLint; 13] = [
        egl::SURFACE_TYPE as EGLint,
        egl::WINDOW_BIT as EGLint,
        egl::RENDERABLE_TYPE as EGLint,
        EGL_OPENGL_ES3_BIT_KHR,
        egl::RED_SIZE as EGLint,
        8,
        egl::GREEN_SIZE as EGLint,
        8,
        egl::BLUE_SIZE as EGLint,
        8,
        egl::ALPHA_SIZE as EGLint,
        0,
        egl::NONE as EGLint,
    ];

    let mut total_configs: EGLint = 0;
    if unsafe { egl.GetConfigs(display, ptr::null_mut(), 0, &mut total_configs) } == egl::FALSE
        || total_configs <= 0
    {
        return Err(format!(
            "failed to enumerate EGL configs (EGL error={:#06x})",
            egl_error_code(egl)
        ));
    }

    let mut configs = vec![ptr::null(); total_configs as usize];
    let mut matched_configs: EGLint = 0;
    if unsafe {
        egl.ChooseConfig(
            display,
            config_attribs.as_ptr(),
            configs.as_mut_ptr(),
            total_configs,
            &mut matched_configs,
        )
    } == egl::FALSE
        || matched_configs <= 0
    {
        return Err(format!(
            "failed to choose RGB8 ES3 EGL configs (EGL error={:#06x})",
            egl_error_code(egl)
        ));
    }
    configs.truncate(matched_configs as usize);

    let required_visual = GbmFormat::Xrgb8888 as EGLint;
    let config = configs
        .iter()
        .copied()
        .find(|candidate| {
            egl_config_attribute(
                egl,
                display,
                *candidate,
                egl::NATIVE_VISUAL_ID as EGLint,
            ) == Some(required_visual)
        })
        .ok_or_else(|| {
            let available_visuals = configs
                .iter()
                .filter_map(|candidate| {
                    egl_config_attribute(
                        egl,
                        display,
                        *candidate,
                        egl::NATIVE_VISUAL_ID as EGLint,
                    )
                })
                .map(|visual| format!("{:#010x}", visual as u32))
                .collect::<Vec<_>>();
            format!(
                "no RGB8 ES3 EGL config matches GBM XRGB8888 visual {:#010x}; available visuals={available_visuals:?}",
                required_visual as u32
            )
        })?;
    let config_diagnostics = egl_config_diagnostics(egl, display, config);

    let context_attribs: [EGLint; 3] = [
        egl::CONTEXT_CLIENT_VERSION as EGLint,
        3,
        egl::NONE as EGLint,
    ];
    let context =
        unsafe { egl.CreateContext(display, config, egl::NO_CONTEXT, context_attribs.as_ptr()) };
    if context == egl::NO_CONTEXT {
        return Err(format!(
            "failed to create EGL context ({config_diagnostics}; EGL error={:#06x})",
            egl_error_code(egl)
        ));
    }

    let surface = unsafe {
        egl.CreateWindowSurface(
            display,
            config,
            gbm_surface_ptr as egl::EGLNativeWindowType,
            ptr::null(),
        )
    };
    if surface == egl::NO_SURFACE {
        let error = egl_error_code(egl);
        unsafe {
            egl.DestroyContext(display, context);
            egl.Terminate(display);
        }
        return Err(format!(
            "failed to create EGL window surface ({config_diagnostics}; EGL error={error:#06x})"
        ));
    }

    if unsafe { egl.MakeCurrent(display, surface, surface, context) } == egl::FALSE {
        let error = egl_error_code(egl);
        unsafe {
            egl.DestroySurface(display, surface);
            egl.DestroyContext(display, context);
            egl.Terminate(display);
        }
        return Err(format!(
            "failed to make EGL context current ({config_diagnostics}; EGL error={error:#06x})"
        ));
    }

    // Atomic KMS page flips are the direct-DRM backend's sole presentation clock. Disable
    // EGL's compositor-style swap throttling so the GBM render fence represents GPU work only;
    // otherwise KMS can inherit an additional one-vblank wait from eglSwapBuffers().
    if unsafe { egl.SwapInterval(display, 0) } == egl::FALSE {
        let error = egl_error_code(egl);
        unsafe {
            egl.MakeCurrent(display, egl::NO_SURFACE, egl::NO_SURFACE, egl::NO_CONTEXT);
            egl.DestroySurface(display, surface);
            egl.DestroyContext(display, context);
            egl.Terminate(display);
        }
        return Err(format!(
            "failed to disable EGL swap throttling for direct DRM (EGL error={error:#06x})"
        ));
    }

    let extensions = egl_query_string(egl, display, egl::EXTENSIONS as EGLint);
    let diagnostics = EglDiagnostics {
        version: egl_query_string(egl, display, egl::VERSION as EGLint),
        vendor: egl_query_string(egl, display, egl::VENDOR as EGLint),
        client_apis: egl_query_string(egl, display, egl::CLIENT_APIS as EGLint),
        native_visual_id: egl_config_attribute(
            egl,
            display,
            config,
            egl::NATIVE_VISUAL_ID as EGLint,
        ),
        min_swap_interval: egl_config_attribute(
            egl,
            display,
            config,
            egl::MIN_SWAP_INTERVAL as EGLint,
        ),
        max_swap_interval: egl_config_attribute(
            egl,
            display,
            config,
            egl::MAX_SWAP_INTERVAL as EGLint,
        ),
        surface_size: (
            egl_surface_attribute(egl, display, surface, egl::WIDTH as EGLint),
            egl_surface_attribute(egl, display, surface, egl::HEIGHT as EGLint),
        ),
        native_fence_sync: extensions.contains("EGL_ANDROID_native_fence_sync"),
        fence_sync: extensions.contains("EGL_KHR_fence_sync"),
        wait_sync: extensions.contains("EGL_KHR_wait_sync"),
        buffer_age: extensions.contains("EGL_EXT_buffer_age"),
    };

    Ok((display, context, surface, diagnostics))
}

fn create_frame_surface(egl: &egl::Egl, dimensions: (u32, u32)) -> Result<GlFrameSurface, String> {
    gl::load_with(|s| unsafe {
        let symbol = CString::new(s).expect("gl symbol");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    });

    let interface = skia_safe::gpu::gl::Interface::new_load_with(|name| unsafe {
        if name == "eglGetCurrentDisplay" {
            return ptr::null();
        }
        let symbol = CString::new(name).expect("egl symbol");
        egl.GetProcAddress(symbol.as_ptr()) as *const _
    })
    .ok_or_else(|| "could not create Skia GL interface".to_string())?;

    let gr_context = skia_safe::gpu::direct_contexts::make_gl(interface, None)
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

    Ok(GlFrameSurface::new(dimensions, fb_info, gr_context, 0, 0))
}

fn gbm_bo_diagnostics(bo: &BufferObject<()>) -> String {
    let plane_count = bo.plane_count().min(4);
    let planes = (0..plane_count)
        .map(|plane| {
            format!(
                "{}:stride={} offset={}",
                plane,
                bo.stride_for_plane(plane as i32),
                bo.offset(plane as i32)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{}x{} format={:?} bpp={} modifier={:?} planes={} [{}] framebuffer_api=addfb2_with_linear_legacy_fallback",
        bo.width(),
        bo.height(),
        bo.format(),
        bo.bpp(),
        bo.modifier(),
        bo.plane_count(),
        planes,
    )
}

fn create_renderer_frame_surface(
    rendering_api: RenderingApi,
    egl: &egl::Egl,
    dimensions: (u32, u32),
) -> Result<GlFrameSurface, String> {
    match rendering_api {
        RenderingApi::OpenGl | RenderingApi::Raster => create_frame_surface(egl, dimensions),
        RenderingApi::Auto => unreachable!("auto is resolved before DRM startup"),
        RenderingApi::Metal => Err("DRM does not support Metal renderer".to_string()),
        RenderingApi::Vulkan => Err("DRM Vulkan renderer is not implemented yet".to_string()),
    }
}

fn framebuffer_for_bo(
    card: &Card,
    cache: &mut HashMap<u32, framebuffer::Handle>,
    bo: &BufferObject<()>,
    native_log: &NativeLogRelay,
) -> Result<framebuffer::Handle, String> {
    let handle = unsafe { bo.handle().u32_ };
    if let Some(existing) = cache.get(&handle).copied() {
        return Ok(existing);
    }

    let modifier = bo.modifier();
    let planar_result = match modifier {
        GbmModifier::Invalid => card.add_planar_framebuffer(bo, FbCmd2Flags::empty()),
        _ => card.add_planar_framebuffer(bo, FbCmd2Flags::MODIFIERS),
    };
    let framebuffer = match planar_result {
        Ok(framebuffer) => {
            native_log.info(
                "drm",
                format!("created primary framebuffer via AddFB2 modifier={modifier:?}"),
            );
            framebuffer
        }
        Err(addfb2_err) if matches!(modifier, GbmModifier::Invalid | GbmModifier::Linear) => {
            native_log.warning(
                "drm",
                format!(
                    "AddFB2 failed for linear-compatible GBM buffer ({addfb2_err}); falling back to legacy AddFB"
                ),
            );
            card.add_framebuffer(bo, 24, 32)
                .map_err(|e| format!("failed to create linear DRM framebuffer: {e}"))?
        }
        Err(err) => {
            return Err(format!(
                "failed to create modifier-aware DRM framebuffer for {modifier:?}: {err}; refusing unsafe legacy fallback for a non-linear GBM buffer"
            ));
        }
    };
    cache.insert(handle, framebuffer);
    Ok(framebuffer)
}

#[allow(clippy::too_many_arguments)]
fn prepare_primary_frame(
    generation: u64,
    rendering_api: RenderingApi,
    renderer: &mut SceneRenderer,
    raster_renderer: Option<&mut RasterBackend>,
    frame_surface: &mut GlFrameSurface,
    dimensions: (u32, u32),
    render_state: &RenderState,
    cursor_pos: (f32, f32),
    cursor_visible: bool,
    hw_cursor_enabled: bool,
    cursor_icon: CursorIcon,
    cursor_theme: &DrmCursorTheme,
    video_registry: &Arc<VideoRegistry>,
    video_import: Option<&VideoImportContext>,
    egl_state: &EglState,
    gbm_surface: &Surface<()>,
    force_gpu_finish: bool,
    card: &Card,
    framebuffer_cache: &mut HashMap<u32, framebuffer::Handle>,
    stats: Option<&RendererStatsCollector>,
    profile_render: bool,
    sample_gpu_queue: bool,
    gpu_queue_timer: &mut GpuQueueTimer,
    native_log: &NativeLogRelay,
    last_video_sync_error: &mut Option<String>,
    logged_video_import: &mut bool,
    latest_frame: &LatestFrameStore,
) -> Result<PreparedPrimaryFrame, String> {
    gpu_queue_timer.poll(stats, native_log);
    let render_started_at = Instant::now();
    let (
        render_timings,
        captured_frame,
        video_sync_succeeded,
        video_needs_cleanup,
        imported_video_frames,
        newest_video_submitted_at,
    ) = match rendering_api {
        RenderingApi::OpenGl => {
            let mut frame = frame_surface.frame();
            let (
                video_sync_succeeded,
                video_needs_cleanup,
                imported_video_frames,
                newest_video_submitted_at,
            ) = match renderer.sync_video_frames(&mut frame, video_registry, video_import) {
                Ok(result) => {
                    if last_video_sync_error.take().is_some() {
                        native_log.info("video", "Prime video import recovered");
                    }
                    if result.imported_frames > 0 && !*logged_video_import {
                        native_log.info("video", "Imported first DMA-BUF frame successfully");
                        *logged_video_import = true;
                    }
                    if let Some(diagnostics) = result.first_frame_diagnostics {
                        native_log.info("video", format!("First frame samples: {diagnostics}"));
                    }
                    (
                        true,
                        result.needs_cleanup,
                        result.imported_frames,
                        result.newest_import_submitted_at,
                    )
                }
                Err(err) => {
                    // Keep cleanup sticky across a failed import/sync. This prepared frame may
                    // still be presented using the last good video image, then its page flip
                    // retries sync.
                    if last_video_sync_error.as_deref() != Some(err.as_str()) {
                        native_log.error("video", format!("video sync failed: {err}"));
                        *last_video_sync_error = Some(err);
                    }
                    (false, true, 0, None)
                }
            };

            let render_timings = if sample_gpu_queue {
                let mut begin_gpu_queue_sample = || gpu_queue_timer.begin_sample(native_log);
                if profile_render {
                    renderer.render_profiled_with_before_flush(
                        &mut frame,
                        render_state,
                        &mut begin_gpu_queue_sample,
                    )
                } else {
                    renderer.render_with_before_flush(
                        &mut frame,
                        render_state,
                        &mut begin_gpu_queue_sample,
                    )
                }
            } else if profile_render {
                renderer.render_profiled(&mut frame, render_state)
            } else {
                renderer.render(&mut frame, render_state)
            };
            if !hw_cursor_enabled && cursor_visible {
                draw_software_cursor(
                    renderer,
                    &mut frame,
                    cursor_theme.cursor(cursor_icon),
                    cursor_pos,
                );
            }
            if sample_gpu_queue {
                gpu_queue_timer.end_sample(render_state.render_version, &render_timings);
            }
            (
                render_timings,
                frame_surface.capture_rgba_pixels(),
                video_sync_succeeded,
                video_needs_cleanup,
                imported_video_frames,
                newest_video_submitted_at,
            )
        }
        RenderingApi::Raster => {
            let raster_renderer = raster_renderer
                .ok_or_else(|| "DRM raster renderer was not initialized".to_string())?;
            let (raster_frame, render_timings) = raster_renderer.render_with_timings(render_state);
            frame_surface.present_rgba_pixels(dimensions.0, dimensions.1, &raster_frame.data)?;
            (
                render_timings,
                Some((dimensions.0, dimensions.1, raster_frame.data)),
                true,
                false,
                0,
                None,
            )
        }
        RenderingApi::Auto => unreachable!("auto is resolved before DRM startup"),
        RenderingApi::Metal => return Err("DRM does not support Metal renderer".to_string()),
        RenderingApi::Vulkan => {
            return Err("DRM Vulkan renderer is not implemented yet".to_string());
        }
    };

    if let Some(stats) = stats {
        stats.record_render_timings(render_started_at.elapsed(), &render_timings);
    }
    if profile_render && render_frame_has_slow_stage(&render_timings) {
        native_log.info(
            "renderer_slow_frame",
            format_slow_render_frame_log("drm", &render_timings, render_state.scene.summary()),
        );
    }

    let present_submit_started_at = Instant::now();

    if force_gpu_finish {
        let finish_started_at = Instant::now();
        unsafe {
            gl::Finish();
        }
        if let Some(stats) = stats {
            stats.record_drm_forced_gpu_finish_before_swap(finish_started_at.elapsed());
        }
    }

    let swap_started_at = Instant::now();
    if unsafe {
        egl_state
            .egl
            .SwapBuffers(egl_state.display, egl_state.surface)
    } == egl::FALSE
    {
        return Err("eglSwapBuffers failed".to_string());
    }
    if let Some(stats) = stats {
        stats.record_drm_egl_swap_buffers(swap_started_at.elapsed());
    }

    if force_gpu_finish {
        let finish_started_at = Instant::now();
        unsafe {
            gl::Finish();
        }
        if let Some(stats) = stats {
            stats.record_drm_forced_gpu_finish_after_swap(finish_started_at.elapsed());
        }
    }

    if let Some((width, height, pixels)) = captured_frame {
        latest_frame.publish_rgba(width, height, 1.0, pixels);
    }

    let lock_started_at = Instant::now();
    let bo = unsafe { gbm_surface.lock_front_buffer() }
        .map_err(|e| format!("locking GBM buffer failed: {e}"))?;
    if let Some(stats) = stats {
        stats.record_drm_gbm_lock_front_buffer(lock_started_at.elapsed());
    }

    let framebuffer_started_at = Instant::now();
    let fb = framebuffer_for_bo(card, framebuffer_cache, &bo, native_log)?;
    if let Some(stats) = stats {
        stats.record_drm_framebuffer_lookup(framebuffer_started_at.elapsed());
    }

    Ok(PreparedPrimaryFrame {
        generation,
        render_version: render_state.render_version,
        pipeline_submitted_at: render_state.pipeline_submitted_at,
        pipeline_swap_done_at: None,
        bo,
        fb,
        video_sync_succeeded,
        video_needs_cleanup,
        imported_video_frames,
        newest_video_submitted_at,
        prepared_at: Instant::now(),
        atomic_commit_submitted_at: None,
        atomic_commit_monotonic_at: None,
        present_submit_duration: present_submit_started_at.elapsed(),
    })
}

fn draw_software_cursor(
    renderer: &mut SceneRenderer,
    frame: &mut crate::renderer::RenderFrame<'_>,
    visual: &CursorVisual,
    cursor_pos: (f32, f32),
) {
    let (cursor_width, cursor_height) = visual.size();
    let hotspot = visual.hotspot();
    let x = cursor_pos.0 - hotspot.0;
    let y = cursor_pos.1 - hotspot.1;
    let canvas = frame.surface_mut().canvas();
    let sampling =
        skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
    let paint = Paint::default();
    let dst = Rect::from_xywh(x, y, cursor_width as f32, cursor_height as f32);
    canvas.draw_image_rect_with_sampling_options(visual.image(), None, dst, sampling, &paint);

    renderer.flush(frame);
}

#[derive(Clone)]
pub(crate) struct DrmRunConfig {
    pub(crate) requested_size: Option<(u32, u32)>,
    pub(crate) card_path: Option<String>,
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

pub(crate) struct DrmRunContext {
    pub(crate) startup_tx: StartupSender<Result<(), String>>,
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
    let DrmRunContext {
        startup_tx,
        stop,
        running_flag,
        presenter_wake,
        input_wake,
        tree_tx,
        render_rx,
        cursor_icon_rx,
        cursor_state,
        event_tx,
        screen_tx,
        render_counter,
        native_log,
        stats,
        latest_frame,
        video_registry,
    } = context;

    let log_render = config.render_log;
    if config.force_gpu_finish {
        native_log.info(
            "drm",
            "diagnostic GPU finishes before and after eglSwapBuffers are enabled",
        );
    }
    if config.renderer_stats_log {
        native_log.info(
            "drm",
            "sampled slow-frame profiling enabled (at most one frame per second)",
        );
    }
    let _requested_raster_present = config.raster_present;
    let mut startup_tx = Some(startup_tx);
    let retry_interval = Duration::from_millis(config.retry_interval_ms as u64);
    let mut startup_retries_remaining = config.startup_retries;
    let mut last_dimensions: Option<(u32, u32)> = None;
    let hotplug_interval = Duration::from_millis(750);
    let mut logged_cursor_info = false;
    let mut logged_mode_info = false;
    let cursor_theme = match DrmCursorTheme::load(&config.asset_config, &config.cursor_overrides) {
        Ok(theme) => theme,
        Err(err) => {
            if let Some(startup_tx) = startup_tx.take() {
                let _ = startup_tx.send(Err(format!("DRM cursor setup failed: {err}")));
            }
            running_flag.store(false, Ordering::Relaxed);
            return;
        }
    };

    loop {
        if stop.load(Ordering::Relaxed) {
            if let Some(startup_tx) = startup_tx.take() {
                let _ = startup_tx.send(Err("DRM startup aborted".to_string()));
            }
            running_flag.store(false, Ordering::Relaxed);
            break;
        }

        let card = match open_card(config.card_path.as_deref()) {
            Ok(card) => card,
            Err(err) => {
                if handle_startup_failure(
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    err,
                ) {
                    break;
                }

                continue;
            }
        };

        let card = Arc::new(card);

        if let Err(err) = card.acquire_master_lock() {
            if handle_startup_failure(
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("acquiring DRM master failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        if let Err(err) = card.set_client_capability(ClientCapability::UniversalPlanes, true) {
            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("enabling universal planes failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        if let Err(err) = card.set_client_capability(ClientCapability::Atomic, true) {
            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("enabling atomic modesetting failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        let gbm_device = match GbmDevice::new(card.as_fd()) {
            Ok(device) => device,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating GBM device failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let resources = match card.resource_handles() {
            Ok(handles) => handles,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("querying DRM resources failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (connector, mode, crtc_handle, encoder_handle) =
            match first_connected_connector(&card, &resources, config.requested_size) {
                Ok(values) => values,
                Err(err) => {
                    if handle_startup_failure_with_card(
                        &card,
                        &mut startup_tx,
                        &running_flag,
                        &stop,
                        &mut startup_retries_remaining,
                        retry_interval,
                        format!("selecting connector failed: {err}"),
                    ) {
                        break;
                    }

                    continue;
                }
            };

        let plane = match find_primary_plane(&card, &resources, crtc_handle) {
            Ok(handle) => handle,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("selecting primary plane failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let con_props = match card
            .get_properties(connector)
            .and_then(|props| props.as_hashmap(card.as_ref()))
        {
            Ok(props) => props,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("reading connector properties failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let crtc_props = match card
            .get_properties(crtc_handle)
            .and_then(|props| props.as_hashmap(card.as_ref()))
        {
            Ok(props) => props,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("reading CRTC properties failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let plane_props = match card
            .get_properties(plane)
            .and_then(|props| props.as_hashmap(card.as_ref()))
        {
            Ok(props) => props,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("reading plane properties failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (width, height) = mode.size();
        let dimensions = (width as u32, height as u32);
        let refresh_hz = mode_refresh_hz(&mode);
        let frame_interval = mode_frame_interval(&mode);
        record_drm_display_interval(stats.as_deref(), frame_interval);
        let _ = screen_tx.send(dimensions);
        let _ = input_wake.signal();
        if !logged_mode_info {
            let driver = card.get_driver().ok();
            let cap = |capability| card.get_driver_capability(capability).unwrap_or_default();
            let plane_formats = card
                .get_plane(plane)
                .map(|info| format!("{:?}", info.formats()))
                .unwrap_or_else(|error| format!("unavailable ({error})"));
            let usage = BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING;
            native_log.info(
                "drm",
                format!(
                    "DRM driver: name={} version={:?} description={} GBM_backend={} XRGB8888_scanout_render={}",
                    driver
                        .as_ref()
                        .map(|driver| driver.name().to_string_lossy())
                        .unwrap_or_else(|| "unavailable".into()),
                    driver.as_ref().map(|driver| driver.version),
                    driver
                        .as_ref()
                        .map(|driver| driver.description().to_string_lossy())
                        .unwrap_or_else(|| "unavailable".into()),
                    gbm_device.backend_name(),
                    gbm_device.is_format_supported(GbmFormat::Xrgb8888, usage),
                ),
            );
            native_log.info(
                "drm",
                format!(
                    "DRM capabilities: monotonic_timestamps={} addfb2_modifiers={} syncobj={} syncobj_timeline={} async_page_flip={} atomic_async_page_flip={}",
                    cap(DriverCapability::MonotonicTimestamp),
                    cap(DriverCapability::AddFB2Modifiers),
                    cap(DriverCapability::SyncObj),
                    cap(DriverCapability::TimelineSyncObj),
                    cap(DriverCapability::ASyncPageFlip),
                    cap(DriverCapability::AtomicASyncPageFlip),
                ),
            );
            native_log.info(
                "drm",
                format!(
                    "DRM resources: connector={} encoder={} crtc={} plane={} plane_formats={} IN_FENCE_FD={} OUT_FENCE_PTR={}",
                    u32::from(connector),
                    u32::from(encoder_handle),
                    u32::from(crtc_handle),
                    u32::from(plane),
                    plane_formats,
                    plane_props.contains_key("IN_FENCE_FD"),
                    crtc_props.contains_key("OUT_FENCE_PTR"),
                ),
            );
            native_log.info(
                "drm",
                format!(
                    "DRM mode: name={} clock={}kHz H={} {} {} {} V={} {} {} {} flags={:?} ({:.3} ms/frame, {:.6} Hz computed)",
                    mode.name().to_string_lossy(),
                    mode.clock(),
                    dimensions.0,
                    mode.hsync().0,
                    mode.hsync().1,
                    mode.hsync().2,
                    dimensions.1,
                    mode.vsync().0,
                    mode.vsync().1,
                    mode.vsync().2,
                    mode.flags(),
                    frame_interval.as_secs_f64() * 1_000.0,
                    refresh_hz
                ),
            );
            logged_mode_info = true;
        }
        if last_dimensions != Some(dimensions) {
            let _ = event_tx.send(EventMsg::InputEvent(InputEvent::Resized {
                width: dimensions.0,
                height: dimensions.1,
                scale_factor: 1.0,
            }));
            last_dimensions = Some(dimensions);
        }

        let mut cursor_plane = if config.hw_cursor {
            match create_cursor_plane(&card, &gbm_device, &resources, crtc_handle, &cursor_theme) {
                Ok(plane) => plane,
                Err(e) => {
                    native_log.warning("drm", format!("DRM cursor setup failed: {e}"));
                    None
                }
            }
        } else {
            None
        };
        if !logged_cursor_info {
            if config.hw_cursor {
                if cursor_plane.is_some() {
                    native_log.info("drm", "DRM cursor: hardware plane enabled");
                } else {
                    native_log.info("drm", "DRM cursor: hardware unavailable, using software");
                }
            } else {
                native_log.info("drm", "DRM cursor: hardware disabled, using software");
            }
            logged_cursor_info = true;
        }

        let gbm_surface: Surface<()> = match gbm_device.create_surface(
            dimensions.0,
            dimensions.1,
            GbmFormat::Xrgb8888,
            BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
        ) {
            Ok(surface) => surface,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating GBM surface failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (egl_lib, egl_api) = match load_egl() {
            Ok(values) => values,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("loading EGL failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let (display, context, surface, egl_diagnostics) = match init_egl(
            &egl_api,
            gbm_device.as_raw() as *mut c_void,
            gbm_surface.as_raw() as *mut c_void,
        ) {
            Ok(values) => values,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("initializing EGL failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let egl_state = EglState {
            egl: egl_api,
            _egl_lib: egl_lib,
            display,
            _context: context,
            surface,
        };
        native_log.info(
            "drm",
            format!(
                "EGL: vendor={} version={} APIs={} visual={:#010x} swap_range={:?}..{:?} surface={:?}x{:?} extensions={{native_fence={}, fence={}, wait={}, buffer_age={}}}",
                egl_diagnostics.vendor,
                egl_diagnostics.version,
                egl_diagnostics.client_apis,
                egl_diagnostics.native_visual_id.unwrap_or_default(),
                egl_diagnostics.min_swap_interval,
                egl_diagnostics.max_swap_interval,
                egl_diagnostics.surface_size.0,
                egl_diagnostics.surface_size.1,
                egl_diagnostics.native_fence_sync,
                egl_diagnostics.fence_sync,
                egl_diagnostics.wait_sync,
                egl_diagnostics.buffer_age,
            ),
        );
        native_log.info(
            "drm",
            "EGL swap interval=0; atomic KMS page flips own vblank pacing",
        );
        if egl_diagnostics.native_fence_sync
            && plane_props.contains_key("IN_FENCE_FD")
            && crtc_props.contains_key("OUT_FENCE_PTR")
        {
            native_log.warning(
                "drm",
                "explicit GPU/KMS fencing is available but not yet enabled; atomic commits currently rely on implicit GBM/DRM synchronization",
            );
        }

        let mut frame_surface =
            match create_renderer_frame_surface(config.rendering_api, &egl_state.egl, dimensions) {
                Ok(frame_surface) => frame_surface,
                Err(err) => {
                    if handle_startup_failure_with_card(
                        &card,
                        &mut startup_tx,
                        &running_flag,
                        &stop,
                        &mut startup_retries_remaining,
                        retry_interval,
                        format!("creating renderer failed: {err}"),
                    ) {
                        break;
                    }

                    continue;
                }
            };
        native_log.info(
            "drm",
            format!(
                "GL: vendor={} renderer={} version={} shading_language={}",
                gl_string(gl::VENDOR),
                gl_string(gl::RENDERER),
                gl_string(gl::VERSION),
                gl_string(gl::SHADING_LANGUAGE_VERSION),
            ),
        );
        let mut gpu_queue_timer = GpuQueueTimer::new(
            &egl_state.egl,
            stats.is_some() || config.renderer_stats_log,
            config.renderer_stats_log,
            &native_log,
        );
        let mut renderer = SceneRenderer::with_cache_config(config.renderer_cache_config);
        let mut raster_renderer = if matches!(config.rendering_api, RenderingApi::Raster) {
            match RasterBackend::with_cache_config(
                &RasterConfig {
                    width: dimensions.0,
                    height: dimensions.1,
                },
                config.renderer_cache_config,
            ) {
                Ok(renderer) => Some(renderer),
                Err(err) => {
                    if handle_startup_failure_with_card(
                        &card,
                        &mut startup_tx,
                        &running_flag,
                        &stop,
                        &mut startup_retries_remaining,
                        retry_interval,
                        format!("creating raster renderer failed: {err}"),
                    ) {
                        break;
                    }

                    continue;
                }
            }
        } else {
            None
        };
        let video_import = if matches!(config.rendering_api, RenderingApi::OpenGl) {
            match VideoImportContext::new_current_direct() {
                Ok(ctx) => {
                    native_log.info(
                        "video",
                        "Prime video import context initialized (direct external composition)",
                    );
                    Some(ctx)
                }
                Err(err) => {
                    native_log.error("video", format!("prime video import unavailable: {err}"));
                    None
                }
            }
        } else {
            None
        };
        let mut last_video_sync_error = None;
        let mut logged_video_import = false;

        let mode_blob = match card.create_property_blob(&mode) {
            Ok(blob) => blob,
            Err(err) => {
                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating mode blob failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };
        let mode_blob_id = mode_blob_id(&mode_blob);

        let mut framebuffer_cache: HashMap<u32, framebuffer::Handle> = HashMap::new();

        let mut render_state = RenderState::default();
        {
            let mut frame = frame_surface.frame();
            renderer.render(&mut frame, &render_state);
        }

        if unsafe {
            egl_state
                .egl
                .SwapBuffers(egl_state.display, egl_state.surface)
        } == egl::FALSE
        {
            destroy_session_resources(
                &card,
                cursor_plane.take(),
                &mut framebuffer_cache,
                mode_blob_id,
            );

            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                "eglSwapBuffers failed".to_string(),
            ) {
                break;
            }

            continue;
        }

        let bo = match unsafe { gbm_surface.lock_front_buffer() } {
            Ok(bo) => bo,
            Err(err) => {
                destroy_session_resources(
                    &card,
                    cursor_plane.take(),
                    &mut framebuffer_cache,
                    mode_blob_id,
                );

                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("locking first GBM buffer failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        native_log.info(
            "drm",
            format!("GBM primary BO: {}", gbm_bo_diagnostics(&bo)),
        );
        if !matches!(bo.modifier(), GbmModifier::Invalid | GbmModifier::Linear) {
            native_log.info(
                "drm",
                "GBM selected a tiled/compressed modifier; Emerge will require modifier-aware AddFB2 for scanout",
            );
        }

        let fb = match framebuffer_for_bo(&card, &mut framebuffer_cache, &bo, &native_log) {
            Ok(fb) => fb,
            Err(err) => {
                destroy_session_resources(
                    &card,
                    cursor_plane.take(),
                    &mut framebuffer_cache,
                    mode_blob_id,
                );

                if handle_startup_failure_with_card(
                    &card,
                    &mut startup_tx,
                    &running_flag,
                    &stop,
                    &mut startup_retries_remaining,
                    retry_interval,
                    format!("creating framebuffer failed: {err}"),
                ) {
                    break;
                }

                continue;
            }
        };

        let mut atomic_req = atomic::AtomicModeReq::new();
        if let Err(e) = (|| -> Result<(), String> {
            atomic_req.add_property(
                connector,
                prop_handle(&con_props, "CRTC_ID")?,
                property::Value::CRTC(Some(crtc_handle)),
            );
            atomic_req.add_property(crtc_handle, prop_handle(&crtc_props, "MODE_ID")?, mode_blob);
            atomic_req.add_property(
                crtc_handle,
                prop_handle(&crtc_props, "ACTIVE")?,
                property::Value::Boolean(true),
            );
            add_plane_properties(&mut atomic_req, plane, &plane_props, crtc_handle, fb)?;
            add_plane_geometry(&mut atomic_req, plane, &plane_props, &mode)
        })() {
            drop(bo);
            destroy_session_resources(
                &card,
                cursor_plane.take(),
                &mut framebuffer_cache,
                mode_blob_id,
            );

            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("preparing initial atomic commit failed: {e}"),
            ) {
                break;
            }

            continue;
        }

        if let Err(err) = card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, atomic_req) {
            drop(bo);
            destroy_session_resources(
                &card,
                cursor_plane.take(),
                &mut framebuffer_cache,
                mode_blob_id,
            );

            if handle_startup_failure_with_card(
                &card,
                &mut startup_tx,
                &running_flag,
                &stop,
                &mut startup_retries_remaining,
                retry_interval,
                format!("initial atomic commit failed: {err}"),
            ) {
                break;
            }

            continue;
        }

        if let Some(startup_tx) = startup_tx.take() {
            let _ = startup_tx.send(Ok(()));
        }

        if log_render {
            eprintln!("drm present version={}", render_state.render_version);
        }

        let mut current_primary = CurrentPrimaryFrame {
            generation: 0,
            render_version: render_state.render_version,
            bo,
            fb,
        };
        let mut prepared_primary: Option<PreparedPrimaryFrame> = None;
        let mut in_flight: Option<InFlightCommit> = None;
        let mut desired_primary_generation = 1u64;
        let mut committed_primary_generation = 0u64;
        let mut next_render_profile_at = Instant::now();
        let mut cursor_snapshot = cursor_state.snapshot();
        let mut cursor_pos = cursor_snapshot.state.pos;
        let mut cursor_visible = cursor_snapshot.state.visible;
        let mut current_cursor_icon = CursorIcon::Default;
        let mut last_cursor_pos = cursor_pos;
        let mut last_cursor_visible = cursor_visible;
        let mut last_cursor_icon = current_cursor_icon;
        let mut hw_cursor_enabled = cursor_plane.is_some();
        let mut committed_cursor_version: Option<u64> = None;
        let mut committed_cursor_visible = false;
        let mut committed_cursor_icon: Option<CursorIcon> = None;
        let mut present_state = DrmPresentState::new(frame_interval);
        let monotonic_page_flip_timestamps = card
            .get_driver_capability(DriverCapability::MonotonicTimestamp)
            .unwrap_or_default()
            != 0;
        let mut last_kernel_page_flip_at: Option<Duration> = None;
        let mut last_page_flip_sequence: Option<u32> = None;
        let mut follow_up_primary_until: Option<Instant> = None;
        let mut retry_commit_at: Option<Instant> = None;
        let mut drm_ready = false;
        let mut presenter_wake_ready = false;

        let mut next_hotplug_check = Instant::now() + hotplug_interval;
        let mut last_video_generation = video_registry.generation();
        // Video submission and fence cleanup mutate renderer-owned resources without changing the
        // scene fingerprint. Keep this sticky until sync_video_frames() has run so the generic
        // unchanged-frame optimization cannot consume the generation and strand a pending frame.
        let mut video_sync_required = true;
        let mut stop_requested = false;

        loop {
            if presenter_wake_ready {
                let _ = presenter_wake.drain();
                presenter_wake_ready = false;
            }

            if drm_ready {
                drm_ready = false;
                match card.receive_events() {
                    Ok(events) => {
                        for event in events {
                            if let control::Event::PageFlip(page_flip) = event {
                                if page_flip.crtc != crtc_handle {
                                    continue;
                                }

                                let dispatch_monotonic_at = monotonic_now();
                                let kernel_page_flip_at = monotonic_page_flip_timestamps
                                    .then_some(page_flip.duration)
                                    .filter(|timestamp| !timestamp.is_zero());
                                let sequence_delta = last_page_flip_sequence
                                    .map(|last| page_flip.frame.wrapping_sub(last))
                                    .filter(|delta| (1..=1_000).contains(delta));
                                if let Some(stats) = stats.as_deref() {
                                    stats.record_drm_page_flip_sequence(sequence_delta);
                                    if let (Some(previous), Some(current)) =
                                        (last_kernel_page_flip_at, kernel_page_flip_at)
                                        && let Some(duration) = duration_since(current, previous)
                                    {
                                        stats.record_drm_kernel_page_flip_interval(duration);
                                    }
                                    if let (Some(dispatched), Some(kernel)) =
                                        (dispatch_monotonic_at, kernel_page_flip_at)
                                        && let Some(duration) = duration_since(dispatched, kernel)
                                    {
                                        stats.record_drm_page_flip_dispatch_delay(duration);
                                    }
                                }
                                last_page_flip_sequence = Some(page_flip.frame);
                                if let Some(timestamp) = kernel_page_flip_at {
                                    last_kernel_page_flip_at = Some(timestamp);
                                }

                                if let Some(submitted) = in_flight.take() {
                                    if let Some(frame) = submitted.primary {
                                        let old_primary = std::mem::replace(
                                            &mut current_primary,
                                            CurrentPrimaryFrame {
                                                generation: frame.generation,
                                                render_version: frame.render_version,
                                                bo: frame.bo,
                                                fb: frame.fb,
                                            },
                                        );
                                        drop(old_primary.bo);
                                        committed_primary_generation = current_primary.generation;
                                        if should_schedule_video_cleanup(
                                            frame.video_needs_cleanup,
                                            prepared_primary
                                                .as_ref()
                                                .is_some_and(|frame| frame.video_sync_succeeded),
                                        ) {
                                            desired_primary_generation =
                                                desired_primary_generation.wrapping_add(1);
                                            video_sync_required = true;
                                        }
                                        if log_render {
                                            eprintln!(
                                                "drm present version={}",
                                                current_primary.render_version
                                            );
                                        }

                                        let presented_at = Instant::now();
                                        if let Some(stats) = stats.as_ref() {
                                            stats.record_frame_present();
                                            if let (Some(committed), Some(kernel)) = (
                                                frame.atomic_commit_monotonic_at,
                                                kernel_page_flip_at,
                                            ) && let Some(duration) =
                                                duration_since(kernel, committed)
                                            {
                                                stats.record_drm_commit_to_kernel_page_flip(
                                                    duration,
                                                );
                                            }
                                            if let Some(committed_at) =
                                                frame.atomic_commit_submitted_at
                                            {
                                                stats.record_drm_primary_presented(
                                                    frame.imported_video_frames > 0,
                                                    presented_at
                                                        .saturating_duration_since(committed_at),
                                                    frame.newest_video_submitted_at.map(
                                                        |submitted_at| {
                                                            presented_at.saturating_duration_since(
                                                                submitted_at,
                                                            )
                                                        },
                                                    ),
                                                );
                                            }
                                            record_drm_pipeline_presented(
                                                Some(stats),
                                                frame.pipeline_submitted_at,
                                                frame.pipeline_swap_done_at,
                                                presented_at,
                                            );
                                        }

                                        let predicted_next_present_at =
                                            present_state.observe_present(presented_at);

                                        record_drm_display_interval(
                                            stats.as_deref(),
                                            present_state.mode_frame_interval(),
                                        );

                                        send_present_timing(
                                            &event_tx,
                                            presented_at,
                                            predicted_next_present_at,
                                        );

                                        if submitted.emit_animation_pulse {
                                            if !send_animation_pulse(
                                                &tree_tx,
                                                presented_at,
                                                predicted_next_present_at,
                                                log_render,
                                            ) {
                                                stop_requested = true;
                                                break;
                                            }
                                            follow_up_primary_until =
                                                Some(presented_at + FOLLOW_UP_PRIMARY_WINDOW);
                                        }
                                    }

                                    if let Some(cursor) = submitted.cursor {
                                        committed_cursor_version = cursor.version;
                                        committed_cursor_visible = cursor.visible;
                                        committed_cursor_icon = Some(cursor.icon);
                                    }
                                }
                            }
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                    Err(err) => {
                        eprintln!(
                            "DRM backend unavailable: failed to receive page flip events: {err}"
                        );
                        break;
                    }
                }
            }

            if stop.load(Ordering::Relaxed) {
                stop_requested = true;
                break;
            }

            let now = Instant::now();
            if now >= next_hotplug_check {
                let resources = match card.resource_handles() {
                    Ok(handles) => handles,
                    Err(_) => break,
                };
                let next = first_connected_connector(&card, &resources, config.requested_size);
                match next {
                    Ok((next_connector, next_mode, next_crtc, _next_encoder)) => {
                        let next_dimensions = next_mode.size();
                        let next_dimensions = (next_dimensions.0 as u32, next_dimensions.1 as u32);
                        let next_frame_interval = mode_frame_interval(&next_mode);
                        if next_connector != connector
                            || next_crtc != crtc_handle
                            || drm_session_mode_changed(
                                dimensions,
                                next_dimensions,
                                frame_interval,
                                next_frame_interval,
                            )
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
                next_hotplug_check = Instant::now() + hotplug_interval;
            }

            while let Ok(msg) = render_rx.try_recv() {
                match msg {
                    RenderMsg::Scene {
                        scene,
                        version,
                        pipeline_submitted_at,
                        pipeline_render_queued_at,
                        animate,
                        ..
                    } => {
                        let received_at = Instant::now();
                        let scene = *scene;
                        render_state.set_scene(scene);
                        if let Err(error) =
                            video_registry.set_active_targets(&render_state.video_target_ids)
                        {
                            eprintln!("video target visibility update failed: {error}");
                        }
                        render_state.render_version = version;
                        render_state.pipeline_submitted_at = pipeline_submitted_at;
                        render_state.pipeline_render_queued_at = pipeline_render_queued_at;
                        render_state.animate = animate;
                        record_drm_pipeline_scene_received(
                            stats.as_deref(),
                            pipeline_render_queued_at,
                            received_at,
                        );
                        desired_primary_generation = desired_primary_generation.wrapping_add(1);
                        follow_up_primary_until = None;
                        if log_render {
                            let latest = render_counter.load(Ordering::Relaxed);
                            let delta = latest.saturating_sub(version);
                            eprintln!("drm render version={version} latest={latest} delta={delta}");
                        }
                    }
                    RenderMsg::Stop => {
                        stop_requested = true;
                        break;
                    }
                }
            }

            if stop_requested {
                break;
            }

            if let Some(retry_at) = retry_commit_at
                && Instant::now() >= retry_at
            {
                retry_commit_at = None;
            }
            if let Some(deadline) = follow_up_primary_until
                && Instant::now() >= deadline
            {
                follow_up_primary_until = None;
            }
            while let Ok(icon) = cursor_icon_rx.try_recv() {
                current_cursor_icon = icon;
            }

            cursor_snapshot = cursor_state.snapshot();
            cursor_pos = cursor_snapshot.state.pos;
            cursor_visible = cursor_snapshot.state.visible;

            if !hw_cursor_enabled {
                if cursor_visible && cursor_pos != last_cursor_pos {
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                }
                if cursor_visible != last_cursor_visible {
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                }
                if cursor_visible && current_cursor_icon != last_cursor_icon {
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                }
            }

            let video_generation = video_registry.generation();
            if video_generation != last_video_generation {
                desired_primary_generation = desired_primary_generation.wrapping_add(1);
                last_video_generation = video_generation;
                video_sync_required = true;
            }

            last_cursor_pos = cursor_pos;
            last_cursor_visible = cursor_visible;
            last_cursor_icon = current_cursor_icon;

            let primary_dirty = desired_primary_generation != committed_primary_generation;
            // Animation pulses are driven by primary page-flip completions. Even
            // when an enter frame is initially visually unchanged (for example
            // translated outside a clip), it still needs a committed primary so
            // the next pulse advances the animation clock.
            if should_consider_unchanged_primary_skip(
                in_flight.is_some(),
                primary_dirty,
                video_sync_required,
                hw_cursor_enabled,
                cursor_visible,
                render_state.animate,
            ) && renderer.can_skip_unchanged_visible_frame(&render_state, dimensions)
            {
                committed_primary_generation = desired_primary_generation;
                // A frame prepared behind the previous commit may prove visually redundant once
                // that commit lands. Release its GBM lock when consuming the generation as a noop.
                if let Some(stale) = prepared_primary.take()
                    && let Some(stats) = stats.as_deref()
                {
                    stats.record_drm_stale_prepared(stale.imported_video_frames > 0);
                }
                render_state.pipeline_submitted_at = None;
                render_state.pipeline_render_queued_at = None;
            }
            let in_flight_primary_generation = in_flight
                .as_ref()
                .and_then(|commit| commit.primary.as_ref().map(|frame| frame.generation));
            let prepared_primary_generation =
                prepared_primary.as_ref().map(|frame| frame.generation);

            // Render at most one staged primary while the previous atomic commit waits for
            // vblank. New camera submissions remain in the registry's latest-frame slot until
            // that staged frame is committed; replacing it here would submit GPU work for frames
            // that can never reach scanout and can delay the in-flight frame's implicit fence.
            if should_prepare_primary(
                desired_primary_generation,
                committed_primary_generation,
                in_flight_primary_generation,
                prepared_primary_generation,
            ) {
                if gbm_surface.has_free_buffers() {
                    let now = Instant::now();
                    let sampled_diagnostics_due = (stats.is_some() || config.renderer_stats_log)
                        && now >= next_render_profile_at;
                    if sampled_diagnostics_due {
                        next_render_profile_at = now + RENDER_PROFILE_INTERVAL;
                    }
                    let profile_render = config.renderer_stats_log && sampled_diagnostics_due;
                    match prepare_primary_frame(
                        desired_primary_generation,
                        config.rendering_api,
                        &mut renderer,
                        raster_renderer.as_mut(),
                        &mut frame_surface,
                        dimensions,
                        &render_state,
                        cursor_pos,
                        cursor_visible,
                        hw_cursor_enabled,
                        current_cursor_icon,
                        &cursor_theme,
                        &video_registry,
                        video_import.as_ref(),
                        &egl_state,
                        &gbm_surface,
                        config.force_gpu_finish,
                        &card,
                        &mut framebuffer_cache,
                        stats.as_deref(),
                        profile_render,
                        sampled_diagnostics_due,
                        &mut gpu_queue_timer,
                        &native_log,
                        &mut last_video_sync_error,
                        &mut logged_video_import,
                        &latest_frame,
                    ) {
                        Ok(frame) => {
                            // Failed syncs and pending retired-import fences must survive stale
                            // prepared-frame replacement and unchanged-frame checks.
                            video_sync_required =
                                !frame.video_sync_succeeded || frame.video_needs_cleanup;
                            if let Some(stats) = stats.as_deref() {
                                stats.record_drm_primary_prepared(frame.imported_video_frames > 0);
                            }
                            prepared_primary = Some(frame);
                        }
                        Err(err) => {
                            eprintln!("DRM backend unavailable: {err}");
                            break;
                        }
                    }
                } else if let Some(stats) = stats.as_deref() {
                    stats.record_drm_gbm_no_free_buffer();
                }
            }

            let submit_primary = should_submit_prepared_primary(
                in_flight.is_some(),
                committed_primary_generation,
                prepared_primary.as_ref().map(|frame| frame.generation),
            );
            let submit_cursor = cursor_plane.is_some()
                && in_flight.is_none()
                && ((hw_cursor_enabled
                    && (committed_cursor_version != Some(cursor_snapshot.version)
                        || committed_cursor_icon != Some(current_cursor_icon)))
                    || (!hw_cursor_enabled && committed_cursor_visible));
            let now = Instant::now();
            let defer_cursor_only = should_defer_cursor_only_commit(
                submit_primary,
                submit_cursor,
                follow_up_primary_until,
                now,
            );
            if defer_cursor_only && log_render {
                eprintln!("drm defer cursor-only commit waiting for follow-up primary");
            }

            if submit_primary || (submit_cursor && !defer_cursor_only) {
                let present_submit_started_at = submit_primary.then(Instant::now);
                let mut commit_req = atomic::AtomicModeReq::new();
                let primary_fb = prepared_primary
                    .as_ref()
                    .filter(|_| submit_primary)
                    .map(|frame| frame.fb)
                    .unwrap_or(current_primary.fb);
                if let Err(err) = add_plane_properties(
                    &mut commit_req,
                    plane,
                    &plane_props,
                    crtc_handle,
                    primary_fb,
                ) {
                    eprintln!("DRM backend unavailable: {err}");
                    break;
                }

                let cursor_visual = cursor_theme.cursor(current_cursor_icon);

                if submit_cursor
                    && hw_cursor_enabled
                    && let Some(cursor_plane) = cursor_plane.as_mut()
                    && let Err(err) = cursor_plane.write_visual(cursor_visual)
                {
                    native_log.error(
                        "drm",
                        format!("DRM cursor setup failed during cursor upload: {err}"),
                    );
                    hw_cursor_enabled = false;
                    committed_cursor_visible = false;
                    committed_cursor_icon = None;
                    desired_primary_generation = desired_primary_generation.wrapping_add(1);
                    continue;
                }

                if submit_cursor && let Some(plane) = cursor_plane.as_ref().map(CursorPlane::commit)
                {
                    let cursor_for_commit = if hw_cursor_enabled {
                        cursor_snapshot.state
                    } else {
                        CursorState {
                            pos: cursor_pos,
                            visible: false,
                        }
                    };

                    if let Err(err) = add_cursor_plane_properties(
                        &mut commit_req,
                        crtc_handle,
                        plane,
                        cursor_for_commit,
                        cursor_visual,
                        dimensions,
                    ) {
                        if hw_cursor_enabled {
                            native_log.error(
                                "drm",
                                format!("DRM cursor setup failed during commit build: {err}"),
                            );
                            hw_cursor_enabled = false;
                            committed_cursor_visible = false;
                            committed_cursor_icon = None;
                            desired_primary_generation = desired_primary_generation.wrapping_add(1);
                            continue;
                        }
                        eprintln!("DRM backend unavailable: {err}");
                        break;
                    }
                }

                if submit_primary && let Some(stats) = stats.as_deref() {
                    stats.record_drm_primary_commit_attempt();
                }

                let atomic_commit_started_at = Instant::now();
                let atomic_commit_monotonic_at = monotonic_now();
                let atomic_commit_result = card.atomic_commit(
                    AtomicCommitFlags::NONBLOCK | AtomicCommitFlags::PAGE_FLIP_EVENT,
                    commit_req,
                );
                if submit_primary && let Some(stats) = stats.as_deref() {
                    stats.record_drm_atomic_commit_ioctl(atomic_commit_started_at.elapsed());
                }
                match atomic_commit_result {
                    Ok(()) => {
                        let swap_done_at = Instant::now();
                        if submit_primary && let Some(frame) = prepared_primary.as_mut() {
                            frame.atomic_commit_submitted_at = Some(atomic_commit_started_at);
                            frame.atomic_commit_monotonic_at = atomic_commit_monotonic_at;
                            if let Some(stats) = stats.as_deref() {
                                stats.record_drm_primary_committed();
                                stats.record_drm_prepared_to_commit(
                                    atomic_commit_started_at
                                        .saturating_duration_since(frame.prepared_at),
                                );
                                if let (Some(commit), Some(previous_flip)) =
                                    (atomic_commit_monotonic_at, last_kernel_page_flip_at)
                                    && let Some(duration) = duration_since(commit, previous_flip)
                                {
                                    stats.record_drm_previous_flip_to_commit(duration);
                                }
                            }
                        }
                        if let Some(stats) = stats.as_ref()
                            && let (Some(present_submit_started_at), Some(frame)) = (
                                present_submit_started_at,
                                prepared_primary.as_ref().filter(|_| submit_primary),
                            )
                        {
                            stats.record_present_submit(
                                frame.present_submit_duration
                                    + swap_done_at
                                        .saturating_duration_since(present_submit_started_at),
                            );
                        }

                        retry_commit_at = None;
                        let submitted_primary = if submit_primary {
                            let mut frame = prepared_primary.take();
                            if let Some(frame) = frame.as_mut()
                                && frame.pipeline_submitted_at.is_some()
                            {
                                record_drm_pipeline_swap_done(
                                    stats.as_deref(),
                                    frame.pipeline_submitted_at,
                                    swap_done_at,
                                );
                                frame.pipeline_swap_done_at = Some(swap_done_at);
                                render_state.pipeline_submitted_at = None;
                                render_state.pipeline_render_queued_at = None;
                            }
                            frame
                        } else {
                            None
                        };
                        in_flight = Some(InFlightCommit {
                            primary: submitted_primary,
                            cursor: if submit_cursor {
                                Some(SubmittedCursorState {
                                    version: if hw_cursor_enabled {
                                        Some(cursor_snapshot.version)
                                    } else {
                                        None
                                    },
                                    visible: hw_cursor_enabled && cursor_snapshot.state.visible,
                                    icon: current_cursor_icon,
                                })
                            } else {
                                None
                            },
                            emit_animation_pulse: submit_primary && render_state.animate,
                        });
                    }
                    Err(err) => {
                        let err = err.to_string();
                        if is_ebusy(&err) {
                            if submit_primary && let Some(stats) = stats.as_deref() {
                                stats.record_drm_primary_commit_ebusy();
                            }
                            if log_render {
                                eprintln!("drm atomic commit EBUSY, retrying staged state");
                            }
                            retry_commit_at = Some(Instant::now() + Duration::from_millis(1));
                            continue;
                        }

                        if submit_cursor && hw_cursor_enabled {
                            native_log.error(
                                "drm",
                                format!(
                                    "DRM cursor commit failed: {err}; falling back to software cursor"
                                ),
                            );

                            if let Some(cursor_plane_commit) =
                                cursor_plane.as_ref().map(CursorPlane::commit)
                            {
                                let mut hide_req = atomic::AtomicModeReq::new();
                                let _ = add_cursor_plane_properties(
                                    &mut hide_req,
                                    crtc_handle,
                                    cursor_plane_commit,
                                    CursorState {
                                        pos: cursor_pos,
                                        visible: false,
                                    },
                                    cursor_theme.cursor(current_cursor_icon),
                                    dimensions,
                                );
                                let _ = add_plane_properties(
                                    &mut hide_req,
                                    plane,
                                    &plane_props,
                                    crtc_handle,
                                    current_primary.fb,
                                );
                                let _ = card.atomic_commit(AtomicCommitFlags::empty(), hide_req);
                            }

                            hw_cursor_enabled = false;
                            committed_cursor_visible = false;
                            committed_cursor_icon = None;
                            desired_primary_generation = desired_primary_generation.wrapping_add(1);
                            continue;
                        }

                        eprintln!("DRM backend unavailable: {err}");
                        break;
                    }
                }
            }

            let mut next_deadline = Some(next_hotplug_check);
            if let Some(retry_at) = retry_commit_at {
                next_deadline = Some(
                    next_deadline
                        .map(|deadline| deadline.min(retry_at))
                        .unwrap_or(retry_at),
                );
            }
            if let Some(deadline) = follow_up_primary_until {
                next_deadline = Some(
                    next_deadline
                        .map(|current_deadline| current_deadline.min(deadline))
                        .unwrap_or(deadline),
                );
            }
            if in_flight.is_none()
                && (submit_primary || (submit_cursor && !defer_cursor_only))
                && retry_commit_at.is_none()
            {
                continue;
            }

            let timeout =
                next_deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
            let mut pollfds = [
                libc::pollfd {
                    fd: card.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: presenter_wake.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];

            match poll_fds(&mut pollfds, timeout) {
                Ok(_) => {
                    drm_ready = (pollfds[0].revents & (libc::POLLIN | libc::POLLPRI)) != 0;
                    presenter_wake_ready = (pollfds[1].revents & libc::POLLIN) != 0;
                    if (pollfds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0
                    {
                        eprintln!("DRM backend unavailable: poll reported DRM fd error");
                        break;
                    }
                }
                Err(err) => {
                    eprintln!("DRM backend unavailable: poll failed: {err}");
                    break;
                }
            }
        }

        // Keep prepared and in-flight GBM buffers alive until teardown completes.
        // A blocking ALLOW_MODESET teardown is our final barrier before those
        // buffers can be released safely.
        cleanup_active_session(
            &card,
            connector,
            crtc_handle,
            plane,
            &con_props,
            &crtc_props,
            &plane_props,
            cursor_plane.take(),
            &mut framebuffer_cache,
            mode_blob_id,
        );

        if stop_requested {
            running_flag.store(false, Ordering::Relaxed);
            break;
        }
    }
}
