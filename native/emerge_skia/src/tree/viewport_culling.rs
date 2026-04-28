//! Shared retained-subtree viewport culling helpers.
//!
//! These helpers classify whole retained subtrees against the effective scene
//! clip before render or pointer-registry traversal descends into them. They
//! are intentionally conservative: nearby-mounted subtrees and retained
//! keyboard/text-input state keep participating even when their pointer-visible
//! paint is outside the current viewport.

use super::attrs::Attrs;
use super::element::{Element, ElementTree, Frame, NodeIx};
use super::geometry::Rect;
use super::scene::{SceneContext, resolve_node_state};
use super::transform::{Affine2, element_transform};

pub(crate) fn should_skip_render_viewport_subtree(
    tree: &ElementTree,
    ix: NodeIx,
    scene_ctx: &SceneContext,
) -> bool {
    let Some(element) = tree.get_ix(ix) else {
        return true;
    };

    should_skip_element_viewport_subtree(tree, ix, element, scene_ctx)
}

pub(crate) fn should_skip_registry_viewport_subtree(
    tree: &ElementTree,
    ix: NodeIx,
    scene_ctx: &SceneContext,
) -> bool {
    let Some(element) = tree.get_ix(ix) else {
        return true;
    };

    should_skip_element_viewport_subtree(tree, ix, element, scene_ctx)
        && !subtree_has_retained_registry_exception(tree, ix)
}

pub(crate) fn should_skip_element_viewport_subtree(
    tree: &ElementTree,
    ix: NodeIx,
    element: &Element,
    scene_ctx: &SceneContext,
) -> bool {
    let Some(state) = resolve_node_state(element, scene_ctx.clone()) else {
        return true;
    };
    let attrs = &element.layout.effective;
    let transform = element_transform(state.adjusted_frame, attrs);

    should_skip_resolved_viewport_subtree(
        tree,
        ix,
        attrs,
        state.adjusted_frame,
        transform,
        scene_ctx,
    )
}

pub(crate) fn should_skip_resolved_viewport_subtree(
    tree: &ElementTree,
    ix: NodeIx,
    attrs: &Attrs,
    frame: Frame,
    transform: Affine2,
    scene_ctx: &SceneContext,
) -> bool {
    let inherited_clip = if scene_ctx.front_nearby_subtree {
        scene_ctx.nearby_visible_clip
    } else {
        scene_ctx.visible_clip
    };
    let Some(clip) = inherited_clip else {
        return false;
    };

    let visual_bounds = transform.map_rect_aabb(element_visual_bounds(frame, attrs));
    visual_bounds.intersect(clip.rect).is_none() && !tree.has_nearby_mounts_ix(ix)
}

pub(crate) fn element_visual_bounds(frame: Frame, attrs: &Attrs) -> Rect {
    let rect = Rect::from_frame(frame);
    attrs
        .box_shadows
        .as_deref()
        .into_iter()
        .flatten()
        .filter(|shadow| !shadow.inset)
        .fold(rect, |bounds, shadow| {
            let offset_x = shadow.offset_x as f32;
            let offset_y = shadow.offset_y as f32;
            let blur = shadow.blur.abs() as f32;
            let spread = shadow.size.abs() as f32;
            let pad = blur * 2.0 + spread;
            union_rect(
                bounds,
                Rect {
                    x: rect.x + offset_x - pad,
                    y: rect.y + offset_y - pad,
                    width: rect.width + pad * 2.0,
                    height: rect.height + pad * 2.0,
                },
            )
        })
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

fn subtree_has_retained_registry_exception(tree: &ElementTree, ix: NodeIx) -> bool {
    let Some(element) = tree.get_ix(ix) else {
        return false;
    };

    element_has_retained_registry_exception(element)
        || tree
            .child_ixs(ix)
            .into_iter()
            .any(|child_ix| subtree_has_retained_registry_exception(tree, child_ix))
        || tree
            .nearby_ixs(ix)
            .into_iter()
            .any(|mount| subtree_has_retained_registry_exception(tree, mount.ix))
}

fn element_has_retained_registry_exception(element: &Element) -> bool {
    let attrs = &element.layout.effective;
    element.spec.kind.is_text_input_family()
        || element.runtime.text_input_focused
        || element.runtime.focused_active
        || element.runtime.mouse_down_active
        || element.runtime.scrollbar_hover_axis.is_some()
        || attrs.on_focus.unwrap_or(false)
        || attrs.on_blur.unwrap_or(false)
        || attrs.focus_on_mount.unwrap_or(false)
        || attrs.on_key_down.is_some()
        || attrs.on_key_up.is_some()
        || attrs.on_key_press.is_some()
        || attrs.virtual_key.is_some()
}
