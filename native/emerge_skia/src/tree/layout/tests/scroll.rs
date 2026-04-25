use super::super::*;
use super::common::*;

#[test]
fn test_layout_preserves_scroll_offsets() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.scrollbar_y = Some(true);
    attrs.scroll_y = Some(40.0);
    attrs.height = Some(Length::Px(100.0));

    let mut root = make_element("root", ElementKind::Column, attrs);
    let root_id = root.id.clone();
    let mut child_attrs = Attrs::default();
    child_attrs.height = Some(Length::Px(200.0));
    let child = make_element("child", ElementKind::El, child_attrs);
    let child_id = child.id.clone();
    root.children = vec![child_id.clone()];

    tree.set_root_id(root_id.clone());
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(100.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let first = tree.get(&root_id).unwrap().layout.scroll_y;
    assert_eq!(first, 40.0);

    layout_tree(
        &mut tree,
        Constraint::new(100.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let second = tree.get(&root_id).unwrap().layout.scroll_y;
    assert_eq!(second, 40.0);
}

#[test]
fn test_layout_scroll_offset_clamps_when_max_shrinks() {
    let mut tree = ElementTree::new();

    let mut root_attrs = Attrs::default();
    root_attrs.scrollbar_x = Some(true);
    root_attrs.scroll_x = Some(60.0);
    root_attrs.width = Some(Length::Fill);
    root_attrs.height = Some(Length::Px(100.0));

    let mut root = make_element("root", ElementKind::Row, root_attrs);

    let mut child_attrs = Attrs::default();
    child_attrs.width = Some(Length::Px(200.0));
    child_attrs.height = Some(Length::Px(50.0));
    let child = make_element("child", ElementKind::El, child_attrs);

    let root_id = root.id.clone();
    let child_id = child.id.clone();
    root.children = vec![child_id.clone()];

    tree.set_root_id(root_id.clone());
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(100.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let first = tree.get(&root_id).unwrap();
    assert_eq!(first.layout.scroll_x_max, 100.0);
    assert_eq!(first.layout.scroll_x, 60.0);

    layout_tree(
        &mut tree,
        Constraint::new(250.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let second = tree.get(&root_id).unwrap();
    assert_eq!(second.layout.scroll_x_max, 0.0);
    assert_eq!(second.layout.scroll_x, 0.0);
}

#[test]
fn test_layout_scroll_offset_stays_start_when_max_grows_from_zero() {
    let mut tree = ElementTree::new();

    let mut root_attrs = Attrs::default();
    root_attrs.scrollbar_x = Some(true);
    root_attrs.scroll_x = Some(0.0);
    root_attrs.width = Some(Length::Fill);
    root_attrs.height = Some(Length::Px(100.0));

    let mut root = make_element("root", ElementKind::Row, root_attrs);

    let mut child_attrs = Attrs::default();
    child_attrs.width = Some(Length::Px(200.0));
    child_attrs.height = Some(Length::Px(50.0));
    let child = make_element("child", ElementKind::El, child_attrs);

    let root_id = root.id.clone();
    let child_id = child.id.clone();
    root.children = vec![child_id.clone()];

    tree.set_root_id(root_id.clone());
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(250.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let first = tree.get(&root_id).unwrap();
    assert_eq!(first.layout.scroll_x_max, 0.0);
    assert_eq!(first.layout.scroll_x, 0.0);

    layout_tree(
        &mut tree,
        Constraint::new(100.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let second = tree.get(&root_id).unwrap();
    assert_eq!(second.layout.scroll_x_max, 100.0);
    assert_eq!(second.layout.scroll_x, 0.0);
}

#[test]
fn test_layout_clears_scroll_when_scrollbar_disabled() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.scrollbar_x = Some(false);
    attrs.scroll_x = Some(30.0);
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(100.0));

    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id.clone();
    tree.set_root_id(root_id.clone());
    tree.insert(root);

    layout_tree(
        &mut tree,
        Constraint::new(100.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );
    let root = tree.get(&root_id).unwrap();
    assert_eq!(root.layout.scroll_x, 0.0);
    assert_eq!(root.layout.scroll_x_max, 0.0);
}

#[test]
fn test_scrollable_el_uses_child_frame_height_for_content() {
    let mut tree = ElementTree::new();

    let mut el_attrs = Attrs::default();
    el_attrs.width = Some(Length::Px(100.0));
    el_attrs.height = Some(Length::Px(30.0));
    el_attrs.scrollbar_y = Some(true);

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let mut col_attrs = Attrs::default();
    col_attrs.spacing = Some(10.0);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let top = make_element("top", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let bottom = make_element("bottom", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(80.0));
        a.height = Some(Length::Px(20.0));
        a.align_y = Some(AlignY::Bottom);
        a
    });

    let el_id = el.id.clone();
    let col_id = col.id.clone();
    let top_id = top.id.clone();
    let bottom_id = bottom.id.clone();

    col.children = vec![top_id.clone(), bottom_id.clone()];
    el.children = vec![col_id.clone()];

    tree.set_root_id(el_id.clone());
    tree.insert(el);
    tree.insert(col);
    tree.insert(top);
    tree.insert(bottom);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let el_frame = tree.get(&el_id).unwrap().layout.frame.unwrap();
    let col_frame = tree.get(&col_id).unwrap().layout.frame.unwrap();

    // Column includes spacing between top and bottom alignment zones.
    assert_eq!(col_frame.height, 50.0);
    assert_eq!(col_frame.content_height, 50.0);

    // Scrollable parent should size content from child frame height.
    assert_eq!(el_frame.content_height, 50.0);

    let el_layout = &tree.get(&el_id).unwrap().layout;
    assert_eq!(el_layout.scroll_y_max, 20.0);
}

#[test]
fn test_layout_clears_only_disabled_scroll_axis() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(80.0));
    attrs.scrollbar_x = Some(true);
    attrs.scrollbar_y = Some(false);
    attrs.scroll_x = Some(12.0);
    attrs.scroll_y = Some(15.0);

    let mut root = make_element("root", ElementKind::El, attrs);
    let child = make_element("child", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(300.0));
        a.height = Some(Length::Px(200.0));
        a
    });

    let root_id = root.id.clone();
    let child_id = child.id.clone();
    root.children = vec![child_id.clone()];

    tree.set_root_id(root_id.clone());
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let layout = &tree.get(&root_id).unwrap().layout;

    // X axis stays active and clamped.
    assert_eq!(layout.scroll_x, 12.0);
    assert_eq!(layout.scroll_x_max, 200.0);

    // Disabled Y axis is cleared.
    assert_eq!(layout.scroll_y, 0.0);
    assert_eq!(layout.scroll_y_max, 0.0);
}
