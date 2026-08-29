//! Isolated, opt-in no-WSI DRM/Vulkan functional probe.
//!
//! This is diagnostic process code, not production presenter admission. It deliberately exercises
//! only the plan-preferred GBM scanout allocation imported into Vulkan. A failed or uncertain
//! operation terminates the probe path and never enables the DRM Vulkan runtime.

use std::{
    collections::{HashMap, HashSet},
    mem,
    os::fd::{AsFd, AsRawFd, OwnedFd},
    sync::Arc,
    time::{Duration, Instant},
};

use ash::vk;
use drm::{
    ClientCapability, Device as BasicDevice,
    control::{
        AtomicCommitFlags, Device as ControlDevice, FbCmd2Flags, atomic, framebuffer, property,
    },
};
use gbm::{BufferObject, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat, Modifier};
use sha2::{Digest, Sha256};
use skia_safe::{Color, Paint, Rect};
use video_interop::dmabuf_allocation_size;

use crate::{
    backend::vulkan::{
        AcquiredTarget, DMA_BUF_EXTERNAL_QUEUE_FAMILY, ExactDeviceRequirements,
        ExportableSyncFdSemaphore, ImportedDmaBufImage, ImportedPlane, TargetImageState,
        VulkanDevice, VulkanEngine, VulkanInstance, VulkanTargetFormat, VulkanTargetSurface,
        validate_bgra_scanout_import_support,
    },
    renderer::RendererCacheConfig,
};

use super::core::{
    AtomicCommitErrorKind, KmsOutputProbe, classify_atomic_commit_error,
    open_vulkan_selection_node, probe_kms_output, prop_handle,
};

const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes([b'X', b'R', b'2', b'4']);
const MAX_PAGE_FLIP_TIMEOUT: Duration = Duration::from_secs(60);
const REQUIRED_EXTENSIONS: [&std::ffi::CStr; 5] = [
    ash::khr::external_memory_fd::NAME,
    ash::ext::external_memory_dma_buf::NAME,
    ash::ext::image_drm_format_modifier::NAME,
    ash::khr::external_semaphore_fd::NAME,
    ash::ext::physical_device_drm::NAME,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationDirection {
    GbmImportedIntoVulkan,
    VulkanExportedToKms,
}

impl AllocationDirection {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "gbm-import" => Ok(Self::GbmImportedIntoVulkan),
            "vulkan-export" => Ok(Self::VulkanExportedToKms),
            other => Err(format!(
                "unsupported allocation direction {other:?}; expected gbm-import or vulkan-export"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::GbmImportedIntoVulkan => "gbm-import",
            Self::VulkanExportedToKms => "vulkan-export",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FunctionalProbeConfig {
    pub drm_card: String,
    pub vulkan_drm_node: String,
    pub requested_size: Option<(u32, u32)>,
    pub allocation_direction: AllocationDirection,
    pub page_flip_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct FunctionalProbeReport {
    pub dimensions: (u32, u32),
    pub connector_id: u32,
    pub encoder_id: u32,
    pub crtc_id: u32,
    pub plane_id: u32,
    pub modifier: u64,
    pub pitch: u32,
    pub offset: u64,
    pub object_size: u64,
    pub commit_attempts: u32,
    pub ebusy_retries: u32,
    pub page_flip_sequence: u32,
    pub gpu_fence_signaled: bool,
    pub capture_exact: bool,
    pub capture_sha256: String,
    pub cleanup_complete: bool,
}

pub fn run(config: &FunctionalProbeConfig) -> Result<FunctionalProbeReport, String> {
    if config.page_flip_timeout.is_zero() {
        return Err("functional probe page-flip timeout must be non-zero".to_string());
    }
    if config.page_flip_timeout > MAX_PAGE_FLIP_TIMEOUT
        || Instant::now()
            .checked_add(config.page_flip_timeout)
            .is_none()
    {
        return Err(format!(
            "functional probe page-flip timeout exceeds the bounded {} ms maximum",
            MAX_PAGE_FLIP_TIMEOUT.as_millis()
        ));
    }
    if config.allocation_direction == AllocationDirection::VulkanExportedToKms {
        return Err(
            "vulkan-export allocation experiment is not implemented or target-proven; no alternate direction was attempted"
                .to_string(),
        );
    }

    let mut kms = probe_kms_output(Some(&config.drm_card), config.requested_size)?;
    if !kms.primary_supports_xrgb8888 {
        return Err("selected KMS primary plane does not advertise XRGB8888".to_string());
    }
    if !kms.primary_has_in_fence_fd {
        return Err("selected KMS primary plane has no IN_FENCE_FD".to_string());
    }
    kms.card
        .acquire_master_lock()
        .map_err(|error| format!("functional probe could not acquire DRM master: {error}"))?;
    kms.card
        .set_client_capability(ClientCapability::UniversalPlanes, true)
        .map_err(|error| format!("functional probe could not enable universal planes: {error}"))?;
    kms.card
        .set_client_capability(ClientCapability::Atomic, true)
        .map_err(|error| {
            format!("functional probe could not enable atomic modesetting: {error}")
        })?;

    let result = run_with_master(&mut kms, config);
    let release_result = kms.card.release_master_lock();
    match (result, release_result) {
        (Ok(report), Ok(())) => Ok(report),
        (Ok(_), Err(error)) => Err(format!(
            "functional probe completed but failed to release DRM master: {error}"
        )),
        (Err(error), _) => Err(error),
    }
}

fn run_with_master(
    kms: &mut KmsOutputProbe,
    config: &FunctionalProbeConfig,
) -> Result<FunctionalProbeReport, String> {
    if !kms.resources.connectors().contains(&kms.connector)
        || !kms.resources.encoders().contains(&kms.encoder)
        || !kms.resources.crtcs().contains(&kms.crtc)
    {
        return Err(
            "selected KMS output handles are absent from the retained resource snapshot"
                .to_string(),
        );
    }
    let selection = open_vulkan_selection_node(&config.vulkan_drm_node)?;
    let instance = VulkanInstance::new(&[])?;
    let device = VulkanDevice::new_for_drm_node(
        Arc::clone(&instance),
        ExactDeviceRequirements {
            required_extensions: &REQUIRED_EXTENSIONS,
            require_timestamps: true,
            selection_node: selection.selection,
        },
    )?;

    let plane_properties = object_property_set(&kms.card, kms.primary)?;
    let kms_modifiers = kms_xrgb8888_modifiers(&kms.card, &plane_properties)?;
    let mut vulkan_failures = Vec::new();
    let common_modifiers = kms_modifiers
        .into_iter()
        .filter(
            |modifier| match validate_bgra_scanout_import_support(&device, *modifier) {
                Ok(()) => true,
                Err(error) => {
                    vulkan_failures.push(format!("{modifier:#018x}: {error}"));
                    false
                }
            },
        )
        .collect::<Vec<_>>();
    if common_modifiers.is_empty() {
        return Err(format!(
            "no KMS XRGB8888 modifier has Vulkan BGRA color-attachment import support: {}",
            vulkan_failures.join("; ")
        ));
    }

    let gbm = GbmDevice::new(kms.card.as_fd())
        .map_err(|error| format!("failed to create functional-probe GBM device: {error}"))?;
    let usage = BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING;
    if !gbm.is_format_supported(GbmFormat::Xrgb8888, usage) {
        return Err("GBM does not support XRGB8888 SCANOUT|RENDERING".to_string());
    }
    let bo: BufferObject<()> = gbm
        .create_buffer_object_with_modifiers2(
            kms.dimensions.0,
            kms.dimensions.1,
            GbmFormat::Xrgb8888,
            common_modifiers.iter().copied().map(Modifier::from),
            usage,
        )
        .map_err(|error| {
            format!(
                "GBM could not allocate from the KMS/Vulkan XRGB8888 modifier intersection {:?}: {error}",
                common_modifiers
            )
        })?;
    if bo.format() as u32 != DRM_FORMAT_XRGB8888 || bo.plane_count() != 1 {
        return Err(format!(
            "GBM returned unsupported scanout topology format={:?} planes={}",
            bo.format(),
            bo.plane_count()
        ));
    }
    let modifier_kind = bo.modifier();
    if modifier_kind == Modifier::Invalid {
        return Err("GBM scanout BO has no explicit DRM modifier".to_string());
    }
    let modifier = u64::from(modifier_kind);
    if !common_modifiers.contains(&modifier) {
        return Err(format!(
            "GBM returned modifier {modifier:#018x} outside the supplied KMS/Vulkan intersection"
        ));
    }
    let pitch = bo.stride_for_plane(0);
    let offset = u64::from(bo.offset(0));
    let dma_buf = bo
        .fd()
        .map_err(|error| format!("failed to export GBM BO DMA-BUF: {error}"))?;
    let object_size = dma_buf_size(&dma_buf)?;
    let framebuffer = kms
        .card
        .add_planar_framebuffer(&bo, FbCmd2Flags::MODIFIERS)
        .map_err(|error| {
            format!("modifier-aware AddFB2 rejected XRGB8888 modifier {modifier:#018x}: {error}")
        })?;

    let imported = match ImportedDmaBufImage::new_bgra_scanout(
        Arc::clone(&device),
        kms.dimensions,
        dma_buf.as_raw_fd(),
        object_size,
        modifier,
        ImportedPlane { offset, pitch },
    ) {
        Ok(imported) => imported,
        Err(error) => {
            let _ = kms.card.destroy_framebuffer(framebuffer);
            return Err(error);
        }
    };
    drop(dma_buf);

    let prepared_kms = match PreparedKmsState::new(kms, plane_properties) {
        Ok(prepared) => prepared,
        Err(error) => {
            drop(imported);
            let _ = kms.card.destroy_framebuffer(framebuffer);
            return Err(error);
        }
    };
    let submission =
        match submit_ganesh_pattern_and_external_release(&device, imported.image(), kms.dimensions)
        {
            Ok(submission) => submission,
            Err(error) => {
                let _ = prepared_kms.destroy_blobs(&kms.card);
                let _ = kms.card.destroy_framebuffer(framebuffer);
                if error.ownership_uncertain {
                    quarantine_without_submission(imported, bo, gbm);
                } else {
                    drop(imported);
                }
                return Err(error.message);
            }
        };
    let master_sync_file = match submission.export_sync_file() {
        Ok(fence) => fence,
        Err(error) => {
            quarantine(imported, submission, bo, gbm);
            return Err(error);
        }
    };

    let commit = commit_with_retries(
        kms,
        framebuffer,
        &prepared_kms.connector.infos,
        &prepared_kms.crtc.infos,
        &prepared_kms.plane.infos,
        prepared_kms.probe_mode_blob,
        &master_sync_file,
        config.page_flip_timeout,
    );
    let (commit_attempts, ebusy_retries) = match commit {
        Ok(attempts) => attempts,
        Err(error) => {
            quarantine(imported, submission, bo, gbm);
            return Err(error);
        }
    };

    let page_flip_sequence = match wait_for_page_flip(kms, config.page_flip_timeout) {
        Ok(sequence) => sequence,
        Err(error) => {
            quarantine(imported, submission, bo, gbm);
            return Err(error);
        }
    };
    let gpu_fence_signaled = match sync_file_signaled(&master_sync_file, config.page_flip_timeout) {
        Ok(signaled) => signaled,
        Err(error) => {
            quarantine(imported, submission, bo, gbm);
            return Err(error);
        }
    };
    drop(master_sync_file);
    if !gpu_fence_signaled {
        quarantine(imported, submission, bo, gbm);
        return Err("GPU release sync file was not signaled after the KMS page flip".to_string());
    }
    let capture_sha256 = submission.capture_sha256.clone();

    if let Err(error) = prepared_kms.restore(kms, config.page_flip_timeout) {
        quarantine(imported, submission, bo, gbm);
        return Err(error);
    }
    let blob_cleanup = prepared_kms.destroy_blobs(&kms.card);
    if let Err(error) = device.wait_idle("functional DRM probe cleanup") {
        quarantine(imported, submission, bo, gbm);
        return Err(error);
    }
    drop(submission);
    drop(imported);
    kms.card
        .destroy_framebuffer(framebuffer)
        .map_err(|error| format!("failed to remove functional-probe framebuffer: {error}"))?;
    drop(bo);
    drop(gbm);
    blob_cleanup?;

    Ok(FunctionalProbeReport {
        dimensions: kms.dimensions,
        connector_id: u32::from(kms.connector),
        encoder_id: u32::from(kms.encoder),
        crtc_id: u32::from(kms.crtc),
        plane_id: u32::from(kms.primary),
        modifier,
        pitch,
        offset,
        object_size,
        commit_attempts,
        ebusy_retries,
        page_flip_sequence,
        gpu_fence_signaled,
        capture_exact: true,
        capture_sha256,
        cleanup_complete: true,
    })
}

pub(super) struct ObjectPropertySet {
    infos: HashMap<String, property::Info>,
    values: HashMap<property::Handle, property::RawValue>,
}

impl ObjectPropertySet {
    pub(super) fn infos(&self) -> &HashMap<String, property::Info> {
        &self.infos
    }

    fn raw(&self, name: &str) -> Result<property::RawValue, String> {
        let handle = prop_handle(&self.infos, name)?;
        self.values
            .get(&handle)
            .copied()
            .ok_or_else(|| format!("DRM property {name} has no current value"))
    }
}

pub(super) fn object_property_set<T: drm::control::ResourceHandle + Copy>(
    card: &super::core::Card,
    object: T,
) -> Result<ObjectPropertySet, String> {
    let values = card
        .get_properties(object)
        .map_err(|error| format!("failed to inspect DRM object properties: {error}"))?;
    let infos = values
        .as_hashmap(card)
        .map_err(|error| format!("failed to inspect DRM property metadata: {error}"))?;
    Ok(ObjectPropertySet {
        infos,
        values: values.into_iter().collect(),
    })
}

pub(super) fn kms_xrgb8888_modifiers(
    card: &super::core::Card,
    plane: &ObjectPropertySet,
) -> Result<Vec<u64>, String> {
    let blob_id = plane.raw("IN_FORMATS")?;
    if blob_id == 0 {
        return Err("selected KMS primary plane has an empty IN_FORMATS blob".to_string());
    }
    let blob = card
        .get_property_blob(blob_id)
        .map_err(|error| format!("failed to read KMS IN_FORMATS blob {blob_id}: {error}"))?;
    parse_in_formats_modifiers(&blob, DRM_FORMAT_XRGB8888)
}

fn parse_in_formats_modifiers(blob: &[u8], required_format: u32) -> Result<Vec<u64>, String> {
    const HEADER_SIZE: usize = 24;
    const MODIFIER_SIZE: usize = 24;
    if blob.len() < HEADER_SIZE {
        return Err("KMS IN_FORMATS blob is shorter than its header".to_string());
    }
    let read_u32 = |offset: usize| -> Result<u32, String> {
        let bytes = blob
            .get(offset..offset + 4)
            .ok_or_else(|| "KMS IN_FORMATS u32 is out of bounds".to_string())?;
        Ok(u32::from_ne_bytes(
            bytes.try_into().expect("four-byte slice"),
        ))
    };
    let read_u64 = |offset: usize| -> Result<u64, String> {
        let bytes = blob
            .get(offset..offset + 8)
            .ok_or_else(|| "KMS IN_FORMATS u64 is out of bounds".to_string())?;
        Ok(u64::from_ne_bytes(
            bytes.try_into().expect("eight-byte slice"),
        ))
    };
    let version = read_u32(0)?;
    let flags = read_u32(4)?;
    if version != 1 || flags != 0 {
        return Err(format!(
            "unsupported KMS IN_FORMATS header version={version} flags={flags:#x}"
        ));
    }
    let format_count =
        usize::try_from(read_u32(8)?).map_err(|_| "KMS format count exceeds usize".to_string())?;
    let formats_offset = usize::try_from(read_u32(12)?)
        .map_err(|_| "KMS formats offset exceeds usize".to_string())?;
    let modifier_count = usize::try_from(read_u32(16)?)
        .map_err(|_| "KMS modifier count exceeds usize".to_string())?;
    let modifiers_offset = usize::try_from(read_u32(20)?)
        .map_err(|_| "KMS modifiers offset exceeds usize".to_string())?;
    let formats_size = format_count
        .checked_mul(4)
        .ok_or_else(|| "KMS formats array size overflow".to_string())?;
    let modifiers_size = modifier_count
        .checked_mul(MODIFIER_SIZE)
        .ok_or_else(|| "KMS modifiers array size overflow".to_string())?;
    if formats_offset
        .checked_add(formats_size)
        .is_none_or(|end| end > blob.len())
        || modifiers_offset
            .checked_add(modifiers_size)
            .is_none_or(|end| end > blob.len())
    {
        return Err("KMS IN_FORMATS arrays are out of bounds".to_string());
    }
    let format_index = (0..format_count)
        .find(|index| read_u32(formats_offset + index * 4).ok() == Some(required_format))
        .ok_or_else(|| "KMS IN_FORMATS does not contain XRGB8888".to_string())?;
    let modifiers = (0..modifier_count)
        .filter_map(|index| {
            let base = modifiers_offset + index * MODIFIER_SIZE;
            let mask = read_u64(base).ok()?;
            let offset = usize::try_from(read_u32(base + 8).ok()?).ok()?;
            let relative = format_index.checked_sub(offset)?;
            (relative < 64 && (mask & (1_u64 << relative)) != 0)
                .then(|| read_u64(base + 16).ok())
                .flatten()
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if modifiers.is_empty() {
        return Err("KMS IN_FORMATS exposes no explicit modifier for XRGB8888".to_string());
    }
    let mut modifiers = modifiers;
    modifiers.sort_unstable();
    Ok(modifiers)
}

pub(super) fn dma_buf_size(fd: &OwnedFd) -> Result<u64, String> {
    dmabuf_allocation_size(fd.as_raw_fd())
        .map_err(|error| format!("failed to query DMA-BUF object size: {error}"))
}

struct GpuSubmission {
    // Field order is intentional: drop the Skia surface before Ganesh, then retire the exported
    // semaphore. The imported image and GBM BO remain owned by the caller until after this owner.
    _surface: VulkanTargetSurface,
    _engine: VulkanEngine,
    completion: std::cell::RefCell<ExportableSyncFdSemaphore>,
    capture_sha256: String,
}

impl GpuSubmission {
    fn export_sync_file(&self) -> Result<OwnedFd, String> {
        self.completion.borrow_mut().export_submitted_sync_fd()
    }
}

struct SubmitClearError {
    message: String,
    ownership_uncertain: bool,
}

impl SubmitClearError {
    fn before_submit(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ownership_uncertain: false,
        }
    }

    fn uncertain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ownership_uncertain: true,
        }
    }
}

fn submit_ganesh_pattern_and_external_release(
    device: &Arc<VulkanDevice>,
    image: vk::Image,
    dimensions: (u32, u32),
) -> Result<GpuSubmission, SubmitClearError> {
    let mut engine = VulkanEngine::new(Arc::clone(device), RendererCacheConfig::default())
        .map_err(SubmitClearError::before_submit)?;
    let initial_state = TargetImageState {
        layout: vk::ImageLayout::UNDEFINED,
        queue_family_index: DMA_BUF_EXTERNAL_QUEUE_FAMILY,
    };
    let final_state = TargetImageState {
        layout: vk::ImageLayout::GENERAL,
        queue_family_index: DMA_BUF_EXTERNAL_QUEUE_FAMILY,
    };
    let mut surface = engine
        .create_target_surface_with_format_usage_and_tiling(
            image,
            dimensions,
            initial_state,
            VulkanTargetFormat::Bgra8888,
            crate::backend::vulkan::GANESH_TARGET_IMAGE_USAGE,
            skia_safe::gpu::vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
        )
        .map_err(SubmitClearError::before_submit)?;
    let mut completion = ExportableSyncFdSemaphore::new(Arc::clone(device))
        .map_err(SubmitClearError::before_submit)?;
    let completion_semaphore = completion
        .begin_signal()
        .map_err(SubmitClearError::before_submit)?;
    let acquired = AcquiredTarget {
        token: (),
        image,
        dimensions,
        current_state: initial_state,
        acquire_semaphore: None,
        completion_semaphore,
        final_state,
    };

    let rendered = engine.render(&mut surface, acquired, true, |_renderer, frame| {
        draw_probe_pattern(frame.surface_mut(), dimensions);
        frame.flush()
    });
    let completed = match rendered {
        Ok((_timings, completed)) => completed,
        Err(error) => {
            // Ganesh can fail after accepting work. Preserve every context-bound wrapper and the
            // exportable semaphore until process teardown instead of guessing queue ownership.
            mem::forget(surface);
            mem::forget(engine);
            mem::forget(completion);
            return Err(SubmitClearError::uncertain(format!(
                "functional-probe Ganesh render failed; ownership is uncertain: {error}"
            )));
        }
    };
    if completed.completion_semaphore != completion_semaphore
        || completed.final_state != final_state
    {
        mem::forget(surface);
        mem::forget(engine);
        mem::forget(completion);
        return Err(SubmitClearError::uncertain(
            "functional-probe Ganesh completion identity/state changed unexpectedly",
        ));
    }
    let capture = match completed.capture {
        Some(capture) => capture,
        None => {
            mem::forget(surface);
            mem::forget(engine);
            mem::forget(completion);
            return Err(SubmitClearError::uncertain(
                "functional-probe Ganesh render returned no capture",
            ));
        }
    };
    if !capture_matches_pattern(&capture.pixels, dimensions) {
        mem::forget(surface);
        mem::forget(engine);
        mem::forget(completion);
        return Err(SubmitClearError::uncertain(
            "functional-probe Ganesh capture did not match the exact RGBA quadrant pattern",
        ));
    }
    let capture_sha256 = format!("{:x}", Sha256::digest(&capture.pixels));
    Ok(GpuSubmission {
        _surface: surface,
        _engine: engine,
        completion: std::cell::RefCell::new(completion),
        capture_sha256,
    })
}

fn draw_probe_pattern(surface: &mut skia_safe::Surface, dimensions: (u32, u32)) {
    let (width, height) = (dimensions.0 as f32, dimensions.1 as f32);
    let (half_width, half_height) = ((dimensions.0 / 2) as f32, (dimensions.1 / 2) as f32);
    let mut paint = Paint::default();
    let canvas = surface.canvas();
    for (color, rect) in [
        (
            Color::from_argb(255, 235, 56, 20),
            Rect::from_xywh(0.0, 0.0, half_width, half_height),
        ),
        (
            Color::from_argb(255, 76, 175, 80),
            Rect::from_xywh(half_width, 0.0, width - half_width, half_height),
        ),
        (
            Color::from_argb(255, 33, 150, 243),
            Rect::from_xywh(0.0, half_height, half_width, height - half_height),
        ),
        (
            Color::from_argb(255, 255, 235, 59),
            Rect::from_xywh(
                half_width,
                half_height,
                width - half_width,
                height - half_height,
            ),
        ),
    ] {
        paint.set_color(color);
        canvas.draw_rect(rect, &paint);
    }
}

fn capture_matches_pattern(pixels: &[u8], dimensions: (u32, u32)) -> bool {
    let (width, height) = dimensions;
    if width == 0 || height == 0 {
        return false;
    }
    let expected_len = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(usize::try_from(height).ok()?))
        .and_then(|pixels| pixels.checked_mul(4));
    let width_usize = usize::try_from(width).unwrap_or(usize::MAX);
    expected_len == Some(pixels.len())
        && pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .all(|(index, pixel)| {
                let x = index % width_usize;
                let y = index / width_usize;
                let expected = match (
                    x < usize::try_from(width / 2).unwrap_or(0),
                    y < usize::try_from(height / 2).unwrap_or(0),
                ) {
                    (true, true) => [235, 56, 20, 255],
                    (false, true) => [76, 175, 80, 255],
                    (true, false) => [33, 150, 243, 255],
                    (false, false) => [255, 235, 59, 255],
                };
                pixel == expected
            })
}

pub(super) fn checked_deadline(timeout: Duration) -> Result<Instant, String> {
    Instant::now().checked_add(timeout).ok_or_else(|| {
        "functional probe timeout cannot be represented by the monotonic clock".to_string()
    })
}

#[allow(clippy::too_many_arguments)]
fn commit_with_retries(
    kms: &KmsOutputProbe,
    framebuffer: framebuffer::Handle,
    connector_props: &HashMap<String, property::Info>,
    crtc_props: &HashMap<String, property::Info>,
    plane_props: &HashMap<String, property::Info>,
    mode_blob: property::Value<'_>,
    master_sync_file: &OwnedFd,
    timeout: Duration,
) -> Result<(u32, u32), String> {
    let deadline = checked_deadline(timeout)?;
    let mut attempts = 0_u32;
    let mut busy = 0_u32;
    loop {
        attempts = attempts.saturating_add(1);
        let attempt_fence = master_sync_file
            .try_clone()
            .map_err(|error| format!("failed to duplicate master IN_FENCE_FD: {error}"))?;
        let mut request = atomic::AtomicModeReq::new();
        request.add_property(
            kms.connector,
            prop_handle(connector_props, "CRTC_ID")?,
            property::Value::CRTC(Some(kms.crtc)),
        );
        request.add_property(kms.crtc, prop_handle(crtc_props, "MODE_ID")?, mode_blob);
        request.add_property(
            kms.crtc,
            prop_handle(crtc_props, "ACTIVE")?,
            property::Value::Boolean(true),
        );
        add_primary_properties(kms, &mut request, framebuffer, plane_props)?;
        request.add_property(
            kms.primary,
            prop_handle(plane_props, "IN_FENCE_FD")?,
            property::Value::SignedRange(i64::from(attempt_fence.as_raw_fd())),
        );
        let result = kms.card.atomic_commit(
            AtomicCommitFlags::ALLOW_MODESET
                | AtomicCommitFlags::NONBLOCK
                | AtomicCommitFlags::PAGE_FLIP_EVENT,
            request,
        );
        drop(attempt_fence);
        match result {
            Ok(()) => return Ok((attempts, busy)),
            Err(error)
                if classify_atomic_commit_error(&error) == AtomicCommitErrorKind::Busy
                    && Instant::now() < deadline =>
            {
                busy = busy.saturating_add(1);
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => {
                return Err(format!(
                    "terminal or timed-out functional-probe atomic commit after {attempts} attempts: {error}"
                ));
            }
        }
    }
}

pub(super) fn add_primary_properties(
    kms: &KmsOutputProbe,
    request: &mut atomic::AtomicModeReq,
    framebuffer: framebuffer::Handle,
    props: &HashMap<String, property::Info>,
) -> Result<(), String> {
    let (width, height) = kms.mode.size();
    request.add_property(
        kms.primary,
        prop_handle(props, "FB_ID")?,
        property::Value::Framebuffer(Some(framebuffer)),
    );
    request.add_property(
        kms.primary,
        prop_handle(props, "CRTC_ID")?,
        property::Value::CRTC(Some(kms.crtc)),
    );
    for (name, value) in [
        ("SRC_X", property::Value::UnsignedRange(0)),
        ("SRC_Y", property::Value::UnsignedRange(0)),
        (
            "SRC_W",
            property::Value::UnsignedRange(u64::from(width) << 16),
        ),
        (
            "SRC_H",
            property::Value::UnsignedRange(u64::from(height) << 16),
        ),
        ("CRTC_X", property::Value::SignedRange(0)),
        ("CRTC_Y", property::Value::SignedRange(0)),
        ("CRTC_W", property::Value::UnsignedRange(u64::from(width))),
        ("CRTC_H", property::Value::UnsignedRange(u64::from(height))),
    ] {
        request.add_property(kms.primary, prop_handle(props, name)?, value);
    }
    Ok(())
}

fn sync_file_signaled(sync_file: &OwnedFd, timeout: Duration) -> Result<bool, String> {
    let deadline = checked_deadline(timeout)?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let timeout_ms =
            i32::try_from(remaining.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        let mut pollfd = libc::pollfd {
            fd: sync_file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let polled = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if polled < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("functional-probe sync-file poll failed: {error}"));
        }
        if polled == 0 {
            return Ok(false);
        }
        if pollfd.revents & libc::POLLIN != 0 {
            return Ok(true);
        }
        if pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("functional-probe sync file reported a terminal poll error".to_string());
        }
    }
}

fn wait_for_page_flip(kms: &KmsOutputProbe, timeout: Duration) -> Result<u32, String> {
    let deadline = checked_deadline(timeout)?;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("functional-probe KMS page-flip timeout".to_string());
        }
        let timeout_ms =
            i32::try_from(remaining.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        let mut pollfd = libc::pollfd {
            fd: kms.card.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let polled = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if polled < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("functional-probe DRM poll failed: {error}"));
        }
        if polled == 0 {
            return Err("functional-probe KMS page-flip timeout".to_string());
        }
        if pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("functional-probe DRM poll reported a terminal fd error".to_string());
        }
        for event in kms
            .card
            .receive_events()
            .map_err(|error| format!("failed to receive functional-probe DRM events: {error}"))?
        {
            if let drm::control::Event::PageFlip(page_flip) = event
                && page_flip.crtc == kms.crtc
            {
                return Ok(page_flip.frame);
            }
        }
    }
}

const CONNECTOR_RESTORE_PROPERTIES: &[&str] = &["CRTC_ID"];
const CRTC_RESTORE_PROPERTIES: &[&str] = &["MODE_ID", "ACTIVE"];
const PLANE_RESTORE_PROPERTIES: &[&str] = &[
    "FB_ID", "CRTC_ID", "SRC_X", "SRC_Y", "SRC_W", "SRC_H", "CRTC_X", "CRTC_Y", "CRTC_W", "CRTC_H",
];

struct RawProperties(Vec<(String, property::Handle, property::RawValue)>);

impl RawProperties {
    fn capture(set: &ObjectPropertySet, names: &[&str]) -> Result<Self, String> {
        names
            .iter()
            .map(|name| {
                Ok((
                    (*name).to_string(),
                    prop_handle(&set.infos, name)?,
                    set.raw(name)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()
            .map(Self)
    }

    fn add_to<H: drm::control::ResourceHandle>(
        &self,
        request: &mut atomic::AtomicModeReq,
        object: H,
        mode_blob_override: Option<u64>,
    ) {
        let object = object.into();
        for (name, handle, value) in &self.0 {
            let value = if name == "MODE_ID" {
                mode_blob_override.unwrap_or(*value)
            } else {
                *value
            };
            request.add_raw_property(object, *handle, value);
        }
    }
}

pub(super) struct PreparedKmsState {
    connector: ObjectPropertySet,
    crtc: ObjectPropertySet,
    plane: ObjectPropertySet,
    connector_restore: RawProperties,
    crtc_restore: RawProperties,
    plane_restore: RawProperties,
    restore_mode_blob_id: Option<u64>,
    pub(super) probe_mode_blob: property::Value<'static>,
    probe_mode_blob_id: u64,
}

impl PreparedKmsState {
    pub(super) fn connector_infos(&self) -> &HashMap<String, property::Info> {
        self.connector.infos()
    }

    pub(super) fn crtc_infos(&self) -> &HashMap<String, property::Info> {
        self.crtc.infos()
    }

    pub(super) fn plane_infos(&self) -> &HashMap<String, property::Info> {
        self.plane.infos()
    }

    pub(super) fn new(kms: &KmsOutputProbe, plane: ObjectPropertySet) -> Result<Self, String> {
        let connector = object_property_set(&kms.card, kms.connector)?;
        let crtc = object_property_set(&kms.card, kms.crtc)?;
        let connector_restore = RawProperties::capture(&connector, CONNECTOR_RESTORE_PROPERTIES)?;
        let crtc_restore = RawProperties::capture(&crtc, CRTC_RESTORE_PROPERTIES)?;
        let plane_restore = RawProperties::capture(&plane, PLANE_RESTORE_PROPERTIES)?;
        let restore_mode_blob_id = if crtc.raw("ACTIVE")? != 0 {
            let current_mode = kms
                .card
                .get_crtc(kms.crtc)
                .map_err(|error| format!("failed to snapshot current KMS CRTC: {error}"))?
                .mode()
                .ok_or_else(|| "active KMS CRTC has no current mode to restore".to_string())?;
            Some(create_mode_blob(&kms.card, &current_mode, "restore")?.1)
        } else {
            None
        };
        let (probe_mode_blob, probe_mode_blob_id) =
            match create_mode_blob(&kms.card, &kms.mode, "probe") {
                Ok(blob) => blob,
                Err(error) => {
                    if let Some(id) = restore_mode_blob_id {
                        let _ = kms.card.destroy_property_blob(id);
                    }
                    return Err(error);
                }
            };
        Ok(Self {
            connector,
            crtc,
            plane,
            connector_restore,
            crtc_restore,
            plane_restore,
            restore_mode_blob_id,
            probe_mode_blob,
            probe_mode_blob_id,
        })
    }

    pub(super) fn restore(&self, kms: &KmsOutputProbe, timeout: Duration) -> Result<(), String> {
        let deadline = checked_deadline(timeout)?;
        loop {
            let mut request = atomic::AtomicModeReq::new();
            self.connector_restore
                .add_to(&mut request, kms.connector, None);
            self.crtc_restore
                .add_to(&mut request, kms.crtc, self.restore_mode_blob_id);
            self.plane_restore.add_to(&mut request, kms.primary, None);
            match kms
                .card
                .atomic_commit(AtomicCommitFlags::ALLOW_MODESET, request)
            {
                Ok(()) => return Ok(()),
                Err(error)
                    if classify_atomic_commit_error(&error) == AtomicCommitErrorKind::Busy
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => {
                    return Err(format!(
                        "failed to restore pre-probe KMS state; scanout ownership is uncertain: {error}"
                    ));
                }
            }
        }
    }

    pub(super) fn destroy_blobs(&self, card: &super::core::Card) -> Result<(), String> {
        let probe_result = card.destroy_property_blob(self.probe_mode_blob_id);
        let restore_result = self
            .restore_mode_blob_id
            .map(|id| card.destroy_property_blob(id))
            .transpose();
        match (probe_result, restore_result) {
            (Ok(()), Ok(_)) => Ok(()),
            (Err(error), Ok(_)) => Err(format!("failed to destroy probe KMS mode blob: {error}")),
            (Ok(()), Err(error)) => {
                Err(format!("failed to destroy restore KMS mode blob: {error}"))
            }
            (Err(probe), Err(restore)) => Err(format!(
                "failed to destroy probe and restore KMS mode blobs: {probe}; {restore}"
            )),
        }
    }
}

fn create_mode_blob(
    card: &super::core::Card,
    mode: &drm::control::Mode,
    purpose: &str,
) -> Result<(property::Value<'static>, u64), String> {
    let value = card.create_property_blob(mode).map_err(|error| {
        format!("failed to create functional-probe {purpose} mode blob: {error}")
    })?;
    match value {
        property::Value::Blob(id) if id != 0 => Ok((value, id)),
        _ => Err(format!(
            "functional probe received an invalid {purpose} KMS mode blob"
        )),
    }
}

fn quarantine_without_submission(
    imported: ImportedDmaBufImage,
    bo: BufferObject<()>,
    gbm: GbmDevice<impl AsFd>,
) {
    mem::forget(imported);
    mem::forget(bo);
    mem::forget(gbm);
}

fn quarantine(
    imported: ImportedDmaBufImage,
    submission: GpuSubmission,
    bo: BufferObject<()>,
    gbm: GbmDevice<impl AsFd>,
) {
    // The diagnostic process exits immediately after returning the terminal error. Leak uncertain
    // userspace owners until process teardown rather than running ordinary destructors before KMS
    // or Vulkan ownership is known. Kernel fd/process teardown remains the final quarantine.
    mem::forget(imported);
    mem::forget(submission);
    mem::forget(bo);
    mem::forget(gbm);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_direction_is_explicit_and_never_auto() {
        assert_eq!(
            AllocationDirection::parse("gbm-import").unwrap(),
            AllocationDirection::GbmImportedIntoVulkan
        );
        assert_eq!(
            AllocationDirection::parse("vulkan-export").unwrap(),
            AllocationDirection::VulkanExportedToKms
        );
        assert!(AllocationDirection::parse("auto").is_err());
    }

    #[test]
    fn zero_timeout_is_rejected_before_hardware_access() {
        let config = FunctionalProbeConfig {
            drm_card: "/missing/card".to_string(),
            vulkan_drm_node: "/missing/render".to_string(),
            requested_size: None,
            allocation_direction: AllocationDirection::GbmImportedIntoVulkan,
            page_flip_timeout: Duration::ZERO,
        };
        assert!(run(&config).unwrap_err().contains("must be non-zero"));
    }

    #[test]
    fn oversized_timeout_is_rejected_before_hardware_access() {
        let config = FunctionalProbeConfig {
            drm_card: "/missing/card".to_string(),
            vulkan_drm_node: "/missing/render".to_string(),
            requested_size: None,
            allocation_direction: AllocationDirection::GbmImportedIntoVulkan,
            page_flip_timeout: Duration::MAX,
        };
        assert!(run(&config).unwrap_err().contains("bounded"));
    }

    #[test]
    fn unproven_vulkan_export_direction_fails_without_trying_gbm() {
        let config = FunctionalProbeConfig {
            drm_card: "/missing/card".to_string(),
            vulkan_drm_node: "/missing/render".to_string(),
            requested_size: None,
            allocation_direction: AllocationDirection::VulkanExportedToKms,
            page_flip_timeout: Duration::from_secs(1),
        };
        let error = run(&config).unwrap_err();
        assert!(error.contains("not implemented or target-proven"));
        assert!(error.contains("no alternate direction was attempted"));
    }

    #[test]
    fn parses_only_modifiers_whose_masks_cover_xrgb8888() {
        let mut blob = Vec::new();
        for value in [1_u32, 0, 2, 24, 2, 32] {
            blob.extend_from_slice(&value.to_ne_bytes());
        }
        blob.extend_from_slice(&DRM_FORMAT_XRGB8888.to_ne_bytes());
        blob.extend_from_slice(&0x3432_5258_u32.to_ne_bytes());
        // First modifier covers format index 0. Second starts at index 1 and does not.
        blob.extend_from_slice(&1_u64.to_ne_bytes());
        blob.extend_from_slice(&0_u32.to_ne_bytes());
        blob.extend_from_slice(&0_u32.to_ne_bytes());
        blob.extend_from_slice(&0x0102_0304_0506_0708_u64.to_ne_bytes());
        blob.extend_from_slice(&1_u64.to_ne_bytes());
        blob.extend_from_slice(&1_u32.to_ne_bytes());
        blob.extend_from_slice(&0_u32.to_ne_bytes());
        blob.extend_from_slice(&0x1112_1314_1516_1718_u64.to_ne_bytes());

        assert_eq!(
            parse_in_formats_modifiers(&blob, DRM_FORMAT_XRGB8888).unwrap(),
            vec![0x0102_0304_0506_0708]
        );
    }

    #[test]
    fn exact_capture_pattern_detects_channel_and_orientation_changes() {
        let exact = [
            [235, 56, 20, 255],
            [76, 175, 80, 255],
            [33, 150, 243, 255],
            [255, 235, 59, 255],
        ]
        .concat();
        assert!(capture_matches_pattern(&exact, (2, 2)));

        let mut wrong = exact;
        wrong.swap(0, 8);
        assert!(!capture_matches_pattern(&wrong, (2, 2)));
        assert!(!capture_matches_pattern(&wrong[..12], (2, 2)));
    }

    #[test]
    fn rejects_malformed_in_formats_bounds() {
        let mut blob = Vec::new();
        for value in [1_u32, 0, 1, 24, 1, u32::MAX] {
            blob.extend_from_slice(&value.to_ne_bytes());
        }
        blob.extend_from_slice(&DRM_FORMAT_XRGB8888.to_ne_bytes());
        assert!(
            parse_in_formats_modifiers(&blob, DRM_FORMAT_XRGB8888)
                .unwrap_err()
                .contains("out of bounds")
        );
    }
}
