use std::sync::Arc;

use ash::vk;
use skia_safe::gpu::{self, Protected, vk as sk_vk};

use super::{VulkanDevice, device::VulkanDeviceIdentity, raw};

pub use video_interop::vulkan::{
    ImportedPlane, Nv12AllocationBindingRecipe, Nv12Conversion, Nv12FrameTopology,
    Nv12ImportStrategy, Nv12Plane, Nv12SharedObjectLayout, Nv12StagingPreference,
    PackedImageFormat, PackedImageImport, PackedImageImportStrategy, SampledImageFormat,
    StagedNv12Planes, VulkanDmaBufImporter, VulkanImportPoolLimits, YcbcrModel, YcbcrOffset,
    YcbcrRange, map_nv12_colorimetry, validate_nv12_shared_object_topology,
};

pub type InteropVulkanDmaBufImporter = VulkanDmaBufImporter<VulkanDevice>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Nv12TargetAllocationProof {
    pub device_identity: VulkanDeviceIdentity,
    pub topology: Nv12FrameTopology,
    pub recipe: Nv12AllocationBindingRecipe,
}

/// Immutable import capability observed from the already-selected active device. The generic
/// Vulkan adapter owns the actual format/modifier facts; Emerge adds exact renderer identity and
/// runtime allocation attestation without taking format-import responsibility back from it.
#[derive(Clone, Copy)]
pub struct Nv12ModifierCapability {
    pub modifier: u64,
    pub active_device_identity: VulkanDeviceIdentity,
    pub target_allocation_proof: Option<Nv12TargetAllocationProof>,
    interop: video_interop::vulkan::Nv12ModifierCapability,
}

impl Nv12ModifierCapability {
    pub fn from_interop(
        active_device_identity: VulkanDeviceIdentity,
        interop: video_interop::vulkan::Nv12ModifierCapability,
    ) -> Self {
        Self {
            modifier: interop.modifier,
            active_device_identity,
            target_allocation_proof: None,
            interop,
        }
    }

    pub fn import_strategy(self) -> Nv12ImportStrategy {
        self.interop.strategy
    }

    pub fn modifier_plane_count(self) -> u32 {
        self.interop.modifier_plane_count
    }

    pub fn allocation_recipe(self) -> Nv12AllocationBindingRecipe {
        self.interop.allocation_recipe()
    }

    #[cfg_attr(
        not(any(feature = "wayland-core", feature = "drm-core")),
        allow(dead_code)
    )]
    pub(crate) fn interop(self) -> video_interop::vulkan::Nv12ModifierCapability {
        self.interop
    }
}

pub fn inventory_nv12_modifier_capabilities(
    device: &Arc<VulkanDevice>,
) -> Result<Vec<Nv12ModifierCapability>, String> {
    video_interop::vulkan::inventory_nv12_modifier_capabilities(device.as_ref()).map(
        |capabilities| {
            capabilities
                .into_iter()
                .map(|capability| {
                    Nv12ModifierCapability::from_interop(device.identity(), capability)
                })
                .collect()
        },
    )
}

pub fn capabilities_for_importer(
    device: &VulkanDevice,
    importer: &InteropVulkanDmaBufImporter,
) -> Vec<Nv12ModifierCapability> {
    importer
        .nv12_capabilities()
        .iter()
        .copied()
        .map(|capability| Nv12ModifierCapability::from_interop(device.identity(), capability))
        .collect()
}

pub fn query_nv12_modifier_capability(
    device: &Arc<VulkanDevice>,
    modifier: u64,
) -> Result<Nv12ModifierCapability, String> {
    inventory_nv12_modifier_capabilities(device)?
        .into_iter()
        .find(|capability| capability.modifier == modifier)
        .ok_or_else(|| {
            format!("Vulkan device has no usable NV12 import path for modifier {modifier:#018x}")
        })
}

pub fn validate_nv12_modifier_capability(
    capability: Nv12ModifierCapability,
    dimensions: (u32, u32),
    conversion: Nv12Conversion,
) -> Result<(), String> {
    video_interop::vulkan::validate_nv12_modifier_capability(
        capability.interop,
        dimensions,
        conversion,
    )
}

pub fn validate_nv12_allocation_proof(
    active_device_identity: VulkanDeviceIdentity,
    proof: Nv12TargetAllocationProof,
    topology: Nv12FrameTopology,
    recipe: Nv12AllocationBindingRecipe,
) -> Result<(), String> {
    if proof.device_identity != active_device_identity {
        return Err(
            "Vulkan NV12 target proof does not match the active DRM/device/driver identity"
                .to_string(),
        );
    }
    if proof.recipe != recipe {
        return Err(
            "Vulkan NV12 target proof does not match the allocation/binding recipe".to_string(),
        );
    }
    if proof.topology != topology {
        return Err(format!(
            "Vulkan NV12 frame topology does not exactly match the target proof: expected {:?}, got {:?}",
            proof.topology, topology
        ));
    }
    Ok(())
}

pub fn validate_nv12_target_allocation_proof(
    capability: Nv12ModifierCapability,
    topology: Nv12FrameTopology,
) -> Result<(), String> {
    let proof = capability.target_allocation_proof.ok_or_else(|| {
        format!(
            "Vulkan NV12 modifier {:#018x} has no target allocation proof",
            capability.modifier
        )
    })?;
    validate_nv12_allocation_proof(
        capability.active_device_identity,
        proof,
        topology,
        capability.allocation_recipe(),
    )
}

/// Renderer-side wrapper around the framework-neutral Vulkan import owner. Emerge adds only Skia
/// backend-texture construction; DMA-BUF FDs, image memory, conversion resources, and import
/// synchronization remain owned by `video-interop`.
pub struct ImportedDmaBufImage {
    interop: video_interop::vulkan::ImportedDmaBufImage<VulkanDevice>,
}

pub struct StagedNv12BackendTextures {
    pub luma: gpu::BackendTexture,
    pub chroma: gpu::BackendTexture,
    pub conversion: Nv12Conversion,
}

impl ImportedDmaBufImage {
    pub fn new_bgra_scanout(
        device: Arc<VulkanDevice>,
        dimensions: (u32, u32),
        source_fd: i32,
        source_size: u64,
        modifier: u64,
        plane: ImportedPlane,
    ) -> Result<Self, String> {
        video_interop::vulkan::ImportedDmaBufImage::new_bgra_scanout(
            device,
            dimensions,
            source_fd,
            source_size,
            modifier,
            plane,
            super::ganesh::GANESH_TARGET_IMAGE_USAGE,
        )
        .map(Self::from_interop)
    }

    pub fn from_interop(interop: video_interop::vulkan::ImportedDmaBufImage<VulkanDevice>) -> Self {
        Self { interop }
    }

    pub fn interop(&self) -> &video_interop::vulkan::ImportedDmaBufImage<VulkanDevice> {
        &self.interop
    }

    pub fn image(&self) -> vk::Image {
        self.interop.image()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.interop.dimensions()
    }

    pub fn modifier(&self) -> u64 {
        self.interop.modifier()
    }

    pub fn make_backend_texture(&self, label: &str) -> Result<gpu::BackendTexture, String> {
        let dimensions = self.interop.dimensions();
        let width = i32::try_from(dimensions.0)
            .map_err(|_| "Vulkan imported image width exceeds i32".to_string())?;
        let height = i32::try_from(dimensions.1)
            .map_err(|_| "Vulkan imported image height exceeds i32".to_string())?;
        let (format, ycbcr) = match self.interop.sampled_format() {
            SampledImageFormat::Rgba8888 => (sk_vk::Format::R8G8B8A8_UNORM, None),
            SampledImageFormat::Bgra8888 => (sk_vk::Format::B8G8R8A8_UNORM, None),
            SampledImageFormat::Nv12 => {
                let sampling = self.interop.nv12_sampling().ok_or_else(|| {
                    "Vulkan NV12 image is missing sampler-YCbCr metadata".to_string()
                })?;
                (
                    sk_vk::Format::G8_B8R8_2PLANE_420_UNORM,
                    Some(skia_ycbcr_info(
                        sampling.conversion,
                        sampling.format_features,
                    )),
                )
            }
            SampledImageFormat::Nv12Planes => {
                return Err(
                    "staged Vulkan NV12 planes require the Emerge runtime YUV shader".to_string(),
                );
            }
        };
        let tiling = match self.interop.sampled_tiling() {
            vk::ImageTiling::OPTIMAL => sk_vk::ImageTiling::OPTIMAL,
            vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT => sk_vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT,
            other => {
                return Err(format!(
                    "unsupported sampled Vulkan image tiling {}",
                    other.as_raw()
                ));
            }
        };
        let mut image_info = unsafe {
            sk_vk::ImageInfo::new(
                raw::image_to_skia(self.interop.image()),
                sk_vk::Alloc::default(),
                tiling,
                sk_vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                format,
                1,
                Some(self.interop_device_queue_family()),
                ycbcr,
                Some(Protected::No),
                Some(sk_vk::SharingMode::EXCLUSIVE),
            )
        };
        image_info.image_usage_flags = self.interop.sampled_usage().as_raw();
        image_info.sample_count = 1;
        Ok(unsafe { gpu::backend_textures::make_vk((width, height), &image_info, label) })
    }

    pub fn make_staged_nv12_backend_textures(
        &self,
        label: &str,
    ) -> Result<Option<StagedNv12BackendTextures>, String> {
        let Some(planes) = self.interop.staged_nv12_planes() else {
            return Ok(None);
        };
        let dimensions = self.interop.dimensions();
        let luma = self.make_optimal_backend_texture(
            planes.luma_image,
            dimensions,
            sk_vk::Format::R8_UNORM,
            &format!("{label}:y"),
        )?;
        let chroma = self.make_optimal_backend_texture(
            planes.chroma_image,
            (dimensions.0 / 2, dimensions.1 / 2),
            sk_vk::Format::R8G8_UNORM,
            &format!("{label}:uv"),
        )?;
        Ok(Some(StagedNv12BackendTextures {
            luma,
            chroma,
            conversion: planes.conversion,
        }))
    }

    fn make_optimal_backend_texture(
        &self,
        image: vk::Image,
        dimensions: (u32, u32),
        format: sk_vk::Format,
        label: &str,
    ) -> Result<gpu::BackendTexture, String> {
        let width = i32::try_from(dimensions.0)
            .map_err(|_| "Vulkan imported image width exceeds i32".to_string())?;
        let height = i32::try_from(dimensions.1)
            .map_err(|_| "Vulkan imported image height exceeds i32".to_string())?;
        let mut image_info = unsafe {
            sk_vk::ImageInfo::new(
                raw::image_to_skia(image),
                sk_vk::Alloc::default(),
                sk_vk::ImageTiling::OPTIMAL,
                sk_vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                format,
                1,
                Some(self.interop_device_queue_family()),
                None,
                Some(Protected::No),
                Some(sk_vk::SharingMode::EXCLUSIVE),
            )
        };
        image_info.image_usage_flags = self.interop.sampled_usage().as_raw();
        image_info.sample_count = 1;
        Ok(unsafe { gpu::backend_textures::make_vk((width, height), &image_info, label) })
    }

    fn interop_device_queue_family(&self) -> u32 {
        self.interop_device().queue_family_index()
    }

    fn interop_device(&self) -> &VulkanDevice {
        // The generic owner intentionally does not expose its Arc. Every imported image in this
        // renderer uses the same active device; the queue family is encoded by a small helper on
        // the owner to avoid leaking allocation ownership into Skia integration.
        self.interop.device()
    }
}

fn skia_ycbcr_info(
    conversion: Nv12Conversion,
    features: vk::FormatFeatureFlags,
) -> sk_vk::YcbcrConversionInfo {
    let model = match conversion.model {
        YcbcrModel::Bt601 => sk_vk::SamplerYcbcrModelConversion::YCBCR_601,
        YcbcrModel::Bt709 => sk_vk::SamplerYcbcrModelConversion::YCBCR_709,
        YcbcrModel::Bt2020 => sk_vk::SamplerYcbcrModelConversion::YCBCR_2020,
    };
    let range = match conversion.range {
        YcbcrRange::Narrow => sk_vk::SamplerYcbcrRange::ITU_NARROW,
        YcbcrRange::Full => sk_vk::SamplerYcbcrRange::ITU_FULL,
    };
    let offset = |offset| match offset {
        YcbcrOffset::CositedEven => sk_vk::ChromaLocation::COSITED_EVEN,
        YcbcrOffset::Midpoint => sk_vk::ChromaLocation::MIDPOINT,
    };
    let identity = sk_vk::ComponentSwizzle::VK_COMPONENT_SWIZZLE_IDENTITY;
    sk_vk::YcbcrConversionInfo::new_with_format(
        sk_vk::Format::G8_B8R8_2PLANE_420_UNORM,
        model,
        range,
        offset(conversion.x_offset),
        offset(conversion.y_offset),
        sk_vk::Filter::LINEAR,
        0,
        sk_vk::ComponentMapping {
            r: identity,
            g: identity,
            b: identity,
            a: identity,
        },
        features.as_raw(),
    )
}

pub fn validate_bgra_scanout_import_support(
    device: &Arc<VulkanDevice>,
    modifier: u64,
) -> Result<(), String> {
    video_interop::vulkan::validate_bgra_scanout_import_support(
        device.as_ref(),
        modifier,
        super::ganesh::GANESH_TARGET_IMAGE_USAGE,
    )
}

pub fn validate_rgba_import_support(
    device: &Arc<VulkanDevice>,
    modifier: u64,
) -> Result<(), String> {
    video_interop::vulkan::validate_rgba_import_support(device.as_ref(), modifier)
}

pub fn validate_packed_import_support(
    device: &Arc<VulkanDevice>,
    format: PackedImageFormat,
    modifier: u64,
) -> Result<(), String> {
    video_interop::vulkan::validate_packed_import_support(device.as_ref(), format, modifier)
}

pub fn validate_packed_staging_support(
    device: &Arc<VulkanDevice>,
    format: PackedImageFormat,
    modifier: u64,
) -> Result<(), String> {
    video_interop::vulkan::validate_packed_staging_support(device.as_ref(), format, modifier)
}

#[cfg(test)]
mod tests {
    use video_interop::{ChromaLocation, ColorRange, Colorimetry, Matrix, Primaries, Transfer};

    use super::*;

    fn identity() -> VulkanDeviceIdentity {
        VulkanDeviceIdentity {
            primary_node: Some(super::super::DrmNodeId {
                major: 226,
                minor: 0,
            }),
            render_node: Some(super::super::DrmNodeId {
                major: 226,
                minor: 128,
            }),
            vendor_id: 0x14e4,
            device_id: 0x2712,
            device_uuid: [1; vk::UUID_SIZE],
            driver_id: Some(vk::DriverId::MESA_V3DV.as_raw()),
            driver_version: 1,
            driver_uuid: [2; vk::UUID_SIZE],
        }
    }

    fn topology() -> Nv12FrameTopology {
        Nv12FrameTopology {
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
        }
    }

    fn conversion() -> Nv12Conversion {
        map_nv12_colorimetry(Colorimetry {
            primaries: Primaries::Bt709,
            transfer: Transfer::Bt709,
            matrix: Matrix::Bt709,
            range: ColorRange::Limited,
            chroma_location: ChromaLocation::Left,
        })
        .unwrap()
    }

    fn staged_capability() -> Nv12ModifierCapability {
        Nv12ModifierCapability::from_interop(
            identity(),
            video_interop::vulkan::Nv12ModifierCapability {
                modifier: 0,
                strategy: Nv12ImportStrategy::LinearBufferToRgba,
                modifier_plane_count: 1,
                source_tiling_features: vk::FormatFeatureFlags::TRANSFER_SRC,
                sampled_tiling_features: vk::FormatFeatureFlags::SAMPLED_IMAGE
                    | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR
                    | vk::FormatFeatureFlags::STORAGE_IMAGE
                    | vk::FormatFeatureFlags::TRANSFER_SRC
                    | vk::FormatFeatureFlags::TRANSFER_DST,
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

    #[test]
    fn accepts_v3dv_linear_nv12_through_generic_staging_capability() {
        let capability = staged_capability();
        validate_nv12_modifier_capability(capability, (64, 32), conversion()).unwrap();
        assert_eq!(capability.modifier_plane_count(), 1);
        assert_eq!(
            capability.allocation_recipe(),
            Nv12AllocationBindingRecipe::LinearBufferToRgba
        );
    }

    #[test]
    fn target_proof_matches_device_topology_and_actual_import_recipe() {
        let capability = staged_capability();
        let proof = Nv12TargetAllocationProof {
            device_identity: identity(),
            topology: topology(),
            recipe: capability.allocation_recipe(),
        };
        validate_nv12_allocation_proof(
            identity(),
            proof,
            topology(),
            capability.allocation_recipe(),
        )
        .unwrap();

        assert!(
            validate_nv12_allocation_proof(
                identity(),
                proof,
                topology(),
                Nv12AllocationBindingRecipe::DirectSharedImage,
            )
            .is_err()
        );
    }
}
