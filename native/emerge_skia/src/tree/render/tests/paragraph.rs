use super::common::*;
use super::*;
use crate::events::registry_builder;
use crate::render_scene::RenderNode;
use crate::tree::geometry::{ClipShape, Rect};
use crate::tree::render::{box_model, collect_border_nodes, collect_box_shadow_nodes};

#[test]
fn aligned_paragraph_text_and_decorations_share_positions_and_owner_paint() {
    use crate::tree::attrs::{AlignX, Length, Padding, SolidColor};
    use crate::tree::layout::{Constraint, layout_tree_default};

    for color in [
        Color::Rgb {
            r: 30,
            g: 80,
            b: 140,
        },
        Color::Gradient {
            colors: vec![
                SolidColor::Rgb { r: 255, g: 0, b: 0 },
                SolidColor::Rgb { r: 0, g: 0, b: 255 },
            ]
            .into(),
            angle: 0.0,
        },
    ] {
        for alignment in [AlignX::Center, AlignX::Right] {
            let id = NodeId::from_u64(801);
            let text_id = NodeId::from_u64(802);
            let mut tree = ElementTree::new();
            tree.set_root_id(id);
            tree.insert(Element::with_attrs(
                id,
                ElementKind::Paragraph,
                vec![],
                Attrs {
                    width: Some(Length::Px(70.0)),
                    padding: Some(Padding::Uniform(5.0)),
                    align_x: Some(alignment),
                    font_size: Some(20.0),
                    font_color: Some(color.clone()),
                    font_underline: Some(true),
                    font_strike: Some(true),
                    on_mouse_down: Some(true),
                    ..Attrs::default()
                },
            ));
            tree.insert(Element::with_attrs(
                text_id,
                ElementKind::Text,
                vec![],
                Attrs {
                    content: Some("AA BB CC".to_string()),
                    ..Attrs::default()
                },
            ));
            tree.set_children(&id, vec![text_id]).unwrap();
            layout_tree_default(&mut tree, Constraint::new(200.0, 200.0), 1.0);
            let paragraph = tree.get(&id).unwrap();
            let frame = paragraph.layout.frame.unwrap();
            let fragments = paragraph.layout.paragraph_fragments.as_ref().unwrap();
            assert_eq!(fragments.len(), 3);
            assert!(fragments[2].y > fragments[0].y);
            assert!(fragments[0].x > 5.0);
            let expected_paint = color.render(crate::tree::geometry::Rect {
                x: frame.x + 5.0,
                y: frame.y + 5.0,
                width: frame.width - 10.0,
                height: frame.height - 10.0,
            });
            let draws = observe_tree(&tree);
            assert_eq!(draws.len(), fragments.len() * 3);
            for (fragment, group) in fragments.iter().zip(draws.as_chunks::<3>().0.iter()) {
                let DrawPrimitive::TextWithFont(x, y, text, _, paint, ..) = &group[0].primitive
                else {
                    panic!("expected text before its decorations");
                };
                assert_eq!((*x, *y), (fragment.x, fragment.y + fragment.ascent));
                assert_eq!(text, &fragment.text);
                assert_eq!(paint, &expected_paint);
                for decoration in &group[1..] {
                    let DrawPrimitive::Rect(x, _, _, _, paint) = &decoration.primitive else {
                        panic!("expected decoration rectangle");
                    };
                    assert_eq!(*x, fragment.x);
                    assert_eq!(paint, &expected_paint);
                }
            }
        }
    }
}

#[test]
fn test_render_paragraph_emits_text_commands() {
    use crate::tree::attrs::TextFragment;

    let attrs = Attrs {
        paragraph_fragments: Some(vec![
            TextFragment {
                x: 10.0,
                y: 5.0,
                text: "Hello".to_string(),
                font_size: 16.0,
                color: crate::render_color::RenderColor::Solid(0xFFFFFFFF),
                family: "default".to_string(),
                weight: 400,
                italic: false,
                underline: false,
                strike: false,
                ascent: 12.0,
            },
            TextFragment {
                x: 60.0,
                y: 5.0,
                text: "World".to_string(),
                font_size: 16.0,
                color: crate::render_color::RenderColor::Solid(0xFF0000FF),
                family: "default".to_string(),
                weight: 700,
                italic: false,
                underline: false,
                strike: false,
                ascent: 12.0,
            },
        ]),
        ..Attrs::default()
    };

    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 30.0,
        content_width: 200.0,
        content_height: 30.0,
    };

    let tree = build_paragraph_tree(attrs, frame);
    let draws = observe_tree(&tree);

    let text_cmds: Vec<(f32, f32, String, u32, u16)> = draws
        .iter()
        .filter_map(|draw| match &draw.primitive {
            DrawPrimitive::TextWithFont(x, y, text, _size, color, _family, weight, _italic) => {
                Some((
                    *x,
                    *y,
                    text.clone(),
                    color.solid().expect("solid color"),
                    *weight,
                ))
            }
            _ => None,
        })
        .collect();

    assert_eq!(text_cmds.len(), 2);
    // First fragment: x=10, baseline_y = 5 + 12 = 17
    assert_eq!(text_cmds[0].0, 10.0);
    assert_eq!(text_cmds[0].1, 17.0);
    assert_eq!(text_cmds[0].2, "Hello");
    assert_eq!(text_cmds[0].3, 0xFFFFFFFF);
    assert_eq!(text_cmds[0].4, 400);

    // Second fragment: x=60, baseline_y = 5 + 12 = 17
    assert_eq!(text_cmds[1].0, 60.0);
    assert_eq!(text_cmds[1].1, 17.0);
    assert_eq!(text_cmds[1].2, "World");
    assert_eq!(text_cmds[1].3, 0xFF0000FF);
    assert_eq!(text_cmds[1].4, 700);
}

#[test]
fn test_render_paragraph_renders_float_child_and_fragments() {
    use crate::tree::attrs::{AlignX, TextFragment};

    let para_id = NodeId::from_term_bytes(vec![10]);
    let float_id = NodeId::from_term_bytes(vec![11]);

    let para_attrs = Attrs {
        paragraph_fragments: Some(vec![TextFragment {
            x: 24.0,
            y: 8.0,
            text: "AA".to_string(),
            font_size: 16.0,
            color: crate::render_color::RenderColor::Solid(0xFFFFFFFF),
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: false,
            strike: false,
            ascent: 12.0,
        }]),
        ..Attrs::default()
    };

    let mut paragraph =
        Element::with_attrs(para_id, ElementKind::Paragraph, Vec::new(), para_attrs);
    paragraph.children = vec![float_id];
    paragraph.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 40.0,
        content_width: 120.0,
        content_height: 40.0,
    });

    let float_attrs = Attrs {
        align_x: Some(AlignX::Left),
        background: Some(Background::Color(Color::Rgb { r: 255, g: 0, b: 0 })),
        ..Attrs::default()
    };
    let mut float_el = Element::with_attrs(float_id, ElementKind::El, Vec::new(), float_attrs);
    float_el.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
        content_width: 20.0,
        content_height: 20.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(para_id);
    tree.insert(paragraph);
    tree.insert(float_el);

    let draws = observe_tree(&tree);

    let float_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(
                0.0,
                0.0,
                20.0,
                20.0,
                crate::render_color::RenderColor::Solid(0xFF0000FF)
            )
        )
    });
    let text_draw = only_draw(
        &draws,
        |draw| matches!(&draw.primitive, DrawPrimitive::TextWithFont(x, y, text, _, _, _, _, _) if *x == 24.0 && *y == 20.0 && text == "AA"),
    );

    assert!(paints_before(float_draw, text_draw));
}

#[test]
fn test_render_paragraph_rebuild_keeps_float_before_inline_event_children() {
    use crate::tree::attrs::TextFragment;

    let para_id = NodeId::from_term_bytes(vec![13]);
    let float_id = NodeId::from_term_bytes(vec![14]);
    let inline_id = NodeId::from_term_bytes(vec![15]);

    let para_attrs = Attrs {
        paragraph_fragments: Some(vec![TextFragment {
            x: 20.0,
            y: 6.0,
            text: "AA".to_string(),
            font_size: 16.0,
            color: crate::render_color::RenderColor::Solid(0xFFFFFFFF),
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: false,
            strike: false,
            ascent: 12.0,
        }]),
        ..Attrs::default()
    };
    let mut paragraph =
        Element::with_attrs(para_id, ElementKind::Paragraph, Vec::new(), para_attrs);
    paragraph.children = vec![inline_id, float_id];
    paragraph.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 40.0,
        content_width: 120.0,
        content_height: 40.0,
    });

    let float_attrs = Attrs {
        align_x: Some(AlignX::Left),
        on_mouse_down: Some(true),
        ..Attrs::default()
    };
    let mut float_el = Element::with_attrs(float_id, ElementKind::El, Vec::new(), float_attrs);
    float_el.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
        content_width: 20.0,
        content_height: 20.0,
    });

    let inline_attrs = Attrs {
        on_mouse_down: Some(true),
        ..Attrs::default()
    };
    let mut inline_el = Element::with_attrs(inline_id, ElementKind::Text, Vec::new(), inline_attrs);
    inline_el.layout.frame = Some(Frame {
        x: 24.0,
        y: 8.0,
        width: 32.0,
        height: 16.0,
        content_width: 32.0,
        content_height: 16.0,
    });

    let direct_elements = vec![paragraph.clone(), float_el.clone(), inline_el.clone()];

    let mut tree = ElementTree::new();
    tree.set_root_id(para_id);
    tree.insert(paragraph);
    tree.insert(float_el);
    tree.insert(inline_el);

    let piggyback = super::super::render_tree(&tree);
    let piggyback_press_ids: Vec<_> = piggyback
        .event_rebuild
        .base_registry
        .view()
        .iter_precedence()
        .filter(|listener| {
            listener.matcher.kind() == ListenerMatcherKind::CursorButtonLeftPressInside
        })
        .filter_map(|listener| listener.element_id)
        .collect();

    let direct_press_ids: Vec<_> = registry_builder::registry_for_elements(&direct_elements)
        .view()
        .iter_precedence()
        .filter(|listener| {
            listener.matcher.kind() == ListenerMatcherKind::CursorButtonLeftPressInside
        })
        .filter_map(|listener| listener.element_id)
        .collect();

    assert_eq!(piggyback_press_ids, vec![inline_id, float_id]);
    assert_eq!(piggyback_press_ids, direct_press_ids);
}

#[test]
fn test_render_paragraph_underline_and_strike() {
    use crate::tree::attrs::TextFragment;

    let attrs = Attrs {
        paragraph_fragments: Some(vec![TextFragment {
            x: 10.0,
            y: 5.0,
            text: "Decorated".to_string(),
            font_size: 18.0,
            color: crate::render_color::RenderColor::Solid(0x010203FF),
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: true,
            strike: true,
            ascent: 14.0,
        }]),
        ..Attrs::default()
    };

    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 30.0,
        content_width: 200.0,
        content_height: 30.0,
    };

    let tree = build_paragraph_tree(attrs, frame);
    let draws = observe_tree(&tree);

    let text_count = draws
        .iter()
        .filter(|draw| matches!(draw.primitive, DrawPrimitive::TextWithFont(..)))
        .count();
    assert_eq!(text_count, 1);

    let decoration_rects: Vec<_> = draws
        .iter()
        .filter(|draw| {
            matches!(
                draw.primitive,
                DrawPrimitive::Rect(
                    _,
                    _,
                    _,
                    _,
                    crate::render_color::RenderColor::Solid(0x010203FF)
                )
            )
        })
        .collect();
    assert_eq!(decoration_rects.len(), 2);
}

#[test]
fn test_render_paragraph_no_fragments() {
    let attrs = Attrs {
        paragraph_fragments: Some(vec![]),
        ..Attrs::default()
    };

    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 30.0,
        content_width: 200.0,
        content_height: 30.0,
    };

    let tree = build_paragraph_tree(attrs, frame);
    let draws = observe_tree(&tree);

    let text_count = draws
        .iter()
        .filter(|draw| matches!(draw.primitive, DrawPrimitive::TextWithFont(..)))
        .count();
    assert_eq!(text_count, 0);
}

#[test]
fn test_render_paragraph_with_background() {
    use crate::tree::attrs::TextFragment;

    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 128 })),
        paragraph_fragments: Some(vec![TextFragment {
            x: 0.0,
            y: 0.0,
            text: "Hi".to_string(),
            font_size: 16.0,
            color: crate::render_color::RenderColor::Solid(0xFFFFFFFF),
            family: "default".to_string(),
            weight: 400,
            italic: false,
            underline: false,
            strike: false,
            ascent: 12.0,
        }]),
        ..Attrs::default()
    };

    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 20.0,
        content_width: 100.0,
        content_height: 20.0,
    };

    let tree = build_paragraph_tree(attrs, frame);
    let draws = observe_tree(&tree);

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(
                _,
                _,
                100.0,
                20.0,
                crate::render_color::RenderColor::Solid(0x000080FF)
            )
        )
    });
    let text = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::TextWithFont(..))
    });

    assert!(
        paints_before(background, text),
        "background should render before text"
    );
}

fn inline_decoration_tree(attrs: Attrs) -> (ElementTree, NodeId, NodeId) {
    use crate::tree::attrs::{AlignX, Length, Padding};
    use crate::tree::layout::{Constraint, layout_tree_default};
    let mut tree = ElementTree::new();
    let paragraph = NodeId::from_u64(910);
    let wrapper = NodeId::from_u64(911);
    let text = NodeId::from_u64(912);
    tree.insert(Element::with_attrs(
        paragraph,
        ElementKind::Paragraph,
        vec![],
        Attrs {
            width: Some(Length::Px(120.0)),
            padding: Some(Padding::Uniform(20.0)),
            align_x: Some(AlignX::Center),
            font_size: Some(24.0),
            font_color: Some(Color::Named("white".into())),
            ..Attrs::default()
        },
    ));
    tree.insert(Element::with_attrs(wrapper, ElementKind::El, vec![], attrs));
    tree.insert(Element::with_attrs(
        text,
        ElementKind::Text,
        vec![],
        Attrs {
            content: Some("AA BB CC DD".into()),
            ..Attrs::default()
        },
    ));
    tree.set_children(&paragraph, vec![wrapper]).unwrap();
    tree.set_children(&wrapper, vec![text]).unwrap();
    tree.set_root_id(paragraph);
    layout_tree_default(&mut tree, Constraint::new(200.0, 240.0), 1.0);
    (tree, paragraph, wrapper)
}

#[test]
fn inline_decoration_matrix_reuses_complete_shared_box_paint() {
    use crate::tree::attrs::{BorderRadius, BorderStyle, BorderWidth, BoxShadow, SolidColor};
    let gradient = Color::Gradient {
        colors: vec![
            SolidColor::Rgb { r: 255, g: 0, b: 0 },
            SolidColor::Rgba {
                r: 0,
                g: 0,
                b: 255,
                a: 120,
            },
            SolidColor::Rgb { r: 0, g: 255, b: 0 },
        ]
        .into(),
        angle: 25.0,
    };
    for style in [BorderStyle::Solid, BorderStyle::Dashed, BorderStyle::Dotted] {
        for radius in [
            BorderRadius::Uniform(6.0),
            BorderRadius::Corners {
                tl: 8.0,
                tr: 1.0,
                br: 4.0,
                bl: 0.0,
            },
        ] {
            for color in [Color::Named("blue".into()), gradient.clone()] {
                let attrs = Attrs {
                    border_width: Some(BorderWidth::Sides {
                        top: 1.0,
                        right: 2.0,
                        bottom: 3.0,
                        left: 4.0,
                    }),
                    border_color: Some(color.clone()),
                    border_radius: Some(radius.clone()),
                    border_style: Some(style),
                    box_shadows: Some(
                        [false, true, false]
                            .into_iter()
                            .enumerate()
                            .map(|(i, inset)| BoxShadow {
                                offset_x: i as f64 - 1.0,
                                offset_y: 2.0,
                                blur: 2.0,
                                size: 2.0,
                                color: color.clone(),
                                inset,
                            })
                            .collect(),
                    ),
                    ..Attrs::default()
                };
                let (tree, paragraph, _) = inline_decoration_tree(attrs.clone());
                let boxes = &tree.get(&paragraph).unwrap().layout.paragraph_boxes;
                assert!(boxes.len() > 1);
                let draws = observe_tree(&tree);
                for draw in &draws {
                    if let DrawPrimitive::TextWithFont(_, _, _, _, paint, ..) = &draw.primitive {
                        assert_eq!(*paint, crate::render_color::RenderColor::Solid(0xFFFFFFFF));
                    }
                }
                let actual: Vec<_> = draws
                    .iter()
                    .filter(|d| !matches!(d.primitive, DrawPrimitive::TextWithFont(..)))
                    .map(|d| d.primitive.clone())
                    .collect();
                let expected: Vec<_> = boxes
                    .iter()
                    .flat_map(|b| {
                        collect_box_shadow_nodes(
                            b.frame,
                            &attrs,
                            attrs.border_radius.as_ref(),
                            false,
                        )
                        .into_iter()
                        .chain(collect_box_shadow_nodes(
                            b.frame,
                            &attrs,
                            attrs.border_radius.as_ref(),
                            true,
                        ))
                        .chain(collect_border_nodes(b.frame, &attrs))
                    })
                    .map(|n| match n {
                        RenderNode::Primitive(p) => p,
                        _ => unreachable!(),
                    })
                    .collect();
                assert_eq!(actual, expected);
                for primitive in &actual {
                    match primitive {
                        DrawPrimitive::BorderEdges(.., corners, _, _, _, _, _, _)
                        | DrawPrimitive::Shadow(_, _, _, _, _, _, _, _, corners, _)
                        | DrawPrimitive::InsetShadow(_, _, _, _, _, _, _, _, corners, _) => {
                            assert_eq!(*corners, box_model::border_radii(Some(&radius)))
                        }
                        _ => unreachable!(),
                    }
                }
                // Exercise the composed raster path as well as primitive emission.
                let pixels = render_scene_to_pixels(200, 240, render_output(&tree).scene);
                assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            }
        }
    }
}

#[test]
fn inline_shadow_culling_includes_overflow_beyond_the_paragraph_frame() {
    use crate::tree::attrs::BoxShadow;
    use crate::tree::scene::SceneContext;
    use crate::tree::viewport_culling::should_skip_render_viewport_subtree;
    let (tree, paragraph, _) = inline_decoration_tree(Attrs {
        box_shadows: Some(vec![BoxShadow {
            offset_x: 100.0,
            offset_y: 0.0,
            blur: 4.0,
            size: 4.0,
            color: Color::Named("blue".into()),
            inset: false,
        }]),
        ..Attrs::default()
    });
    let scene = SceneContext {
        visible_clip: Some(ClipShape {
            rect: Rect {
                x: 125.0,
                y: 0.0,
                width: 20.0,
                height: 240.0,
            },
            radii: None,
        }),
        ..SceneContext::default()
    };
    assert!(!should_skip_render_viewport_subtree(
        &tree,
        tree.ix_of(&paragraph).unwrap(),
        &scene
    ));
    let pixels = render_scene_to_pixels(200, 240, render_output(&tree).scene);
    assert!((125..145).any(|x| (0..240).any(|y| rgba_at(&pixels, 200, x, y).3 > 0)));
}

#[test]
fn inline_shadow_overflow_obeys_each_scroll_axis() {
    use crate::tree::attrs::{BoxShadow, Length, Padding};
    use crate::tree::layout::{Constraint, layout_tree_default};
    for (clip_x, clip_y) in [(false, false), (true, false), (false, true), (true, true)] {
        let shadows = [(110.0, 0.0), (0.0, 70.0)]
            .into_iter()
            .map(|(offset_x, offset_y)| BoxShadow {
                offset_x,
                offset_y,
                blur: 2.0,
                size: 2.0,
                color: Color::Named("blue".into()),
                inset: false,
            })
            .collect();
        let (mut tree, paragraph, _) = inline_decoration_tree(Attrs {
            box_shadows: Some(shadows),
            ..Attrs::default()
        });
        let root = NodeId::from_u64(920);
        let host = NodeId::from_u64(921);
        tree.insert(Element::with_attrs(
            root,
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Px(240.0)),
                height: Some(Length::Px(240.0)),
                padding: Some(Padding::Uniform(24.0)),
                ..Attrs::default()
            },
        ));
        tree.insert(Element::with_attrs(
            host,
            ElementKind::El,
            vec![],
            Attrs {
                width: Some(Length::Px(120.0)),
                height: Some(Length::Px(80.0)),
                scrollbar_x: Some(clip_x),
                scrollbar_y: Some(clip_y),
                ..Attrs::default()
            },
        ));
        tree.set_root_id(root);
        tree.set_children(&root, vec![host]).unwrap();
        tree.set_children(&host, vec![paragraph]).unwrap();
        layout_tree_default(&mut tree, Constraint::new(240.0, 240.0), 1.0);
        let pixels = render_scene_to_pixels(240, 240, render_output(&tree).scene);
        let blue = |x, y| {
            let (r, g, b, a) = rgba_at(&pixels, 240, x, y);
            a > 30 && b > r.saturating_add(30) && b > g.saturating_add(30)
        };
        let outside_x = (150..230).any(|x| (0..100).any(|y| blue(x, y)));
        let outside_y = (0..140).any(|x| (110..230).any(|y| blue(x, y)));
        assert_eq!(
            outside_x, !clip_x,
            "horizontal shadow clipping: {clip_x}/{clip_y}"
        );
        assert_eq!(
            outside_y, !clip_y,
            "vertical shadow clipping: {clip_x}/{clip_y}"
        );
    }
}

#[test]
fn inline_wrappers_without_a_background_emit_no_background_draws() {
    use crate::tree::attrs::{Background, BoxShadow};
    for with_shadow in [false, true] {
        let (mut tree, paragraph, _) = inline_decoration_tree(Attrs {
            box_shadows: with_shadow.then(|| {
                vec![BoxShadow {
                    offset_x: 0.0,
                    offset_y: 2.0,
                    blur: 4.0,
                    size: 2.0,
                    color: Color::Named("blue".into()),
                    inset: false,
                }]
            }),
            ..Attrs::default()
        });
        let draws = observe_tree(&tree);
        assert!(draws.iter().all(|draw| matches!(
            draw.primitive,
            DrawPrimitive::TextWithFont(..) | DrawPrimitive::Shadow(..)
        )));
        assert_eq!(
            draws.len(),
            4 + usize::from(with_shadow)
                * tree.get(&paragraph).unwrap().layout.paragraph_boxes.len()
        );
        // An ancestor background must not manufacture backgrounds for its spans.
        tree.get_mut(&paragraph)
            .unwrap()
            .layout
            .effective
            .background = Some(Background::Color(Color::Named("black".into())));
        assert_eq!(
            observe_tree(&tree)
                .iter()
                .filter(|d| matches!(d.primitive, DrawPrimitive::Rect(..)))
                .count(),
            1
        );
    }
}

#[test]
fn inline_color_backgrounds_clone_per_line_with_corner_clips_and_gradient_bounds() {
    use crate::tree::attrs::{Background, BorderRadius, SolidColor};
    for color in [
        Color::Rgb { r: 0, g: 255, b: 0 },
        Color::Gradient {
            colors: vec![
                SolidColor::Rgb { r: 255, g: 0, b: 0 },
                SolidColor::Rgb { r: 0, g: 255, b: 0 },
                SolidColor::Rgb { r: 0, g: 0, b: 255 },
            ]
            .into(),
            angle: 0.0,
        },
    ] {
        let (tree, paragraph, _) = inline_decoration_tree(Attrs {
            background: Some(Background::Color(color.clone())),
            border_radius: Some(BorderRadius::Corners {
                tl: 12.0,
                tr: 0.0,
                br: 4.0,
                bl: 0.0,
            }),
            ..Attrs::default()
        });
        let boxes = &tree.get(&paragraph).unwrap().layout.paragraph_boxes;
        assert!(boxes.len() > 1);
        let draws = observe_tree(&tree);
        let backgrounds: Vec<_> = draws
            .iter()
            .filter(|d| matches!(d.primitive, DrawPrimitive::Rect(..)))
            .collect();
        assert_eq!(backgrounds.len(), boxes.len());
        let pixels = render_scene_to_pixels(200, 240, render_output(&tree).scene);
        for (b, draw) in boxes.iter().zip(backgrounds) {
            assert_eq!(
                draw.primitive,
                DrawPrimitive::Rect(
                    b.frame.x,
                    b.frame.y,
                    b.frame.width,
                    b.frame.height,
                    color.render(Rect::from_frame(b.frame))
                )
            );
            let y = b.frame.y.ceil() as u32 + 1;
            assert_eq!(
                rgba_at(&pixels, 200, b.frame.x.ceil() as u32 + 1, y).3,
                0,
                "rounded background corner must stay clipped"
            );
            let (r, g, blue, alpha) =
                rgba_at(&pixels, 200, (b.frame.x + b.frame.width / 2.0) as u32, y);
            assert!(
                g > 200 && r < 20 && blue < 20 && alpha == 255,
                "background must paint behind each line independently"
            );
        }
    }
}

#[test]
fn inline_background_paints_between_outer_and_inset_shadows_before_text_and_border() {
    use crate::tree::attrs::{Background, BorderWidth, BoxShadow};
    let (tree, paragraph, _) = inline_decoration_tree(Attrs {
        background: Some(Background::Color(Color::Named("red".into()))),
        border_width: Some(BorderWidth::Uniform(1.0)),
        border_color: Some(Color::Named("blue".into())),
        box_shadows: Some(
            [false, true]
                .into_iter()
                .map(|inset| BoxShadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur: 2.0,
                    size: 2.0,
                    color: Color::Named("blue".into()),
                    inset,
                })
                .collect(),
        ),
        ..Attrs::default()
    });
    let draws = observe_tree(&tree);
    tree.get(&paragraph)
        .unwrap()
        .layout
        .paragraph_boxes
        .iter()
        .fold(0, |start, b| {
            let count = b.text_range.len();
            assert!(matches!(draws[start].primitive, DrawPrimitive::Shadow(..)));
            assert!(matches!(
                draws[start + 1].primitive,
                DrawPrimitive::Rect(..)
            ));
            assert!(matches!(
                draws[start + 2].primitive,
                DrawPrimitive::InsetShadow(..)
            ));
            assert!(
                draws[start + 3..start + 3 + count]
                    .iter()
                    .all(|d| matches!(d.primitive, DrawPrimitive::TextWithFont(..)))
            );
            assert!(matches!(
                draws[start + 3 + count].primitive,
                DrawPrimitive::Border(..)
            ));
            start + count + 4
        });
}
