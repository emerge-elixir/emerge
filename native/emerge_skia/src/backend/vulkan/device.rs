use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ash::{Device, vk};

use super::{
    capabilities::{
        DrmNodeId, EXT_IMAGE_FORMAT_LIST, EXT_PHYSICAL_DEVICE_DRM, EXT_SAMPLER_YCBCR_CONVERSION,
        SelectionCandidate, SelectionNode, SurfaceQueueCandidate, WaylandSelectionCandidate,
        api_at_least, classify_software_device, driver_properties_source, effective_api,
        extension_map, properties2_available, select_matching_device,
        select_wayland_surface_device,
    },
    instance::VulkanInstance,
};

pub struct DeviceRequirements<'a> {
    pub required_extensions: &'a [&'a CStr],
    pub require_timestamps: bool,
    pub compositor_device: Option<DrmNodeId>,
}

pub struct ExactDeviceRequirements<'a> {
    pub required_extensions: &'a [&'a CStr],
    pub require_timestamps: bool,
    pub selection_node: SelectionNode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VulkanDeviceIdentity {
    pub primary_node: Option<DrmNodeId>,
    pub render_node: Option<DrmNodeId>,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_uuid: [u8; vk::UUID_SIZE],
    pub driver_id: Option<i32>,
    pub driver_version: u32,
    pub driver_uuid: [u8; vk::UUID_SIZE],
}

/// Immutable public diagnostics retained from the physical device that actually won selection.
/// This must never be reconstructed by re-enumerating devices after startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VulkanDeviceReport {
    pub physical_device_name: String,
    pub driver_name: Option<String>,
    pub driver_id: Option<String>,
    pub software: bool,
}

/// Exact DRM node used as the selection constraint for a Vulkan renderer. KMS output identity is
/// intentionally separate: split VC5/V3D systems may use different primary and render devices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VulkanDrmNodeReport {
    pub path: String,
    pub match_field: &'static str,
    pub major: u32,
    pub minor: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VulkanRendererReport {
    pub device: VulkanDeviceReport,
    pub drm_node: Option<VulkanDrmNodeReport>,
}

impl VulkanRendererReport {
    pub fn for_device(device: &VulkanDevice) -> Self {
        Self {
            device: device.report().clone(),
            drm_node: None,
        }
    }

    pub fn for_selected_node(
        device: &VulkanDevice,
        path: impl Into<String>,
        selection: SelectionNode,
    ) -> Self {
        Self {
            device: device.report().clone(),
            drm_node: Some(VulkanDrmNodeReport {
                path: path.into(),
                match_field: selection.field.as_str(),
                major: selection.node.major,
                minor: selection.node.minor,
            }),
        }
    }
}

struct EnumeratedDevice {
    physical_device: vk::PhysicalDevice,
    properties: vk::PhysicalDeviceProperties,
    identity: VulkanDeviceIdentity,
    report: VulkanDeviceReport,
    selection: SelectionCandidate,
    sampler_ycbcr_conversion: bool,
    queue_families: Vec<vk::QueueFamilyProperties>,
}

/// Shared logical-device and queue owner. It contains no Wayland or DRM presentation objects.
pub struct VulkanDevice {
    instance: Arc<VulkanInstance>,
    physical_device: vk::PhysicalDevice,
    device: Device,
    queue: vk::Queue,
    queue_family_index: u32,
    physical_device_name: String,
    physical_device_api: u32,
    identity: VulkanDeviceIdentity,
    report: VulkanDeviceReport,
    enabled_extensions: Vec<String>,
    lost: AtomicBool,
}

impl VulkanDevice {
    pub fn new_for_surface(
        instance: Arc<VulkanInstance>,
        requirements: DeviceRequirements<'_>,
        mut presentation_support: impl FnMut(vk::PhysicalDevice, u32) -> Result<bool, String>,
    ) -> Result<Arc<Self>, String> {
        let devices = enumerate_devices(&instance, requirements.required_extensions)?;
        let mut selection_candidates = Vec::with_capacity(devices.len());
        for candidate in &devices {
            let mut queue_families = Vec::with_capacity(candidate.queue_families.len());
            for (family_index, family) in candidate.queue_families.iter().enumerate() {
                let family_index_u32 = u32::try_from(family_index)
                    .map_err(|_| "Vulkan queue-family index exceeds u32".to_string())?;
                let present = presentation_support(candidate.physical_device, family_index_u32)?;
                queue_families.push(SurfaceQueueCandidate {
                    family_index,
                    graphics: family.queue_flags.contains(vk::QueueFlags::GRAPHICS),
                    present,
                    queue_count: family.queue_count,
                    timestamp_valid_bits: family.timestamp_valid_bits,
                });
            }
            selection_candidates.push(WaylandSelectionCandidate {
                device: candidate.selection.clone(),
                queue_families,
            });
        }

        let selected = select_wayland_surface_device(
            requirements.compositor_device,
            requirements.require_timestamps,
            &selection_candidates,
        )?;
        let candidate = devices
            .get(selected.device_index)
            .ok_or_else(|| "selected Vulkan physical-device index is out of bounds".to_string())?;
        let queue_family_index = u32::try_from(selected.queue_family_index)
            .map_err(|_| "selected Vulkan queue-family index exceeds u32".to_string())?;
        Self::create_logical(
            instance,
            candidate,
            queue_family_index,
            requirements.required_extensions,
        )
    }

    pub fn new_for_drm_node(
        instance: Arc<VulkanInstance>,
        requirements: ExactDeviceRequirements<'_>,
    ) -> Result<Arc<Self>, String> {
        let devices = enumerate_devices(&instance, requirements.required_extensions)?;
        let candidates = devices
            .iter()
            .map(|candidate| candidate.selection.clone())
            .collect::<Vec<_>>();
        let selected_index = select_matching_device(requirements.selection_node, &candidates)?;
        let candidate = devices
            .get(selected_index)
            .ok_or_else(|| "selected Vulkan physical-device index is out of bounds".to_string())?;
        let queue_family_index = candidate
            .queue_families
            .iter()
            .enumerate()
            .find(|(_index, family)| {
                family.queue_count > 0
                    && family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                    && (!requirements.require_timestamps || family.timestamp_valid_bits > 0)
            })
            .map(|(index, _family)| index)
            .ok_or_else(|| {
                format!(
                    "selected Vulkan physical device {} has no eligible graphics queue",
                    candidate.selection.name
                )
            })?;
        let queue_family_index = u32::try_from(queue_family_index)
            .map_err(|_| "selected Vulkan queue-family index exceeds u32".to_string())?;
        Self::create_logical(
            instance,
            candidate,
            queue_family_index,
            requirements.required_extensions,
        )
    }

    fn create_logical(
        instance: Arc<VulkanInstance>,
        candidate: &EnumeratedDevice,
        queue_family_index: u32,
        required_extensions: &[&CStr],
    ) -> Result<Arc<Self>, String> {
        let enabled_extension_names = required_extensions
            .iter()
            .copied()
            .filter(|name| {
                !device_extension_promoted(
                    instance.api_version(),
                    candidate.properties.api_version,
                    name,
                )
            })
            .collect::<Vec<_>>();
        let enabled_extensions = enabled_extension_names
            .iter()
            .map(|name| {
                name.to_str()
                    .map(str::to_string)
                    .map_err(|_| "Vulkan device extension name is not UTF-8".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let extension_ptrs = enabled_extension_names
            .iter()
            .map(|name| name.as_ptr())
            .collect::<Vec<_>>();
        let priority = [1.0_f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priority)];
        let enable_sampler_ycbcr =
            requires_extension(required_extensions, EXT_SAMPLER_YCBCR_CONVERSION);
        if enable_sampler_ycbcr && !candidate.sampler_ycbcr_conversion {
            return Err(format!(
                "selected Vulkan physical device {} does not support samplerYcbcrConversion",
                candidate.selection.name
            ));
        }
        let mut sampler_ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default()
            .sampler_ycbcr_conversion(enable_sampler_ycbcr);
        let mut create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&extension_ptrs);
        if enable_sampler_ycbcr {
            create_info = create_info.push_next(&mut sampler_ycbcr);
        }
        // SAFETY: the selected family exists with a queue, advertised/promoted extension and
        // feature requirements were checked, and all create-info slices live for the call. This
        // owner destroys the device after Ganesh and presenter resources are gone.
        let device = unsafe {
            instance
                .raw()
                .create_device(candidate.physical_device, &create_info, None)
        }
        .map_err(|result| {
            format!(
                "failed to create Vulkan device {}: {result:?}",
                candidate.selection.name
            )
        })?;
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        Ok(Arc::new(Self {
            instance,
            physical_device: candidate.physical_device,
            device,
            queue,
            queue_family_index,
            physical_device_name: candidate.selection.name.clone(),
            physical_device_api: candidate.properties.api_version,
            identity: candidate.identity,
            report: candidate.report.clone(),
            enabled_extensions,
            lost: AtomicBool::new(false),
        }))
    }

    pub fn instance(&self) -> &Arc<VulkanInstance> {
        &self.instance
    }

    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.physical_device
    }

    pub fn raw(&self) -> &Device {
        &self.device
    }

    pub fn queue(&self) -> vk::Queue {
        self.queue
    }

    pub fn queue_family_index(&self) -> u32 {
        self.queue_family_index
    }

    pub fn physical_device_api(&self) -> u32 {
        self.physical_device_api
    }

    pub fn physical_device_name(&self) -> &str {
        &self.physical_device_name
    }

    pub fn identity(&self) -> VulkanDeviceIdentity {
        self.identity
    }

    pub fn report(&self) -> &VulkanDeviceReport {
        &self.report
    }

    pub fn enabled_extensions(&self) -> &[String] {
        &self.enabled_extensions
    }

    pub fn mark_device_lost(&self) {
        self.lost.store(true, Ordering::Release);
    }

    pub fn is_device_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Used only for swapchain recreation and explicit shutdown.
    pub fn wait_idle(&self, reason: &str) -> Result<(), String> {
        if self.is_device_lost() {
            return Err(format!(
                "cannot wait for lost Vulkan device during {reason}"
            ));
        }
        match unsafe { self.device.device_wait_idle() } {
            Ok(()) => Ok(()),
            Err(vk::Result::ERROR_DEVICE_LOST) => {
                self.mark_device_lost();
                Err(format!("Vulkan device lost during {reason}"))
            }
            Err(result) => Err(format!(
                "Vulkan device wait idle failed during {reason}: {result:?}"
            )),
        }
    }
}

impl video_interop::vulkan::VulkanDeviceContext for VulkanDevice {
    fn instance(&self) -> &ash::Instance {
        self.instance.raw()
    }

    fn physical_device(&self) -> vk::PhysicalDevice {
        self.physical_device
    }

    fn device(&self) -> &ash::Device {
        &self.device
    }

    fn queue(&self) -> vk::Queue {
        self.queue
    }

    fn queue_family_index(&self) -> u32 {
        self.queue_family_index
    }

    fn mark_device_lost(&self) {
        VulkanDevice::mark_device_lost(self);
    }

    fn is_device_lost(&self) -> bool {
        VulkanDevice::is_device_lost(self)
    }
}

fn enumerate_devices(
    instance: &Arc<VulkanInstance>,
    required_extensions: &[&CStr],
) -> Result<Vec<EnumeratedDevice>, String> {
    let physical_devices = unsafe { instance.raw().enumerate_physical_devices() }
        .map_err(|result| format!("failed to enumerate Vulkan physical devices: {result:?}"))?;
    if physical_devices.is_empty() {
        return Err("Vulkan reported no physical devices".to_string());
    }

    physical_devices
        .into_iter()
        .enumerate()
        .map(|(index, physical_device)| {
            let properties = unsafe {
                instance
                    .raw()
                    .get_physical_device_properties(physical_device)
            };
            let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            let extensions = unsafe {
                instance
                    .raw()
                    .enumerate_device_extension_properties(physical_device)
            }
            .map_err(|result| {
                format!(
                    "failed to enumerate Vulkan device extensions for {}: {result:?}",
                    name
                )
            })?;
            let extensions = extension_map(&extensions);
            let required_extensions_available = required_extensions.iter().all(|name| {
                name.to_str().ok().is_some_and(|name_string| {
                    extensions.contains_key(name_string)
                        || device_extension_promoted(
                            instance.api_version(),
                            properties.api_version,
                            name,
                        )
                })
            });
            let sampler_ycbcr_conversion = {
                let mut sampler = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
                let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut sampler);
                unsafe {
                    instance
                        .raw()
                        .get_physical_device_features2(physical_device, &mut features)
                };
                sampler.sampler_ycbcr_conversion == vk::TRUE
            };

            let mut id = vk::PhysicalDeviceIDProperties::default();
            let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
            // SAFETY: Emerge's Vulkan device floor is 1.1, where properties2 and device/driver
            // UUID identity are core. The chained storage remains live for the call.
            unsafe {
                instance
                    .raw()
                    .get_physical_device_properties2(physical_device, &mut properties2)
            };
            let driver_properties = if driver_properties_source(
                instance.api_version(),
                properties.api_version,
                &extensions,
            )
            .is_supported()
            {
                let mut driver = vk::PhysicalDeviceDriverProperties::default();
                let mut properties2 =
                    vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
                // SAFETY: the effective API/extension rule above makes DriverProperties legal in
                // this properties2 chain and all storage remains live for the call.
                unsafe {
                    instance
                        .raw()
                        .get_physical_device_properties2(physical_device, &mut properties2)
                };
                Some(driver)
            } else {
                None
            };
            let driver_id = driver_properties
                .as_ref()
                .map(|driver| driver.driver_id.as_raw());
            let driver_name = driver_properties.as_ref().map(|driver| {
                // SAFETY: Vulkan defines driver_name as a fixed, NUL-terminated C string.
                unsafe { CStr::from_ptr(driver.driver_name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned()
            });
            let driver_id_name = driver_properties
                .as_ref()
                .map(|driver| driver_id_name(driver.driver_id));
            let (software, _software_reason) =
                classify_software_device(properties.device_type, &name, driver_id);
            let (primary, render) =
                if properties2_available(instance.api_version(), properties.api_version)
                    && extensions.contains_key(EXT_PHYSICAL_DEVICE_DRM)
                {
                    let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
                    let mut properties2 =
                        vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
                    // SAFETY: Vulkan 1.1 makes properties2 core, the device advertises the DRM
                    // properties extension, and the chained storage remains live for the call.
                    unsafe {
                        instance
                            .raw()
                            .get_physical_device_properties2(physical_device, &mut properties2)
                    };
                    (
                        drm_node_id(drm.has_primary, drm.primary_major, drm.primary_minor),
                        drm_node_id(drm.has_render, drm.render_major, drm.render_minor),
                    )
                } else {
                    (None, None)
                };
            let sampler_ycbcr_eligible =
                !requires_extension(required_extensions, EXT_SAMPLER_YCBCR_CONVERSION)
                    || sampler_ycbcr_conversion;
            let api_eligible = api_at_least(
                effective_api(instance.api_version(), properties.api_version),
                1,
                1,
            ) && required_extensions_available
                && sampler_ycbcr_eligible;
            let queue_families = unsafe {
                instance
                    .raw()
                    .get_physical_device_queue_family_properties(physical_device)
            };

            Ok(EnumeratedDevice {
                physical_device,
                properties,
                identity: VulkanDeviceIdentity {
                    primary_node: primary,
                    render_node: render,
                    vendor_id: properties.vendor_id,
                    device_id: properties.device_id,
                    device_uuid: id.device_uuid,
                    driver_id,
                    driver_version: properties.driver_version,
                    driver_uuid: id.driver_uuid,
                },
                report: VulkanDeviceReport {
                    physical_device_name: name.clone(),
                    driver_name,
                    driver_id: driver_id_name,
                    software,
                },
                selection: SelectionCandidate {
                    index,
                    name,
                    primary,
                    render,
                    software,
                    api_eligible,
                },
                sampler_ycbcr_conversion,
                queue_families,
            })
        })
        .collect()
}

fn driver_id_name(driver_id: vk::DriverId) -> String {
    let name = match driver_id.as_raw() {
        1 => "AMD_PROPRIETARY",
        2 => "AMD_OPEN_SOURCE",
        3 => "MESA_RADV",
        4 => "NVIDIA_PROPRIETARY",
        5 => "INTEL_PROPRIETARY_WINDOWS",
        6 => "INTEL_OPEN_SOURCE_MESA",
        7 => "IMAGINATION_PROPRIETARY",
        8 => "QUALCOMM_PROPRIETARY",
        9 => "ARM_PROPRIETARY",
        10 => "GOOGLE_SWIFTSHADER",
        11 => "GGP_PROPRIETARY",
        12 => "BROADCOM_PROPRIETARY",
        13 => "MESA_LLVMPIPE",
        14 => "MOLTENVK",
        15 => "COREAVI_PROPRIETARY",
        16 => "JUICE_PROPRIETARY",
        17 => "VERISILICON_PROPRIETARY",
        18 => "MESA_TURNIP",
        19 => "MESA_V3DV",
        20 => "MESA_PANVK",
        21 => "SAMSUNG_PROPRIETARY",
        22 => "MESA_VENUS",
        23 => "MESA_DOZEN",
        24 => "MESA_NVK",
        25 => "IMAGINATION_OPEN_SOURCE_MESA",
        26 => "MESA_AGXV",
        raw => return format!("UNKNOWN_{raw}"),
    };
    name.to_string()
}

fn requires_extension(required: &[&CStr], extension: &str) -> bool {
    required
        .iter()
        .any(|candidate| candidate.to_bytes() == extension.as_bytes())
}

fn device_extension_promoted(
    instance_api: u32,
    physical_device_api: u32,
    extension: &CStr,
) -> bool {
    let effective = effective_api(instance_api, physical_device_api);
    (extension.to_bytes() == EXT_IMAGE_FORMAT_LIST.as_bytes() && api_at_least(effective, 1, 2))
        || (extension.to_bytes() == EXT_SAMPLER_YCBCR_CONVERSION.as_bytes()
            && api_at_least(effective, 1, 1))
}

fn drm_node_id(has_node: vk::Bool32, major: i64, minor: i64) -> Option<DrmNodeId> {
    if has_node != vk::TRUE {
        return None;
    }
    Some(DrmNodeId {
        major: u32::try_from(major).ok()?,
        minor: u32::try_from(minor).ok()?,
    })
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        if !self.is_device_lost()
            && let Err(result) = unsafe { self.device.device_wait_idle() }
        {
            eprintln!("Vulkan device shutdown wait failed: {result:?}");
        }
        // SAFETY: the shared engine/presenter teardown removes all device children before the last
        // Arc is released. Device loss uses Vulkan's allowed lost-device destruction path.
        unsafe { self.device.destroy_device(None) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_format_list_is_enabled_below_core_promotion_only() {
        assert!(!device_extension_promoted(
            vk::API_VERSION_1_1,
            vk::API_VERSION_1_2,
            ash::khr::image_format_list::NAME,
        ));
        assert!(device_extension_promoted(
            vk::API_VERSION_1_2,
            vk::API_VERSION_1_2,
            ash::khr::image_format_list::NAME,
        ));
    }

    #[test]
    fn stable_driver_id_names_include_v3dv_and_do_not_guess_unknown_ids() {
        assert_eq!(driver_id_name(vk::DriverId::MESA_V3DV), "MESA_V3DV");
        assert_eq!(
            driver_id_name(vk::DriverId::from_raw(9_999)),
            "UNKNOWN_9999"
        );
    }

    #[test]
    fn sampler_ycbcr_is_core_at_the_vulkan_floor_but_still_requires_its_feature() {
        assert!(device_extension_promoted(
            vk::API_VERSION_1_1,
            vk::API_VERSION_1_1,
            ash::khr::sampler_ycbcr_conversion::NAME,
        ));
        assert!(requires_extension(
            &[ash::khr::sampler_ycbcr_conversion::NAME],
            EXT_SAMPLER_YCBCR_CONVERSION,
        ));
    }
}
