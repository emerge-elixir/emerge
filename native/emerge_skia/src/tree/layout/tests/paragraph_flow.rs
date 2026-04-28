use super::super::*;
use super::common::*;
use crate::tree::attrs::FontWeight;

fn build_paragraph(
    paragraph_attrs: Attrs,
    children: Vec<(&str, ElementKind, Attrs)>,
) -> (ElementTree, NodeId, Vec<NodeId>) {
    let mut tree = ElementTree::new();

    let mut para = make_element("para", ElementKind::Paragraph, paragraph_attrs);
    let para_id = para.id;

    let mut child_ids = Vec::new();
    for (i, (name, kind, attrs)) in children.into_iter().enumerate() {
        let child_name = format!("{}_{}", name, i);
        let child = make_element(&child_name, kind, attrs);
        child_ids.push(child.id);
        tree.insert(child);
    }

    para.children = child_ids.clone();
    tree.set_root_id(para_id);
    tree.insert(para);

    (tree, para_id, child_ids)
}

#[test]
fn test_line_spacing_row_pushes_following_heading() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(20.0));
    col_attrs.spacing = Some(12.0);
    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Fill);
    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let mut para_attrs = Attrs::default();
    para_attrs.width = Some(Length::Fill);
    para_attrs.spacing = Some(8.0);
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);

    let txt = make_element("txt", ElementKind::Text, text_attrs("AA BB"));

    let heading = make_element("heading", ElementKind::Text, text_attrs("Document Style"));

    let col_id = col.id;
    let row_id = row.id;
    let para_id = para.id;
    let txt_id = txt.id;
    let heading_id = heading.id;

    para.children = vec![txt_id];
    row.children = vec![para_id];
    col.children = vec![row_id, heading_id];

    tree.set_root_id(col_id);
    tree.insert(col);
    tree.insert(row);
    tree.insert(para);
    tree.insert(txt);
    tree.insert(heading);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().layout.frame.unwrap();
    // Two lines with spacing(8): 16 + 8 + 16 = 40
    assert_eq!(row_frame.height, 40.0);

    let heading_frame = tree.get(&heading_id).unwrap().layout.frame.unwrap();
    // heading y = row height (40) + column spacing (12)
    assert_eq!(heading_frame.y, 52.0);
}

#[test]
fn test_paragraph_single_text_no_wrap() {
    // "Hello" = 5 chars * 8px = 40px, fits within 200px
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![("t", ElementKind::Text, text_attrs("Hello"))],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].text, "Hello");
    assert_eq!(fragments[0].x, 0.0);
    assert_eq!(fragments[0].y, 0.0);
}

#[test]
fn test_paragraph_wraps_words() {
    // "AA BB CC" -> 3 words: "AA" (16px), "BB" (16px), "CC" (16px)
    // space = 8px
    // Container 40px wide:
    // "AA" (16) fits, + space (8) + "BB" (16) = 40 fits
    // + space (8) + "CC" (16) = 64 > 40, "CC" wraps
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(40.0));
            a
        },
        vec![("t", ElementKind::Text, text_attrs("AA BB CC"))],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 3);

    // Line 1: "AA" at x=0, "BB" at x=24 (16+8)
    assert_eq!(fragments[0].text, "AA");
    assert_eq!(fragments[0].x, 0.0);
    assert_eq!(fragments[0].y, 0.0);

    assert_eq!(fragments[1].text, "BB");
    assert_eq!(fragments[1].x, 24.0);
    assert_eq!(fragments[1].y, 0.0);

    // Line 2: "CC" wraps to y=16 (font_size)
    assert_eq!(fragments[2].text, "CC");
    assert_eq!(fragments[2].x, 0.0);
    assert_eq!(fragments[2].y, 16.0);
}

#[test]
fn test_paragraph_align_left_float_wraps_then_releases_width() {
    let mut float_attrs = Attrs::default();
    float_attrs.align_x = Some(AlignX::Left);
    float_attrs.width = Some(Length::Px(24.0));
    float_attrs.height = Some(Length::Px(40.0));

    let (mut tree, para_id, child_ids) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a
        },
        vec![
            ("float", ElementKind::El, float_attrs),
            (
                "t",
                ElementKind::Text,
                text_attrs("AA BB CC DD EE FF GG HH"),
            ),
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let float_frame = tree.get(&child_ids[0]).unwrap().layout.frame.unwrap();
    assert_eq!(float_frame.x, 0.0);
    assert_eq!(float_frame.y, 0.0);
    assert_eq!(float_frame.width, 24.0);
    assert_eq!(float_frame.height, 40.0);

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert!(!fragments.is_empty());

    let first_fragment = &fragments[0];
    assert_eq!(first_fragment.text, "AA");
    assert_eq!(first_fragment.x, 24.0);
    assert_eq!(first_fragment.y, 0.0);

    let released = fragments
        .iter()
        .find(|fragment| fragment.text == "GG")
        .expect("expected GG fragment after float expires");
    assert_eq!(released.x, 0.0);
    assert!(released.y >= 40.0);
}

#[test]
fn test_text_column_non_paragraph_child_clears_below_active_floats() {
    let mut tree = ElementTree::new();

    let mut text_col_attrs = Attrs::default();
    text_col_attrs.width = Some(Length::Px(80.0));
    text_col_attrs.spacing_y = Some(8.0);
    let mut text_col = make_element("text_col", ElementKind::TextColumn, text_col_attrs);

    let mut float_attrs = Attrs::default();
    float_attrs.align_x = Some(AlignX::Left);
    float_attrs.width = Some(Length::Px(24.0));
    float_attrs.height = Some(Length::Px(40.0));
    let float_el = make_element("float", ElementKind::El, float_attrs);

    let mut para = make_element("para", ElementKind::Paragraph, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a
    });

    let para_text = make_element("para_text", ElementKind::Text, text_attrs("AA"));

    let below_block = make_element("below", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(10.0));
        a
    });

    let text_col_id = text_col.id;
    let float_id = float_el.id;
    let para_id = para.id;
    let para_text_id = para_text.id;
    let below_id = below_block.id;

    para.children = vec![para_text_id];
    text_col.children = vec![float_id, para_id, below_id];

    tree.set_root_id(text_col_id);
    tree.insert(text_col);
    tree.insert(float_el);
    tree.insert(para);
    tree.insert(para_text);
    tree.insert(below_block);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let float_frame = tree.get(&float_id).unwrap().layout.frame.unwrap();
    assert_eq!(float_frame.y, 0.0);
    assert_eq!(float_frame.height, 40.0);

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments[0].x, 24.0);
    assert_eq!(fragments[0].y, 8.0);

    let below_frame = tree.get(&below_id).unwrap().layout.frame.unwrap();
    assert_eq!(below_frame.y, 40.0);

    let text_col_frame = tree.get(&text_col_id).unwrap().layout.frame.unwrap();
    assert_eq!(text_col_frame.content_height, 50.0);
}

#[test]
fn test_paragraph_multiple_text_children() {
    // Two text children flow inline
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![
            ("t1", ElementKind::Text, text_attrs("Hello ")),
            ("t2", ElementKind::Text, text_attrs("World")),
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    // "Hello " -> word "Hello", trailing space
    // "World" -> word "World"
    assert_eq!(fragments.len(), 2);
    assert_eq!(fragments[0].text, "Hello");
    assert_eq!(fragments[1].text, "World");
    // "Hello" = 40px, + trailing space 8px = cursor at 48
    assert_eq!(fragments[1].x, 48.0);
}

#[test]
fn test_paragraph_line_spacing() {
    // "AA BB" with 40px container -> wraps
    // spacing_y = 5
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a.spacing = Some(5.0);
            a
        },
        vec![("t", ElementKind::Text, text_attrs("AA BB"))],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 2);

    // Line 1: "AA" at y=0
    assert_eq!(fragments[0].y, 0.0);
    // Line 2: "BB" at y = 16 (line height) + 5 (spacing) = 21
    assert_eq!(fragments[1].y, 21.0);
}

#[test]
fn test_paragraph_expands_height() {
    // Paragraph has no explicit height; wrapping should expand it
    // "AA BB" in 20px container -> 2 lines of 16px each
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a
        },
        vec![("t", ElementKind::Text, text_attrs("AA BB"))],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let frame = para.layout.frame.unwrap();
    // 2 lines * 16px = 32px
    assert_eq!(frame.height, 32.0);
}

#[test]
fn test_paragraph_with_padding() {
    // Paragraph with padding, text should be offset
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a.padding = Some(Padding::Uniform(10.0));
            a
        },
        vec![("t", ElementKind::Text, text_attrs("Hi"))],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 1);
    // Fragment at content_x = 0 + 10 (padding_left)
    assert_eq!(fragments[0].x, 10.0);
    assert_eq!(fragments[0].y, 10.0);
}

#[test]
fn test_paragraph_el_wrapped_text() {
    // el([Font.bold()], text("Bold")) should participate in paragraph flow
    let mut el_attrs = Attrs::default();
    el_attrs.font_weight = Some(FontWeight("bold".to_string()));

    let mut el_child = make_element("el_child", ElementKind::El, el_attrs);
    let text_child = make_element("el_text", ElementKind::Text, text_attrs("Bold"));
    let text_child_id = text_child.id;
    el_child.children = vec![text_child_id];

    let mut tree = ElementTree::new();
    let mut para_attrs = Attrs::default();
    para_attrs.width = Some(Length::Px(200.0));
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);
    let para_id = para.id;
    let el_id = el_child.id;

    // Also add a direct text child
    let plain_text = make_element("plain", ElementKind::Text, text_attrs("Hi "));
    let plain_id = plain_text.id;

    para.children = vec![plain_id, el_id];
    tree.set_root_id(para_id);
    tree.insert(para);
    tree.insert(plain_text);
    tree.insert(el_child);
    tree.insert(text_child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 2);
    assert_eq!(fragments[0].text, "Hi");
    assert_eq!(fragments[1].text, "Bold");
    // "Hi" trailing space makes cursor at 16+8=24
    assert_eq!(fragments[1].x, 24.0);
    // Bold child should inherit weight 700
    assert_eq!(fragments[1].weight, 700);
}

#[test]
fn test_paragraph_skips_non_text_children() {
    // Non-text, non-el children (e.g., Row) should be silently skipped
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(50.0));
    row_attrs.height = Some(Length::Px(20.0));

    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![
            ("t1", ElementKind::Text, text_attrs("Hi")),
            ("r", ElementKind::Row, row_attrs),
            ("t2", ElementKind::Text, text_attrs("There")),
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    // Row child should be skipped, only "Hi" and "There"
    assert_eq!(fragments.len(), 2);
    assert_eq!(fragments[0].text, "Hi");
    assert_eq!(fragments[1].text, "There");
}

#[test]
fn test_paragraph_empty_text_skipped() {
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![
            ("t1", ElementKind::Text, text_attrs("")),
            ("t2", ElementKind::Text, text_attrs("Hello")),
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].text, "Hello");
    assert_eq!(fragments[0].x, 0.0);
}

#[test]
fn test_paragraph_no_children() {
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert!(fragments.is_empty());
}

#[test]
fn test_paragraph_inherits_font_context() {
    // Paragraph sets font_size=20, child text has no font_size -> inherits 20
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a.font_size = Some(20.0);
            a
        },
        vec![("t", ElementKind::Text, {
            let mut a = Attrs::default();
            a.content = Some("Hi".to_string());
            // No font_size set -> should inherit from paragraph
            a
        })],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].font_size, 20.0);
}

#[test]
fn test_paragraph_intrinsic_width_is_sum_of_children() {
    // Without explicit width, paragraph intrinsic = sum of child widths
    let (mut tree, para_id, _) = build_paragraph(
        Attrs::default(),
        vec![
            ("t1", ElementKind::Text, text_attrs("AA")),   // 16px
            ("t2", ElementKind::Text, text_attrs("BBBB")), // 32px
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let frame = para.layout.frame.unwrap();
    // 16 + 32 = 48, constrained to 800 but intrinsic should be 48
    assert_eq!(frame.width, 48.0);
}

#[test]
fn test_paragraph_fragment_colors_from_children() {
    // Test that font_color from a child text element is used in fragment
    let mut child_attrs = Attrs::default();
    child_attrs.content = Some("Red".to_string());
    child_attrs.font_size = Some(16.0);
    child_attrs.font_color = Some(Color::Rgb { r: 255, g: 0, b: 0 });

    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![("t", ElementKind::Text, child_attrs)],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].color, 0xFF0000FF);
}

#[test]
fn test_paragraph_fragment_defaults_to_black() {
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(200.0));
            a
        },
        vec![("t", ElementKind::Text, text_attrs("Default"))],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].color, 0x000000FF);
}

#[test]
fn test_paragraph_inside_el_shifts_fragments() {
    // An el with padding=12 containing a paragraph with text.
    // The paragraph fragments should be offset by the parent's padding.
    let mut tree = ElementTree::new();

    // Parent el with explicit size and padding
    let mut parent_attrs = Attrs::default();
    parent_attrs.width = Some(Length::Px(400.0));
    parent_attrs.height = Some(Length::Px(200.0));
    parent_attrs.padding = Some(Padding::Uniform(12.0));

    let mut parent_el = make_element("parent", ElementKind::El, parent_attrs);
    let parent_id = parent_el.id;

    // Paragraph child
    let mut para_attrs = Attrs::default();
    para_attrs.width = Some(Length::Fill);
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);
    let para_id = para.id;

    // Text child of paragraph
    let text_child = make_element("t", ElementKind::Text, text_attrs("Hello world"));
    let text_id = text_child.id;

    // Wire up children
    para.children = vec![text_id];
    parent_el.children = vec![para_id];

    tree.set_root_id(parent_id);
    tree.insert(text_child);
    tree.insert(para);
    tree.insert(parent_el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para_el = tree.get(&para_id).unwrap();
    let para_frame = para_el.layout.frame.unwrap();
    // Paragraph frame should be at (12, 12) due to parent padding
    assert_eq!(para_frame.x, 12.0);
    assert_eq!(para_frame.y, 12.0);

    let fragments = para_el.layout.paragraph_fragments.as_ref().unwrap();
    assert!(!fragments.is_empty(), "paragraph should have fragments");
    // Fragment positions should also be offset by the parent padding
    assert_eq!(fragments[0].x, 12.0);
    assert_eq!(fragments[0].y, 12.0);
    assert_eq!(fragments[0].text, "Hello");
}

#[test]
fn test_paragraph_wraps_to_parent_constraint() {
    // A paragraph with NO explicit width inside an el with width=100px.
    // MockTextMeasurer: each char = 8px, space = 8px
    // "AA BB CC DD" -> "AA" (16) + " " (8) + "BB" (16) + " " (8) + "CC" (16) + " " (8) + "DD" (16) = 88px
    // But words must wrap within 100px parent constraint.
    // Line 1: "AA" (16) + space (8) + "BB" (16) + space (8) + "CC" (16) = 64, + space (8) + "DD" (16) = 88 fits in 100
    // Actually let's use a narrower parent: 50px
    // Line 1: "AA" (16) + space (8) + "BB" (16) = 40, fits in 50
    // + space (8) + "CC" (16) = 64 > 50, wraps
    // Line 2: "CC" (16) + space (8) + "DD" (16) = 40, fits in 50
    let mut tree = ElementTree::new();

    let mut parent_attrs = Attrs::default();
    parent_attrs.width = Some(Length::Px(50.0));

    let mut parent_el = make_element("parent", ElementKind::El, parent_attrs);
    let parent_id = parent_el.id;

    // Paragraph with NO explicit width
    let para_attrs = Attrs::default();
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);
    let para_id = para.id;

    let text_child = make_element("t", ElementKind::Text, text_attrs("AA BB CC DD"));
    let text_id = text_child.id;

    para.children = vec![text_id];
    parent_el.children = vec![para_id];

    tree.set_root_id(parent_id);
    tree.insert(text_child);
    tree.insert(para);
    tree.insert(parent_el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para_el = tree.get(&para_id).unwrap();
    let fragments = para_el.layout.paragraph_fragments.as_ref().unwrap();

    // Should wrap into 2 lines, not be a single line
    assert!(
        fragments.len() >= 3,
        "paragraph should wrap text into multiple lines, got {} fragments",
        fragments.len()
    );

    // Line 1 fragments should be at y=0
    assert_eq!(fragments[0].y, 0.0);
    assert_eq!(fragments[1].y, 0.0);
    // At least one fragment should be on line 2 (y > 0)
    let has_second_line = fragments.iter().any(|f| f.y > 0.0);
    assert!(
        has_second_line,
        "paragraph text should wrap to a second line"
    );
}

#[test]
fn test_paragraph_preserves_leading_space_between_nodes() {
    // Three text children: "Hello", " World", " End"
    // Leading spaces on " World" and " End" must be preserved.
    // MockTextMeasurer: each char = 8px, space = 8px
    // "Hello" = 40px
    // " World" -> leading space (8) + "World" (40) -> cursor at 88
    // " End" -> leading space (8) + "End" (24) -> cursor at 120
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(400.0));
            a
        },
        vec![
            ("t1", ElementKind::Text, text_attrs("Hello")),
            ("t2", ElementKind::Text, text_attrs(" World")),
            ("t3", ElementKind::Text, text_attrs(" End")),
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para.layout.paragraph_fragments.as_ref().unwrap();
    assert_eq!(fragments.len(), 3);
    assert_eq!(fragments[0].text, "Hello");
    assert_eq!(fragments[0].x, 0.0);
    // "Hello" = 40px, + leading space 8px = 48
    assert_eq!(fragments[1].text, "World");
    assert_eq!(fragments[1].x, 48.0);
    // "World" = 40px, cursor at 88, + leading space 8px = 96
    assert_eq!(fragments[2].text, "End");
    assert_eq!(fragments[2].x, 96.0);
}

#[test]
fn test_paragraph_left_and_right_floats_constrain_first_line_bounds() {
    let (mut tree, para_id, _) = build_paragraph(
        {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(100.0));
            a
        },
        vec![
            ("left_float", ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(20.0));
                a.height = Some(Length::Px(20.0));
                a.align_x = Some(AlignX::Left);
                a
            }),
            ("right_float", ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(20.0));
                a.height = Some(Length::Px(20.0));
                a.align_x = Some(AlignX::Right);
                a
            }),
            ("text", ElementKind::Text, text_attrs("AAAA BBBB CCCC DDDD")),
        ],
    );

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let para = tree.get(&para_id).unwrap();
    let fragments = para
        .layout
        .paragraph_fragments
        .as_ref()
        .expect("paragraph fragments should exist");
    assert!(!fragments.is_empty(), "expected paragraph fragments");

    // First line should be constrained to x range [20, 80] by left+right floats.
    let first_line_y = fragments[0].y;
    let first_line: Vec<_> = fragments
        .iter()
        .filter(|frag| (frag.y - first_line_y).abs() < 0.001)
        .collect();
    assert!(!first_line.is_empty(), "expected first-line fragments");

    for frag in first_line {
        let word_w = frag.text.len() as f32 * 8.0;
        assert!(
            frag.x >= 20.0,
            "first-line fragment starts before left float bound: x={} text={} ",
            frag.x,
            frag.text
        );
        assert!(
            frag.x + word_w <= 80.0,
            "first-line fragment overflows right float bound: x={} w={} text={} ",
            frag.x,
            word_w,
            frag.text
        );
    }
}
