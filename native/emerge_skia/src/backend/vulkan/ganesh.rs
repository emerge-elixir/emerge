use std::sync::Arc;

use ash::vk;
use skia_safe::{
    ColorType, Surface,
    gpu::{self, SurfaceOrigin, backend_render_targets, direct_contexts, surfaces, vk as sk_vk},
};

use crate::renderer::text_surface_props;

use super::{capabilities::effective_api, device::VulkanDevice, frame::TargetImageState, raw};

pub struct GaneshContext {
    direct_context: Option<gpu::DirectContext>,
}

impl GaneshContext {
    pub fn new(device: &Arc<VulkanDevice>) -> Result<Self, String> {
        let instance = device.instance();
        let get_proc = |request| {
            // SAFETY: the BackendContext is built from these exact live ash owners. The closure is
            // used synchronously while Skia creates its context; Skia copies its interface table.
            unsafe { raw::resolve_proc(instance.entry(), instance.raw(), request) }
        };
        let instance_extensions = instance
            .enabled_extensions()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let device_extensions = device
            .enabled_extensions()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let api_version = effective_api(instance.api_version(), device.physical_device_api());
        // SAFETY: conversions are confined to the audited raw module and all ash handles remain
        // owned by `device`/`instance` for longer than this Ganesh context.
        let backend = unsafe {
            sk_vk::BackendContext::new_builder(
                raw::instance_to_skia(instance.raw().handle()),
                raw::physical_device_to_skia(device.physical_device()),
                raw::device_to_skia(device.raw().handle()),
                (
                    raw::queue_to_skia(device.queue()),
                    device.queue_family_index() as usize,
                ),
                &get_proc,
                Some(sk_vk::Version::new(
                    vk::api_version_major(api_version) as usize,
                    vk::api_version_minor(api_version) as usize,
                    vk::api_version_patch(api_version) as usize,
                )),
            )
            .with_extensions(&instance_extensions, &device_extensions)
            .build()
        };
        let direct_context = direct_contexts::make_vulkan(&backend, None)
            .ok_or_else(|| "failed to create Skia Ganesh Vulkan DirectContext".to_string())?;

        Ok(Self {
            direct_context: Some(direct_context),
        })
    }

    pub fn direct_context_mut(&mut self) -> Result<&mut gpu::DirectContext, String> {
        self.direct_context
            .as_mut()
            .ok_or_else(|| "Vulkan Ganesh context is shut down".to_string())
    }

    pub fn abandon(&mut self) {
        if let Some(mut context) = self.direct_context.take() {
            context.abandon();
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(mut context) = self.direct_context.take() {
            context.perform_deferred_cleanup(std::time::Duration::ZERO, None);
            context.free_gpu_resources();
            context.flush_and_submit();
            drop(context);
        }
    }
}

impl Drop for GaneshContext {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub const GANESH_TARGET_IMAGE_USAGE: vk::ImageUsageFlags = vk::ImageUsageFlags::from_raw(
    vk::ImageUsageFlags::COLOR_ATTACHMENT.as_raw()
        | vk::ImageUsageFlags::TRANSFER_SRC.as_raw()
        | vk::ImageUsageFlags::TRANSFER_DST.as_raw(),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VulkanTargetFormat {
    Bgra8888,
    Rgba8888,
}

impl VulkanTargetFormat {
    fn skia_format(self) -> sk_vk::Format {
        match self {
            Self::Bgra8888 => sk_vk::Format::B8G8R8A8_UNORM,
            Self::Rgba8888 => sk_vk::Format::R8G8B8A8_UNORM,
        }
    }

    fn color_type(self) -> ColorType {
        match self {
            Self::Bgra8888 => ColorType::BGRA8888,
            Self::Rgba8888 => ColorType::RGBA8888,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Bgra8888 => "B8G8R8A8_UNORM/BGRA8888",
            Self::Rgba8888 => "R8G8B8A8_UNORM/RGBA8888",
        }
    }
}

pub struct VulkanTargetSurface {
    image: vk::Image,
    dimensions: (u32, u32),
    state: TargetImageState,
    surface: Surface,
}

impl VulkanTargetSurface {
    pub fn new(
        context: &mut GaneshContext,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
    ) -> Result<Self, String> {
        Self::new_with_format(
            context,
            image,
            dimensions,
            initial_state,
            VulkanTargetFormat::Bgra8888,
        )
    }

    pub fn new_with_format(
        context: &mut GaneshContext,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
        format: VulkanTargetFormat,
    ) -> Result<Self, String> {
        Self::new_with_format_and_usage(
            context,
            image,
            dimensions,
            initial_state,
            format,
            GANESH_TARGET_IMAGE_USAGE,
        )
    }

    pub fn new_with_format_and_usage(
        context: &mut GaneshContext,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
        format: VulkanTargetFormat,
        usage: vk::ImageUsageFlags,
    ) -> Result<Self, String> {
        Self::new_with_format_usage_and_tiling(
            context,
            image,
            dimensions,
            initial_state,
            format,
            usage,
            sk_vk::ImageTiling::OPTIMAL,
        )
    }

    pub fn new_with_format_usage_and_tiling(
        context: &mut GaneshContext,
        image: vk::Image,
        dimensions: (u32, u32),
        initial_state: TargetImageState,
        format: VulkanTargetFormat,
        usage: vk::ImageUsageFlags,
        tiling: sk_vk::ImageTiling,
    ) -> Result<Self, String> {
        let width = i32::try_from(dimensions.0)
            .map_err(|_| "Vulkan target width exceeds i32".to_string())?;
        let height = i32::try_from(dimensions.1)
            .map_err(|_| "Vulkan target height exceeds i32".to_string())?;
        if width <= 0 || height <= 0 {
            return Err("Vulkan target dimensions must be non-zero".to_string());
        }

        // Presenter memory is implementation-owned, so Skia receives a borrowed image with an
        // empty allocation descriptor. The presenter keeps the image and its storage alive.
        let initial_layout = raw::image_layout_to_skia(initial_state.layout)?;
        let mut image_info = unsafe {
            sk_vk::ImageInfo::new(
                raw::image_to_skia(image),
                sk_vk::Alloc::default(),
                tiling,
                initial_layout,
                format.skia_format(),
                1,
                Some(initial_state.queue_family_index),
                None,
                Some(gpu::Protected::No),
                Some(sk_vk::SharingMode::EXCLUSIVE),
            )
        };
        // Ganesh rejects borrowed Vulkan images unless both transfer directions are declared,
        // even when the presenter only needs TRANSFER_SRC for on-demand capture.
        image_info.image_usage_flags = usage.as_raw();
        image_info.sample_count = 1;
        let target = backend_render_targets::make_vk((width, height), &image_info);
        let surface = surfaces::wrap_backend_render_target(
            context.direct_context_mut()?,
            &target,
            SurfaceOrigin::TopLeft,
            format.color_type(),
            None,
            Some(&text_surface_props()),
        )
        .ok_or_else(|| {
            format!(
                "failed to wrap borrowed Vulkan image as a Skia surface (dimensions={}x{}, format={}, usage=0x{:x}, tiling={:?}, layout={}, queue_family={}, sharing=EXCLUSIVE, samples=1, origin=TopLeft)",
                dimensions.0,
                dimensions.1,
                format.label(),
                usage.as_raw(),
                tiling,
                initial_state.layout.as_raw(),
                initial_state.queue_family_index,
            )
        })?;

        Ok(Self {
            image,
            dimensions,
            state: initial_state,
            surface,
        })
    }

    pub fn image(&self) -> vk::Image {
        self.image
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }

    pub fn state(&self) -> TargetImageState {
        self.state
    }

    pub fn set_state(&mut self, state: TargetImageState) {
        self.state = state;
    }

    pub fn surface_mut(&mut self) -> &mut Surface {
        &mut self.surface
    }
}
