//! Backend implementations for different display systems.
//!
//! Each backend provides a way to create a window/surface and run an event loop.

#[cfg(all(feature = "drm-core", target_os = "linux"))]
pub mod drm;
pub mod headless;
#[cfg(feature = "macos")]
pub mod macos;
pub mod present;
pub mod raster;
#[cfg(feature = "linux-opengl")]
pub mod skia_gpu;
#[cfg(feature = "vulkan")]
pub mod vulkan;
pub mod wake;
#[cfg(all(feature = "wayland", target_os = "linux"))]
pub mod wayland;
#[cfg(all(feature = "wayland", target_os = "linux"))]
pub mod wayland_config;
