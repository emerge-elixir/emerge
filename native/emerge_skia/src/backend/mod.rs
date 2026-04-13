//! Backend implementations for different display systems.
//!
//! Each backend provides a way to create a window/surface and run an event loop.

#[cfg(all(feature = "drm", target_os = "linux"))]
pub mod drm;
pub mod raster;
pub mod skia_gpu;
pub mod wake;
#[cfg(all(feature = "wayland", target_os = "linux"))]
pub mod wayland;
#[cfg(all(feature = "wayland", target_os = "linux"))]
pub mod wayland_config;
