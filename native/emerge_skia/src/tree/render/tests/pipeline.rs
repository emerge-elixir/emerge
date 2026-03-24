use super::common::*;
use super::*;
use crate::tree::geometry::{ClipShape, CornerRadii, Rect};
use crate::tree::transform::{element_transform, Affine2};

#[test]
fn test_render_nested_wrapper_children_use_host_clips() {
    let root_id = ElementId::from_term_bytes(vec![40]);
    let column_id = ElementId::from_term_bytes(vec![41]);
    let text_holder_id = ElementId::from_term_bytes(vec![42]);
    let text_id = ElementId::from_term_bytes(vec![43]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 20,
        g: 20,
        b: 40,
    }));
    let mut root = Element::with_attrs(root_id.clone(), ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![column_id.clone()];
    root.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 220.0,
        height: 120.0,
        content_width: 220.0,
        content_height: 120.0,
    });

    let mut column = Element::with_attrs(
        column_id.clone(),
        ElementKind::Column,
        Vec::new(),
        Attrs::default(),
    );
    column.children = vec![text_holder_id.clone()];
    column.frame = Some(Frame {
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
    let mut text_holder = Element::with_attrs(
        text_holder_id.clone(),
        ElementKind::El,
        Vec::new(),
        holder_attrs,
    );
    text_holder.children = vec![text_id.clone()];
    text_holder.frame = Some(Frame {
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
    let mut text = Element::with_attrs(text_id.clone(), ElementKind::Text, Vec::new(), text_attrs);
    text.frame = Some(Frame {
        x: 24.0,
        y: 22.0,
        width: 100.0,
        height: 28.0,
        content_width: 100.0,
        content_height: 28.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(column);
    tree.insert(text_holder);
    tree.insert(text);

    let draws = observe_tree(&tree);

    let text_draw = only_draw(
        &draws,
        |draw| matches!(&draw.primitive, DrawPrimitive::TextWithFont(_, _, text, _, _, _, _, _) if text == "Overview"),
    );
    assert!(
        text_draw.clips.len() >= 3,
        "nested hosts should each clip their own content subtree"
    );
}

#[test]
fn test_render_transformed_children_stay_inside_parent_host_clip() {
    let root_id = ElementId::from_term_bytes(vec![65]);
    let left_id = ElementId::from_term_bytes(vec![66]);
    let right_id = ElementId::from_term_bytes(vec![67]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 20,
        g: 20,
        b: 40,
    }));
    let mut root = Element::with_attrs(root_id.clone(), ElementKind::Row, Vec::new(), root_attrs);
    root.children = vec![left_id.clone(), right_id.clone()];
    root.frame = Some(Frame {
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
    left.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 104.0,
        height: 60.0,
        content_width: 104.0,
        content_height: 60.0,
    });
    let left_transform = element_transform(left.frame.expect("left frame"), &left.attrs);

    let mut right_attrs = Attrs::default();
    right_attrs.background = Some(Background::Color(Color::Rgb {
        r: 70,
        g: 60,
        b: 90,
    }));
    right_attrs.scale = Some(1.06);
    right_attrs.move_y = Some(-14.0);
    let mut right = Element::with_attrs(right_id, ElementKind::El, Vec::new(), right_attrs);
    right.frame = Some(Frame {
        x: 116.0,
        y: 0.0,
        width: 104.0,
        height: 60.0,
        content_width: 104.0,
        content_height: 60.0,
    });
    let right_transform = element_transform(right.frame.expect("right frame"), &right.attrs);

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(left);
    tree.insert(right);

    let draws = observe_tree(&tree);
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
    assert!(left_draw
        .clips
        .iter()
        .any(|clip| clip.shape == expected_root_clip));
    assert!(right_draw
        .clips
        .iter()
        .any(|clip| clip.shape == expected_root_clip));
}

#[test]
fn test_render_rounded_parent_clips_child_background_corners() {
    let root_id = ElementId::from_term_bytes(vec![68]);
    let child_id = ElementId::from_term_bytes(vec![69]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    }));
    root_attrs.border_radius = Some(BorderRadius::Uniform(12.0));
    let mut root =
        Element::with_attrs(root_id.clone(), ElementKind::Column, Vec::new(), root_attrs);
    root.children = vec![child_id.clone()];
    root.frame = Some(Frame {
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
    child.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 365.0,
        height: 80.0,
        content_width: 365.0,
        content_height: 80.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(child);

    let draws = observe_tree(&tree);
    let child_rect = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 365.0, 80.0, 0xF0EDF8FF)
        )
    });

    assert!(child_rect.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
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
            }
    }));
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
    let draws = observe_tree(&tree);

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
    let draws = observe_tree(&tree);

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
    let draws = observe_tree(&tree);

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
    let draws = observe_tree(&tree);

    let draw = only_draw(&draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.alpha_scopes.len(), 1);
    assert_eq!(draw.alpha_scopes[0].alpha, 0.5);
}

#[test]
fn test_alpha_shadow_keeps_shadow_visible_and_alpha_reduced_inside_parent_clip() {
    let parent_id = ElementId::from_term_bytes(vec![90]);
    let child_id = ElementId::from_term_bytes(vec![91]);

    let mut parent_attrs = Attrs::default();
    parent_attrs.scrollbar_y = Some(true);

    let mut parent =
        Element::with_attrs(parent_id.clone(), ElementKind::El, Vec::new(), parent_attrs);
    parent.children = vec![child_id.clone()];
    parent.frame = Some(Frame {
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
    child.frame = Some(Frame {
        x: 20.0,
        y: 15.0,
        width: 30.0,
        height: 15.0,
        content_width: 30.0,
        content_height: 15.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(parent_id);
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
fn test_render_skips_transform_when_default() {
    let attrs = Attrs::default();
    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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
    assert_eq!(behind.clips.len(), 1);
    assert!(front.clips.is_empty());
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
    let host_id = tree.root.clone().unwrap();
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

    let draws = observe_tree(&tree);

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
    assert_eq!(behind.clips.len(), 1);
    assert_eq!(child.clips.len(), 1);
    assert_eq!(behind.clips[0].shape, child.clips[0].shape);
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
    let host_id = tree.root.clone().unwrap();
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

    let draws = observe_tree(&tree);

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

    assert_eq!(behind.clips.len(), 1);
    assert_eq!(child.clips.len(), 1);
    assert_eq!(behind.clips[0].shape, child.clips[0].shape);
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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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
    let child_id = ElementId::from_term_bytes(vec![5]);
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

    let draws = observe_tree(&tree);

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

    assert_eq!(child.clips.len(), 1);
    assert!(nearby.clips.is_empty());
    assert!(paints_before(child, nearby));
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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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
    let host_id = tree.root.clone().unwrap();
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
    let draws = observe_tree(&tree);

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

    let draws = observe_tree(&tree);

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 12.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });

    assert!(shadow.clips.is_empty());
    assert_eq!(body.clips.len(), 1);
    assert!(paints_before(shadow, body));
}

#[test]
fn test_outer_shadow_clips_only_on_vertical_scroll_axis() {
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

    let draws = observe_tree(&tree);

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 12.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });

    assert_eq!(shadow.clips.len(), 1);
    assert_eq!(body.clips.len(), 1);
    assert_eq!(shadow.clips[0].shape, body.clips[0].shape);
}

#[test]
fn test_outer_shadow_reuses_full_rounded_clip_when_both_scroll_axes_enabled() {
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

    let draws = observe_tree(&tree);

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 12.0, 30.0, 15.0, 0xFFFFFFFF)
        )
    });
    let expected_clip = ClipShape {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        },
        radii: Some(CornerRadii {
            tl: 8.0,
            tr: 8.0,
            br: 8.0,
            bl: 8.0,
        }),
    };

    assert_eq!(shadow.clips.len(), 1);
    assert_eq!(body.clips.len(), 1);
    assert_eq!(shadow.clips[0].shape, expected_clip);
    assert_eq!(body.clips[0].shape, expected_clip);
}

#[test]
fn test_scrollable_shadowed_child_uses_screen_space_positions_without_translation() {
    let root_id = ElementId::from_term_bytes(vec![30]);
    let child_a_id = ElementId::from_term_bytes(vec![31]);
    let child_b_id = ElementId::from_term_bytes(vec![32]);
    let child_c_id = ElementId::from_term_bytes(vec![33]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(10.0);

    let mut root = Element::with_attrs(root_id.clone(), ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![child_a_id.clone(), child_b_id.clone(), child_c_id.clone()];
    root.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 120.0,
    });

    let mut child_a = Element::with_attrs(
        child_a_id.clone(),
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((255, 0, 0)),
    );
    child_a.frame = Some(Frame {
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
    let mut child_b = Element::with_attrs(
        child_b_id.clone(),
        ElementKind::El,
        Vec::new(),
        child_b_attrs,
    );
    child_b.frame = Some(Frame {
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
    child_c.frame = Some(Frame {
        x: 0.0,
        y: 40.0,
        width: 100.0,
        height: 20.0,
        content_width: 100.0,
        content_height: 20.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(child_a);
    tree.insert(child_b);
    tree.insert(child_c);

    let draws = observe_tree(&tree);

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
    let root_id = ElementId::from_term_bytes(vec![60]);
    let inner_id = ElementId::from_term_bytes(vec![61]);
    let text_id = ElementId::from_term_bytes(vec![62]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(150.0);
    let mut root = Element::with_attrs(root_id.clone(), ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![inner_id.clone()];
    root.frame = Some(Frame {
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
    let mut inner = Element::with_attrs(inner_id.clone(), ElementKind::El, Vec::new(), inner_attrs);
    inner.children = vec![text_id.clone()];
    inner.frame = Some(Frame {
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
    text.frame = Some(Frame {
        x: 12.0,
        y: 210.0,
        width: 40.0,
        height: 16.0,
        content_width: 40.0,
        content_height: 16.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(inner);
    tree.insert(text);

    let draws = observe_tree(&tree);

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
    let root_id = ElementId::from_term_bytes(vec![63]);
    let text_id = ElementId::from_term_bytes(vec![64]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    root_attrs.scrollbar_y = Some(true);
    root_attrs.scroll_y = Some(10.0);
    let mut root = Element::with_attrs(root_id.clone(), ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![text_id.clone()];
    root.frame = Some(Frame {
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
    text.frame = Some(Frame {
        x: 60.0,
        y: 80.0,
        width: 60.0,
        height: 14.0,
        content_width: 60.0,
        content_height: 14.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(text);

    let draws = observe_tree(&tree);

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
    let draws = observe_tree(&tree);

    let child_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 10.0, 0xFFFFFFFF)
        )
    });
    let border_draw = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Border(..))
    });

    assert!(child_draw.clips.iter().any(|clip| {
        clip.shape
            == ClipShape {
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
            }
    }));
    assert!(border_draw.clips.is_empty());
    assert!(paints_before(child_draw, border_draw));
}

#[test]
fn test_render_skips_host_clip_when_nothing_uses_it() {
    let mut attrs = Attrs::default();
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    assert!(
        draws.iter().all(|draw| draw.clips.is_empty()),
        "plain hosts without clipped content should not emit host clips"
    );
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
    let draws = observe_tree(&tree);

    let clip_scope_count = unique_clip_scope_count(&draws, |clip| {
        clip.shape
            == ClipShape {
                rect: Rect {
                    x: 2.0,
                    y: 2.0,
                    width: 96.0,
                    height: 46.0,
                },
                radii: None,
            }
    });

    assert_eq!(clip_scope_count, 1, "should have only one host clip scope");
}
