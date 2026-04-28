use super::super::*;
use super::common::*;

#[test]
fn test_layout_text() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some("Hello".to_string());
    attrs.font_size = Some(16.0);

    let el = make_element("text", ElementKind::Text, attrs);
    let root_id = el.id;
    tree.set_root_id(root_id);
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.layout.frame.unwrap();
    assert_eq!(frame.width, 40.0); // 5 chars * 8px
    assert_eq!(frame.height, 16.0); // font_size
}

#[test]
fn test_layout_text_letter_and_word_spacing() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some("a b".to_string());
    attrs.font_size = Some(10.0);
    attrs.font_letter_spacing = Some(2.0);
    attrs.font_word_spacing = Some(3.0);

    let el = make_element("text", ElementKind::Text, attrs);
    let root_id = el.id;
    tree.set_root_id(root_id);
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.layout.frame.unwrap();
    // 3 chars * 8 + letter spacing (2 gaps * 2) + word spacing (1 gap * 3)
    assert_eq!(frame.width, 31.0);
    assert_eq!(frame.height, 10.0);
}

#[test]
fn test_layout_multiline_defaults_to_one_line_minimum_height() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some(String::new());
    attrs.font_size = Some(16.0);

    let el = make_element("multiline", ElementKind::Multiline, attrs);
    let root_id = el.id;
    tree.set_root_id(root_id);
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let frame = tree.get(&root_id).unwrap().layout.frame.unwrap();
    assert_eq!(frame.height, 16.0);
    assert_eq!(frame.content_height, 16.0);
}

#[test]
fn test_layout_multiline_wraps_and_auto_grows_height() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some("abcd".to_string());
    attrs.width = Some(Length::Px(16.0));
    attrs.font_size = Some(16.0);

    let el = make_element("multiline", ElementKind::Multiline, attrs);
    let root_id = el.id;
    tree.set_root_id(root_id);
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let frame = tree.get(&root_id).unwrap().layout.frame.unwrap();
    assert_eq!(frame.width, 16.0);
    assert_eq!(frame.height, 32.0);
    assert_eq!(frame.content_height, 32.0);
}

#[test]
fn test_layout_multiline_respects_explicit_height_override() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some("abcd".to_string());
    attrs.width = Some(Length::Px(16.0));
    attrs.height = Some(Length::Px(16.0));
    attrs.font_size = Some(16.0);

    let el = make_element("multiline", ElementKind::Multiline, attrs);
    let root_id = el.id;
    tree.set_root_id(root_id);
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let frame = tree.get(&root_id).unwrap().layout.frame.unwrap();
    assert_eq!(frame.height, 16.0);
    assert_eq!(frame.content_height, 32.0);
}

#[test]
fn test_content_size_basic_element() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(50.0));

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id;
    tree.set_root_id(root_id);
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.layout.frame.unwrap();

    // For a basic element without children, content size equals frame size
    assert_eq!(frame.content_width, 100.0);
    assert_eq!(frame.content_height, 50.0);
}

#[test]
fn test_content_size_row_with_children() {
    let mut tree = ElementTree::new();

    // Row with 300px width, 3 children of 80px each + 10px spacing
    // Children: 80 + 10 + 80 + 10 + 80 = 260px total content width
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(300.0));
    row_attrs.height = Some(Length::Px(50.0));
    row_attrs.spacing = Some(10.0);

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let children: Vec<_> = (0..3)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(80.0));
                a.height = Some(Length::Px(30.0));
                a
            })
        })
        .collect();

    let child_ids: Vec<_> = children.iter().map(|c| c.id).collect();
    let row_id = row.id;
    row.children = child_ids;

    tree.set_root_id(row_id);
    tree.insert(row);
    for child in children {
        tree.insert(child);
    }

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().layout.frame.unwrap();

    // Frame size is the specified size
    assert_eq!(row_frame.width, 300.0);
    assert_eq!(row_frame.height, 50.0);

    // Content size reflects actual children layout
    // 80 + 10 + 80 + 10 + 80 = 260px content width
    // Max child height = 30px content height
    assert_eq!(row_frame.content_width, 260.0);
    assert_eq!(row_frame.content_height, 30.0);
}

#[test]
fn test_content_size_column_with_children() {
    let mut tree = ElementTree::new();

    // Column with 3 children of 30px each + 10px spacing
    // Children: 30 + 10 + 30 + 10 + 30 = 110px total content height
    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(100.0));
    col_attrs.height = Some(Length::Px(200.0));
    col_attrs.spacing = Some(10.0);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let children: Vec<_> = (0..3)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(80.0));
                a.height = Some(Length::Px(30.0));
                a
            })
        })
        .collect();

    let child_ids: Vec<_> = children.iter().map(|c| c.id).collect();
    let col_id = col.id;
    col.children = child_ids;

    tree.set_root_id(col_id);
    tree.insert(col);
    for child in children {
        tree.insert(child);
    }

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let col_frame = tree.get(&col_id).unwrap().layout.frame.unwrap();

    // Frame size is the specified size
    assert_eq!(col_frame.width, 100.0);
    assert_eq!(col_frame.height, 200.0);

    // Content height: 30 + 10 + 30 + 10 + 30 = 110px
    assert_eq!(col_frame.content_height, 110.0);
}

#[test]
fn test_content_size_scrollable_column() {
    let mut tree = ElementTree::new();

    // Scrollable column with content that would overflow
    // 5 children of 50px each + 10px spacing = 250 + 40 = 290px
    // But frame is constrained to 150px height
    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(100.0));
    col_attrs.height = Some(Length::Px(150.0));
    col_attrs.spacing = Some(10.0);
    col_attrs.scrollbar_y = Some(true); // Makes it scrollable

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let children: Vec<_> = (0..5)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(80.0));
                a.height = Some(Length::Px(50.0));
                a
            })
        })
        .collect();

    let child_ids: Vec<_> = children.iter().map(|c| c.id).collect();
    let col_id = col.id;
    col.children = child_ids;

    tree.set_root_id(col_id);
    tree.insert(col);
    for child in children {
        tree.insert(child);
    }

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let col_frame = tree.get(&col_id).unwrap().layout.frame.unwrap();

    // Frame stays at specified size (clipped/scrollable)
    assert_eq!(col_frame.width, 100.0);
    assert_eq!(col_frame.height, 150.0);

    // Content height reflects actual content: 5 * 50 + 4 * 10 = 290px
    assert_eq!(col_frame.content_height, 290.0);

    let col_layout = &tree.get(&col_id).unwrap().layout;
    assert_eq!(col_layout.scroll_y, 0.0);
    assert_eq!(col_layout.scroll_y_max, 140.0);
}

#[test]
fn test_content_size_el_with_child() {
    let mut tree = ElementTree::new();

    // El container with a child smaller than the container
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(150.0));

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(60.0));
        a
    });

    let child_id = child.id;
    let el_id = el.id;
    el.children = vec![child_id];

    tree.set_root_id(el_id);
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let el_frame = tree.get(&el_id).unwrap().layout.frame.unwrap();

    // Frame is the specified size
    assert_eq!(el_frame.width, 200.0);
    assert_eq!(el_frame.height, 150.0);

    // Content size reflects the child's dimensions
    assert_eq!(el_frame.content_width, 80.0);
    assert_eq!(el_frame.content_height, 60.0);
}

#[test]
fn test_content_size_image_intrinsic_includes_padding_and_border() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.image_size = Some((30.0, 20.0));
    attrs.padding = Some(Padding::Uniform(5.0));
    attrs.border_width = Some(BorderWidth::Uniform(2.0));

    let image = make_element("image", ElementKind::Image, attrs);
    let image_id = image.id;
    tree.set_root_id(image_id);
    tree.insert(image);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let frame = tree.get(&image_id).unwrap().layout.frame.unwrap();

    // Inset each side = padding(5) + border(2) = 7
    // width = image(30) + 14, height = image(20) + 14
    assert_eq!(frame.width, 44.0);
    assert_eq!(frame.height, 34.0);
    assert_eq!(frame.content_width, 44.0);
    assert_eq!(frame.content_height, 34.0);
}
