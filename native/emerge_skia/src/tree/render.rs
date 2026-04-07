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
    TextDecorationSpec, render_text_input_items, render_text_items, text_decoration_items,
};
use super::attrs::{Attrs, effective_scrollbar_x, effective_scrollbar_y};
use super::element::{
    Element, ElementId, ElementKind, ElementTree, Frame, NearbySlot, RetainedChildMode,
};
use super::geometry::{ClipShape, Rect, host_clip_shape, self_shape as geometry_self_shape};
use super::layout::FontContext;
use super::scene::{
    ResolvedNodeState, SceneContext, child_context as next_scene_context, resolve_node_state,
};
use super::transform::element_transform;
use crate::events::{RegistryRebuildPayload, registry_builder};
use crate::render_scene::{DrawPrimitive, RenderNode, RenderScene};
use crate::renderer::make_font_with_style;

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
}

impl RenderBuildContext {
    fn with_host_clip(&self, clip: HostClipDescriptor, self_clip: ClipShape) -> Self {
        let mut inherited_host_clips = self.inherited_host_clips.clone();
        let mut inherited_self_clips = self.inherited_self_clips.clone();
        inherited_host_clips.push(clip);
        inherited_self_clips.push(self_clip);
        Self {
            scene_bounds: self.scene_bounds,
            inherited_host_clips,
            inherited_self_clips,
        }
    }

    fn without_host_clips(&self) -> Self {
        Self {
            scene_bounds: self.scene_bounds,
            inherited_host_clips: Vec::new(),
            inherited_self_clips: Vec::new(),
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
    event_acc: Option<&'a mut registry_builder::RegistryBuildAcc>,
}

impl<'a> RenderOutputs<'a> {
    fn event_acc_mut(&mut self) -> Option<&mut registry_builder::RegistryBuildAcc> {
        self.event_acc.as_deref_mut()
    }

    fn reborrow(&mut self) -> RenderOutputs<'_> {
        let event_acc = self.event_acc.as_deref_mut();
        let text_input_focused = &mut *self.text_input_focused;
        let text_input_cursor_area = &mut *self.text_input_cursor_area;

        RenderOutputs {
            text_input_focused,
            text_input_cursor_area,
            event_acc,
        }
    }
}

#[derive(Clone)]
struct RenderTraversal<'a> {
    scroll_contexts: &'a [registry_builder::ScrollContext],
    collect_events: bool,
    scene_ctx: SceneContext,
    render_ctx: &'a RenderBuildContext,
}

/// Render the tree and collect rebuild metadata.
/// Reads from pre-scaled attrs (layout pass must run first).
pub(crate) fn render_tree(tree: &ElementTree) -> RenderOutput {
    let Some(root) = tree.root.as_ref() else {
        return RenderOutput {
            scene: RenderScene::default(),
            event_rebuild: RegistryRebuildPayload::default(),
            text_input_focused: false,
            text_input_cursor_area: None,
        };
    };

    let mut text_input_focused = false;
    let mut text_input_cursor_area = None;
    let mut rebuild_acc = registry_builder::RegistryBuildAcc::for_tree(tree);
    let render_ctx = RenderBuildContext {
        scene_bounds: scene_bounds_for_root(tree, root),
        ..RenderBuildContext::default()
    };
    let mut outputs = RenderOutputs {
        text_input_focused: &mut text_input_focused,
        text_input_cursor_area: &mut text_input_cursor_area,
        event_acc: Some(&mut rebuild_acc),
    };
    let nodes = build_element_nodes(
        tree,
        root,
        &FontContext::default(),
        &mut outputs,
        RenderTraversal {
            scroll_contexts: &[],
            collect_events: true,
            scene_ctx: SceneContext::default(),
            render_ctx: &render_ctx,
        },
    );

    RenderOutput {
        scene: RenderScene { nodes },
        event_rebuild: registry_builder::finalize_registry_rebuild(rebuild_acc),
        text_input_focused,
        text_input_cursor_area,
    }
}

fn build_element_nodes(
    tree: &ElementTree,
    id: &ElementId,
    inherited: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
) -> Vec<RenderNode> {
    let Some(element) = tree.get(id) else {
        return Vec::new();
    };
    let Some(frame) = element.frame else {
        return Vec::new();
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
    let next_scroll_contexts = if traversal.collect_events {
        if let Some(acc) = outputs.event_acc_mut() {
            registry_builder::accumulate_element_rebuild(
                acc,
                element,
                scene_state.as_ref(),
                traversal.scroll_contexts,
            )
        } else {
            traversal.scroll_contexts.to_vec()
        }
    } else {
        traversal.scroll_contexts.to_vec()
    };

    let mut sections = Vec::new();

    let outer_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, false);
    sections.extend(wrap_with_shadow_pass(wrap_with_clips(
        wrap_with_transform(outer_shadow_nodes, transform),
        traversal.render_ctx.shadow_clip_shapes(),
    )));

    let background_nodes = build_background_nodes(render_frame, attrs);
    let inset_shadow_nodes = collect_box_shadow_nodes(render_frame, attrs, radius, true);
    let host_content_nodes = build_host_content_nodes(
        tree,
        element,
        render_frame,
        &element_context,
        &mut outputs.reborrow(),
        RenderTraversal {
            scroll_contexts: &next_scroll_contexts,
            collect_events: traversal.collect_events,
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

        sections.extend(wrap_with_clips(
            wrap_with_transform(decorative_nodes, transform),
            inherited_host_clips.clone(),
        ));
        sections.extend(wrap_with_relaxed_clips(
            wrap_with_transform(host_content_nodes, transform),
            content_clips,
        ));
    } else {
        let mut normal_nodes = Vec::new();
        normal_nodes.extend(background_nodes);
        normal_nodes.extend(inset_shadow_nodes);
        normal_nodes.extend(host_content_nodes);
        normal_nodes.extend(border_nodes);

        sections.extend(wrap_with_clips(
            wrap_with_transform(normal_nodes, transform),
            inherited_host_clips,
        ));
    }

    sections.extend(wrap_with_transform(
        build_front_nearby_nodes(
            tree,
            element,
            &element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scroll_contexts: traversal.scroll_contexts,
                collect_events: traversal.collect_events,
                scene_ctx: traversal.scene_ctx,
                render_ctx: &traversal.render_ctx.without_host_clips(),
            },
            scene_state,
        ),
        transform,
    ));

    wrap_with_alpha(sections, alpha)
}

fn build_host_content_nodes(
    tree: &ElementTree,
    element: &Element,
    render_frame: Frame,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> Vec<RenderNode> {
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
    );

    let mut nodes = build_nearby_nodes(
        tree,
        element,
        NearbySlot::BehindContent,
        element_context,
        &mut outputs.reborrow(),
        RenderTraversal {
            scroll_contexts: traversal.scroll_contexts,
            collect_events: traversal.collect_events,
            scene_ctx: traversal.scene_ctx.clone(),
            render_ctx: &child_render_ctx,
        },
        scene_state.clone(),
    );

    nodes.extend(wrap_own_content_nodes(
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
        nodes.extend(build_paragraph_nodes(
            tree,
            element,
            element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scroll_contexts: traversal.scroll_contexts,
                collect_events: traversal.collect_events,
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx,
            },
            scene_state.clone(),
            current_host_clip.clip,
        ));
    } else {
        nodes.extend(build_children_nodes(
            tree,
            element,
            element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scroll_contexts: traversal.scroll_contexts,
                collect_events: traversal.collect_events,
                scene_ctx: traversal.scene_ctx.clone(),
                render_ctx: &child_render_ctx,
            },
            scene_state.clone(),
        ));
    }

    nodes.extend(wrap_with_host_clip(
        collect_scrollbar_nodes(scene_state.as_ref(), render_frame, attrs),
        current_host_clip.clip,
    ));

    nodes
}

fn build_front_nearby_nodes(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> Vec<RenderNode> {
    let mut nodes = Vec::new();
    for slot in NearbySlot::OVERLAY_PAINT_ORDER {
        nodes.extend(build_nearby_nodes(
            tree,
            element,
            slot,
            element_context,
            &mut outputs.reborrow(),
            traversal.clone(),
            scene_state.clone(),
        ));
    }
    nodes
}

fn build_nearby_nodes(
    tree: &ElementTree,
    element: &Element,
    slot: NearbySlot,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> Vec<RenderNode> {
    let mut nodes = Vec::new();

    for nearby_id in element.nearby.ids(slot) {
        nodes.extend(build_element_nodes(
            tree,
            nearby_id,
            element_context,
            &mut outputs.reborrow(),
            RenderTraversal {
                scroll_contexts: traversal.scroll_contexts,
                collect_events: traversal.collect_events,
                scene_ctx: scene_state
                    .clone()
                    .map(|state| next_scene_context(state, slot.spec().phase))
                    .unwrap_or_default(),
                render_ctx: traversal.render_ctx,
            },
        ));
    }

    nodes
}

fn build_children_nodes(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
) -> Vec<RenderNode> {
    element
        .children
        .iter()
        .flat_map(|child_id| {
            build_element_nodes(
                tree,
                child_id,
                element_context,
                &mut outputs.reborrow(),
                RenderTraversal {
                    scroll_contexts: traversal.scroll_contexts,
                    collect_events: traversal.collect_events,
                    scene_ctx: scene_state
                        .clone()
                        .map(|state| {
                            next_scene_context(state, super::element::RetainedPaintPhase::Children)
                        })
                        .unwrap_or_default(),
                    render_ctx: traversal.render_ctx,
                },
            )
        })
        .collect()
}

fn build_paragraph_nodes(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    outputs: &mut RenderOutputs<'_>,
    traversal: RenderTraversal<'_>,
    scene_state: Option<ResolvedNodeState>,
    current_host_clip: ClipShape,
) -> Vec<RenderNode> {
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

    let mut nodes = Vec::new();
    element.for_each_retained_child(tree, |child| match child.mode {
        RetainedChildMode::Scope => {
            nodes.extend(build_element_nodes(
                tree,
                child.id,
                element_context,
                &mut outputs.reborrow(),
                RenderTraversal {
                    scroll_contexts: traversal.scroll_contexts,
                    collect_events: traversal.collect_events,
                    scene_ctx: child_scene_ctx.clone(),
                    render_ctx: traversal.render_ctx,
                },
            ));
        }
        RetainedChildMode::InlineEventOnly => {
            if traversal.collect_events
                && let Some(acc) = outputs.event_acc_mut()
            {
                registry_builder::accumulate_subtree_rebuild(
                    tree,
                    child.id,
                    acc,
                    traversal.scroll_contexts,
                    child_scene_ctx.clone(),
                );
            }
        }
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
                let (word_width, _) = font.measure_str(&frag.text, None);
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
    nodes.extend(wrap_with_host_clip(fragment_nodes, current_host_clip));

    nodes
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

fn scene_bounds_for_root(tree: &ElementTree, root: &ElementId) -> Rect {
    tree.get(root)
        .and_then(|element| element.frame)
        .map(Rect::from_frame)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
