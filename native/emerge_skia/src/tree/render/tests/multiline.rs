use super::common::*;
use super::*;
use crate::renderer::{RendererCacheConfig, SceneRenderer, make_font_with_style};
use crate::tree::layout::{Constraint, layout_and_refresh_default};

fn word_wrap_metrics() -> (f32, f32, f32) {
    let font = make_font_with_style("default", 400, false, 44.0);
    let a = font.measure_str("a", None).0;
    let width = 3.0 * a + font.measure_str(" ", None).0;
    let (_, metrics) = font.metrics();
    (a, width, (metrics.ascent.abs() + metrics.descent).max(44.0))
}

#[test]
fn multiline_word_wrap_paint_selection_caret_and_interaction_agree() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (a, width, line_height) = word_wrap_metrics();
    let frame = Frame {
        x: 10.0,
        y: 20.0,
        width: width + 64.0,
        height: 3.0 * line_height + 64.0,
        content_width: width + 64.0,
        content_height: 3.0 * line_height + 64.0,
    };
    for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
        let attrs = Attrs {
            content: Some("a aaa aa".into()),
            font_size: Some(44.0),
            padding: Some(Padding::Uniform(32.0)),
            text_align: Some(align),
            text_input_focused: Some(true),
            text_input_cursor: Some(8),
            text_input_selection_anchor: Some(2),
            ..Attrs::default()
        };
        let tree = build_multiline_tree_with_frame(attrs, frame);
        let (output, draws) = observe_output(&tree);
        let text: Vec<_> = draws
            .iter()
            .filter_map(|draw| match &draw.primitive {
                DrawPrimitive::TextWithFont(x, y, text, ..) => Some((*x, *y, text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            text.iter().map(|line| line.2).collect::<Vec<_>>(),
            ["a ", "aaa ", "aa"]
        );
        let selection: Vec<_> = draws
            .iter()
            .filter_map(|draw| match &draw.primitive {
                DrawPrimitive::Rect(_, y, width, _, color) if *color == 0x4A90E266 => {
                    Some((*y, *width))
                }
                _ => None,
            })
            .collect();
        assert_eq!(selection.len(), 2);
        assert!((selection[0].1 - width).abs() < 0.001);
        assert!((selection[1].1 - 2.0 * a).abs() < 0.001);
        assert!((selection[1].0 - selection[0].0 - line_height).abs() < 0.001);
        let last_line_x = frame.x
            + 32.0
            + match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (width - 2.0 * a) / 2.0,
                TextAlign::Right => width - 2.0 * a,
            };
        assert!((text[2].0 - last_line_x).abs() < 0.001);
        assert!((text[2].1 - text[0].1 - 2.0 * line_height).abs() < 0.001);
        let (caret_x, caret_y, _, _) = output.text_input_cursor_area.unwrap();
        assert!((caret_x - last_line_x - 2.0 * a).abs() < 0.001);
        assert!((caret_y - frame.y - 32.0 - 2.0 * line_height).abs() < 0.001);
        let state = &output.event_rebuild.text_inputs[&tree.root_id().unwrap()];
        assert_eq!(
            state.cursor_from_click_point(caret_x, caret_y + line_height / 2.0),
            8
        );
        assert_eq!(state.move_home_target(), 6);
        assert_eq!(state.move_end_target(), 8);
        assert_eq!(state.move_vertical_target(-1), 4);
        let mut moved = state.clone();
        moved.cursor = 4;
        assert_eq!(moved.move_vertical_target(1), 8);
    }
}

#[test]
fn multiline_word_wrap_raster_and_warm_cache_follow_native_edits() {
    let assets = crate::assets::AssetRuntime::new();
    let _assets = assets.enter();
    let (_, width, line_height) = word_wrap_metrics();
    let mut input = Element::with_attrs(
        NodeId(2),
        ElementKind::Multiline,
        vec![],
        Attrs {
            width: Some(Length::Fill),
            font_size: Some(44.0),
            padding: Some(Padding::Uniform(32.0)),
            border_radius: Some(BorderRadius::Uniform(24.0)),
            background: Some(Background::Color(Color::Rgb {
                r: 30,
                g: 30,
                b: 30,
            })),
            font_color: Some(Color::Named("white".into())),
            content: Some(String::new()),
            ..Attrs::default()
        },
    );
    input.runtime.text_input_focused = true;
    let mut root = Element::with_attrs(
        NodeId(1),
        ElementKind::Column,
        vec![],
        Attrs {
            width: Some(Length::Px((width + 80.0) as f64)),
            height: Some(Length::Px(320.0)),
            padding: Some(Padding::Uniform(8.0)),
            scrollbar_y: Some(true),
            ..Attrs::default()
        },
    );
    root.children = vec![NodeId(2)];
    let mut tree = ElementTree::new();
    tree.set_root_id(NodeId(1));
    tree.insert(root);
    tree.insert(input);
    let mut cached = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: true,
        ..RendererCacheConfig::default()
    });
    let mut direct = SceneRenderer::with_cache_config(RendererCacheConfig {
        enabled: false,
        ..RendererCacheConfig::default()
    });
    let render = super::pipeline::render_scene_with_renderer_to_pixels_and_timings;
    let pixel_width = (width + 80.0).ceil() as u32;
    for (text, lines) in [
        ("a aaa ", 2),
        ("a aaa a", 3),
        ("a aaa aa", 3),
        ("a aaa ", 2),
    ] {
        tree.set_text_input_content(&NodeId(2), text.into());
        tree.set_text_input_runtime(
            &NodeId(2),
            true,
            Some(text.chars().count() as u32),
            None,
            None,
            None,
        );
        let output =
            layout_and_refresh_default(&mut tree, Constraint::new(width + 80.0, 320.0), 1.0);
        let frame = tree.get(&NodeId(2)).unwrap().layout.frame.unwrap();
        assert!(
            (frame.height - 64.0 - lines as f32 * line_height).abs() < 0.001,
            "{text:?}"
        );
        let expected = render(&mut direct, output.scene.clone(), pixel_width, 320).0;
        let cold = render(&mut cached, output.scene.clone(), pixel_width, 320).0;
        let (warm, timings) = render(&mut cached, output.scene, pixel_width, 320);
        assert_eq!(
            cold, expected,
            "{text:?}: cold cache differs from direct paint"
        );
        assert_eq!(
            warm, expected,
            "{text:?}: warm cache differs from direct paint"
        );
        assert!(timings.renderer_cache.unwrap().paint_layer.hits > 0);
        // Text/caret must not bleed into the right padding or below the field.
        let right_padding_x = (frame.x + frame.width - 16.0) as u32;
        for y in (frame.y + 24.0) as u32..(frame.y + frame.height - 24.0) as u32 {
            assert_eq!(
                rgba_at(&warm, pixel_width, right_padding_x, y),
                (30, 30, 30, 255),
                "{text:?}"
            );
        }
        assert_eq!(
            rgba_at(
                &warm,
                pixel_width,
                (frame.x + 32.0) as u32,
                (frame.y + frame.height + 4.0) as u32
            )
            .3,
            0
        );
    }
}
