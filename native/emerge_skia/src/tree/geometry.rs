use super::attrs::{Attrs, BorderRadius, BorderWidth};
use super::element::Frame;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    #[inline]
    pub fn from_frame(frame: Frame) -> Self {
        Self {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        }
    }

    #[inline]
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);

        if x2 <= x1 || y2 <= y1 {
            return None;
        }

        Some(Rect {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        })
    }

    #[inline]
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    #[inline]
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x - dx,
            y: self.y - dy,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeBounds {
    pub rect: Rect,
    pub radii: Option<CornerRadii>,
}

impl ShapeBounds {
    #[inline]
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.offset(dx, dy),
            radii: self.radii,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipShape {
    pub rect: Rect,
    pub radii: Option<CornerRadii>,
}

impl ClipShape {
    #[inline]
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.offset(dx, dy),
            radii: self.radii,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ResolvedInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

pub fn resolved_border_width(border_width: Option<&BorderWidth>) -> (f32, f32, f32, f32) {
    let insets = match border_width {
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
    };

    (insets.left, insets.top, insets.right, insets.bottom)
}

pub fn radii_from_border_radius(radius: Option<&BorderRadius>) -> Option<CornerRadii> {
    match radius {
        Some(BorderRadius::Uniform(v)) => {
            let value = *v as f32;
            Some(CornerRadii {
                tl: value,
                tr: value,
                br: value,
                bl: value,
            })
        }
        Some(BorderRadius::Corners { tl, tr, br, bl }) => Some(CornerRadii {
            tl: *tl as f32,
            tr: *tr as f32,
            br: *br as f32,
            bl: *bl as f32,
        }),
        None => None,
    }
}

pub fn clamp_radii(rect: Rect, radii: CornerRadii) -> CornerRadii {
    let max_x = rect.width / 2.0;
    let max_y = rect.height / 2.0;
    let clamp = |r: f32| r.min(max_x).min(max_y).max(0.0);
    CornerRadii {
        tl: clamp(radii.tl),
        tr: clamp(radii.tr),
        br: clamp(radii.br),
        bl: clamp(radii.bl),
    }
}

pub fn point_in_rounded_rect(rect: Rect, radii: CornerRadii, x: f32, y: f32) -> bool {
    if !rect.contains(x, y) {
        return false;
    }

    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    if x < left + radii.tl && y < top + radii.tl {
        let dx = x - (left + radii.tl);
        let dy = y - (top + radii.tl);
        return dx * dx + dy * dy <= radii.tl * radii.tl;
    }

    if x > right - radii.tr && y < top + radii.tr {
        let dx = x - (right - radii.tr);
        let dy = y - (top + radii.tr);
        return dx * dx + dy * dy <= radii.tr * radii.tr;
    }

    if x > right - radii.br && y > bottom - radii.br {
        let dx = x - (right - radii.br);
        let dy = y - (bottom - radii.br);
        return dx * dx + dy * dy <= radii.br * radii.br;
    }

    if x < left + radii.bl && y > bottom - radii.bl {
        let dx = x - (left + radii.bl);
        let dy = y - (bottom - radii.bl);
        return dx * dx + dy * dy <= radii.bl * radii.bl;
    }

    true
}

pub fn point_hits_shape(shape: ShapeBounds, x: f32, y: f32) -> bool {
    shape.rect.contains(x, y)
        && shape
            .radii
            .is_none_or(|radii| point_in_rounded_rect(shape.rect, radii, x, y))
}

pub fn point_hits_clip(clip: ClipShape, x: f32, y: f32) -> bool {
    clip.rect.contains(x, y)
        && clip
            .radii
            .is_none_or(|radii| point_in_rounded_rect(clip.rect, radii, x, y))
}

pub fn host_clip_shape(frame: Frame, attrs: &Attrs) -> ClipShape {
    let (left, top, right, bottom) = resolved_border_width(attrs.border_width.as_ref());
    let rect = Rect {
        x: frame.x + left,
        y: frame.y + top,
        width: (frame.width - left - right).max(0.0),
        height: (frame.height - top - bottom).max(0.0),
    };

    let radii = radii_from_border_radius(attrs.border_radius.as_ref()).and_then(|radii| {
        let inner = CornerRadii {
            tl: (radii.tl - left.max(top)).max(0.0),
            tr: (radii.tr - right.max(top)).max(0.0),
            br: (radii.br - right.max(bottom)).max(0.0),
            bl: (radii.bl - left.max(bottom)).max(0.0),
        };

        (inner.tl > 0.0 || inner.tr > 0.0 || inner.br > 0.0 || inner.bl > 0.0)
            .then_some(clamp_radii(rect, inner))
    });

    ClipShape { rect, radii }
}

pub fn self_shape(frame: Frame, attrs: &Attrs) -> ShapeBounds {
    let rect = Rect::from_frame(frame);
    let radii =
        radii_from_border_radius(attrs.border_radius.as_ref()).map(|r| clamp_radii(rect, r));
    ShapeBounds { rect, radii }
}

pub fn intersect_clip(base: Option<ClipShape>, next: ClipShape) -> ClipShape {
    let rect = base
        .and_then(|clip| clip.rect.intersect(next.rect))
        .unwrap_or(next.rect);
    let radii = next.radii.map(|radii| clamp_radii(rect, radii));
    ClipShape { rect, radii }
}

pub fn visible_bounds(shape: ShapeBounds, inherited_clip: Option<ClipShape>) -> Rect {
    inherited_clip
        .and_then(|clip| shape.rect.intersect(clip.rect))
        .unwrap_or(shape.rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::{Attrs, Padding};

    #[test]
    fn shape_offset_moves_rect_but_preserves_radii() {
        let shape = ShapeBounds {
            rect: Rect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            },
            radii: Some(CornerRadii {
                tl: 4.0,
                tr: 4.0,
                br: 4.0,
                bl: 4.0,
            }),
        };

        assert_eq!(
            shape.offset(3.0, 5.0),
            ShapeBounds {
                rect: Rect {
                    x: 7.0,
                    y: 15.0,
                    width: 30.0,
                    height: 40.0,
                },
                radii: shape.radii,
            }
        );
    }

    #[test]
    fn host_clip_shape_ignores_padding() {
        let mut attrs = Attrs::default();
        attrs.padding = Some(Padding::Uniform(10.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                radii: None,
            }
        );
    }

    #[test]
    fn host_clip_shape_uses_inner_border_rect() {
        let mut attrs = Attrs::default();
        attrs.border_width = Some(BorderWidth::Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        });
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 4.0,
                    y: 1.0,
                    width: 94.0,
                    height: 46.0,
                },
                radii: None,
            }
        );
    }

    #[test]
    fn host_clip_shape_exists_without_explicit_clip_attrs() {
        let attrs = Attrs::default();
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                radii: None,
            }
        );
    }

    #[test]
    fn host_clip_shape_with_uniform_radius_uses_inner_radii() {
        let mut attrs = Attrs::default();
        attrs.border_radius = Some(BorderRadius::Uniform(10.0));
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 2.0,
                    y: 2.0,
                    width: 96.0,
                    height: 46.0,
                },
                radii: Some(CornerRadii {
                    tl: 8.0,
                    tr: 8.0,
                    br: 8.0,
                    bl: 8.0,
                }),
            }
        );
    }

    #[test]
    fn host_clip_shape_with_corner_radii_uses_inner_corner_radii() {
        let mut attrs = Attrs::default();
        attrs.border_radius = Some(BorderRadius::Corners {
            tl: 12.0,
            tr: 8.0,
            br: 4.0,
            bl: 16.0,
        });
        attrs.border_width = Some(BorderWidth::Uniform(3.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 3.0,
                    y: 3.0,
                    width: 94.0,
                    height: 44.0,
                },
                radii: Some(CornerRadii {
                    tl: 9.0,
                    tr: 5.0,
                    br: 1.0,
                    bl: 13.0,
                }),
            }
        );
    }

    #[test]
    fn host_clip_shape_falls_back_to_rect_when_radius_consumed() {
        let mut attrs = Attrs::default();
        attrs.border_radius = Some(BorderRadius::Uniform(3.0));
        attrs.border_width = Some(BorderWidth::Uniform(5.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 5.0,
                    y: 5.0,
                    width: 90.0,
                    height: 40.0,
                },
                radii: None,
            }
        );
    }

    #[test]
    fn host_clip_shape_without_border_keeps_radius() {
        let mut attrs = Attrs::default();
        attrs.border_radius = Some(BorderRadius::Uniform(8.0));
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        };

        assert_eq!(
            host_clip_shape(frame, &attrs),
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                radii: Some(CornerRadii {
                    tl: 8.0,
                    tr: 8.0,
                    br: 8.0,
                    bl: 8.0,
                }),
            }
        );
    }
}
