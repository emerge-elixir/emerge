use std::sync::Arc;

use ash::{khr, vk};
use wayland_client::{Connection, protocol::wl_surface};

use crate::{
    backend::vulkan::{
        AcquiredTarget, CapturedRgba, CompletedTarget, DeviceRequirements,
        GANESH_TARGET_IMAGE_USAGE, TargetImageState, VulkanDevice, VulkanEngine, VulkanInstance,
        VulkanTargetSurface, capabilities::DrmNodeId,
    },
    renderer::{RenderFrame, RendererCacheConfig, SceneRenderer},
};

use super::handles::{raw_display_handle, raw_window_handle};

const SWAPCHAIN_FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;
const SWAPCHAIN_COLOR_SPACE: vk::ColorSpaceKHR = vk::ColorSpaceKHR::SRGB_NONLINEAR;
const ACQUIRE_TIMEOUT_NS: u64 = 0;
// Swapchain creation and the borrowed-image descriptor must expose the same usage contract.
const SWAPCHAIN_USAGE: vk::ImageUsageFlags = GANESH_TARGET_IMAGE_USAGE;

struct SurfaceOwner {
    _instance: Arc<VulkanInstance>,
    loader: khr::surface::Instance,
    handle: vk::SurfaceKHR,
}

impl Drop for SurfaceOwner {
    fn drop(&mut self) {
        // SAFETY: WaylandVulkanEnv explicitly drops swapchain/device children before this owner.
        unsafe { self.loader.destroy_surface(self.handle, None) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImageToken {
    generation: u64,
    image_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImageState {
    NeverPresented,
    Acquired,
    Rendered,
    Presented,
    Poisoned,
}

impl ImageState {
    fn acquire(&mut self) -> Result<vk::ImageLayout, String> {
        let layout = match self {
            Self::NeverPresented => vk::ImageLayout::UNDEFINED,
            Self::Presented => vk::ImageLayout::PRESENT_SRC_KHR,
            Self::Acquired | Self::Rendered => {
                return Err("Vulkan swapchain image was acquired while still in flight".to_string());
            }
            Self::Poisoned => return Err("Vulkan swapchain image state is poisoned".to_string()),
        };
        *self = Self::Acquired;
        Ok(layout)
    }

    fn rendered(&mut self) -> Result<(), String> {
        if !matches!(self, Self::Acquired) {
            return Err("Vulkan swapchain image rendered from an invalid state".to_string());
        }
        *self = Self::Rendered;
        Ok(())
    }

    fn presented(&mut self) -> Result<(), String> {
        if !matches!(self, Self::Rendered) {
            return Err("Vulkan swapchain image presented from an invalid state".to_string());
        }
        *self = Self::Presented;
        Ok(())
    }
}

struct SwapchainImage {
    target: Option<VulkanTargetSurface>,
    device: Arc<VulkanDevice>,
    image: vk::Image,
    render_finished: vk::Semaphore,
    state: ImageState,
}

impl Drop for SwapchainImage {
    fn drop(&mut self) {
        // Skia's borrowed surface must disappear before its presentation semaphore.
        self.target.take();
        unsafe {
            self.device
                .raw()
                .destroy_semaphore(self.render_finished, None)
        };
    }
}

struct PendingPresent {
    completed: CompletedTarget<ImageToken>,
}

pub(super) struct PresentOutcome {
    pub(super) submitted: bool,
    pub(super) capture: Option<CapturedRgba>,
}

pub(super) struct WaylandVulkanEnv {
    surface: Option<SurfaceOwner>,
    swapchain_loader: khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    images: Vec<SwapchainImage>,
    engine: Option<VulkanEngine>,
    dimensions: (u32, u32),
    generation: u64,
    pending_present: Option<PendingPresent>,
    recreate_before_acquire: bool,
}

impl WaylandVulkanEnv {
    pub(super) fn new(
        conn: &Connection,
        wl_surface: &wl_surface::WlSurface,
        dimensions: (u32, u32),
        renderer_cache_config: RendererCacheConfig,
        compositor_device: Option<DrmNodeId>,
    ) -> Result<Self, String> {
        let display_handle = raw_display_handle(conn)?;
        let window_handle = raw_window_handle(wl_surface)?;
        let required_instance_extensions =
            ash_window::enumerate_required_extensions(display_handle).map_err(|result| {
                format!("failed to enumerate Wayland Vulkan instance extensions: {result:?}")
            })?;
        let instance = VulkanInstance::new(required_instance_extensions)?;
        let surface_loader = khr::surface::Instance::new(instance.entry(), instance.raw());
        let surface_handle = unsafe {
            ash_window::create_surface(
                instance.entry(),
                instance.raw(),
                display_handle,
                window_handle,
                None,
            )
        }
        .map_err(|result| format!("failed to create Wayland Vulkan surface: {result:?}"))?;
        let surface = SurfaceOwner {
            _instance: Arc::clone(&instance),
            loader: surface_loader,
            handle: surface_handle,
        };
        let required_device_extensions = [
            khr::swapchain::NAME,
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::ext::image_drm_format_modifier::NAME,
            ash::khr::image_format_list::NAME,
            ash::khr::external_semaphore_fd::NAME,
            ash::khr::sampler_ycbcr_conversion::NAME,
            ash::ext::physical_device_drm::NAME,
        ];
        let device = VulkanDevice::new_for_surface(
            instance,
            DeviceRequirements {
                required_extensions: &required_device_extensions,
                require_timestamps: true,
                compositor_device,
            },
            |physical_device, queue_family| unsafe {
                surface
                    .loader
                    .get_physical_device_surface_support(
                        physical_device,
                        queue_family,
                        surface.handle,
                    )
                    .map_err(|result| {
                        format!("failed to query Wayland Vulkan presentation support: {result:?}")
                    })
            },
        )?;
        let swapchain_loader = khr::swapchain::Device::new(device.instance().raw(), device.raw());
        let engine = VulkanEngine::new(Arc::clone(&device), renderer_cache_config)?;
        let mut env = Self {
            surface: Some(surface),
            swapchain_loader,
            swapchain: vk::SwapchainKHR::null(),
            images: Vec::new(),
            engine: Some(engine),
            dimensions: clamp_dimensions(dimensions),
            generation: 0,
            pending_present: None,
            recreate_before_acquire: false,
        };
        env.recreate_swapchain(clamp_dimensions(dimensions), false)?;
        eprintln!(
            "Wayland Vulkan selected hardware device {}",
            device.physical_device_name()
        );
        Ok(env)
    }

    pub(super) fn supports_late_replacement(&self) -> bool {
        false
    }

    pub(super) fn renderer_mut(&mut self) -> Result<&mut SceneRenderer, String> {
        self.engine_mut()?.renderer_mut()
    }

    pub(super) fn device(&self) -> Result<&Arc<VulkanDevice>, String> {
        Ok(self.engine_ref()?.device())
    }

    pub(super) fn render_frame<R>(
        &mut self,
        capture_requested: bool,
        draw: impl FnOnce(&mut SceneRenderer, &mut RenderFrame<'_>) -> R,
    ) -> Result<Option<R>, String> {
        if self.pending_present.is_some() {
            return Err(
                "Vulkan frame rendered while another image awaits presentation".to_string(),
            );
        }
        if self.recreate_before_acquire {
            self.recreate_swapchain(self.dimensions, true)?;
        }
        let Some(acquired) = self.acquire_next_image()? else {
            return Ok(None);
        };
        let image_index = acquired.token.image_index;
        let engine = self
            .engine
            .as_mut()
            .ok_or_else(|| "Wayland Vulkan engine is shut down".to_string())?;
        let slot = self
            .images
            .get_mut(image_index)
            .ok_or_else(|| "Vulkan acquired image index is out of bounds".to_string())?;
        let target = slot
            .target
            .as_mut()
            .ok_or_else(|| "Vulkan acquired image has no Skia target".to_string())?;
        let (result, completed) = match engine.render(target, acquired, capture_requested, draw) {
            Ok(completed) => completed,
            Err(error) => {
                if engine.device().is_device_lost() {
                    engine.mark_device_lost();
                }
                return Err(error);
            }
        };
        slot.state.rendered()?;
        self.pending_present = Some(PendingPresent { completed });
        Ok(Some(result))
    }

    pub(super) fn present(&mut self) -> Result<PresentOutcome, String> {
        let pending = self
            .pending_present
            .take()
            .ok_or_else(|| "Vulkan present called without a rendered image".to_string())?;
        if pending.completed.token.generation != self.generation {
            return Err("Vulkan present token belongs to a stale swapchain generation".to_string());
        }
        let image_index = pending.completed.token.image_index;
        let present_state = wayland_image_state(self.engine_ref()?.device());
        if pending.completed.final_state != present_state {
            return Err(
                "completed Vulkan target is not in the Wayland presentation state".to_string(),
            );
        }
        let image_index_u32 = u32::try_from(image_index)
            .map_err(|_| "Vulkan present image index exceeds u32".to_string())?;
        let waits = [pending.completed.completion_semaphore];
        let swapchains = [self.swapchain];
        let image_indices = [image_index_u32];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&waits)
            .swapchains(&swapchains)
            .image_indices(&image_indices);
        let queue = self.engine_ref()?.device().queue();
        match unsafe { self.swapchain_loader.queue_present(queue, &present_info) } {
            Ok(suboptimal) => {
                self.images
                    .get_mut(image_index)
                    .ok_or_else(|| "Vulkan present image index is out of bounds".to_string())?
                    .state
                    .presented()?;
                self.recreate_before_acquire |= suboptimal;
                Ok(PresentOutcome {
                    submitted: true,
                    capture: pending.completed.capture,
                })
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                if let Some(image) = self.images.get_mut(image_index) {
                    image.state = ImageState::Poisoned;
                }
                self.recreate_before_acquire = true;
                Ok(PresentOutcome {
                    submitted: false,
                    capture: None,
                })
            }
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                self.mark_device_lost();
                Err("Vulkan device lost during Wayland presentation".to_string())
            }
            Err(result) => {
                if let Some(image) = self.images.get_mut(image_index) {
                    image.state = ImageState::Poisoned;
                }
                Err(format!("Wayland Vulkan queue present failed: {result:?}"))
            }
        }
    }

    pub(super) fn resize(&mut self, dimensions: (u32, u32)) -> Result<(), String> {
        let dimensions = clamp_dimensions(dimensions);
        if dimensions == self.dimensions && !self.recreate_before_acquire {
            return Ok(());
        }
        self.recreate_swapchain(dimensions, true)
    }

    fn acquire_next_image(&mut self) -> Result<Option<AcquiredTarget<ImageToken>>, String> {
        let device = Arc::clone(self.engine_ref()?.device());
        for attempt in 0..2 {
            let acquire_semaphore = create_semaphore(&device, "acquire")?;
            let result = unsafe {
                self.swapchain_loader.acquire_next_image(
                    self.swapchain,
                    ACQUIRE_TIMEOUT_NS,
                    acquire_semaphore,
                    vk::Fence::null(),
                )
            };
            match result {
                Ok((image_index, suboptimal)) => {
                    let image_index = match usize::try_from(image_index) {
                        Ok(image_index) => image_index,
                        Err(_) => {
                            unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                            return Err("Vulkan acquired image index exceeds usize".to_string());
                        }
                    };
                    let Some(slot) = self.images.get_mut(image_index) else {
                        unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                        return Err("Vulkan acquired image index is outside swapchain inventory"
                            .to_string());
                    };
                    let current_layout = match slot.state.acquire() {
                        Ok(layout) => layout,
                        Err(error) => {
                            unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                            return Err(error);
                        }
                    };
                    self.recreate_before_acquire |= suboptimal;
                    return Ok(Some(AcquiredTarget {
                        token: ImageToken {
                            generation: self.generation,
                            image_index,
                        },
                        image: slot.image,
                        dimensions: self.dimensions,
                        current_state: TargetImageState {
                            layout: current_layout,
                            queue_family_index: device.queue_family_index(),
                        },
                        acquire_semaphore: Some(acquire_semaphore),
                        completion_semaphore: slot.render_finished,
                        final_state: wayland_image_state(&device),
                    }));
                }
                Err(vk::Result::NOT_READY | vk::Result::TIMEOUT) => {
                    unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                    return Ok(None);
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                    if attempt == 0 {
                        self.recreate_swapchain(self.dimensions, true)?;
                        continue;
                    }
                    return Err(
                        "Wayland Vulkan swapchain remained out of date after recreation"
                            .to_string(),
                    );
                }
                Err(vk::Result::ERROR_DEVICE_LOST) => {
                    unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                    self.mark_device_lost();
                    return Err("Vulkan device lost while acquiring a Wayland image".to_string());
                }
                Err(result) => {
                    unsafe { device.raw().destroy_semaphore(acquire_semaphore, None) };
                    return Err(format!(
                        "failed to acquire Wayland Vulkan swapchain image: {result:?}"
                    ));
                }
            }
        }
        Err("Wayland Vulkan image acquisition exhausted its bounded retry".to_string())
    }

    fn recreate_swapchain(
        &mut self,
        requested_dimensions: (u32, u32),
        wait_for_old: bool,
    ) -> Result<(), String> {
        if self.pending_present.is_some() {
            return Err("cannot recreate Vulkan swapchain with a pending present".to_string());
        }
        if wait_for_old && self.swapchain != vk::SwapchainKHR::null() {
            self.engine_ref()?
                .device()
                .wait_idle("Wayland swapchain recreation")?;
        }
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| "Wayland Vulkan surface is shut down".to_string())?;
        let device = Arc::clone(self.engine_ref()?.device());
        let capabilities = unsafe {
            surface
                .loader
                .get_physical_device_surface_capabilities(device.physical_device(), surface.handle)
        }
        .map_err(|result| format!("failed to query Vulkan surface capabilities: {result:?}"))?;
        let formats = unsafe {
            surface
                .loader
                .get_physical_device_surface_formats(device.physical_device(), surface.handle)
        }
        .map_err(|result| format!("failed to query Vulkan surface formats: {result:?}"))?;
        let present_modes = unsafe {
            surface
                .loader
                .get_physical_device_surface_present_modes(device.physical_device(), surface.handle)
        }
        .map_err(|result| format!("failed to query Vulkan present modes: {result:?}"))?;
        let choice =
            choose_swapchain(capabilities, &formats, &present_modes, requested_dimensions)?;
        let old_swapchain = self.swapchain;
        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface.handle)
            .min_image_count(choice.image_count)
            .image_format(choice.format.format)
            .image_color_space(choice.format.color_space)
            .image_extent(choice.extent)
            .image_array_layers(1)
            .image_usage(SWAPCHAIN_USAGE)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(choice.composite_alpha)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true)
            .old_swapchain(old_swapchain);
        let new_swapchain = unsafe { self.swapchain_loader.create_swapchain(&create_info, None) }
            .map_err(|result| {
            format!("failed to create Wayland Vulkan swapchain: {result:?}")
        })?;
        let raw_images = match unsafe { self.swapchain_loader.get_swapchain_images(new_swapchain) }
        {
            Ok(images) => images,
            Err(result) => {
                unsafe { self.swapchain_loader.destroy_swapchain(new_swapchain, None) };
                return Err(format!(
                    "failed to enumerate Wayland Vulkan swapchain images: {result:?}"
                ));
            }
        };
        if raw_images.is_empty() {
            unsafe { self.swapchain_loader.destroy_swapchain(new_swapchain, None) };
            return Err("Wayland Vulkan swapchain has no images".to_string());
        }
        let dimensions = (choice.extent.width, choice.extent.height);
        let new_images = {
            let engine = self
                .engine
                .as_mut()
                .ok_or_else(|| "Wayland Vulkan engine is shut down".to_string())?;
            raw_images
                .iter()
                .copied()
                .map(|image| {
                    let render_finished = create_semaphore(engine.device(), "render-finished")?;
                    let initial_state = TargetImageState {
                        layout: vk::ImageLayout::UNDEFINED,
                        queue_family_index: engine.device().queue_family_index(),
                    };
                    match engine.create_target_surface(image, dimensions, initial_state) {
                        Ok(target) => Ok(SwapchainImage {
                            target: Some(target),
                            device: Arc::clone(engine.device()),
                            image,
                            render_finished,
                            state: ImageState::NeverPresented,
                        }),
                        Err(error) => {
                            unsafe {
                                engine
                                    .device()
                                    .raw()
                                    .destroy_semaphore(render_finished, None)
                            };
                            Err(error)
                        }
                    }
                })
                .collect::<Result<Vec<_>, String>>()
        };
        let new_images = match new_images {
            Ok(images) => images,
            Err(error) => {
                unsafe { self.swapchain_loader.destroy_swapchain(new_swapchain, None) };
                return Err(error);
            }
        };

        // Device idle above makes it safe to retire all old wrapped surfaces and presentation
        // semaphores. Normal frame operation never waits the queue or device idle.
        self.images.clear();
        if old_swapchain != vk::SwapchainKHR::null() {
            unsafe { self.swapchain_loader.destroy_swapchain(old_swapchain, None) };
        }
        self.swapchain = new_swapchain;
        self.images = new_images;
        self.dimensions = dimensions;
        self.generation = self.generation.wrapping_add(1);
        self.recreate_before_acquire = false;
        Ok(())
    }

    fn engine_ref(&self) -> Result<&VulkanEngine, String> {
        self.engine
            .as_ref()
            .ok_or_else(|| "Wayland Vulkan engine is shut down".to_string())
    }

    fn engine_mut(&mut self) -> Result<&mut VulkanEngine, String> {
        self.engine
            .as_mut()
            .ok_or_else(|| "Wayland Vulkan engine is shut down".to_string())
    }

    pub(super) fn mark_device_lost(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            engine.mark_device_lost();
        }
    }
}

impl Drop for WaylandVulkanEnv {
    fn drop(&mut self) {
        self.pending_present.take();
        if let Some(engine) = self.engine.as_mut() {
            let shutdown = if engine.device().is_device_lost() {
                Err("Wayland Vulkan device was already lost at shutdown".to_string())
            } else {
                (|| {
                    engine
                        .device()
                        .wait_idle("Wayland Vulkan pre-video shutdown")?;
                    engine.renderer_mut()?.prepare_vulkan_video_shutdown()?;
                    engine
                        .device()
                        .wait_idle("Wayland Vulkan imported-image release shutdown")
                })()
            };
            match shutdown {
                Ok(()) => {
                    // Scene caches can retain SkImages. Current Video images have already been
                    // released to external ownership and all release fences are complete.
                    engine.drop_scene_renderer();
                }
                Err(error) => {
                    eprintln!("{error}");
                    engine.mark_device_lost();
                }
            }
        }
        self.images.clear();
        if let Some(engine) = self.engine.as_mut() {
            engine.shutdown_ganesh();
        }
        if self.swapchain != vk::SwapchainKHR::null() {
            unsafe {
                self.swapchain_loader
                    .destroy_swapchain(self.swapchain, None)
            };
            self.swapchain = vk::SwapchainKHR::null();
        }
        // Drops the logical device while SurfaceOwner still keeps the instance and Wayland handles
        // alive. VkSurfaceKHR is destroyed next; the shared instance is last.
        self.engine.take();
        self.surface.take();
    }
}

fn wayland_image_state(device: &VulkanDevice) -> TargetImageState {
    TargetImageState {
        layout: vk::ImageLayout::PRESENT_SRC_KHR,
        queue_family_index: device.queue_family_index(),
    }
}

fn create_semaphore(device: &VulkanDevice, label: &str) -> Result<vk::Semaphore, String> {
    unsafe {
        device
            .raw()
            .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
    }
    .map_err(|result| format!("failed to create Vulkan {label} semaphore: {result:?}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SwapchainChoice {
    format: vk::SurfaceFormatKHR,
    extent: vk::Extent2D,
    image_count: u32,
    composite_alpha: vk::CompositeAlphaFlagsKHR,
}

impl std::fmt::Debug for SwapchainChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SwapchainChoice")
            .field("format", &self.format.format.as_raw())
            .field("color_space", &self.format.color_space.as_raw())
            .field("width", &self.extent.width)
            .field("height", &self.extent.height)
            .field("image_count", &self.image_count)
            .field("composite_alpha", &self.composite_alpha.as_raw())
            .finish()
    }
}

fn choose_swapchain(
    capabilities: vk::SurfaceCapabilitiesKHR,
    formats: &[vk::SurfaceFormatKHR],
    present_modes: &[vk::PresentModeKHR],
    requested_dimensions: (u32, u32),
) -> Result<SwapchainChoice, String> {
    if !present_modes.contains(&vk::PresentModeKHR::FIFO) {
        return Err(
            "Wayland Vulkan surface does not support required FIFO presentation".to_string(),
        );
    }
    if !capabilities.supported_usage_flags.contains(SWAPCHAIN_USAGE) {
        return Err(
            "Wayland Vulkan surface lacks COLOR_ATTACHMENT|TRANSFER_SRC|TRANSFER_DST image usage"
                .to_string(),
        );
    }
    let format = formats
        .iter()
        .copied()
        .find(|format| {
            format.format == SWAPCHAIN_FORMAT && format.color_space == SWAPCHAIN_COLOR_SPACE
        })
        .or_else(|| {
            (formats.len() == 1 && formats[0].format == vk::Format::UNDEFINED).then_some(
                vk::SurfaceFormatKHR {
                    format: SWAPCHAIN_FORMAT,
                    color_space: SWAPCHAIN_COLOR_SPACE,
                },
            )
        })
        .ok_or_else(|| {
            "Wayland Vulkan surface lacks B8G8R8A8_UNORM/SRGB_NONLINEAR format".to_string()
        })?;
    let extent = if capabilities.current_extent.width != u32::MAX {
        capabilities.current_extent
    } else {
        vk::Extent2D {
            width: requested_dimensions
                .0
                .max(capabilities.min_image_extent.width)
                .min(capabilities.max_image_extent.width),
            height: requested_dimensions
                .1
                .max(capabilities.min_image_extent.height)
                .min(capabilities.max_image_extent.height),
        }
    };
    if extent.width == 0 || extent.height == 0 {
        return Err("Wayland Vulkan swapchain extent is zero".to_string());
    }
    let preferred_count = capabilities.min_image_count.saturating_add(1);
    let image_count = if capabilities.max_image_count == 0 {
        preferred_count
    } else {
        preferred_count.min(capabilities.max_image_count)
    };
    let composite_alpha = [
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ]
    .into_iter()
    .find(|mode| capabilities.supported_composite_alpha.contains(*mode))
    .ok_or_else(|| "Wayland Vulkan surface has no supported composite alpha mode".to_string())?;

    Ok(SwapchainChoice {
        format,
        extent,
        image_count,
        composite_alpha,
    })
}

fn clamp_dimensions(dimensions: (u32, u32)) -> (u32, u32) {
    (dimensions.0.max(1), dimensions.1.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> vk::SurfaceCapabilitiesKHR {
        vk::SurfaceCapabilitiesKHR {
            min_image_count: 2,
            max_image_count: 3,
            current_extent: vk::Extent2D {
                width: u32::MAX,
                height: u32::MAX,
            },
            min_image_extent: vk::Extent2D {
                width: 16,
                height: 16,
            },
            max_image_extent: vk::Extent2D {
                width: 1920,
                height: 1080,
            },
            supported_usage_flags: SWAPCHAIN_USAGE,
            supported_composite_alpha: vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            ..Default::default()
        }
    }

    #[test]
    fn swapchain_choice_requires_exact_format_fifo_and_capture_usage() {
        let formats = [vk::SurfaceFormatKHR {
            format: SWAPCHAIN_FORMAT,
            color_space: SWAPCHAIN_COLOR_SPACE,
        }];
        let choice = choose_swapchain(
            capabilities(),
            &formats,
            &[vk::PresentModeKHR::FIFO],
            (2560, 4),
        )
        .expect("compatible surface");
        assert_eq!(choice.extent.width, 1920);
        assert_eq!(choice.extent.height, 16);
        assert_eq!(choice.image_count, 3);
        assert!(choice.composite_alpha == vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED);

        let mut missing_capture = capabilities();
        missing_capture.supported_usage_flags = vk::ImageUsageFlags::COLOR_ATTACHMENT;
        assert!(
            choose_swapchain(
                missing_capture,
                &formats,
                &[vk::PresentModeKHR::FIFO],
                (100, 100),
            )
            .expect_err("both Vulkan transfer directions are required")
            .contains("TRANSFER_DST")
        );
        assert!(
            choose_swapchain(
                capabilities(),
                &formats,
                &[vk::PresentModeKHR::MAILBOX],
                (100, 100),
            )
            .expect_err("FIFO is required")
            .contains("FIFO")
        );
    }

    #[test]
    fn swapchain_image_reuse_is_tied_to_successful_present_then_reacquire() {
        let mut state = ImageState::NeverPresented;
        assert!(state.acquire() == Ok(vk::ImageLayout::UNDEFINED));
        assert!(state.acquire().is_err());
        state.rendered().expect("rendered");
        state.presented().expect("presented");
        assert!(state.acquire() == Ok(vk::ImageLayout::PRESENT_SRC_KHR));
    }

    #[test]
    fn ordinary_acquisition_never_waits_the_compositor_thread() {
        assert_eq!(ACQUIRE_TIMEOUT_NS, 0);
    }

    #[test]
    fn poisoned_out_of_date_image_cannot_be_reused_before_recreation() {
        let mut state = ImageState::Presented;
        state.acquire().expect("acquired");
        state.rendered().expect("rendered");
        state = ImageState::Poisoned;
        assert!(state.acquire().is_err());
        assert_eq!(clamp_dimensions((0, 0)), (1, 1));
    }
}
