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
    Element, ElementKind, ElementTree, Frame, NearbySlot, NodeIx, RenderSubtreeCache,
    RenderSubtreeKey, RetainedChildMode, RetainedLocalBranchRef,
};
use super::geometry::{ClipShape, Rect, host_clip_shape, self_shape as geometry_self_shape};
use super::layout::FontContext;
use super::scene::{
    ResolvedNodeState, SceneContext, child_context as next_scene_context, resolve_node_state,
};
use super::transform::element_transform;
#[cfg(test)]
use crate::events::{RegistryRebuildPayload, registry_builder};
use crate::render_scene::{DrawPrimitive, RenderNode, RenderScene};
use crate::renderer::{make_font_with_style, measure_text_visual_metrics_with_font};
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Write};
use std::hash::{Hash, Hasher};

const RENDER_SUBTREE_CACHE_MAX_RENDER_NODES: usize = 128;
const RENDER_SUBTREE_CACHE_STORE_BUDGET: usize = 32;

#[cfg(test)]
thread_local! {
    static RENDER_SUBTREE_CACHE_LOOKUP_KEY_BUILDS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_render_subtree_cache_lookup_key_builds() {
    RENDER_SUBTREE_CACHE_LOOKUP_KEY_BUILDS.with(|builds| builds.set(0));
}

#[cfg(test)]
pub(crate) fn render_subtree_cache_lookup_key_builds() -> usize {
    RENDER_SUBTREE_CACHE_LOOKUP_KEY_BUILDS.with(Cell::get)
}

#[cfg(test)]
fn record_render_subtree_cache_lookup_key_build() {
    RENDER_SUBTREE_CACHE_LOOKUP_KEY_BUILDS.with(|builds| builds.set(builds.get() + 1));
}

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
        Self {
            scene_bounds: self.scene_bounds,
            inherited_host_clips,
            inherited_self_clips,
            nearby_host_clips,
            nearby_self_clips,
        }
    }

    fn without_host_clips(&self) -> Self {
        Self {
            scene_bounds: self.scene_bounds,
            inherited_host_clips: self.nearby_host_clips.clone(),
            inherited_self_clips: self.nearby_self_clips.clone(),
            nearby_host_clips: self.nearby_host_clips.clone(),
            nearby_self_clips: self.nearby_self_clips.clone(),
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
            .filter_map(|clip| match (clip.scroll_x, clip.scroll_y) {
                (false, false) => None,
                (true, true) => Some(clip.clip),
                (true, false) => Some(ClipShape {
                    rect: Rect {
                        x: clip.clip.rect.x,
                        y: self.scene_bounds.y,
                        width: clip.clip.rect.width,
                        height: self.scene_bounds.height,
                    },
                    radii: None,
                }),
                (false, true) => Some(ClipShape {
                    rect: Rect {
                        x: self.scene_bounds.x,
                        y: clip.clip.rect.y,
                        width: self.scene_bounds.width,
                        height: clip.clip.rect.height,
                    },
                    radii: None,
                }),
            })
            .collect()
    }

    fn nearest_self_clip(&self) -> Option<ClipShape> {
        self.inherited_self_clips.last().copied()
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
    cache_store_budget: &'a Cell<usize>,
}

#[derive(Clone, Default)]
struct RenderSubtree {
    local: Vec<RenderNode>,
    escapes: Vec<RenderNode>,
    text_input_focused: bool,
    text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

impl RenderSubtree {
    fn from_cache(cache: RenderSubtreeCache) -> Self {
        Self {
            local: cache.local,
            escapes: cache.escapes,
            text_input_focused: cache.text_input_focused,
            text_input_cursor_area: cache.text_input_cursor_area,
        }
    }

    fn to_cache(&self, key: RenderSubtreeKey) -> RenderSubtreeCache {
        RenderSubtreeCache {
            key,
            local: self.local.clone(),
            escapes: self.escapes.clone(),
            text_input_focused: self.text_input_focused,
            text_input_cursor_area: self.text_input_cursor_area,
        }
    }

    fn render_node_count(&self) -> usize {
        render_node_count(&self.local) + render_node_count(&self.escapes)
    }

    fn extend_local(&mut self, subtree: RenderSubtree) {
        self.merge_outputs(&subtree);
        self.local.extend(subtree.local);
        self.escapes.extend(subtree.escapes);
    }

    fn extend_escape(&mut self, subtree: RenderSubtree) {
        self.merge_outputs(&subtree);
        self.escapes.extend(subtree.into_nodes());
    }

    fn merge_outputs(&mut self, subtree: &RenderSubtree) {
        self.text_input_focused |= subtree.text_input_focused;
        if self.text_input_cursor_area.is_none() {
            self.text_input_cursor_area = subtree.text_input_cursor_area;
        }
    }

    fn merge_output_values(
        &mut self,
        text_input_focused: bool,
        text_input_cursor_area: Option<(f32, f32, f32, f32)>,
    ) {
        self.text_input_focused |= text_input_focused;
        if self.text_input_cursor_area.is_none() {
            self.text_input_cursor_area = text_input_cursor_area;
        }
    }

    fn into_nodes(self) -> Vec<RenderNode> {
        let mut nodes = self.local;
        nodes.extend(self.escapes);
        nodes
    }
}

/// Render the tree without rebuilding event registry metadata and without using
/// retained render subtree caches.
///
/// Reads from pre-scaled attrs (layout pass must run first). This is kept as a
/// safe baseline for correctness tests and performance regression benchmarks.
pub(crate) fn render_tree_scene(tree: &ElementTree) -> RenderSceneOutput {
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
    let cache_store_budget = Cell::new(0);
    let subtree = build_element_subtree(
        tree,
        root_ix,
        &FontContext::default(),
        &mut outputs,
        RenderTraversal {
            scene_ctx: SceneContext::default(),
            render_ctx: &render_ctx,
            cache_store_budget: &cache_store_budget,
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

pub(crate) fn render_tree_scene_cached(tree: &mut ElementTree) -> RenderSceneOutput {
    if tree.has_render_refresh_damage() && !tree.has_render_subtree_cache() {
        return render_tree_scene(tree);
    }

    let Some(root_ix) = tree.root_ix() else {
        return RenderSceneOutput {
            scene: RenderScene::default(),
            text_input_focused: false,
            text_input_cursor_area: None,
        };
    };

    let render_ctx = RenderBuildContext {
        scene_bounds: scene_bounds_for_root(tree, root_ix),
        ..RenderBuildContext::default()
    };
    let cache_store_budget = Cell::new(if tree.has_render_refresh_damage() {
        0
    } else {
        RENDER_SUBTREE_CACHE_STORE_BUDGET
    });
    let subtree = build_element_subtree_cached(
        tree,
        root_ix,
        &FontContext::default(),
        RenderTraversal {
            scene_ctx: SceneContext::default(),
            render_ctx: &render_ctx,
            cache_store_budget: &cache_store_budget,
        },
    );

    let text_input_focused = subtree.text_input_focused;
    let text_input_cursor_area = subtree.text_input_cursor_area;

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

    let attrs = &element.layout.effective;
    let radius = attrs.border_radius.as_ref();
    let scene_state = resolve_node_state(element, traversal.scene_ctx.clone());
    let render_frame = scene_state
        .as_ref()
        .map(|state| state.adjusted_frame)
        .unwrap_or(frame);
    let transform = element_transform(render_frame, attrs);
    let alpha = attrs.alpha.unwrap_or(1.0) as f32;

    let element_context = inherited.merge_with_attrs(attrs);
    let mut local = Vec::new();

    let outer_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, false);
    local.extend(wrap_with_shadow_pass(wrap_with_clips(
        wrap_with_transform(outer_shadow_nodes, transform),
        traversal.render_ctx.shadow_clip_shapes(),
    )));

    let background_nodes = build_background_nodes(render_frame, attrs);
    let inset_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, true);
    let host_content = build_host_content_subtree(
        tree,
        element,
        ix,
        render_frame,
        &element_context,
        &mut outputs.reborrow(),
        RenderTraversal {
            scene_ctx: traversal.scene_ctx.clone(),
            render_ctx: traversal.render_ctx,
            cache_store_budget: traversal.cache_store_budget,
        },
        scene_state.clone(),
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

        local.extend(wrap_with_clips(
            wrap_with_transform(normal_nodes, transform),
            inherited_host_clips,
        ));
    }

    let escapes = wrap_with_alpha(wrap_with_transform(host_content.escapes, transform), alpha);

    RenderSubtree {
        local: wrap_with_alpha(local, alpha),
        escapes,
        text_input_focused: false,
        text_input_cursor_area: None,
    }
}

fn build_element_subtree_cached(
    tree: &mut ElementTree,
    ix: NodeIx,
    inherited: &FontContext,
    traversal: RenderTraversal<'_>,
) -> RenderSubtree {
    if render_subtree_cache_bypassed(tree, ix, &traversal) {
        return build_element_subtree_uncached_in_cached_path(tree, ix, inherited, traversal);
    }

    let Some(element) = tree.get_ix(ix).map(Element::render_snapshot) else {
        return RenderSubtree::default();
    };
    let Some(frame) = element.layout.frame else {
        return RenderSubtree::default();
    };

    let render_damage = element.refresh.render_dirty || element.refresh.render_descendant_dirty;
    let has_existing_cache = tree
        .get_ix(ix)
        .is_some_and(|element| element.refresh.render_cache.is_some());
    let lookup_key = if !render_damage && has_existing_cache {
        #[cfg(test)]
        record_render_subtree_cache_lookup_key_build();
        Some(render_subtree_key(
            tree, ix, &element, inherited, &traversal,
        ))
    } else {
        None
    };

    if let Some(key) = lookup_key.as_ref()
        && let Some(cache) = tree
            .get_ix(ix)
            .and_then(|element| element.refresh.render_cache.as_ref())
            .filter(|cache| &cache.key == key)
            .cloned()
    {
        return RenderSubtree::from_cache(cache);
    }

    let attrs = &element.layout.effective;
    let radius = attrs.border_radius.as_ref();
    let scene_state = resolve_node_state(&element, traversal.scene_ctx.clone());
    let render_frame = scene_state
        .as_ref()
        .map(|state| state.adjusted_frame)
        .unwrap_or(frame);
    let transform = element_transform(render_frame, attrs);
    let alpha = attrs.alpha.unwrap_or(1.0) as f32;

    let element_context = inherited.merge_with_attrs(attrs);
    let mut local = Vec::new();

    let outer_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, false);
    local.extend(wrap_with_shadow_pass(wrap_with_clips(
        wrap_with_transform(outer_shadow_nodes, transform),
        traversal.render_ctx.shadow_clip_shapes(),
    )));

    let background_nodes = build_background_nodes(render_frame, attrs);
    let inset_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, true);
    let host_content = build_host_content_subtree_cached(
        tree,
        &element,
        ix,
        render_frame,
        &element_context,
        RenderTraversal {
            scene_ctx: traversal.scene_ctx.clone(),
            render_ctx: traversal.render_ctx,
            cache_store_budget: traversal.cache_store_budget,
        },
        scene_state.clone(),
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

        local.extend(wrap_with_clips(
            wrap_with_transform(normal_nodes, transform),
            inherited_host_clips,
        ));
    }

    let escapes = wrap_with_alpha(wrap_with_transform(host_content.escapes, transform), alpha);

    let subtree = RenderSubtree {
        local: wrap_with_alpha(local, alpha),
        escapes,
        text_input_focused: host_content.text_input_focused,
        text_input_cursor_area: host_content.text_input_cursor_area,
    };

    if should_store_render_subtree_cache(&element, render_damage, &traversal, &subtree) {
        let key = lookup_key
            .unwrap_or_else(|| render_subtree_key(tree, ix, &element, inherited, &traversal));
        if let Some(element) = tree.get_ix_mut(ix) {
            element.refresh.render_cache = Some(subtree.to_cache(key));
        }
    } else if let Some(element) = tree.get_ix_mut(ix) {
        element.refresh.render_cache = None;
    }

    subtree
}

fn build_element_subtree_uncached_in_cached_path(
    tree: &ElementTree,
    ix: NodeIx,
    inherited: &FontContext,
    traversal: RenderTraversal<'_>,
) -> RenderSubtree {
    let mut text_input_focused = false;
    let mut text_input_cursor_area = None;
    let mut outputs = RenderOutputs {
        text_input_focused: &mut text_input_focused,
        text_input_cursor_area: &mut text_input_cursor_area,
    };
    let mut subtree = build_element_subtree(tree, ix, inherited, &mut outputs, traversal);
    subtree.merge_output_values(text_input_focused, text_input_cursor_area);
    subtree
}

fn render_subtree_cache_bypassed(
    tree: &ElementTree,
    ix: NodeIx,
    traversal: &RenderTraversal<'_>,
) -> bool {
    tree.get_ix(ix)
        .is_some_and(|element| element.refresh.render_dirty)
        || scene_context_has_scroll_offset(&traversal.scene_ctx)
}

fn should_store_render_subtree_cache(
    element: &Element,
    render_damage: bool,
    traversal: &RenderTraversal<'_>,
    subtree: &RenderSubtree,
) -> bool {
    if render_damage
        || scene_context_has_scroll_offset(&traversal.scene_ctx)
        || is_scroll_container(element)
    {
        return false;
    }

    let remaining = traversal.cache_store_budget.get();
    if remaining == 0 || subtree.render_node_count() > RENDER_SUBTREE_CACHE_MAX_RENDER_NODES {
        return false;
    }

    traversal.cache_store_budget.set(remaining - 1);
    true
}

fn scene_context_has_scroll_offset(scene_ctx: &SceneContext) -> bool {
    scene_ctx.scroll_dx != 0.0 || scene_ctx.scroll_dy != 0.0
}

fn is_scroll_container(element: &Element) -> bool {
    element.layout.scroll_x != 0.0
        || element.layout.scroll_y != 0.0
        || element.layout.scroll_x_max > 0.0
        || element.layout.scroll_y_max > 0.0
        || effective_scrollbar_x(&element.layout.effective)
        || effective_scrollbar_y(&element.layout.effective)
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
            RenderNode::Primitive(_) => 1,
        })
        .sum()
}

fn render_subtree_key(
    tree: &ElementTree,
    ix: NodeIx,
    element: &Element,
    inherited: &FontContext,
    traversal: &RenderTraversal<'_>,
) -> RenderSubtreeKey {
    RenderSubtreeKey {
        kind: element.spec.kind,
        attrs_hash: render_attrs_hash(&element.layout.effective),
        runtime_hash: debug_hash(&element.runtime),
        frame: element.layout.frame,
        scroll_x: element.layout.scroll_x,
        scroll_y: element.layout.scroll_y,
        scroll_x_max: element.layout.scroll_x_max,
        scroll_y_max: element.layout.scroll_y_max,
        inherited_hash: debug_hash(inherited),
        scene_context_hash: debug_hash(&traversal.scene_ctx),
        render_context_hash: debug_hash(traversal.render_ctx),
        topology: tree.render_topology_dependency_key_ix(ix),
        paragraph_fragments_hash: debug_hash(&element.layout.paragraph_fragments),
    }
}

struct HashWriter<'a> {
    hasher: &'a mut DefaultHasher,
}

impl Write for HashWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        s.hash(self.hasher);
        Ok(())
    }
}

fn debug_hash(value: &impl fmt::Debug) -> u64 {
    let mut hasher = DefaultHasher::new();
    write!(
        &mut HashWriter {
            hasher: &mut hasher
        },
        "{value:?}"
    )
    .expect("hash writer should not fail");
    hasher.finish()
}

fn render_attrs_hash(attrs: &Attrs) -> u64 {
    let mut hasher = DefaultHasher::new();
    debug_hash_into(&mut hasher, &attrs.width);
    debug_hash_into(&mut hasher, &attrs.height);
    debug_hash_into(&mut hasher, &attrs.padding);
    debug_hash_into(&mut hasher, &attrs.spacing);
    debug_hash_into(&mut hasher, &attrs.spacing_x);
    debug_hash_into(&mut hasher, &attrs.spacing_y);
    debug_hash_into(&mut hasher, &attrs.align_x);
    debug_hash_into(&mut hasher, &attrs.align_y);
    debug_hash_into(&mut hasher, &attrs.scrollbar_y);
    debug_hash_into(&mut hasher, &attrs.scrollbar_x);
    debug_hash_into(&mut hasher, &attrs.ghost_scrollbar_y);
    debug_hash_into(&mut hasher, &attrs.ghost_scrollbar_x);
    debug_hash_into(&mut hasher, &attrs.scroll_x);
    debug_hash_into(&mut hasher, &attrs.scroll_y);
    debug_hash_into(&mut hasher, &attrs.clip_nearby);
    debug_hash_into(&mut hasher, &attrs.border_width);
    debug_hash_into(&mut hasher, &attrs.background);
    debug_hash_into(&mut hasher, &attrs.border_radius);
    debug_hash_into(&mut hasher, &attrs.border_style);
    debug_hash_into(&mut hasher, &attrs.border_color);
    debug_hash_into(&mut hasher, &attrs.box_shadows);
    debug_hash_into(&mut hasher, &attrs.font);
    debug_hash_into(&mut hasher, &attrs.font_weight);
    debug_hash_into(&mut hasher, &attrs.font_style);
    debug_hash_into(&mut hasher, &attrs.font_size);
    debug_hash_into(&mut hasher, &attrs.font_color);
    debug_hash_into(&mut hasher, &attrs.svg_color);
    debug_hash_into(&mut hasher, &attrs.font_underline);
    debug_hash_into(&mut hasher, &attrs.font_strike);
    debug_hash_into(&mut hasher, &attrs.font_letter_spacing);
    debug_hash_into(&mut hasher, &attrs.font_word_spacing);
    debug_hash_into(&mut hasher, &attrs.image_src);
    debug_hash_into(&mut hasher, &attrs.image_fit);
    debug_hash_into(&mut hasher, &attrs.image_size);
    debug_hash_into(&mut hasher, &attrs.text_align);
    debug_hash_into(&mut hasher, &attrs.content);
    debug_hash_into(&mut hasher, &attrs.snap_layout);
    debug_hash_into(&mut hasher, &attrs.snap_text_metrics);
    debug_hash_into(&mut hasher, &attrs.space_evenly);
    debug_hash_into(&mut hasher, &attrs.move_x);
    debug_hash_into(&mut hasher, &attrs.move_y);
    debug_hash_into(&mut hasher, &attrs.rotate);
    debug_hash_into(&mut hasher, &attrs.scale);
    debug_hash_into(&mut hasher, &attrs.alpha);
    debug_hash_into(&mut hasher, &attrs.video_target);
    hasher.finish()
}

fn debug_hash_into(hasher: &mut DefaultHasher, value: &impl fmt::Debug) {
    write!(HashWriter { hasher }, "{value:?}").expect("hash writer should not fail");
}

fn build_host_content_subtree_cached(
    tree: &mut ElementTree,
    element: &Element,
    element_ix: NodeIx,
    render_frame: Frame,
    element_context: &FontContext,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> RenderSubtree {
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
            let branch_subtree = build_nearby_mount_subtree_cached(
                tree,
                mount.ix,
                mount.slot,
                element_context,
                RenderTraversal {
                    scene_ctx: traversal.scene_ctx.clone(),
                    render_ctx: &child_render_ctx,
                    cache_store_budget: traversal.cache_store_budget,
                },
                scene_state.clone(),
            );
            subtree.extend_local(branch_subtree);
        }
    } else {
        let mut branches = Vec::new();
        element.for_each_retained_local_branch(tree, |branch| branches.push(branch));
        let child_scene_ctx = scene_state
            .clone()
            .map(|resolved| {
                next_scene_context(resolved, super::element::RetainedPaintPhase::Children)
            })
            .unwrap_or_default();

        for branch in branches {
            match branch {
                RetainedLocalBranchRef::Nearby(mount) => {
                    let branch_subtree = build_nearby_mount_subtree_cached(
                        tree,
                        mount.ix,
                        mount.slot,
                        element_context,
                        RenderTraversal {
                            scene_ctx: traversal.scene_ctx.clone(),
                            render_ctx: &child_render_ctx,
                            cache_store_budget: traversal.cache_store_budget,
                        },
                        scene_state.clone(),
                    );
                    subtree.extend_local(branch_subtree);
                }
                RetainedLocalBranchRef::Child(child) => {
                    let branch_subtree = build_element_subtree_cached(
                        tree,
                        child.ix,
                        element_context,
                        RenderTraversal {
                            scene_ctx: child_scene_ctx.clone(),
                            render_ctx: &child_render_ctx,
                            cache_store_budget: traversal.cache_store_budget,
                        },
                    );
                    subtree.extend_local(branch_subtree);
                }
            }
        }
    }

    let mut own_text_input_focused = false;
    let mut own_text_input_cursor_area = None;
    subtree.local.extend(wrap_own_content_nodes(
        build_own_content_nodes(
            element,
            render_frame,
            attrs,
            element_context,
            &mut own_text_input_focused,
            &mut own_text_input_cursor_area,
        ),
        attrs,
        element.spec.kind,
        current_host_clip.clip,
    ));
    subtree.merge_output_values(own_text_input_focused, own_text_input_cursor_area);

    if element.spec.kind == ElementKind::Paragraph {
        let paragraph_subtree = build_paragraph_subtree_cached(
            tree,
            element,
            element_context,
            RenderTraversal {
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx,
                cache_store_budget: traversal.cache_store_budget,
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
        subtree.extend_escape(build_nearby_mount_subtree_cached(
            tree,
            mount.ix,
            mount.slot,
            element_context,
            RenderTraversal {
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx.without_host_clips(),
                cache_store_budget: traversal.cache_store_budget,
            },
            scene_state.clone(),
        ));
    }

    subtree
}

fn build_nearby_mount_subtree_cached(
    tree: &mut ElementTree,
    nearby_ix: NodeIx,
    slot: NearbySlot,
    element_context: &FontContext,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> RenderSubtree {
    build_element_subtree_cached(
        tree,
        nearby_ix,
        element_context,
        RenderTraversal {
            scene_ctx: scene_state
                .map(|state| next_scene_context(state, slot.spec().phase))
                .unwrap_or_default(),
            render_ctx: traversal.render_ctx,
            cache_store_budget: traversal.cache_store_budget,
        },
    )
}

fn build_paragraph_subtree_cached(
    tree: &mut ElementTree,
    element: &Element,
    element_context: &FontContext,
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
    let mut children = Vec::new();
    element.for_each_retained_child(tree, |child| children.push(child));
    for child in children {
        match child.mode {
            RetainedChildMode::Scope => {
                let child_subtree = build_element_subtree_cached(
                    tree,
                    child.ix,
                    element_context,
                    RenderTraversal {
                        scene_ctx: child_scene_ctx.clone(),
                        render_ctx: traversal.render_ctx,
                        cache_store_budget: traversal.cache_store_budget,
                    },
                );
                subtree.extend_local(child_subtree);
            }
            RetainedChildMode::InlineEventOnly => {}
        }
    }

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

fn build_host_content_subtree(
    tree: &ElementTree,
    element: &Element,
    element_ix: NodeIx,
    render_frame: Frame,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> RenderSubtree {
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
                    cache_store_budget: traversal.cache_store_budget,
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
                        cache_store_budget: traversal.cache_store_budget,
                    },
                    scene_state.clone(),
                );
                subtree.extend_local(branch_subtree);
            }
            RetainedLocalBranchRef::Child(child) => {
                let branch_subtree = build_element_subtree(
                    tree,
                    child.ix,
                    element_context,
                    &mut outputs.reborrow(),
                    RenderTraversal {
                        scene_ctx: scene_state
                            .clone()
                            .map(|state| {
                                next_scene_context(
                                    state,
                                    super::element::RetainedPaintPhase::Children,
                                )
                            })
                            .unwrap_or_default(),
                        render_ctx: &child_render_ctx,
                        cache_store_budget: traversal.cache_store_budget,
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
                cache_store_budget: traversal.cache_store_budget,
            },
            scene_state.clone(),
            current_host_clip.clip,
        );
        subtree.local.extend(paragraph_subtree.local);
        subtree.escapes.extend(paragraph_subtree.escapes);
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
                cache_store_budget: traversal.cache_store_budget,
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
    build_element_subtree(
        tree,
        nearby_ix,
        element_context,
        &mut outputs.reborrow(),
        RenderTraversal {
            scene_ctx: scene_state
                .map(|state| next_scene_context(state, slot.spec().phase))
                .unwrap_or_default(),
            render_ctx: traversal.render_ctx,
            cache_store_budget: traversal.cache_store_budget,
        },
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
            let child_subtree = build_element_subtree(
                tree,
                child.ix,
                element_context,
                &mut outputs.reborrow(),
                RenderTraversal {
                    scene_ctx: child_scene_ctx.clone(),
                    render_ctx: traversal.render_ctx,
                    cache_store_budget: traversal.cache_store_budget,
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

    vec![RenderNode::Clip {
        clips,
        children: nodes,
    }]
}

fn wrap_with_relaxed_clips(nodes: Vec<RenderNode>, clips: Vec<ClipShape>) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    if clips.is_empty() {
        return nodes;
    }

    vec![RenderNode::RelaxedClip {
        clips,
        children: nodes,
    }]
}

fn wrap_with_shadow_pass(nodes: Vec<RenderNode>) -> Vec<RenderNode> {
    if nodes.is_empty() {
        return nodes;
    }

    vec![RenderNode::ShadowPass { children: nodes }]
}

fn wrap_with_host_clip(nodes: Vec<RenderNode>, host_clip: ClipShape) -> Vec<RenderNode> {
    wrap_with_clips(nodes, vec![host_clip])
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
