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
    Element, ElementKind, ElementTree, Frame, NearbySlot, NodeIx, RenderFragmentCache,
    RenderFragmentCacheKey, RenderFragmentCacheKind, RenderLayerCache, RenderLayerCacheKey,
    RetainedChildMode, RetainedLocalBranchRef,
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
    RenderNode, RenderPaintLayer, RenderPaintLayerBuildParts, RenderPaintLayerChildRef,
    RenderPaintLayerContent, RenderScene, hash_paint_layer_render_nodes,
    paint_layer_bounds_from_visual_bounds, paint_layer_own_content_visual_bounds,
    split_paint_layer_content_owned,
};
use crate::renderer::{make_font_with_style, measure_text_visual_metrics_with_font};
#[cfg(any(test, feature = "bench-diagnostics"))]
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

const RENDER_MOVING_PAINT_LAYER_MIN_RENDER_NODES: usize = 1;
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
        self.scroll_clip_descendant_depth.is_some()
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
    tree_paint_generation: u64,
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

    fn from_fragment_cache(cache: &RenderFragmentCache) -> Self {
        Self {
            local: cache.local.clone(),
            escapes: cache.escapes.clone(),
            text_input_focused: cache.text_input_focused,
            text_input_cursor_area: cache.text_input_cursor_area,
        }
    }

    fn to_fragment_cache(&self, key: RenderFragmentCacheKey) -> RenderFragmentCache {
        RenderFragmentCache {
            key,
            local: self.local.clone(),
            escapes: self.escapes.clone(),
            text_input_focused: self.text_input_focused,
            text_input_cursor_area: self.text_input_cursor_area,
        }
    }
}

fn apply_subtree_outputs(outputs: &mut RenderOutputs<'_>, subtree: &RenderSubtree) {
    if subtree.text_input_focused {
        *outputs.text_input_focused = true;
    }
    if outputs.text_input_cursor_area.is_none() {
        *outputs.text_input_cursor_area = subtree.text_input_cursor_area;
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
    render_tree_scene_with_paint_layer_policy(tree, true, true)
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
            tree_paint_generation: semantic_paint_generation(tree.revision()),
            allow_moving_paint_layers,
            emit_dynamic_paint_layers,
            disable_viewport_culling: false,
            inside_dynamic_paint_layer: false,
        },
    );

    let mut nodes = subtree.into_nodes();
    if allow_moving_paint_layers || emit_dynamic_paint_layers {
        nodes = wrap_with_root_paint_layer(
            nodes,
            tree.get_ix(root_ix),
            semantic_paint_generation(tree.revision()),
        );
    }

    RenderSceneOutput {
        scene: RenderScene { nodes },
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
    if let Some(subtree) =
        try_reuse_moving_paint_layer_cache(tree, ix, element, render_frame, transform, &traversal)
    {
        return subtree;
    }

    let preserve_moving_paint_layer_content = should_preserve_moving_paint_layer_content(
        element,
        render_frame,
        transform,
        render_damage,
        &traversal,
    );
    let emit_dynamic_paint_layer = should_wrap_dynamic_paint_layer(element, attrs, &traversal);
    let child_inside_dynamic_paint_layer =
        should_descend_inside_dynamic_paint_layer(element, render_damage, &traversal);

    let element_context = inherited.merge_with_attrs(attrs);
    let mut local = Vec::new();

    let outer_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, false);
    let has_outer_shadow = !outer_shadow_nodes.is_empty();
    let focused_stable_own_payload =
        should_wrap_focused_own_payload_layer(element, has_outer_shadow);
    let moving_boundary_requirements =
        if should_allow_scroll_moving_paint_layer_at_current_node(&traversal) {
            subtree_moving_boundary_requirements(tree, ix)
        } else {
            MovingBoundaryRequirements::default()
        };
    let can_capture_current_moving_layer_content = preserve_moving_paint_layer_content
        && !moving_boundary_requirements.focused_text_input
        && !moving_boundary_requirements.uncacheable_media_leaf;
    let child_allow_moving_paint_layers = traversal.allow_moving_paint_layers
        && (!can_capture_current_moving_layer_content
            || focused_stable_own_payload
            || moving_boundary_requirements.focused_slider_own_payload);

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
            tree_paint_generation: traversal.tree_paint_generation,
            allow_moving_paint_layers: child_allow_moving_paint_layers,
            emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
            disable_viewport_culling: traversal.disable_viewport_culling
                || can_capture_current_moving_layer_content,
            inside_dynamic_paint_layer: child_inside_dynamic_paint_layer,
        },
    );
    let border_nodes = collect_border_nodes(render_frame, attrs);
    let inherited_host_clips = traversal.render_ctx.full_clip_shapes();
    let inherited_self_clip = traversal.render_ctx.nearest_self_clip();
    let mut emitted_current_element_paint_layer = false;

    if matches!(element.spec.kind, ElementKind::Image | ElementKind::Video) {
        let wrap_media_order_boundary =
            should_wrap_media_leaf_order_boundary(tree, ix, element, &host_content, &traversal);
        let mut media_nodes = Vec::new();
        media_nodes.extend(wrap_outer_shadow_nodes(
            outer_shadow_nodes,
            transform,
            traversal.render_ctx,
        ));
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

        media_nodes.extend(wrap_with_clips(
            wrap_with_transform(decorative_nodes, transform),
            inherited_host_clips.clone(),
        ));
        media_nodes.extend(wrap_with_relaxed_clips(
            wrap_with_transform(host_content.local, transform),
            content_clips,
        ));
        if wrap_media_order_boundary {
            emitted_current_element_paint_layer = true;
            local.extend(wrap_with_paint_layer(
                media_nodes,
                element.id.to_wire_u64(),
                PaintLayerPlacement::Fixed,
                PaintLayerPolicy::DirectOnly,
                PaintLayerReason::StableSubtree,
                render_frame,
                None,
            ));
        } else {
            local.extend(media_nodes);
        }
    } else {
        let needs_same_scroll_moving_boundary =
            !render_damage || (has_outer_shadow && element.runtime.focused_active);
        let can_try_moving_layer = needs_same_scroll_moving_boundary
            && should_allow_scroll_moving_paint_layer_at_current_node(&traversal)
            && !moving_boundary_requirements.focused_text_input
            && !moving_boundary_requirements.uncacheable_media_leaf
            && host_content.escapes.is_empty();
        let fixed_focused_own_layer = !can_try_moving_layer
            && should_wrap_focused_own_payload_layer(element, has_outer_shadow)
            && host_content.escapes.is_empty();
        let normal_nodes = if fixed_focused_own_layer {
            emitted_current_element_paint_layer = true;
            let own_nodes =
                wrap_outer_shadow_nodes(outer_shadow_nodes, transform, traversal.render_ctx);
            let mut decorative_nodes = Vec::new();
            decorative_nodes.extend(background_nodes);
            decorative_nodes.extend(inset_shadow_nodes);
            decorative_nodes.extend(host_content.local);
            decorative_nodes.extend(border_nodes);
            let child_nodes = wrap_with_clips(
                wrap_with_transform(decorative_nodes, transform),
                inherited_host_clips.clone(),
            );
            local.extend(wrap_with_focused_own_payload_layer(
                own_nodes,
                child_nodes,
                element,
                render_frame,
            ));
            Vec::new()
        } else if can_try_moving_layer {
            let moving_outer_shadow_nodes = wrap_outer_shadow_nodes(
                outer_shadow_nodes.clone(),
                Affine2::identity(),
                traversal.render_ctx,
            );
            if focused_stable_own_payload {
                emitted_current_element_paint_layer = true;
                let mut own_nodes = Vec::new();
                own_nodes.extend(moving_outer_shadow_nodes);
                own_nodes.extend(background_nodes);
                own_nodes.extend(inset_shadow_nodes);
                own_nodes.extend(border_nodes);
                wrap_with_explicit_moving_own_payload_layer(MovingPaintLayerOwnPayloadWrapInput {
                    own_nodes,
                    child_nodes: host_content.local,
                    element,
                    cache_key: Some(moving_paint_layer_cache_key(
                        tree,
                        ix,
                        element,
                        render_frame,
                    )),
                    render_frame,
                    transform,
                    text_input_focused: host_content.text_input_focused,
                    inside_local_transform: traversal.render_ctx.inside_local_transform(),
                    ancestor_clip_context: traversal.render_ctx,
                })
            } else {
                let mut normal_nodes = Vec::new();
                normal_nodes.extend(background_nodes);
                normal_nodes.extend(inset_shadow_nodes);
                normal_nodes.extend(host_content.local);
                normal_nodes.extend(border_nodes);

                if can_emit_explicit_moving_paint_layer(
                    element,
                    render_frame,
                    transform,
                    host_content.text_input_focused,
                    traversal.render_ctx.inside_local_transform(),
                    &[
                        moving_outer_shadow_nodes.as_slice(),
                        normal_nodes.as_slice(),
                    ],
                ) {
                    emitted_current_element_paint_layer = true;
                    let mut moving_nodes = Vec::with_capacity(
                        moving_outer_shadow_nodes
                            .len()
                            .saturating_add(normal_nodes.len()),
                    );
                    moving_nodes.extend(moving_outer_shadow_nodes);
                    moving_nodes.extend(normal_nodes);
                    wrap_with_explicit_moving_paint_layer_payload(
                        MovingPaintLayerPayloadWrapInput {
                            nodes: moving_nodes,
                            element,
                            cache_key: Some(moving_paint_layer_cache_key(
                                tree,
                                ix,
                                element,
                                render_frame,
                            )),
                            render_frame,
                            transform,
                            render_damage,
                            text_input_focused: host_content.text_input_focused,
                            inside_local_transform: traversal.render_ctx.inside_local_transform(),
                            ancestor_clip_context: traversal.render_ctx,
                            stable_own_payload_generation: element.runtime.focused_active
                                && !outer_shadow_nodes.is_empty(),
                        },
                    )
                } else {
                    local.extend(wrap_outer_shadow_nodes(
                        outer_shadow_nodes,
                        transform,
                        traversal.render_ctx,
                    ));
                    wrap_with_transform(normal_nodes, transform)
                }
            }
        } else {
            let mut normal_nodes = Vec::new();
            normal_nodes.extend(background_nodes);
            normal_nodes.extend(inset_shadow_nodes);
            normal_nodes.extend(host_content.local);
            normal_nodes.extend(border_nodes);
            local.extend(wrap_outer_shadow_nodes(
                outer_shadow_nodes,
                transform,
                traversal.render_ctx,
            ));
            wrap_with_transform(normal_nodes, transform)
        };
        local.extend(wrap_with_clips(
            wrap_with_paint_layer_if_scroll_container(
                normal_nodes,
                element,
                render_frame,
                traversal.tree_paint_generation,
            ),
            inherited_host_clips,
        ));
    }

    let escapes = wrap_with_alpha(wrap_with_transform(host_content.escapes, transform), alpha);
    let local = if emitted_current_element_paint_layer {
        local
    } else {
        wrap_with_dynamic_paint_layer_if_dirty(
            local,
            element,
            render_frame,
            emit_dynamic_paint_layer,
        )
    };

    RenderSubtree {
        local: wrap_with_alpha(local, alpha),
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
    cache_key: Option<RenderLayerCacheKey>,
    render_frame: Frame,
    transform: Affine2,
    render_damage: bool,
    text_input_focused: bool,
    inside_local_transform: bool,
    ancestor_clip_context: &'a RenderBuildContext,
    stable_own_payload_generation: bool,
}

struct MovingPaintLayerOwnPayloadWrapInput<'a> {
    own_nodes: Vec<RenderNode>,
    child_nodes: Vec<RenderNode>,
    element: &'a Element,
    cache_key: Option<RenderLayerCacheKey>,
    render_frame: Frame,
    transform: Affine2,
    text_input_focused: bool,
    inside_local_transform: bool,
    ancestor_clip_context: &'a RenderBuildContext,
}

fn wrap_with_explicit_moving_own_payload_layer(
    input: MovingPaintLayerOwnPayloadWrapInput<'_>,
) -> Vec<RenderNode> {
    let MovingPaintLayerOwnPayloadWrapInput {
        own_nodes,
        child_nodes,
        element,
        cache_key,
        render_frame,
        transform,
        text_input_focused,
        inside_local_transform,
        ancestor_clip_context,
    } = input;

    let Some(placement) = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        text_input_focused,
        inside_local_transform,
    ) else {
        let mut nodes = own_nodes;
        nodes.extend(child_nodes);
        return wrap_with_transform(nodes, transform);
    };

    let own_nodes = localize_moving_paint_layer_nodes(
        strip_moving_paint_layer_payload_ancestor_clips(own_nodes, ancestor_clip_context),
        placement.local_origin_x,
        placement.local_origin_y,
    );
    if own_nodes.is_empty() {
        let child_nodes = localize_moving_paint_layer_nodes(
            strip_moving_paint_layer_payload_ancestor_clips(child_nodes, ancestor_clip_context),
            placement.local_origin_x,
            placement.local_origin_y,
        );
        return wrap_with_transform(child_nodes, placement.transform);
    }

    let child_nodes = localize_moving_paint_layer_nodes(
        strip_moving_paint_layer_payload_ancestor_clips(child_nodes, ancestor_clip_context),
        placement.local_origin_x,
        placement.local_origin_y,
    );
    let visual_bounds = paint_layer_own_content_visual_bounds(&own_nodes);
    let bounds = paint_layer_bounds_from_visual_bounds(visual_bounds, placement.bounds);
    let child_refs = if child_nodes.is_empty() {
        Vec::new()
    } else {
        vec![RenderPaintLayerChildRef::from_nodes(child_nodes)]
    };
    #[cfg(test)]
    let raw_children = own_nodes
        .iter()
        .cloned()
        .chain(
            child_refs
                .iter()
                .flat_map(|child| child.nodes.iter().cloned()),
        )
        .collect();
    #[cfg(not(test))]
    let raw_children = Vec::new();
    let content_generation = moving_paint_layer_own_content_generation(&own_nodes, bounds);
    let layer = RenderPaintLayer::from_prepared_children(
        RenderPaintLayerBuildParts {
            stable_id: element.id.to_wire_u64(),
            root_id: element.id.to_wire_u64(),
            bounds,
            placement: PaintLayerPlacement::ScrollMoving,
            policy: PaintLayerPolicy::Cacheable,
            reason: PaintLayerReason::StableSubtree,
            content_generation,
            visual_bounds,
        },
        RenderPaintLayerContent {
            own_nodes,
            child_refs,
        },
        raw_children,
    );

    if let Some(key) = cache_key {
        element
            .refresh
            .render_layer_cache
            .borrow_mut()
            .replace(RenderLayerCache {
                key,
                layer: layer.clone(),
            });
    }

    wrap_with_transform(vec![RenderNode::PaintLayer(layer)], placement.transform)
}

fn wrap_with_explicit_moving_paint_layer_payload(
    input: MovingPaintLayerPayloadWrapInput<'_>,
) -> Vec<RenderNode> {
    let MovingPaintLayerPayloadWrapInput {
        nodes,
        element,
        cache_key,
        render_frame,
        transform,
        render_damage,
        text_input_focused,
        inside_local_transform,
        ancestor_clip_context,
        stable_own_payload_generation,
    } = input;

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
    #[cfg(test)]
    let raw_children = local_children.clone();
    #[cfg(not(test))]
    let raw_children = Vec::new();
    let content = split_paint_layer_content_owned(local_children);
    if content.own_nodes.is_empty() {
        let child_nodes = content
            .child_refs
            .into_iter()
            .fold(Vec::new(), |mut nodes, child| {
                nodes.extend(child.nodes.iter().cloned());
                nodes
            });
        return wrap_with_transform(child_nodes, placement.transform);
    }
    let visual_bounds = paint_layer_own_content_visual_bounds(&content.own_nodes);
    let bounds = paint_layer_bounds_from_visual_bounds(visual_bounds, placement.bounds);
    let cache_own_payload = !render_damage || stable_own_payload_generation;
    let (policy, reason, content_generation) = if cache_own_payload {
        (
            PaintLayerPolicy::Cacheable,
            PaintLayerReason::StableSubtree,
            if stable_own_payload_generation {
                moving_paint_layer_own_content_generation(&content.own_nodes, bounds)
            } else {
                element.refresh.paint_generation
            },
        )
    } else {
        (
            PaintLayerPolicy::DynamicRedraw,
            PaintLayerReason::Animation,
            0,
        )
    };
    let layer = RenderPaintLayer::from_prepared_children(
        RenderPaintLayerBuildParts {
            stable_id: element.id.to_wire_u64(),
            root_id: element.id.to_wire_u64(),
            bounds,
            placement: PaintLayerPlacement::ScrollMoving,
            policy,
            reason,
            content_generation,
            visual_bounds,
        },
        content,
        raw_children,
    );

    if policy == PaintLayerPolicy::Cacheable
        && let Some(key) = cache_key
    {
        element
            .refresh
            .render_layer_cache
            .borrow_mut()
            .replace(RenderLayerCache {
                key,
                layer: layer.clone(),
            });
    }

    wrap_with_transform(vec![RenderNode::PaintLayer(layer)], placement.transform)
}

fn can_emit_explicit_moving_paint_layer(
    element: &Element,
    render_frame: Frame,
    transform: Affine2,
    text_input_focused: bool,
    inside_local_transform: bool,
    node_groups: &[&[RenderNode]],
) -> bool {
    let Some(placement) = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        text_input_focused,
        inside_local_transform,
    ) else {
        return false;
    };

    should_emit_moving_paint_layer_groups(node_groups, placement)
}

fn try_reuse_moving_paint_layer_cache(
    tree: &ElementTree,
    ix: NodeIx,
    element: &Element,
    render_frame: Frame,
    transform: Affine2,
    traversal: &RenderTraversal<'_>,
) -> Option<RenderSubtree> {
    if element.refresh.render_dirty || element.refresh.render_descendant_dirty {
        return None;
    }
    if !should_allow_scroll_moving_paint_layer_at_current_node(traversal) {
        return None;
    }

    let placement = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        false,
        traversal.render_ctx.inside_local_transform(),
    )?;
    let key = moving_paint_layer_cache_key(tree, ix, element, render_frame);
    let layer = element
        .refresh
        .render_layer_cache
        .borrow()
        .as_ref()
        .filter(|cache| cache.key == key)
        .map(|cache| cache.layer.clone())?;

    Some(RenderSubtree {
        local: wrap_with_transform(vec![RenderNode::PaintLayer(layer)], placement.transform),
        escapes: Vec::new(),
        text_input_focused: false,
        text_input_cursor_area: None,
    })
}

fn moving_paint_layer_cache_key(
    tree: &ElementTree,
    ix: NodeIx,
    element: &Element,
    render_frame: Frame,
) -> RenderLayerCacheKey {
    RenderLayerCacheKey {
        paint_generation: element.refresh.paint_generation,
        topology: tree.topology_dependency_key_ix(ix),
        bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: render_frame.width.ceil(),
            height: render_frame.height.ceil(),
        },
    }
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

    if is_scroll_container(element) {
        return None;
    }

    if inside_local_transform {
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
    should_emit_moving_paint_layer_groups(&[nodes], placement)
}

fn should_emit_moving_paint_layer_groups(
    node_groups: &[&[RenderNode]],
    _placement: MovingPaintLayerPlacement,
) -> bool {
    if node_groups.iter().all(|nodes| nodes.is_empty()) {
        return false;
    }

    if !node_groups
        .iter()
        .all(|nodes| moving_paint_layer_children_are_supported(nodes))
    {
        return false;
    }

    let node_count = node_groups
        .iter()
        .map(|nodes| render_node_count(nodes))
        .sum::<usize>();
    node_count >= RENDER_MOVING_PAINT_LAYER_MIN_RENDER_NODES
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

    let Some(_placement) = moving_paint_layer_static_placement(
        element,
        render_frame,
        transform,
        false,
        traversal.render_ctx.inside_local_transform(),
    ) else {
        return false;
    };

    true
}

fn should_allow_scroll_moving_paint_layer_at_current_node(traversal: &RenderTraversal<'_>) -> bool {
    traversal.allow_moving_paint_layers
        && traversal.render_ctx.is_scroll_moving_paint_layer_context()
}

fn should_emit_dynamic_paint_layer(element: &Element, traversal: &RenderTraversal<'_>) -> bool {
    traversal.emit_dynamic_paint_layers && !is_scroll_container(element)
}

fn should_wrap_focused_own_payload_layer(element: &Element, has_outer_shadow: bool) -> bool {
    element.spec.kind == ElementKind::Slider && element.runtime.focused_active && has_outer_shadow
}

fn should_wrap_media_leaf_order_boundary(
    tree: &ElementTree,
    ix: NodeIx,
    element: &Element,
    host_content: &RenderSubtree,
    traversal: &RenderTraversal<'_>,
) -> bool {
    matches!(element.spec.kind, ElementKind::Image | ElementKind::Video)
        && host_content.escapes.is_empty()
        && should_allow_scroll_moving_paint_layer_at_current_node(traversal)
        && super::element::parent_ix_from_link(tree.parent_link_of(ix))
            .and_then(|parent_ix| tree.get_ix(parent_ix))
            .is_some_and(|parent| parent.spec.kind == ElementKind::Slider)
}

#[derive(Clone, Copy, Debug, Default)]
struct MovingBoundaryRequirements {
    focused_text_input: bool,
    focused_slider_own_payload: bool,
    uncacheable_media_leaf: bool,
}

fn subtree_moving_boundary_requirements(
    tree: &ElementTree,
    ix: NodeIx,
) -> MovingBoundaryRequirements {
    let Some(element) = tree.get_ix(ix) else {
        return MovingBoundaryRequirements::default();
    };

    let own = MovingBoundaryRequirements {
        focused_text_input: element.spec.kind.is_text_input_family()
            && element.runtime.text_input_focused,
        focused_slider_own_payload: element.spec.kind == ElementKind::Slider
            && element.runtime.focused_active
            && attrs_have_outer_shadow(&element.layout.effective),
        uncacheable_media_leaf: matches!(
            element.spec.kind,
            ElementKind::Image | ElementKind::Video
        ),
    };
    if own.focused_text_input && own.focused_slider_own_payload && own.uncacheable_media_leaf {
        return own;
    }

    tree.child_ixs(ix)
        .into_iter()
        .map(|child_ix| subtree_moving_boundary_requirements(tree, child_ix))
        .chain(
            tree.nearby_ixs(ix)
                .into_iter()
                .map(|mount| subtree_moving_boundary_requirements(tree, mount.ix)),
        )
        .fold(own, |mut acc, child| {
            acc.focused_text_input |= child.focused_text_input;
            acc.focused_slider_own_payload |= child.focused_slider_own_payload;
            acc.uncacheable_media_leaf |= child.uncacheable_media_leaf;
            acc
        })
}

fn attrs_have_outer_shadow(attrs: &Attrs) -> bool {
    attrs
        .box_shadows
        .as_ref()
        .is_some_and(|shadows| shadows.iter().any(|shadow| !shadow.inset))
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

    false
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

fn semantic_paint_generation(tree_revision: u64) -> u64 {
    tree_revision.saturating_add(1)
}

#[cfg(test)]
fn moving_paint_layer_content_generation(nodes: &[RenderNode], payload_bounds: Rect) -> u64 {
    let content = crate::render_scene::split_paint_layer_content(nodes);
    moving_paint_layer_own_content_generation(&content.own_nodes, payload_bounds)
}

fn moving_paint_layer_own_content_generation(
    own_nodes: &[RenderNode],
    payload_bounds: Rect,
) -> u64 {
    let mut hasher = MovingPaintLayerPayloadContentHasher::default();
    hash_paint_layer_render_nodes(
        &mut hasher,
        own_nodes,
        PaintLayerHashFloat::Quantized {
            scale: RENDER_MOVING_PAINT_LAYER_PAYLOAD_CONTENT_HASH_COORD_SCALE,
        },
        Some(payload_bounds),
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
            let children = strip_moving_paint_layer_payload_ancestor_clips(
                layer.content_nodes(),
                ancestor_clip_context,
            );
            vec![RenderNode::PaintLayer(layer.with_children(children))]
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
        | RenderNode::ShadowPass { children }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => moving_paint_layer_children_are_supported(children),
        RenderNode::PaintLayer(_) => true,
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
        RenderNode::PaintLayer(layer) => {
            let bounds = Rect {
                x: layer.bounds.x - origin_x,
                y: layer.bounds.y - origin_y,
                ..layer.bounds
            };
            let children =
                localize_moving_paint_layer_nodes(layer.content_nodes(), origin_x, origin_y);
            RenderNode::PaintLayer(layer.with_bounds_and_children(bounds, children))
        }
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
            RenderNode::PaintLayer(layer) => {
                1 + render_node_count(&layer.own_nodes)
                    + layer
                        .child_refs
                        .iter()
                        .map(|child| render_node_count(&child.nodes))
                        .sum::<usize>()
            }
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
                    tree_paint_generation: traversal.tree_paint_generation,
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
                        tree_paint_generation: traversal.tree_paint_generation,
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
                        tree_paint_generation: traversal.tree_paint_generation,
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

    let mut own_text_input_focused = false;
    let mut own_text_input_cursor_area = None;
    let own_content_nodes = build_own_content_nodes(
        element,
        render_frame,
        attrs,
        element_context,
        &mut own_text_input_focused,
        &mut own_text_input_cursor_area,
    );
    if own_text_input_focused {
        *outputs.text_input_focused = true;
    }
    if outputs.text_input_cursor_area.is_none() {
        *outputs.text_input_cursor_area = own_text_input_cursor_area;
    }
    subtree.text_input_focused |= own_text_input_focused;
    if subtree.text_input_cursor_area.is_none() {
        subtree.text_input_cursor_area = own_text_input_cursor_area;
    }

    subtree.local.extend(wrap_own_content_nodes(
        own_content_nodes,
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
                tree_paint_generation: traversal.tree_paint_generation,
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
                tree_paint_generation: traversal.tree_paint_generation,
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

    let cache_key = tree.get_ix(nearby_ix).and_then(|element| {
        nearby_render_fragment_cache_key(
            tree,
            nearby_ix,
            element,
            &nearby_scene_ctx,
            traversal.render_ctx,
        )
    });
    if let Some(cache_key) = cache_key
        && let Some(cached) = tree
            .get_ix(nearby_ix)
            .filter(|element| {
                !(element.refresh.render_dirty || element.refresh.render_descendant_dirty)
            })
            .and_then(|element| {
                element
                    .refresh
                    .render_fragment_cache
                    .borrow()
                    .as_ref()
                    .filter(|cache| cache.key == cache_key)
                    .cloned()
            })
    {
        let subtree = RenderSubtree::from_fragment_cache(&cached);
        apply_subtree_outputs(outputs, &subtree);
        return subtree;
    }

    let subtree = build_element_subtree(
        tree,
        nearby_ix,
        element_context,
        &mut outputs.reborrow(),
        RenderTraversal {
            scene_ctx: nearby_scene_ctx.clone(),
            render_ctx: traversal.render_ctx,
            tree_paint_generation: traversal.tree_paint_generation,
            allow_moving_paint_layers: traversal.allow_moving_paint_layers,
            emit_dynamic_paint_layers: traversal.emit_dynamic_paint_layers,
            disable_viewport_culling: traversal.disable_viewport_culling,
            inside_dynamic_paint_layer: traversal.inside_dynamic_paint_layer,
        },
    );
    let subtree = wrap_nearby_subtree_with_nearby_layer_boundary(
        tree.get_ix(nearby_ix),
        &nearby_scene_ctx,
        traversal.render_ctx,
        subtree,
    );
    if let Some(cache_key) = cache_key
        && render_subtree_has_cacheable_fragment(&subtree)
        && let Some(element) = tree.get_ix(nearby_ix)
    {
        element
            .refresh
            .render_fragment_cache
            .borrow_mut()
            .replace(subtree.to_fragment_cache(cache_key));
    }
    subtree
}

fn render_subtree_has_cacheable_fragment(subtree: &RenderSubtree) -> bool {
    !subtree.local.is_empty()
        || !subtree.escapes.is_empty()
        || subtree.text_input_focused
        || subtree.text_input_cursor_area.is_some()
}

fn nearby_render_fragment_cache_key(
    tree: &ElementTree,
    ix: NodeIx,
    element: &Element,
    scene_ctx: &SceneContext,
    render_ctx: &RenderBuildContext,
) -> Option<RenderFragmentCacheKey> {
    let frame = element.layout.frame?;
    let raw_render_frame = element.layout.render_frame.unwrap_or(frame);
    let render_frame = Frame {
        x: raw_render_frame.x - scene_ctx.scroll_dx,
        y: raw_render_frame.y - scene_ctx.scroll_dy,
        ..raw_render_frame
    };

    Some(RenderFragmentCacheKey {
        kind: RenderFragmentCacheKind::Nearby,
        paint_generation: element.refresh.paint_generation,
        topology: tree.topology_dependency_key_ix(ix),
        bounds: Rect::from_frame(render_frame),
        context: nearby_render_fragment_context_key(scene_ctx, render_ctx),
    })
}

fn nearby_render_fragment_context_key(
    scene_ctx: &SceneContext,
    render_ctx: &RenderBuildContext,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_f32_bits(&mut hasher, scene_ctx.scroll_dx);
    hash_f32_bits(&mut hasher, scene_ctx.scroll_dy);
    hasher.write_u8(scene_ctx.front_nearby_subtree as u8);
    hasher.write_u8(scene_ctx.front_nearby_root as u8);
    scene_ctx
        .visible_clip
        .iter()
        .for_each(|clip| hash_clip_shape(&mut hasher, clip));
    scene_ctx
        .nearby_visible_clip
        .iter()
        .for_each(|clip| hash_clip_shape(&mut hasher, clip));
    render_ctx
        .full_clip_shapes()
        .iter()
        .for_each(|clip| hash_clip_shape(&mut hasher, clip));
    hasher.finish()
}

fn hash_clip_shape(hasher: &mut DefaultHasher, clip: &ClipShape) {
    hash_rect_bits(hasher, clip.rect);
    match clip.radii {
        Some(radii) => {
            hasher.write_u8(1);
            hash_f32_bits(hasher, radii.tl);
            hash_f32_bits(hasher, radii.tr);
            hash_f32_bits(hasher, radii.br);
            hash_f32_bits(hasher, radii.bl);
        }
        None => hasher.write_u8(0),
    }
}

fn hash_rect_bits(hasher: &mut DefaultHasher, rect: Rect) {
    hash_f32_bits(hasher, rect.x);
    hash_f32_bits(hasher, rect.y);
    hash_f32_bits(hasher, rect.width);
    hash_f32_bits(hasher, rect.height);
}

fn hash_f32_bits(hasher: &mut DefaultHasher, value: f32) {
    hasher.write_u32(if value == 0.0 { 0.0f32 } else { value }.to_bits());
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
                    tree_paint_generation: traversal.tree_paint_generation,
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
    render_ctx: &RenderBuildContext,
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
    let local = strip_moving_paint_layer_payload_ancestor_clips(
        std::mem::take(&mut subtree.local),
        render_ctx,
    );
    if local.is_empty() {
        return subtree;
    }
    subtree.local = wrap_with_clips(
        wrap_with_paint_layer(
            local,
            element.id.to_wire_u64(),
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::Cacheable,
            PaintLayerReason::Nearby,
            render_frame,
            None,
        ),
        render_ctx.full_clip_shapes(),
    );
    subtree
}

fn wrap_with_paint_layer_if_scroll_container(
    nodes: Vec<RenderNode>,
    element: &Element,
    render_frame: Frame,
    _tree_paint_generation: u64,
) -> Vec<RenderNode> {
    if nodes.is_empty() || !is_scroll_container(element) {
        return nodes;
    }

    wrap_with_paint_layer(
        nodes,
        element.id.to_wire_u64(),
        PaintLayerPlacement::Fixed,
        PaintLayerPolicy::DynamicRedraw,
        PaintLayerReason::ScrollContainer,
        render_frame,
        None,
    )
}

fn wrap_with_root_paint_layer(
    nodes: Vec<RenderNode>,
    root: Option<&Element>,
    _tree_paint_generation: u64,
) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    let Some(root) = root else {
        return nodes;
    };
    let Some(frame) = root.layout.frame else {
        return nodes;
    };
    if root.refresh.render_dirty || element_has_active_animation(root, &root.layout.effective) {
        return nodes;
    }

    wrap_with_paint_layer(
        nodes,
        root.id.to_wire_u64(),
        PaintLayerPlacement::Fixed,
        PaintLayerPolicy::Cacheable,
        PaintLayerReason::Root,
        frame,
        None,
    )
}

fn wrap_with_focused_own_payload_layer(
    own_nodes: Vec<RenderNode>,
    child_nodes: Vec<RenderNode>,
    element: &Element,
    render_frame: Frame,
) -> Vec<RenderNode> {
    let child_layer_nodes = wrap_with_paint_layer(
        child_nodes,
        focused_own_payload_child_layer_id(element),
        PaintLayerPlacement::Fixed,
        PaintLayerPolicy::DynamicRedraw,
        PaintLayerReason::Animation,
        render_frame,
        None,
    );
    let child_refs = if child_layer_nodes.is_empty() {
        Vec::new()
    } else {
        vec![RenderPaintLayerChildRef::from_nodes(child_layer_nodes)]
    };
    let visual_bounds = paint_layer_own_content_visual_bounds(&own_nodes);
    let nominal_bounds = Rect {
        x: render_frame.x,
        y: render_frame.y,
        width: render_frame.width,
        height: render_frame.height,
    };
    let bounds = paint_layer_bounds_from_visual_bounds(visual_bounds, nominal_bounds);
    let content_generation = moving_paint_layer_own_content_generation(&own_nodes, bounds);
    let layer = RenderPaintLayer::from_prepared_children(
        RenderPaintLayerBuildParts {
            stable_id: element.id.to_wire_u64(),
            root_id: element.id.to_wire_u64(),
            bounds,
            placement: PaintLayerPlacement::Fixed,
            policy: PaintLayerPolicy::Cacheable,
            reason: PaintLayerReason::StableSubtree,
            content_generation,
            visual_bounds,
        },
        RenderPaintLayerContent {
            own_nodes,
            child_refs,
        },
        Vec::new(),
    );

    wrap_with_shadow_pass(vec![RenderNode::PaintLayer(layer)])
}

fn focused_own_payload_child_layer_id(element: &Element) -> u64 {
    element
        .id
        .to_wire_u64()
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0xF0C5_1A7E_5EED)
}

fn wrap_with_paint_layer(
    nodes: Vec<RenderNode>,
    stable_id: u64,
    placement: PaintLayerPlacement,
    policy: PaintLayerPolicy,
    reason: PaintLayerReason,
    render_frame: Frame,
    content_generation: Option<u64>,
) -> Vec<RenderNode> {
    let nominal_bounds = Rect {
        x: render_frame.x,
        y: render_frame.y,
        width: render_frame.width,
        height: render_frame.height,
    };
    #[cfg(test)]
    let raw_children = nodes.clone();
    #[cfg(not(test))]
    let raw_children = Vec::new();
    let content = split_paint_layer_content_owned(nodes);
    let visual_bounds = paint_layer_own_content_visual_bounds(&content.own_nodes);
    let bounds = if reason != PaintLayerReason::ScrollContainer {
        paint_layer_bounds_from_visual_bounds(visual_bounds, nominal_bounds)
    } else {
        nominal_bounds
    };
    let content_generation = if let Some(content_generation) = content_generation {
        content_generation
    } else if policy == PaintLayerPolicy::Cacheable
        || (policy == PaintLayerPolicy::DynamicRedraw
            && reason == PaintLayerReason::ScrollContainer)
    {
        moving_paint_layer_own_content_generation(&content.own_nodes, bounds)
    } else {
        0
    };

    vec![RenderNode::PaintLayer(
        RenderPaintLayer::from_prepared_children(
            RenderPaintLayerBuildParts {
                stable_id,
                root_id: stable_id,
                bounds,
                placement,
                policy,
                reason,
                content_generation,
                visual_bounds,
            },
            content,
            raw_children,
        ),
    )]
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
        None,
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
