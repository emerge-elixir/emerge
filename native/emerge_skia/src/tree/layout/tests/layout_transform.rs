use super::super::*;
use super::common::*;
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec};
use crate::tree::attrs::Background;
use crate::tree::element::{NearbyMount, NearbySlot};
use crate::tree::invalidation::TreeInvalidation;
use crate::tree::patch::{Patch, apply_patches};
use std::time::{Duration, Instant};

fn assert_approx(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.01,
        "expected {actual} to be within 0.01 of {expected}"
    );
}

fn fixed_attrs(width: f64, height: f64) -> Attrs {
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(width));
    attrs.height = Some(Length::Px(height));
    attrs
}

fn root_shell_attrs(layout_scale: Option<f64>) -> Attrs {
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Fill);
    attrs.height = Some(Length::Fill);
    attrs.padding = Some(Padding::Uniform(12.0));
    attrs.spacing = Some(8.0);
    attrs.layout_scale = layout_scale;
    attrs
}

fn raw_root_shell_attrs(layout_scale: Option<f64>) -> Vec<u8> {
    let attr_count = if layout_scale.is_some() { 5 } else { 4 };
    let mut data = vec![0, attr_count];
    data.extend_from_slice(&[1, 0]);
    data.extend_from_slice(&[2, 0]);
    data.push(3);
    data.push(0);
    data.extend_from_slice(&12.0_f64.to_be_bytes());
    data.push(4);
    data.extend_from_slice(&8.0_f64.to_be_bytes());
    if let Some(scale) = layout_scale {
        data.push(78);
        data.extend_from_slice(&scale.to_be_bytes());
    }
    data
}

fn raw_root_shell_attrs_with_rotate(layout_rotate: Option<f64>) -> Vec<u8> {
    let attr_count = if layout_rotate.is_some() { 5 } else { 4 };
    let mut data = vec![0, attr_count];
    data.extend_from_slice(&[1, 0]);
    data.extend_from_slice(&[2, 0]);
    data.push(3);
    data.push(0);
    data.extend_from_slice(&12.0_f64.to_be_bytes());
    data.push(4);
    data.extend_from_slice(&8.0_f64.to_be_bytes());
    if let Some(rotate) = layout_rotate {
        data.push(79);
        data.extend_from_slice(&rotate.to_be_bytes());
    }
    data
}

fn scaled_shell_tree(layout_scale: Option<f64>) -> (ElementTree, NodeId) {
    let mut tree = ElementTree::new();
    let mut root = make_element(
        "scale_shell_root",
        ElementKind::Column,
        root_shell_attrs(layout_scale),
    );
    let root_id = root.id;
    let mut wrapper = make_element("scale_shell_wrapper", ElementKind::Column, Attrs::default());
    let wrapper_id = wrapper.id;

    let mut title_attrs = text_attrs("Scale shell");
    title_attrs.background = Some(Background::Color(Color::Rgba {
        r: 40,
        g: 80,
        b: 140,
        a: 255,
    }));
    let title = make_element("scale_shell_title", ElementKind::Text, title_attrs);
    let title_id = title.id;

    let mut row_attrs = Attrs::default();
    row_attrs.spacing = Some(4.0);
    let mut row = make_element("scale_shell_row", ElementKind::Row, row_attrs);
    let row_id = row.id;

    let first = make_element(
        "scale_shell_first",
        ElementKind::El,
        fixed_attrs(80.0, 24.0),
    );
    let first_id = first.id;
    let second = make_element(
        "scale_shell_second",
        ElementKind::El,
        fixed_attrs(64.0, 24.0),
    );
    let second_id = second.id;

    root.children = vec![wrapper_id];
    wrapper.children = vec![title_id, row_id];
    row.children = vec![first_id, second_id];

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(wrapper);
    tree.insert(title);
    tree.insert(row);
    tree.insert(first);
    tree.insert(second);
    (tree, root_id)
}

#[test]
fn local_layout_scale_matches_global_scale_for_basic_attrs() {
    let build_tree = |layout_scale: Option<f64>| {
        let mut tree = ElementTree::new();
        let mut attrs = fixed_attrs(100.0, 50.0);
        attrs.layout_scale = layout_scale;
        attrs.padding = Some(Padding::Uniform(8.0));
        attrs.border_width = Some(BorderWidth::Uniform(2.0));
        attrs.font_size = Some(16.0);

        let root = make_element("root", ElementKind::El, attrs);
        let root_id = root.id;
        tree.set_root_id(root_id);
        tree.insert(root);
        (tree, root_id)
    };

    let (mut local_tree, local_root_id) = build_tree(Some(2.0));
    layout_tree_default(&mut local_tree, Constraint::new(800.0, 600.0), 1.0);

    let (mut global_tree, global_root_id) = build_tree(None);
    layout_tree_default(&mut global_tree, Constraint::new(800.0, 600.0), 2.0);

    let local = local_tree.get(&local_root_id).unwrap();
    let global = global_tree.get(&global_root_id).unwrap();

    assert_eq!(local.layout.frame, global.layout.frame);
    assert_eq!(local.layout.effective.width, global.layout.effective.width);
    assert_eq!(
        local.layout.effective.height,
        global.layout.effective.height
    );
    assert_eq!(
        local.layout.effective.padding,
        global.layout.effective.padding
    );
    assert_eq!(
        local.layout.effective.border_width,
        global.layout.effective.border_width
    );
    assert_eq!(
        local.layout.effective.font_size,
        global.layout.effective.font_size
    );
}

#[test]
fn root_layout_scale_patch_matches_fresh_scaled_layout_and_render_scene() {
    let (mut patched, root_id) = scaled_shell_tree(None);
    layout_and_refresh_default(&mut patched, Constraint::new(480.0, 320.0), 1.0);

    let invalidation = apply_patches(
        &mut patched,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_root_shell_attrs(Some(1.25)),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    let preparation = prepare_frame_attrs_for_update(&mut patched, 1.0, None, None);
    let patched_output = layout_and_refresh_prepared_default(
        &mut patched,
        Constraint::new(480.0, 320.0),
        preparation,
    )
    .output;

    let (mut fresh, _fresh_root_id) = scaled_shell_tree(Some(1.25));
    let fresh_output = layout_and_refresh_default(&mut fresh, Constraint::new(480.0, 320.0), 1.0);

    assert_eq!(patched_output.scene.nodes, fresh_output.scene.nodes);
}

#[test]
fn root_layout_rotate_patch_matches_fresh_rotated_layout_and_render_scene() {
    let (mut patched, root_id) = scaled_shell_tree(None);
    layout_and_refresh_default(&mut patched, Constraint::new(480.0, 320.0), 1.0);

    let invalidation = apply_patches(
        &mut patched,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_root_shell_attrs_with_rotate(Some(90.0)),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    let preparation = prepare_frame_attrs_for_update(&mut patched, 1.0, None, None);
    let patched_output = layout_and_refresh_prepared_default(
        &mut patched,
        Constraint::new(480.0, 320.0),
        preparation,
    )
    .output;

    let (mut fresh, fresh_root_id) = scaled_shell_tree(None);
    fresh
        .get_mut(&fresh_root_id)
        .unwrap()
        .spec
        .declared
        .layout_rotate = Some(90.0);
    let fresh_output = layout_and_refresh_default(&mut fresh, Constraint::new(480.0, 320.0), 1.0);

    assert_eq!(patched_output.scene.nodes, fresh_output.scene.nodes);
}

#[test]
fn nested_layout_scales_multiply_down_the_subtree() {
    let mut tree = ElementTree::new();

    let mut root_attrs = Attrs::default();
    root_attrs.layout_scale = Some(2.0);
    let mut root = make_element("root", ElementKind::Column, root_attrs);

    let mut child_attrs = fixed_attrs(20.0, 10.0);
    child_attrs.layout_scale = Some(1.5);
    let child = make_element("child", ElementKind::El, child_attrs);
    let child_id = child.id;
    root.children = vec![child_id];

    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    layout_tree_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    let child = tree.get(&child_id).unwrap();
    let frame = child.layout.frame.unwrap();
    assert_eq!(child.layout.effective.width, Some(Length::Px(60.0)));
    assert_eq!(child.layout.effective.height, Some(Length::Px(30.0)));
    assert_eq!(frame.width, 60.0);
    assert_eq!(frame.height, 30.0);
}

#[test]
fn root_layout_scale_animation_scales_descendant_attrs() {
    let mut tree = ElementTree::new();

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Fill);
    root_attrs.height = Some(Length::Fill);
    root_attrs.animate = Some(layout_scale_animation_spec(1.0, 2.0));
    let mut root = make_element("animated_scale_root", ElementKind::Column, root_attrs);
    let root_id = root.id;

    let mut child_attrs = fixed_attrs(20.0, 10.0);
    child_attrs.padding = Some(Padding::Uniform(2.0));
    let child = make_element("animated_scale_child", ElementKind::El, child_attrs);
    let child_id = child.id;
    root.children = vec![child_id];

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    layout_and_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start,
    );
    let update = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start + Duration::from_millis(50),
    );

    assert!(update.layout_performed);
    let child = tree.get(&child_id).unwrap();
    assert_eq!(child.layout.effective.width, Some(Length::Px(30.0)));
    assert_eq!(child.layout.effective.padding, Some(Padding::Uniform(3.0)));
    assert_eq!(child.layout.frame.unwrap().width, 30.0);
}

#[test]
fn layout_scale_animation_scales_same_frame_pixel_keyframes() {
    let mut tree = ElementTree::new();

    let mut root = make_element(
        "animated_scale_width_root",
        ElementKind::Column,
        Attrs::default(),
    );
    let root_id = root.id;

    let mut from = Attrs::default();
    from.layout_scale = Some(1.0);
    from.width = Some(Length::Px(20.0));
    let mut to = Attrs::default();
    to.layout_scale = Some(2.0);
    to.width = Some(Length::Px(40.0));

    let mut child_attrs = fixed_attrs(20.0, 10.0);
    child_attrs.animate = Some(AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });
    let child = make_element("animated_scale_width_child", ElementKind::El, child_attrs);
    let child_id = child.id;
    root.children = vec![child_id];

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    layout_and_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start,
    );
    let update = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start + Duration::from_millis(50),
    );

    assert!(update.layout_performed);
    let child = tree.get(&child_id).unwrap();
    assert_eq!(child.layout.effective.layout_scale, Some(1.5));
    assert_eq!(child.layout.effective.width, Some(Length::Px(45.0)));
    assert_eq!(child.layout.frame.unwrap().width, 45.0);
}

#[test]
fn layout_rotate_animation_reserves_sampled_aabb() {
    let mut tree = ElementTree::new();

    let mut row = make_element("animated_rotate_row", ElementKind::Row, Attrs::default());
    let row_id = row.id;

    let mut from = Attrs::default();
    from.layout_rotate = Some(0.0);
    let mut to = Attrs::default();
    to.layout_rotate = Some(90.0);
    let mut child_attrs = fixed_attrs(100.0, 40.0);
    child_attrs.animate = Some(AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });
    let child = make_element("animated_rotate_child", ElementKind::El, child_attrs);
    let child_id = child.id;
    row.children = vec![child_id];

    tree.set_root_id(row_id);
    tree.insert(row);
    tree.insert(child);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    layout_and_refresh_default_with_animation(
        &mut tree,
        Constraint::new(300.0, 300.0),
        1.0,
        &runtime,
        start,
    );
    let update = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(300.0, 300.0),
        1.0,
        &runtime,
        start + Duration::from_millis(50),
    );

    assert!(update.layout_performed);
    let child = tree.get(&child_id).unwrap();
    let layout_frame = child.layout.frame.unwrap();
    let render_frame = child.layout.render_frame.unwrap();
    let expected = std::f32::consts::FRAC_1_SQRT_2 * 100.0 + std::f32::consts::FRAC_1_SQRT_2 * 40.0;

    assert_approx(layout_frame.width, expected);
    assert_approx(layout_frame.height, expected);
    assert_eq!(render_frame.width, 100.0);
    assert_eq!(render_frame.height, 40.0);
}

#[test]
fn root_layout_scale_nearby_exit_ghost_keeps_captured_size() {
    let mut tree = ElementTree::new();

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Fill);
    root_attrs.height = Some(Length::Fill);
    root_attrs.layout_scale = Some(1.25);
    let root = make_element("root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut menu_attrs = fixed_attrs(196.0, 80.0);
    menu_attrs.animate_exit = Some(exit_alpha_spec());
    let menu = make_element("menu", ElementKind::Column, menu_attrs);
    let menu_id = menu.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(menu);
    tree.set_nearby_mounts(
        &root_id,
        vec![NearbyMount {
            slot: NearbySlot::InFront,
            id: menu_id,
        }],
    )
    .unwrap();

    layout_tree_default(&mut tree, Constraint::new(480.0, 320.0), 1.0);
    let live_frame = tree.get(&menu_id).unwrap().layout.frame.unwrap();
    assert_approx(live_frame.width, 245.0);

    apply_patches(&mut tree, vec![Patch::Remove { id: menu_id }]).unwrap();
    let ghost_id = tree.nearby_mounts_for(&root_id)[0].id;
    assert!(tree.get(&ghost_id).unwrap().is_ghost_root());

    layout_tree_default(&mut tree, Constraint::new(480.0, 320.0), 1.0);
    let ghost_frame = tree.get(&ghost_id).unwrap().layout.frame.unwrap();

    assert_approx(ghost_frame.width, live_frame.width);
    assert_approx(ghost_frame.height, live_frame.height);
}

#[test]
fn root_layout_scale_nearby_exit_ghost_keeps_size_when_subtree_scale_pass_runs() {
    let mut tree = ElementTree::new();

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Fill);
    root_attrs.height = Some(Length::Fill);
    root_attrs.layout_scale = Some(1.25);
    let mut root = make_element("root_with_scaled_child", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut child_attrs = fixed_attrs(20.0, 20.0);
    child_attrs.layout_scale = Some(1.1);
    let child = make_element("scaled_child", ElementKind::El, child_attrs);
    let child_id = child.id;
    root.children = vec![child_id];

    let mut menu_attrs = fixed_attrs(196.0, 80.0);
    menu_attrs.animate_exit = Some(exit_alpha_spec());
    let menu = make_element("menu_with_scaled_sibling", ElementKind::Column, menu_attrs);
    let menu_id = menu.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);
    tree.insert(menu);
    tree.set_nearby_mounts(
        &root_id,
        vec![NearbyMount {
            slot: NearbySlot::InFront,
            id: menu_id,
        }],
    )
    .unwrap();

    layout_tree_default(&mut tree, Constraint::new(480.0, 320.0), 1.0);
    let live_frame = tree.get(&menu_id).unwrap().layout.frame.unwrap();
    assert_approx(live_frame.width, 245.0);

    apply_patches(&mut tree, vec![Patch::Remove { id: menu_id }]).unwrap();
    let ghost_id = tree.nearby_mounts_for(&root_id)[0].id;
    assert!(tree.get(&ghost_id).unwrap().is_ghost_root());

    layout_tree_default(&mut tree, Constraint::new(480.0, 320.0), 1.0);
    let ghost_frame = tree.get(&ghost_id).unwrap().layout.frame.unwrap();

    assert_approx(ghost_frame.width, live_frame.width);
    assert_approx(ghost_frame.height, live_frame.height);
}

#[test]
fn non_root_quarter_rotate_reserves_aabb_and_keeps_render_frame_centered() {
    let mut tree = ElementTree::new();

    let mut row = make_element("row", ElementKind::Row, Attrs::default());
    let mut rotated_attrs = fixed_attrs(100.0, 40.0);
    rotated_attrs.layout_rotate = Some(90.0);
    let rotated = make_element("rotated", ElementKind::El, rotated_attrs);
    let rotated_id = rotated.id;

    let sibling = make_element("sibling", ElementKind::El, fixed_attrs(20.0, 10.0));
    let sibling_id = sibling.id;
    row.children = vec![rotated_id, sibling_id];

    let row_id = row.id;
    tree.set_root_id(row_id);
    tree.insert(row);
    tree.insert(rotated);
    tree.insert(sibling);

    layout_tree_default(&mut tree, Constraint::new(300.0, 200.0), 1.0);

    let rotated = tree.get(&rotated_id).unwrap();
    let layout_frame = rotated.layout.frame.unwrap();
    let render_frame = rotated.layout.render_frame.unwrap();
    let sibling_frame = tree.get(&sibling_id).unwrap().layout.frame.unwrap();

    assert_eq!(layout_frame.width, 40.0);
    assert_eq!(layout_frame.height, 100.0);
    assert_eq!(render_frame.width, 100.0);
    assert_eq!(render_frame.height, 40.0);
    assert_eq!(render_frame.x, -30.0);
    assert_eq!(render_frame.y, 30.0);
    assert_eq!(sibling_frame.x, 40.0);
}

fn exit_alpha_spec() -> AnimationSpec {
    let mut from = Attrs::default();
    from.alpha = Some(1.0);

    let mut to = Attrs::default();
    to.alpha = Some(0.1);

    AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    }
}

fn layout_scale_animation_spec(from_scale: f64, to_scale: f64) -> AnimationSpec {
    let mut from = Attrs::default();
    from.layout_scale = Some(from_scale);

    let mut to = Attrs::default();
    to.layout_scale = Some(to_scale);

    AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    }
}

#[test]
fn arbitrary_rotate_uses_aabb_reservation() {
    let mut tree = ElementTree::new();
    let mut attrs = fixed_attrs(100.0, 40.0);
    attrs.layout_rotate = Some(45.0);

    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);

    layout_tree_default(&mut tree, Constraint::new(300.0, 300.0), 1.0);

    let root = tree.get(&root_id).unwrap();
    let layout_frame = root.layout.frame.unwrap();
    let render_frame = root.layout.render_frame.unwrap();
    let expected = std::f32::consts::FRAC_1_SQRT_2 * 100.0 + std::f32::consts::FRAC_1_SQRT_2 * 40.0;

    assert_approx(layout_frame.width, expected);
    assert_approx(layout_frame.height, expected);
    assert_eq!(render_frame.width, 100.0);
    assert_eq!(render_frame.height, 40.0);
}

#[test]
fn root_quarter_rotate_swaps_logical_constraints_and_fits_physical_viewport() {
    let mut tree = ElementTree::new();
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Fill);
    attrs.height = Some(Length::Fill);
    attrs.layout_rotate = Some(90.0);

    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);

    layout_tree_default(&mut tree, Constraint::new(300.0, 500.0), 1.0);

    let root = tree.get(&root_id).unwrap();
    let layout_frame = root.layout.frame.unwrap();
    let render_frame = root.layout.render_frame.unwrap();

    assert_eq!(layout_frame.width, 300.0);
    assert_eq!(layout_frame.height, 500.0);
    assert_eq!(render_frame.width, 500.0);
    assert_eq!(render_frame.height, 300.0);
}

#[test]
fn root_quarter_rotate_places_nearby_inside_logical_render_frame() {
    let mut tree = ElementTree::new();
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Fill);
    attrs.height = Some(Length::Fill);
    attrs.clip_nearby = Some(true);
    attrs.layout_rotate = Some(90.0);

    let root = make_element("rotated_root_with_nearby", ElementKind::El, attrs);
    let root_id = root.id;
    let menu = make_element(
        "rotated_root_nearby_menu",
        ElementKind::El,
        fixed_attrs(40.0, 40.0),
    );
    let menu_id = menu.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(menu);
    tree.set_nearby_mounts(
        &root_id,
        vec![NearbyMount {
            slot: NearbySlot::InFront,
            id: menu_id,
        }],
    )
    .unwrap();

    layout_tree_default(&mut tree, Constraint::new(480.0, 320.0), 1.0);

    let root_render_frame = tree.get(&root_id).unwrap().layout.render_frame.unwrap();
    let menu_frame = tree.get(&menu_id).unwrap().layout.frame.unwrap();

    assert_eq!(root_render_frame.x, 80.0);
    assert_eq!(root_render_frame.y, -80.0);
    assert_eq!(root_render_frame.width, 320.0);
    assert_eq!(root_render_frame.height, 480.0);
    assert_eq!(menu_frame.x, root_render_frame.x);
    assert_eq!(menu_frame.y, root_render_frame.y);
}
