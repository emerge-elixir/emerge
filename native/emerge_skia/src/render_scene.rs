use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::renderer::measure_text_visual_metrics;
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
    /// Reports whether the semantic layer tree contains any cacheable payload.
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
            collect_video_target_ids_from_layer_content(layer.content.nodes.as_slice(), targets)
        }
        RenderNode::Primitive(DrawPrimitive::Video(_, _, _, _, target, _)) => {
            targets.insert(target.clone());
        }
        RenderNode::Primitive(_) => {}
    });
}

fn collect_video_target_ids_from_layer_content(
    content: &[RenderPaintLayerContentNode],
    targets: &mut HashSet<String>,
) {
    content.iter().for_each(|node| match node {
        RenderPaintLayerContentNode::Own(run) => collect_video_target_ids(&run.nodes, targets),
        RenderPaintLayerContentNode::Child(layer) => {
            collect_video_target_ids_from_layer_content(&layer.content.nodes, targets)
        }
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            collect_video_target_ids_from_layer_content(children, targets)
        }
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
                || layer_content_has_payload_cache_candidate(&layer.content.nodes)
        }
        RenderNode::Primitive(_) => false,
    })
}

fn layer_content_has_payload_cache_candidate(content: &[RenderPaintLayerContentNode]) -> bool {
    content.iter().any(|node| match node {
        RenderPaintLayerContentNode::Own(_) => false,
        RenderPaintLayerContentNode::Child(layer) => {
            layer.policy.allows_payload_cache()
                || layer_content_has_payload_cache_candidate(&layer.content.nodes)
        }
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            layer_content_has_payload_cache_candidate(children)
        }
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
                || layer_content_has_scroll_moving_paint_layer(&layer.content.nodes)
        }
        RenderNode::Primitive(_) => false,
    })
}

fn layer_content_has_scroll_moving_paint_layer(content: &[RenderPaintLayerContentNode]) -> bool {
    content.iter().any(|node| match node {
        RenderPaintLayerContentNode::Own(_) => false,
        RenderPaintLayerContentNode::Child(layer) => {
            layer.placement == PaintLayerPlacement::ScrollMoving
                || layer_content_has_scroll_moving_paint_layer(&layer.content.nodes)
        }
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            layer_content_has_scroll_moving_paint_layer(children)
        }
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
    pub moving_layers: usize,
    pub direct_only_layers: usize,
}

impl RenderSceneSummary {
    pub(crate) fn from_nodes(nodes: &[RenderNode]) -> Self {
        let mut summary = Self::default();
        summary.record_nodes(nodes);
        summary
    }

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
                RenderNode::PaintLayer(layer) => self.record_paint_layer(layer),
                RenderNode::Primitive(primitive) => self.record_primitive(primitive),
            }
        }
    }

    fn record_paint_layer(&mut self, layer: &RenderPaintLayer) {
        self.paint_layers += 1;
        match layer.policy {
            PaintLayerPolicy::Cacheable => self.cacheable_layers += 1,
            PaintLayerPolicy::DirectOnly => self.direct_only_layers += 1,
        }
        if layer.placement == PaintLayerPlacement::ScrollMoving {
            self.moving_layers += 1;
        }
        self.record_layer_content(&layer.content.nodes);
    }

    fn record_layer_content(&mut self, content: &[RenderPaintLayerContentNode]) {
        content.iter().for_each(|node| match node {
            RenderPaintLayerContentNode::Own(run) => self.record_nodes(&run.nodes),
            RenderPaintLayerContentNode::Child(layer) => self.record_paint_layer(layer),
            RenderPaintLayerContentNode::ShadowPass { children } => {
                self.nodes += 1;
                self.shadow_passes += 1;
                self.record_layer_content(children);
            }
            RenderPaintLayerContentNode::Clip { clips, children } => {
                self.nodes += 1;
                self.clips += 1;
                self.clip_shapes += clips.len();
                self.record_layer_content(children);
            }
            RenderPaintLayerContentNode::RelaxedClip { clips, children } => {
                self.nodes += 1;
                self.relaxed_clips += 1;
                self.clip_shapes += clips.len();
                self.record_layer_content(children);
            }
            RenderPaintLayerContentNode::Transform { children, .. } => {
                self.nodes += 1;
                self.transforms += 1;
                self.record_layer_content(children);
            }
            RenderPaintLayerContentNode::Alpha { children, .. } => {
                self.nodes += 1;
                self.alphas += 1;
                self.record_layer_content(children);
            }
        });
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
                "image_failed={}}} paint_layers={{total={} cacheable={} moving={} direct_only={}}}"
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaintLayerId {
    pub node_id: u64,
    pub role: PaintLayerReason,
}

impl PaintLayerId {
    pub const fn new(node_id: u64, role: PaintLayerReason) -> Self {
        Self { node_id, role }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPaintLayer {
    pub id: PaintLayerId,
    pub bounds: Rect,
    pub placement: PaintLayerPlacement,
    pub policy: PaintLayerPolicy,
    pub content_generation: u64,
    pub content: Arc<RenderPaintLayerContent>,
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
        Self::from_prepared_children(
            RenderPaintLayerBuildParts {
                id: PaintLayerId::new(stable_id, reason),
                bounds,
                placement,
                policy,
                content_generation,
                visual_bounds: None,
            },
            RenderPaintLayerContent::from_nodes(children),
        )
    }

    pub(crate) fn from_prepared_children(
        parts: RenderPaintLayerBuildParts,
        mut content: RenderPaintLayerContent,
    ) -> Self {
        content.assign_run_metadata(parts.bounds);
        let metrics = RenderPaintLayerMetrics::from_content(
            &content.nodes,
            parts.bounds,
            parts.visual_bounds,
        );

        Self {
            id: parts.id,
            bounds: parts.bounds,
            placement: parts.placement,
            policy: parts.policy,
            content_generation: parts.content_generation,
            content: Arc::new(content),
            metrics,
        }
    }

    pub(crate) fn content_nodes(&self) -> Vec<RenderNode> {
        self.content.to_render_nodes()
    }

    pub(crate) fn with_children(&self, children: Vec<RenderNode>) -> Self {
        self.with_bounds_and_children(self.bounds, children)
    }

    pub(crate) fn with_bounds_and_children(&self, bounds: Rect, children: Vec<RenderNode>) -> Self {
        Self::from_prepared_children(
            RenderPaintLayerBuildParts {
                id: self.id,
                bounds,
                placement: self.placement,
                policy: self.policy,
                content_generation: self.content_generation,
                visual_bounds: None,
            },
            RenderPaintLayerContent::from_composition_nodes(children),
        )
    }

    pub(crate) fn child_layer_count(&self) -> usize {
        self.content.child_layer_count()
    }

    #[cfg(test)]
    pub(crate) fn own_render_nodes(&self) -> Vec<RenderNode> {
        self.content.own_render_nodes()
    }

    #[cfg(test)]
    pub(crate) fn own_runs(&self) -> Vec<&RenderPaintRun> {
        let mut runs = Vec::new();
        collect_layer_own_runs(&self.content.nodes, &mut runs);
        runs
    }

    #[cfg(test)]
    pub(crate) fn descendant_layers(&self) -> Vec<&RenderPaintLayer> {
        let mut layers = Vec::new();
        collect_layer_descendants(&self.content.nodes, &mut layers);
        layers
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RenderPaintLayerBuildParts {
    pub id: PaintLayerId,
    pub bounds: Rect,
    pub placement: PaintLayerPlacement,
    pub policy: PaintLayerPolicy,
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
    fn from_content(
        content: &[RenderPaintLayerContentNode],
        bounds: Rect,
        visual_bounds: Option<Rect>,
    ) -> Self {
        let own_nodes = layer_content_own_render_nodes(content);
        let visual_bounds = visual_bounds
            .or_else(|| render_nodes_visual_bounds(&own_nodes, Affine2::identity()))
            .map(|content_bounds| union_rect(bounds, content_bounds))
            .unwrap_or(bounds);
        let node_metrics = render_nodes_metrics(&own_nodes);

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
pub struct RenderPaintLayerContent {
    pub nodes: Vec<RenderPaintLayerContentNode>,
}

impl RenderPaintLayerContent {
    pub(crate) fn from_nodes(nodes: Vec<RenderNode>) -> Self {
        Self {
            nodes: build_ordered_layer_content(nodes),
        }
    }

    pub(crate) fn from_composition_nodes(nodes: Vec<RenderNode>) -> Self {
        Self {
            nodes: build_composition_layer_content(nodes),
        }
    }

    pub(crate) fn own_render_nodes(&self) -> Vec<RenderNode> {
        layer_content_own_render_nodes(&self.nodes)
    }

    pub(crate) fn own_payload_render_nodes(&self) -> Vec<RenderNode> {
        let mut nodes = Vec::new();
        collect_layer_own_payload_nodes(&self.nodes, &mut nodes);
        nodes
    }

    pub(crate) fn to_render_nodes(&self) -> Vec<RenderNode> {
        layer_content_to_render_nodes(&self.nodes)
    }

    pub(crate) fn child_layer_count(&self) -> usize {
        layer_content_child_layer_count(&self.nodes)
    }

    fn assign_run_metadata(&mut self, bounds: Rect) {
        let mut next_slot = 0;
        assign_layer_run_metadata(&mut self.nodes, bounds, &mut next_slot);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderPaintLayerContentNode {
    Own(RenderPaintRun),
    Child(RenderPaintLayer),
    ShadowPass {
        children: Vec<RenderPaintLayerContentNode>,
    },
    Clip {
        clips: Vec<ClipShape>,
        children: Vec<RenderPaintLayerContentNode>,
    },
    RelaxedClip {
        clips: Vec<ClipShape>,
        children: Vec<RenderPaintLayerContentNode>,
    },
    Transform {
        transform: Affine2,
        children: Vec<RenderPaintLayerContentNode>,
    },
    Alpha {
        alpha: f32,
        children: Vec<RenderPaintLayerContentNode>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPaintRun {
    pub slot: u32,
    pub bounds: Rect,
    pub nodes: Arc<Vec<RenderNode>>,
    pub metrics: RenderPaintLayerMetrics,
}

impl RenderPaintRun {
    fn from_node(node: RenderNode) -> Self {
        Self {
            slot: 0,
            bounds: Rect::default(),
            nodes: Arc::new(vec![node]),
            metrics: RenderPaintLayerMetrics::default(),
        }
    }
}

fn build_ordered_layer_content(nodes: Vec<RenderNode>) -> Vec<RenderPaintLayerContentNode> {
    nodes.into_iter().fold(Vec::new(), |mut content, node| {
        append_ordered_layer_node(&mut content, node);
        content
    })
}

fn append_ordered_layer_node(content: &mut Vec<RenderPaintLayerContentNode>, node: RenderNode) {
    match node {
        RenderNode::PaintLayer(layer) => content.push(RenderPaintLayerContentNode::Child(layer)),
        RenderNode::ShadowPass { children } => {
            content.push(RenderPaintLayerContentNode::ShadowPass {
                children: build_ordered_layer_content(children),
            });
        }
        RenderNode::Clip { clips, children } if render_nodes_contain_paint_layer(&children) => {
            content.push(RenderPaintLayerContentNode::Clip {
                clips,
                children: build_ordered_layer_content(children),
            });
        }
        RenderNode::RelaxedClip { clips, children }
            if render_nodes_contain_paint_layer(&children) =>
        {
            content.push(RenderPaintLayerContentNode::RelaxedClip {
                clips,
                children: build_ordered_layer_content(children),
            });
        }
        RenderNode::Transform {
            transform,
            children,
        } if render_nodes_contain_paint_layer(&children) => {
            content.push(RenderPaintLayerContentNode::Transform {
                transform,
                children: build_ordered_layer_content(children),
            });
        }
        RenderNode::Alpha { alpha, children } if render_nodes_contain_paint_layer(&children) => {
            content.push(RenderPaintLayerContentNode::Alpha {
                alpha,
                children: build_ordered_layer_content(children),
            });
        }
        own => append_own_render_node(content, own),
    }
}

fn build_composition_layer_content(nodes: Vec<RenderNode>) -> Vec<RenderPaintLayerContentNode> {
    nodes.into_iter().fold(Vec::new(), |mut content, node| {
        append_composition_layer_node(&mut content, node);
        content
    })
}

fn append_composition_layer_node(content: &mut Vec<RenderPaintLayerContentNode>, node: RenderNode) {
    match node {
        RenderNode::PaintLayer(layer) => content.push(RenderPaintLayerContentNode::Child(layer)),
        RenderNode::ShadowPass { children } => {
            content.push(RenderPaintLayerContentNode::ShadowPass {
                children: build_composition_layer_content(children),
            })
        }
        RenderNode::Clip { clips, children } => content.push(RenderPaintLayerContentNode::Clip {
            clips,
            children: build_composition_layer_content(children),
        }),
        RenderNode::RelaxedClip { clips, children } => {
            content.push(RenderPaintLayerContentNode::RelaxedClip {
                clips,
                children: build_composition_layer_content(children),
            });
        }
        RenderNode::Transform {
            transform,
            children,
        } => {
            content.push(RenderPaintLayerContentNode::Transform {
                transform,
                children: build_composition_layer_content(children),
            });
        }
        RenderNode::Alpha { alpha, children } => content.push(RenderPaintLayerContentNode::Alpha {
            alpha,
            children: build_composition_layer_content(children),
        }),
        RenderNode::Primitive(_) => append_own_render_node(content, node),
    }
}

fn append_own_render_node(content: &mut Vec<RenderPaintLayerContentNode>, node: RenderNode) {
    if let Some(RenderPaintLayerContentNode::Own(run)) = content.last_mut() {
        Arc::make_mut(&mut run.nodes).push(node);
    } else {
        content.push(RenderPaintLayerContentNode::Own(RenderPaintRun::from_node(
            node,
        )));
    }
}

fn render_nodes_contain_paint_layer(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::PaintLayer(_) => true,
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => render_nodes_contain_paint_layer(children),
        RenderNode::Primitive(_) => false,
    })
}

fn assign_layer_run_metadata(
    content: &mut [RenderPaintLayerContentNode],
    bounds: Rect,
    next_slot: &mut u32,
) {
    content.iter_mut().for_each(|node| match node {
        RenderPaintLayerContentNode::Own(run) => {
            run.slot = *next_slot;
            *next_slot = (*next_slot).saturating_add(1);
            let node_metrics = render_nodes_metrics(&run.nodes);
            let visual_bounds =
                render_nodes_visual_bounds(&run.nodes, Affine2::identity()).unwrap_or(bounds);
            run.bounds = visual_bounds;
            run.metrics = RenderPaintLayerMetrics {
                own_node_count: node_metrics.node_count.min(u32::MAX as usize) as u32,
                own_primitive_count: node_metrics.primitive_count.min(u32::MAX as usize) as u32,
                own_primitive_cost: node_metrics.primitive_cost,
                payload_pixels: paint_layer_payload_pixels(run.bounds),
                visual_bounds,
                opaque: false,
            };
        }
        RenderPaintLayerContentNode::Child(_) => {}
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            assign_layer_run_metadata(children, bounds, next_slot)
        }
    });
}

fn collect_layer_own_payload_nodes(
    content: &[RenderPaintLayerContentNode],
    nodes: &mut Vec<RenderNode>,
) {
    content.iter().for_each(|node| match node {
        RenderPaintLayerContentNode::Own(run) => nodes.extend(run.nodes.iter().cloned()),
        RenderPaintLayerContentNode::Child(_) => {}
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            collect_layer_own_payload_nodes(children, nodes)
        }
    });
}

#[cfg(test)]
fn collect_layer_own_runs<'a>(
    content: &'a [RenderPaintLayerContentNode],
    runs: &mut Vec<&'a RenderPaintRun>,
) {
    content.iter().for_each(|node| match node {
        RenderPaintLayerContentNode::Own(run) => runs.push(run),
        RenderPaintLayerContentNode::Child(_) => {}
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            collect_layer_own_runs(children, runs)
        }
    });
}

#[cfg(test)]
fn collect_layer_descendants<'a>(
    content: &'a [RenderPaintLayerContentNode],
    layers: &mut Vec<&'a RenderPaintLayer>,
) {
    content.iter().for_each(|node| match node {
        RenderPaintLayerContentNode::Own(_) => {}
        RenderPaintLayerContentNode::Child(layer) => {
            layers.push(layer);
            collect_layer_descendants(&layer.content.nodes, layers);
        }
        RenderPaintLayerContentNode::ShadowPass { children }
        | RenderPaintLayerContentNode::Clip { children, .. }
        | RenderPaintLayerContentNode::RelaxedClip { children, .. }
        | RenderPaintLayerContentNode::Transform { children, .. }
        | RenderPaintLayerContentNode::Alpha { children, .. } => {
            collect_layer_descendants(children, layers)
        }
    });
}

fn layer_content_child_layer_count(content: &[RenderPaintLayerContentNode]) -> usize {
    content
        .iter()
        .map(|node| match node {
            RenderPaintLayerContentNode::Own(_) => 0,
            RenderPaintLayerContentNode::Child(layer) => {
                1 + layer_content_child_layer_count(&layer.content.nodes)
            }
            RenderPaintLayerContentNode::ShadowPass { children }
            | RenderPaintLayerContentNode::Clip { children, .. }
            | RenderPaintLayerContentNode::RelaxedClip { children, .. }
            | RenderPaintLayerContentNode::Transform { children, .. }
            | RenderPaintLayerContentNode::Alpha { children, .. } => {
                layer_content_child_layer_count(children)
            }
        })
        .sum()
}

fn layer_content_own_render_nodes(content: &[RenderPaintLayerContentNode]) -> Vec<RenderNode> {
    content
        .iter()
        .filter_map(|node| match node {
            RenderPaintLayerContentNode::Own(run) => Some(run.nodes.iter().cloned().collect()),
            RenderPaintLayerContentNode::Child(_) => None,
            RenderPaintLayerContentNode::ShadowPass { children } => {
                nonempty_scoped_own_nodes(children, |children| RenderNode::ShadowPass { children })
            }
            RenderPaintLayerContentNode::Clip { clips, children } => {
                nonempty_scoped_own_nodes(children, |children| RenderNode::Clip {
                    clips: clips.clone(),
                    children,
                })
            }
            RenderPaintLayerContentNode::RelaxedClip { clips, children } => {
                nonempty_scoped_own_nodes(children, |children| RenderNode::RelaxedClip {
                    clips: clips.clone(),
                    children,
                })
            }
            RenderPaintLayerContentNode::Transform {
                transform,
                children,
            } => nonempty_scoped_own_nodes(children, |children| RenderNode::Transform {
                transform: *transform,
                children,
            }),
            RenderPaintLayerContentNode::Alpha { alpha, children } => {
                nonempty_scoped_own_nodes(children, |children| RenderNode::Alpha {
                    alpha: *alpha,
                    children,
                })
            }
        })
        .flatten()
        .collect()
}

fn nonempty_scoped_own_nodes(
    children: &[RenderPaintLayerContentNode],
    wrap: impl FnOnce(Vec<RenderNode>) -> RenderNode,
) -> Option<Vec<RenderNode>> {
    let children = layer_content_own_render_nodes(children);
    (!children.is_empty()).then(|| vec![wrap(children)])
}

fn layer_content_to_render_nodes(content: &[RenderPaintLayerContentNode]) -> Vec<RenderNode> {
    content
        .iter()
        .flat_map(|node| match node {
            RenderPaintLayerContentNode::Own(run) => run.nodes.iter().cloned().collect(),
            RenderPaintLayerContentNode::Child(layer) => {
                vec![RenderNode::PaintLayer(layer.clone())]
            }
            RenderPaintLayerContentNode::ShadowPass { children } => {
                vec![RenderNode::ShadowPass {
                    children: layer_content_to_render_nodes(children),
                }]
            }
            RenderPaintLayerContentNode::Clip { clips, children } => vec![RenderNode::Clip {
                clips: clips.clone(),
                children: layer_content_to_render_nodes(children),
            }],
            RenderPaintLayerContentNode::RelaxedClip { clips, children } => {
                vec![RenderNode::RelaxedClip {
                    clips: clips.clone(),
                    children: layer_content_to_render_nodes(children),
                }]
            }
            RenderPaintLayerContentNode::Transform {
                transform,
                children,
            } => vec![RenderNode::Transform {
                transform: *transform,
                children: layer_content_to_render_nodes(children),
            }],
            RenderPaintLayerContentNode::Alpha { alpha, children } => vec![RenderNode::Alpha {
                alpha: *alpha,
                children: layer_content_to_render_nodes(children),
            }],
        })
        .collect()
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
        DrawPrimitive::TextWithFont(x, y, text, font_size, _, family, weight, italic) => {
            const TEXT_VISUAL_BOUNDS_OUTSET: f32 = 1.0;
            let metrics =
                measure_text_visual_metrics(family, *weight, *italic, *font_size, text.as_str());
            Rect {
                x: *x - metrics.left_overhang - TEXT_VISUAL_BOUNDS_OUTSET,
                y: *y + metrics.visual_top - TEXT_VISUAL_BOUNDS_OUTSET,
                width: metrics.visual_width + TEXT_VISUAL_BOUNDS_OUTSET * 2.0,
                height: (metrics.visual_bottom - metrics.visual_top).max(0.0)
                    + TEXT_VISUAL_BOUNDS_OUTSET * 2.0,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayerPlacement {
    Fixed,
    ScrollMoving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaintLayerPolicy {
    Cacheable,
    DirectOnly,
}

impl PaintLayerPolicy {
    pub(crate) fn allows_payload_cache(self) -> bool {
        self == Self::Cacheable
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaintLayerReason {
    Root,
    Nearby,
    ScrollContent,
    Animation,
    SliderValue,
    DirectMedia,
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
}

impl PaintLayerHashFloat {
    pub(crate) fn hash_f32<H: Hasher>(self, hasher: &mut H, value: f32) {
        match self {
            Self::Exact => value.to_bits().hash(hasher),
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
    layer.id.hash(hasher);
    layer.placement.hash(hasher);
    layer.policy.hash(hasher);
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
    use super::{
        DrawPrimitive, PaintLayerPlacement, PaintLayerPolicy, PaintLayerReason, RenderNode,
        RenderPaintLayer, RenderScene,
    };
    use crate::tree::attrs::ImageFit;
    use crate::tree::geometry::Rect;
    use std::collections::HashSet;

    #[test]
    fn summary_and_video_targets_follow_ordered_layer_content() {
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 32.0,
        };
        let child = RenderPaintLayer::from_children(
            2,
            bounds,
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::DirectOnly,
            PaintLayerReason::Nearby,
            0,
            vec![RenderNode::Primitive(DrawPrimitive::Video(
                0.0,
                0.0,
                64.0,
                32.0,
                "secondary".to_string(),
                ImageFit::Contain,
            ))],
        );
        let parent = RenderPaintLayer::from_children(
            1,
            bounds,
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::DirectOnly,
            PaintLayerReason::Root,
            0,
            vec![RenderNode::Alpha {
                alpha: 1.0,
                children: vec![
                    RenderNode::Primitive(DrawPrimitive::Video(
                        0.0,
                        0.0,
                        64.0,
                        32.0,
                        "preview".to_string(),
                        ImageFit::Contain,
                    )),
                    RenderNode::PaintLayer(child),
                ],
            }],
        );
        let scene = RenderScene {
            nodes: vec![RenderNode::PaintLayer(parent)],
        };

        assert_eq!(
            scene.video_target_ids(),
            HashSet::from(["preview".to_string(), "secondary".to_string()])
        );
        let summary = scene.summary();
        assert_eq!(summary.paint_layers, 2);
        assert_eq!(summary.alphas, 1);
        assert_eq!(summary.videos, 2);
        assert!(RenderScene::default().video_target_ids().is_empty());
    }
}
