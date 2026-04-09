use super::super::text::{
    measure_text_width_with_font, text_metrics_with_font, text_offset_for_char_index, TextRunStyle,
    TEXT_SELECTION_COLOR,
};
use super::common::*;
use super::*;

#[test]
fn test_render_text_with_underline_and_strike_emits_decoration_rects() {
    let mut attrs = Attrs::default();
    attrs.content = Some("Decorated".to_string());
    attrs.font_size = Some(18.0);
    attrs.font_color = Some(Color::Rgb { r: 1, g: 2, b: 3 });
    attrs.font_underline = Some(true);
    attrs.font_strike = Some(true);

    let tree = build_text_tree_with_frame(
        attrs,
        Frame {
            x: 10.0,
            y: 20.0,
            width: 220.0,
            height: 40.0,
            content_width: 220.0,
            content_height: 40.0,
        },
    );

    let draws = observe_tree(&tree);

    assert!(draws.iter().any(|draw| {
        matches!(
            &draw.primitive,
            DrawPrimitive::TextWithFont(_, _, content, _, _, _, _, _) if content == "Decorated"
        )
    }));

    let decoration_rects: Vec<(f32, f32, f32, f32)> = draws
        .iter()
        .filter_map(|draw| match &draw.primitive {
            DrawPrimitive::Rect(x, y, w, h, color) if *color == 0x010203FF => {
                Some((*x, *y, *w, *h))
            }
            _ => None,
        })
        .collect();

    assert_eq!(decoration_rects.len(), 2);
    assert!(decoration_rects
        .iter()
        .all(|(_, _, width, height)| *width > 0.0 && *height >= 1.0));
}

#[test]
fn test_render_text_defaults_to_black() {
    let mut attrs = Attrs::default();
    attrs.content = Some("Default".to_string());
    attrs.font_size = Some(16.0);

    let tree = build_text_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
            content_width: 200.0,
            content_height: 40.0,
        },
    );

    let draws = observe_tree(&tree);
    let text_cmd = only_draw(
        &draws,
        |draw| matches!(&draw.primitive, DrawPrimitive::TextWithFont(_, _, content, _, _, _, _, _) if content == "Default"),
    );

    match &text_cmd.primitive {
        DrawPrimitive::TextWithFont(_, _, _, _, color, _, _, _) => {
            assert_eq!(*color, 0x000000FF);
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_text_with_spacing_emits_per_glyph_commands() {
    let mut attrs = Attrs::default();
    attrs.content = Some("A A".to_string());
    attrs.font_size = Some(16.0);
    attrs.font_letter_spacing = Some(4.0);
    attrs.font_word_spacing = Some(6.0);

    let tree = build_text_tree_with_frame(
        attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
            content_width: 200.0,
            content_height: 40.0,
        },
    );

    let draws = observe_tree(&tree);

    let text_cmds: Vec<(f32, String)> = draws
        .iter()
        .filter_map(|draw| match &draw.primitive {
            DrawPrimitive::TextWithFont(x, _y, text, _size, _fill, _family, _weight, _italic) => {
                Some((*x, text.clone()))
            }
            _ => None,
        })
        .collect();

    assert_eq!(text_cmds.len(), 3);
    assert_eq!(text_cmds[0].1, "A");
    assert_eq!(text_cmds[1].1, " ");
    assert_eq!(text_cmds[2].1, "A");
    assert!(text_cmds[0].0 < text_cmds[1].0);
    assert!(text_cmds[1].0 < text_cmds[2].0);
}

#[test]
fn test_render_text_insets_by_padding_and_border() {
    let mut attrs = Attrs::default();
    attrs.content = Some("Inset".to_string());
    attrs.font_size = Some(16.0);
    attrs.padding = Some(Padding::Uniform(4.0));
    attrs.border_width = Some(BorderWidth::Uniform(3.0));

    let tree = build_text_tree_with_frame(
        attrs,
        Frame {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 60.0,
            content_width: 200.0,
            content_height: 60.0,
        },
    );

    let draws = observe_tree(&tree);
    let (ascent, _) = text_metrics_with_font(16.0, "default", 400, false);

    let text_cmd = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::TextWithFont(..))
    });

    match &text_cmd.primitive {
        DrawPrimitive::TextWithFont(x, y, content, _, _, _, _, _) => {
            assert_eq!(*x, 17.0, "x should include 4px padding + 3px border");
            assert_eq!(*y, 27.0 + ascent, "baseline should include top insets");
            assert_eq!(content, "Inset");
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_text_leaf_uses_host_clip() {
    let mut attrs = Attrs::default();
    attrs.content = Some("Menu".to_string());
    attrs.font_size = Some(16.0);
    attrs.font_color = Some(Color::Named("white".to_string()));

    let tree = build_text_tree_with_frame(
        attrs,
        Frame {
            x: 12.0,
            y: 8.0,
            width: 80.0,
            height: 24.0,
            content_width: 80.0,
            content_height: 24.0,
        },
    );
    let draws = observe_tree(&tree);

    let text_draw = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::TextWithFont(..))
    });
    assert!(
        !text_draw.clips.is_empty(),
        "text leaves should render inside their host clip"
    );
}

#[test]
fn test_render_text_input_preedit_underlines_segment_and_reports_composition_caret() {
    let mut attrs = Attrs::default();
    attrs.content = Some("quick".to_string());
    attrs.font_size = Some(16.0);
    attrs.text_input_focused = Some(true);
    attrs.text_input_cursor = Some(2);
    attrs.text_input_preedit = Some("xy".to_string());
    attrs.text_input_preedit_cursor = Some((1, 1));

    let frame = Frame {
        x: 10.0,
        y: 20.0,
        width: 280.0,
        height: 40.0,
        content_width: 280.0,
        content_height: 40.0,
    };

    let tree = build_text_input_tree_with_frame(attrs, frame);
    let (output, draws) = observe_output(&tree);

    assert!(output.text_input_focused);

    let (caret_x, _caret_y, _caret_w, _caret_h) = output
        .text_input_cursor_area
        .expect("caret area should be present");

    let displayed = "quxyick";
    let expected_caret_offset = text_offset_for_char_index(
        displayed,
        3,
        TextRunStyle {
            font_size: 16.0,
            color: 0,
            family: "default",
            weight: 400,
            italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        },
    );
    let expected_caret_x = 10.0 + expected_caret_offset;
    assert!((caret_x - expected_caret_x).abs() < 0.2);

    let (ascent, _descent) = text_metrics_with_font(16.0, "default", 400, false);
    let baseline_y = 20.0 + ascent;

    let preedit_x = 10.0
        + text_offset_for_char_index(
            displayed,
            2,
            TextRunStyle {
                font_size: 16.0,
                color: 0,
                family: "default",
                weight: 400,
                italic: false,
                letter_spacing: 0.0,
                word_spacing: 0.0,
            },
        );
    let preedit_width = measure_text_width_with_font("xy", 16.0, "default", 400, false, 0.0, 0.0);
    let underline_y = baseline_y + 16.0_f32 * 0.08 - (16.0_f32 * 0.06).max(1.0) / 2.0;

    let has_preedit_underline = draws.iter().any(|draw| match &draw.primitive {
        DrawPrimitive::Rect(x, y, w, _h, _color) => {
            (x - preedit_x).abs() < 0.3
                && (y - underline_y).abs() < 0.3
                && (w - preedit_width).abs() < 0.3
        }
        _ => false,
    });

    assert!(has_preedit_underline);
}

#[test]
fn test_render_text_input_selection_emits_highlight_rect() {
    let mut attrs = Attrs::default();
    attrs.content = Some("hello".to_string());
    attrs.font_size = Some(16.0);
    attrs.text_input_focused = Some(true);
    attrs.text_input_cursor = Some(4);
    attrs.text_input_selection_anchor = Some(1);

    let frame = Frame {
        x: 10.0,
        y: 20.0,
        width: 280.0,
        height: 40.0,
        content_width: 280.0,
        content_height: 40.0,
    };

    let tree = build_text_input_tree_with_frame(attrs, frame);
    let (_output, draws) = observe_output(&tree);

    let has_selection_rect = draws.iter().any(|draw| match &draw.primitive {
        DrawPrimitive::Rect(_x, _y, w, h, color) => {
            *color == TEXT_SELECTION_COLOR && *w > 0.0 && *h > 0.0
        }
        _ => false,
    });

    assert!(has_selection_rect);
}

#[test]
fn test_render_multiline_text_input_places_caret_on_wrapped_second_line() {
    let mut attrs = Attrs::default();
    attrs.content = Some("abcd".to_string());
    attrs.font_size = Some(16.0);
    attrs.text_input_focused = Some(true);
    attrs.text_input_cursor = Some(4);

    let frame = Frame {
        x: 10.0,
        y: 20.0,
        width: 16.0,
        height: 32.0,
        content_width: 16.0,
        content_height: 32.0,
    };

    let tree = build_multiline_tree_with_frame(attrs, frame);
    let (output, _draws) = observe_output(&tree);

    assert!(output.text_input_focused);

    let (_caret_x, caret_y, _caret_w, _caret_h) = output
        .text_input_cursor_area
        .expect("caret area should be present");

    assert!(caret_y > 20.0);
}

#[test]
fn test_render_scrollable_text_column_emits_text_commands() {
    let root_id = ElementId::from_term_bytes(vec![50]);
    let column_id = ElementId::from_term_bytes(vec![51]);
    let row1_id = ElementId::from_term_bytes(vec![52]);
    let row1_text_id = ElementId::from_term_bytes(vec![53]);
    let row2_id = ElementId::from_term_bytes(vec![54]);
    let row2_text_id = ElementId::from_term_bytes(vec![55]);

    let mut root_attrs = Attrs::default();
    root_attrs.background = Some(Background::Color(Color::Rgb {
        r: 45,
        g: 45,
        b: 65,
    }));
    root_attrs.scrollbar_y = Some(true);
    let mut root = Element::with_attrs(root_id.clone(), ElementKind::El, Vec::new(), root_attrs);
    root.children = vec![column_id.clone()];
    root.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 240.0,
        height: 140.0,
        content_width: 240.0,
        content_height: 180.0,
    });

    let mut column = Element::with_attrs(
        column_id.clone(),
        ElementKind::Column,
        Vec::new(),
        Attrs::default(),
    );
    column.children = vec![row1_id.clone(), row2_id.clone()];
    column.frame = Some(Frame {
        x: 10.0,
        y: 10.0,
        width: 220.0,
        height: 160.0,
        content_width: 220.0,
        content_height: 160.0,
    });

    let mut row1 = Element::with_attrs(
        row1_id.clone(),
        ElementKind::El,
        Vec::new(),
        Attrs::default(),
    );
    row1.children = vec![row1_text_id.clone()];
    row1.frame = Some(Frame {
        x: 10.0,
        y: 10.0,
        width: 220.0,
        height: 20.0,
        content_width: 220.0,
        content_height: 20.0,
    });

    let mut row1_text_attrs = Attrs::default();
    row1_text_attrs.content = Some("Scrollable item 1".to_string());
    row1_text_attrs.font_size = Some(12.0);
    row1_text_attrs.font_color = Some(Color::Named("white".to_string()));
    let mut row1_text =
        Element::with_attrs(row1_text_id, ElementKind::Text, Vec::new(), row1_text_attrs);
    row1_text.frame = Some(Frame {
        x: 10.0,
        y: 10.0,
        width: 110.0,
        height: 14.0,
        content_width: 110.0,
        content_height: 14.0,
    });

    let mut row2 = Element::with_attrs(
        row2_id.clone(),
        ElementKind::El,
        Vec::new(),
        Attrs::default(),
    );
    row2.children = vec![row2_text_id.clone()];
    row2.frame = Some(Frame {
        x: 10.0,
        y: 36.0,
        width: 220.0,
        height: 20.0,
        content_width: 220.0,
        content_height: 20.0,
    });

    let mut row2_text_attrs = Attrs::default();
    row2_text_attrs.content = Some("Scrollable item 2".to_string());
    row2_text_attrs.font_size = Some(12.0);
    row2_text_attrs.font_color = Some(Color::Named("white".to_string()));
    let mut row2_text =
        Element::with_attrs(row2_text_id, ElementKind::Text, Vec::new(), row2_text_attrs);
    row2_text.frame = Some(Frame {
        x: 10.0,
        y: 36.0,
        width: 110.0,
        height: 14.0,
        content_width: 110.0,
        content_height: 14.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(root_id);
    tree.insert(root);
    tree.insert(column);
    tree.insert(row1);
    tree.insert(row1_text);
    tree.insert(row2);
    tree.insert(row2_text);

    let draws = observe_tree(&tree);

    assert!(draws.iter().any(|draw| matches!(
        &draw.primitive,
        DrawPrimitive::TextWithFont(_, _, text, _, _, _, _, _) if text == "Scrollable item 1"
    )));
    assert!(draws.iter().any(|draw| matches!(
        &draw.primitive,
        DrawPrimitive::TextWithFont(_, _, text, _, _, _, _, _) if text == "Scrollable item 2"
    )));
}

#[test]
fn test_nearby_text_inherits_parent_font_context() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 0 }));
    attrs.font_size = Some(24.0);

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
    let mut nearby_attrs = Attrs::default();
    nearby_attrs.content = Some("Hi".to_string());
    mount_nearby(
        &mut tree,
        &host_id,
        NearbySlot::InFront,
        ElementKind::Text,
        nearby_attrs,
        Frame {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
            content_width: 20.0,
            content_height: 10.0,
        },
        21,
    );
    let draws = observe_tree(&tree);

    assert!(draws.iter().any(|draw| {
        matches!(
            &draw.primitive,
            DrawPrimitive::TextWithFont(_, _, text, font_size, _, _, _, _)
                if text == "Hi" && (*font_size - 24.0).abs() < f32::EPSILON
        )
    }));
}
