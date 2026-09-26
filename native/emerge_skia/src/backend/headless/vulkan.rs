use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::{FileTypeExt, MetadataExt},
    },
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use ash::vk;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use video_interop::{AcquireSync, SyncFile};

use crate::{
    backend::vulkan::{
        AcquiredTarget, DMA_BUF_EXTERNAL_QUEUE_FAMILY, DrmNodeId, EXPORTED_PRIME_IMAGE_USAGE,
        ExactDeviceRequirements, ExportableSyncFdSemaphore, ExportedDmaBufImage,
        ExternalQueueTransfer, SelectionNode, TargetImageState, VulkanDevice, VulkanEngine,
        VulkanInstance, VulkanRendererReport, VulkanTargetFormat, VulkanTargetSurface,
        capabilities::DrmMatchField,
    },
    renderer::{RenderState, RendererCacheConfig},
};

use super::{HeadlessPrimeExport, HeadlessPrimeTimings, PrimeObjectMeta, PrimePlaneMeta};

const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");

struct GpuCompletionRequest {
    release_id: u64,
    fence: OwnedFd,
}

enum GpuCompletionAck {
    Complete,
    Retry,
}

struct GpuCompletionWaiter {
    request_tx: Option<Sender<GpuCompletionRequest>>,
    ready_rx: Receiver<u64>,
    ack_tx: Sender<GpuCompletionAck>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl GpuCompletionWaiter {
    fn start() -> Result<Self, String> {
        let (request_tx, request_rx) = unbounded::<GpuCompletionRequest>();
        let (ready_tx, ready_rx) = unbounded::<u64>();
        let (ack_tx, ack_rx) = bounded::<GpuCompletionAck>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("emerge-vulkan-prime-completion".to_string())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    let Ok(request) = request_rx.recv_timeout(Duration::from_millis(100)) else {
                        continue;
                    };
                    if !wait_sync_file(&request.fence, &worker_stop) {
                        break;
                    }
                    'notify: loop {
                        if ready_tx.send(request.release_id).is_err() {
                            return;
                        }
                        loop {
                            match ack_rx.recv_timeout(Duration::from_millis(100)) {
                                Ok(GpuCompletionAck::Complete) => break 'notify,
                                Ok(GpuCompletionAck::Retry) => {
                                    thread::sleep(Duration::from_millis(1));
                                    continue 'notify;
                                }
                                Err(crossbeam_channel::RecvTimeoutError::Timeout)
                                    if !worker_stop.load(Ordering::Acquire) => {}
                                Err(_) => return,
                            }
                        }
                    }
                }
            })
            .map_err(|error| format!("failed to spawn Vulkan completion waiter: {error}"))?;
        Ok(Self {
            request_tx: Some(request_tx),
            ready_rx,
            ack_tx,
            stop,
            handle: Some(handle),
        })
    }

    fn submit(&self, release_id: u64, fence: OwnedFd) -> Result<(), String> {
        self.request_tx
            .as_ref()
            .ok_or_else(|| "Vulkan completion waiter is closed".to_string())?
            .send(GpuCompletionRequest { release_id, fence })
            .map_err(|_| "Vulkan completion waiter request channel closed".to_string())
    }

    fn receiver(&self) -> Receiver<u64> {
        self.ready_rx.clone()
    }

    fn acknowledge(&self, complete: bool) {
        let ack = if complete {
            GpuCompletionAck::Complete
        } else {
            GpuCompletionAck::Retry
        };
        let _ = self.ack_tx.send(ack);
    }

    fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.request_tx.take();
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            eprintln!("Vulkan completion waiter panicked during shutdown");
        }
    }
}

impl Drop for GpuCompletionWaiter {
    fn drop(&mut self) {
        self.close();
    }
}

fn wait_sync_file(fence: &OwnedFd, stop: &AtomicBool) -> bool {
    let mut poll_fd = libc::pollfd {
        fd: fence.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    while !stop.load(Ordering::Acquire) {
        let result = unsafe { libc::poll(&mut poll_fd, 1, 100) };
        if result > 0 {
            return true;
        }
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return true;
        }
    }
    false
}

struct VulkanPrimeSlot {
    // Borrowed Ganesh surfaces must be dropped before their allocation owner.
    surface: Option<VulkanTargetSurface>,
    transfer: ExternalQueueTransfer,
    completion: ExportableSyncFdSemaphore,
    acquire_fence: Option<OwnedFd>,
    allocation: ExportedDmaBufImage,
}

pub(super) struct VulkanHeadlessRenderer {
    _drm_node: File,
    renderer_report: VulkanRendererReport,
    engine: Option<VulkanEngine>,
    dimensions: (u32, u32),
    max_in_flight: usize,
    next_release_id: u64,
    available: Vec<VulkanPrimeSlot>,
    in_flight: HashMap<u64, VulkanPrimeSlot>,
    retiring: HashMap<u64, VulkanPrimeSlot>,
    completion_waiter: GpuCompletionWaiter,
    quarantined: Vec<VulkanPrimeSlot>,
    terminal_error: Option<String>,
}

impl VulkanHeadlessRenderer {
    pub(super) fn new_prime(
        width: u32,
        height: u32,
        renderer_cache_config: RendererCacheConfig,
        max_in_flight: u32,
        configured_drm_node: Option<&str>,
    ) -> Result<Self, String> {
        let dimensions = (width.max(1), height.max(1));
        let (drm_node, path, selection_node) = open_selection_node(configured_drm_node)?;
        let instance = VulkanInstance::new(&[])?;
        let required_extensions = [
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::ext::image_drm_format_modifier::NAME,
            ash::khr::external_semaphore_fd::NAME,
            ash::ext::physical_device_drm::NAME,
        ];
        let device = VulkanDevice::new_for_drm_node(
            Arc::clone(&instance),
            ExactDeviceRequirements {
                required_extensions: &required_extensions,
                require_timestamps: false,
                selection_node,
            },
        )?;
        eprintln!(
            "headless PRIME Vulkan device: {} via {path} ({} {}:{})",
            device.physical_device_name(),
            selection_node.field.as_str(),
            selection_node.node.major,
            selection_node.node.minor
        );
        let renderer_report =
            VulkanRendererReport::for_selected_node(&device, path, selection_node);
        let mut engine = VulkanEngine::new(Arc::clone(&device), renderer_cache_config)?;
        let first_slot = create_slot(&mut engine, device, dimensions)?;

        Ok(Self {
            _drm_node: drm_node,
            renderer_report,
            engine: Some(engine),
            dimensions,
            max_in_flight: max_in_flight.max(1) as usize,
            next_release_id: 1,
            available: vec![first_slot],
            in_flight: HashMap::new(),
            retiring: HashMap::new(),
            completion_waiter: GpuCompletionWaiter::start()?,
            quarantined: Vec::new(),
            terminal_error: None,
        })
    }

    pub(super) fn renderer_report(&self) -> VulkanRendererReport {
        self.renderer_report.clone()
    }

    pub(super) fn render_prime(
        &mut self,
        state: &RenderState,
    ) -> Result<Option<HeadlessPrimeExport>, String> {
        let prepare_started_at = Instant::now();
        if let Some(error) = self.terminal_error.clone() {
            return Err(error);
        }
        if self.in_flight.len() + self.retiring.len() >= self.max_in_flight {
            return Ok(None);
        }

        let mut slot = match self.available.pop() {
            Some(slot) => slot,
            None => {
                let engine = self
                    .engine
                    .as_mut()
                    .ok_or_else(|| "headless Vulkan engine is shut down".to_string())?;
                create_slot(engine, Arc::clone(engine.device()), self.dimensions)?
            }
        };
        let release_id = self.next_release_id;
        self.next_release_id = self.next_release_id.wrapping_add(1).max(1);
        let prepare = prepare_started_at.elapsed();

        let result = self.render_slot(state, release_id, prepare, &mut slot);
        match result {
            Ok(export) => {
                self.in_flight.insert(release_id, slot);
                Ok(Some(export))
            }
            Err(error) => {
                self.quarantined.push(slot);
                self.enter_terminal_error(error.clone());
                Err(error)
            }
        }
    }

    fn render_slot(
        &mut self,
        state: &RenderState,
        release_id: u64,
        prepare: Duration,
        slot: &mut VulkanPrimeSlot,
    ) -> Result<HeadlessPrimeExport, String> {
        debug_assert!(slot.acquire_fence.is_none());
        let surface = slot
            .surface
            .as_mut()
            .ok_or_else(|| "headless Vulkan slot surface is destroyed".to_string())?;
        let acquire_semaphore =
            if surface.state().queue_family_index == DMA_BUF_EXTERNAL_QUEUE_FAMILY {
                let semaphore = slot.transfer.submit_acquire(slot.allocation.image())?;
                surface.set_state(TargetImageState {
                    layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    queue_family_index: self
                        .engine
                        .as_ref()
                        .ok_or_else(|| "headless Vulkan engine is shut down".to_string())?
                        .device()
                        .queue_family_index(),
                });
                Some(semaphore)
            } else {
                None
            };
        let current_state = surface.state();
        let final_state = TargetImageState {
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            queue_family_index: current_state.queue_family_index,
        };
        let ganesh_complete = slot.transfer.ganesh_completion_semaphore()?;
        let completion_semaphore = slot.completion.begin_signal()?;
        let acquired = AcquiredTarget {
            token: (),
            image: slot.allocation.image(),
            dimensions: self.dimensions,
            current_state,
            acquire_semaphore,
            completion_semaphore: ganesh_complete,
            final_state,
        };
        let engine = self
            .engine
            .as_mut()
            .ok_or_else(|| "headless Vulkan engine is shut down".to_string())?;
        let (timings, completed) = engine.render(surface, acquired, false, |renderer, frame| {
            renderer.render(frame, state)
        })?;
        if completed.completion_semaphore != ganesh_complete || completed.final_state != final_state
        {
            return Err("shared Vulkan engine returned inconsistent completion state".to_string());
        }

        slot.transfer.submit_release(
            slot.allocation.image(),
            final_state.layout,
            completion_semaphore,
        )?;
        surface.set_state(TargetImageState {
            layout: vk::ImageLayout::GENERAL,
            queue_family_index: DMA_BUF_EXTERNAL_QUEUE_FAMILY,
        });

        let fence_export_started_at = Instant::now();
        let fence = slot.completion.export_submitted_sync_fd()?;
        let fence_export = fence_export_started_at.elapsed();
        let acquire_fence_fd = fence.as_raw_fd();
        slot.acquire_fence = Some(fence);

        let export_metadata_started_at = Instant::now();
        let plane = slot.allocation.plane();
        let export = HeadlessPrimeExport {
            release_id,
            width: self.dimensions.0,
            height: self.dimensions.1,
            format: DRM_FORMAT_ABGR8888,
            objects: vec![PrimeObjectMeta {
                fd: slot.allocation.fd(),
                size: slot.allocation.fd_allocation_size(),
                modifier: Some(slot.allocation.modifier()),
            }],
            planes: vec![PrimePlaneMeta {
                object_index: 0,
                pitch: plane.pitch,
                offset: plane.offset,
            }],
            acquire_sync: AcquireSync::SyncFile(SyncFile { acquire_fence_fd }),
            timings,
            prime_timings: HeadlessPrimeTimings {
                prepare,
                retarget: Duration::ZERO,
                fence_export: Some(fence_export),
                gpu_finish_fallback: None,
                export_metadata: export_metadata_started_at.elapsed(),
            },
        };
        Ok(export)
    }

    pub(super) fn release_prime(&mut self, release_id: u64) -> bool {
        let Some(mut slot) = self.in_flight.remove(&release_id) else {
            return false;
        };
        let Some(fence) = slot.acquire_fence.take() else {
            self.enter_terminal_error(
                "Vulkan PRIME slot lost its retained completion sync file".to_string(),
            );
            self.quarantined.push(slot);
            return false;
        };
        self.retiring.insert(release_id, slot);
        if let Err(error) = self.completion_waiter.submit(release_id, fence) {
            if let Some(slot) = self.retiring.remove(&release_id) {
                self.quarantined.push(slot);
            }
            self.enter_terminal_error(error);
        }
        false
    }

    pub(super) fn completion_receiver(&self) -> Receiver<u64> {
        self.completion_waiter.receiver()
    }

    pub(super) fn complete_retirement(&mut self, release_id: u64) -> bool {
        let Some(slot) = self.retiring.remove(&release_id) else {
            self.completion_waiter.acknowledge(true);
            return false;
        };
        match slot.transfer.release_complete() {
            Ok(true) => {
                self.available.push(slot);
                self.completion_waiter.acknowledge(true);
                true
            }
            Ok(false) => {
                self.retiring.insert(release_id, slot);
                self.completion_waiter.acknowledge(false);
                false
            }
            Err(error) => {
                self.enter_terminal_error(error);
                self.quarantined.push(slot);
                self.completion_waiter.acknowledge(true);
                false
            }
        }
    }

    fn enter_terminal_error(&mut self, error: String) {
        if self
            .engine
            .as_ref()
            .is_some_and(|engine| engine.device().is_device_lost())
            && let Some(engine) = self.engine.as_mut()
        {
            engine.mark_device_lost();
        }
        self.terminal_error.get_or_insert(error);
    }

    pub(super) fn terminal_prime_shutdown_ready(&self) -> bool {
        self.terminal_error.is_some() && self.in_flight.is_empty()
    }
}

impl Drop for VulkanHeadlessRenderer {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            if !engine.device().is_device_lost()
                && let Err(error) = engine.device().wait_idle("headless Vulkan shutdown")
            {
                eprintln!("{error}");
                engine.mark_device_lost();
            }
            engine.drop_scene_renderer();
        }
        self.completion_waiter.close();
        self.available
            .iter_mut()
            .chain(self.in_flight.values_mut())
            .chain(self.retiring.values_mut())
            .chain(self.quarantined.iter_mut())
            .for_each(|slot| {
                slot.surface.take();
            });
        if let Some(engine) = self.engine.as_mut() {
            engine.shutdown_ganesh();
        }
        self.available.clear();
        self.in_flight.clear();
        self.retiring.clear();
        self.quarantined.clear();
        self.engine.take();
    }
}

fn create_slot(
    engine: &mut VulkanEngine,
    device: Arc<VulkanDevice>,
    dimensions: (u32, u32),
) -> Result<VulkanPrimeSlot, String> {
    let allocation = ExportedDmaBufImage::new_linear_rgba(Arc::clone(&device), dimensions)?;
    let initial_state = TargetImageState {
        layout: vk::ImageLayout::UNDEFINED,
        queue_family_index: device.queue_family_index(),
    };
    let surface = engine.create_target_surface_with_format_usage_and_tiling(
        allocation.image(),
        dimensions,
        initial_state,
        VulkanTargetFormat::Rgba8888,
        EXPORTED_PRIME_IMAGE_USAGE,
        skia_safe::gpu::vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
    )?;
    let transfer = ExternalQueueTransfer::new(Arc::clone(&device))?;
    let completion = ExportableSyncFdSemaphore::new(device)?;
    Ok(VulkanPrimeSlot {
        surface: Some(surface),
        transfer,
        completion,
        acquire_fence: None,
        allocation,
    })
}

fn open_selection_node(
    configured_path: Option<&str>,
) -> Result<(File, String, SelectionNode), String> {
    let path = match configured_path {
        Some(path) => path.to_string(),
        None => automatically_select_drm_node()?,
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| format!("failed to open headless Vulkan DRM node {path}: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to stat headless Vulkan DRM node {path}: {error}"))?;
    if !metadata.file_type().is_char_device() {
        return Err(format!(
            "headless Vulkan DRM node {path} is not a character device"
        ));
    }
    let rdev = metadata.rdev();
    let node = DrmNodeId {
        major: libc::major(rdev),
        minor: libc::minor(rdev),
    };
    let sysfs_node = std::fs::canonicalize(format!("/sys/dev/char/{}:{}", node.major, node.minor))
        .map_err(|error| format!("failed to resolve DRM identity for {path}: {error}"))?;
    let name = sysfs_node
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("headless Vulkan DRM node {path} has no sysfs name"))?;
    let field = if name.starts_with("renderD") {
        DrmMatchField::Render
    } else if name.starts_with("card") {
        DrmMatchField::Primary
    } else {
        return Err(format!(
            "headless Vulkan DRM node {path} must be a primary or render node, got {name}"
        ));
    };
    Ok((file, path, SelectionNode { node, field }))
}

fn automatically_select_drm_node() -> Result<String, String> {
    let candidates = discovered_dri_device_paths()
        .into_iter()
        .filter_map(|path| {
            let file = OpenOptions::new().read(true).write(true).open(&path).ok()?;
            let metadata = file.metadata().ok()?;
            let rdev = metadata.rdev();
            let node = DrmNodeId {
                major: libc::major(rdev),
                minor: libc::minor(rdev),
            };
            let device = std::fs::canonicalize(format!(
                "/sys/dev/char/{}:{}/device",
                node.major, node.minor
            ))
            .ok()?;
            Some((device.to_string_lossy().into_owned(), path, node))
        })
        .fold(
            HashMap::<String, Vec<(String, DrmNodeId)>>::new(),
            |mut groups, (device, path, node)| {
                groups.entry(device).or_default().push((path, node));
                groups
            },
        );
    if candidates.len() != 1 {
        return Err(format!(
            "headless Vulkan DRM node selection is ambiguous across {} devices; configure :headless.prime.drm_node explicitly",
            candidates.len()
        ));
    }
    let mut nodes = candidates.into_values().next().unwrap_or_default();
    nodes.sort_by_key(|(_path, node)| (node.minor < 128, node.minor));
    nodes
        .into_iter()
        .next()
        .map(|(path, _node)| path)
        .ok_or_else(|| "headless Vulkan found no usable DRM node".to_string())
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
