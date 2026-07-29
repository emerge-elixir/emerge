use super::common::{build_tree_with_child_frame, mount_nearby, solid_fill_attrs};
use super::*;
use crate::render_scene::{
    DrawPrimitive, PaintLayerPlacement, PaintLayerPolicy, PaintLayerReason, RenderNode,
    RenderPaintLayer,
};
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
use crate::tree::geometry::Rect;

#[test]
fn moving_paint_layer_payload_content_generation_ignores_float_noise() {
    let nodes_a = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.1 + 0.2,
        10.000_001,
        80.0,
        40.0,
        0x123456FF,
    ))];
    let nodes_b = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.3, 10.0, 80.0, 40.0, 0x123456FF,
    ))];

    assert_eq!(
        super::super::moving_paint_layer_content_generation(
            &nodes_a,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            }
        ),
        super::super::moving_paint_layer_content_generation(
            &nodes_b,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            }
        )
    );
}

#[test]
fn moving_paint_layer_payload_content_generation_preserves_real_geometry_changes() {
    let nodes_a = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.0, 10.0, 80.0, 40.0, 0x123456FF,
    ))];
    let nodes_b = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.01, 10.0, 80.0, 40.0, 0x123456FF,
    ))];

    assert_ne!(
        super::super::moving_paint_layer_content_generation(
            &nodes_a,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            }
        ),
        super::super::moving_paint_layer_content_generation(
            &nodes_b,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
            }
        )
    );
}

#[test]
fn moving_paint_layer_payload_content_generation_ignores_off_payload_changes() {
    let payload_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 80.0,
    };
    let visible_a = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.0, 10.0, 80.0, 40.0, 0x123456FF,
    ))];
    let visible_b = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.0, 11.0, 80.0, 40.0, 0x123456FF,
    ))];
    let offscreen_a = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.0, 180.0, 80.0, 40.0, 0x123456FF,
    ))];
    let offscreen_b = vec![RenderNode::Primitive(DrawPrimitive::Rect(
        0.0, 181.0, 80.0, 40.0, 0x123456FF,
    ))];

    assert_ne!(
        super::super::moving_paint_layer_content_generation(&visible_a, payload_bounds),
        super::super::moving_paint_layer_content_generation(&visible_b, payload_bounds)
    );
    assert_eq!(
        super::super::moving_paint_layer_content_generation(&offscreen_a, payload_bounds),
        super::super::moving_paint_layer_content_generation(&offscreen_b, payload_bounds)
    );
}

#[test]
fn dynamic_paint_layer_generation_tracks_visible_content() {
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 40.0,
        content_width: 80.0,
        content_height: 40.0,
    };
    let scene = |x, color| {
        super::super::wrap_with_paint_layer(
            vec![RenderNode::Primitive(DrawPrimitive::Rect(
                x, 0.0, 80.0, 40.0, color,
            ))],
            77,
            PaintLayerPlacement::Fixed,
            PaintLayerPolicy::DynamicRedraw,
            PaintLayerReason::Animation,
            frame,
            None,
        )
    };
    let red_a = scene(0.0, 0xFF0000FF);
    let red_b = scene(0.0, 0xFF0000FF);
    let sub_bucket_red = scene(0.000_1, 0xFF0000FF);
    let green = scene(0.0, 0x00FF00FF);
    let generation = |nodes: &[RenderNode]| {
        paint_layers(nodes)
            .into_iter()
            .next()
            .expect("dynamic paint layer should be emitted")
            .content_generation
    };

    assert_ne!(generation(&red_a), 0);
    assert_eq!(generation(&red_a), generation(&red_b));
    assert_ne!(generation(&red_a), generation(&sub_bucket_red));
    assert_ne!(generation(&red_a), generation(&green));
}

#[test]
fn render_damage_emits_dynamic_paint_layer_with_stable_id() {
    let parent_id = NodeId::from_term_bytes(vec![4]);
    let child_id = NodeId::from_term_bytes(vec![5]);
    let mut tree = build_tree_with_child_frame(
        Attrs::default(),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 120.0,
            content_width: 200.0,
            content_height: 120.0,
        },
        Attrs {
            background: Some(Background::Color(Color::Rgb {
                r: 20,
                g: 30,
                b: 40,
            })),
            ..Attrs::default()
        },
        Frame {
            x: 20.0,
            y: 30.0,
            width: 80.0,
            height: 40.0,
            content_width: 80.0,
            content_height: 40.0,
        },
    );
    tree.get_mut(&parent_id).unwrap().layout.scroll_y_max = 140.0;
    tree.clear_refresh_dirty();
    tree.mark_render_and_registry_refresh_dirty(&child_id);

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let dirty_ids = dynamic_paint_layer_ids(&output.scene.nodes);
    let dirty_layer = paint_layers(&output.scene.nodes)
        .into_iter()
        .find(|layer| {
            layer.stable_id == child_id.to_wire_u64()
                && layer.policy == PaintLayerPolicy::DynamicRedraw
        })
        .expect("render-damaged child should emit a dynamic paint layer");

    assert!(dirty_ids.contains(&child_id.to_wire_u64()));
    assert_ne!(dirty_layer.content_generation, 0);
}

#[test]
fn scroll_moving_paint_layer_bounds_include_outer_glow() {
    let parent_id = NodeId::from_term_bytes(vec![4]);
    let child_id = NodeId::from_term_bytes(vec![5]);
    let mut tree = build_tree_with_child_frame(
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
        Frame {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 120.0,
            content_width: 220.0,
            content_height: 260.0,
        },
        Attrs {
            border_radius: Some(BorderRadius::Uniform(10.0)),
            box_shadows: Some(vec![BoxShadow {
                offset_x: 0.0,
                offset_y: 0.0,
                blur: 6.0,
                size: 3.0,
                color: Color::Rgba {
                    r: 255,
                    g: 220,
                    b: 120,
                    a: 90,
                },
                inset: false,
            }]),
            ..solid_fill_attrs((244, 248, 255))
        },
        Frame {
            x: 40.0,
            y: 40.0,
            width: 100.0,
            height: 32.0,
            content_width: 100.0,
            content_height: 32.0,
        },
    );
    tree.get_mut(&parent_id).unwrap().layout.scroll_y_max = 140.0;
    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let scroll_layer =
        paint_layer_by_reason(&output.scene.nodes, PaintLayerReason::ScrollContainer)
            .expect("scroll container should emit a paint layer");
    let layer = paint_layers(&output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == child_id.to_wire_u64())
        .expect("scroll child should emit a stable moving paint layer");

    assert_eq!(layer.reason, PaintLayerReason::StableSubtree);
    assert!(
        !contains_shadow_primitive(&scroll_layer.own_nodes),
        "scroll parent should not own the child's cacheable outer glow"
    );
    assert!(
        contains_shadow_primitive(&layer.own_nodes),
        "scroll child paint layer should cache its own outer glow"
    );
    assert!(layer.bounds.x < 0.0, "{:?}", layer.bounds);
    assert!(layer.bounds.y < 0.0, "{:?}", layer.bounds);
    assert!(layer.bounds.width > 100.0, "{:?}", layer.bounds);
    assert!(layer.bounds.height > 32.0, "{:?}", layer.bounds);
}

#[test]
fn clean_deep_scroll_subtree_reuses_single_retained_paint_layer_without_descending() {
    let root_id = NodeId::from_term_bytes(vec![30]);
    let depth = 32_u8;
    let mut tree = ElementTree::new();

    let mut root = Element::with_attrs(
        root_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            ..solid_fill_attrs((248, 250, 252))
        },
    );
    root.children = vec![NodeId::from_term_bytes(vec![31])];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 160.0,
        content_width: 320.0,
        content_height: 640.0,
    });
    root.layout.scroll_y_max = 480.0;
    tree.set_root_id(root_id);
    tree.insert(root);

    for index in 0..depth {
        let id = NodeId::from_term_bytes(vec![31 + index]);
        let next = (index + 1 < depth).then(|| NodeId::from_term_bytes(vec![32 + index]));
        let mut node = Element::with_attrs(
            id,
            ElementKind::El,
            Vec::new(),
            Attrs {
                border_width: Some(BorderWidth::Uniform(1.0)),
                ..solid_fill_attrs((220_u8.saturating_sub(index), 230, 240))
            },
        );
        node.children = next.into_iter().collect();
        node.layout.frame = Some(Frame {
            x: 8.0 + f32::from(index),
            y: 12.0 + f32::from(index) * 4.0,
            width: 260.0,
            height: 28.0,
            content_width: 260.0,
            content_height: 28.0,
        });
        tree.insert(node);
    }

    tree.clear_refresh_dirty();
    let first_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let first_subtree_layer = paint_layers(&first_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == NodeId::from_term_bytes(vec![31]).to_wire_u64())
        .expect("top scroll child should become the retained paint-layer boundary");

    assert_eq!(first_subtree_layer.reason, PaintLayerReason::StableSubtree);
    assert!(
        first_subtree_layer.metrics.own_primitive_count >= u32::from(depth),
        "the top clean subtree layer should own static descendants instead of \
         splitting them into depth-based child layers"
    );
    assert!(
        first_subtree_layer.child_refs.is_empty(),
        "clean static descendants should be one payload, not nested child refs"
    );

    super::super::reset_render_traversal_diagnostics_for_benchmark();
    let second_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let diagnostics = super::super::take_render_traversal_diagnostics_for_benchmark();

    assert_eq!(
        diagnostics.element_visits, 2,
        "second refresh should visit only the scroll root and retained subtree root"
    );
    let second_subtree_layer = paint_layers(&second_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == NodeId::from_term_bytes(vec![31]).to_wire_u64())
        .expect("retained paint-layer should still be present");
    assert!(std::sync::Arc::ptr_eq(
        &first_subtree_layer.own_nodes,
        &second_subtree_layer.own_nodes
    ));
}

#[test]
fn dirty_scroll_moving_focused_slider_layer_keeps_child_layers_independent() {
    let scroll_id = NodeId::from_term_bytes(vec![40]);
    let slider_id = NodeId::from_term_bytes(vec![41]);
    let track_id = NodeId::from_term_bytes(vec![42]);

    let mut scroll = Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    scroll.children = vec![slider_id];
    scroll.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 120.0,
        content_width: 320.0,
        content_height: 260.0,
    });
    scroll.layout.scroll_y_max = 140.0;

    let mut slider = Element::with_attrs(
        slider_id,
        ElementKind::Slider,
        Vec::new(),
        Attrs {
            border_radius: Some(BorderRadius::Uniform(999.0)),
            box_shadows: Some(vec![BoxShadow {
                offset_x: 0.0,
                offset_y: 0.0,
                blur: 6.0,
                size: 3.0,
                color: Color::Rgba {
                    r: 255,
                    g: 220,
                    b: 120,
                    a: 90,
                },
                inset: false,
            }]),
            ..Attrs::default()
        },
    );
    slider.children = vec![track_id];
    slider.runtime.focused_active = true;
    slider.layout.frame = Some(Frame {
        x: 40.0,
        y: 40.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
    });

    let mut track = Element::with_attrs(
        track_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((80, 80, 80)),
    );
    track.layout.frame = Some(Frame {
        x: 55.0,
        y: 56.0,
        width: 150.0,
        height: 12.0,
        content_width: 150.0,
        content_height: 12.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);
    tree.insert(scroll);
    tree.insert(slider);
    tree.insert(track);
    tree.clear_refresh_dirty();

    let clean_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let clean_slider_layer = paint_layers(&clean_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("clean focused slider should emit its own paint layer");

    tree.mark_render_and_registry_refresh_dirty(&slider_id);

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let scroll_layer =
        paint_layer_by_reason(&output.scene.nodes, PaintLayerReason::ScrollContainer)
            .expect("scroll container should emit a paint layer");
    let slider_layer = paint_layers(&output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("dirty focused slider should still emit its own paint layer");

    assert_eq!(slider_layer.policy, PaintLayerPolicy::Cacheable);
    assert_eq!(slider_layer.reason, PaintLayerReason::StableSubtree);
    assert_eq!(slider_layer.placement, PaintLayerPlacement::ScrollMoving);
    assert_eq!(
        slider_layer.content_generation, clean_slider_layer.content_generation,
        "dirty slider child movement should keep the focused glow payload key stable"
    );
    assert!(
        !contains_shadow_primitive(&scroll_layer.own_nodes),
        "scroll parent should not own the focused slider glow"
    );
    assert!(
        contains_shadow_primitive(&slider_layer.own_nodes),
        "focused slider paint layer should own its glow during drag"
    );
    assert!(
        slider_layer.child_refs.is_empty(),
        "scroll-moving focused slider glow layer should not carry clipped child refs"
    );
    assert!(
        paint_layers(&output.scene.nodes)
            .into_iter()
            .any(|layer| layer.stable_id == track_id.to_wire_u64()),
        "slider child rendering should stay independently cacheable"
    );
    assert!(slider_layer.bounds.x < 0.0, "{:?}", slider_layer.bounds);
    assert!(slider_layer.bounds.y < 0.0, "{:?}", slider_layer.bounds);
    assert!(
        slider_layer.bounds.width > 180.0,
        "{:?}",
        slider_layer.bounds
    );
    assert!(
        slider_layer.bounds.height > 44.0,
        "{:?}",
        slider_layer.bounds
    );
}

#[test]
fn dirty_fixed_focused_slider_layer_keeps_glow_payload_stable() {
    let root_id = NodeId::from_term_bytes(vec![50]);
    let slider_id = NodeId::from_term_bytes(vec![51]);
    let track_id = NodeId::from_term_bytes(vec![52]);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), Attrs::default());
    root.children = vec![slider_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 140.0,
        content_width: 320.0,
        content_height: 140.0,
    });

    let mut slider = Element::with_attrs(
        slider_id,
        ElementKind::Slider,
        Vec::new(),
        Attrs {
            border_radius: Some(BorderRadius::Uniform(999.0)),
            box_shadows: Some(vec![BoxShadow {
                offset_x: 0.0,
                offset_y: 0.0,
                blur: 6.0,
                size: 3.0,
                color: Color::Rgba {
                    r: 255,
                    g: 220,
                    b: 120,
                    a: 90,
                },
                inset: false,
            }]),
            ..Attrs::default()
        },
    );
    slider.children = vec![track_id];
    slider.runtime.focused_active = true;
    slider.layout.frame = Some(Frame {
        x: 40.0,
        y: 40.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
    });

    let mut track = Element::with_attrs(
        track_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((80, 80, 80)),
    );
    track.layout.frame = Some(Frame {
        x: 55.0,
        y: 56.0,
        width: 150.0,
        height: 12.0,
        content_width: 150.0,
        content_height: 12.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(slider);
    tree.insert(track);
    tree.clear_refresh_dirty();

    let clean_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let clean_slider_layer = paint_layers(&clean_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("clean focused slider should emit its own fixed paint layer");

    tree.get_mut(&track_id).unwrap().layout.frame = Some(Frame {
        x: 78.0,
        y: 56.0,
        width: 112.0,
        height: 12.0,
        content_width: 112.0,
        content_height: 12.0,
    });
    tree.mark_render_and_registry_refresh_dirty(&slider_id);
    tree.mark_render_and_registry_refresh_dirty(&track_id);

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let slider_layer = paint_layers(&output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("dirty focused slider should keep its own fixed paint layer");

    assert_eq!(slider_layer.policy, PaintLayerPolicy::Cacheable);
    assert_eq!(slider_layer.reason, PaintLayerReason::StableSubtree);
    assert_eq!(slider_layer.placement, PaintLayerPlacement::Fixed);
    assert_eq!(
        slider_layer.content_generation, clean_slider_layer.content_generation,
        "dirty slider child movement should not invalidate the focused glow payload"
    );
    assert!(
        contains_shadow_primitive(&slider_layer.own_nodes),
        "fixed focused slider paint layer should own its glow during drag"
    );
    assert!(
        !contains_rect_color(&slider_layer.own_nodes, 0x505050FF),
        "fixed focused slider own payload should not include moving track pixels"
    );
    assert!(
        slider_layer
            .child_refs
            .iter()
            .flat_map(|child| paint_layers(&child.nodes))
            .any(|layer| layer.reason == PaintLayerReason::Animation),
        "moving slider children should be composed through a child paint layer"
    );
    assert!(slider_layer.bounds.x < 40.0, "{:?}", slider_layer.bounds);
    assert!(slider_layer.bounds.y < 40.0, "{:?}", slider_layer.bounds);
    assert!(
        slider_layer.bounds.width > 180.0,
        "{:?}",
        slider_layer.bounds
    );
    assert!(
        slider_layer.bounds.height > 44.0,
        "{:?}",
        slider_layer.bounds
    );
}

#[test]
fn focused_text_input_inside_scroll_subtree_keeps_sibling_moving_layers() {
    let scroll_id = NodeId::from_term_bytes(vec![60]);
    let content_id = NodeId::from_term_bytes(vec![61]);
    let before_id = NodeId::from_term_bytes(vec![62]);
    let input_id = NodeId::from_term_bytes(vec![63]);
    let after_id = NodeId::from_term_bytes(vec![64]);

    let mut scroll = Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    scroll.children = vec![content_id];
    scroll.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 360.0,
        height: 180.0,
        content_width: 360.0,
        content_height: 520.0,
    });
    scroll.layout.scroll_y_max = 340.0;

    let mut content = Element::with_attrs(
        content_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            spacing: Some(12.0),
            ..Attrs::default()
        },
    );
    content.children = vec![before_id, input_id, after_id];
    content.layout.frame = Some(Frame {
        x: 16.0,
        y: 16.0,
        width: 320.0,
        height: 420.0,
        content_width: 320.0,
        content_height: 420.0,
    });

    let mut before = Element::with_attrs(
        before_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((220, 230, 240)),
    );
    before.layout.frame = Some(Frame {
        x: 24.0,
        y: 24.0,
        width: 300.0,
        height: 80.0,
        content_width: 300.0,
        content_height: 80.0,
    });

    let mut input = Element::with_attrs(
        input_id,
        ElementKind::TextInput,
        Vec::new(),
        Attrs {
            content: Some("focused".to_string()),
            font_size: Some(16.0),
            ..solid_fill_attrs((255, 255, 255))
        },
    );
    input.runtime.text_input_focused = true;
    input.runtime.text_input_cursor = Some(7);
    input.layout.frame = Some(Frame {
        x: 24.0,
        y: 116.0,
        width: 300.0,
        height: 36.0,
        content_width: 300.0,
        content_height: 36.0,
    });

    let mut after = Element::with_attrs(
        after_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((210, 220, 230)),
    );
    after.layout.frame = Some(Frame {
        x: 24.0,
        y: 164.0,
        width: 300.0,
        height: 120.0,
        content_width: 300.0,
        content_height: 120.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);
    tree.insert(scroll);
    tree.insert(content);
    tree.insert(before);
    tree.insert(input);
    tree.insert(after);
    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let layers = paint_layers(&output.scene.nodes);

    assert!(output.text_input_focused);
    assert!(
        layers
            .iter()
            .any(|layer| layer.stable_id == before_id.to_wire_u64()
                && layer.placement == PaintLayerPlacement::ScrollMoving),
        "focused text input should not force sibling scroll content back to direct drawing"
    );
    assert!(
        layers
            .iter()
            .any(|layer| layer.stable_id == after_id.to_wire_u64()
                && layer.placement == PaintLayerPlacement::ScrollMoving),
        "stable sibling after the focused input should remain independently cached"
    );
    assert!(
        !layers
            .iter()
            .any(|layer| layer.stable_id == content_id.to_wire_u64()),
        "the focused text subtree ancestor should not be cached as one moving layer \
         because render output still needs focused text metadata"
    );
}

#[test]
fn pending_image_inside_scroll_subtree_keeps_sibling_moving_layers() {
    let scroll_id = NodeId::from_term_bytes(vec![65]);
    let content_id = NodeId::from_term_bytes(vec![66]);
    let image_id = NodeId::from_term_bytes(vec![67]);
    let after_id = NodeId::from_term_bytes(vec![68]);

    let mut scroll = Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    scroll.children = vec![content_id];
    scroll.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 360.0,
        height: 180.0,
        content_width: 360.0,
        content_height: 520.0,
    });
    scroll.layout.scroll_y_max = 340.0;

    let mut content = Element::with_attrs(
        content_id,
        ElementKind::Column,
        Vec::new(),
        Attrs::default(),
    );
    content.children = vec![image_id, after_id];
    content.layout.frame = Some(Frame {
        x: 16.0,
        y: 16.0,
        width: 320.0,
        height: 420.0,
        content_width: 320.0,
        content_height: 420.0,
    });

    let mut image = Element::with_attrs(
        image_id,
        ElementKind::Image,
        Vec::new(),
        Attrs {
            image_src: Some(ImageSource::Logical("images/pending.png".to_string())),
            image_fit: Some(ImageFit::Contain),
            ..Attrs::default()
        },
    );
    image.layout.frame = Some(Frame {
        x: 24.0,
        y: 24.0,
        width: 120.0,
        height: 90.0,
        content_width: 120.0,
        content_height: 90.0,
    });

    let mut after = Element::with_attrs(
        after_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((210, 220, 230)),
    );
    after.layout.frame = Some(Frame {
        x: 24.0,
        y: 128.0,
        width: 300.0,
        height: 120.0,
        content_width: 300.0,
        content_height: 120.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);
    tree.insert(scroll);
    tree.insert(content);
    tree.insert(image);
    tree.insert(after);
    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let layers = paint_layers(&output.scene.nodes);

    assert!(
        layers
            .iter()
            .any(|layer| layer.stable_id == after_id.to_wire_u64()
                && layer.placement == PaintLayerPlacement::ScrollMoving),
        "unsupported media placeholders should not force sibling scroll content \
         back to direct drawing"
    );
    assert!(
        !layers
            .iter()
            .any(|layer| layer.stable_id == content_id.to_wire_u64()),
        "ancestor with an uncacheable media leaf should not swallow the whole scroll subtree"
    );
}

#[test]
fn focused_slider_inside_clean_scroll_ancestor_keeps_own_glow_layer() {
    let scroll_id = NodeId::from_term_bytes(vec![70]);
    let content_id = NodeId::from_term_bytes(vec![71]);
    let slider_id = NodeId::from_term_bytes(vec![72]);
    let track_id = NodeId::from_term_bytes(vec![73]);

    let mut scroll = Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    scroll.children = vec![content_id];
    scroll.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 360.0,
        height: 160.0,
        content_width: 360.0,
        content_height: 340.0,
    });
    scroll.layout.scroll_y_max = 180.0;

    let mut content = Element::with_attrs(
        content_id,
        ElementKind::Column,
        Vec::new(),
        Attrs::default(),
    );
    content.children = vec![slider_id];
    content.layout.frame = Some(Frame {
        x: 24.0,
        y: 36.0,
        width: 300.0,
        height: 220.0,
        content_width: 300.0,
        content_height: 220.0,
    });

    let mut slider = Element::with_attrs(
        slider_id,
        ElementKind::Slider,
        Vec::new(),
        Attrs {
            border_radius: Some(BorderRadius::Uniform(999.0)),
            box_shadows: Some(vec![BoxShadow {
                offset_x: 0.0,
                offset_y: 0.0,
                blur: 6.0,
                size: 3.0,
                color: Color::Rgba {
                    r: 255,
                    g: 220,
                    b: 120,
                    a: 90,
                },
                inset: false,
            }]),
            ..Attrs::default()
        },
    );
    slider.children = vec![track_id];
    slider.runtime.focused_active = true;
    slider.layout.frame = Some(Frame {
        x: 40.0,
        y: 60.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
    });

    let mut track = Element::with_attrs(
        track_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((80, 80, 80)),
    );
    track.layout.frame = Some(Frame {
        x: 55.0,
        y: 76.0,
        width: 150.0,
        height: 12.0,
        content_width: 150.0,
        content_height: 12.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);
    tree.insert(scroll);
    tree.insert(content);
    tree.insert(slider);
    tree.insert(track);
    tree.clear_refresh_dirty();

    let clean_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let clean_slider_layer = paint_layers(&clean_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("clean ancestor should preserve focused slider's own glow layer");

    assert_eq!(clean_slider_layer.reason, PaintLayerReason::StableSubtree);
    assert_eq!(
        clean_slider_layer.placement,
        PaintLayerPlacement::ScrollMoving
    );
    assert!(
        contains_shadow_primitive(&clean_slider_layer.own_nodes),
        "focused slider layer should own its glow even under a clean cached ancestor"
    );

    tree.get_mut(&track_id).unwrap().layout.frame = Some(Frame {
        x: 75.0,
        y: 76.0,
        width: 130.0,
        height: 12.0,
        content_width: 130.0,
        content_height: 12.0,
    });
    tree.mark_render_and_registry_refresh_dirty(&slider_id);
    tree.mark_render_and_registry_refresh_dirty(&track_id);

    let dirty_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let dirty_slider_layer = paint_layers(&dirty_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("dirty focused slider should keep the same own glow layer");

    assert_eq!(
        dirty_slider_layer.content_generation, clean_slider_layer.content_generation,
        "track/thumb movement should not make the focused glow blink"
    );
    assert!(contains_shadow_primitive(&dirty_slider_layer.own_nodes));
}

#[test]
fn focused_style_slider_glow_payload_ignores_child_layout_changes() {
    let scroll_id = NodeId::from_term_bytes(vec![74]);
    let slider_id = NodeId::from_term_bytes(vec![75]);
    let label_id = NodeId::from_term_bytes(vec![76]);

    let mut scroll = Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    scroll.children = vec![slider_id];
    scroll.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 360.0,
        height: 160.0,
        content_width: 360.0,
        content_height: 280.0,
    });
    scroll.layout.scroll_y_max = 120.0;

    let mut slider = Element::with_attrs(
        slider_id,
        ElementKind::Slider,
        Vec::new(),
        Attrs {
            border_radius: Some(BorderRadius::Uniform(999.0)),
            focused: Some(MouseOverAttrs {
                box_shadows: Some(vec![BoxShadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur: 6.0,
                    size: 3.0,
                    color: Color::Rgba {
                        r: 255,
                        g: 220,
                        b: 120,
                        a: 90,
                    },
                    inset: false,
                }]),
                ..MouseOverAttrs::default()
            }),
            focused_active: Some(true),
            ..Attrs::default()
        },
    );
    slider.children = vec![label_id];
    slider.layout.frame = Some(Frame {
        x: 40.0,
        y: 60.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
    });

    let mut label = Element::with_attrs(
        label_id,
        ElementKind::Text,
        Vec::new(),
        Attrs {
            content: Some("thumb".to_string()),
            font_size: Some(14.0),
            font_color: Some(Color::Rgb {
                r: 80,
                g: 80,
                b: 80,
            }),
            ..Attrs::default()
        },
    );
    label.layout.frame = Some(Frame {
        x: 55.0,
        y: 76.0,
        width: 60.0,
        height: 18.0,
        content_width: 60.0,
        content_height: 18.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);
    tree.insert(scroll);
    tree.insert(slider);
    tree.insert(label);

    let clean_output =
        crate::tree::layout::refresh_default_with_frame_attrs(&mut tree, 1.0, None, None);
    let clean_slider_layer = paint_layers(&clean_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("focused style should produce a slider-owned glow layer");

    assert_eq!(clean_slider_layer.reason, PaintLayerReason::StableSubtree);
    assert_eq!(
        clean_slider_layer.placement,
        PaintLayerPlacement::ScrollMoving
    );
    assert!(
        contains_shadow_primitive(&clean_slider_layer.own_nodes),
        "focused style shadow should live in the slider's own cached payload"
    );
    assert!(
        !contains_text_primitive(&clean_slider_layer.own_nodes),
        "slider glow payload must not own moving child content"
    );

    tree.get_mut(&label_id).unwrap().layout.frame = Some(Frame {
        x: 125.0,
        y: 76.0,
        width: 60.0,
        height: 18.0,
        content_width: 60.0,
        content_height: 18.0,
    });
    tree.mark_render_and_registry_refresh_dirty(&slider_id);

    let dirty_output =
        crate::tree::layout::refresh_default_with_frame_attrs(&mut tree, 1.0, None, None);
    let dirty_slider_layer = paint_layers(&dirty_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == slider_id.to_wire_u64())
        .expect("focused slider should keep its glow layer while a child moves");

    assert_eq!(
        dirty_slider_layer.content_generation, clean_slider_layer.content_generation,
        "child movement must not invalidate the focused glow payload"
    );
    assert!(contains_shadow_primitive(&dirty_slider_layer.own_nodes));
    assert!(!contains_text_primitive(&dirty_slider_layer.own_nodes));
}

#[test]
fn focused_style_text_input_glow_payload_ignores_content_changes() {
    let root_id = NodeId::from_term_bytes(vec![77]);
    let input_id = NodeId::from_term_bytes(vec![78]);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), Attrs::default());
    root.children = vec![input_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 260.0,
        height: 120.0,
        content_width: 260.0,
        content_height: 120.0,
    });

    let mut input = Element::with_attrs(
        input_id,
        ElementKind::TextInput,
        Vec::new(),
        Attrs {
            content: Some("todo".to_string()),
            width: Some(Length::Px(180.0)),
            height: Some(Length::Px(44.0)),
            padding: Some(Padding::Uniform(16.0)),
            font_size: Some(24.0),
            font_color: Some(Color::Rgb { r: 0, g: 0, b: 0 }),
            focused: Some(MouseOverAttrs {
                border_color: Some(Color::Rgba {
                    r: 207,
                    g: 125,
                    b: 125,
                    a: 255,
                }),
                box_shadows: Some(vec![BoxShadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur: 4.0,
                    size: 2.0,
                    color: Color::Rgba {
                        r: 207,
                        g: 125,
                        b: 125,
                        a: 71,
                    },
                    inset: false,
                }]),
                ..MouseOverAttrs::default()
            }),
            focused_active: Some(true),
            border_width: Some(BorderWidth::Uniform(1.0)),
            background: Some(Background::Color(Color::Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 0,
            })),
            ..Attrs::default()
        },
    );
    input.runtime.focused_active = true;
    input.runtime.text_input_focused = true;
    input.layout.frame = Some(Frame {
        x: 40.0,
        y: 40.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(input);

    let clean_output =
        crate::tree::layout::refresh_default_with_frame_attrs(&mut tree, 1.0, None, None);
    let clean_input_layer = paint_layers(&clean_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == input_id.to_wire_u64())
        .expect("focused style should produce a text-input-owned glow layer");

    assert_eq!(clean_input_layer.reason, PaintLayerReason::StableSubtree);
    assert_eq!(clean_input_layer.policy, PaintLayerPolicy::Cacheable);
    assert!(
        contains_shadow_primitive(&clean_input_layer.own_nodes),
        "focused text input layer should own its glow"
    );
    assert!(
        !contains_text_primitive(&clean_input_layer.own_nodes),
        "focused text input glow payload must not own changing input content"
    );

    tree.set_text_input_content(&input_id, "new todo".to_string());

    let dirty_output =
        crate::tree::layout::refresh_default_with_frame_attrs(&mut tree, 1.0, None, None);
    let dirty_input_layer = paint_layers(&dirty_output.scene.nodes)
        .into_iter()
        .find(|layer| layer.stable_id == input_id.to_wire_u64())
        .expect("focused text input should keep its glow layer while content changes");

    assert_eq!(
        dirty_input_layer.content_generation, clean_input_layer.content_generation,
        "content edits must not invalidate the focused glow payload"
    );
    assert!(contains_shadow_primitive(&dirty_input_layer.own_nodes));
    assert!(!contains_text_primitive(&dirty_input_layer.own_nodes));
}

#[test]
fn nested_animated_shadow_inside_scroll_container_layer_emits_dirty_slot() {
    let scroll_id = NodeId::from_term_bytes(vec![10]);
    let page_id = NodeId::from_term_bytes(vec![11]);
    let section_id = NodeId::from_term_bytes(vec![12]);
    let row_id = NodeId::from_term_bytes(vec![13]);
    let card_id = NodeId::from_term_bytes(vec![14]);
    let text_id = NodeId::from_term_bytes(vec![15]);

    let mut tree = ElementTree::new();
    tree.set_root_id(scroll_id);

    let mut scroll = Element::with_attrs(
        scroll_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            scrollbar_y: Some(true),
            background: Some(Background::Color(Color::Rgb {
                r: 240,
                g: 244,
                b: 250,
            })),
            ..Attrs::default()
        },
    );
    scroll.children = vec![page_id];
    scroll.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 960.0,
        height: 900.0,
        content_width: 960.0,
        content_height: 2200.0,
    });
    scroll.layout.scroll_y_max = 1300.0;

    let mut page = Element::with_attrs(
        page_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Fill),
            spacing: Some(28.0),
            ..Attrs::default()
        },
    );
    page.children = vec![section_id];
    page.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 960.0,
        height: 2200.0,
        content_width: 960.0,
        content_height: 2200.0,
    });

    let mut section = Element::with_attrs(
        section_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::Fill),
            spacing: Some(16.0),
            ..Attrs::default()
        },
    );
    section.children = vec![row_id];
    section.layout.frame = Some(Frame {
        x: 24.0,
        y: 320.0,
        width: 912.0,
        height: 260.0,
        content_width: 912.0,
        content_height: 260.0,
    });

    let mut row = Element::with_attrs(
        row_id,
        ElementKind::Row,
        Vec::new(),
        Attrs {
            width: Some(Length::Fill),
            spacing: Some(14.0),
            ..Attrs::default()
        },
    );
    row.children = vec![card_id];
    row.layout.frame = Some(Frame {
        x: 60.0,
        y: 420.0,
        width: 840.0,
        height: 140.0,
        content_width: 840.0,
        content_height: 140.0,
    });

    let mut card = Element::with_attrs(
        card_id,
        ElementKind::Column,
        Vec::new(),
        Attrs {
            width: Some(Length::FillWeighted(1.0)),
            height: Some(Length::Px(94.0)),
            padding: Some(Padding::Uniform(14.0)),
            background: Some(Background::Color(Color::Rgb {
                r: 244,
                g: 248,
                b: 255,
            })),
            border_radius: Some(BorderRadius::Uniform(14.0)),
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
            animate: Some(AnimationSpec {
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
            }),
            ..Attrs::default()
        },
    );
    card.children = vec![text_id];
    card.layout.frame = Some(Frame {
        x: 80.0,
        y: 440.0,
        width: 260.0,
        height: 94.0,
        content_width: 260.0,
        content_height: 94.0,
    });

    let mut text = Element::with_attrs(
        text_id,
        ElementKind::Text,
        Vec::new(),
        Attrs {
            content: Some("Stacked".to_string()),
            font_size: Some(14.0),
            ..Attrs::default()
        },
    );
    text.layout.frame = Some(Frame {
        x: 94.0,
        y: 454.0,
        width: 80.0,
        height: 16.0,
        content_width: 80.0,
        content_height: 16.0,
    });

    tree.insert(scroll);
    tree.insert(page);
    tree.insert(section);
    tree.insert(row);
    tree.insert(card);
    tree.insert(text);

    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, false, true);
    let scroll_layer =
        paint_layer_by_reason(&output.scene.nodes, PaintLayerReason::ScrollContainer)
            .expect("scroll container should emit a paint layer");
    let dirty_ids = dynamic_paint_layer_ids(&scroll_layer.content_nodes());

    assert!(dirty_ids.contains(&card_id.to_wire_u64()));
    assert!(dynamic_slot_count(&scroll_layer.content_nodes()) > 0);
}

#[test]
fn nearby_escape_root_emits_nearby_layer_boundary_next_to_scroll_container_layer() {
    let parent_id = NodeId::from_term_bytes(vec![4]);
    let host_id = NodeId::from_term_bytes(vec![5]);
    let nearby_id = NodeId::from_term_bytes(vec![42]);
    let mut tree = build_tree_with_child_frame(
        Attrs {
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
        Frame {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 180.0,
            content_width: 320.0,
            content_height: 420.0,
        },
        solid_fill_attrs((20, 24, 32)),
        Frame {
            x: 24.0,
            y: 36.0,
            width: 120.0,
            height: 56.0,
            content_width: 120.0,
            content_height: 56.0,
        },
    );
    tree.get_mut(&parent_id).unwrap().layout.scroll_y_max = 240.0;
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((248, 250, 252)),
        Frame {
            x: 24.0,
            y: 36.0,
            width: 160.0,
            height: 72.0,
            content_width: 160.0,
            content_height: 72.0,
        },
        42,
    );
    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, false, true);
    paint_layer_by_reason(&output.scene.nodes, PaintLayerReason::ScrollContainer)
        .expect("scroll container should emit a paint layer");

    let nearby_layer = paint_layers(&output.scene.nodes)
        .into_iter()
        .find(|layer| layer.reason == PaintLayerReason::Nearby)
        .expect("nearby root should be isolated as an escape paint layer");
    assert_eq!(nearby_layer.stable_id, nearby_id.to_wire_u64());
    assert_eq!(nearby_layer.policy, PaintLayerPolicy::Cacheable);
    assert_eq!(semantic_paint_layer_count(&output.scene.nodes), 2);
}

#[test]
fn nearby_layer_bounds_include_transformed_overlay_content() {
    let host_id = NodeId::from_term_bytes(vec![5]);
    let mut tree = build_tree_with_child_frame(
        Attrs::default(),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 160.0,
            content_width: 220.0,
            content_height: 160.0,
        },
        solid_fill_attrs((20, 24, 32)),
        Frame {
            x: 48.0,
            y: 48.0,
            width: 80.0,
            height: 50.0,
            content_width: 80.0,
            content_height: 50.0,
        },
    );
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        Attrs {
            move_x: Some(40.0),
            rotate: Some(16.0),
            scale: Some(1.18),
            ..solid_fill_attrs((248, 250, 252))
        },
        Frame {
            x: 48.0,
            y: 48.0,
            width: 80.0,
            height: 50.0,
            content_width: 80.0,
            content_height: 50.0,
        },
        42,
    );
    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, false, true);
    let nearby_layer = paint_layers(&output.scene.nodes)
        .into_iter()
        .find(|layer| layer.reason == PaintLayerReason::Nearby)
        .expect("nearby root should be isolated as a paint layer");

    assert_eq!(nearby_layer.policy, PaintLayerPolicy::Cacheable);
    assert!(nearby_layer.bounds.y < 48.0, "{:?}", nearby_layer.bounds);
    assert!(
        nearby_layer.bounds.width > 120.0,
        "{:?}",
        nearby_layer.bounds
    );
}

#[test]
fn clean_nearby_fragment_cache_reuses_mounted_subtree_without_descending() {
    let host_id = NodeId::from_term_bytes(vec![5]);
    let nearby_id = NodeId::from_term_bytes(vec![42]);
    let mut tree = build_tree_with_child_frame(
        Attrs::default(),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 360.0,
            height: 240.0,
            content_width: 360.0,
            content_height: 240.0,
        },
        solid_fill_attrs((20, 24, 32)),
        Frame {
            x: 64.0,
            y: 72.0,
            width: 120.0,
            height: 48.0,
            content_width: 120.0,
            content_height: 48.0,
        },
    );
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((248, 250, 252)),
        Frame {
            x: 64.0,
            y: 72.0,
            width: 220.0,
            height: 180.0,
            content_width: 220.0,
            content_height: 180.0,
        },
        42,
    );

    let child_ids: Vec<NodeId> = (0u8..24)
        .map(|index| {
            let id = NodeId::from_term_bytes(vec![100 + index]);
            let mut child = Element::with_attrs(
                id,
                ElementKind::El,
                Vec::new(),
                solid_fill_attrs((80 + index, 120, 180)),
            );
            child.layout.frame = Some(Frame {
                x: 72.0,
                y: 80.0 + f32::from(index) * 5.0,
                width: 160.0,
                height: 4.0,
                content_width: 160.0,
                content_height: 4.0,
            });
            tree.insert(child);
            id
        })
        .collect();
    tree.get_mut(&nearby_id).unwrap().children = child_ids;
    tree.clear_refresh_dirty();

    super::super::reset_render_traversal_diagnostics_for_benchmark();
    let first_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, false, true);
    let first = super::super::take_render_traversal_diagnostics_for_benchmark();
    assert!(
        paint_layers(&first_output.scene.nodes)
            .iter()
            .any(|layer| layer.reason == PaintLayerReason::Nearby)
    );

    super::super::reset_render_traversal_diagnostics_for_benchmark();
    let second_output = super::super::render_tree_scene_with_paint_layer_policy(&tree, false, true);
    let second = super::super::take_render_traversal_diagnostics_for_benchmark();

    assert_eq!(
        semantic_paint_layer_count(&first_output.scene.nodes),
        semantic_paint_layer_count(&second_output.scene.nodes)
    );
    assert!(
        first.element_visits >= 26,
        "first render should visit the mounted nearby subtree, got {first:?}"
    );
    assert!(
        second.element_visits <= 3,
        "clean render should reuse the nearby fragment instead of visiting descendants, got {second:?}"
    );
}

fn dynamic_paint_layer_ids(nodes: &[RenderNode]) -> Vec<u64> {
    nodes
        .iter()
        .flat_map(|node| match node {
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Transform { children, .. }
            | RenderNode::Alpha { children, .. } => dynamic_paint_layer_ids(children),
            RenderNode::PaintLayer(layer) if layer.policy == PaintLayerPolicy::DynamicRedraw => {
                let mut ids = vec![layer.stable_id];
                ids.extend(dynamic_paint_layer_ids(&layer.content_nodes()));
                ids
            }
            RenderNode::PaintLayer(layer) => dynamic_paint_layer_ids(&layer.content_nodes()),
            RenderNode::Primitive(_) => Vec::new(),
        })
        .collect()
}

fn paint_layer_by_reason(
    nodes: &[RenderNode],
    reason: PaintLayerReason,
) -> Option<&RenderPaintLayer> {
    paint_layers(nodes)
        .into_iter()
        .find(|layer| layer.reason == reason)
}

fn paint_layers(nodes: &[RenderNode]) -> Vec<&RenderPaintLayer> {
    nodes
        .iter()
        .flat_map(|node| match node {
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Transform { children, .. }
            | RenderNode::Alpha { children, .. } => paint_layers(children),
            RenderNode::PaintLayer(layer) => {
                let mut layers = vec![layer];
                layers.extend(paint_layers(&layer.own_nodes));
                layer
                    .child_refs
                    .iter()
                    .for_each(|child| layers.extend(paint_layers(&child.nodes)));
                layers
            }
            RenderNode::Primitive(_) => Vec::new(),
        })
        .collect()
}

fn dynamic_slot_count(nodes: &[RenderNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            RenderNode::ShadowPass { children }
            | RenderNode::Clip { children, .. }
            | RenderNode::RelaxedClip { children, .. }
            | RenderNode::Transform { children, .. }
            | RenderNode::Alpha { children, .. } => dynamic_slot_count(children),
            RenderNode::PaintLayer(_) => 1,
            RenderNode::Primitive(_) => 0,
        })
        .sum()
}

fn contains_shadow_primitive(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => contains_shadow_primitive(children),
        RenderNode::PaintLayer(layer) => {
            contains_shadow_primitive(&layer.own_nodes)
                || layer
                    .child_refs
                    .iter()
                    .any(|child| contains_shadow_primitive(&child.nodes))
        }
        RenderNode::Primitive(DrawPrimitive::Shadow(..)) => true,
        RenderNode::Primitive(_) => false,
    })
}

fn contains_rect_color(nodes: &[RenderNode], expected: u32) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => contains_rect_color(children, expected),
        RenderNode::PaintLayer(layer) => {
            contains_rect_color(&layer.own_nodes, expected)
                || layer
                    .child_refs
                    .iter()
                    .any(|child| contains_rect_color(&child.nodes, expected))
        }
        RenderNode::Primitive(DrawPrimitive::Rect(_, _, _, _, color)) => *color == expected,
        RenderNode::Primitive(_) => false,
    })
}

fn contains_text_primitive(nodes: &[RenderNode]) -> bool {
    nodes.iter().any(|node| match node {
        RenderNode::ShadowPass { children }
        | RenderNode::Clip { children, .. }
        | RenderNode::RelaxedClip { children, .. }
        | RenderNode::Transform { children, .. }
        | RenderNode::Alpha { children, .. } => contains_text_primitive(children),
        RenderNode::PaintLayer(layer) => {
            contains_text_primitive(&layer.own_nodes)
                || layer
                    .child_refs
                    .iter()
                    .any(|child| contains_text_primitive(&child.nodes))
        }
        RenderNode::Primitive(DrawPrimitive::TextWithFont(..)) => true,
        RenderNode::Primitive(_) => false,
    })
}

fn semantic_paint_layer_count(nodes: &[RenderNode]) -> usize {
    paint_layers(nodes)
        .into_iter()
        .filter(|layer| layer.reason != PaintLayerReason::Root)
        .count()
}
