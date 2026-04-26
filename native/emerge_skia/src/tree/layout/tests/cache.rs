use super::super::*;
use super::common::*;
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec};
use crate::tree::attrs::Background;
use crate::tree::invalidation::TreeInvalidation;
use crate::tree::patch::{Patch, apply_patches};
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

    assert!(stats.subtree_measure_dirty_bypasses > 0);
    assert!(stats.subtree_measure_hits > 0);
    assert!(stats.resolve_dirty_bypasses > 0);
    assert!(stats.resolve_hits > 0);
}

#[test]
fn test_layout_cache_stats_report_resolve_ineligible_nodes() {
    let mut tree = ElementTree::new();
    tree.set_layout_cache_stats_enabled(true);
    let paragraph = make_element("paragraph", ElementKind::Paragraph, Attrs::default());
    let paragraph_id = paragraph.id;

    tree.set_root_id(paragraph_id);
    tree.insert(paragraph);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();

    assert_eq!(stats.resolve_stores, 0);
    assert!(stats.resolve_ineligible_bypasses > 0);
    assert!(stats.resolve_store_bypasses > 0);
}

#[test]
fn test_layout_cache_stats_report_animation_bypasses() {
    let mut attrs = Attrs::default();
    let mut start_attrs = Attrs::default();
    let mut end_attrs = Attrs::default();
    start_attrs.move_x = Some(0.0);
    end_attrs.move_x = Some(10.0);
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
    assert!(stats.subtree_measure_animation_bypasses > 0);
    assert!(stats.resolve_animation_bypasses > 0);
    assert_eq!(stats.subtree_measure_stores, 0);
    assert_eq!(stats.resolve_stores, 0);
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
fn test_unsupported_kind_does_not_store_resolve_cache() {
    let mut tree = ElementTree::new();
    let paragraph = make_element("paragraph", ElementKind::Paragraph, Attrs::default());
    let paragraph_id = paragraph.id;

    tree.set_root_id(paragraph_id);
    tree.insert(paragraph);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert!(
        tree.get(&paragraph_id)
            .unwrap()
            .layout
            .resolve_cache
            .is_none()
    );
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

fn raw_text_background_attrs(content: &str) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    data.extend_from_slice(&[12, 0, 1, 255, 0, 0, 255]);
    data
}

fn raw_text_event_attrs(content: &str) -> Vec<u8> {
    let mut data = vec![0, 3];
    push_content_attr(&mut data, content);
    push_font_size_attr(&mut data, 16.0);
    data.extend_from_slice(&[40, 1]);
    data
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

fn push_align_x_attr(data: &mut Vec<u8>, align_x: AlignX) {
    data.push(5);
    data.push(match align_x {
        AlignX::Left => 0,
        AlignX::Center => 1,
        AlignX::Right => 2,
    });
}
