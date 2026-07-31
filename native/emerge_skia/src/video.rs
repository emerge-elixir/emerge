#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::cell::{Cell, RefCell};
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
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::time::Duration;
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
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use video_interop::egl::{
    ClientWaitOutcome, NativeFenceCapabilities, NativeFenceFunctions, ServerWaitOutcome,
    SyncFilePollOutcome, SyncHandle, has_extension, poll_sync_file,
};
use video_interop::{
    ClaimedLease, ClaimedVideoFrame, Modifier, OwnedAcquireSync, OwnedFrame, OwnedStorage,
    PreparedVideoFrame,
};

use crate::{CleanupDispatcher, backend::wake::BackendWakeHandle, stats::RendererStatsCollector};

rustler::atoms! {
    keepalive,
    acquire_fence_fd,
}

static NEXT_RENDERER_EPOCH: AtomicU64 = AtomicU64::new(1);

const DRM_FORMAT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
const DRM_FORMAT_ABGR8888: u32 = fourcc(b'A', b'B', b'2', b'4');
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const DMA_BUF_IOCTL_SYNC: libc::c_ulong = 0x4008_6200;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
const DMA_BUF_SYNC_READ: u64 = 1 << 0;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

    if !matches!(frame_format, DRM_FORMAT_NV12 | DRM_FORMAT_ABGR8888) {
        return Err(format!(
            "unsupported DRM format {frame_format:#x}; supported formats are NV12 and ABGR8888"
        ));
    }

    Ok(())
}

fn validate_prime_descriptor_layout(
    width: u32,
    format: u32,
    object_count: usize,
    planes: &[PrimePlane],
) -> Result<(), String> {
    if !(1..=4).contains(&object_count) {
        return Err(format!(
            "PRIME descriptor must contain between 1 and 4 objects, got {object_count}"
        ));
    }

    let expected_planes = match format {
        DRM_FORMAT_NV12 => 2,
        DRM_FORMAT_ABGR8888 => 1,
        _ => return Ok(()),
    };
    if planes.len() != expected_planes {
        return Err(format!(
            "PRIME format {format:#x} requires {expected_planes} plane(s), got {}",
            planes.len()
        ));
    }

    let minimum_pitch = if format == DRM_FORMAT_ABGR8888 {
        width
            .checked_mul(4)
            .ok_or_else(|| "ABGR8888 minimum pitch overflow".to_string())?
    } else {
        width
    };

    planes.iter().enumerate().try_for_each(|(index, plane)| {
        let object_index = usize::try_from(plane.obj_idx)
            .map_err(|_| format!("PRIME plane {index} object index is invalid"))?;
        if object_index >= object_count {
            return Err(format!(
                "PRIME plane {index} references object {} but descriptor has {object_count} object(s)",
                plane.obj_idx
            ));
        }
        if plane.pitch < minimum_pitch {
            return Err(format!(
                "PRIME plane {index} pitch {} is smaller than required {minimum_pitch}",
                plane.pitch
            ));
        }
        if plane.pitch > 2_147_483_647 {
            return Err(format!("PRIME plane {index} pitch exceeds EGL integer range"));
        }
        if plane.offset > 2_147_483_647 {
            return Err(format!("PRIME plane {index} offset exceeds EGL integer range"));
        }
        Ok(())
    })
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
struct LegacyPrimePlane {
    obj_idx: u32,
    pitch: u32,
    offset: u32,
}

#[derive(Debug, rustler::NifMap)]
struct PrimePlaneMap {
    object_index: u32,
    pitch: u32,
    offset: u64,
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
struct LegacyPrimeObject {
    fd: Fd,
    modifier: Option<u64>,
}

#[derive(rustler::NifMap)]
struct PrimeObjectMap {
    fd: Fd,
    modifier: Option<u64>,
}

#[derive(rustler::NifStruct)]
#[module = "Membrane.PrimeDesc"]
struct LegacyPrimeDesc {
    width: u32,
    height: u32,
    format: Fourcc,
    objects: Vec<LegacyPrimeObject>,
    planes: Vec<LegacyPrimePlane>,
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
    trace_token: Option<TraceToken>,
}

#[derive(rustler::NifMap)]
struct PrimeDescMapWithAcquireFence {
    width: u32,
    height: u32,
    format: Fourcc,
    objects: Vec<PrimeObjectMap>,
    planes: Vec<PrimePlaneMap>,
    acquire_fence_fd: Option<Fd>,
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
}

#[derive(rustler::NifMap)]
struct PrimeDescMap {
    width: u32,
    height: u32,
    format: Fourcc,
    objects: Vec<PrimeObjectMap>,
    planes: Vec<PrimePlaneMap>,
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
}

struct PrimeObject {
    fd: Fd,
    modifier: Option<u64>,
}

struct PrimePlane {
    obj_idx: u32,
    pitch: u32,
    offset: u64,
}

pub struct PrimeDesc {
    width: u32,
    height: u32,
    format: Fourcc,
    objects: Vec<PrimeObject>,
    planes: Vec<PrimePlane>,
    acquire_fence: Option<Fd>,
    keepalive: FrozenTerm,
    owner_pid: LocalPid,
    #[allow(dead_code)]
    trace_token: Option<TraceToken>,
}

impl<'a> Decoder<'a> for PrimeDesc {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        if let Ok(desc) = LegacyPrimeDesc::decode(term) {
            return Ok(desc.into());
        }

        if let Ok(desc) = PrimeDescMapWithAcquireFence::decode(term) {
            return Ok(desc.into());
        }
        if term.map_get(acquire_fence_fd()).is_ok() {
            return Err(rustler::Error::BadArg);
        }

        PrimeDescMap::decode(term).map(Into::into)
    }
}

impl From<LegacyPrimeDesc> for PrimeDesc {
    fn from(desc: LegacyPrimeDesc) -> Self {
        Self {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            objects: desc
                .objects
                .into_iter()
                .map(|object| PrimeObject {
                    fd: object.fd,
                    modifier: object.modifier,
                })
                .collect(),
            planes: desc
                .planes
                .into_iter()
                .map(|plane| PrimePlane {
                    obj_idx: plane.obj_idx,
                    pitch: plane.pitch,
                    offset: u64::from(plane.offset),
                })
                .collect(),
            acquire_fence: None,
            keepalive: desc.keepalive,
            owner_pid: desc.owner_pid,
            trace_token: desc.trace_token,
        }
    }
}

impl From<PrimeDescMapWithAcquireFence> for PrimeDesc {
    fn from(desc: PrimeDescMapWithAcquireFence) -> Self {
        Self {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            objects: desc
                .objects
                .into_iter()
                .map(|object| PrimeObject {
                    fd: object.fd,
                    modifier: object.modifier,
                })
                .collect(),
            planes: desc
                .planes
                .into_iter()
                .map(|plane| PrimePlane {
                    obj_idx: plane.object_index,
                    pitch: plane.pitch,
                    offset: plane.offset,
                })
                .collect(),
            acquire_fence: desc.acquire_fence_fd,
            keepalive: desc.keepalive,
            owner_pid: desc.owner_pid,
            trace_token: None,
        }
    }
}

impl From<PrimeDescMap> for PrimeDesc {
    fn from(desc: PrimeDescMap) -> Self {
        Self {
            width: desc.width,
            height: desc.height,
            format: desc.format,
            objects: desc
                .objects
                .into_iter()
                .map(|object| PrimeObject {
                    fd: object.fd,
                    modifier: object.modifier,
                })
                .collect(),
            planes: desc
                .planes
                .into_iter()
                .map(|plane| PrimePlane {
                    obj_idx: plane.object_index,
                    pitch: plane.pitch,
                    offset: plane.offset,
                })
                .collect(),
            acquire_fence: None,
            keepalive: desc.keepalive,
            owner_pid: desc.owner_pid,
            trace_token: None,
        }
    }
}

impl PrimeDesc {
    pub fn validate_for_target(
        &self,
        target_id: &str,
        mode: VideoMode,
        target_width: u32,
        target_height: u32,
    ) -> Result<(), String> {
        validate_prime_target(
            target_id,
            mode,
            target_width,
            target_height,
            self.width,
            self.height,
            self.format.0,
        )?;
        validate_prime_descriptor_layout(
            self.width,
            self.format.0,
            self.objects.len(),
            &self.planes,
        )
    }
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
    acquire_fence: Option<OwnedFd>,
    lease: Option<PrimeFrameLease>,
    stream_id: Option<u64>,
    submitted_at: Instant,
    stats: Option<Arc<RendererStatsCollector>>,
    #[cfg(test)]
    drop_signal: Option<Sender<()>>,
}

enum PrimeFrameLease {
    Legacy(VideoLease),
    Canonical(ClaimedLease),
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
                    offset: plane.offset,
                })
                .collect(),
            acquire_fence: desc.acquire_fence.map(|fence| fence.0),
            lease: Some(PrimeFrameLease::Legacy(VideoLease {
                keepalive: desc.keepalive,
                owner_pid: desc.owner_pid,
            })),
            stream_id: None,
            submitted_at: Instant::now(),
            stats: None,
            #[cfg(test)]
            drop_signal: None,
        }
    }
}

impl PrimeFrame {
    fn validate_canonical(
        target_id: &str,
        mode: VideoMode,
        target_width: u32,
        target_height: u32,
        expected_fourcc: u32,
        frame: &OwnedFrame,
    ) -> Result<(), String> {
        if frame.visible_rect.x != 0
            || frame.visible_rect.y != 0
            || frame.visible_rect.width != frame.coded_width
            || frame.visible_rect.height != frame.coded_height
        {
            return Err("video target requires a full-frame visible rectangle".to_string());
        }

        let descriptor = match &frame.storage {
            OwnedStorage::DmaBuf(descriptor) => descriptor,
            _ => return Err("unsupported canonical video storage".to_string()),
        };
        if descriptor.layers.len() != 1 {
            return Err(format!(
                "video target requires exactly one DMA-BUF layer, got {}",
                descriptor.layers.len()
            ));
        }
        let layer = descriptor
            .layers
            .first()
            .ok_or_else(|| "video target requires one DMA-BUF layer".to_string())?;
        if layer.fourcc != expected_fourcc {
            return Err(format!(
                "frame DRM format {:#x} does not match stream format {expected_fourcc:#x}",
                layer.fourcc
            ));
        }
        validate_prime_target(
            target_id,
            mode,
            target_width,
            target_height,
            frame.coded_width,
            frame.coded_height,
            layer.fourcc,
        )?;
        let planes = layer
            .planes
            .iter()
            .map(|plane| PrimePlane {
                obj_idx: plane.object_index,
                pitch: plane.pitch,
                offset: plane.offset,
            })
            .collect::<Vec<_>>();
        validate_prime_descriptor_layout(
            frame.coded_width,
            layer.fourcc,
            descriptor.objects.len(),
            &planes,
        )?;

        planes.iter().enumerate().try_for_each(|(index, plane)| {
            let object = descriptor
                .objects
                .get(plane.obj_idx as usize)
                .ok_or_else(|| format!("PRIME plane {index} object index is invalid"))?;
            let rows = match (layer.fourcc, index) {
                (DRM_FORMAT_NV12, 1) => frame.coded_height.div_ceil(2),
                _ => frame.coded_height,
            };
            let row_bytes = if layer.fourcc == DRM_FORMAT_ABGR8888 {
                u64::from(frame.coded_width) * 4
            } else {
                u64::from(frame.coded_width)
            };
            let required = plane
                .offset
                .checked_add(u64::from(plane.pitch) * u64::from(rows.saturating_sub(1)))
                .and_then(|offset| offset.checked_add(row_bytes))
                .ok_or_else(|| format!("PRIME plane {index} object size overflow"))?;
            if required > object.size {
                return Err(format!(
                    "PRIME plane {index} requires {required} bytes but object {} has {}",
                    plane.obj_idx, object.size
                ));
            }
            Ok(())
        })
    }

    fn from_claimed(claimed: ClaimedVideoFrame, stream_id: u64) -> Result<Self, String> {
        let (frame, lease) = claimed.into_parts();
        let OwnedFrame {
            coded_width,
            coded_height,
            storage,
            acquire_sync,
            ..
        } = frame;
        let descriptor = match storage {
            OwnedStorage::DmaBuf(descriptor) => descriptor,
            _ => return Err("claimed frame has unsupported storage".to_string()),
        };
        let mut layers = descriptor.layers.into_iter();
        let Some(layer) = layers.next() else {
            return Err("claimed frame has no DMA-BUF layer".to_string());
        };
        if layers.next().is_some() {
            return Err("claimed frame has multiple DMA-BUF layers".to_string());
        }

        Ok(Self {
            width: coded_width,
            height: coded_height,
            format: layer.fourcc,
            objects: descriptor
                .objects
                .into_iter()
                .map(|object| PrimeObjectOwned {
                    fd: object.fd,
                    modifier: object.modifier.explicit(),
                })
                .collect(),
            planes: layer
                .planes
                .into_iter()
                .map(|plane| PrimePlaneDesc {
                    obj_idx: plane.object_index,
                    pitch: plane.pitch,
                    offset: plane.offset,
                })
                .collect(),
            acquire_fence: match acquire_sync {
                OwnedAcquireSync::Implicit => None,
                OwnedAcquireSync::SyncFile(fence) => Some(fence),
            },
            lease: Some(PrimeFrameLease::Canonical(lease)),
            stream_id: Some(stream_id),
            submitted_at: Instant::now(),
            stats: None,
            #[cfg(test)]
            drop_signal: None,
        })
    }
}

impl Drop for PrimeFrame {
    fn drop(&mut self) {
        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_lease_released(self.submitted_at.elapsed());
        }
        self.planes.clear();
        self.objects.clear();
        drop(self.acquire_fence.take());
        if let Some(lease) = self.lease.take() {
            match lease {
                PrimeFrameLease::Legacy(lease) => lease.release_from_native_thread(),
                PrimeFrameLease::Canonical(lease) => lease.retire(),
            }
        }
        #[cfg(test)]
        if let Some(signal) = self.drop_signal.take() {
            let _ = signal.send(());
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamModifierPolicy {
    PerBuffer,
    Implicit,
    Explicit(u64),
}

impl StreamModifierPolicy {
    pub fn validate_supported(self) -> Result<Self, String> {
        match self {
            Self::Explicit(modifier) if modifier != 0 => Err(format!(
                "unsupported negotiated DRM modifier {modifier:#018x}; only implicit and linear are supported"
            )),
            supported => Ok(supported),
        }
    }

    fn validate_frame(self, frame: &OwnedFrame) -> Result<(), String> {
        let descriptor = match &frame.storage {
            OwnedStorage::DmaBuf(descriptor) => descriptor,
            _ => return Err("unsupported canonical video storage".to_string()),
        };

        descriptor
            .objects
            .iter()
            .enumerate()
            .try_for_each(|(index, object)| match (self, object.modifier) {
                (Self::PerBuffer, Modifier::Implicit | Modifier::Explicit(0))
                | (Self::Implicit, Modifier::Implicit)
                | (Self::Explicit(0), Modifier::Explicit(0)) => Ok(()),
                (Self::PerBuffer, Modifier::Explicit(modifier)) => Err(format!(
                    "DMA-BUF object {index} has unsupported DRM modifier {modifier:#018x}; only implicit and linear are supported"
                )),
                (Self::Implicit, modifier) => Err(format!(
                    "DMA-BUF object {index} modifier {modifier:?} does not match negotiated implicit modifier policy"
                )),
                (Self::Explicit(expected), modifier) => Err(format!(
                    "DMA-BUF object {index} modifier {modifier:?} does not match negotiated DRM modifier {expected:#018x}"
                )),
            })
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveStream {
    id: u64,
    fourcc: u32,
    modifier_policy: StreamModifierPolicy,
}

struct VideoTargetEntry {
    spec: VideoTargetSpec,
    incarnation: u64,
    active: bool,
    active_stream: Option<ActiveStream>,
    pending: Option<PrimeFrame>,
}

#[derive(Clone, Debug)]
pub struct RegisteredVideoTargetSpec {
    pub spec: VideoTargetSpec,
    pub incarnation: u64,
    pub active: bool,
    pub active_stream: Option<u64>,
}

struct VideoRegistryState {
    open: bool,
    targets: HashMap<String, VideoTargetEntry>,
    active_scene_targets: HashSet<String>,
}

impl Default for VideoRegistryState {
    fn default() -> Self {
        Self {
            open: true,
            targets: HashMap::new(),
            active_scene_targets: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoSubmitResult {
    Queued,
    DroppedInactive,
}

#[derive(Debug)]
pub enum CanonicalSubmitError {
    CallerOwned(String),
    Transferred(String),
}

pub struct VideoRegistry {
    pub renderer_epoch: u64,
    admission_closed: std::sync::atomic::AtomicBool,
    state: Mutex<VideoRegistryState>,
    release_tx: Sender<PrimeFrame>,
    cleanup_dispatcher: CleanupDispatcher,
    generation: AtomicU64,
    next_incarnation: AtomicU64,
    next_stream_id: AtomicU64,
    stats: Option<Arc<RendererStatsCollector>>,
}

impl VideoRegistry {
    pub(crate) fn new(
        release_tx: Sender<PrimeFrame>,
        cleanup_dispatcher: CleanupDispatcher,
        stats: Option<Arc<RendererStatsCollector>>,
    ) -> Self {
        Self {
            renderer_epoch: NEXT_RENDERER_EPOCH.fetch_add(1, Ordering::Relaxed),
            admission_closed: std::sync::atomic::AtomicBool::new(false),
            state: Mutex::new(VideoRegistryState::default()),
            release_tx,
            cleanup_dispatcher,
            generation: AtomicU64::new(0),
            next_incarnation: AtomicU64::new(1),
            next_stream_id: AtomicU64::new(1),
            stats,
        }
    }

    pub fn create_target(&self, spec: VideoTargetSpec) -> Result<u64, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        if self.admission_closed.load(Ordering::Acquire) || !state.open {
            return Err("video registry is closed".to_string());
        }
        if state.targets.contains_key(&spec.id) {
            return Err(format!("video target already exists: {}", spec.id));
        }

        let active = state.active_scene_targets.contains(&spec.id);
        let incarnation = self.next_incarnation.fetch_add(1, Ordering::Relaxed);
        state.targets.insert(
            spec.id.clone(),
            VideoTargetEntry {
                spec,
                incarnation,
                active,
                active_stream: None,
                pending: None,
            },
        );
        self.bump_generation();
        Ok(incarnation)
    }

    pub fn remove_target(&self, id: &str, incarnation: u64) {
        let (removed, pending) = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            let exact = state
                .targets
                .get(id)
                .is_some_and(|entry| entry.incarnation == incarnation);
            if exact {
                let pending = state.targets.remove(id).and_then(|entry| entry.pending);
                (true, pending)
            } else {
                (false, None)
            }
        };
        if removed {
            self.bump_generation();
        }
        if let Some(frame) = pending {
            if let Some(stats) = self.stats.as_deref() {
                stats.record_video_pending_taken(1);
            }
            self.defer_release(frame);
        }
    }

    pub fn open_stream(
        &self,
        id: &str,
        incarnation: u64,
        width: u32,
        height: u32,
        fourcc: u32,
        modifier_policy: StreamModifierPolicy,
    ) -> Result<u64, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned".to_string())?;
        if self.admission_closed.load(Ordering::Acquire) || !state.open {
            return Err("video registry is closed".to_string());
        }
        let entry = state
            .targets
            .get_mut(id)
            .ok_or_else(|| format!("unknown video target: {id}"))?;
        if entry.incarnation != incarnation {
            return Err(format!("stale video target incarnation: {id}"));
        }
        validate_prime_target(
            id,
            entry.spec.mode,
            entry.spec.width,
            entry.spec.height,
            width,
            height,
            fourcc,
        )?;
        if entry.active_stream.is_some() {
            return Err("target_busy".to_string());
        }
        let modifier_policy = modifier_policy.validate_supported()?;
        let stream_id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        entry.active_stream = Some(ActiveStream {
            id: stream_id,
            fourcc,
            modifier_policy,
        });
        drop(state);
        self.bump_generation();
        Ok(stream_id)
    }

    pub fn close_stream(&self, id: &str, incarnation: u64, stream_id: u64) {
        let pending = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            state.targets.get_mut(id).and_then(|entry| {
                if entry.incarnation != incarnation
                    || !matches!(entry.active_stream, Some(active) if active.id == stream_id)
                {
                    return None;
                }
                entry.active_stream = None;
                Some(entry.pending.take())
            })
        };
        if let Some(pending) = pending {
            self.bump_generation();
            if let Some(frame) = pending {
                self.defer_release(frame);
            }
        }
    }

    pub fn close_admission(&self) {
        self.admission_closed.store(true, Ordering::Release);
    }

    pub fn close(&self) {
        self.close_admission();
        let pending = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            if !state.open {
                return;
            }
            state.open = false;
            state
                .targets
                .drain()
                .filter_map(|(_id, entry)| entry.pending)
                .collect::<Vec<_>>()
        };
        self.bump_generation();
        pending
            .into_iter()
            .for_each(|frame| self.defer_release(frame));
    }

    pub fn submit_canonical(
        &self,
        id: &str,
        incarnation: u64,
        stream_id: u64,
        prepared: PreparedVideoFrame,
    ) -> Result<VideoSubmitResult, CanonicalSubmitError> {
        let mut state = self.state.lock().map_err(|_| {
            CanonicalSubmitError::CallerOwned("video registry lock poisoned".into())
        })?;
        if self.admission_closed.load(Ordering::Acquire) || !state.open {
            return Err(CanonicalSubmitError::CallerOwned(
                "video registry is closed".to_string(),
            ));
        }
        let entry = state.targets.get_mut(id).ok_or_else(|| {
            CanonicalSubmitError::CallerOwned(format!("unknown video target: {id}"))
        })?;
        if entry.incarnation != incarnation {
            return Err(CanonicalSubmitError::CallerOwned(format!(
                "stale video target incarnation: {id}"
            )));
        }
        let active_stream = match entry.active_stream {
            Some(active) if active.id == stream_id => active,
            _ => {
                return Err(CanonicalSubmitError::CallerOwned(
                    "video consumer stream is closed or stale".to_string(),
                ));
            }
        };
        active_stream
            .modifier_policy
            .validate_frame(prepared.frame())
            .map_err(CanonicalSubmitError::CallerOwned)?;
        PrimeFrame::validate_canonical(
            id,
            entry.spec.mode,
            entry.spec.width,
            entry.spec.height,
            active_stream.fourcc,
            prepared.frame(),
        )
        .map_err(CanonicalSubmitError::CallerOwned)?;
        if !entry.active {
            if let Some(stats) = self.stats.as_deref() {
                stats.record_video_inactive_drop();
            }
            return Err(CanonicalSubmitError::CallerOwned(
                "video target is inactive".to_string(),
            ));
        }

        let claimed = prepared.claim();
        let mut frame = PrimeFrame::from_claimed(claimed, stream_id)
            .map_err(CanonicalSubmitError::Transferred)?;
        frame.submitted_at = Instant::now();
        frame.stats = self.stats.clone();
        let previous = entry.pending.replace(frame);
        drop(state);

        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_submitted(previous.is_some());
        }
        if let Some(previous) = previous {
            self.defer_release(previous);
        }
        self.bump_generation();
        Ok(VideoSubmitResult::Queued)
    }

    pub fn submit_prime(&self, id: &str, frame: PrimeFrame) -> Result<VideoSubmitResult, String> {
        let incarnation = {
            let state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned".to_string())?;
            state
                .targets
                .get(id)
                .map(|entry| entry.incarnation)
                .ok_or_else(|| format!("unknown video target: {id}"))?
        };
        self.submit_prime_exact(id, incarnation, frame)
    }

    pub fn submit_prime_exact(
        &self,
        id: &str,
        incarnation: u64,
        mut frame: PrimeFrame,
    ) -> Result<VideoSubmitResult, String> {
        let frame_width = frame.width;
        let frame_height = frame.height;
        let frame_format = frame.format;

        enum SubmitAction {
            Queue { previous: Option<PrimeFrame> },
            DropInactive(PrimeFrame),
        }

        let action = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => {
                    self.defer_release(frame);
                    return Err("video registry lock poisoned".to_string());
                }
            };
            if self.admission_closed.load(Ordering::Acquire) || !state.open {
                drop(state);
                self.defer_release(frame);
                return Err("video registry is closed".to_string());
            }
            let entry = match state.targets.get_mut(id) {
                Some(entry) if entry.incarnation == incarnation => entry,
                Some(_) => {
                    drop(state);
                    self.defer_release(frame);
                    return Err(format!("stale video target incarnation: {id}"));
                }
                None => {
                    drop(state);
                    self.defer_release(frame);
                    return Err(format!("unknown video target: {id}"));
                }
            };

            if entry.active_stream.is_some() {
                drop(state);
                self.defer_release(frame);
                return Err("video target has an active canonical consumer stream".to_string());
            }

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
            if entry.active {
                SubmitAction::Queue {
                    previous: entry.pending.replace(frame),
                }
            } else {
                SubmitAction::DropInactive(frame)
            }
        };

        match action {
            SubmitAction::Queue { previous } => {
                if let Some(stats) = self.stats.as_deref() {
                    stats.record_video_submitted(previous.is_some());
                }
                if let Some(previous) = previous {
                    self.defer_release(previous);
                }
                self.bump_generation();
                Ok(VideoSubmitResult::Queued)
            }
            SubmitAction::DropInactive(frame) => {
                if let Some(stats) = self.stats.as_deref() {
                    stats.record_video_inactive_drop();
                }
                self.defer_release(frame);
                Ok(VideoSubmitResult::DroppedInactive)
            }
        }
    }

    #[cfg(all(feature = "drm", target_os = "linux"))]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_active_targets(&self, active_targets: &HashSet<String>) -> Result<(), String> {
        let pending = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned".to_string())?;
            if state.active_scene_targets == *active_targets {
                return Ok(());
            }
            state.active_scene_targets = active_targets.clone();
            state
                .targets
                .iter_mut()
                .filter_map(|(id, entry)| {
                    let active = active_targets.contains(id);
                    let pending = (entry.active && !active)
                        .then(|| entry.pending.take())
                        .flatten();
                    entry.active = active;
                    pending
                })
                .collect::<Vec<_>>()
        };

        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_pending_taken(pending.len());
        }
        pending
            .into_iter()
            .for_each(|frame| self.defer_release(frame));
        Ok(())
    }

    pub fn snapshot_for_sync(
        &self,
        take_pending: bool,
    ) -> Result<VideoRegistrySyncSnapshot, String> {
        let (targets, pending) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned".to_string())?;
            let targets = state
                .targets
                .values()
                .map(|entry| RegisteredVideoTargetSpec {
                    spec: entry.spec.clone(),
                    incarnation: entry.incarnation,
                    active: entry.active,
                    active_stream: entry.active_stream.map(|active| active.id),
                })
                .collect::<Vec<_>>();
            let pending = state
                .targets
                .iter_mut()
                .filter_map(|(id, entry)| {
                    take_pending
                        .then(|| entry.pending.take())
                        .flatten()
                        .map(|frame| PendingVideoFrame {
                            id: id.clone(),
                            spec: entry.spec.clone(),
                            incarnation: entry.incarnation,
                            frame,
                        })
                })
                .collect::<Vec<_>>();
            (targets, pending)
        };

        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_pending_taken(pending.len());
        }
        Ok(VideoRegistrySyncSnapshot { targets, pending })
    }

    pub fn snapshot_pending(&self) -> Result<VideoRegistrySnapshot, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        let mut pending = Vec::new();

        for (id, entry) in &mut state.targets {
            if let Some(frame) = entry.pending.take() {
                pending.push(PendingVideoFrame {
                    id: id.clone(),
                    spec: entry.spec.clone(),
                    incarnation: entry.incarnation,
                    frame,
                });
            }
        }

        if let Some(stats) = self.stats.as_deref() {
            stats.record_video_pending_taken(pending.len());
        }

        Ok(VideoRegistrySnapshot { pending })
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

    fn record_import_gauges(&self, direct_imports: usize, retired_imports: usize) {
        if let Some(stats) = self.stats.as_deref() {
            stats.set_video_import_gauges(direct_imports, retired_imports);
        }
    }

    pub fn defer_release(&self, frame: PrimeFrame) {
        if let Err(error) = self.release_tx.send(frame) {
            let frame = error.into_inner();
            self.cleanup_dispatcher
                .dispatch(Box::new(move || drop(frame)));
        }
    }
}

pub struct VideoRegistrySnapshot {
    pub pending: Vec<PendingVideoFrame>,
}

pub struct VideoRegistrySyncSnapshot {
    pub targets: Vec<RegisteredVideoTargetSpec>,
    pub pending: Vec<PendingVideoFrame>,
}

pub struct PendingVideoFrame {
    pub id: String,
    pub spec: VideoTargetSpec,
    pub incarnation: u64,
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
    pub renderer_epoch: u64,
    pub incarnation: u64,
    pub _width: u32,
    pub _height: u32,
    pub _mode: VideoMode,
    pub registry: Arc<VideoRegistry>,
    pub wake: VideoWake,
    pub cleanup_dispatcher: CleanupDispatcher,
}

#[rustler::resource_impl]
impl rustler::Resource for VideoTargetResource {}

impl Drop for VideoTargetResource {
    fn drop(&mut self) {
        let registry = Arc::clone(&self.registry);
        let id = self.id.clone();
        let incarnation = self.incarnation;
        let wake = self.wake.clone();
        self.cleanup_dispatcher.dispatch(Box::new(move || {
            registry.remove_target(&id, incarnation);
            wake.notify();
        }));
    }
}

pub struct VideoConsumerSessionResource {
    pub id: String,
    pub renderer_epoch: u64,
    pub incarnation: u64,
    pub stream_id: u64,
    pub registry: Arc<VideoRegistry>,
    pub wake: VideoWake,
    cleanup_dispatcher: CleanupDispatcher,
    closed: std::sync::atomic::AtomicBool,
}

impl VideoConsumerSessionResource {
    pub fn new(target: &VideoTargetResource, stream_id: u64) -> Self {
        Self {
            id: target.id.clone(),
            renderer_epoch: target.renderer_epoch,
            incarnation: target.incarnation,
            stream_id,
            registry: Arc::clone(&target.registry),
            wake: target.wake.clone(),
            cleanup_dispatcher: target.cleanup_dispatcher.clone(),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.registry
                .close_stream(&self.id, self.incarnation, self.stream_id);
            self.wake.notify();
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

#[rustler::resource_impl]
impl rustler::Resource for VideoConsumerSessionResource {}

impl Drop for VideoConsumerSessionResource {
    fn drop(&mut self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let registry = Arc::clone(&self.registry);
            let id = self.id.clone();
            let incarnation = self.incarnation;
            let stream_id = self.stream_id;
            let wake = self.wake.clone();
            self.cleanup_dispatcher.dispatch(Box::new(move || {
                registry.close_stream(&id, incarnation, stream_id);
                wake.notify();
            }));
        }
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
        Self::new_current_with_path(VideoImportPath::BlitRgba)
    }

    #[cfg(all(feature = "drm", target_os = "linux"))]
    pub fn new_current_direct() -> Result<Self, String> {
        Self::new_current_with_path(VideoImportPath::DirectExternal)
    }

    fn path(&self) -> VideoImportPath {
        self.path
    }

    pub(crate) fn retry_acquire_cleanup(&self) -> bool {
        self.support.retry_deferred_sync_destroy()
    }

    pub(crate) fn has_acquire_cleanup(&self) -> bool {
        self.support.has_deferred_sync_destroy()
    }

    fn new_current_with_path(path: VideoImportPath) -> Result<Self, String> {
        let support = Rc::new(EglDmabufSupport::new_current()?);
        // DRM normally samples the external texture directly, but retain the blitter as a
        // one-way compatibility fallback when Ganesh cannot wrap an external texture.
        let blitter = Some(ExternalVideoBlitter::new()?);
        let use_gl_fences = gl::FenceSync::is_loaded()
            && gl::ClientWaitSync::is_loaded()
            && gl::DeleteSync::is_loaded();
        Ok(Self {
            support,
            blitter,
            use_gl_fences,
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

    pub(crate) fn retry_acquire_cleanup(&self) -> bool {
        false
    }

    pub(crate) fn has_acquire_cleanup(&self) -> bool {
        false
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

#[derive(Clone, Copy, Debug, Default)]
pub struct VideoCleanupResult {
    pub resources_changed: bool,
    pub needs_cleanup: bool,
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
    native_fence: Option<NativeFenceFunctions>,
    native_fence_capabilities: NativeFenceCapabilities,
    deferred_sync_destroy: RefCell<Vec<SyncHandle>>,
    acquire_received: Cell<u64>,
    acquire_server_queued: Cell<u64>,
    acquire_client_fallback: Cell<u64>,
    acquire_timeouts: Cell<u64>,
    acquire_errors: Cell<u64>,
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
        let native_fence_loader = unsafe {
            NativeFenceFunctions::load_with(|name| {
                let cname = CString::new(name).expect("EGL symbol");
                match lib.get::<*const c_void>(cname.as_bytes_with_nul()) {
                    Ok(symbol) => *symbol,
                    Err(_) => get_proc_address(cname.as_ptr()) as *const c_void,
                }
            })
        };
        let egl_extensions = egl_query_string(&egl, display, egl::EXTENSIONS as egl::types::EGLint);
        let egl_version = egl_query_string(&egl, display, egl::VERSION as egl::types::EGLint);
        let gl_extensions = gl_string(gl::EXTENSIONS);
        let gl_version = gl_string(gl::VERSION);
        let native_fence = native_fence_loader.select_consumer(
            &egl_extensions,
            &gl_extensions,
            egl_version_at_least_15(&egl_version),
            gl_supports_core_egl_sync(&gl_version, &gl_extensions),
        );
        let native_fence_capabilities = native_fence
            .as_ref()
            .map_or_else(NativeFenceCapabilities::default, |functions| {
                functions.capabilities()
            });

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
            native_fence,
            native_fence_capabilities,
            deferred_sync_destroy: RefCell::new(Vec::new()),
            acquire_received: Cell::new(0),
            acquire_server_queued: Cell::new(0),
            acquire_client_fallback: Cell::new(0),
            acquire_timeouts: Cell::new(0),
            acquire_errors: Cell::new(0),
        })
    }

    fn prepare_acquire(
        &self,
        frame: &mut PrimeFrame,
        cpu_diagnostic: bool,
    ) -> Result<bool, String> {
        let Some(fence) = frame.acquire_fence.take() else {
            return Ok(true);
        };
        let stats = frame.stats.clone();
        self.acquire_received
            .set(self.acquire_received.get().saturating_add(1));
        if let Some(stats) = stats.as_deref() {
            stats.record_video_acquire_fence_received();
        }

        if !self.native_fence_capabilities.consumer_import {
            return match poll_sync_file(&fence, Duration::from_secs(1)) {
                SyncFilePollOutcome::Signaled => Ok(true),
                SyncFilePollOutcome::Timeout => {
                    self.acquire_timeouts
                        .set(self.acquire_timeouts.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_timeout();
                    }
                    Err("video acquire sync-file poll timed out after 1s".to_string())
                }
                SyncFilePollOutcome::Error(error) => {
                    self.acquire_errors
                        .set(self.acquire_errors.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_error();
                    }
                    Err(format!("video acquire sync-file poll failed: {error}"))
                }
            };
        }

        let native_fence = self
            .native_fence
            .as_ref()
            .expect("consumer import capability requires a selected EGL sync ABI");
        let sync = unsafe { native_fence.import_sync_file(self.display.cast_mut().cast(), fence) }
            .map_err(|error| {
                self.acquire_errors
                    .set(self.acquire_errors.get().saturating_add(1));
                if let Some(stats) = stats.as_deref() {
                    stats.record_video_acquire_wait_error();
                }
                format!("video acquire fence import failed: {error}")
            })?;

        let server_queued = if self.native_fence_capabilities.server_wait {
            match unsafe { native_fence.wait_server(&sync) } {
                ServerWaitOutcome::Queued => {
                    self.acquire_server_queued
                        .set(self.acquire_server_queued.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_server_wait_queued();
                    }
                    true
                }
                ServerWaitOutcome::Unsupported => false,
                ServerWaitOutcome::Failed { egl_error } => {
                    self.acquire_errors
                        .set(self.acquire_errors.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_error();
                    }
                    eprintln!(
                        "video acquire server wait failed with EGL error {egl_error:#x}; using bounded client wait"
                    );
                    false
                }
            }
        } else {
            false
        };

        if !server_queued {
            self.acquire_client_fallback
                .set(self.acquire_client_fallback.get().saturating_add(1));
            if let Some(stats) = stats.as_deref() {
                stats.record_video_acquire_client_wait_fallback();
            }
            match unsafe { native_fence.wait_client(&sync, Duration::from_secs(1)) } {
                ClientWaitOutcome::Satisfied => {}
                ClientWaitOutcome::Timeout => {
                    self.acquire_timeouts
                        .set(self.acquire_timeouts.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_timeout();
                    }
                    self.destroy_or_defer(sync);
                    return Err("video acquire EGL client wait timed out after 1s".to_string());
                }
                outcome => {
                    self.acquire_errors
                        .set(self.acquire_errors.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_error();
                    }
                    self.destroy_or_defer(sync);
                    return Err(format!("video acquire EGL client wait failed: {outcome:?}"));
                }
            }
        }

        let cpu_ready = if cpu_diagnostic && server_queued {
            match unsafe { native_fence.wait_client(&sync, Duration::from_secs(1)) } {
                ClientWaitOutcome::Satisfied => true,
                ClientWaitOutcome::Timeout => {
                    self.acquire_timeouts
                        .set(self.acquire_timeouts.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_timeout();
                    }
                    false
                }
                _ => {
                    self.acquire_errors
                        .set(self.acquire_errors.get().saturating_add(1));
                    if let Some(stats) = stats.as_deref() {
                        stats.record_video_acquire_wait_error();
                    }
                    false
                }
            }
        } else {
            true
        };
        self.destroy_or_defer(sync);
        Ok(cpu_ready)
    }

    fn destroy_or_defer(&self, sync: SyncHandle) {
        let native_fence = self
            .native_fence
            .as_ref()
            .expect("an imported EGL sync requires a selected ABI");
        if let Err(error) = unsafe { native_fence.destroy(sync) } {
            eprintln!("video acquire EGL sync destroy deferred: {error}");
            self.deferred_sync_destroy.borrow_mut().push(error.handle);
        }
    }

    fn retry_deferred_sync_destroy(&self) -> bool {
        let pending = self.deferred_sync_destroy.replace(Vec::new());
        let native_fence = self
            .native_fence
            .as_ref()
            .expect("deferred EGL syncs require a selected ABI");
        let retained = pending
            .into_iter()
            .filter_map(|sync| unsafe { native_fence.destroy(sync) }.err())
            .map(|error| error.handle)
            .collect::<Vec<_>>();
        let needs_cleanup = !retained.is_empty();
        self.deferred_sync_destroy.replace(retained);
        needs_cleanup
    }

    fn has_deferred_sync_destroy(&self) -> bool {
        !self.deferred_sync_destroy.borrow().is_empty()
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
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl Drop for EglDmabufSupport {
    fn drop(&mut self) {
        let pending = self.deferred_sync_destroy.get_mut();
        let unresolved = self
            .native_fence
            .as_ref()
            .map_or(pending.len(), |native_fence| {
                std::mem::take(pending)
                    .into_iter()
                    .filter_map(|sync| unsafe { native_fence.destroy(sync) }.err())
                    .count()
            });
        eprintln!(
            "video acquire sync counters: received={} server_queued={} client_fallback={} timeouts={} errors={} deferred_to_display_teardown={}",
            self.acquire_received.get(),
            self.acquire_server_queued.get(),
            self.acquire_client_fallback.get(),
            self.acquire_timeouts.get(),
            self.acquire_errors.get(),
            unresolved
        );
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn egl_query_string(
    egl: &egl::Egl,
    display: egl::types::EGLDisplay,
    name: egl::types::EGLint,
) -> String {
    let value = unsafe { egl.QueryString(display, name) };
    if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn gl_string(name: u32) -> String {
    let value = unsafe { gl::GetString(name) };
    if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value.cast()) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn gl_supports_core_egl_sync(version: &str, extensions: &str) -> bool {
    if has_extension(extensions, "GL_ARB_sync") || has_extension(extensions, "GL_OES_EGL_sync") {
        return true;
    }

    let is_es = version.starts_with("OpenGL ES");
    let number = version
        .split_ascii_whitespace()
        .find(|token| token.as_bytes().first().is_some_and(u8::is_ascii_digit));
    let parsed = number
        .and_then(|number| number.split_once('.'))
        .and_then(|(major, minor)| {
            let minor = minor
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?))
        });

    parsed.is_some_and(|(major, minor)| {
        if is_es {
            major >= 3
        } else {
            major > 3 || (major == 3 && minor >= 2)
        }
    })
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn egl_version_at_least_15(version: &str) -> bool {
    version
        .split_ascii_whitespace()
        .next()
        .and_then(|number| number.split_once('.'))
        .and_then(|(major, minor)| Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?)))
        .is_some_and(|(major, minor)| major > 1 || (major == 1 && minor >= 5))
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn should_sample_luma(diagnostic_requested: bool, acquire_cpu_ready: bool) -> bool {
    diagnostic_requested && acquire_cpu_ready
}

struct RenderedVideoTarget {
    spec: VideoTargetSpec,
    incarnation: u64,
    stream_id: Option<u64>,
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
        incarnation: u64,
        stream_id: Option<u64>,
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
            incarnation,
            stream_id,
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
        mut frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        let wants_luma = self.diagnostics_pending && frame.format == DRM_FORMAT_NV12;
        let cpu_ready = ctx.support.prepare_acquire(&mut frame, wants_luma)?;
        let luma_sample = should_sample_luma(wants_luma, cpu_ready).then(|| frame.sample_luma());
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
        mut frame: PrimeFrame,
        ctx: &VideoImportContext,
        gr_context: &mut gpu::DirectContext,
    ) -> Result<Option<String>, String> {
        let wants_luma = self.diagnostics_pending && frame.format == DRM_FORMAT_NV12;
        let cpu_ready = ctx.support.prepare_acquire(&mut frame, wants_luma)?;
        let luma_sample = should_sample_luma(wants_luma, cpu_ready).then(|| frame.sample_luma());
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

    fn reap_retired_imports(&mut self) -> VideoCleanupResult {
        let retired_count = self.retired_imports.len();
        let resources_changed = (0..retired_count).fold(false, |resources_changed, _| {
            let retired = self
                .retired_imports
                .pop_front()
                .expect("retired imports length changed during poll");
            match retired.poll() {
                Ok(RetiredImportPoll::Pending) => {
                    self.retired_imports.push_back(retired);
                    resources_changed
                }
                Ok(RetiredImportPoll::Released) => {
                    retired.wait_blocking(&self.spec.id);
                    true
                }
                Err(RetiredImportPollError::WaitFailed) => {
                    eprintln!(
                        "video sync failed: glClientWaitSync WAIT_FAILED for target={}; forcing blocking cleanup",
                        self.spec.id
                    );
                    retired.wait_blocking(&self.spec.id);
                    true
                }
                Err(RetiredImportPollError::UnexpectedStatus(status)) => {
                    eprintln!(
                        "video sync failed: glClientWaitSync returned unexpected status={status:#x} for target={}; forcing blocking cleanup",
                        self.spec.id
                    );
                    retired.wait_blocking(&self.spec.id);
                    true
                }
            }
        });

        VideoCleanupResult {
            resources_changed,
            needs_cleanup: !self.retired_imports.is_empty(),
        }
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

fn rendered_target_matches_registration(
    incarnation: u64,
    stream_id: Option<u64>,
    registered: Option<(u64, Option<u64>)>,
) -> bool {
    registered == Some((incarnation, stream_id))
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
        let initial_cleanup = self.reap_retired_imports(registry);
        let mut needs_cleanup = initial_cleanup.needs_cleanup;
        if let Some(ctx) = ctx {
            needs_cleanup |= ctx.retry_acquire_cleanup();
        }

        let snapshot = registry.snapshot_for_sync(ctx.is_some())?;
        let import_path = ctx
            .map(VideoImportContext::path)
            .unwrap_or(VideoImportPath::BlitRgba);
        let registered = snapshot
            .targets
            .iter()
            .map(|target| {
                (
                    target.spec.id.clone(),
                    (target.incarnation, target.active_stream),
                )
            })
            .collect::<HashMap<_, _>>();
        let before = self.targets.len();
        self.targets.retain(|id, target| {
            rendered_target_matches_registration(
                target.incarnation,
                target.stream_id,
                registered.get(id).copied(),
            )
        });
        let mut resources_changed =
            initial_cleanup.resources_changed || self.targets.len() != before;

        for target in snapshot.targets.iter().filter(|target| target.active) {
            let id = &target.spec.id;
            if !self.targets.contains_key(id) {
                self.targets.insert(
                    id.clone(),
                    RenderedVideoTarget::new(
                        target.spec.clone(),
                        target.incarnation,
                        target.active_stream,
                        gr_context,
                        import_path,
                    )?,
                );
                resources_changed = true;
            }
        }

        let mut imported_frames = 0;
        let mut newest_import_submitted_at = None;
        let mut first_frame_diagnostics = None;
        if let Some(ctx) = ctx {
            for pending in snapshot.pending {
                let target = self.targets.get_mut(&pending.id).ok_or_else(|| {
                    format!("video target disappeared during sync: {}", pending.id)
                })?;
                if target.incarnation != pending.incarnation {
                    return Err(format!(
                        "video target incarnation changed during sync: {}",
                        pending.id
                    ));
                }
                if target.stream_id != pending.frame.stream_id {
                    return Err(format!(
                        "video consumer stream changed during sync: {}",
                        pending.id
                    ));
                }
                let submitted_at = pending.frame.submitted_at;
                let diagnostics = match target.upload_frame(pending.frame, ctx, gr_context) {
                    Ok(diagnostics) => diagnostics,
                    Err(error) => {
                        eprintln!(
                            "video frame import dropped for target={}: {error}",
                            pending.id
                        );
                        needs_cleanup |= ctx.has_acquire_cleanup();
                        continue;
                    }
                };
                if let Some(diagnostics) = diagnostics {
                    first_frame_diagnostics.get_or_insert(diagnostics);
                }
                imported_frames += 1;
                newest_import_submitted_at = Some(
                    newest_import_submitted_at
                        .map(|current: Instant| current.max(submitted_at))
                        .unwrap_or(submitted_at),
                );
                resources_changed = true;
                let cleanup = target.reap_retired_imports();
                resources_changed |= cleanup.resources_changed;
                needs_cleanup |= cleanup.needs_cleanup;
            }
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
        if let Some(ctx) = ctx {
            needs_cleanup |= ctx.has_acquire_cleanup();
        }

        Ok(VideoSyncResult {
            resources_changed,
            needs_cleanup,
            imported_frames,
            newest_import_submitted_at,
            first_frame_diagnostics,
        })
    }

    pub fn reap_retired_imports(&mut self, registry: &Arc<VideoRegistry>) -> VideoCleanupResult {
        let cleanup = self
            .targets
            .values_mut()
            .map(RenderedVideoTarget::reap_retired_imports)
            .fold(VideoCleanupResult::default(), |combined, target| {
                VideoCleanupResult {
                    resources_changed: combined.resources_changed || target.resources_changed,
                    needs_cleanup: combined.needs_cleanup || target.needs_cleanup,
                }
            });
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
        cleanup
    }

    pub fn image(&self, id: &str) -> Option<(&Image, u32, u32)> {
        self.targets.get(id).and_then(RenderedVideoTarget::image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::RendererTimingMetric;
    use crossbeam_channel::{Receiver, bounded, unbounded};
    use std::fs::File;
    use std::sync::OnceLock;
    use std::time::Duration;
    use video_interop::{Layer, OwnedDescriptor, OwnedObject, Plane, Rect};

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    #[test]
    fn core_egl_sync_checks_current_gl_api_capability() {
        assert!(gl_supports_core_egl_sync("4.6 (Core Profile) Mesa", ""));
        assert!(gl_supports_core_egl_sync("OpenGL ES 3.0 Mesa", ""));
        assert!(gl_supports_core_egl_sync("2.1 Mesa", "GL_ARB_sync"));
        assert!(gl_supports_core_egl_sync(
            "OpenGL ES 2.0",
            "GL_OES_EGL_sync"
        ));
        assert!(!gl_supports_core_egl_sync("3.1 Mesa", ""));
        assert!(!gl_supports_core_egl_sync("OpenGL ES 2.0", ""));
        assert!(!gl_supports_core_egl_sync("unknown", ""));
    }

    fn test_cleanup_dispatcher() -> CleanupDispatcher {
        static DISPATCHER: OnceLock<CleanupDispatcher> = OnceLock::new();
        DISPATCHER
            .get_or_init(|| CleanupDispatcher::start().expect("start test cleanup dispatcher"))
            .clone()
    }

    fn test_registry(
        release_tx: Sender<PrimeFrame>,
        stats: Option<Arc<RendererStatsCollector>>,
    ) -> VideoRegistry {
        VideoRegistry::new(release_tx, test_cleanup_dispatcher(), stats)
    }

    fn frame_with_drop_signal(signal: Sender<()>) -> PrimeFrame {
        let mut frame = test_prime_frame(64, 32);
        frame.drop_signal = Some(signal);
        frame
    }

    fn assert_dropped_exactly_once(signal: &Receiver<()>) {
        signal
            .recv_timeout(Duration::from_secs(2))
            .expect("frame should drop on a cleanup worker");
        assert!(
            signal.recv_timeout(Duration::from_millis(50)).is_err(),
            "frame must not drop twice"
        );
    }

    fn test_prime_frame(width: u32, height: u32) -> PrimeFrame {
        test_prime_frame_with_format(width, height, DRM_FORMAT_NV12)
    }

    fn activate_target(registry: &VideoRegistry, id: &str) {
        registry
            .set_active_targets(&HashSet::from([id.to_string()]))
            .expect("target should become active");
    }

    fn canonical_owned_frame(width: u32, height: u32, format: u32) -> OwnedFrame {
        let fd: OwnedFd = File::open("/dev/null").expect("open /dev/null").into();
        let (size, planes) = if format == DRM_FORMAT_NV12 {
            let luma_size = u64::from(width) * u64::from(height);
            (
                luma_size + luma_size / 2,
                vec![
                    Plane {
                        object_index: 0,
                        offset: 0,
                        pitch: width,
                    },
                    Plane {
                        object_index: 0,
                        offset: luma_size,
                        pitch: width,
                    },
                ],
            )
        } else {
            (
                u64::from(width) * u64::from(height) * 4,
                vec![Plane {
                    object_index: 0,
                    offset: 0,
                    pitch: width * 4,
                }],
            )
        };

        OwnedFrame {
            coded_width: width,
            coded_height: height,
            visible_rect: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            storage: OwnedStorage::DmaBuf(OwnedDescriptor {
                version: 1,
                objects: vec![OwnedObject {
                    fd,
                    size,
                    modifier: video_interop::Modifier::Implicit,
                }],
                layers: vec![Layer {
                    fourcc: format,
                    planes,
                }],
            }),
            acquire_sync: OwnedAcquireSync::Implicit,
        }
    }

    fn test_prime_frame_with_format(width: u32, height: u32, format: u32) -> PrimeFrame {
        PrimeFrame {
            width,
            height,
            format,
            objects: Vec::new(),
            planes: Vec::new(),
            acquire_fence: None,
            lease: None,
            stream_id: None,
            submitted_at: Instant::now(),
            stats: None,
            drop_signal: None,
        }
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    ))]
    #[test]
    fn luma_diagnostic_requires_cpu_acquire_completion() {
        assert!(should_sample_luma(true, true));
        assert!(!should_sample_luma(true, false));
        assert!(!should_sample_luma(false, true));
    }

    #[test]
    fn unimported_prime_frame_closes_owned_acquire_fence() {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        let raw = fds[0];
        let mut frame = test_prime_frame(64, 32);
        frame.acquire_fence = Some(unsafe { OwnedFd::from_raw_fd(raw) });
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };

        drop(frame);
        assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
        assert!(write.as_raw_fd() >= 0);
    }

    #[test]
    fn canonical_abgr8888_and_nv12_owned_frames_validate_for_target() {
        for format in [DRM_FORMAT_ABGR8888, DRM_FORMAT_NV12] {
            PrimeFrame::validate_canonical(
                "preview",
                VideoMode::Prime,
                64,
                32,
                format,
                &canonical_owned_frame(64, 32, format),
            )
            .expect("canonical owned frame should validate");
        }
    }

    #[test]
    fn stream_modifier_policy_accepts_only_exact_implicit_or_linear_objects() {
        let mut frame = canonical_owned_frame(64, 32, DRM_FORMAT_ABGR8888);

        StreamModifierPolicy::PerBuffer
            .validate_frame(&frame)
            .expect("per-buffer policy should accept implicit objects");
        StreamModifierPolicy::Implicit
            .validate_frame(&frame)
            .expect("implicit policy should accept implicit objects");
        assert!(
            StreamModifierPolicy::Explicit(0)
                .validate_frame(&frame)
                .unwrap_err()
                .contains("does not match negotiated DRM modifier")
        );

        let OwnedStorage::DmaBuf(descriptor) = &mut frame.storage else {
            panic!("test frame must use DMA-BUF storage");
        };
        descriptor.objects[0].modifier = Modifier::Explicit(0);

        StreamModifierPolicy::PerBuffer
            .validate_frame(&frame)
            .expect("per-buffer policy should accept explicit linear objects");
        StreamModifierPolicy::Explicit(0)
            .validate_frame(&frame)
            .expect("linear policy should accept explicit linear objects");
        assert!(
            StreamModifierPolicy::Implicit
                .validate_frame(&frame)
                .unwrap_err()
                .contains("does not match negotiated implicit modifier policy")
        );

        let OwnedStorage::DmaBuf(descriptor) = &mut frame.storage else {
            panic!("test frame must use DMA-BUF storage");
        };
        descriptor.objects[0].modifier = Modifier::Explicit(1);
        assert!(
            StreamModifierPolicy::PerBuffer
                .validate_frame(&frame)
                .unwrap_err()
                .contains("unsupported DRM modifier 0x0000000000000001")
        );
    }

    #[test]
    fn unsupported_negotiated_modifier_does_not_open_stream() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        assert!(
            registry
                .open_stream(
                    "preview",
                    incarnation,
                    64,
                    32,
                    DRM_FORMAT_ABGR8888,
                    StreamModifierPolicy::Explicit(1),
                )
                .unwrap_err()
                .contains("unsupported negotiated DRM modifier 0x0000000000000001")
        );
        assert_eq!(
            registry
                .snapshot_for_sync(false)
                .expect("registry snapshot")
                .targets[0]
                .active_stream,
            None
        );
    }

    #[test]
    fn canonical_validation_accepts_explicit_sync_and_rejects_partial_visible_rect() {
        let mut explicit = canonical_owned_frame(64, 32, DRM_FORMAT_ABGR8888);
        explicit.acquire_sync =
            OwnedAcquireSync::SyncFile(File::open("/dev/null").expect("open sync fd").into());
        PrimeFrame::validate_canonical(
            "preview",
            VideoMode::Prime,
            64,
            32,
            DRM_FORMAT_ABGR8888,
            &explicit,
        )
        .expect("explicit acquire sync should validate before render-thread wait");

        let mut undersized = canonical_owned_frame(64, 32, DRM_FORMAT_ABGR8888);
        let OwnedStorage::DmaBuf(descriptor) = &mut undersized.storage else {
            panic!("test frame must use DMA-BUF storage");
        };
        descriptor.objects[0].size -= 1;
        assert!(
            PrimeFrame::validate_canonical(
                "preview",
                VideoMode::Prime,
                64,
                32,
                DRM_FORMAT_ABGR8888,
                &undersized,
            )
            .unwrap_err()
            .contains("requires")
        );

        let mut partial = canonical_owned_frame(64, 32, DRM_FORMAT_ABGR8888);
        partial.visible_rect.width = 63;
        assert!(
            PrimeFrame::validate_canonical(
                "preview",
                VideoMode::Prime,
                64,
                32,
                DRM_FORMAT_ABGR8888,
                &partial,
            )
            .unwrap_err()
            .contains("full-frame visible rectangle")
        );
    }

    #[test]
    fn prime_target_accepts_headless_abgr8888_output() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");

        registry
            .submit_prime(
                "preview",
                test_prime_frame_with_format(64, 32, DRM_FORMAT_ABGR8888),
            )
            .expect("ABGR8888 frame should be accepted");
    }

    #[test]
    fn headless_abgr8888_descriptor_layout_is_valid() {
        validate_prime_descriptor_layout(
            64,
            DRM_FORMAT_ABGR8888,
            1,
            &[PrimePlane {
                obj_idx: 0,
                pitch: 256,
                offset: 0,
            }],
        )
        .expect("single-plane ABGR8888 descriptor should be valid");
    }

    #[test]
    fn prime_descriptor_layout_rejects_missing_or_invalid_planes() {
        let missing = validate_prime_descriptor_layout(64, DRM_FORMAT_ABGR8888, 1, &[])
            .expect_err("ABGR8888 requires one plane");
        assert!(missing.contains("requires 1 plane"));

        let invalid_object = validate_prime_descriptor_layout(
            64,
            DRM_FORMAT_ABGR8888,
            1,
            &[PrimePlane {
                obj_idx: 1,
                pitch: 256,
                offset: 0,
            }],
        )
        .expect_err("plane object index must be in range");
        assert!(invalid_object.contains("references object 1"));

        let short_pitch = validate_prime_descriptor_layout(
            64,
            DRM_FORMAT_ABGR8888,
            1,
            &[PrimePlane {
                obj_idx: 0,
                pitch: 64,
                offset: 0,
            }],
        )
        .expect_err("ABGR8888 pitch must cover one row");
        assert!(short_pitch.contains("smaller than required 256"));
    }

    #[test]
    fn prime_target_rejects_unknown_drm_format() {
        let error = validate_prime_target(
            "preview",
            VideoMode::Prime,
            64,
            32,
            64,
            32,
            fourcc(b'B', b'A', b'D', b'!'),
        )
        .expect_err("unknown format should be rejected");

        assert!(error.contains("supported formats are NV12 and ABGR8888"));
    }

    #[test]
    fn target_created_after_visible_scene_starts_active() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_active_targets(&HashSet::from(["preview".to_string()]))
            .expect("scene visibility should be stored");
        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        assert_eq!(
            registry
                .submit_prime("preview", test_prime_frame(64, 32))
                .expect("visible target should queue immediately"),
            VideoSubmitResult::Queued
        );
    }

    #[test]
    fn recreated_target_gets_a_new_incarnation() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let spec = VideoTargetSpec {
            id: "preview".to_string(),
            width: 64,
            height: 32,
            mode: VideoMode::Prime,
        };

        registry
            .create_target(spec.clone())
            .expect("first target should be created");
        let first = registry
            .snapshot_for_sync(false)
            .expect("first target should be visible to sync")
            .targets[0]
            .incarnation;
        registry.remove_target("preview", first);
        registry
            .create_target(spec)
            .expect("replacement target should be created");
        let second = registry
            .snapshot_for_sync(false)
            .expect("second target should be visible to sync")
            .targets[0]
            .incarnation;

        assert_ne!(first, second);
    }

    #[test]
    fn inactive_target_drops_frames_without_queueing_them() {
        let (release_tx, release_rx) = unbounded();
        let stats = Arc::new(RendererStatsCollector::new());
        let registry = test_registry(release_tx, Some(Arc::clone(&stats)));

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        assert_eq!(
            registry
                .submit_prime("preview", test_prime_frame(64, 32))
                .expect("inactive submission should be dropped cleanly"),
            VideoSubmitResult::DroppedInactive
        );
        assert!(
            registry
                .snapshot_pending()
                .expect("snapshot should succeed")
                .pending
                .is_empty()
        );
        drop(
            release_rx
                .try_recv()
                .expect("expected inactive frame release"),
        );

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.video_pipeline.submitted, 0);
        assert_eq!(snapshot.video_pipeline.inactive_dropped, 1);
        assert_eq!(snapshot.video_pipeline.leases_released, 1);
    }

    #[test]
    fn deactivating_target_drains_its_pending_frame() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");
        assert_eq!(
            registry
                .submit_prime("preview", test_prime_frame(64, 32))
                .expect("active submission should queue"),
            VideoSubmitResult::Queued
        );

        registry
            .set_active_targets(&HashSet::new())
            .expect("target should become inactive");

        assert!(
            registry
                .snapshot_for_sync(false)
                .expect("target specs should be readable")
                .targets
                .iter()
                .all(|target| !target.active)
        );
        drop(
            release_rx
                .try_recv()
                .expect("expected pending frame release"),
        );
    }

    #[test]
    fn drain_pending_to_release_moves_pending_frames_to_release_queue() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");

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
        let registry = test_registry(release_tx, None);

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
        let registry = test_registry(release_tx, Some(Arc::clone(&stats)));

        registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");
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
    fn exact_target_identity_rejects_stale_submit_and_removal() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let spec = VideoTargetSpec {
            id: "preview".to_string(),
            width: 64,
            height: 32,
            mode: VideoMode::Prime,
        };
        let first = registry
            .create_target(spec.clone())
            .expect("first target should be created");
        registry.remove_target("preview", first);
        let second = registry
            .create_target(spec)
            .expect("replacement target should be created");
        activate_target(&registry, "preview");

        let error = registry
            .submit_prime_exact("preview", first, test_prime_frame(64, 32))
            .expect_err("stale target must be rejected");
        assert!(error.contains("stale video target incarnation"));
        drop(release_rx.try_recv().expect("stale frame should retire"));

        registry.remove_target("preview", first);
        assert_eq!(registry.target_spec("preview").unwrap().width, 64);
        assert_ne!(first, second);
    }

    #[test]
    fn closed_stream_no_longer_matches_final_rendered_claim() {
        assert!(rendered_target_matches_registration(
            7,
            Some(11),
            Some((7, Some(11)))
        ));
        assert!(!rendered_target_matches_registration(
            7,
            Some(11),
            Some((7, None))
        ));
        assert!(!rendered_target_matches_registration(
            7,
            Some(11),
            Some((8, Some(11)))
        ));
    }

    #[test]
    fn registries_have_distinct_renderer_epochs() {
        let (first_tx, _first_rx) = unbounded();
        let (second_tx, _second_rx) = unbounded();
        let first = test_registry(first_tx, None);
        let second = test_registry(second_tx, None);

        assert_ne!(first.renderer_epoch, second.renderer_epoch);
    }

    #[test]
    fn registry_close_rejects_targets_and_streams() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        registry.close();
        assert_eq!(
            registry
                .open_stream(
                    "preview",
                    incarnation,
                    64,
                    32,
                    DRM_FORMAT_NV12,
                    StreamModifierPolicy::PerBuffer,
                )
                .unwrap_err(),
            "video registry is closed"
        );
        assert!(
            registry
                .create_target(VideoTargetSpec {
                    id: "other".to_string(),
                    width: 64,
                    height: 32,
                    mode: VideoMode::Prime,
                })
                .unwrap_err()
                .contains("closed")
        );
    }

    #[test]
    fn disconnected_release_channel_drops_frame_once_on_persistent_cleanup_worker() {
        let (release_tx, release_rx) = unbounded();
        drop(release_rx);
        let registry = test_registry(release_tx, None);
        let (drop_tx, drop_rx) = bounded(2);

        registry.defer_release(frame_with_drop_signal(drop_tx));

        assert_dropped_exactly_once(&drop_rx);
    }

    #[test]
    fn stream_close_is_idempotent_and_retires_pending_frame() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");
        let stream_id = registry
            .open_stream(
                "preview",
                incarnation,
                64,
                32,
                DRM_FORMAT_NV12,
                StreamModifierPolicy::PerBuffer,
            )
            .expect("stream should open");
        let mut frame = test_prime_frame(64, 32);
        frame.stream_id = Some(stream_id);
        registry
            .state
            .lock()
            .expect("registry state should lock")
            .targets
            .get_mut("preview")
            .expect("target should exist")
            .pending = Some(frame);

        registry.close_stream("preview", incarnation, stream_id);
        registry.close_stream("preview", incarnation, stream_id);
        let retired = release_rx.try_recv().expect("pending frame should retire");
        assert_eq!(retired.stream_id, Some(stream_id));
        assert!(release_rx.try_recv().is_err());
        assert!(
            registry
                .open_stream(
                    "preview",
                    incarnation,
                    64,
                    32,
                    DRM_FORMAT_NV12,
                    StreamModifierPolicy::PerBuffer,
                )
                .is_ok()
        );
    }

    #[test]
    fn consumer_session_close_then_drop_retires_pending_frame_exactly_once() {
        let (release_tx, release_rx) = unbounded();
        drop(release_rx);
        let cleanup_dispatcher = test_cleanup_dispatcher();
        let registry = Arc::new(VideoRegistry::new(
            release_tx,
            cleanup_dispatcher.clone(),
            None,
        ));
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");
        let stream_id = registry
            .open_stream(
                "preview",
                incarnation,
                64,
                32,
                DRM_FORMAT_NV12,
                StreamModifierPolicy::PerBuffer,
            )
            .expect("stream should open");
        let target = VideoTargetResource {
            id: "preview".to_string(),
            renderer_epoch: registry.renderer_epoch,
            incarnation,
            _width: 64,
            _height: 32,
            _mode: VideoMode::Prime,
            registry: Arc::clone(&registry),
            wake: VideoWake::noop(),
            cleanup_dispatcher,
        };
        let session = VideoConsumerSessionResource::new(&target, stream_id);
        let (drop_tx, drop_rx) = bounded(2);
        registry
            .state
            .lock()
            .expect("registry state should lock")
            .targets
            .get_mut("preview")
            .expect("target should exist")
            .pending = Some(frame_with_drop_signal(drop_tx));

        session.close();
        session.close();
        drop(session);

        assert_dropped_exactly_once(&drop_rx);
        assert!(
            registry
                .state
                .lock()
                .expect("registry state should lock")
                .targets
                .get("preview")
                .expect("target should exist")
                .active_stream
                .is_none()
        );
        drop(target);
    }

    #[test]
    fn deprecated_raw_submit_rejects_an_active_canonical_stream() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        registry
            .open_stream(
                "preview",
                incarnation,
                64,
                32,
                DRM_FORMAT_NV12,
                StreamModifierPolicy::PerBuffer,
            )
            .expect("canonical stream should open");

        assert!(
            registry
                .submit_prime_exact("preview", incarnation, test_prime_frame(64, 32))
                .unwrap_err()
                .contains("active canonical consumer stream")
        );
        drop(release_rx.try_recv().expect("raw frame should retire"));
    }

    #[test]
    fn submit_prime_releases_rejected_frame_to_release_queue() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);

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
