use std::{sync::Arc, time::Instant};

use ash::vk;
use skia_safe::{
    AlphaType, ColorType, ImageInfo,
    gpu::{self, backend_semaphores, vk as sk_vk},
};

use crate::renderer::{RenderFlushTimings, RenderFrame, RendererCacheConfig, SceneRenderer};

use super::{
    device::VulkanDevice,
    ganesh::{GaneshContext, VulkanTargetFormat, VulkanTargetSurface},
    raw,
};

/// Presenter-owned image state at an acquisition or completion boundary. This contract uses only
/// Vulkan state and queue-family values; it does not encode Wayland presentation or DRM ownership.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TargetImageState {
    pub layout: vk::ImageLayout,
    pub queue_family_index: u32,
}

/// Presenter-neutral description of one borrowed output target. The generic token stays opaque to
/// the engine and can be a Wayland generation/image pair or a future DRM slot id. Acquire
/// semaphores are one-shot: a successful Skia wait transfers destruction ownership to Skia, while
/// every pre-wait failure leaves destruction to the engine.
pub struct AcquiredTarget<T> {
    pub token: T,
    pub image: vk::Image,
    pub dimensions: (u32, u32),
    pub current_state: TargetImageState,
    pub acquire_semaphore: Option<vk::Semaphore>,
    pub completion_semaphore: vk::Semaphore,
    pub final_state: TargetImageState,
}

#[derive(Clone)]
pub struct CapturedRgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Completed shared rendering work. The presenter decides whether the binary completion signal is
/// consumed by WSI or exported/bridged by a future no-WSI presenter. Presentation acknowledgement
/// remains outside the shared engine.
pub struct CompletedTarget<T> {
    pub token: T,
    pub completion_semaphore: vk::Semaphore,
    pub final_state: TargetImageState,
    pub capture: Option<CapturedRgba>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureFlushRoute {
    DirectToCompletion,
    CaptureThenCompletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AcquireSemaphoreOwner {
    Presenter,
    Skia,
    Destroyed,
}

const fn acquire_semaphore_owner_after_wait(
    owner: AcquireSemaphoreOwner,
    accepted: bool,
) -> AcquireSemaphoreOwner {
    match (owner, accepted) {
        (AcquireSemaphoreOwner::Presenter, true) => AcquireSemaphoreOwner::Skia,
        (AcquireSemaphoreOwner::Presenter, false) => AcquireSemaphoreOwner::Destroyed,
        (owner, _) => owner,
    }
}

const fn capture_flush_route(capture_requested: bool) -> CaptureFlushRoute {
    if capture_requested {
        CaptureFlushRoute::CaptureThenCompletion
    } else {
        CaptureFlushRoute::DirectToCompletion
    }
}

pub struct VulkanEngine {
    renderer: Option<SceneRenderer>,
    ganesh: GaneshContext,
    device: Arc<VulkanDevice>,
}

impl VulkanEngine {
    pub fn new(
        device: Arc<VulkanDevice>,
        renderer_cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        let ganesh = GaneshContext::new(&device)?;
        Ok(Self {
            renderer: Some(SceneRenderer::with_cache_config(renderer_cache_config)),
            ganesh,
            device,
        })
    }

    pub fn device(&self) -> &Arc<VulkanDevice> {
        &self.device
    }

    pub fn create_target_surface(
        &mut self,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
    ) -> Result<VulkanTargetSurface, String> {
        VulkanTargetSurface::new(&mut self.ganesh, image, dimensions, initial_state)
    }

    pub fn create_target_surface_with_format(
        &mut self,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
        format: VulkanTargetFormat,
    ) -> Result<VulkanTargetSurface, String> {
        VulkanTargetSurface::new_with_format(
            &mut self.ganesh,
            image,
            dimensions,
            initial_state,
            format,
        )
    }

    pub fn create_target_surface_with_format_and_usage(
        &mut self,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
        format: VulkanTargetFormat,
        usage: vk::ImageUsageFlags,
    ) -> Result<VulkanTargetSurface, String> {
        VulkanTargetSurface::new_with_format_and_usage(
            &mut self.ganesh,
            image,
            dimensions,
            initial_state,
            format,
            usage,
        )
    }

    pub fn create_target_surface_with_format_usage_and_tiling(
        &mut self,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
        format: VulkanTargetFormat,
        usage: vk::ImageUsageFlags,
        tiling: sk_vk::ImageTiling,
    ) -> Result<VulkanTargetSurface, String> {
        VulkanTargetSurface::new_with_format_usage_and_tiling(
            &mut self.ganesh,
            image,
            dimensions,
            initial_state,
            format,
            usage,
            tiling,
        )
    }

    pub fn renderer_mut(&mut self) -> Result<&mut SceneRenderer, String> {
        self.renderer
            .as_mut()
            .ok_or_else(|| "Vulkan scene renderer is shut down".to_string())
    }

    pub fn render<T, R>(
        &mut self,
        target_surface: &mut VulkanTargetSurface,
        acquired: AcquiredTarget<T>,
        capture_requested: bool,
        draw: impl FnOnce(&mut SceneRenderer, &mut RenderFrame<'_>) -> R,
    ) -> Result<(R, CompletedTarget<T>), String> {
        self.validate_target(target_surface, &acquired)?;
        let final_layout = match raw::image_layout_to_skia(acquired.final_state.layout) {
            Ok(layout) => layout,
            Err(error) => {
                if let Some(acquire_semaphore) = acquired.acquire_semaphore {
                    retire_rejected_acquire_semaphore(&self.device, acquire_semaphore);
                }
                return Err(error);
            }
        };

        if let Some(acquire_semaphore) = acquired.acquire_semaphore {
            let backend_acquire =
                unsafe { backend_semaphores::make_vk(raw::semaphore_to_skia(acquire_semaphore)) };
            let wait_accepted = target_surface.surface_mut().wait(&[backend_acquire], true);
            if acquire_semaphore_owner_after_wait(AcquireSemaphoreOwner::Presenter, wait_accepted)
                == AcquireSemaphoreOwner::Destroyed
            {
                // The signal operation may still be pending. A rejected wait is terminal and may
                // use a teardown wait, but must never destroy a semaphore still referenced by a
                // queue or WSI acquire operation.
                retire_rejected_acquire_semaphore(&self.device, acquire_semaphore);
                return Err("Skia rejected the Vulkan acquire semaphore wait".to_string());
            }
            // Surface::wait(true) has transferred destruction ownership to Skia. The presenter
            // must never reuse or destroy this one-shot semaphore.
        }

        let route = capture_flush_route(capture_requested);
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| "Vulkan scene renderer is shut down".to_string())?;
        let context = self.ganesh.direct_context_mut()?;
        let queue_family_index = self.device.queue_family_index();
        let completion_semaphore = acquired.completion_semaphore;
        let final_state = acquired.final_state;
        let mut flush_error = None;
        let mut flushed = false;
        let result = {
            let mut flusher = |surface: &mut skia_safe::Surface,
                               context: &mut gpu::DirectContext,
                               post_flush_tasks: &mut Vec<
                std::sync::Arc<dyn crate::renderer::BackendPostFlushTask>,
            >| {
                if std::mem::replace(&mut flushed, true) {
                    flush_error = Some(
                        "shared Vulkan frame attempted more than one Ganesh flush".to_string(),
                    );
                    return RenderFlushTimings::default();
                }
                let final_submission = matches!(route, CaptureFlushRoute::DirectToCompletion);
                match flush_surface(
                    context,
                    surface,
                    if final_submission {
                        final_state.queue_family_index
                    } else {
                        queue_family_index
                    },
                    final_submission.then_some(completion_semaphore),
                    if final_submission {
                        final_layout
                    } else {
                        sk_vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
                    },
                ) {
                    Ok(timings) => {
                        if let Err(error) = post_flush_tasks
                            .drain(..)
                            .try_for_each(|task| task.submit_after_flush())
                        {
                            flush_error = Some(error);
                        }
                        timings
                    }
                    Err(error) => {
                        flush_error = Some(error);
                        RenderFlushTimings::default()
                    }
                }
            };
            let mut frame = RenderFrame::new_with_backend_flusher(
                target_surface.surface_mut(),
                context,
                &mut flusher,
            );
            draw(renderer, &mut frame)
        };
        if let Some(error) = flush_error {
            return Err(error);
        }
        if !flushed {
            return Err("shared Vulkan frame completed without a Ganesh flush".to_string());
        }

        let capture = if matches!(route, CaptureFlushRoute::CaptureThenCompletion) {
            let capture = capture_surface(context, target_surface)?;
            flush_surface(
                context,
                target_surface.surface_mut(),
                final_state.queue_family_index,
                Some(completion_semaphore),
                final_layout,
            )?;
            Some(capture)
        } else {
            None
        };
        target_surface.set_state(final_state);

        Ok((
            result,
            CompletedTarget {
                token: acquired.token,
                completion_semaphore,
                final_state,
                capture,
            },
        ))
    }

    fn validate_target<T>(
        &self,
        target_surface: &VulkanTargetSurface,
        acquired: &AcquiredTarget<T>,
    ) -> Result<(), String> {
        let validation_error = if target_surface.image() != acquired.image
            || target_surface.dimensions() != acquired.dimensions
        {
            Some("acquired Vulkan target does not match its wrapped Skia surface")
        } else if target_surface.state() != acquired.current_state {
            Some("acquired Vulkan target state does not match the shared engine state")
        } else {
            None
        };

        if let Some(error) = validation_error {
            // The signal operation can still be pending even on a pre-wait validation error.
            if let Some(acquire_semaphore) = acquired.acquire_semaphore {
                retire_rejected_acquire_semaphore(&self.device, acquire_semaphore);
            }
            Err(error.to_string())
        } else {
            Ok(())
        }
    }

    pub fn drop_scene_renderer(&mut self) {
        self.renderer.take();
    }

    pub fn shutdown_ganesh(&mut self) {
        if self.device.is_device_lost() {
            self.ganesh.abandon();
        } else {
            self.ganesh.shutdown();
        }
    }

    pub fn mark_device_lost(&mut self) {
        self.device.mark_device_lost();
        // Device loss is terminal. Abandon Ganesh before dropping context-associated scene/cache
        // wrappers; presenter-owned images and canonical leases remain quarantined by their owners.
        self.ganesh.abandon();
        self.drop_scene_renderer();
    }
}

impl Drop for VulkanEngine {
    fn drop(&mut self) {
        self.drop_scene_renderer();
        self.shutdown_ganesh();
    }
}

fn retire_rejected_acquire_semaphore(device: &VulkanDevice, semaphore: vk::Semaphore) {
    if device
        .wait_idle("rejected Vulkan acquire semaphore retirement")
        .is_ok()
    {
        unsafe { device.raw().destroy_semaphore(semaphore, None) };
    }
    // Device loss leaves the raw handle intentionally quarantined until logical-device teardown.
}

fn flush_surface(
    context: &mut gpu::DirectContext,
    surface: &mut skia_safe::Surface,
    queue_family_index: u32,
    signal_semaphore: Option<vk::Semaphore>,
    final_layout: sk_vk::ImageLayout,
) -> Result<RenderFlushTimings, String> {
    let started_at = Instant::now();
    let state = sk_vk::mutable_texture_states::new_vulkan(final_layout, queue_family_index);
    let mut info = gpu::FlushInfo::default();
    let mut backend_signal = signal_semaphore
        .map(|semaphore| unsafe { backend_semaphores::make_vk(raw::semaphore_to_skia(semaphore)) });
    if let Some(semaphore) = backend_signal.as_mut() {
        // SAFETY: the one-element storage remains mutably borrowed until the flush returns. The
        // Vulkan semaphore is presenter-owned and outlives queue presentation.
        unsafe { info.set_signal_semaphores(std::slice::from_mut(semaphore)) };
    }

    let flush_started_at = Instant::now();
    let submitted = context.flush_surface_with_texture_state(surface, &info, Some(&state));
    let gpu_flush = flush_started_at.elapsed();
    if signal_semaphore.is_some() && submitted != gpu::SemaphoresSubmitted::Yes {
        return Err("Skia failed to enqueue the Vulkan render-finished semaphore".to_string());
    }

    let submit_started_at = Instant::now();
    if !context.submit(gpu::SyncCpu::No) {
        return Err("Skia failed to submit Vulkan rendering work".to_string());
    }
    let submit = submit_started_at.elapsed();

    Ok(RenderFlushTimings {
        total: started_at.elapsed(),
        gpu_flush,
        submit,
    })
}

fn capture_surface(
    context: &mut gpu::DirectContext,
    target: &mut VulkanTargetSurface,
) -> Result<CapturedRgba, String> {
    let (width, height) = target.dimensions();
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| "Vulkan screenshot row size overflow".to_string())?;
    let pixel_len = row_bytes
        .checked_mul(usize::try_from(height).map_err(|_| "Vulkan screenshot height overflow")?)
        .ok_or_else(|| "Vulkan screenshot buffer size overflow".to_string())?;
    let mut pixels = vec![0_u8; pixel_len];
    let width_i32 = i32::try_from(width).map_err(|_| "Vulkan screenshot width exceeds i32")?;
    let height_i32 = i32::try_from(height).map_err(|_| "Vulkan screenshot height exceeds i32")?;
    let info = ImageInfo::new(
        (width_i32, height_i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );

    // The only synchronous Vulkan completion wait in the renderer. This path is reachable solely
    // for an explicit screenshot generation; ordinary frames never use SyncCpu::Yes.
    if !context.submit(gpu::SyncCpu::Yes) {
        return Err("failed to wait for Vulkan screenshot rendering".to_string());
    }
    if !target
        .surface_mut()
        .read_pixels(&info, &mut pixels, row_bytes, (0, 0))
    {
        return Err("failed to read Vulkan screenshot pixels".to_string());
    }
    Ok(CapturedRgba {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_semaphore_ownership_transfers_only_after_an_accepted_skia_wait() {
        assert_eq!(
            acquire_semaphore_owner_after_wait(AcquireSemaphoreOwner::Presenter, true),
            AcquireSemaphoreOwner::Skia
        );
        assert_eq!(
            acquire_semaphore_owner_after_wait(AcquireSemaphoreOwner::Presenter, false),
            AcquireSemaphoreOwner::Destroyed
        );
        assert_ne!(
            acquire_semaphore_owner_after_wait(AcquireSemaphoreOwner::Presenter, true),
            AcquireSemaphoreOwner::Presenter
        );
    }

    #[test]
    fn target_state_contract_can_describe_non_wsi_layout_and_ownership() {
        let state = TargetImageState {
            layout: vk::ImageLayout::GENERAL,
            queue_family_index: vk::QUEUE_FAMILY_EXTERNAL,
        };

        assert!(state.layout == vk::ImageLayout::GENERAL);
        assert_eq!(state.queue_family_index, vk::QUEUE_FAMILY_EXTERNAL);
    }

    #[test]
    fn ordinary_frames_have_no_capture_or_synchronous_wait_route() {
        assert_eq!(
            capture_flush_route(false),
            CaptureFlushRoute::DirectToCompletion
        );
        assert_eq!(
            capture_flush_route(true),
            CaptureFlushRoute::CaptureThenCompletion
        );
    }
}
