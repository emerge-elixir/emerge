use super::attrs::{Attrs, ScrollbarHoverAxis};
use super::element::Frame;

pub const SCROLLBAR_THICKNESS: f32 = 4.0;
pub const SCROLLBAR_THICKNESS_HOVER: f32 = 8.0;
pub const SCROLLBAR_MIN_LENGTH: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarAxis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
    pub axis: ScrollbarAxis,
    pub track_x: f32,
    pub track_y: f32,
    pub track_width: f32,
    pub track_height: f32,
    pub thumb_x: f32,
    pub thumb_y: f32,
    pub thumb_width: f32,
    pub thumb_height: f32,
    pub track_start: f32,
    pub track_len: f32,
    pub thumb_start: f32,
    pub thumb_len: f32,
    pub scroll_offset: f32,
    pub scroll_range: f32,
}

fn is_axis_hovered(attrs: &Attrs, axis: ScrollbarAxis) -> bool {
    matches!(
        (attrs.scrollbar_hover_axis, axis),
        (Some(ScrollbarHoverAxis::X), ScrollbarAxis::X)
            | (Some(ScrollbarHoverAxis::Y), ScrollbarAxis::Y)
    )
}

fn thickness_for_axis(attrs: &Attrs, axis: ScrollbarAxis) -> f32 {
    if is_axis_hovered(attrs, axis) {
        SCROLLBAR_THICKNESS_HOVER
    } else {
        SCROLLBAR_THICKNESS
    }
}

pub fn horizontal_metrics(frame: Frame, attrs: &Attrs) -> Option<ScrollbarMetrics> {
    if !attrs.scrollbar_x.unwrap_or(false) {
        return None;
    }

    let viewport = frame.width;
    let content = frame.content_width;
    if content <= viewport || viewport <= 0.0 {
        return None;
    }

    let thickness = thickness_for_axis(attrs, ScrollbarAxis::X);
    let thumb_len = (viewport * viewport / content)
        .max(SCROLLBAR_MIN_LENGTH)
        .min(viewport);
    let scroll_offset = attrs.scroll_x.unwrap_or(0.0) as f32;
    let scroll_range = (content - viewport).max(0.0);
    let ratio = if scroll_range > 0.0 {
        (scroll_offset / scroll_range).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let track_len = (viewport - thumb_len).max(0.0);
    let thumb_start = frame.x + ratio * track_len;

    Some(ScrollbarMetrics {
        axis: ScrollbarAxis::X,
        track_x: frame.x,
        track_y: frame.y + frame.height - thickness,
        track_width: viewport,
        track_height: thickness,
        thumb_x: thumb_start,
        thumb_y: frame.y + frame.height - thickness,
        thumb_width: thumb_len,
        thumb_height: thickness,
        track_start: frame.x,
        track_len,
        thumb_start,
        thumb_len,
        scroll_offset,
        scroll_range,
    })
}

pub fn vertical_metrics(frame: Frame, attrs: &Attrs) -> Option<ScrollbarMetrics> {
    if !attrs.scrollbar_y.unwrap_or(false) {
        return None;
    }

    let viewport = frame.height;
    let content = frame.content_height;
    if content <= viewport || viewport <= 0.0 {
        return None;
    }

    let thickness = thickness_for_axis(attrs, ScrollbarAxis::Y);
    let thumb_len = (viewport * viewport / content)
        .max(SCROLLBAR_MIN_LENGTH)
        .min(viewport);
    let scroll_offset = attrs.scroll_y.unwrap_or(0.0) as f32;
    let scroll_range = (content - viewport).max(0.0);
    let ratio = if scroll_range > 0.0 {
        (scroll_offset / scroll_range).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let track_len = (viewport - thumb_len).max(0.0);
    let thumb_start = frame.y + ratio * track_len;

    Some(ScrollbarMetrics {
        axis: ScrollbarAxis::Y,
        track_x: frame.x + frame.width - thickness,
        track_y: frame.y,
        track_width: thickness,
        track_height: viewport,
        thumb_x: frame.x + frame.width - thickness,
        thumb_y: thumb_start,
        thumb_width: thickness,
        thumb_height: thumb_len,
        track_start: frame.y,
        track_len,
        thumb_start,
        thumb_len,
        scroll_offset,
        scroll_range,
    })
}
