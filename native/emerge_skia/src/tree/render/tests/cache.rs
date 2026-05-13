use super::common::{build_tree_with_child_frame, mount_nearby, solid_fill_attrs};
use super::*;
use crate::render_scene::{
    DrawPrimitive, PaintLayerPolicy, PaintLayerReason, RenderNode, RenderPaintLayer,
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

    assert!(dirty_ids.contains(&child_id.to_wire_u64()));
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
    let dirty_ids = dynamic_paint_layer_ids(&scroll_layer.children);

    assert!(dirty_ids.contains(&card_id.to_wire_u64()));
    assert!(dynamic_slot_count(&scroll_layer.children) > 0);
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
                ids.extend(dynamic_paint_layer_ids(&layer.children));
                ids
            }
            RenderNode::PaintLayer(layer) => dynamic_paint_layer_ids(&layer.children),
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
                layers.extend(paint_layers(&layer.children));
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

fn semantic_paint_layer_count(nodes: &[RenderNode]) -> usize {
    paint_layers(nodes)
        .into_iter()
        .filter(|layer| layer.reason != PaintLayerReason::Root)
        .count()
}
