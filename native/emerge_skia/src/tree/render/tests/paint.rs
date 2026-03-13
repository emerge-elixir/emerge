use super::super::paint::SCROLLBAR_COLOR;
use super::common::*;
use super::*;

#[test]
fn test_render_image_source_pending_emits_loading_placeholder() {
    let id = ElementId::from_term_bytes(vec![9]);
    let mut attrs = Attrs::default();
    attrs.image_src = Some(ImageSource::Logical("images/photo.jpg".to_string()));
    attrs.image_fit = Some(ImageFit::Contain);

    let mut element = Element::with_attrs(id.clone(), ElementKind::Image, Vec::new(), attrs);
    element.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 90.0,
        content_width: 120.0,
        content_height: 90.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);

    let commands = render_tree(&tree);

    assert!(
        commands
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::ImageLoading(_, _, _, _)))
    );
}

#[test]
fn test_render_scrollbar_y_thumb() {
    let mut attrs = Attrs::default();
    attrs.scrollbar_y = Some(true);
    attrs.scroll_y = Some(50.0);
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 150.0,
    };
    let tree = build_tree_with_frame(attrs, frame);
    let commands = render_tree(&tree);

    assert_eq!(
        commands,
        vec![
            DrawCmd::RoundedRect(0.0, 0.0, 100.0, 50.0, 8.0, 0x000000FF),
            DrawCmd::PushClipRounded(0.0, 0.0, 100.0, 50.0, 8.0),
            DrawCmd::RoundedRect(95.0, 13.0, 5.0, 24.0, 2.5, SCROLLBAR_COLOR),
            DrawCmd::PopClip,
        ]
    );

    assert!(!commands.iter().any(|cmd| match cmd {
        DrawCmd::PushClip(_, _, w, h) => *w > 10_000.0 || *h > 10_000.0,
        _ => false,
    }));
}

#[test]
fn test_render_scrollbar_x_thumb() {
    let mut attrs = Attrs::default();
    attrs.scrollbar_x = Some(true);
    attrs.scroll_x = Some(30.0);
    attrs.border_radius = Some(BorderRadius::Corners {
        tl: 4.0,
        tr: 6.0,
        br: 12.0,
        bl: 8.0,
    });
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 40.0,
        content_width: 160.0,
        content_height: 40.0,
    };
    let tree = build_tree_with_frame(attrs, frame);
    let commands = render_tree(&tree);

    assert_eq!(
        commands,
        vec![
            DrawCmd::RoundedRectCorners(0.0, 0.0, 80.0, 40.0, 4.0, 6.0, 12.0, 8.0, 0x000000FF),
            DrawCmd::PushClipRoundedCorners(0.0, 0.0, 80.0, 40.0, 4.0, 6.0, 12.0, 8.0),
            DrawCmd::RoundedRect(15.0, 35.0, 40.0, 5.0, 2.5, SCROLLBAR_COLOR),
            DrawCmd::PopClip,
        ]
    );
}

#[test]
fn test_render_scrollbar_hover_uses_wider_thumb() {
    let mut attrs = Attrs::default();
    attrs.scrollbar_y = Some(true);
    attrs.scroll_y = Some(50.0);
    attrs.scrollbar_hover_axis = Some(crate::tree::attrs::ScrollbarHoverAxis::Y);
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 150.0,
    };
    let tree = build_tree_with_frame(attrs, frame);
    let commands = render_tree(&tree);

    assert!(commands.contains(&DrawCmd::RoundedRect(
        93.0,
        13.0,
        7.0,
        24.0,
        3.5,
        SCROLLBAR_COLOR,
    )));
}

#[test]
fn test_render_border_uniform_emits_border_cmd() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_color = Some(Color::Named("red".to_string()));
    attrs.border_radius = Some(BorderRadius::Uniform(4.0));

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    assert!(commands.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::Border(_, _, _, _, 4.0, 2.0, 0xFF0000FF, BorderStyle::Solid)
    )));
}

#[test]
fn test_render_border_edges_emits_border_edges_cmd() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    });
    attrs.border_color = Some(Color::Named("red".to_string()));

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    assert!(commands.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::BorderEdges(
            _,
            _,
            _,
            _,
            _,
            1.0,
            2.0,
            3.0,
            4.0,
            0xFF0000FF,
            BorderStyle::Solid
        )
    )));
}

#[test]
fn test_render_border_dashed_passes_style() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_style = Some(BorderStyle::Dashed);
    attrs.border_color = Some(Color::Named("white".to_string()));

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    assert!(commands.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::Border(_, _, _, _, _, 2.0, 0xFFFFFFFF, BorderStyle::Dashed)
    )));
}

#[test]
fn test_render_shadow_emits_before_background() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Named("white".to_string())));
    attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let shadow_idx = commands
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Shadow(..)))
        .expect("shadow should exist");
    let bg_idx = commands
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Rect(..) | DrawCmd::RoundedRect(..)))
        .expect("background should exist");

    assert!(
        shadow_idx < bg_idx,
        "shadow should render before background"
    );
}

#[test]
fn test_render_inset_shadow_emits_after_background() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Named("white".to_string())));
    attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 10.0,
        size: 0.0,
        color: Color::Named("black".to_string()),
        inset: true,
    }]);

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let bg_idx = commands
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Rect(..) | DrawCmd::RoundedRect(..)))
        .expect("background should exist");
    let inset_idx = commands
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::InsetShadow(..)))
        .expect("inset shadow should exist");

    assert!(
        inset_idx > bg_idx,
        "inset shadow should render after background"
    );
}

#[test]
fn test_render_no_border_without_color() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    // No border_color set

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    assert!(
        !commands
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Border(..) | DrawCmd::BorderEdges(..)))
    );
}

#[test]
fn test_render_gradient_with_rounded_corners_emits_radius() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb {
            r: 67,
            g: 97,
            b: 238,
        },
        to: Color::Rgb {
            r: 114,
            g: 9,
            b: 183,
        },
        angle: 135.0,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(10.0));

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let gradient = commands
        .iter()
        .find(|cmd| matches!(cmd, DrawCmd::Gradient(..)))
        .expect("gradient command should exist");

    match gradient {
        DrawCmd::Gradient(_, _, _, _, _, _, _, radius) => {
            assert_eq!(*radius, 10.0, "gradient should carry the border radius");
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_gradient_without_radius_emits_zero() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb { r: 0, g: 0, b: 0 },
        to: Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        angle: 90.0,
    });
    // No border_radius set

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let gradient = commands
        .iter()
        .find(|cmd| matches!(cmd, DrawCmd::Gradient(..)))
        .expect("gradient command should exist");

    match gradient {
        DrawCmd::Gradient(_, _, _, _, _, _, _, radius) => {
            assert_eq!(
                *radius, 0.0,
                "gradient without border_radius should have radius 0"
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_gradient_with_per_corner_radius_emits_zero() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb { r: 0, g: 0, b: 0 },
        to: Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        angle: 0.0,
    });
    attrs.border_radius = Some(BorderRadius::Corners {
        tl: 10.0,
        tr: 5.0,
        br: 10.0,
        bl: 5.0,
    });

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let gradient = commands
        .iter()
        .find(|cmd| matches!(cmd, DrawCmd::Gradient(..)))
        .expect("gradient command should exist");

    // Per-corner radius falls back to 0 via border_radius_uniform
    match gradient {
        DrawCmd::Gradient(_, _, _, _, _, _, _, radius) => {
            assert_eq!(
                *radius, 0.0,
                "per-corner radius should fall back to 0 for gradient"
            );
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_border_edges_asymmetric_widths() {
    // Regression: thick top/bottom, thin sides should emit correct per-edge widths
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 4.0,
        right: 1.0,
        bottom: 4.0,
        left: 1.0,
    });
    attrs.border_color = Some(Color::Rgb {
        r: 120,
        g: 200,
        b: 160,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let edges_cmd = commands
        .iter()
        .find(|cmd| matches!(cmd, DrawCmd::BorderEdges(..)))
        .expect("BorderEdges command should exist");

    match edges_cmd {
        DrawCmd::BorderEdges(_, _, _, _, radius, top, right, bottom, left, _, _) => {
            assert_eq!(*top, 4.0);
            assert_eq!(*right, 1.0);
            assert_eq!(*bottom, 4.0);
            assert_eq!(*left, 1.0);
            assert_eq!(*radius, 8.0, "border radius should be passed through");
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_border_edges_bottom_only() {
    // Regression: bottom-only border should emit BorderEdges with zero for other sides
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 0.0,
        right: 0.0,
        bottom: 3.0,
        left: 0.0,
    });
    attrs.border_color = Some(Color::Rgb {
        r: 200,
        g: 180,
        b: 100,
    });

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    let edges_cmd = commands
        .iter()
        .find(|cmd| matches!(cmd, DrawCmd::BorderEdges(..)))
        .expect("BorderEdges command should exist for bottom-only border");

    match edges_cmd {
        DrawCmd::BorderEdges(_, _, _, _, radius, top, right, bottom, left, _, _) => {
            assert_eq!(*top, 0.0);
            assert_eq!(*right, 0.0);
            assert_eq!(*bottom, 3.0);
            assert_eq!(*left, 0.0);
            assert_eq!(*radius, 0.0, "no border radius set");
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_border_edges_with_style() {
    // Per-edge borders should forward the border style
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 2.0,
        right: 2.0,
        bottom: 2.0,
        left: 2.0,
    });
    attrs.border_color = Some(Color::Named("white".to_string()));
    attrs.border_style = Some(BorderStyle::Dashed);

    let tree = build_tree_with_attrs(attrs);
    let commands = render_tree(&tree);

    assert!(commands.iter().any(|cmd| matches!(
        cmd,
        DrawCmd::BorderEdges(
            _,
            _,
            _,
            _,
            _,
            2.0,
            2.0,
            2.0,
            2.0,
            0xFFFFFFFF,
            BorderStyle::Dashed
        )
    )));
}
