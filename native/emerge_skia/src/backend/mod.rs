//! Backend implementations for different display systems.
//!
//! Each backend provides a way to create a window/surface and run an event loop.

pub mod raster;
pub mod wayland;
