use std::collections::BTreeMap;
use std::env;
use std::ffi::c_char;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use ash::{Entry, vk};
use drm::Device as _;
use drm::control::Device as _;
use drm::node::NodeType;
use emerge_skia::backend::drm::functional_probe::{
    self, AllocationDirection, FunctionalProbeConfig,
};
use emerge_skia::backend::vulkan::capabilities::{
    DrmMatchField, DrmNodeId, EXT_BIND_MEMORY_2, EXT_DEDICATED_ALLOCATION, EXT_EXTERNAL_MEMORY,
    EXT_EXTERNAL_MEMORY_CAPABILITIES, EXT_EXTERNAL_MEMORY_DMA_BUF, EXT_EXTERNAL_MEMORY_FD,
    EXT_EXTERNAL_SEMAPHORE, EXT_EXTERNAL_SEMAPHORE_CAPABILITIES, EXT_EXTERNAL_SEMAPHORE_FD,
    EXT_GET_MEMORY_REQUIREMENTS_2, EXT_GET_PHYSICAL_DEVICE_PROPERTIES_2,
    EXT_IMAGE_DRM_FORMAT_MODIFIER, EXT_IMAGE_FORMAT_LIST, EXT_PHYSICAL_DEVICE_DRM,
    EXT_QUEUE_FAMILY_FOREIGN, EXT_SAMPLER_YCBCR_CONVERSION, SelectionCandidate, SelectionNode,
    SupportSource, api_at_least, classify_software_device, driver_properties_source, effective_api,
    extension_map, format_api_version, properties2_available, select_graphics_queue_family,
    select_instance_api, select_matching_device, support_source,
};

const DEFAULT_DRM_CARD: &str = "/dev/dri/card0";
const MAX_DRM_FORMAT_MODIFIERS: u32 = 4_096;
const USAGE: &str = "usage: vulkan_probe [--drm-card PATH] [--vulkan-drm-node PATH] [--require-v3dv] [--functional [--allocation-direction gbm-import|vulkan-export] [--page-flip-timeout-ms N] [--validation]]";

const OUTPUT_FORMAT_NAME: &str = "B8G8R8A8_UNORM";
const CAMERA_FORMAT_NAME: &str = "G8_B8R8_2PLANE_420_UNORM";

fn main() -> ExitCode {
    match parse_args(env::args_os().skip(1)) {
        Ok(ParseResult::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(ParseResult::Run(config)) => match run_probe(&config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(errors) => {
                report_errors(&errors);
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            println!("probe.schema=1");
            report_errors(&[ProbeError::new("arguments", "invalid_arguments", error)]);
            ExitCode::from(2)
        }
    }
}

#[derive(Debug)]
struct Config {
    drm_card: PathBuf,
    vulkan_drm_node: PathBuf,
    vulkan_drm_node_explicit: bool,
    require_v3dv: bool,
    functional: bool,
    allocation_direction: AllocationDirection,
    allocation_direction_explicit: bool,
    page_flip_timeout_ms: u64,
    validation: bool,
}

#[derive(Debug)]
enum ParseResult {
    Help,
    Run(Config),
}

#[derive(Debug)]
struct ProbeError {
    stage: &'static str,
    code: &'static str,
    raw_result: Option<String>,
    message: String,
    inventory_passed: bool,
}

impl ProbeError {
    fn new(stage: &'static str, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            code,
            raw_result: None,
            message: message.into(),
            inventory_passed: false,
        }
    }

    fn after_inventory(
        stage: &'static str,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            code,
            raw_result: None,
            message: message.into(),
            inventory_passed: true,
        }
    }

    fn vulkan(
        stage: &'static str,
        code: &'static str,
        operation: &'static str,
        result: vk::Result,
    ) -> Self {
        Self {
            stage,
            code,
            raw_result: Some(result.as_raw().to_string()),
            message: format!("{operation} failed: {result:?}"),
            inventory_passed: false,
        }
    }
}

fn parse_args(args: impl Iterator<Item = std::ffi::OsString>) -> Result<ParseResult, String> {
    let mut drm_card = None;
    let mut vulkan_drm_node = None;
    let mut require_v3dv = false;
    let mut functional = false;
    let mut allocation_direction = None;
    let mut page_flip_timeout_ms = None;
    let mut validation = false;
    let mut args = args.peekable();

    while let Some(argument) = args.next() {
        if argument == "--help" || argument == "-h" {
            return Ok(ParseResult::Help);
        }
        if argument == "--drm-card"
            || argument == "--vulkan-drm-node"
            || argument == "--allocation-direction"
            || argument == "--page-flip-timeout-ms"
        {
            if argument == "--allocation-direction" {
                if allocation_direction.is_some() {
                    return Err("--allocation-direction may only be specified once".to_string());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "--allocation-direction requires a value".to_string())?;
                allocation_direction = Some(AllocationDirection::parse(&value.to_string_lossy())?);
                continue;
            }
            if argument == "--page-flip-timeout-ms" {
                if page_flip_timeout_ms.is_some() {
                    return Err("--page-flip-timeout-ms may only be specified once".to_string());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "--page-flip-timeout-ms requires a value".to_string())?;
                let value = value
                    .to_string_lossy()
                    .parse::<u64>()
                    .map_err(|_| "--page-flip-timeout-ms must be a positive integer".to_string())?;
                if value == 0 {
                    return Err("--page-flip-timeout-ms must be a positive integer".to_string());
                }
                page_flip_timeout_ms = Some(value);
                continue;
            }
            let slot = if argument == "--drm-card" {
                &mut drm_card
            } else {
                &mut vulkan_drm_node
            };
            if slot.is_some() {
                return Err(format!(
                    "{} may only be specified once",
                    argument.to_string_lossy()
                ));
            }
            let option = argument.to_string_lossy();
            let path = args
                .next()
                .ok_or_else(|| format!("{option} requires a path"))?;
            if path.is_empty() {
                return Err(format!("{option} requires a non-empty path"));
            }
            *slot = Some(PathBuf::from(path));
            continue;
        }
        if argument == "--require-v3dv" {
            if require_v3dv {
                return Err("--require-v3dv may only be specified once".to_string());
            }
            require_v3dv = true;
            continue;
        }
        if argument == "--functional" {
            if functional {
                return Err("--functional may only be specified once".to_string());
            }
            functional = true;
            continue;
        }
        if argument == "--validation" {
            if validation {
                return Err("--validation may only be specified once".to_string());
            }
            validation = true;
            continue;
        }
        return Err(format!(
            "unexpected argument '{}'; {USAGE}",
            escape_value(&argument.to_string_lossy())
        ));
    }

    let drm_card = drm_card.unwrap_or_else(|| PathBuf::from(DEFAULT_DRM_CARD));
    let vulkan_drm_node_explicit = vulkan_drm_node.is_some();
    if !functional
        && (allocation_direction.is_some() || page_flip_timeout_ms.is_some() || validation)
    {
        return Err(
            "--allocation-direction, --page-flip-timeout-ms, and --validation require --functional"
                .to_string(),
        );
    }
    if functional && !vulkan_drm_node_explicit {
        return Err(
            "--functional requires an explicit --vulkan-drm-node; split-device identity must not be inferred"
                .to_string(),
        );
    }
    if functional && allocation_direction.is_none() {
        return Err(
            "--functional requires an explicit --allocation-direction; allocation policy must not be inferred"
                .to_string(),
        );
    }
    Ok(ParseResult::Run(Config {
        vulkan_drm_node: vulkan_drm_node.unwrap_or_else(|| drm_card.clone()),
        drm_card,
        vulkan_drm_node_explicit,
        require_v3dv,
        functional,
        allocation_direction: allocation_direction
            .unwrap_or(AllocationDirection::GbmImportedIntoVulkan),
        allocation_direction_explicit: allocation_direction.is_some(),
        page_flip_timeout_ms: page_flip_timeout_ms.unwrap_or(3_000),
        validation,
    }))
}

fn run_probe(config: &Config) -> Result<(), Vec<ProbeError>> {
    println!("probe.schema=1");
    println!(
        "probe.scope={}",
        if config.functional {
            "functional_no_wsi_drm_kms"
        } else {
            "static_inventory_preflight"
        }
    );
    println!("probe.wsi_enabled=false");
    println!(
        "probe.drm_card={}",
        escape_value(&config.drm_card.to_string_lossy())
    );
    println!(
        "probe.vulkan_drm_node={}",
        escape_value(&config.vulkan_drm_node.to_string_lossy())
    );
    println!(
        "probe.vulkan_drm_node_explicit={}",
        config.vulkan_drm_node_explicit
    );
    println!("probe.require_v3dv={}", config.require_v3dv);
    println!("probe.functional={}", config.functional);
    println!(
        "probe.allocation_direction={}",
        config.allocation_direction.as_str()
    );
    println!(
        "probe.allocation_direction_explicit={}",
        config.allocation_direction_explicit
    );
    println!("probe.page_flip_timeout_ms={}", config.page_flip_timeout_ms);
    println!("probe.validation_requested={}", config.validation);

    let (drm_card, drm_resources) = open_drm_card(&config.drm_card).map_err(|error| {
        vec![ProbeError::new(
            "kms_drm_card",
            "kms_drm_card_invalid",
            error,
        )]
    })?;
    report_drm_card(&drm_card, &drm_resources);

    let vulkan_drm_node = open_vulkan_drm_node(&config.vulkan_drm_node).map_err(|error| {
        vec![ProbeError::new(
            "vulkan_drm_node",
            "vulkan_drm_node_invalid",
            error,
        )]
    })?;
    report_vulkan_drm_node(&vulkan_drm_node);

    // SAFETY: ash loads the Vulkan loader through libloading and owns the resulting function table.
    let entry = unsafe { Entry::load() }.map_err(|error| {
        vec![ProbeError::new(
            "loader",
            "loader_unavailable",
            format!("failed to dynamically load the Vulkan loader: {error}"),
        )]
    })?;
    println!("loader.available=true");

    // SAFETY: this is a read-only loader version query with no borrowed output storage.
    let loader_api_version = unsafe { entry.try_enumerate_instance_version() }
        .map_err(|error| {
            vec![ProbeError::vulkan(
                "loader",
                "instance_version_query_failed",
                "vkEnumerateInstanceVersion",
                error,
            )]
        })?
        .unwrap_or(vk::API_VERSION_1_0);
    println!(
        "loader.api_version={}",
        format_api_version(loader_api_version)
    );
    println!(
        "loader.api_variant={}",
        vk::api_version_variant(loader_api_version)
    );

    // SAFETY: ash performs the count/data enumeration and returns owned extension properties.
    let instance_extension_properties =
        unsafe { entry.enumerate_instance_extension_properties(None) }.map_err(|error| {
            vec![ProbeError::vulkan(
                "instance_extensions",
                "instance_extension_enumeration_failed",
                "vkEnumerateInstanceExtensionProperties",
                error,
            )]
        })?;
    let instance_extensions = extension_map(&instance_extension_properties);
    report_extension_map("instance", &instance_extensions);

    let instance_api = select_instance_api(loader_api_version)
        .map_err(|error| vec![ProbeError::new("loader", "loader_api_too_old", error)])?;
    report_instance_prerequisites(instance_api, &instance_extensions);

    let application_info = vk::ApplicationInfo::default()
        .application_name(c"emerge-vulkan-probe")
        .application_version(1)
        .engine_name(c"emerge")
        .engine_version(1)
        .api_version(instance_api);
    let create_info = vk::InstanceCreateInfo::default().application_info(&application_info);
    // SAFETY: create_info points to data that remains alive for the call; no extensions or layers
    // are enabled, and InstanceOwner destroys a successfully created instance.
    let instance = unsafe { entry.create_instance(&create_info, None) }.map_err(|error| {
        vec![ProbeError::vulkan(
            "instance_create",
            "instance_create_failed",
            "vkCreateInstance",
            error,
        )]
    })?;
    let instance = InstanceOwner(instance);
    println!("instance.created=true");
    println!("instance.api_version={}", format_api_version(instance_api));
    println!("instance.enabled_extensions=");

    // SAFETY: the instance is live and ash returns owned handles.
    let physical_devices = unsafe { instance.0.enumerate_physical_devices() }.map_err(|error| {
        vec![ProbeError::vulkan(
            "physical_device_enumeration",
            "physical_device_enumeration_failed",
            "vkEnumeratePhysicalDevices",
            error,
        )]
    })?;
    println!("physical_devices.count={}", physical_devices.len());

    let mut records = Vec::with_capacity(physical_devices.len());
    let mut enumeration_errors = Vec::new();
    for (index, handle) in physical_devices.into_iter().enumerate() {
        match inspect_device(&instance.0, instance_api, index, handle) {
            Ok(record) => records.push(record),
            Err(error) => enumeration_errors.push(ProbeError::new(
                "physical_device_inspection",
                "physical_device_inspection_failed",
                error,
            )),
        }
    }

    for record in &records {
        report_device(record, vulkan_drm_node.selection());
    }
    if !enumeration_errors.is_empty() {
        return Err(enumeration_errors);
    }

    let candidates = records
        .iter()
        .map(DeviceRecord::selection_candidate)
        .collect::<Vec<_>>();
    let selection_node = vulkan_drm_node.selection();
    let selected_index = select_matching_device(selection_node, &candidates).map_err(|error| {
        vec![ProbeError::new(
            "physical_device_selection",
            "no_unique_hardware_drm_match",
            error,
        )]
    })?;
    let selected = records
        .iter()
        .find(|record| record.index == selected_index)
        .ok_or_else(|| {
            vec![ProbeError::new(
                "physical_device_selection",
                "invalid_internal_selection",
                "internal selection result did not name an enumerated device",
            )]
        })?;

    println!("selected.index={}", selected.index);
    report_selected_identity(selected, selection_node);

    let mut capability_errors = Vec::new();
    probe_selected_capabilities(
        &instance.0,
        selected,
        selection_node,
        config.require_v3dv,
        &mut capability_errors,
    );

    if !capability_errors.is_empty() {
        return Err(capability_errors);
    }
    if !config.functional {
        report_inventory_success();
        return Ok(());
    }
    validate_functional_environment(config)?;

    let fd_before = process_fd_count();
    let rss_before = process_rss_kib();
    println!("probe.resources.fd_before={}", optional_usize(fd_before));
    println!(
        "probe.resources.rss_kib_before={}",
        optional_u64(rss_before)
    );
    let functional_config = FunctionalProbeConfig {
        drm_card: config.drm_card.to_string_lossy().into_owned(),
        vulkan_drm_node: config.vulkan_drm_node.to_string_lossy().into_owned(),
        requested_size: None,
        allocation_direction: config.allocation_direction,
        page_flip_timeout: Duration::from_millis(config.page_flip_timeout_ms),
    };
    let report = match functional_probe::run(&functional_config) {
        Ok(report) => report,
        Err(error) => {
            let fd_after = process_fd_count();
            let rss_after = process_rss_kib();
            report_resource_after(fd_before, fd_after, rss_before, rss_after);
            return Err(vec![ProbeError::after_inventory(
                "functional_drm_kms",
                "functional_probe_failed",
                error,
            )]);
        }
    };
    let fd_after = process_fd_count();
    let rss_after = process_rss_kib();
    report_functional_success(config, &report, fd_before, fd_after, rss_before, rss_after);
    Ok(())
}

struct InstanceOwner(ash::Instance);

impl Drop for InstanceOwner {
    fn drop(&mut self) {
        // SAFETY: this owner uniquely destroys its live instance after all instance queries finish.
        unsafe { self.0.destroy_instance(None) };
    }
}

struct DrmCard {
    file: File,
    node: DrmNodeId,
    driver: drm::Driver,
}

impl AsFd for DrmCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl drm::Device for DrmCard {}
impl drm::control::Device for DrmCard {}

struct VulkanDrmNode {
    file: File,
    node: DrmNodeId,
    match_field: DrmMatchField,
    driver: drm::Driver,
}

impl AsFd for VulkanDrmNode {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl drm::Device for VulkanDrmNode {}

impl VulkanDrmNode {
    fn selection(&self) -> SelectionNode {
        SelectionNode {
            node: self.node,
            field: self.match_field,
        }
    }
}

fn open_drm_card(path: &Path) -> Result<(DrmCard, drm::control::ResourceHandles), String> {
    let file = open_and_stat_character_device(path, "KMS DRM card")?;
    let drm_node = drm::node::DrmNode::from_file(&file).map_err(|error| {
        format!(
            "configured KMS DRM card '{}' is not a DRM device node: {error}",
            path.display()
        )
    })?;
    if drm_node.ty() != NodeType::Primary {
        return Err(format!(
            "configured KMS DRM card '{}' is a {:?} DRM node, not a primary modeset node",
            path.display(),
            drm_node.ty()
        ));
    }

    let mut card = DrmCard {
        file,
        node: DrmNodeId {
            major: drm_node.major(),
            minor: drm_node.minor(),
        },
        driver: empty_drm_driver(),
    };
    card.driver = card.get_driver().map_err(|error| {
        format!(
            "configured KMS DRM card '{}' does not support DRM driver queries: {error}",
            path.display()
        )
    })?;
    let resources = card.resource_handles().map_err(|error| {
        format!(
            "configured KMS DRM card '{}' does not expose usable DRM control resources: {error}",
            path.display()
        )
    })?;
    Ok((card, resources))
}

fn open_vulkan_drm_node(path: &Path) -> Result<VulkanDrmNode, String> {
    let file = open_and_stat_character_device(path, "Vulkan DRM selection node")?;
    let drm_node = drm::node::DrmNode::from_file(&file).map_err(|error| {
        format!(
            "configured Vulkan DRM selection node '{}' is not a DRM device node: {error}",
            path.display()
        )
    })?;
    let match_field = match drm_node.ty() {
        NodeType::Primary => DrmMatchField::Primary,
        NodeType::Render => DrmMatchField::Render,
        NodeType::Control => {
            return Err(format!(
                "configured Vulkan DRM selection node '{}' is a control node; a primary or render node is required",
                path.display()
            ));
        }
    };
    let mut node = VulkanDrmNode {
        file,
        node: DrmNodeId {
            major: drm_node.major(),
            minor: drm_node.minor(),
        },
        match_field,
        driver: empty_drm_driver(),
    };
    node.driver = node.get_driver().map_err(|error| {
        format!(
            "configured Vulkan DRM selection node '{}' does not support DRM driver queries: {error}",
            path.display()
        )
    })?;
    Ok(node)
}

fn open_and_stat_character_device(path: &Path, role: &str) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("failed to open {role} '{}': {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to stat opened {role} '{}': {error}", path.display()))?;
    if !metadata.file_type().is_char_device() {
        return Err(format!(
            "configured {role} '{}' is not a character device",
            path.display()
        ));
    }
    Ok(file)
}

fn empty_drm_driver() -> drm::Driver {
    drm::Driver {
        version: (0, 0, 0),
        name: std::ffi::OsString::new(),
        date: std::ffi::OsString::new(),
        desc: std::ffi::OsString::new(),
    }
}

fn report_drm_card(card: &DrmCard, resources: &drm::control::ResourceHandles) {
    println!("drm.kms.opened=true");
    println!("drm.kms.control_resources_validated=true");
    println!("drm.kms.primary.major={}", card.node.major);
    println!("drm.kms.primary.minor={}", card.node.minor);
    println!(
        "drm.kms.resources.connector_count={}",
        resources.connectors().len()
    );
    println!("drm.kms.resources.crtc_count={}", resources.crtcs().len());
    println!(
        "drm.kms.resources.encoder_count={}",
        resources.encoders().len()
    );
    report_drm_driver("drm.kms", &card.driver);
}

fn report_vulkan_drm_node(node: &VulkanDrmNode) {
    println!("drm.vulkan.opened=true");
    println!("drm.vulkan.node_type={}", node.match_field.as_str());
    println!("drm.vulkan.major={}", node.node.major);
    println!("drm.vulkan.minor={}", node.node.minor);
    report_drm_driver("drm.vulkan", &node.driver);
}

fn report_drm_driver(prefix: &str, driver: &drm::Driver) {
    println!(
        "{prefix}.kernel_driver.name={}",
        escape_value(&driver.name.to_string_lossy())
    );
    println!(
        "{prefix}.kernel_driver.version={}.{}.{}",
        driver.version.0, driver.version.1, driver.version.2
    );
    println!(
        "{prefix}.kernel_driver.description={}",
        escape_value(&driver.desc.to_string_lossy())
    );
}

struct DeviceRecord {
    index: usize,
    handle: vk::PhysicalDevice,
    name: String,
    device_type: vk::PhysicalDeviceType,
    api_version: u32,
    effective_api_version: u32,
    driver_version: u32,
    vendor_id: u32,
    device_id: u32,
    device_uuid: Option<[u8; vk::UUID_SIZE]>,
    driver_uuid: Option<[u8; vk::UUID_SIZE]>,
    driver_id: Option<vk::DriverId>,
    driver_name: Option<String>,
    driver_info: Option<String>,
    drm: Option<DrmProperties>,
    extensions: BTreeMap<String, u32>,
    queue_families: Vec<vk::QueueFamilyProperties>,
    timestamp_period: f32,
    software: bool,
    software_reason: Option<&'static str>,
    properties2_queried: bool,
}

#[derive(Clone, Copy)]
struct DrmProperties {
    has_primary: bool,
    primary_major: i64,
    primary_minor: i64,
    has_render: bool,
    render_major: i64,
    render_minor: i64,
}

impl DrmProperties {
    fn node(self, field: DrmMatchField) -> Option<DrmNodeId> {
        let (available, major, minor) = match field {
            DrmMatchField::Primary => (self.has_primary, self.primary_major, self.primary_minor),
            DrmMatchField::Render => (self.has_render, self.render_major, self.render_minor),
        };
        if !available {
            return None;
        }
        Some(DrmNodeId {
            major: u32::try_from(major).ok()?,
            minor: u32::try_from(minor).ok()?,
        })
    }
}

impl DeviceRecord {
    fn selection_candidate(&self) -> SelectionCandidate {
        SelectionCandidate {
            index: self.index,
            name: self.name.clone(),
            primary: self
                .drm
                .and_then(|properties| properties.node(DrmMatchField::Primary)),
            render: self
                .drm
                .and_then(|properties| properties.node(DrmMatchField::Render)),
            software: self.software,
            api_eligible: api_at_least(self.api_version, 1, 1),
        }
    }
}

fn inspect_device(
    instance: &ash::Instance,
    instance_api: u32,
    index: usize,
    handle: vk::PhysicalDevice,
) -> Result<DeviceRecord, String> {
    // SAFETY: handle was returned by this live instance and the call writes a fixed-size value.
    let base = unsafe { instance.get_physical_device_properties(handle) };
    // SAFETY: handle is live and ash performs the count/data enumeration into owned storage.
    let extension_properties = unsafe { instance.enumerate_device_extension_properties(handle) }
        .map_err(|error| {
            format!("device {index} vkEnumerateDeviceExtensionProperties failed: {error:?}")
        })?;
    let extensions = extension_map(&extension_properties);

    let effective_api_version = effective_api(instance_api, base.api_version);
    let properties2_queried = properties2_available(instance_api, base.api_version);
    let supports_id = properties2_queried;
    let supports_drm = properties2_queried && extensions.contains_key(EXT_PHYSICAL_DEVICE_DRM);
    let supports_driver = properties2_queried
        && driver_properties_source(instance_api, base.api_version, &extensions).is_supported();

    let mut id_properties = vk::PhysicalDeviceIDProperties::default();
    let mut drm_properties = vk::PhysicalDeviceDrmPropertiesEXT::default();
    let mut driver_properties = vk::PhysicalDeviceDriverProperties::default();
    if properties2_queried {
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut id_properties);
        if supports_drm {
            properties = properties.push_next(&mut drm_properties);
        }
        if supports_driver {
            properties = properties.push_next(&mut driver_properties);
        }
        // SAFETY: the instance/device effective API exposes properties2; extension-only pNext
        // structures are appended only when advertised, and output storage remains alive.
        unsafe { instance.get_physical_device_properties2(handle, &mut properties) };
    }

    // SAFETY: handle is live and ash returns owned queue-family property values.
    let queue_families = unsafe { instance.get_physical_device_queue_family_properties(handle) };
    let name = char_array_to_string(&base.device_name);
    let driver_id = supports_driver.then_some(driver_properties.driver_id);
    let (software, software_reason) =
        classify_software_device(base.device_type, &name, driver_id.map(vk::DriverId::as_raw));

    Ok(DeviceRecord {
        index,
        handle,
        name,
        device_type: base.device_type,
        api_version: base.api_version,
        effective_api_version,
        driver_version: base.driver_version,
        vendor_id: base.vendor_id,
        device_id: base.device_id,
        device_uuid: supports_id.then_some(id_properties.device_uuid),
        driver_uuid: supports_id.then_some(id_properties.driver_uuid),
        driver_id,
        driver_name: supports_driver.then(|| char_array_to_string(&driver_properties.driver_name)),
        driver_info: supports_driver.then(|| char_array_to_string(&driver_properties.driver_info)),
        drm: supports_drm.then_some(DrmProperties {
            has_primary: drm_properties.has_primary == vk::TRUE,
            primary_major: drm_properties.primary_major,
            primary_minor: drm_properties.primary_minor,
            has_render: drm_properties.has_render == vk::TRUE,
            render_major: drm_properties.render_major,
            render_minor: drm_properties.render_minor,
        }),
        extensions,
        queue_families,
        timestamp_period: base.limits.timestamp_period,
        software,
        software_reason,
        properties2_queried,
    })
}

fn report_device(record: &DeviceRecord, selection_node: SelectionNode) {
    let prefix = format!("device.{}", record.index);
    println!("{prefix}.name={}", escape_value(&record.name));
    println!("{prefix}.type={}", device_type_name(record.device_type));
    println!(
        "{prefix}.api_version={}",
        format_api_version(record.api_version)
    );
    println!(
        "{prefix}.effective_api_version={}",
        format_api_version(record.effective_api_version)
    );
    println!(
        "{prefix}.driver_version_raw=0x{:08x}",
        record.driver_version
    );
    println!("{prefix}.vendor_id=0x{:04x}", record.vendor_id);
    println!("{prefix}.device_id=0x{:04x}", record.device_id);
    println!("{prefix}.software={}", record.software);
    println!(
        "{prefix}.software_reason={}",
        record.software_reason.unwrap_or("none")
    );
    println!(
        "{prefix}.vulkan_1_1_eligible={}",
        api_at_least(record.api_version, 1, 1)
    );
    println!(
        "{prefix}.properties2_queried={}",
        record.properties2_queried
    );
    println!(
        "{prefix}.device_uuid={}",
        optional_uuid(record.device_uuid.as_ref())
    );
    println!(
        "{prefix}.driver_uuid={}",
        optional_uuid(record.driver_uuid.as_ref())
    );
    println!(
        "{prefix}.driver_id={}",
        record
            .driver_id
            .map(driver_id_name)
            .unwrap_or("unavailable")
    );
    println!(
        "{prefix}.driver_id_raw={}",
        record
            .driver_id
            .map(|id| id.as_raw().to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{prefix}.driver_name={}",
        record
            .driver_name
            .as_deref()
            .map(escape_value)
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "{prefix}.driver_info={}",
        record
            .driver_info
            .as_deref()
            .map(escape_value)
            .unwrap_or_else(|| "unavailable".to_string())
    );
    report_device_drm(&prefix, record.drm, selection_node);
    report_extension_map(&prefix, &record.extensions);

    let graphics = record
        .queue_families
        .iter()
        .enumerate()
        .filter(|(_, family)| family.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .collect::<Vec<_>>();
    println!("{prefix}.graphics_queue_family_count={}", graphics.len());
    for (reported_index, (queue_index, family)) in graphics.into_iter().enumerate() {
        println!("{prefix}.graphics_queue_family.{reported_index}.index={queue_index}");
        println!(
            "{prefix}.graphics_queue_family.{reported_index}.queue_count={}",
            family.queue_count
        );
        println!(
            "{prefix}.graphics_queue_family.{reported_index}.queue_flags=0x{:08x}",
            family.queue_flags.as_raw()
        );
        println!(
            "{prefix}.graphics_queue_family.{reported_index}.timestamp_valid_bits={}",
            family.timestamp_valid_bits
        );
    }
    println!("{prefix}.timestamp_period_ns={}", record.timestamp_period);
}

fn report_device_drm(prefix: &str, drm: Option<DrmProperties>, selection_node: SelectionNode) {
    let Some(drm) = drm else {
        println!("{prefix}.drm.extension_exposed=false");
        println!("{prefix}.drm.has_primary=false");
        println!("{prefix}.drm.has_render=false");
        println!("{prefix}.drm.primary_matches_selection_node=false");
        println!("{prefix}.drm.render_matches_selection_node=false");
        return;
    };
    println!("{prefix}.drm.extension_exposed=true");
    println!("{prefix}.drm.has_primary={}", drm.has_primary);
    println!("{prefix}.drm.primary_major={}", drm.primary_major);
    println!("{prefix}.drm.primary_minor={}", drm.primary_minor);
    println!("{prefix}.drm.has_render={}", drm.has_render);
    println!("{prefix}.drm.render_major={}", drm.render_major);
    println!("{prefix}.drm.render_minor={}", drm.render_minor);
    println!(
        "{prefix}.drm.primary_matches_selection_node={}",
        selection_node.field == DrmMatchField::Primary
            && drm.node(DrmMatchField::Primary) == Some(selection_node.node)
    );
    println!(
        "{prefix}.drm.render_matches_selection_node={}",
        selection_node.field == DrmMatchField::Render
            && drm.node(DrmMatchField::Render) == Some(selection_node.node)
    );
}

fn report_selected_identity(selected: &DeviceRecord, selection_node: SelectionNode) {
    println!("selected.name={}", escape_value(&selected.name));
    println!("selected.type={}", device_type_name(selected.device_type));
    println!(
        "selected.api_version={}",
        format_api_version(selected.api_version)
    );
    println!(
        "selected.effective_api_version={}",
        format_api_version(selected.effective_api_version)
    );
    println!(
        "selected.driver_version_raw=0x{:08x}",
        selected.driver_version
    );
    println!(
        "selected.device_uuid={}",
        optional_uuid(selected.device_uuid.as_ref())
    );
    println!(
        "selected.driver_uuid={}",
        optional_uuid(selected.driver_uuid.as_ref())
    );
    println!(
        "selected.driver_id={}",
        selected
            .driver_id
            .map(driver_id_name)
            .unwrap_or("unavailable")
    );
    println!(
        "selected.driver_name={}",
        selected
            .driver_name
            .as_deref()
            .map(escape_value)
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "selected.driver_info={}",
        selected
            .driver_info
            .as_deref()
            .map(escape_value)
            .unwrap_or_else(|| "unavailable".to_string())
    );
    println!(
        "selected.driver_is_v3dv={}",
        is_provable_v3dv(selected.driver_id)
    );
    println!("selected.drm_match=true");
    println!("selected.drm_match_field={}", selection_node.field.as_str());
    println!("selected.drm_match_major={}", selection_node.node.major);
    println!("selected.drm_match_minor={}", selection_node.node.minor);
    if let Some(drm) = selected.drm {
        println!("selected.drm.has_primary={}", drm.has_primary);
        println!("selected.drm.primary_major={}", drm.primary_major);
        println!("selected.drm.primary_minor={}", drm.primary_minor);
        println!("selected.drm.has_render={}", drm.has_render);
        println!("selected.drm.render_major={}", drm.render_major);
        println!("selected.drm.render_minor={}", drm.render_minor);
    }
}

fn is_provable_v3dv(driver_id: Option<vk::DriverId>) -> bool {
    driver_id == Some(vk::DriverId::MESA_V3DV)
}

fn probe_selected_capabilities(
    instance: &ash::Instance,
    selected: &DeviceRecord,
    selection_node: SelectionNode,
    require_v3dv: bool,
    errors: &mut Vec<ProbeError>,
) {
    report_required_capability(
        "vulkan_1_1",
        api_at_least(selected.api_version, 1, 1),
        "device API is below Vulkan 1.1",
        errors,
    );
    report_conditionally_required_capability(
        "mesa_v3dv_driver_identity",
        require_v3dv,
        is_provable_v3dv(selected.driver_id),
        "--require-v3dv was set but the selected driver identity is not provably MESA_V3DV",
        errors,
    );

    let external_memory = report_promoted_device_capability(
        "external_memory",
        selected,
        EXT_EXTERNAL_MEMORY,
        Some((1, 1)),
        errors,
    );
    let bind_memory_2 = report_promoted_device_capability(
        "bind_memory_2",
        selected,
        EXT_BIND_MEMORY_2,
        Some((1, 1)),
        errors,
    );
    let memory_requirements_2 = report_promoted_device_capability(
        "get_memory_requirements_2",
        selected,
        EXT_GET_MEMORY_REQUIREMENTS_2,
        Some((1, 1)),
        errors,
    );
    let dedicated_allocation = report_promoted_device_capability(
        "dedicated_allocation",
        selected,
        EXT_DEDICATED_ALLOCATION,
        Some((1, 1)),
        errors,
    );
    let external_memory_fd = report_extension_capability(
        "external_memory_fd",
        selected,
        EXT_EXTERNAL_MEMORY_FD,
        errors,
    );
    let external_memory_dma_buf = report_extension_capability(
        "external_memory_dma_buf",
        selected,
        EXT_EXTERNAL_MEMORY_DMA_BUF,
        errors,
    );
    let drm_format_modifier = report_extension_capability(
        "image_drm_format_modifier",
        selected,
        EXT_IMAGE_DRM_FORMAT_MODIFIER,
        errors,
    );
    let image_format_list = report_promoted_device_capability(
        "image_format_list",
        selected,
        EXT_IMAGE_FORMAT_LIST,
        Some((1, 2)),
        errors,
    );
    let external_semaphore = report_promoted_device_capability(
        "external_semaphore",
        selected,
        EXT_EXTERNAL_SEMAPHORE,
        Some((1, 1)),
        errors,
    );
    let external_semaphore_fd = report_extension_capability(
        "external_semaphore_fd",
        selected,
        EXT_EXTERNAL_SEMAPHORE_FD,
        errors,
    );
    let sampler_ycbcr_api = report_promoted_device_capability(
        "sampler_ycbcr_conversion_api",
        selected,
        EXT_SAMPLER_YCBCR_CONVERSION,
        Some((1, 1)),
        errors,
    );
    report_diagnostic_extension_capability(
        "queue_family_foreign",
        selected,
        EXT_QUEUE_FAMILY_FOREIGN,
    );
    let drm_matching = report_extension_capability(
        "physical_device_drm",
        selected,
        EXT_PHYSICAL_DEVICE_DRM,
        errors,
    );
    let exact_node_match = selected
        .drm
        .and_then(|properties| properties.node(selection_node.field))
        == Some(selection_node.node);
    println!(
        "capability.physical_device_drm.exact_selection_node_match={}",
        drm_matching && exact_node_match
    );
    println!(
        "capability.physical_device_drm.match_field={}",
        selection_node.field.as_str()
    );

    let has_graphics_queue = selected.queue_families.iter().any(|family| {
        family.queue_count > 0 && family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
    });
    let selected_queue = select_graphics_queue_family(&selected.queue_families);
    report_required_capability(
        "graphics_queue",
        has_graphics_queue,
        "selected device exposes no non-empty graphics queue family",
        errors,
    );
    report_required_capability(
        "graphics_timestamps",
        selected_queue.is_some(),
        "selected device exposes no non-empty graphics queue with timestampValidBits greater than zero",
        errors,
    );
    println!("selected.graphics_queue_family.selection_policy=lowest_matching_index");
    if let Some(index) = selected_queue {
        let family = &selected.queue_families[index];
        println!("selected.graphics_queue_family.index={index}");
        println!(
            "selected.graphics_queue_family.queue_count={}",
            family.queue_count
        );
        println!(
            "selected.graphics_queue_family.queue_flags=0x{:08x}",
            family.queue_flags.as_raw()
        );
        println!(
            "selected.graphics_queue_family.timestamp_valid_bits={}",
            family.timestamp_valid_bits
        );
    } else {
        println!("selected.graphics_queue_family.index=unavailable");
        println!("selected.graphics_queue_family.queue_count=0");
        println!("selected.graphics_queue_family.queue_flags=0x00000000");
        println!("selected.graphics_queue_family.timestamp_valid_bits=0");
    }

    let sampler_feature = if sampler_ycbcr_api {
        let mut ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
        let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut ycbcr);
        // SAFETY: the feature structure is supported by core 1.1 or the advertised extension and
        // remains alive for the query.
        unsafe { instance.get_physical_device_features2(selected.handle, &mut features) };
        ycbcr.sampler_ycbcr_conversion == vk::TRUE
    } else {
        false
    };
    report_required_capability(
        "sampler_ycbcr_conversion_feature",
        sampler_feature,
        "samplerYcbcrConversion feature is unavailable",
        errors,
    );

    let sync_fd_features = if external_semaphore && external_semaphore_fd {
        let info = vk::PhysicalDeviceExternalSemaphoreInfo::default()
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        let mut properties = vk::ExternalSemaphoreProperties::default();
        // SAFETY: the selected handle is live, the sync-FD extension is advertised, and output is
        // fixed-size caller-owned storage.
        unsafe {
            instance.get_physical_device_external_semaphore_properties(
                selected.handle,
                &info,
                &mut properties,
            )
        };
        println!(
            "capability.external_semaphore_sync_fd.features=0x{:08x}",
            properties.external_semaphore_features.as_raw()
        );
        println!(
            "capability.external_semaphore_sync_fd.compatible_handle_types=0x{:08x}",
            properties.compatible_handle_types.as_raw()
        );
        properties.external_semaphore_features
    } else {
        println!("capability.external_semaphore_sync_fd.features=0x00000000");
        println!("capability.external_semaphore_sync_fd.compatible_handle_types=0x00000000");
        vk::ExternalSemaphoreFeatureFlags::empty()
    };
    report_required_capability(
        "external_semaphore_sync_fd_importable",
        sync_fd_features.contains(vk::ExternalSemaphoreFeatureFlags::IMPORTABLE),
        "SYNC_FD external semaphores are not importable",
        errors,
    );
    report_required_capability(
        "external_semaphore_sync_fd_exportable",
        sync_fd_features.contains(vk::ExternalSemaphoreFeatureFlags::EXPORTABLE),
        "SYNC_FD external semaphores are not exportable",
        errors,
    );

    let external_image_queries = external_memory
        && bind_memory_2
        && memory_requirements_2
        && dedicated_allocation
        && external_memory_fd
        && external_memory_dma_buf
        && drm_format_modifier
        && image_format_list;

    let output = query_format(
        instance,
        selected.handle,
        vk::Format::B8G8R8A8_UNORM,
        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        drm_format_modifier,
        external_image_queries,
    );
    match output {
        Ok(report) => {
            report_format("output", OUTPUT_FORMAT_NAME, &report);
            println!("format.output.scope=static_inventory_preflight");
            report_required_capability(
                "preflight_output_b8g8r8a8_drm_modifier_external_image",
                output_format_supported(&report),
                "static preflight found no B8G8R8A8_UNORM DRM modifier supporting COLOR_ATTACHMENT|TRANSFER_SRC and importable or exportable DMA_BUF memory",
                errors,
            );
        }
        Err(error) => {
            println!("format.output.query_error={}", escape_value(&error));
            println!("format.output.scope=static_inventory_preflight");
            report_required_capability(
                "preflight_output_b8g8r8a8_drm_modifier_external_image",
                false,
                &format!("B8G8R8A8_UNORM static format preflight failed: {error}"),
                errors,
            );
        }
    }

    let camera = query_format(
        instance,
        selected.handle,
        vk::Format::G8_B8R8_2PLANE_420_UNORM,
        vk::ImageUsageFlags::SAMPLED,
        drm_format_modifier,
        external_image_queries,
    );
    match camera {
        Ok(report) => {
            report_format("camera", CAMERA_FORMAT_NAME, &report);
            println!("format.camera.scope=static_inventory_preflight");
            report_required_capability(
                "preflight_camera_nv12_drm_modifier_sampled_import",
                camera_format_supported(sampler_feature, &report),
                "static preflight found no importable G8_B8R8_2PLANE_420_UNORM DMA_BUF DRM modifier with SAMPLED_IMAGE support and sampler YCbCr conversion",
                errors,
            );
        }
        Err(error) => {
            println!("format.camera.query_error={}", escape_value(&error));
            println!("format.camera.scope=static_inventory_preflight");
            report_required_capability(
                "preflight_camera_nv12_drm_modifier_sampled_import",
                false,
                &format!("G8_B8R8_2PLANE_420_UNORM static format preflight failed: {error}"),
                errors,
            );
        }
    }
}

fn report_promoted_device_capability(
    capability: &str,
    selected: &DeviceRecord,
    extension: &str,
    promoted_to: Option<(u32, u32)>,
    errors: &mut Vec<ProbeError>,
) -> bool {
    let source = support_source(
        selected.effective_api_version,
        &selected.extensions,
        extension,
        promoted_to,
    );
    let supported = !matches!(source, SupportSource::Missing);
    println!("capability.{capability}.extension={extension}");
    println!(
        "capability.{capability}.extension_available={}",
        selected.extensions.contains_key(extension)
    );
    println!("capability.{capability}.source={}", source.as_str());
    report_required_capability(
        capability,
        supported,
        &format!("{extension} is unavailable and is not provided by the selected device API core"),
        errors,
    );
    supported
}

fn report_extension_capability(
    capability: &str,
    selected: &DeviceRecord,
    extension: &str,
    errors: &mut Vec<ProbeError>,
) -> bool {
    let supported = selected.extensions.contains_key(extension);
    println!("capability.{capability}.extension={extension}");
    println!("capability.{capability}.extension_available={supported}");
    println!(
        "capability.{capability}.source={}",
        if supported { "extension" } else { "missing" }
    );
    report_required_capability(
        capability,
        supported,
        &format!("required device extension {extension} is unavailable"),
        errors,
    );
    supported
}

fn report_diagnostic_extension_capability(
    capability: &str,
    selected: &DeviceRecord,
    extension: &str,
) -> bool {
    let supported = selected.extensions.contains_key(extension);
    println!("capability.{capability}.extension={extension}");
    println!("capability.{capability}.extension_available={supported}");
    println!(
        "capability.{capability}.source={}",
        if supported { "extension" } else { "missing" }
    );
    println!("capability.{capability}.required=false");
    println!("capability.{capability}.supported={supported}");
    supported
}

fn report_required_capability(
    capability: &str,
    supported: bool,
    error: &str,
    errors: &mut Vec<ProbeError>,
) {
    report_conditionally_required_capability(capability, true, supported, error, errors);
}

fn report_conditionally_required_capability(
    capability: &str,
    required: bool,
    supported: bool,
    error: &str,
    errors: &mut Vec<ProbeError>,
) {
    println!("capability.{capability}.required={required}");
    println!("capability.{capability}.supported={supported}");
    if required && !supported {
        errors.push(ProbeError::new(
            "inventory_capabilities",
            "required_inventory_capability_missing",
            error,
        ));
    }
}

fn report_instance_prerequisites(instance_api_version: u32, extensions: &BTreeMap<String, u32>) {
    for (capability, extension) in [
        (
            "get_physical_device_properties_2",
            EXT_GET_PHYSICAL_DEVICE_PROPERTIES_2,
        ),
        (
            "external_memory_capabilities",
            EXT_EXTERNAL_MEMORY_CAPABILITIES,
        ),
        (
            "external_semaphore_capabilities",
            EXT_EXTERNAL_SEMAPHORE_CAPABILITIES,
        ),
    ] {
        let source = support_source(instance_api_version, extensions, extension, Some((1, 1)));
        println!("instance.capability.{capability}.extension={extension}");
        println!(
            "instance.capability.{capability}.extension_available={}",
            extensions.contains_key(extension)
        );
        println!(
            "instance.capability.{capability}.source={}",
            source.as_str()
        );
        println!(
            "instance.capability.{capability}.supported={}",
            !matches!(source, SupportSource::Missing)
        );
    }
}

struct FormatReport {
    linear_tiling_features: vk::FormatFeatureFlags,
    optimal_tiling_features: vk::FormatFeatureFlags,
    buffer_features: vk::FormatFeatureFlags,
    modifiers: Vec<ModifierReport>,
}

struct ModifierReport {
    modifier: u64,
    plane_count: u32,
    tiling_features: vk::FormatFeatureFlags,
    external: Option<ExternalImageSupport>,
    external_error: Option<String>,
}

struct ExternalImageSupport {
    features: vk::ExternalMemoryFeatureFlags,
    compatible_handle_types: vk::ExternalMemoryHandleTypeFlags,
    importable: bool,
    exportable: bool,
}

fn output_format_supported(report: &FormatReport) -> bool {
    let required_features =
        vk::FormatFeatureFlags::COLOR_ATTACHMENT | vk::FormatFeatureFlags::TRANSFER_SRC;
    report.modifiers.iter().any(|modifier| {
        modifier.tiling_features.contains(required_features)
            && modifier
                .external
                .as_ref()
                .is_some_and(|external| external.importable || external.exportable)
    })
}

fn camera_format_supported(sampler_ycbcr_conversion: bool, report: &FormatReport) -> bool {
    sampler_ycbcr_conversion
        && report.modifiers.iter().any(|modifier| {
            modifier
                .tiling_features
                .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
                && modifier
                    .external
                    .as_ref()
                    .is_some_and(|external| external.importable)
        })
}

fn query_format(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    query_modifiers: bool,
    query_external: bool,
) -> Result<FormatReport, String> {
    let mut basic_properties = vk::FormatProperties2::default();
    // SAFETY: the physical device is live and the output is fixed-size caller-owned storage.
    unsafe {
        instance.get_physical_device_format_properties2(
            physical_device,
            format,
            &mut basic_properties,
        )
    };

    let modifier_properties = if query_modifiers {
        let first_count = {
            let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
            let mut properties = vk::FormatProperties2::default().push_next(&mut list);
            // SAFETY: the DRM modifier extension is advertised; the first call supplies a null
            // property pointer and receives only the count.
            unsafe {
                instance.get_physical_device_format_properties2(
                    physical_device,
                    format,
                    &mut properties,
                )
            };
            list.drm_format_modifier_count
        };
        let allocation_count = bounded_modifier_count(first_count, MAX_DRM_FORMAT_MODIFIERS)?;
        let mut values = vec![vk::DrmFormatModifierPropertiesEXT::default(); allocation_count];
        if !values.is_empty() {
            let returned_count = {
                let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
                    .drm_format_modifier_properties(&mut values);
                let mut properties = vk::FormatProperties2::default().push_next(&mut list);
                // SAFETY: values is initialized and bounded; the input count is its exact capacity,
                // and Vulkan writes no more than that count.
                unsafe {
                    instance.get_physical_device_format_properties2(
                        physical_device,
                        format,
                        &mut properties,
                    )
                };
                list.drm_format_modifier_count
            };
            let returned_count = bounded_modifier_count(returned_count, MAX_DRM_FORMAT_MODIFIERS)?;
            if returned_count > values.len() {
                return Err(format!(
                    "DRM modifier count grew from {} to {} between count and data queries",
                    values.len(),
                    returned_count
                ));
            }
            values.truncate(returned_count);
        }
        values
    } else {
        Vec::new()
    };

    let mut modifiers = modifier_properties
        .into_iter()
        .map(|property| {
            let external_result = query_external.then(|| {
                query_external_image_support(
                    instance,
                    physical_device,
                    format,
                    usage,
                    property.drm_format_modifier,
                )
            });
            let (external, external_error) = match external_result {
                Some(Ok(support)) => (Some(support), None),
                Some(Err(error)) => (None, Some(error)),
                None => (None, None),
            };
            ModifierReport {
                modifier: property.drm_format_modifier,
                plane_count: property.drm_format_modifier_plane_count,
                tiling_features: property.drm_format_modifier_tiling_features,
                external,
                external_error,
            }
        })
        .collect::<Vec<_>>();
    modifiers.sort_by_key(|property| property.modifier);

    Ok(FormatReport {
        linear_tiling_features: basic_properties.format_properties.linear_tiling_features,
        optimal_tiling_features: basic_properties.format_properties.optimal_tiling_features,
        buffer_features: basic_properties.format_properties.buffer_features,
        modifiers,
    })
}

fn query_external_image_support(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    modifier: u64,
) -> Result<ExternalImageSupport, String> {
    let mut modifier_info = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
        .drm_format_modifier(modifier)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let mut external_info = vk::PhysicalDeviceExternalImageFormatInfo::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let format_info = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(format)
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(usage)
        .flags(vk::ImageCreateFlags::empty())
        .push_next(&mut modifier_info)
        .push_next(&mut external_info);
    let mut external_properties = vk::ExternalImageFormatProperties::default();
    let mut properties = vk::ImageFormatProperties2::default().push_next(&mut external_properties);
    // SAFETY: all queried extensions are advertised, pNext storage remains alive for the call, and
    // the selected modifier came from this device's format properties.
    unsafe {
        instance.get_physical_device_image_format_properties2(
            physical_device,
            &format_info,
            &mut properties,
        )
    }
    .map_err(|error| format!("{error:?}"))?;

    let memory = external_properties.external_memory_properties;
    Ok(ExternalImageSupport {
        features: memory.external_memory_features,
        compatible_handle_types: memory.compatible_handle_types,
        importable: memory
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
            && memory
                .compatible_handle_types
                .contains(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT),
        exportable: memory
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::EXPORTABLE)
            && memory
                .compatible_handle_types
                .contains(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT),
    })
}

fn report_format(role: &str, name: &str, report: &FormatReport) {
    println!("format.{role}.name={name}");
    println!(
        "format.{role}.linear_tiling_features=0x{:08x}",
        report.linear_tiling_features.as_raw()
    );
    println!(
        "format.{role}.optimal_tiling_features=0x{:08x}",
        report.optimal_tiling_features.as_raw()
    );
    println!(
        "format.{role}.buffer_features=0x{:08x}",
        report.buffer_features.as_raw()
    );
    println!("format.{role}.modifier_count={}", report.modifiers.len());
    for (index, modifier) in report.modifiers.iter().enumerate() {
        println!(
            "format.{role}.modifier.{index}.value=0x{:016x}",
            modifier.modifier
        );
        println!(
            "format.{role}.modifier.{index}.plane_count={}",
            modifier.plane_count
        );
        println!(
            "format.{role}.modifier.{index}.tiling_features=0x{:08x}",
            modifier.tiling_features.as_raw()
        );
        if let Some(external) = &modifier.external {
            println!("format.{role}.modifier.{index}.external_query=success");
            println!(
                "format.{role}.modifier.{index}.external_memory_features=0x{:08x}",
                external.features.as_raw()
            );
            println!(
                "format.{role}.modifier.{index}.compatible_handle_types=0x{:08x}",
                external.compatible_handle_types.as_raw()
            );
            println!(
                "format.{role}.modifier.{index}.dma_buf_importable={}",
                external.importable
            );
            println!(
                "format.{role}.modifier.{index}.dma_buf_exportable={}",
                external.exportable
            );
        } else if let Some(error) = &modifier.external_error {
            println!(
                "format.{role}.modifier.{index}.external_query={}",
                escape_value(error)
            );
            println!("format.{role}.modifier.{index}.dma_buf_importable=false");
            println!("format.{role}.modifier.{index}.dma_buf_exportable=false");
        } else {
            println!("format.{role}.modifier.{index}.external_query=not_available");
            println!("format.{role}.modifier.{index}.dma_buf_importable=false");
            println!("format.{role}.modifier.{index}.dma_buf_exportable=false");
        }
    }
}

fn bounded_modifier_count(count: u32, maximum: u32) -> Result<usize, String> {
    if count > maximum {
        return Err(format!(
            "driver reported {count} DRM format modifiers, exceeding bounded maximum {maximum}"
        ));
    }
    usize::try_from(count).map_err(|_| format!("DRM modifier count {count} does not fit usize"))
}

fn report_extension_map(prefix: &str, extensions: &BTreeMap<String, u32>) {
    println!("{prefix}.extensions.count={}", extensions.len());
    println!(
        "{prefix}.extensions.names={}",
        extensions.keys().cloned().collect::<Vec<_>>().join(",")
    );
    for (name, spec_version) in extensions {
        println!("{prefix}.extension.{name}.available=true");
        println!("{prefix}.extension.{name}.spec_version={spec_version}");
    }
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

fn optional_uuid(uuid: Option<&[u8; vk::UUID_SIZE]>) -> String {
    match uuid {
        Some(uuid) if uuid.iter().any(|byte| *byte != 0) => uuid
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        _ => "unavailable".to_string(),
    }
}

fn device_type_name(device_type: vk::PhysicalDeviceType) -> &'static str {
    if device_type == vk::PhysicalDeviceType::OTHER {
        "other"
    } else if device_type == vk::PhysicalDeviceType::INTEGRATED_GPU {
        "integrated_gpu"
    } else if device_type == vk::PhysicalDeviceType::DISCRETE_GPU {
        "discrete_gpu"
    } else if device_type == vk::PhysicalDeviceType::VIRTUAL_GPU {
        "virtual_gpu"
    } else if device_type == vk::PhysicalDeviceType::CPU {
        "cpu"
    } else {
        "unknown"
    }
}

fn driver_id_name(driver_id: vk::DriverId) -> &'static str {
    if driver_id == vk::DriverId::MESA_V3DV {
        "mesa_v3dv"
    } else if driver_id == vk::DriverId::MESA_LLVMPIPE {
        "mesa_llvmpipe"
    } else if driver_id == vk::DriverId::MESA_RADV {
        "mesa_radv"
    } else if driver_id == vk::DriverId::INTEL_OPEN_SOURCE_MESA {
        "intel_open_source_mesa"
    } else if driver_id == vk::DriverId::MESA_TURNIP {
        "mesa_turnip"
    } else if driver_id == vk::DriverId::MESA_PANVK {
        "mesa_panvk"
    } else if driver_id == vk::DriverId::GOOGLE_SWIFTSHADER {
        "google_swiftshader"
    } else {
        "other"
    }
}

fn escape_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn report_phase1_incomplete_fields() {
    println!("probe.phase1_ready=false");
    println!("probe.phase1.status=incomplete");
    println!("probe.phase1.kms_allocator_intersection_tested=false");
    println!("probe.phase1.output_create_import_bind_tested=false");
    println!("probe.phase1.camera_holder_lifecycle_tested=false");
    println!("probe.phase1.sync_fd_round_trip_tested=false");
    println!("probe.phase1.vulkan_to_kms_sync_fd_tested=false");
    println!("probe.phase1.kms_in_fence_fd_tested=false");
    println!("probe.phase1.kms_page_flip_tested=false");
    println!("probe.phase1.skia_ganesh_tested=false");
    println!("probe.phase1.capture_hash_tested=false");
    println!("probe.phase1.nerves_boot_tested=false");
    println!("probe.phase1.synchronization_validation_clean=false");
}

fn report_inventory_success() {
    println!("probe.stage=inventory_complete");
    println!("probe.inventory_passed=true");
    report_phase1_incomplete_fields();
    println!("probe.error_count=0");
    println!("probe.status=incomplete");
}

fn validate_functional_environment(config: &Config) -> Result<(), Vec<ProbeError>> {
    if !config.validation {
        println!("probe.validation.layer_enabled=false");
        println!("probe.validation.synchronization_enabled=false");
        return Ok(());
    }
    let layers = env::var("VK_INSTANCE_LAYERS").unwrap_or_default();
    let layer_enabled = layers
        .split([':', ','])
        .any(|layer| layer == "VK_LAYER_KHRONOS_validation");
    let enables = env::var("VK_LAYER_ENABLES").unwrap_or_default();
    let sync_enabled = enables
        .split([':', ','])
        .any(|value| value == "VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT");
    println!("probe.validation.layer_enabled={layer_enabled}");
    println!("probe.validation.synchronization_enabled={sync_enabled}");
    println!("probe.validation.message_count=unavailable_without_debug_callback");
    println!("probe.validation.zero_errors_proven=false");
    if layer_enabled && sync_enabled {
        Ok(())
    } else {
        Err(vec![ProbeError::after_inventory(
            "validation_environment",
            "validation_not_enabled",
            "--validation requires VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation and VK_LAYER_ENABLES=VK_VALIDATION_FEATURE_ENABLE_SYNCHRONIZATION_VALIDATION_EXT",
        )])
    }
}

fn report_functional_success(
    config: &Config,
    report: &functional_probe::FunctionalProbeReport,
    fd_before: Option<usize>,
    fd_after: Option<usize>,
    rss_before: Option<u64>,
    rss_after: Option<u64>,
) {
    println!("probe.stage=functional_page_flip_complete");
    println!("probe.inventory_passed=true");
    println!("probe.functional_passed=true");
    println!("probe.functional.pattern=ganesh_rgba_quadrants_on_bgra_scanout");
    println!("probe.functional.width={}", report.dimensions.0);
    println!("probe.functional.height={}", report.dimensions.1);
    println!("probe.functional.connector_id={}", report.connector_id);
    println!("probe.functional.encoder_id={}", report.encoder_id);
    println!("probe.functional.crtc_id={}", report.crtc_id);
    println!("probe.functional.plane_id={}", report.plane_id);
    println!("probe.functional.drm_fourcc=XRGB8888");
    println!("probe.functional.vulkan_format=B8G8R8A8_UNORM");
    println!("probe.functional.modifier={:#018x}", report.modifier);
    println!("probe.functional.pitch={}", report.pitch);
    println!("probe.functional.offset={}", report.offset);
    println!("probe.functional.object_size={}", report.object_size);
    println!(
        "probe.functional.commit_attempts={}",
        report.commit_attempts
    );
    println!("probe.functional.ebusy_retries={}", report.ebusy_retries);
    println!(
        "probe.functional.page_flip_sequence={}",
        report.page_flip_sequence
    );
    println!(
        "probe.functional.gpu_fence_signaled={}",
        report.gpu_fence_signaled
    );
    println!("probe.functional.capture_exact={}", report.capture_exact);
    println!("probe.functional.capture_sha256={}", report.capture_sha256);
    println!(
        "probe.functional.cleanup_complete={}",
        report.cleanup_complete
    );
    report_resource_after(fd_before, fd_after, rss_before, rss_after);
    println!("probe.phase1_ready=false");
    println!("probe.phase1.status=incomplete");
    println!("probe.phase1.kms_allocator_intersection_tested=true");
    println!("probe.phase1.output_create_import_bind_tested=true");
    println!("probe.phase1.camera_holder_lifecycle_tested=false");
    println!("probe.phase1.sync_fd_round_trip_tested=false");
    println!("probe.phase1.vulkan_to_kms_sync_fd_tested=true");
    println!("probe.phase1.kms_in_fence_fd_tested=true");
    println!("probe.phase1.kms_page_flip_tested=true");
    println!("probe.phase1.skia_ganesh_tested=true");
    println!("probe.phase1.capture_hash_tested=true");
    println!("probe.phase1.nerves_boot_tested=false");
    println!("probe.phase1.synchronization_validation_clean=false");
    println!("probe.validation_requested={}", config.validation);
    println!("probe.error_count=0");
    println!("probe.status=incomplete");
}

fn report_resource_after(
    fd_before: Option<usize>,
    fd_after: Option<usize>,
    rss_before: Option<u64>,
    rss_after: Option<u64>,
) {
    println!("probe.resources.fd_after={}", optional_usize(fd_after));
    println!("probe.resources.rss_kib_after={}", optional_u64(rss_after));
    println!(
        "probe.resources.fd_delta={}",
        signed_delta(
            fd_before.map(|value| value as u64),
            fd_after.map(|value| value as u64)
        )
    );
    println!(
        "probe.resources.rss_kib_delta={}",
        signed_delta(rss_before, rss_after)
    );
}

fn process_fd_count() -> Option<usize> {
    std::fs::read_dir("/proc/self/fd")
        .ok()
        .map(|entries| entries.count())
}

fn process_rss_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn optional_usize(value: Option<usize>) -> String {
    value.map_or_else(|| "unavailable".to_string(), |value| value.to_string())
}

fn optional_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "unavailable".to_string(), |value| value.to_string())
}

fn signed_delta(before: Option<u64>, after: Option<u64>) -> String {
    match (before, after) {
        (Some(before), Some(after)) => i128::from(after)
            .saturating_sub(i128::from(before))
            .to_string(),
        _ => "unavailable".to_string(),
    }
}

fn report_errors(errors: &[ProbeError]) {
    let stage = errors
        .first()
        .filter(|first| errors.iter().all(|error| error.stage == first.stage))
        .map_or("multiple", |error| error.stage);
    println!("probe.stage={stage}");
    println!(
        "probe.inventory_passed={}",
        errors.iter().all(|error| error.inventory_passed)
    );
    report_phase1_incomplete_fields();
    println!("probe.error_count={}", errors.len());
    for (index, error) in errors.iter().enumerate() {
        println!("probe.error.{index}.stage={}", error.stage);
        println!("probe.error.{index}.code={}", error.code);
        println!(
            "probe.error.{index}.raw_result={}",
            error.raw_result.as_deref().unwrap_or("unavailable")
        );
        println!(
            "probe.error.{index}.message={}",
            escape_value(&error.message)
        );
    }
    println!("probe.status=failed");
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use emerge_skia::backend::vulkan::capabilities::EXT_DRIVER_PROPERTIES;

    use super::*;

    fn node(minor: u32) -> DrmNodeId {
        DrmNodeId { major: 226, minor }
    }

    fn selection(field: DrmMatchField, node: DrmNodeId) -> SelectionNode {
        SelectionNode { node, field }
    }

    fn candidate(
        index: usize,
        name: &str,
        primary: Option<DrmNodeId>,
        render: Option<DrmNodeId>,
        software: bool,
        api_eligible: bool,
    ) -> SelectionCandidate {
        SelectionCandidate {
            index,
            name: name.to_string(),
            primary,
            render,
            software,
            api_eligible,
        }
    }

    #[test]
    fn selects_exact_primary_field_match() {
        let candidates = [
            candidate(0, "other", Some(node(0)), Some(node(128)), false, true),
            candidate(7, "target", Some(node(1)), Some(node(129)), false, true),
        ];

        assert_eq!(
            select_matching_device(selection(DrmMatchField::Primary, node(1)), &candidates),
            Ok(7)
        );
    }

    #[test]
    fn selects_exact_render_field_match_for_split_drm() {
        let candidates = [
            candidate(0, "other", Some(node(0)), Some(node(128)), false, true),
            candidate(7, "v3dv", None, Some(node(129)), false, true),
        ];

        assert_eq!(
            select_matching_device(selection(DrmMatchField::Render, node(129)), &candidates),
            Ok(7)
        );
    }

    #[test]
    fn rejects_duplicate_hardware_drm_matches() {
        let candidates = [
            candidate(0, "first", None, Some(node(128)), false, true),
            candidate(1, "second", None, Some(node(128)), false, true),
        ];

        let error =
            select_matching_device(selection(DrmMatchField::Render, node(128)), &candidates)
                .unwrap_err();
        assert!(error.contains("2 eligible hardware Vulkan physical-device matches"));
        assert!(error.contains("exactly one is required"));
    }

    #[test]
    fn does_not_match_the_wrong_drm_property_field() {
        let candidates = [candidate(
            0,
            "wrong-field",
            Some(node(128)),
            Some(node(129)),
            false,
            true,
        )];

        let error =
            select_matching_device(selection(DrmMatchField::Render, node(128)), &candidates)
                .unwrap_err();
        assert!(error.contains("no eligible hardware Vulkan physical device"));
    }

    #[test]
    fn rejects_software_only_drm_match() {
        let candidates = [candidate(
            0,
            "lavapipe",
            Some(node(0)),
            Some(node(128)),
            true,
            true,
        )];

        let error = select_matching_device(selection(DrmMatchField::Primary, node(0)), &candidates)
            .unwrap_err();
        assert!(error.contains("matches only rejected software"));
        assert!(error.contains("lavapipe"));
    }

    #[test]
    fn sub_vulkan_1_1_device_is_not_queried_with_core_properties2_or_selected() {
        assert!(!properties2_available(
            vk::API_VERSION_1_1,
            vk::API_VERSION_1_0
        ));
        assert!(properties2_available(
            vk::API_VERSION_1_1,
            vk::API_VERSION_1_1
        ));

        let candidates = [candidate(
            0,
            "vulkan-1.0",
            Some(node(0)),
            None,
            false,
            false,
        )];
        let error = select_matching_device(selection(DrmMatchField::Primary, node(0)), &candidates)
            .unwrap_err();
        assert!(error.contains("below API 1.1"));
    }

    #[test]
    fn classifies_cpu_and_lavapipe_as_software() {
        assert!(classify_software_device(vk::PhysicalDeviceType::CPU, "cpu", None).0);
        assert!(classify_software_device(vk::PhysicalDeviceType::OTHER, "llvmpipe", None).0);
        assert!(
            classify_software_device(
                vk::PhysicalDeviceType::OTHER,
                "unknown",
                Some(vk::DriverId::MESA_LLVMPIPE.as_raw()),
            )
            .0
        );
    }

    #[test]
    fn require_v3dv_uses_provable_driver_id() {
        assert!(is_provable_v3dv(Some(vk::DriverId::MESA_V3DV)));
        assert!(!is_provable_v3dv(Some(vk::DriverId::MESA_RADV)));
        assert!(!is_provable_v3dv(None));
    }

    #[test]
    fn driver_identity_uses_the_effective_api_for_core_promotion() {
        assert_eq!(
            driver_properties_source(vk::API_VERSION_1_1, vk::API_VERSION_1_2, &BTreeMap::new(),),
            SupportSource::Missing
        );
        assert_eq!(
            driver_properties_source(vk::API_VERSION_1_2, vk::API_VERSION_1_2, &BTreeMap::new(),),
            SupportSource::Core
        );
        assert_eq!(
            driver_properties_source(
                vk::API_VERSION_1_1,
                vk::API_VERSION_1_2,
                &BTreeMap::from([(EXT_DRIVER_PROPERTIES.to_string(), 1)]),
            ),
            SupportSource::Extension
        );
    }

    #[test]
    fn parses_split_drm_and_v3dv_target_options() {
        let parsed = parse_args(
            [
                "--drm-card",
                "/dev/dri/card1",
                "--vulkan-drm-node",
                "/dev/dri/renderD128",
                "--require-v3dv",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .unwrap();
        let ParseResult::Run(config) = parsed else {
            panic!("expected run config");
        };
        assert_eq!(config.drm_card, PathBuf::from("/dev/dri/card1"));
        assert_eq!(config.vulkan_drm_node, PathBuf::from("/dev/dri/renderD128"));
        assert!(config.vulkan_drm_node_explicit);
        assert!(config.require_v3dv);
        assert!(!config.functional);
        assert_eq!(
            config.allocation_direction,
            AllocationDirection::GbmImportedIntoVulkan
        );
        assert!(!config.allocation_direction_explicit);
        assert_eq!(config.page_flip_timeout_ms, 3_000);
        assert!(!config.validation);
        assert!(USAGE.contains("--vulkan-drm-node PATH"));
        assert!(USAGE.contains("--require-v3dv"));
    }

    #[test]
    fn defaults_vulkan_selection_to_kms_card_for_unified_devices() {
        let parsed = parse_args(
            ["--drm-card", "/dev/dri/card1"]
                .into_iter()
                .map(OsString::from),
        )
        .unwrap();
        let ParseResult::Run(config) = parsed else {
            panic!("expected run config");
        };
        assert_eq!(config.vulkan_drm_node, config.drm_card);
        assert!(!config.vulkan_drm_node_explicit);
        assert!(!config.require_v3dv);
        assert!(!config.functional);
    }

    #[test]
    fn parses_explicit_functional_probe_policy() {
        let ParseResult::Run(config) = parse_args(
            [
                "--drm-card",
                "/dev/dri/card1",
                "--vulkan-drm-node",
                "/dev/dri/renderD128",
                "--functional",
                "--allocation-direction",
                "gbm-import",
                "--page-flip-timeout-ms",
                "2500",
                "--validation",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .unwrap() else {
            panic!("expected run config");
        };
        assert!(config.functional);
        assert_eq!(
            config.allocation_direction,
            AllocationDirection::GbmImportedIntoVulkan
        );
        assert!(config.allocation_direction_explicit);
        assert_eq!(config.page_flip_timeout_ms, 2_500);
        assert!(config.validation);
    }

    #[test]
    fn functional_probe_requires_exact_vulkan_node_and_no_hidden_auto_direction() {
        let missing_node = parse_args(
            ["--drm-card", "/dev/dri/card1", "--functional"]
                .into_iter()
                .map(OsString::from),
        )
        .unwrap_err();
        assert!(missing_node.contains("requires an explicit --vulkan-drm-node"));

        let missing_direction = parse_args(
            ["--vulkan-drm-node", "/dev/dri/renderD128", "--functional"]
                .into_iter()
                .map(OsString::from),
        )
        .unwrap_err();
        assert!(missing_direction.contains("requires an explicit --allocation-direction"));

        let hidden_functional = parse_args(
            ["--allocation-direction", "gbm-import"]
                .into_iter()
                .map(OsString::from),
        )
        .unwrap_err();
        assert!(hidden_functional.contains("require --functional"));
    }

    #[test]
    fn rejects_duplicate_vulkan_drm_node_option() {
        let result = parse_args(
            [
                "--vulkan-drm-node",
                "/dev/dri/renderD128",
                "--vulkan-drm-node",
                "/dev/dri/renderD129",
            ]
            .into_iter()
            .map(OsString::from),
        );
        assert_eq!(
            result.unwrap_err(),
            "--vulkan-drm-node may only be specified once"
        );
    }

    #[test]
    fn selects_lowest_timestamp_capable_graphics_queue_family() {
        let families = [
            vk::QueueFamilyProperties::default()
                .queue_flags(vk::QueueFlags::GRAPHICS)
                .queue_count(1)
                .timestamp_valid_bits(0),
            vk::QueueFamilyProperties::default()
                .queue_flags(vk::QueueFlags::GRAPHICS)
                .queue_count(2)
                .timestamp_valid_bits(32),
            vk::QueueFamilyProperties::default()
                .queue_flags(vk::QueueFlags::GRAPHICS)
                .queue_count(1)
                .timestamp_valid_bits(64),
        ];
        assert_eq!(select_graphics_queue_family(&families), Some(1));
    }

    #[test]
    fn core_promotion_does_not_require_the_extension_name() {
        let no_extensions = BTreeMap::new();
        assert_eq!(
            support_source(
                vk::API_VERSION_1_1,
                &no_extensions,
                EXT_EXTERNAL_MEMORY,
                Some((1, 1)),
            ),
            SupportSource::Core
        );

        let extensions = BTreeMap::from([(EXT_EXTERNAL_MEMORY.to_string(), 1)]);
        assert_eq!(
            support_source(
                vk::API_VERSION_1_0,
                &extensions,
                EXT_EXTERNAL_MEMORY,
                Some((1, 1)),
            ),
            SupportSource::Extension
        );
        assert_eq!(
            support_source(
                vk::API_VERSION_1_1,
                &no_extensions,
                EXT_EXTERNAL_MEMORY_FD,
                None,
            ),
            SupportSource::Missing
        );
    }

    #[test]
    fn image_format_list_is_extension_required_on_1_1_and_core_on_1_2() {
        assert_eq!(EXT_IMAGE_FORMAT_LIST, "VK_KHR_image_format_list");
        assert_eq!(
            support_source(
                vk::API_VERSION_1_1,
                &BTreeMap::new(),
                EXT_IMAGE_FORMAT_LIST,
                Some((1, 2)),
            ),
            SupportSource::Missing
        );
        assert_eq!(
            support_source(
                vk::API_VERSION_1_2,
                &BTreeMap::new(),
                EXT_IMAGE_FORMAT_LIST,
                Some((1, 2)),
            ),
            SupportSource::Core
        );
    }

    #[test]
    fn modifier_count_allocation_is_bounded() {
        assert_eq!(bounded_modifier_count(0, 32), Ok(0));
        assert_eq!(bounded_modifier_count(32, 32), Ok(32));
        assert_eq!(
            bounded_modifier_count(33, 32),
            Err(
                "driver reported 33 DRM format modifiers, exceeding bounded maximum 32".to_string()
            )
        );
    }
}
