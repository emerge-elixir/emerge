//! Backend implementations for different display systems.
//!
//! Each backend provides a way to create a window/surface and run an event loop.

// Pure owner-side backlight primitives are tested without enabling DRM or
// touching hardware. Presenters remain fail-closed until power/barrier wiring.
#[cfg(all(target_os = "linux", any(feature = "drm-core", test)))]
pub mod backlight;
pub(crate) mod cadence;
#[cfg(all(feature = "drm-core", target_os = "linux"))]
pub mod drm;
pub mod headless;
// The wire protocol is platform-independent and is exercised on Linux CI too.
#[cfg(any(feature = "macos", test))]
pub mod macos;
pub mod present;
pub mod production;
pub mod raster;
#[cfg(feature = "linux-opengl")]
pub mod skia_gpu;
#[cfg(feature = "vulkan")]
pub mod vulkan;
pub mod wake;
#[cfg(all(feature = "wayland-core", target_os = "linux"))]
pub mod wayland;
#[cfg(all(feature = "wayland-core", target_os = "linux"))]
pub mod wayland_config;
