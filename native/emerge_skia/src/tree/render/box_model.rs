use crate::tree::attrs::{Attrs, BorderRadius, BorderWidth, Padding};
use crate::tree::element::Frame;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ResolvedInsets {
    pub(super) top: f32,
    pub(super) right: f32,
    pub(super) bottom: f32,
    pub(super) left: f32,
}

pub(super) fn resolved_padding(padding: Option<&Padding>) -> ResolvedInsets {
    match padding {
        Some(Padding::Uniform(value)) => {
            let value = *value as f32;
            ResolvedInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedInsets::default(),
    }
}

pub(super) fn resolved_border_width(border_width: Option<&BorderWidth>) -> ResolvedInsets {
    match border_width {
        Some(BorderWidth::Uniform(value)) => {
            let value = *value as f32;
            ResolvedInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedInsets::default(),
    }
}

pub(super) fn content_insets(attrs: &Attrs) -> ResolvedInsets {
    let padding = resolved_padding(attrs.padding.as_ref());
    let border = resolved_border_width(attrs.border_width.as_ref());
    ResolvedInsets {
        top: padding.top + border.top,
        right: padding.right + border.right,
        bottom: padding.bottom + border.bottom,
        left: padding.left + border.left,
    }
}

pub(super) fn content_rect(frame: Frame, attrs: &Attrs) -> (f32, f32, f32, f32) {
    let insets = content_insets(attrs);
    let x = frame.x + insets.left;
    let y = frame.y + insets.top;
    let w = (frame.width - insets.left - insets.right).max(0.0);
    let h = (frame.height - insets.top - insets.bottom).max(0.0);
    (x, y, w, h)
}

/// Preserve independent corners for borders and shadow casting geometry.
pub(super) fn border_radii(radius: Option<&BorderRadius>) -> [f32; 4] {
    match radius {
        Some(BorderRadius::Uniform(r)) => [*r as f32; 4],
        Some(BorderRadius::Corners { tl, tr, br, bl }) => {
            [*tl as f32, *tr as f32, *br as f32, *bl as f32]
        }
        None => [0.0; 4],
    }
}
