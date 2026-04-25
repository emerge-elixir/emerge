use super::common::*;
use super::*;
use crate::events::registry_builder;

#[test]
fn test_render_paragraph_emits_text_commands() {
    use crate::tree::attrs::TextFragment;

    let mut attrs = Attrs::default();
    attrs.paragraph_fragments = Some(vec![
        TextFragment {
            x: 10.0,
            y: 5.0,
            text: "Hello".to_string(),
            font_size: 16.0,
            color: 0xFFFFFFFF,
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
            color: 0xFF0000FF,
            family: "default".to_string(),
            weight: 700,
            italic: false,
            underline: false,
            strike: false,
            ascent: 12.0,
        },
    ]);

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
                Some((*x, *y, text.clone(), *color, *weight))
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

    let mut para_attrs = Attrs::default();
    para_attrs.paragraph_fragments = Some(vec![TextFragment {
        x: 24.0,
        y: 8.0,
        text: "AA".to_string(),
        font_size: 16.0,
        color: 0xFFFFFFFF,
        family: "default".to_string(),
        weight: 400,
        italic: false,
        underline: false,
        strike: false,
        ascent: 12.0,
    }]);

    let mut paragraph = Element::with_attrs(
        para_id.clone(),
        ElementKind::Paragraph,
        Vec::new(),
        para_attrs,
    );
    paragraph.children = vec![float_id.clone()];
    paragraph.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 40.0,
        content_width: 120.0,
        content_height: 40.0,
    });

    let mut float_attrs = Attrs::default();
    float_attrs.align_x = Some(AlignX::Left);
    float_attrs.background = Some(Background::Color(Color::Rgb { r: 255, g: 0, b: 0 }));
    let mut float_el =
        Element::with_attrs(float_id.clone(), ElementKind::El, Vec::new(), float_attrs);
    float_el.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
        content_width: 20.0,
        content_height: 20.0,
    });

    let mut tree = ElementTree::new();
    tree.set_root_id(para_id.clone());
    tree.insert(paragraph);
    tree.insert(float_el);

    let draws = observe_tree(&tree);

    let float_draw = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 20.0, 20.0, 0xFF0000FF)
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

    let mut para_attrs = Attrs::default();
    para_attrs.paragraph_fragments = Some(vec![TextFragment {
        x: 20.0,
        y: 6.0,
        text: "AA".to_string(),
        font_size: 16.0,
        color: 0xFFFFFFFF,
        family: "default".to_string(),
        weight: 400,
        italic: false,
        underline: false,
        strike: false,
        ascent: 12.0,
    }]);
    let mut paragraph = Element::with_attrs(
        para_id.clone(),
        ElementKind::Paragraph,
        Vec::new(),
        para_attrs,
    );
    paragraph.children = vec![inline_id.clone(), float_id.clone()];
    paragraph.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 40.0,
        content_width: 120.0,
        content_height: 40.0,
    });

    let mut float_attrs = Attrs::default();
    float_attrs.align_x = Some(AlignX::Left);
    float_attrs.on_mouse_down = Some(true);
    let mut float_el =
        Element::with_attrs(float_id.clone(), ElementKind::El, Vec::new(), float_attrs);
    float_el.layout.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
        content_width: 20.0,
        content_height: 20.0,
    });

    let mut inline_attrs = Attrs::default();
    inline_attrs.on_mouse_down = Some(true);
    let mut inline_el = Element::with_attrs(
        inline_id.clone(),
        ElementKind::Text,
        Vec::new(),
        inline_attrs,
    );
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
    tree.set_root_id(para_id.clone());
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
        .filter_map(|listener| listener.element_id.clone())
        .collect();

    let direct_press_ids: Vec<_> = registry_builder::registry_for_elements(&direct_elements)
        .view()
        .iter_precedence()
        .filter(|listener| {
            listener.matcher.kind() == ListenerMatcherKind::CursorButtonLeftPressInside
        })
        .filter_map(|listener| listener.element_id.clone())
        .collect();

    assert_eq!(
        piggyback_press_ids,
        vec![inline_id.clone(), float_id.clone()]
    );
    assert_eq!(piggyback_press_ids, direct_press_ids);
}

#[test]
fn test_render_paragraph_underline_and_strike() {
    use crate::tree::attrs::TextFragment;

    let mut attrs = Attrs::default();
    attrs.paragraph_fragments = Some(vec![TextFragment {
        x: 10.0,
        y: 5.0,
        text: "Decorated".to_string(),
        font_size: 18.0,
        color: 0x010203FF,
        family: "default".to_string(),
        weight: 400,
        italic: false,
        underline: true,
        strike: true,
        ascent: 14.0,
    }]);

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
        .filter(|draw| matches!(draw.primitive, DrawPrimitive::Rect(_, _, _, _, 0x010203FF)))
        .collect();
    assert_eq!(decoration_rects.len(), 2);
}

#[test]
fn test_render_paragraph_no_fragments() {
    let mut attrs = Attrs::default();
    attrs.paragraph_fragments = Some(vec![]);

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

    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb { r: 0, g: 0, b: 128 }));
    attrs.paragraph_fragments = Some(vec![TextFragment {
        x: 0.0,
        y: 0.0,
        text: "Hi".to_string(),
        font_size: 16.0,
        color: 0xFFFFFFFF,
        family: "default".to_string(),
        weight: 400,
        italic: false,
        underline: false,
        strike: false,
        ascent: 12.0,
    }]);

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
            DrawPrimitive::Rect(_, _, 100.0, 20.0, 0x000080FF)
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
