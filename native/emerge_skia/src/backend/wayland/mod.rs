//! Wayland backend built on smithay-client-toolkit.

mod egl;
mod geometry;
mod input;
mod keyboard;
mod present;
mod protocols;
mod runtime;

pub(crate) use runtime::run;
