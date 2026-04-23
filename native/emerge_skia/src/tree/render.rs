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
use super::transform::element_transform;
use crate::events::{RegistryRebuildPayload, registry_builder};
use crate::render_scene::{DrawPrimitive, RenderNode, RenderScene};
use crate::renderer::{make_font_with_style, measure_text_visual_metrics_with_font};

pub(crate) struct RenderOutput {
    pub scene: RenderScene,
    pub event_rebuild: RegistryRebuildPayload,
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
}

#[derive(Default)]
struct RenderSubtree {
    local: Vec<RenderNode>,
    escapes: Vec<RenderNode>,
}

impl RenderSubtree {
    fn extend_local(&mut self, subtree: RenderSubtree) {
        self.local.extend(subtree.local);
        self.escapes.extend(subtree.escapes);
    }

    fn extend_escape(&mut self, subtree: RenderSubtree) {
        self.escapes.extend(subtree.into_nodes());
    }

    fn into_nodes(self) -> Vec<RenderNode> {
        let mut nodes = self.local;
        nodes.extend(self.escapes);
        nodes
    }
}

/// Render the tree and collect rebuild metadata.
/// Reads from pre-scaled attrs (layout pass must run first).
pub(crate) fn render_tree(tree: &ElementTree) -> RenderOutput {
    let Some(root_ix) = tree.root_ix() else {
        return RenderOutput {
            scene: RenderScene::default(),
            event_rebuild: RegistryRebuildPayload::default(),
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
        },
    );

    RenderOutput {
        scene: RenderScene {
            nodes: subtree.into_nodes(),
        },
        event_rebuild: registry_builder::build_registry_rebuild(tree),
        text_input_focused,
        text_input_cursor_area,
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
    let Some(frame) = element.frame else {
        return RenderSubtree::default();
    };

    let attrs = &element.attrs;
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
        },
        scene_state.clone(),
    );
    let border_nodes = collect_border_nodes(render_frame, attrs);
    let inherited_host_clips = traversal.render_ctx.full_clip_shapes();
    let inherited_self_clip = traversal.render_ctx.nearest_self_clip();

    if matches!(element.kind, ElementKind::Image | ElementKind::Video) {
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
    }
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
    let attrs = &element.attrs;
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

    if element.kind == ElementKind::Paragraph {
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
        element.kind,
        current_host_clip.clip,
    ));

    if element.kind == ElementKind::Paragraph {
        let paragraph_subtree = build_paragraph_subtree(
            tree,
            element,
            element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx,
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
                },
            );
            subtree.extend_local(child_subtree);
        }
        RetainedChildMode::InlineEventOnly => {}
    });

    let mut fragment_nodes = Vec::new();
    if let Some(fragments) = &element.attrs.paragraph_fragments {
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

    match element.kind {
        ElementKind::Text => nodes.extend(render_text_items(frame, attrs, inherited)),
        ElementKind::TextInput => {
            if attrs.text_input_focused.unwrap_or(false) {
                *text_input_focused = true;
            }

            if text_input_cursor_area.is_none() {
                *text_input_cursor_area =
                    render_text_input_items(&mut nodes, frame, attrs, inherited);
            } else {
                let _ = render_text_input_items(&mut nodes, frame, attrs, inherited);
            }
        }
        ElementKind::Multiline => {
            if attrs.text_input_focused.unwrap_or(false) {
                *text_input_focused = true;
            }

            if text_input_cursor_area.is_none() {
                *text_input_cursor_area =
                    render_multiline_text_input_items(&mut nodes, frame, attrs, inherited);
            } else {
                let _ = render_multiline_text_input_items(&mut nodes, frame, attrs, inherited);
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
        .and_then(|element| element.frame)
        .map(Rect::from_frame)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
