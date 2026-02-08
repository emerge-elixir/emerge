use super::{EventNode, Rect, point_hits_node};
use crate::tree::element::ElementId;
use crate::tree::scrollbar::{ScrollbarAxis, ScrollbarMetrics};

#[derive(Clone, Copy, Debug)]
pub struct ScrollbarNode {
    pub track_rect: Rect,
    pub thumb_rect: Rect,
    pub track_start: f32,
    pub track_len: f32,
    pub thumb_start: f32,
    pub thumb_len: f32,
    pub scroll_offset: f32,
    pub scroll_range: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollbarHitArea {
    Thumb,
    Track,
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollbarHit {
    pub id: ElementId,
    pub axis: ScrollbarAxis,
    pub area: ScrollbarHitArea,
    pub node: ScrollbarNode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScrollbarThumbHover {
    pub id: ElementId,
    pub axis: ScrollbarAxis,
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollbarDragState {
    pub id: ElementId,
    pub axis: ScrollbarAxis,
    pub track_start: f32,
    pub track_len: f32,
    pub thumb_len: f32,
    pub pointer_offset: f32,
    pub scroll_range: f32,
    pub current_scroll: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollbarThumbDragRequest {
    X { element_id: ElementId, dx: f32 },
    Y { element_id: ElementId, dy: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollbarHoverRequest {
    X {
        element_id: ElementId,
        hovered: bool,
    },
    Y {
        element_id: ElementId,
        hovered: bool,
    },
}

#[derive(Clone, Debug, Default)]
pub(crate) enum ScrollbarInteraction {
    #[default]
    Idle,
    Captured,
    Dragging(ScrollbarDragState),
    SuppressMouseRelease,
}

impl ScrollbarInteraction {
    pub(crate) fn clear(&mut self) {
        *self = Self::Idle;
    }

    pub(crate) fn mark_captured(&mut self) {
        *self = Self::Captured;
    }

    pub(crate) fn set_dragging(&mut self, drag: ScrollbarDragState) {
        *self = Self::Dragging(drag);
    }

    pub(crate) fn suppress_release(&mut self) {
        *self = Self::SuppressMouseRelease;
    }

    pub(crate) fn take_release_suppression(&mut self) -> bool {
        if matches!(self, Self::SuppressMouseRelease) {
            *self = Self::Idle;
            return true;
        }
        false
    }

    pub(crate) fn is_captured(&self) -> bool {
        matches!(self, Self::Captured | Self::Dragging(_))
    }

    pub(crate) fn blocks_content_drag(&self) -> bool {
        self.is_captured()
    }

    pub(crate) fn dragging(&self) -> Option<&ScrollbarDragState> {
        match self {
            Self::Dragging(state) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn dragging_mut(&mut self) -> Option<&mut ScrollbarDragState> {
        match self {
            Self::Dragging(state) => Some(state),
            _ => None,
        }
    }
}

pub(crate) fn thumb_hover_from_hit(hit: ScrollbarHit) -> Option<ScrollbarThumbHover> {
    if hit.area != ScrollbarHitArea::Thumb {
        return None;
    }
    Some(ScrollbarThumbHover {
        id: hit.id,
        axis: hit.axis,
    })
}

pub(crate) fn scrollbar_node_from_metrics(
    metrics: ScrollbarMetrics,
    offset_x: f32,
    offset_y: f32,
) -> ScrollbarNode {
    ScrollbarNode {
        track_rect: Rect {
            x: metrics.track_x - offset_x,
            y: metrics.track_y - offset_y,
            width: metrics.track_width,
            height: metrics.track_height,
        },
        thumb_rect: Rect {
            x: metrics.thumb_x - offset_x,
            y: metrics.thumb_y - offset_y,
            width: metrics.thumb_width,
            height: metrics.thumb_height,
        },
        track_start: metrics.track_start
            - match metrics.axis {
                ScrollbarAxis::X => offset_x,
                ScrollbarAxis::Y => offset_y,
            },
        track_len: metrics.track_len,
        thumb_start: metrics.thumb_start
            - match metrics.axis {
                ScrollbarAxis::X => offset_x,
                ScrollbarAxis::Y => offset_y,
            },
        thumb_len: metrics.thumb_len,
        scroll_offset: metrics.scroll_offset,
        scroll_range: metrics.scroll_range,
    }
}

pub(crate) fn hit_test_scrollbar(registry: &[EventNode], x: f32, y: f32) -> Option<ScrollbarHit> {
    for node in registry.iter().rev() {
        if !point_hits_node(node, x, y) {
            continue;
        }

        if let Some(scrollbar) = node.scrollbar_y
            && scrollbar.thumb_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::Y,
                area: ScrollbarHitArea::Thumb,
                node: scrollbar,
            });
        }

        if let Some(scrollbar) = node.scrollbar_x
            && scrollbar.thumb_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::X,
                area: ScrollbarHitArea::Thumb,
                node: scrollbar,
            });
        }

        if let Some(scrollbar) = node.scrollbar_y
            && scrollbar.track_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::Y,
                area: ScrollbarHitArea::Track,
                node: scrollbar,
            });
        }

        if let Some(scrollbar) = node.scrollbar_x
            && scrollbar.track_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::X,
                area: ScrollbarHitArea::Track,
                node: scrollbar,
            });
        }
    }

    None
}

pub(crate) fn axis_coord(axis: ScrollbarAxis, x: f32, y: f32) -> f32 {
    match axis {
        ScrollbarAxis::X => x,
        ScrollbarAxis::Y => y,
    }
}

pub(crate) fn scroll_from_pointer(
    pointer_axis: f32,
    track_start: f32,
    track_len: f32,
    pointer_offset: f32,
    scroll_range: f32,
) -> f32 {
    if track_len <= 0.0 || scroll_range <= 0.0 {
        return 0.0;
    }

    let min = track_start;
    let max = track_start + track_len;
    let next_thumb_start = (pointer_axis - pointer_offset).clamp(min, max);
    let ratio = if track_len > 0.0 {
        (next_thumb_start - track_start) / track_len
    } else {
        0.0
    };
    (ratio * scroll_range).clamp(0.0, scroll_range)
}
