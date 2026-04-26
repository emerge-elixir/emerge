use super::super::*;
use super::common::*;
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec};
use crate::tree::attrs::{Background, BoxShadow};
use crate::tree::invalidation::{
    RefreshAvailability, RefreshDecision, TreeInvalidation, decide_refresh_action,
};
use crate::tree::patch::{Patch, apply_patches};
use crate::tree::render::render_tree_scene;
use std::time::{Duration, Instant};

#[test]
fn test_leaf_text_measurement_cache_reuses_repeated_layout() {
    let mut tree = ElementTree::new();
    let text = make_element("text", ElementKind::Text, text_attrs("Hello"));
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(text_id);
    tree.insert(text);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    let first_frame = tree.get(&text_id).unwrap().layout.measured_frame.unwrap();
    assert!(first_calls > 0);
    assert!(
        tree.get(&text_id)
            .unwrap()
            .layout
            .intrinsic_measure_cache
            .is_some()
    );

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
    assert_eq!(
        tree.get(&text_id)
            .unwrap()
            .layout
            .measured_frame
            .unwrap()
            .width,
        first_frame.width
    );
    assert_eq!(
        tree.get(&text_id)
            .unwrap()
            .layout
            .measured_frame
            .unwrap()
            .height,
        first_frame.height
    );
}

#[test]
fn test_paint_only_attr_change_reuses_leaf_measurement_cache() {
    let mut tree = ElementTree::new();
    let text = make_element("text", ElementKind::Text, text_attrs("Hello"));
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(text_id);
    tree.insert(text);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();

    tree.get_mut(&text_id).unwrap().spec.declared.background =
        Some(Background::Color(Color::Named("red".to_string())));

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
}

#[test]
fn test_text_content_and_font_size_changes_miss_leaf_measurement_cache() {
    let mut tree = ElementTree::new();
    let text = make_element("text", ElementKind::Text, text_attrs("Hi"));
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(text_id);
    tree.insert(text);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    assert_eq!(
        tree.get(&text_id).unwrap().layout.frame.unwrap().width,
        16.0
    );

    tree.get_mut(&text_id).unwrap().spec.declared.content = Some("Hello".to_string());

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let second_calls = measurer.total_calls();
    assert!(second_calls > first_calls);
    assert_eq!(
        tree.get(&text_id).unwrap().layout.frame.unwrap().width,
        40.0
    );

    tree.get_mut(&text_id).unwrap().spec.declared.font_size = Some(20.0);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    assert!(measurer.total_calls() > second_calls);
    assert_eq!(
        tree.get(&text_id).unwrap().layout.frame.unwrap().height,
        20.0
    );
}

#[test]
fn test_image_size_change_misses_leaf_measurement_cache() {
    let mut tree = ElementTree::new();
    let mut attrs = Attrs::default();
    attrs.image_size = Some((10.0, 20.0));
    let image = make_element("image", ElementKind::Image, attrs);
    let image_id = image.id;

    tree.set_root_id(image_id);
    tree.insert(image);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let first_key = tree
        .get(&image_id)
        .unwrap()
        .layout
        .intrinsic_measure_cache
        .as_ref()
        .unwrap()
        .key
        .clone();
    assert_eq!(
        tree.get(&image_id).unwrap().layout.frame.unwrap().width,
        10.0
    );
    assert_eq!(
        tree.get(&image_id).unwrap().layout.frame.unwrap().height,
        20.0
    );

    tree.get_mut(&image_id).unwrap().spec.declared.image_size = Some((30.0, 40.0));

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let second_key = tree
        .get(&image_id)
        .unwrap()
        .layout
        .intrinsic_measure_cache
        .as_ref()
        .unwrap()
        .key
        .clone();

    assert_ne!(second_key, first_key);
    assert_eq!(
        tree.get(&image_id).unwrap().layout.frame.unwrap().width,
        30.0
    );
    assert_eq!(
        tree.get(&image_id).unwrap().layout.frame.unwrap().height,
        40.0
    );
}

#[test]
fn test_leaf_measurement_cache_survives_keyed_reorder() {
    let mut tree = ElementTree::new();
    let row = make_element("row", ElementKind::Row, Attrs::default());
    let row_id = row.id;
    let first = make_element("first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(row_id);
    tree.insert(row);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&row_id, vec![first_id, second_id])
        .unwrap();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    assert!(first_calls > 0);

    tree.set_children(&row_id, vec![second_id, first_id])
        .unwrap();
    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
    assert!(
        tree.get(&first_id)
            .unwrap()
            .layout
            .intrinsic_measure_cache
            .is_some()
    );
    assert!(
        tree.get(&second_id)
            .unwrap()
            .layout
            .intrinsic_measure_cache
            .is_some()
    );
}

#[test]
fn test_subtree_measurement_cache_skips_clean_descendants() {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Column, Attrs::default());
    let root_id = root.id;
    let text = make_element("text", ElementKind::Text, text_attrs("Hello"));
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    assert!(first_calls > 0);
    assert!(
        tree.get(&root_id)
            .unwrap()
            .layout
            .subtree_measure_cache
            .is_some()
    );

    tree.get_mut(&text_id)
        .unwrap()
        .layout
        .intrinsic_measure_cache = None;
    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
}

#[test]
fn test_paint_only_patch_keeps_subtree_measurement_cache_hot() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];
    let measurer = CountingTextMeasurer::default();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    tree.get_mut(&text_id)
        .unwrap()
        .layout
        .intrinsic_measure_cache = None;

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_background_attrs("Hello"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
}

#[test]
fn test_event_only_patch_keeps_subtree_measurement_cache_hot() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];
    let measurer = CountingTextMeasurer::default();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    tree.get_mut(&text_id)
        .unwrap()
        .layout
        .intrinsic_measure_cache = None;

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_event_attrs("Hello"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Registry);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
}

#[test]
fn test_text_patch_dirties_changed_path_only() {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Row, Attrs::default());
    let root_id = root.id;
    let first = make_element("first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&root_id, vec![first_id, second_id])
        .unwrap();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: first_id,
            attrs_raw: raw_text_attrs("One!"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);
    assert!(tree.get(&first_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&second_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&root_id).unwrap().layout.measure_dirty);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls + 2);
}

#[test]
fn test_parent_font_change_invalidates_inherited_text_measurement() {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Column, Attrs::default());
    let root_id = root.id;
    let mut child_attrs = Attrs::default();
    child_attrs.content = Some("Hello".to_string());
    let text = make_element("text", ElementKind::Text, child_attrs);
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_font_size_attrs(20.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls + 2);
    assert_eq!(
        tree.get(&text_id)
            .unwrap()
            .layout
            .measured_frame
            .unwrap()
            .height,
        20.0
    );
}

#[test]
fn test_subtree_cache_survives_keyed_reorder_without_remeasuring_leaves() {
    let mut tree = ElementTree::new();
    let row = make_element("row", ElementKind::Row, Attrs::default());
    let row_id = row.id;
    let first = make_element("first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(row_id);
    tree.insert(row);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&row_id, vec![first_id, second_id])
        .unwrap();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();

    tree.set_children(&row_id, vec![second_id, first_id])
        .unwrap();
    assert!(tree.get(&row_id).unwrap().layout.measure_dirty);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);

    assert_eq!(measurer.total_calls(), first_calls);
}

#[test]
fn test_scale_change_misses_subtree_measurement_cache() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];
    let measurer = CountingTextMeasurer::default();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 2.0, &measurer);

    assert!(measurer.total_calls() > first_calls);
    assert_eq!(
        tree.get(&text_id)
            .unwrap()
            .layout
            .measured_frame
            .unwrap()
            .height,
        32.0
    );
}

#[test]
fn test_resolve_cache_stores_for_simple_subtree() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    for id in [root_id, text_id] {
        let layout = &tree.get(&id).unwrap().layout;
        assert!(layout.resolve_cache.is_some());
        assert!(!layout.resolve_dirty);
    }
}

#[test]
fn test_layout_cache_stats_report_warm_cache_hits() {
    let mut tree = text_child_tree("Hello");
    tree.set_layout_cache_stats_enabled(true);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let cold_stats = tree.layout_cache_stats();
    assert_eq!(cold_stats.subtree_measure_hits, 0);
    assert_eq!(cold_stats.resolve_hits, 0);
    assert!(cold_stats.subtree_measure_stores > 0);
    assert!(cold_stats.resolve_stores > 0);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let warm_stats = tree.layout_cache_stats();

    assert!(warm_stats.subtree_measure_hits > 0);
    assert!(warm_stats.resolve_hits > 0);
    assert_eq!(warm_stats.subtree_measure_misses, 0);
    assert_eq!(warm_stats.resolve_misses, 0);
    assert_eq!(warm_stats.subtree_measure_stores, 0);
    assert_eq!(warm_stats.resolve_stores, 0);
}

#[test]
fn test_layout_cache_stats_are_disabled_by_default() {
    let mut tree = text_child_tree("Hello");

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert_eq!(
        tree.layout_cache_stats(),
        crate::stats::LayoutCacheStats::default()
    );
}

#[test]
fn test_layout_cache_stats_report_shifted_sibling_reuse() {
    let mut tree = shifted_sibling_tree(10.0);
    tree.set_layout_cache_stats_enabled(true);
    let root_id = tree.root_id().unwrap();
    let control_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: control_id,
            attrs_raw: raw_control_height_attrs(20.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();

    assert!(stats.subtree_measure_misses > 0);
    assert!(stats.subtree_measure_hits > 0);
    assert!(stats.resolve_misses > 0);
    assert!(stats.resolve_hits > 0);
}

#[test]
fn test_layout_cache_stats_report_cold_paragraph_resolve_store() {
    let mut tree = paragraph_inline_tree("Hello paragraph cache", 120.0);
    tree.set_layout_cache_stats_enabled(true);
    let paragraph_id = tree.root_id().unwrap();

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();

    assert!(stats.resolve_misses > 0);
    assert!(stats.resolve_stores > 0);
    assert!(
        tree.get(&paragraph_id)
            .unwrap()
            .layout
            .resolve_cache
            .is_some()
    );
}

#[test]
fn test_layout_cache_stats_report_layout_affecting_animation_cache_misses() {
    let mut attrs = Attrs::default();
    let mut start_attrs = Attrs::default();
    let mut end_attrs = Attrs::default();
    start_attrs.width = Some(Length::Px(10.0));
    end_attrs.width = Some(Length::Px(20.0));
    attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Once,
    });

    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);
    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    let animations_active = layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start + Duration::from_millis(1)),
    );
    let stats = tree.layout_cache_stats();

    assert!(animations_active);
    assert!(stats.subtree_measure_misses > 0);
    assert!(stats.resolve_misses > 0);
    assert!(stats.subtree_measure_stores > 0);
    assert!(stats.resolve_stores > 0);
}

#[test]
fn test_measure_affecting_animation_preserves_unrelated_sibling_cache_reuse() {
    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);

    let root = make_element("root", ElementKind::Row, Attrs::default());
    let root_id = root.id;

    let mut animated_attrs = Attrs::default();
    animated_attrs.height = Some(Length::Px(20.0));
    let mut start_attrs = Attrs::default();
    start_attrs.width = Some(Length::Px(20.0));
    let mut end_attrs = Attrs::default();
    end_attrs.width = Some(Length::Px(60.0));
    animated_attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });
    let animated = make_element("animated", ElementKind::El, animated_attrs);
    let animated_id = animated.id;

    let text = make_element("text", ElementKind::Text, text_attrs("Sibling"));
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(animated);
    tree.insert(text);
    tree.set_children(&root_id, vec![animated_id, text_id])
        .unwrap();

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    assert!(layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &measurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start),
    ));
    let first_calls = measurer.total_calls();
    assert!(first_calls > 0);

    assert!(layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &measurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start + Duration::from_millis(25)),
    ));
    let stats = tree.layout_cache_stats();

    assert_eq!(measurer.total_calls(), first_calls);
    assert!(stats.subtree_measure_misses > 0);
    assert!(stats.subtree_measure_hits > 0);
    assert!(stats.subtree_measure_stores > 0);
}

#[test]
fn test_resolve_affecting_animation_does_not_remeasure_text() {
    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(200.0));
    root_attrs.height = Some(Length::Px(60.0));
    let root = make_element("root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut text_element_attrs = text_attrs("Aligned");
    let mut start_attrs = Attrs::default();
    start_attrs.align_x = Some(AlignX::Left);
    let mut end_attrs = Attrs::default();
    end_attrs.align_x = Some(AlignX::Right);
    text_element_attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });
    let text = make_element("text", ElementKind::Text, text_element_attrs);
    let text_id = text.id;
    let measurer = CountingTextMeasurer::default();

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    assert!(layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &measurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start),
    ));
    let first_calls = measurer.total_calls();
    assert!(first_calls > 0);

    assert!(layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &measurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start + Duration::from_millis(75)),
    ));
    let stats = tree.layout_cache_stats();

    assert_eq!(measurer.total_calls(), first_calls);
    assert!(stats.subtree_measure_hits > 0);
    assert_eq!(stats.intrinsic_measure_misses, 0);
    assert!(stats.resolve_misses > 0);
    assert_eq!(
        tree.get(&text_id).unwrap().layout.effective.align_x,
        Some(AlignX::Right)
    );
}

#[test]
fn test_paint_only_shadow_animation_refresh_skips_layout_after_warm_frame() {
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(120.0));
    attrs.height = Some(Length::Px(64.0));

    let mut start_attrs = Attrs::default();
    start_attrs.box_shadows = Some(vec![test_shadow(0.0, -12.0)]);
    let mut end_attrs = Attrs::default();
    end_attrs.box_shadows = Some(vec![test_shadow(12.0, 0.0)]);
    attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });

    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);
    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    let initial = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start,
    );
    assert!(initial.layout_performed);
    let initial_frame = tree.get(&root_id).unwrap().layout.frame.unwrap();

    let update = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start + Duration::from_millis(25),
    );

    assert!(update.output.animations_active);
    assert!(!update.layout_performed);
    assert_eq!(
        tree.layout_cache_stats(),
        crate::stats::LayoutCacheStats::default()
    );
    assert_eq!(
        tree.get(&root_id).unwrap().layout.frame.unwrap(),
        initial_frame
    );
    assert_eq!(
        tree.get(&root_id)
            .unwrap()
            .layout
            .effective
            .box_shadows
            .as_ref()
            .unwrap()[0]
            .offset_x,
        3.0
    );
}

#[test]
fn test_scroll_with_paint_only_animation_refresh_skips_layout() {
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(64.0));
    attrs.scrollbar_y = Some(true);

    let mut start_attrs = Attrs::default();
    start_attrs.box_shadows = Some(vec![test_shadow(0.0, -12.0)]);
    let mut end_attrs = Attrs::default();
    end_attrs.box_shadows = Some(vec![test_shadow(12.0, 0.0)]);
    attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });

    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);
    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id;
    let mut child_attrs = Attrs::default();
    child_attrs.width = Some(Length::Px(80.0));
    child_attrs.height = Some(Length::Px(200.0));
    let child = make_element("child", ElementKind::El, child_attrs);
    let child_id = child.id;
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);
    tree.set_children(&root_id, vec![child_id]).unwrap();

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    let initial = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start,
    );
    assert!(initial.layout_performed);
    assert_eq!(tree.get(&root_id).unwrap().layout.scroll_y_max, 136.0);

    let scroll_invalidation = tree.apply_scroll_y(&root_id, -24.0);
    assert_eq!(scroll_invalidation, TreeInvalidation::Paint);

    let update = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start + Duration::from_millis(25),
    );

    assert!(update.output.animations_active);
    assert!(!update.layout_performed);
    assert_eq!(
        tree.layout_cache_stats(),
        crate::stats::LayoutCacheStats::default()
    );
    assert_eq!(tree.get(&root_id).unwrap().layout.scroll_y, 24.0);
    assert_eq!(
        tree.get(&root_id)
            .unwrap()
            .layout
            .effective
            .box_shadows
            .as_ref()
            .unwrap()[0]
            .offset_x,
        3.0
    );
}

#[test]
fn test_paint_only_shadow_patch_refresh_skips_layout() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let initial_frame = tree.get(&text_id).unwrap().layout.frame.unwrap();
    tree.set_layout_cache_stats_enabled(true);

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_shadow_attrs("Hello", 5.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);

    let preparation = prepare_frame_attrs_for_update(&mut tree, 1.0, None, None);
    let combined_invalidation = invalidation.join(preparation.animation_result.invalidation);
    assert_eq!(combined_invalidation, TreeInvalidation::Paint);
    assert_eq!(
        decide_refresh_action(
            combined_invalidation,
            false,
            RefreshAvailability {
                has_cached_rebuild: false,
                has_root_frame: prepared_root_has_frame(&tree, &preparation),
            },
        ),
        RefreshDecision::RefreshOnly
    );

    let update = refresh_prepared_default(&mut tree, preparation);

    assert!(!update.layout_performed);
    assert_eq!(
        tree.layout_cache_stats(),
        crate::stats::LayoutCacheStats::default()
    );
    assert_eq!(
        tree.get(&text_id).unwrap().layout.frame.unwrap(),
        initial_frame
    );
    assert_eq!(
        tree.get(&text_id)
            .unwrap()
            .layout
            .effective
            .box_shadows
            .as_ref()
            .unwrap()[0]
            .offset_x,
        5.0
    );
}

#[test]
fn test_render_subtree_cache_matches_uncached_scene_after_sibling_paint_patch() {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Row, Attrs::default());
    let root_id = root.id;
    let first = make_element("first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&root_id, vec![first_id, second_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let second_cache_before = tree
        .get(&second_id)
        .unwrap()
        .refresh
        .render_cache
        .clone()
        .expect("warm refresh should store second child render cache");

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: first_id,
            attrs_raw: raw_text_background_attrs("One"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);

    let output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let uncached_scene = render_tree_scene(&tree).scene;

    assert_eq!(output.scene, uncached_scene);
    assert_eq!(
        tree.get(&second_id).unwrap().refresh.render_cache.as_ref(),
        Some(&second_cache_before)
    );
}

#[test]
fn test_registry_only_root_refresh_reuses_render_cache() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let root_cache_before = tree
        .get(&root_id)
        .unwrap()
        .refresh
        .render_cache
        .clone()
        .expect("warm refresh should store root render cache");

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_event_only_attrs(),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Registry);
    assert!(!tree.has_render_refresh_damage());
    assert!(tree.has_registry_refresh_damage());

    let output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));

    assert!(output.event_rebuild_changed);
    assert_eq!(output.scene, render_tree_scene(&tree).scene);
    assert_eq!(
        tree.get(&root_id).unwrap().refresh.render_cache.as_ref(),
        Some(&root_cache_before)
    );
}

#[test]
fn test_decorative_paint_refresh_reuses_cached_registry() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    assert!(!tree.has_render_refresh_damage());
    assert!(!tree.has_registry_refresh_damage());

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_shadow_attrs("Hello", 5.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);
    assert!(tree.has_render_refresh_damage());
    assert!(!tree.has_registry_refresh_damage());

    let output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));

    assert!(!output.event_rebuild_changed);
    assert!(!tree.has_render_refresh_damage());
    assert!(!tree.has_registry_refresh_damage());
}

#[test]
fn test_transform_paint_refresh_rebuilds_registry() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    assert!(!tree.has_registry_refresh_damage());

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_move_x_attrs("Hello", 12.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);
    assert!(tree.has_render_refresh_damage());
    assert!(tree.has_registry_refresh_damage());

    let output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));

    assert!(output.event_rebuild_changed);
    assert!(!tree.has_render_refresh_damage());
    assert!(!tree.has_registry_refresh_damage());
}

#[test]
fn test_paint_only_patch_and_paint_only_animation_refresh_skip_layout() {
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(120.0));
    root_attrs.height = Some(Length::Px(64.0));

    let mut start_attrs = Attrs::default();
    start_attrs.box_shadows = Some(vec![test_shadow(0.0, -12.0)]);
    let mut end_attrs = Attrs::default();
    end_attrs.box_shadows = Some(vec![test_shadow(12.0, 0.0)]);
    root_attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });

    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);
    let root = make_element("root", ElementKind::El, root_attrs);
    let root_id = root.id;
    let child = make_element("child", ElementKind::Text, text_attrs("Hello"));
    let child_id = child.id;
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);
    tree.set_children(&root_id, vec![child_id]).unwrap();

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);
    let initial = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start,
    );
    assert!(initial.layout_performed);
    let initial_child_frame = tree.get(&child_id).unwrap().layout.frame.unwrap();

    let patch_invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: child_id,
            attrs_raw: raw_text_shadow_attrs("Hello", 7.0),
        }],
    )
    .unwrap();
    assert_eq!(patch_invalidation, TreeInvalidation::Paint);

    let preparation = prepare_frame_attrs_for_update(
        &mut tree,
        1.0,
        Some(&runtime),
        Some(start + Duration::from_millis(25)),
    );
    let combined_invalidation = patch_invalidation.join(preparation.animation_result.invalidation);
    assert_eq!(combined_invalidation, TreeInvalidation::Paint);
    assert_eq!(
        decide_refresh_action(
            combined_invalidation,
            false,
            RefreshAvailability {
                has_cached_rebuild: false,
                has_root_frame: prepared_root_has_frame(&tree, &preparation),
            },
        ),
        RefreshDecision::RefreshOnly
    );

    let update = refresh_prepared_default(&mut tree, preparation);

    assert!(update.output.animations_active);
    assert!(!update.layout_performed);
    assert_eq!(
        tree.layout_cache_stats(),
        crate::stats::LayoutCacheStats::default()
    );
    assert_eq!(
        tree.get(&child_id).unwrap().layout.frame.unwrap(),
        initial_child_frame
    );
    assert_eq!(
        tree.get(&root_id)
            .unwrap()
            .layout
            .effective
            .box_shadows
            .as_ref()
            .unwrap()[0]
            .offset_x,
        3.0
    );
    assert_eq!(
        tree.get(&child_id)
            .unwrap()
            .layout
            .effective
            .box_shadows
            .as_ref()
            .unwrap()[0]
            .offset_x,
        7.0
    );
}

#[test]
fn test_layout_affecting_animation_refresh_still_runs_layout() {
    let mut attrs = Attrs::default();
    attrs.height = Some(Length::Px(64.0));

    let mut start_attrs = Attrs::default();
    start_attrs.width = Some(Length::Px(120.0));
    let mut end_attrs = Attrs::default();
    end_attrs.width = Some(Length::Px(160.0));
    attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });

    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);
    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    let initial = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start,
    );
    assert!(initial.layout_performed);

    let update = layout_or_refresh_default_with_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &runtime,
        start + Duration::from_millis(25),
    );
    let stats = tree.layout_cache_stats();

    assert!(update.output.animations_active);
    assert!(update.layout_performed);
    assert!(stats.subtree_measure_misses > 0);
    assert!(stats.resolve_misses > 0);
    assert_eq!(
        tree.get(&root_id).unwrap().layout.frame.unwrap().width,
        130.0
    );
}

#[test]
fn test_paint_only_patch_keeps_resolve_cache_hot() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_background_attrs("Hello"),
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Paint);
    assert!(!tree.get(&root_id).unwrap().layout.resolve_dirty);
    assert!(!tree.get(&text_id).unwrap().layout.resolve_dirty);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert!(tree.get(&root_id).unwrap().layout.resolve_cache.is_some());
    assert!(tree.get(&text_id).unwrap().layout.resolve_cache.is_some());
}

#[test]
fn test_event_only_patch_keeps_resolve_cache_hot() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_event_attrs("Hello"),
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Registry);
    assert!(!tree.get(&root_id).unwrap().layout.resolve_dirty);
    assert!(!tree.get(&text_id).unwrap().layout.resolve_dirty);
}

#[test]
fn test_align_patch_dirties_resolve_not_measure() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_align_attrs("Hello", AlignX::Center),
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Resolve);
    assert!(!tree.get(&text_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&text_id).unwrap().layout.resolve_dirty);
    assert!(tree.get(&root_id).unwrap().layout.resolve_dirty);
}

#[test]
fn test_text_patch_dirties_measure_and_resolve() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_attrs("Hello!"),
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Measure);
    assert!(tree.get(&text_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&text_id).unwrap().layout.resolve_dirty);
    assert!(tree.get(&root_id).unwrap().layout.resolve_dirty);
}

#[test]
fn test_text_patch_inside_fixed_size_el_stops_parent_measure_dirty_but_keeps_traversal() {
    let mut tree = fixed_el_text_tree("Hi", AlignX::Right, AlignY::Bottom);
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];
    let measurer = CountingTextMeasurer::default();

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let first_calls = measurer.total_calls();
    assert!(first_calls > 0);

    tree.set_layout_cache_stats_enabled(true);
    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_attrs("Hello!"),
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Measure);
    assert!(tree.get(&text_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&root_id).unwrap().layout.measure_descendant_dirty);
    assert!(tree.get(&root_id).unwrap().layout.resolve_dirty);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let stats = tree.layout_cache_stats();

    assert!(measurer.total_calls() > first_calls);
    assert!(stats.subtree_measure_hits > 0);
    assert_eq!(stats.subtree_measure_misses, 1);
    assert!(!tree.get(&root_id).unwrap().layout.measure_descendant_dirty);

    let text_frame = tree.get(&text_id).unwrap().layout.frame.unwrap();
    assert_eq!(text_frame.x, 52.0);
    assert_eq!(text_frame.y, 84.0);
}

#[test]
fn test_text_patch_inside_content_sized_el_still_dirties_parent_measurement() {
    let mut tree = content_el_text_tree("Hi");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: text_id,
            attrs_raw: raw_text_attrs("Hello!"),
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Measure);
    assert!(tree.get(&text_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_descendant_dirty);
}

#[test]
fn test_measure_affecting_animation_inside_fixed_size_el_reuses_parent_measure_cache() {
    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);

    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(100.0));
    root_attrs.height = Some(Length::Px(100.0));
    let root = make_element("fixed_animation_root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut child_attrs = Attrs::default();
    child_attrs.height = Some(Length::Px(20.0));
    let mut start_attrs = Attrs::default();
    start_attrs.width = Some(Length::Px(20.0));
    let mut end_attrs = Attrs::default();
    end_attrs.width = Some(Length::Px(60.0));
    child_attrs.animate = Some(AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });
    let child = make_element("fixed_animation_child", ElementKind::El, child_attrs);
    let child_id = child.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);
    tree.set_children(&root_id, vec![child_id]).unwrap();

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);

    assert!(layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start),
    ));

    assert!(layout_tree_with_context_and_animation(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
        &FontContext::default(),
        Some(&runtime),
        Some(start + Duration::from_millis(50)),
    ));
    let stats = tree.layout_cache_stats();

    assert!(stats.subtree_measure_hits > 0);
    assert_eq!(stats.subtree_measure_misses, 1);
    assert_eq!(
        tree.get(&root_id).unwrap().layout.frame.unwrap().width,
        100.0
    );
    assert_eq!(
        tree.get(&child_id).unwrap().layout.frame.unwrap().width,
        40.0
    );
}

#[test]
fn test_keyed_reorder_dirties_container_resolve_only() {
    let mut tree = ElementTree::new();
    let row = make_element("row", ElementKind::Row, Attrs::default());
    let row_id = row.id;
    let first = make_element("first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;

    tree.set_root_id(row_id);
    tree.insert(row);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&row_id, vec![first_id, second_id])
        .unwrap();

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    tree.set_children(&row_id, vec![second_id, first_id])
        .unwrap();

    assert!(tree.get(&row_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&row_id).unwrap().layout.resolve_dirty);
    assert!(!tree.get(&first_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&first_id).unwrap().layout.resolve_dirty);
    assert!(!tree.get(&second_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&second_id).unwrap().layout.resolve_dirty);
}

#[test]
fn test_paragraph_inline_text_stores_resolve_cache() {
    let mut tree = paragraph_inline_tree("Paragraph inline text stores fragments", 120.0);
    let paragraph_id = tree.root_id().unwrap();

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let paragraph = tree.get(&paragraph_id).unwrap();
    assert!(paragraph.layout.resolve_cache.is_some());
    assert!(paragraph.layout.paragraph_fragments.is_some());
}

#[test]
fn test_multiline_resolve_cache_hits_after_warm_layout() {
    let mut tree = multiline_tree("alpha beta gamma delta", 72.0, 16.0);
    tree.set_layout_cache_stats_enabled(true);
    let multiline_id = tree.root_id().unwrap();

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    assert!(
        tree.get(&multiline_id)
            .unwrap()
            .layout
            .resolve_cache
            .is_some()
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();

    assert!(stats.resolve_hits > 0);
    assert_eq!(stats.resolve_misses, 0);
    assert_eq!(stats.resolve_stores, 0);
}

#[test]
fn test_multiline_width_and_font_change_misses_and_matches_uncached_layout() {
    let mut cached = multiline_tree("alpha beta gamma delta", 120.0, 16.0);
    let multiline_id = cached.root_id().unwrap();
    cached.set_layout_cache_stats_enabled(true);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut cached,
        vec![Patch::SetAttrs {
            id: multiline_id,
            attrs_raw: raw_multiline_attrs("alpha beta gamma delta", 56.0, 20.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = cached.layout_cache_stats();
    assert!(stats.resolve_misses > 0);
    assert!(stats.resolve_stores > 0);

    let mut uncached = multiline_tree("alpha beta gamma delta", 56.0, 20.0);
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert_layout_matches(&cached, &uncached);
}

#[test]
fn test_text_column_resolve_cache_hits_with_paragraph_child() {
    let mut cached = text_column_flow_tree();
    cached.set_layout_cache_stats_enabled(true);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let root_id = cached.root_id().unwrap();
    assert!(cached.get(&root_id).unwrap().layout.resolve_cache.is_some());

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = cached.layout_cache_stats();
    assert!(stats.resolve_hits > 0);
    assert_eq!(stats.resolve_misses, 0);

    let mut uncached = text_column_flow_tree();
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert_layout_matches(&cached, &uncached);
}

#[test]
fn test_wrapped_row_resolve_cache_hits_and_width_change_misses() {
    let mut cached = wrapped_row_tree(160.0);
    cached.set_layout_cache_stats_enabled(true);
    let root_id = cached.root_id().unwrap();

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    assert!(cached.get(&root_id).unwrap().layout.resolve_cache.is_some());

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let warm_stats = cached.layout_cache_stats();
    assert!(warm_stats.resolve_hits > 0);
    assert_eq!(warm_stats.resolve_misses, 0);

    let invalidation = apply_patches(
        &mut cached,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_wrapped_row_attrs(72.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let changed_stats = cached.layout_cache_stats();
    assert!(changed_stats.resolve_misses > 0);
    assert!(changed_stats.resolve_stores > 0);

    let mut uncached = wrapped_row_tree(72.0);
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert_layout_matches(&cached, &uncached);
}

#[test]
fn test_paragraph_resolve_cache_shifts_fragments_after_parent_alignment_change() {
    let mut cached = aligned_paragraph_tree(AlignX::Left);
    cached.set_layout_cache_stats_enabled(true);
    let root_id = cached.root_id().unwrap();
    let paragraph_id = cached.child_ids(&root_id)[0];

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let before_fragments = fragment_snapshot(&cached, &paragraph_id);
    assert!(!before_fragments.is_empty());

    let invalidation = apply_patches(
        &mut cached,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_aligned_root_attrs(AlignX::Right),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Resolve);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = cached.layout_cache_stats();
    assert!(stats.resolve_hits > 0);

    let mut uncached = aligned_paragraph_tree(AlignX::Right);
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert_layout_matches(&cached, &uncached);
    assert_ne!(before_fragments, fragment_snapshot(&cached, &paragraph_id));
}

#[test]
fn test_cached_and_uncached_frames_match_for_simple_tree() {
    let mut cached = nested_simple_tree();
    let mut uncached = cached.clone();

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    for id in cached
        .iter_node_pairs()
        .map(|(id, _)| id)
        .collect::<Vec<_>>()
    {
        assert_eq!(
            cached.get(&id).unwrap().layout.frame,
            uncached.get(&id).unwrap().layout.frame
        );
    }
}

#[test]
fn test_resolve_cache_restores_shifted_subtree_before_parent_realignment() {
    let mut cached = aligned_nested_tree(AlignX::Center);
    let root_id = cached.root_id().unwrap();

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut cached,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_aligned_root_attrs(AlignX::Right),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Resolve);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let mut uncached = aligned_nested_tree(AlignX::Right);
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    for id in cached
        .iter_node_pairs()
        .map(|(id, _)| id)
        .collect::<Vec<_>>()
    {
        assert_eq!(
            cached.get(&id).unwrap().layout.frame,
            uncached.get(&id).unwrap().layout.frame
        );
    }

    let row_id = cached.child_ids(&root_id)[0];
    let text_id = cached.child_ids(&row_id)[0];
    assert_eq!(cached.get(&text_id).unwrap().layout.frame.unwrap().x, 84.0);
}

#[test]
fn test_resolve_cache_translates_clean_sibling_after_previous_sibling_layout_change() {
    let mut cached = shifted_sibling_tree(10.0);
    let root_id = cached.root_id().unwrap();
    let control_id = cached.child_ids(&root_id)[0];

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut cached,
        vec![Patch::SetAttrs {
            id: control_id,
            attrs_raw: raw_control_height_attrs(20.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Measure);

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let mut uncached = shifted_sibling_tree(20.0);
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    for id in cached
        .iter_node_pairs()
        .map(|(id, _)| id)
        .collect::<Vec<_>>()
    {
        assert_eq!(
            cached.get(&id).unwrap().layout.frame,
            uncached.get(&id).unwrap().layout.frame
        );
    }
}

fn assert_layout_matches(left: &ElementTree, right: &ElementTree) {
    for id in left.iter_node_pairs().map(|(id, _)| id).collect::<Vec<_>>() {
        assert_eq!(
            left.get(&id).unwrap().layout.frame,
            right.get(&id).unwrap().layout.frame,
            "frame mismatch for {id:?}"
        );
        assert_eq!(
            fragment_snapshot(left, &id),
            fragment_snapshot(right, &id),
            "fragment mismatch for {id:?}"
        );
    }
}

fn fragment_snapshot(tree: &ElementTree, id: &NodeId) -> Vec<(f32, f32, String)> {
    tree.get(id)
        .and_then(|element| element.layout.paragraph_fragments.as_ref())
        .map(|fragments| {
            fragments
                .iter()
                .map(|fragment| (fragment.x, fragment.y, fragment.text.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn test_shadow(offset_x: f64, offset_y: f64) -> BoxShadow {
    BoxShadow {
        offset_x,
        offset_y,
        blur: 12.0,
        size: 2.0,
        color: Color::Rgba {
            r: 15,
            g: 23,
            b: 42,
            a: 96,
        },
        inset: false,
    }
}

fn text_child_tree(content: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Column, Attrs::default());
    let root_id = root.id;
    let text = make_element("text", ElementKind::Text, text_attrs(content));
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();
    tree
}

fn fixed_el_text_tree(content: &str, align_x: AlignX, align_y: AlignY) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(100.0));
    root_attrs.height = Some(Length::Px(100.0));
    root_attrs.align_x = Some(align_x);
    root_attrs.align_y = Some(align_y);
    let root = make_element("fixed_el_root", ElementKind::El, root_attrs);
    let root_id = root.id;
    let text = make_element("fixed_el_text", ElementKind::Text, text_attrs(content));
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();
    tree
}

fn content_el_text_tree(content: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element("content_el_root", ElementKind::El, Attrs::default());
    let root_id = root.id;
    let text = make_element("content_el_text", ElementKind::Text, text_attrs(content));
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();
    tree
}

fn multiline_tree(content: &str, width: f64, font_size: f64) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut attrs = text_attrs(content);
    attrs.width = Some(Length::Px(width));
    attrs.font_size = Some(font_size);
    let multiline = make_element("multiline", ElementKind::Multiline, attrs);
    let multiline_id = multiline.id;

    tree.set_root_id(multiline_id);
    tree.insert(multiline);
    tree
}

fn paragraph_inline_tree(content: &str, width: f64) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut paragraph_attrs = Attrs::default();
    paragraph_attrs.width = Some(Length::Px(width));
    let paragraph = make_element("paragraph", ElementKind::Paragraph, paragraph_attrs);
    let paragraph_id = paragraph.id;
    let text = make_element("paragraph_text", ElementKind::Text, text_attrs(content));
    let text_id = text.id;

    tree.set_root_id(paragraph_id);
    tree.insert(paragraph);
    tree.insert(text);
    tree.set_children(&paragraph_id, vec![text_id]).unwrap();
    tree
}

fn text_column_flow_tree() -> ElementTree {
    let mut tree = ElementTree::new();
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(128.0));
    root_attrs.spacing_y = Some(4.0);
    let root = make_element("text_column", ElementKind::TextColumn, root_attrs);
    let root_id = root.id;

    let mut paragraph_attrs = Attrs::default();
    paragraph_attrs.width = Some(Length::Content);
    let paragraph = make_element("flow_paragraph", ElementKind::Paragraph, paragraph_attrs);
    let paragraph_id = paragraph.id;
    let paragraph_text = make_element(
        "flow_paragraph_text",
        ElementKind::Text,
        text_attrs("alpha beta gamma delta"),
    );
    let paragraph_text_id = paragraph_text.id;

    let tail = make_element("flow_tail", ElementKind::Text, text_attrs("tail"));
    let tail_id = tail.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(paragraph);
    tree.insert(paragraph_text);
    tree.insert(tail);
    tree.set_children(&paragraph_id, vec![paragraph_text_id])
        .unwrap();
    tree.set_children(&root_id, vec![paragraph_id, tail_id])
        .unwrap();
    tree
}

fn wrapped_row_tree(width: f64) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(width));
    root_attrs.spacing_x = Some(4.0);
    root_attrs.spacing_y = Some(6.0);
    let root = make_element("wrapped_row", ElementKind::WrappedRow, root_attrs);
    let root_id = root.id;

    let first = make_element("wrapped_first", ElementKind::Text, text_attrs("alpha"));
    let first_id = first.id;
    let second = make_element("wrapped_second", ElementKind::Text, text_attrs("beta"));
    let second_id = second.id;
    let third = make_element("wrapped_third", ElementKind::Text, text_attrs("gamma"));
    let third_id = third.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.insert(third);
    tree.set_children(&root_id, vec![first_id, second_id, third_id])
        .unwrap();
    tree
}

fn aligned_paragraph_tree(align_x: AlignX) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(100.0));
    root_attrs.height = Some(Length::Px(100.0));
    root_attrs.align_x = Some(align_x);
    let root = make_element("aligned_paragraph_root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut paragraph_attrs = Attrs::default();
    paragraph_attrs.width = Some(Length::Px(96.0));
    let paragraph = make_element("aligned_paragraph", ElementKind::Paragraph, paragraph_attrs);
    let paragraph_id = paragraph.id;
    let text = make_element(
        "aligned_paragraph_text",
        ElementKind::Text,
        text_attrs("one two three four"),
    );
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(paragraph);
    tree.insert(text);
    tree.set_children(&paragraph_id, vec![text_id]).unwrap();
    tree.set_children(&root_id, vec![paragraph_id]).unwrap();
    tree
}

fn nested_simple_tree() -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Column, Attrs::default());
    let root_id = root.id;
    let row = make_element("row", ElementKind::Row, Attrs::default());
    let row_id = row.id;
    let first = make_element("first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(row);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&row_id, vec![first_id, second_id])
        .unwrap();
    tree.set_children(&root_id, vec![row_id]).unwrap();
    tree
}

fn aligned_nested_tree(align_x: AlignX) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut root_attrs = Attrs::default();
    root_attrs.width = Some(Length::Px(100.0));
    root_attrs.height = Some(Length::Px(100.0));
    root_attrs.align_x = Some(align_x);

    let root = make_element("root", ElementKind::El, root_attrs);
    let root_id = root.id;
    let row = make_element("row", ElementKind::Row, Attrs::default());
    let row_id = row.id;
    let text = make_element("text", ElementKind::Text, text_attrs("Hi"));
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(row);
    tree.insert(text);
    tree.set_children(&row_id, vec![text_id]).unwrap();
    tree.set_children(&root_id, vec![row_id]).unwrap();
    tree
}

fn shifted_sibling_tree(control_height: f64) -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element("root", ElementKind::Column, Attrs::default());
    let root_id = root.id;

    let mut control_attrs = Attrs::default();
    control_attrs.height = Some(Length::Px(control_height));
    let control = make_element("control", ElementKind::El, control_attrs);
    let control_id = control.id;

    let body = make_element("body", ElementKind::Column, Attrs::default());
    let body_id = body.id;
    let text = make_element("text", ElementKind::Text, text_attrs("Body"));
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(control);
    tree.insert(body);
    tree.insert(text);
    tree.set_children(&body_id, vec![text_id]).unwrap();
    tree.set_children(&root_id, vec![control_id, body_id])
        .unwrap();
    tree
}

fn raw_text_attrs(content: &str) -> Vec<u8> {
    let mut data = vec![0, 2];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    data
}

fn raw_multiline_attrs(content: &str, width: f64, font_size: f64) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_px_length_attr(&mut data, 1, width);
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, font_size);
    data
}

fn raw_wrapped_row_attrs(width: f64) -> Vec<u8> {
    let mut data = vec![0, 2];
    push_px_length_attr(&mut data, 1, width);
    push_spacing_xy_attr(&mut data, 4.0, 6.0);
    data
}

fn raw_text_background_attrs(content: &str) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    data.extend_from_slice(&[12, 0, 1, 255, 0, 0, 255]);
    data
}

fn raw_text_shadow_attrs(content: &str, offset_x: f64) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    push_box_shadow_attr(&mut data, offset_x);
    data
}

fn raw_text_move_x_attrs(content: &str, move_x: f64) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    push_move_x_attr(&mut data, move_x);
    data
}

fn raw_text_event_attrs(content: &str) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    data.extend_from_slice(&[40, 1]);
    data
}

fn raw_event_only_attrs() -> Vec<u8> {
    vec![0, 1, 40, 1]
}

fn raw_text_align_attrs(content: &str, align_x: AlignX) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    push_align_x_attr(&mut data, align_x);
    data
}

fn raw_aligned_root_attrs(align_x: AlignX) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_px_length_attr(&mut data, 1, 100.0);
    push_px_length_attr(&mut data, 2, 100.0);
    push_align_x_attr(&mut data, align_x);
    data
}

fn raw_control_height_attrs(height: f64) -> Vec<u8> {
    let mut data = vec![0, 1];
    push_px_length_attr(&mut data, 2, height);
    data
}

fn raw_font_size_attrs(size: f64) -> Vec<u8> {
    let mut data = vec![0, 1];
    push_font_size_attr(&mut data, size);
    data
}

fn push_content_attr(data: &mut Vec<u8>, content: &str) {
    data.push(21);
    data.extend_from_slice(&(content.len() as u16).to_be_bytes());
    data.extend_from_slice(content.as_bytes());
}

fn push_font_size_attr(data: &mut Vec<u8>, size: f64) {
    data.push(16);
    data.extend_from_slice(&size.to_be_bytes());
}

fn push_px_length_attr(data: &mut Vec<u8>, tag: u8, value: f64) {
    data.push(tag);
    data.push(2);
    data.extend_from_slice(&value.to_be_bytes());
}

fn push_spacing_xy_attr(data: &mut Vec<u8>, spacing_x: f64, spacing_y: f64) {
    data.push(36);
    data.extend_from_slice(&spacing_x.to_be_bytes());
    data.extend_from_slice(&spacing_y.to_be_bytes());
}

fn push_box_shadow_attr(data: &mut Vec<u8>, offset_x: f64) {
    data.push(52);
    data.push(1);
    data.extend_from_slice(&offset_x.to_be_bytes());
    data.extend_from_slice(&3.0_f64.to_be_bytes());
    data.extend_from_slice(&8.0_f64.to_be_bytes());
    data.extend_from_slice(&4.0_f64.to_be_bytes());
    data.extend_from_slice(&[2, 0, 3, b'r', b'e', b'd']);
    data.push(0);
}

fn push_move_x_attr(data: &mut Vec<u8>, move_x: f64) {
    data.push(31);
    data.extend_from_slice(&move_x.to_be_bytes());
}

fn push_align_x_attr(data: &mut Vec<u8>, align_x: AlignX) {
    data.push(5);
    data.push(match align_x {
        AlignX::Left => 0,
        AlignX::Center => 1,
        AlignX::Right => 2,
    });
}
