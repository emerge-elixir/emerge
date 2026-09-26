//! Presenter-neutral Vulkan renderer ownership and frame contracts.
//!
//! Platform presenters provide images, tokens, layouts, and acquire/present requirements. This
//! module owns loader/instance/device/queue/Ganesh state and shared `SceneRenderer` traversal; it
//! deliberately contains no Wayland surface/swapchain or DRM/KMS types.

pub mod capabilities;
pub mod device;
pub mod external_image;
pub mod frame;
pub mod ganesh;
pub mod imported_image;
pub mod instance;
mod raw;
pub mod sync;

pub use capabilities::{DrmNodeId, SelectionNode};
pub use device::{
    DeviceRequirements, ExactDeviceRequirements, VulkanDevice, VulkanDeviceIdentity,
    VulkanDeviceReport, VulkanDrmNodeReport, VulkanRendererReport,
};
pub use external_image::{
    DRM_FORMAT_MOD_LINEAR, EXPORTED_PRIME_IMAGE_USAGE, ExportedDmaBufImage, ExportedPlane,
};
pub use frame::{AcquiredTarget, CapturedRgba, CompletedTarget, TargetImageState, VulkanEngine};
pub use ganesh::{GANESH_TARGET_IMAGE_USAGE, VulkanTargetFormat, VulkanTargetSurface};
pub use imported_image::{
    ImportedDmaBufImage, ImportedPlane, InteropVulkanDmaBufImporter, Nv12AllocationBindingRecipe,
    Nv12Conversion, Nv12FrameTopology, Nv12ImportStrategy, Nv12ModifierCapability, Nv12Plane,
    Nv12StagingPreference, Nv12TargetAllocationProof, PackedImageFormat, PackedImageImport,
    PackedImageImportStrategy, VulkanImportPoolLimits, YcbcrModel, YcbcrOffset, YcbcrRange,
    capabilities_for_importer, inventory_nv12_modifier_capabilities, map_nv12_colorimetry,
    query_nv12_modifier_capability, resolve_nv12_modifier_capability,
    validate_bgra_scanout_import_support, validate_nv12_allocation_proof,
    validate_nv12_modifier_capability, validate_nv12_shared_object_topology,
    validate_nv12_target_allocation_proof, validate_packed_import_support,
    validate_packed_staging_support, validate_rgba_import_support,
};
pub use instance::{VulkanInstance, VulkanValidationReport};
pub use sync::{
    DMA_BUF_EXTERNAL_QUEUE_FAMILY, ExportableSyncFdSemaphore, ExternalQueueTransfer,
    ImportedImageSync, ImportedImageSyncError, ImportedImageSyncErrorKind, VulkanVideoTiming,
    validate_sync_fd_import, wait_surface_on_semaphore,
};
