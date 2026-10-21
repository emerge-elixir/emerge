//! Wayland backend built on smithay-client-toolkit.

#[cfg(all(feature = "wayland", feature = "opengl"))]
mod egl;
mod geometry;
#[cfg(any(
    all(feature = "wayland", feature = "opengl"),
    all(feature = "wayland", feature = "vulkan")
))]
mod handles;
mod input;
mod keyboard;
mod present;
mod protocols;
mod renderer_env;
mod runtime;
mod text_input;
#[cfg(all(feature = "wayland", feature = "vulkan"))]
mod vulkan;

pub(crate) use runtime::{WaylandRunArgs, run};
