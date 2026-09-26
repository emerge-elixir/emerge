use std::{
    os::fd::{FromRawFd, OwnedFd},
    sync::Arc,
};

use ash::vk;
use skia_safe::gpu::backend_semaphores;

use super::VulkanDevice;

/// DMA-BUF ownership outside this logical Vulkan device. `EXTERNAL` is the core external-memory
/// identity; `FOREIGN_EXT` is not enabled or silently substituted. This exact recipe is shared by
/// producer and importer and is hardware-matrix validated against EGL and Vulkan consumers.
pub const DMA_BUF_EXTERNAL_QUEUE_FAMILY: u32 = video_interop::vulkan::DMA_BUF_EXTERNAL_QUEUE_FAMILY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportPayloadState {
    ReadyForSignal,
    SignalPendingExport,
}

fn begin_export_payload_signal(
    state: ExportPayloadState,
) -> Result<ExportPayloadState, &'static str> {
    match state {
        ExportPayloadState::ReadyForSignal => Ok(ExportPayloadState::SignalPendingExport),
        ExportPayloadState::SignalPendingExport => {
            Err("Vulkan SYNC_FD semaphore payload was not exported before reuse")
        }
    }
}

fn finish_export_payload(state: ExportPayloadState) -> Result<ExportPayloadState, &'static str> {
    match state {
        ExportPayloadState::SignalPendingExport => Ok(ExportPayloadState::ReadyForSignal),
        ExportPayloadState::ReadyForSignal => {
            Err("Vulkan semaphore has no submitted payload to export")
        }
    }
}

pub struct ExportableSyncFdSemaphore {
    device: Arc<VulkanDevice>,
    semaphore: vk::Semaphore,
    state: ExportPayloadState,
}

impl ExportableSyncFdSemaphore {
    pub fn new(device: Arc<VulkanDevice>) -> Result<Self, String> {
        validate_sync_fd_export(&device)?;
        let mut export_info = vk::ExportSemaphoreCreateInfo::default()
            .handle_types(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        let create_info = vk::SemaphoreCreateInfo::default().push_next(&mut export_info);
        // SAFETY: the export create-info remains live for the call and the returned semaphore is
        // destroyed by this owner.
        let semaphore =
            unsafe { device.raw().create_semaphore(&create_info, None) }.map_err(|result| {
                format!("failed to create exportable Vulkan semaphore: {result:?}")
            })?;
        Ok(Self {
            device,
            semaphore,
            state: ExportPayloadState::ReadyForSignal,
        })
    }

    /// Reserves the semaphore's next binary payload for exactly one queue submission.
    pub fn begin_signal(&mut self) -> Result<vk::Semaphore, String> {
        self.state = begin_export_payload_signal(self.state).map_err(str::to_string)?;
        Ok(self.semaphore)
    }

    /// Exports and consumes the submitted binary payload exactly once. Successful SYNC_FD export
    /// resets the semaphore to its reusable unsignaled state.
    pub fn export_submitted_sync_fd(&mut self) -> Result<OwnedFd, String> {
        let next_state = finish_export_payload(self.state).map_err(str::to_string)?;
        let loader = ash::khr::external_semaphore_fd::Device::new(
            self.device.instance().raw(),
            self.device.raw(),
        );
        let info = vk::SemaphoreGetFdInfoKHR::default()
            .semaphore(self.semaphore)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        // SAFETY: this semaphore was created exportable and has one pending signal operation. A
        // successful SYNC_FD export transfers a new owned descriptor and consumes the payload.
        let raw_fd = match unsafe { loader.get_semaphore_fd(&info) } {
            Ok(fd) => fd,
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                self.device.mark_device_lost();
                return Err("Vulkan device lost while exporting render SYNC_FD".to_string());
            }
            Err(result) => {
                return Err(format!(
                    "failed to export Vulkan render SYNC_FD: {result:?}"
                ));
            }
        };
        if raw_fd < 0 {
            return Err("Vulkan returned an invalid render SYNC_FD".to_string());
        }
        self.state = next_state;
        // SAFETY: successful vkGetSemaphoreFdKHR transfers one new owned descriptor to us.
        Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
    }
}

impl Drop for ExportableSyncFdSemaphore {
    fn drop(&mut self) {
        // SAFETY: the containing presenter waits for or abandons device work before dropping slot
        // semaphores. Lost-device destruction is explicitly allowed by Vulkan.
        unsafe { self.device.raw().destroy_semaphore(self.semaphore, None) };
    }
}

pub struct ExternalQueueTransfer {
    device: Arc<VulkanDevice>,
    acquire_pool: vk::CommandPool,
    acquire_command: vk::CommandBuffer,
    release_pool: vk::CommandPool,
    release_command: vk::CommandBuffer,
    release_fence: vk::Fence,
    ganesh_complete: vk::Semaphore,
    release_pending: bool,
}

impl ExternalQueueTransfer {
    pub fn new(device: Arc<VulkanDevice>) -> Result<Self, String> {
        let acquire_pool = create_command_pool(&device, "external acquire")?;
        let release_pool = match create_command_pool(&device, "external release") {
            Ok(pool) => pool,
            Err(error) => {
                unsafe { device.raw().destroy_command_pool(acquire_pool, None) };
                return Err(error);
            }
        };
        let result = (|| {
            let acquire_command = allocate_command(&device, acquire_pool, "external acquire")?;
            let release_command = allocate_command(&device, release_pool, "external release")?;
            let release_fence = unsafe {
                device
                    .raw()
                    .create_fence(&vk::FenceCreateInfo::default(), None)
            }
            .map_err(|result| format!("failed to create Vulkan release fence: {result:?}"))?;
            let ganesh_complete = match unsafe {
                device
                    .raw()
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            } {
                Ok(semaphore) => semaphore,
                Err(result) => {
                    unsafe { device.raw().destroy_fence(release_fence, None) };
                    return Err(format!(
                        "failed to create Ganesh completion semaphore: {result:?}"
                    ));
                }
            };
            Ok(Self {
                device: Arc::clone(&device),
                acquire_pool,
                acquire_command,
                release_pool,
                release_command,
                release_fence,
                ganesh_complete,
                release_pending: false,
            })
        })();
        if result.is_err() {
            unsafe {
                device.raw().destroy_command_pool(release_pool, None);
                device.raw().destroy_command_pool(acquire_pool, None);
            }
        }
        result
    }

    pub fn ganesh_completion_semaphore(&self) -> Result<vk::Semaphore, String> {
        if self.release_pending {
            return Err("Vulkan slot release submission is still pending".to_string());
        }
        Ok(self.ganesh_complete)
    }

    pub fn submit_release(
        &mut self,
        image: vk::Image,
        old_layout: vk::ImageLayout,
        completion_semaphore: vk::Semaphore,
    ) -> Result<(), String> {
        if self.release_pending {
            return Err("Vulkan slot already has a pending external release".to_string());
        }
        unsafe {
            self.device
                .raw()
                .reset_fences(&[self.release_fence])
                .map_err(|result| format!("failed to reset Vulkan release fence: {result:?}"))?;
            self.device
                .raw()
                .reset_command_pool(self.release_pool, vk::CommandPoolResetFlags::empty())
                .map_err(|result| {
                    format!("failed to reset Vulkan release command pool: {result:?}")
                })?;
        }
        record_ownership_barrier(
            &self.device,
            self.release_command,
            image,
            old_layout,
            vk::ImageLayout::GENERAL,
            self.device.queue_family_index(),
            DMA_BUF_EXTERNAL_QUEUE_FAMILY,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::empty(),
        )?;
        let wait = [self.ganesh_complete];
        let wait_stages = [vk::PipelineStageFlags::ALL_COMMANDS];
        let commands = [self.release_command];
        let signals = [completion_semaphore];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&commands)
            .signal_semaphores(&signals);
        match unsafe {
            self.device
                .raw()
                .queue_submit(self.device.queue(), &[submit], self.release_fence)
        } {
            Ok(()) => {
                self.release_pending = true;
                Ok(())
            }
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                self.device.mark_device_lost();
                Err("Vulkan device lost while submitting external release".to_string())
            }
            Err(result) => Err(format!(
                "failed to submit Vulkan external release: {result:?}"
            )),
        }
    }

    pub fn release_complete(&self) -> Result<bool, String> {
        if !self.release_pending {
            return Ok(true);
        }
        match unsafe { self.device.raw().get_fence_status(self.release_fence) } {
            Ok(signaled) => Ok(signaled),
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                self.device.mark_device_lost();
                Err("Vulkan device lost while polling external release".to_string())
            }
            Err(result) => Err(format!(
                "failed to poll Vulkan external release fence: {result:?}"
            )),
        }
    }

    /// Submits the paired external-to-graphics acquire and returns a fresh one-shot semaphore for
    /// Ganesh. The caller must transfer semaphore destruction ownership through Surface::wait.
    pub fn submit_acquire(&mut self, image: vk::Image) -> Result<vk::Semaphore, String> {
        if !self.release_pending || !self.release_complete()? {
            return Err("Vulkan slot external release is not complete".to_string());
        }
        unsafe {
            self.device
                .raw()
                .reset_command_pool(self.acquire_pool, vk::CommandPoolResetFlags::empty())
                .map_err(|result| {
                    format!("failed to reset Vulkan acquire command pool: {result:?}")
                })?;
        }
        let semaphore = unsafe {
            self.device
                .raw()
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
        }
        .map_err(|result| format!("failed to create Vulkan acquire semaphore: {result:?}"))?;
        if let Err(error) = record_ownership_barrier(
            &self.device,
            self.acquire_command,
            image,
            vk::ImageLayout::GENERAL,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            DMA_BUF_EXTERNAL_QUEUE_FAMILY,
            self.device.queue_family_index(),
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::empty(),
            vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        ) {
            unsafe { self.device.raw().destroy_semaphore(semaphore, None) };
            return Err(error);
        }
        let commands = [self.acquire_command];
        let signals = [semaphore];
        let submit = vk::SubmitInfo::default()
            .command_buffers(&commands)
            .signal_semaphores(&signals);
        match unsafe {
            self.device
                .raw()
                .queue_submit(self.device.queue(), &[submit], vk::Fence::null())
        } {
            Ok(()) => {
                self.release_pending = false;
                Ok(semaphore)
            }
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                unsafe { self.device.raw().destroy_semaphore(semaphore, None) };
                self.device.mark_device_lost();
                Err("Vulkan device lost while submitting external acquire".to_string())
            }
            Err(result) => {
                unsafe { self.device.raw().destroy_semaphore(semaphore, None) };
                Err(format!(
                    "failed to submit Vulkan external acquire: {result:?}"
                ))
            }
        }
    }
}

impl Drop for ExternalQueueTransfer {
    fn drop(&mut self) {
        unsafe {
            self.device
                .raw()
                .destroy_semaphore(self.ganesh_complete, None);
            self.device.raw().destroy_fence(self.release_fence, None);
            self.device
                .raw()
                .destroy_command_pool(self.release_pool, None);
            self.device
                .raw()
                .destroy_command_pool(self.acquire_pool, None);
        }
    }
}

fn create_command_pool(device: &VulkanDevice, label: &str) -> Result<vk::CommandPool, String> {
    let info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(device.queue_family_index())
        .flags(vk::CommandPoolCreateFlags::TRANSIENT);
    unsafe { device.raw().create_command_pool(&info, None) }
        .map_err(|result| format!("failed to create Vulkan {label} command pool: {result:?}"))
}

fn allocate_command(
    device: &VulkanDevice,
    pool: vk::CommandPool,
    label: &str,
) -> Result<vk::CommandBuffer, String> {
    let info = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    unsafe { device.raw().allocate_command_buffers(&info) }
        .map_err(|result| format!("failed to allocate Vulkan {label} command buffer: {result:?}"))?
        .into_iter()
        .next()
        .ok_or_else(|| format!("Vulkan returned no {label} command buffer"))
}

#[allow(clippy::too_many_arguments)]
fn record_ownership_barrier(
    device: &VulkanDevice,
    command: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
    source_queue_family: u32,
    destination_queue_family: u32,
    source_stage: vk::PipelineStageFlags,
    destination_stage: vk::PipelineStageFlags,
    source_access: vk::AccessFlags,
    destination_access: vk::AccessFlags,
) -> Result<(), String> {
    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe { device.raw().begin_command_buffer(command, &begin) }
        .map_err(|result| format!("failed to begin Vulkan ownership command buffer: {result:?}"))?;
    let barrier = vk::ImageMemoryBarrier::default()
        .src_access_mask(source_access)
        .dst_access_mask(destination_access)
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(source_queue_family)
        .dst_queue_family_index(destination_queue_family)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        );
    unsafe {
        device.raw().cmd_pipeline_barrier(
            command,
            source_stage,
            destination_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
        device.raw().end_command_buffer(command)
    }
    .map_err(|result| format!("failed to end Vulkan ownership command buffer: {result:?}"))
}

pub use video_interop::vulkan::{
    ImportedImageSyncError, ImportedImageSyncErrorKind, VulkanVideoTiming,
};

pub type ImportedImageSync = video_interop::vulkan::ImportedImageSync<VulkanDevice>;

pub fn validate_sync_fd_import(device: &Arc<VulkanDevice>) -> Result<(), String> {
    video_interop::vulkan::validate_sync_fd_import(device.as_ref())
}

pub fn wait_surface_on_semaphore(
    surface: &mut skia_safe::Surface,
    semaphore: vk::Semaphore,
) -> bool {
    let backend = unsafe { backend_semaphores::make_vk(super::raw::semaphore_to_skia(semaphore)) };
    surface.wait(&[backend], true)
}

fn validate_sync_fd_export(device: &Arc<VulkanDevice>) -> Result<(), String> {
    let info = vk::PhysicalDeviceExternalSemaphoreInfo::default()
        .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
    let mut properties = vk::ExternalSemaphoreProperties::default();
    // SAFETY: query input and output storage remain valid for the call.
    unsafe {
        device
            .instance()
            .raw()
            .get_physical_device_external_semaphore_properties(
                device.physical_device(),
                &info,
                &mut properties,
            )
    };
    if !properties
        .external_semaphore_features
        .contains(vk::ExternalSemaphoreFeatureFlags::EXPORTABLE)
        || !properties
            .compatible_handle_types
            .contains(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD)
    {
        return Err("Vulkan device cannot export binary semaphores as SYNC_FD".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_payload_state_requires_one_export_per_signal() {
        let pending =
            begin_export_payload_signal(ExportPayloadState::ReadyForSignal).expect("first signal");
        assert!(begin_export_payload_signal(pending).is_err());
        let ready = finish_export_payload(pending).expect("one export");
        assert!(finish_export_payload(ready).is_err());
        assert_eq!(
            begin_export_payload_signal(ready).expect("next signal"),
            ExportPayloadState::SignalPendingExport
        );
    }

    #[test]
    fn dma_buf_ownership_uses_the_core_external_queue_family() {
        assert_eq!(DMA_BUF_EXTERNAL_QUEUE_FAMILY, vk::QUEUE_FAMILY_EXTERNAL);
        assert_ne!(DMA_BUF_EXTERNAL_QUEUE_FAMILY, vk::QUEUE_FAMILY_FOREIGN_EXT);
    }
}
