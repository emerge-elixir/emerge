//! The retained window needs WSI and Ganesh only, not the scene/video engine.
// These source modules also contain entry points for the reference DRM renderer.
#[allow(dead_code)]
#[path = "capabilities.rs"]
pub(crate) mod capabilities;
#[allow(dead_code)]
#[path = "device.rs"]
pub(crate) mod device;
#[allow(dead_code)]
#[path = "ganesh.rs"]
pub(crate) mod ganesh;
#[allow(dead_code)]
#[path = "instance.rs"]
pub(crate) mod instance;
#[path = "raw.rs"]
pub(crate) mod raw;
