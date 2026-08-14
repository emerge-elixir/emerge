//! Wayland backend built on smithay-client-toolkit.

mod egl;
mod geometry;
mod handles;
mod input;
mod keyboard;
mod present;
mod protocols;
mod renderer_env;
mod runtime;
mod text_input;
#[cfg(feature = "wayland-vulkan")]
mod vulkan;

pub(crate) use runtime::{WaylandRunArgs, run};
