use super::super::*;
use super::common::*;

#[test]
fn test_layout_single_el() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(50.0));

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.frame.unwrap();
    assert_eq!(frame.x, 0.0);
    assert_eq!(frame.y, 0.0);
    assert_eq!(frame.width, 100.0);
    assert_eq!(frame.height, 50.0);
}

#[test]
fn test_layout_el_shrink_to_content() {
    let mut tree = ElementTree::new();

    let mut parent_attrs = Attrs::default();
    parent_attrs.width = Some(Length::Content);
    parent_attrs.height = Some(Length::Content);
    parent_attrs.padding = Some(Padding::Uniform(10.0));

    let mut child_attrs = Attrs::default();
    child_attrs.content = Some("Hi".to_string());
    child_attrs.font_size = Some(10.0);

    let mut parent = make_element("root", ElementKind::El, parent_attrs);
    let child = make_element("child", ElementKind::Text, child_attrs);
    let root_id = parent.id.clone();
    let child_id = child.id.clone();

    parent.children = vec![child_id.clone()];
    tree.root = Some(root_id.clone());
    tree.insert(parent);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let frame = tree.get(&root_id).unwrap().frame.unwrap();
    assert_eq!(frame.width, 36.0); // 2 chars * 8px + 20 padding
    assert_eq!(frame.height, 30.0); // font_size 10 + 20 padding
}

#[test]
fn test_layout_minimum_constraint() {
    let mut tree = ElementTree::new();

    // Element with width = minimum(200, fill())
    // When constraint is 800px, fill() = 800px, but minimum clamps to at least 200px
    // Result should be 800px (fill wins since 800 > 200)
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Minimum(200.0, Box::new(Length::Fill)));
    attrs.height = Some(Length::Px(50.0));

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.frame.unwrap();
    assert_eq!(frame.width, 800.0); // fill() = 800, 800 >= 200, so 800
}

#[test]
fn test_layout_minimum_constraint_enforced() {
    let mut tree = ElementTree::new();

    // Element with width = minimum(200, content)
    // When content is small, minimum should enforce 200px
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Minimum(200.0, Box::new(Length::Content)));
    attrs.height = Some(Length::Px(50.0));

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.frame.unwrap();
    assert_eq!(frame.width, 200.0); // content = 0, minimum enforces 200
}

#[test]
fn test_layout_maximum_constraint() {
    let mut tree = ElementTree::new();

    // Element with width = maximum(300, fill())
    // When constraint is 800px, fill() = 800px, but maximum clamps to 300px
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Maximum(300.0, Box::new(Length::Fill)));
    attrs.height = Some(Length::Px(50.0));

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.frame.unwrap();
    assert_eq!(frame.width, 300.0); // fill() = 800, clamped to max 300
}

#[test]
fn test_available_space_resolve() {
    // Definite resolves to its value
    let definite = AvailableSpace::Definite(100.0);
    assert_eq!(definite.resolve(50.0), 100.0);

    // MinContent resolves to default
    let min_content = AvailableSpace::MinContent;
    assert_eq!(min_content.resolve(50.0), 50.0);

    // MaxContent resolves to default
    let max_content = AvailableSpace::MaxContent;
    assert_eq!(max_content.resolve(50.0), 50.0);
}

#[test]
fn test_available_space_is_definite() {
    assert!(AvailableSpace::Definite(100.0).is_definite());
    assert!(!AvailableSpace::MinContent.is_definite());
    assert!(!AvailableSpace::MaxContent.is_definite());
}

#[test]
fn test_constraint_max_methods() {
    let constraint = Constraint::new(800.0, 600.0);
    assert_eq!(constraint.max_width(100.0), 800.0);
    assert_eq!(constraint.max_height(100.0), 600.0);

    // With content-based constraints
    let content_constraint =
        Constraint::with_space(AvailableSpace::MaxContent, AvailableSpace::MinContent);
    // Should resolve to the default values
    assert_eq!(content_constraint.max_width(150.0), 150.0);
    assert_eq!(content_constraint.max_height(200.0), 200.0);
}

#[test]
fn test_available_space_from_f32() {
    let space: AvailableSpace = 100.0.into();
    assert_eq!(space, AvailableSpace::Definite(100.0));
}

#[test]
fn test_el_center_x_aligns_child() {
    let mut tree = ElementTree::new();

    // El with center_x alignment and a smaller child
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(50.0));
    el_attrs.align_x = Some(AlignX::Center);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Child should be centered horizontally: (200 - 80) / 2 = 60
    assert_eq!(child_frame.x, 60.0);
    // Child should be at top (default align_y is Top)
    assert_eq!(child_frame.y, 0.0);
}

#[test]
fn test_el_center_y_aligns_child() {
    let mut tree = ElementTree::new();

    // El with center_y alignment and a smaller child
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(100.0));
    el_attrs.align_y = Some(AlignY::Center);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(40.0));
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Child should be at left (default align_x is Left)
    assert_eq!(child_frame.x, 0.0);
    // Child should be centered vertically: (100 - 40) / 2 = 30
    assert_eq!(child_frame.y, 30.0);
}

#[test]
fn test_el_center_both_axes() {
    let mut tree = ElementTree::new();

    // El with both center_x and center_y
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(100.0));
    el_attrs.align_x = Some(AlignX::Center);
    el_attrs.align_y = Some(AlignY::Center);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(40.0));
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Child should be centered: (200 - 80) / 2 = 60, (100 - 40) / 2 = 30
    assert_eq!(child_frame.x, 60.0);
    assert_eq!(child_frame.y, 30.0);
}

#[test]
fn test_el_align_right() {
    let mut tree = ElementTree::new();

    // El with align_right
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(50.0));
    el_attrs.align_x = Some(AlignX::Right);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Child should be right-aligned: 200 - 80 = 120
    assert_eq!(child_frame.x, 120.0);
}

#[test]
fn test_el_align_bottom() {
    let mut tree = ElementTree::new();

    // El with align_bottom
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(100.0));
    el_attrs.align_y = Some(AlignY::Bottom);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(40.0));
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Child should be bottom-aligned: 100 - 40 = 60
    assert_eq!(child_frame.y, 60.0);
}

#[test]
fn test_child_alignment_overrides_parent() {
    let mut tree = ElementTree::new();

    // Parent has center_x, child has align_right - child should win
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(50.0));
    el_attrs.align_x = Some(AlignX::Center);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(30.0));
        a.align_x = Some(AlignX::Right); // Child overrides parent
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Child should be right-aligned (override): 200 - 80 = 120
    assert_eq!(child_frame.x, 120.0);
}

#[test]
fn test_el_with_padding_and_center() {
    let mut tree = ElementTree::new();

    // El with padding and center alignment
    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(200.0));
    el_attrs.height = Some(Length::Px(100.0));
    el_attrs.padding = Some(Padding::Uniform(20.0));
    el_attrs.align_x = Some(AlignX::Center);
    el_attrs.align_y = Some(AlignY::Center);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(60.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let el_id = el.id.clone();
    let child_id = child.id.clone();
    el.children = vec![child_id.clone()];

    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Content area: 200 - 40 = 160 width, 100 - 40 = 60 height
    // Child centered in content area:
    // x = 20 (padding) + (160 - 60) / 2 = 20 + 50 = 70
    // y = 20 (padding) + (60 - 20) / 2 = 20 + 20 = 40
    assert_eq!(child_frame.x, 70.0);
    assert_eq!(child_frame.y, 40.0);
}

#[test]
fn test_el_expands_height_for_wrapped_paragraph() {
    // El with width=50, no explicit height, containing a paragraph with text
    // wider than 50px. The paragraph wraps, and the El should expand.
    let mut tree = ElementTree::new();

    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(50.0));
    // No height set — should shrink-to-fit then expand

    let mut el = make_element("el", ElementKind::El, el_attrs);
    let el_id = el.id.clone();

    // Paragraph child, no explicit width/height (inherits from parent)
    let para_attrs = Attrs::default();
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);
    let para_id = para.id.clone();

    // Text child: "AAAA BBBB" = 9 chars * 8px = 72px wide
    // In 50px container: "AAAA" (32px) fits line 1, "BBBB" (32px) fits line 2
    // => 2 lines * 16px = 32px tall
    let text_child = make_element("txt", ElementKind::Text, text_attrs("AAAA BBBB"));
    let text_id = text_child.id.clone();

    para.children = vec![text_id.clone()];
    el.children = vec![para_id.clone()];
    tree.root = Some(el_id.clone());
    tree.insert(el);
    tree.insert(para);
    tree.insert(text_child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let el_frame = tree.get(&el_id).unwrap().frame.unwrap();
    // El should expand to fit the wrapped paragraph: 2 lines * 16px = 32px
    assert_eq!(el_frame.height, 32.0);
    assert_eq!(el_frame.width, 50.0);
}

#[test]
fn test_text_align_center_without_explicit_width_fills_constraint() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some("Hello".to_string());
    attrs.font_size = Some(16.0);
    attrs.text_align = Some(TextAlign::Center);
    // width intentionally unset

    let text = make_element("text", ElementKind::Text, attrs);
    let text_id = text.id.clone();
    tree.root = Some(text_id.clone());
    tree.insert(text);

    layout_tree(
        &mut tree,
        Constraint::new(200.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );

    let frame = tree.get(&text_id).unwrap().frame.unwrap();
    // Text with non-left alignment should fill available width.
    assert_eq!(frame.width, 200.0);
    assert_eq!(frame.height, 16.0);
}

#[test]
fn test_el_alignment_uses_asymmetric_padding_and_border_content_box() {
    let mut tree = ElementTree::new();

    let mut parent_attrs = Attrs::default();
    parent_attrs.width = Some(Length::Px(200.0));
    parent_attrs.height = Some(Length::Px(120.0));
    parent_attrs.padding = Some(Padding::Sides {
        top: 10.0,
        right: 20.0,
        bottom: 30.0,
        left: 40.0,
    });
    parent_attrs.border_width = Some(BorderWidth::Sides {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    });
    parent_attrs.align_x = Some(AlignX::Right);
    parent_attrs.align_y = Some(AlignY::Bottom);

    let mut parent = make_element("parent", ElementKind::El, parent_attrs);
    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let parent_id = parent.id.clone();
    let child_id = child.id.clone();
    parent.children = vec![child_id.clone()];

    tree.root = Some(parent_id.clone());
    tree.insert(parent);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

    // Content box:
    // x = 40 + 4 = 44
    // y = 10 + 1 = 11
    // w = 200 - (40+20+4+2) = 134
    // h = 120 - (10+30+1+3) = 76
    // right/bottom aligned child(50x20):
    // x = 44 + 134 - 50 = 128
    // y = 11 + 76 - 20 = 67
    assert_eq!(child_frame.x, 128.0);
    assert_eq!(child_frame.y, 67.0);
}
