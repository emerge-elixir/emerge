use std::ptr::NonNull;

use raw_window_handle::{HasDisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandWindowHandle};
use wayland_client::{Connection, Proxy, protocol::wl_surface};

pub(super) fn raw_display_handle(conn: &Connection) -> Result<RawDisplayHandle, String> {
    conn.backend()
        .display_handle()
        .map(|handle| handle.as_raw())
        .map_err(|err| format!("failed to get wayland display handle: {err}"))
}

pub(super) fn raw_window_handle(
    surface: &wl_surface::WlSurface,
) -> Result<RawWindowHandle, String> {
    let ptr = NonNull::new(surface.id().as_ptr().cast())
        .ok_or_else(|| "failed to get wl_surface pointer".to_string())?;

    Ok(RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr)))
}
