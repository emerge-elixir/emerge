//! Wayland backend built on smithay-client-toolkit.

mod egl;
mod geometry;
mod input;
mod keyboard;
mod present;
mod protocols;
mod runtime;
mod text_input;

pub(crate) use runtime::run;
