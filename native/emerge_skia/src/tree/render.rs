//! Render an ElementTree into DrawCmds.
//!
//! Reads from pre-scaled attrs (scaling is applied in the layout pass).

mod box_model;
mod color;
mod paint;
mod scope;
mod text;

use self::paint::{
    build_background_items, collect_border_items, collect_box_shadow_items,
    collect_scrollbar_items, render_image_items, render_video_items,
};
pub(super) use self::scope::{
    HostClipDescriptor, InheritedClipMode, RenderItem, RenderScope, ScopeTransform,
};
use self::text::{
    TextDecorationSpec, render_text_input_items, render_text_items, text_decoration_items,
};
use super::attrs::{Attrs, BorderRadius};
use super::element::{
    Element, ElementId, ElementKind, ElementTree, Frame, NearbySlot, RetainedChildMode,
};
use super::layout::FontContext;
use super::render_lower::lower_render_scope;
use super::scene::{SceneContext, child_context as next_scene_context, resolve_node_state};
use crate::events::{RegistryRebuildPayload, registry_builder};
use crate::renderer::{DrawCmd, make_font_with_style};

pub(crate) struct RenderOutput {
    pub commands: Vec<DrawCmd>,
    pub event_rebuild: RegistryRebuildPayload,
    pub text_input_focused: bool,
    pub text_input_cursor_area: Option<(f32, f32, f32, f32)>,
}

/// Render the tree and collect rebuild metadata.
/// Reads from pre-scaled attrs (layout pass must run first).
pub(crate) fn render_tree(tree: &ElementTree) -> RenderOutput {
    let Some(root) = tree.root.as_ref() else {
        return RenderOutput {
            commands: Vec::new(),
            event_rebuild: RegistryRebuildPayload::default(),
            text_input_focused: false,
            text_input_cursor_area: None,
        };
    };

    let mut text_input_focused = false;
    let mut text_input_cursor_area = None;
    let mut rebuild_acc = registry_builder::RegistryBuildAcc::default();
    let root_scope = match build_element_scope(
        tree,
        root,
        &FontContext::default(),
        &mut text_input_focused,
        &mut text_input_cursor_area,
        Some(&mut rebuild_acc),
        &[],
        true,
        SceneContext::default(),
    ) {
        Some(scope) => scope,
        None => RenderScope::default(),
    };
    let commands = lower_render_scope(&root_scope, scene_bounds_for_root(tree, root));

    RenderOutput {
        commands,
        event_rebuild: registry_builder::finalize_registry_rebuild(rebuild_acc),
        text_input_focused,
        text_input_cursor_area,
    }
}

fn build_element_scope(
    tree: &ElementTree,
    id: &ElementId,
    inherited: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
    mut event_acc: Option<&mut registry_builder::RegistryBuildAcc>,
    scroll_contexts: &[registry_builder::ScrollContext],
    collect_events: bool,
    scene_ctx: SceneContext,
) -> Option<RenderScope> {
    let element = tree.get(id)?;
    let frame = element.frame?;
    let attrs = &element.attrs;
    let radius = attrs.border_radius.as_ref();
    let scene_state = resolve_node_state(element, scene_ctx);
    let render_frame = scene_state
        .map(|state| state.adjusted_frame)
        .unwrap_or(frame);

    let element_context = inherited.merge_with_attrs(attrs);
    let next_scroll_contexts = if collect_events {
        if let Some(acc) = event_acc.as_deref_mut() {
            registry_builder::accumulate_element_rebuild(
                acc,
                element,
                scene_state.as_ref(),
                scroll_contexts,
            )
        } else {
            scroll_contexts.to_vec()
        }
    } else {
        scroll_contexts.to_vec()
    };

    let mut scope = RenderScope {
        transform: scope_transform(render_frame, attrs),
        ..RenderScope::default()
    };

    if let Some(shadow_scope) = build_outer_shadow_scope(render_frame, attrs, radius) {
        scope.items.push(RenderItem::Scope(shadow_scope));
    }

    scope
        .items
        .extend(build_background_items(render_frame, attrs, radius));
    scope
        .items
        .extend(collect_box_shadow_items(render_frame, attrs, radius, true));

    if let Some(content_scope) = build_host_content_scope(
        tree,
        element,
        render_frame,
        &element_context,
        text_input_focused,
        text_input_cursor_area,
        event_acc.as_deref_mut(),
        &next_scroll_contexts,
        collect_events,
        scene_state,
    ) {
        scope.items.push(RenderItem::Scope(content_scope));
    }

    scope
        .items
        .extend(collect_border_items(render_frame, attrs));

    if let Some(front_scope) = build_front_nearby_scope(
        tree,
        element,
        &element_context,
        text_input_focused,
        text_input_cursor_area,
        event_acc.as_deref_mut(),
        scroll_contexts,
        collect_events,
        scene_state,
    ) {
        scope.items.push(RenderItem::Scope(front_scope));
    }

    Some(scope)
}

fn build_outer_shadow_scope(
    frame: Frame,
    attrs: &Attrs,
    radius: Option<&BorderRadius>,
) -> Option<RenderScope> {
    if !has_outer_shadows(attrs) {
        return None;
    }

    let items = collect_box_shadow_items(frame, attrs, radius, false);
    Some(RenderScope {
        inherited_clip_mode: InheritedClipMode::ShadowAxes,
        items,
        ..RenderScope::default()
    })
}

fn build_host_content_scope(
    tree: &ElementTree,
    element: &Element,
    render_frame: Frame,
    element_context: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
    mut event_acc: Option<&mut registry_builder::RegistryBuildAcc>,
    scroll_contexts: &[registry_builder::ScrollContext],
    collect_events: bool,
    scene_state: Option<super::scene::ResolvedNodeState>,
) -> Option<RenderScope> {
    let attrs = &element.attrs;
    let mut items = Vec::new();

    if let Some(behind_scope) = build_nearby_scope(
        tree,
        element,
        NearbySlot::BehindContent,
        element_context,
        text_input_focused,
        text_input_cursor_area,
        event_acc.as_deref_mut(),
        scroll_contexts,
        collect_events,
        scene_state,
    ) {
        items.push(RenderItem::Scope(behind_scope));
    }

    items.extend(build_own_content_items(
        element,
        render_frame,
        attrs,
        element_context,
        text_input_focused,
        text_input_cursor_area,
    ));

    if element.kind == ElementKind::Paragraph {
        items.extend(build_paragraph_items(
            tree,
            element,
            element_context,
            text_input_focused,
            text_input_cursor_area,
            event_acc.as_deref_mut(),
            scroll_contexts,
            collect_events,
            scene_state,
        ));
    } else if let Some(children_scope) = build_children_scope(
        tree,
        element,
        element_context,
        text_input_focused,
        text_input_cursor_area,
        event_acc.as_deref_mut(),
        scroll_contexts,
        collect_events,
        scene_state,
    ) {
        items.push(RenderItem::Scope(children_scope));
    }

    items.extend(collect_scrollbar_items(
        scene_state.as_ref(),
        render_frame,
        attrs,
    ));

    if items.is_empty() {
        return None;
    }

    let host_clip = scene_state
        .map(|state| state.host_clip)
        .unwrap_or_else(|| super::geometry::host_clip_shape(render_frame, attrs));

    Some(RenderScope {
        host_clip: Some(HostClipDescriptor {
            clip: host_clip,
            scroll_x: attrs.scrollbar_x.unwrap_or(false),
            scroll_y: attrs.scrollbar_y.unwrap_or(false),
        }),
        items,
        ..RenderScope::default()
    })
}

fn build_front_nearby_scope(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
    mut event_acc: Option<&mut registry_builder::RegistryBuildAcc>,
    scroll_contexts: &[registry_builder::ScrollContext],
    collect_events: bool,
    scene_state: Option<super::scene::ResolvedNodeState>,
) -> Option<RenderScope> {
    let mut items = Vec::new();
    for slot in NearbySlot::OVERLAY_PAINT_ORDER {
        if let Some(scope) = build_nearby_scope(
            tree,
            element,
            slot,
            element_context,
            text_input_focused,
            text_input_cursor_area,
            event_acc.as_deref_mut(),
            scroll_contexts,
            collect_events,
            scene_state,
        ) {
            items.push(RenderItem::Scope(scope));
        }
    }

    (!items.is_empty()).then_some(RenderScope {
        inherited_clip_mode: InheritedClipMode::None,
        items,
        ..RenderScope::default()
    })
}

fn build_nearby_scope(
    tree: &ElementTree,
    element: &Element,
    slot: NearbySlot,
    element_context: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
    mut event_acc: Option<&mut registry_builder::RegistryBuildAcc>,
    scroll_contexts: &[registry_builder::ScrollContext],
    collect_events: bool,
    scene_state: Option<super::scene::ResolvedNodeState>,
) -> Option<RenderScope> {
    let nearby_id = element.nearby.get(slot)?;
    build_element_scope(
        tree,
        nearby_id,
        element_context,
        text_input_focused,
        text_input_cursor_area,
        event_acc.as_deref_mut(),
        scroll_contexts,
        collect_events,
        scene_state
            .map(|state| next_scene_context(state, slot.spec().phase))
            .unwrap_or_default(),
    )
}

fn build_children_scope(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
    mut event_acc: Option<&mut registry_builder::RegistryBuildAcc>,
    scroll_contexts: &[registry_builder::ScrollContext],
    collect_events: bool,
    scene_state: Option<super::scene::ResolvedNodeState>,
) -> Option<RenderScope> {
    if element.children.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    for child_id in &element.children {
        if let Some(scope) = build_element_scope(
            tree,
            child_id,
            element_context,
            text_input_focused,
            text_input_cursor_area,
            event_acc.as_deref_mut(),
            scroll_contexts,
            collect_events,
            scene_state
                .map(|state| {
                    next_scene_context(state, super::element::RetainedPaintPhase::Children)
                })
                .unwrap_or_default(),
        ) {
            items.push(RenderItem::Scope(scope));
        }
    }

    Some(RenderScope {
        items,
        ..RenderScope::default()
    })
}

fn build_paragraph_items(
    tree: &ElementTree,
    element: &Element,
    element_context: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
    mut event_acc: Option<&mut registry_builder::RegistryBuildAcc>,
    scroll_contexts: &[registry_builder::ScrollContext],
    collect_events: bool,
    scene_state: Option<super::scene::ResolvedNodeState>,
) -> Vec<RenderItem> {
    let mut items = Vec::new();
    let child_scene_ctx = paragraph_children_scene_context(scene_state);
    let fragment_offset = scene_state
        .map(|state| {
            (
                state.adjusted_frame.x - state.frame.x,
                state.adjusted_frame.y - state.frame.y,
            )
        })
        .unwrap_or_default();

    element.for_each_retained_child(tree, |child| match child.mode {
        RetainedChildMode::Scope => {
            if let Some(scope) = build_element_scope(
                tree,
                child.id,
                element_context,
                text_input_focused,
                text_input_cursor_area,
                event_acc.as_deref_mut(),
                scroll_contexts,
                collect_events,
                child_scene_ctx,
            ) {
                items.push(RenderItem::Scope(scope));
            }
        }
        RetainedChildMode::InlineEventOnly => {
            if collect_events && let Some(acc) = event_acc.as_deref_mut() {
                registry_builder::accumulate_subtree_rebuild(
                    tree,
                    child.id,
                    acc,
                    scroll_contexts,
                    child_scene_ctx,
                );
            }
        }
    });

    let mut fragment_items = Vec::new();
    if let Some(fragments) = &element.attrs.paragraph_fragments {
        for frag in fragments {
            let x = frag.x + fragment_offset.0;
            let baseline_y = frag.y + fragment_offset.1 + frag.ascent;
            fragment_items.push(RenderItem::Draw(DrawCmd::TextWithFont(
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
                fragment_items.extend(text_decoration_items(TextDecorationSpec {
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
    items.extend(fragment_items);

    items
}

fn paragraph_children_scene_context(
    scene_state: Option<super::scene::ResolvedNodeState>,
) -> SceneContext {
    scene_state
        .map(|state| next_scene_context(state, super::element::RetainedPaintPhase::Children))
        .unwrap_or_default()
}

fn build_own_content_items(
    element: &Element,
    frame: Frame,
    attrs: &Attrs,
    inherited: &FontContext,
    text_input_focused: &mut bool,
    text_input_cursor_area: &mut Option<(f32, f32, f32, f32)>,
) -> Vec<RenderItem> {
    let mut items = Vec::new();

    match element.kind {
        ElementKind::Text => items.extend(render_text_items(frame, attrs, inherited)),
        ElementKind::TextInput => {
            if attrs.text_input_focused.unwrap_or(false) {
                *text_input_focused = true;
            }

            if text_input_cursor_area.is_none() {
                *text_input_cursor_area =
                    render_text_input_items(&mut items, frame, attrs, inherited);
            } else {
                let _ = render_text_input_items(&mut items, frame, attrs, inherited);
            }
        }
        ElementKind::Image => items.extend(render_image_items(frame, attrs)),
        ElementKind::Video => items.extend(render_video_items(frame, attrs)),
        _ => {}
    }

    items
}

fn scope_transform(frame: Frame, attrs: &super::attrs::Attrs) -> Option<ScopeTransform> {
    let move_x = attrs.move_x.unwrap_or(0.0) as f32;
    let move_y = attrs.move_y.unwrap_or(0.0) as f32;
    let rotate = attrs.rotate.unwrap_or(0.0) as f32;
    let scale = attrs.scale.unwrap_or(1.0) as f32;
    let alpha = attrs.alpha.unwrap_or(1.0) as f32;

    let has_translation = move_x != 0.0 || move_y != 0.0;
    let has_rotation = rotate != 0.0;
    let has_scale = (scale - 1.0).abs() > f32::EPSILON;
    let has_alpha = alpha < 1.0;

    (has_translation || has_rotation || has_scale || has_alpha).then_some(ScopeTransform {
        move_x,
        move_y,
        rotate,
        scale,
        center_x: frame.x + frame.width / 2.0,
        center_y: frame.y + frame.height / 2.0,
        alpha,
    })
}

fn has_outer_shadows(attrs: &Attrs) -> bool {
    attrs
        .box_shadows
        .as_ref()
        .is_some_and(|shadows| shadows.iter().any(|shadow| !shadow.inset))
}

fn scene_bounds_for_root(tree: &ElementTree, root: &ElementId) -> super::geometry::Rect {
    tree.get(root)
        .and_then(|element| element.frame)
        .map(super::geometry::Rect::from_frame)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
