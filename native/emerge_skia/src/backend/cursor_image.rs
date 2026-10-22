//! Built-in artwork shared by DRM cursors and the Wayland theme fallback.
//! The originals and their CC0 license live alongside the DRM cursor assets.
use crate::events::CursorIcon;

pub(crate) fn builtin_cursor(icon: CursorIcon) -> (&'static str, (f32, f32)) {
    match icon {
        CursorIcon::Default => (
            include_str!("drm/cursors/mocu_black_right/default.svg"),
            (1.0, 1.0),
        ),
        CursorIcon::Pointer => (
            include_str!("drm/cursors/mocu_black_right/pointer.svg"),
            (7.0, 2.0),
        ),
        CursorIcon::Text => (
            include_str!("drm/cursors/mocu_black_right/text.svg"),
            (11.5, 11.5),
        ),
    }
}
