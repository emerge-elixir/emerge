//! Pure Vulkan capability, promotion, and physical-device selection rules.
//!
//! This module deliberately does not open DRM nodes or create platform surfaces. Presenters turn
//! their live platform objects into these constraints, while the shared Vulkan engine consumes the
//! same profile and selection vocabulary.

use std::{collections::BTreeMap, ffi::c_char};

use ash::vk;

pub const MIN_INSTANCE_API_VERSION: u32 = vk::API_VERSION_1_1;
pub const MAX_INSTANCE_API_VERSION: u32 = vk::API_VERSION_1_2;

pub const EXT_GET_PHYSICAL_DEVICE_PROPERTIES_2: &str = "VK_KHR_get_physical_device_properties2";
pub const EXT_EXTERNAL_MEMORY_CAPABILITIES: &str = "VK_KHR_external_memory_capabilities";
pub const EXT_EXTERNAL_SEMAPHORE_CAPABILITIES: &str = "VK_KHR_external_semaphore_capabilities";
pub const EXT_EXTERNAL_MEMORY: &str = "VK_KHR_external_memory";
pub const EXT_EXTERNAL_MEMORY_FD: &str = "VK_KHR_external_memory_fd";
pub const EXT_EXTERNAL_MEMORY_DMA_BUF: &str = "VK_EXT_external_memory_dma_buf";
pub const EXT_IMAGE_DRM_FORMAT_MODIFIER: &str = "VK_EXT_image_drm_format_modifier";
pub const EXT_IMAGE_FORMAT_LIST: &str = "VK_KHR_image_format_list";
pub const EXT_EXTERNAL_SEMAPHORE: &str = "VK_KHR_external_semaphore";
pub const EXT_EXTERNAL_SEMAPHORE_FD: &str = "VK_KHR_external_semaphore_fd";
pub const EXT_SAMPLER_YCBCR_CONVERSION: &str = "VK_KHR_sampler_ycbcr_conversion";
pub const EXT_QUEUE_FAMILY_FOREIGN: &str = "VK_EXT_queue_family_foreign";
pub const EXT_PHYSICAL_DEVICE_DRM: &str = "VK_EXT_physical_device_drm";
pub const EXT_BIND_MEMORY_2: &str = "VK_KHR_bind_memory2";
pub const EXT_GET_MEMORY_REQUIREMENTS_2: &str = "VK_KHR_get_memory_requirements2";
pub const EXT_DEDICATED_ALLOCATION: &str = "VK_KHR_dedicated_allocation";
pub const EXT_DRIVER_PROPERTIES: &str = "VK_KHR_driver_properties";
pub const EXT_WAYLAND_SURFACE: &str = "VK_KHR_wayland_surface";
pub const EXT_SWAPCHAIN: &str = "VK_KHR_swapchain";

pub type ExtensionMap = BTreeMap<String, u32>;

pub fn extension_map(properties: &[vk::ExtensionProperties]) -> ExtensionMap {
    properties
        .iter()
        .map(|property| {
            (
                char_array_to_string(&property.extension_name),
                property.spec_version,
            )
        })
        .collect()
}

fn char_array_to_string<const N: usize>(value: &[c_char; N]) -> String {
    let bytes = value
        .iter()
        .copied()
        .take_while(|character| *character != 0)
        .map(|character| character as u8)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportSource {
    Core,
    Extension,
    Missing,
}

impl SupportSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Extension => "extension",
            Self::Missing => "missing",
        }
    }

    pub const fn is_supported(self) -> bool {
        !matches!(self, Self::Missing)
    }
}

/// Select the highest API this engine intentionally uses, bounded by loader support.
pub fn select_instance_api(loader_api: u32) -> Result<u32, String> {
    if !api_at_least(loader_api, 1, 1) {
        return Err(format!(
            "Vulkan loader API {} is below required Vulkan 1.1",
            format_api_version(loader_api)
        ));
    }

    Ok(loader_api.min(MAX_INSTANCE_API_VERSION))
}

/// Vulkan core promotions are legal only up to the API shared by the instance and device.
pub const fn effective_api(instance_api: u32, physical_device_api: u32) -> u32 {
    if instance_api < physical_device_api {
        instance_api
    } else {
        physical_device_api
    }
}

pub fn api_at_least(version: u32, required_major: u32, required_minor: u32) -> bool {
    (
        vk::api_version_major(version),
        vk::api_version_minor(version),
    ) >= (required_major, required_minor)
}

pub fn support_source(
    effective_api: u32,
    extensions: &ExtensionMap,
    extension: &str,
    promoted_to: Option<(u32, u32)>,
) -> SupportSource {
    if promoted_to.is_some_and(|(major, minor)| api_at_least(effective_api, major, minor)) {
        SupportSource::Core
    } else if extensions.contains_key(extension) {
        SupportSource::Extension
    } else {
        SupportSource::Missing
    }
}

pub fn driver_properties_source(
    instance_api: u32,
    physical_device_api: u32,
    extensions: &ExtensionMap,
) -> SupportSource {
    support_source(
        effective_api(instance_api, physical_device_api),
        extensions,
        EXT_DRIVER_PROPERTIES,
        Some((1, 2)),
    )
}

pub fn properties2_available(instance_api: u32, physical_device_api: u32) -> bool {
    api_at_least(effective_api(instance_api, physical_device_api), 1, 1)
}

pub fn format_api_version(version: u32) -> String {
    let base = format!(
        "{}.{}.{}",
        vk::api_version_major(version),
        vk::api_version_minor(version),
        vk::api_version_patch(version)
    );
    let variant = vk::api_version_variant(version);
    if variant == 0 {
        base
    } else {
        format!("{variant}:{base}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityProfile {
    Ganesh,
    WaylandWsi,
    DrmScanout,
    ExternalDmaBufVideo,
    Capture,
    Timestamps,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapabilityProfiles(u8);

impl CapabilityProfiles {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn of(profile: CapabilityProfile) -> Self {
        Self(profile_bit(profile))
    }

    pub const fn with(self, profile: CapabilityProfile) -> Self {
        Self(self.0 | profile_bit(profile))
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, profile: CapabilityProfile) -> bool {
        self.0 & profile_bit(profile) != 0
    }

    pub const fn wayland_ui() -> Self {
        Self::of(CapabilityProfile::Ganesh)
            .with(CapabilityProfile::WaylandWsi)
            .with(CapabilityProfile::Capture)
            .with(CapabilityProfile::Timestamps)
    }

    pub const fn drm_camera() -> Self {
        Self::of(CapabilityProfile::Ganesh)
            .with(CapabilityProfile::DrmScanout)
            .with(CapabilityProfile::ExternalDmaBufVideo)
            .with(CapabilityProfile::Capture)
            .with(CapabilityProfile::Timestamps)
    }
}

const fn profile_bit(profile: CapabilityProfile) -> u8 {
    match profile {
        CapabilityProfile::Ganesh => 1 << 0,
        CapabilityProfile::WaylandWsi => 1 << 1,
        CapabilityProfile::DrmScanout => 1 << 2,
        CapabilityProfile::ExternalDmaBufVideo => 1 << 3,
        CapabilityProfile::Capture => 1 << 4,
        CapabilityProfile::Timestamps => 1 << 5,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrmNodeId {
    pub major: u32,
    pub minor: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrmMatchField {
    Primary,
    Render,
}

impl DrmMatchField {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Render => "render",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionNode {
    pub node: DrmNodeId,
    pub field: DrmMatchField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionConstraint {
    ExactDrmNode(SelectionNode),
    WaylandSurface {
        compositor_device: Option<DrmNodeId>,
        require_timestamps: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionCandidate {
    pub index: usize,
    pub name: String,
    pub primary: Option<DrmNodeId>,
    pub render: Option<DrmNodeId>,
    pub software: bool,
    pub api_eligible: bool,
}

impl SelectionCandidate {
    pub const fn node(&self, field: DrmMatchField) -> Option<DrmNodeId> {
        match field {
            DrmMatchField::Primary => self.primary,
            DrmMatchField::Render => self.render,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceQueueCandidate {
    pub family_index: usize,
    pub graphics: bool,
    pub present: bool,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaylandSelectionCandidate {
    pub device: SelectionCandidate,
    pub queue_families: Vec<SurfaceQueueCandidate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceDeviceSelection {
    pub device_index: usize,
    pub queue_family_index: usize,
}

pub fn select_matching_device(
    selection_node: SelectionNode,
    candidates: &[SelectionCandidate],
) -> Result<usize, String> {
    let exact_matches = candidates
        .iter()
        .filter(|candidate| candidate.node(selection_node.field) == Some(selection_node.node))
        .collect::<Vec<_>>();
    let hardware_matches = exact_matches
        .iter()
        .copied()
        .filter(|candidate| !candidate.software && candidate.api_eligible)
        .collect::<Vec<_>>();

    match hardware_matches.as_slice() {
        [selected] => Ok(selected.index),
        [] => {
            let software_names = exact_matches
                .iter()
                .filter(|candidate| candidate.software)
                .map(|candidate| escape_value(&candidate.name))
                .collect::<Vec<_>>();
            let old_api_names = exact_matches
                .iter()
                .filter(|candidate| !candidate.software && !candidate.api_eligible)
                .map(|candidate| escape_value(&candidate.name))
                .collect::<Vec<_>>();
            if !software_names.is_empty() {
                Err(format!(
                    "{} DRM node {}:{} matches only rejected software Vulkan device(s): {}",
                    selection_node.field.as_str(),
                    selection_node.node.major,
                    selection_node.node.minor,
                    software_names.join(",")
                ))
            } else if !old_api_names.is_empty() {
                Err(format!(
                    "{} DRM node {}:{} matches only Vulkan physical device(s) below API 1.1: {}",
                    selection_node.field.as_str(),
                    selection_node.node.major,
                    selection_node.node.minor,
                    old_api_names.join(",")
                ))
            } else {
                Err(format!(
                    "no eligible hardware Vulkan physical device has exact {} DRM node {}:{}",
                    selection_node.field.as_str(),
                    selection_node.node.major,
                    selection_node.node.minor
                ))
            }
        }
        matches => Err(format!(
            "{} DRM node {}:{} has {} eligible hardware Vulkan physical-device matches; exactly one is required: {}",
            selection_node.field.as_str(),
            selection_node.node.major,
            selection_node.node.minor,
            matches.len(),
            matches
                .iter()
                .map(|candidate| format!("{}:{}", candidate.index, escape_value(&candidate.name)))
                .collect::<Vec<_>>()
                .join(",")
        )),
    }
}

/// Select one hardware device and one combined graphics/present queue for a live Wayland surface.
/// Surface support is supplied by the Wayland presenter after its WSI query.
pub fn select_wayland_surface_device(
    compositor_device: Option<DrmNodeId>,
    require_timestamps: bool,
    candidates: &[WaylandSelectionCandidate],
) -> Result<SurfaceDeviceSelection, String> {
    let matching = candidates
        .iter()
        .filter(|candidate| !candidate.device.software && candidate.device.api_eligible)
        .filter(|candidate| {
            compositor_device.is_none_or(|node| {
                candidate.device.primary == Some(node) || candidate.device.render == Some(node)
            })
        })
        .filter_map(|candidate| {
            candidate
                .queue_families
                .iter()
                .find(|queue| {
                    queue.queue_count > 0
                        && queue.graphics
                        && queue.present
                        && (!require_timestamps || queue.timestamp_valid_bits > 0)
                })
                .map(|queue| SurfaceDeviceSelection {
                    device_index: candidate.device.index,
                    queue_family_index: queue.family_index,
                })
        })
        .collect::<Vec<_>>();

    match (compositor_device, matching.as_slice()) {
        (_, [selected]) => Ok(*selected),
        (Some(node), []) => Err(format!(
            "no eligible hardware Vulkan device matches Wayland compositor DRM device {}:{} with a combined graphics/present queue",
            node.major, node.minor,
        )),
        (None, []) => Err(
            "no eligible hardware Vulkan device has a combined graphics/present queue for the Wayland surface"
                .to_string(),
        ),
        (Some(node), matches) => Err(format!(
            "Wayland compositor DRM device {}:{} matches {} eligible hardware Vulkan devices; exactly one is required",
            node.major,
            node.minor,
            matches.len(),
        )),
        (None, matches) => Err(format!(
            "Wayland surface has {} eligible hardware Vulkan devices but compositor DMA-BUF feedback is unavailable; exactly one is required",
            matches.len(),
        )),
    }
}

pub fn select_graphics_queue_family(families: &[vk::QueueFamilyProperties]) -> Option<usize> {
    families.iter().position(|family| {
        family.queue_count > 0
            && family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            && family.timestamp_valid_bits > 0
    })
}

pub fn classify_software_device(
    device_type: vk::PhysicalDeviceType,
    name: &str,
    driver_id: Option<i32>,
) -> (bool, Option<&'static str>) {
    if device_type == vk::PhysicalDeviceType::CPU {
        return (true, Some("cpu_device_type"));
    }
    if driver_id == Some(vk::DriverId::MESA_LLVMPIPE.as_raw()) {
        return (true, Some("mesa_llvmpipe_driver"));
    }
    if driver_id == Some(vk::DriverId::GOOGLE_SWIFTSHADER.as_raw()) {
        return (true, Some("swiftshader_driver"));
    }
    let lowercase_name = name.to_ascii_lowercase();
    if lowercase_name.contains("lavapipe") || lowercase_name.contains("llvmpipe") {
        return (true, Some("software_device_name"));
    }
    (false, None)
}

fn escape_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(minor: u32) -> DrmNodeId {
        DrmNodeId { major: 226, minor }
    }

    fn candidate(index: usize, render: Option<DrmNodeId>) -> SelectionCandidate {
        SelectionCandidate {
            index,
            name: format!("device-{index}"),
            primary: None,
            render,
            software: false,
            api_eligible: true,
        }
    }

    #[test]
    fn instance_api_is_loader_bounded_with_vulkan_1_1_floor() {
        assert!(select_instance_api(vk::API_VERSION_1_0).is_err());
        assert_eq!(
            select_instance_api(vk::make_api_version(0, 1, 1, 42)),
            Ok(vk::make_api_version(0, 1, 1, 42))
        );
        assert_eq!(
            select_instance_api(vk::API_VERSION_1_2),
            Ok(vk::API_VERSION_1_2)
        );
        assert_eq!(
            select_instance_api(vk::API_VERSION_1_3),
            Ok(vk::API_VERSION_1_2)
        );
    }

    #[test]
    fn effective_api_bounds_every_core_promotion_by_instance_and_device() {
        assert_eq!(
            effective_api(vk::API_VERSION_1_1, vk::API_VERSION_1_3),
            vk::API_VERSION_1_1
        );
        assert_eq!(
            driver_properties_source(
                vk::API_VERSION_1_1,
                vk::API_VERSION_1_2,
                &ExtensionMap::new()
            ),
            SupportSource::Missing
        );
        assert_eq!(
            driver_properties_source(
                vk::API_VERSION_1_1,
                vk::API_VERSION_1_2,
                &ExtensionMap::from([(EXT_DRIVER_PROPERTIES.to_string(), 1)])
            ),
            SupportSource::Extension
        );
        assert_eq!(
            driver_properties_source(
                vk::API_VERSION_1_2,
                vk::API_VERSION_1_3,
                &ExtensionMap::new()
            ),
            SupportSource::Core
        );
    }

    #[test]
    fn capability_profiles_compose_without_mixing_presenters() {
        let wayland = CapabilityProfiles::wayland_ui();
        assert!(wayland.contains(CapabilityProfile::Ganesh));
        assert!(wayland.contains(CapabilityProfile::WaylandWsi));
        assert!(!wayland.contains(CapabilityProfile::DrmScanout));

        let drm = CapabilityProfiles::drm_camera();
        assert!(drm.contains(CapabilityProfile::DrmScanout));
        assert!(drm.contains(CapabilityProfile::ExternalDmaBufVideo));
        assert!(!drm.contains(CapabilityProfile::WaylandWsi));
    }

    #[test]
    fn exact_drm_selection_preserves_primary_render_field_identity() {
        let candidates = [candidate(3, Some(node(128))), candidate(7, Some(node(129)))];
        assert_eq!(
            select_matching_device(
                SelectionNode {
                    node: node(129),
                    field: DrmMatchField::Render,
                },
                &candidates,
            ),
            Ok(7)
        );
    }

    #[test]
    fn wayland_selection_requires_compositor_identity_for_multiple_hardware_devices() {
        let queue = SurfaceQueueCandidate {
            family_index: 0,
            graphics: true,
            present: true,
            queue_count: 1,
            timestamp_valid_bits: 64,
        };
        let candidates = [
            WaylandSelectionCandidate {
                device: candidate(1, Some(node(128))),
                queue_families: vec![queue],
            },
            WaylandSelectionCandidate {
                device: candidate(2, Some(node(129))),
                queue_families: vec![queue],
            },
        ];

        assert!(
            select_wayland_surface_device(None, true, &candidates)
                .expect_err("multi-GPU Wayland selection requires compositor feedback")
                .contains("DMA-BUF feedback is unavailable")
        );
        assert_eq!(
            select_wayland_surface_device(Some(node(128)), true, &candidates),
            Ok(SurfaceDeviceSelection {
                device_index: 1,
                queue_family_index: 0,
            })
        );
    }

    #[test]
    fn wayland_selection_rejects_software_only_devices() {
        let queue = SurfaceQueueCandidate {
            family_index: 0,
            graphics: true,
            present: true,
            queue_count: 1,
            timestamp_valid_bits: 64,
        };
        let mut software = candidate(3, None);
        software.software = true;
        assert!(
            select_wayland_surface_device(
                None,
                true,
                &[WaylandSelectionCandidate {
                    device: software,
                    queue_families: vec![queue],
                }],
            )
            .expect_err("software devices must fail")
            .contains("no eligible hardware")
        );
    }

    #[test]
    fn wayland_selection_requires_one_combined_graphics_present_queue() {
        let candidates = [WaylandSelectionCandidate {
            device: candidate(4, None),
            queue_families: vec![
                SurfaceQueueCandidate {
                    family_index: 0,
                    graphics: true,
                    present: false,
                    queue_count: 1,
                    timestamp_valid_bits: 64,
                },
                SurfaceQueueCandidate {
                    family_index: 2,
                    graphics: true,
                    present: true,
                    queue_count: 1,
                    timestamp_valid_bits: 64,
                },
            ],
        }];

        assert_eq!(
            select_wayland_surface_device(None, true, &candidates),
            Ok(SurfaceDeviceSelection {
                device_index: 4,
                queue_family_index: 2,
            })
        );
    }
}
