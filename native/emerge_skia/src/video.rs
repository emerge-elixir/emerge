use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::{c_void, CString};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::raw::c_char;
use std::ptr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::thread;

use crossbeam_channel::{unbounded, Sender};
use glutin_egl_sys::egl;
use libloading::Library;
use rustler::env::SavedTerm;
use rustler::{Decoder, Encoder, Env, LocalPid, NifResult, OwnedEnv, Term};
use skia_safe::{
    gpu::{self, gl::TextureInfo, Mipmapped, Protected, SurfaceOrigin},
    AlphaType, ColorType, Image,
};

use crate::backend::wake::BackendWakeHandle;

rustler::atoms! {
    keepalive,
}

const DRM_FORMAT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');

const EGL_LINUX_DMA_BUF_EXT: egl::types::EGLenum = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: egl::types::EGLint = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: egl::types::EGLint = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: egl::types::EGLint = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: egl::types::EGLint = 0x3274;
const EGL_DMA_BUF_PLANE1_FD_EXT: egl::types::EGLint = 0x3275;
const EGL_DMA_BUF_PLANE1_OFFSET_EXT: egl::types::EGLint = 0x3276;
const EGL_DMA_BUF_PLANE1_PITCH_EXT: egl::types::EGLint = 0x3277;
const EGL_DMA_BUF_PLANE2_FD_EXT: egl::types::EGLint = 0x3278;
const EGL_DMA_BUF_PLANE2_OFFSET_EXT: egl::types::EGLint = 0x3279;
const EGL_DMA_BUF_PLANE2_PITCH_EXT: egl::types::EGLint = 0x327A;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: egl::types::EGLint = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: egl::types::EGLint = 0x3444;
const EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT: egl::types::EGLint = 0x3445;
const EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT: egl::types::EGLint = 0x3446;
const EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT: egl::types::EGLint = 0x3447;
const EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT: egl::types::EGLint = 0x3448;

const GL_TEXTURE_EXTERNAL_OES: u32 = 0x8D65;

const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

pub struct FrozenTerm {
    env: Option<OwnedEnv>,
    saved: Option<SavedTerm>,
}

impl FrozenTerm {
    pub fn send_once_with<F>(&mut self, pid: &LocalPid, make_msg: F)
    where
        F: for<'a> FnOnce(Env<'a>, Term<'a>) -> Term<'a>,
    {
        if let (Some(mut env), Some(saved)) = (self.env.take(), self.saved.take()) {
            let _ = env.send_and_clear(pid, move |send_env| -> Term<'_> {
                let payload = saved.load(send_env);
                make_msg(send_env, payload)
            });
        }
    }
}

impl<'a> Decoder<'a> for FrozenTerm {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        let env = OwnedEnv::new();
        let saved = env.save(term);
        Ok(Self {
            env: Some(env),
            saved: Some(saved),
        })
    }
}

impl Encoder for FrozenTerm {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        self.saved
            .as_ref()
            .expect("frozen term already consumed")
            .load(env)
    }
}

struct Fd(OwnedFd);

impl<'a> Decoder<'a> for Fd {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        let fd: i32 = term.decode()?;
        if fd < 0 {
            return Err(rustler::Error::BadArg);
        }

        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }))
    }
}

impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> i32 {
        self.0.as_raw_fd()
    }
}

impl Encoder for Fd {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        let dup_fd = unsafe { libc::dup(self.0.as_raw_fd()) };
        dup_fd.encode(env)
    }
}

#[derive(Clone, Debug, rustler::NifStruct)]
#[module = "Membrane.DRM.Instrumentation.TraceToken"]
struct TraceToken {
    trace_id: u64,
    frame_id: u64,
    created_at_ns: u64,
    sampled: bool,
    pts: Option<i64>,
}

#[derive(Debug, rustler::NifStruct)]
#[module = "Membrane.PrimePlane"]
struct PrimePlane {
    obj_idx: u32,
    pitch: u32,
    offset: u32,
}

struct Fourcc(u32);

impl<'a> Decoder<'a> for Fourcc {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        Ok(Self(term.decode()?))
    }
}

impl Encoder for Fourcc {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        self.0.encode(env)
    }
}

#[derive(rustler::NifStruct)]
#[module = "Membrane.PrimeObject"]
struct PrimeObject {
    fd: Fd,
    modifier: Option<u64>,
}

#[derive(rustler::NifStruct)]
#[module = "Membrane.PrimeDesc"]
pub struct PrimeDesc {
    width: u32,
    height: u32,
    format: Fourcc,
    objects: Vec<PrimeObject>,
    planes: Vec<PrimePlane>,
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
    trace_token: Option<TraceToken>,
}

struct PrimeObjectOwned {
    fd: OwnedFd,
    modifier: Option<u64>,
}

#[derive(Clone)]
struct PrimePlaneDesc {
    obj_idx: u32,
    pitch: u32,
    offset: u32,
}

pub struct PrimeFrame {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    objects: Vec<PrimeObjectOwned>,
    planes: Vec<PrimePlaneDesc>,
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
}

impl PrimeFrame {
    fn object(&self, index: usize) -> Result<&PrimeObjectOwned, String> {
        self.objects
            .get(index)
            .ok_or_else(|| format!("prime object index out of range: {index}"))
    }

    fn plane(&self, index: usize) -> Result<&PrimePlaneDesc, String> {
        self.planes
            .get(index)
            .ok_or_else(|| format!("prime plane index out of range: {index}"))
    }
}

impl From<PrimeDesc> for PrimeFrame {
    fn from(desc: PrimeDesc) -> Self {
        Self {
            width: desc.width,
            height: desc.height,
            format: desc.format.0,
            objects: desc
                .objects
                .into_iter()
                .map(|object| PrimeObjectOwned {
                    fd: object.fd.0,
                    modifier: object.modifier,
                })
                .collect(),
            planes: desc
                .planes
                .into_iter()
                .map(|plane| PrimePlaneDesc {
                    obj_idx: plane.obj_idx,
                    pitch: plane.pitch,
                    offset: plane.offset,
                })
                .collect(),
            keepalive: desc.keepalive,
            owner_pid: desc.owner_pid,
        }
    }
}

impl Drop for PrimeFrame {
    fn drop(&mut self) {
        self.keepalive
            .send_once_with(&self.owner_pid, |env, payload| {
                (keepalive(), payload).encode(env)
            });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoMode {
    Prime,
}

impl VideoMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "prime" => Ok(Self::Prime),
            other => Err(format!("unsupported video target mode: {other}")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct VideoTargetSpec {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub mode: VideoMode,
}

struct VideoTargetEntry {
    spec: VideoTargetSpec,
    pending: Option<PrimeFrame>,
}

pub struct VideoRegistry {
    state: Mutex<HashMap<String, VideoTargetEntry>>,
    release_tx: Sender<PrimeFrame>,
    generation: AtomicU64,
}

impl VideoRegistry {
    pub fn new(release_tx: Sender<PrimeFrame>) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            release_tx,
            generation: AtomicU64::new(0),
        }
    }

    pub fn create_target(&self, spec: VideoTargetSpec) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        if state.contains_key(&spec.id) {
            return Err(format!("video target already exists: {}", spec.id));
        }

        state.insert(
            spec.id.clone(),
            VideoTargetEntry {
                spec,
                pending: None,
            },
        );
        self.bump_generation();
        Ok(())
    }

    pub fn remove_target(&self, id: &str) {
        let pending = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.remove(id).and_then(|entry| entry.pending));
        self.bump_generation();
        if let Some(frame) = pending {
            self.defer_release(frame);
        }
    }

    pub fn submit_prime(&self, id: &str, frame: PrimeFrame) -> Result<(), String> {
        let (mode, target_width, target_height) = {
            let state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned")?;
            let entry = state
                .get(id)
                .ok_or_else(|| format!("unknown video target: {id}"))?;
            (entry.spec.mode, entry.spec.width, entry.spec.height)
        };

        if mode != VideoMode::Prime {
            self.defer_release(frame);
            return Err(format!("video target {id} is not a prime target"));
        }

        let frame_width = frame.width;
        let frame_height = frame.height;
        let frame_format = frame.format;

        if frame_width != target_width || frame_height != target_height {
            self.defer_release(frame);
            return Err(format!(
                "prime frame size {}x{} does not match target {}x{}",
                frame_width, frame_height, target_width, target_height
            ));
        }

        if frame_format != DRM_FORMAT_NV12 {
            self.defer_release(frame);
            return Err(format!(
                "unsupported DRM format {:#x}; only NV12 is supported in v1",
                frame_format
            ));
        }

        let previous = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned")?;
            let entry = state
                .get_mut(id)
                .ok_or_else(|| format!("unknown video target: {id}"))?;
            entry.pending.replace(frame)
        };

        if let Some(previous) = previous {
            self.defer_release(previous);
        }

        self.bump_generation();

        Ok(())
    }

    #[cfg(feature = "drm")]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot_pending(&self) -> Result<VideoRegistrySnapshot, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        let mut pending = Vec::new();

        for (id, entry) in state.iter_mut() {
            if let Some(frame) = entry.pending.take() {
                pending.push(PendingVideoFrame {
                    id: id.clone(),
                    spec: entry.spec.clone(),
                    frame,
                });
            }
        }

        Ok(VideoRegistrySnapshot { pending })
    }

    pub fn target_specs(&self) -> Result<HashMap<String, VideoTargetSpec>, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        Ok(state
            .iter()
            .map(|(id, entry)| (id.clone(), entry.spec.clone()))
            .collect())
    }

    #[cfg_attr(not(feature = "wayland"), allow(dead_code))]
    pub fn drain_pending_to_release(&self) -> Result<(), String> {
        let snapshot = self.snapshot_pending()?;

        for pending in snapshot.pending {
            self.defer_release(pending.frame);
        }

        Ok(())
    }

    pub fn defer_release(&self, frame: PrimeFrame) {
        if let Err(err) = self.release_tx.send(frame) {
            let frame = err.into_inner();
            let _ = thread::Builder::new()
                .name("emerge_skia_video_release".into())
                .spawn(move || drop(frame));
        }
    }
}

pub struct VideoRegistrySnapshot {
    pub pending: Vec<PendingVideoFrame>,
}

pub struct PendingVideoFrame {
    pub id: String,
    pub spec: VideoTargetSpec,
    pub frame: PrimeFrame,
}

#[derive(Clone)]
pub struct VideoWake(BackendWakeHandle);

impl VideoWake {
    #[cfg_attr(not(feature = "wayland"), allow(dead_code))]
    pub fn new(wake: BackendWakeHandle) -> Self {
        Self(wake)
    }

    pub fn noop() -> Self {
        Self(BackendWakeHandle::noop())
    }

    pub fn notify(&self) {
        self.0.notify_video_frame();
    }
}

pub struct VideoTargetResource {
    pub id: String,
    pub _width: u32,
    pub _height: u32,
    pub _mode: VideoMode,
    pub registry: Arc<VideoRegistry>,
    pub wake: VideoWake,
}

impl rustler::Resource for VideoTargetResource {}

impl Drop for VideoTargetResource {
    fn drop(&mut self) {
        self.registry.remove_target(&self.id);
        self.wake.notify();
    }
}

pub fn spawn_release_worker() -> Sender<PrimeFrame> {
    let (tx, rx) = unbounded();
    let _ = thread::Builder::new()
        .name("emerge_skia_video_release".into())
        .spawn(move || {
            while let Ok(frame) = rx.recv() {
                drop(frame);
            }
        });
    tx
}

type GlEglImageTargetTexture2DOes = unsafe extern "system" fn(u32, *const c_void);
type RawEglGetProcAddress =
    unsafe extern "system" fn(
        *const c_char,
    ) -> egl::types::__eglMustCastToProperFunctionPointerType;

pub struct VideoImportContext {
    support: EglDmabufSupport,
    blitter: ExternalVideoBlitter,
    use_gl_fences: bool,
}

impl VideoImportContext {
    pub fn new_current() -> Result<Self, String> {
        let support = EglDmabufSupport::new_current()?;
        let blitter = ExternalVideoBlitter::new()?;
        let use_gl_fences = gl::FenceSync::is_loaded()
            && gl::ClientWaitSync::is_loaded()
            && gl::DeleteSync::is_loaded();
        Ok(Self {
            support,
            blitter,
            use_gl_fences,
        })
    }
}

fn collect_gl_errors() -> Vec<u32> {
    let mut errors = Vec::new();
    loop {
        let err = unsafe { gl::GetError() };
        if err == gl::NO_ERROR {
            break;
        }
        errors.push(err);
    }
    errors
}

fn gl_step_check(step: &str) -> Result<(), String> {
    let errors = collect_gl_errors();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("OpenGL errors after {step}: {errors:?}"))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VideoSyncResult {
    pub resources_changed: bool,
    pub needs_cleanup: bool,
}

struct RetiredImport {
    sync: gl::types::GLsync,
    imported: ImportedExternalFrame,
}

enum RetiredImportPoll {
    Released,
    Pending,
}

enum RetiredImportPollError {
    WaitFailed,
    UnexpectedStatus(u32),
}

impl RetiredImport {
    fn poll(&self) -> Result<RetiredImportPoll, RetiredImportPollError> {
        let status = unsafe { gl::ClientWaitSync(self.sync, 0, 0) };
        match status {
            gl::ALREADY_SIGNALED | gl::CONDITION_SATISFIED => Ok(RetiredImportPoll::Released),
            gl::TIMEOUT_EXPIRED => Ok(RetiredImportPoll::Pending),
            gl::WAIT_FAILED => Err(RetiredImportPollError::WaitFailed),
            other => Err(RetiredImportPollError::UnexpectedStatus(other)),
        }
    }

    fn wait_blocking(self, target_id: &str) {
        unsafe {
            let status =
                gl::ClientWaitSync(self.sync, gl::SYNC_FLUSH_COMMANDS_BIT, gl::TIMEOUT_IGNORED);
            if status == gl::WAIT_FAILED {
                eprintln!(
                    "video sync failed: glClientWaitSync WAIT_FAILED during blocking cleanup for target={target_id}; forcing glFinish"
                );
                gl::Finish();
            }
            gl::DeleteSync(self.sync);
        }
        drop(self.imported);
    }
}

struct EglDmabufSupport {
    egl: egl::Egl,
    _lib: Library,
    display: egl::types::EGLDisplay,
    image_target_texture_2d_oes: GlEglImageTargetTexture2DOes,
}

impl EglDmabufSupport {
    fn new_current() -> Result<Self, String> {
        let lib = unsafe { Library::new("libEGL.so.1") }
            .map_err(|err| format!("failed to load libEGL.so.1: {err}"))?;
        let get_proc_address = unsafe {
            lib.get::<RawEglGetProcAddress>(b"eglGetProcAddress\0")
                .map(|symbol| *symbol)
                .map_err(|err| format!("failed to load eglGetProcAddress: {err}"))?
        };
        let egl = egl::Egl::load_with(|name| unsafe {
            let cname = CString::new(name).expect("EGL symbol");
            match lib.get::<*const c_void>(cname.as_bytes_with_nul()) {
                Ok(symbol) => *symbol,
                Err(_) => get_proc_address(cname.as_ptr()) as *const c_void,
            }
        });
        let display = unsafe { egl.GetCurrentDisplay() };
        if display == egl::NO_DISPLAY {
            return Err("eglGetCurrentDisplay returned NO_DISPLAY".to_string());
        }

        if !egl.CreateImageKHR.is_loaded() && !egl.CreateImage.is_loaded() {
            return Err("neither eglCreateImageKHR nor eglCreateImage is available".to_string());
        }

        if !egl.DestroyImageKHR.is_loaded() && !egl.DestroyImage.is_loaded() {
            return Err("neither eglDestroyImageKHR nor eglDestroyImage is available".to_string());
        }

        let func = unsafe {
            let symbol = CString::new("glEGLImageTargetTexture2DOES").expect("GL symbol");
            get_proc_address(symbol.as_ptr()) as *const c_void
        };

        if func.is_null() {
            return Err("glEGLImageTargetTexture2DOES is not available".to_string());
        }

        Ok(Self {
            egl,
            _lib: lib,
            display,
            image_target_texture_2d_oes: unsafe {
                std::mem::transmute::<
                    *const libc::c_void,
                    unsafe extern "system" fn(u32, *const libc::c_void),
                >(func)
            },
        })
    }

    fn create_image(
        &self,
        target_id: &str,
        frame: &PrimeFrame,
    ) -> Result<egl::types::EGLImageKHR, String> {
        let mut attrs = vec![
            egl::WIDTH as egl::types::EGLint,
            frame.width as egl::types::EGLint,
            egl::HEIGHT as egl::types::EGLint,
            frame.height as egl::types::EGLint,
            EGL_LINUX_DRM_FOURCC_EXT,
            frame.format as egl::types::EGLint,
        ];

        for plane_index in 0..frame.planes.len() {
            let plane = frame.plane(plane_index)?;
            let object = frame.object(plane.obj_idx as usize)?;
            attrs.push(plane_fd_attr(plane_index)?);
            attrs.push(object.fd.as_raw_fd());
            attrs.push(plane_offset_attr(plane_index)?);
            attrs.push(plane.offset as egl::types::EGLint);
            attrs.push(plane_pitch_attr(plane_index)?);
            attrs.push(plane.pitch as egl::types::EGLint);

            if let Some(modifier) = object.modifier {
                attrs.push(plane_modifier_lo_attr(plane_index)?);
                attrs.push(modifier as u32 as egl::types::EGLint);
                attrs.push(plane_modifier_hi_attr(plane_index)?);
                attrs.push((modifier >> 32) as u32 as egl::types::EGLint);
            }
        }

        attrs.push(egl::NONE as egl::types::EGLint);

        let image = if self.egl.CreateImageKHR.is_loaded() {
            unsafe {
                self.egl.CreateImageKHR(
                    self.display,
                    egl::NO_CONTEXT,
                    EGL_LINUX_DMA_BUF_EXT,
                    ptr::null_mut(),
                    attrs.as_ptr(),
                )
            }
        } else {
            let attrs_1_5: Vec<egl::types::EGLAttrib> = attrs
                .iter()
                .map(|value| *value as egl::types::EGLAttrib)
                .collect();
            let image = unsafe {
                self.egl.CreateImage(
                    self.display,
                    egl::NO_CONTEXT,
                    EGL_LINUX_DMA_BUF_EXT,
                    ptr::null_mut(),
                    attrs_1_5.as_ptr(),
                )
            };
            image as egl::types::EGLImageKHR
        };

        if image == egl::NO_IMAGE_KHR {
            return Err(format!(
                "failed to create EGL image for target={target_id} drm_format={:#x}",
                frame.format
            ));
        }

        Ok(image)
    }

    fn destroy_image(&self, image: egl::types::EGLImageKHR) {
        unsafe {
            if self.egl.DestroyImageKHR.is_loaded() {
                let _ = self.egl.DestroyImageKHR(self.display, image);
            } else if self.egl.DestroyImage.is_loaded() {
                let _ = self
                    .egl
                    .DestroyImage(self.display, image as egl::types::EGLImage);
            }
        }
    }
}

fn plane_fd_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_FD_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_FD_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_FD_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

fn plane_offset_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_OFFSET_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_OFFSET_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_OFFSET_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

fn plane_pitch_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_PITCH_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_PITCH_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_PITCH_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

fn plane_modifier_lo_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

fn plane_modifier_hi_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

struct ExternalVideoBlitter {
    program: u32,
    pos_loc: u32,
    tex_coord_loc: u32,
    sampler_loc: i32,
    vertex_buffer: u32,
    vertex_array: u32,
}

impl ExternalVideoBlitter {
    fn new() -> Result<Self, String> {
        let vertices: [f32; 16] = [
            -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0,
        ];

        let vertex_shader = compile_shader(
            gl::VERTEX_SHADER,
            r#"
attribute vec2 aPos;
attribute vec2 aTexCoord;
varying vec2 vTexCoord;
void main() {
  gl_Position = vec4(aPos, 0.0, 1.0);
  vTexCoord = aTexCoord;
}
"#,
        )?;

        let fragment_shader = compile_shader(
            gl::FRAGMENT_SHADER,
            r#"
#extension GL_OES_EGL_image_external : require
precision mediump float;
varying vec2 vTexCoord;
uniform samplerExternalOES uTex;
void main() {
  vec3 rgb = texture2D(uTex, vTexCoord).rgb;
  gl_FragColor = vec4(rgb, 1.0);
}
"#,
        )?;

        let program = link_program(vertex_shader, fragment_shader)?;
        unsafe {
            gl::DeleteShader(vertex_shader);
            gl::DeleteShader(fragment_shader);
        }

        let pos_loc = unsafe {
            gl::GetAttribLocation(program, CString::new("aPos").unwrap().as_ptr()) as u32
        };
        let tex_coord_loc = unsafe {
            gl::GetAttribLocation(program, CString::new("aTexCoord").unwrap().as_ptr()) as u32
        };
        let sampler_loc =
            unsafe { gl::GetUniformLocation(program, CString::new("uTex").unwrap().as_ptr()) };

        if sampler_loc < 0 {
            return Err("video blitter shader is missing uTex uniform".to_string());
        }

        let mut vertex_buffer = 0;
        let mut vertex_array = 0;
        unsafe {
            if gl::GenVertexArrays::is_loaded() && gl::BindVertexArray::is_loaded() {
                gl::GenVertexArrays(1, &mut vertex_array);
                gl::BindVertexArray(vertex_array);
            }
            gl::GenBuffers(1, &mut vertex_buffer);
            gl::BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (vertices.len() * std::mem::size_of::<f32>()) as isize,
                vertices.as_ptr() as *const c_void,
                gl::STATIC_DRAW,
            );
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            if vertex_array != 0 {
                gl::BindVertexArray(0);
            }
        }

        Ok(Self {
            program,
            pos_loc,
            tex_coord_loc,
            sampler_loc,
            vertex_buffer,
            vertex_array,
        })
    }

    fn blit(
        &self,
        target_id: &str,
        external_texture: u32,
        target_fbo: u32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        unsafe {
            let blend_enabled = gl::IsEnabled(gl::BLEND) == gl::TRUE;
            gl::BindFramebuffer(gl::FRAMEBUFFER, target_fbo);
            gl::Viewport(0, 0, width as i32, height as i32);
            gl::Disable(gl::BLEND);
            gl::UseProgram(self.program);
            if self.vertex_array != 0 {
                gl::BindVertexArray(self.vertex_array);
            }
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vertex_buffer);

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(GL_TEXTURE_EXTERNAL_OES, external_texture);
            gl::Uniform1i(self.sampler_loc, 0);

            gl::EnableVertexAttribArray(self.pos_loc);
            gl::VertexAttribPointer(
                self.pos_loc,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * std::mem::size_of::<f32>()) as i32,
                ptr::null(),
            );

            gl::EnableVertexAttribArray(self.tex_coord_loc);
            gl::VertexAttribPointer(
                self.tex_coord_loc,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * std::mem::size_of::<f32>()) as i32,
                (2 * std::mem::size_of::<f32>()) as *const c_void,
            );

            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);

            gl::DisableVertexAttribArray(self.pos_loc);
            gl::DisableVertexAttribArray(self.tex_coord_loc);
            gl::BindTexture(GL_TEXTURE_EXTERNAL_OES, 0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            if self.vertex_array != 0 {
                gl::BindVertexArray(0);
            }
            gl::UseProgram(0);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            if blend_enabled {
                gl::Enable(gl::BLEND);
            }
        }

        gl_step_check(&format!("drawing imported frame for target={target_id}"))
    }
}

fn compile_shader(kind: u32, source: &str) -> Result<u32, String> {
    let shader = unsafe { gl::CreateShader(kind) };
    let source = CString::new(source).map_err(|_| "shader source contained interior nul")?;
    unsafe {
        gl::ShaderSource(shader, 1, &source.as_ptr(), ptr::null());
        gl::CompileShader(shader);
    }

    let mut status = 0;
    unsafe {
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut status);
    }
    if status == 0 {
        let message = shader_info_log(shader);
        unsafe { gl::DeleteShader(shader) };
        return Err(format!("video shader compile failed: {message}"));
    }

    Ok(shader)
}

fn link_program(vertex_shader: u32, fragment_shader: u32) -> Result<u32, String> {
    let program = unsafe { gl::CreateProgram() };
    unsafe {
        gl::AttachShader(program, vertex_shader);
        gl::AttachShader(program, fragment_shader);
        gl::LinkProgram(program);
    }

    let mut status = 0;
    unsafe {
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut status);
    }
    if status == 0 {
        let message = program_info_log(program);
        unsafe { gl::DeleteProgram(program) };
        return Err(format!("video shader link failed: {message}"));
    }

    Ok(program)
}

fn shader_info_log(shader: u32) -> String {
    let mut len = 0;
    unsafe {
        gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
    }
    if len <= 1 {
        return "unknown error".to_string();
    }
    let mut buf = vec![0u8; len as usize];
    unsafe {
        gl::GetShaderInfoLog(
            shader,
            len,
            ptr::null_mut(),
            buf.as_mut_ptr() as *mut gl::types::GLchar,
        );
    }
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_string()
}

fn program_info_log(program: u32) -> String {
    let mut len = 0;
    unsafe {
        gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
    }
    if len <= 1 {
        return "unknown error".to_string();
    }
    let mut buf = vec![0u8; len as usize];
    unsafe {
        gl::GetProgramInfoLog(
            program,
            len,
            ptr::null_mut(),
            buf.as_mut_ptr() as *mut gl::types::GLchar,
        );
    }
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_string()
}

struct ImportedExternalFrame {
    support: *const EglDmabufSupport,
    egl_image: egl::types::EGLImageKHR,
    texture_id: u32,
    _frame: PrimeFrame,
}

impl ImportedExternalFrame {
    fn new(target_id: &str, frame: PrimeFrame, support: &EglDmabufSupport) -> Result<Self, String> {
        let egl_image = support.create_image(target_id, &frame)?;
        let mut texture_id = 0;
        unsafe {
            gl::GenTextures(1, &mut texture_id);
            gl::BindTexture(GL_TEXTURE_EXTERNAL_OES, texture_id);
            gl::TexParameteri(
                GL_TEXTURE_EXTERNAL_OES,
                gl::TEXTURE_MIN_FILTER,
                gl::LINEAR as i32,
            );
            gl::TexParameteri(
                GL_TEXTURE_EXTERNAL_OES,
                gl::TEXTURE_MAG_FILTER,
                gl::LINEAR as i32,
            );
            gl::TexParameteri(
                GL_TEXTURE_EXTERNAL_OES,
                gl::TEXTURE_WRAP_S,
                gl::CLAMP_TO_EDGE as i32,
            );
            gl::TexParameteri(
                GL_TEXTURE_EXTERNAL_OES,
                gl::TEXTURE_WRAP_T,
                gl::CLAMP_TO_EDGE as i32,
            );
            (support.image_target_texture_2d_oes)(GL_TEXTURE_EXTERNAL_OES, egl_image);
            gl::BindTexture(GL_TEXTURE_EXTERNAL_OES, 0);
        }

        gl_step_check(&format!(
            "binding imported external texture for target={target_id}"
        ))?;

        Ok(Self {
            support,
            egl_image,
            texture_id,
            _frame: frame,
        })
    }
}

impl Drop for ImportedExternalFrame {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.texture_id);
            let support = &*self.support;
            support.destroy_image(self.egl_image);
        }
    }
}

struct RenderedVideoTarget {
    spec: VideoTargetSpec,
    output_texture: u32,
    output_fbo: u32,
    _backend_texture: gpu::BackendTexture,
    image: Option<Image>,
    retired_imports: VecDeque<RetiredImport>,
}

impl RenderedVideoTarget {
    fn new(spec: VideoTargetSpec, gr_context: &mut gpu::DirectContext) -> Result<Self, String> {
        let mut output_texture = 0;
        let mut output_fbo = 0;

        unsafe {
            gl::GenTextures(1, &mut output_texture);
            gl::BindTexture(gl::TEXTURE_2D, output_texture);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                spec.width as i32,
                spec.height as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                ptr::null(),
            );

            gl::GenFramebuffers(1, &mut output_fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, output_fbo);
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                output_texture,
                0,
            );

            let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            if status != gl::FRAMEBUFFER_COMPLETE {
                gl::DeleteFramebuffers(1, &output_fbo);
                gl::DeleteTextures(1, &output_texture);
                return Err(format!("video output framebuffer incomplete: {status:#x}"));
            }
        }

        let backend_texture = unsafe {
            gpu::backend_textures::make_gl(
                (spec.width as i32, spec.height as i32),
                Mipmapped::No,
                TextureInfo {
                    target: gl::TEXTURE_2D,
                    id: output_texture,
                    format: skia_safe::gpu::gl::Format::RGBA8.into(),
                    protected: Protected::No,
                },
                format!("video:{}", spec.id),
            )
        };

        paint_video_placeholder(output_fbo, spec.width, spec.height);
        let image = Some(make_output_image(&backend_texture, &spec.id, gr_context)?);

        Ok(Self {
            spec,
            output_texture,
            output_fbo,
            _backend_texture: backend_texture,
            image,
            retired_imports: VecDeque::new(),
        })
    }

    fn upload_frame(
        &mut self,
        frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<(), String> {
        let imported = ImportedExternalFrame::new(&self.spec.id, frame, &ctx.support)?;
        ctx.blitter.blit(
            &self.spec.id,
            imported.texture_id,
            self.output_fbo,
            self.spec.width,
            self.spec.height,
        )?;

        self.image = Some(make_output_image(
            &self._backend_texture,
            &self.spec.id,
            gr_context,
        )?);

        if ctx.use_gl_fences {
            let sync = unsafe { gl::FenceSync(gl::SYNC_GPU_COMMANDS_COMPLETE, 0) };
            if sync.is_null() {
                unsafe {
                    gl::Finish();
                }
            } else {
                unsafe {
                    gl::Flush();
                }
                self.retired_imports
                    .push_back(RetiredImport { sync, imported });
                return Ok(());
            }
        } else {
            unsafe {
                gl::Finish();
            }
        }

        Ok(())
    }

    fn reap_retired_imports(&mut self) -> bool {
        let retired_count = self.retired_imports.len();
        for _ in 0..retired_count {
            let retired = self
                .retired_imports
                .pop_front()
                .expect("retired imports length changed during poll");
            match retired.poll() {
                Ok(RetiredImportPoll::Pending) => self.retired_imports.push_back(retired),
                Ok(RetiredImportPoll::Released) => retired.wait_blocking(&self.spec.id),
                Err(RetiredImportPollError::WaitFailed) => {
                    eprintln!(
                        "video sync failed: glClientWaitSync WAIT_FAILED for target={}; forcing blocking cleanup",
                        self.spec.id
                    );
                    retired.wait_blocking(&self.spec.id);
                }
                Err(RetiredImportPollError::UnexpectedStatus(status)) => {
                    eprintln!(
                        "video sync failed: glClientWaitSync returned unexpected status={status:#x} for target={}; forcing blocking cleanup",
                        self.spec.id
                    );
                    retired.wait_blocking(&self.spec.id);
                }
            }
        }

        !self.retired_imports.is_empty()
    }

    fn drain_retired_imports(&mut self) {
        while let Some(retired) = self.retired_imports.pop_front() {
            retired.wait_blocking(&self.spec.id);
        }
    }

    fn image(&self) -> Option<(&Image, u32, u32)> {
        self.image
            .as_ref()
            .map(|image| (image, self.spec.width, self.spec.height))
    }
}

fn make_output_image(
    backend_texture: &gpu::BackendTexture,
    id: &str,
    gr_context: &mut gpu::DirectContext,
) -> Result<Image, String> {
    Image::from_texture(
        gr_context,
        backend_texture,
        SurfaceOrigin::BottomLeft,
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    )
    .ok_or_else(|| format!("failed to wrap output texture for target {id}"))
}

fn paint_video_placeholder(target_fbo: u32, width: u32, height: u32) {
    let background = [0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32];
    let outer = [0.46_f32, 0.48_f32, 0.52_f32, 1.0_f32];
    let inner = [0.05_f32, 0.06_f32, 0.08_f32, 1.0_f32];
    let symbol = [0.86_f32, 0.88_f32, 0.91_f32, 1.0_f32];

    unsafe {
        let blend_enabled = gl::IsEnabled(gl::BLEND) == gl::TRUE;
        let scissor_enabled = gl::IsEnabled(gl::SCISSOR_TEST) == gl::TRUE;

        let mut previous_fbo: i32 = 0;
        let mut previous_viewport = [0_i32; 4];
        let mut previous_clear_color = [0.0_f32; 4];

        gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut previous_fbo);
        gl::GetIntegerv(gl::VIEWPORT, previous_viewport.as_mut_ptr());
        gl::GetFloatv(gl::COLOR_CLEAR_VALUE, previous_clear_color.as_mut_ptr());

        gl::BindFramebuffer(gl::FRAMEBUFFER, target_fbo);
        gl::Viewport(0, 0, width as i32, height as i32);
        gl::Disable(gl::BLEND);
        gl::Enable(gl::SCISSOR_TEST);

        clear_scissored_rect(0, 0, width as i32, height as i32, background);

        let screen_w = ((width as f32) * 0.24) as i32;
        let screen_h = ((height as f32) * 0.18) as i32;
        let screen_x = ((width as i32 - screen_w) / 2).max(0);
        let screen_y = ((height as i32 - screen_h) / 2).max(0);
        let border = (((width.min(height)) as f32) * 0.006) as i32;
        let border = border.max(3);

        clear_scissored_rect(screen_x, screen_y, screen_w.max(1), screen_h.max(1), outer);
        clear_scissored_rect(
            screen_x + border,
            screen_y + border,
            (screen_w - border * 2).max(1),
            (screen_h - border * 2).max(1),
            inner,
        );

        let play_w = ((screen_w as f32) * 0.18) as i32;
        let play_h = ((screen_h as f32) * 0.34) as i32;
        let bar_w = (play_w / 5).max(2);
        let start_x = screen_x + (screen_w - play_w) / 2;
        let center_y = screen_y + screen_h / 2;

        for column in 0..5 {
            let bar_height = ((play_h as f32) * ((5 - column) as f32 / 5.0)) as i32;
            clear_scissored_rect(
                start_x + column * bar_w,
                center_y - bar_height / 2,
                bar_w,
                bar_height.max(2),
                symbol,
            );
        }

        if !scissor_enabled {
            gl::Disable(gl::SCISSOR_TEST);
        }
        if blend_enabled {
            gl::Enable(gl::BLEND);
        }
        gl::ClearColor(
            previous_clear_color[0],
            previous_clear_color[1],
            previous_clear_color[2],
            previous_clear_color[3],
        );
        gl::Viewport(
            previous_viewport[0],
            previous_viewport[1],
            previous_viewport[2],
            previous_viewport[3],
        );
        gl::BindFramebuffer(gl::FRAMEBUFFER, previous_fbo as u32);
    }
}

fn clear_scissored_rect(x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
    unsafe {
        gl::Scissor(x, y, width.max(1), height.max(1));
        gl::ClearColor(color[0], color[1], color[2], color[3]);
        gl::Clear(gl::COLOR_BUFFER_BIT);
    }
}

impl Drop for RenderedVideoTarget {
    fn drop(&mut self) {
        self.drain_retired_imports();
        unsafe {
            gl::DeleteFramebuffers(1, &self.output_fbo);
            gl::DeleteTextures(1, &self.output_texture);
        }
    }
}

#[derive(Default)]
pub struct RendererVideoState {
    targets: HashMap<String, RenderedVideoTarget>,
}

impl RendererVideoState {
    pub fn sync_pending(
        &mut self,
        registry: &Arc<VideoRegistry>,
        gr_context: &mut gpu::DirectContext,
        ctx: Option<&VideoImportContext>,
    ) -> Result<VideoSyncResult, String> {
        let mut needs_cleanup = false;
        for target in self.targets.values_mut() {
            needs_cleanup |= target.reap_retired_imports();
        }

        let target_specs = registry.target_specs()?;
        let existing: HashSet<_> = target_specs.keys().cloned().collect();
        let before = self.targets.len();
        self.targets.retain(|id, _| existing.contains(id));
        let mut resources_changed = self.targets.len() != before;

        for (id, spec) in &target_specs {
            if !self.targets.contains_key(id) {
                self.targets.insert(
                    id.clone(),
                    RenderedVideoTarget::new(spec.clone(), gr_context)?,
                );
                resources_changed = true;
            }
        }

        if let Some(ctx) = ctx {
            let snapshot = registry.snapshot_pending()?;
            for pending in snapshot.pending {
                let target = self
                    .targets
                    .entry(pending.id.clone())
                    .or_insert(RenderedVideoTarget::new(pending.spec, gr_context)?);
                target.upload_frame(pending.frame, ctx, gr_context)?;
                resources_changed = true;
                needs_cleanup |= target.reap_retired_imports();
            }
        }

        Ok(VideoSyncResult {
            resources_changed,
            needs_cleanup,
        })
    }

    pub fn image(&self, id: &str) -> Option<(&Image, u32, u32)> {
        self.targets.get(id).and_then(RenderedVideoTarget::image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn test_prime_frame(width: u32, height: u32) -> PrimeFrame {
        PrimeFrame {
            width,
            height,
            format: DRM_FORMAT_NV12,
            objects: Vec::new(),
            planes: Vec::new(),
            keepalive: FrozenTerm {
                env: None,
                saved: None,
            },
            owner_pid: unsafe { std::mem::zeroed() },
        }
    }

    #[test]
    fn drain_pending_to_release_moves_pending_frames_to_release_queue() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx);

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        registry
            .submit_prime("preview", test_prime_frame(64, 32))
            .expect("frame should be accepted");
        registry
            .drain_pending_to_release()
            .expect("pending frames should drain");

        assert!(registry
            .snapshot_pending()
            .expect("snapshot should succeed")
            .pending
            .is_empty());

        let released = release_rx.try_recv().expect("expected released frame");
        assert_eq!(released.width, 64);
        assert_eq!(released.height, 32);
        assert!(release_rx.try_recv().is_err());
    }

    #[test]
    fn drain_pending_to_release_is_noop_when_registry_has_no_pending_frames() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx);

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        registry
            .drain_pending_to_release()
            .expect("empty drain should succeed");

        assert!(release_rx.try_recv().is_err());
        assert!(registry
            .snapshot_pending()
            .expect("snapshot should succeed")
            .pending
            .is_empty());
    }
}
