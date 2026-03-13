use super::box_model::{border_radius_uniform, content_rect};
use super::color::color_to_u32;
use super::scope::{RenderItem, RenderScope};
use crate::assets::{self, AssetStatus};
use crate::renderer::DrawCmd;
use crate::tree::attrs::{
    Attrs, Background, BorderRadius, BorderStyle, BorderWidth, ImageFit, ImageSource,
};
use crate::tree::element::Frame;
use crate::tree::geometry::{ClipShape, Rect, self_shape as geometry_self_shape};
use crate::tree::scene::ResolvedNodeState;
use crate::tree::scrollbar;

pub(super) const SCROLLBAR_COLOR: u32 = 0xD0D5DC99;

pub(super) fn collect_box_shadow_items(
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
    inset: bool,
) -> Vec<RenderItem> {
    let Some(shadows) = &attrs.box_shadows else {
        return Vec::new();
    };

    let rect = Rect::from_frame(frame);
    let radius = border_radius_uniform(radius);

    shadows
        .iter()
        .filter(|shadow| shadow.inset == inset)
        .map(|shadow| {
            let offset_x = shadow.offset_x as f32;
            let offset_y = shadow.offset_y as f32;
            let blur = shadow.blur as f32;
            let size = shadow.size as f32;
            let color = color_to_u32(&shadow.color);

            RenderItem::Draw(if inset {
                DrawCmd::InsetShadow(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    offset_x,
                    offset_y,
                    blur,
                    size,
                    radius,
                    color,
                )
            } else {
                DrawCmd::Shadow(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    offset_x,
                    offset_y,
                    blur,
                    size,
                    radius,
                    color,
                )
            })
        })
        .collect()
}

pub(super) fn build_background_items(
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
) -> Vec<RenderItem> {
    let Some(background) = &attrs.background else {
        return Vec::new();
    };

    match background {
        Background::Color(color) => {
            let fill = color_to_u32(color);
            collect_background_rect_items(frame, radius, fill)
        }
        Background::Gradient { from, to, angle } => vec![RenderItem::Draw(DrawCmd::Gradient(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            color_to_u32(from),
            color_to_u32(to),
            *angle as f32,
            border_radius_uniform(radius),
        ))],
        Background::Image { source, fit } => {
            let image_item = paint_item_for_image_source(Rect::from_frame(frame), source, *fit);
            let shape = geometry_self_shape(frame, attrs);
            let clip = ClipShape {
                rect: shape.rect,
                radii: shape.radii,
            };
            if shape.radii.is_some() {
                vec![RenderItem::Scope(RenderScope {
                    local_clip: Some(clip),
                    items: image_item.into_iter().map(RenderItem::Draw).collect(),
                    ..RenderScope::default()
                })]
            } else {
                image_item.into_iter().map(RenderItem::Draw).collect()
            }
        }
    }
}

pub(super) fn render_image_items(frame: Frame, attrs: &Attrs) -> Vec<RenderItem> {
    let Some(source) = attrs.image_src.as_ref() else {
        return Vec::new();
    };

    let fit = attrs.image_fit.unwrap_or(ImageFit::Contain);
    let (draw_x, draw_y, draw_w, draw_h) = content_rect(frame, attrs);

    paint_item_for_image_source(
        Rect {
            x: draw_x,
            y: draw_y,
            width: draw_w,
            height: draw_h,
        },
        source,
        fit,
    )
    .into_iter()
    .map(RenderItem::Draw)
    .collect()
}

pub(super) fn render_video_items(frame: Frame, attrs: &Attrs) -> Vec<RenderItem> {
    let Some(target_id) = attrs.video_target.as_ref() else {
        return Vec::new();
    };

    let fit = attrs.image_fit.unwrap_or(ImageFit::Contain);
    let (draw_x, draw_y, draw_w, draw_h) = content_rect(frame, attrs);

    if draw_w <= 0.0 || draw_h <= 0.0 {
        return Vec::new();
    }

    vec![RenderItem::Draw(DrawCmd::Video(
        draw_x,
        draw_y,
        draw_w,
        draw_h,
        target_id.clone(),
        fit,
    ))]
}

fn paint_item_for_image_source(rect: Rect, source: &ImageSource, fit: ImageFit) -> Option<DrawCmd> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }

    assets::ensure_source(source);

    match assets::source_status(source) {
        Some(AssetStatus::Ready(asset)) => Some(DrawCmd::Image(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            asset.id,
            fit,
        )),
        Some(AssetStatus::Failed) => Some(DrawCmd::ImageFailed(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        )),
        _ => Some(DrawCmd::ImageLoading(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        )),
    }
}

pub(super) fn collect_scrollbar_items(
    scene_state: Option<&ResolvedNodeState>,
    frame: Frame,
    attrs: &Attrs,
) -> Vec<RenderItem> {
    let mut items = Vec::new();

    if let Some(metrics) = scene_state
        .and_then(|state| state.scrollbar_y)
        .or_else(|| scrollbar::vertical_metrics(frame, attrs))
    {
        items.push(RenderItem::Draw(DrawCmd::RoundedRect(
            metrics.thumb_x,
            metrics.thumb_y,
            metrics.thumb_width,
            metrics.thumb_height,
            metrics.thumb_width / 2.0,
            SCROLLBAR_COLOR,
        )));
    }

    if let Some(metrics) = scene_state
        .and_then(|state| state.scrollbar_x)
        .or_else(|| scrollbar::horizontal_metrics(frame, attrs))
    {
        items.push(RenderItem::Draw(DrawCmd::RoundedRect(
            metrics.thumb_x,
            metrics.thumb_y,
            metrics.thumb_width,
            metrics.thumb_height,
            metrics.thumb_height / 2.0,
            SCROLLBAR_COLOR,
        )));
    }

    items
}

pub(super) fn collect_background_rect_items(
    frame: Frame,
    radius: Option<&BorderRadius>,
    fill: u32,
) -> Vec<RenderItem> {
    let item = match radius {
        Some(BorderRadius::Uniform(value)) if *value > 0.0 => Some(DrawCmd::RoundedRect(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            *value as f32,
            fill,
        )),
        Some(BorderRadius::Corners { tl, tr, br, bl }) => Some(DrawCmd::RoundedRectCorners(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            *tl as f32,
            *tr as f32,
            *br as f32,
            *bl as f32,
            fill,
        )),
        _ => Some(DrawCmd::Rect(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            fill,
        )),
    };

    item.into_iter().map(RenderItem::Draw).collect()
}

pub(super) fn collect_border_items(frame: Frame, attrs: &Attrs) -> Vec<RenderItem> {
    let radius = attrs.border_radius.as_ref();
    let Some(border_width) = attrs.border_width.as_ref() else {
        return Vec::new();
    };
    let Some(border_color) = attrs.border_color.as_ref() else {
        return Vec::new();
    };

    let color = color_to_u32(border_color);
    let style = attrs.border_style.unwrap_or(BorderStyle::Solid);

    let item = match border_width {
        BorderWidth::Uniform(w) if *w > 0.0 => match radius {
            Some(BorderRadius::Uniform(value)) if *value > 0.0 => Some(DrawCmd::Border(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *value as f32,
                *w as f32,
                color,
                style,
            )),
            Some(BorderRadius::Corners { tl, tr, br, bl }) => Some(DrawCmd::BorderCorners(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *tl as f32,
                *tr as f32,
                *br as f32,
                *bl as f32,
                *w as f32,
                color,
                style,
            )),
            _ => Some(DrawCmd::Border(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                0.0,
                *w as f32,
                color,
                style,
            )),
        },
        BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        } => Some(DrawCmd::BorderEdges(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            border_radius_uniform(radius),
            *top as f32,
            *right as f32,
            *bottom as f32,
            *left as f32,
            color,
            style,
        )),
        _ => None,
    };

    item.into_iter().map(RenderItem::Draw).collect()
}
