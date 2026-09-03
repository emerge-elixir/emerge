#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::cell::{Cell, RefCell};
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux"),
    all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    )
))]
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
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
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::ptr;
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use std::rc::Rc;
#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
use std::sync::OnceLock;
#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
use std::sync::atomic::AtomicBool;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
#[cfg(any(feature = "wayland", feature = "drm", feature = "vulkan"))]
use std::time::Duration;
use std::time::Instant;

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
use ash::vk;
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
#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core"),
    not(any(feature = "wayland", feature = "drm"))
))]
use skia_safe::{AlphaType, ColorType, gpu::SurfaceOrigin};
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use skia_safe::{
    AlphaType, ColorType,
    gpu::{Mipmapped, Protected, SurfaceOrigin, gl::TextureInfo},
};
#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
use skia_safe::{
    Data, FilterMode, MipmapMode, RuntimeEffect, SamplingOptions, Shader, TileMode,
    runtime_effect::ChildPtr,
};
use skia_safe::{Image, gpu};
#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
use video_interop::egl::{
    ClientWaitOutcome, NativeFenceCapabilities, NativeFenceFunctions, ServerWaitOutcome,
    SyncFilePollOutcome, SyncHandle, has_extension, poll_sync_file,
};
use video_interop::{
    AcquireSyncPolicy as InteropAcquireSyncPolicy, AlphaMode as InteropAlphaMode, ClaimedLease,
    ClaimedVideoFrame, Colorimetry, Format as InteropFormat, InterlaceMode as InteropInterlaceMode,
    Modifier, ModifierPolicy as InteropModifierPolicy, OwnedAcquireSync, OwnedFrame, OwnedStorage,
    PreparedVideoFrame, StorageFormat as InteropStorageFormat,
};

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
use crate::stats::VulkanVideoImportPoolStats;
use crate::{CleanupDispatcher, backend::wake::BackendWakeHandle, stats::RendererStatsCollector};
#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
use crate::{
    backend::vulkan::{
        DRM_FORMAT_MOD_LINEAR, ImportedDmaBufImage, ImportedImageSync, ImportedImageSyncError,
        ImportedImageSyncErrorKind, ImportedPlane, InteropVulkanDmaBufImporter,
        Nv12AllocationBindingRecipe, Nv12Conversion, Nv12FrameTopology, Nv12ModifierCapability,
        Nv12Plane, Nv12StagingPreference, Nv12TargetAllocationProof, PackedImageFormat,
        PackedImageImport, PackedImageImportStrategy, VulkanDevice, VulkanImportPoolLimits,
        VulkanVideoTiming, YcbcrModel, YcbcrOffset, YcbcrRange, capabilities_for_importer,
        map_nv12_colorimetry, resolve_nv12_modifier_capability, validate_nv12_allocation_proof,
        validate_nv12_shared_object_topology, validate_rgba_import_support,
        validate_sync_fd_import, wait_surface_on_semaphore,
    },
    renderer::{BackendPostFlushTask, RenderFrame},
};

rustler::atoms! {
    keepalive,
    acquire_fence_fd,
}

static NEXT_RENDERER_EPOCH: AtomicU64 = AtomicU64::new(1);

const DRM_FORMAT_NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
const DRM_FORMAT_ABGR8888: u32 = fourcc(b'A', b'B', b'2', b'4');
const DRM_FORMAT_XRGB8888: u32 = fourcc(b'X', b'R', b'2', b'4');
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

/// Immutable native copy of the complete framework-neutral stream contract. It is captured once
/// at consumer open and copied into claimed canonical frames; per-frame metadata cannot mutate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoStreamFormat {
    pub width: u32,
    pub height: u32,
    pub framerate: Option<(u32, u32)>,
    pub fourcc: u32,
    pub modifier_policy: StreamModifierPolicy,
    pub acquire_sync_policy: StreamAcquireSyncPolicy,
    pub colorimetry: Colorimetry,
    pub pixel_aspect_ratio: (u32, u32),
    pub interlace_mode: InteropInterlaceMode,
    pub alpha_mode: InteropAlphaMode,
}

impl TryFrom<InteropFormat> for VideoStreamFormat {
    type Error = String;

    fn try_from(format: InteropFormat) -> Result<Self, Self::Error> {
        format
            .validate()
            .map_err(|error| format!("invalid video stream format: {error}"))?;
        let InteropStorageFormat::DmaBuf(storage) = format.storage else {
            return Err("direct video streams require DMA-BUF format storage".to_string());
        };
        Ok(Self {
            width: format.width,
            height: format.height,
            framerate: format.framerate,
            fourcc: storage.fourcc,
            modifier_policy: match storage.modifier {
                InteropModifierPolicy::PerBuffer => StreamModifierPolicy::PerBuffer,
                InteropModifierPolicy::Implicit => StreamModifierPolicy::Implicit,
                InteropModifierPolicy::Explicit(modifier) => {
                    StreamModifierPolicy::Explicit(modifier)
                }
            },
            acquire_sync_policy: match format.acquire_sync {
                InteropAcquireSyncPolicy::PerFrame => StreamAcquireSyncPolicy::PerFrame,
                InteropAcquireSyncPolicy::Implicit => StreamAcquireSyncPolicy::Implicit,
                InteropAcquireSyncPolicy::SyncFile => StreamAcquireSyncPolicy::SyncFile,
            },
            colorimetry: format.colorimetry,
            pixel_aspect_ratio: format.pixel_aspect_ratio,
            interlace_mode: format.interlace_mode,
            alpha_mode: format.alpha_mode,
        })
    }
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

    if !matches!(
        frame_format,
        DRM_FORMAT_NV12 | DRM_FORMAT_ABGR8888 | DRM_FORMAT_XRGB8888
    ) {
        return Err(format!(
            "unsupported DRM format {frame_format:#x}; supported formats are NV12, ABGR8888, and XRGB8888"
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
    if matches!(format, DRM_FORMAT_ABGR8888 | DRM_FORMAT_XRGB8888) && object_count != 1 {
        return Err(format!(
            "packed PRIME format {format:#x} requires one object, got {object_count}"
        ));
    }

    let expected_planes = match format {
        DRM_FORMAT_NV12 => 2,
        DRM_FORMAT_ABGR8888 | DRM_FORMAT_XRGB8888 => 1,
        _ => return Ok(()),
    };
    if planes.len() != expected_planes {
        return Err(format!(
            "PRIME format {format:#x} requires {expected_planes} plane(s), got {}",
            planes.len()
        ));
    }

    let minimum_pitch = if matches!(format, DRM_FORMAT_ABGR8888 | DRM_FORMAT_XRGB8888) {
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
    #[cfg_attr(not(feature = "vulkan"), allow(dead_code))]
    size: Option<u64>,
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
    #[cfg_attr(not(feature = "vulkan"), allow(dead_code))]
    stream_format: Option<VideoStreamFormat>,
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
        all(feature = "drm", target_os = "linux"),
        all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        )
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
        all(feature = "drm", target_os = "linux"),
        all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        )
    ))]
    fn object(&self, index: usize) -> Result<&PrimeObjectOwned, String> {
        self.objects
            .get(index)
            .ok_or_else(|| format!("prime object index out of range: {index}"))
    }

    #[cfg(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux"),
        all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        )
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
                    size: None,
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
            stream_format: None,
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
            let row_bytes = if matches!(layer.fourcc, DRM_FORMAT_ABGR8888 | DRM_FORMAT_XRGB8888) {
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

    fn from_claimed(
        claimed: ClaimedVideoFrame,
        stream_id: u64,
        stream_format: VideoStreamFormat,
    ) -> Result<Self, String> {
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
        let lease = lease.ok_or_else(|| "claimed DMA-BUF frame has no lease".to_string())?;
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
                    size: Some(object.size),
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
            stream_format: Some(stream_format),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamModifierPolicy {
    PerBuffer,
    Implicit,
    Explicit(u64),
}

impl StreamModifierPolicy {
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
                (Self::PerBuffer, _) | (Self::Implicit, Modifier::Implicit) => Ok(()),
                (Self::Explicit(expected), Modifier::Explicit(actual)) if expected == actual => {
                    Ok(())
                }
                (Self::Implicit, modifier) => Err(format!(
                    "DMA-BUF object {index} modifier {modifier:?} does not match negotiated implicit modifier policy"
                )),
                (Self::Explicit(expected), modifier) => Err(format!(
                    "DMA-BUF object {index} modifier {modifier:?} does not match negotiated DRM modifier {expected:#018x}"
                )),
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamAcquireSyncPolicy {
    PerFrame,
    Implicit,
    SyncFile,
}

impl StreamAcquireSyncPolicy {
    fn validate_frame(self, frame: &OwnedFrame) -> Result<(), String> {
        match (self, &frame.acquire_sync) {
            (Self::PerFrame, _)
            | (Self::Implicit, OwnedAcquireSync::Implicit)
            | (Self::SyncFile, OwnedAcquireSync::SyncFile(_)) => Ok(()),
            (Self::Implicit, OwnedAcquireSync::SyncFile(_)) => Err(
                "video frame sync file does not match negotiated implicit acquire synchronization"
                    .to_string(),
            ),
            (Self::SyncFile, OwnedAcquireSync::Implicit) => Err(
                "implicit video frame does not match negotiated sync-file acquire synchronization"
                    .to_string(),
            ),
        }
    }
}

fn validate_vulkan_stream_format(
    format: VideoStreamFormat,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    rgba_linear_supported: bool,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    bgra_import_supported: bool,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    nv12_capabilities: Option<&[Nv12ModifierCapability]>,
) -> Result<(), String> {
    if format.acquire_sync_policy != StreamAcquireSyncPolicy::SyncFile {
        return Err("Vulkan video streams require immutable acquire_sync: :sync_file".to_string());
    }
    match (format.fourcc, format.modifier_policy) {
        (DRM_FORMAT_ABGR8888, StreamModifierPolicy::Explicit(0)) => {
            #[cfg(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            ))]
            if !rgba_linear_supported {
                return Err(
                    "Vulkan ABGR8888 linear DMA-BUF sampling is unavailable on the active device"
                        .to_string(),
                );
            }
            Ok(())
        }
        (DRM_FORMAT_ABGR8888, modifier) => Err(format!(
            "Vulkan ABGR8888 requires negotiated explicit linear modifier 0, got {modifier:?}"
        )),
        (DRM_FORMAT_XRGB8888, StreamModifierPolicy::Explicit(0)) => {
            if format.interlace_mode != InteropInterlaceMode::Progressive
                || format.alpha_mode != InteropAlphaMode::Opaque
                || format.colorimetry.primaries != video_interop::Primaries::Bt709
                || format.colorimetry.transfer != video_interop::Transfer::Bt709
                || format.colorimetry.matrix != video_interop::Matrix::Rgb
                || format.colorimetry.range != video_interop::ColorRange::Full
                || format.colorimetry.chroma_location != video_interop::ChromaLocation::Unspecified
            {
                return Err(
                    "Vulkan XRGB8888 requires progressive opaque Rec.709/Rec.709/RGB/full color semantics"
                        .to_string(),
                );
            }
            #[cfg(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            ))]
            if !bgra_import_supported {
                return Err(
                    "Vulkan XRGB8888 linear DMA-BUF direct/staged import is unavailable on the active device"
                        .to_string(),
                );
            }
            Ok(())
        }
        (DRM_FORMAT_XRGB8888, modifier) => Err(format!(
            "Vulkan XRGB8888 requires negotiated explicit linear modifier 0, got {modifier:?}"
        )),
        (DRM_FORMAT_NV12, StreamModifierPolicy::Explicit(_)) => {
            if format.interlace_mode != InteropInterlaceMode::Progressive {
                return Err("Vulkan NV12 requires progressive scan".to_string());
            }
            if format.alpha_mode != InteropAlphaMode::Opaque {
                return Err("Vulkan NV12 requires opaque alpha semantics".to_string());
            }
            #[cfg(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            ))]
            {
                let conversion = map_nv12_colorimetry(format.colorimetry)?;
                let StreamModifierPolicy::Explicit(modifier) = format.modifier_policy else {
                    unreachable!("explicit NV12 modifier was matched above")
                };
                let capabilities = nv12_capabilities.ok_or_else(|| {
                    "Vulkan NV12 has no active-device import candidates".to_string()
                })?;
                resolve_nv12_modifier_capability(
                    capabilities,
                    modifier,
                    (format.width, format.height),
                    conversion,
                )
                .map(|_capability| ())
            }
            #[cfg(not(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            )))]
            {
                Err("Vulkan NV12 is unavailable in this build".to_string())
            }
        }
        (DRM_FORMAT_NV12, modifier) => Err(format!(
            "Vulkan NV12 requires one explicit negotiated DRM modifier, got {modifier:?}"
        )),
        (fourcc, _) => Err(format!(
            "unsupported Vulkan video stream DRM format {fourcc:#x}"
        )),
    }
}

fn validate_vulkan_frame_contract(
    format: VideoStreamFormat,
    frame: &OwnedFrame,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    nv12_capabilities: Option<&[Nv12ModifierCapability]>,
) -> Result<(), String> {
    if format.fourcc != DRM_FORMAT_NV12 {
        return Ok(());
    }
    let descriptor = match &frame.storage {
        OwnedStorage::DmaBuf(descriptor) => descriptor,
        _ => return Err("Vulkan NV12 requires DMA-BUF storage".to_string()),
    };
    let layer = descriptor
        .layers
        .first()
        .ok_or_else(|| "Vulkan NV12 frame has no DMA-BUF layer".to_string())?;
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    {
        let layout = validate_nv12_shared_object_topology(
            (frame.coded_width, frame.coded_height),
            &descriptor
                .objects
                .iter()
                .map(|object| object.size)
                .collect::<Vec<_>>(),
            &descriptor
                .objects
                .iter()
                .map(|object| object.modifier.explicit())
                .collect::<Vec<_>>(),
            &layer
                .planes
                .iter()
                .map(|plane| Nv12Plane {
                    object_index: plane.object_index,
                    offset: plane.offset,
                    pitch: plane.pitch,
                })
                .collect::<Vec<_>>(),
        )?;
        let capabilities = nv12_capabilities
            .ok_or_else(|| "Vulkan NV12 has no immutable active-device candidates".to_string())?;
        let conversion = map_nv12_colorimetry(format.colorimetry)?;
        resolve_nv12_modifier_capability(
            capabilities,
            layout.modifier,
            (frame.coded_width, frame.coded_height),
            conversion,
        )
        .map(|_capability| ())
    }
    #[cfg(not(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    )))]
    {
        let _ = layer;
        Err("Vulkan NV12 is unavailable in this build".to_string())
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveStream {
    id: u64,
    format: VideoStreamFormat,
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VideoStreamIdentity {
    pub renderer_epoch: u64,
    pub target_id: String,
    pub target_incarnation: u64,
    pub stream_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoTargetInfo {
    pub renderer_epoch: u64,
    pub target_id: String,
    pub target_incarnation: u64,
    pub active_stream_id: Option<u64>,
}

#[derive(Clone)]
pub struct CpuVideoFrame {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

struct VideoRegistryState {
    open: bool,
    targets: HashMap<String, VideoTargetEntry>,
    cpu_frames: HashMap<String, CpuVideoFrame>,
    active_scene_targets: HashSet<String>,
    prime_video_available: bool,
    stream_requirements: Option<PrimeStreamRequirements>,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    vulkan_rgba_linear_supported: bool,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    vulkan_bgra_import_supported: bool,
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    vulkan_nv12_capabilities: Option<Vec<Nv12ModifierCapability>>,
}

impl Default for VideoRegistryState {
    fn default() -> Self {
        Self {
            open: true,
            targets: HashMap::new(),
            cpu_frames: HashMap::new(),
            active_scene_targets: HashSet::new(),
            prime_video_available: false,
            stream_requirements: None,
            #[cfg(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            ))]
            vulkan_rgba_linear_supported: false,
            #[cfg(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            ))]
            vulkan_bgra_import_supported: false,
            #[cfg(all(
                target_os = "linux",
                feature = "vulkan",
                any(feature = "wayland-core", feature = "drm-core")
            ))]
            vulkan_nv12_capabilities: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimeStreamRequirements {
    Vulkan,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanonicalSubmitDisposition {
    Queue,
    DropInactive,
}

fn canonical_submit_disposition(active: bool) -> CanonicalSubmitDisposition {
    if active {
        CanonicalSubmitDisposition::Queue
    } else {
        CanonicalSubmitDisposition::DropInactive
    }
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

    pub fn submit_cpu_frame(
        &self,
        id: &str,
        mut frame: CpuVideoFrame,
    ) -> Result<VideoSubmitResult, String> {
        let pending = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned".to_string())?;
            if self.admission_closed.load(Ordering::Acquire) || !state.open {
                return Err("video registry is closed".to_string());
            }
            if !state.active_scene_targets.contains(id) {
                return Ok(VideoSubmitResult::DroppedInactive);
            }
            frame.generation = self.generation.load(Ordering::Relaxed).saturating_add(1);
            let pending = state
                .targets
                .get_mut(id)
                .and_then(|entry| entry.pending.take());
            state.cpu_frames.insert(id.to_string(), frame);
            pending
        };
        if let Some(pending) = pending {
            self.defer_release(pending);
        }
        self.bump_generation();
        Ok(VideoSubmitResult::Queued)
    }

    pub fn ensure_direct_stream(
        &self,
        id: &str,
        format: VideoStreamFormat,
    ) -> Result<(u64, u64), String> {
        let (identity, retired) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "video registry lock poisoned".to_string())?;
            if self.admission_closed.load(Ordering::Acquire) || !state.open {
                return Err("video registry is closed".to_string());
            }
            if !state.prime_video_available {
                return Err(prime_video_unavailable_error());
            }
            match state.stream_requirements {
                Some(PrimeStreamRequirements::Vulkan) => validate_vulkan_stream_format(
                    format,
                    #[cfg(all(
                        target_os = "linux",
                        feature = "vulkan",
                        any(feature = "wayland-core", feature = "drm-core")
                    ))]
                    state.vulkan_rgba_linear_supported,
                    #[cfg(all(
                        target_os = "linux",
                        feature = "vulkan",
                        any(feature = "wayland-core", feature = "drm-core")
                    ))]
                    state.vulkan_bgra_import_supported,
                    #[cfg(all(
                        target_os = "linux",
                        feature = "vulkan",
                        any(feature = "wayland-core", feature = "drm-core")
                    ))]
                    state.vulkan_nv12_capabilities.as_deref(),
                )?,
                None => {}
            }

            if let Some(entry) = state.targets.get(id)
                && entry.spec.width == format.width
                && entry.spec.height == format.height
                && let Some(active) = entry.active_stream
                && active.format == format
            {
                return Ok((entry.incarnation, active.id));
            }

            let active = state.active_scene_targets.contains(id);
            let retired = state.targets.remove(id).and_then(|entry| entry.pending);
            state.cpu_frames.remove(id);
            let incarnation = self.next_incarnation.fetch_add(1, Ordering::Relaxed);
            let stream_id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
            state.targets.insert(
                id.to_string(),
                VideoTargetEntry {
                    spec: VideoTargetSpec {
                        id: id.to_string(),
                        width: format.width,
                        height: format.height,
                        mode: VideoMode::Prime,
                    },
                    incarnation,
                    active,
                    active_stream: Some(ActiveStream {
                        id: stream_id,
                        format,
                    }),
                    pending: None,
                },
            );
            ((incarnation, stream_id), retired)
        };
        if let Some(retired) = retired {
            self.defer_release(retired);
        }
        self.bump_generation();
        Ok(identity)
    }

    pub fn cpu_frame_snapshot(&self) -> Result<HashMap<String, CpuVideoFrame>, String> {
        self.state
            .lock()
            .map(|state| state.cpu_frames.clone())
            .map_err(|_| "video registry lock poisoned".to_string())
    }

    pub fn target_is_active(&self, id: &str) -> Result<bool, String> {
        self.state
            .lock()
            .map(|state| state.open && state.active_scene_targets.contains(id))
            .map_err(|_| "video registry lock poisoned".to_string())
    }

    pub fn create_target(&self, spec: VideoTargetSpec) -> Result<u64, String> {
        self.create_target_with_policy(spec, false)
    }

    pub fn create_target_if_available(&self, spec: VideoTargetSpec) -> Result<u64, String> {
        self.create_target_with_policy(spec, true)
    }

    fn create_target_with_policy(
        &self,
        spec: VideoTargetSpec,
        require_available: bool,
    ) -> Result<u64, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        if self.admission_closed.load(Ordering::Acquire) || !state.open {
            return Err("video registry is closed".to_string());
        }
        if require_available && !state.prime_video_available {
            return Err(prime_video_unavailable_error());
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
        drop(state);
        self.bump_generation();
        Ok(incarnation)
    }

    pub fn set_prime_stream_requirements(
        &self,
        requirements: Option<PrimeStreamRequirements>,
    ) -> Result<(), String> {
        #[cfg(all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        if requirements == Some(PrimeStreamRequirements::Vulkan)
            && vulkan_process_quarantine_terminal()
        {
            return Err(
                "Vulkan video runtime is process-terminal after uncertain GPU ownership; restart the VM/process"
                    .to_string(),
            );
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        if state
            .targets
            .values()
            .any(|entry| entry.active_stream.is_some())
        {
            return Err("cannot change video stream requirements with active streams".to_string());
        }
        state.stream_requirements = requirements;
        Ok(())
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    pub fn set_vulkan_import_capabilities(
        &self,
        rgba_linear_supported: bool,
        bgra_import_supported: bool,
        nv12_capabilities: Vec<Nv12ModifierCapability>,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned")?;
        if state
            .targets
            .values()
            .any(|entry| entry.active_stream.is_some())
        {
            return Err("cannot change Vulkan import capabilities with active streams".to_string());
        }
        state.vulkan_rgba_linear_supported = rgba_linear_supported;
        state.vulkan_bgra_import_supported = bgra_import_supported;
        state.vulkan_nv12_capabilities = Some(nv12_capabilities);
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
                state
                    .targets
                    .values_mut()
                    .filter_map(|entry| entry.pending.take())
                    .collect()
            };
            (changed, pending)
        };

        if changed || !pending.is_empty() {
            self.bump_generation();
        }
        self.record_pending_taken(pending.len());
        pending
            .into_iter()
            .for_each(|frame| self.defer_release(frame));
        Ok(())
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
        format: VideoStreamFormat,
    ) -> Result<u64, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned".to_string())?;
        if self.admission_closed.load(Ordering::Acquire) || !state.open {
            return Err("video registry is closed".to_string());
        }
        let stream_requirements = state.stream_requirements;
        #[cfg(all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        let vulkan_rgba_linear_supported = state.vulkan_rgba_linear_supported;
        #[cfg(all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        let vulkan_bgra_import_supported = state.vulkan_bgra_import_supported;
        #[cfg(all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        let vulkan_nv12_capabilities = state.vulkan_nv12_capabilities.clone();
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
            format.width,
            format.height,
            format.fourcc,
        )?;
        if entry.active_stream.is_some() {
            return Err("target_busy".to_string());
        }
        match stream_requirements {
            Some(PrimeStreamRequirements::Vulkan) => validate_vulkan_stream_format(
                format,
                #[cfg(all(
                    target_os = "linux",
                    feature = "vulkan",
                    any(feature = "wayland-core", feature = "drm-core")
                ))]
                vulkan_rgba_linear_supported,
                #[cfg(all(
                    target_os = "linux",
                    feature = "vulkan",
                    any(feature = "wayland-core", feature = "drm-core")
                ))]
                vulkan_bgra_import_supported,
                #[cfg(all(
                    target_os = "linux",
                    feature = "vulkan",
                    any(feature = "wayland-core", feature = "drm-core")
                ))]
                vulkan_nv12_capabilities.as_deref(),
            )?,
            None => {}
        }
        let stream_id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        entry.active_stream = Some(ActiveStream {
            id: stream_id,
            format,
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
            return Err(CanonicalSubmitError::CallerOwned(format!(
                "video registry is closed: renderer_epoch={} target={id} target_incarnation={incarnation} stream_id={stream_id}",
                self.renderer_epoch
            )));
        }
        let stream_requirements = state.stream_requirements;
        #[cfg(all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        let vulkan_nv12_capabilities = state.vulkan_nv12_capabilities.clone();
        let entry = state.targets.get_mut(id).ok_or_else(|| {
            CanonicalSubmitError::CallerOwned(format!(
                "unknown video target: {id}; renderer_epoch={} target_incarnation={incarnation} stream_id={stream_id}",
                self.renderer_epoch
            ))
        })?;
        if entry.incarnation != incarnation {
            return Err(CanonicalSubmitError::CallerOwned(format!(
                "stale video target incarnation: target={id} submitted_incarnation={incarnation} active_incarnation={} renderer_epoch={} stream_id={stream_id}",
                entry.incarnation, self.renderer_epoch
            )));
        }
        let active_stream = match entry.active_stream {
            Some(active) if active.id == stream_id => active,
            Some(active) => {
                return Err(CanonicalSubmitError::CallerOwned(format!(
                    "stale video consumer stream: renderer_epoch={} target={id} target_incarnation={incarnation} submitted_stream_id={stream_id} active_stream_id={}",
                    self.renderer_epoch, active.id
                )));
            }
            None => {
                return Err(CanonicalSubmitError::CallerOwned(format!(
                    "video consumer stream is closed: renderer_epoch={} target={id} target_incarnation={incarnation} stream_id={stream_id}",
                    self.renderer_epoch
                )));
            }
        };
        active_stream
            .format
            .modifier_policy
            .validate_frame(prepared.frame())
            .map_err(CanonicalSubmitError::CallerOwned)?;
        active_stream
            .format
            .acquire_sync_policy
            .validate_frame(prepared.frame())
            .map_err(CanonicalSubmitError::CallerOwned)?;
        PrimeFrame::validate_canonical(
            id,
            entry.spec.mode,
            entry.spec.width,
            entry.spec.height,
            active_stream.format.fourcc,
            prepared.frame(),
        )
        .map_err(CanonicalSubmitError::CallerOwned)?;
        if matches!(stream_requirements, Some(PrimeStreamRequirements::Vulkan)) {
            validate_vulkan_frame_contract(
                active_stream.format,
                prepared.frame(),
                #[cfg(all(
                    target_os = "linux",
                    feature = "vulkan",
                    any(feature = "wayland-core", feature = "drm-core")
                ))]
                vulkan_nv12_capabilities.as_deref(),
            )
            .map_err(CanonicalSubmitError::CallerOwned)?;
        }
        let disposition = canonical_submit_disposition(entry.active);
        let claimed = prepared.claim();
        let mut frame = PrimeFrame::from_claimed(claimed, stream_id, active_stream.format)
            .map_err(CanonicalSubmitError::Transferred)?;
        frame.submitted_at = Instant::now();
        frame.stats = self.stats.clone();

        match disposition {
            CanonicalSubmitDisposition::Queue => {
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
            CanonicalSubmitDisposition::DropInactive => {
                drop(state);
                if let Some(stats) = self.stats.as_deref() {
                    stats.record_video_inactive_drop();
                }
                self.defer_release(frame);
                Ok(VideoSubmitResult::DroppedInactive)
            }
        }
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
        frame: PrimeFrame,
    ) -> Result<VideoSubmitResult, String> {
        self.submit_prime_exact_with_policy(id, incarnation, frame, false)
    }

    pub fn submit_prime_exact_if_available(
        &self,
        id: &str,
        incarnation: u64,
        frame: PrimeFrame,
    ) -> Result<VideoSubmitResult, String> {
        self.submit_prime_exact_with_policy(id, incarnation, frame, true)
    }

    fn submit_prime_exact_with_policy(
        &self,
        id: &str,
        incarnation: u64,
        mut frame: PrimeFrame,
        require_available: bool,
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
            if require_available && !state.prime_video_available {
                drop(state);
                self.defer_release(frame);
                return Err(prime_video_unavailable_error());
            }
            if matches!(
                state.stream_requirements,
                Some(PrimeStreamRequirements::Vulkan)
            ) {
                drop(state);
                self.defer_release(frame);
                return Err(
                    "legacy raw PRIME submission is unavailable for Vulkan video; use the canonical stream path"
                        .to_string(),
                );
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

    #[doc(hidden)]
    pub fn pipeline_counts_for_test(&self) -> Option<(u64, u64, u64)> {
        self.stats.as_ref().map(|stats| {
            let snapshot = stats.peek();
            (
                snapshot.video_pipeline.submitted,
                snapshot.video_pipeline.inactive_dropped,
                snapshot.video_pipeline.current_pending,
            )
        })
    }

    #[cfg(all(feature = "drm-core", target_os = "linux"))]
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
                .cpu_frames
                .retain(|id, _frame| active_targets.contains(id));
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

    pub fn target_info(&self, id: &str, incarnation: u64) -> Result<VideoTargetInfo, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "video registry lock poisoned".to_string())?;
        if !state.open {
            return Err("video registry is closed".to_string());
        }
        let entry = state
            .targets
            .get(id)
            .ok_or_else(|| format!("unknown video target: {id}"))?;
        if entry.incarnation != incarnation {
            return Err(format!("stale video target incarnation: {id}"));
        }
        Ok(VideoTargetInfo {
            renderer_epoch: self.renderer_epoch,
            target_id: id.to_string(),
            target_incarnation: incarnation,
            active_stream_id: entry.active_stream.map(|stream| stream.id),
        })
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

    pub(crate) fn retry_acquire_cleanup(&self) -> bool {
        self.support.retry_deferred_sync_destroy()
    }

    pub(crate) fn has_acquire_cleanup(&self) -> bool {
        self.support.has_deferred_sync_destroy()
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

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Default)]
struct Nv12RuntimeAttestations {
    proofs: HashMap<(u32, u32, u64), Nv12TargetAllocationProof>,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl Nv12RuntimeAttestations {
    fn validate(
        &self,
        active_device_identity: crate::backend::vulkan::VulkanDeviceIdentity,
        topology: Nv12FrameTopology,
        recipe: Nv12AllocationBindingRecipe,
    ) -> Result<(), String> {
        let key = (
            topology.dimensions.0,
            topology.dimensions.1,
            topology.modifier,
        );
        match self.proofs.get(&key).copied() {
            Some(proof) => {
                validate_nv12_allocation_proof(active_device_identity, proof, topology, recipe)
            }
            None => Ok(()),
        }
    }

    fn record(
        &mut self,
        active_device_identity: crate::backend::vulkan::VulkanDeviceIdentity,
        topology: Nv12FrameTopology,
        recipe: Nv12AllocationBindingRecipe,
    ) -> Result<bool, String> {
        self.validate(active_device_identity, topology, recipe)?;
        let key = (
            topology.dimensions.0,
            topology.dimensions.1,
            topology.modifier,
        );
        if self.proofs.contains_key(&key) {
            return Ok(false);
        }
        self.proofs.insert(
            key,
            Nv12TargetAllocationProof {
                device_identity: active_device_identity,
                topology,
                recipe,
            },
        );
        Ok(true)
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanImportSyncPool {
    device: Arc<VulkanDevice>,
    available: Mutex<Vec<ImportedImageSync>>,
    max_lanes: usize,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanImportSyncPool {
    fn new(device: Arc<VulkanDevice>, max_lanes: usize) -> Self {
        Self {
            device,
            available: Mutex::new(Vec::new()),
            max_lanes,
        }
    }

    fn checkout(&self) -> Result<ImportedImageSync, ImportedImageSyncError> {
        let mut available = match self.available.lock() {
            Ok(available) => available,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(sync) = available.pop() {
            return Ok(sync);
        }
        drop(available);
        ImportedImageSync::new(Arc::clone(&self.device))
    }

    fn recycle(&self, mut sync: ImportedImageSync) -> Result<(), String> {
        sync.reset_for_reuse()?;
        let mut available = match self.available.lock() {
            Ok(available) => available,
            Err(poisoned) => poisoned.into_inner(),
        };
        if available.len() < self.max_lanes {
            available.push(sync);
        }
        Ok(())
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn make_nv12_runtime_effect(conversion: Nv12Conversion) -> Result<RuntimeEffect, String> {
    if conversion.model != YcbcrModel::Bt709 {
        return Err(
            "Emerge Vulkan planar NV12 shader requires the exact BT.709 matrix".to_string(),
        );
    }
    let range = match conversion.range {
        YcbcrRange::Narrow => (
            "clamp((y_code - 16.0 / 255.0) * (255.0 / 219.0), 0.0, 1.0)",
            "(uv_code - half2(128.0 / 255.0)) * (255.0 / 224.0)",
        ),
        YcbcrRange::Full => ("y_code", "(uv_code - half2(128.0 / 255.0))"),
    };
    let offset = |offset| match offset {
        YcbcrOffset::CositedEven => "0.25",
        YcbcrOffset::Midpoint => "0.0",
    };
    let source = format!(
        r#"
        uniform shader y_plane;
        uniform shader uv_plane;
        half4 main(float2 p) {{
            half y_code = y_plane.eval(p).r;
            half2 uv_code = uv_plane.eval(p * 0.5 + float2({}, {})).rg;
            half y = {};
            half2 chroma = {};
            half cb = chroma.x;
            half cr = chroma.y;
            half3 rgb = half3(
                y + 1.5748 * cr,
                y - 0.187324 * cb - 0.468124 * cr,
                y + 1.8556 * cb
            );
            return half4(clamp(rgb, 0.0, 1.0), 1.0);
        }}
        "#,
        offset(conversion.x_offset),
        offset(conversion.y_offset),
        range.0,
        range.1,
    );
    RuntimeEffect::make_for_shader(source, None)
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_nv12_staging_preference_from_value(
    value: Option<&str>,
) -> Result<Nv12StagingPreference, String> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("auto") => Ok(Nv12StagingPreference::PreferPlanar),
        Some("planar") => Ok(Nv12StagingPreference::RequirePlanar),
        Some("rgba") => Ok(Nv12StagingPreference::RequireRgba),
        Some(value) => Err(format!(
            "EMERGE_VULKAN_NV12_STAGING must be auto, planar, or rgba, got {value:?}"
        )),
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
pub struct VulkanVideoImportContext {
    device: Arc<VulkanDevice>,
    importer: Arc<InteropVulkanDmaBufImporter>,
    sync_pool: Arc<VulkanImportSyncPool>,
    nv12_effects: Mutex<HashMap<Nv12Conversion, RuntimeEffect>>,
    rgba_linear_supported: bool,
    bgra_import_strategy: Option<PackedImageImportStrategy>,
    #[cfg_attr(not(feature = "wayland-vulkan"), allow(dead_code))]
    nv12_capabilities: Vec<Nv12ModifierCapability>,
    nv12_attestations: Mutex<Nv12RuntimeAttestations>,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanVideoImportContext {
    pub fn new(device: Arc<VulkanDevice>) -> Result<Self, String> {
        if vulkan_process_quarantine_terminal() {
            return Err(
                "Vulkan video importer is process-terminal after uncertain GPU ownership; restart the VM/process"
                    .to_string(),
            );
        }
        validate_sync_fd_import(&device)?;
        let rgba_import = validate_rgba_import_support(&device, DRM_FORMAT_MOD_LINEAR);
        let rgba_linear_supported = rgba_import.is_ok();
        let staging_preference = vulkan_nv12_staging_preference_from_value(
            std::env::var("EMERGE_VULKAN_NV12_STAGING").ok().as_deref(),
        )?;
        eprintln!("Vulkan NV12 staging preference: {staging_preference:?}");
        let importer = Arc::new(
            InteropVulkanDmaBufImporter::new_with_limits_and_staging_preference(
                Arc::clone(&device),
                VulkanImportPoolLimits {
                    nv12_source_cache_entries: 16,
                    nv12_output_slots: 4,
                    packed_source_cache_entries: 16,
                    packed_output_slots: 4,
                },
                staging_preference,
            )?,
        );
        let bgra_import =
            importer.packed_import_strategy(PackedImageFormat::Bgra8888, DRM_FORMAT_MOD_LINEAR);
        let bgra_import_strategy = bgra_import.as_ref().ok().copied();
        match &bgra_import {
            Ok(strategy) => {
                eprintln!("Vulkan XRGB8888 import strategy: {strategy:?}");
            }
            Err(error) => {
                eprintln!("Vulkan XRGB8888 import unavailable: {error}");
            }
        }
        let nv12_capabilities = capabilities_for_importer(&device, &importer);
        nv12_capabilities.iter().for_each(|capability| {
            eprintln!(
                "Vulkan NV12 import capability: modifier={:#018x} strategy={:?} advertised_memory_planes={}",
                capability.modifier,
                capability.import_strategy(),
                capability.modifier_plane_count(),
            );
        });
        if !rgba_linear_supported && bgra_import_strategy.is_none() && nv12_capabilities.is_empty()
        {
            return Err(format!(
                "Vulkan device has no usable packed/NV12 video import strategy: RGBA={}; BGRA={}; no importable NV12 DRM modifiers were advertised",
                rgba_import.expect_err("RGBA support was checked as unavailable"),
                bgra_import.expect_err("BGRA support was checked as unavailable"),
            ));
        }
        let sync_pool = Arc::new(VulkanImportSyncPool::new(Arc::clone(&device), 8));
        Ok(Self {
            device,
            importer,
            sync_pool,
            nv12_effects: Mutex::new(HashMap::new()),
            rgba_linear_supported,
            bgra_import_strategy,
            nv12_capabilities,
            nv12_attestations: Mutex::new(Nv12RuntimeAttestations::default()),
        })
    }

    fn device(&self) -> &Arc<VulkanDevice> {
        &self.device
    }

    fn importer(&self) -> &InteropVulkanDmaBufImporter {
        &self.importer
    }

    fn checkout_sync(&self) -> Result<ImportedImageSync, ImportedImageSyncError> {
        self.sync_pool.checkout()
    }

    fn runtime_effect(&self, conversion: Nv12Conversion) -> Result<RuntimeEffect, String> {
        let mut effects = self
            .nv12_effects
            .lock()
            .map_err(|_| "Vulkan NV12 runtime-effect lock poisoned".to_string())?;
        if let Some(effect) = effects.get(&conversion) {
            return Ok(effect.clone());
        }
        let effect = make_nv12_runtime_effect(conversion)?;
        effects.insert(conversion, effect.clone());
        Ok(effect)
    }

    fn evict_stream(&self, stream_incarnation: u64) -> Result<(), String> {
        self.importer.evict_nv12_stream(stream_incarnation)?;
        self.importer.evict_packed_stream(stream_incarnation)
    }

    fn record_pool_stats(&self, stats: Option<&RendererStatsCollector>) {
        let Some(stats) = stats else {
            return;
        };
        let Ok(pool) = self.importer.nv12_cache_stats() else {
            return;
        };
        let Ok(packed) = self.importer.packed_cache_stats() else {
            return;
        };
        let validation = self.device.instance().validation_report();
        stats.set_vulkan_video_import_pool_stats(VulkanVideoImportPoolStats {
            validation_enabled: validation.enabled,
            validation_errors: validation.errors,
            validation_warnings: validation.warnings,
            source_cache_hits: pool
                .source_cache_hits
                .saturating_add(packed.source_cache_hits),
            source_cache_misses: pool
                .source_cache_misses
                .saturating_add(packed.source_cache_misses),
            source_cache_evictions: pool
                .source_cache_evictions
                .saturating_add(packed.source_cache_evictions),
            source_active_reuse_rejections: pool
                .source_active_reuse_rejections
                .saturating_add(packed.source_active_reuse_rejections),
            source_topology_collisions: pool
                .source_topology_collisions
                .saturating_add(packed.source_topology_collisions),
            output_pool_busy_rejections: pool
                .output_pool_busy_rejections
                .saturating_add(packed.output_pool_busy_rejections),
            source_cache_entries: pool.source_entries.saturating_add(packed.source_entries),
            output_pool_slots: pool.output_slots.saturating_add(packed.output_slots),
            packed_cache_hits: packed.source_cache_hits,
            packed_cache_misses: packed.source_cache_misses,
            packed_cache_evictions: packed.source_cache_evictions,
            packed_active_reuse_rejections: packed.source_active_reuse_rejections,
            packed_topology_collisions: packed.source_topology_collisions,
            packed_allocation_size_rejections: packed.allocation_size_rejections,
            packed_cache_entries: packed.source_entries,
        });
    }

    pub(crate) fn rgba_linear_supported(&self) -> bool {
        self.rgba_linear_supported
    }

    pub(crate) fn bgra_import_supported(&self) -> bool {
        self.bgra_import_strategy.is_some()
    }

    fn bgra_import_strategy(&self) -> Result<PackedImageImportStrategy, String> {
        self.bgra_import_strategy.ok_or_else(|| {
            "Vulkan XRGB8888 has no active-device direct or staged import strategy".to_string()
        })
    }

    pub(crate) fn supported_format_names(&self) -> Vec<&'static str> {
        let mut formats = Vec::new();
        if !self.nv12_capabilities.is_empty() {
            formats.push("NV12");
        }
        if self.rgba_linear_supported {
            formats.push("ABGR8888");
        }
        if self.bgra_import_strategy.is_some() {
            formats.push("XRGB8888");
        }
        formats
    }

    #[cfg_attr(not(feature = "drm-vulkan"), allow(dead_code))]
    pub(crate) fn supports_any_format(&self) -> bool {
        self.rgba_linear_supported
            || self.bgra_import_strategy.is_some()
            || !self.nv12_capabilities.is_empty()
    }

    #[cfg_attr(not(feature = "wayland-vulkan"), allow(dead_code))]
    pub(crate) fn nv12_capabilities(&self) -> &[Nv12ModifierCapability] {
        &self.nv12_capabilities
    }

    fn validate_nv12_topology(
        &self,
        topology: Nv12FrameTopology,
        recipe: Nv12AllocationBindingRecipe,
    ) -> Result<(), String> {
        self.nv12_attestations
            .lock()
            .map_err(|_| "Vulkan NV12 runtime-attestation lock poisoned".to_string())?
            .validate(self.device.identity(), topology, recipe)
    }

    fn attest_nv12_topology(
        &self,
        topology: Nv12FrameTopology,
        recipe: Nv12AllocationBindingRecipe,
    ) -> Result<bool, String> {
        self.nv12_attestations
            .lock()
            .map_err(|_| "Vulkan NV12 runtime-attestation lock poisoned".to_string())?
            .record(self.device.identity(), topology, recipe)
    }

    fn nv12_capability(
        &self,
        modifier: u64,
        dimensions: (u32, u32),
        conversion: Nv12Conversion,
    ) -> Result<Nv12ModifierCapability, String> {
        resolve_nv12_modifier_capability(&self.nv12_capabilities, modifier, dimensions, conversion)
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

#[cfg_attr(
    not(any(feature = "linux-opengl", feature = "vulkan")),
    allow(dead_code)
)]
#[derive(Clone, Debug, Default)]
pub struct VideoSyncResult {
    pub resources_changed: bool,
    pub needs_cleanup: bool,
    pub imported_frames: usize,
    pub imported_streams: Vec<VideoStreamIdentity>,
    pub newest_import_submitted_at: Option<Instant>,
    pub first_frame_diagnostics: Option<String>,
}

#[cfg_attr(
    not(any(feature = "linux-opengl", feature = "vulkan", test)),
    allow(dead_code)
)]
fn canonical_import_identity(
    renderer_epoch: u64,
    pending: &PendingVideoFrame,
) -> Option<VideoStreamIdentity> {
    pending
        .frame
        .stream_id
        .map(|stream_id| VideoStreamIdentity {
            renderer_epoch,
            target_id: pending.id.clone(),
            target_incarnation: pending.incarnation,
            stream_id,
        })
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
fn retired_import_wait_needs_gl_finish(status: gl::types::GLenum) -> bool {
    status == gl::WAIT_FAILED
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

    fn wait_blocking(self, target_id: &str, count_runtime_fallback: bool) {
        unsafe {
            let status =
                gl::ClientWaitSync(self.sync, gl::SYNC_FLUSH_COMMANDS_BIT, gl::TIMEOUT_IGNORED);
            if retired_import_wait_needs_gl_finish(status) {
                eprintln!(
                    "video sync failed: glClientWaitSync WAIT_FAILED during blocking cleanup for target={target_id}; forcing glFinish"
                );
                if count_runtime_fallback && let Some(stats) = self.stats.as_deref() {
                    stats.record_video_retired_gl_finish_fallback();
                }
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
            native_fence,
            native_fence_capabilities,
            deferred_sync_destroy: RefCell::new(Vec::new()),
            acquire_received: Cell::new(0),
            acquire_server_queued: Cell::new(0),
            acquire_client_fallback: Cell::new(0),
            acquire_timeouts: Cell::new(0),
            acquire_errors: Cell::new(0),
            supports_modifiers,
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

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
const MAX_RETIRED_VULKAN_VIDEO_IMPORTS: usize = 8;
#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
const VULKAN_VIDEO_RETIRE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Default)]
struct VulkanQuarantinePolicy {
    terminal: bool,
    live_imports: usize,
    quarantined_imports: usize,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VulkanProcessAdmissionError {
    Terminal,
    Saturated,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanQuarantinePolicy {
    fn admit(&mut self) -> Result<(), VulkanProcessAdmissionError> {
        if self.terminal {
            return Err(VulkanProcessAdmissionError::Terminal);
        }
        if self.live_imports >= MAX_RETIRED_VULKAN_VIDEO_IMPORTS {
            return Err(VulkanProcessAdmissionError::Saturated);
        }
        self.live_imports += 1;
        Ok(())
    }

    fn release(&mut self) {
        self.live_imports = self.live_imports.saturating_sub(1);
    }

    fn mark_terminal(&mut self) {
        self.terminal = true;
    }

    fn reserve_quarantine_slot(&mut self) -> bool {
        self.mark_terminal();
        if self.quarantined_imports >= MAX_RETIRED_VULKAN_VIDEO_IMPORTS {
            return false;
        }
        self.quarantined_imports += 1;
        true
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Default)]
struct VulkanProcessQuarantineOwner {
    policy: VulkanQuarantinePolicy,
    resources: Vec<VulkanImportedResource>,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
static VULKAN_PROCESS_QUARANTINE: OnceLock<Mutex<VulkanProcessQuarantineOwner>> = OnceLock::new();

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_process_quarantine() -> &'static Mutex<VulkanProcessQuarantineOwner> {
    VULKAN_PROCESS_QUARANTINE.get_or_init(|| Mutex::new(VulkanProcessQuarantineOwner::default()))
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn with_vulkan_process_quarantine<T>(
    operation: impl FnOnce(&mut VulkanProcessQuarantineOwner) -> T,
) -> T {
    let mut owner = vulkan_process_quarantine()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    operation(&mut owner)
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_process_quarantine_terminal() -> bool {
    with_vulkan_process_quarantine(|owner| owner.policy.terminal)
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
pub(crate) fn ensure_vulkan_process_runtime_admission() -> Result<(), String> {
    if vulkan_process_quarantine_terminal() {
        Err(
            "Vulkan runtime is process-terminal after uncertain GPU ownership; restart the VM/process"
                .to_string(),
        )
    } else {
        Ok(())
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanImportAdmission;

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanImportAdmission {
    fn acquire() -> Result<Self, VulkanProcessAdmissionError> {
        with_vulkan_process_quarantine(|owner| owner.policy.admit())?;
        Ok(Self)
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl Drop for VulkanImportAdmission {
    fn drop(&mut self) {
        with_vulkan_process_quarantine(|owner| owner.policy.release());
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
pub(crate) fn mark_vulkan_process_quarantine_terminal() {
    with_vulkan_process_quarantine(|owner| owner.policy.mark_terminal());
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn retain_vulkan_quarantined_resource(resource: VulkanImportedResource) {
    with_vulkan_process_quarantine(|owner| {
        if !owner.policy.reserve_quarantine_slot() {
            eprintln!(
                "process-wide Vulkan quarantine invariant exceeded its hard cap; restart is required"
            );
            // Dropping this resource could return an uncertain canonical lease to its producer.
            // The admission cap makes this branch unreachable; abort preserves the hard bound and
            // requires the process restart already mandated by the terminal state.
            std::process::abort();
        }
        owner.resources.push(resource);
    });
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn can_enqueue_vulkan_retirement(current_retired: usize) -> bool {
    current_retired < MAX_RETIRED_VULKAN_VIDEO_IMPORTS
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_import_capacity_available(
    current_count: usize,
    blocked_count: usize,
    retired_count: usize,
    replacing_current: bool,
) -> bool {
    let total_has_headroom = current_count
        .saturating_add(blocked_count)
        .saturating_add(retired_count)
        < MAX_RETIRED_VULKAN_VIDEO_IMPORTS;
    let current_has_replacement_headroom =
        replacing_current || current_count < MAX_RETIRED_VULKAN_VIDEO_IMPORTS.saturating_sub(1);
    total_has_headroom && current_has_replacement_headroom
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_retirement_timed_out(elapsed: Duration) -> bool {
    elapsed >= VULKAN_VIDEO_RETIRE_TIMEOUT
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VulkanImportFault {
    AcquireFenceRejected,
    AcquireSubmitFailed,
    ReleaseSubmitFailed,
    RetirementTimeout,
    DeviceLost,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_fault_requires_quarantine(fault: VulkanImportFault) -> bool {
    matches!(
        fault,
        VulkanImportFault::ReleaseSubmitFailed
            | VulkanImportFault::RetirementTimeout
            | VulkanImportFault::DeviceLost
    )
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VulkanTicketResourceDisposition {
    NormalDrop,
    RetainInProcessQuarantine,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn vulkan_ticket_resource_disposition(
    lock_poisoned: bool,
    quarantined: bool,
    device_lost: bool,
) -> VulkanTicketResourceDisposition {
    if lock_poisoned || quarantined || device_lost {
        VulkanTicketResourceDisposition::RetainInProcessQuarantine
    } else {
        VulkanTicketResourceDisposition::NormalDrop
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanImportedResource {
    // The staged source frame may retire before the renderer-native output. The cached Vulkan
    // source allocation remains attached to `allocation` and is recycled only after its exact
    // source-release fence proves external ownership.
    sync: Option<ImportedImageSync>,
    sync_pool: Arc<VulkanImportSyncPool>,
    allocation: ImportedDmaBufImage,
    frame: Option<PrimeFrame>,
    stats: Option<Arc<RendererStatsCollector>>,
    _process_admission: VulkanImportAdmission,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanImportTicket {
    resource: Mutex<Option<VulkanImportedResource>>,
    timing: Mutex<Option<VulkanVideoTiming>>,
    source_submitted_at: Instant,
    quarantined: AtomicBool,
    device_loss_recorded: AtomicBool,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanImportTicket {
    fn record_stats(&self, record: impl FnOnce(&RendererStatsCollector)) {
        let stats = match self.resource.lock() {
            Ok(resource) => resource
                .as_ref()
                .and_then(|resource| resource.stats.clone()),
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                let stats = resource
                    .as_ref()
                    .and_then(|resource| resource.stats.clone());
                self.mark_quarantined(stats.as_deref());
                if let Some(resource) = resource.take() {
                    retain_vulkan_quarantined_resource(resource);
                }
                stats
            }
        };
        if let Some(stats) = stats.as_deref() {
            record(stats);
        }
    }

    fn record_sync_error(&self, error: &ImportedImageSyncError) {
        self.record_stats(|stats| match error.kind() {
            ImportedImageSyncErrorKind::TemporarySemaphoreImport => {
                stats.record_vulkan_video_temporary_semaphore_import_failure();
            }
            ImportedImageSyncErrorKind::AcquireSubmit => {
                stats.record_vulkan_video_acquire_submit_failure();
            }
            ImportedImageSyncErrorKind::ReleaseSubmit => {
                stats.record_vulkan_video_release_submit_failure();
            }
            ImportedImageSyncErrorKind::ReleaseFenceCreate => {
                stats.record_vulkan_video_release_fence_error();
            }
            ImportedImageSyncErrorKind::ReleaseFencePoll
            | ImportedImageSyncErrorKind::SourceFencePoll => {
                stats.record_vulkan_video_release_fence_error();
            }
            ImportedImageSyncErrorKind::Other => {}
        });
        if error.is_device_lost() && !self.device_loss_recorded.swap(true, Ordering::AcqRel) {
            self.record_stats(RendererStatsCollector::record_vulkan_video_device_lost);
        }
    }

    fn mark_quarantined(&self, stats: Option<&RendererStatsCollector>) {
        if !self.quarantined.swap(true, Ordering::AcqRel) {
            mark_vulkan_process_quarantine_terminal();
            if let Some(stats) = stats {
                stats.record_vulkan_video_quarantined();
                stats.record_vulkan_video_global_quarantine_terminal();
            }
        }
    }

    fn quarantine(&self) {
        match self.resource.lock() {
            Ok(resource) => {
                let stats = resource
                    .as_ref()
                    .and_then(|resource| resource.stats.clone());
                self.mark_quarantined(stats.as_deref());
            }
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
            }
        }
    }

    fn quarantine_poisoned_resource(&self, resource: &mut Option<VulkanImportedResource>) {
        let stats = resource
            .as_ref()
            .and_then(|resource| resource.stats.clone());
        self.mark_quarantined(stats.as_deref());
        if let Some(resource) = resource.take() {
            retain_vulkan_quarantined_resource(resource);
        }
    }

    fn ganesh_wait_accepted(&self, semaphore: vk::Semaphore) -> Result<(), String> {
        let result = match self.resource.lock() {
            Ok(mut resource) => match resource.as_mut() {
                Some(resource) => resource
                    .sync
                    .as_mut()
                    .ok_or_else(|| "Vulkan imported-image sync was already recycled".to_string())?
                    .ganesh_wait_accepted(semaphore),
                None => Err("Vulkan imported-image ticket is quarantined".to_string()),
            },
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
                return Err("Vulkan imported-image ticket lock poisoned".to_string());
            }
        };
        if let Err(error) = result {
            // Surface::wait(true) has already accepted ownership of the semaphore. Any mismatch or
            // state uncertainty after that handoff makes destruction timing unknowable, so retain
            // the complete image/session rather than unwinding native children onto live GPU work.
            self.quarantine();
            return Err(error);
        }
        Ok(())
    }

    fn ganesh_wait_rejected(&self, semaphore: vk::Semaphore) -> Result<(), String> {
        let result = match self.resource.lock() {
            Ok(mut resource) => match resource.as_mut() {
                Some(resource) => resource
                    .sync
                    .as_mut()
                    .ok_or_else(|| "Vulkan imported-image sync was already recycled".to_string())?
                    .ganesh_wait_rejected(semaphore),
                None => Err("Vulkan imported-image ticket is quarantined".to_string()),
            },
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
                return Err("Vulkan imported-image ticket lock poisoned".to_string());
            }
        };
        if result.is_err() {
            self.quarantine();
        }
        result
    }

    fn record_imported(&self) -> Result<(), String> {
        match self.resource.lock() {
            Ok(resource) => match resource.as_ref() {
                Some(resource) => {
                    if let Some(frame) = resource.frame.as_ref() {
                        frame.record_imported();
                    }
                    Ok(())
                }
                None => Err("Vulkan imported-image ticket is quarantined".to_string()),
            },
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
                Err("Vulkan imported-image ticket lock poisoned".to_string())
            }
        }
    }

    fn ensure_release_submitted(&self) -> Result<(), String> {
        if self.quarantined.load(Ordering::Acquire) {
            return Err("Vulkan imported-image ticket is quarantined".to_string());
        }
        let submitted = match self.resource.lock() {
            Ok(resource) => resource.as_ref().is_some_and(|resource| {
                resource
                    .sync
                    .as_ref()
                    .is_none_or(ImportedImageSync::release_submitted)
            }),
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
                return Err("Vulkan imported-image ticket lock poisoned".to_string());
            }
        };
        if submitted {
            Ok(())
        } else {
            self.submit_after_flush()
        }
    }

    fn source_release_complete(&self) -> Result<bool, String> {
        if self.quarantined.load(Ordering::Acquire) {
            return Err("Vulkan imported-image ticket is quarantined".to_string());
        }
        let mut resource = match self.resource.lock() {
            Ok(resource) => resource,
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
                return Err("Vulkan imported-image ticket lock poisoned".to_string());
            }
        };
        let resource = resource
            .as_mut()
            .ok_or_else(|| "Vulkan imported-image ticket is quarantined".to_string())?;
        if !resource.allocation.interop().is_staged() {
            return Ok(false);
        }
        if resource.frame.is_none() {
            return Ok(true);
        }
        let complete = match resource
            .sync
            .as_ref()
            .ok_or_else(|| "Vulkan staged source sync was recycled too early".to_string())?
            .source_release_complete()
        {
            Ok(complete) => complete,
            Err(error) => {
                if let Some(stats) = resource.stats.as_deref() {
                    stats.record_vulkan_video_release_fence_error();
                    if error.is_device_lost()
                        && !self.device_loss_recorded.swap(true, Ordering::AcqRel)
                    {
                        stats.record_vulkan_video_device_lost();
                    }
                }
                return Err(error.to_string());
            }
        };
        if complete {
            resource.allocation.interop().release_staged_source();
            resource.frame.take();
            return Ok(true);
        }
        if vulkan_retirement_timed_out(self.source_submitted_at.elapsed()) {
            if let Some(stats) = resource.stats.as_deref() {
                stats.record_vulkan_video_retirement_timeout();
            }
            return Err("Vulkan staged source-release fence timed out".to_string());
        }
        Ok(false)
    }

    fn staged_source_pending(&self) -> bool {
        self.resource.lock().is_ok_and(|resource| {
            resource.as_ref().is_some_and(|resource| {
                resource.allocation.interop().is_staged() && resource.frame.is_some()
            })
        })
    }

    fn take_timing(&self) -> Option<VulkanVideoTiming> {
        self.timing.lock().ok()?.take()
    }

    fn release_complete(&self) -> Result<bool, String> {
        if self.quarantined.load(Ordering::Acquire) {
            return Err("Vulkan imported-image ticket is quarantined".to_string());
        }
        let mut resource = match self.resource.lock() {
            Ok(resource) => resource,
            Err(poisoned) => {
                let mut resource = poisoned.into_inner();
                self.quarantine_poisoned_resource(&mut resource);
                return Err("Vulkan imported-image ticket lock poisoned".to_string());
            }
        };
        let resource = resource
            .as_mut()
            .ok_or_else(|| "Vulkan imported-image ticket is quarantined".to_string())?;
        let result = resource
            .sync
            .as_ref()
            .map_or(Ok(true), ImportedImageSync::release_complete);
        match result {
            Ok(true) => {
                if let Some(sync) = resource.sync.take() {
                    if let Some(timing) = sync.take_timing()
                        && let Ok(mut sample) = self.timing.lock()
                    {
                        *sample = Some(timing);
                    }
                    let _ = resource.sync_pool.recycle(sync);
                }
                resource.allocation.interop().release_staged_source();
                resource.frame.take();
                if let Some(stats) = resource.stats.as_deref() {
                    stats.record_vulkan_video_release_completed();
                    stats.record_vulkan_video_release_fence_completion();
                }
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(error) => {
                if let Some(stats) = resource.stats.as_deref() {
                    stats.record_vulkan_video_release_fence_error();
                    if error.is_device_lost()
                        && !self.device_loss_recorded.swap(true, Ordering::AcqRel)
                    {
                        stats.record_vulkan_video_device_lost();
                    }
                }
                Err(error.to_string())
            }
        }
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl BackendPostFlushTask for VulkanImportTicket {
    fn submit_after_flush(&self) -> Result<(), String> {
        let result = {
            let mut resource = match self.resource.lock() {
                Ok(resource) => resource,
                Err(poisoned) => {
                    let mut resource = poisoned.into_inner();
                    self.quarantine_poisoned_resource(&mut resource);
                    return Err("Vulkan imported-image ticket lock poisoned".to_string());
                }
            };
            let resource = resource
                .as_mut()
                .ok_or_else(|| "Vulkan imported-image ticket is quarantined".to_string())?;
            resource
                .sync
                .as_mut()
                .ok_or_else(|| "Vulkan imported-image sync was already recycled".to_string())?
                .submit_release(resource.allocation.interop())
        };
        match result {
            Ok(()) => {
                self.record_stats(RendererStatsCollector::record_vulkan_video_release_submitted);
                Ok(())
            }
            Err(error) => {
                self.record_sync_error(&error);
                if vulkan_fault_requires_quarantine(VulkanImportFault::ReleaseSubmitFailed) {
                    self.quarantine();
                }
                Err(error.to_string())
            }
        }
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl Drop for VulkanImportTicket {
    fn drop(&mut self) {
        let (lock_poisoned, device_lost, stats) = match self.resource.get_mut() {
            Ok(resource) => (
                false,
                resource.as_ref().is_some_and(|resource| {
                    resource
                        .sync
                        .as_ref()
                        .is_some_and(ImportedImageSync::is_device_lost)
                }),
                resource
                    .as_ref()
                    .and_then(|resource| resource.stats.clone()),
            ),
            Err(poisoned) => {
                let resource = poisoned.into_inner();
                (
                    true,
                    resource.as_ref().is_some_and(|resource| {
                        resource
                            .sync
                            .as_ref()
                            .is_some_and(ImportedImageSync::is_device_lost)
                    }),
                    resource
                        .as_ref()
                        .and_then(|resource| resource.stats.clone()),
                )
            }
        };
        if lock_poisoned || device_lost {
            self.mark_quarantined(stats.as_deref());
        }
        if device_lost
            && !self.device_loss_recorded.swap(true, Ordering::AcqRel)
            && let Some(stats) = stats.as_deref()
        {
            stats.record_vulkan_video_device_lost();
        }

        let disposition = vulkan_ticket_resource_disposition(
            lock_poisoned,
            self.quarantined.load(Ordering::Acquire),
            device_lost,
        );
        let resource = match self.resource.get_mut() {
            Ok(resource) => resource,
            Err(poisoned) => poisoned.into_inner(),
        };
        if disposition == VulkanTicketResourceDisposition::RetainInProcessQuarantine
            && let Some(resource) = resource.take()
        {
            // Failed ownership release, lock poison, or device-loss completion is unknowable.
            // Transfer every uncertain child and its canonical lease to the one bounded process
            // owner. Its terminal flag rejects later Vulkan runtimes until process restart.
            retain_vulkan_quarantined_resource(resource);
        }
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanRetiredImport {
    ticket: Arc<VulkanImportTicket>,
    retired_at: Instant,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
pub(crate) struct VulkanPlanarVideoFrame {
    effect: RuntimeEffect,
    luma_image: Image,
    chroma_image: Image,
    _luma_texture: gpu::BackendTexture,
    _chroma_texture: gpu::BackendTexture,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanPlanarVideoFrame {
    pub(crate) fn shader(&self, tile_modes: (TileMode, TileMode)) -> Result<Shader, String> {
        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        let luma = self
            .luma_image
            .to_shader(tile_modes, sampling, None)
            .ok_or_else(|| "failed to create Vulkan NV12 luma shader".to_string())?;
        let chroma = self
            .chroma_image
            .to_shader(tile_modes, sampling, None)
            .ok_or_else(|| "failed to create Vulkan NV12 chroma shader".to_string())?;
        self.effect
            .make_shader(
                Data::new_empty(),
                &[ChildPtr::Shader(luma), ChildPtr::Shader(chroma)],
                None,
            )
            .ok_or_else(|| "failed to create exact Vulkan NV12 runtime shader".to_string())
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
pub(crate) enum VulkanVideoFrameContent<'a> {
    Image(&'a Image),
    Nv12Planes(&'a VulkanPlanarVideoFrame),
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
enum VulkanOwnedVideoFrameContent {
    Image {
        image: Image,
        _backend_texture: gpu::BackendTexture,
    },
    Nv12Planes(VulkanPlanarVideoFrame),
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanDisplayedVideoFrame {
    content: VulkanOwnedVideoFrameContent,
    ticket: Arc<VulkanImportTicket>,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanDisplayedVideoFrame {
    fn content(&self) -> VulkanVideoFrameContent<'_> {
        match &self.content {
            VulkanOwnedVideoFrameContent::Image { image, .. } => {
                VulkanVideoFrameContent::Image(image)
            }
            VulkanOwnedVideoFrameContent::Nv12Planes(planes) => {
                VulkanVideoFrameContent::Nv12Planes(planes)
            }
        }
    }

    fn into_ticket(self) -> Arc<VulkanImportTicket> {
        let Self { content, ticket } = self;
        // Ganesh wrappers must disappear before the ownership release is submitted.
        drop(content);
        ticket
    }

    fn retire(
        self,
        render_frame: &mut RenderFrame<'_>,
        retired: &mut VecDeque<VulkanRetiredImport>,
    ) {
        let ticket = self.into_ticket();
        render_frame.register_post_flush_task(ticket.clone());
        retired.push_back(VulkanRetiredImport {
            ticket,
            retired_at: Instant::now(),
        });
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
struct VulkanRenderedVideoTarget {
    spec: VideoTargetSpec,
    incarnation: u64,
    stream_id: Option<u64>,
    current: Option<VulkanDisplayedVideoFrame>,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
enum VulkanFrameImport {
    Ready(VulkanDisplayedVideoFrame),
    RejectedAfterAcquire {
        ticket: Arc<VulkanImportTicket>,
        error: String,
    },
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
fn import_vulkan_frame(
    target_id: &str,
    stream_incarnation: u64,
    frame: PrimeFrame,
    render_frame: &mut RenderFrame<'_>,
    context: &VulkanVideoImportContext,
    gr_context: &mut gpu::DirectContext,
) -> Result<VulkanFrameImport, String> {
    let process_admission = match VulkanImportAdmission::acquire() {
        Ok(admission) => admission,
        Err(VulkanProcessAdmissionError::Terminal) => {
            if let Some(stats) = frame.stats.as_deref() {
                stats.record_vulkan_video_global_quarantine_terminal();
            }
            return Err(
                "Vulkan video importer is process-terminal after uncertain GPU ownership; restart the VM/process"
                    .to_string(),
            );
        }
        Err(VulkanProcessAdmissionError::Saturated) => {
            if let Some(stats) = frame.stats.as_deref() {
                stats.record_vulkan_video_import_cap_saturation();
            }
            return Err("process-wide Vulkan video import cap is saturated".to_string());
        }
    };
    let (allocation, color_type, alpha_type) = match frame.format {
        format @ (DRM_FORMAT_ABGR8888 | DRM_FORMAT_XRGB8888) => {
            let label = if format == DRM_FORMAT_XRGB8888 {
                "XRGB8888"
            } else {
                "ABGR8888"
            };
            if frame.objects.len() != 1 || frame.planes.len() != 1 {
                return Err(format!(
                    "Vulkan {label} import requires one object and one plane, got {} object(s) and {} plane(s)",
                    frame.objects.len(),
                    frame.planes.len()
                ));
            }
            let object = frame.object(0)?;
            let plane = frame.plane(0)?;
            if plane.obj_idx != 0 {
                return Err(format!("Vulkan {label} plane must reference object zero"));
            }
            let source_size = object.size.ok_or_else(|| {
                format!(
                    "Vulkan {label} object has unknown allocation size; target allocation facts are required"
                )
            })?;
            let modifier = object.modifier.ok_or_else(|| {
                "Vulkan DMA-BUF import requires an explicit DRM modifier; implicit modifier is unsupported"
                    .to_string()
            })?;
            let (packed_format, import_strategy, color_type, alpha_type) =
                if format == DRM_FORMAT_XRGB8888 {
                    (
                        PackedImageFormat::Bgra8888,
                        context.bgra_import_strategy()?,
                        ColorType::BGRA8888,
                        AlphaType::Opaque,
                    )
                } else {
                    (
                        PackedImageFormat::Rgba8888,
                        PackedImageImportStrategy::DirectSampledImage,
                        ColorType::RGBA8888,
                        AlphaType::Premul,
                    )
                };
            (
                ImportedDmaBufImage::from_interop(context.importer().import_packed_with_strategy(
                    PackedImageImport {
                        stream_incarnation,
                        dimensions: (frame.width, frame.height),
                        source_fd: object.fd.as_raw_fd(),
                        source_size,
                        modifier,
                        plane: ImportedPlane {
                            offset: plane.offset,
                            pitch: plane.pitch,
                        },
                        format: packed_format,
                    },
                    import_strategy,
                )?),
                color_type,
                alpha_type,
            )
        }
        DRM_FORMAT_NV12 => {
            let stream_format = frame.stream_format.ok_or_else(|| {
                "Vulkan NV12 requires an immutable canonical stream format; legacy raw descriptors are unsupported"
                    .to_string()
            })?;
            let conversion = map_nv12_colorimetry(stream_format.colorimetry)?;
            let object_sizes = frame
                .objects
                .iter()
                .enumerate()
                .map(|(index, object)| {
                    object.size.ok_or_else(|| {
                        format!(
                            "Vulkan NV12 object {index} has unknown allocation size; target allocation facts are required"
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let modifiers = frame
                .objects
                .iter()
                .map(|object| object.modifier)
                .collect::<Vec<_>>();
            let planes = frame
                .planes
                .iter()
                .map(|plane| Nv12Plane {
                    object_index: plane.obj_idx,
                    offset: plane.offset,
                    pitch: plane.pitch,
                })
                .collect::<Vec<_>>();
            let layout = validate_nv12_shared_object_topology(
                (frame.width, frame.height),
                &object_sizes,
                &modifiers,
                &planes,
            )?;
            let object = frame.object(0)?;
            let capability = context.nv12_capability(
                layout.modifier,
                (frame.width, frame.height),
                conversion,
            )?;
            let topology = layout.frame_topology((frame.width, frame.height));
            let recipe = capability.allocation_recipe();
            context.validate_nv12_topology(topology, recipe)?;
            if frame.acquire_fence.is_none() {
                return Err("Vulkan NV12 requires one acquire SYNC_FD".to_string());
            }
            let allocation =
                ImportedDmaBufImage::from_interop(context.importer().import_nv12_shared_object(
                    stream_incarnation,
                    (frame.width, frame.height),
                    object.fd.as_raw_fd(),
                    layout,
                    conversion,
                    capability.interop(),
                )?);
            // A successful exact import/allocation construction on the selected physical device
            // pins both producer topology and the adapter-selected import recipe for this session.
            if context.attest_nv12_topology(topology, recipe)? {
                eprintln!(
                    "Vulkan NV12 runtime allocation proof established: device={:?} topology={topology:?} strategy={:?}",
                    context.device().identity(),
                    capability.import_strategy(),
                );
            }
            let color_type = match allocation.interop().sampled_format() {
                // Skia represents Vulkan sampler-YCbCr textures as opaque RGB. Passing RGBA8888
                // makes BorrowTextureFrom reject an otherwise valid multi-planar image.
                video_interop::vulkan::SampledImageFormat::Nv12 => ColorType::RGB888x,
                video_interop::vulkan::SampledImageFormat::Rgba8888 => ColorType::RGBA8888,
                video_interop::vulkan::SampledImageFormat::Bgra8888 => ColorType::BGRA8888,
                // Separate planes are wrapped independently below; this value is not used.
                video_interop::vulkan::SampledImageFormat::Nv12Planes => ColorType::RGB888x,
            };
            (allocation, color_type, AlphaType::Opaque)
        }
        fourcc => {
            return Err(format!(
                "Vulkan video import does not support DRM format {fourcc:#x}"
            ));
        }
    };
    let content = match allocation
        .make_staged_nv12_backend_textures(&format!("video-vulkan:{target_id}"))?
    {
        Some(textures) => {
            let effect = context.runtime_effect(textures.conversion)?;
            let luma_image = Image::from_texture(
                gr_context,
                &textures.luma,
                SurfaceOrigin::TopLeft,
                ColorType::R8UNorm,
                AlphaType::Opaque,
                None,
            )
            .ok_or_else(|| {
                format!("failed to wrap Vulkan NV12 luma image for target {target_id}")
            })?;
            let chroma_image = Image::from_texture(
                gr_context,
                &textures.chroma,
                SurfaceOrigin::TopLeft,
                ColorType::R8G8UNorm,
                AlphaType::Opaque,
                None,
            )
            .ok_or_else(|| {
                format!("failed to wrap Vulkan NV12 chroma image for target {target_id}")
            })?;
            VulkanOwnedVideoFrameContent::Nv12Planes(VulkanPlanarVideoFrame {
                effect,
                luma_image,
                chroma_image,
                _luma_texture: textures.luma,
                _chroma_texture: textures.chroma,
            })
        }
        None => {
            let backend_texture =
                allocation.make_backend_texture(&format!("video-vulkan:{target_id}"))?;
            let image = Image::from_texture(
                gr_context,
                &backend_texture,
                SurfaceOrigin::TopLeft,
                color_type,
                alpha_type,
                None,
            )
            .ok_or_else(|| {
                format!(
                    "failed to wrap Vulkan video image for target {target_id}: sampled_format={:?} color_type={color_type:?} size={}x{}",
                    allocation.interop().sampled_format(),
                    frame.width,
                    frame.height
                )
            })?;
            VulkanOwnedVideoFrameContent::Image {
                image,
                _backend_texture: backend_texture,
            }
        }
    };
    let acquire_sync_fd = frame.acquire_fence.as_ref().map(AsRawFd::as_raw_fd);
    let stats = frame.stats.clone();
    let sync = match context.checkout_sync() {
        Ok(sync) => sync,
        Err(error) => {
            if let Some(stats) = stats.as_deref()
                && error.kind() == ImportedImageSyncErrorKind::ReleaseFenceCreate
            {
                stats.record_vulkan_video_release_fence_error();
            }
            return Err(error.to_string());
        }
    };
    if let Some(stats) = stats.as_deref() {
        stats.record_vulkan_video_release_fence_created();
    }
    let ticket = Arc::new(VulkanImportTicket {
        resource: Mutex::new(Some(VulkanImportedResource {
            sync: Some(sync),
            sync_pool: Arc::clone(&context.sync_pool),
            allocation,
            frame: Some(frame),
            stats,
            _process_admission: process_admission,
        })),
        timing: Mutex::new(None),
        source_submitted_at: Instant::now(),
        quarantined: AtomicBool::new(false),
        device_loss_recorded: AtomicBool::new(false),
    });
    // The ticket owns the canonical frame before the first queue submission whose completion can
    // become uncertain. Device-loss unwind therefore quarantines rather than retiring the lease.
    let acquire_result = {
        let mut resource = ticket
            .resource
            .lock()
            .map_err(|_| "Vulkan imported-image ticket lock poisoned".to_string())?;
        let resource = resource
            .as_mut()
            .ok_or_else(|| "Vulkan imported-image ticket is quarantined".to_string())?;
        // SAFETY: the ticket owns both allocation and sync lane until the exact release fence
        // completes or the complete resource is retained in process-wide quarantine.
        unsafe {
            resource
                .sync
                .as_mut()
                .ok_or_else(|| "Vulkan imported-image sync was already recycled".to_string())?
                .submit_acquire(resource.allocation.interop(), acquire_sync_fd)
        }
    };
    let ready = match acquire_result {
        Ok(ready) => ready,
        Err(error) => {
            ticket.record_sync_error(&error);
            let fault = if error.is_device_lost() {
                VulkanImportFault::DeviceLost
            } else {
                VulkanImportFault::AcquireSubmitFailed
            };
            if vulkan_fault_requires_quarantine(fault) {
                ticket.quarantine();
            }
            return Err(error.to_string());
        }
    };
    if acquire_sync_fd.is_some() {
        ticket.record_stats(RendererStatsCollector::record_vulkan_video_acquire_sync_fd_imported);
    }
    ticket.record_stats(RendererStatsCollector::record_vulkan_video_ownership_acquire_submitted);
    let wait_accepted = wait_surface_on_semaphore(render_frame.surface_mut(), ready);
    if !wait_accepted {
        debug_assert!(!vulkan_fault_requires_quarantine(
            VulkanImportFault::AcquireFenceRejected
        ));
        ticket.record_stats(RendererStatsCollector::record_vulkan_video_ganesh_wait_rejected);
        ticket.ganesh_wait_rejected(ready)?;
        drop(content);
        return Ok(VulkanFrameImport::RejectedAfterAcquire {
            ticket,
            error: format!(
                "Skia rejected imported Vulkan image acquire wait for target {target_id}"
            ),
        });
    }
    ticket.ganesh_wait_accepted(ready)?;
    ticket.record_imported()?;
    Ok(VulkanFrameImport::Ready(VulkanDisplayedVideoFrame {
        content,
        ticket,
    }))
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

        if let Some(stats) = imported.stats().as_deref() {
            stats.record_video_retired_gl_finish_fallback();
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
                    retired.wait_blocking(&self.spec.id, true);
                    true
                }
                Err(RetiredImportPollError::WaitFailed) => {
                    eprintln!(
                        "video sync failed: glClientWaitSync WAIT_FAILED for target={}; forcing blocking cleanup",
                        self.spec.id
                    );
                    retired.wait_blocking(&self.spec.id, true);
                    true
                }
                Err(RetiredImportPollError::UnexpectedStatus(status)) => {
                    eprintln!(
                        "video sync failed: glClientWaitSync returned unexpected status={status:#x} for target={}; forcing blocking cleanup",
                        self.spec.id
                    );
                    retired.wait_blocking(&self.spec.id, true);
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
            // Teardown waits are intentionally excluded from the runtime fallback counter.
            retired.wait_blocking(&self.spec.id, false);
        }
    }

    fn image(&self) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        self.image.as_ref().map(|image| {
            (
                RenderedVideoFrame::Image(image),
                self.spec.width,
                self.spec.height,
            )
        })
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
    for pixel in pixels.as_chunks::<4>().0 {
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
fn clear_scissored_rect(x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
    unsafe {
        gl::Scissor(x, y, width.max(1), height.max(1));
        gl::ClearColor(color[0], color[1], color[2], color[3]);
        gl::Clear(gl::COLOR_BUFFER_BIT);
    }
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
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

#[cfg_attr(
    not(any(feature = "linux-opengl", feature = "vulkan", test)),
    allow(dead_code)
)]
pub(crate) enum RenderedVideoFrame<'a> {
    Image(&'a Image),
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    Nv12Planes(&'a VulkanPlanarVideoFrame),
}

#[cfg(any(
    test,
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux"),
    all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    )
))]
fn rendered_target_matches_registration(
    incarnation: u64,
    stream_id: Option<u64>,
    registered: Option<(u64, Option<u64>)>,
) -> bool {
    registered == Some((incarnation, stream_id))
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
#[derive(Default)]
struct VulkanRendererVideoState {
    targets: HashMap<String, VulkanRenderedVideoTarget>,
    blocked_stale: Vec<VulkanDisplayedVideoFrame>,
    retired: VecDeque<VulkanRetiredImport>,
    streams_pending_eviction: HashSet<u64>,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core")
))]
impl VulkanRendererVideoState {
    fn sync_pending(
        &mut self,
        registry: &Arc<VideoRegistry>,
        render_frame: &mut RenderFrame<'_>,
        gr_context: &mut gpu::DirectContext,
        context: &VulkanVideoImportContext,
    ) -> Result<VideoSyncResult, String> {
        self.reap_source_leases()?;
        let initial_cleanup = self.reap_retired()?;
        let blocked = std::mem::take(&mut self.blocked_stale);
        let retirement_capacity =
            MAX_RETIRED_VULKAN_VIDEO_IMPORTS.saturating_sub(self.retired.len());
        let mut blocked = blocked.into_iter();
        blocked
            .by_ref()
            .take(retirement_capacity)
            .for_each(|current| current.retire(render_frame, &mut self.retired));
        self.blocked_stale = blocked.collect();
        let can_import = self.total_import_capacity_available();
        let mut snapshot = registry.snapshot_for_sync(can_import)?;
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
        let stale = self
            .targets
            .iter()
            .filter(|(id, target)| {
                !rendered_target_matches_registration(
                    target.incarnation,
                    target.stream_id,
                    registered.get(*id).copied(),
                )
            })
            .map(|(id, _target)| id.clone())
            .collect::<Vec<_>>();
        let mut resources_changed = initial_cleanup.resources_changed || !stale.is_empty();
        stale.into_iter().for_each(|id| {
            if let Some(mut target) = self.targets.remove(&id) {
                self.streams_pending_eviction
                    .insert(target.stream_id.unwrap_or(target.incarnation));
                if let Some(current) = target.current.take() {
                    if can_enqueue_vulkan_retirement(self.retired.len()) {
                        current.retire(render_frame, &mut self.retired);
                    } else {
                        // Remove stale identity/pixels immediately, but retain ownership outside the
                        // bounded retired queue until a later frame has retirement capacity.
                        self.blocked_stale.push(current);
                    }
                }
            }
        });

        snapshot
            .targets
            .iter()
            .filter(|target| target.active)
            .for_each(|target| {
                self.targets
                    .entry(target.spec.id.clone())
                    .or_insert_with(|| VulkanRenderedVideoTarget {
                        spec: target.spec.clone(),
                        incarnation: target.incarnation,
                        stream_id: target.active_stream,
                        current: None,
                    });
            });

        let mut imported_frames = 0;
        let mut imported_streams = Vec::new();
        let mut newest_import_submitted_at = None;
        for pending in snapshot.pending.drain(..) {
            let imported_stream = canonical_import_identity(registry.renderer_epoch, &pending);
            let replacing_current = self
                .targets
                .get(&pending.id)
                .is_some_and(|target| target.current.is_some());
            if !self.import_capacity_available(replacing_current) {
                if let Some(stats) = registry.stats.as_deref() {
                    stats.record_vulkan_video_import_cap_saturation();
                }
                registry.defer_release(pending.frame);
                continue;
            }
            let target = self.targets.get_mut(&pending.id).ok_or_else(|| {
                format!(
                    "video target disappeared during Vulkan sync: {}",
                    pending.id
                )
            })?;
            if target.incarnation != pending.incarnation {
                registry.defer_release(pending.frame);
                continue;
            }
            if target.stream_id != pending.frame.stream_id {
                registry.defer_release(pending.frame);
                continue;
            }
            let submitted_at = pending.frame.submitted_at;
            let stream_incarnation = pending.frame.stream_id.unwrap_or(target.incarnation);
            match import_vulkan_frame(
                &pending.id,
                stream_incarnation,
                pending.frame,
                render_frame,
                context,
                gr_context,
            ) {
                Ok(VulkanFrameImport::Ready(imported)) => {
                    if let Some(previous) = target.current.replace(imported) {
                        previous.retire(render_frame, &mut self.retired);
                    }
                    imported_frames += 1;
                    if let Some(imported_stream) = imported_stream {
                        imported_streams.push(imported_stream);
                    }
                    newest_import_submitted_at = Some(
                        newest_import_submitted_at
                            .map(|current: Instant| current.max(submitted_at))
                            .unwrap_or(submitted_at),
                    );
                    resources_changed = true;
                }
                Ok(VulkanFrameImport::RejectedAfterAcquire { ticket, error }) => {
                    eprintln!(
                        "video frame import dropped for target={}: {error}",
                        pending.id
                    );
                    render_frame.register_post_flush_task(ticket.clone());
                    self.retired.push_back(VulkanRetiredImport {
                        ticket,
                        retired_at: Instant::now(),
                    });
                }
                Err(error) => {
                    // The failed candidate is dropped while the last valid displayed frame stays.
                    eprintln!(
                        "video frame import dropped for target={}: {error}",
                        pending.id
                    );
                }
            }
        }

        self.reap_source_leases()?;
        let cleanup = self.reap_retired()?;
        resources_changed |= cleanup.resources_changed;
        let evictable = self
            .streams_pending_eviction
            .iter()
            .copied()
            .filter(|stream| context.evict_stream(*stream).is_ok())
            .collect::<Vec<_>>();
        evictable.into_iter().for_each(|stream| {
            self.streams_pending_eviction.remove(&stream);
        });
        self.record_gauges(registry);
        context.record_pool_stats(registry.stats.as_deref());
        Ok(VideoSyncResult {
            resources_changed,
            needs_cleanup: cleanup.needs_cleanup
                || !self.retired.is_empty()
                || self.has_pending_source_leases(),
            imported_frames,
            imported_streams,
            newest_import_submitted_at,
            first_frame_diagnostics: None,
        })
    }

    fn has_pending_source_leases(&self) -> bool {
        self.targets
            .values()
            .filter_map(|target| target.current.as_ref())
            .map(|frame| &frame.ticket)
            .chain(self.blocked_stale.iter().map(|frame| &frame.ticket))
            .chain(self.retired.iter().map(|retired| &retired.ticket))
            .any(|ticket| ticket.staged_source_pending())
    }

    fn reap_source_leases(&self) -> Result<(), String> {
        let tickets = self
            .targets
            .values()
            .filter_map(|target| target.current.as_ref())
            .map(|frame| &frame.ticket)
            .chain(self.blocked_stale.iter().map(|frame| &frame.ticket))
            .chain(self.retired.iter().map(|retired| &retired.ticket));
        for ticket in tickets {
            if let Err(error) = ticket.source_release_complete() {
                ticket.quarantine();
                return Err(error);
            }
        }
        Ok(())
    }

    fn current_import_count(&self) -> usize {
        self.targets
            .values()
            .filter(|target| target.current.is_some())
            .count()
    }

    fn total_import_capacity_available(&self) -> bool {
        self.current_import_count()
            .saturating_add(self.blocked_stale.len())
            .saturating_add(self.retired.len())
            < MAX_RETIRED_VULKAN_VIDEO_IMPORTS
    }

    fn import_capacity_available(&self, replacing_current: bool) -> bool {
        vulkan_import_capacity_available(
            self.current_import_count(),
            self.blocked_stale.len(),
            self.retired.len(),
            replacing_current,
        )
    }

    fn reap_retired(&mut self) -> Result<VideoCleanupResult, String> {
        let retired_count = self.retired.len();
        let mut resources_changed = false;
        for _ in 0..retired_count {
            let retired = self
                .retired
                .pop_front()
                .expect("Vulkan retired import count changed during poll");
            match retired.ticket.release_complete() {
                Ok(true) => {
                    if let Some(timing) = retired.ticket.take_timing() {
                        retired.ticket.record_stats(|stats| {
                            stats.record_vulkan_video_gpu_timing(
                                timing.conversion_ns,
                                timing.composition_ns,
                                timing.total_gpu_ns,
                            );
                        });
                    }
                    resources_changed = true;
                }
                Ok(false) if !vulkan_retirement_timed_out(retired.retired_at.elapsed()) => {
                    self.retired.push_back(retired);
                }
                Ok(false) => {
                    retired.ticket.record_stats(
                        RendererStatsCollector::record_vulkan_video_retirement_timeout,
                    );
                    if vulkan_fault_requires_quarantine(VulkanImportFault::RetirementTimeout) {
                        retired.ticket.quarantine();
                    }
                    self.retired.push_back(retired);
                    return Err("Vulkan imported-image retirement timed out".to_string());
                }
                Err(error) => {
                    retired.ticket.quarantine();
                    self.retired.push_back(retired);
                    return Err(error);
                }
            }
        }
        Ok(VideoCleanupResult {
            resources_changed,
            needs_cleanup: !self.retired.is_empty(),
        })
    }

    fn prepare_shutdown(&mut self) -> Result<(), String> {
        let current = self
            .targets
            .values_mut()
            .filter_map(|target| target.current.take())
            .chain(std::mem::take(&mut self.blocked_stale))
            .map(|current| VulkanRetiredImport {
                ticket: current.into_ticket(),
                retired_at: Instant::now(),
            })
            .collect::<Vec<_>>();
        self.retired.extend(current);

        // The pre-shutdown device-idle wait makes it safe to repair any ticket registered before
        // a failed Ganesh flush prevented its post-flush task from running. Keep every ticket in
        // renderer state throughout this pass so no error can unwind a graphics-owned lease.
        let mut first_error = None;
        for retired in &self.retired {
            if let Err(error) = retired.ticket.ensure_release_submitted() {
                retired.ticket.quarantine();
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn record_gauges(&self, registry: &VideoRegistry) {
        let direct = self
            .targets
            .values()
            .filter(|target| target.current.is_some())
            .count()
            .saturating_add(self.blocked_stale.len());
        registry.record_import_gauges(direct, self.retired.len());
    }

    fn image(&self, id: &str) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        self.targets.get(id).and_then(|target| {
            target.current.as_ref().map(|current| {
                let content = match current.content() {
                    VulkanVideoFrameContent::Image(image) => RenderedVideoFrame::Image(image),
                    VulkanVideoFrameContent::Nv12Planes(planes) => {
                        RenderedVideoFrame::Nv12Planes(planes)
                    }
                };
                (content, target.spec.width, target.spec.height)
            })
        })
    }
}

struct CpuRenderedVideoFrame {
    generation: u64,
    image: Image,
    width: u32,
    height: u32,
}

fn sync_cpu_frames(
    rendered: &mut HashMap<String, CpuRenderedVideoFrame>,
    registry: &Arc<VideoRegistry>,
) -> Result<bool, String> {
    let snapshot = registry.cpu_frame_snapshot()?;
    let before = rendered.len();
    rendered.retain(|id, _frame| snapshot.contains_key(id));
    let mut changed = rendered.len() != before;

    for (id, frame) in snapshot {
        if rendered
            .get(&id)
            .is_some_and(|current| current.generation == frame.generation)
        {
            continue;
        }
        let info = skia_safe::ImageInfo::new(
            (frame.width as i32, frame.height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let image = skia_safe::images::raster_from_data(
            &info,
            skia_safe::Data::new_copy(frame.rgba.as_ref()),
            frame.width as usize * 4,
        )
        .ok_or_else(|| format!("failed to create binary video image for target {id}"))?;
        rendered.insert(
            id,
            CpuRenderedVideoFrame {
                generation: frame.generation,
                image,
                width: frame.width,
                height: frame.height,
            },
        );
        changed = true;
    }
    Ok(changed)
}

fn cpu_image<'a>(
    rendered: &'a HashMap<String, CpuRenderedVideoFrame>,
    id: &str,
) -> Option<(RenderedVideoFrame<'a>, u32, u32)> {
    rendered.get(id).map(|frame| {
        (
            RenderedVideoFrame::Image(&frame.image),
            frame.width,
            frame.height,
        )
    })
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
#[derive(Default)]
pub struct RendererVideoState {
    cpu: HashMap<String, CpuRenderedVideoFrame>,
    targets: HashMap<String, RenderedVideoTarget>,
    #[cfg(all(
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    vulkan: VulkanRendererVideoState,
}

#[cfg(any(
    all(feature = "wayland", target_os = "linux"),
    all(feature = "drm", target_os = "linux")
))]
impl RendererVideoState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_cpu(&mut self, registry: &Arc<VideoRegistry>) -> Result<bool, String> {
        sync_cpu_frames(&mut self.cpu, registry)
    }

    pub fn sync_pending(
        &mut self,
        registry: &Arc<VideoRegistry>,
        gr_context: &mut gpu::DirectContext,
        ctx: Option<&VideoImportContext>,
    ) -> Result<VideoSyncResult, String> {
        let cpu_changed = self.sync_cpu(registry)?;
        let initial_cleanup = self.reap_retired_imports(registry);
        let mut needs_cleanup = initial_cleanup.needs_cleanup;
        if let Some(ctx) = ctx {
            needs_cleanup |= ctx.retry_acquire_cleanup();
        }

        let mut snapshot = registry.snapshot_for_sync(ctx.is_some())?;
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
            cpu_changed || initial_cleanup.resources_changed || self.targets.len() != before;

        for target in snapshot.targets.iter().filter(|target| target.active) {
            let id = &target.spec.id;
            if !self.targets.contains_key(id) {
                let rendered = match RenderedVideoTarget::new(
                    target.spec.clone(),
                    target.incarnation,
                    target.active_stream,
                    gr_context,
                    import_path,
                ) {
                    Ok(rendered) => rendered,
                    Err(error) => {
                        snapshot
                            .pending
                            .drain(..)
                            .for_each(|pending| registry.defer_release(pending.frame));
                        return Err(error);
                    }
                };
                self.targets.insert(id.clone(), rendered);
                resources_changed = true;
            }
        }

        let mut imported_frames = 0;
        let mut imported_streams = Vec::new();
        let mut newest_import_submitted_at = None;
        let mut first_frame_diagnostics = None;
        if let Some(ctx) = ctx {
            for pending in snapshot.pending {
                let imported_stream = canonical_import_identity(registry.renderer_epoch, &pending);
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
                if let Some(imported_stream) = imported_stream {
                    imported_streams.push(imported_stream);
                }
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
        if let Some(ctx) = ctx {
            needs_cleanup |= ctx.has_acquire_cleanup();
        }

        Ok(VideoSyncResult {
            resources_changed,
            needs_cleanup,
            imported_frames,
            imported_streams,
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

    #[cfg(all(
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    pub fn sync_pending_vulkan(
        &mut self,
        registry: &Arc<VideoRegistry>,
        render_frame: &mut RenderFrame<'_>,
        gr_context: &mut gpu::DirectContext,
        context: &VulkanVideoImportContext,
    ) -> Result<VideoSyncResult, String> {
        self.vulkan
            .sync_pending(registry, render_frame, gr_context, context)
    }

    #[cfg(all(
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    pub fn prepare_vulkan_shutdown(&mut self) -> Result<(), String> {
        self.vulkan.prepare_shutdown()
    }

    #[cfg(all(
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    pub fn reap_retired_vulkan_imports(
        &mut self,
        registry: &Arc<VideoRegistry>,
    ) -> Result<VideoCleanupResult, String> {
        self.vulkan.reap_source_leases()?;
        let mut cleanup = self.vulkan.reap_retired()?;
        cleanup.needs_cleanup |= self.vulkan.has_pending_source_leases();
        self.vulkan.record_gauges(registry);
        Ok(cleanup)
    }

    pub fn image(&self, id: &str) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        if let Some(image) = cpu_image(&self.cpu, id) {
            return Some(image);
        }
        #[cfg(all(
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        if let Some(image) = self.vulkan.image(id) {
            return Some(image);
        }
        self.targets.get(id).and_then(RenderedVideoTarget::image)
    }
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core"),
    not(any(feature = "wayland", feature = "drm"))
))]
#[derive(Default)]
pub struct RendererVideoState {
    cpu: HashMap<String, CpuRenderedVideoFrame>,
    vulkan: VulkanRendererVideoState,
}

#[cfg(all(
    target_os = "linux",
    feature = "vulkan",
    any(feature = "wayland-core", feature = "drm-core"),
    not(any(feature = "wayland", feature = "drm"))
))]
impl RendererVideoState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_cpu(&mut self, registry: &Arc<VideoRegistry>) -> Result<bool, String> {
        sync_cpu_frames(&mut self.cpu, registry)
    }

    pub fn sync_pending(
        &mut self,
        registry: &Arc<VideoRegistry>,
        _gr_context: &mut gpu::DirectContext,
        _ctx: Option<&VideoImportContext>,
    ) -> Result<VideoSyncResult, String> {
        let resources_changed = self.sync_cpu(registry)?;
        registry.drain_pending_to_release()?;
        registry.record_import_gauges(0, self.vulkan.retired.len());
        Ok(VideoSyncResult {
            resources_changed,
            ..VideoSyncResult::default()
        })
    }

    pub fn reap_retired_imports(&mut self, registry: &Arc<VideoRegistry>) -> VideoCleanupResult {
        self.vulkan.record_gauges(registry);
        VideoCleanupResult::default()
    }

    pub fn sync_pending_vulkan(
        &mut self,
        registry: &Arc<VideoRegistry>,
        render_frame: &mut RenderFrame<'_>,
        gr_context: &mut gpu::DirectContext,
        context: &VulkanVideoImportContext,
    ) -> Result<VideoSyncResult, String> {
        self.vulkan
            .sync_pending(registry, render_frame, gr_context, context)
    }

    pub fn prepare_vulkan_shutdown(&mut self) -> Result<(), String> {
        self.vulkan.prepare_shutdown()
    }

    pub fn reap_retired_vulkan_imports(
        &mut self,
        registry: &Arc<VideoRegistry>,
    ) -> Result<VideoCleanupResult, String> {
        self.vulkan.reap_source_leases()?;
        let mut cleanup = self.vulkan.reap_retired()?;
        cleanup.needs_cleanup |= self.vulkan.has_pending_source_leases();
        self.vulkan.record_gauges(registry);
        Ok(cleanup)
    }

    pub fn image(&self, id: &str) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        cpu_image(&self.cpu, id).or_else(|| self.vulkan.image(id))
    }
}

#[cfg(all(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    not(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))
))]
#[derive(Default)]
pub struct RendererVideoState {
    cpu: HashMap<String, CpuRenderedVideoFrame>,
}

#[cfg(all(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    not(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))
))]
impl RendererVideoState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_cpu(&mut self, registry: &Arc<VideoRegistry>) -> Result<bool, String> {
        sync_cpu_frames(&mut self.cpu, registry)
    }

    #[allow(dead_code)]
    pub fn sync_pending(
        &mut self,
        registry: &Arc<VideoRegistry>,
        _gr_context: &mut gpu::DirectContext,
        _ctx: Option<&VideoImportContext>,
    ) -> Result<VideoSyncResult, String> {
        let resources_changed = self.sync_cpu(registry)?;
        registry.drain_pending_to_release()?;
        registry.record_import_gauges(0, 0);
        Ok(VideoSyncResult {
            resources_changed,
            ..VideoSyncResult::default()
        })
    }

    pub fn reap_retired_imports(&mut self, registry: &Arc<VideoRegistry>) -> VideoCleanupResult {
        registry.record_import_gauges(0, 0);
        VideoCleanupResult::default()
    }

    pub fn image(&self, id: &str) -> Option<(RenderedVideoFrame<'_>, u32, u32)> {
        cpu_image(&self.cpu, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    use crate::backend::vulkan::{DrmNodeId, VulkanDeviceIdentity};
    use crate::stats::RendererTimingMetric;
    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    use ash::vk;
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
    fn retired_import_wait_failed_requires_gl_finish_fallback() {
        assert!(retired_import_wait_needs_gl_finish(gl::WAIT_FAILED));
        assert!(!retired_import_wait_needs_gl_finish(gl::ALREADY_SIGNALED));
        assert!(!retired_import_wait_needs_gl_finish(
            gl::CONDITION_SATISFIED
        ));
    }

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

    #[test]
    fn canonical_submission_disposition_drops_only_inactive_targets() {
        assert_eq!(
            canonical_submit_disposition(true),
            CanonicalSubmitDisposition::Queue
        );
        assert_eq!(
            canonical_submit_disposition(false),
            CanonicalSubmitDisposition::DropInactive
        );
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

    fn stream_format(
        format: u32,
        modifier_policy: StreamModifierPolicy,
        acquire_sync_policy: StreamAcquireSyncPolicy,
    ) -> VideoStreamFormat {
        VideoStreamFormat {
            width: 64,
            height: 32,
            framerate: None,
            fourcc: format,
            modifier_policy,
            acquire_sync_policy,
            colorimetry: Colorimetry::default(),
            pixel_aspect_ratio: (1, 1),
            interlace_mode: InteropInterlaceMode::Progressive,
            alpha_mode: InteropAlphaMode::Opaque,
        }
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
            stream_format: None,
            submitted_at: Instant::now(),
            stats: None,
            drop_signal: None,
        }
    }

    #[test]
    fn canonical_import_identity_uses_pending_stream_and_rejects_legacy_frames() {
        let spec = VideoTargetSpec {
            id: "preview".to_string(),
            width: 64,
            height: 32,
            mode: VideoMode::Prime,
        };
        let mut frame = test_prime_frame(64, 32);
        frame.stream_id = Some(19);
        let canonical = PendingVideoFrame {
            id: spec.id.clone(),
            spec: spec.clone(),
            incarnation: 5,
            frame,
        };
        assert_eq!(
            canonical_import_identity(7, &canonical),
            Some(VideoStreamIdentity {
                renderer_epoch: 7,
                target_id: "preview".to_string(),
                target_incarnation: 5,
                stream_id: 19,
            })
        );

        let legacy = PendingVideoFrame {
            id: spec.id.clone(),
            spec,
            incarnation: 5,
            frame: test_prime_frame(64, 32),
        };
        assert_eq!(canonical_import_identity(7, &legacy), None);
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
        for format in [DRM_FORMAT_ABGR8888, DRM_FORMAT_XRGB8888, DRM_FORMAT_NV12] {
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
    fn canonical_xrgb8888_requires_one_object_and_complete_packed_span() {
        let mut extra_object = canonical_owned_frame(64, 32, DRM_FORMAT_XRGB8888);
        let OwnedStorage::DmaBuf(descriptor) = &mut extra_object.storage else {
            unreachable!("test frame is DMA-BUF")
        };
        descriptor.objects.push(OwnedObject {
            fd: File::open("/dev/null").expect("open /dev/null").into(),
            size: 64,
            modifier: video_interop::Modifier::Implicit,
        });
        assert!(
            PrimeFrame::validate_canonical(
                "preview",
                VideoMode::Prime,
                64,
                32,
                DRM_FORMAT_XRGB8888,
                &extra_object,
            )
            .unwrap_err()
            .contains("requires one object")
        );

        let mut undersized = canonical_owned_frame(64, 32, DRM_FORMAT_XRGB8888);
        let OwnedStorage::DmaBuf(descriptor) = &mut undersized.storage else {
            unreachable!("test frame is DMA-BUF")
        };
        descriptor.objects[0].size -= 1;
        assert!(
            PrimeFrame::validate_canonical(
                "preview",
                VideoMode::Prime,
                64,
                32,
                DRM_FORMAT_XRGB8888,
                &undersized,
            )
            .unwrap_err()
            .contains("requires 8192 bytes")
        );
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
        let registry = test_registry(release_tx, None);
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
        let registry = test_registry(release_tx, None);
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");

        let error = registry
            .submit_prime_exact_if_available("preview", incarnation, test_prime_frame(64, 32))
            .expect_err("unavailable import should reject the frame");

        assert_eq!(error, prime_video_unavailable_error());
        let released = release_rx.try_recv().expect("expected released frame");
        assert_eq!((released.width, released.height), (64, 32));
    }

    #[test]
    fn disabling_prime_video_atomically_drains_a_pending_frame() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_prime_video_available(true)
            .expect("availability should update");
        let incarnation = registry
            .create_target_if_available(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");
        registry
            .submit_prime_exact_if_available("preview", incarnation, test_prime_frame(64, 32))
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
        let registry = Arc::new(test_registry(release_tx, None));
        registry
            .set_prime_video_available(true)
            .expect("availability should update");
        let incarnation = registry
            .create_target_if_available(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .expect("target should be created");
        activate_target(&registry, "preview");

        for _ in 0..32 {
            registry
                .set_prime_video_available(true)
                .expect("availability should update");
            let barrier = Arc::new(std::sync::Barrier::new(3));
            let submit_registry = Arc::clone(&registry);
            let submit_barrier = Arc::clone(&barrier);
            let submit = thread::spawn(move || {
                submit_barrier.wait();
                let _ = submit_registry.submit_prime_exact_if_available(
                    "preview",
                    incarnation,
                    test_prime_frame(64, 32),
                );
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
    fn stream_modifier_policy_accepts_per_buffer_and_exact_nonzero_contracts() {
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
        StreamModifierPolicy::Explicit(1)
            .validate_frame(&frame)
            .expect("an exact nonzero negotiated/frame modifier match must be accepted");
        assert!(
            StreamModifierPolicy::Explicit(2)
                .validate_frame(&frame)
                .unwrap_err()
                .contains("does not match negotiated DRM modifier 0x0000000000000002")
        );
        StreamModifierPolicy::PerBuffer
            .validate_frame(&frame)
            .expect("per-buffer policy must preserve explicit non-linear modifiers");
    }

    #[test]
    fn stream_acquire_sync_policy_rejects_mismatches_before_claim() {
        let mut frame = canonical_owned_frame(64, 32, DRM_FORMAT_ABGR8888);

        StreamAcquireSyncPolicy::PerFrame
            .validate_frame(&frame)
            .expect("per-frame policy should accept implicit sync");
        StreamAcquireSyncPolicy::Implicit
            .validate_frame(&frame)
            .expect("implicit policy should accept implicit sync");
        assert!(
            StreamAcquireSyncPolicy::SyncFile
                .validate_frame(&frame)
                .unwrap_err()
                .contains("does not match negotiated sync-file")
        );

        frame.acquire_sync =
            OwnedAcquireSync::SyncFile(File::open("/dev/null").expect("open /dev/null").into());
        StreamAcquireSyncPolicy::PerFrame
            .validate_frame(&frame)
            .expect("per-frame policy should accept sync files");
        StreamAcquireSyncPolicy::SyncFile
            .validate_frame(&frame)
            .expect("sync-file policy should accept sync files");
        assert!(
            StreamAcquireSyncPolicy::Implicit
                .validate_frame(&frame)
                .unwrap_err()
                .contains("does not match negotiated implicit")
        );
    }

    #[test]
    fn renderer_stream_requirements_reject_weaker_vulkan_contracts() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_prime_stream_requirements(Some(PrimeStreamRequirements::Vulkan))
            .expect("requirements should configure");
        #[cfg(all(
            target_os = "linux",
            feature = "vulkan",
            any(feature = "wayland-core", feature = "drm-core")
        ))]
        registry
            .set_vulkan_import_capabilities(true, true, Vec::new())
            .expect("test device should advertise linear ABGR import");
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
                    stream_format(
                        DRM_FORMAT_ABGR8888,
                        StreamModifierPolicy::PerBuffer,
                        StreamAcquireSyncPolicy::SyncFile,
                    ),
                )
                .unwrap_err()
                .contains("explicit linear modifier")
        );
        assert!(
            registry
                .open_stream(
                    "preview",
                    incarnation,
                    stream_format(
                        DRM_FORMAT_ABGR8888,
                        StreamModifierPolicy::Explicit(0),
                        StreamAcquireSyncPolicy::PerFrame,
                    ),
                )
                .unwrap_err()
                .contains("acquire_sync")
        );
        assert!(
            registry
                .open_stream(
                    "preview",
                    incarnation,
                    stream_format(
                        DRM_FORMAT_ABGR8888,
                        StreamModifierPolicy::Explicit(0),
                        StreamAcquireSyncPolicy::SyncFile,
                    ),
                )
                .is_ok()
        );
    }

    #[test]
    fn legacy_raw_prime_is_deferred_and_rejected_when_vulkan_requirements_are_active() {
        let (release_tx, release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_prime_stream_requirements(Some(PrimeStreamRequirements::Vulkan))
            .unwrap();
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .unwrap();
        activate_target(&registry, "preview");

        let error = registry
            .submit_prime_exact("preview", incarnation, test_prime_frame(64, 32))
            .unwrap_err();
        assert!(error.contains("legacy raw PRIME submission is unavailable for Vulkan video"));
        let released = release_rx.try_recv().expect("raw frame must be deferred");
        assert_eq!((released.width, released.height), (64, 32));
        assert!(registry.snapshot_pending().unwrap().pending.is_empty());
    }

    #[test]
    fn complete_stream_format_is_immutable_in_native_active_stream_state() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .unwrap();
        let mut format = stream_format(
            DRM_FORMAT_ABGR8888,
            StreamModifierPolicy::Explicit(0),
            StreamAcquireSyncPolicy::SyncFile,
        );
        format.framerate = Some((60, 1));
        format.pixel_aspect_ratio = (4, 3);
        format.colorimetry = Colorimetry {
            primaries: video_interop::Primaries::Bt709,
            transfer: video_interop::Transfer::Bt709,
            matrix: video_interop::Matrix::Bt709,
            range: video_interop::ColorRange::Full,
            chroma_location: video_interop::ChromaLocation::Center,
        };

        let stream_id = registry
            .open_stream("preview", incarnation, format)
            .unwrap();
        let active = registry
            .state
            .lock()
            .unwrap()
            .targets
            .get("preview")
            .unwrap()
            .active_stream
            .unwrap();
        assert_eq!(active.id, stream_id);
        assert_eq!(active.format, format);
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    fn generic_nv12_capability(modifier: u64) -> Nv12ModifierCapability {
        let identity = VulkanDeviceIdentity {
            primary_node: Some(DrmNodeId {
                major: 226,
                minor: 0,
            }),
            render_node: Some(DrmNodeId {
                major: 226,
                minor: 128,
            }),
            vendor_id: 0x14e4,
            device_id: 0x2712,
            device_uuid: [1; vk::UUID_SIZE],
            driver_id: Some(vk::DriverId::MESA_V3DV.as_raw()),
            driver_version: 1,
            driver_uuid: [2; vk::UUID_SIZE],
        };
        let features = vk::FormatFeatureFlags::SAMPLED_IMAGE
            | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR
            | vk::FormatFeatureFlags::SAMPLED_IMAGE_YCBCR_CONVERSION_LINEAR_FILTER
            | vk::FormatFeatureFlags::MIDPOINT_CHROMA_SAMPLES
            | vk::FormatFeatureFlags::COSITED_CHROMA_SAMPLES;
        Nv12ModifierCapability::from_interop(
            identity,
            video_interop::vulkan::Nv12ModifierCapability {
                modifier,
                strategy: video_interop::vulkan::Nv12ImportStrategy::DirectSampledImage,
                modifier_plane_count: 2,
                source_tiling_features: features,
                sampled_tiling_features: features,
                external_features: vk::ExternalMemoryFeatureFlags::IMPORTABLE,
                compatible_handle_types: vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                max_extent: vk::Extent3D {
                    width: 4096,
                    height: 4096,
                    depth: 1,
                },
            },
        )
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_nv12_stream_rejects_unspecified_or_unsupported_contract_before_open() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_prime_stream_requirements(Some(PrimeStreamRequirements::Vulkan))
            .unwrap();
        registry
            .set_vulkan_import_capabilities(false, false, vec![generic_nv12_capability(0)])
            .unwrap();
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .unwrap();
        let mut format = stream_format(
            DRM_FORMAT_NV12,
            StreamModifierPolicy::Explicit(0),
            StreamAcquireSyncPolicy::SyncFile,
        );
        assert!(
            registry
                .open_stream("preview", incarnation, format)
                .unwrap_err()
                .contains("explicit primaries")
        );

        format.colorimetry = Colorimetry {
            primaries: video_interop::Primaries::Bt709,
            transfer: video_interop::Transfer::Bt709,
            matrix: video_interop::Matrix::Bt709,
            range: video_interop::ColorRange::Limited,
            chroma_location: video_interop::ChromaLocation::Left,
        };
        let rgba_format = VideoStreamFormat {
            fourcc: DRM_FORMAT_ABGR8888,
            modifier_policy: StreamModifierPolicy::Explicit(0),
            colorimetry: Colorimetry::default(),
            alpha_mode: InteropAlphaMode::Premultiplied,
            ..format
        };
        assert!(
            registry
                .open_stream("preview", incarnation, rgba_format)
                .unwrap_err()
                .contains("ABGR8888 linear DMA-BUF sampling is unavailable")
        );

        format.modifier_policy = StreamModifierPolicy::Explicit(99);
        assert!(
            registry
                .open_stream("preview", incarnation, format)
                .unwrap_err()
                .contains("no active-device import candidate")
        );

        format.modifier_policy = StreamModifierPolicy::Explicit(0);
        assert!(registry.open_stream("preview", incarnation, format).is_ok());
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_xrgb_stream_requires_exact_rgb_full_range_contract() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_prime_stream_requirements(Some(PrimeStreamRequirements::Vulkan))
            .unwrap();
        registry
            .set_vulkan_import_capabilities(false, true, Vec::new())
            .unwrap();
        let incarnation = registry
            .create_target(VideoTargetSpec {
                id: "preview".to_string(),
                width: 64,
                height: 32,
                mode: VideoMode::Prime,
            })
            .unwrap();
        let mut format = stream_format(
            DRM_FORMAT_XRGB8888,
            StreamModifierPolicy::Explicit(0),
            StreamAcquireSyncPolicy::SyncFile,
        );
        format.colorimetry = Colorimetry {
            primaries: video_interop::Primaries::Bt709,
            transfer: video_interop::Transfer::Bt709,
            matrix: video_interop::Matrix::Rgb,
            range: video_interop::ColorRange::Full,
            chroma_location: video_interop::ChromaLocation::Unspecified,
        };
        assert!(registry.open_stream("preview", incarnation, format).is_ok());

        registry.close_stream("preview", incarnation, 1);
        format.colorimetry.range = video_interop::ColorRange::Limited;
        assert!(
            registry
                .open_stream("preview", incarnation, format)
                .unwrap_err()
                .contains("Rec.709/Rec.709/RGB/full")
        );
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_nv12_frame_contract_and_runtime_attestation_are_both_exact() {
        let mut format = stream_format(
            DRM_FORMAT_NV12,
            StreamModifierPolicy::Explicit(0),
            StreamAcquireSyncPolicy::SyncFile,
        );
        format.colorimetry = Colorimetry {
            primaries: video_interop::Primaries::Bt709,
            transfer: video_interop::Transfer::Bt709,
            matrix: video_interop::Matrix::Bt709,
            range: video_interop::ColorRange::Limited,
            chroma_location: video_interop::ChromaLocation::Center,
        };
        let mut frame = canonical_owned_frame(64, 32, DRM_FORMAT_NV12);
        let OwnedStorage::DmaBuf(descriptor) = &mut frame.storage else {
            unreachable!()
        };
        descriptor.objects[0].modifier = Modifier::Explicit(0);
        let capabilities = [generic_nv12_capability(0)];
        validate_vulkan_frame_contract(format, &frame, Some(&capabilities)).unwrap();

        let identity = capabilities[0].active_device_identity;
        let mut attestations = Nv12RuntimeAttestations::default();
        let proven_topology = Nv12FrameTopology {
            dimensions: (64, 32),
            object_count: 1,
            object_size: 3_072,
            plane_count: 2,
            planes: [
                Nv12Plane {
                    object_index: 0,
                    offset: 0,
                    pitch: 64,
                },
                Nv12Plane {
                    object_index: 0,
                    offset: 2_048,
                    pitch: 64,
                },
            ],
            modifier: 0,
        };
        let recipe = capabilities[0].allocation_recipe();
        assert!(
            attestations
                .record(identity, proven_topology, recipe)
                .unwrap()
        );
        assert!(
            !attestations
                .record(identity, proven_topology, recipe)
                .unwrap()
        );
        assert!(
            attestations
                .validate(
                    identity,
                    Nv12FrameTopology {
                        object_size: 4_096,
                        ..proven_topology
                    },
                    recipe,
                )
                .unwrap_err()
                .contains("does not exactly match the target proof")
        );

        let OwnedStorage::DmaBuf(descriptor) = &mut frame.storage else {
            unreachable!()
        };
        descriptor.objects[0].size = 4_096;
        validate_vulkan_frame_contract(format, &frame, Some(&capabilities))
            .expect("generic frame admission validates layout without fabricating import proof");
        let OwnedStorage::DmaBuf(descriptor) = &mut frame.storage else {
            unreachable!()
        };
        descriptor.objects[0].size = 3_072;

        let second_fd: OwnedFd = File::open("/dev/null").unwrap().into();
        let OwnedStorage::DmaBuf(descriptor) = &mut frame.storage else {
            unreachable!()
        };
        descriptor.objects.push(OwnedObject {
            fd: second_fd,
            size: 1_024,
            modifier: Modifier::Explicit(0),
        });
        descriptor.layers[0].planes[1].object_index = 1;
        assert!(
            validate_vulkan_frame_contract(format, &frame, Some(&capabilities))
                .unwrap_err()
                .contains("exactly one object")
        );
    }

    #[test]
    fn direct_opengl_stream_accepts_explicit_non_linear_modifier() {
        let (release_tx, _release_rx) = unbounded();
        let registry = test_registry(release_tx, None);
        registry
            .set_prime_video_available(true)
            .expect("PRIME video should become available");

        let (incarnation, stream_id) = registry
            .ensure_direct_stream(
                "preview",
                stream_format(
                    DRM_FORMAT_NV12,
                    StreamModifierPolicy::Explicit(0x0200_0000_1040_1b04),
                    StreamAcquireSyncPolicy::SyncFile,
                ),
            )
            .expect("OpenGL defers exact modifier support to EGL image import");
        let snapshot = registry
            .snapshot_for_sync(false)
            .expect("registry snapshot");
        assert_eq!(snapshot.targets[0].incarnation, incarnation);
        assert_eq!(snapshot.targets[0].active_stream, Some(stream_id));
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

        let mut aligned_tail = canonical_owned_frame(64, 32, DRM_FORMAT_ABGR8888);
        let OwnedStorage::DmaBuf(descriptor) = &mut aligned_tail.storage else {
            panic!("test frame must use DMA-BUF storage");
        };
        descriptor.objects[0].size += 2_048;
        PrimeFrame::validate_canonical(
            "preview",
            VideoMode::Prime,
            64,
            32,
            DRM_FORMAT_ABGR8888,
            &aligned_tail,
        )
        .expect("allocation padding after the packed plane span should validate");

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

        assert!(error.contains("supported formats are NV12, ABGR8888, and XRGB8888"));
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
                    stream_format(
                        DRM_FORMAT_NV12,
                        StreamModifierPolicy::PerBuffer,
                        StreamAcquireSyncPolicy::PerFrame,
                    ),
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
                stream_format(
                    DRM_FORMAT_NV12,
                    StreamModifierPolicy::PerBuffer,
                    StreamAcquireSyncPolicy::PerFrame,
                ),
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
                    stream_format(
                        DRM_FORMAT_NV12,
                        StreamModifierPolicy::PerBuffer,
                        StreamAcquireSyncPolicy::PerFrame,
                    ),
                )
                .is_ok()
        );
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
                stream_format(
                    DRM_FORMAT_NV12,
                    StreamModifierPolicy::PerBuffer,
                    StreamAcquireSyncPolicy::PerFrame,
                ),
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

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_retirement_policy_is_hard_bounded_and_times_out() {
        assert!(can_enqueue_vulkan_retirement(0));
        assert!(can_enqueue_vulkan_retirement(
            MAX_RETIRED_VULKAN_VIDEO_IMPORTS - 1
        ));
        assert!(!can_enqueue_vulkan_retirement(
            MAX_RETIRED_VULKAN_VIDEO_IMPORTS
        ));
        assert!(vulkan_import_capacity_available(2, 2, 3, false));
        assert!(!vulkan_import_capacity_available(2, 2, 4, true));
        assert!(!vulkan_import_capacity_available(7, 0, 0, false));
        assert!(vulkan_import_capacity_available(7, 0, 0, true));
        assert!(!vulkan_import_capacity_available(8, 0, 0, true));
        assert!(!vulkan_retirement_timed_out(
            VULKAN_VIDEO_RETIRE_TIMEOUT - Duration::from_nanos(1)
        ));
        assert!(vulkan_retirement_timed_out(VULKAN_VIDEO_RETIRE_TIMEOUT));
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn poisoned_vulkan_ticket_never_selects_normal_resource_drop() {
        assert_eq!(
            vulkan_ticket_resource_disposition(true, false, false),
            VulkanTicketResourceDisposition::RetainInProcessQuarantine
        );
        assert_eq!(
            vulkan_ticket_resource_disposition(true, true, false),
            VulkanTicketResourceDisposition::RetainInProcessQuarantine
        );
        assert_eq!(
            vulkan_ticket_resource_disposition(false, false, true),
            VulkanTicketResourceDisposition::RetainInProcessQuarantine
        );
        assert_eq!(
            vulkan_ticket_resource_disposition(false, true, false),
            VulkanTicketResourceDisposition::RetainInProcessQuarantine
        );
        assert_eq!(
            vulkan_ticket_resource_disposition(false, false, false),
            VulkanTicketResourceDisposition::NormalDrop
        );
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_process_quarantine_is_bounded_and_rejects_in_vm_restart_admission() {
        let mut presenter_terminal = VulkanQuarantinePolicy::default();
        presenter_terminal.mark_terminal();
        assert_eq!(
            presenter_terminal.admit(),
            Err(VulkanProcessAdmissionError::Terminal),
            "uncertain presenter ownership must reject every later Vulkan runtime"
        );

        let mut policy = VulkanQuarantinePolicy::default();
        for _ in 0..MAX_RETIRED_VULKAN_VIDEO_IMPORTS {
            policy.admit().expect("capacity should admit");
        }
        assert_eq!(policy.admit(), Err(VulkanProcessAdmissionError::Saturated));
        policy.release();
        policy
            .admit()
            .expect("released capacity should be reusable");

        for _ in 0..MAX_RETIRED_VULKAN_VIDEO_IMPORTS {
            assert!(policy.reserve_quarantine_slot());
        }
        assert!(!policy.reserve_quarantine_slot());
        assert!(policy.terminal);
        assert_eq!(policy.live_imports, MAX_RETIRED_VULKAN_VIDEO_IMPORTS);
        assert_eq!(
            policy.admit(),
            Err(VulkanProcessAdmissionError::Terminal),
            "a renderer restart in the same VM must remain rejected"
        );
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_fault_policy_preserves_rejected_candidates_and_quarantines_uncertain_release() {
        assert!(!vulkan_fault_requires_quarantine(
            VulkanImportFault::AcquireFenceRejected
        ));
        assert!(!vulkan_fault_requires_quarantine(
            VulkanImportFault::AcquireSubmitFailed
        ));
        assert!(vulkan_fault_requires_quarantine(
            VulkanImportFault::ReleaseSubmitFailed
        ));
        assert!(vulkan_fault_requires_quarantine(
            VulkanImportFault::RetirementTimeout
        ));
        assert!(vulkan_fault_requires_quarantine(
            VulkanImportFault::DeviceLost
        ));
        // A delayed fence remains pending and bounded until the watchdog threshold.
        assert!(!vulkan_retirement_timed_out(Duration::from_millis(250)));
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn vulkan_nv12_staging_preference_is_explicit_and_fail_closed() {
        assert_eq!(
            vulkan_nv12_staging_preference_from_value(None),
            Ok(Nv12StagingPreference::PreferPlanar)
        );
        assert_eq!(
            vulkan_nv12_staging_preference_from_value(Some("auto")),
            Ok(Nv12StagingPreference::PreferPlanar)
        );
        assert_eq!(
            vulkan_nv12_staging_preference_from_value(Some("planar")),
            Ok(Nv12StagingPreference::RequirePlanar)
        );
        assert_eq!(
            vulkan_nv12_staging_preference_from_value(Some("rgba")),
            Ok(Nv12StagingPreference::RequireRgba)
        );
        assert!(vulkan_nv12_staging_preference_from_value(Some("fallback")).is_err());
    }

    #[cfg(all(
        target_os = "linux",
        feature = "vulkan",
        any(feature = "wayland-core", feature = "drm-core")
    ))]
    #[test]
    fn exact_nv12_runtime_effect_compiles_for_supported_range_and_siting() {
        for range in [YcbcrRange::Narrow, YcbcrRange::Full] {
            for x_offset in [YcbcrOffset::CositedEven, YcbcrOffset::Midpoint] {
                for y_offset in [YcbcrOffset::CositedEven, YcbcrOffset::Midpoint] {
                    let effect = make_nv12_runtime_effect(Nv12Conversion {
                        model: YcbcrModel::Bt709,
                        range,
                        x_offset,
                        y_offset,
                    });
                    assert!(effect.is_ok(), "runtime effect failed: {effect:?}");
                }
            }
        }
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
