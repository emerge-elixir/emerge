use super::common::*;
use super::*;
use crate::render_scene::PaintLayerReason;
use crate::renderer::{RenderFrame, RenderState, RendererCacheConfig, SceneRenderer};
use crate::tree::geometry::{ClipShape, CornerRadii, Rect};
use crate::tree::layout::{Constraint, layout_tree_default, refresh_render_scene_for_benchmark};
use crate::tree::transform::{Affine2, Point, element_transform};

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
        y: 0.5,
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

fn render_scene_with_renderer_to_pixels(
    renderer: &mut SceneRenderer,
    scene: crate::render_scene::RenderScene,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .expect("raster surface should be created for render test");
    let state = RenderState::new(scene, skia_safe::Color::TRANSPARENT, 1, false);
    let mut frame = RenderFrame::new(&mut surface, None);
    renderer.render(&mut frame, &state);

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    surface.read_pixels(&info, pixels.as_mut_slice(), (width * 4) as usize, (0, 0));
    pixels
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
fn test_cached_scroll_container_repaints_direct_content_when_scroll_offset_changes() {
    let root_id = NodeId::from_u64(810_000);
    let red_id = NodeId::from_u64(810_001);
    let green_id = NodeId::from_u64(810_002);

    let mut root_attrs = solid_fill_attrs((245, 245, 245));
    root_attrs.scrollbar_y = Some(true);
    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![red_id, green_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 100.0,
    });
    root.layout.scroll_y_max = 50.0;

    let mut red = Element::with_attrs(
        red_id,
        ElementKind::Text,
        Vec::new(),
        solid_fill_attrs((255, 0, 0)),
    );
    red.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 90.0,
        height: 50.0,
        content_width: 90.0,
        content_height: 50.0,
    });

    let mut green = Element::with_attrs(
        green_id,
        ElementKind::Text,
        Vec::new(),
        solid_fill_attrs((0, 255, 0)),
    );
    green.layout.frame = Some(Frame {
        x: 0.0,
        y: 50.0,
        width: 90.0,
        height: 50.0,
        content_width: 90.0,
        content_height: 50.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(red);
    tree.insert(green);

    let first_scene = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let mut cached_renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: true,
        ..RendererCacheConfig::default()
    });
    let _ = render_scene_with_renderer_to_pixels(&mut cached_renderer, first_scene, 100, 50);

    assert!(tree.apply_scroll_y(&root_id, -50.0).is_dirty());
    let scrolled_scene = super::super::render_tree_scene_with_scroll_layers(&tree).scene;

    let cached_pixels =
        render_scene_with_renderer_to_pixels(&mut cached_renderer, scrolled_scene.clone(), 100, 50);
    let mut direct_renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: false,
        ..RendererCacheConfig::default()
    });
    let direct_pixels =
        render_scene_with_renderer_to_pixels(&mut direct_renderer, scrolled_scene, 100, 50);

    assert_eq!(
        rgba_at(&cached_pixels, 100, 20, 25),
        (0, 255, 0, 255),
        "cached scroll container reused stale pre-scroll pixels"
    );
    assert_eq!(
        cached_pixels, direct_pixels,
        "cached scrolled frame must match direct rendering after scroll offset changes"
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

    let root_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 20,
            g: 20,
            b: 40,
        })),
        ..Attrs::default()
    };
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

    let holder_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 60,
            g: 50,
            b: 80,
        })),
        ..Attrs::default()
    };
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

    let text_attrs = Attrs {
        content: Some("Overview".to_string()),
        font_size: Some(22.0),
        font_color: Some(Color::Named("white".to_string())),
        ..Attrs::default()
    };
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

    let root_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 20,
            g: 20,
            b: 40,
        })),
        ..Attrs::default()
    };
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

    let left_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 50,
            g: 70,
            b: 90,
        })),
        rotate: Some(-6.0),
        alpha: Some(0.85),
        ..Attrs::default()
    };
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

    let right_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 70,
            g: 60,
            b: 90,
        })),
        scale: Some(1.06),
        move_y: Some(-14.0),
        ..Attrs::default()
    };
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

    let left_draw = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 104.0, 60.0, 0x32465AFF)
        )
    });
    let right_draw = only_draw(draws, |draw| {
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

    let root_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        })),
        border_radius: Some(BorderRadius::Uniform(12.0)),
        ..Attrs::default()
    };
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

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 240,
            g: 237,
            b: 248,
        })),
        ..Attrs::default()
    };
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
    let child_rect = only_draw(draws, |draw| {
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
    let attrs = Attrs {
        move_x: Some(10.0),
        move_y: Some(5.0),
        ..Attrs::default()
    };
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

    let draw = only_draw(draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, expected_transform);
}

#[test]
fn test_render_emits_rotate_for_rotation() {
    let attrs = Attrs {
        rotate: Some(45.0),
        ..Attrs::default()
    };
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

    let draw = only_draw(draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, expected_transform);
}

#[test]
fn test_render_emits_scale_for_scale() {
    let attrs = Attrs {
        scale: Some(1.1),
        ..Attrs::default()
    };
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

    let draw = only_draw(draws, |resolved| {
        matches!(
            resolved.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    assert_eq!(draw.cumulative_transform, expected_transform);
}

#[test]
fn test_render_emits_alpha_layer() {
    let attrs = Attrs {
        alpha: Some(0.5),
        ..Attrs::default()
    };
    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let draw = only_draw(draws, |resolved| {
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

    let parent_attrs = Attrs {
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

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

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        })),
        alpha: Some(0.5),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgba {
            r: 255,
            g: 255,
            b: 255,
            a: 0,
        })),
        border_radius: Some(BorderRadius::Uniform(8.0)),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 6.0,
            size: 2.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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
fn test_zero_offset_outer_glow_paints_all_sides() {
    let parent_id = NodeId::from_u64(914_000);
    let child_id = NodeId::from_u64(914_001);

    let mut parent = Element::with_attrs(parent_id, ElementKind::El, Vec::new(), Attrs::default());
    parent.children = vec![child_id];
    parent.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 180.0,
        height: 100.0,
        content_width: 180.0,
        content_height: 100.0,
    });

    let child_attrs = Attrs {
        border_radius: Some(BorderRadius::Uniform(8.0)),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 4.0,
            size: 2.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

    let mut child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), child_attrs);
    child.layout.frame = Some(Frame {
        x: 40.0,
        y: 30.0,
        width: 80.0,
        height: 30.0,
        content_width: 80.0,
        content_height: 30.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(parent_id);
    tree.insert(parent);
    tree.insert(child);

    let (_output, pixels) = render_tree_to_pixels(180, 100, &tree);
    let left = rgba_at(&pixels, 180, 37, 45).3;
    let right = rgba_at(&pixels, 180, 123, 45).3;
    let top = rgba_at(&pixels, 180, 80, 27).3;
    let bottom = rgba_at(&pixels, 180, 80, 63).3;

    assert!(left > 0, "left glow should paint");
    assert!(right > 0, "right glow should paint");
    assert!(top > 0, "top glow should paint");
    assert!(bottom > 0, "bottom glow should paint");
}

#[test]
fn test_slider_thumb_outer_glow_bleeds_past_slider_frame() {
    let root_id = NodeId::from_u64(914_100);
    let slider_id = NodeId::from_u64(914_101);
    let track_id = NodeId::from_u64(914_102);
    let filled_id = NodeId::from_u64(914_103);
    let thumb_id = NodeId::from_u64(914_104);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), Attrs::default());
    root.children = vec![slider_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 560.0,
        height: 120.0,
        content_width: 560.0,
        content_height: 120.0,
    });

    let mut slider =
        Element::with_attrs(slider_id, ElementKind::Slider, Vec::new(), Attrs::default());
    slider.children = vec![track_id, filled_id, thumb_id];
    slider.layout.frame = Some(Frame {
        x: 240.0,
        y: 46.0,
        width: 280.0,
        height: 38.0,
        content_width: 280.0,
        content_height: 38.0,
    });

    let mut track = Element::with_attrs(track_id, ElementKind::El, Vec::new(), Attrs::default());
    track.layout.frame = Some(Frame {
        x: 252.0,
        y: 61.0,
        width: 256.0,
        height: 8.0,
        content_width: 256.0,
        content_height: 8.0,
    });

    let mut filled = Element::with_attrs(filled_id, ElementKind::El, Vec::new(), Attrs::default());
    filled.layout.frame = Some(Frame {
        x: 252.0,
        y: 61.0,
        width: 256.0,
        height: 8.0,
        content_width: 256.0,
        content_height: 8.0,
    });

    let mut thumb_attrs = solid_fill_attrs((242, 246, 255));
    thumb_attrs.border_radius = Some(BorderRadius::Uniform(999.0));
    thumb_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 0.0,
        size: 10.0,
        color: Color::Rgba {
            r: 255,
            g: 220,
            b: 120,
            a: 204,
        },
        inset: false,
    }]);
    let mut thumb = Element::with_attrs(thumb_id, ElementKind::El, Vec::new(), thumb_attrs);
    thumb.layout.frame = Some(Frame {
        x: 496.0,
        y: 53.0,
        width: 24.0,
        height: 24.0,
        content_width: 24.0,
        content_height: 24.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(slider);
    tree.insert(track);
    tree.insert(filled);
    tree.insert(thumb);

    let trace = trace_tree(&tree);
    let shadow = only_draw(&trace.draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    assert!(
        clip_scope_chain(&trace, shadow).is_empty(),
        "thumb glow should not inherit the slider host clip"
    );

    let (_output, pixels) = render_tree_to_pixels(560, 120, &tree);
    assert!(
        rgba_at(&pixels, 560, 524, 65).3 > 0,
        "thumb glow should paint to the right of the slider frame"
    );
    assert!(
        rgba_at(&pixels, 560, 508, 44).3 > 0,
        "thumb glow should paint above the slider frame"
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

    let draw = only_draw(draws, |resolved| {
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
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };
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

    let background = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let behind = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 20.0, 10.0, 0xFF0000FF)
        )
    });
    let front = only_draw(draws, |draw| {
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
    let parent_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 255, b: 0 })),
        ..Attrs::default()
    };

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

    let background = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let behind = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 20.0, 10.0, 0xFF0000FF)
        )
    });
    let child = only_draw(draws, |draw| {
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
    let parent_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        padding: Some(Padding::Uniform(10.0)),
        ..Attrs::default()
    };

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 255, b: 0 })),
        ..Attrs::default()
    };

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

    let behind = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0xFF0000FF)
        )
    });
    let child = only_draw(draws, |draw| {
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
fn test_todo_create_placeholder_behind_text_input_survives_cached_layer_composition() {
    let host_id = NodeId::from_term_bytes(vec![220]);
    let input_id = NodeId::from_term_bytes(vec![221]);
    let placeholder_id = NodeId::from_term_bytes(vec![222]);

    let mut host = Element::with_attrs(
        host_id,
        ElementKind::El,
        Vec::new(),
        Attrs {
            background: Some(Background::Color(Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            })),
            ..Attrs::default()
        },
    );
    host.children = vec![input_id];
    host.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 260.0,
        height: 64.0,
        content_width: 260.0,
        content_height: 64.0,
    });
    host.nearby.push(NearbySlot::BehindContent, placeholder_id);

    let mut input = Element::with_attrs(
        input_id,
        ElementKind::TextInput,
        Vec::new(),
        Attrs {
            content: Some(String::new()),
            padding: Some(Padding::Uniform(16.0)),
            font_size: Some(24.0),
            font_color: Some(Color::Rgb { r: 0, g: 0, b: 0 }),
            background: Some(Background::Color(Color::Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 0,
            })),
            ..Attrs::default()
        },
    );
    input.layout.frame = host.layout.frame;

    let mut placeholder = Element::with_attrs(
        placeholder_id,
        ElementKind::Text,
        Vec::new(),
        Attrs {
            content: Some("What needs to be done?".to_string()),
            font_size: Some(24.0),
            font_color: Some(Color::Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 180,
            }),
            font_style: Some(FontStyle("italic".to_string())),
            ..Attrs::default()
        },
    );
    placeholder.layout.frame = Some(Frame {
        x: 16.0,
        y: 18.5,
        width: 230.0,
        height: 32.0,
        content_width: 230.0,
        content_height: 32.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(host_id);
    tree.insert(host);
    tree.insert(input);
    tree.insert(placeholder);

    let output = render_output(&tree);
    let info = skia_safe::ImageInfo::new(
        (260, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut direct_surface = skia_safe::surfaces::raster(&info, None, None)
        .expect("raster surface should be created for render test");
    let mut direct_renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: false,
        ..RendererCacheConfig::default()
    });
    let state = RenderState::new(
        output.scene.clone(),
        skia_safe::Color::TRANSPARENT,
        1,
        false,
    );
    let mut direct_frame = RenderFrame::new(&mut direct_surface, None);
    direct_renderer.render(&mut direct_frame, &state);

    let mut direct_pixels = vec![0u8; 260 * 64 * 4];
    direct_surface.read_pixels(&info, direct_pixels.as_mut_slice(), 260 * 4, (0, 0));

    let mut cached_surface = skia_safe::surfaces::raster(&info, None, None)
        .expect("raster surface should be created for render test");
    let mut renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: true,
        ..RendererCacheConfig::default()
    });

    for _ in 0..2 {
        let mut frame = RenderFrame::new(&mut cached_surface, None);
        renderer.render(&mut frame, &state);
    }

    let mut pixels = vec![0u8; 260 * 64 * 4];
    cached_surface.read_pixels(&info, pixels.as_mut_slice(), 260 * 4, (0, 0));
    let mismatched_bytes = pixels
        .iter()
        .zip(direct_pixels.iter())
        .filter(|(cached, direct)| cached != direct)
        .count();
    assert_eq!(
        mismatched_bytes, 0,
        "cached todo placeholder rendering diverged from direct rendering by {mismatched_bytes} bytes"
    );

    let dark_placeholder_pixels = (16..230)
        .flat_map(|x| (18..50).map(move |y| (x, y)))
        .filter(|(x, y)| {
            let (r, g, b, a) = rgba_at(&pixels, 260, *x, *y);
            a > 0 && r < 235 && g < 235 && b < 235
        })
        .count();

    assert!(
        dark_placeholder_pixels > 40,
        "expected cached todo placeholder text to remain visible, dark pixels={dark_placeholder_pixels}"
    );
}

#[test]
fn test_render_nearby_above_below_order_after_parent() {
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

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

    let background = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let above = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, -10.0, 20.0, 10.0, 0x00FF00FF)
        )
    });
    let below = only_draw(draws, |draw| {
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
    let parent_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 255, b: 0 })),
        ..Attrs::default()
    };

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

    let child = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 10.0, 0x00FF00FF)
        )
    });
    let nearby = only_draw(draws, |draw| {
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
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

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

    let first = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 20.0, 0xFF0000FF)
        )
    });
    let second = only_draw(draws, |draw| {
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

    let child_attrs = Attrs {
        clip_nearby: Some(true),
        ..Attrs::default()
    };

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
fn test_render_rotated_root_paints_in_front_nearby_from_logical_render_frame() {
    let mut root_attrs = solid_fill_attrs((0, 0, 0));
    root_attrs.clip_nearby = Some(true);
    root_attrs.layout_rotate = Some(90.0);

    let mut tree = build_tree_with_frame(
        root_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 480.0,
            height: 320.0,
            content_width: 480.0,
            content_height: 320.0,
        },
    );
    let root_id = tree.root_id().unwrap();
    tree.get_mut(&root_id).unwrap().layout.render_frame = Some(Frame {
        x: 80.0,
        y: -80.0,
        width: 320.0,
        height: 480.0,
        content_width: 320.0,
        content_height: 480.0,
    });

    mount_nearby(
        &mut tree,
        &root_id,
        NearbySlot::InFront,
        ElementKind::El,
        solid_fill_attrs((255, 0, 0)),
        Frame {
            x: 80.0,
            y: -80.0,
            width: 72.0,
            height: 72.0,
            content_width: 72.0,
            content_height: 72.0,
        },
        90,
    );

    let (_output, pixels) = render_tree_to_pixels(480, 320, &tree);

    assert_eq!(rgba_at(&pixels, 480, 440, 20), (255, 0, 0, 255));
}

#[test]
fn test_render_rotated_root_keeps_nested_fill_column_content_visible() {
    let root_id = NodeId::from_u64(910_000);
    let screen_id = NodeId::from_u64(910_001);
    let column_id = NodeId::from_u64(910_002);
    let header_id = NodeId::from_u64(910_003);
    let body_id = NodeId::from_u64(910_004);
    let panel_id = NodeId::from_u64(910_005);
    let content_id = NodeId::from_u64(910_006);

    let root_attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Fill),
        layout_rotate: Some(90.0),
        ..Attrs::default()
    };

    let mut screen_attrs = solid_fill_attrs((243, 244, 247));
    screen_attrs.width = Some(Length::Fill);
    screen_attrs.height = Some(Length::Fill);

    let column_attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Fill),
        ..Attrs::default()
    };

    let mut header_attrs = solid_fill_attrs((255, 0, 0));
    header_attrs.width = Some(Length::Fill);
    header_attrs.height = Some(Length::Px(96.0));

    let body_attrs = Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Fill),
        padding: Some(Padding::Uniform(16.0)),
        ..Attrs::default()
    };

    let mut panel_attrs = solid_fill_attrs((255, 255, 255));
    panel_attrs.width = Some(Length::Fill);
    panel_attrs.height = Some(Length::Fill);
    panel_attrs.padding = Some(Padding::Uniform(24.0));
    panel_attrs.scrollbar_y = Some(true);
    panel_attrs.border_radius = Some(BorderRadius::Uniform(24.0));
    panel_attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 16.0,
        blur: 40.0,
        size: 0.0,
        color: Color::Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 20,
        },
        inset: false,
    }]);

    let mut content_attrs = solid_fill_attrs((0, 255, 0));
    content_attrs.width = Some(Length::Fill);
    content_attrs.height = Some(Length::Px(1600.0));

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![screen_id];
    let mut screen = Element::with_attrs(screen_id, ElementKind::El, Vec::new(), screen_attrs);
    screen.children = vec![column_id];
    let mut column = Element::with_attrs(column_id, ElementKind::Column, Vec::new(), column_attrs);
    column.children = vec![header_id, body_id];
    let header = Element::with_attrs(header_id, ElementKind::El, Vec::new(), header_attrs);
    let mut body = Element::with_attrs(body_id, ElementKind::El, Vec::new(), body_attrs);
    body.children = vec![panel_id];
    let mut panel = Element::with_attrs(panel_id, ElementKind::El, Vec::new(), panel_attrs);
    panel.children = vec![content_id];
    let content = Element::with_attrs(content_id, ElementKind::El, Vec::new(), content_attrs);

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(screen);
    tree.insert(column);
    tree.insert(header);
    tree.insert(body);
    tree.insert(panel);
    tree.insert(content);

    layout_tree_default(&mut tree, Constraint::new(480.0, 320.0), 1.0);

    let scene = refresh_render_scene_for_benchmark(&mut tree);
    let pixels = render_scene_to_pixels(480, 320, scene);

    assert_eq!(rgba_at(&pixels, 480, 440, 24), (255, 0, 0, 255));
    assert_eq!(rgba_at(&pixels, 480, 180, 160), (0, 255, 0, 255));
}

fn rotated_slider_tree(value: f64) -> (ElementTree, NodeId) {
    let root_id = NodeId::from_u64(911_000);
    let slider_id = NodeId::from_u64(911_001);
    let track_id = NodeId::from_u64(911_002);
    let filled_id = NodeId::from_u64(911_003);
    let thumb_id = NodeId::from_u64(911_004);

    let mut root_attrs = solid_fill_attrs((10, 10, 10));
    root_attrs.width = Some(Length::Px(96.0));
    root_attrs.height = Some(Length::Px(228.0));
    root_attrs.padding = Some(Padding::Uniform(14.0));

    let slider_attrs = Attrs {
        width: Some(Length::Px(180.0)),
        height: Some(Length::Px(38.0)),
        align_x: Some(AlignX::Center),
        align_y: Some(AlignY::Center),
        layout_rotate: Some(-90.0),
        slider_min: Some(0.0),
        slider_max: Some(100.0),
        slider_value: Some(value),
        ..Attrs::default()
    };

    let mut track_attrs = solid_fill_attrs((68, 84, 92));
    track_attrs.height = Some(Length::Px(8.0));

    let mut filled_attrs = solid_fill_attrs((126, 204, 176));
    filled_attrs.height = Some(Length::Px(8.0));

    let mut thumb_attrs = solid_fill_attrs((228, 252, 242));
    thumb_attrs.width = Some(Length::Px(24.0));
    thumb_attrs.height = Some(Length::Px(24.0));

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![slider_id];
    let mut slider = Element::with_attrs(slider_id, ElementKind::Slider, Vec::new(), slider_attrs);
    slider.children = vec![track_id, filled_id, thumb_id];
    let track = Element::with_attrs(track_id, ElementKind::El, Vec::new(), track_attrs);
    let filled = Element::with_attrs(filled_id, ElementKind::El, Vec::new(), filled_attrs);
    let thumb = Element::with_attrs(thumb_id, ElementKind::El, Vec::new(), thumb_attrs);

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(slider);
    tree.insert(track);
    tree.insert(filled);
    tree.insert(thumb);
    layout_tree_default(&mut tree, Constraint::new(96.0, 228.0), 1.0);
    (tree, slider_id)
}

fn rotated_slider_child_center(
    tree: &ElementTree,
    slider_id: NodeId,
    child_index: usize,
) -> (u32, u32) {
    let slider = tree.get(&slider_id).expect("slider should exist");
    let child_id = slider
        .children
        .get(child_index)
        .copied()
        .expect("slider child should exist");
    let child_frame = tree
        .get(&child_id)
        .and_then(|child| child.layout.frame)
        .expect("slider child should have a frame");
    let render_frame = slider
        .layout
        .render_frame
        .expect("rotated slider should keep render frame");
    let transform = element_transform(render_frame, &slider.layout.effective);
    let point = transform.map_point(Point {
        x: child_frame.x + child_frame.width / 2.0,
        y: child_frame.y + child_frame.height / 2.0,
    });

    (point.x.round() as u32, point.y.round() as u32)
}

fn assert_rotated_slider_thumb_inside_render_frame(tree: &ElementTree, slider_id: NodeId) {
    let slider = tree.get(&slider_id).expect("slider should exist");
    let render_frame = slider
        .layout
        .render_frame
        .expect("rotated slider should keep render frame");
    let thumb_id = slider.children[2];
    let thumb_frame = tree
        .get(&thumb_id)
        .and_then(|thumb| thumb.layout.frame)
        .expect("thumb should have a frame");

    assert!(thumb_frame.x >= render_frame.x);
    assert!(thumb_frame.x + thumb_frame.width <= render_frame.x + render_frame.width);
    assert!(thumb_frame.y >= render_frame.y);
    assert!(thumb_frame.y + thumb_frame.height <= render_frame.y + render_frame.height);
}

#[test]
fn test_render_rotated_slider_paints_track_and_thumb_near_range_edges() {
    let (low_tree, low_slider_id) = rotated_slider_tree(0.0);
    assert_rotated_slider_thumb_inside_render_frame(&low_tree, low_slider_id);
    let (_low_output, low_pixels) = render_tree_to_pixels(96, 228, &low_tree);
    let low_track = rotated_slider_child_center(&low_tree, low_slider_id, 0);
    let low_thumb = rotated_slider_child_center(&low_tree, low_slider_id, 2);

    assert_eq!(
        rgba_at(&low_pixels, 96, low_thumb.0, low_thumb.1),
        (228, 252, 242, 255)
    );
    assert_eq!(
        rgba_at(&low_pixels, 96, low_track.0, low_track.1),
        (68, 84, 92, 255)
    );

    let (high_tree, high_slider_id) = rotated_slider_tree(100.0);
    assert_rotated_slider_thumb_inside_render_frame(&high_tree, high_slider_id);
    let (_high_output, high_pixels) = render_tree_to_pixels(96, 228, &high_tree);
    let high_filled = rotated_slider_child_center(&high_tree, high_slider_id, 1);
    let high_thumb = rotated_slider_child_center(&high_tree, high_slider_id, 2);

    assert_eq!(
        rgba_at(&high_pixels, 96, high_thumb.0, high_thumb.1),
        (228, 252, 242, 255)
    );
    assert_eq!(
        rgba_at(&high_pixels, 96, high_filled.0, high_filled.1),
        (126, 204, 176, 255)
    );
}

#[test]
fn test_svg_slider_thumb_paints_above_scroll_moving_track_layers() {
    let image_id = "svg_slider_thumb_paints_above_track";
    let root_id = NodeId::from_u64(912_000);
    let slider_id = NodeId::from_u64(912_001);
    let track_id = NodeId::from_u64(912_002);
    let filled_id = NodeId::from_u64(912_003);
    let thumb_id = NodeId::from_u64(912_004);

    let mut root_attrs = solid_fill_attrs((4, 8, 12));
    root_attrs.scrollbar_y = Some(true);
    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![slider_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 220.0,
        height: 100.0,
        content_width: 220.0,
        content_height: 180.0,
    });
    root.layout.scroll_y_max = 80.0;

    let mut slider =
        Element::with_attrs(slider_id, ElementKind::Slider, Vec::new(), Attrs::default());
    slider.children = vec![track_id, filled_id, thumb_id];
    slider.layout.frame = Some(Frame {
        x: 20.0,
        y: 20.0,
        width: 180.0,
        height: 48.0,
        content_width: 180.0,
        content_height: 48.0,
    });

    let mut track = Element::with_attrs(
        track_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((20, 24, 32)),
    );
    track.layout.frame = Some(Frame {
        x: 30.0,
        y: 38.0,
        width: 160.0,
        height: 10.0,
        content_width: 160.0,
        content_height: 10.0,
    });

    let mut filled = Element::with_attrs(
        filled_id,
        ElementKind::El,
        Vec::new(),
        solid_fill_attrs((20, 24, 32)),
    );
    filled.layout.frame = Some(Frame {
        x: 30.0,
        y: 38.0,
        width: 100.0,
        height: 10.0,
        content_width: 100.0,
        content_height: 10.0,
    });

    let thumb_attrs = Attrs {
        image_src: Some(ImageSource::Id(image_id.to_string())),
        image_fit: Some(ImageFit::Contain),
        svg_expected: Some(true),
        ..Attrs::default()
    };
    let mut thumb = Element::with_attrs(thumb_id, ElementKind::Image, Vec::new(), thumb_attrs);
    thumb.layout.frame = Some(Frame {
        x: 90.0,
        y: 28.0,
        width: 30.0,
        height: 30.0,
        content_width: 30.0,
        content_height: 30.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(slider);
    tree.insert(track);
    tree.insert(filled);
    tree.insert(thumb);
    tree.clear_refresh_dirty();

    let output = super::super::render_tree_scene_with_paint_layer_policy(&tree, true, true);
    let scroll_layer =
        first_paint_layer_with_reason(&output.scene.nodes, PaintLayerReason::ScrollContainer)
            .expect("scroll container should produce the compositing layer");

    assert!(
        !render_nodes_contain_image_asset(&scroll_layer.own_nodes, image_id),
        "SVG thumb must not be parent-owned payload that can be painted below child track layers"
    );
    let child_layer_ids = paint_layer_ids_in_nodes(
        &scroll_layer
            .child_refs
            .iter()
            .flat_map(|child| child.nodes.iter().cloned())
            .collect::<Vec<_>>(),
    );
    let track_pos = child_layer_ids
        .iter()
        .position(|id| *id == track_id.to_wire_u64())
        .expect("track should be a child paint layer");
    let filled_pos = child_layer_ids
        .iter()
        .position(|id| *id == filled_id.to_wire_u64())
        .expect("filled track should be a child paint layer");
    let thumb_pos = child_layer_ids
        .iter()
        .position(|id| *id == thumb_id.to_wire_u64())
        .expect("SVG thumb should be a child paint layer");

    assert!(
        track_pos < thumb_pos && filled_pos < thumb_pos,
        "SVG thumb child layer must stay after track layers: {child_layer_ids:?}"
    );
}

fn first_paint_layer_with_reason(
    nodes: &[crate::render_scene::RenderNode],
    reason: PaintLayerReason,
) -> Option<&crate::render_scene::RenderPaintLayer> {
    nodes.iter().find_map(|node| match node {
        crate::render_scene::RenderNode::ShadowPass { children }
        | crate::render_scene::RenderNode::Clip { children, .. }
        | crate::render_scene::RenderNode::RelaxedClip { children, .. }
        | crate::render_scene::RenderNode::Transform { children, .. }
        | crate::render_scene::RenderNode::Alpha { children, .. } => {
            first_paint_layer_with_reason(children, reason)
        }
        crate::render_scene::RenderNode::PaintLayer(layer) if layer.reason == reason => Some(layer),
        crate::render_scene::RenderNode::PaintLayer(layer) => layer
            .child_refs
            .iter()
            .find_map(|child| first_paint_layer_with_reason(&child.nodes, reason)),
        crate::render_scene::RenderNode::Primitive(_) => None,
    })
}

fn paint_layer_ids_in_nodes(nodes: &[crate::render_scene::RenderNode]) -> Vec<u64> {
    nodes.iter().fold(Vec::new(), |mut ids, node| {
        match node {
            crate::render_scene::RenderNode::ShadowPass { children }
            | crate::render_scene::RenderNode::Clip { children, .. }
            | crate::render_scene::RenderNode::RelaxedClip { children, .. }
            | crate::render_scene::RenderNode::Transform { children, .. }
            | crate::render_scene::RenderNode::Alpha { children, .. } => {
                ids.extend(paint_layer_ids_in_nodes(children));
            }
            crate::render_scene::RenderNode::PaintLayer(layer) => {
                ids.push(layer.stable_id);
                layer.child_refs.iter().for_each(|child| {
                    ids.extend(paint_layer_ids_in_nodes(&child.nodes));
                });
            }
            crate::render_scene::RenderNode::Primitive(_) => {}
        }
        ids
    })
}

fn render_nodes_contain_image_asset(
    nodes: &[crate::render_scene::RenderNode],
    image_id: &str,
) -> bool {
    nodes.iter().any(|node| match node {
        crate::render_scene::RenderNode::ShadowPass { children }
        | crate::render_scene::RenderNode::Clip { children, .. }
        | crate::render_scene::RenderNode::RelaxedClip { children, .. }
        | crate::render_scene::RenderNode::Transform { children, .. }
        | crate::render_scene::RenderNode::Alpha { children, .. } => {
            render_nodes_contain_image_asset(children, image_id)
        }
        crate::render_scene::RenderNode::PaintLayer(layer) => {
            render_nodes_contain_image_asset(&layer.own_nodes, image_id)
                || layer
                    .child_refs
                    .iter()
                    .any(|child| render_nodes_contain_image_asset(&child.nodes, image_id))
        }
        crate::render_scene::RenderNode::Primitive(DrawPrimitive::Image(
            _,
            _,
            _,
            _,
            asset_id,
            _,
            _,
        )) => asset_id == image_id,
        crate::render_scene::RenderNode::Primitive(_) => false,
    })
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
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

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

    let background = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let front = only_draw(draws, |draw| {
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
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

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

    only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(-30.0, -30.0, 160.0, 80.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_above_fill_width_uses_parent_slot() {
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

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

    only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, -10.0, 100.0, 10.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_on_right_fill_height_uses_parent_slot() {
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        ..Attrs::default()
    };

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

    only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(100.0, 0.0, 20.0, 50.0, 0xFF0000FF)
        )
    });
}

#[test]
fn test_render_in_front_ignores_host_clip() {
    let attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        padding: Some(Padding::Uniform(10.0)),
        ..Attrs::default()
    };

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

    let front = only_draw(draws, |draw| {
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
    let child_attrs = Attrs {
        box_shadows: Some(vec![BoxShadow {
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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

    let shadow = only_draw(draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(draws, |draw| {
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
fn test_outer_shadow_escapes_nested_non_scrollable_ancestor_clips() {
    let root_attrs = Attrs::default();
    let parent_attrs = Attrs::default();
    let child_attrs = Attrs {
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 8.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

    let tree = build_nested_child_tree(
        root_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
            content_width: 120.0,
            content_height: 80.0,
        },
        parent_attrs,
        Frame {
            x: 16.0,
            y: 16.0,
            width: 80.0,
            height: 40.0,
            content_width: 80.0,
            content_height: 40.0,
        },
        child_attrs,
        Frame {
            x: 84.0,
            y: 20.0,
            width: 24.0,
            height: 20.0,
            content_width: 24.0,
            content_height: 20.0,
        },
    );

    let trace = trace_tree(&tree);
    let shadow = only_draw(&trace.draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });

    assert!(
        clip_scope_chain(&trace, shadow).is_empty(),
        "outer shadow should bleed through every non-scroll ancestor clip"
    );
}

#[test]
fn test_outer_shadow_bleeds_into_parent_padding() {
    let parent_attrs = Attrs {
        padding: Some(Padding::Uniform(10.0)),
        background: Some(Background::Color(Color::Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        })),
        ..Attrs::default()
    };

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        })),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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
fn test_outer_shadow_bleeds_into_parent_top_and_right_padding() {
    let parent_attrs = Attrs {
        padding: Some(Padding::Uniform(10.0)),
        ..Attrs::default()
    };

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        })),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 0.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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
            x: 30.0,
            y: 10.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
    );

    let (_output, pixels) = render_tree_to_pixels(60, 30, &tree);
    let top_padding_shadow = rgba_at(&pixels, 60, 40, 8);
    let right_padding_shadow = rgba_at(&pixels, 60, 52, 14);

    assert!(
        top_padding_shadow.3 > 0,
        "outer shadow should remain visible in the parent's top padding"
    );
    assert!(
        right_padding_shadow.3 > 0,
        "outer shadow should remain visible in the parent's right padding"
    );
}

#[test]
fn test_cached_focused_slider_glow_escapes_non_scroll_ancestor_clip() {
    let root_id = NodeId::from_u64(820_000);
    let panel_id = NodeId::from_u64(820_001);
    let slot_id = NodeId::from_u64(820_002);
    let slider_id = NodeId::from_u64(820_003);
    let track_id = NodeId::from_u64(820_004);

    let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), Attrs::default());
    root.children = vec![panel_id];
    root.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 260.0,
        height: 120.0,
        content_width: 260.0,
        content_height: 120.0,
    });

    let mut panel = Element::with_attrs(panel_id, ElementKind::El, Vec::new(), Attrs::default());
    panel.children = vec![slot_id];
    panel.layout.frame = Some(Frame {
        x: 40.0,
        y: 40.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
    });

    let mut slot = Element::with_attrs(slot_id, ElementKind::El, Vec::new(), Attrs::default());
    slot.children = vec![slider_id];
    slot.layout.frame = Some(Frame {
        x: 40.0,
        y: 40.0,
        width: 180.0,
        height: 44.0,
        content_width: 180.0,
        content_height: 44.0,
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
                    a: 180,
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
    tree.insert(panel);
    tree.insert(slot);
    tree.insert(slider);
    tree.insert(track);

    let direct_pixels =
        render_scene_to_pixels(260, 120, super::super::render_tree_scene(&tree).scene);
    let cached_scene = super::super::render_tree_scene_with_scroll_layers(&tree).scene;
    let mut cached_renderer = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: true,
        ..RendererCacheConfig::default()
    });
    let _ =
        render_scene_with_renderer_to_pixels(&mut cached_renderer, cached_scene.clone(), 260, 120);
    let cached_pixels =
        render_scene_with_renderer_to_pixels(&mut cached_renderer, cached_scene, 260, 120);

    let top_direct = rgba_at(&direct_pixels, 260, 130, 34).3;
    let top_cached = rgba_at(&cached_pixels, 260, 130, 34).3;
    let right_direct = rgba_at(&direct_pixels, 260, 226, 62).3;
    let right_cached = rgba_at(&cached_pixels, 260, 226, 62).3;

    assert!(top_direct > 0, "direct top glow should be visible");
    assert!(right_direct > 0, "direct right glow should be visible");
    assert!(
        top_cached > 0,
        "cached focused slider top glow should not be clipped"
    );
    assert!(
        right_cached > 0,
        "cached focused slider right glow should not be clipped"
    );
}

#[test]
fn test_outer_shadow_clips_only_on_vertical_scroll_axis() {
    let root_attrs = Attrs::default();
    let parent_attrs = Attrs {
        scrollbar_y: Some(true),
        ..Attrs::default()
    };
    let child_attrs = Attrs {
        box_shadows: Some(vec![BoxShadow {
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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

    let shadow = only_draw(draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(draws, |draw| {
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
    let parent_attrs = Attrs {
        scrollbar_x: Some(true),
        ..Attrs::default()
    };
    let child_attrs = Attrs {
        box_shadows: Some(vec![BoxShadow {
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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

    let shadow = only_draw(draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(draws, |draw| {
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
    let parent_attrs = Attrs {
        scrollbar_x: Some(true),
        scrollbar_y: Some(true),
        border_radius: Some(BorderRadius::Uniform(8.0)),
        ..Attrs::default()
    };
    let child_attrs = Attrs {
        box_shadows: Some(vec![BoxShadow {
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            size: 4.0,
            color: Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Attrs::default()
    };

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

    let shadow = only_draw(draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let body = only_draw(draws, |draw| {
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

    let root_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        scrollbar_y: Some(true),
        scroll_y: Some(10.0),
        ..Attrs::default()
    };

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

    let shadow = only_draw(draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let child_c = only_draw(draws, |draw| {
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

    let root_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        scrollbar_y: Some(true),
        scroll_y: Some(150.0),
        ..Attrs::default()
    };
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

    let inner_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        })),
        scrollbar_y: Some(true),
        scroll_y: Some(10.0),
        ..Attrs::default()
    };
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

    let text_attrs = Attrs {
        content: Some("visible".to_string()),
        font_size: Some(12.0),
        font_color: Some(Color::Named("white".to_string())),
        ..Attrs::default()
    };
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
        draws,
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

    let root_attrs = Attrs {
        background: Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 })),
        scrollbar_y: Some(true),
        scroll_y: Some(10.0),
        ..Attrs::default()
    };
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

    let text_attrs = Attrs {
        content: Some("shifted".to_string()),
        font_size: Some(12.0),
        font_color: Some(Color::Named("white".to_string())),
        ..Attrs::default()
    };
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
        draws,
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
    let attrs = Attrs {
        border_width: Some(BorderWidth::Uniform(2.0)),
        border_color: Some(Color::Named("red".to_string())),
        border_radius: Some(BorderRadius::Uniform(8.0)),
        scrollbar_y: Some(true),
        ..Attrs::default()
    };

    let child_attrs = Attrs {
        background: Some(Background::Color(Color::Named("white".to_string()))),
        ..Attrs::default()
    };

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

    let child_draw = only_draw(draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(10.0, 10.0, 20.0, 10.0, 0xFFFFFFFF)
        )
    });
    let border_draw = only_draw(draws, |draw| {
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
            .any(|scope| { clip_scope_shapes(scope).unwrap() == [expected_host_clip] })
    );
    assert!(scope_chain(&trace, border_draw).is_empty());
    assert!(paints_before(child_draw, border_draw));
}

#[test]
fn test_render_uses_only_background_self_clip_when_nothing_else_is_clipped() {
    let attrs = Attrs {
        border_radius: Some(BorderRadius::Uniform(8.0)),
        ..Attrs::default()
    };

    let tree = build_tree_with_attrs(attrs);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;
    let background = only_draw(draws, |draw| {
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
    let attrs = Attrs {
        border_width: Some(BorderWidth::Uniform(2.0)),
        border_color: Some(Color::Named("red".to_string())),
        scrollbar_y: Some(true),
        scroll_y_max: Some(20.0),
        ..Attrs::default()
    };

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
