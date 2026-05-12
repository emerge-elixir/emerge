use std::fmt;
use std::hash::{Hash, Hasher};

use crate::tree::attrs::{BorderStyle, ImageFit};
use crate::tree::geometry::{ClipShape, Rect};
use crate::tree::transform::Affine2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScene {
    pub nodes: Vec<RenderNode>,
}

impl RenderScene {
    pub fn summary(&self) -> RenderSceneSummary {
        let mut summary = RenderSceneSummary::default();
        summary.record_nodes(&self.nodes);
        summary
    }

    pub fn has_moving_paint_layers(&self) -> bool {
        nodes_have_moving_paint_layers(&self.nodes)
    }
}

fn nodes_have_moving_paint_layers(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => nodes_have_moving_paint_layers(children),
        RenderNode::PaintLayer(layer) => {
            layer.placement == PaintLayerPlacement::ScrollMoving
                || nodes_have_moving_paint_layers(&layer.children)
        }
        RenderNode::Primitive(_) => false,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderSceneSummary {
    pub nodes: usize,
    pub shadow_passes: usize,
    pub clips: usize,
    pub relaxed_clips: usize,
    pub clip_shapes: usize,
    pub transforms: usize,
    pub alphas: usize,
    pub primitives: usize,
    pub rects: usize,
    pub rounded_rects: usize,
    pub borders: usize,
    pub border_corners: usize,
    pub border_edges: usize,
    pub shadows: usize,
    pub inset_shadows: usize,
    pub texts: usize,
    pub text_bytes: usize,
    pub gradients: usize,
    pub images: usize,
    pub videos: usize,
    pub image_loading: usize,
    pub image_failed: usize,
    pub paint_layers: usize,
    pub cacheable_layers: usize,
    pub dynamic_layers: usize,
    pub moving_layers: usize,
    pub direct_only_layers: usize,
}

impl RenderSceneSummary {
    fn record_nodes(&mut self, nodes: &[RenderNode]) {
        for node in nodes {
            self.nodes += 1;

            match node {
                RenderNode::ShadowPass { children } => {
                    self.shadow_passes += 1;
                    self.record_nodes(children);
                }
                RenderNode::Clip { clips, children } => {
                    self.clips += 1;
                    self.clip_shapes += clips.len();
                    self.record_nodes(children);
                }
                RenderNode::RelaxedClip { clips, children } => {
                    self.relaxed_clips += 1;
                    self.clip_shapes += clips.len();
                    self.record_nodes(children);
                }
                RenderNode::Transform { children, .. } => {
                    self.transforms += 1;
                    self.record_nodes(children);
                }
                RenderNode::Alpha { children, .. } => {
                    self.alphas += 1;
                    self.record_nodes(children);
                }
                RenderNode::PaintLayer(layer) => {
                    self.paint_layers += 1;
                    match layer.policy {
                        PaintLayerPolicy::Cacheable => self.cacheable_layers += 1,
                        PaintLayerPolicy::DynamicRedraw => self.dynamic_layers += 1,
                        PaintLayerPolicy::DirectOnly => self.direct_only_layers += 1,
                    }
                    if layer.placement == PaintLayerPlacement::ScrollMoving {
                        self.moving_layers += 1;
                    }
                    self.record_nodes(&layer.children);
                }
                RenderNode::Primitive(primitive) => self.record_primitive(primitive),
            }
        }
    }

    fn record_primitive(&mut self, primitive: &DrawPrimitive) {
        self.primitives += 1;

        match primitive {
            DrawPrimitive::Rect(..) => self.rects += 1,
            DrawPrimitive::RoundedRect(..) => self.rounded_rects += 1,
            DrawPrimitive::Border(..) => self.borders += 1,
            DrawPrimitive::BorderCorners(..) => self.border_corners += 1,
            DrawPrimitive::BorderEdges(..) => self.border_edges += 1,
            DrawPrimitive::Shadow(..) => self.shadows += 1,
            DrawPrimitive::InsetShadow(..) => self.inset_shadows += 1,
            DrawPrimitive::TextWithFont(_, _, text, ..) => {
                self.texts += 1;
                self.text_bytes += text.len();
            }
            DrawPrimitive::Gradient(..) => self.gradients += 1,
            DrawPrimitive::Image(..) => self.images += 1,
            DrawPrimitive::Video(..) => self.videos += 1,
            DrawPrimitive::ImageLoading(..) => self.image_loading += 1,
            DrawPrimitive::ImageFailed(..) => self.image_failed += 1,
        }
    }
}

impl fmt::Display for RenderSceneSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            concat!(
                "nodes={} primitives={} scopes={{shadow_passes={} clips={} relaxed_clips={} ",
                "clip_shapes={} transforms={} alphas={}}} draws={{rects={} rounded_rects={} ",
                "borders={} border_corners={} border_edges={} shadows={} inset_shadows={} ",
                "texts={} text_bytes={} gradients={} images={} videos={} image_loading={} ",
                "image_failed={}}} paint_layers={{total={} cacheable={} dynamic={} moving={} ",
                "direct_only={}}}"
            ),
            self.nodes,
            self.primitives,
            self.shadow_passes,
            self.clips,
            self.relaxed_clips,
            self.clip_shapes,
            self.transforms,
            self.alphas,
            self.rects,
            self.rounded_rects,
            self.borders,
            self.border_corners,
            self.border_edges,
            self.shadows,
            self.inset_shadows,
            self.texts,
            self.text_bytes,
            self.gradients,
            self.images,
            self.videos,
            self.image_loading,
            self.image_failed,
            self.paint_layers,
            self.cacheable_layers,
            self.dynamic_layers,
            self.moving_layers,
            self.direct_only_layers
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderNode {
    ShadowPass {
        children: Vec<RenderNode>,
    },
    Clip {
        clips: Vec<ClipShape>,
        children: Vec<RenderNode>,
    },
    RelaxedClip {
        clips: Vec<ClipShape>,
        children: Vec<RenderNode>,
    },
    Transform {
        transform: Affine2,
        children: Vec<RenderNode>,
    },
    Alpha {
        alpha: f32,
        children: Vec<RenderNode>,
    },
    PaintLayer(RenderPaintLayer),
    Primitive(DrawPrimitive),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPaintLayer {
    pub stable_id: u64,
    pub bounds: Rect,
    pub placement: PaintLayerPlacement,
    pub policy: PaintLayerPolicy,
    pub reason: PaintLayerReason,
    pub content_generation: u64,
    pub children: Vec<RenderNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayerPlacement {
    Fixed,
    ScrollMoving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayerPolicy {
    Cacheable,
    DynamicRedraw,
    DirectOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayerReason {
    Root,
    ScrollContainer,
    StableSubtree,
    Animation,
    Nearby,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    Rect(f32, f32, f32, f32, u32),
    RoundedRect(f32, f32, f32, f32, f32, u32),
    Border(f32, f32, f32, f32, f32, f32, u32, BorderStyle),
    BorderCorners(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    BorderEdges(
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        u32,
        BorderStyle,
    ),
    Shadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    InsetShadow(f32, f32, f32, f32, f32, f32, f32, f32, f32, u32),
    TextWithFont(f32, f32, String, f32, u32, String, u16, bool),
    Gradient(f32, f32, f32, f32, u32, u32, f32),
    Image(f32, f32, f32, f32, String, ImageFit, Option<u32>),
    Video(f32, f32, f32, f32, String, ImageFit),
    ImageLoading(f32, f32, f32, f32),
    ImageFailed(f32, f32, f32, f32),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PaintLayerHashFloat {
    Exact,
    Quantized { scale: f64 },
}

impl PaintLayerHashFloat {
    pub(crate) fn hash_f32<H: Hasher>(self, hasher: &mut H, value: f32) {
        match self {
            Self::Exact => value.to_bits().hash(hasher),
            Self::Quantized { scale } => {
                let bucket = if value.is_finite() {
                    (f64::from(value) * scale).round() as i64
                } else if value.is_nan() {
                    i64::MIN
                } else if value.is_sign_positive() {
                    i64::MAX
                } else {
                    i64::MIN + 1
                };
                bucket.hash(hasher);
            }
        }
    }
}

pub(crate) fn hash_paint_layer_render_nodes<H: Hasher>(
    hasher: &mut H,
    nodes: &[RenderNode],
    float: PaintLayerHashFloat,
) {
    nodes.len().hash(hasher);
    for node in nodes {
        hash_paint_layer_render_node(hasher, node, float);
    }
}

pub(crate) fn hash_paint_layer_render_node<H: Hasher>(
    hasher: &mut H,
    node: &RenderNode,
    float: PaintLayerHashFloat,
) {
    match node {
        RenderNode::ShadowPass { children } => {
            0u8.hash(hasher);
            hash_paint_layer_render_nodes(hasher, children, float);
        }
        RenderNode::Clip { clips, children } => {
            1u8.hash(hasher);
            hash_paint_layer_clip_shapes(hasher, clips, float);
            hash_paint_layer_render_nodes(hasher, children, float);
        }
        RenderNode::RelaxedClip { clips, children } => {
            2u8.hash(hasher);
            hash_paint_layer_clip_shapes(hasher, clips, float);
            hash_paint_layer_render_nodes(hasher, children, float);
        }
        RenderNode::Transform {
            transform,
            children,
        } => {
            3u8.hash(hasher);
            hash_paint_layer_affine2(hasher, *transform, float);
            hash_paint_layer_render_nodes(hasher, children, float);
        }
        RenderNode::Alpha { alpha, children } => {
            4u8.hash(hasher);
            float.hash_f32(hasher, *alpha);
            hash_paint_layer_render_nodes(hasher, children, float);
        }
        RenderNode::PaintLayer(layer) => {
            5u8.hash(hasher);
            hash_paint_layer_metadata(hasher, layer);
            hash_paint_layer_rect(hasher, layer.bounds, float);
            hash_paint_layer_render_nodes(hasher, &layer.children, float);
        }
        RenderNode::Primitive(primitive) => {
            6u8.hash(hasher);
            hash_paint_layer_draw_primitive(hasher, primitive, float);
        }
    }
}

pub(crate) fn hash_paint_layer_metadata<H: Hasher>(hasher: &mut H, layer: &RenderPaintLayer) {
    layer.stable_id.hash(hasher);
    layer.placement.hash(hasher);
    layer.policy.hash(hasher);
    layer.reason.hash(hasher);
    layer.content_generation.hash(hasher);
}

pub(crate) fn hash_paint_layer_clip_shapes<H: Hasher>(
    hasher: &mut H,
    clips: &[ClipShape],
    float: PaintLayerHashFloat,
) {
    clips.len().hash(hasher);
    for clip in clips {
        hash_paint_layer_rect(hasher, clip.rect, float);
        match clip.radii {
            Some(radii) => {
                true.hash(hasher);
                hash_paint_layer_f32s(hasher, &[radii.tl, radii.tr, radii.br, radii.bl], float);
            }
            None => false.hash(hasher),
        }
    }
}

pub(crate) fn hash_paint_layer_rect<H: Hasher>(
    hasher: &mut H,
    rect: Rect,
    float: PaintLayerHashFloat,
) {
    hash_paint_layer_f32s(hasher, &[rect.x, rect.y, rect.width, rect.height], float);
}

pub(crate) fn hash_paint_layer_affine2<H: Hasher>(
    hasher: &mut H,
    transform: Affine2,
    float: PaintLayerHashFloat,
) {
    hash_paint_layer_f32s(
        hasher,
        &[
            transform.xx,
            transform.yx,
            transform.xy,
            transform.yy,
            transform.tx,
            transform.ty,
        ],
        float,
    );
}

pub(crate) fn hash_paint_layer_draw_primitive<H: Hasher>(
    hasher: &mut H,
    primitive: &DrawPrimitive,
    float: PaintLayerHashFloat,
) {
    match primitive {
        DrawPrimitive::Rect(x, y, w, h, color) => {
            0u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h], float);
            color.hash(hasher);
        }
        DrawPrimitive::RoundedRect(x, y, w, h, radius, color) => {
            1u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h, *radius], float);
            color.hash(hasher);
        }
        DrawPrimitive::Border(x, y, w, h, radius, width, color, style) => {
            2u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h, *radius, *width], float);
            color.hash(hasher);
            hash_paint_layer_border_style(hasher, *style);
        }
        DrawPrimitive::BorderCorners(x, y, w, h, tl, tr, br, bl, width, color, style) => {
            3u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h, *tl, *tr, *br, *bl, *width], float);
            color.hash(hasher);
            hash_paint_layer_border_style(hasher, *style);
        }
        DrawPrimitive::BorderEdges(x, y, w, h, radius, top, right, bottom, left, color, style) => {
            4u8.hash(hasher);
            hash_paint_layer_f32s(
                hasher,
                &[*x, *y, *w, *h, *radius, *top, *right, *bottom, *left],
                float,
            );
            color.hash(hasher);
            hash_paint_layer_border_style(hasher, *style);
        }
        DrawPrimitive::Shadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
            5u8.hash(hasher);
            hash_paint_layer_f32s(
                hasher,
                &[*x, *y, *w, *h, *offset_x, *offset_y, *blur, *size, *radius],
                float,
            );
            color.hash(hasher);
        }
        DrawPrimitive::InsetShadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
            6u8.hash(hasher);
            hash_paint_layer_f32s(
                hasher,
                &[*x, *y, *w, *h, *offset_x, *offset_y, *blur, *size, *radius],
                float,
            );
            color.hash(hasher);
        }
        DrawPrimitive::TextWithFont(x, y, text, font_size, fill, family, weight, italic) => {
            7u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *font_size], float);
            text.hash(hasher);
            fill.hash(hasher);
            family.hash(hasher);
            weight.hash(hasher);
            italic.hash(hasher);
        }
        DrawPrimitive::Gradient(x, y, w, h, from, to, angle) => {
            8u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h, *angle], float);
            from.hash(hasher);
            to.hash(hasher);
        }
        DrawPrimitive::Image(x, y, w, h, image_id, fit, tint) => {
            9u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h], float);
            image_id.hash(hasher);
            hash_paint_layer_image_fit(hasher, *fit);
            tint.hash(hasher);
        }
        DrawPrimitive::Video(x, y, w, h, target, fit) => {
            10u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h], float);
            target.hash(hasher);
            hash_paint_layer_image_fit(hasher, *fit);
        }
        DrawPrimitive::ImageLoading(x, y, w, h) => {
            11u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h], float);
        }
        DrawPrimitive::ImageFailed(x, y, w, h) => {
            12u8.hash(hasher);
            hash_paint_layer_f32s(hasher, &[*x, *y, *w, *h], float);
        }
    }
}

pub(crate) fn hash_paint_layer_f32s<H: Hasher>(
    hasher: &mut H,
    values: &[f32],
    float: PaintLayerHashFloat,
) {
    for value in values {
        float.hash_f32(hasher, *value);
    }
}

fn hash_paint_layer_border_style<H: Hasher>(hasher: &mut H, style: BorderStyle) {
    match style {
        BorderStyle::Solid => 0u8,
        BorderStyle::Dashed => 1u8,
        BorderStyle::Dotted => 2u8,
    }
    .hash(hasher);
}

fn hash_paint_layer_image_fit<H: Hasher>(hasher: &mut H, fit: ImageFit) {
    match fit {
        ImageFit::Contain => 0u8,
        ImageFit::Cover => 1u8,
        ImageFit::Repeat => 2u8,
        ImageFit::RepeatX => 3u8,
        ImageFit::RepeatY => 4u8,
    }
    .hash(hasher);
}
