use crate::tree::geometry::Rect;
use crate::tree::scrollbar::{ScrollbarAxis, ScrollbarMetrics};

#[derive(Clone, Copy, Debug, PartialEq)]
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
pub enum ScrollbarHitArea {
    Thumb,
    Track,
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
