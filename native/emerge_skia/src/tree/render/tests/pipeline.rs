use super::common::*;
use super::*;
use crate::tree::geometry::{ClipShape, CornerRadii, Rect};
use crate::tree::transform::{Affine2, element_transform};

fn build_two_child_tree(
    root_attrs: Attrs,
    root_frame: Frame,
    left_attrs: Attrs,
    left_frame: Frame,
    right_attrs: Attrs,
    right_frame: Frame,
) -> ElementTree {
    let root_id = NodeId::from_term_bytes(vec![200]);
    let left_id = NodeId::from_term_bytes(vec![201]);
    let right_id = NodeId::from_term_bytes(vec![202]);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![left_id, right_id];
    root.layout.frame = Some(root_frame);

    let mut left = Element::with_attrs(left_id, ElementKind::El, Vec::new(), left_attrs);
    left.layout.frame = Some(left_frame);

    let mut right = Element::with_attrs(right_id, ElementKind::El, Vec::new(), right_attrs);
    right.layout.frame = Some(right_frame);

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(left);
    tree.insert(right);
    tree
}

fn build_nested_child_tree(
    mut root_attrs: Attrs,
    root_frame: Frame,
    mut parent_attrs: Attrs,
    parent_frame: Frame,
    mut child_attrs: Attrs,
    child_frame: Frame,
) -> ElementTree {
    if root_attrs.background.is_none() {
        root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    }

    if parent_attrs.background.is_none() {
        parent_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    }

    if child_attrs.background.is_none() {
        child_attrs.background = Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }));
    }

    let root_id = NodeId::from_term_bytes(vec![210]);
    let parent_id = NodeId::from_term_bytes(vec![211]);
    let child_id = NodeId::from_term_bytes(vec![212]);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![parent_id];
    root.layout.frame = Some(root_frame);

    let mut parent = Element::with_attrs(parent_id, ElementKind::El, Vec::new(), parent_attrs);
    parent.children = vec![child_id];
    parent.layout.frame = Some(parent_frame);

    let mut child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), child_attrs);
    child.layout.frame = Some(child_frame);

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(parent);
    tree.insert(child);
    tree
}

fn build_manual_scroll_row_tree(row_count: usize) -> ElementTree {
    let root_id = NodeId::from_u64(800_000);
    let content_id = NodeId::from_u64(800_001);
    let row_height = 10.0;

    let mut root_attrs = solid_fill_attrs((240, 240, 240));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(500.0);
    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.layout.scroll_y = 500.0;
    root.layout.scroll_y_max = 1_000.0;
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: row_count as f32 * row_height,
    });
    root.children = vec![content_id];

    let mut content = Element::with_attrs(
        content_id,
        ElementKind::Column,
        Vec::new(),
        Attrs::default(),
    );
    content.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: row_count as f32 * row_height,
        content_width: 100.0,
        content_height: row_count as f32 * row_height,
    });

    let row_ids: Vec<_> = (0..row_count)
        .map(|index| NodeId::from_u64(801_000 + index as u64))
        .collect();
    content.children = row_ids.clone();

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(content);
    for (index, row_id) in row_ids.into_iter().enumerate() {
        let mut row = Element::with_attrs(
            row_id,
            ElementKind::El,
            Vec::new(),
            solid_fill_attrs((255, 255, 255)),
        );
        row.layout.frame = Some(Frame {
            x: 0.0,
            y: index as f32 * row_height,
            width: 100.0,
            height: row_height,
            content_width: 100.0,
            content_height: row_height,
        });
        tree.insert(row);
    }
    tree
}

#[test]
fn test_scroll_viewport_culling_skips_offscreen_child_roots_before_render_visit() {
    let tree = build_manual_scroll_row_tree(120);

    super::super::reset_render_traversal_diagnostics_for_benchmark();
    let output = super::super::render_tree_scene(&tree);
    let diagnostics = super::super::take_render_traversal_diagnostics_for_benchmark();

    assert!(
        diagnostics.element_visits < 20,
        "expected only visible row roots to be visited, got {:?}",
        diagnostics
    );
    assert!(
        diagnostics.culled_subtrees > 100,
        "expected offscreen rows to be culled before traversal, got {:?}",
        diagnostics
    );
    assert!(
        !output.scene.nodes.is_empty(),
        "visible rows should still produce a scene"
    );
}

#[test]
fn test_render_skips_child_fully_outside_inherited_clip() {
    let tree = build_tree_with_child_frame(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 120.0,
        },
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 80.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );

    let draws = observe_tree(&tree);
    assert_eq!(
        matching_draws(&draws, |draw| matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 80.0, 20.0, 10.0, 0xFF0000FF)
        ))
        .len(),
        0,
        "fully clipped child should not contribute render primitives"
    );
}

#[test]
fn test_render_keeps_shadow_overflow_that_reaches_inherited_clip() {
    let mut child_attrs = solid_fill_attrs((255, 255, 255));
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: -20.0,
        blur: 0.0,
        size: 0.0,
        color: Color::Rgb { r: 255, g: 0, b: 0 },
        inset: false,
    }]);

    let tree = build_tree_with_child_frame(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 120.0,
        },
        child_attrs,
        Frame {
            x: 0.0,
            y: 60.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );

    let draws = observe_tree(&tree);
    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Shadow(0.0, 60.0, 20.0, 10.0, 0.0, -20.0, 0.0, 0.0, 0.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_keeps_transformed_child_that_reaches_inherited_clip() {
    let mut child_attrs = solid_fill_attrs((0, 255, 0));
    child_attrs.move_y = Some(-40.0);

    let tree = build_tree_with_child_frame(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 120.0,
        },
        child_attrs,
        Frame {
            x: 0.0,
            y: 80.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );

    let draws = observe_tree(&tree);
    let child = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 80.0, 20.0, 10.0, 0x00FF00FF)
        )
    });
    assert_eq!(child.cumulative_transform, Affine2::translation(0.0, -40.0));
}

#[test]
fn test_render_nested_wrapper_children_use_host_clips() {
    let root_id = NodeId::from_term_bytes(vec![40]);
    let column_id = NodeId::from_term_bytes(vec![41]);
    let text_holder_id = NodeId::from_term_bytes(vec![42]);
    let text_id = NodeId::from_term_bytes(vec![43]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 20,
        g: 20,
        b: 40,
    }));
    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![column_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 220.0,
        height: 120.0,
        content_width: 220.0,
        content_height: 120.0,
    });

    let mut column =
        Element::with_attrs(column_id, ElementKind::Column, Vec::new(), Attrs::default());
    column.children = vec![text_holder_id];
    column.layout.frame = Some(Frame {
        x: 16.0,
        y: 14.0,
        width: 180.0,
        height: 60.0,
        content_width: 180.0,
        content_height: 60.0,
    });

    let mut holder_attrs = Attrs::default();
    holder_attrs.background = Some(Background::Color(Color::Rgb {
        r: 60,
        g: 50,
        b: 80,
    }));
    let mut text_holder =
        Element::with_attrs(text_holder_id, ElementKind::El, Vec::new(), holder_attrs);
    text_holder.children = vec![text_id];
    text_holder.layout.frame = Some(Frame {
        x: 16.0,
        y: 14.0,
        width: 180.0,
        height: 40.0,
        content_width: 180.0,
        content_height: 40.0,
    });

    let mut text_attrs = Attrs::default();
    text_attrs.content = Some("Overview".to_string());
    text_attrs.font_size = Some(22.0);
    text_attrs.font_color = Some(Color::Named("white".to_string()));
    let mut text = Element::with_attrs(text_id, ElementKind::Text, Vec::new(), text_attrs);
    text.layout.frame = Some(Frame {
        x: 24.0,
        y: 22.0,
        width: 100.0,
        height: 28.0,
        content_width: 100.0,
        content_height: 28.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(column);
    tree.insert(text_holder);
    tree.insert(text);

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let text_draw = only_draw(
        draws,
        |draw| matches!(&draw.primitive, DrawPrimitive::TextWithFont(_, _, text, _, _, _, _, _) if text == "Overview"),
    );
    let clip_scopes = clip_scope_chain(&trace, text_draw);
    assert_eq!(
        clip_scopes.len(),
        4,
        "nested hosts should contribute distinct clip scopes"
    );
    assert_eq!(
        clip_scope_shapes(clip_scopes[0]).expect("root clip scope should expose its shape"),
        &[ClipShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 120.0,
            },
            radii: None,
        }]
    );
    assert_eq!(
        clip_scope_shapes(clip_scopes[1]).expect("column clip scope should expose its shape"),
        &[
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 220.0,
                    height: 120.0,
                },
                radii: None,
            },
            ClipShape {
                rect: Rect {
                    x: 16.0,
                    y: 14.0,
                    width: 180.0,
                    height: 60.0,
                },
                radii: None,
            },
        ]
    );
    assert_eq!(
        clip_scope_shapes(clip_scopes[2]).expect("holder clip scope should expose its shape"),
        &[
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 220.0,
                    height: 120.0,
                },
                radii: None,
            },
            ClipShape {
                rect: Rect {
                    x: 16.0,
                    y: 14.0,
                    width: 180.0,
                    height: 60.0,
                },
                radii: None,
            },
            ClipShape {
                rect: Rect {
                    x: 16.0,
                    y: 14.0,
                    width: 180.0,
                    height: 40.0,
                },
                radii: None,
            },
        ]
    );
    assert_eq!(
        clip_scope_shapes(clip_scopes[3]).expect("text clip scope should expose its shape"),
        &[ClipShape {
            rect: Rect {
                x: 24.0,
                y: 22.0,
                width: 100.0,
                height: 28.0,
            },
            radii: None,
        }]
    );
}

#[test]
fn test_render_transformed_children_stay_inside_parent_host_clip() {
    let root_id = NodeId::from_term_bytes(vec![65]);
    let left_id = NodeId::from_term_bytes(vec![66]);
    let right_id = NodeId::from_term_bytes(vec![67]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 20,
        g: 20,
        b: 40,
    }));
    let mut root = Element::with_attrs(root_id, ElementKind::Row, Vec::new(), root_attrs);
    root.children = vec![left_id, right_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 220.0,
        height: 60.0,
        content_width: 220.0,
        content_height: 60.0,
    });

    let mut left_attrs = Attrs::default();
    left_attrs.background = Some(Background::Color(Color::Rgb {
        r: 50,
        g: 70,
        b: 90,
    }));
    left_attrs.rotate = Some(-6.0);
    left_attrs.alpha = Some(0.85);
    let mut left = Element::with_attrs(left_id, ElementKind::El, Vec::new(), left_attrs);
    left.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 104.0,
        height: 60.0,
        content_width: 104.0,
        content_height: 60.0,
    });
    let left_transform = element_transform(
        left.layout.frame.expect("left frame"),
        &left.layout.effective,
    );

    let mut right_attrs = Attrs::default();
    right_attrs.background = Some(Background::Color(Color::Rgb {
        r: 70,
        g: 60,
        b: 90,
    }));
    right_attrs.scale = Some(1.06);
    right_attrs.move_y = Some(-14.0);
    let mut right = Element::with_attrs(right_id, ElementKind::El, Vec::new(), right_attrs);
    right.layout.frame = Some(Frame {
        x: 116.0,
        y: 0.0,
        width: 104.0,
        height: 60.0,
        content_width: 104.0,
        content_height: 60.0,
    });
    let right_transform = element_transform(
        right.layout.frame.expect("right frame"),
        &right.layout.effective,
    );

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(left);
    tree.insert(right);

    let trace = trace_tree(&tree);
    let draws = &trace.draws;
    let expected_root_clip = ClipShape {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 60.0,
        },
        radii: None,
    };

    let left_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 104.0, 60.0, 0x32465AFF)
        )
    });
    let right_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(116.0, 0.0, 104.0, 60.0, 0x463C5AFF)
        )
    });

    assert_eq!(left_draw.cumulative_transform, left_transform);
    assert_eq!(right_draw.cumulative_transform, right_transform);

    let left_scopes = scope_chain(&trace, left_draw);
    assert!(matches!(left_scopes[0].kind, ScopeKind::Alpha { alpha } if alpha == 0.85));
    assert!(matches!(left_scopes[1].kind, ScopeKind::Clip { .. }));
    assert_eq!(
        clip_scope_shapes(left_scopes[1]).unwrap(),
        &[expected_root_clip]
    );
    assert!(
        matches!(left_scopes[2].kind, ScopeKind::Transform { transform } if transform == left_transform)
    );
    assert_eq!(
        left_draw.clips[0].transform_at_application,
        Affine2::identity()
    );

    let right_scopes = scope_chain(&trace, right_draw);
    assert!(matches!(right_scopes[0].kind, ScopeKind::Clip { .. }));
    assert_eq!(
        clip_scope_shapes(right_scopes[0]).unwrap(),
        &[expected_root_clip]
    );
    assert!(
        matches!(right_scopes[1].kind, ScopeKind::Transform { transform } if transform == right_transform)
    );
    assert_eq!(
        right_draw.clips[0].transform_at_application,
        Affine2::identity()
    );
}

#[test]
fn test_render_rounded_parent_clips_child_background_corners() {
    let root_id = NodeId::from_term_bytes(vec![68]);
    let child_id = NodeId::from_term_bytes(vec![69]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    }));
    root_attrs.border_radius = Some(BorderRadius::Uniform(12.0));
    let mut root = Element::with_attrs(root_id, ElementKind::Column, Vec::new(), root_attrs);
    root.children = vec![child_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 365.0,
        height: 160.0,
        content_width: 365.0,
        content_height: 160.0,
    });

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgb {
        r: 240,
        g: 237,
        b: 248,
    }));
    let mut child = Element::with_attrs(child_id, ElementKind::Row, Vec::new(), child_attrs);
    child.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 365.0,
        height: 80.0,
        content_width: 365.0,
        content_height: 80.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    let trace = trace_tree(&tree);
    let draws = &trace.draws;
    let child_rect = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 365.0, 80.0, 0xF0EDF8FF)
        )
    });

    let root_clip_scope = clip_scope_chain(&trace, child_rect)
        .into_iter()
        .next()
        .expect("child background should retain its parent clip scope");
    assert_eq!(
        clip_scope_shapes(root_clip_scope).unwrap(),
        &[ClipShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 365.0,
                height: 160.0,
            },
            radii: Some(CornerRadii {
                tl: 12.0,
                tr: 12.0,
                br: 12.0,
                bl: 12.0,
            }),
        }]
    );
}

#[test]
fn test_nearby_position_calculations() {
    let parent = Frame {
        x: 100.0,
        y: 100.0,
        width: 200.0,
        height: 50.0,
        content_width: 200.0,
        content_height: 50.0,
    };
    let nearby = Frame {
        x: 0.0,
        y: 0.0,
        width: 50.0,
        height: 20.0,
        content_width: 50.0,
        content_height: 20.0,
    };
    let default_x = AlignX::Left;
    let default_y = AlignY::Top;

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::Above, default_x, default_y);
    assert_eq!(x, 100.0);
    assert_eq!(y, 80.0);

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::Below, default_x, default_y);
    assert_eq!(x, 100.0);
    assert_eq!(y, 150.0);

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::OnLeft, default_x, default_y);
    assert_eq!(x, 50.0);
    assert_eq!(y, 100.0);

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::OnRight, default_x, default_y);
    assert_eq!(x, 300.0);
    assert_eq!(y, 100.0);

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::InFront, default_x, default_y);
    assert_eq!(x, 100.0);
    assert_eq!(y, 100.0);

    let (x, y) = nearby_origin(
        parent,
        nearby,
        NearbySlot::BehindContent,
        default_x,
        default_y,
    );
    assert_eq!(x, 100.0);
    assert_eq!(y, 100.0);

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::Above, AlignX::Center, default_y);
    assert_eq!(x, 175.0);
    assert_eq!(y, 80.0);

    let (x, y) = nearby_origin(parent, nearby, NearbySlot::Below, AlignX::Right, default_y);
    assert_eq!(x, 250.0);
    assert_eq!(y, 150.0);

    let (x, y) = nearby_origin(
        parent,
        nearby,
        NearbySlot::OnLeft,
        default_x,
        AlignY::Center,
    );
    assert_eq!(x, 50.0);
    assert_eq!(y, 115.0);

    let (x, y) = nearby_origin(
        parent,
        nearby,
        NearbySlot::OnRight,
        default_x,
        AlignY::Bottom,
    );
    assert_eq!(x, 300.0);
    assert_eq!(y, 130.0);

    let (x, y) = nearby_origin(
        parent,
        nearby,
        NearbySlot::InFront,
        AlignX::Right,
        AlignY::Bottom,
    );
    assert_eq!(x, 250.0);
    assert_eq!(y, 130.0);
}

#[test]
fn test_render_emits_translate_for_move() {
    let mut attrs = Attrs::default();
    attrs.move_x = Some(10.0);
    attrs.move_y = Some(5.0);
    let expected_transform = element_transform(
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        &attrs,
    );
    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let draw = only_draw(&draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, expected_transform);
}

#[test]
fn test_render_emits_rotate_for_rotation() {
    let mut attrs = Attrs::default();
    attrs.rotate = Some(45.0);
    let expected_transform = element_transform(
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        &attrs,
    );
    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let draw = only_draw(&draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, expected_transform);
}

#[test]
fn test_render_emits_scale_for_scale() {
    let mut attrs = Attrs::default();
    attrs.scale = Some(1.1);
    let expected_transform = element_transform(
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        &attrs,
    );
    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let draw = only_draw(&draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, expected_transform);
}

#[test]
fn test_render_emits_alpha_layer() {
    let mut attrs = Attrs::default();
    attrs.alpha = Some(0.5);
    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let draw = only_draw(&draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let alpha_scopes = alpha_scope_chain(&trace, draw);
    assert_eq!(alpha_scopes.len(), 1);
    assert_eq!(alpha_scope_value(alpha_scopes[0]), Some(0.5));
}

#[test]
fn test_alpha_shadow_keeps_shadow_visible_and_alpha_reduced_inside_parent_clip() {
    let parent_id = NodeId::from_term_bytes(vec![90]);
    let child_id = NodeId::from_term_bytes(vec![91]);

    let mut parent_attrs = Attrs::default();
    parent_attrs.scrollbar_y = Some(true);

    let mut parent = Element::with_attrs(parent_id, ElementKind::El, Vec::new(), parent_attrs);
    parent.children = vec![child_id];
    parent.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 50.0,
    });

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    }));
    child_attrs.alpha = Some(0.5);
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 0.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let mut child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), child_attrs);
    child.layout.frame = Some(Frame {
        x: 20.0,
        y: 15.0,
        width: 30.0,
        height: 15.0,
        content_width: 30.0,
        content_height: 15.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(parent_id);
    tree.insert(parent);
    tree.insert(child);

    let (output, draws) = observe_output(&tree);
    let shadow_draw = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(20.0, 15.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });

    assert!(shares_alpha_scope(shadow_draw, body_draw));

    let pixels = render_scene_to_pixels(100, 50, output.scene);
    let shadow = rgba_at(&pixels, 100, 18, 22);
    let body = rgba_at(&pixels, 100, 25, 22);
    let outside = rgba_at(&pixels, 100, 15, 22);

    assert_eq!(
        outside.3, 0,
        "pixel outside the shadow halo should stay transparent"
    );
    assert!(shadow.3 > 0, "shadow halo should remain visible");
    assert!(
        shadow.3 < 255,
        "shadow halo should inherit the alpha wrapper"
    );
    assert!(body.3 > 0, "body fill should render");
    assert!(body.3 < 255, "body fill should also inherit alpha");
}

#[test]
fn test_outer_shadow_on_transparent_rounded_element_keeps_center_transparent() {
    let parent_id = NodeId::from_term_bytes(vec![12]);
    let child_id = NodeId::from_term_bytes(vec![13]);

    let mut parent = Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
    parent.children = vec![child_id];
    parent.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 50.0,
    });

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 0,
    }));
    child_attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 6.0,
        size: 2.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let mut child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), child_attrs);
    child.layout.frame = Some(Frame {
        x: 20.0,
        y: 15.0,
        width: 30.0,
        height: 15.0,
        content_width: 30.0,
        content_height: 15.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(parent_id);
    tree.insert(parent);
    tree.insert(child);

    let (_output, pixels) = render_tree_to_pixels(100, 50, &tree);
    let halo = rgba_at(&pixels, 100, 17, 22);
    let center = rgba_at(&pixels, 100, 35, 22);

    assert!(
        halo.3 > 0,
        "shadow halo should remain visible outside the element"
    );
    assert_eq!(
        center.3, 0,
        "transparent element center should not be filled by the outer shadow"
    );
}

#[test]
fn test_tree_clip_scope_does_not_clip_following_sibling_pixels() {
    let tree = build_two_child_tree(
        Attrs::default(),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 10.0,
            content_width: 40.0,
            content_height: 10.0,
        },
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            content_width: 10.0,
            content_height: 10.0,
        },
        solid_fill_attrs((0, 0, 255)),
        Frame {
            x: 20.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            content_width: 10.0,
            content_height: 10.0,
        },
    );

    let (_output, pixels) = render_tree_to_pixels(40, 10, &tree);

    assert_eq!(rgba_at(&pixels, 40, 5, 5), (255, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 40, 25, 5), (0, 0, 255, 255));
}

#[test]
fn test_tree_alpha_scope_does_not_affect_following_sibling_pixels() {
    let mut left_attrs = solid_fill_attrs((255, 0, 0));
    left_attrs.alpha = Some(0.5);

    let tree = build_two_child_tree(
        Attrs::default(),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 10.0,
            content_width: 40.0,
            content_height: 10.0,
        },
        left_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            content_width: 10.0,
            content_height: 10.0,
        },
        solid_fill_attrs((0, 0, 255)),
        Frame {
            x: 20.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            content_width: 10.0,
            content_height: 10.0,
        },
    );

    let (_output, pixels) = render_tree_to_pixels(40, 10, &tree);
    let red = rgba_at(&pixels, 40, 5, 5);
    let blue = rgba_at(&pixels, 40, 25, 5);

    assert!(red.3 > 0 && red.3 < 255);
    assert_eq!(blue, (0, 0, 255, 255));
}

#[test]
fn test_tree_transform_scope_does_not_affect_following_sibling_pixels() {
    let mut left_attrs = solid_fill_attrs((255, 0, 0));
    left_attrs.move_x = Some(10.0);

    let tree = build_two_child_tree(
        Attrs::default(),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 10.0,
            content_width: 50.0,
            content_height: 10.0,
        },
        left_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            content_width: 10.0,
            content_height: 10.0,
        },
        solid_fill_attrs((0, 0, 255)),
        Frame {
            x: 20.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            content_width: 10.0,
            content_height: 10.0,
        },
    );

    let (_output, pixels) = render_tree_to_pixels(50, 10, &tree);

    assert_eq!(rgba_at(&pixels, 50, 15, 5), (255, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 50, 25, 5), (0, 0, 255, 255));
    assert_eq!(rgba_at(&pixels, 50, 35, 5).3, 0);
}

#[test]
fn test_render_translated_full_width_row_moves_host_frame_and_children_together() {
    let root_id = NodeId::from_term_bytes(vec![220]);
    let row_id = NodeId::from_term_bytes(vec![221]);
    let child_id = NodeId::from_term_bytes(vec![222]);

    let mut root = Element::with_attrs(
        root_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((0, 0, 0)),
    );
    root.children = vec![row_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 320.0,
        height: 100.0,
        content_width: 320.0,
        content_height: 100.0,
    });

    let mut row_attrs = solid_fill_attrs((255, 255, 255));
    row_attrs.move_x = Some(30.0);
    row_attrs.border_width = Some(BorderWidth::Sides {
        top: 0.0,
        right: 0.0,
        bottom: 4.0,
        left: 0.0,
    });
    row_attrs.border_color = Some(Color::Rgb { r: 0, g: 0, b: 255 });

    let mut row = Element::with_attrs(row_id, ElementKind::Row, Vec::new(), row_attrs);
    row.children = vec![child_id];
    row.layout.frame = Some(Frame {
        x: 20.0,
        y: 30.0,
        width: 220.0,
        height: 40.0,
        content_width: 220.0,
        content_height: 40.0,
    });

    let mut child = Element::with_attrs(
        child_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((255, 0, 0)),
    );
    child.layout.frame = Some(Frame {
        x: 28.0,
        y: 42.0,
        width: 16.0,
        height: 16.0,
        content_width: 16.0,
        content_height: 16.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(row);
    tree.insert(child);

    let (_output, pixels) = render_tree_to_pixels(320, 100, &tree);

    assert_eq!(rgba_at(&pixels, 320, 30, 50), (0, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 320, 260, 50), (255, 255, 255, 255));

    assert_eq!(rgba_at(&pixels, 320, 30, 68), (0, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 320, 260, 68), (0, 0, 255, 255));

    assert_eq!(rgba_at(&pixels, 320, 34, 50), (0, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 320, 64, 50), (255, 0, 0, 255));
}

#[test]
fn test_render_skips_transform_when_default() {
    let attrs = Attrs::default();
    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let draw = only_draw(&draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, Affine2::identity());
    assert!(draw.alpha_scopes.is_empty());
}

#[test]
fn test_render_nearby_behind_and_in_front_order() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::BehindContent,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        10,
    );
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((0, 0, 255)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        11,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let behind = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 20.0, 10.0, 0xFF0000FF)
        )
    });
    let front = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 20.0, 10.0, 0x0000FFFF)
        )
    });

    assert!(paints_before(background, behind));
    assert!(paints_before(behind, front));
    assert_eq!(clip_scope_chain(&trace, behind).len(), 1);
    assert!(clip_scope_chain(&trace, front).is_empty());
}

#[test]
fn test_render_behind_between_background_and_children() {
    let mut parent_attrs = Attrs::default();
    parent_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 255, b: 0 }));

    let mut tree = build_tree_with_child_frame(
        parent_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        child_attrs,
        Frame {
            x: 10.0,
            y: 12.0,
            width: 30.0,
            height: 15.0,
            content_width: 30.0,
            content_height: 15.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::BehindContent,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        12,
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let behind = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 20.0, 10.0, 0xFF0000FF)
        )
    });
    let child = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 12.0, 30.0, 15.0, 0x00FF00FF)
        )
    });

    assert!(paints_before(background, behind));
    assert!(paints_before(behind, child));

    let behind_clip_scopes = clip_scope_chain(&trace, behind);
    let child_clip_scopes = clip_scope_chain(&trace, child);
    assert_eq!(behind_clip_scopes.len(), 1);
    assert_eq!(child_clip_scopes.len(), 1);
    assert_eq!(
        clip_scope_shapes(behind_clip_scopes[0]).unwrap(),
        clip_scope_shapes(child_clip_scopes[0]).unwrap()
    );
    assert!(!same_immediate_clip_scope(&trace, behind, child));
}

#[test]
fn test_render_behind_inside_host_clip() {
    let mut parent_attrs = Attrs::default();
    parent_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    parent_attrs.padding = Some(Padding::Uniform(10.0));

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 255, b: 0 }));

    let mut tree = build_tree_with_child_frame(
        parent_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        child_attrs,
        Frame {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::BehindContent,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        13,
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let behind = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0xFF0000FF)
        )
    });
    let child = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 10.0, 0x00FF00FF)
        )
    });

    let behind_clip_scopes = clip_scope_chain(&trace, behind);
    let child_clip_scopes = clip_scope_chain(&trace, child);
    assert_eq!(behind_clip_scopes.len(), 1);
    assert_eq!(child_clip_scopes.len(), 1);
    assert_eq!(
        clip_scope_shapes(behind_clip_scopes[0]).unwrap(),
        clip_scope_shapes(child_clip_scopes[0]).unwrap()
    );
    assert!(!same_immediate_clip_scope(&trace, behind, child));
}

#[test]
fn test_render_nearby_above_below_order_after_parent() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::Above,
        ElementKind::El,
        solid_fill_attrs((0, 255, 0)),
        Frame {
            x: 0.0,
            y: -10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        14,
    );
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::Below,
        ElementKind::El,
        solid_fill_attrs((255, 255, 0)),
        Frame {
            x: 0.0,
            y: 50.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        15,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let above = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, -10.0, 20.0, 10.0, 0x00FF00FF)
        )
    });
    let below = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 50.0, 20.0, 10.0, 0xFFFF00FF)
        )
    });

    assert!(paints_before(background, above));
    assert!(paints_before(above, below));
    assert!(above.clips.is_empty());
    assert!(below.clips.is_empty());
}

#[test]
fn test_render_front_nearby_escapes_ancestor_host_clip() {
    let mut parent_attrs = Attrs::default();
    parent_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    parent_attrs.scrollbar_y = Some(true);

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 255, b: 0 }));

    let mut tree = build_tree_with_child_frame(
        parent_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        child_attrs,
        Frame {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );
    let child_id = NodeId::from_term_bytes(vec![5]);
    mount_nearby(
        &mut tree,
        &child_id,
        NearbySlot::Above,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 10.0,
            y: -10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        22,
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let child = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 10.0, 0x00FF00FF)
        )
    });
    let nearby = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, -10.0, 20.0, 10.0, 0xFF0000FF)
        )
    });

    assert_eq!(clip_scope_chain(&trace, child).len(), 1);
    assert!(clip_scope_chain(&trace, nearby).is_empty());
    assert!(paints_before(child, nearby));
}

#[test]
fn test_render_same_host_escape_nearby_uses_definition_order_across_slots() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
            content_width: 80.0,
            content_height: 40.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
            content_width: 20.0,
            content_height: 20.0,
        },
        62,
    );
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::Above,
        ElementKind::El,
        solid_fill_attrs((0, 255, 0)),
        Frame {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
            content_width: 20.0,
            content_height: 20.0,
        },
        63,
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let first = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 20.0, 0xFF0000FF)
        )
    });
    let second = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 20.0, 0x00FF00FF)
        )
    });

    assert!(paints_before(first, second));
}

#[test]
fn test_render_clip_nearby_clips_escape_overlay() {
    let parent_attrs = Attrs::default();

    let mut child_attrs = Attrs::default();
    child_attrs.clip_nearby = Some(true);

    let mut tree = build_tree_with_child_frame(
        parent_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
            content_width: 200.0,
            content_height: 200.0,
        },
        child_attrs,
        Frame {
            x: 50.0,
            y: 50.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let child_id = NodeId::from_term_bytes(vec![5]);
    mount_nearby(
        &mut tree,
        &child_id,
        NearbySlot::Above,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 60.0,
            y: 30.0,
            width: 20.0,
            height: 30.0,
            content_width: 20.0,
            content_height: 30.0,
        },
        64,
    );

    let (_output, pixels) = render_tree_to_pixels(200, 200, &tree);

    assert_eq!(rgba_at(&pixels, 200, 65, 35), (0, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 200, 65, 55), (255, 0, 0, 255));
}

#[test]
fn test_render_earlier_child_escape_paints_after_later_normal_sibling() {
    let mut tree = build_two_child_tree(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 140.0,
            content_width: 220.0,
            content_height: 140.0,
        },
        solid_fill_attrs((30, 30, 30)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 40.0,
            content_width: 120.0,
            content_height: 40.0,
        },
        solid_fill_attrs((0, 0, 255)),
        Frame {
            x: 0.0,
            y: 48.0,
            width: 220.0,
            height: 40.0,
            content_width: 220.0,
            content_height: 40.0,
        },
    );

    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![201]),
        NearbySlot::Below,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 100.0,
            y: 48.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        65,
    );

    let trace = trace_tree(&tree);
    let blue = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 48.0, 220.0, 40.0, 0x0000FFFF)
        )
    });
    let red = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(100.0, 48.0, 60.0, 40.0, 0xFF0000FF)
        )
    });

    assert!(paints_before(blue, red));

    let (_output, pixels) = render_tree_to_pixels(220, 140, &tree);
    assert_eq!(rgba_at(&pixels, 220, 110, 60), (255, 0, 0, 255));
}

#[test]
fn test_render_ancestor_in_front_beats_descendant_below() {
    let mut tree = build_nested_child_tree(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 140.0,
            content_width: 220.0,
            content_height: 140.0,
        },
        solid_fill_attrs((20, 20, 20)),
        Frame {
            x: 60.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        },
        solid_fill_attrs((10, 10, 10)),
        Frame {
            x: 60.0,
            y: 0.0,
            width: 40.0,
            height: 20.0,
            content_width: 40.0,
            content_height: 20.0,
        },
    );

    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![210]),
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((0, 255, 0)),
        Frame {
            x: 80.0,
            y: 48.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        66,
    );
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![211]),
        NearbySlot::Below,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 80.0,
            y: 48.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        67,
    );

    let trace = trace_tree(&tree);
    let red = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(80.0, 48.0, 60.0, 40.0, 0xFF0000FF)
        )
    });
    let green = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(80.0, 48.0, 60.0, 40.0, 0x00FF00FF)
        )
    });

    assert!(paints_before(red, green));
}

#[test]
fn test_render_later_sibling_escape_beats_earlier_sibling_escape() {
    let mut tree = build_two_child_tree(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 100.0,
            content_width: 220.0,
            content_height: 100.0,
        },
        solid_fill_attrs((30, 30, 30)),
        Frame {
            x: 0.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        },
        solid_fill_attrs((30, 30, 30)),
        Frame {
            x: 120.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        },
    );

    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![201]),
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 80.0,
            y: 20.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        68,
    );
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![202]),
        NearbySlot::OnLeft,
        ElementKind::El,
        solid_fill_attrs((0, 255, 0)),
        Frame {
            x: 80.0,
            y: 20.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        69,
    );

    let trace = trace_tree(&tree);
    let red = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(80.0, 20.0, 60.0, 40.0, 0xFF0000FF)
        )
    });
    let green = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(80.0, 20.0, 60.0, 40.0, 0x00FF00FF)
        )
    });

    assert!(paints_before(red, green));
}

#[test]
fn test_render_transforms_do_not_change_escape_z_order() {
    let mut tree = build_two_child_tree(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 100.0,
            content_width: 220.0,
            content_height: 100.0,
        },
        solid_fill_attrs((30, 30, 30)),
        Frame {
            x: 0.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        },
        solid_fill_attrs((30, 30, 30)),
        Frame {
            x: 120.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        },
    );

    let mut moved_red = solid_fill_attrs((255, 0, 0));
    moved_red.move_x = Some(60.0);
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![201]),
        NearbySlot::InFront,
        ElementKind::El,
        moved_red,
        Frame {
            x: 20.0,
            y: 20.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        70,
    );
    mount_nearby(
        &mut tree,
        &NodeId::from_term_bytes(vec![202]),
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((0, 255, 0)),
        Frame {
            x: 80.0,
            y: 20.0,
            width: 60.0,
            height: 40.0,
            content_width: 60.0,
            content_height: 40.0,
        },
        71,
    );

    let (_output, pixels) = render_tree_to_pixels(220, 100, &tree);
    assert_eq!(rgba_at(&pixels, 220, 100, 40), (0, 255, 0, 255));
}

#[test]
fn test_render_nested_escape_submenu_paints_after_parent_menu() {
    let mut tree = build_tree_with_frame(
        solid_fill_attrs((0, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 260.0,
            height: 160.0,
            content_width: 260.0,
            content_height: 160.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    let menu_id = NodeId::from_term_bytes(vec![72]);
    let submenu_id = NodeId::from_term_bytes(vec![73]);

    let mut menu = Element::with_attrs(
        menu_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((255, 255, 255)),
    );
    menu.layout.frame = Some(Frame {
        x: 80.0,
        y: 40.0,
        width: 80.0,
        height: 60.0,
        content_width: 80.0,
        content_height: 60.0,
    });
    menu.nearby.push(NearbySlot::OnRight, submenu_id);

    let mut submenu = Element::with_attrs(
        submenu_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((255, 255, 0)),
    );
    submenu.layout.frame = Some(Frame {
        x: 130.0,
        y: 50.0,
        width: 60.0,
        height: 40.0,
        content_width: 60.0,
        content_height: 40.0,
    });

    tree.insert(menu);
    tree.insert(submenu);
    tree.get_mut(&host_id)
        .expect("host should exist")
        .nearby
        .push(NearbySlot::Below, menu_id);

    let trace = trace_tree(&tree);
    let menu_draw = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(80.0, 40.0, 80.0, 60.0, 0xFFFFFFFF)
        )
    });
    let submenu_draw = only_draw(&trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(130.0, 50.0, 60.0, 40.0, 0xFFFF00FF)
        )
    });

    assert!(paints_before(menu_draw, submenu_draw));
}

#[test]
fn test_render_in_front_fill_uses_parent_border_box_slot() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        16,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let front = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0xFF0000FF)
        )
    });

    assert!(paints_before(background, front));
    assert!(front.clips.is_empty());
}

#[test]
fn test_render_in_front_explicit_size_can_overflow_slot_with_alignment() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: -30.0,
            y: -30.0,
            width: 160.0,
            height: 80.0,
            content_width: 160.0,
            content_height: 80.0,
        },
        17,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(-30.0, -30.0, 160.0, 80.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_above_fill_width_uses_parent_slot() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::Above,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: -10.0,
            width: 100.0,
            height: 10.0,
            content_width: 100.0,
            content_height: 10.0,
        },
        18,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, -10.0, 100.0, 10.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_on_right_fill_height_uses_parent_slot() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::OnRight,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 100.0,
            y: 0.0,
            width: 20.0,
            height: 50.0,
            content_width: 20.0,
            content_height: 50.0,
        },
        19,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(100.0, 0.0, 20.0, 50.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_in_front_ignores_host_clip() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    attrs.padding = Some(Padding::Uniform(10.0));

    let mut tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
    );
    let host_id = tree.root_id().unwrap();
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        20,
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let front = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0xFF0000FF)
        )
    });
    assert!(front.clips.is_empty());
}

#[test]
fn test_outer_shadow_escapes_non_scrollable_ancestor_clip() {
    let parent_attrs = Attrs::default();
    let mut child_attrs = Attrs::default();
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_tree_with_child_frame(
        parent_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        child_attrs,
        Frame {
            x: 10.0,
            y: 12.0,
            width: 30.0,
            height: 15.0,
            content_width: 30.0,
            content_height: 15.0,
        },
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 12.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });

    assert!(clip_scope_chain(&trace, shadow).is_empty());
    assert_eq!(clip_scope_chain(&trace, body).len(), 1);
    assert!(paints_before(shadow, body));
}

#[test]
fn test_outer_shadow_bleeds_into_parent_padding() {
    let mut parent_attrs = Attrs::default();
    parent_attrs.padding = Some(Padding::Uniform(10.0));
    parent_attrs.background = Some(Background::Color(Color::Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }));

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    }));
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 0.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_tree_with_child_frame(
        parent_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 30.0,
            content_width: 60.0,
            content_height: 30.0,
        },
        child_attrs,
        Frame {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );

    let (_output, pixels) = render_tree_to_pixels(60, 30, &tree);
    let padding_shadow = rgba_at(&pixels, 60, 8, 14);
    let outside = rgba_at(&pixels, 60, 4, 14);

    assert!(
        padding_shadow.3 > 0,
        "outer shadow should remain visible in the parent's padding"
    );
    assert_eq!(
        outside.3, 0,
        "pixels outside the outer shadow halo should stay transparent"
    );
}

#[test]
fn test_outer_shadow_clips_only_on_vertical_scroll_axis() {
    let root_attrs = Attrs::default();
    let mut parent_attrs = Attrs::default();
    parent_attrs.scrollbar_y = Some(true);
    let mut child_attrs = Attrs::default();
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_nested_child_tree(
        root_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 120.0,
            content_width: 200.0,
            content_height: 120.0,
        },
        parent_attrs,
        Frame {
            x: 40.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
            content_width: 80.0,
            content_height: 60.0,
        },
        child_attrs,
        Frame {
            x: 50.0,
            y: 30.0,
            width: 30.0,
            height: 15.0,
            content_width: 30.0,
            content_height: 15.0,
        },
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(50.0, 30.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });

    let shadow_clip_scope = immediate_clip_scope(&trace, shadow).unwrap();
    let body_clip_scope = immediate_clip_scope(&trace, body).unwrap();
    assert_eq!(
        clip_scope_shapes(shadow_clip_scope).unwrap(),
        &[ClipShape {
            rect: Rect {
                x: 0.0,
                y: 20.0,
                width: 200.0,
                height: 60.0,
            },
            radii: None,
        }]
    );
    assert_eq!(
        clip_scope_shapes(body_clip_scope).unwrap(),
        &[
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 120.0,
                },
                radii: None,
            },
            ClipShape {
                rect: Rect {
                    x: 40.0,
                    y: 20.0,
                    width: 80.0,
                    height: 60.0,
                },
                radii: None,
            },
        ]
    );
}

#[test]
fn test_outer_shadow_clips_only_on_horizontal_scroll_axis() {
    let root_attrs = Attrs::default();
    let mut parent_attrs = Attrs::default();
    parent_attrs.scrollbar_x = Some(true);
    let mut child_attrs = Attrs::default();
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_nested_child_tree(
        root_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 120.0,
            content_width: 200.0,
            content_height: 120.0,
        },
        parent_attrs,
        Frame {
            x: 40.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
            content_width: 80.0,
            content_height: 60.0,
        },
        child_attrs,
        Frame {
            x: 50.0,
            y: 30.0,
            width: 30.0,
            height: 15.0,
            content_width: 30.0,
            content_height: 15.0,
        },
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(50.0, 30.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });

    let shadow_clip_scope = immediate_clip_scope(&trace, shadow).unwrap();
    let body_clip_scope = immediate_clip_scope(&trace, body).unwrap();
    assert_eq!(
        clip_scope_shapes(shadow_clip_scope).unwrap(),
        &[ClipShape {
            rect: Rect {
                x: 40.0,
                y: 0.0,
                width: 80.0,
                height: 120.0,
            },
            radii: None,
        }]
    );
    assert_eq!(
        clip_scope_shapes(body_clip_scope).unwrap(),
        &[
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 120.0,
                },
                radii: None,
            },
            ClipShape {
                rect: Rect {
                    x: 40.0,
                    y: 20.0,
                    width: 80.0,
                    height: 60.0,
                },
                radii: None,
            },
        ]
    );
}

#[test]
fn test_outer_shadow_reuses_full_rounded_clip_when_both_scroll_axes_enabled() {
    let root_attrs = Attrs::default();
    let mut parent_attrs = Attrs::default();
    parent_attrs.scrollbar_x = Some(true);
    parent_attrs.scrollbar_y = Some(true);
    parent_attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    let mut child_attrs = Attrs::default();
    child_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_nested_child_tree(
        root_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 120.0,
            content_width: 200.0,
            content_height: 120.0,
        },
        parent_attrs,
        Frame {
            x: 40.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
            content_width: 80.0,
            content_height: 60.0,
        },
        child_attrs,
        Frame {
            x: 50.0,
            y: 30.0,
            width: 30.0,
            height: 15.0,
            content_width: 30.0,
            content_height: 15.0,
        },
    );

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(50.0, 30.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });
    let expected_clip = ClipShape {
        rect: Rect {
            x: 40.0,
            y: 20.0,
            width: 80.0,
            height: 60.0,
        },
        radii: Some(CornerRadii {
            tl: 8.0,
            tr: 8.0,
            br: 8.0,
            bl: 8.0,
        }),
    };

    let shadow_clip_scope = immediate_clip_scope(&trace, shadow).unwrap();
    let body_clip_scope = immediate_clip_scope(&trace, body).unwrap();
    assert_eq!(
        clip_scope_shapes(shadow_clip_scope).unwrap(),
        &[expected_clip]
    );
    assert_eq!(
        clip_scope_shapes(body_clip_scope).unwrap(),
        &[
            ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 120.0,
                },
                radii: None,
            },
            expected_clip,
        ]
    );
}

#[test]
fn test_scrollable_shadowed_child_uses_screen_space_positions_without_translation() {
    let root_id = NodeId::from_term_bytes(vec![30]);
    let child_a_id = NodeId::from_term_bytes(vec![31]);
    let child_b_id = NodeId::from_term_bytes(vec![32]);
    let child_c_id = NodeId::from_term_bytes(vec![33]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(10.0);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![child_a_id, child_b_id, child_c_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 120.0,
    });

    let mut child_a = Element::with_attrs(
        child_a_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((255, 0, 0)),
    );
    child_a.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 20.0,
        content_width: 100.0,
        content_height: 20.0,
    });

    let mut child_b_attrs = solid_fill_attrs((0, 255, 0));
    child_b_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);
    let mut child_b = Element::with_attrs(child_b_id, ElementKind::El, Vec::new(), child_b_attrs);
    child_b.layout.frame = Some(Frame {
        x: 0.0,
        y: 20.0,
        width: 100.0,
        height: 20.0,
        content_width: 100.0,
        content_height: 20.0,
    });

    let mut child_c = Element::with_attrs(
        child_c_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((0, 0, 255)),
    );
    child_c.layout.frame = Some(Frame {
        x: 0.0,
        y: 40.0,
        width: 100.0,
        height: 20.0,
        content_width: 100.0,
        content_height: 20.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child_a);
    tree.insert(child_b);
    tree.insert(child_c);

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    assert!(
        draws
            .iter()
            .all(|draw| draw.cumulative_transform == Affine2::identity()),
        "scroll rendering should not need transform wrappers"
    );

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let child_c = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 30.0, 100.0, 20.0, 0x0000FFFF)
        )
    });

    assert!(paints_before(shadow, child_c));
    assert!(shadow.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                },
                radii: None,
            }
    }));
}

#[test]
fn test_nested_scroll_host_clip_uses_screen_space_geometry_without_translation() {
    let root_id = NodeId::from_term_bytes(vec![60]);
    let inner_id = NodeId::from_term_bytes(vec![61]);
    let text_id = NodeId::from_term_bytes(vec![62]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(150.0);
    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![inner_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 100.0,
        content_width: 120.0,
        content_height: 400.0,
    });

    let mut inner_attrs = Attrs::default();
    inner_attrs.background = Some(Background::Color(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    }));
    inner_attrs.scrollbar_y = Some(true);
    inner_attrs.scroll_y = Some(10.0);
    let mut inner = Element::with_attrs(inner_id, ElementKind::El, Vec::new(), inner_attrs);
    inner.children = vec![text_id];
    inner.layout.frame = Some(Frame {
        x: 10.0,
        y: 200.0,
        width: 80.0,
        height: 40.0,
        content_width: 80.0,
        content_height: 120.0,
    });

    let mut text_attrs = Attrs::default();
    text_attrs.content = Some("visible".to_string());
    text_attrs.font_size = Some(12.0);
    text_attrs.font_color = Some(Color::Named("white".to_string()));
    let mut text = Element::with_attrs(text_id, ElementKind::Text, Vec::new(), text_attrs);
    text.layout.frame = Some(Frame {
        x: 12.0,
        y: 210.0,
        width: 40.0,
        height: 16.0,
        content_width: 40.0,
        content_height: 16.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(inner);
    tree.insert(text);

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let text_draw = only_draw(
        &draws,
        |draw| matches!(&draw.primitive, DrawPrimitive::TextWithFont(_, _, text, _, _, _, _, _) if text == "visible"),
    );

    assert_eq!(text_draw.cumulative_transform, Affine2::identity());
    assert!(text_draw.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
                rect: Rect {
                    x: 10.0,
                    y: 50.0,
                    width: 80.0,
                    height: 40.0,
                },
                radii: None,
            }
    }));
    assert!(!text_draw.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
                rect: Rect {
                    x: 10.0,
                    y: 200.0,
                    width: 80.0,
                    height: 40.0,
                },
                radii: None,
            }
    }));
}

#[test]
fn test_render_scroll_host_clip_uses_current_frame_geometry() {
    let root_id = NodeId::from_term_bytes(vec![63]);
    let text_id = NodeId::from_term_bytes(vec![64]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(10.0);
    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![text_id];
    root.layout.frame = Some(Frame {
        x: 50.0,
        y: 60.0,
        width: 120.0,
        height: 40.0,
        content_width: 120.0,
        content_height: 120.0,
    });

    let mut text_attrs = Attrs::default();
    text_attrs.content = Some("shifted".to_string());
    text_attrs.font_size = Some(12.0);
    text_attrs.font_color = Some(Color::Named("white".to_string()));
    let mut text = Element::with_attrs(text_id, ElementKind::Text, Vec::new(), text_attrs);
    text.layout.frame = Some(Frame {
        x: 60.0,
        y: 80.0,
        width: 60.0,
        height: 14.0,
        content_width: 60.0,
        content_height: 14.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(text);

    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let text_draw = only_draw(
        &draws,
        |draw| matches!(&draw.primitive, DrawPrimitive::TextWithFont(_, _, text, _, _, _, _, _) if text == "shifted"),
    );
    assert!(text_draw.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
                rect: Rect {
                    x: 50.0,
                    y: 60.0,
                    width: 120.0,
                    height: 40.0,
                },
                radii: None,
            }
    }));
    assert!(!text_draw.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 120.0,
                    height: 40.0,
                },
                radii: None,
            }
    }));
}

#[test]
fn test_border_renders_after_host_clip_pops() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_color = Some(Color::Named("red".to_string()));
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    attrs.scrollbar_y = Some(true);

    let mut child_attrs = Attrs::default();
    child_attrs.background = Some(Background::Color(Color::Named("white".to_string())));

    let tree = build_tree_with_child_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 50.0,
        },
        child_attrs,
        Frame {
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let child_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 10.0, 0xFFFFFFFF)
        )
    });
    let border_draw = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Border(..))
    });
    let expected_host_clip = ClipShape {
        rect: Rect {
            x: 2.0,
            y: 2.0,
            width: 96.0,
            height: 46.0,
        },
        radii: Some(CornerRadii {
            tl: 6.0,
            tr: 6.0,
            br: 6.0,
            bl: 6.0,
        }),
    };

    let child_clip_scopes = clip_scope_chain(&trace, child_draw);
    assert!(
        child_clip_scopes
            .iter()
            .any(|scope| { clip_scope_shapes(scope).unwrap() == &[expected_host_clip] })
    );
    assert!(scope_chain(&trace, border_draw).is_empty());
    assert!(paints_before(child_draw, border_draw));
}

#[test]
fn test_render_uses_only_background_self_clip_when_nothing_else_is_clipped() {
    let mut attrs = Attrs::default();
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));

    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;
    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });

    assert_eq!(trace.scopes.len(), 1);
    assert_eq!(clip_scope_chain(&trace, background).len(), 1);
    assert_eq!(background.clips.len(), 1);
}

#[test]
fn test_host_clip_pushes_once_for_square_border() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_color = Some(Color::Named("red".to_string()));
    attrs.scrollbar_y = Some(true);
    attrs.scroll_y_max = Some(20.0);

    let tree = build_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            content_width: 100.0,
            content_height: 150.0,
        },
    );
    let trace = trace_tree(&tree);
    let expected_host_clip = ClipShape {
        rect: Rect {
            x: 2.0,
            y: 2.0,
            width: 96.0,
            height: 46.0,
        },
        radii: None,
    };

    let clip_scope_count = clip_scope_usage(
        &trace,
        |scope| matches!(clip_scope_shapes(scope), Some([clip]) if *clip == expected_host_clip),
    );

    assert_eq!(clip_scope_count, 1, "should have only one host clip scope");
}
