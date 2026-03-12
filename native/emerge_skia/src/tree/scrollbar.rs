use super::attrs::{Attrs, ScrollbarHoverAxis};
use super::element::Frame;

pub const SCROLLBAR_THICKNESS: f32 = 5.0;
pub const SCROLLBAR_THICKNESS_HOVER: f32 = 7.0;
pub const SCROLLBAR_MIN_LENGTH: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::{Attrs, ScrollbarHoverAxis};

    fn frame(w: f32, h: f32, cw: f32, ch: f32) -> Frame {
        Frame {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            content_width: cw,
            content_height: ch,
        }
    }

    #[test]
    fn test_vertical_metrics_default_and_hover_thickness() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(50.0);

        let base = vertical_metrics(frame(100.0, 50.0, 100.0, 150.0), &attrs).unwrap();
        assert_eq!(base.track_width, 5.0);
        assert_eq!(base.thumb_width, 5.0);
        assert_eq!(base.track_x, 95.0);

        attrs.scrollbar_hover_axis = Some(ScrollbarHoverAxis::Y);
        let hover = vertical_metrics(frame(100.0, 50.0, 100.0, 150.0), &attrs).unwrap();
        assert_eq!(hover.track_width, 7.0);
        assert_eq!(hover.thumb_width, 7.0);
        assert_eq!(hover.track_x, 93.0);
    }

    #[test]
    fn test_horizontal_metrics_default_and_hover_thickness() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(30.0);

        let base = horizontal_metrics(frame(80.0, 40.0, 160.0, 40.0), &attrs).unwrap();
        assert_eq!(base.track_height, 5.0);
        assert_eq!(base.thumb_height, 5.0);
        assert_eq!(base.track_y, 35.0);

        attrs.scrollbar_hover_axis = Some(ScrollbarHoverAxis::X);
        let hover = horizontal_metrics(frame(80.0, 40.0, 160.0, 40.0), &attrs).unwrap();
        assert_eq!(hover.track_height, 7.0);
        assert_eq!(hover.thumb_height, 7.0);
        assert_eq!(hover.track_y, 33.0);
    }

    #[test]
    fn test_min_thumb_length_applies() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);

        let metrics = vertical_metrics(frame(100.0, 50.0, 100.0, 5000.0), &attrs).unwrap();
        assert_eq!(metrics.thumb_len, SCROLLBAR_MIN_LENGTH);
    }

    #[test]
    fn test_scroll_offset_is_clamped_for_thumb_position() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(9999.0);

        let max = vertical_metrics(frame(100.0, 50.0, 100.0, 150.0), &attrs).unwrap();
        assert!((max.thumb_start - 26.0).abs() < 0.001);

        attrs.scroll_y = Some(-123.0);
        let min = vertical_metrics(frame(100.0, 50.0, 100.0, 150.0), &attrs).unwrap();
        assert!((min.thumb_start - 0.0).abs() < 0.001);
    }
}
