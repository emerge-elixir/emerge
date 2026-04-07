use super::box_model::{border_radius_uniform, content_rect};
use super::color::color_to_u32;
use crate::assets::{self, AssetStatus};
use crate::render_scene::{DrawPrimitive, RenderNode};
use crate::tree::attrs::{
    Attrs, Background, BorderRadius, BorderStyle, BorderWidth, ImageFit, ImageSource,
};
use crate::tree::element::Frame;
use crate::tree::geometry::{ClipShape, Rect, self_shape as geometry_self_shape};
use crate::tree::scene::ResolvedNodeState;
use crate::tree::scrollbar;

pub(super) const SCROLLBAR_COLOR: u32 = 0xD0D5DC99;

pub(super) fn collect_box_shadow_nodes(
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
    inset: bool,
) -> Vec<RenderNode> {
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

            RenderNode::Primitive(if inset {
                DrawPrimitive::InsetShadow(
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
                DrawPrimitive::Shadow(
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

pub(super) fn build_background_nodes(frame: Frame, attrs: &Attrs) -> Vec<RenderNode> {
    let Some(background) = &attrs.background else {
        return Vec::new();
    };

    let nodes = match background {
        Background::Color(color) => collect_background_rect_nodes(frame, color_to_u32(color)),
        Background::Gradient { from, to, angle } => {
            vec![RenderNode::Primitive(DrawPrimitive::Gradient(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                color_to_u32(from),
                color_to_u32(to),
                *angle as f32,
            ))]
        }
        Background::Image { source, fit } => {
            paint_node_for_image_source(Rect::from_frame(frame), source, *fit, None, false)
                .into_iter()
                .collect()
        }
    };

    wrap_with_self_clip(nodes, frame, attrs)
}

pub(super) fn render_image_nodes(frame: Frame, attrs: &Attrs) -> Vec<RenderNode> {
    let Some(source) = attrs.image_src.as_ref() else {
        return Vec::new();
    };

    let fit = attrs.image_fit.unwrap_or(ImageFit::Contain);
    let (draw_x, draw_y, draw_w, draw_h) = content_rect(frame, attrs);

    paint_node_for_image_source(
        Rect {
            x: draw_x,
            y: draw_y,
            width: draw_w,
            height: draw_h,
        },
        source,
        fit,
        attrs.svg_color.as_ref().map(color_to_u32),
        attrs.svg_expected.unwrap_or(false),
    )
    .into_iter()
    .collect()
}

pub(super) fn render_video_nodes(frame: Frame, attrs: &Attrs) -> Vec<RenderNode> {
    let Some(target_id) = attrs.video_target.as_ref() else {
        return Vec::new();
    };

    let fit = attrs.image_fit.unwrap_or(ImageFit::Contain);
    let (draw_x, draw_y, draw_w, draw_h) = content_rect(frame, attrs);

    if draw_w <= 0.0 || draw_h <= 0.0 {
        return Vec::new();
    }

    vec![RenderNode::Primitive(DrawPrimitive::Video(
        draw_x,
        draw_y,
        draw_w,
        draw_h,
        target_id.clone(),
        fit,
    ))]
}

fn paint_node_for_image_source(
    rect: Rect,
    source: &ImageSource,
    fit: ImageFit,
    svg_tint: Option<u32>,
    svg_expected: bool,
) -> Option<RenderNode> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }

    assets::ensure_source(source);

    match assets::source_status(source) {
        Some(AssetStatus::Ready(asset)) => {
            let asset_is_vector = matches!(
                crate::renderer::asset_kind(&asset.id),
                Some(crate::renderer::AssetKind::Vector)
            );

            if svg_expected && !asset_is_vector {
                Some(RenderNode::Primitive(DrawPrimitive::ImageFailed(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                )))
            } else {
                Some(RenderNode::Primitive(DrawPrimitive::Image(
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    asset.id,
                    fit,
                    if svg_expected && asset_is_vector {
                        svg_tint
                    } else {
                        None
                    },
                )))
            }
        }
        Some(AssetStatus::Failed) => Some(RenderNode::Primitive(DrawPrimitive::ImageFailed(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        ))),
        _ => Some(RenderNode::Primitive(DrawPrimitive::ImageLoading(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        ))),
    }
}

pub(super) fn collect_scrollbar_nodes(
    scene_state: Option<&ResolvedNodeState>,
    frame: Frame,
    attrs: &Attrs,
) -> Vec<RenderNode> {
    let mut nodes = Vec::new();

    if let Some(metrics) = scene_state
        .and_then(|state| state.scrollbar_y)
        .or_else(|| scrollbar::vertical_metrics(frame, attrs))
    {
        nodes.push(RenderNode::Primitive(DrawPrimitive::RoundedRect(
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
        nodes.push(RenderNode::Primitive(DrawPrimitive::RoundedRect(
            metrics.thumb_x,
            metrics.thumb_y,
            metrics.thumb_width,
            metrics.thumb_height,
            metrics.thumb_height / 2.0,
            SCROLLBAR_COLOR,
        )));
    }

    nodes
}

pub(super) fn collect_background_rect_nodes(frame: Frame, fill: u32) -> Vec<RenderNode> {
    vec![RenderNode::Primitive(DrawPrimitive::Rect(
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        fill,
    ))]
}

fn wrap_with_self_clip(nodes: Vec<RenderNode>, frame: Frame, attrs: &Attrs) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    let Some(clip) = self_clip_shape(frame, attrs) else {
        return nodes;
    };

    vec![RenderNode::Clip {
        clips: vec![clip],
        children: nodes,
    }]
}

fn self_clip_shape(frame: Frame, attrs: &Attrs) -> Option<ClipShape> {
    let shape = geometry_self_shape(frame, attrs);
    let radii = shape
        .radii
        .filter(|radii| radii.tl > 0.0 || radii.tr > 0.0 || radii.br > 0.0 || radii.bl > 0.0)?;

    Some(ClipShape {
        rect: shape.rect,
        radii: Some(radii),
    })
}

pub(super) fn collect_border_nodes(frame: Frame, attrs: &Attrs) -> Vec<RenderNode> {
    let radius = attrs.border_radius.as_ref();
    let Some(border_width) = attrs.border_width.as_ref() else {
        return Vec::new();
    };
    let Some(border_color) = attrs.border_color.as_ref() else {
        return Vec::new();
    };

    let color = color_to_u32(border_color);
    let style = attrs.border_style.unwrap_or(BorderStyle::Solid);

    let primitive = match border_width {
        BorderWidth::Uniform(w) if *w > 0.0 => match radius {
            Some(BorderRadius::Uniform(value)) if *value > 0.0 => Some(DrawPrimitive::Border(
                frame.x,
                frame.y,
                frame.width,
                frame.height,
                *value as f32,
                *w as f32,
                color,
                style,
            )),
            Some(BorderRadius::Corners { tl, tr, br, bl }) => Some(DrawPrimitive::BorderCorners(
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
            _ => Some(DrawPrimitive::Border(
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
        } => Some(DrawPrimitive::BorderEdges(
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

    primitive.into_iter().map(RenderNode::Primitive).collect()
}
