use std::{
    ffi::{CStr, c_void},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use ash::{Entry, Instance, vk};

use super::capabilities::{extension_map, format_api_version, select_instance_api};

const VALIDATION_LAYER_NAME: &CStr = c"VK_LAYER_KHRONOS_validation";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VulkanValidationReport {
    pub enabled: bool,
    pub errors: u64,
    pub warnings: u64,
}

#[derive(Default)]
struct ValidationCounters {
    errors: AtomicU64,
    warnings: AtomicU64,
}

struct DebugUtilsState {
    loader: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    counters: Box<ValidationCounters>,
}

/// Dynamically loaded Vulkan entry and instance. Platform surfaces and logical devices retain an
/// `Arc`, so the instance is destroyed only after every child owner has released it.
pub struct VulkanInstance {
    entry: Entry,
    instance: Instance,
    debug_utils: Option<DebugUtilsState>,
    api_version: u32,
    enabled_extensions: Vec<String>,
}

impl VulkanInstance {
    pub fn new(required_extensions: &[*const std::ffi::c_char]) -> Result<Arc<Self>, String> {
        // SAFETY: ash loads the process Vulkan loader and owns the resulting function table.
        let entry = unsafe { Entry::load() }
            .map_err(|error| format!("failed to load Vulkan loader: {error}"))?;
        let loader_api = unsafe { entry.try_enumerate_instance_version() }
            .map_err(|result| format!("failed to query Vulkan loader API version: {result:?}"))?
            .unwrap_or(vk::API_VERSION_1_0);
        let api_version = select_instance_api(loader_api)?;

        let available =
            unsafe { entry.enumerate_instance_extension_properties(None) }.map_err(|result| {
                format!("failed to enumerate Vulkan instance extensions: {result:?}")
            })?;
        let available = extension_map(&available);
        let mut enabled_extensions = required_extensions
            .iter()
            .map(|name| {
                if name.is_null() {
                    return Err("Vulkan instance extension name is null".to_string());
                }
                // SAFETY: ash-window and ash extension constants provide static NUL-terminated
                // names. The caller keeps any custom name alive for this constructor call.
                let name = unsafe { CStr::from_ptr(*name) }
                    .to_str()
                    .map_err(|_| "Vulkan instance extension name is not UTF-8".to_string())?
                    .to_string();
                if !available.contains_key(&name) {
                    return Err(format!(
                        "required Vulkan instance extension {name} is unavailable"
                    ));
                }
                Ok(name)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let validation_requested = validation_requested_from_values(
            std::env::var("EMERGE_VULKAN_VALIDATION").ok().as_deref(),
            std::env::var("VK_INSTANCE_LAYERS").ok().as_deref(),
        );
        if validation_requested {
            let debug_utils_name = ash::ext::debug_utils::NAME
                .to_str()
                .expect("Vulkan extension names are UTF-8");
            if !available.contains_key(debug_utils_name) {
                return Err(
                    "Vulkan validation was requested but VK_EXT_debug_utils is unavailable"
                        .to_string(),
                );
            }
            if !enabled_extensions
                .iter()
                .any(|extension| extension == debug_utils_name)
            {
                enabled_extensions.push(debug_utils_name.to_string());
            }
            let layers =
                unsafe { entry.enumerate_instance_layer_properties() }.map_err(|result| {
                    format!("failed to enumerate Vulkan instance layers: {result:?}")
                })?;
            let validation_available = layers.iter().any(|layer| {
                // SAFETY: Vulkan layer names are fixed NUL-terminated arrays.
                unsafe { CStr::from_ptr(layer.layer_name.as_ptr()) == VALIDATION_LAYER_NAME }
            });
            if !validation_available {
                return Err(
                    "Vulkan validation was requested but VK_LAYER_KHRONOS_validation is unavailable"
                        .to_string(),
                );
            }
        }

        let mut extension_names = required_extensions.to_vec();
        if validation_requested
            && !required_extensions.iter().any(|name| {
                !name.is_null() && unsafe { CStr::from_ptr(*name) == ash::ext::debug_utils::NAME }
            })
        {
            extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
        }
        let layer_names = if validation_requested {
            vec![VALIDATION_LAYER_NAME.as_ptr()]
        } else {
            Vec::new()
        };
        let application_name = c"emerge-skia";
        let application_info = vk::ApplicationInfo::default()
            .application_name(application_name)
            .application_version(1)
            .engine_name(application_name)
            .engine_version(1)
            .api_version(api_version);
        let validation_counters = validation_requested.then(Box::<ValidationCounters>::default);
        let mut debug_create_info = validation_counters
            .as_deref()
            .map(debug_messenger_create_info);
        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&application_info)
            .enabled_extension_names(&extension_names)
            .enabled_layer_names(&layer_names);
        let create_info = match debug_create_info.as_mut() {
            Some(debug) => create_info.push_next(debug),
            None => create_info,
        };

        // SAFETY: extension/layer pointers, callback state, and application strings remain valid
        // for the call. The resulting instance is owned by this object and destroyed in `Drop`.
        let instance = unsafe { entry.create_instance(&create_info, None) }.map_err(|result| {
            format!(
                "failed to create Vulkan {} instance: {result:?}",
                format_api_version(api_version)
            )
        })?;
        let debug_utils = match validation_counters {
            Some(counters) => match create_debug_utils_state(&entry, &instance, counters) {
                Ok(state) => Some(state),
                Err(error) => {
                    unsafe { instance.destroy_instance(None) };
                    return Err(error);
                }
            },
            None => None,
        };

        Ok(Arc::new(Self {
            entry,
            instance,
            debug_utils,
            api_version,
            enabled_extensions,
        }))
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    pub fn raw(&self) -> &Instance {
        &self.instance
    }

    pub fn api_version(&self) -> u32 {
        self.api_version
    }

    pub fn enabled_extensions(&self) -> &[String] {
        &self.enabled_extensions
    }

    pub fn validation_report(&self) -> VulkanValidationReport {
        self.debug_utils
            .as_ref()
            .map(|debug| VulkanValidationReport {
                enabled: true,
                errors: debug.counters.errors.load(Ordering::Relaxed),
                warnings: debug.counters.warnings.load(Ordering::Relaxed),
            })
            .unwrap_or_default()
    }
}

impl Drop for VulkanInstance {
    fn drop(&mut self) {
        if let Some(debug) = self.debug_utils.take() {
            let report = VulkanValidationReport {
                enabled: true,
                errors: debug.counters.errors.load(Ordering::Relaxed),
                warnings: debug.counters.warnings.load(Ordering::Relaxed),
            };
            eprintln!(
                "Vulkan validation final count: errors={} warnings={}",
                report.errors, report.warnings
            );
            unsafe {
                debug
                    .loader
                    .destroy_debug_utils_messenger(debug.messenger, None)
            };
        }
        // SAFETY: all child devices and surfaces hold an Arc to this owner, so the instance is the
        // final object destroyed from this ownership tree.
        unsafe { self.instance.destroy_instance(None) };
    }
}

fn validation_requested_from_values(
    emerge_validation: Option<&str>,
    instance_layers: Option<&str>,
) -> bool {
    emerge_validation.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }) || instance_layers.is_some_and(|layers| {
        layers
            .split([':', ',', ';'])
            .any(|layer| layer.trim() == "VK_LAYER_KHRONOS_validation")
    })
}

fn debug_messenger_create_info(
    counters: &ValidationCounters,
) -> vk::DebugUtilsMessengerCreateInfoEXT<'_> {
    let user_data = counters as *const ValidationCounters as *mut c_void;
    vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(vulkan_debug_callback))
        .user_data(user_data)
}

fn create_debug_utils_state(
    entry: &Entry,
    instance: &Instance,
    counters: Box<ValidationCounters>,
) -> Result<DebugUtilsState, String> {
    let create_info = debug_messenger_create_info(&counters);
    let loader = ash::ext::debug_utils::Instance::new(entry, instance);
    let messenger = unsafe { loader.create_debug_utils_messenger(&create_info, None) }
        .map_err(|result| format!("failed to create Vulkan debug-utils messenger: {result:?}"))?;
    Ok(DebugUtilsState {
        loader,
        messenger,
        counters,
    })
}

unsafe extern "system" fn vulkan_debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    user_data: *mut c_void,
) -> vk::Bool32 {
    if !user_data.is_null() {
        // SAFETY: the messenger's Box-owned counters outlive every callback and remain stationary.
        let counters = unsafe { &*(user_data as *const ValidationCounters) };
        if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
            counters.errors.fetch_add(1, Ordering::Relaxed);
        } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
            counters.warnings.fetch_add(1, Ordering::Relaxed);
        }
    }
    let message = if callback_data.is_null() {
        "Vulkan validation callback had no message data".into()
    } else {
        // SAFETY: callback data and message are provided by Vulkan for this callback invocation.
        let message = unsafe { (*callback_data).p_message };
        if message.is_null() {
            "Vulkan validation callback had no message".into()
        } else {
            // SAFETY: Vulkan validation messages are NUL-terminated for this callback invocation.
            unsafe { CStr::from_ptr(message) }.to_string_lossy()
        }
    };
    eprintln!(
        "Vulkan validation severity={:#x} types={:#x}: {}",
        severity.as_raw(),
        message_types.as_raw(),
        message
    );
    vk::FALSE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_request_is_explicit_or_layer_driven() {
        assert!(validation_requested_from_values(Some("true"), None));
        assert!(validation_requested_from_values(
            None,
            Some("other:VK_LAYER_KHRONOS_validation")
        ));
        assert!(!validation_requested_from_values(Some("false"), None));
        assert!(!validation_requested_from_values(None, Some("other")));
    }
}
