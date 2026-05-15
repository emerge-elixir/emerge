use super::super::*;
use super::common::*;
use crate::events::registry_builder::{
    assert_registry_rebuild_payloads_equivalent, build_registry_rebuild,
    build_registry_rebuild_cached,
};
use crate::renderer::{RenderFrame, RenderState, RendererCacheFrameStats, SceneRenderer};
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec};
use crate::tree::attrs::{Background, BorderRadius, BoxShadow};
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
fn clean_deep_child_subtree_measure_cache_skips_descendant_layout() {
    let root_id = NodeId::from_u64(71_000);
    let depth = 64_u64;
    let text_id = NodeId::from_u64(71_000 + depth);
    let mut tree = ElementTree::new();

    tree.set_root_id(root_id);
    tree.insert(Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        fixed_box_attrs(320.0, 120.0),
    ));

    for index in 1..depth {
        let id = NodeId::from_u64(71_000 + index);
        let mut attrs = fixed_box_attrs(300.0, 80.0);
        attrs.padding = Some(Padding::Uniform(1.0));
        tree.insert(Element::with_attrs(id, ElementKind::Column, vec![], attrs));
    }

    tree.insert(Element::with_attrs(
        text_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs("deep retained text"),
    ));
    tree.set_children(&root_id, vec![NodeId::from_u64(71_001)])
        .unwrap();
    for index in 1..depth {
        tree.set_children(
            &NodeId::from_u64(71_000 + index),
            vec![NodeId::from_u64(71_000 + index + 1)],
        )
        .unwrap();
    }

    let measurer = CountingTextMeasurer::default();
    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let cold_calls = measurer.total_calls();
    assert!(cold_calls > 0);

    tree.set_layout_cache_stats_enabled(true);
    tree.reset_layout_cache_stats();
    tree.mark_measure_dirty(&root_id);

    layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &measurer);
    let stats = tree.layout_cache_stats();

    assert_eq!(
        measurer.total_calls(),
        cold_calls,
        "dirty parent layout should restore the clean child subtree cache \
         without walking down to the text leaf"
    );
    assert_eq!(
        stats.subtree_measure_hits, 1,
        "the top clean child subtree should be the retained measurement boundary"
    );
    assert_eq!(stats.intrinsic_measure_hits, 0);
}

#[test]
fn animation_refresh_reusing_registry_keeps_dynamic_layer_inside_scrolled_paint_layer() {
    let scroll_id = NodeId::from_u64(91_000);
    let content_id = NodeId::from_u64(91_001);
    let before_id = NodeId::from_u64(91_002);
    let card_id = NodeId::from_u64(91_003);
    let text_id = NodeId::from_u64(91_004);
    let after_id = NodeId::from_u64(91_005);

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);

    let mut scroll_attrs = fixed_box_attrs(320.0, 180.0);
    scroll_attrs.scrollbar_y = Some(true);
    scroll_attrs.background = Some(Background::Color(Color::Rgb {
        r: 241,
        g: 244,
        b: 250,
    }));
    tree.insert(Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        scroll_attrs,
    ));

    let content_attrs = Attrs {
        width: Some(Length::Fill),
        spacing: Some(16.0),
        ..Attrs::default()
    };
    tree.insert(Element::with_attrs(
        content_id,
        ElementKind::Column,
        Vec::new(),
        content_attrs,
    ));

    tree.insert(Element::with_attrs(
        before_id,
        ElementKind::El,
        Vec::new(),
        fixed_box_attrs(300.0, 220.0),
    ));

    let mut card_attrs = fixed_box_attrs(300.0, 94.0);
    card_attrs.padding = Some(Padding::Uniform(14.0));
    card_attrs.background = Some(Background::Color(Color::Rgb {
        r: 244,
        g: 248,
        b: 255,
    }));
    card_attrs.border_radius = Some(BorderRadius::Uniform(14.0));
    card_attrs.animate = Some(AnimationSpec {
        keyframes: vec![
            Attrs {
                box_shadows: Some(vec![BoxShadow {
                    offset_x: 0.0,
                    offset_y: -12.0,
                    blur: 18.0,
                    size: 2.0,
                    color: Color::Rgba {
                        r: 15,
                        g: 23,
                        b: 42,
                        a: 40,
                    },
                    inset: false,
                }]),
                ..Attrs::default()
            },
            Attrs {
                box_shadows: Some(vec![BoxShadow {
                    offset_x: 12.0,
                    offset_y: 0.0,
                    blur: 18.0,
                    size: 2.0,
                    color: Color::Rgba {
                        r: 15,
                        g: 23,
                        b: 42,
                        a: 40,
                    },
                    inset: false,
                }]),
                ..Attrs::default()
            },
        ],
        duration_ms: 2800.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });
    tree.insert(Element::with_attrs(
        card_id,
        ElementKind::Column,
        Vec::new(),
        card_attrs,
    ));

    tree.insert(Element::with_attrs(
        text_id,
        ElementKind::Text,
        Vec::new(),
        text_attrs("Stacked"),
    ));

    tree.insert(Element::with_attrs(
        after_id,
        ElementKind::El,
        Vec::new(),
        fixed_box_attrs(300.0, 260.0),
    ));

    tree.set_children(&scroll_id, vec![content_id]).unwrap();
    tree.set_children(&content_id, vec![before_id, card_id, after_id])
        .unwrap();
    tree.set_children(&card_id, vec![text_id]).unwrap();

    let start = Instant::now();
    let mut runtime = AnimationRuntime::default();
    runtime.sync_with_tree(&tree, start);
    let first = layout_and_refresh_default_with_animation(
        &mut tree,
        Constraint::new(320.0, 180.0),
        1.0,
        &runtime,
        start,
    );
    let cached_rebuild = first.event_rebuild.clone();

    assert_eq!(
        tree.apply_scroll_y(&scroll_id, -190.0),
        TreeInvalidation::Paint
    );
    let _ = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));

    let preparation = prepare_frame_attrs_for_update(
        &mut tree,
        1.0,
        Some(&runtime),
        Some(start + Duration::from_millis(16)),
    );
    assert!(preparation.animation_result.active);

    let update = refresh_prepared_default_reusing_clean_registry(
        &mut tree,
        preparation,
        Some(&cached_rebuild),
    );
    let summaries = paint_layer_debug_summaries(&update.output.scene.nodes);
    let scroll_container_layer = summaries
        .iter()
        .find(|(stable_id, _, _, _)| *stable_id == scroll_id.to_wire_u64())
        .unwrap_or_else(|| {
            panic!("expected scroll-container paint-layer summary, got {summaries:?}")
        });

    assert!(
        scroll_container_layer.2 > 0,
        "animated paint should be a dynamic slot inside the scrolled paint layer: {summaries:?}"
    );
}

fn paint_layer_debug_summaries(
    nodes: &[crate::render_scene::RenderNode],
) -> Vec<(u64, usize, usize, usize)> {
    fn visit(
        nodes: &[crate::render_scene::RenderNode],
        out: &mut Vec<(u64, usize, usize, usize)>,
    ) -> (usize, usize) {
        nodes
            .iter()
            .fold((0, 0), |(static_count, dynamic_count), node| {
                use crate::render_scene::RenderNode;

                match node {
                    RenderNode::ShadowPass { children }
                    | RenderNode::Clip { children, .. }
                    | RenderNode::RelaxedClip { children, .. }
                    | RenderNode::Transform { children, .. }
                    | RenderNode::Alpha { children, .. } => {
                        let (child_static, child_dynamic) = visit(children, out);
                        (
                            static_count + child_static + usize::from(child_static > 0),
                            dynamic_count + child_dynamic,
                        )
                    }
                    RenderNode::PaintLayer(layer)
                        if layer.placement == crate::render_scene::PaintLayerPlacement::Fixed
                            && (layer.policy
                                == crate::render_scene::PaintLayerPolicy::Cacheable
                                || layer.reason
                                    == crate::render_scene::PaintLayerReason::ScrollContainer) =>
                    {
                        let (child_static, child_dynamic) = visit(&layer.children, out);
                        out.push((
                            layer.stable_id,
                            child_static,
                            child_dynamic,
                            layer.children.len(),
                        ));
                        (static_count, dynamic_count + 1)
                    }
                    RenderNode::PaintLayer(_) => (static_count, dynamic_count + 1),
                    RenderNode::Primitive(_) => (static_count + 1, dynamic_count),
                }
            })
    }

    let mut out = Vec::new();
    visit(nodes, &mut out);
    out
}

#[derive(Clone)]
struct MovingPaintLayerView {
    content_generation: u64,
    bounds: crate::tree::geometry::Rect,
    children: Vec<crate::render_scene::RenderNode>,
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
    let attrs = Attrs {
        image_size: Some((10.0, 20.0)),
        ..Attrs::default()
    };
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
    let child_attrs = Attrs {
        content: Some("Hello".to_string()),
        ..Attrs::default()
    };
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

    let mut animated_attrs = fixed_height_attrs(20.0);
    let start_attrs = fixed_width_attrs(20.0);
    let end_attrs = fixed_width_attrs(60.0);
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

    let root_attrs = fixed_box_attrs(200.0, 60.0);
    let root = make_element("root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut text_element_attrs = text_attrs("Aligned");
    let start_attrs = Attrs {
        align_x: Some(AlignX::Left),
        ..Attrs::default()
    };
    let end_attrs = Attrs {
        align_x: Some(AlignX::Right),
        ..Attrs::default()
    };
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
    let mut attrs = fixed_box_attrs(120.0, 64.0);

    let start_attrs = Attrs {
        box_shadows: Some(vec![test_shadow(0.0, -12.0)]),
        ..Attrs::default()
    };
    let end_attrs = Attrs {
        box_shadows: Some(vec![test_shadow(12.0, 0.0)]),
        ..Attrs::default()
    };
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
    assert!(!render_nodes_have_moving_paint_layers(
        &update.output.scene.nodes
    ));
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
    let mut attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(64.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let start_attrs = Attrs {
        box_shadows: Some(vec![test_shadow(0.0, -12.0)]),
        ..Attrs::default()
    };
    let end_attrs = Attrs {
        box_shadows: Some(vec![test_shadow(12.0, 0.0)]),
        ..Attrs::default()
    };
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
    let child_attrs = fixed_box_attrs(80.0, 200.0);
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
fn test_paint_only_inherited_text_animation_refresh_matches_uncached_render() {
    assert_paint_only_inherited_text_animation_matches_uncached(false);
}

#[test]
fn test_paint_only_nearby_inherited_text_animation_refresh_matches_uncached_render() {
    assert_paint_only_inherited_text_animation_matches_uncached(true);
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
fn test_render_snapshot_omits_layout_cache_entries() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];

    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    let root_snapshot = {
        let root = tree.get(&root_id).unwrap();
        assert!(root.layout.subtree_measure_cache.is_some());
        assert!(root.layout.resolve_cache.is_some());
        root.render_snapshot()
    };
    assert!(root_snapshot.layout.intrinsic_measure_cache.is_none());
    assert!(root_snapshot.layout.subtree_measure_cache.is_none());
    assert!(root_snapshot.layout.resolve_cache.is_none());

    let text_snapshot = {
        let text = tree.get(&text_id).unwrap();
        assert!(text.layout.intrinsic_measure_cache.is_some());
        text.render_snapshot()
    };
    assert!(text_snapshot.layout.intrinsic_measure_cache.is_none());
    assert!(text_snapshot.layout.subtree_measure_cache.is_none());
    assert!(text_snapshot.layout.resolve_cache.is_none());
}

#[test]
fn test_scrolled_clean_child_emits_reusable_moving_paint_layer() {
    let root_attrs = Attrs {
        width: Some(Length::Px(120.0)),
        height: Some(Length::Px(80.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut tree = ElementTree::new();
    let root = make_element("scroll_layer_root", ElementKind::Column, root_attrs);
    let root_id = root.id;
    let top = make_element(
        "scroll_layer_top",
        ElementKind::El,
        fixed_box_attrs(100.0, 20.0),
    );
    let top_id = top.id;

    let mut target_attrs = fixed_box_attrs(100.0, 40.0);
    target_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    let target = make_element("scroll_layer_target", ElementKind::El, target_attrs);
    let target_id = target.id;
    let bottom = make_element(
        "scroll_layer_bottom",
        ElementKind::El,
        fixed_box_attrs(100.0, 120.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(top);
    tree.insert(target);
    tree.insert(bottom);
    tree.set_children(&root_id, vec![top_id, target_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &warmed_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .is_some(),
        "stable scroll-content children should keep paint-layer boundaries outside scroll-dirty frames"
    );

    assert_eq!(
        tree.apply_scroll_y(&root_id, -24.0),
        TreeInvalidation::Paint
    );
    assert!(tree.has_scroll_refresh_damage());
    let first_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(!tree.has_scroll_refresh_damage());
    let (first_scrolled_placement, first_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &first_scrolled_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .expect("visible clean child should emit a scroll-moving paint layer while scrolling");

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );
    let second_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (second_scrolled_placement, second_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &second_scrolled_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .expect("visible clean child should keep emitting a layer during scroll");

    assert_eq!(
        second_scrolled_layer.content_generation,
        first_scrolled_layer.content_generation
    );
    assert_eq!(second_scrolled_layer.bounds, first_scrolled_layer.bounds);
    assert!((second_scrolled_placement.tx - first_scrolled_placement.tx).abs() <= 0.001);
    assert!((second_scrolled_placement.ty - (first_scrolled_placement.ty - 16.0)).abs() <= 0.001);
}

#[test]
fn test_registry_only_patch_keeps_stable_scroll_child_payload_generation() {
    let root_attrs = Attrs {
        width: Some(Length::Px(160.0)),
        height: Some(Length::Px(90.0)),
        spacing: Some(8.0),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut tree = ElementTree::new();
    let root = make_element(
        "registry_stable_scroll_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let mut target_attrs = fixed_box_attrs(140.0, 48.0);
    target_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    let target = make_element(
        "registry_stable_scroll_target",
        ElementKind::El,
        target_attrs,
    );
    let target_id = target.id;
    let event_text = make_element(
        "registry_stable_scroll_event_text",
        ElementKind::Text,
        text_attrs("Hover target"),
    );
    let event_text_id = event_text.id;
    let bottom = make_element(
        "registry_stable_scroll_bottom",
        ElementKind::El,
        fixed_box_attrs(140.0, 120.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(target);
    tree.insert(event_text);
    tree.insert(bottom);
    tree.set_children(&root_id, vec![target_id, event_text_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (_, warmed_layer) = moving_paint_layer_with_placement_for_stable_id(
        &warmed_output.scene.nodes,
        target_id.to_wire_u64(),
    )
    .expect("stable scroll child should emit a reusable moving paint layer");

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: event_text_id,
            attrs_raw: raw_text_event_attrs("Hover target"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Registry);

    let refreshed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (_, refreshed_layer) = moving_paint_layer_with_placement_for_stable_id(
        &refreshed_output.scene.nodes,
        target_id.to_wire_u64(),
    )
    .expect("registry-only sibling patch should not remove the stable child layer");

    assert_eq!(
        refreshed_layer.content_generation,
        warmed_layer.content_generation
    );
}

#[test]
fn test_fractional_scrolled_clean_child_keeps_fractional_placement_out_of_content_key() {
    let root_attrs = Attrs {
        width: Some(Length::Px(120.0)),
        height: Some(Length::Px(80.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut tree = ElementTree::new();
    let root = make_element("fractional_scroll_root", ElementKind::Column, root_attrs);
    let root_id = root.id;
    let top = make_element(
        "fractional_scroll_top",
        ElementKind::El,
        fixed_box_attrs(100.0, 20.5),
    );
    let top_id = top.id;

    let mut target_attrs = fixed_box_attrs(100.0, 40.0);
    target_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    let target = make_element("fractional_scroll_target", ElementKind::El, target_attrs);
    let target_id = target.id;
    let bottom = make_element(
        "fractional_scroll_bottom",
        ElementKind::El,
        fixed_box_attrs(100.0, 120.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(top);
    tree.insert(target);
    tree.insert(bottom);
    tree.set_children(&root_id, vec![top_id, target_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &warmed_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .is_some(),
        "stable scroll-content children should keep paint-layer boundaries outside scroll-dirty frames"
    );

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );
    let first_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (first_scrolled_placement, first_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &first_scrolled_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .expect("fractional clean child should emit a scroll-moving paint layer on scroll");

    assert!((first_scrolled_placement.ty - 4.5).abs() <= 0.001);
    assert_eq!(first_scrolled_layer.bounds.height, 40.0);

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );
    let second_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (second_scrolled_placement, second_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &second_scrolled_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .expect("fractional clean child should keep a layer after integer scroll");

    assert_eq!(second_scrolled_layer.bounds, first_scrolled_layer.bounds);
    assert_eq!(
        second_scrolled_layer.content_generation,
        first_scrolled_layer.content_generation
    );
    assert!((second_scrolled_placement.ty - (first_scrolled_placement.ty - 16.0)).abs() <= 0.001);
}

#[test]
fn test_fractional_scroll_delta_keeps_moving_paint_layer_payload_content_key_stable() {
    let root_attrs = Attrs {
        width: Some(Length::Px(120.0)),
        height: Some(Length::Px(80.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut target_attrs = fixed_box_attrs(100.0, 40.0);
    target_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));

    let mut tree = ElementTree::new();
    let root = make_element(
        "fractional_delta_scroll_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let top = make_element(
        "fractional_delta_scroll_top",
        ElementKind::El,
        fixed_box_attrs(100.0, 20.0),
    );
    let top_id = top.id;
    let target = make_element(
        "fractional_delta_scroll_target",
        ElementKind::El,
        target_attrs,
    );
    let target_id = target.id;
    let bottom = make_element(
        "fractional_delta_scroll_bottom",
        ElementKind::El,
        fixed_box_attrs(100.0, 120.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(top);
    tree.insert(target);
    tree.insert(bottom);
    tree.set_children(&root_id, vec![top_id, target_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.25),
        TreeInvalidation::Paint
    );
    let first_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (first_scrolled_placement, first_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &first_scrolled_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .expect("fractional scroll should emit a scroll-moving paint layer");

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.25),
        TreeInvalidation::Paint
    );
    let second_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (second_scrolled_placement, second_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &second_scrolled_output.scene.nodes,
            target_id.to_wire_u64(),
        )
        .expect("fractional scroll should keep emitting a scroll-moving paint layer");

    assert_eq!(second_scrolled_layer.bounds, first_scrolled_layer.bounds);
    assert_eq!(
        second_scrolled_layer.content_generation,
        first_scrolled_layer.content_generation
    );
    assert!((second_scrolled_placement.ty - (first_scrolled_placement.ty - 16.25)).abs() <= 0.001);
}

#[test]
fn test_scrolled_clean_container_prefers_ancestor_layer_over_child_layers() {
    let root_attrs = Attrs {
        width: Some(Length::Px(180.0)),
        height: Some(Length::Px(84.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let row_attrs = Attrs {
        width: Some(Length::Px(168.0)),
        spacing: Some(8.0),
        ..Attrs::default()
    };

    let card_attrs = |width, height, r, g, b| {
        let mut attrs = fixed_box_attrs(width, height);
        attrs.background = Some(Background::Color(Color::Rgba { r, g, b, a: 255 }));
        attrs
    };

    let mut tree = ElementTree::new();
    let root = make_element(
        "scroll_ancestor_layer_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let spacer = make_element(
        "scroll_ancestor_layer_spacer",
        ElementKind::El,
        fixed_box_attrs(168.0, 20.0),
    );
    let spacer_id = spacer.id;
    let row = make_element("scroll_ancestor_layer_row", ElementKind::Row, row_attrs);
    let row_id = row.id;
    let first = make_element(
        "scroll_ancestor_layer_first",
        ElementKind::El,
        card_attrs(80.0, 44.0, 248, 250, 252),
    );
    let first_id = first.id;
    let first_text = make_element(
        "scroll_ancestor_layer_first_text",
        ElementKind::Text,
        text_attrs("Alpha"),
    );
    let first_text_id = first_text.id;
    let second = make_element(
        "scroll_ancestor_layer_second",
        ElementKind::El,
        card_attrs(80.0, 44.0, 241, 245, 249),
    );
    let second_id = second.id;
    let second_text = make_element(
        "scroll_ancestor_layer_second_text",
        ElementKind::Text,
        text_attrs("Beta"),
    );
    let second_text_id = second_text.id;
    let bottom = make_element(
        "scroll_ancestor_layer_bottom",
        ElementKind::El,
        fixed_box_attrs(168.0, 120.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(spacer);
    tree.insert(row);
    tree.insert(first);
    tree.insert(first_text);
    tree.insert(second);
    tree.insert(second_text);
    tree.insert(bottom);
    tree.set_children(&first_id, vec![first_text_id]).unwrap();
    tree.set_children(&second_id, vec![second_text_id]).unwrap();
    tree.set_children(&row_id, vec![first_id, second_id])
        .unwrap();
    tree.set_children(&root_id, vec![spacer_id, row_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &warmed_output.scene.nodes,
            row_id.to_wire_u64(),
        )
        .is_some(),
        "stable scroll-content rows should keep paint-layer boundaries outside scroll-dirty frames"
    );

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );
    let first_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (first_scrolled_placement, first_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &first_scrolled_output.scene.nodes,
            row_id.to_wire_u64(),
        )
        .expect("scrolled clean row should emit the ancestor layer boundary");
    let first_generation = first_scrolled_layer.content_generation;
    let first_bounds = first_scrolled_layer.bounds;

    assert!(
        !render_nodes_have_moving_paint_layers(&first_scrolled_layer.children),
        "clean static descendants should be owned by the ancestor payload instead of \
         split into depth-based child layers"
    );

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );
    let second_scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (second_scrolled_placement, second_scrolled_layer) =
        moving_paint_layer_with_placement_for_stable_id(
            &second_scrolled_output.scene.nodes,
            row_id.to_wire_u64(),
        )
        .expect("scrolled clean row should keep the ancestor layer boundary");

    assert_eq!(second_scrolled_layer.content_generation, first_generation);
    assert_eq!(second_scrolled_layer.bounds, first_bounds);
    assert!((second_scrolled_placement.tx - first_scrolled_placement.tx).abs() <= 0.001);
    assert!((second_scrolled_placement.ty - (first_scrolled_placement.ty - 16.0)).abs() <= 0.001);
}

#[test]
fn test_scrolled_clean_section_with_shadow_prefers_section_layer() {
    let root_attrs = Attrs {
        width: Some(Length::Px(220.0)),
        height: Some(Length::Px(110.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let section_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        spacing: Some(10.0),
        ..Attrs::default()
    };

    let mut card_attrs = fixed_box_attrs(200.0, 56.0);
    card_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    card_attrs.box_shadows = Some(vec![test_shadow(0.0, 10.0)]);

    let mut tree = ElementTree::new();
    let root = make_element(
        "scroll_shadow_section_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let spacer = make_element(
        "scroll_shadow_section_spacer",
        ElementKind::El,
        fixed_box_attrs(200.0, 18.0),
    );
    let spacer_id = spacer.id;
    let section = make_element("scroll_shadow_section", ElementKind::Column, section_attrs);
    let section_id = section.id;
    let title = make_element(
        "scroll_shadow_section_title",
        ElementKind::Text,
        text_attrs("Shadow section"),
    );
    let title_id = title.id;
    let card = make_element("scroll_shadow_section_card", ElementKind::El, card_attrs);
    let card_id = card.id;
    let card_text = make_element(
        "scroll_shadow_section_card_text",
        ElementKind::Text,
        text_attrs("Stable shadow card"),
    );
    let card_text_id = card_text.id;
    let bottom = make_element(
        "scroll_shadow_section_bottom",
        ElementKind::El,
        fixed_box_attrs(200.0, 140.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(spacer);
    tree.insert(section);
    tree.insert(title);
    tree.insert(card);
    tree.insert(card_text);
    tree.insert(bottom);
    tree.set_children(&card_id, vec![card_text_id]).unwrap();
    tree.set_children(&section_id, vec![title_id, card_id])
        .unwrap();
    tree.set_children(&root_id, vec![spacer_id, section_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    assert_eq!(
        tree.apply_scroll_y(&root_id, -12.0),
        TreeInvalidation::Paint
    );
    let scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (first_placement, section_layer) = moving_paint_layer_with_placement_for_stable_id(
        &scrolled_output.scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("scrolled clean section should emit one section-level layer");
    let first_generation = section_layer.content_generation;
    let first_bounds = section_layer.bounds;

    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &scrolled_output.scene.nodes,
            card_id.to_wire_u64(),
        )
        .is_none(),
        "clean static card content should be owned by the section layer"
    );
    assert!(
        !render_nodes_have_moving_paint_layers(&section_layer.children),
        "clean static descendants should not be split into nested paint layers"
    );

    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );
    let scrolled_again_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let (second_placement, second_section_layer) = moving_paint_layer_with_placement_for_stable_id(
        &scrolled_again_output.scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("scrolled clean section should remain a section-level layer");

    assert_eq!(second_section_layer.content_generation, first_generation);
    assert_eq!(second_section_layer.bounds, first_bounds);
    assert!((second_placement.tx - first_placement.tx).abs() <= 0.001);
    assert!((second_placement.ty - (first_placement.ty - 16.0)).abs() <= 0.001);
}

#[test]
fn test_scroll_refresh_clean_layers_stay_under_scroll_clip() {
    let root_attrs = fixed_box_attrs(280.0, 110.0);

    let scroll_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        height: Some(Length::Px(90.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut chrome_attrs = fixed_box_attrs(60.0, 90.0);
    chrome_attrs.background = Some(Background::Color(Color::Rgba {
        r: 15,
        g: 23,
        b: 42,
        a: 255,
    }));

    let section_attrs = Attrs {
        width: Some(Length::Px(180.0)),
        spacing: Some(8.0),
        ..Attrs::default()
    };

    let mut card_attrs = fixed_box_attrs(180.0, 40.0);
    card_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));

    let mut tree = ElementTree::new();
    let root = make_element("scroll_layer_scope_root", ElementKind::Row, root_attrs);
    let root_id = root.id;
    let chrome = make_element("scroll_layer_scope_chrome", ElementKind::El, chrome_attrs);
    let chrome_id = chrome.id;
    let scroll = make_element(
        "scroll_layer_scope_scroll",
        ElementKind::Column,
        scroll_attrs,
    );
    let scroll_id = scroll.id;
    let spacer = make_element(
        "scroll_layer_scope_spacer",
        ElementKind::El,
        fixed_box_attrs(180.0, 18.0),
    );
    let spacer_id = spacer.id;
    let section = make_element(
        "scroll_layer_scope_section",
        ElementKind::Column,
        section_attrs,
    );
    let section_id = section.id;
    let card = make_element("scroll_layer_scope_card", ElementKind::El, card_attrs);
    let card_id = card.id;
    let bottom = make_element(
        "scroll_layer_scope_bottom",
        ElementKind::El,
        fixed_box_attrs(180.0, 140.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(chrome);
    tree.insert(scroll);
    tree.insert(spacer);
    tree.insert(section);
    tree.insert(card);
    tree.insert(bottom);
    tree.set_children(&section_id, vec![card_id]).unwrap();
    tree.set_children(&scroll_id, vec![spacer_id, section_id, bottom_id])
        .unwrap();
    tree.set_children(&root_id, vec![chrome_id, scroll_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    assert_eq!(
        tree.apply_scroll_y(&scroll_id, -16.0),
        TreeInvalidation::Paint
    );
    let scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));

    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &scrolled_output.scene.nodes,
            chrome_id.to_wire_u64(),
        )
        .is_none(),
        "scroll-dirty refresh should not emit scroll-moving paint layers outside the scroll clip"
    );
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &scrolled_output.scene.nodes,
            section_id.to_wire_u64(),
        )
        .is_some(),
        "stable descendants inside the scroll clip should still emit scroll-moving paint layers"
    );
}

#[test]
fn test_tree_emitted_scroll_layer_records_moved_hit_after_scroll() {
    let root_attrs = Attrs {
        width: Some(Length::Px(220.0)),
        height: Some(Length::Px(100.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let section_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        spacing: Some(8.0),
        ..Attrs::default()
    };

    let mut card_attrs = fixed_box_attrs(200.0, 48.0);
    card_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    card_attrs.box_shadows = Some(vec![test_shadow(0.0, 8.0)]);

    let mut tree = ElementTree::new();
    let root = make_element("scroll_moved_hit_root", ElementKind::Column, root_attrs);
    let root_id = root.id;
    let spacer = make_element(
        "scroll_moved_hit_spacer",
        ElementKind::El,
        fixed_box_attrs(200.0, 16.0),
    );
    let spacer_id = spacer.id;
    let section = make_element(
        "scroll_moved_hit_section",
        ElementKind::Column,
        section_attrs,
    );
    let section_id = section.id;
    let card = make_element("scroll_moved_hit_card", ElementKind::El, card_attrs);
    let card_id = card.id;
    let bottom = make_element(
        "scroll_moved_hit_bottom",
        ElementKind::El,
        fixed_box_attrs(200.0, 140.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(spacer);
    tree.insert(section);
    tree.insert(card);
    tree.insert(bottom);
    tree.set_children(&section_id, vec![card_id]).unwrap();
    tree.set_children(&root_id, vec![spacer_id, section_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let mut renderer = SceneRenderer::new();

    assert_eq!(tree.apply_scroll_y(&root_id, -8.0), TreeInvalidation::Paint);
    let first_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &first_scene.nodes,
            section_id.to_wire_u64(),
        )
        .is_some()
    );
    let first_stats = render_cache_stats_for_scene(&mut renderer, first_scene, 260, 140);
    assert!(first_stats.paint_layer.stores >= 1, "{first_stats:?}");

    assert_eq!(tree.apply_scroll_y(&root_id, -8.0), TreeInvalidation::Paint);
    let second_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;
    let second_stats = render_cache_stats_for_scene(&mut renderer, second_scene, 260, 140);
    assert!(second_stats.paint_layer.hits >= 1, "{second_stats:?}");
}

#[test]
fn test_scrolled_clean_layer_strips_outer_paint_layer_clip_from_content_key() {
    let wrapper_attrs = fixed_box_attrs(260.0, 140.0);

    let root_attrs = Attrs {
        width: Some(Length::Px(220.0)),
        height: Some(Length::Px(100.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let section_attrs = fixed_width_attrs(200.0);

    let mut card_attrs = fixed_box_attrs(200.0, 48.0);
    card_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    card_attrs.box_shadows = Some(vec![test_shadow(0.0, 8.0)]);

    let mut tree = ElementTree::new();
    let wrapper = make_element(
        "scroll_container_layer_clip_wrapper",
        ElementKind::Column,
        wrapper_attrs,
    );
    let wrapper_id = wrapper.id;
    let root = make_element(
        "scroll_container_layer_clip_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let spacer = make_element(
        "scroll_container_layer_clip_spacer",
        ElementKind::El,
        fixed_box_attrs(200.0, 16.0),
    );
    let spacer_id = spacer.id;
    let section = make_element(
        "scroll_container_layer_clip_section",
        ElementKind::Column,
        section_attrs,
    );
    let section_id = section.id;
    let card = make_element(
        "scroll_container_layer_clip_card",
        ElementKind::El,
        card_attrs,
    );
    let card_id = card.id;
    let bottom = make_element(
        "scroll_container_layer_clip_bottom",
        ElementKind::El,
        fixed_box_attrs(200.0, 140.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(wrapper_id);
    tree.insert(wrapper);
    tree.insert(root);
    tree.insert(spacer);
    tree.insert(section);
    tree.insert(card);
    tree.insert(bottom);
    tree.set_children(&section_id, vec![card_id]).unwrap();
    tree.set_children(&root_id, vec![spacer_id, section_id, bottom_id])
        .unwrap();
    tree.set_children(&wrapper_id, vec![root_id]).unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let mut renderer = SceneRenderer::new();

    assert_eq!(tree.apply_scroll_y(&root_id, -8.0), TreeInvalidation::Paint);
    let first_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;
    let (first_placement, first_layer) = moving_paint_layer_with_placement_for_stable_id(
        &first_scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("first scroll should emit the clean section layer");
    let first_generation = first_layer.content_generation;
    let first_stats = render_cache_stats_for_scene(&mut renderer, first_scene, 260, 140);
    assert!(first_stats.paint_layer.stores >= 1, "{first_stats:?}");

    assert_eq!(tree.apply_scroll_y(&root_id, -8.0), TreeInvalidation::Paint);
    let second_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;
    let (second_placement, second_layer) = moving_paint_layer_with_placement_for_stable_id(
        &second_scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("second scroll should keep the same clean section layer");

    assert_eq!(second_layer.content_generation, first_generation);
    assert!((second_placement.tx - first_placement.tx).abs() <= 0.001);
    assert!((second_placement.ty - (first_placement.ty - 8.0)).abs() <= 0.001);

    let second_stats = render_cache_stats_for_scene(&mut renderer, second_scene, 260, 140);
    assert!(second_stats.paint_layer.hits >= 1, "{second_stats:?}");
}

#[test]
fn test_active_scroll_context_emits_moving_layers_after_layout_pass() {
    let root_attrs = Attrs {
        width: Some(Length::Px(220.0)),
        height: Some(Length::Px(100.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let section_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        spacing: Some(8.0),
        ..Attrs::default()
    };

    let mut card_attrs = fixed_box_attrs(200.0, 48.0);
    card_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));

    let mut tree = ElementTree::new();
    let root = make_element(
        "layout_scroll_context_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let spacer = make_element(
        "layout_scroll_context_spacer",
        ElementKind::El,
        fixed_box_attrs(200.0, 24.0),
    );
    let spacer_id = spacer.id;
    let section = make_element(
        "layout_scroll_context_section",
        ElementKind::Column,
        section_attrs,
    );
    let section_id = section.id;
    let card = make_element("layout_scroll_context_card", ElementKind::El, card_attrs);
    let card_id = card.id;
    let bottom = make_element(
        "layout_scroll_context_bottom",
        ElementKind::El,
        fixed_box_attrs(200.0, 220.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(spacer);
    tree.insert(section);
    tree.insert(card);
    tree.insert(bottom);
    tree.set_children(&section_id, vec![card_id]).unwrap();
    tree.set_children(&root_id, vec![spacer_id, section_id, bottom_id])
        .unwrap();

    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    assert_eq!(
        tree.apply_scroll_y(&root_id, -16.0),
        TreeInvalidation::Paint
    );

    let relayout_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    moving_paint_layer_with_placement_for_stable_id(
        &relayout_output.scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("active scroll offset should keep stable sections cacheable after layout");
}

#[test]
fn test_scroll_container_own_payload_generation_changes_when_direct_content_scrolls() {
    let root_attrs = Attrs {
        width: Some(Length::Px(220.0)),
        height: Some(Length::Px(64.0)),
        spacing: Some(8.0),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut tree = ElementTree::new();
    let root = make_element(
        "direct_scroll_content_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);

    let child_ids: Vec<_> = (0..8)
        .map(|index| {
            let child = make_element(
                &format!("direct_scroll_content_text_{index}"),
                ElementKind::Text,
                text_attrs(&format!("Row {index}")),
            );
            let child_id = child.id;
            tree.insert(child);
            child_id
        })
        .collect();
    tree.set_children(&root_id, child_ids).unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild.clone();
    let initial_layer = paint_layer_by_reason(
        &initial_output.scene.nodes,
        crate::render_scene::PaintLayerReason::ScrollContainer,
    )
    .expect("scroll container should emit a paint layer");
    let initial_generation = initial_layer.content_generation;

    assert_eq!(
        tree.apply_scroll_y(&root_id, -24.0),
        TreeInvalidation::Paint
    );
    let scrolled_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    let scrolled_layer = paint_layer_by_reason(
        &scrolled_output.scene.nodes,
        crate::render_scene::PaintLayerReason::ScrollContainer,
    )
    .expect("scroll container should remain a paint layer after scroll");

    assert_ne!(
        scrolled_layer.content_generation, initial_generation,
        "direct own content inside a scroll container must not reuse a payload across scroll offsets"
    );
}

#[test]
fn test_todo_filter_like_fill_list_shrinks_after_cached_layout() {
    let mut cached = todo_filter_like_tree("cached", 4);
    let mut fresh = todo_filter_like_tree("fresh", 1);
    let cached_ids = todo_filter_like_ids("cached");
    let fresh_ids = todo_filter_like_ids("fresh");

    layout_and_refresh_default(&mut cached, Constraint::new(760.0, 900.0), 1.0);

    let remaining = *cached_ids.rows.first().unwrap();
    cached
        .set_children(&cached_ids.entries, vec![remaining])
        .expect("entries children should update");
    cached.mark_measure_dirty(&cached_ids.entries);
    cached.mark_resolve_dirty(&cached_ids.entries);

    layout_and_refresh_default(&mut cached, Constraint::new(760.0, 900.0), 1.0);
    layout_and_refresh_default(&mut fresh, Constraint::new(760.0, 900.0), 1.0);

    let cached_app = cached.get(&cached_ids.app).unwrap().layout.frame.unwrap();
    let cached_entries = cached
        .get(&cached_ids.entries)
        .unwrap()
        .layout
        .frame
        .unwrap();
    let cached_controls = cached
        .get(&cached_ids.controls)
        .unwrap()
        .layout
        .frame
        .unwrap();
    let fresh_app = fresh.get(&fresh_ids.app).unwrap().layout.frame.unwrap();
    let fresh_entries = fresh.get(&fresh_ids.entries).unwrap().layout.frame.unwrap();
    let fresh_controls = fresh
        .get(&fresh_ids.controls)
        .unwrap()
        .layout
        .frame
        .unwrap();

    assert_eq!(cached_app.height, fresh_app.height);
    assert_eq!(cached_entries.height, fresh_entries.height);
    assert_eq!(cached_controls.y, fresh_controls.y);
    assert!(
        cached_entries.height <= 64.0,
        "filtered todo list should not retain stale multi-row height: {cached_entries:?}"
    );
}

#[test]
fn test_scrolled_section_layer_keeps_full_content_when_descendants_cross_viewport() {
    let root_attrs = Attrs {
        width: Some(Length::Px(220.0)),
        height: Some(Length::Px(100.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let section_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        spacing: Some(8.0),
        ..Attrs::default()
    };

    let card_attrs = |name: &str, r, g, b| {
        let mut attrs = fixed_box_attrs(200.0, 48.0);
        attrs.background = Some(Background::Color(Color::Rgba { r, g, b, a: 255 }));
        make_element(name, ElementKind::El, attrs)
    };

    let mut tree = ElementTree::new();
    let root = make_element("scroll_full_payload_root", ElementKind::Column, root_attrs);
    let root_id = root.id;
    let spacer = make_element(
        "scroll_full_payload_spacer",
        ElementKind::El,
        fixed_box_attrs(200.0, 16.0),
    );
    let spacer_id = spacer.id;
    let section = make_element(
        "scroll_full_payload_section",
        ElementKind::Column,
        section_attrs,
    );
    let section_id = section.id;
    let first = card_attrs("scroll_full_payload_first", 248, 250, 252);
    let first_id = first.id;
    let second = card_attrs("scroll_full_payload_second", 241, 245, 249);
    let second_id = second.id;
    let third = card_attrs("scroll_full_payload_third", 226, 232, 240);
    let third_id = third.id;
    let fourth = card_attrs("scroll_full_payload_fourth", 203, 213, 225);
    let fourth_id = fourth.id;
    let bottom = make_element(
        "scroll_full_payload_bottom",
        ElementKind::El,
        fixed_box_attrs(200.0, 160.0),
    );
    let bottom_id = bottom.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(spacer);
    tree.insert(section);
    tree.insert(first);
    tree.insert(second);
    tree.insert(third);
    tree.insert(fourth);
    tree.insert(bottom);
    tree.set_children(&section_id, vec![first_id, second_id, third_id, fourth_id])
        .unwrap();
    tree.set_children(&root_id, vec![spacer_id, section_id, bottom_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;
    let mut renderer = SceneRenderer::new();

    assert_eq!(tree.apply_scroll_y(&root_id, -8.0), TreeInvalidation::Paint);
    let first_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;
    let (first_placement, first_layer) = moving_paint_layer_with_placement_for_stable_id(
        &first_scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("first scroll should emit the stable section layer");
    let first_generation = first_layer.content_generation;
    let first_bounds = first_layer.bounds;
    let first_stats = render_cache_stats_for_scene(&mut renderer, first_scene, 260, 140);
    assert!(first_stats.paint_layer.stores >= 1, "{first_stats:?}");

    assert_eq!(
        tree.apply_scroll_y(&root_id, -48.0),
        TreeInvalidation::Paint
    );
    let second_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;
    let (second_placement, second_layer) = moving_paint_layer_with_placement_for_stable_id(
        &second_scene.nodes,
        section_id.to_wire_u64(),
    )
    .expect("second scroll should keep the same stable section layer");

    assert_eq!(second_layer.content_generation, first_generation);
    assert_eq!(second_layer.bounds, first_bounds);
    assert!((second_placement.tx - first_placement.tx).abs() <= 0.001);
    assert!((second_placement.ty - (first_placement.ty - 48.0)).abs() <= 0.001);

    let second_stats = render_cache_stats_for_scene(&mut renderer, second_scene, 260, 140);
    assert!(second_stats.paint_layer.hits >= 1, "{second_stats:?}");
}

#[test]
fn test_large_moving_paint_layer_keeps_smaller_child_layer() {
    let root_attrs = Attrs {
        width: Some(Length::Px(320.0)),
        height: Some(Length::Px(120.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let mut wrapper_attrs = fixed_box_attrs(1_100.0, 1_100.0);
    wrapper_attrs.background = Some(Background::Color(Color::Rgba {
        r: 241,
        g: 245,
        b: 249,
        a: 255,
    }));

    let mut card_attrs = fixed_box_attrs(220.0, 64.0);
    card_attrs.background = Some(Background::Color(Color::Rgba {
        r: 248,
        g: 250,
        b: 252,
        a: 255,
    }));
    card_attrs.box_shadows = Some(vec![test_shadow(0.0, 8.0)]);

    let mut tree = ElementTree::new();
    let root = make_element(
        "scroll_oversized_boundary_root",
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let wrapper = make_element(
        "scroll_oversized_boundary_wrapper",
        ElementKind::Column,
        wrapper_attrs,
    );
    let wrapper_id = wrapper.id;
    let card = make_element(
        "scroll_oversized_boundary_card",
        ElementKind::El,
        card_attrs,
    );
    let card_id = card.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(wrapper);
    tree.insert(card);
    tree.set_children(&wrapper_id, vec![card_id]).unwrap();
    tree.set_children(&root_id, vec![wrapper_id]).unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    assert_eq!(tree.apply_scroll_y(&root_id, -8.0), TreeInvalidation::Paint);
    let scrolled_scene = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild)).scene;

    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &scrolled_scene.nodes,
            wrapper_id.to_wire_u64(),
        )
        .is_some(),
        "large scroll-moving paint-layer boundaries should be semantic; cache admission decides whether the payload stores"
    );
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &scrolled_scene.nodes,
            card_id.to_wire_u64(),
        )
        .is_none(),
        "clean static descendants should stay inside the ancestor payload; cache \
         admission decides whether that semantic layer stores"
    );
}

#[test]
fn test_registry_chunked_rebuild_matches_full_after_registry_patch() {
    let mut tree = ElementTree::new();
    let root = make_element("registry_root", ElementKind::Row, Attrs::default());
    let root_id = root.id;
    let first = make_element("registry_first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("registry_second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&root_id, vec![first_id, second_id])
        .unwrap();

    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    let mut chunked_tree = tree.clone();
    build_registry_rebuild_cached(&mut chunked_tree);
    assert!(chunked_tree.has_registry_subtree_cache());

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: first_id,
            attrs_raw: raw_text_event_attrs("One"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Registry);

    let full_rebuild = build_registry_rebuild(&tree);
    let mut unseeded_chunked_tree = tree.clone();
    let unseeded_chunked_rebuild = build_registry_rebuild_cached(&mut unseeded_chunked_tree);
    assert_registry_rebuild_payloads_equivalent(&full_rebuild, &unseeded_chunked_rebuild);

    let chunked_invalidation = apply_patches(
        &mut chunked_tree,
        vec![Patch::SetAttrs {
            id: first_id,
            attrs_raw: raw_text_event_attrs("One"),
        }],
    )
    .unwrap();
    assert_eq!(chunked_invalidation, TreeInvalidation::Registry);
    let chunked_rebuild = build_registry_rebuild_cached(&mut chunked_tree);

    assert_registry_rebuild_payloads_equivalent(&full_rebuild, &chunked_rebuild);
    assert!(tree.has_registry_refresh_damage());
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
fn test_dirty_integer_move_transform_bypasses_moving_paint_layer() {
    let mut tree = ElementTree::new();
    let root = make_element(
        "moving_row",
        ElementKind::Row,
        moving_row_attrs_with_move_x(12.0),
    );
    let root_id = root.id;
    let first = make_element("moving_first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("moving_second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&root_id, vec![first_id, second_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        first_moving_paint_layer(&warmed_output.scene.nodes).is_none(),
        "non-scroll clean refresh should not emit opportunistic scroll-moving paint layers"
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_moving_row_attrs_with_move_x(24.0),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);

    let moved_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &moved_output.scene.nodes,
            root_id.to_wire_u64(),
        )
        .is_none(),
        "dirty transform refresh should draw the dirty element directly instead of rebuilding a scroll-moving paint layer"
    );
    assert_eq!(
        scene_without_moving_paint_layers(moved_output.scene),
        render_tree_scene(&tree).scene
    );
}

#[test]
fn test_child_paint_change_bypasses_dirty_parent_moving_paint_layer() {
    let mut tree = ElementTree::new();
    let root = make_element(
        "moving_row_generation",
        ElementKind::Row,
        moving_row_attrs_with_move_x(12.0),
    );
    let root_id = root.id;
    let first = make_element(
        "moving_generation_first",
        ElementKind::Text,
        text_attrs("One"),
    );
    let first_id = first.id;
    let second = make_element(
        "moving_generation_second",
        ElementKind::Text,
        text_attrs("Two"),
    );
    let second_id = second.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&root_id, vec![first_id, second_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        first_moving_paint_layer(&warmed_output.scene.nodes).is_none(),
        "non-scroll clean refresh should not emit opportunistic scroll-moving paint layers"
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: first_id,
            attrs_raw: raw_text_background_attrs("One"),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);

    let content_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &content_output.scene.nodes,
            root_id.to_wire_u64(),
        )
        .is_none(),
        "paint-dirty descendant should make the parent draw directly instead of hashing a new parent layer during refresh"
    );
    assert_eq!(
        scene_without_moving_paint_layers(content_output.scene),
        render_tree_scene(&tree).scene
    );
}

#[test]
fn test_dirty_root_alpha_change_bypasses_moving_paint_layer() {
    let mut tree = ElementTree::new();
    let root = make_element("alpha_row", ElementKind::Row, alpha_row_attrs(0.42));
    let root_id = root.id;
    let first = make_element("alpha_first", ElementKind::Text, text_attrs("One"));
    let first_id = first.id;
    let second = make_element("alpha_second", ElementKind::Text, text_attrs("Two"));
    let second_id = second.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first);
    tree.insert(second);
    tree.set_children(&root_id, vec![first_id, second_id])
        .unwrap();

    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    let warmed_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        first_moving_paint_layer(&warmed_output.scene.nodes).is_none(),
        "non-scroll clean refresh should not emit opportunistic scroll-moving paint layers"
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_alpha_row_attrs(0.72),
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);

    let faded_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &faded_output.scene.nodes,
            root_id.to_wire_u64(),
        )
        .is_none(),
        "dirty alpha refresh should draw the alpha subtree directly instead of rebuilding a scroll-moving paint layer"
    );
    assert_eq!(
        scene_without_moving_paint_layers(faded_output.scene),
        render_tree_scene(&tree).scene
    );
}

#[test]
fn test_layout_reflow_does_not_emit_old_moving_paint_layer_without_scroll_damage() {
    let (mut tree, root_id, target_card_id) = layout_reflow_layer_tree();
    let initial_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    let cached_rebuild = initial_output.event_rebuild;

    let wide_output = refresh_reusing_clean_registry(&mut tree, Some(&cached_rebuild));
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &wide_output.scene.nodes,
            target_card_id.to_wire_u64(),
        )
        .is_none(),
        "non-scroll clean refresh should not emit opportunistic scroll-moving paint layers"
    );

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetAttrs {
            id: root_id,
            attrs_raw: raw_layout_reflow_root_attrs(210.0),
        }],
    )
    .unwrap();
    assert!(invalidation >= TreeInvalidation::Measure);

    let narrow_output = layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    assert!(
        moving_paint_layer_with_placement_for_stable_id(
            &narrow_output.scene.nodes,
            target_card_id.to_wire_u64(),
        )
        .is_none(),
        "layout reflow should not use the old opportunistic local-subtree paint-layer path"
    );
}

#[test]
fn test_paint_only_patch_and_paint_only_animation_refresh_skip_layout() {
    let mut root_attrs = fixed_box_attrs(120.0, 64.0);

    let start_attrs = Attrs {
        box_shadows: Some(vec![test_shadow(0.0, -12.0)]),
        ..Attrs::default()
    };
    let end_attrs = Attrs {
        box_shadows: Some(vec![test_shadow(12.0, 0.0)]),
        ..Attrs::default()
    };
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
    let mut attrs = fixed_height_attrs(64.0);

    let start_attrs = fixed_width_attrs(120.0);
    let end_attrs = fixed_width_attrs(160.0);
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
fn test_slider_value_change_is_resolve_only_and_keeps_measure_cache_hot() {
    let slider_id = NodeId::from_u64(81_000);
    let track_id = NodeId::from_u64(81_001);
    let filled_id = NodeId::from_u64(81_002);
    let thumb_id = NodeId::from_u64(81_003);

    let mut tree = ElementTree::new();
    tree.set_root_id(slider_id);

    let mut slider_attrs = fixed_box_attrs(200.0, 40.0);
    slider_attrs.slider_min = Some(0.0);
    slider_attrs.slider_max = Some(100.0);
    slider_attrs.slider_value = Some(25.0);
    slider_attrs.slider_step = Some(1.0);
    tree.insert(Element::with_attrs(
        slider_id,
        ElementKind::Slider,
        Vec::new(),
        slider_attrs,
    ));
    tree.insert(Element::with_attrs(
        track_id,
        ElementKind::El,
        Vec::new(),
        fixed_height_attrs(8.0),
    ));
    tree.insert(Element::with_attrs(
        filled_id,
        ElementKind::El,
        Vec::new(),
        fixed_height_attrs(8.0),
    ));
    tree.insert(Element::with_attrs(
        thumb_id,
        ElementKind::El,
        Vec::new(),
        fixed_box_attrs(20.0, 20.0),
    ));
    tree.set_children(&slider_id, vec![track_id, filled_id, thumb_id])
        .unwrap();

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let initial_thumb_x = tree.get(&thumb_id).unwrap().layout.frame.unwrap().x;

    tree.set_layout_cache_stats_enabled(true);
    let invalidation = tree.set_slider_value(&slider_id, 75.0);

    assert_eq!(invalidation, TreeInvalidation::Resolve);
    assert!(!tree.get(&slider_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&slider_id).unwrap().layout.resolve_dirty);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();
    let changed_thumb_x = tree.get(&thumb_id).unwrap().layout.frame.unwrap().x;
    let filled_width = tree.get(&filled_id).unwrap().layout.frame.unwrap().width;

    assert_eq!(stats.subtree_measure_misses, 0);
    assert!(stats.resolve_misses > 0);
    assert!(changed_thumb_x > initial_thumb_x);
    assert_eq!(filled_width, 135.0);
}

#[test]
fn test_row_text_patch_dirties_measure_and_resolve() {
    let mut tree = text_child_tree("Hello");
    let root_id = tree.root_id().unwrap();
    let text_id = tree.child_ids(&root_id)[0];
    tree.get_mut(&root_id).unwrap().spec.kind = ElementKind::Row;

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
fn test_text_patch_inside_nested_fixed_el_keeps_outer_resolve_cache_hot() {
    let mut tree = ElementTree::new();
    let outer = make_element(
        "nested_fixed_outer",
        ElementKind::El,
        fixed_box_attrs(200.0, 200.0),
    );
    let outer_id = outer.id;
    let inner_attrs = Attrs {
        align_x: Some(AlignX::Right),
        align_y: Some(AlignY::Bottom),
        ..fixed_box_attrs(100.0, 100.0)
    };
    let inner = make_element("nested_fixed_inner", ElementKind::El, inner_attrs);
    let inner_id = inner.id;
    let text = make_element("nested_fixed_text", ElementKind::Text, text_attrs("Hi"));
    let text_id = text.id;

    tree.set_root_id(outer_id);
    tree.insert(outer);
    tree.insert(inner);
    tree.insert(text);
    tree.set_children(&outer_id, vec![inner_id]).unwrap();
    tree.set_children(&inner_id, vec![text_id]).unwrap();

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

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
    assert!(tree.get(&inner_id).unwrap().layout.resolve_dirty);
    assert!(!tree.get(&outer_id).unwrap().layout.resolve_dirty);
    assert!(tree.get(&outer_id).unwrap().layout.resolve_descendant_dirty);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();

    assert!(stats.resolve_hits > 0);
    assert_eq!(stats.subtree_measure_misses, 1);
    assert!(!tree.get(&outer_id).unwrap().layout.resolve_descendant_dirty);

    let text_frame = tree.get(&text_id).unwrap().layout.frame.unwrap();
    assert_eq!(text_frame.x, 152.0);
    assert_eq!(text_frame.y, 184.0);
}

#[test]
fn test_nearby_slot_change_reuses_host_measure_cache() {
    let mut tree = fixed_host_with_nearby_tree(true);
    let root_id = tree.root_id().unwrap();
    let host_id = tree.child_ids(&root_id)[0];
    let nearby_id = tree.nearby_mounts_for(&host_id)[0].id;

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let host_measured = tree.get(&host_id).unwrap().layout.measured_frame;
    tree.set_layout_cache_stats_enabled(true);

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetNearbyMounts {
            host_id,
            mounts: vec![NearbyMount {
                slot: NearbySlot::Below,
                id: nearby_id,
            }],
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Paint);
    assert!(!tree.get(&host_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&host_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_descendant_dirty);
    let stats = tree.layout_cache_stats();

    assert_eq!(
        tree.get(&host_id).unwrap().layout.measured_frame,
        host_measured
    );
    assert!(stats.subtree_measure_hits >= 1);
    assert!(stats.resolve_hits >= 1);
    assert!(!tree.get(&host_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&host_id).unwrap().layout.resolve_descendant_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.resolve_descendant_dirty);
}

#[test]
fn test_nearby_slot_change_cached_resolve_matches_uncached_layout() {
    let mut cached = fixed_host_with_nearby_tree(true);
    let root_id = cached.root_id().unwrap();
    let host_id = cached.child_ids(&root_id)[0];
    let nearby_id = cached.nearby_mounts_for(&host_id)[0].id;

    layout_tree(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let invalidation = apply_patches(
        &mut cached,
        vec![Patch::SetNearbyMounts {
            host_id,
            mounts: vec![NearbyMount {
                slot: NearbySlot::Below,
                id: nearby_id,
            }],
        }],
    )
    .unwrap();
    assert_eq!(invalidation, TreeInvalidation::Paint);

    let mut uncached = fixed_host_with_nearby_tree(true);
    let uncached_root_id = uncached.root_id().unwrap();
    let uncached_host_id = uncached.child_ids(&uncached_root_id)[0];
    let uncached_nearby_id = uncached.nearby_mounts_for(&uncached_host_id)[0].id;
    uncached
        .set_nearby_mounts(
            &uncached_host_id,
            vec![NearbyMount {
                slot: NearbySlot::Below,
                id: uncached_nearby_id,
            }],
        )
        .unwrap();
    layout_tree(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    assert_layout_matches(&cached, &uncached);
}

#[test]
fn test_insert_nearby_subtree_keeps_host_measurement_clean() {
    let mut tree = fixed_host_with_nearby_tree(false);
    let root_id = tree.root_id().unwrap();
    let host_id = tree.child_ids(&root_id)[0];

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let host_measured = tree.get(&host_id).unwrap().layout.measured_frame;
    let sibling_id = tree.child_ids(&root_id)[1];
    let sibling_frame = tree.get(&sibling_id).unwrap().layout.frame;
    tree.set_layout_cache_stats_enabled(true);

    let mut subtree = ElementTree::new();
    let nearby = make_element(
        "inserted_nearby",
        ElementKind::El,
        fixed_box_attrs(120.0, 40.0),
    );
    let nearby_id = nearby.id;
    let text = make_element(
        "inserted_nearby_text",
        ElementKind::Text,
        text_attrs("Code"),
    );
    let text_id = text.id;
    subtree.set_root_id(nearby_id);
    subtree.insert(nearby);
    subtree.insert(text);
    subtree.set_children(&nearby_id, vec![text_id]).unwrap();

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::InsertNearbySubtree {
            host_id,
            index: 0,
            slot: NearbySlot::Below,
            subtree,
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Paint);
    assert!(!tree.get(&host_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&host_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&nearby_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&nearby_id).unwrap().layout.frame.is_some());
    let stats = tree.layout_cache_stats();

    assert_eq!(
        tree.get(&host_id).unwrap().layout.measured_frame,
        host_measured
    );
    assert_eq!(tree.get(&sibling_id).unwrap().layout.frame, sibling_frame);
    assert!(stats.subtree_measure_misses <= 2);
    assert!(stats.resolve_misses <= 2);
    assert!(!tree.get(&host_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_descendant_dirty);
    assert!(!tree.get(&host_id).unwrap().layout.resolve_descendant_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.resolve_descendant_dirty);
}

#[test]
fn test_set_children_inside_nearby_stops_layout_dirty_at_nearby_boundary() {
    let mut tree = ElementTree::new();
    let root = make_element(
        "nearby_children_root",
        ElementKind::Column,
        Attrs::default(),
    );
    let root_id = root.id;
    let host = make_element(
        "nearby_children_host",
        ElementKind::El,
        fixed_box_attrs(120.0, 48.0),
    );
    let host_id = host.id;
    let overlay = make_element(
        "nearby_children_overlay",
        ElementKind::Column,
        fixed_box_attrs(100.0, 40.0),
    );
    let overlay_id = overlay.id;
    let old_text = make_element("nearby_children_old", ElementKind::Text, text_attrs("Old"));
    let old_text_id = old_text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(host);
    tree.insert(overlay);
    tree.insert(old_text);
    tree.set_children(&root_id, vec![host_id]).unwrap();
    tree.set_children(&overlay_id, vec![old_text_id]).unwrap();
    tree.set_nearby_mounts(
        &host_id,
        vec![NearbyMount {
            slot: NearbySlot::Above,
            id: overlay_id,
        }],
    )
    .unwrap();

    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    let new_text = make_element("nearby_children_new", ElementKind::Text, text_attrs("New"));
    let new_text_id = new_text.id;
    tree.insert(new_text);

    let invalidation = apply_patches(
        &mut tree,
        vec![Patch::SetChildren {
            id: overlay_id,
            children: vec![new_text_id],
        }],
    )
    .unwrap();

    assert_eq!(invalidation, TreeInvalidation::Resolve);
    assert!(tree.get(&overlay_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&host_id).unwrap().layout.measure_dirty);
    assert!(!tree.get(&root_id).unwrap().layout.measure_dirty);
    assert!(tree.get(&host_id).unwrap().layout.resolve_descendant_dirty);
    assert!(tree.get(&root_id).unwrap().layout.resolve_descendant_dirty);
}

#[test]
fn test_reinserted_nearby_subtree_reuses_detached_layout_cache() {
    let mut tree = nearby_placeholder_tree("detached_initial");
    let root_id = tree.root_id().unwrap();
    let host_id = tree.child_ids(&root_id)[0];

    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    assert_eq!(
        replace_nearby_root(
            &mut tree,
            host_id,
            nearby_code_subtree("first", &["Code", "Border.width(2)", "Border.dashed()"]),
        ),
        TreeInvalidation::Paint
    );
    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    assert_eq!(
        replace_nearby_root(&mut tree, host_id, nearby_none_subtree("detached_hidden")),
        TreeInvalidation::Paint
    );
    assert!(tree.has_render_refresh_damage());
    assert!(!tree.has_registry_refresh_damage());
    refresh(&mut tree);

    tree.set_layout_cache_stats_enabled(true);
    assert_eq!(
        replace_nearby_root(
            &mut tree,
            host_id,
            nearby_code_subtree("second", &["Code", "Border.width(2)", "Border.dashed()"]),
        ),
        TreeInvalidation::Paint
    );
    assert!(tree.has_render_refresh_damage());
    assert!(!tree.has_registry_refresh_damage());
    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );
    let stats = tree.layout_cache_stats();

    assert_eq!(stats.intrinsic_measure_misses, 0);
    assert_eq!(stats.subtree_measure_misses, 0);
    assert_eq!(stats.resolve_misses, 0);
    assert!(stats.subtree_measure_hits > 0);
    assert!(stats.resolve_hits > 0);
}

#[test]
fn test_reinserted_nearby_subtree_changed_slot_misses_detached_layout_cache() {
    let mut cached = nearby_placeholder_tree("detached_slot_context");
    let root_id = cached.root_id().unwrap();
    let host_id = cached.child_ids(&root_id)[0];

    layout_and_refresh_default(&mut cached, Constraint::new(800.0, 600.0), 1.0);
    assert_eq!(
        replace_nearby_root_in_slot(
            &mut cached,
            host_id,
            NearbySlot::Above,
            nearby_fill_width_subtree("slot_context_first"),
        ),
        TreeInvalidation::Paint
    );
    layout_and_refresh_default(&mut cached, Constraint::new(800.0, 600.0), 1.0);

    cached.set_layout_cache_stats_enabled(true);
    assert_eq!(
        replace_nearby_root_in_slot(
            &mut cached,
            host_id,
            NearbySlot::OnRight,
            nearby_fill_width_subtree("slot_context_second"),
        ),
        TreeInvalidation::Paint
    );

    let mut uncached = nearby_placeholder_tree("detached_slot_context");
    let uncached_root_id = uncached.root_id().unwrap();
    let uncached_host_id = uncached.child_ids(&uncached_root_id)[0];
    assert_eq!(
        replace_nearby_root_in_slot(
            &mut uncached,
            uncached_host_id,
            NearbySlot::OnRight,
            nearby_fill_width_subtree("slot_context_second"),
        ),
        TreeInvalidation::Resolve
    );
    layout_tree_default(&mut uncached, Constraint::new(800.0, 600.0), 1.0);

    assert_layout_matches(&cached, &uncached);
}

#[test]
fn test_reinserted_nearby_subtree_changed_host_misses_detached_layout_cache() {
    let mut cached = nearby_two_host_placeholder_tree("detached_host_context");
    let root_id = cached.root_id().unwrap();
    let host_ids = cached.child_ids(&root_id);
    let first_host_id = host_ids[0];
    let second_host_id = host_ids[1];

    layout_and_refresh_default(&mut cached, Constraint::new(800.0, 600.0), 1.0);
    assert_eq!(
        replace_nearby_root_in_slot(
            &mut cached,
            first_host_id,
            NearbySlot::Above,
            nearby_fill_width_subtree("host_context_first"),
        ),
        TreeInvalidation::Paint
    );
    layout_and_refresh_default(&mut cached, Constraint::new(800.0, 600.0), 1.0);

    let old_id = cached.nearby_mounts_for(&first_host_id)[0].id;
    cached.set_layout_cache_stats_enabled(true);
    assert_eq!(
        apply_patches(
            &mut cached,
            vec![
                Patch::Remove { id: old_id },
                Patch::InsertNearbySubtree {
                    host_id: second_host_id,
                    index: 0,
                    slot: NearbySlot::Above,
                    subtree: nearby_fill_width_subtree("host_context_second"),
                },
            ],
        )
        .unwrap(),
        TreeInvalidation::Paint
    );

    let mut uncached = nearby_two_host_placeholder_tree("detached_host_context");
    let uncached_root_id = uncached.root_id().unwrap();
    let uncached_host_ids = uncached.child_ids(&uncached_root_id);
    let uncached_first_host_id = uncached_host_ids[0];
    let uncached_second_host_id = uncached_host_ids[1];
    let hidden_id = uncached.nearby_mounts_for(&uncached_first_host_id)[0].id;
    assert_eq!(
        apply_patches(
            &mut uncached,
            vec![
                Patch::Remove { id: hidden_id },
                Patch::InsertNearbySubtree {
                    host_id: uncached_second_host_id,
                    index: 0,
                    slot: NearbySlot::Above,
                    subtree: nearby_fill_width_subtree("host_context_second"),
                },
            ],
        )
        .unwrap(),
        TreeInvalidation::Resolve
    );
    layout_tree_default(&mut uncached, Constraint::new(800.0, 600.0), 1.0);

    assert_layout_matches(&cached, &uncached);
}

#[test]
fn test_nearby_registry_subtree_removal_keeps_registry_invalidation() {
    let mut tree = nearby_placeholder_tree("detached_registry_initial");
    let root_id = tree.root_id().unwrap();
    let host_id = tree.child_ids(&root_id)[0];

    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);
    assert_eq!(
        replace_nearby_root(&mut tree, host_id, nearby_event_subtree("event_first")),
        TreeInvalidation::Paint
    );
    layout_and_refresh_default(&mut tree, Constraint::new(800.0, 600.0), 1.0);

    assert_eq!(
        replace_nearby_root(&mut tree, host_id, nearby_none_subtree("event_hidden")),
        TreeInvalidation::Paint
    );
    assert!(tree.has_render_refresh_damage());
    assert!(tree.has_registry_refresh_damage());
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

    let root_attrs = fixed_box_attrs(100.0, 100.0);
    let root = make_element("fixed_animation_root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let mut child_attrs = fixed_height_attrs(20.0);
    let start_attrs = fixed_width_attrs(20.0);
    let end_attrs = fixed_width_attrs(60.0);
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

fn assert_paint_only_inherited_text_animation_matches_uncached(use_nearby: bool) {
    let start = Instant::now();
    let mut cached = inherited_text_color_animation_tree(use_nearby);
    let mut uncached = inherited_text_color_animation_tree(use_nearby);

    let mut cached_runtime = AnimationRuntime::default();
    cached_runtime.sync_with_tree(&cached, start);
    let mut uncached_runtime = AnimationRuntime::default();
    uncached_runtime.sync_with_tree(&uncached, start);

    let initial_cached = layout_or_refresh_default_with_animation(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &cached_runtime,
        start,
    );
    let initial_uncached = layout_or_refresh_default_with_animation_uncached_for_benchmark(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &uncached_runtime,
        start,
    );

    assert!(initial_cached.layout_performed);
    assert!(initial_uncached.layout_performed);

    let cached_update = layout_or_refresh_default_with_animation(
        &mut cached,
        Constraint::new(800.0, 600.0),
        1.0,
        &cached_runtime,
        start + Duration::from_millis(25),
    );
    let uncached_update = layout_or_refresh_default_with_animation_uncached_for_benchmark(
        &mut uncached,
        Constraint::new(800.0, 600.0),
        1.0,
        &uncached_runtime,
        start + Duration::from_millis(25),
    );

    assert!(cached_update.output.animations_active);
    assert!(uncached_update.output.animations_active);
    assert!(!cached_update.layout_performed);
    assert!(!uncached_update.layout_performed);
    assert_eq!(
        scene_without_moving_paint_layers(cached_update.output.scene),
        scene_without_moving_paint_layers(uncached_update.output.scene)
    );
    assert_layout_matches(&cached, &uncached);
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

fn layout_reflow_layer_tree() -> (ElementTree, NodeId, NodeId) {
    let mut tree = ElementTree::new();
    let root_attrs = Attrs {
        width: Some(Length::Px(420.0)),
        spacing_x: Some(8.0),
        spacing_y: Some(8.0),
        ..Attrs::default()
    };
    let root = make_element("layout_reflow_root", ElementKind::WrappedRow, root_attrs);
    let root_id = root.id;

    let cards: Vec<NodeId> = ["alpha", "beta", "gamma"]
        .into_iter()
        .map(|label| {
            let mut card_attrs = fixed_box_attrs(180.0, 48.0);
            card_attrs.background = Some(Background::Color(Color::Rgba {
                r: 248,
                g: 250,
                b: 252,
                a: 255,
            }));
            let card = make_element(
                &format!("layout_reflow_card_{label}"),
                ElementKind::El,
                card_attrs,
            );
            let card_id = card.id;
            tree.insert(card);
            card_id
        })
        .collect();
    let target_card_id = cards[1];

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.set_children(&root_id, cards).unwrap();
    (tree, root_id, target_card_id)
}

fn fixed_host_with_nearby_tree(include_nearby: bool) -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element(
        "nearby_boundary_root",
        ElementKind::Column,
        Attrs::default(),
    );
    let root_id = root.id;
    let host = make_element(
        "nearby_boundary_host",
        ElementKind::El,
        fixed_box_attrs(100.0, 40.0),
    );
    let host_id = host.id;
    let sibling = make_element(
        "nearby_boundary_sibling",
        ElementKind::Text,
        text_attrs("Sibling"),
    );
    let sibling_id = sibling.id;
    let nearby = make_element(
        "nearby_boundary_existing_nearby",
        ElementKind::El,
        fixed_box_attrs(80.0, 24.0),
    );
    let nearby_id = nearby.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(host);
    tree.insert(sibling);
    tree.insert(nearby);
    tree.set_children(&root_id, vec![host_id, sibling_id])
        .unwrap();

    if include_nearby {
        tree.set_nearby_mounts(
            &host_id,
            vec![NearbyMount {
                slot: NearbySlot::Above,
                id: nearby_id,
            }],
        )
        .unwrap();
    }

    tree
}

fn nearby_placeholder_tree(seed: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element(
        &format!("{seed}_root"),
        ElementKind::Column,
        Attrs::default(),
    );
    let root_id = root.id;
    let host = make_element(
        &format!("{seed}_host"),
        ElementKind::El,
        fixed_box_attrs(120.0, 48.0),
    );
    let host_id = host.id;
    let hidden = make_element(
        &format!("{seed}_hidden"),
        ElementKind::None,
        Attrs::default(),
    );
    let hidden_id = hidden.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(host);
    tree.insert(hidden);
    tree.set_children(&root_id, vec![host_id]).unwrap();
    tree.set_nearby_mounts(
        &host_id,
        vec![NearbyMount {
            slot: NearbySlot::Above,
            id: hidden_id,
        }],
    )
    .unwrap();
    tree
}

fn nearby_two_host_placeholder_tree(seed: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let root = make_element(
        &format!("{seed}_root"),
        ElementKind::Column,
        Attrs::default(),
    );
    let root_id = root.id;
    let first_host = make_element(
        &format!("{seed}_first_host"),
        ElementKind::El,
        fixed_box_attrs(120.0, 48.0),
    );
    let first_host_id = first_host.id;
    let second_host = make_element(
        &format!("{seed}_second_host"),
        ElementKind::El,
        fixed_box_attrs(240.0, 48.0),
    );
    let second_host_id = second_host.id;
    let hidden = make_element(
        &format!("{seed}_hidden"),
        ElementKind::None,
        Attrs::default(),
    );
    let hidden_id = hidden.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(first_host);
    tree.insert(second_host);
    tree.insert(hidden);
    tree.set_children(&root_id, vec![first_host_id, second_host_id])
        .unwrap();
    tree.set_nearby_mounts(
        &first_host_id,
        vec![NearbyMount {
            slot: NearbySlot::Above,
            id: hidden_id,
        }],
    )
    .unwrap();
    tree
}

fn nearby_none_subtree(seed: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let hidden = make_element(&format!("{seed}_none"), ElementKind::None, Attrs::default());
    let hidden_id = hidden.id;
    tree.set_root_id(hidden_id);
    tree.insert(hidden);
    tree
}

fn nearby_event_subtree(seed: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let mut attrs = fixed_box_attrs(96.0, 24.0);
    attrs.on_mouse_enter = Some(true);
    let root = make_element(&format!("{seed}_event_root"), ElementKind::El, attrs);
    let root_id = root.id;
    tree.set_root_id(root_id);
    tree.insert(root);
    tree
}

fn nearby_code_subtree(seed: &str, lines: &[&str]) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_attrs = Attrs {
        width: Some(Length::Px(320.0)),
        padding: Some(Padding::Uniform(8.0)),
        spacing: Some(4.0),
        ..Attrs::default()
    };
    let root = make_element(
        &format!("{seed}_code_root"),
        ElementKind::Column,
        root_attrs,
    );
    let root_id = root.id;
    let children: Vec<NodeId> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let text = make_element(
                &format!("{seed}_code_line_{index}"),
                ElementKind::Text,
                text_attrs(line),
            );
            let text_id = text.id;
            tree.insert(text);
            text_id
        })
        .collect();

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.set_children(&root_id, children).unwrap();
    tree
}

fn nearby_fill_width_subtree(seed: &str) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_attrs = Attrs {
        width: Some(Length::Fill),
        padding: Some(Padding::Uniform(2.0)),
        ..Attrs::default()
    };
    let root = make_element(&format!("{seed}_fill_root"), ElementKind::El, root_attrs);
    let root_id = root.id;
    let text = make_element(
        &format!("{seed}_fill_text"),
        ElementKind::Text,
        text_attrs("Detached layout cache context"),
    );
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);
    tree.set_children(&root_id, vec![text_id]).unwrap();
    tree
}

fn replace_nearby_root(
    tree: &mut ElementTree,
    host_id: NodeId,
    subtree: ElementTree,
) -> TreeInvalidation {
    replace_nearby_root_in_slot(tree, host_id, NearbySlot::Above, subtree)
}

fn replace_nearby_root_in_slot(
    tree: &mut ElementTree,
    host_id: NodeId,
    slot: NearbySlot,
    subtree: ElementTree,
) -> TreeInvalidation {
    let old_id = tree.nearby_mounts_for(&host_id)[0].id;
    apply_patches(
        tree,
        vec![
            Patch::Remove { id: old_id },
            Patch::InsertNearbySubtree {
                host_id,
                index: 0,
                slot,
                subtree,
            },
        ],
    )
    .unwrap()
}

fn fixed_el_text_tree(content: &str, align_x: AlignX, align_y: AlignY) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(100.0)),
        align_x: Some(align_x),
        align_y: Some(align_y),
        ..Attrs::default()
    };
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
    let paragraph_attrs = fixed_width_attrs(width);
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
    let root_attrs = Attrs {
        width: Some(Length::Px(128.0)),
        spacing_y: Some(4.0),
        ..Attrs::default()
    };
    let root = make_element("text_column", ElementKind::TextColumn, root_attrs);
    let root_id = root.id;

    let paragraph_attrs = Attrs {
        width: Some(Length::Content),
        ..Attrs::default()
    };
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
    let root_attrs = Attrs {
        width: Some(Length::Px(width)),
        spacing_x: Some(4.0),
        spacing_y: Some(6.0),
        ..Attrs::default()
    };
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
    let root_attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(100.0)),
        align_x: Some(align_x),
        ..Attrs::default()
    };
    let root = make_element("aligned_paragraph_root", ElementKind::El, root_attrs);
    let root_id = root.id;

    let paragraph_attrs = fixed_width_attrs(96.0);
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
    let root_attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(100.0)),
        align_x: Some(align_x),
        ..Attrs::default()
    };

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

    let control_attrs = fixed_height_attrs(control_height);
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

fn inherited_text_color_animation_tree(use_nearby: bool) -> ElementTree {
    let mut tree = ElementTree::new();
    let root_attrs = Attrs {
        width: Some(Length::Px(160.0)),
        height: Some(Length::Px(64.0)),
        animate: Some(font_color_animation_spec()),
        ..Attrs::default()
    };

    let root = make_element("animated_color_root", ElementKind::El, root_attrs);
    let root_id = root.id;
    let text = make_element(
        "inherited_color_text",
        ElementKind::Text,
        text_attrs("Inherited"),
    );
    let text_id = text.id;

    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);

    if use_nearby {
        tree.set_nearby_mounts(
            &root_id,
            vec![NearbyMount {
                slot: NearbySlot::InFront,
                id: text_id,
            }],
        )
        .unwrap();
    } else {
        tree.set_children(&root_id, vec![text_id]).unwrap();
    }

    tree
}

fn font_color_animation_spec() -> AnimationSpec {
    let start_attrs = Attrs {
        font_color: Some(Color::Rgba {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }),
        ..Attrs::default()
    };
    let end_attrs = Attrs {
        font_color: Some(Color::Rgba {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        }),
        ..Attrs::default()
    };

    AnimationSpec {
        keyframes: vec![start_attrs, end_attrs],
        duration_ms: 100.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    }
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

fn raw_moving_row_attrs_with_move_x(move_x: f64) -> Vec<u8> {
    let mut data = vec![0, 4];
    push_px_length_attr(&mut data, 1, 180.0);
    push_px_length_attr(&mut data, 2, 48.0);
    push_move_x_attr(&mut data, move_x);
    data.extend_from_slice(&[12, 0, 1, 248, 250, 252, 255]);
    data
}

fn raw_alpha_row_attrs(alpha: f64) -> Vec<u8> {
    let mut data = vec![0, 4];
    push_px_length_attr(&mut data, 1, 180.0);
    push_px_length_attr(&mut data, 2, 48.0);
    push_alpha_attr(&mut data, alpha);
    data.extend_from_slice(&[12, 0, 1, 248, 250, 252, 255]);
    data
}

fn raw_layout_reflow_root_attrs(width: f64) -> Vec<u8> {
    let mut data = vec![0, 2];
    push_px_length_attr(&mut data, 1, width);
    push_spacing_xy_attr(&mut data, 8.0, 8.0);
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

fn push_alpha_attr(data: &mut Vec<u8>, alpha: f64) {
    data.push(35);
    data.extend_from_slice(&alpha.to_be_bytes());
}

fn moving_row_attrs_with_move_x(move_x: f64) -> Attrs {
    Attrs {
        width: Some(Length::Px(180.0)),
        height: Some(Length::Px(48.0)),
        move_x: Some(move_x),
        background: Some(Background::Color(Color::Rgba {
            r: 248,
            g: 250,
            b: 252,
            a: 255,
        })),
        ..Attrs::default()
    }
}

fn alpha_row_attrs(alpha: f64) -> Attrs {
    Attrs {
        width: Some(Length::Px(180.0)),
        height: Some(Length::Px(48.0)),
        alpha: Some(alpha),
        background: Some(Background::Color(Color::Rgba {
            r: 248,
            g: 250,
            b: 252,
            a: 255,
        })),
        ..Attrs::default()
    }
}

fn first_moving_paint_layer(
    nodes: &[crate::render_scene::RenderNode],
) -> Option<MovingPaintLayerView> {
    nodes.iter().find_map(|node| match node {
        crate::render_scene::RenderNode::ShadowPass { children }
        | crate::render_scene::RenderNode::Clip { children, .. }
        | crate::render_scene::RenderNode::RelaxedClip { children, .. }
        | crate::render_scene::RenderNode::Transform { children, .. }
        | crate::render_scene::RenderNode::Alpha { children, .. } => {
            first_moving_paint_layer(children)
        }
        crate::render_scene::RenderNode::PaintLayer(layer)
            if layer.placement == crate::render_scene::PaintLayerPlacement::ScrollMoving =>
        {
            Some(MovingPaintLayerView {
                content_generation: layer.content_generation,
                bounds: layer.bounds,
                children: layer.children.clone(),
            })
        }
        crate::render_scene::RenderNode::PaintLayer(layer) => {
            first_moving_paint_layer(&layer.children)
        }
        crate::render_scene::RenderNode::Primitive(_) => None,
    })
}

fn render_nodes_have_moving_paint_layers(nodes: &[crate::render_scene::RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        crate::render_scene::RenderNode::ShadowPass { children }
        | crate::render_scene::RenderNode::Clip { children, .. }
        | crate::render_scene::RenderNode::RelaxedClip { children, .. }
        | crate::render_scene::RenderNode::Transform { children, .. }
        | crate::render_scene::RenderNode::Alpha { children, .. } => {
            render_nodes_have_moving_paint_layers(children)
        }
        crate::render_scene::RenderNode::PaintLayer(layer)
            if layer.placement == crate::render_scene::PaintLayerPlacement::ScrollMoving =>
        {
            true
        }
        crate::render_scene::RenderNode::PaintLayer(layer) => {
            render_nodes_have_moving_paint_layers(&layer.children)
        }
        crate::render_scene::RenderNode::Primitive(_) => false,
    })
}

fn paint_layer_by_reason(
    nodes: &[crate::render_scene::RenderNode],
    reason: crate::render_scene::PaintLayerReason,
) -> Option<&crate::render_scene::RenderPaintLayer> {
    paint_layers(nodes)
        .into_iter()
        .find(|layer| layer.reason == reason)
}

fn paint_layers(
    nodes: &[crate::render_scene::RenderNode],
) -> Vec<&crate::render_scene::RenderPaintLayer> {
    nodes
        .iter()
        .flat_map(|node| match node {
            crate::render_scene::RenderNode::ShadowPass { children }
            | crate::render_scene::RenderNode::Clip { children, .. }
            | crate::render_scene::RenderNode::RelaxedClip { children, .. }
            | crate::render_scene::RenderNode::Transform { children, .. }
            | crate::render_scene::RenderNode::Alpha { children, .. } => paint_layers(children),
            crate::render_scene::RenderNode::PaintLayer(layer) => {
                let mut layers = vec![layer];
                layers.extend(paint_layers(&layer.children));
                layers
            }
            crate::render_scene::RenderNode::Primitive(_) => Vec::new(),
        })
        .collect()
}

fn moving_paint_layer_with_placement_for_stable_id(
    nodes: &[crate::render_scene::RenderNode],
    stable_id: u64,
) -> Option<(crate::tree::transform::Affine2, MovingPaintLayerView)> {
    fn visit(
        nodes: &[crate::render_scene::RenderNode],
        stable_id: u64,
        placement: crate::tree::transform::Affine2,
    ) -> Option<(crate::tree::transform::Affine2, MovingPaintLayerView)> {
        nodes.iter().find_map(|node| match node {
            crate::render_scene::RenderNode::ShadowPass { children }
            | crate::render_scene::RenderNode::Clip { children, .. }
            | crate::render_scene::RenderNode::RelaxedClip { children, .. }
            | crate::render_scene::RenderNode::Alpha { children, .. } => {
                visit(children, stable_id, placement)
            }
            crate::render_scene::RenderNode::Transform {
                transform,
                children,
            } => visit(children, stable_id, placement.then(*transform)),
            crate::render_scene::RenderNode::PaintLayer(layer)
                if layer.placement == crate::render_scene::PaintLayerPlacement::ScrollMoving
                    && layer.stable_id == stable_id =>
            {
                Some((
                    placement,
                    MovingPaintLayerView {
                        content_generation: layer.content_generation,
                        bounds: layer.bounds,
                        children: layer.children.clone(),
                    },
                ))
            }
            crate::render_scene::RenderNode::PaintLayer(layer) => {
                visit(&layer.children, stable_id, placement)
            }
            crate::render_scene::RenderNode::Primitive(_) => None,
        })
    }

    visit(
        nodes,
        stable_id,
        crate::tree::transform::Affine2::identity(),
    )
}

#[derive(Clone)]
struct TodoFilterLikeIds {
    app: NodeId,
    entries: NodeId,
    controls: NodeId,
    rows: Vec<NodeId>,
}

fn todo_filter_like_ids(seed: &str) -> TodoFilterLikeIds {
    TodoFilterLikeIds {
        app: make_element(
            &format!("{seed}_app"),
            ElementKind::Column,
            Attrs::default(),
        )
        .id,
        entries: make_element(
            &format!("{seed}_entries"),
            ElementKind::Column,
            Attrs::default(),
        )
        .id,
        controls: make_element(
            &format!("{seed}_controls"),
            ElementKind::Row,
            Attrs::default(),
        )
        .id,
        rows: (0..4)
            .map(|index| {
                make_element(
                    &format!("{seed}_row_{index}"),
                    ElementKind::Row,
                    Attrs::default(),
                )
                .id
            })
            .collect(),
    }
}

fn todo_filter_like_tree(seed: &str, row_count: usize) -> ElementTree {
    let ids = todo_filter_like_ids(seed);
    let root_id = make_element(
        &format!("{seed}_root"),
        ElementKind::Column,
        Attrs::default(),
    )
    .id;
    let input_id = make_element(&format!("{seed}_input"), ElementKind::El, Attrs::default()).id;

    let mut root = Element::with_attrs(
        root_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Px(720.0)),
            height: Some(Length::Fill),
            ..Attrs::default()
        },
    );
    root.children = vec![ids.app];

    let mut app = Element::with_attrs(
        ids.app,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Min(
                Box::new(Length::Content),
                Box::new(Length::Fill),
            )),
            ..Attrs::default()
        },
    );
    app.children = vec![input_id, ids.entries, ids.controls];

    let input = Element::with_attrs(
        input_id,
        ElementKind::El,
        Vec::new(),
        fixed_box_attrs(720.0, 65.0),
    );

    let mut entries = Element::with_attrs(
        ids.entries,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            spacing: Some(1.0),
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    entries.children = ids.rows.iter().take(row_count).copied().collect();

    let controls = Element::with_attrs(
        ids.controls,
        ElementKind::Row,
        Vec::new(),
        fixed_box_attrs(720.0, 48.0),
    );

    let rows: Vec<_> = ids
        .rows
        .iter()
        .enumerate()
        .map(|(index, row_id)| {
            Element::with_attrs(
                *row_id,
                ElementKind::Row,
                Vec::new(),
                Attrs {
                    width: Some(Length::Fill),
                    height: Some(Length::Px(58.0)),
                    content: Some(format!("Todo {index}")),
                    ..Attrs::default()
                },
            )
        })
        .collect();

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(app);
    tree.insert(input);
    tree.insert(entries);
    tree.insert(controls);
    rows.into_iter().for_each(|row| tree.insert(row));
    tree
}

fn render_cache_stats_for_scene(
    renderer: &mut SceneRenderer,
    scene: crate::render_scene::RenderScene,
    width: u32,
    height: u32,
) -> RendererCacheFrameStats {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .expect("raster surface should be created for cache stats test");
    let state = RenderState::new(scene, skia_safe::Color::TRANSPARENT, 1, false);
    let mut frame = RenderFrame::new(&mut surface, None);
    renderer
        .render(&mut frame, &state)
        .renderer_cache
        .map(|stats| *stats)
        .unwrap_or_default()
}

fn scene_without_moving_paint_layers(
    scene: crate::render_scene::RenderScene,
) -> crate::render_scene::RenderScene {
    crate::render_scene::RenderScene {
        nodes: nodes_without_moving_paint_layers(scene.nodes),
    }
}

fn nodes_without_moving_paint_layers(
    nodes: Vec<crate::render_scene::RenderNode>,
) -> Vec<crate::render_scene::RenderNode> {
    nodes_without_moving_paint_layers_with_flag(nodes).0
}

fn nodes_without_moving_paint_layers_with_flag(
    nodes: Vec<crate::render_scene::RenderNode>,
) -> (Vec<crate::render_scene::RenderNode>, bool) {
    nodes
        .into_iter()
        .map(node_without_moving_paint_layers)
        .fold(
            (Vec::new(), false),
            |(mut nodes, had_layer), (mut next, next_had)| {
                nodes.append(&mut next);
                (nodes, had_layer || next_had)
            },
        )
}

fn node_without_moving_paint_layers(
    node: crate::render_scene::RenderNode,
) -> (Vec<crate::render_scene::RenderNode>, bool) {
    match node {
        crate::render_scene::RenderNode::ShadowPass { children } => {
            let (children, had_layer) = nodes_without_moving_paint_layers_with_flag(children);
            (
                vec![crate::render_scene::RenderNode::ShadowPass { children }],
                had_layer,
            )
        }
        crate::render_scene::RenderNode::Clip { clips, children } => {
            let (children, had_layer) = nodes_without_moving_paint_layers_with_flag(children);
            (
                vec![crate::render_scene::RenderNode::Clip { clips, children }],
                had_layer,
            )
        }
        crate::render_scene::RenderNode::RelaxedClip { clips, children } => {
            let (children, had_layer) = nodes_without_moving_paint_layers_with_flag(children);
            (
                vec![crate::render_scene::RenderNode::RelaxedClip { clips, children }],
                had_layer,
            )
        }
        crate::render_scene::RenderNode::Transform {
            transform,
            children,
        } => {
            let (children, had_layer) = nodes_without_moving_paint_layers_with_flag(children);
            if had_layer && transform_is_translation(transform) {
                (
                    translate_render_nodes(children, transform.tx, transform.ty),
                    true,
                )
            } else {
                (
                    vec![crate::render_scene::RenderNode::Transform {
                        transform,
                        children,
                    }],
                    had_layer,
                )
            }
        }
        crate::render_scene::RenderNode::Alpha { alpha, children } => {
            let (children, had_layer) = nodes_without_moving_paint_layers_with_flag(children);
            (
                vec![crate::render_scene::RenderNode::Alpha { alpha, children }],
                had_layer,
            )
        }
        crate::render_scene::RenderNode::PaintLayer(layer)
            if layer.policy == crate::render_scene::PaintLayerPolicy::DynamicRedraw =>
        {
            nodes_without_moving_paint_layers_with_flag(layer.children)
        }
        crate::render_scene::RenderNode::PaintLayer(layer)
            if layer.reason == crate::render_scene::PaintLayerReason::Root =>
        {
            nodes_without_moving_paint_layers_with_flag(layer.children)
        }
        crate::render_scene::RenderNode::PaintLayer(layer)
            if layer.placement == crate::render_scene::PaintLayerPlacement::ScrollMoving =>
        {
            (nodes_without_moving_paint_layers(layer.children), true)
        }
        crate::render_scene::RenderNode::PaintLayer(mut layer) => {
            let (children, had_layer) = nodes_without_moving_paint_layers_with_flag(layer.children);
            layer.children = children;
            (
                vec![crate::render_scene::RenderNode::PaintLayer(layer)],
                had_layer,
            )
        }
        crate::render_scene::RenderNode::Primitive(_) => (vec![node], false),
    }
}

fn transform_is_translation(transform: crate::tree::transform::Affine2) -> bool {
    transform.xx == 1.0 && transform.yx == 0.0 && transform.xy == 0.0 && transform.yy == 1.0
}

fn translate_render_nodes(
    nodes: Vec<crate::render_scene::RenderNode>,
    dx: f32,
    dy: f32,
) -> Vec<crate::render_scene::RenderNode> {
    nodes
        .into_iter()
        .map(|node| translate_render_node(node, dx, dy))
        .collect()
}

fn translate_render_node(
    node: crate::render_scene::RenderNode,
    dx: f32,
    dy: f32,
) -> crate::render_scene::RenderNode {
    match node {
        crate::render_scene::RenderNode::ShadowPass { children } => {
            crate::render_scene::RenderNode::ShadowPass {
                children: translate_render_nodes(children, dx, dy),
            }
        }
        crate::render_scene::RenderNode::Clip { clips, children } => {
            crate::render_scene::RenderNode::Clip {
                clips: translate_clip_shapes(clips, dx, dy),
                children: translate_render_nodes(children, dx, dy),
            }
        }
        crate::render_scene::RenderNode::RelaxedClip { clips, children } => {
            crate::render_scene::RenderNode::RelaxedClip {
                clips: translate_clip_shapes(clips, dx, dy),
                children: translate_render_nodes(children, dx, dy),
            }
        }
        crate::render_scene::RenderNode::Transform {
            transform,
            children,
        } => crate::render_scene::RenderNode::Transform {
            transform: crate::tree::transform::Affine2::translation(dx, dy).then(transform),
            children,
        },
        crate::render_scene::RenderNode::Alpha { alpha, children } => {
            crate::render_scene::RenderNode::Alpha {
                alpha,
                children: translate_render_nodes(children, dx, dy),
            }
        }
        crate::render_scene::RenderNode::PaintLayer(mut layer) => {
            layer.bounds.x += dx;
            layer.bounds.y += dy;
            layer.children = translate_render_nodes(layer.children, dx, dy);
            crate::render_scene::RenderNode::PaintLayer(layer)
        }
        crate::render_scene::RenderNode::Primitive(primitive) => {
            crate::render_scene::RenderNode::Primitive(translate_primitive(primitive, dx, dy))
        }
    }
}

fn translate_clip_shapes(
    clips: Vec<crate::tree::geometry::ClipShape>,
    dx: f32,
    dy: f32,
) -> Vec<crate::tree::geometry::ClipShape> {
    clips
        .into_iter()
        .map(|mut clip| {
            clip.rect.x += dx;
            clip.rect.y += dy;
            clip
        })
        .collect()
}

fn translate_primitive(
    primitive: crate::render_scene::DrawPrimitive,
    dx: f32,
    dy: f32,
) -> crate::render_scene::DrawPrimitive {
    match primitive {
        crate::render_scene::DrawPrimitive::Rect(x, y, w, h, fill) => {
            crate::render_scene::DrawPrimitive::Rect(x + dx, y + dy, w, h, fill)
        }
        crate::render_scene::DrawPrimitive::RoundedRect(x, y, w, h, radius, fill) => {
            crate::render_scene::DrawPrimitive::RoundedRect(x + dx, y + dy, w, h, radius, fill)
        }
        crate::render_scene::DrawPrimitive::Border(x, y, w, h, radius, width, color, style) => {
            crate::render_scene::DrawPrimitive::Border(
                x + dx,
                y + dy,
                w,
                h,
                radius,
                width,
                color,
                style,
            )
        }
        crate::render_scene::DrawPrimitive::BorderCorners(
            x,
            y,
            w,
            h,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            width,
            color,
            style,
        ) => crate::render_scene::DrawPrimitive::BorderCorners(
            x + dx,
            y + dy,
            w,
            h,
            top_left,
            top_right,
            bottom_right,
            bottom_left,
            width,
            color,
            style,
        ),
        crate::render_scene::DrawPrimitive::BorderEdges(
            x,
            y,
            w,
            h,
            radius,
            top,
            right,
            bottom,
            left,
            color,
            style,
        ) => crate::render_scene::DrawPrimitive::BorderEdges(
            x + dx,
            y + dy,
            w,
            h,
            radius,
            top,
            right,
            bottom,
            left,
            color,
            style,
        ),
        crate::render_scene::DrawPrimitive::Shadow(
            x,
            y,
            w,
            h,
            offset_x,
            offset_y,
            blur,
            size,
            radius,
            color,
        ) => crate::render_scene::DrawPrimitive::Shadow(
            x + dx,
            y + dy,
            w,
            h,
            offset_x,
            offset_y,
            blur,
            size,
            radius,
            color,
        ),
        crate::render_scene::DrawPrimitive::InsetShadow(
            x,
            y,
            w,
            h,
            offset_x,
            offset_y,
            blur,
            size,
            radius,
            color,
        ) => crate::render_scene::DrawPrimitive::InsetShadow(
            x + dx,
            y + dy,
            w,
            h,
            offset_x,
            offset_y,
            blur,
            size,
            radius,
            color,
        ),
        crate::render_scene::DrawPrimitive::TextWithFont(
            x,
            y,
            text,
            font_size,
            fill,
            family,
            weight,
            italic,
        ) => crate::render_scene::DrawPrimitive::TextWithFont(
            x + dx,
            y + dy,
            text,
            font_size,
            fill,
            family,
            weight,
            italic,
        ),
        crate::render_scene::DrawPrimitive::Gradient(x, y, w, h, from, to, angle) => {
            crate::render_scene::DrawPrimitive::Gradient(x + dx, y + dy, w, h, from, to, angle)
        }
        crate::render_scene::DrawPrimitive::Image(x, y, w, h, image_id, fit, tint) => {
            crate::render_scene::DrawPrimitive::Image(x + dx, y + dy, w, h, image_id, fit, tint)
        }
        crate::render_scene::DrawPrimitive::Video(x, y, w, h, target, fit) => {
            crate::render_scene::DrawPrimitive::Video(x + dx, y + dy, w, h, target, fit)
        }
        crate::render_scene::DrawPrimitive::ImageLoading(x, y, w, h) => {
            crate::render_scene::DrawPrimitive::ImageLoading(x + dx, y + dy, w, h)
        }
        crate::render_scene::DrawPrimitive::ImageFailed(x, y, w, h) => {
            crate::render_scene::DrawPrimitive::ImageFailed(x + dx, y + dy, w, h)
        }
    }
}

fn push_align_x_attr(data: &mut Vec<u8>, align_x: AlignX) {
    data.push(5);
    data.push(match align_x {
        AlignX::Left => 0,
        AlignX::Center => 1,
        AlignX::Right => 2,
    });
}
