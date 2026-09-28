use super::super::*;
use super::common::*;

#[test]
fn test_border_box_el_insets_child() {
    let mut tree = ElementTree::new();

    let root_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        height: Some(Length::Px(100.0)),
        border_width: Some(BorderWidth::Uniform(5.0)),
        ..Attrs::default()
    };

    let mut root = make_element("root", ElementKind::El, root_attrs);
    let child = make_element("child", ElementKind::El, {
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Attrs::default()
        }
    });

    let root_id = root.id;
    let child_id = child.id;
    root.children = vec![child_id];
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().layout.frame.unwrap();
    assert_eq!(child_frame.x, 5.0);
    assert_eq!(child_frame.y, 5.0);
    assert_eq!(child_frame.width, 190.0);
    assert_eq!(child_frame.height, 90.0);
}

#[test]
fn test_border_box_el_with_padding_and_border() {
    let mut tree = ElementTree::new();

    let root_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        height: Some(Length::Px(100.0)),
        padding: Some(Padding::Uniform(10.0)),
        border_width: Some(BorderWidth::Uniform(5.0)),
        ..Attrs::default()
    };

    let mut root = make_element("root", ElementKind::El, root_attrs);
    let child = make_element("child", ElementKind::El, {
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Attrs::default()
        }
    });

    let root_id = root.id;
    let child_id = child.id;
    root.children = vec![child_id];
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().layout.frame.unwrap();
    assert_eq!(child_frame.x, 15.0); // 10 padding + 5 border
    assert_eq!(child_frame.y, 15.0);
    assert_eq!(child_frame.width, 170.0); // 200 - 2*(10+5)
    assert_eq!(child_frame.height, 70.0); // 100 - 2*(10+5)
}

#[test]
fn test_border_box_per_side_border_width() {
    let mut tree = ElementTree::new();

    let root_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        height: Some(Length::Px(100.0)),
        border_width: Some(BorderWidth::Sides {
            top: 2.0,
            right: 4.0,
            bottom: 6.0,
            left: 8.0,
        }),
        ..Attrs::default()
    };

    let mut root = make_element("root", ElementKind::El, root_attrs);
    let child = make_element("child", ElementKind::El, {
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Attrs::default()
        }
    });

    let root_id = root.id;
    let child_id = child.id;
    root.children = vec![child_id];
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().layout.frame.unwrap();
    assert_eq!(child_frame.x, 8.0);
    assert_eq!(child_frame.y, 2.0);
    assert_eq!(child_frame.width, 188.0); // 200 - 8 - 4
    assert_eq!(child_frame.height, 92.0); // 100 - 2 - 6
}

#[test]
fn test_border_box_intrinsic_sizing() {
    let mut tree = ElementTree::new();

    let root_attrs = Attrs {
        border_width: Some(BorderWidth::Uniform(5.0)),
        ..Attrs::default()
    };

    let mut root = make_element("root", ElementKind::El, root_attrs);
    let child = make_element("child", ElementKind::Text, {
        Attrs {
            content: Some("Hello".to_string()),
            font_size: Some(16.0),
            ..Attrs::default()
        }
    });

    let root_id = root.id;
    let child_id = child.id;
    root.children = vec![child_id];
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let root_frame = tree.get(&root_id).unwrap().layout.frame.unwrap();
    // "Hello" = 5 chars * 8px = 40px wide, 16px tall (mock measurer)
    // + border: 5 left + 5 right = 10, 5 top + 5 bottom = 10
    assert_eq!(root_frame.width, 50.0);
    assert_eq!(root_frame.height, 26.0);
}

#[test]
fn test_border_box_row_intrinsic_with_border() {
    let mut tree = ElementTree::new();

    let row_attrs = Attrs {
        spacing: Some(4.0),
        border_width: Some(BorderWidth::Uniform(3.0)),
        ..Attrs::default()
    };

    let mut row = make_element("row", ElementKind::Row, row_attrs);
    let child1 = make_element("c1", ElementKind::Text, {
        Attrs {
            content: Some("Hi".to_string()),
            font_size: Some(16.0),
            ..Attrs::default()
        }
    });
    let child2 = make_element("c2", ElementKind::Text, {
        Attrs {
            content: Some("Lo".to_string()),
            font_size: Some(16.0),
            ..Attrs::default()
        }
    });

    let row_id = row.id;
    let c1_id = child1.id;
    let c2_id = child2.id;
    row.children = vec![c1_id, c2_id];
    tree.set_root_id(row_id);
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().layout.frame.unwrap();
    // "Hi" = 16px, "Lo" = 16px, spacing = 4px => content = 36px
    // + border: 3 left + 3 right = 6
    assert_eq!(row_frame.width, 42.0);
    // height = max child height (16) + 3 top + 3 bottom = 22
    assert_eq!(row_frame.height, 22.0);
}

#[test]
fn test_border_box_shadows_do_not_affect_layout() {
    use crate::tree::attrs::BoxShadow;

    let mut tree = ElementTree::new();

    let root_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        height: Some(Length::Px(100.0)),
        box_shadows: Some(vec![BoxShadow {
            offset_x: 10.0,
            offset_y: 10.0,
            blur: 20.0,
            size: 5.0,
            color: Color::Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            inset: false,
        }]),
        ..Attrs::default()
    };

    let mut root = make_element("root", ElementKind::El, root_attrs);
    let child = make_element("child", ElementKind::El, {
        Attrs {
            width: Some(Length::Fill),
            height: Some(Length::Fill),
            ..Attrs::default()
        }
    });

    let root_id = root.id;
    let child_id = child.id;
    root.children = vec![child_id];
    tree.set_root_id(root_id);
    tree.insert(root);
    tree.insert(child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let child_frame = tree.get(&child_id).unwrap().layout.frame.unwrap();
    // Shadows should NOT inset children
    assert_eq!(child_frame.x, 0.0);
    assert_eq!(child_frame.y, 0.0);
    assert_eq!(child_frame.width, 200.0);
    assert_eq!(child_frame.height, 100.0);
}

#[test]
fn content_sized_el_reserves_fixed_child_border_box_not_descendant_content() {
    for scale in [1.0, 1.5, 2.0] {
        for kind in [ElementKind::Row, ElementKind::Column] {
            for explicit_content in [false, true] {
                for label in ["", "x", "content wider than the fixed child"] {
                    for (child_width, child_height) in [(28.0, 28.0), (40.0, 18.0)] {
                        let mut root = make_element("root", kind, fixed_box_attrs(300.0, 160.0));
                        let mut wrapper = make_element(
                            "wrapper",
                            ElementKind::El,
                            Attrs {
                                width: explicit_content.then_some(Length::Content),
                                height: explicit_content.then_some(Length::Content),
                                padding: Some(Padding::Uniform(12.0)),
                                border_width: Some(BorderWidth::Uniform(1.0)),
                                ..Attrs::default()
                            },
                        );
                        let mut child = make_element(
                            "fixed_child",
                            ElementKind::El,
                            Attrs {
                                align_x: Some(AlignX::Center),
                                align_y: Some(AlignY::Center),
                                border_width: Some(BorderWidth::Uniform(1.0)),
                                ..fixed_box_attrs(child_width, child_height)
                            },
                        );
                        let text = make_element("label", ElementKind::Text, text_attrs(label));
                        let sibling =
                            make_element("sibling", ElementKind::El, fixed_box_attrs(20.0, 20.0));
                        let [root_id, wrapper_id, child_id, sibling_id] =
                            [root.id, wrapper.id, child.id, sibling.id];
                        root.children = vec![wrapper_id, sibling_id];
                        wrapper.children = vec![child_id];
                        child.children = vec![text.id];
                        let mut tree = ElementTree::new();
                        tree.set_root_id(root_id);
                        [root, wrapper, child, text, sibling]
                            .into_iter()
                            .for_each(|element| tree.insert(element));

                        // Also exercise a warm layout-cache pass. Text content must not change
                        // the footprint of the fixed child (the todo's empty and checked states).
                        for _ in 0..2 {
                            layout_tree(
                                &mut tree,
                                Constraint::new(900.0, 600.0),
                                scale,
                                &MockTextMeasurer,
                            );
                            let wrapper = tree.get(&wrapper_id).unwrap().layout.frame.unwrap();
                            let child = tree.get(&child_id).unwrap().layout.frame.unwrap();
                            let sibling = tree.get(&sibling_id).unwrap().layout.frame.unwrap();
                            let context = format!(
                                "{kind:?} scale={scale} explicit_content={explicit_content} label={label:?}"
                            );
                            assert_eq!(
                                wrapper.width,
                                (child_width as f32 + 26.0) * scale,
                                "{context}"
                            );
                            assert_eq!(
                                wrapper.height,
                                (child_height as f32 + 26.0) * scale,
                                "{context}"
                            );
                            assert_eq!(child.width, child_width as f32 * scale, "{context}");
                            assert_eq!(child.height, child_height as f32 * scale, "{context}");
                            assert_eq!(child.x - wrapper.x, 13.0 * scale, "{context}");
                            assert_eq!(child.y - wrapper.y, 13.0 * scale, "{context}");
                            match kind {
                                ElementKind::Row => {
                                    assert_eq!(sibling.x, wrapper.x + wrapper.width, "{context}")
                                }
                                ElementKind::Column => {
                                    assert_eq!(sibling.y, wrapper.y + wrapper.height, "{context}")
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                }
            }
        }
    }
}
