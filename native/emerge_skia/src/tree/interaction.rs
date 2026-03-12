//! Interaction geometry computed from laid-out tree state.
//!
//! This module computes and stores per-element interaction geometry used by the
//! event system (hit testing, clipping, rounded-corner checks). The data is
//! derived from layout output and ancestor clip/scroll context.

use super::attrs::{BorderRadius, Padding};
use super::element::{ElementId, ElementTree, Frame, RetainedPaintPhase};

/// Axis-aligned rectangle in world coordinates.
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
}

/// Corner radii for rounded-rect hit checks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

/// Precomputed interaction geometry for one element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementInteraction {
    pub visible: bool,
    pub hit_rect: Rect,
    pub self_rect: Rect,
    pub self_radii: Option<CornerRadii>,
    pub clip_rect: Option<Rect>,
    pub clip_radii: Option<CornerRadii>,
}

#[derive(Clone, Copy, Debug)]
struct ClipContext {
    rect: Rect,
    radii: Option<CornerRadii>,
}

/// Recompute interaction geometry for all nodes in the tree.
pub fn populate_interaction(tree: &mut ElementTree) {
    let Some(root) = tree.root.clone() else {
        return;
    };

    collect_interaction(tree, &root, 0.0, 0.0, None);
}

fn collect_interaction(
    tree: &mut ElementTree,
    id: &ElementId,
    offset_x: f32,
    offset_y: f32,
    clip_rect: Option<ClipContext>,
) {
    let (children, child_offset_x, child_offset_y, next_clip) = {
        let Some(element) = tree.get_mut(id) else {
            return;
        };

        let mut next_clip = clip_rect;
        let mut scroll_x = 0.0;
        let mut scroll_y = 0.0;

        if let Some(frame) = element.frame {
            let frame_rect = Rect::from_frame(frame);
            let adjusted_rect = Rect {
                x: frame_rect.x - offset_x,
                y: frame_rect.y - offset_y,
                width: frame_rect.width,
                height: frame_rect.height,
            };

            let active_clip_rect = clip_rect.map(|ctx| ctx.rect);
            let active_clip_radii = clip_rect.and_then(|ctx| ctx.radii);

            let visible_rect = if let Some(active_clip) = active_clip_rect {
                adjusted_rect.intersect(active_clip).unwrap_or(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                })
            } else {
                adjusted_rect
            };
            let visible = visible_rect.width > 0.0 && visible_rect.height > 0.0;

            let self_radii = radii_from_border_radius(element.attrs.border_radius.as_ref())
                .map(|radii| clamp_radii(adjusted_rect, radii));
            let clip_radii = active_clip_rect
                .and_then(|rect| active_clip_radii.map(|radii| clamp_radii(rect, radii)));

            element.interaction = Some(ElementInteraction {
                visible,
                hit_rect: visible_rect,
                self_rect: adjusted_rect,
                self_radii,
                clip_rect: active_clip_rect,
                clip_radii,
            });

            let (left, top, right, bottom) = match element.attrs.padding.as_ref() {
                Some(Padding::Uniform(v)) => (*v as f32, *v as f32, *v as f32, *v as f32),
                Some(Padding::Sides {
                    left,
                    top,
                    right,
                    bottom,
                }) => (*left as f32, *top as f32, *right as f32, *bottom as f32),
                None => (0.0, 0.0, 0.0, 0.0),
            };

            let content_rect = Rect {
                x: adjusted_rect.x + left,
                y: adjusted_rect.y + top,
                width: (adjusted_rect.width - left - right).max(0.0),
                height: (adjusted_rect.height - top - bottom).max(0.0),
            };

            let scroll_x_enabled = element.attrs.scrollbar_x.unwrap_or(false);
            let scroll_y_enabled = element.attrs.scrollbar_y.unwrap_or(false);
            let max_x = if scroll_x_enabled {
                element
                    .attrs
                    .scroll_x_max
                    .unwrap_or((frame.content_width - frame.width).max(0.0) as f64)
                    as f32
            } else {
                0.0
            }
            .max(0.0);
            let max_y = if scroll_y_enabled {
                element
                    .attrs
                    .scroll_y_max
                    .unwrap_or((frame.content_height - frame.height).max(0.0) as f64)
                    as f32
            } else {
                0.0
            }
            .max(0.0);

            scroll_x = if scroll_x_enabled {
                (element.attrs.scroll_x.unwrap_or(0.0) as f32).clamp(0.0, max_x)
            } else {
                0.0
            };
            scroll_y = if scroll_y_enabled {
                (element.attrs.scroll_y.unwrap_or(0.0) as f32).clamp(0.0, max_y)
            } else {
                0.0
            };

            let clip_enabled = element.attrs.clip_x.unwrap_or(false)
                || element.attrs.clip_y.unwrap_or(false)
                || element.attrs.scrollbar_x.unwrap_or(false)
                || element.attrs.scrollbar_y.unwrap_or(false);

            if clip_enabled {
                let clip_radii = radii_from_border_radius(element.attrs.border_radius.as_ref());
                let clip_rect = match clip_rect {
                    Some(active_clip) => content_rect.intersect(active_clip.rect).unwrap_or(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    }),
                    None => content_rect,
                };

                let clipped_radii = clip_radii.map(|radii| clamp_radii(clip_rect, radii));

                next_clip = Some(ClipContext {
                    rect: clip_rect,
                    radii: clipped_radii,
                });
            }
        } else {
            element.interaction = None;
        }

        let mut children = Vec::new();
        element.for_each_paint_child(|child| {
            children.push((child.id.clone(), child.phase));
        });

        (
            children,
            offset_x + scroll_x,
            offset_y + scroll_y,
            next_clip,
        )
    };

    for (child_id, phase) in children {
        let (next_offset_x, next_offset_y) = match phase {
            RetainedPaintPhase::Children => (child_offset_x, child_offset_y),
            RetainedPaintPhase::BehindContent | RetainedPaintPhase::Overlay(_) => {
                (offset_x, offset_y)
            }
        };

        collect_interaction(tree, &child_id, next_offset_x, next_offset_y, next_clip);
    }
}

fn radii_from_border_radius(radius: Option<&BorderRadius>) -> Option<CornerRadii> {
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

/// Clamp corner radii to fit inside rect half extents.
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

/// Hit test inside rounded rectangle.
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

/// Returns whether `x`,`y` hits the element interaction region.
pub fn point_hits_interaction(interaction: ElementInteraction, x: f32, y: f32) -> bool {
    if !point_hits_subregion(interaction, interaction.hit_rect, None, x, y) {
        return false;
    }

    if let Some(radii) = interaction.self_radii
        && !point_in_rounded_rect(interaction.self_rect, radii, x, y)
    {
        return false;
    }

    true
}

/// Returns whether `x`,`y` hits a subregion constrained by an element interaction.
pub fn point_hits_subregion(
    interaction: ElementInteraction,
    bounds: Rect,
    radii: Option<CornerRadii>,
    x: f32,
    y: f32,
) -> bool {
    if !interaction.visible || !bounds.contains(x, y) {
        return false;
    }

    if let (Some(rect), Some(radii)) = (interaction.clip_rect, interaction.clip_radii)
        && !point_in_rounded_rect(rect, radii, x, y)
    {
        return false;
    }

    if let Some(radii) = radii
        && !point_in_rounded_rect(bounds, clamp_radii(bounds, radii), x, y)
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_hits_interaction_respects_self_radii() {
        let interaction = ElementInteraction {
            visible: true,
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            self_radii: Some(CornerRadii {
                tl: 10.0,
                tr: 10.0,
                br: 10.0,
                bl: 10.0,
            }),
            clip_rect: None,
            clip_radii: None,
        };

        assert!(!point_hits_interaction(interaction, 2.0, 2.0));
        assert!(point_hits_interaction(interaction, 10.0, 2.0));
    }

    #[test]
    fn point_hits_subregion_respects_parent_clip() {
        let interaction = ElementInteraction {
            visible: true,
            hit_rect: Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            },
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            self_radii: None,
            clip_rect: Some(Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            }),
            clip_radii: Some(CornerRadii {
                tl: 10.0,
                tr: 10.0,
                br: 10.0,
                bl: 10.0,
            }),
        };
        let thumb = Rect {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
        };

        assert!(!point_hits_subregion(interaction, thumb, None, 12.0, 12.0));
        assert!(point_hits_subregion(interaction, thumb, None, 20.0, 12.0));
    }
}
