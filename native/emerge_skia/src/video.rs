use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::ffi::{CStr, CString, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::os::raw::c_char;
use std::ptr;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::Instant;

use crossbeam_channel::{Sender, unbounded};
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use glutin_egl_sys::egl;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use libloading::Library;
use rustler::env::SavedTerm;
use rustler::{Decoder, Encoder, Env, LocalPid, NifResult, OwnedEnv, Term};
use skia_safe::{
    AlphaType, ColorType, Image,
    gpu::{self, Mipmapped, Protected, SurfaceOrigin, gl::TextureInfo},
};

use crate::{backend::wake::BackendWakeHandle, stats::RendererStatsCollector};

rustler::atoms! {
    keepalive,
}

const DRM_FORMAT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
#[cfg(target_os = "linux")]
const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
#[cfg(target_os = "linux")]
const DMA_BUF_SYNC_READ: u64 = 1 << 0;
#[cfg(target_os = "linux")]
const DMA_BUF_SYNC_END: u64 = 1 << 2;

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_LINUX_DMA_BUF_EXT: egl::types::EGLenum = 0x3270;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_LINUX_DRM_FOURCC_EXT: egl::types::EGLint = 0x3271;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE0_FD_EXT: egl::types::EGLint = 0x3272;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: egl::types::EGLint = 0x3273;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE0_PITCH_EXT: egl::types::EGLint = 0x3274;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE1_FD_EXT: egl::types::EGLint = 0x3275;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE1_OFFSET_EXT: egl::types::EGLint = 0x3276;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE1_PITCH_EXT: egl::types::EGLint = 0x3277;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE2_FD_EXT: egl::types::EGLint = 0x3278;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE2_OFFSET_EXT: egl::types::EGLint = 0x3279;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE2_PITCH_EXT: egl::types::EGLint = 0x327A;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: egl::types::EGLint = 0x3443;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: egl::types::EGLint = 0x3444;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT: egl::types::EGLint = 0x3445;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT: egl::types::EGLint = 0x3446;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT: egl::types::EGLint = 0x3447;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT: egl::types::EGLint = 0x3448;

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

struct VideoLease {
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
}

impl VideoLease {
    fn release_from_native_thread(self) {
        let Self {
            keepalive: mut keepalive_term,
            owner_pid,
        } = self;
        keepalive_term.send_once_with(&owner_pid, |env, payload| {
            (keepalive(), payload).encode(env)
        });
    }
}

fn validate_prime_target(
    target_id: &str,
    mode: VideoMode,
    target_width: u32,
    target_height: u32,
    frame_width: u32,
    frame_height: u32,
    frame_format: u32,
) -> Result<(), String> {
    if mode != VideoMode::Prime {
        return Err(format!("video target {target_id} is not a prime target"));
    }

    if frame_width != target_width || frame_height != target_height {
        return Err(format!(
            "prime frame size {}x{} does not match target {}x{}",
            frame_width, frame_height, target_width, target_height
        ));
    }

    if frame_format != DRM_FORMAT_NV12 {
        return Err(format!(
            "unsupported DRM format {:#x}; only NV12 is supported in v1",
            frame_format
        ));
    }

    Ok(())
}

struct Fd(OwnedFd);

impl<'a> Decoder<'a> for Fd {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        let fd: i32 = term.decode()?;
        if fd < 0 {
            return Err(rustler::Error::BadArg);
        }

        // Prime descriptor fds are owned by the descriptor's keepalive resource. Duplicate the
        // borrowed fd before taking ownership so dropping or fencing the frame cannot close the
        // producer's copy, and a discarded BEAM message cannot leak an unmanaged integer fd.
        let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicated < 0 {
            return Err(rustler::Error::BadArg);
        }

        Ok(Self(unsafe { OwnedFd::from_raw_fd(duplicated) }))
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

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
struct PrimeObjectOwned {
    fd: OwnedFd,
    modifier: Option<u64>,
}

#[derive(Clone)]
#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
struct PrimePlaneDesc {
    obj_idx: u32,
    pitch: u32,
    offset: u64,
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
pub struct PrimeFrame {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    objects: Vec<PrimeObjectOwned>,
    planes: Vec<PrimePlaneDesc>,
    lease: Option<VideoLease>,
    submitted_at: Instant,
    stats: Option<Arc<RendererStatsCollector>>,
}

impl PrimeFrame {
    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn record_imported(&self) {
        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_imported(self.submitted_at.elapsed());
        }
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn stats(&self) -> Option<Arc<RendererStatsCollector>> {
        self.stats.clone()
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn object(&self, index: usize) -> Result<&PrimeObjectOwned, String> {
        self.objects
            .get(index)
            .ok_or_else(|| format!("prime object index out of range: {index}"))
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn plane(&self, index: usize) -> Result<&PrimePlaneDesc, String> {
        self.planes
            .get(index)
            .ok_or_else(|| format!("prime plane index out of range: {index}"))
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn sample_luma(&self) -> Result<ChannelSample, String> {
        let plane = self.plane(0)?;
        let object = self.object(plane.obj_idx as usize)?;
        let width = self.width as usize;
        let height = self.height as usize;
        let pitch = plane.pitch as usize;
        let offset = plane.offset as usize;
        let map_len = offset
            .checked_add(
                pitch
                    .checked_mul(height.saturating_sub(1))
                    .ok_or_else(|| "luma plane mapping length overflow".to_string())?,
            )
            .and_then(|last_row| last_row.checked_add(width))
            .ok_or_else(|| "luma plane mapping length overflow".to_string())?;

        let mut sync_flags = DMA_BUF_SYNC_READ;
        if unsafe { libc::ioctl(object.fd.as_raw_fd(), DMA_BUF_IOCTL_SYNC, &mut sync_flags) } < 0 {
            return Err(format!(
                "failed to begin DMA-BUF CPU read: {}",
                std::io::Error::last_os_error()
            ));
        }

        let mapping = unsafe {
            libc::mmap(
                ptr::null_mut(),
                map_len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                object.fd.as_raw_fd(),
                0,
            )
        };
        if mapping == libc::MAP_FAILED {
            let mapping_error = std::io::Error::last_os_error();
            let mut end_flags = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_END;
            unsafe {
                libc::ioctl(object.fd.as_raw_fd(), DMA_BUF_IOCTL_SYNC, &mut end_flags);
            }
            return Err(format!("failed to map NV12 luma plane: {mapping_error}"));
        }

        let mut min = u8::MAX;
        let mut max = u8::MIN;
        let mut sum = 0_u64;
        for row in 0..height {
            let row_ptr = unsafe { (mapping as *const u8).add(offset + row * pitch) };
            let pixels = unsafe { std::slice::from_raw_parts(row_ptr, width) };
            for value in pixels {
                min = min.min(*value);
                max = max.max(*value);
                sum += u64::from(*value);
            }
        }

        unsafe {
            libc::munmap(mapping, map_len);
        }
        let mut end_flags = DMA_BUF_SYNC_READ | DMA_BUF_SYNC_END;
        if unsafe { libc::ioctl(object.fd.as_raw_fd(), DMA_BUF_IOCTL_SYNC, &mut end_flags) } < 0 {
            return Err(format!(
                "failed to finish DMA-BUF CPU read: {}",
                std::io::Error::last_os_error()
            ));
        }

        let count = width.saturating_mul(height);
        if count == 0 {
            return Err("cannot sample an empty luma plane".to_string());
        }

        Ok(ChannelSample {
            min,
            max,
            mean: sum as f64 / count as f64,
        })
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
                    offset: u64::from(plane.offset),
                })
                .collect(),
            lease: Some(VideoLease {
                keepalive: desc.keepalive,
                owner_pid: desc.owner_pid,
            }),
            submitted_at: Instant::now(),
            stats: None,
        }
    }
}

impl Drop for PrimeFrame {
    fn drop(&mut self) {
        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_lease_released(self.submitted_at.elapsed());
        }
        self.planes.clear();
        self.objects.clear();
        if let Some(lease) = self.lease.take() {
            lease.release_from_native_thread();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoMode {
    Prime,
}

pub fn prime_video_unavailable_error() -> String {
    "prime video targets require runtime DMA-BUF and external-image support on the active backend"
        .to_string()
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

#[derive(Default)]
struct VideoRegistryState {
    targets: HashMap<String, VideoTargetEntry>,
    prime_video_available: bool,
}

fn take_pending_frames(targets: &mut HashMap<String, VideoTargetEntry>) -> Vec<PendingVideoFrame> {
    targets
        .iter_mut()
        .filter_map(|(id, entry)| {
            entry.pending.take().map(|frame| PendingVideoFrame {
                id: id.clone(),
                spec: entry.spec.clone(),
                frame,
            })
        })
        .collect()
}

pub struct VideoRegistry {
    state: Mutex<VideoRegistryState>,
    release_tx: Sender<PrimeFrame>,
    generation: AtomicU64,
    stats: Option<Arc<RendererStatsCollector>>,
}

impl VideoRegistry {
    pub fn new(release_tx: Sender<PrimeFrame>, stats: Option<Arc<RendererStatsCollector>>) -> Self {
        Self {
            state: Mutex::new(VideoRegistryState::default()),
            release_tx,
            generation: AtomicU64::new(0),
            stats,
        }
    }

    pub fn create_target(&self, spec: VideoTargetSpec) -> Result<(), String> {
        self.create_target_with_policy(spec, false)
    }

    pub fn create_target_if_available(&self, spec: VideoTargetSpec) -> Result<(), String> {
        self.create_target_with_policy(spec, true)
    }

    fn create_target_with_policy(
        &self,
        spec: VideoTargetSpec,
        require_available: bool,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        if require_available && !state.prime_video_available {
            return Err(prime_video_unavailable_error());
        }
        if state.targets.contains_key(&spec.id) {
            return Err(format!("video target already exists: {}", spec.id));
        }

        state.targets.insert(
            spec.id.clone(),
            VideoTargetEntry {
                spec,
                pending: None,
            },
        );
        drop(state);
        self.bump_generation();
        Ok(())
    }

    pub fn set_prime_video_available(&self, available: bool) -> Result<(), String> {
        let (changed, pending) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned")?;
            let changed = state.prime_video_available != available;
            state.prime_video_available = available;
            let pending = if available {
                Vec::new()
            } else {
                take_pending_frames(&mut state.targets)
            };
            (changed, pending)
        };

        if changed || !pending.is_empty() {
            self.bump_generation();
        }
        self.record_pending_taken(pending.len());
        pending
            .into_iter()
            .for_each(|pending| self.defer_release(pending.frame));
        Ok(())
    }

    pub fn remove_target(&self, id: &str) {
        let pending = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.targets.remove(id).and_then(|entry| entry.pending));
        self.bump_generation();
        if let Some(frame) = pending {
            if let Some(stats) = self.stats.as_deref() {
                stats.record_video_pending_taken(1);
            }
            self.defer_release(frame);
        }
    }

    pub fn submit_prime_if_available(&self, id: &str, frame: PrimeFrame) -> Result<(), String> {
        self.submit_prime_with_policy(id, frame, true)
    }

    pub fn submit_prime(&self, id: &str, frame: PrimeFrame) -> Result<(), String> {
        self.submit_prime_with_policy(id, frame, false)
    }

    fn submit_prime_with_policy(
        &self,
        id: &str,
        mut frame: PrimeFrame,
        require_available: bool,
    ) -> Result<(), String> {
        let frame_width = frame.width;
        let frame_height = frame.height;
        let frame_format = frame.format;

        let previous = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => {
                    self.defer_release(frame);
                    return Err("video registry lock poisoned".to_string());
                }
            };
            if require_available && !state.prime_video_available {
                drop(state);
                self.defer_release(frame);
                return Err(prime_video_unavailable_error());
            }
            let entry = match state.targets.get_mut(id) {
                Some(entry) => entry,
                None => {
                    drop(state);
                    self.defer_release(frame);
                    return Err(format!("unknown video target: {id}"));
                }
            };

            if let Err(reason) = validate_prime_target(
                id,
                entry.spec.mode,
                entry.spec.width,
                entry.spec.height,
                frame_width,
                frame_height,
                frame_format,
            ) {
                drop(state);
                self.defer_release(frame);
                return Err(reason);
            }

            frame.submitted_at = Instant::now();
            frame.stats = self.stats.clone();
            entry.pending.replace(frame)
        };

        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_submitted(previous.is_some());
        }

        if let Some(previous) = previous {
            self.defer_release(previous);
        }

        self.bump_generation();

        Ok(())
    }

    #[cfg(all(feature = "drm", target_os = "linux"))]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot_pending(&self) -> Result<VideoRegistrySnapshot, String> {
        let pending = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned")?;
            take_pending_frames(&mut state.targets)
        };
        self.record_pending_taken(pending.len());
        Ok(VideoRegistrySnapshot { pending })
    }

    pub fn target_specs(&self) -> Result<HashMap<String, VideoTargetSpec>, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        Ok(state
            .targets
            .iter()
            .map(|(id, entry)| (id.clone(), entry.spec.clone()))
            .collect())
    }

    pub fn target_spec(&self, id: &str) -> Result<VideoTargetSpec, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned".to_string())?;
        state
            .targets
            .get(id)
            .map(|entry| entry.spec.clone())
            .ok_or_else(|| format!("unknown video target: {id}"))
    }

    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
    pub fn drain_pending_to_release(&self) -> Result<(), String> {
        let snapshot = self.snapshot_pending()?;

        for pending in snapshot.pending {
            self.defer_release(pending.frame);
        }

        Ok(())
    }

    fn record_pending_taken(&self, count: usize) {
        if count > 0
            && let Some(stats) = self.stats.as_deref()
        {
            stats.record_video_pending_taken(count);
        }
    }

    fn record_import_gauges(&self, direct_imports: usize, retired_imports: usize) {
        if let Some(stats) = self.stats.as_deref() {
            stats.set_video_import_gauges(direct_imports, retired_imports);
        }
    }

    pub fn defer_release(&self, frame: PrimeFrame) {
        if let Err(err) = self.release_tx.send(frame) {
            let holder = Arc::new(Mutex::new(Some(err.into_inner())));
            let worker_holder = Arc::clone(&holder);
            let spawned = thread::Builder::new()
                .name("emerge_skia_video_release_fallback".into())
                .spawn(move || {
                    if let Ok(mut frame) = worker_holder.lock() {
                        drop(frame.take());
                    }
                });

            if let Err(error) = spawned {
                eprintln!("failed to start video release fallback thread: {error}");
                // Frozen BEAM terms must be released from a native thread. Leaking in this
                // process-failure path is safer than dropping the frame on a BEAM scheduler.
                std::mem::forget(holder);
            }
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
    #[cfg_attr(not(all(feature = "wayland", target_os = "linux")), allow(dead_code))]
    pub fn new(wake: BackendWakeHandle) -> Self {
        Self(wake)
    }

    #[cfg_attr(
        not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )),
        allow(dead_code)
    )]
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

#[rustler::resource_impl]
impl rustler::Resource for VideoTargetResource {}

impl Drop for VideoTargetResource {
    fn drop(&mut self) {
        self.registry.remove_target(&self.id);
        self.wake.notify();
    }
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
pub fn spawn_release_worker() -> std::io::Result<Sender<PrimeFrame>> {
    let (tx, rx) = unbounded();
    thread::Builder::new()
        .name("emerge_skia_video_release".into())
        .spawn(move || {
            while let Ok(frame) = rx.recv() {
                drop(frame);
            }
        })?;
    Ok(tx)
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
type GlEglImageTargetTexture2DOes = unsafe extern "system" fn(u32, *const c_void);
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
type RawEglGetProcAddress =
    unsafe extern "system" fn(
        *const c_char,
    ) -> egl::types::__eglMustCastToProperFunctionPointerType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VideoImportPath {
    BlitRgba,
    #[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
    DirectExternal,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VideoImportCapabilities {
    external_image: bool,
    core_vertex_arrays: bool,
    core_sync_objects: bool,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl VideoImportCapabilities {
    #[cfg(all(feature = "drm", target_os = "linux"))]
    pub(crate) fn from_gl_report(version: &str, extensions: &str) -> Self {
        Self::classify(
            parse_gles_major(version),
            extension_list_contains(extensions, "GL_OES_EGL_image_external"),
            gl::GenVertexArrays::is_loaded() && gl::BindVertexArray::is_loaded(),
            gl::FenceSync::is_loaded()
                && gl::ClientWaitSync::is_loaded()
                && gl::DeleteSync::is_loaded(),
        )
    }

    fn current() -> Self {
        // Preserve Wayland's existing late shader/symbol capability checks. DRM passes an
        // explicit version-aware report so its GLES2 baseline never invokes core ES3 APIs.
        Self {
            external_image: true,
            core_vertex_arrays: gl::GenVertexArrays::is_loaded()
                && gl::BindVertexArray::is_loaded(),
            core_sync_objects: gl::FenceSync::is_loaded()
                && gl::ClientWaitSync::is_loaded()
                && gl::DeleteSync::is_loaded(),
        }
    }

    #[cfg(any(test, all(feature = "drm", target_os = "linux")))]
    fn classify(
        gles_major: Option<u8>,
        external_image: bool,
        vertex_array_entry_points: bool,
        sync_entry_points: bool,
    ) -> Self {
        let gles3_or_newer = gles_major.is_some_and(|major| major >= 3);
        Self {
            external_image,
            core_vertex_arrays: gles3_or_newer && vertex_array_entry_points,
            core_sync_objects: gles3_or_newer && sync_entry_points,
        }
    }

    pub(crate) fn external_image(self) -> bool {
        self.external_image
    }

    pub(crate) fn core_vertex_arrays(self) -> bool {
        self.core_vertex_arrays
    }

    pub(crate) fn core_sync_objects(self) -> bool {
        self.core_sync_objects
    }
}

#[cfg(any(test, all(feature = "drm", target_os = "linux")))]
fn parse_gles_major(version: &str) -> Option<u8> {
    let version = version.strip_prefix("OpenGL ES")?;
    let version = version.trim_start_matches("-CM").trim_start();
    version.split('.').next()?.parse().ok()
}

#[cfg(any(
    test,
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn extension_list_contains(extensions: &str, expected: &str) -> bool {
    extensions
        .split_ascii_whitespace()
        .any(|extension| extension == expected)
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
pub struct VideoImportContext {
    support: Rc<EglDmabufSupport>,
    blitter: Option<ExternalVideoBlitter>,
    use_gl_fences: bool,
    path: VideoImportPath,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl VideoImportContext {
    pub fn new_current() -> Result<Self, String> {
        Self::new_current_with_path(
            VideoImportPath::BlitRgba,
            VideoImportCapabilities::current(),
        )
    }

    #[cfg(all(feature = "drm", target_os = "linux"))]
    pub(crate) fn new_current_direct(
        capabilities: VideoImportCapabilities,
    ) -> Result<Self, String> {
        Self::new_current_with_path(VideoImportPath::DirectExternal, capabilities)
    }

    fn path(&self) -> VideoImportPath {
        self.path
    }

    fn new_current_with_path(
        path: VideoImportPath,
        capabilities: VideoImportCapabilities,
    ) -> Result<Self, String> {
        if !capabilities.external_image() {
            return Err("GL_OES_EGL_image_external is not advertised".to_string());
        }

        let support = Rc::new(EglDmabufSupport::new_current()?);
        // DRM normally samples the external texture directly. A failed fallback shader must not
        // disable direct composition before Ganesh has had a chance to wrap an actual frame.
        let blitter = match ExternalVideoBlitter::new(capabilities.core_vertex_arrays()) {
            Ok(blitter) => Some(blitter),
            Err(error) if path == VideoImportPath::DirectExternal => {
                eprintln!("RGBA video fallback unavailable: {error}");
                None
            }
            Err(error) => return Err(error),
        };
        Ok(Self {
            support,
            blitter,
            use_gl_fences: capabilities.core_sync_objects(),
            path,
        })
    }
}

#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
pub struct VideoImportContext;

#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
impl VideoImportContext {
    pub fn new_current() -> Result<Self, String> {
        Err("prime video import requires a Wayland or DRM backend build".to_string())
    }

    fn path(&self) -> VideoImportPath {
        VideoImportPath::BlitRgba
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn gl_step_check(step: &str) -> Result<(), String> {
    let errors = collect_gl_errors();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("OpenGL errors after {step}: {errors:?}"))
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
#[derive(Clone, Copy, Debug)]
struct ChannelSample {
    min: u8,
    max: u8,
    mean: f64,
}

#[derive(Clone, Debug, Default)]
pub struct VideoSyncResult {
    pub resources_changed: bool,
    pub needs_cleanup: bool,
    pub imported_frames: usize,
    pub newest_import_submitted_at: Option<Instant>,
    pub first_frame_diagnostics: Option<String>,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
struct RetiredImport {
    sync: gl::types::GLsync,
    imported: ImportedExternalFrame,
    retired_at: Instant,
    stats: Option<Arc<RendererStatsCollector>>,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
enum RetiredImportPoll {
    Released,
    Pending,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
enum RetiredImportPollError {
    WaitFailed,
    UnexpectedStatus(u32),
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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
        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_retired_fence_released(self.retired_at.elapsed());
        }
        drop(self.imported);
    }
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
struct RetiredImport;

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
enum RetiredImportPoll {
    Released,
    Pending,
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
enum RetiredImportPollError {
    WaitFailed,
    UnexpectedStatus(u32),
}

#[cfg(not(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
)))]
impl RetiredImport {
    fn poll(&self) -> Result<RetiredImportPoll, RetiredImportPollError> {
        Ok(RetiredImportPoll::Released)
    }

    fn wait_blocking(self, _target_id: &str) {}
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
struct EglDmabufSupport {
    egl: egl::Egl,
    _lib: Library,
    display: egl::types::EGLDisplay,
    image_target_texture_2d_oes: GlEglImageTargetTexture2DOes,
    supports_modifiers: bool,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

        let egl_extensions = unsafe { egl.QueryString(display, egl::EXTENSIONS as i32) };
        let egl_extensions = if egl_extensions.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(egl_extensions) }
                .to_string_lossy()
                .into_owned()
        };
        if !extension_list_contains(&egl_extensions, "EGL_EXT_image_dma_buf_import") {
            return Err("EGL_EXT_image_dma_buf_import is not advertised".to_string());
        }
        let supports_modifiers =
            extension_list_contains(&egl_extensions, "EGL_EXT_image_dma_buf_import_modifiers");

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
            supports_modifiers,
        })
    }

    fn create_image(
        &self,
        target_id: &str,
        frame: &PrimeFrame,
    ) -> Result<egl::types::EGLImageKHR, String> {
        let width = i32::try_from(frame.width)
            .map_err(|_| "DMA-BUF width exceeds EGL integer range".to_string())?;
        let height = i32::try_from(frame.height)
            .map_err(|_| "DMA-BUF height exceeds EGL integer range".to_string())?;
        validate_modifier_support(
            self.supports_modifiers,
            frame.objects.iter().any(|object| object.modifier.is_some()),
        )?;

        let mut attrs = vec![
            egl::WIDTH as egl::types::EGLint,
            width,
            egl::HEIGHT as egl::types::EGLint,
            height,
            EGL_LINUX_DRM_FOURCC_EXT,
            frame.format as egl::types::EGLint,
        ];

        for plane_index in 0..frame.planes.len() {
            let plane = frame.plane(plane_index)?;
            let object = frame.object(plane.obj_idx as usize)?;
            attrs.push(plane_fd_attr(plane_index)?);
            attrs.push(object.fd.as_raw_fd());
            attrs.push(plane_offset_attr(plane_index)?);
            attrs
                .push(i32::try_from(plane.offset).map_err(|_| {
                    format!("plane {plane_index} offset exceeds EGL integer range")
                })?);
            attrs.push(plane_pitch_attr(plane_index)?);
            attrs.push(
                i32::try_from(plane.pitch)
                    .map_err(|_| format!("plane {plane_index} pitch exceeds EGL integer range"))?,
            );

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

#[cfg(any(
    test,
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn validate_modifier_support(
    supports_modifiers: bool,
    has_explicit_modifier: bool,
) -> Result<(), String> {
    if has_explicit_modifier && !supports_modifiers {
        Err(
            "DMA-BUF frame uses an explicit modifier, but EGL_EXT_image_dma_buf_import_modifiers is not advertised"
                .to_string(),
        )
    } else {
        Ok(())
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn plane_fd_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_FD_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_FD_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_FD_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn plane_offset_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_OFFSET_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_OFFSET_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_OFFSET_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn plane_pitch_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_PITCH_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_PITCH_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_PITCH_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn plane_modifier_lo_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn plane_modifier_hi_attr(index: usize) -> Result<egl::types::EGLint, String> {
    match index {
        0 => Ok(EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT),
        1 => Ok(EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT),
        2 => Ok(EGL_DMA_BUF_PLANE2_MODIFIER_HI_EXT),
        _ => Err(format!("unsupported DMA-BUF plane index: {index}")),
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
struct ExternalVideoBlitter {
    program: u32,
    pos_loc: u32,
    tex_coord_loc: u32,
    sampler_loc: i32,
    vertex_buffer: u32,
    vertex_array: u32,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl ExternalVideoBlitter {
    fn new(use_core_vertex_arrays: bool) -> Result<Self, String> {
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
            if use_core_vertex_arrays {
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
struct ImportedExternalFrame {
    support: Rc<EglDmabufSupport>,
    egl_image: egl::types::EGLImageKHR,
    texture_id: u32,
    _frame: PrimeFrame,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl ImportedExternalFrame {
    fn new(
        target_id: &str,
        frame: PrimeFrame,
        support: &Rc<EglDmabufSupport>,
    ) -> Result<Self, String> {
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

        if let Err(err) = gl_step_check(&format!(
            "binding imported external texture for target={target_id}"
        )) {
            unsafe {
                gl::DeleteTextures(1, &texture_id);
            }
            support.destroy_image(egl_image);
            return Err(err);
        }

        frame.record_imported();
        Ok(Self {
            support: Rc::clone(support),
            egl_image,
            texture_id,
            _frame: frame,
        })
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl ImportedExternalFrame {
    fn stats(&self) -> Option<Arc<RendererStatsCollector>> {
        self._frame.stats()
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl Drop for ImportedExternalFrame {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.texture_id);
        }
        self.support.destroy_image(self.egl_image);
    }
}

struct RenderedVideoTarget {
    spec: VideoTargetSpec,
    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    path: VideoImportPath,
    output_texture: u32,
    output_fbo: u32,
    _backend_texture: gpu::BackendTexture,
    image: Option<Image>,
    direct_backend_texture: Option<gpu::BackendTexture>,
    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    direct_import: Option<ImportedExternalFrame>,
    retired_imports: VecDeque<RetiredImport>,
    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    diagnostics_pending: bool,
}

impl RenderedVideoTarget {
    fn new(
        spec: VideoTargetSpec,
        gr_context: &mut gpu::DirectContext,
        path: VideoImportPath,
    ) -> Result<Self, String> {
        #[cfg(not(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        )))]
        let _ = path;

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
            #[cfg(any(
                all(feature = "wayland", target_os = "linux"),
                all(feature = "drm", target_os = "linux")
            ))]
            path,
            output_texture,
            output_fbo,
            _backend_texture: backend_texture,
            image,
            direct_backend_texture: None,
            #[cfg(any(
                all(feature = "wayland", target_os = "linux"),
                all(feature = "drm", target_os = "linux")
            ))]
            direct_import: None,
            retired_imports: VecDeque::new(),
            #[cfg(any(
                all(feature = "wayland", target_os = "linux"),
                all(feature = "drm", target_os = "linux")
            ))]
            diagnostics_pending: true,
        })
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn upload_frame(
        &mut self,
        frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        match self.path {
            VideoImportPath::BlitRgba => self.upload_blitted_frame(frame, ctx, gr_context),
            VideoImportPath::DirectExternal => self.upload_direct_frame(frame, ctx, gr_context),
        }
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn upload_blitted_frame(
        &mut self,
        frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        let luma_sample = self.diagnostics_pending.then(|| frame.sample_luma());
        let imported = ImportedExternalFrame::new(&self.spec.id, frame, &ctx.support)?;
        let blitter = ctx
            .blitter
            .as_ref()
            .ok_or_else(|| "RGBA video blitter is unavailable".to_string())?;
        if let Err(error) = blitter.blit(
            &self.spec.id,
            imported.texture_id,
            self.output_fbo,
            self.spec.width,
            self.spec.height,
        ) {
            self.retire_import(imported, ctx.use_gl_fences);
            return Err(error);
        }

        let diagnostics = if self.diagnostics_pending {
            self.diagnostics_pending = false;
            let rgba_sample = sample_rgba_output(
                self.output_fbo,
                self.spec.width,
                self.spec.height,
                &self.spec.id,
            );
            Some(format_frame_diagnostics(luma_sample, rgba_sample))
        } else {
            None
        };

        let output_image =
            match make_output_image(&self._backend_texture, &self.spec.id, gr_context) {
                Ok(image) => image,
                Err(error) => {
                    self.retire_import(imported, ctx.use_gl_fences);
                    return Err(error);
                }
            };
        self.image = Some(output_image);
        self.retire_import(imported, ctx.use_gl_fences);

        Ok(diagnostics)
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn upload_direct_frame(
        &mut self,
        frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        let luma_sample = self.diagnostics_pending.then(|| frame.sample_luma());
        let imported = ImportedExternalFrame::new(&self.spec.id, frame, &ctx.support)?;
        let backend_texture = unsafe {
            gpu::backend_textures::make_gl(
                (self.spec.width as i32, self.spec.height as i32),
                Mipmapped::No,
                TextureInfo {
                    target: GL_TEXTURE_EXTERNAL_OES,
                    id: imported.texture_id,
                    format: skia_safe::gpu::gl::Format::RGBA8.into(),
                    protected: Protected::No,
                },
                format!("video-external:{}", self.spec.id),
            )
        };
        let image = match Image::from_texture(
            gr_context,
            &backend_texture,
            SurfaceOrigin::TopLeft,
            ColorType::RGBA8888,
            AlphaType::Premul,
            None,
        ) {
            Some(image) => image,
            None => {
                drop(backend_texture);
                eprintln!(
                    "direct external video unavailable for target={}; falling back to RGBA blit",
                    self.spec.id
                );
                return self.fallback_direct_to_blit(imported, luma_sample, ctx, gr_context);
            }
        };

        let diagnostics = if self.diagnostics_pending {
            self.diagnostics_pending = false;
            Some(format_frame_diagnostics(
                luma_sample,
                Err("RGBA readback skipped for direct external video".to_string()),
            ))
        } else {
            None
        };

        // Build the replacement first. If import or Skia wrapping fails, the last good frame and
        // its lease remain intact. Drop the old Skia wrappers before fencing its imported texture.
        self.image.take();
        self.direct_backend_texture.take();
        let previous_import = self.direct_import.take();
        self.image = Some(image);
        self.direct_backend_texture = Some(backend_texture);
        self.direct_import = Some(imported);

        if let Some(previous_import) = previous_import {
            self.retire_import(previous_import, ctx.use_gl_fences);
        }

        Ok(diagnostics)
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn fallback_direct_to_blit(
        &mut self,
        imported: ImportedExternalFrame,
        luma_sample: Option<Result<ChannelSample, String>>,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        let blitter = ctx
            .blitter
            .as_ref()
            .ok_or_else(|| "RGBA video blitter is unavailable".to_string())?;
        if let Err(error) = blitter.blit(
            &self.spec.id,
            imported.texture_id,
            self.output_fbo,
            self.spec.width,
            self.spec.height,
        ) {
            self.retire_import(imported, ctx.use_gl_fences);
            return Err(error);
        }

        let diagnostics = if self.diagnostics_pending {
            self.diagnostics_pending = false;
            let rgba_sample = sample_rgba_output(
                self.output_fbo,
                self.spec.width,
                self.spec.height,
                &self.spec.id,
            );
            Some(format_frame_diagnostics(luma_sample, rgba_sample))
        } else {
            None
        };
        let output_image =
            match make_output_image(&self._backend_texture, &self.spec.id, gr_context) {
                Ok(image) => image,
                Err(error) => {
                    self.retire_import(imported, ctx.use_gl_fences);
                    return Err(error);
                }
            };

        self.image.take();
        self.direct_backend_texture.take();
        let previous_import = self.direct_import.take();
        self.path = VideoImportPath::BlitRgba;
        self.image = Some(output_image);

        if let Some(previous_import) = previous_import {
            self.retire_import(previous_import, ctx.use_gl_fences);
        }
        self.retire_import(imported, ctx.use_gl_fences);

        Ok(diagnostics)
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn retire_import(&mut self, imported: ImportedExternalFrame, use_gl_fences: bool) {
        if use_gl_fences {
            let sync = unsafe { gl::FenceSync(gl::SYNC_GPU_COMMANDS_COMPLETE, 0) };
            if !sync.is_null() {
                unsafe {
                    gl::Flush();
                }
                let stats = imported.stats();
                self.retired_imports.push_back(RetiredImport {
                    sync,
                    imported,
                    retired_at: Instant::now(),
                    stats: stats.clone(),
                });
                if let Some(stats) = stats.as_deref() {
                    stats.record_video_retired_fence_created(self.retired_imports.len());
                }
                return;
            }
        }

        unsafe {
            gl::Finish();
        }
        drop(imported);
    }

    #[cfg(not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )))]
    fn upload_frame(
        &mut self,
        frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        let _ = (&mut *self, frame, ctx, gr_context);
        Err("prime video import requires a Wayland or DRM backend build".to_string())
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

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    fn direct_import_count(&self) -> usize {
        usize::from(self.direct_import.is_some())
    }

    #[cfg(not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )))]
    fn direct_import_count(&self) -> usize {
        0
    }

    fn retired_import_count(&self) -> usize {
        self.retired_imports.len()
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn sample_rgba_output(
    target_fbo: u32,
    width: u32,
    height: u32,
    target_id: &str,
) -> Result<[ChannelSample; 3], String> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "RGBA diagnostic pixel count overflow".to_string())?;
    let byte_count = pixel_count
        .checked_mul(4)
        .ok_or_else(|| "RGBA diagnostic byte count overflow".to_string())?;
    if pixel_count == 0 {
        return Err("cannot sample an empty RGBA frame".to_string());
    }

    let mut pixels = vec![0_u8; byte_count];
    let mut previous_fbo = 0_i32;
    let mut previous_pack_alignment = 0_i32;
    let _ = collect_gl_errors();
    unsafe {
        gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut previous_fbo);
        gl::GetIntegerv(gl::PACK_ALIGNMENT, &mut previous_pack_alignment);
        gl::BindFramebuffer(gl::FRAMEBUFFER, target_fbo);
        gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
        gl::ReadPixels(
            0,
            0,
            width as i32,
            height as i32,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_mut_ptr() as *mut c_void,
        );
        gl::PixelStorei(gl::PACK_ALIGNMENT, previous_pack_alignment);
        gl::BindFramebuffer(gl::FRAMEBUFFER, previous_fbo as u32);
    }
    gl_step_check(&format!(
        "reading diagnostic RGBA frame for target={target_id}"
    ))?;

    let mut min = [u8::MAX; 3];
    let mut max = [u8::MIN; 3];
    let mut sum = [0_u64; 3];
    for pixel in pixels.chunks_exact(4) {
        for channel in 0..3 {
            min[channel] = min[channel].min(pixel[channel]);
            max[channel] = max[channel].max(pixel[channel]);
            sum[channel] += u64::from(pixel[channel]);
        }
    }

    Ok(std::array::from_fn(|channel| ChannelSample {
        min: min[channel],
        max: max[channel],
        mean: sum[channel] as f64 / pixel_count as f64,
    }))
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn format_frame_diagnostics(
    luma: Option<Result<ChannelSample, String>>,
    rgba: Result<[ChannelSample; 3], String>,
) -> String {
    let luma = match luma {
        Some(Ok(sample)) => format!(
            "NV12 Y min={} max={} mean={:.2}",
            sample.min, sample.max, sample.mean
        ),
        Some(Err(err)) => format!("NV12 Y sample failed: {err}"),
        None => "NV12 Y sample unavailable".to_string(),
    };
    let rgba = match rgba {
        Ok([r, g, b]) => format!(
            "RGBA R={}/{}/{:.2} G={}/{}/{:.2} B={}/{}/{:.2}",
            r.min, r.max, r.mean, g.min, g.max, g.mean, b.min, b.max, b.mean
        ),
        Err(err) => format!("RGBA sample failed: {err}"),
    };
    format!("{luma}; {rgba}")
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
        // The current direct import can be sampled again by UI-only redraws, so unlike retired
        // imports it remains leased until target teardown. Finish before destroying either kind of
        // backing, then explicitly drop Skia wrappers before their GL objects and EGL image.
        unsafe {
            gl::Finish();
        }
        self.image.take();
        self.direct_backend_texture.take();
        #[cfg(any(
            all(feature = "wayland", target_os = "linux"),
            all(feature = "drm", target_os = "linux")
        ))]
        self.direct_import.take();
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
        let import_path = ctx
            .map(VideoImportContext::path)
            .unwrap_or(VideoImportPath::BlitRgba);
        let existing: HashSet<_> = target_specs.keys().cloned().collect();
        let before = self.targets.len();
        self.targets.retain(|id, _| existing.contains(id));
        let mut resources_changed = self.targets.len() != before;

        for (id, spec) in &target_specs {
            if !self.targets.contains_key(id) {
                let target = match RenderedVideoTarget::new(spec.clone(), gr_context, import_path) {
                    Ok(target) => target,
                    Err(error) => {
                        registry.drain_pending_to_release().map_err(|release_error| {
                            format!(
                                "{error}; additionally failed to release pending video frames: {release_error}"
                            )
                        })?;
                        return Err(error);
                    }
                };
                self.targets.insert(id.clone(), target);
                resources_changed = true;
            }
        }

        let mut imported_frames = 0;
        let mut newest_import_submitted_at = None;
        let mut first_frame_diagnostics = None;
        if let Some(ctx) = ctx {
            let snapshot = registry.snapshot_pending()?;
            for pending in snapshot.pending {
                let target = self.targets.get_mut(&pending.id).ok_or_else(|| {
                    format!("video target disappeared during sync: {}", pending.id)
                })?;
                let submitted_at = pending.frame.submitted_at;
                if let Some(diagnostics) = target.upload_frame(pending.frame, ctx, gr_context)? {
                    first_frame_diagnostics.get_or_insert(diagnostics);
                }
                imported_frames += 1;
                newest_import_submitted_at = Some(
                    newest_import_submitted_at
                        .map(|current: Instant| current.max(submitted_at))
                        .unwrap_or(submitted_at),
                );
                resources_changed = true;
                needs_cleanup |= target.reap_retired_imports();
            }
        } else {
            registry.drain_pending_to_release()?;
        }

        let direct_imports = self
            .targets
            .values()
            .map(RenderedVideoTarget::direct_import_count)
            .sum();
        let retired_imports = self
            .targets
            .values()
            .map(RenderedVideoTarget::retired_import_count)
            .sum();
        registry.record_import_gauges(direct_imports, retired_imports);

        Ok(VideoSyncResult {
            resources_changed,
            needs_cleanup,
            imported_frames,
            newest_import_submitted_at,
            first_frame_diagnostics,
        })
    }

    pub fn image(&self, id: &str) -> Option<(&Image, u32, u32)> {
        self.targets.get(id).and_then(RenderedVideoTarget::image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::RendererTimingMetric;
    use crossbeam_channel::unbounded;

    fn test_prime_frame(width: u32, height: u32) -> PrimeFrame {
        PrimeFrame {
            width,
            height,
            format: DRM_FORMAT_NV12,
            objects: Vec::new(),
            planes: Vec::new(),
            lease: None,
            submitted_at: Instant::now(),
            stats: None,
        }
    }

    #[test]
    fn gles_version_and_extension_parsing_are_conservative() {
        assert_eq!(parse_gles_major("OpenGL ES 2.0 Mesa 24.3"), Some(2));
        assert_eq!(parse_gles_major("OpenGL ES 3.2 Vendor"), Some(3));
        assert_eq!(parse_gles_major("OpenGL ES-CM 1.1"), Some(1));
        assert_eq!(parse_gles_major("OpenGL 4.6"), None);

        let extensions = "GL_OES_EGL_image_external GL_EXT_disjoint_timer_query";
        assert!(extension_list_contains(
            extensions,
            "GL_OES_EGL_image_external"
        ));
        assert!(!extension_list_contains(extensions, "GL_OES_EGL_image"));
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    #[test]
    fn es2_disables_core_vao_and_sync_even_with_loaded_entry_points() {
        let capabilities = VideoImportCapabilities::classify(Some(2), true, true, true);
        assert!(capabilities.external_image());
        assert!(!capabilities.core_vertex_arrays());
        assert!(!capabilities.core_sync_objects());

        let capabilities = VideoImportCapabilities::classify(Some(3), true, true, true);
        assert!(capabilities.core_vertex_arrays());
        assert!(capabilities.core_sync_objects());
    }

    #[test]
    fn explicit_dma_buf_modifiers_require_the_modifier_extension() {
        assert!(validate_modifier_support(false, false).is_ok());
        assert!(validate_modifier_support(true, true).is_ok());
        assert!(validate_modifier_support(false, true).is_err());
    }

    #[test]
    fn unavailable_prime_target_creation_is_rejected() {
        let (release_tx, _release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx, None);
        let spec = VideoTargetSpec {
            id: "preview".to_string(),
            width: 64,
            height: 32,
            mode: VideoMode::Prime,
        };

        assert_eq!(
            registry
                .create_target_if_available(spec.clone())
                .expect_err("unavailable import should reject the target"),
            prime_video_unavailable_error()
        );
        registry
            .set_prime_video_available(true)
            .expect("availability should update");
        registry
            .create_target_if_available(spec)
            .expect("available import should accept the target");
    }

    #[test]
    fn unavailable_prime_submission_releases_the_frame() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx, None);
        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        let error = registry
            .submit_prime_if_available("preview", test_prime_frame(64, 32))
            .expect_err("unavailable import should reject the frame");

        assert_eq!(error, prime_video_unavailable_error());
        let released = release_rx.try_recv().expect("expected released frame");
        assert_eq!((released.width, released.height), (64, 32));
    }

    #[test]
    fn disabling_prime_video_atomically_drains_a_pending_frame() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx, None);
        registry
            .set_prime_video_available(true)
            .expect("availability should update");
        registry
            .create_target_if_available(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        registry
            .submit_prime_if_available("preview", test_prime_frame(64, 32))
            .expect("frame should be accepted");

        registry
            .set_prime_video_available(false)
            .expect("availability should update");

        assert!(
            registry
                .snapshot_pending()
                .expect("snapshot should succeed")
                .pending
                .is_empty()
        );
        assert!(release_rx.try_recv().is_ok());
    }

    #[test]
    fn availability_loss_racing_submission_never_strands_a_frame() {
        let (release_tx, release_rx) = unbounded();
        let registry = Arc::new(VideoRegistry::new(release_tx, None));
        registry
            .set_prime_video_available(true)
            .expect("availability should update");
        registry
            .create_target_if_available(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        for _ in 0..32 {
            registry
                .set_prime_video_available(true)
                .expect("availability should update");
            let barrier = Arc::new(std::sync::Barrier::new(3));
            let submit_registry = Arc::clone(&registry);
            let submit_barrier = Arc::clone(&barrier);
            let submit = thread::spawn(move || {
                submit_barrier.wait();
                let _ =
                    submit_registry.submit_prime_if_available("preview", test_prime_frame(64, 32));
            });
            let disable_registry = Arc::clone(&registry);
            let disable_barrier = Arc::clone(&barrier);
            let disable = thread::spawn(move || {
                disable_barrier.wait();
                disable_registry
                    .set_prime_video_available(false)
                    .expect("availability should update");
            });

            barrier.wait();
            submit.join().expect("submit thread should finish");
            disable.join().expect("disable thread should finish");

            assert!(
                registry
                    .snapshot_pending()
                    .expect("snapshot should succeed")
                    .pending
                    .is_empty()
            );
            release_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("racing frame should be released");
            assert!(release_rx.try_recv().is_err());
        }
    }

    #[test]
    fn drain_pending_to_release_moves_pending_frames_to_release_queue() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx, None);

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

        assert!(
            registry
                .snapshot_pending()
                .expect("snapshot should succeed")
                .pending
                .is_empty()
        );

        let released = release_rx.try_recv().expect("expected released frame");
        assert_eq!(released.width, 64);
        assert_eq!(released.height, 32);
        assert!(release_rx.try_recv().is_err());
    }

    #[test]
    fn drain_pending_to_release_is_noop_when_registry_has_no_pending_frames() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx, None);

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
        assert!(
            registry
                .snapshot_pending()
                .expect("snapshot should succeed")
                .pending
                .is_empty()
        );
    }

    #[test]
    fn registry_records_latest_frame_replacement_and_release_lifetimes() {
        let (release_tx, release_rx) = unbounded();
        let stats = Arc::new(RendererStatsCollector::new());
        let registry = VideoRegistry::new(release_tx, Some(Arc::clone(&stats)));

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
            .expect("first frame should be accepted");
        registry
            .submit_prime("preview", test_prime_frame(64, 32))
            .expect("replacement frame should be accepted");

        let replaced = release_rx.try_recv().expect("expected replaced frame");
        drop(replaced);
        let snapshot = registry
            .snapshot_pending()
            .expect("snapshot should succeed");
        drop(snapshot);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.video_pipeline.submitted, 2);
        assert_eq!(snapshot.video_pipeline.pending_replaced, 1);
        assert_eq!(snapshot.video_pipeline.pending_taken, 1);
        assert_eq!(snapshot.video_pipeline.current_pending, 0);
        assert_eq!(snapshot.video_pipeline.leases_released, 2);
        assert_eq!(
            snapshot
                .timing(RendererTimingMetric::VideoSubmitToRelease)
                .count,
            2
        );
    }

    #[test]
    fn submit_prime_releases_rejected_frame_to_release_queue() {
        let (release_tx, release_rx) = unbounded();
        let registry = VideoRegistry::new(release_tx, None);

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        let error = registry
            .submit_prime("preview", test_prime_frame(16, 16))
            .expect_err("mismatched frame should be rejected");

        assert!(error.contains("does not match target"));

        let released = release_rx.try_recv().expect("expected released frame");
        assert_eq!(released.width, 16);
        assert_eq!(released.height, 16);
        assert!(release_rx.try_recv().is_err());
    }
}
