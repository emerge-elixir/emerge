use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::Arc,
};

use ash::vk;
use video_interop::dmabuf_allocation_size;

use super::{GANESH_TARGET_IMAGE_USAGE, VulkanDevice};

pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;
pub const PRIME_RGBA_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
pub const EXPORTED_PRIME_IMAGE_USAGE: vk::ImageUsageFlags = vk::ImageUsageFlags::from_raw(
    GANESH_TARGET_IMAGE_USAGE.as_raw() | vk::ImageUsageFlags::SAMPLED.as_raw(),
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportedPlane {
    pub offset: u64,
    pub pitch: u32,
}

/// One Vulkan-owned, DMA-BUF-exportable image allocation. The caller must drop every borrowed
/// Ganesh surface wrapping `image()` before this owner is dropped.
pub struct ExportedDmaBufImage {
    device: Arc<VulkanDevice>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    fd: OwnedFd,
    fd_allocation_size: u64,
    modifier: u64,
    plane: ExportedPlane,
    dimensions: (u32, u32),
}

impl ExportedDmaBufImage {
    pub fn new_linear_rgba(
        device: Arc<VulkanDevice>,
        dimensions: (u32, u32),
    ) -> Result<Self, String> {
        if dimensions.0 == 0 || dimensions.1 == 0 {
            return Err("Vulkan PRIME dimensions must be non-zero".to_string());
        }
        validate_linear_rgba_export(&device)?;

        let modifiers = [DRM_FORMAT_MOD_LINEAR];
        let mut modifier_info =
            vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(PRIME_RGBA_FORMAT)
            .extent(vk::Extent3D {
                width: dimensions.0,
                height: dimensions.1,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(EXPORTED_PRIME_IMAGE_USAGE)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut modifier_info)
            .push_next(&mut external_info);

        // SAFETY: all create-info storage remains valid for the call. The returned image is
        // destroyed by this owner on every success and error path.
        let image = unsafe { device.raw().create_image(&create_info, None) }
            .map_err(|result| format!("failed to create linear Vulkan PRIME image: {result:?}"))?;
        let mut memory = vk::DeviceMemory::null();

        let result = (|| {
            let mut dedicated_requirements = vk::MemoryDedicatedRequirements::default();
            let mut requirements2 =
                vk::MemoryRequirements2::default().push_next(&mut dedicated_requirements);
            let image_requirements = vk::ImageMemoryRequirementsInfo2::default().image(image);
            // SAFETY: `image` is live and belongs to this exact device.
            unsafe {
                device
                    .raw()
                    .get_image_memory_requirements2(&image_requirements, &mut requirements2)
            };
            let requirements = requirements2.memory_requirements;
            let memory_type_index = select_memory_type(&device, requirements.memory_type_bits)?;

            let mut export_info = vk::ExportMemoryAllocateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            let mut dedicated_info = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let allocation_info = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type_index)
                .push_next(&mut export_info)
                .push_next(&mut dedicated_info);
            // SAFETY: the selected memory type is advertised for this image and the pNext chain
            // requests a dedicated exportable allocation owned by this exact device.
            memory = unsafe { device.raw().allocate_memory(&allocation_info, None) }.map_err(
                |result| format!("failed to allocate Vulkan PRIME image memory: {result:?}"),
            )?;
            let bind = vk::BindImageMemoryInfo::default()
                .image(image)
                .memory(memory)
                .memory_offset(0);
            // SAFETY: image and memory are live, compatible, and dedicated to one another.
            unsafe { device.raw().bind_image_memory2(&[bind]) }.map_err(|result| {
                format!("failed to bind Vulkan PRIME image memory: {result:?}")
            })?;

            let modifier_loader = ash::ext::image_drm_format_modifier::Device::new(
                device.instance().raw(),
                device.raw(),
            );
            let mut modifier_properties = vk::ImageDrmFormatModifierPropertiesEXT::default();
            // SAFETY: the extension was required at logical-device creation and `image` is live.
            unsafe {
                modifier_loader
                    .get_image_drm_format_modifier_properties(image, &mut modifier_properties)
            }
            .map_err(|result| format!("failed to query Vulkan PRIME image modifier: {result:?}"))?;
            if modifier_properties.drm_format_modifier != DRM_FORMAT_MOD_LINEAR {
                return Err(format!(
                    "Vulkan PRIME image selected non-linear modifier {:#018x}",
                    modifier_properties.drm_format_modifier
                ));
            }

            // DRM-modifier images permit vkGetImageSubresourceLayout for one memory plane.
            let layout = unsafe {
                device.raw().get_image_subresource_layout(
                    image,
                    vk::ImageSubresource::default()
                        .aspect_mask(vk::ImageAspectFlags::MEMORY_PLANE_0_EXT)
                        .mip_level(0)
                        .array_layer(0),
                )
            };
            let pitch = u32::try_from(layout.row_pitch)
                .map_err(|_| "Vulkan PRIME row pitch exceeds u32".to_string())?;
            if pitch == 0 {
                return Err("Vulkan PRIME image reported a zero row pitch".to_string());
            }

            let memory_fd_loader =
                ash::khr::external_memory_fd::Device::new(device.instance().raw(), device.raw());
            let fd_info = vk::MemoryGetFdInfoKHR::default()
                .memory(memory)
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
            // SAFETY: this exports one new owned descriptor for the live exportable allocation.
            let raw_fd = unsafe { memory_fd_loader.get_memory_fd(&fd_info) }.map_err(|result| {
                format!("failed to export Vulkan PRIME DMA-BUF fd: {result:?}")
            })?;
            if raw_fd < 0 {
                return Err("Vulkan returned an invalid PRIME DMA-BUF fd".to_string());
            }
            // SAFETY: successful vkGetMemoryFdKHR transfers one new owned descriptor to us.
            let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
            let fd_allocation_size = dmabuf_allocation_size(fd.as_raw_fd()).map_err(|error| {
                format!("failed to query exported Vulkan PRIME DMA-BUF allocation size: {error}")
            })?;
            validate_exported_allocation_size(requirements.size, fd_allocation_size)?;

            Ok(Self {
                device: Arc::clone(&device),
                image,
                memory,
                fd,
                fd_allocation_size,
                modifier: modifier_properties.drm_format_modifier,
                plane: ExportedPlane {
                    offset: layout.offset,
                    pitch,
                },
                dimensions,
            })
        })();

        if result.is_err() {
            // SAFETY: no borrowed surface exists yet. Destroy the image before its bound memory.
            unsafe {
                device.raw().destroy_image(image, None);
                if memory != vk::DeviceMemory::null() {
                    device.raw().free_memory(memory, None);
                }
            }
        }
        result
    }

    pub fn image(&self) -> vk::Image {
        self.image
    }

    pub fn fd(&self) -> i32 {
        self.fd.as_raw_fd()
    }

    pub fn fd_allocation_size(&self) -> u64 {
        self.fd_allocation_size
    }

    pub fn modifier(&self) -> u64 {
        self.modifier
    }

    pub fn plane(&self) -> ExportedPlane {
        self.plane
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }
}

impl Drop for ExportedDmaBufImage {
    fn drop(&mut self) {
        // SAFETY: callers drop borrowed Ganesh surfaces before this allocation owner. Vulkan
        // requires image destruction before freeing its bound memory.
        unsafe {
            self.device.raw().destroy_image(self.image, None);
            self.device.raw().free_memory(self.memory, None);
        }
    }
}

fn validate_linear_rgba_export(device: &Arc<VulkanDevice>) -> Result<(), String> {
    let mut count = vk::DrmFormatModifierPropertiesListEXT::default();
    let mut properties = vk::FormatProperties2::default().push_next(&mut count);
    // SAFETY: output storage and pNext chain remain valid for the call.
    unsafe {
        device
            .instance()
            .raw()
            .get_physical_device_format_properties2(
                device.physical_device(),
                PRIME_RGBA_FORMAT,
                &mut properties,
            )
    };
    let mut modifiers = vec![
        vk::DrmFormatModifierPropertiesEXT::default();
        usize::try_from(count.drm_format_modifier_count)
            .map_err(|_| "Vulkan modifier count exceeds usize".to_string())?
    ];
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
        .drm_format_modifier_properties(&mut modifiers);
    let mut properties = vk::FormatProperties2::default().push_next(&mut list);
    // SAFETY: the allocated modifier slice remains valid for the call.
    unsafe {
        device
            .instance()
            .raw()
            .get_physical_device_format_properties2(
                device.physical_device(),
                PRIME_RGBA_FORMAT,
                &mut properties,
            )
    };
    let required_features = vk::FormatFeatureFlags::COLOR_ATTACHMENT
        | vk::FormatFeatureFlags::TRANSFER_SRC
        | vk::FormatFeatureFlags::TRANSFER_DST
        | vk::FormatFeatureFlags::SAMPLED_IMAGE;
    let modifier = modifiers.iter().find(|modifier| {
        modifier.drm_format_modifier == DRM_FORMAT_MOD_LINEAR
            && modifier.drm_format_modifier_plane_count == 1
            && modifier
                .drm_format_modifier_tiling_features
                .contains(required_features)
    });
    if modifier.is_none() {
        return Err(
            "Vulkan device cannot export one-plane linear R8G8B8A8 PRIME render targets"
                .to_string(),
        );
    }

    let mut modifier_info = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
        .drm_format_modifier(DRM_FORMAT_MOD_LINEAR)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let mut external_info = vk::PhysicalDeviceExternalImageFormatInfo::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let format_info = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(PRIME_RGBA_FORMAT)
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(EXPORTED_PRIME_IMAGE_USAGE)
        .flags(vk::ImageCreateFlags::empty())
        .push_next(&mut modifier_info)
        .push_next(&mut external_info);
    let mut external_properties = vk::ExternalImageFormatProperties::default();
    let mut image_properties =
        vk::ImageFormatProperties2::default().push_next(&mut external_properties);
    // SAFETY: all input and output pNext storage remains valid for the query.
    unsafe {
        device
            .instance()
            .raw()
            .get_physical_device_image_format_properties2(
                device.physical_device(),
                &format_info,
                &mut image_properties,
            )
    }
    .map_err(|result| format!("linear R8G8B8A8 DMA-BUF image format is unsupported: {result:?}"))?;
    let external = external_properties.external_memory_properties;
    if !external
        .external_memory_features
        .contains(vk::ExternalMemoryFeatureFlags::EXPORTABLE)
        || !external
            .compatible_handle_types
            .contains(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
    {
        return Err("linear R8G8B8A8 Vulkan images are not DMA-BUF exportable".to_string());
    }
    Ok(())
}

fn validate_exported_allocation_size(
    vulkan_requirement: u64,
    fd_allocation_size: u64,
) -> Result<(), String> {
    if fd_allocation_size < vulkan_requirement {
        return Err(format!(
            "exported Vulkan PRIME DMA-BUF allocation size {fd_allocation_size} is smaller than Vulkan image requirement {vulkan_requirement}"
        ));
    }
    Ok(())
}

fn select_memory_type(device: &Arc<VulkanDevice>, bits: u32) -> Result<u32, String> {
    let properties = unsafe {
        device
            .instance()
            .raw()
            .get_physical_device_memory_properties(device.physical_device())
    };
    let candidates = properties.memory_types
        [..usize::try_from(properties.memory_type_count).unwrap_or(0)]
        .iter()
        .enumerate()
        .filter(|(index, _memory_type)| bits & (1_u32 << index) != 0)
        .collect::<Vec<_>>();
    candidates
        .iter()
        .find(|(_index, memory_type)| {
            memory_type
                .property_flags
                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .or_else(|| candidates.first())
        .map(|(index, _memory_type)| *index as u32)
        .ok_or_else(|| "Vulkan PRIME image has no compatible memory type".to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_exported_allocation_size;

    #[test]
    fn exported_fd_allocation_may_include_alignment_tail() {
        validate_exported_allocation_size(1_075_200, 1_075_200)
            .expect("an exact fd-backed allocation is valid");
        validate_exported_allocation_size(1_075_200, 1_077_248)
            .expect("an fd-backed alignment tail is valid");
    }

    #[test]
    fn exported_fd_allocation_cannot_be_shorter_than_vulkan_requirement() {
        assert!(validate_exported_allocation_size(1_075_200, 0).is_err());
        assert!(validate_exported_allocation_size(1_075_200, 1_075_199).is_err());
    }
}
