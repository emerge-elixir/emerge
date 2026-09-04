//! Opt-in no-WSI DRM/KMS Vulkan presenter.
//!
//! Production admission is intentionally narrow: KMS XRGB8888 GBM allocations with explicit
//! modifiers are imported as Vulkan B8G8R8A8 targets. KMS page flips, not GPU submission, are the
//! sole scanout/reuse authority. Every atomic attempt consumes a duplicate of one retained master
//! SYNC_FD; only EBUSY is retryable.

use std::{
    io, mem,
    os::fd::{AsRawFd, OwnedFd},
    sync::{Arc, Mutex, OnceLock, atomic::Ordering},
    time::{Duration, Instant},
};

use ash::vk;
use crossbeam_channel::TrySendError;
use drm::{
    ClientCapability, Device as BasicDevice,
    control::{
        AtomicCommitFlags, Device as ControlDevice, FbCmd2Flags, atomic, framebuffer, property,
    },
};
use gbm::{BufferObject, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat, Modifier};
use skia_safe::{Paint, Rect};

use crate::{
    actors::{EventMsg, RenderMsg, TreeMsg},
    backend::vulkan::{
        AcquiredTarget, DMA_BUF_EXTERNAL_QUEUE_FAMILY, ExactDeviceRequirements,
        ExportableSyncFdSemaphore, ImportedDmaBufImage, ImportedPlane, TargetImageState,
        VulkanDevice, VulkanEngine, VulkanRendererReport, VulkanTargetFormat, VulkanTargetSurface,
        validate_bgra_scanout_import_support,
    },
    events::CursorIcon,
    renderer::{RenderState, RenderTimings},
    stats::{format_slow_render_frame_log, render_frame_has_slow_stage},
    video::{
        VideoStreamIdentity, VulkanVideoImportContext, mark_vulkan_process_quarantine_terminal,
    },
};

use super::{
    DrmBackendStartupInfo, DrmRunConfig, DrmRunContext,
    core::{
        AtomicCommitErrorKind, KmsOutputProbe, classify_atomic_commit_error, duplicate_card,
        mode_frame_interval, open_vulkan_selection_node, probe_kms_output, prop_handle,
    },
    cursor_theme::{CursorVisual, DrmCursorTheme},
    functional_probe::{
        PreparedKmsState, add_primary_properties, checked_deadline, dma_buf_size,
        kms_xrgb8888_modifiers, object_property_set,
    },
};

const MIN_SCANOUT_SLOTS: usize = 3;

fn sleep_with_stop(stop: &Arc<std::sync::atomic::AtomicBool>, duration: Duration) {
    let deadline = Instant::now() + duration;

    while !stop.load(Ordering::Acquire) {
        let now = Instant::now();
        if now >= deadline {
            break;
        }

        std::thread::sleep((deadline - now).min(Duration::from_millis(25)));
    }
}
const PAGE_FLIP_TIMEOUT: Duration = Duration::from_secs(2);
const HOTPLUG_INTERVAL: Duration = Duration::from_millis(750);
const RENDER_PROFILE_INTERVAL: Duration = Duration::from_secs(1);
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");

fn required_device_extensions() -> [&'static std::ffi::CStr; 7] {
    [
        ash::khr::external_memory_fd::NAME,
        ash::ext::external_memory_dma_buf::NAME,
        ash::ext::image_drm_format_modifier::NAME,
        ash::khr::image_format_list::NAME,
        ash::khr::external_semaphore_fd::NAME,
        ash::khr::sampler_ycbcr_conversion::NAME,
        ash::ext::physical_device_drm::NAME,
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogicalState {
    Available,
    Rendering,
    Prepared,
    CommitInFlight,
    Current,
    Retiring,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SlotRefs {
    gpu_submission: bool,
    kms_current: bool,
    capture: bool,
    timing: bool,
}

impl SlotRefs {
    fn reusable(self) -> bool {
        !self.gpu_submission && !self.kms_current && !self.capture && !self.timing
    }
}

struct ScanoutSlot {
    state: LogicalState,
    generation: Option<u64>,
    refs: SlotRefs,
    imported_video_frames: usize,
    imported_video_streams: Vec<VideoStreamIdentity>,
    master_sync_file: Option<OwnedFd>,
}

impl ScanoutSlot {
    fn available() -> Self {
        Self {
            state: LogicalState::Available,
            generation: None,
            refs: SlotRefs::default(),
            imported_video_frames: 0,
            imported_video_streams: Vec::new(),
            master_sync_file: None,
        }
    }
}

struct ImportedVideo {
    frame_count: usize,
    stream_identities: Vec<VideoStreamIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomicAttemptResult {
    Committed,
    Busy,
    Terminal(AtomicCommitErrorKind),
}

impl AtomicAttemptResult {
    fn from_io_result(result: &io::Result<()>) -> Self {
        match result {
            Ok(()) => Self::Committed,
            Err(error) => match classify_atomic_commit_error(error) {
                AtomicCommitErrorKind::Busy => Self::Busy,
                terminal => Self::Terminal(terminal),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitBusyAction {
    RetryAfter(Duration),
    Exhausted { attempts: u64 },
}

/// Per-prepared-frame retry budget. `max_retries` counts retries after the first atomic attempt.
/// Exhaustion is recoverable only after the caller proves the unaccepted GPU submission idle.
#[derive(Clone, Copy, Debug)]
struct CommitRetryPolicy {
    max_retries: u32,
    retries_remaining: u32,
    attempts: u64,
    delay: Duration,
}

fn optional_video_import<T>(result: Result<T, String>) -> (Option<T>, Option<String>) {
    match result {
        Ok(context) => (Some(context), None),
        Err(error) => (None, Some(error)),
    }
}

impl CommitRetryPolicy {
    fn new(max_retries: u32, retry_interval_ms: u32) -> Self {
        Self {
            max_retries,
            retries_remaining: max_retries,
            attempts: 0,
            delay: Duration::from_millis(u64::from(retry_interval_ms)),
        }
    }

    fn begin_prepared_frame(&mut self) {
        self.retries_remaining = self.max_retries;
        self.attempts = 0;
    }

    fn record_attempt(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
    }

    fn on_busy(&mut self) -> CommitBusyAction {
        if self.retries_remaining == 0 {
            CommitBusyAction::Exhausted {
                attempts: self.attempts,
            }
        } else {
            self.retries_remaining -= 1;
            CommitBusyAction::RetryAfter(self.delay)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PresentedHooks {
    slot: usize,
    generation: u64,
    capture: bool,
    timing: bool,
    imported_video_frames: usize,
    imported_video_streams: Vec<VideoStreamIdentity>,
}

/// Bounded persistent scanout ownership policy. Concrete Vulkan/KMS resources live in the
/// same-indexed `ConcreteSlot` inventory.
struct ScanoutLifecycle {
    slots: Vec<ScanoutSlot>,
    rendering: Option<usize>,
    prepared: Option<usize>,
    in_flight: Option<usize>,
    current: Option<usize>,
    terminal_error: Option<String>,
}

impl ScanoutLifecycle {
    fn new(slot_count: usize) -> Result<Self, String> {
        if slot_count < MIN_SCANOUT_SLOTS {
            return Err(format!(
                "DRM Vulkan requires at least {MIN_SCANOUT_SLOTS} persistent scanout slots"
            ));
        }
        Ok(Self {
            slots: (0..slot_count).map(|_| ScanoutSlot::available()).collect(),
            rendering: None,
            prepared: None,
            in_flight: None,
            current: None,
            terminal_error: None,
        })
    }

    fn begin_render(&mut self) -> Result<usize, String> {
        self.ensure_healthy()?;
        if self.rendering.is_some() {
            return self.poison("a DRM Vulkan scanout slot is already rendering");
        }
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.state == LogicalState::Available && slot.refs.reusable())
            .ok_or_else(|| "no reusable DRM Vulkan scanout slot".to_string())?;
        self.slots[slot].state = LogicalState::Rendering;
        self.slots[slot].refs.gpu_submission = true;
        self.rendering = Some(slot);
        Ok(slot)
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &mut self,
        slot: usize,
        generation: u64,
        master_sync_file: OwnedFd,
        imported_video: ImportedVideo,
        capture: bool,
        timing: bool,
    ) -> Result<(), String> {
        self.ensure_healthy()?;
        if self.prepared.is_some() {
            return self.poison("a DRM Vulkan scanout slot is already prepared");
        }
        if self.rendering != Some(slot) {
            return self.poison("only the active rendering DRM Vulkan slot can become prepared");
        }
        let candidate = match self.slots.get_mut(slot) {
            Some(candidate)
                if candidate.state == LogicalState::Rendering && candidate.refs.gpu_submission =>
            {
                candidate
            }
            Some(_) => return self.poison("only a rendering DRM Vulkan slot can become prepared"),
            None => return self.poison(format!("DRM Vulkan scanout slot {slot} is out of bounds")),
        };
        candidate.state = LogicalState::Prepared;
        candidate.generation = Some(generation);
        candidate.refs.capture = capture;
        candidate.refs.timing = timing;
        candidate.imported_video_frames = imported_video.frame_count;
        candidate.imported_video_streams = imported_video.stream_identities;
        candidate.master_sync_file = Some(master_sync_file);
        self.rendering = None;
        self.prepared = Some(slot);
        Ok(())
    }

    fn duplicate_in_fence_for_atomic_attempt(&mut self) -> Result<OwnedFd, String> {
        self.ensure_healthy()?;
        if self.in_flight.is_some() {
            return Err("cannot attempt a DRM Vulkan atomic commit while one is in flight".into());
        }
        let slot = self
            .prepared
            .ok_or_else(|| "no prepared DRM Vulkan slot".to_string())?;
        match self.slots[slot]
            .master_sync_file
            .as_ref()
            .map(OwnedFd::try_clone)
        {
            Some(Ok(duplicate)) => Ok(duplicate),
            Some(Err(error)) => self.poison(format!(
                "failed to duplicate DRM Vulkan IN_FENCE_FD: {error}"
            )),
            None => self.poison("prepared DRM Vulkan slot lost its master sync file"),
        }
    }

    fn complete_atomic_attempt(&mut self, result: AtomicAttemptResult) -> Result<(), String> {
        self.ensure_healthy()?;
        if self.in_flight.is_some() {
            return self
                .poison("atomic commit completed while a DRM Vulkan commit was already in flight");
        }
        let Some(slot) = self.prepared else {
            return self.poison("atomic commit completed without a prepared DRM Vulkan slot");
        };
        match result {
            AtomicAttemptResult::Committed => {
                if self.slots[slot].master_sync_file.take().is_none() {
                    return self.poison("successful atomic commit lost its master sync file");
                }
                self.slots[slot].state = LogicalState::CommitInFlight;
                self.prepared = None;
                self.in_flight = Some(slot);
                Ok(())
            }
            AtomicAttemptResult::Busy => {
                if self.slots[slot].master_sync_file.is_none() {
                    return self.poison("EBUSY retry lost its master sync file");
                }
                Ok(())
            }
            AtomicAttemptResult::Terminal(kind) => self.poison(format!(
                "terminal DRM Vulkan atomic commit failure: {kind:?}"
            )),
        }
    }

    #[cfg(test)]
    fn page_flip(&mut self) -> Result<PresentedHooks, String> {
        self.page_flip_with_retired()
            .map(|(presented, _)| presented)
    }

    fn page_flip_with_retired(&mut self) -> Result<(PresentedHooks, Option<usize>), String> {
        self.ensure_healthy()?;
        let Some(next) = self.in_flight.take() else {
            return self.poison("page flip arrived without a DRM Vulkan commit in flight");
        };
        let Some(generation) = self.slots[next].generation else {
            return self.poison("presented DRM Vulkan slot has no generation");
        };
        let retired = self.current.replace(next);
        if let Some(previous) = retired {
            let previous = &mut self.slots[previous];
            previous.refs.kms_current = false;
            previous.state = LogicalState::Retiring;
            Self::make_available_if_retired(previous);
        }
        let slot = &mut self.slots[next];
        slot.state = LogicalState::Current;
        slot.refs.gpu_submission = false;
        slot.refs.kms_current = true;
        Ok((
            PresentedHooks {
                slot: next,
                generation,
                capture: slot.refs.capture,
                timing: slot.refs.timing,
                imported_video_frames: slot.imported_video_frames,
                imported_video_streams: slot.imported_video_streams.clone(),
            },
            retired,
        ))
    }

    fn retire_capture(&mut self, slot: usize) -> Result<(), String> {
        let slot = self.slot_mut(slot)?;
        slot.refs.capture = false;
        Self::make_available_if_retired(slot);
        Ok(())
    }

    fn retire_timing(&mut self, slot: usize) -> Result<(), String> {
        let slot = self.slot_mut(slot)?;
        slot.refs.timing = false;
        Self::make_available_if_retired(slot);
        Ok(())
    }

    fn page_flip_timeout(&mut self) -> Result<(), String> {
        self.poison("DRM Vulkan page-flip timeout")
    }

    /// Releases an unaccepted prepared identity after the caller has proved its GPU submission
    /// complete. KMS never owned this slot, so this is a clean repair rather than quarantine.
    fn discard_prepared_after_gpu_idle(&mut self) -> Result<usize, String> {
        self.ensure_healthy()?;
        if self.in_flight.is_some() || self.rendering.is_some() {
            return self.poison(
                "cannot repair a prepared DRM Vulkan slot with other unresolved ownership",
            );
        }
        let slot = self
            .prepared
            .take()
            .ok_or_else(|| "no prepared DRM Vulkan slot to repair".to_string())?;
        let candidate = &mut self.slots[slot];
        if candidate.state != LogicalState::Prepared || candidate.master_sync_file.is_none() {
            return self.poison("prepared DRM Vulkan repair identity is inconsistent");
        }
        candidate.master_sync_file.take();
        candidate.refs = SlotRefs::default();
        candidate.state = LogicalState::Available;
        candidate.generation = None;
        candidate.imported_video_frames = 0;
        candidate.imported_video_streams.clear();
        Ok(slot)
    }

    fn poison<T>(&mut self, message: impl Into<String>) -> Result<T, String> {
        let message = message.into();
        self.terminal_error.get_or_insert_with(|| message.clone());
        self.quarantine_uncertain_slots();
        Err(message)
    }

    fn quarantine_uncertain_slots(&mut self) {
        self.rendering = None;
        self.prepared = None;
        self.in_flight = None;
        self.current = None;
        self.slots.iter_mut().for_each(|slot| {
            if slot.state != LogicalState::Available {
                slot.state = LogicalState::Quarantined;
                slot.master_sync_file.take();
            }
        });
    }

    fn ensure_healthy(&self) -> Result<(), String> {
        self.terminal_error.clone().map_or(Ok(()), Err)
    }

    fn slot_mut(&mut self, slot: usize) -> Result<&mut ScanoutSlot, String> {
        self.slots
            .get_mut(slot)
            .ok_or_else(|| format!("DRM Vulkan scanout slot {slot} is out of bounds"))
    }

    fn make_available_if_retired(slot: &mut ScanoutSlot) {
        if slot.state == LogicalState::Retiring && slot.refs.reusable() {
            slot.state = LogicalState::Available;
            slot.generation = None;
            slot.imported_video_frames = 0;
            slot.imported_video_streams.clear();
        }
    }
}

struct ConcreteSlot {
    // Drop borrowed Skia surface before allocation; BO remains last.
    surface: Option<VulkanTargetSurface>,
    completion: ExportableSyncFdSemaphore,
    imported: ImportedDmaBufImage,
    framebuffer: framebuffer::Handle,
    bo: BufferObject<()>,
    capture_generation: Option<u64>,
    captured: Option<crate::backend::vulkan::CapturedRgba>,
    render_version: u64,
    render_timings: RenderTimings,
    pipeline_submitted_at: Option<Instant>,
    pipeline_swap_done_at: Option<Instant>,
    prepared_at: Instant,
    committed_at: Option<Instant>,
    newest_video_submitted_at: Option<Instant>,
    video_needs_cleanup: bool,
    animate: bool,
}

struct PresenterSession {
    // Declaration order is the unwind/fallback drop order. Borrowed Skia surfaces and BOs must
    // disappear before Ganesh, GBM, Vulkan, and the KMS card even when startup exits early.
    slots: Vec<ConcreteSlot>,
    engine: Option<VulkanEngine>,
    gbm: Option<GbmDevice<super::core::Card>>,
    device: Option<Arc<VulkanDevice>>,
    instance: Option<Arc<crate::backend::vulkan::VulkanInstance>>,
    kms_state: Option<PreparedKmsState>,
    kms: Option<KmsOutputProbe>,
    lifecycle: ScanoutLifecycle,
}

impl PresenterSession {
    fn kms(&self) -> &KmsOutputProbe {
        self.kms.as_ref().expect("presenter KMS owner")
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        config: &DrmRunConfig,
        native_log: &crate::native_log::NativeLogRelay,
    ) -> Result<(Self, VulkanRendererReport), String> {
        let vulkan_node_path = config.vulkan_drm_node.as_deref().ok_or_else(|| {
            "explicit DRM Vulkan requires vulkan_drm_node; it is selected independently from drm_card"
                .to_string()
        })?;
        let kms = probe_kms_output(config.card_path.as_deref(), config.requested_size)?;
        if !kms.primary_supports_xrgb8888 {
            return Err("selected KMS primary plane does not advertise DRM XRGB8888".into());
        }
        if !kms.primary_has_in_fence_fd {
            return Err(
                "selected KMS primary plane has no IN_FENCE_FD; explicit DRM Vulkan refuses implicit synchronization"
                    .into(),
            );
        }
        kms.card
            .acquire_master_lock()
            .map_err(|error| format!("could not acquire DRM master: {error}"))?;
        kms.card
            .set_client_capability(ClientCapability::UniversalPlanes, true)
            .map_err(|error| format!("could not enable universal planes: {error}"))?;
        kms.card
            .set_client_capability(ClientCapability::Atomic, true)
            .map_err(|error| format!("could not enable atomic modesetting: {error}"))?;

        let selection = open_vulkan_selection_node(vulkan_node_path)?;
        let instance = crate::backend::vulkan::VulkanInstance::new(&[])?;
        let extensions = required_device_extensions();
        let device = VulkanDevice::new_for_drm_node(
            Arc::clone(&instance),
            ExactDeviceRequirements {
                required_extensions: &extensions,
                require_timestamps: true,
                selection_node: selection.selection,
            },
        )?;
        let report = VulkanRendererReport::for_selected_node(
            &device,
            selection.path.clone(),
            selection.selection,
        );

        let plane_properties = object_property_set(&kms.card, kms.primary)?;
        let modifiers = kms_xrgb8888_modifiers(&kms.card, &plane_properties)?;
        let mut rejected = Vec::new();
        let common_modifiers = modifiers
            .into_iter()
            .filter(
                |modifier| match validate_bgra_scanout_import_support(&device, *modifier) {
                    Ok(()) => true,
                    Err(error) => {
                        rejected.push(format!("{modifier:#018x}: {error}"));
                        false
                    }
                },
            )
            .collect::<Vec<_>>();
        if common_modifiers.is_empty() {
            return Err(format!(
                "no KMS XRGB8888 modifier has Vulkan BGRA color-attachment import support: {}",
                rejected.join("; ")
            ));
        }

        let gbm = GbmDevice::new(duplicate_card(&kms.card)?)
            .map_err(|error| format!("failed to create DRM Vulkan GBM device: {error}"))?;
        let usage = BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING;
        if !gbm.is_format_supported(GbmFormat::Xrgb8888, usage) {
            return Err("GBM does not support XRGB8888 SCANOUT|RENDERING".into());
        }

        let mut engine = VulkanEngine::new(Arc::clone(&device), config.renderer_cache_config)?;
        let initial_state = TargetImageState {
            layout: vk::ImageLayout::UNDEFINED,
            queue_family_index: DMA_BUF_EXTERNAL_QUEUE_FAMILY,
        };
        let mut slots = Vec::with_capacity(MIN_SCANOUT_SLOTS);
        for index in 0..MIN_SCANOUT_SLOTS {
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
                        "GBM could not allocate DRM Vulkan slot {index} from modifier intersection {:?}: {error}",
                        common_modifiers
                    )
                })?;
            if bo.format() as u32 != DRM_FORMAT_XRGB8888 || bo.plane_count() != 1 {
                return Err(format!(
                    "GBM returned unsupported slot {index} topology format={:?} planes={}",
                    bo.format(),
                    bo.plane_count()
                ));
            }
            let modifier = u64::from(bo.modifier());
            if bo.modifier() == Modifier::Invalid || !common_modifiers.contains(&modifier) {
                return Err(format!(
                    "GBM returned slot {index} modifier {modifier:#018x} outside the explicit intersection"
                ));
            }
            let dma_buf = bo
                .fd()
                .map_err(|error| format!("failed to export DRM Vulkan slot {index}: {error}"))?;
            let imported = ImportedDmaBufImage::new_bgra_scanout(
                Arc::clone(&device),
                kms.dimensions,
                dma_buf.as_raw_fd(),
                dma_buf_size(&dma_buf)?,
                modifier,
                ImportedPlane {
                    offset: u64::from(bo.offset(0)),
                    pitch: bo.stride_for_plane(0),
                },
            )?;
            let framebuffer = kms
                .card
                .add_planar_framebuffer(&bo, FbCmd2Flags::MODIFIERS)
                .map_err(|error| {
                    format!(
                        "modifier-aware AddFB2 rejected DRM Vulkan slot {index} modifier {modifier:#018x}: {error}"
                    )
                })?;
            let surface = engine.create_target_surface_with_format_usage_and_tiling(
                imported.image(),
                kms.dimensions,
                initial_state,
                VulkanTargetFormat::Bgra8888,
                crate::backend::vulkan::GANESH_TARGET_IMAGE_USAGE,
                skia_safe::gpu::vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
            )?;
            slots.push(ConcreteSlot {
                surface: Some(surface),
                completion: ExportableSyncFdSemaphore::new(Arc::clone(&device))?,
                imported,
                framebuffer,
                bo,
                capture_generation: None,
                captured: None,
                render_version: 0,
                render_timings: RenderTimings::default(),
                pipeline_submitted_at: None,
                pipeline_swap_done_at: None,
                prepared_at: Instant::now(),
                committed_at: None,
                newest_video_submitted_at: None,
                video_needs_cleanup: false,
                animate: false,
            });
        }
        let kms_state = PreparedKmsState::new(&kms, plane_properties)?;
        native_log.info(
            "drm_vulkan",
            format!(
                "trial presenter admitted: kms_card={} mode={}x{} vulkan_node={} ({} {}:{}) device={} driver={:?} allocation=gbm-import slots={} wsi=false explicit_sync=true modifiers={:?}",
                config.card_path.as_deref().unwrap_or("/dev/dri/card0"),
                kms.dimensions.0,
                kms.dimensions.1,
                selection.path,
                selection.selection.field.as_str(),
                selection.selection.node.major,
                selection.selection.node.minor,
                device.physical_device_name(),
                device.report().driver_name,
                MIN_SCANOUT_SLOTS,
                common_modifiers,
            ),
        );
        Ok((
            Self {
                kms: Some(kms),
                kms_state: Some(kms_state),
                gbm: Some(gbm),
                slots,
                engine: Some(engine),
                device: Some(device),
                instance: Some(instance),
                lifecycle: ScanoutLifecycle::new(MIN_SCANOUT_SLOTS)?,
            },
            report,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        generation: u64,
        render_state: &RenderState,
        video_registry: &Arc<crate::video::VideoRegistry>,
        video_context: Option<&VulkanVideoImportContext>,
        cursor: Option<(&CursorVisual, (f32, f32))>,
        capture_generation: Option<u64>,
        profile: bool,
    ) -> Result<Option<String>, String> {
        let slot_index = self.lifecycle.begin_render()?;
        let initial_state = self.slots[slot_index]
            .surface
            .as_ref()
            .ok_or_else(|| "DRM Vulkan slot surface is unavailable".to_string())?
            .state();
        if initial_state.queue_family_index != DMA_BUF_EXTERNAL_QUEUE_FAMILY {
            return self
                .lifecycle
                .poison("reusable DRM Vulkan slot is not externally owned before render");
        }
        let completion_semaphore = self.slots[slot_index].completion.begin_signal()?;
        let acquired = AcquiredTarget {
            token: slot_index,
            image: self.slots[slot_index].imported.image(),
            dimensions: self.kms().dimensions,
            current_state: initial_state,
            acquire_semaphore: None,
            completion_semaphore,
            final_state: TargetImageState {
                layout: vk::ImageLayout::GENERAL,
                queue_family_index: DMA_BUF_EXTERNAL_QUEUE_FAMILY,
            },
        };
        let engine = self
            .engine
            .as_mut()
            .ok_or_else(|| "DRM Vulkan engine is shut down".to_string())?;
        let surface = self.slots[slot_index]
            .surface
            .as_mut()
            .ok_or_else(|| "DRM Vulkan slot surface is unavailable".to_string())?;
        let mut video_sync_error = None;
        let rendered = engine.render(
            surface,
            acquired,
            capture_generation.is_some(),
            |renderer, frame| {
                let video = match video_context {
                    Some(video_context) => match renderer.sync_vulkan_video_frames(
                        frame,
                        video_registry,
                        video_context,
                    ) {
                        Ok(video) => video,
                        Err(error) => {
                            // Preserve the last valid displayed import. Failed candidates and partial
                            // retirement stay renderer-owned and receive a follow-up cleanup render.
                            video_sync_error = Some(error);
                            crate::video::VideoSyncResult {
                                needs_cleanup: true,
                                ..crate::video::VideoSyncResult::default()
                            }
                        }
                    },
                    None => crate::video::VideoSyncResult::default(),
                };
                let mut draw_cursor = |surface: &mut skia_safe::Surface| {
                    if let Some((visual, pos)) = cursor {
                        draw_software_cursor_before_flush(surface, visual, pos);
                    }
                };
                let timings = if profile {
                    renderer.render_profiled_with_before_flush(
                        frame,
                        render_state,
                        &mut draw_cursor,
                    )
                } else {
                    renderer.render_with_before_flush(frame, render_state, &mut draw_cursor)
                };
                (timings, video)
            },
        );
        let ((timings, video), completed) = match rendered {
            Ok(result) => result,
            Err(error) => {
                return self.lifecycle.poison(format!(
                    "DRM Vulkan render submission failed; ownership is uncertain: {error}"
                ));
            }
        };
        if completed.token != slot_index
            || completed.completion_semaphore != completion_semaphore
            || completed.final_state.queue_family_index != DMA_BUF_EXTERNAL_QUEUE_FAMILY
        {
            return self
                .lifecycle
                .poison("DRM Vulkan completed target identity/state changed unexpectedly");
        }
        let master_sync_file = self.slots[slot_index]
            .completion
            .export_submitted_sync_fd()
            .map_err(|error| {
                let _ = self.lifecycle.poison::<()>(format!(
                    "DRM Vulkan SYNC_FD export failed after submission: {error}"
                ));
                error
            })?;
        let slot = &mut self.slots[slot_index];
        slot.capture_generation = capture_generation;
        slot.captured = completed.capture;
        slot.render_version = render_state.render_version;
        slot.render_timings = timings;
        slot.pipeline_submitted_at = render_state.pipeline_submitted_at;
        slot.pipeline_swap_done_at = None;
        slot.prepared_at = Instant::now();
        slot.committed_at = None;
        slot.newest_video_submitted_at = video.newest_import_submitted_at;
        slot.video_needs_cleanup = video.needs_cleanup;
        slot.animate = render_state.animate;
        self.lifecycle.prepare(
            slot_index,
            generation,
            master_sync_file,
            ImportedVideo {
                frame_count: video.imported_frames,
                stream_identities: video.imported_streams,
            },
            capture_generation.is_some(),
            true,
        )?;
        Ok(video_sync_error)
    }

    fn commit_prepared(&mut self, initial: bool) -> Result<AtomicAttemptResult, String> {
        let slot_index = self
            .lifecycle
            .prepared
            .ok_or_else(|| "no prepared DRM Vulkan slot".to_string())?;
        let attempt_fence = self.lifecycle.duplicate_in_fence_for_atomic_attempt()?;
        let props = self
            .kms_state
            .as_ref()
            .ok_or_else(|| "DRM Vulkan KMS property state is unavailable".to_string())?;
        let mut request = atomic::AtomicModeReq::new();
        if initial {
            request.add_property(
                self.kms().connector,
                prop_handle(props.connector_infos(), "CRTC_ID")?,
                property::Value::CRTC(Some(self.kms().crtc)),
            );
            request.add_property(
                self.kms().crtc,
                prop_handle(props.crtc_infos(), "MODE_ID")?,
                props.probe_mode_blob,
            );
            request.add_property(
                self.kms().crtc,
                prop_handle(props.crtc_infos(), "ACTIVE")?,
                property::Value::Boolean(true),
            );
        }
        add_primary_properties(
            self.kms(),
            &mut request,
            self.slots[slot_index].framebuffer,
            props.plane_infos(),
        )?;
        request.add_property(
            self.kms().primary,
            prop_handle(props.plane_infos(), "IN_FENCE_FD")?,
            property::Value::SignedRange(i64::from(attempt_fence.as_raw_fd())),
        );
        let flags = AtomicCommitFlags::NONBLOCK
            | AtomicCommitFlags::PAGE_FLIP_EVENT
            | if initial {
                AtomicCommitFlags::ALLOW_MODESET
            } else {
                AtomicCommitFlags::empty()
            };
        let result = self.kms().card.atomic_commit(flags, request);
        drop(attempt_fence);
        let classified = AtomicAttemptResult::from_io_result(&result);
        self.lifecycle.complete_atomic_attempt(classified)?;
        if classified == AtomicAttemptResult::Committed {
            self.slots[slot_index].committed_at = Some(Instant::now());
        }
        Ok(classified)
    }

    fn repair_unaccepted_prepared_frame(&mut self, context: &str) -> Result<(), String> {
        self.device
            .as_ref()
            .ok_or_else(|| "DRM Vulkan device already dropped".to_string())?
            .wait_idle(context)?;
        let slot = self.lifecycle.discard_prepared_after_gpu_idle()?;
        let concrete = &mut self.slots[slot];
        concrete.capture_generation = None;
        concrete.captured.take();
        concrete.committed_at = None;
        concrete.video_needs_cleanup = false;
        Ok(())
    }

    fn receive_page_flips(&mut self) -> Result<Vec<(u32, PresentedHooks, Option<usize>)>, String> {
        let mut presented = Vec::new();
        for event in self
            .kms()
            .card
            .receive_events()
            .map_err(|error| format!("failed to receive DRM Vulkan events: {error}"))?
        {
            if let drm::control::Event::PageFlip(flip) = event
                && flip.crtc == self.kms().crtc
            {
                let (hooks, retired) = self.lifecycle.page_flip_with_retired()?;
                presented.push((flip.frame, hooks, retired));
            }
        }
        Ok(presented)
    }

    fn normal_shutdown(&mut self) -> Result<(), String> {
        if self.lifecycle.in_flight.is_some()
            || self.lifecycle.prepared.is_some()
            || self.lifecycle.rendering.is_some()
        {
            return Err("cannot normally destroy DRM Vulkan with unresolved ownership".into());
        }
        let engine = self
            .engine
            .as_mut()
            .ok_or_else(|| "DRM Vulkan engine already shut down".to_string())?;
        engine.device().wait_idle("DRM Vulkan pre-video shutdown")?;
        engine.renderer_mut()?.prepare_vulkan_video_shutdown()?;
        engine
            .device()
            .wait_idle("DRM Vulkan imported-image release shutdown")?;
        engine.drop_scene_renderer();
        self.slots.iter_mut().for_each(|slot| {
            slot.captured.take();
            slot.surface.take();
        });
        engine.shutdown_ganesh();
        self.engine.take();

        if let Some(state) = self.kms_state.as_ref() {
            state.restore(self.kms(), PAGE_FLIP_TIMEOUT)?;
        }
        let state = self
            .kms_state
            .take()
            .ok_or_else(|| "DRM Vulkan KMS state already destroyed".to_string())?;
        let slots = mem::take(&mut self.slots);
        for slot in slots {
            drop(slot.completion);
            drop(slot.imported);
            self.kms()
                .card
                .destroy_framebuffer(slot.framebuffer)
                .map_err(|error| format!("failed to remove DRM Vulkan framebuffer: {error}"))?;
            drop(slot.bo);
        }
        self.gbm.take();
        state.destroy_blobs(&self.kms().card)?;
        let device = self
            .device
            .take()
            .ok_or_else(|| "DRM Vulkan device already dropped".to_string())?;
        device.wait_idle("DRM Vulkan final device shutdown")?;
        drop(device);
        self.instance.take();
        let kms = self
            .kms
            .take()
            .ok_or_else(|| "DRM Vulkan KMS owner already dropped".to_string())?;
        kms.card
            .release_master_lock()
            .map_err(|error| format!("failed to release DRM master: {error}"))?;
        drop(kms);
        Ok(())
    }
}

fn draw_software_cursor_before_flush(
    surface: &mut skia_safe::Surface,
    visual: &CursorVisual,
    cursor_pos: (f32, f32),
) {
    let (width, height) = visual.size();
    let hotspot = visual.hotspot();
    let sampling =
        skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
    surface.canvas().draw_image_rect_with_sampling_options(
        visual.image(),
        None,
        Rect::from_xywh(
            cursor_pos.0 - hotspot.0,
            cursor_pos.1 - hotspot.1,
            width as f32,
            height as f32,
        ),
        sampling,
        &Paint::default(),
    );
}

#[allow(dead_code)]
struct QuarantinedPresenterSession(PresenterSession);

#[derive(Default)]
struct ScanoutQuarantinePolicy {
    terminal: bool,
    retained_sessions: usize,
}

impl ScanoutQuarantinePolicy {
    fn admit(&self) -> Result<(), String> {
        if self.terminal {
            Err(
                "DRM Vulkan presenter is process-terminal after uncertain scanout ownership; restart the VM/process"
                    .into(),
            )
        } else {
            Ok(())
        }
    }

    fn retain_one(&mut self) -> bool {
        self.terminal = true;
        if self.retained_sessions == 0 {
            self.retained_sessions = 1;
            true
        } else {
            false
        }
    }
}

// SAFETY: quarantine is write-once process-lifetime storage. The retained session is never read,
// mutated, or dropped after insertion. All Vulkan objects were created and last touched by the
// presenter thread; Send is used solely to park the allocation behind a mutex until process exit.
unsafe impl Send for QuarantinedPresenterSession {}

#[derive(Default)]
struct ScanoutQuarantineOwner {
    policy: ScanoutQuarantinePolicy,
    session: Option<QuarantinedPresenterSession>,
}

fn scanout_quarantine() -> &'static Mutex<ScanoutQuarantineOwner> {
    static OWNER: OnceLock<Mutex<ScanoutQuarantineOwner>> = OnceLock::new();
    OWNER.get_or_init(|| Mutex::new(ScanoutQuarantineOwner::default()))
}

fn ensure_scanout_admitted() -> Result<(), String> {
    let owner = scanout_quarantine()
        .lock()
        .map_err(|_| "DRM Vulkan scanout quarantine lock is poisoned".to_string())?;
    owner.policy.admit()
}

fn quarantine_session(session: PresenterSession) {
    // A retained presenter may contain uncertain scanout and imported-video resources. Make the
    // terminal state Vulkan-global before parking it so no later DRM, Wayland, or headless Vulkan
    // runtime can coexist with ownership that is only safe because it remains alive.
    mark_vulkan_process_quarantine_terminal();
    let mut owner = scanout_quarantine()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if owner.policy.retain_one() {
        debug_assert!(owner.session.is_none());
        owner.session = Some(QuarantinedPresenterSession(session));
    } else {
        // The terminal flag prevents a second runtime. This branch protects the one-session hard
        // cap even if called twice during unwinding; leak rather than run uncertain destructors.
        mem::forget(session);
    }
}

pub(super) fn run(context: DrmRunContext, config: DrmRunConfig) {
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

    if config.force_gpu_finish {
        let _ = startup_tx.send(Err(
            "DRM Vulkan backend unavailable: drm_force_gpu_finish is OpenGL-only".into(),
        ));
        running_flag.store(false, Ordering::Release);
        return;
    }
    if let Err(error) = ensure_scanout_admitted() {
        let _ = startup_tx.send(Err(format!("DRM Vulkan backend unavailable: {error}")));
        running_flag.store(false, Ordering::Release);
        return;
    }
    let cursor_theme = match DrmCursorTheme::load(&config.asset_config, &config.cursor_overrides) {
        Ok(theme) => theme,
        Err(error) => {
            let _ = startup_tx.send(Err(format!(
                "DRM Vulkan backend unavailable: cursor setup failed: {error}"
            )));
            running_flag.store(false, Ordering::Release);
            return;
        }
    };
    native_log.info(
        "drm_vulkan",
        "trial presenter uses software cursor composited into the explicitly fenced primary image",
    );

    let retry_interval = Duration::from_millis(u64::from(config.retry_interval_ms));
    let mut retries_remaining = config.startup_retries;
    let (mut session, report) = loop {
        if stop.load(Ordering::Acquire) {
            let _ = startup_tx.send(Err("DRM Vulkan startup aborted".into()));
            running_flag.store(false, Ordering::Release);
            return;
        }

        match PresenterSession::new(&config, &native_log) {
            Ok(session) => break session,
            Err(error) if retries_remaining > 0 => {
                eprintln!(
                    "DRM Vulkan backend unavailable: {error} (retrying, {retries_remaining} attempts left)"
                );
                retries_remaining -= 1;
                sleep_with_stop(&stop, retry_interval);
            }
            Err(error) => {
                let _ = startup_tx.send(Err(format!(
                    "DRM Vulkan backend unavailable after {} attempts: {error}",
                    u64::from(config.startup_retries) + 1
                )));
                running_flag.store(false, Ordering::Release);
                return;
            }
        }
    };
    let (video_context, video_import_error) = optional_video_import(VulkanVideoImportContext::new(
        Arc::clone(session.device.as_ref().expect("presenter Vulkan device")),
    ));
    if let Some(error) = video_import_error {
        native_log.warning(
            "video",
            format!(
                "PRIME video unavailable for this Vulkan presenter; UI scanout remains enabled: {error}"
            ),
        );
    }
    if let Some(video_context) = video_context.as_ref() {
        if !video_context.rgba_linear_supported() {
            native_log.warning(
                "video",
                "Vulkan ABGR8888 linear DMA-BUF import is unavailable; NV12 remains independently capability-driven",
            );
        }
        if let Err(error) = video_registry.set_vulkan_import_capabilities(
            video_context.rgba_linear_supported(),
            video_context.bgra_import_supported(),
            video_context.nv12_capabilities().to_vec(),
        ) {
            let _ = startup_tx.send(Err(format!(
                "DRM Vulkan backend unavailable: failed to publish active-device video import capabilities: {error}"
            )));
            running_flag.store(false, Ordering::Release);
            return;
        }
    }
    let prime_video_supported = video_context
        .as_ref()
        .is_some_and(VulkanVideoImportContext::supports_any_format);
    let prime_video_formats = video_context
        .as_ref()
        .map(VulkanVideoImportContext::supported_format_names)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Err(error) = video_registry.set_prime_video_available(prime_video_supported) {
        let _ = startup_tx.send(Err(format!(
            "DRM Vulkan backend unavailable: failed to publish PRIME video availability: {error}"
        )));
        running_flag.store(false, Ordering::Release);
        return;
    }
    let _ = screen_tx.send(session.kms().dimensions);
    let _ = input_wake.signal();
    let _ = event_tx.send(EventMsg::InputEvent(crate::input::InputEvent::Resized {
        width: session.kms().dimensions.0,
        height: session.kms().dimensions.1,
        scale_factor: 1.0,
    }));

    let mut render_state = RenderState::default();
    let mut desired_generation = 1_u64;
    let mut committed_generation = 0_u64;
    let mut observed_capture_generation = 0_u64;
    let mut last_video_generation = video_registry.generation();
    let mut current_cursor_icon = CursorIcon::Default;
    let mut last_cursor_snapshot = cursor_state.snapshot();
    let mut retry_at = None;
    let mut commit_retry_policy =
        CommitRetryPolicy::new(config.startup_retries, config.retry_interval_ms);
    let mut page_flip_deadline = None;
    let mut initial_commit = true;
    let mut startup_tx = Some(startup_tx);
    let mut next_hotplug = Instant::now() + HOTPLUG_INTERVAL;
    let mut next_profile = Instant::now();
    let mut last_presented_scene = None;
    let mut terminal = None;
    let mut recovered_error = None;
    let mut stop_requested = false;

    loop {
        let _ = presenter_wake.drain();
        while let Ok(message) = render_rx.try_recv() {
            match message {
                RenderMsg::Scene {
                    scene,
                    version,
                    pipeline_submitted_at,
                    pipeline_render_queued_at,
                    animate,
                    ..
                } => {
                    let received_at = Instant::now();
                    render_state.set_scene(*scene);
                    if let Err(error) =
                        video_registry.set_active_targets(&render_state.video_target_ids)
                    {
                        terminal = Some(format!("video target visibility update failed: {error}"));
                        break;
                    }
                    render_state.render_version = version;
                    render_state.pipeline_submitted_at = pipeline_submitted_at;
                    render_state.pipeline_render_queued_at = pipeline_render_queued_at;
                    render_state.animate = animate;
                    if let Some(stats) = stats.as_deref() {
                        stats.record_pipeline_draw_started(pipeline_render_queued_at, received_at);
                        stats.record_drm_scene_selected_for_draw();
                    }
                    desired_generation = desired_generation.wrapping_add(1);
                    if config.render_log {
                        let latest = render_counter.load(Ordering::Relaxed);
                        native_log.info(
                            "drm_vulkan",
                            format!(
                                "render version={version} latest={latest} delta={}",
                                latest.saturating_sub(version)
                            ),
                        );
                    }
                }
                RenderMsg::Stop => stop_requested = true,
            }
        }
        if terminal.is_some() || stop_requested || stop.load(Ordering::Acquire) {
            break;
        }
        while let Ok(icon) = cursor_icon_rx.try_recv() {
            current_cursor_icon = icon;
            desired_generation = desired_generation.wrapping_add(1);
        }
        let cursor_snapshot = cursor_state.snapshot();
        if cursor_snapshot.version != last_cursor_snapshot.version {
            desired_generation = desired_generation.wrapping_add(1);
            last_cursor_snapshot = cursor_snapshot;
        }
        let video_generation = video_registry.generation();
        if video_generation != last_video_generation {
            last_video_generation = video_generation;
            desired_generation = desired_generation.wrapping_add(1);
        }
        if let Some(capture) = latest_frame.pending_capture_generation()
            && capture != observed_capture_generation
        {
            observed_capture_generation = capture;
            desired_generation = desired_generation.wrapping_add(1);
        }

        let now = Instant::now();
        if session.lifecycle.in_flight.is_some()
            && page_flip_deadline.is_some_and(|deadline| now >= deadline)
        {
            let _ = session.lifecycle.page_flip_timeout();
            terminal = Some("DRM Vulkan page-flip timeout".into());
            break;
        }
        if now >= next_hotplug {
            match probe_hotplug_unchanged(&session, &config) {
                Ok(true) => next_hotplug = now + HOTPLUG_INTERVAL,
                Ok(false) => {
                    terminal = Some("DRM Vulkan output topology changed during trial".into());
                    break;
                }
                Err(error) => {
                    terminal = Some(error);
                    break;
                }
            }
        }
        if retry_at.is_some_and(|deadline| now >= deadline) {
            retry_at = None;
        }

        if session.lifecycle.prepared.is_none()
            && desired_generation != committed_generation
            && session.lifecycle.rendering.is_none()
        {
            let profile = config.renderer_stats_log && now >= next_profile;
            if profile {
                next_profile = now + RENDER_PROFILE_INTERVAL;
            }
            let capture_generation = latest_frame.pending_capture_generation();
            let cursor = last_cursor_snapshot.state.visible.then(|| {
                (
                    cursor_theme.cursor(current_cursor_icon),
                    last_cursor_snapshot.state.pos,
                )
            });
            let render_started = Instant::now();
            let video_sync_error = match session.render(
                desired_generation,
                &render_state,
                &video_registry,
                video_context.as_ref(),
                cursor,
                capture_generation,
                profile,
            ) {
                Ok(error) => error,
                Err(error) => {
                    terminal = Some(error);
                    break;
                }
            };
            if let Some(error) = video_sync_error {
                native_log.error("video", format!("Vulkan video sync failed: {error}"));
            }
            let slot = session.lifecycle.prepared.expect("render prepared slot");
            commit_retry_policy.begin_prepared_frame();
            if let Some(stats) = stats.as_deref() {
                stats.record_render_timings(
                    render_started.elapsed(),
                    &session.slots[slot].render_timings,
                );
                stats.record_drm_primary_prepared(
                    session.lifecycle.slots[slot].imported_video_frames > 0,
                );
            }
            if profile && render_frame_has_slow_stage(&session.slots[slot].render_timings) {
                native_log.info(
                    "renderer_slow_frame",
                    format_slow_render_frame_log(
                        "drm_vulkan",
                        &session.slots[slot].render_timings,
                        render_state.scene.summary(),
                    ),
                );
            }
        }

        if session.lifecycle.prepared.is_some()
            && session.lifecycle.in_flight.is_none()
            && retry_at.is_none()
        {
            if let Some(stats) = stats.as_deref() {
                stats.record_drm_primary_commit_attempt();
            }
            let started = Instant::now();
            commit_retry_policy.record_attempt();
            match session.commit_prepared(initial_commit) {
                Ok(AtomicAttemptResult::Committed) => {
                    if let Some(stats) = stats.as_deref() {
                        stats.record_drm_atomic_commit_ioctl(started.elapsed());
                        stats.record_drm_primary_committed();
                    }
                    page_flip_deadline = checked_deadline(PAGE_FLIP_TIMEOUT).ok();
                    initial_commit = false;
                }
                Ok(AtomicAttemptResult::Busy) => {
                    if let Some(stats) = stats.as_deref() {
                        stats.record_drm_atomic_commit_ioctl(started.elapsed());
                        stats.record_drm_primary_commit_ebusy();
                    }
                    match commit_retry_policy.on_busy() {
                        CommitBusyAction::RetryAfter(delay) => {
                            retry_at = Some(Instant::now() + delay);
                        }
                        CommitBusyAction::Exhausted { attempts } => {
                            let reason = format!(
                                "DRM Vulkan atomic commit remained EBUSY after {attempts} attempts"
                            );
                            match session.repair_unaccepted_prepared_frame(
                                "DRM Vulkan EBUSY exhaustion repair",
                            ) {
                                Ok(()) => {
                                    recovered_error = Some(reason);
                                }
                                Err(error) => {
                                    terminal = Some(format!(
                                        "{reason}; prepared-frame repair failed: {error}"
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }
                Ok(AtomicAttemptResult::Terminal(kind)) => {
                    terminal = Some(format!("terminal DRM Vulkan atomic commit: {kind:?}"));
                    break;
                }
                Err(error) => {
                    terminal = Some(error);
                    break;
                }
            }
        }

        let next_deadline = [Some(next_hotplug), retry_at, page_flip_deadline]
            .into_iter()
            .flatten()
            .min();
        let timeout = if session.lifecycle.in_flight.is_none()
            && session.lifecycle.prepared.is_none()
            && desired_generation != committed_generation
        {
            Duration::ZERO
        } else {
            next_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(PAGE_FLIP_TIMEOUT)
        };
        let mut pollfds = [
            libc::pollfd {
                fd: session.kms().card.as_raw_fd(),
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
            Ok(()) => {}
            Err(error) => {
                terminal = Some(format!("DRM Vulkan poll failed: {error}"));
                break;
            }
        }
        if pollfds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            terminal = Some("DRM Vulkan fd reported a terminal poll error".into());
            break;
        }
        if pollfds[0].revents & (libc::POLLIN | libc::POLLPRI) != 0 {
            match session.receive_page_flips() {
                Ok(flips) => {
                    for (sequence, hooks, retired_slot) in flips {
                        page_flip_deadline = None;
                        if let Some(retired_slot) = retired_slot {
                            session.slots[retired_slot].captured.take();
                        }
                        committed_generation = hooks.generation;
                        let presented_at = Instant::now();
                        let predicted = presented_at + mode_frame_interval(&session.kms().mode);
                        let slot = &mut session.slots[hooks.slot];
                        if let Some(stats) = stats.as_deref() {
                            stats.record_frame_present();
                            stats.record_drm_page_flip_sequence(None);
                            stats.record_drm_primary_presented(
                                hooks.imported_video_frames > 0,
                                &hooks.imported_video_streams,
                                slot.committed_at
                                    .map(|at| presented_at.saturating_duration_since(at))
                                    .unwrap_or_default(),
                                slot.newest_video_submitted_at
                                    .map(|at| presented_at.saturating_duration_since(at)),
                            );
                            stats.record_present_submit(
                                slot.committed_at
                                    .map(|at| at.saturating_duration_since(slot.prepared_at))
                                    .unwrap_or_default(),
                            );
                            if slot.render_version > 0
                                && last_presented_scene != Some(slot.render_version)
                            {
                                stats.record_drm_scene_presented();
                                last_presented_scene = Some(slot.render_version);
                            }
                            if let Some(swap_done_at) = slot.pipeline_swap_done_at {
                                stats.record_pipeline_presented(
                                    slot.pipeline_submitted_at,
                                    swap_done_at,
                                    presented_at,
                                );
                            }
                        }
                        if let (Some(generation), Some(capture)) =
                            (slot.capture_generation.take(), slot.captured.as_ref())
                        {
                            latest_frame.publish_requested_capture(
                                generation,
                                capture.width,
                                capture.height,
                                1.0,
                                capture.pixels.clone(),
                            );
                        }
                        // Keep current capture bytes attached to the current slot. The replacing
                        // page flip retires that slot, at which point capture ownership can drop.
                        session.lifecycle.retire_capture(hooks.slot).ok();
                        session.lifecycle.retire_timing(hooks.slot).ok();
                        let _ = event_tx.try_send(EventMsg::PresentTiming {
                            presented_at,
                            predicted_next_present_at: predicted,
                        });
                        if slot.video_needs_cleanup {
                            desired_generation = desired_generation.wrapping_add(1);
                        }
                        if slot.animate {
                            let message = TreeMsg::AnimationPulse {
                                presented_at,
                                predicted_next_present_at: predicted,
                                trace: None,
                            };
                            match tree_tx.try_send(message) {
                                Ok(()) => {}
                                Err(TrySendError::Full(message)) => {
                                    if tree_tx.send(message).is_err() {
                                        stop_requested = true;
                                    }
                                }
                                Err(TrySendError::Disconnected(_)) => stop_requested = true,
                            }
                        }
                        if startup_tx.is_some() {
                            if let Some(tx) = startup_tx.take() {
                                let _ = tx.send(Ok(DrmBackendStartupInfo {
                                    prime_video_supported,
                                    prime_video_formats: prime_video_formats.clone(),
                                    vulkan_device: Some(report.clone()),
                                }));
                            }
                            native_log.info(
                                "drm_vulkan",
                                format!(
                                    "initial explicitly fenced page flip accepted: sequence={sequence}"
                                ),
                            );
                        }
                    }
                }
                Err(error) => {
                    terminal = Some(error);
                    break;
                }
            }
        }
        if pollfds[1].revents & libc::POLLIN != 0 {
            let _ = presenter_wake.drain();
        }
    }

    if let Err(error) = video_registry.set_prime_video_available(false) {
        native_log.warning("video", format!("failed to disable PRIME video: {error}"));
    }
    if terminal.is_none()
        && session.lifecycle.in_flight.is_some()
        && let Err(error) = resolve_shutdown_page_flip(&mut session, PAGE_FLIP_TIMEOUT)
    {
        terminal = Some(error);
    }
    // A frame prepared but never submitted has queue work and an exported fence whose ownership
    // was never accepted by KMS. An idle repair proves completion before discarding that prepared
    // identity during an otherwise clean shutdown.
    if terminal.is_none()
        && session.lifecycle.prepared.is_some()
        && let Err(error) = session
            .repair_unaccepted_prepared_frame("DRM Vulkan uncommitted prepared-frame shutdown")
    {
        terminal = Some(error);
    }
    if let Some(reason) = recovered_error.as_ref() {
        native_log.error(
            "drm_vulkan",
            format!("presenter stopped after ownership-safe recovery: {reason}"),
        );
        if let Some(tx) = startup_tx.take() {
            let _ = tx.send(Err(format!("DRM Vulkan backend unavailable: {reason}")));
        }
    }
    let uncertain = terminal.is_some()
        || session.lifecycle.in_flight.is_some()
        || session.lifecycle.prepared.is_some()
        || session.lifecycle.rendering.is_some()
        || session
            .device
            .as_ref()
            .is_some_and(|device| device.is_device_lost());
    if uncertain {
        let reason = terminal.unwrap_or_else(|| "unresolved DRM Vulkan ownership".into());
        native_log.error(
            "drm_vulkan",
            format!("terminal presenter fault; quarantining session until VM restart: {reason}"),
        );
        if let Some(tx) = startup_tx.take() {
            let _ = tx.send(Err(format!("DRM Vulkan backend unavailable: {reason}")));
        }
        quarantine_session(session);
    } else {
        // A clean stop only reaches here with no prepared/in-flight ownership. The explicit KMS
        // restore is the final authority before Vulkan/GBM owners are destroyed.
        if let Err(error) = session.normal_shutdown() {
            native_log.error(
                "drm_vulkan",
                format!("clean shutdown became uncertain; quarantining: {error}"),
            );
            quarantine_session(session);
        }
    }
    running_flag.store(false, Ordering::Release);
}

fn resolve_shutdown_page_flip(
    session: &mut PresenterSession,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = checked_deadline(timeout)?;
    while session.lifecycle.in_flight.is_some() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("DRM Vulkan page-flip timeout during shutdown".to_string());
        }
        let mut pollfd = [libc::pollfd {
            fd: session.kms().card.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        poll_fds(&mut pollfd, remaining)?;
        if pollfd[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err("DRM Vulkan fd failed while resolving shutdown page flip".to_string());
        }
        if pollfd[0].revents & (libc::POLLIN | libc::POLLPRI) != 0 {
            let flips = session.receive_page_flips()?;
            if flips.is_empty() {
                continue;
            }
            if flips.len() != 1 {
                return Err(
                    "unexpected multiple DRM Vulkan page flips while resolving shutdown"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

fn probe_hotplug_unchanged(
    session: &PresenterSession,
    config: &DrmRunConfig,
) -> Result<bool, String> {
    let probe = probe_kms_output(config.card_path.as_deref(), config.requested_size)?;
    Ok(probe.connector == session.kms().connector
        && probe.crtc == session.kms().crtc
        && probe.primary == session.kms().primary
        && probe.dimensions == session.kms().dimensions
        && mode_frame_interval(&probe.mode) == mode_frame_interval(&session.kms().mode))
}

fn poll_fds(fds: &mut [libc::pollfd], timeout: Duration) -> Result<(), String> {
    let timeout_ms = i32::try_from(timeout.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
    loop {
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if result >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsRawFd, FromRawFd};

    use super::*;

    fn sync_file_fixture() -> OwnedFd {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        unsafe { libc::close(fds[1]) };
        unsafe { OwnedFd::from_raw_fd(fds[0]) }
    }

    fn prepare(lifecycle: &mut ScanoutLifecycle, generation: u64) -> usize {
        let slot = lifecycle.begin_render().unwrap();
        lifecycle
            .prepare(
                slot,
                generation,
                sync_file_fixture(),
                ImportedVideo {
                    frame_count: 0,
                    stream_identities: Vec::new(),
                },
                true,
                true,
            )
            .unwrap();
        slot
    }

    #[test]
    fn unavailable_optional_video_import_keeps_ui_presenter_admitted_but_reports_reason() {
        let (context, reason) = optional_video_import::<()>(Err(
            "Vulkan device cannot sample linear RGBA video".to_string(),
        ));

        assert!(context.is_none());
        assert_eq!(
            reason.as_deref(),
            Some("Vulkan device cannot sample linear RGBA video")
        );
        assert!(context.is_none(), "PRIME availability must remain false");
    }

    #[test]
    fn available_optional_video_import_is_reported_without_warning() {
        let (context, reason) = optional_video_import(Ok(37_u8));

        assert_eq!(context, Some(37));
        assert!(reason.is_none());
        assert!(context.is_some(), "PRIME availability may become true");
    }

    #[test]
    fn exactly_three_persistent_slots_are_required_by_trial_owner() {
        assert!(ScanoutLifecycle::new(2).is_err());
        assert_eq!(ScanoutLifecycle::new(3).unwrap().slots.len(), 3);
    }

    #[test]
    fn ebusy_preserves_master_and_prepared_generation() {
        let mut lifecycle = ScanoutLifecycle::new(3).unwrap();
        let slot = prepare(&mut lifecycle, 7);
        let duplicate = lifecycle.duplicate_in_fence_for_atomic_attempt().unwrap();
        assert!(duplicate.as_raw_fd() >= 0);
        drop(duplicate);
        lifecycle
            .complete_atomic_attempt(AtomicAttemptResult::Busy)
            .unwrap();
        assert_eq!(lifecycle.prepared, Some(slot));
        assert_eq!(lifecycle.slots[slot].generation, Some(7));
        assert!(lifecycle.slots[slot].master_sync_file.is_some());
    }

    #[test]
    fn busy_then_success_uses_a_fresh_duplicate_and_preserves_page_flip_identity() {
        let mut lifecycle = ScanoutLifecycle::new(3).unwrap();
        let slot = prepare(&mut lifecycle, 17);
        let mut retry = CommitRetryPolicy::new(2, 25);
        retry.begin_prepared_frame();

        retry.record_attempt();
        let first_duplicate = lifecycle.duplicate_in_fence_for_atomic_attempt().unwrap();
        drop(first_duplicate);
        lifecycle
            .complete_atomic_attempt(AtomicAttemptResult::Busy)
            .unwrap();
        assert_eq!(
            retry.on_busy(),
            CommitBusyAction::RetryAfter(Duration::from_millis(25))
        );

        retry.record_attempt();
        let second_duplicate = lifecycle.duplicate_in_fence_for_atomic_attempt().unwrap();
        drop(second_duplicate);
        lifecycle
            .complete_atomic_attempt(AtomicAttemptResult::Committed)
            .unwrap();
        let presented = lifecycle.page_flip().unwrap();
        assert_eq!(presented.slot, slot);
        assert_eq!(presented.generation, 17);
    }

    #[test]
    fn persistent_busy_exhausts_configured_retries_and_allows_idle_proven_repair() {
        let mut lifecycle = ScanoutLifecycle::new(3).unwrap();
        let slot = prepare(&mut lifecycle, 23);
        let mut retry = CommitRetryPolicy::new(2, 10);
        retry.begin_prepared_frame();

        for attempt in 1..=3 {
            retry.record_attempt();
            let duplicate = lifecycle.duplicate_in_fence_for_atomic_attempt().unwrap();
            drop(duplicate);
            lifecycle
                .complete_atomic_attempt(AtomicAttemptResult::Busy)
                .unwrap();
            let action = retry.on_busy();
            if attempt < 3 {
                assert_eq!(
                    action,
                    CommitBusyAction::RetryAfter(Duration::from_millis(10))
                );
                assert_eq!(lifecycle.prepared, Some(slot));
                assert!(lifecycle.slots[slot].master_sync_file.is_some());
            } else {
                assert_eq!(action, CommitBusyAction::Exhausted { attempts: 3 });
            }
        }

        // The production caller reaches this transition only after device-wait-idle proves the
        // queue submission complete and KMS has rejected every attempt with EBUSY.
        assert_eq!(lifecycle.discard_prepared_after_gpu_idle().unwrap(), slot);
        assert!(lifecycle.prepared.is_none());
        assert_eq!(lifecycle.slots[slot].state, LogicalState::Available);
        assert!(lifecycle.slots[slot].master_sync_file.is_none());
        assert!(lifecycle.begin_render().is_ok());
    }

    #[test]
    fn page_flip_promotes_and_reuses_only_replaced_slot_without_refs() {
        let mut lifecycle = ScanoutLifecycle::new(3).unwrap();
        let first = prepare(&mut lifecycle, 1);
        lifecycle
            .complete_atomic_attempt(AtomicAttemptResult::Committed)
            .unwrap();
        assert_eq!(lifecycle.page_flip().unwrap().slot, first);
        lifecycle.retire_capture(first).unwrap();
        lifecycle.retire_timing(first).unwrap();
        assert_eq!(lifecycle.slots[first].state, LogicalState::Current);

        let second = prepare(&mut lifecycle, 2);
        lifecycle
            .complete_atomic_attempt(AtomicAttemptResult::Committed)
            .unwrap();
        assert_eq!(lifecycle.page_flip().unwrap().slot, second);
        assert_eq!(lifecycle.slots[first].state, LogicalState::Available);
    }

    #[test]
    fn stale_generation_is_not_published_before_page_flip() {
        let mut lifecycle = ScanoutLifecycle::new(3).unwrap();
        let slot = prepare(&mut lifecycle, 44);
        assert!(lifecycle.current.is_none());
        lifecycle
            .complete_atomic_attempt(AtomicAttemptResult::Committed)
            .unwrap();
        assert!(lifecycle.current.is_none());
        assert_eq!(lifecycle.page_flip().unwrap().generation, 44);
        assert_eq!(lifecycle.current, Some(slot));
    }

    #[test]
    fn terminal_atomic_fault_and_timeout_quarantine_without_reuse() {
        for timeout in [false, true] {
            let mut lifecycle = ScanoutLifecycle::new(3).unwrap();
            let slot = prepare(&mut lifecycle, 5);
            let result = if timeout {
                lifecycle
                    .complete_atomic_attempt(AtomicAttemptResult::Committed)
                    .unwrap();
                lifecycle.page_flip_timeout()
            } else {
                lifecycle.complete_atomic_attempt(AtomicAttemptResult::Terminal(
                    AtomicCommitErrorKind::Invalid,
                ))
            };
            assert!(result.is_err());
            assert_eq!(lifecycle.slots[slot].state, LogicalState::Quarantined);
            assert!(lifecycle.begin_render().is_err());
        }
    }

    #[test]
    fn process_quarantine_policy_is_one_session_terminal_and_never_readmits() {
        let mut policy = ScanoutQuarantinePolicy::default();
        assert!(policy.admit().is_ok());
        assert!(policy.retain_one());
        assert_eq!(policy.retained_sessions, 1);
        assert!(policy.terminal);
        assert!(
            policy
                .admit()
                .unwrap_err()
                .contains("restart the VM/process")
        );
        assert!(!policy.retain_one());
        assert_eq!(policy.retained_sessions, 1);
        assert!(policy.admit().is_err());
    }

    #[test]
    fn required_extensions_are_no_wsi() {
        let names = required_device_extensions().map(|name| name.to_string_lossy().into_owned());
        assert!(names.iter().all(|name| {
            !name.contains("surface") && !name.contains("swapchain") && !name.contains("display")
        }));
    }
}
