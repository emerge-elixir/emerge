//! Render an ElementTree into a render scene.
//!
//! Reads from pre-scaled attrs (scaling is applied in the layout pass).

mod box_model;
mod color;
mod paint;
mod text;

pub(crate) use self::color::DEFAULT_TEXT_COLOR;
use self::paint::{
    build_background_nodes, collect_border_nodes, collect_box_shadow_nodes,
    collect_scrollbar_nodes, render_image_nodes, render_video_nodes,
};
use self::text::{
    TextDecorationSpec, render_multiline_text_input_items, render_text_input_items,
    render_text_items, text_decoration_items,
};
use super::attrs::{Attrs, effective_scrollbar_x, effective_scrollbar_y};
use super::element::{
    Element, ElementKind, ElementTree, Frame, NearbySlot, NodeIx, RetainedChildMode,
    RetainedLocalBranchRef,
};
use super::geometry::{ClipShape, Rect, host_clip_shape, self_shape as geometry_self_shape};
use super::layout::FontContext;
use super::scene::{
    ResolvedNodeState, SceneContext, child_context as next_scene_context, resolve_node_state,
};
use super::transform::{Affine2, element_transform};
use super::viewport_culling::{
    should_skip_render_viewport_subtree, should_skip_resolved_viewport_subtree,
};
#[cfg(test)]
use crate::events::{RegistryRebuildPayload, registry_builder};
use crate::render_scene::{
    DrawPrimitive, PaintLayerHashFloat, PaintLayerPlacement, PaintLayerPolicy, PaintLayerReason,
    RenderNode, RenderPaintLayer, RenderScene, hash_paint_layer_render_nodes,
};
use crate::renderer::{make_font_with_style, measure_text_visual_metrics_with_font};
#[cfg(any(test, feature = "bench-diagnostics"))]
use std::cell::Cell;
use std::hash::Hasher;

const RENDER_MOVING_PAINT_LAYER_MIN_RENDER_NODES: usize = 1;
const RENDER_MOVING_PAINT_LAYER_MAX_RENDER_NODES: usize = 256;
const RENDER_MOVING_PAINT_LAYER_MAX_BYTES: u64 = 4 * 1024 * 1024;
const RENDER_MOVING_PAINT_LAYER_BYTES_PER_PIXEL: u64 = 4;
const RENDER_MOVING_PAINT_LAYER_PAYLOAD_MAX_DEPTH: usize = 2;
const RENDER_MOVING_PAINT_LAYER_PAYLOAD_CONTENT_HASH_COORD_SCALE: f64 = 1024.0;

#[cfg(any(test, feature = "bench-diagnostics"))]
thread_local! {
    static RENDER_TRAVERSAL_DIAGNOSTICS_ENABLED: Cell<bool> = const { Cell::new(false) };
    static RENDER_TRAVERSAL_DIAGNOSTICS: Cell<RenderTraversalDiagnostics> = const {
        Cell::new(RenderTraversalDiagnostics::empty())
    };
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderTraversalDiagnostics {
    pub element_visits: u64,
    pub culled_subtrees: u64,
}

#[cfg(any(test, feature = "bench-diagnostics"))]
impl RenderTraversalDiagnostics {
    const fn empty() -> Self {
        Self {
            element_visits: 0,
            culled_subtrees: 0,
        }
    }

    fn with_element_visit(mut self) -> Self {
        self.element_visits = self.element_visits.saturating_add(1);
        self
    }

    fn with_culled_subtree(mut self) -> Self {
        self.culled_subtrees = self.culled_subtrees.saturating_add(1);
        self
    }
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
pub fn reset_render_traversal_diagnostics_for_benchmark() {
    RENDER_TRAVERSAL_DIAGNOSTICS.with(|diagnostics| {
        diagnostics.set(RenderTraversalDiagnostics::empty());
    });
    RENDER_TRAVERSAL_DIAGNOSTICS_ENABLED.with(|enabled| enabled.set(true));
}

#[cfg(any(test, feature = "bench-diagnostics"))]
#[doc(hidden)]
pub fn take_render_traversal_diagnostics_for_benchmark() -> RenderTraversalDiagnostics {
    RENDER_TRAVERSAL_DIAGNOSTICS_ENABLED.with(|enabled| enabled.set(false));
    RENDER_TRAVERSAL_DIAGNOSTICS.with(Cell::get)
}

#[cfg(any(test, feature = "bench-diagnostics"))]
fn update_render_traversal_diagnostics(
    update: impl FnOnce(RenderTraversalDiagnostics) -> RenderTraversalDiagnostics,
) {
    RENDER_TRAVERSAL_DIAGNOSTICS_ENABLED.with(|enabled| {
        if enabled.get() {
            RENDER_TRAVERSAL_DIAGNOSTICS.with(|diagnostics| {
                diagnostics.set(update(diagnostics.get()));
            });
        }
    });
}

#[cfg(any(test, feature = "bench-diagnostics"))]
fn record_render_traversal_element_visit() {
    update_render_traversal_diagnostics(RenderTraversalDiagnostics::with_element_visit);
}

#[cfg(not(any(test, feature = "bench-diagnostics")))]
fn record_render_traversal_element_visit() {}

#[cfg(any(test, feature = "bench-diagnostics"))]
fn record_render_traversal_culled_subtree() {
    update_render_traversal_diagnostics(RenderTraversalDiagnostics::with_culled_subtree);
}

#[cfg(not(any(test, feature = "bench-diagnostics")))]
fn record_render_traversal_culled_subtree() {}

#[cfg(test)]
pub(crate) struct RenderOutput {
    pub scene: RenderScene,
    pub event_rebuild: RegistryRebuildPayload,
    pub text_input_focused: bool,
    pub text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

pub(crate) struct RenderSceneOutput {
    pub scene: RenderScene,
    pub text_input_focused: bool,
    pub text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

#[derive(Clone, Copy, Debug)]
struct HostClipDescriptor {
    clip: ClipShape,
    scroll_x: bool,
    scroll_y: bool,
}

#[derive(Clone, Debug, Default)]
struct RenderBuildContext {
    scene_bounds: Rect,
    inherited_host_clips: Vec<HostClipDescriptor>,
    inherited_self_clips: Vec<ClipShape>,
    nearby_host_clips: Vec<HostClipDescriptor>,
    nearby_self_clips: Vec<ClipShape>,
    inside_local_transform: bool,
    scroll_clip_descendant_depth: Option<usize>,
}

impl RenderBuildContext {
    fn with_host_clip(
        &self,
        clip: HostClipDescriptor,
        self_clip: ClipShape,
        clip_nearby: bool,
    ) -> Self {
        let mut inherited_host_clips = self.inherited_host_clips.clone();
        let mut inherited_self_clips = self.inherited_self_clips.clone();
        let mut nearby_host_clips = self.nearby_host_clips.clone();
        let mut nearby_self_clips = self.nearby_self_clips.clone();
        inherited_host_clips.push(clip);
        inherited_self_clips.push(self_clip);
        if clip_nearby {
            nearby_host_clips.push(clip);
            nearby_self_clips.push(self_clip);
        }
        let scroll_clip_descendant_depth = if clip.scroll_x || clip.scroll_y {
            Some(0)
        } else {
            self.scroll_clip_descendant_depth
                .map(|depth| depth.saturating_add(1))
        };
        Self {
            scene_bounds: self.scene_bounds,
            inherited_host_clips,
            inherited_self_clips,
            nearby_host_clips,
            nearby_self_clips,
            inside_local_transform: self.inside_local_transform,
            scroll_clip_descendant_depth,
        }
    }

    fn without_host_clips(&self) -> Self {
        Self {
            scene_bounds: self.scene_bounds,
            inherited_host_clips: self.nearby_host_clips.clone(),
            inherited_self_clips: self.nearby_self_clips.clone(),
            nearby_host_clips: self.nearby_host_clips.clone(),
            nearby_self_clips: self.nearby_self_clips.clone(),
            inside_local_transform: self.inside_local_transform,
            scroll_clip_descendant_depth: self.scroll_clip_descendant_depth,
        }
    }

    fn within_local_transform(&self) -> Self {
        Self {
            scene_bounds: self.scene_bounds,
            inside_local_transform: true,
            ..Self::default()
        }
    }

    fn full_clip_shapes(&self) -> Vec<ClipShape> {
        self.inherited_host_clips
            .iter()
            .map(|clip| clip.clip)
            .collect()
    }

    fn shadow_clip_shapes(&self) -> Vec<ClipShape> {
        self.inherited_host_clips
            .iter()
            .filter(|clip| clip.scroll_x || clip.scroll_y)
            .map(|clip| moving_paint_layer_payload_shadow_boundary_clip(*clip, self.scene_bounds))
            .collect()
    }

    fn has_inherited_host_clip_shapes(&self) -> bool {
        !self.inherited_host_clips.is_empty()
    }

    fn nearest_self_clip(&self) -> Option<ClipShape> {
        self.inherited_self_clips.last().copied()
    }

    fn is_scroll_moving_paint_layer_context(&self) -> bool {
        self.scroll_clip_descendant_depth
            .is_some_and(|depth| depth <= RENDER_MOVING_PAINT_LAYER_PAYLOAD_MAX_DEPTH)
    }

    fn is_direct_scroll_moving_paint_layer_context(&self) -> bool {
        self.scroll_clip_descendant_depth == Some(0)
    }

    fn inside_local_transform(&self) -> bool {
        self.inside_local_transform
    }
}

struct RenderOutputs<'a> {
    text_input_focused: &'a mut bool,
    text_input_cursor_area: &'a mut Option<(f32, f32, f32, f32)>,
}

impl<'a> RenderOutputs<'a> {
    fn reborrow(&mut self) -> RenderOutputs<'_> {
        let text_input_focused = &mut *self.text_input_focused;
        let text_input_cursor_area = &mut *self.text_input_cursor_area;

        RenderOutputs {
            text_input_focused,
            text_input_cursor_area,
        }
    }
}

#[derive(Clone)]
struct RenderTraversal<'a> {
    scene_ctx: SceneContext,
    render_ctx: &'a RenderBuildContext,
    allow_moving_paint_layers: bool,
    emit_dynamic_paint_layers: bool,
    disable_viewport_culling: bool,
    inside_dynamic_paint_layer: bool,
}

struct HostContentBuild<'a> {
    element: &'a Element,
    element_ix: NodeIx,
    render_frame: Frame,
    element_context: &'a FontContext,
    scene_state: Option<ResolvedNodeState>,
}

#[derive(Clone, Default)]
struct RenderSubtree {
    local: Vec<RenderNode>,
    escapes: Vec<RenderNode>,
    text_input_focused: bool,
    text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

impl RenderSubtree {
    fn extend_local(&mut self, subtree: RenderSubtree) {
        self.merge_outputs(&subtree);
        let RenderSubtree { local, escapes, .. } = subtree;
        self.local.extend(local);
        self.escapes.extend(escapes);
    }

    fn extend_escape(&mut self, subtree: RenderSubtree) {
        self.merge_outputs(&subtree);
        let RenderSubtree { local, escapes, .. } = subtree;
        self.escapes.extend(local);
        self.escapes.extend(escapes);
    }

    fn merge_outputs(&mut self, subtree: &RenderSubtree) {
        self.text_input_focused |= subtree.text_input_focused;
        if self.text_input_cursor_area.is_none() {
            self.text_input_cursor_area = subtree.text_input_cursor_area;
        }
    }

    fn into_nodes(self) -> Vec<RenderNode> {
        let mut nodes = self.local;
        nodes.extend(self.escapes);
        nodes
    }
}

/// Render the tree without rebuilding event registry metadata and without using
/// paint-layer caches.
///
/// Reads from pre-scaled attrs (layout pass must run first). This is kept as a
/// safe baseline for dirty animation refreshes, correctness tests, and
/// performance regression benchmarks.
#[cfg(test)]
pub(crate) fn render_tree_scene(tree: &ElementTree) -> RenderSceneOutput {
    render_tree_scene_with_paint_layer_policy(tree, false, false)
}

pub(crate) fn render_tree_scene_with_scroll_layers(tree: &ElementTree) -> RenderSceneOutput {
    render_tree_scene_with_paint_layer_policy(tree, tree.has_scroll_refresh_damage(), true)
}

fn render_tree_scene_with_paint_layer_policy(
    tree: &ElementTree,
    allow_moving_paint_layers: bool,
    emit_dynamic_paint_layers: bool,
) -> RenderSceneOutput {
    let Some(root_ix) = tree.root_ix() else {
        return RenderSceneOutput {
            scene: RenderScene::default(),
            text_input_focused: false,
            text_input_cursor_area: None,
        };
    };

    let mut text_input_focused = false;
    let mut text_input_cursor_area = None;
    let render_ctx = RenderBuildContext {
        scene_bounds: scene_bounds_for_root(tree, root_ix),
        ..RenderBuildContext::default()
    };
    let mut outputs = RenderOutputs {
        text_input_focused: &mut text_input_focused,
        text_input_cursor_area: &mut text_input_cursor_area,
    };
    let subtree = build_element_subtree(
        tree,
        root_ix,
        &FontContext::default(),
        &mut outputs,
        RenderTraversal {
            scene_ctx: SceneContext::default(),
            render_ctx: &render_ctx,
            allow_moving_paint_layers,
            emit_dynamic_paint_layers,
            disable_viewport_culling: false,
            inside_dynamic_paint_layer: false,
        },
    );

    RenderSceneOutput {
        scene: RenderScene {
            nodes: subtree.into_nodes(),
        },
        text_input_focused,
        text_input_cursor_area,
    }
}

/// Render the tree and collect rebuild metadata.
/// Reads from pre-scaled attrs (layout pass must run first).
#[cfg(test)]
pub(crate) fn render_tree(tree: &ElementTree) -> RenderOutput {
    let scene_output = render_tree_scene(tree);

    RenderOutput {
        scene: scene_output.scene,
        event_rebuild: registry_builder::build_registry_rebuild(tree),
        text_input_focused: scene_output.text_input_focused,
        text_input_cursor_area: scene_output.text_input_cursor_area,
    }
}

fn build_element_subtree(
    tree: &ElementTree,
    ix: NodeIx,
    inherited: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
) -> RenderSubtree {
    let Some(element) = tree.get_ix(ix) else {
        return RenderSubtree::default();
    };
    let Some(frame) = element.layout.frame else {
        return RenderSubtree::default();
    };
    record_render_traversal_element_visit();

    let attrs = &element.layout.effective;
    let radius = attrs.border_radius.as_ref();
    let scene_state = resolve_node_state(element, traversal.scene_ctx.clone());
    let render_frame = scene_state
        .as_ref()
        .map(|state| state.adjusted_render_frame)
        .unwrap_or(frame);
    let transform = element_transform(render_frame, attrs);
    let alpha = attrs.alpha.unwrap_or(1.0) as f32;
    let render_damage = element.refresh.render_dirty || element.refresh.render_descendant_dirty;

    if !traversal.disable_viewport_culling
        && should_cull_render_subtree(
            tree,
            ix,
            attrs,
            render_frame,
            transform,
            &traversal.scene_ctx,
        )
    {
        return RenderSubtree::default();
    }
    let preserve_moving_paint_layer_content = should_preserve_moving_paint_layer_content(
        element,
        render_frame,
        transform,
        render_damage,
        &traversal,
    );
    let emit_dynamic_paint_layer = should_wrap_dynamic_paint_layer(element, attrs, &traversal);
    let child_inside_dynamic_paint_layer = should_descend_inside_dynamic_paint_layer(
        element,
        render_frame,
        transform,
        render_damage,
        &traversal,
    );

    let element_context = inherited.merge_with_attrs(attrs);
    let mut local = Vec::new();

    let outer_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, false);
    local.extend(wrap_outer_shadow_nodes(
        outer_shadow_nodes,
        transform,
        traversal.render_ctx,
    ));

    let background_nodes = build_background_nodes(render_frame, attrs);
    let inset_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, true);
    let local_transform_render_ctx =
        (!transform.is_identity()).then(|| traversal.render_ctx.within_local_transform());
    let host_content_render_ctx = local_transform_render_ctx
        .as_ref()
        .unwrap_or(traversal.render_ctx);
    let host_content = build_host_content_subtree(
        tree,
        HostContentBuild {
            element,
            element_ix: ix,
            render_frame,
            element_context: &element_context,
            scene_state: scene_state.clone(),
        },
        &mut outputs.reborrow(),
        RenderTraversal {
            scene_ctx: traversal.scene_ctx.clone(),
            render_ctx: host_content_render_ctx,
            allow_moving_paint_layers: traversal.allow_moving_paint_layers,
            emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
            disable_viewport_culling: traversal.disable_viewport_culling
                || preserve_moving_paint_layer_content,
            inside_dynamic_paint_layer: child_inside_dynamic_paint_layer,
        },
    );
    let border_nodes = collect_border_nodes(render_frame, attrs);
    let inherited_host_clips = traversal.render_ctx.full_clip_shapes();
    let inherited_self_clip = traversal.render_ctx.nearest_self_clip();

    if matches!(element.spec.kind, ElementKind::Image | ElementKind::Video) {
        let mut decorative_nodes = Vec::new();
        decorative_nodes.extend(background_nodes);
        decorative_nodes.extend(inset_shadow_nodes);
        decorative_nodes.extend(border_nodes);

        let content_clips = if image_video_needs_own_host_clip(attrs) {
            inherited_host_clips.clone()
        } else {
            inherited_self_clip
                .map(|clip| vec![clip])
                .unwrap_or_else(|| inherited_host_clips.clone())
        };

        local.extend(wrap_with_clips(
            wrap_with_transform(decorative_nodes, transform),
            inherited_host_clips.clone(),
        ));
        local.extend(wrap_with_relaxed_clips(
            wrap_with_transform(host_content.local, transform),
            content_clips,
        ));
    } else {
        let mut normal_nodes = Vec::new();
        normal_nodes.extend(background_nodes);
        normal_nodes.extend(inset_shadow_nodes);
        normal_nodes.extend(host_content.local);
        normal_nodes.extend(border_nodes);

        let normal_nodes = if should_allow_scroll_moving_paint_layer_at_current_node(&traversal) {
            wrap_with_explicit_moving_paint_layer_payload(MovingPaintLayerPayloadWrapInput {
                nodes: normal_nodes,
                element,
                render_frame,
                transform,
                render_damage,
                text_input_focused: host_content.text_input_focused,
                inside_local_transform: traversal.render_ctx.inside_local_transform(),
                ancestor_clip_context: traversal.render_ctx,
            })
        } else {
            wrap_with_transform(normal_nodes, transform)
        };
        local.extend(wrap_with_paint_layer_if_scroll_container(
            wrap_with_clips(normal_nodes, inherited_host_clips),
            element,
            render_frame,
        ));
    }

    let escapes = wrap_with_alpha(wrap_with_transform(host_content.escapes, transform), alpha);

    RenderSubtree {
        local: wrap_with_alpha(
            wrap_with_dynamic_paint_layer_if_dirty(
                local,
                element,
                render_frame,
                emit_dynamic_paint_layer,
            ),
            alpha,
        ),
        escapes,
        text_input_focused: false,
        text_input_cursor_area: None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MovingPaintLayerPlacement {
    transform: Affine2,
    bounds: Rect,
    local_origin_x: f32,
    local_origin_y: f32,
}

struct MovingPaintLayerPayloadWrapInput<'a> {
    nodes: Vec<RenderNode>,
    element: &'a Element,
    render_frame: Frame,
    transform: Affine2,
    render_damage: bool,
    text_input_focused: bool,
    inside_local_transform: bool,
    ancestor_clip_context: &'a RenderBuildContext,
}

fn wrap_with_explicit_moving_paint_layer_payload(
    input: MovingPaintLayerPayloadWrapInput<'_>,
) -> Vec<RenderNode> {
    let MovingPaintLayerPayloadWrapInput {
        nodes,
        element,
        render_frame,
        transform,
        render_damage,
        text_input_focused,
        inside_local_transform,
        ancestor_clip_context,
    } = input;

    if render_damage {
        return wrap_with_transform(nodes, transform);
    }

    let Some(placement) = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        text_input_focused,
        inside_local_transform,
    ) else {
        return wrap_with_transform(nodes, transform);
    };

    if !should_emit_moving_paint_layer(&nodes, placement) {
        return wrap_with_transform(nodes, transform);
    }

    let nodes = strip_moving_paint_layer_payload_ancestor_clips(nodes, ancestor_clip_context);
    let local_children = localize_moving_paint_layer_nodes(
        nodes,
        placement.local_origin_x,
        placement.local_origin_y,
    );
    let content_generation = moving_paint_layer_content_generation(&local_children);

    wrap_with_transform(
        vec![RenderNode::PaintLayer(RenderPaintLayer {
            stable_id: element.id.to_wire_u64(),
            bounds: placement.bounds,
            placement: PaintLayerPlacement::ScrollMoving,
            policy: PaintLayerPolicy::Cacheable,
            reason: PaintLayerReason::StableSubtree,
            content_generation,
            children: local_children,
        })],
        placement.transform,
    )
}

fn moving_paint_layer_static_placement(
    element: &Element,
    render_frame: Frame,
    transform: Affine2,
    text_input_focused: bool,
    inside_local_transform: bool,
) -> Option<MovingPaintLayerPlacement> {
    let attrs = &element.layout.effective;
    if !moving_paint_layer_frame_has_finite_bounds(render_frame) {
        return None;
    }

    if text_input_focused {
        return None;
    }

    if inside_local_transform {
        return None;
    }

    if is_scroll_container(element) {
        return None;
    }

    if matches!(
        element.spec.kind,
        ElementKind::Text
            | ElementKind::TextInput
            | ElementKind::Multiline
            | ElementKind::Image
            | ElementKind::Video
            | ElementKind::None
            | ElementKind::Paragraph
    ) {
        return None;
    }

    if attrs.rotate.unwrap_or(0.0) != 0.0
        || attrs.layout_rotate.unwrap_or(0.0) != 0.0
        || attrs.scale.unwrap_or(1.0) != 1.0
    {
        return None;
    }

    moving_paint_layer_exact_placement(
        render_frame,
        moving_paint_layer_placement_transform(render_frame, transform),
    )
}

fn should_emit_moving_paint_layer(
    nodes: &[RenderNode],
    placement: MovingPaintLayerPlacement,
) -> bool {
    if nodes.is_empty() {
        return false;
    }

    if !moving_paint_layer_children_are_supported(nodes) {
        return false;
    }

    let node_count = render_node_count(nodes);
    if node_count < RENDER_MOVING_PAINT_LAYER_MIN_RENDER_NODES {
        return false;
    }

    if node_count > RENDER_MOVING_PAINT_LAYER_MAX_RENDER_NODES {
        return false;
    }

    if moving_paint_layer_payload_bytes(placement.bounds) > RENDER_MOVING_PAINT_LAYER_MAX_BYTES {
        return false;
    }

    true
}

fn moving_paint_layer_placement_transform(render_frame: Frame, transform: Affine2) -> Affine2 {
    transform.then(Affine2::translation(render_frame.x, render_frame.y))
}

fn moving_paint_layer_exact_placement(
    render_frame: Frame,
    transform: Affine2,
) -> Option<MovingPaintLayerPlacement> {
    if !moving_paint_layer_transform_is_translation(transform) {
        return None;
    }

    let width = render_frame.width.ceil();
    let height = render_frame.height.ceil();
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0).then_some(
        MovingPaintLayerPlacement {
            transform,
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            local_origin_x: render_frame.x,
            local_origin_y: render_frame.y,
        },
    )
}

fn moving_paint_layer_payload_bytes(bounds: Rect) -> u64 {
    let width = bounds.width.ceil().max(0.0) as u64;
    let height = bounds.height.ceil().max(0.0) as u64;
    width
        .saturating_mul(height)
        .saturating_mul(RENDER_MOVING_PAINT_LAYER_BYTES_PER_PIXEL)
}

fn should_preserve_moving_paint_layer_content(
    element: &Element,
    render_frame: Frame,
    transform: Affine2,
    render_damage: bool,
    traversal: &RenderTraversal<'_>,
) -> bool {
    if traversal.disable_viewport_culling
        || render_damage
        || !should_allow_scroll_moving_paint_layer_at_current_node(traversal)
    {
        return false;
    }

    let Some(placement) = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        false,
        traversal.render_ctx.inside_local_transform(),
    ) else {
        return false;
    };

    moving_paint_layer_payload_bytes(placement.bounds) <= RENDER_MOVING_PAINT_LAYER_MAX_BYTES
}

fn should_allow_scroll_moving_paint_layer_at_current_node(traversal: &RenderTraversal<'_>) -> bool {
    traversal.allow_moving_paint_layers
        && (traversal
            .render_ctx
            .is_direct_scroll_moving_paint_layer_context()
            || (traversal.inside_dynamic_paint_layer
                && traversal.render_ctx.is_scroll_moving_paint_layer_context()))
}

fn should_emit_dynamic_paint_layer(element: &Element, traversal: &RenderTraversal<'_>) -> bool {
    traversal.emit_dynamic_paint_layers && !is_scroll_container(element)
}

fn should_wrap_dynamic_paint_layer(
    element: &Element,
    attrs: &Attrs,
    traversal: &RenderTraversal<'_>,
) -> bool {
    // Animated paint owns a dynamic boundary even when its damage expands beyond
    // the element frame. For example, animated shadow damage may overlap parent
    // pixels, but the parent paint layer can stay cached because the animated
    // boundary is composited on top of it.
    should_emit_dynamic_paint_layer(element, traversal)
        && (element.refresh.render_dirty || element_has_active_animation(element, attrs))
}

fn should_descend_inside_dynamic_paint_layer(
    element: &Element,
    render_frame: Frame,
    transform: Affine2,
    render_damage: bool,
    traversal: &RenderTraversal<'_>,
) -> bool {
    if traversal.inside_dynamic_paint_layer {
        return true;
    }

    if !traversal.allow_moving_paint_layers
        || !traversal.render_ctx.is_scroll_moving_paint_layer_context()
        || is_scroll_container(element)
    {
        return false;
    }

    if render_damage {
        return true;
    }

    traversal
        .render_ctx
        .is_direct_scroll_moving_paint_layer_context()
        && direct_scroll_moving_paint_layer_too_large(element, render_frame, transform, traversal)
}

fn direct_scroll_moving_paint_layer_too_large(
    element: &Element,
    render_frame: Frame,
    transform: Affine2,
    traversal: &RenderTraversal<'_>,
) -> bool {
    let Some(placement) = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        false,
        traversal.render_ctx.inside_local_transform(),
    ) else {
        return false;
    };

    moving_paint_layer_payload_bytes(placement.bounds) > RENDER_MOVING_PAINT_LAYER_MAX_BYTES
}

fn moving_paint_layer_frame_has_finite_bounds(render_frame: Frame) -> bool {
    render_frame.x.is_finite()
        && render_frame.y.is_finite()
        && render_frame.width.is_finite()
        && render_frame.height.is_finite()
        && render_frame.width > 0.0
        && render_frame.height > 0.0
}

fn moving_paint_layer_transform_is_translation(transform: Affine2) -> bool {
    transform.xx == 1.0
        && transform.yx == 0.0
        && transform.xy == 0.0
        && transform.yy == 1.0
        && transform.tx.is_finite()
        && transform.ty.is_finite()
}

fn moving_paint_layer_content_generation(nodes: &[RenderNode]) -> u64 {
    let mut hasher = MovingPaintLayerPayloadContentHasher::default();
    hash_paint_layer_render_nodes(
        &mut hasher,
        nodes,
        PaintLayerHashFloat::Quantized {
            scale: RENDER_MOVING_PAINT_LAYER_PAYLOAD_CONTENT_HASH_COORD_SCALE,
        },
    );
    hasher.finish()
}

#[derive(Clone, Copy, Debug)]
struct MovingPaintLayerPayloadContentHasher {
    value: u64,
}

impl Default for MovingPaintLayerPayloadContentHasher {
    fn default() -> Self {
        Self {
            value: 0xcbf2_9ce4_8422_2325,
        }
    }
}

impl Hasher for MovingPaintLayerPayloadContentHasher {
    fn finish(&self) -> u64 {
        self.value
    }

    fn write(&mut self, bytes: &[u8]) {
        bytes.iter().for_each(|byte| {
            self.value ^= u64::from(*byte);
            self.value = self.value.wrapping_mul(0x0000_0100_0000_01b3);
        });
    }

    fn write_u8(&mut self, i: u8) {
        self.write(&[i]);
    }

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64);
    }
}

fn strip_moving_paint_layer_payload_ancestor_clips(
    nodes: Vec<RenderNode>,
    ancestor_clip_context: &RenderBuildContext,
) -> Vec<RenderNode> {
    if !ancestor_clip_context.has_inherited_host_clip_shapes() {
        return nodes;
    }

    nodes
        .into_iter()
        .flat_map(|node| {
            strip_moving_paint_layer_payload_ancestor_clip_node(node, ancestor_clip_context)
        })
        .collect()
}

fn strip_moving_paint_layer_payload_ancestor_clip_node(
    node: RenderNode,
    ancestor_clip_context: &RenderBuildContext,
) -> Vec<RenderNode> {
    match node {
        RenderNode::Clip { clips, children } => {
            let children =
                strip_moving_paint_layer_payload_ancestor_clips(children, ancestor_clip_context);
            let clips: Vec<_> = clips
                .into_iter()
                .filter(|clip| {
                    !moving_paint_layer_payload_clip_matches_inherited_host_boundary(
                        *clip,
                        ancestor_clip_context,
                    )
                })
                .collect();
            if children.is_empty() {
                Vec::new()
            } else if clips.is_empty() {
                children
            } else {
                vec![RenderNode::Clip { clips, children }]
            }
        }
        RenderNode::RelaxedClip { clips, children } => {
            let children =
                strip_moving_paint_layer_payload_ancestor_clips(children, ancestor_clip_context);
            let clips: Vec<_> = clips
                .into_iter()
                .filter(|clip| {
                    !moving_paint_layer_payload_clip_matches_inherited_host_boundary(
                        *clip,
                        ancestor_clip_context,
                    )
                })
                .collect();
            if children.is_empty() {
                Vec::new()
            } else if clips.is_empty() {
                children
            } else {
                vec![RenderNode::RelaxedClip { clips, children }]
            }
        }
        RenderNode::ShadowPass { children } => vec![RenderNode::ShadowPass {
            children: strip_moving_paint_layer_payload_ancestor_clips(
                children,
                ancestor_clip_context,
            ),
        }],
        RenderNode::Transform {
            transform,
            children,
        } => vec![RenderNode::Transform {
            transform,
            children: strip_moving_paint_layer_payload_ancestor_clips(
                children,
                ancestor_clip_context,
            ),
        }],
        RenderNode::Alpha { alpha, children } => vec![RenderNode::Alpha {
            alpha,
            children: strip_moving_paint_layer_payload_ancestor_clips(
                children,
                ancestor_clip_context,
            ),
        }],
        RenderNode::PaintLayer(layer) => {
            vec![RenderNode::PaintLayer(RenderPaintLayer {
                children: strip_moving_paint_layer_payload_ancestor_clips(
                    layer.children,
                    ancestor_clip_context,
                ),
                ..layer
            })]
        }
        RenderNode::Primitive(_) => vec![node],
    }
}

fn moving_paint_layer_payload_clip_matches_inherited_host_boundary(
    clip: ClipShape,
    render_ctx: &RenderBuildContext,
) -> bool {
    render_ctx.inherited_host_clips.iter().any(|ancestor| {
        moving_paint_layer_payload_clip_shape_approx_eq(clip, ancestor.clip)
            || ((ancestor.scroll_x || ancestor.scroll_y)
                && moving_paint_layer_payload_clip_shape_approx_eq(
                    clip,
                    moving_paint_layer_payload_shadow_boundary_clip(
                        *ancestor,
                        render_ctx.scene_bounds,
                    ),
                ))
    })
}

fn moving_paint_layer_payload_clip_shape_approx_eq(a: ClipShape, b: ClipShape) -> bool {
    moving_paint_layer_payload_rect_approx_eq(a.rect, b.rect)
        && match (a.radii, b.radii) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                moving_paint_layer_payload_f32_approx_eq(a.tl, b.tl)
                    && moving_paint_layer_payload_f32_approx_eq(a.tr, b.tr)
                    && moving_paint_layer_payload_f32_approx_eq(a.br, b.br)
                    && moving_paint_layer_payload_f32_approx_eq(a.bl, b.bl)
            }
            _ => false,
        }
}

fn moving_paint_layer_payload_rect_approx_eq(a: Rect, b: Rect) -> bool {
    moving_paint_layer_payload_f32_approx_eq(a.x, b.x)
        && moving_paint_layer_payload_f32_approx_eq(a.y, b.y)
        && moving_paint_layer_payload_f32_approx_eq(a.width, b.width)
        && moving_paint_layer_payload_f32_approx_eq(a.height, b.height)
}

fn moving_paint_layer_payload_f32_approx_eq(a: f32, b: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 0.001
}

fn moving_paint_layer_payload_shadow_boundary_clip(
    clip: HostClipDescriptor,
    scene_bounds: Rect,
) -> ClipShape {
    match (clip.scroll_x, clip.scroll_y) {
        (true, true) => clip.clip,
        (true, false) => ClipShape {
            rect: Rect {
                x: clip.clip.rect.x,
                y: scene_bounds.y,
                width: clip.clip.rect.width,
                height: scene_bounds.height,
            },
            radii: None,
        },
        (false, true) => ClipShape {
            rect: Rect {
                x: scene_bounds.x,
                y: clip.clip.rect.y,
                width: scene_bounds.width,
                height: clip.clip.rect.height,
            },
            radii: None,
        },
        (false, false) => clip.clip,
    }
}

fn moving_paint_layer_children_are_supported(nodes: &[RenderNode]) -> bool {
    nodes.iter().all(|node| match node {
        RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::ShadowPass { children } => {
            moving_paint_layer_children_are_supported(children)
        }
        RenderNode::Primitive(primitive) => match primitive {
            DrawPrimitive::Video(..)
            | DrawPrimitive::ImageLoading(..)
            | DrawPrimitive::ImageFailed(..) => false,
            DrawPrimitive::Rect(..)
            | DrawPrimitive::RoundedRect(..)
            | DrawPrimitive::Border(..)
            | DrawPrimitive::BorderCorners(..)
            | DrawPrimitive::BorderEdges(..)
            | DrawPrimitive::Shadow(..)
            | DrawPrimitive::InsetShadow(..)
            | DrawPrimitive::TextWithFont(..)
            | DrawPrimitive::Gradient(..)
            | DrawPrimitive::Image(..) => true,
        },
        RenderNode::Transform { .. } | RenderNode::Alpha { .. } | RenderNode::PaintLayer(_) => {
            false
        }
    })
}

fn localize_moving_paint_layer_nodes(
    nodes: Vec<RenderNode>,
    origin_x: f32,
    origin_y: f32,
) -> Vec<RenderNode> {
    nodes
        .into_iter()
        .map(|node| localize_moving_paint_layer_node(node, origin_x, origin_y))
        .collect()
}

fn localize_moving_paint_layer_node(node: RenderNode, origin_x: f32, origin_y: f32) -> RenderNode {
    match node {
        RenderNode::Clip { clips, children } => RenderNode::Clip {
            clips: localize_moving_paint_layer_payload_clip_shapes(clips, origin_x, origin_y),
            children: localize_moving_paint_layer_nodes(children, origin_x, origin_y),
        },
        RenderNode::RelaxedClip { clips, children } => RenderNode::RelaxedClip {
            clips: localize_moving_paint_layer_payload_clip_shapes(clips, origin_x, origin_y),
            children: localize_moving_paint_layer_nodes(children, origin_x, origin_y),
        },
        RenderNode::Primitive(primitive) => RenderNode::Primitive(
            localize_moving_paint_layer_primitive(primitive, origin_x, origin_y),
        ),
        RenderNode::ShadowPass { children } => RenderNode::ShadowPass {
            children: localize_moving_paint_layer_nodes(children, origin_x, origin_y),
        },
        RenderNode::Transform {
            transform,
            children,
        } => RenderNode::Transform {
            transform: Affine2::translation(-origin_x, -origin_y).then(transform),
            children,
        },
        RenderNode::Alpha { alpha, children } => RenderNode::Alpha {
            alpha,
            children: localize_moving_paint_layer_nodes(children, origin_x, origin_y),
        },
        RenderNode::PaintLayer(layer) => RenderNode::PaintLayer(RenderPaintLayer {
            bounds: Rect {
                x: layer.bounds.x - origin_x,
                y: layer.bounds.y - origin_y,
                ..layer.bounds
            },
            children: localize_moving_paint_layer_nodes(layer.children, origin_x, origin_y),
            ..layer
        }),
    }
}

fn localize_moving_paint_layer_payload_clip_shapes(
    clips: Vec<ClipShape>,
    origin_x: f32,
    origin_y: f32,
) -> Vec<ClipShape> {
    clips
        .into_iter()
        .map(|clip| clip.offset(origin_x, origin_y))
        .collect()
}

fn localize_moving_paint_layer_primitive(
    primitive: DrawPrimitive,
    origin_x: f32,
    origin_y: f32,
) -> DrawPrimitive {
    match primitive {
        DrawPrimitive::Rect(x, y, w, h, color) => {
            DrawPrimitive::Rect(x - origin_x, y - origin_y, w, h, color)
        }
        DrawPrimitive::RoundedRect(x, y, w, h, radius, color) => {
            DrawPrimitive::RoundedRect(x - origin_x, y - origin_y, w, h, radius, color)
        }
        DrawPrimitive::Border(x, y, w, h, radius, width, color, style) => DrawPrimitive::Border(
            x - origin_x,
            y - origin_y,
            w,
            h,
            radius,
            width,
            color,
            style,
        ),
        DrawPrimitive::BorderCorners(x, y, w, h, tl, tr, br, bl, width, color, style) => {
            DrawPrimitive::BorderCorners(
                x - origin_x,
                y - origin_y,
                w,
                h,
                tl,
                tr,
                br,
                bl,
                width,
                color,
                style,
            )
        }
        DrawPrimitive::BorderEdges(x, y, w, h, radius, top, right, bottom, left, color, style) => {
            DrawPrimitive::BorderEdges(
                x - origin_x,
                y - origin_y,
                w,
                h,
                radius,
                top,
                right,
                bottom,
                left,
                color,
                style,
            )
        }
        DrawPrimitive::Shadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
            DrawPrimitive::Shadow(
                x - origin_x,
                y - origin_y,
                w,
                h,
                offset_x,
                offset_y,
                blur,
                size,
                radius,
                color,
            )
        }
        DrawPrimitive::InsetShadow(x, y, w, h, offset_x, offset_y, blur, size, radius, color) => {
            DrawPrimitive::InsetShadow(
                x - origin_x,
                y - origin_y,
                w,
                h,
                offset_x,
                offset_y,
                blur,
                size,
                radius,
                color,
            )
        }
        DrawPrimitive::TextWithFont(x, y, text, font_size, fill, family, weight, italic) => {
            DrawPrimitive::TextWithFont(
                x - origin_x,
                y - origin_y,
                text,
                font_size,
                fill,
                family,
                weight,
                italic,
            )
        }
        DrawPrimitive::Gradient(x, y, w, h, from, to, angle) => {
            DrawPrimitive::Gradient(x - origin_x, y - origin_y, w, h, from, to, angle)
        }
        DrawPrimitive::Image(x, y, w, h, image_id, fit, tint) => {
            DrawPrimitive::Image(x - origin_x, y - origin_y, w, h, image_id, fit, tint)
        }
        DrawPrimitive::Video(x, y, w, h, target, fit) => {
            DrawPrimitive::Video(x - origin_x, y - origin_y, w, h, target, fit)
        }
        DrawPrimitive::ImageLoading(x, y, w, h) => {
            DrawPrimitive::ImageLoading(x - origin_x, y - origin_y, w, h)
        }
        DrawPrimitive::ImageFailed(x, y, w, h) => {
            DrawPrimitive::ImageFailed(x - origin_x, y - origin_y, w, h)
        }
    }
}

fn is_scroll_container(element: &Element) -> bool {
    element.layout.scroll_x != 0.0
        || element.layout.scroll_y != 0.0
        || element.layout.scroll_x_max > 0.0
        || element.layout.scroll_y_max > 0.0
        || effective_scrollbar_x(&element.layout.effective)
        || effective_scrollbar_y(&element.layout.effective)
}

fn element_has_active_animation(element: &Element, attrs: &Attrs) -> bool {
    element.spec.declared.animate.is_some()
        || element.spec.declared.animate_enter.is_some()
        || element.spec.declared.animate_exit.is_some()
        || element.lifecycle.ghost_exit_animation.is_some()
        || attrs.animate.is_some()
        || attrs.animate_enter.is_some()
        || attrs.animate_exit.is_some()
}

fn should_cull_render_subtree(
    tree: &ElementTree,
    ix: NodeIx,
    attrs: &Attrs,
    render_frame: Frame,
    transform: Affine2,
    scene_ctx: &SceneContext,
) -> bool {
    let culled =
        should_skip_resolved_viewport_subtree(tree, ix, attrs, render_frame, transform, scene_ctx);
    if culled {
        record_render_traversal_culled_subtree();
    }
    culled
}

fn should_skip_render_child_subtree(
    tree: &ElementTree,
    child_ix: NodeIx,
    scene_ctx: &SceneContext,
) -> bool {
    let culled = should_skip_render_viewport_subtree(tree, child_ix, scene_ctx);
    if culled {
        record_render_traversal_culled_subtree();
    }
    culled
}

fn render_node_count(nodes: &[RenderNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Transform { children, .. }
            | RenderNode::Alpha { children, .. } => 1 + render_node_count(children),
            RenderNode::PaintLayer(layer) => 1 + render_node_count(&layer.children),
            RenderNode::Primitive(_) => 1,
        })
        .sum()
}

fn build_host_content_subtree(
    tree: &ElementTree,
    input: HostContentBuild<'_>,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
) -> RenderSubtree {
    let HostContentBuild {
        element,
        element_ix,
        render_frame,
        element_context,
        scene_state,
    } = input;

    let attrs = &element.layout.effective;
    let current_host_clip = HostClipDescriptor {
        clip: scene_state
            .as_ref()
            .map(|state| state.host_clip)
            .unwrap_or_else(|| host_clip_shape(render_frame, attrs)),
        scroll_x: effective_scrollbar_x(attrs),
        scroll_y: effective_scrollbar_y(attrs),
    };
    let current_self_shape = geometry_self_shape(render_frame, attrs);
    let child_render_ctx = traversal.render_ctx.with_host_clip(
        current_host_clip,
        ClipShape {
            rect: current_self_shape.rect,
            radii: current_self_shape.radii,
        },
        attrs.clip_nearby.unwrap_or(false),
    );

    let mut subtree = RenderSubtree::default();

    if element.spec.kind == ElementKind::Paragraph {
        for mount in tree.local_nearby_mounts_ix(element_ix) {
            let branch_subtree = build_nearby_mount_subtree(
                tree,
                mount.ix,
                mount.slot,
                element_context,
                &mut outputs.reborrow(),
                RenderTraversal {
                    scene_ctx: traversal.scene_ctx.clone(),
                    render_ctx: &child_render_ctx,
                    allow_moving_paint_layers: traversal.allow_moving_paint_layers,
                    emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
                    disable_viewport_culling: traversal.disable_viewport_culling,
                    inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
                },
                scene_state.clone(),
            );
            subtree.extend_local(branch_subtree);
        }
    } else {
        element.for_each_retained_local_branch(tree, |branch| match branch {
            RetainedLocalBranchRef::Nearby(mount) => {
                let branch_subtree = build_nearby_mount_subtree(
                    tree,
                    mount.ix,
                    mount.slot,
                    element_context,
                    &mut outputs.reborrow(),
                    RenderTraversal {
                        scene_ctx: traversal.scene_ctx.clone(),
                        render_ctx: &child_render_ctx,
                        allow_moving_paint_layers: traversal.allow_moving_paint_layers,
                        emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
                        disable_viewport_culling: traversal.disable_viewport_culling,
                        inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
                    },
                    scene_state.clone(),
                );
                subtree.extend_local(branch_subtree);
            }
            RetainedLocalBranchRef::Child(child) => {
                let child_scene_ctx = scene_state
                    .clone()
                    .map(|state| {
                        next_scene_context(state, super::element::RetainedPaintPhase::Children)
                    })
                    .unwrap_or_default();
                if !traversal.disable_viewport_culling
                    && should_skip_render_child_subtree(tree, child.ix, &child_scene_ctx)
                {
                    return;
                }
                let branch_subtree = build_element_subtree(
                    tree,
                    child.ix,
                    element_context,
                    &mut outputs.reborrow(),
                    RenderTraversal {
                        scene_ctx: child_scene_ctx,
                        render_ctx: &child_render_ctx,
                        allow_moving_paint_layers: traversal.allow_moving_paint_layers,
                        emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
                        disable_viewport_culling: traversal.disable_viewport_culling,
                        inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
                    },
                );
                subtree.extend_local(branch_subtree);
            }
        });
    }

    subtree.local.extend(wrap_own_content_nodes(
        build_own_content_nodes(
            element,
            render_frame,
            attrs,
            element_context,
            outputs.text_input_focused,
            outputs.text_input_cursor_area,
        ),
        attrs,
        element.spec.kind,
        current_host_clip.clip,
    ));

    if element.spec.kind == ElementKind::Paragraph {
        let paragraph_subtree = build_paragraph_subtree(
            tree,
            element,
            element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx,
                allow_moving_paint_layers: traversal.allow_moving_paint_layers,
                emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
                disable_viewport_culling: traversal.disable_viewport_culling,
                inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
            },
            scene_state.clone(),
            current_host_clip.clip,
        );
        subtree.extend_local(paragraph_subtree);
    }

    subtree.local.extend(wrap_with_host_clip(
        collect_scrollbar_nodes(scene_state.as_ref(), render_frame, attrs),
        current_host_clip.clip,
    ));

    for mount in tree.escape_nearby_mounts_ix(element_ix) {
        subtree.extend_escape(build_nearby_mount_subtree(
            tree,
            mount.ix,
            mount.slot,
            element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx.without_host_clips(),
                allow_moving_paint_layers: traversal.allow_moving_paint_layers,
                emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
                disable_viewport_culling: traversal.disable_viewport_culling,
                inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
            },
            scene_state.clone(),
        ));
    }

    subtree
}

fn build_nearby_mount_subtree(
    tree: &ElementTree,
    nearby_ix: NodeIx,
    slot: NearbySlot,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> RenderSubtree {
    let nearby_scene_ctx = scene_state
        .map(|state| next_scene_context(state, slot.spec().phase))
        .unwrap_or_default();
    let subtree = build_element_subtree(
        tree,
        nearby_ix,
        element_context,
        &mut outputs.reborrow(),
        RenderTraversal {
            scene_ctx: nearby_scene_ctx.clone(),
            render_ctx: traversal.render_ctx,
            allow_moving_paint_layers: traversal.allow_moving_paint_layers,
            emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
            disable_viewport_culling: traversal.disable_viewport_culling,
            inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
        },
    );
    wrap_nearby_subtree_with_nearby_layer_boundary(
        tree.get_ix(nearby_ix),
        &nearby_scene_ctx,
        subtree,
    )
}

fn build_paragraph_subtree(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
    current_host_clip: ClipShape,
) -> RenderSubtree {
    let child_scene_ctx = paragraph_children_scene_context(scene_state.clone());
    let fragment_offset = scene_state
        .as_ref()
        .map(|state| {
            (
                state.adjusted_frame.x - state.frame.x,
                state.adjusted_frame.y - state.frame.y,
            )
        })
        .unwrap_or_default();

    let mut subtree = RenderSubtree::default();
    element.for_each_retained_child(tree, |child| match child.mode {
        RetainedChildMode::Scope => {
            if !traversal.disable_viewport_culling
                && should_skip_render_child_subtree(tree, child.ix, &child_scene_ctx)
            {
                return;
            }
            let child_subtree = build_element_subtree(
                tree,
                child.ix,
                element_context,
                &mut outputs.reborrow(),
                RenderTraversal {
                    scene_ctx: child_scene_ctx.clone(),
                    render_ctx: traversal.render_ctx,
                    allow_moving_paint_layers: traversal.allow_moving_paint_layers,
                    emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
                    disable_viewport_culling: traversal.disable_viewport_culling,
                    inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
                },
            );
            subtree.extend_local(child_subtree);
        }
        RetainedChildMode::InlineEventOnly => {}
    });

    let mut fragment_nodes = Vec::new();
    if let Some(fragments) = &element.layout.paragraph_fragments {
        for frag in fragments {
            let x = frag.x + fragment_offset.0;
            let baseline_y = frag.y + fragment_offset.1 + frag.ascent;
            fragment_nodes.push(RenderNode::Primitive(DrawPrimitive::TextWithFont(
                x,
                baseline_y,
                frag.text.clone(),
                frag.font_size,
                frag.color,
                frag.family.clone(),
                frag.weight,
                frag.italic,
            )));

            if frag.underline || frag.strike {
                let font =
                    make_font_with_style(&frag.family, frag.weight, frag.italic, frag.font_size);
                let word_width =
                    measure_text_visual_metrics_with_font(&font, &frag.text).visual_width;
                fragment_nodes.extend(text_decoration_items(TextDecorationSpec {
                    x,
                    baseline_y,
                    width: word_width,
                    font_size: frag.font_size,
                    color: frag.color,
                    underline: frag.underline,
                    strike: frag.strike,
                }));
            }
        }
    }
    subtree
        .local
        .extend(wrap_with_host_clip(fragment_nodes, current_host_clip));

    subtree
}

fn paragraph_children_scene_context(scene_state: Option<ResolvedNodeState>) -> SceneContext {
    scene_state
        .map(|state| next_scene_context(state, super::element::RetainedPaintPhase::Children))
        .unwrap_or_default()
}

fn build_own_content_nodes(
    element: &Element,
    frame: Frame,
    attrs: &Attrs,
    inherited: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
) -> Vec<RenderNode> {
    let mut nodes = Vec::new();

    match element.spec.kind {
        ElementKind::Text => nodes.extend(render_text_items(frame, attrs, inherited)),
        ElementKind::TextInput => {
            if element.runtime.text_input_focused {
                *text_input_focused = true;
            }

            if text_input_cursor_area.is_none() {
                *text_input_cursor_area =
                    render_text_input_items(&mut nodes, frame, attrs, &element.runtime, inherited);
            } else {
                let _ =
                    render_text_input_items(&mut nodes, frame, attrs, &element.runtime, inherited);
            }
        }
        ElementKind::Multiline => {
            if element.runtime.text_input_focused {
                *text_input_focused = true;
            }

            if text_input_cursor_area.is_none() {
                *text_input_cursor_area = render_multiline_text_input_items(
                    &mut nodes,
                    frame,
                    attrs,
                    &element.runtime,
                    inherited,
                );
            } else {
                let _ = render_multiline_text_input_items(
                    &mut nodes,
                    frame,
                    attrs,
                    &element.runtime,
                    inherited,
                );
            }
        }
        ElementKind::Image => nodes.extend(render_image_nodes(frame, attrs)),
        ElementKind::Video => nodes.extend(render_video_nodes(frame, attrs)),
        _ => {}
    }

    nodes
}

fn wrap_with_clips(nodes: Vec<RenderNode>, clips: Vec<ClipShape>) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    if clips.is_empty() {
        return nodes;
    }

    wrap_with_clip_kind(nodes, clips, false)
}

fn wrap_with_relaxed_clips(nodes: Vec<RenderNode>, clips: Vec<ClipShape>) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    if clips.is_empty() {
        return nodes;
    }

    wrap_with_clip_kind(nodes, clips, true)
}

fn wrap_with_clip_kind(
    nodes: Vec<RenderNode>,
    clips: Vec<ClipShape>,
    relaxed: bool,
) -> Vec<RenderNode> {
    let mut out = Vec::new();
    let mut clipped = Vec::new();

    for node in nodes {
        if matches!(node, RenderNode::ShadowPass { .. }) {
            push_clipped_group(&mut out, &clips, relaxed, &mut clipped);
            out.push(node);
        } else {
            clipped.push(node);
        }
    }

    push_clipped_group(&mut out, &clips, relaxed, &mut clipped);
    out
}

fn push_clipped_group(
    out: &mut Vec<RenderNode>,
    clips: &[ClipShape],
    relaxed: bool,
    clipped: &mut Vec<RenderNode>,
) {
    if clipped.is_empty() {
        return;
    }

    let children = std::mem::take(clipped);
    if relaxed {
        out.push(RenderNode::RelaxedClip {
            clips: clips.to_vec(),
            children,
        });
    } else {
        out.push(RenderNode::Clip {
            clips: clips.to_vec(),
            children,
        });
    }
}

fn wrap_with_shadow_pass(nodes: Vec<RenderNode>) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    vec![RenderNode::ShadowPass { children: nodes }]
}

fn wrap_outer_shadow_nodes(
    nodes: Vec<RenderNode>,
    transform: crate::tree::transform::Affine2,
    render_ctx: &RenderBuildContext,
) -> Vec<RenderNode> {
    wrap_with_shadow_pass(wrap_with_clips(
        wrap_with_transform(nodes, transform),
        render_ctx.shadow_clip_shapes(),
    ))
}

fn wrap_with_host_clip(nodes: Vec<RenderNode>, host_clip: ClipShape) -> Vec<RenderNode> {
    wrap_with_clips(nodes, vec![host_clip])
}

fn wrap_nearby_subtree_with_nearby_layer_boundary(
    element: Option<&Element>,
    scene_ctx: &SceneContext,
    mut subtree: RenderSubtree,
) -> RenderSubtree {
    let Some(element) = element else {
        return subtree;
    };
    let Some(frame) = element.layout.frame else {
        return subtree;
    };
    if subtree.local.is_empty() {
        return subtree;
    }

    let raw_render_frame = element.layout.render_frame.unwrap_or(frame);
    let render_frame = Frame {
        x: raw_render_frame.x - scene_ctx.scroll_dx,
        y: raw_render_frame.y - scene_ctx.scroll_dy,
        ..raw_render_frame
    };
    subtree.local = wrap_with_paint_layer(
        subtree.local,
        element.id.to_wire_u64(),
        PaintLayerPlacement::Fixed,
        PaintLayerPolicy::Cacheable,
        PaintLayerReason::Nearby,
        render_frame,
        0,
    );
    subtree
}

fn wrap_with_paint_layer_if_scroll_container(
    nodes: Vec<RenderNode>,
    element: &Element,
    render_frame: Frame,
) -> Vec<RenderNode> {
    if nodes.is_empty() || !is_scroll_container(element) {
        return nodes;
    }

    wrap_with_paint_layer(
        nodes,
        element.id.to_wire_u64(),
        PaintLayerPlacement::Fixed,
        PaintLayerPolicy::Cacheable,
        PaintLayerReason::ScrollContainer,
        render_frame,
        0,
    )
}

fn wrap_with_paint_layer(
    nodes: Vec<RenderNode>,
    stable_id: u64,
    placement: PaintLayerPlacement,
    policy: PaintLayerPolicy,
    reason: PaintLayerReason,
    render_frame: Frame,
    content_generation: u64,
) -> Vec<RenderNode> {
    vec![RenderNode::PaintLayer(RenderPaintLayer {
        stable_id,
        bounds: Rect {
            x: render_frame.x,
            y: render_frame.y,
            width: render_frame.width,
            height: render_frame.height,
        },
        placement,
        policy,
        reason,
        content_generation,
        children: nodes,
    })]
}

fn wrap_with_dynamic_paint_layer_if_dirty(
    nodes: Vec<RenderNode>,
    element: &Element,
    render_frame: Frame,
    render_damage: bool,
) -> Vec<RenderNode> {
    if nodes.is_empty() || !render_damage {
        return nodes;
    }

    wrap_with_paint_layer(
        nodes,
        element.id.to_wire_u64(),
        PaintLayerPlacement::Fixed,
        PaintLayerPolicy::DynamicRedraw,
        PaintLayerReason::Animation,
        render_frame,
        0,
    )
}

fn wrap_own_content_nodes(
    nodes: Vec<RenderNode>,
    attrs: &Attrs,
    kind: ElementKind,
    host_clip: ClipShape,
) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    if matches!(kind, ElementKind::Image | ElementKind::Video) {
        if !image_video_needs_own_host_clip(attrs) {
            return nodes;
        }

        return vec![RenderNode::RelaxedClip {
            clips: vec![host_clip],
            children: nodes,
        }];
    }

    wrap_with_host_clip(nodes, host_clip)
}

fn image_video_needs_own_host_clip(attrs: &Attrs) -> bool {
    attrs.padding.is_some() || attrs.border_width.is_some() || attrs.border_radius.is_some()
}

fn wrap_with_transform(
    nodes: Vec<RenderNode>,
    transform: crate::tree::transform::Affine2,
) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    if transform.is_identity() {
        return nodes;
    }

    vec![RenderNode::Transform {
        transform,
        children: nodes,
    }]
}

fn wrap_with_alpha(nodes: Vec<RenderNode>, alpha: f32) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    if alpha >= 1.0 {
        return nodes;
    }

    vec![RenderNode::Alpha {
        alpha,
        children: nodes,
    }]
}

fn scene_bounds_for_root(tree: &ElementTree, root: NodeIx) -> Rect {
    tree.get_ix(root)
        .and_then(|element| element.layout.frame)
        .map(Rect::from_frame)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
