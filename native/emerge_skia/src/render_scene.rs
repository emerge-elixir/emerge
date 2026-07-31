use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

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

    pub fn has_payload_cache_candidate_layers(&self) -> bool {
        nodes_have_payload_cache_candidate_layers(&self.nodes)
    }

    /// Compatibility name retained for downstream renderer integrations.
    ///
    /// Dynamic redraw layers can now become cacheable after admission, so this reports every
    /// payload-cache candidate rather than only layers whose policy is named `Cacheable`.
    pub fn has_cacheable_paint_layers(&self) -> bool {
        self.has_payload_cache_candidate_layers()
    }

    pub fn has_scroll_moving_paint_layers(&self) -> bool {
        nodes_have_scroll_moving_paint_layers(&self.nodes)
    }

    pub fn video_target_ids(&self) -> HashSet<String> {
        let mut targets = HashSet::new();
        collect_video_target_ids(&self.nodes, &mut targets);
        targets
    }
}

fn collect_video_target_ids(nodes: &[RenderNode], targets: &mut HashSet<String>) {
    nodes.iter().for_each(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => collect_video_target_ids(children, targets),
        RenderNode::PaintLayer(layer) => {
            collect_video_target_ids(&layer.own_nodes, targets);
            layer
                .child_refs
                .iter()
                .for_each(|child| collect_video_target_ids(&child.nodes, targets));
        }
        RenderNode::Primitive(DrawPrimitive::Video(_, _, _, _, target, _)) => {
            targets.insert(target.clone());
        }
        RenderNode::Primitive(_) => {}
    });
}

fn nodes_have_payload_cache_candidate_layers(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => nodes_have_payload_cache_candidate_layers(children),
        RenderNode::PaintLayer(layer) => {
            layer.policy.allows_payload_cache()
                || layer
                    .child_refs
                    .iter()
                    .any(|child| nodes_have_payload_cache_candidate_layers(&child.nodes))
        }
        RenderNode::Primitive(_) => false,
    })
}

fn nodes_have_scroll_moving_paint_layers(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => nodes_have_scroll_moving_paint_layers(children),
        RenderNode::PaintLayer(layer) => {
            layer.placement == PaintLayerPlacement::ScrollMoving
                || layer
                    .child_refs
                    .iter()
                    .any(|child| nodes_have_scroll_moving_paint_layers(&child.nodes))
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
                    self.record_nodes(&layer.own_nodes);
                    layer
                        .child_refs
                        .iter()
                        .for_each(|child| self.record_nodes(&child.nodes));
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
    pub root_id: u64,
    pub bounds: Rect,
    pub placement: PaintLayerPlacement,
    pub policy: PaintLayerPolicy,
    pub reason: PaintLayerReason,
    pub content_generation: u64,
    pub own_nodes: Arc<Vec<RenderNode>>,
    pub child_refs: Arc<Vec<RenderPaintLayerChildRef>>,
    pub metrics: RenderPaintLayerMetrics,
}

impl RenderPaintLayer {
    pub fn from_children(
        stable_id: u64,
        bounds: Rect,
        placement: PaintLayerPlacement,
        policy: PaintLayerPolicy,
        reason: PaintLayerReason,
        content_generation: u64,
        children: Vec<RenderNode>,
    ) -> Self {
        let content = split_paint_layer_content_owned(children);
        Self::from_prepared_children(
            RenderPaintLayerBuildParts {
                stable_id,
                root_id: stable_id,
                bounds,
                placement,
                policy,
                reason,
                content_generation,
                visual_bounds: None,
            },
            content,
        )
    }

    pub(crate) fn from_prepared_children(
        parts: RenderPaintLayerBuildParts,
        content: RenderPaintLayerContent,
    ) -> Self {
        let metrics = RenderPaintLayerMetrics::from_own_nodes(
            &content.own_nodes,
            parts.bounds,
            parts.visual_bounds,
        );

        Self {
            stable_id: parts.stable_id,
            root_id: parts.root_id,
            bounds: parts.bounds,
            placement: parts.placement,
            policy: parts.policy,
            reason: parts.reason,
            content_generation: parts.content_generation,
            own_nodes: Arc::new(content.own_nodes),
            child_refs: Arc::new(content.child_refs),
            metrics,
        }
    }

    pub(crate) fn content_nodes(&self) -> Vec<RenderNode> {
        self.own_nodes
            .iter()
            .cloned()
            .chain(
                self.child_refs
                    .iter()
                    .flat_map(|child| child.nodes.iter().cloned()),
            )
            .collect()
    }

    pub(crate) fn with_children(&self, children: Vec<RenderNode>) -> Self {
        self.with_bounds_and_children(self.bounds, children)
    }

    pub(crate) fn with_bounds_and_children(&self, bounds: Rect, children: Vec<RenderNode>) -> Self {
        let content = split_paint_layer_content_owned(children);
        Self::from_prepared_children(
            RenderPaintLayerBuildParts {
                stable_id: self.stable_id,
                root_id: self.root_id,
                bounds,
                placement: self.placement,
                policy: self.policy,
                reason: self.reason,
                content_generation: self.content_generation,
                visual_bounds: None,
            },
            content,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPaintLayerBuildParts {
    pub stable_id: u64,
    pub root_id: u64,
    pub bounds: Rect,
    pub placement: PaintLayerPlacement,
    pub policy: PaintLayerPolicy,
    pub reason: PaintLayerReason,
    pub content_generation: u64,
    pub visual_bounds: Option<Rect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderPaintLayerMetrics {
    pub own_node_count: u32,
    pub own_primitive_count: u32,
    pub own_primitive_cost: u64,
    pub payload_pixels: u64,
    pub visual_bounds: Rect,
    pub opaque: bool,
}

impl RenderPaintLayerMetrics {
    fn from_own_nodes(nodes: &[RenderNode], bounds: Rect, visual_bounds: Option<Rect>) -> Self {
        let visual_bounds = visual_bounds
            .or_else(|| render_nodes_visual_bounds(nodes, Affine2::identity()))
            .map(|content_bounds| union_rect(bounds, content_bounds))
            .unwrap_or(bounds);
        let node_metrics = render_nodes_metrics(nodes);

        Self {
            own_node_count: node_metrics.node_count.min(u32::MAX as usize) as u32,
            own_primitive_count: node_metrics.primitive_count.min(u32::MAX as usize) as u32,
            own_primitive_cost: node_metrics.primitive_cost,
            payload_pixels: paint_layer_payload_pixels(bounds),
            visual_bounds,
            opaque: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RenderPaintLayerContent {
    pub own_nodes: Vec<RenderNode>,
    pub child_refs: Vec<RenderPaintLayerChildRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPaintLayerChildRef {
    pub nodes: Arc<Vec<RenderNode>>,
}

impl RenderPaintLayerChildRef {
    pub(crate) fn from_nodes(nodes: Vec<RenderNode>) -> Self {
        Self {
            nodes: Arc::new(nodes),
        }
    }
}

#[cfg(test)]
pub(crate) fn split_paint_layer_content(nodes: &[RenderNode]) -> RenderPaintLayerContent {
    nodes
        .iter()
        .cloned()
        .fold(RenderPaintLayerContent::default(), |mut content, node| {
            let mut split = split_paint_layer_node(node);
            content.own_nodes.append(&mut split.own_nodes);
            content.child_refs.append(&mut split.child_refs);
            content
        })
}

pub(crate) fn split_paint_layer_content_owned(nodes: Vec<RenderNode>) -> RenderPaintLayerContent {
    // Renderers draw `own_nodes` before `child_refs`, so only the contiguous
    // prefix before the first nested paint layer can stay in `own_nodes`.
    // Everything after that boundary remains in ordered child refs to avoid
    // repainting dirty child layers over later clean siblings.
    nodes
        .into_iter()
        .fold(RenderPaintLayerContent::default(), |mut content, node| {
            if content.child_refs.is_empty() {
                let mut split = split_paint_layer_node(node);
                content.own_nodes.append(&mut split.own_nodes);
                content.child_refs.append(&mut split.child_refs);
            } else {
                content
                    .child_refs
                    .push(RenderPaintLayerChildRef::from_nodes(vec![node]));
            }
            content
        })
}

pub(crate) fn paint_layer_own_content_visual_bounds(nodes: &[RenderNode]) -> Option<Rect> {
    render_nodes_visual_bounds(nodes, Affine2::identity())
}

pub(crate) fn paint_layer_bounds_from_visual_bounds(
    visual_bounds: Option<Rect>,
    fallback: Rect,
) -> Rect {
    visual_bounds
        .map(|bounds| union_rect(fallback, bounds))
        .unwrap_or(fallback)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RenderNodeMetrics {
    node_count: usize,
    primitive_count: usize,
    primitive_cost: u64,
}

fn render_nodes_metrics(nodes: &[RenderNode]) -> RenderNodeMetrics {
    nodes.iter().map(render_node_metrics).fold(
        RenderNodeMetrics::default(),
        |mut total, metrics| {
            total.node_count = total.node_count.saturating_add(metrics.node_count);
            total.primitive_count = total
                .primitive_count
                .saturating_add(metrics.primitive_count);
            total.primitive_cost = total.primitive_cost.saturating_add(metrics.primitive_cost);
            total
        },
    )
}

fn render_node_metrics(node: &RenderNode) -> RenderNodeMetrics {
    match node {
        RenderNode::ShadowPass { children } => {
            let mut metrics = render_nodes_metrics(children);
            metrics.node_count = metrics.node_count.saturating_add(1);
            metrics.primitive_cost = metrics.primitive_cost.saturating_add(12);
            metrics
        }
        RenderNode::Clip { children, .. } | RenderNode::RelaxedClip { children, .. } => {
            let mut metrics = render_nodes_metrics(children);
            metrics.node_count = metrics.node_count.saturating_add(1);
            metrics.primitive_cost = metrics.primitive_cost.saturating_add(3);
            metrics
        }
        RenderNode::Transform { children, .. } | RenderNode::Alpha { children, .. } => {
            let mut metrics = render_nodes_metrics(children);
            metrics.node_count = metrics.node_count.saturating_add(1);
            metrics.primitive_cost = metrics.primitive_cost.saturating_add(2);
            metrics
        }
        RenderNode::PaintLayer(_) => RenderNodeMetrics {
            node_count: 1,
            primitive_count: 0,
            primitive_cost: 0,
        },
        RenderNode::Primitive(primitive) => RenderNodeMetrics {
            node_count: 1,
            primitive_count: 1,
            primitive_cost: render_primitive_cost(primitive),
        },
    }
}

fn render_primitive_cost(primitive: &DrawPrimitive) -> u64 {
    match primitive {
        DrawPrimitive::Rect(..) => 1,
        DrawPrimitive::RoundedRect(..) => 2,
        DrawPrimitive::Border(.., style)
        | DrawPrimitive::BorderCorners(.., style)
        | DrawPrimitive::BorderEdges(.., style) => match style {
            BorderStyle::Solid => 4,
            BorderStyle::Dashed | BorderStyle::Dotted => 16,
        },
        DrawPrimitive::Shadow(..) | DrawPrimitive::InsetShadow(..) => 80,
        DrawPrimitive::TextWithFont(_, _, text, ..) => 32u64.saturating_add(text.len() as u64 / 2),
        DrawPrimitive::Gradient(..) => 12,
        DrawPrimitive::Image(..) => 20,
        DrawPrimitive::Video(..) => 24,
        DrawPrimitive::ImageLoading(..) | DrawPrimitive::ImageFailed(..) => 4,
    }
}

fn paint_layer_payload_pixels(bounds: Rect) -> u64 {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return 0;
    }

    let width = bounds.width.ceil() as u64;
    let height = bounds.height.ceil() as u64;
    width.saturating_mul(height)
}

fn render_nodes_visual_bounds(nodes: &[RenderNode], transform: Affine2) -> Option<Rect> {
    nodes
        .iter()
        .filter_map(|node| render_node_visual_bounds(node, transform))
        .reduce(union_rect)
}

fn render_node_visual_bounds(node: &RenderNode, transform: Affine2) -> Option<Rect> {
    match node {
        RenderNode::ShadowPass { children } | RenderNode::Alpha { children, .. } => {
            render_nodes_visual_bounds(children, transform)
        }
        RenderNode::Clip { clips, children } | RenderNode::RelaxedClip { clips, children } => {
            render_clipped_nodes_visual_bounds(clips, children, transform)
        }
        RenderNode::Transform {
            transform: child_transform,
            children,
        } => render_nodes_visual_bounds(children, transform.then(*child_transform)),
        RenderNode::PaintLayer(_) => None,
        RenderNode::Primitive(primitive) => {
            Some(transform.map_rect_aabb(draw_primitive_visual_bounds(primitive)))
        }
    }
}

fn render_clipped_nodes_visual_bounds(
    clips: &[ClipShape],
    children: &[RenderNode],
    transform: Affine2,
) -> Option<Rect> {
    if clips.is_empty() {
        return render_nodes_visual_bounds(children, transform);
    }

    let clip_bounds = clips
        .iter()
        .map(|clip| transform.map_rect_aabb(clip.rect))
        .reduce(union_rect)?;

    children
        .iter()
        .filter_map(|child| match child {
            RenderNode::ShadowPass { children } => render_nodes_visual_bounds(children, transform),
            _ => render_node_visual_bounds(child, transform)
                .and_then(|bounds| bounds.intersect(clip_bounds)),
        })
        .reduce(union_rect)
}

pub(crate) fn draw_primitive_visual_bounds(primitive: &DrawPrimitive) -> Rect {
    match primitive {
        DrawPrimitive::Rect(x, y, w, h, _)
        | DrawPrimitive::RoundedRect(x, y, w, h, _, _)
        | DrawPrimitive::Gradient(x, y, w, h, _, _, _)
        | DrawPrimitive::Image(x, y, w, h, _, _, _)
        | DrawPrimitive::Video(x, y, w, h, _, _)
        | DrawPrimitive::ImageLoading(x, y, w, h)
        | DrawPrimitive::ImageFailed(x, y, w, h) => Rect {
            x: *x,
            y: *y,
            width: *w,
            height: *h,
        },
        DrawPrimitive::Border(x, y, w, h, _, width, _, _) => {
            outset_rect(rect_from_xywh(*x, *y, *w, *h), *width / 2.0)
        }
        DrawPrimitive::BorderCorners(x, y, w, h, _, _, _, _, width, _, _) => {
            outset_rect(rect_from_xywh(*x, *y, *w, *h), *width / 2.0)
        }
        DrawPrimitive::BorderEdges(x, y, w, h, _, top, right, bottom, left, _, _) => {
            let outset = top.max(*right).max(*bottom).max(*left) / 2.0;
            outset_rect(rect_from_xywh(*x, *y, *w, *h), outset)
        }
        DrawPrimitive::Shadow(x, y, w, h, offset_x, offset_y, blur, size, _, _) => {
            let pad = blur.abs() * 2.0 + size.abs();
            Rect {
                x: *x + *offset_x - pad,
                y: *y + *offset_y - pad,
                width: *w + pad * 2.0,
                height: *h + pad * 2.0,
            }
        }
        DrawPrimitive::InsetShadow(x, y, w, h, _, _, _, _, _, _) => rect_from_xywh(*x, *y, *w, *h),
        DrawPrimitive::TextWithFont(x, y, text, font_size, _, _, _, _) => {
            let width = text.chars().count() as f32 * *font_size * 0.65;
            Rect {
                x: *x,
                y: *y - *font_size,
                width,
                height: *font_size * 1.25,
            }
        }
    }
}

fn rect_from_xywh(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn outset_rect(rect: Rect, outset: f32) -> Rect {
    let outset = outset.max(0.0);
    Rect {
        x: rect.x - outset,
        y: rect.y - outset,
        width: rect.width + outset * 2.0,
        height: rect.height + outset * 2.0,
    }
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_x = (a.x + a.width).max(b.x + b.width);
    let max_y = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

fn split_paint_layer_node(node: RenderNode) -> RenderPaintLayerContent {
    match node {
        RenderNode::ShadowPass { children } => {
            split_scoped_paint_layer_content(children, |children| RenderNode::ShadowPass {
                children,
            })
        }
        RenderNode::Clip { clips, children } => {
            split_scoped_paint_layer_content(children, |children| RenderNode::Clip {
                clips: clips.clone(),
                children,
            })
        }
        RenderNode::RelaxedClip { clips, children } => {
            split_scoped_paint_layer_content(children, |children| RenderNode::RelaxedClip {
                clips: clips.clone(),
                children,
            })
        }
        RenderNode::Transform {
            transform,
            children,
        } => split_scoped_paint_layer_content(children, |children| RenderNode::Transform {
            transform,
            children,
        }),
        RenderNode::Alpha { alpha, children } => {
            split_scoped_paint_layer_content(children, |children| RenderNode::Alpha {
                alpha,
                children,
            })
        }
        RenderNode::PaintLayer(_) => RenderPaintLayerContent {
            own_nodes: Vec::new(),
            child_refs: vec![RenderPaintLayerChildRef::from_nodes(vec![node])],
        },
        RenderNode::Primitive(_) => RenderPaintLayerContent {
            own_nodes: vec![node],
            child_refs: Vec::new(),
        },
    }
}

fn split_scoped_paint_layer_content(
    children: Vec<RenderNode>,
    wrap: impl Fn(Vec<RenderNode>) -> RenderNode + Copy,
) -> RenderPaintLayerContent {
    let content = split_paint_layer_content_owned(children);
    let own_nodes = if content.own_nodes.is_empty() {
        Vec::new()
    } else {
        vec![wrap(content.own_nodes)]
    };
    let child_refs = content
        .child_refs
        .into_iter()
        .map(|child| {
            RenderPaintLayerChildRef::from_nodes(vec![wrap(child.nodes.iter().cloned().collect())])
        })
        .collect();

    RenderPaintLayerContent {
        own_nodes,
        child_refs,
    }
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

impl PaintLayerPolicy {
    pub(crate) fn allows_payload_cache(self) -> bool {
        self != Self::DirectOnly
    }
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
    payload_bounds: Option<Rect>,
) {
    let mut hashed_nodes = 0usize;
    for node in nodes {
        if hash_paint_layer_render_node(hasher, node, float, payload_bounds) {
            hashed_nodes += 1;
        }
    }
    hashed_nodes.hash(hasher);
}

pub(crate) fn hash_paint_layer_render_node<H: Hasher>(
    hasher: &mut H,
    node: &RenderNode,
    float: PaintLayerHashFloat,
    payload_bounds: Option<Rect>,
) -> bool {
    if payload_bounds
        .and_then(|bounds| {
            render_node_visual_bounds(node, Affine2::identity())
                .and_then(|node_bounds| node_bounds.intersect(bounds))
        })
        .is_none()
        && payload_bounds.is_some()
    {
        return false;
    }

    match node {
        RenderNode::ShadowPass { children } => {
            if !paint_layer_render_nodes_intersect_payload(children, payload_bounds) {
                return false;
            }
            0u8.hash(hasher);
            hash_paint_layer_render_nodes(hasher, children, float, payload_bounds);
            true
        }
        RenderNode::Clip { clips, children } => {
            let (clips, child_payload_bounds) =
                paint_layer_hash_clip_scope(clips, children, payload_bounds);
            if payload_bounds.is_some() && child_payload_bounds.is_none() {
                return false;
            }
            if !paint_layer_render_nodes_intersect_payload(children, child_payload_bounds) {
                return false;
            }
            1u8.hash(hasher);
            hash_paint_layer_clip_shapes(hasher, &clips, float);
            hash_paint_layer_render_nodes(hasher, children, float, child_payload_bounds);
            true
        }
        RenderNode::RelaxedClip { clips, children } => {
            let (clips, child_payload_bounds) =
                paint_layer_hash_clip_scope(clips, children, payload_bounds);
            if payload_bounds.is_some() && child_payload_bounds.is_none() {
                return false;
            }
            if !paint_layer_render_nodes_intersect_payload(children, child_payload_bounds) {
                return false;
            }
            2u8.hash(hasher);
            hash_paint_layer_clip_shapes(hasher, &clips, float);
            hash_paint_layer_render_nodes(hasher, children, float, child_payload_bounds);
            true
        }
        RenderNode::Transform {
            transform,
            children,
        } => {
            let child_payload_bounds = payload_bounds.and_then(|bounds| {
                transform
                    .inverse()
                    .map(|inverse| inverse.map_rect_aabb(bounds))
            });
            if !paint_layer_render_nodes_intersect_payload(children, child_payload_bounds) {
                return false;
            }
            3u8.hash(hasher);
            hash_paint_layer_affine2(hasher, *transform, float);
            hash_paint_layer_render_nodes(hasher, children, float, child_payload_bounds);
            true
        }
        RenderNode::Alpha { alpha, children } => {
            if !paint_layer_render_nodes_intersect_payload(children, payload_bounds) {
                return false;
            }
            4u8.hash(hasher);
            float.hash_f32(hasher, *alpha);
            hash_paint_layer_render_nodes(hasher, children, float, payload_bounds);
            true
        }
        RenderNode::PaintLayer(layer) => {
            if payload_bounds
                .and_then(|bounds| bounds.intersect(layer.bounds))
                .is_none()
                && payload_bounds.is_some()
            {
                return false;
            }
            5u8.hash(hasher);
            hash_paint_layer_metadata(hasher, layer);
            hash_paint_layer_rect(hasher, layer.bounds, float);
            true
        }
        RenderNode::Primitive(primitive) => {
            if payload_bounds
                .and_then(|bounds| bounds.intersect(draw_primitive_visual_bounds(primitive)))
                .is_none()
                && payload_bounds.is_some()
            {
                return false;
            }
            6u8.hash(hasher);
            hash_paint_layer_draw_primitive(hasher, primitive, float);
            true
        }
    }
}

fn paint_layer_render_nodes_intersect_payload(
    nodes: &[RenderNode],
    payload_bounds: Option<Rect>,
) -> bool {
    let Some(payload_bounds) = payload_bounds else {
        return true;
    };
    render_nodes_visual_bounds(nodes, Affine2::identity())
        .and_then(|bounds| bounds.intersect(payload_bounds))
        .is_some()
}

fn paint_layer_hash_clip_scope(
    clips: &[ClipShape],
    children: &[RenderNode],
    payload_bounds: Option<Rect>,
) -> (Vec<ClipShape>, Option<Rect>) {
    let Some(payload_bounds) = payload_bounds else {
        return (clips.to_vec(), None);
    };

    let has_shadow_escape = children
        .iter()
        .any(|child| matches!(child, RenderNode::ShadowPass { .. }));
    let child_payload_bounds = if has_shadow_escape {
        Some(payload_bounds)
    } else {
        clips
            .iter()
            .try_fold(payload_bounds, |bounds, clip| bounds.intersect(clip.rect))
    };
    let clips = clips
        .iter()
        .filter_map(|clip| {
            clip.rect.intersect(payload_bounds).map(|rect| ClipShape {
                rect,
                radii: clip.radii,
            })
        })
        .collect();

    (clips, child_payload_bounds)
}

pub(crate) fn hash_paint_layer_metadata<H: Hasher>(hasher: &mut H, layer: &RenderPaintLayer) {
    layer.stable_id.hash(hasher);
    layer.root_id.hash(hasher);
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

#[cfg(test)]
mod tests {
    use super::{DrawPrimitive, RenderNode, RenderScene};
    use crate::tree::attrs::ImageFit;
    use std::collections::HashSet;

    #[test]
    fn video_target_ids_follow_only_primitives_in_the_current_scene() {
        let scene = RenderScene {
            nodes: vec![
                RenderNode::Primitive(DrawPrimitive::Rect(0.0, 0.0, 10.0, 10.0, 0)),
                RenderNode::Alpha {
                    alpha: 1.0,
                    children: vec![RenderNode::Primitive(DrawPrimitive::Video(
                        0.0,
                        0.0,
                        64.0,
                        32.0,
                        "preview".to_string(),
                        ImageFit::Contain,
                    ))],
                },
            ],
        };

        assert_eq!(
            scene.video_target_ids(),
            HashSet::from(["preview".to_string()])
        );
        assert!(RenderScene::default().video_target_ids().is_empty());
    }
}
