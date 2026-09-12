use super::super::*;
use super::common::*;
use crate::tree::attrs::ImageFit;

#[test]
fn test_layout_text() {
    let mut tree = ElementTree::new();

    let attrs = Attrs {
        content: Some("Hello".to_string()),
        font_size: Some(16.0),
        ..Attrs::default()
    };

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

    let attrs = Attrs {
        content: Some("a b".to_string()),
        font_size: Some(10.0),
        font_letter_spacing: Some(2.0),
        font_word_spacing: Some(3.0),
        ..Attrs::default()
    };

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

    let attrs = Attrs {
        content: Some(String::new()),
        font_size: Some(16.0),
        ..Attrs::default()
    };

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

    let attrs = Attrs {
        content: Some("abcd".to_string()),
        width: Some(Length::Px(16.0)),
        font_size: Some(16.0),
        ..Attrs::default()
    };

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

    let attrs = Attrs {
        content: Some("abcd".to_string()),
        width: Some(Length::Px(16.0)),
        height: Some(Length::Px(16.0)),
        font_size: Some(16.0),
        ..Attrs::default()
    };

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

    let attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(50.0)),
        ..Attrs::default()
    };

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
    let row_attrs = Attrs {
        width: Some(Length::Px(300.0)),
        height: Some(Length::Px(50.0)),
        spacing: Some(10.0),
        ..Attrs::default()
    };

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let children: Vec<_> = (0..3)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                Attrs {
                    width: Some(Length::Px(80.0)),
                    height: Some(Length::Px(30.0)),
                    ..Attrs::default()
                }
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
    let col_attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(200.0)),
        spacing: Some(10.0),
        ..Attrs::default()
    };

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let children: Vec<_> = (0..3)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                Attrs {
                    width: Some(Length::Px(80.0)),
                    height: Some(Length::Px(30.0)),
                    ..Attrs::default()
                }
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
    let col_attrs = Attrs {
        width: Some(Length::Px(100.0)),
        height: Some(Length::Px(150.0)),
        spacing: Some(10.0),
        scrollbar_y: Some(true), // Makes it scrollable
        ..Attrs::default()
    };
    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let children: Vec<_> = (0..5)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                Attrs {
                    width: Some(Length::Px(80.0)),
                    height: Some(Length::Px(50.0)),
                    ..Attrs::default()
                }
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
    let el_attrs = Attrs {
        width: Some(Length::Px(200.0)),
        height: Some(Length::Px(150.0)),
        ..Attrs::default()
    };

    let mut el = make_element("el", ElementKind::El, el_attrs);

    let child = make_element("child", ElementKind::El, {
        Attrs {
            width: Some(Length::Px(80.0)),
            height: Some(Length::Px(60.0)),
            ..Attrs::default()
        }
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

    let attrs = Attrs {
        image_size: Some((30.0, 20.0)),
        padding: Some(Padding::Uniform(5.0)),
        border_width: Some(BorderWidth::Uniform(2.0)),
        ..Attrs::default()
    };

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

#[test]
fn test_image_single_pixel_axis_preserves_intrinsic_aspect_ratio() {
    for (source, fixed, expected) in [
        ((200.0, 200.0), 68.0, (68.0, 68.0)),
        ((200.0, 100.0), 68.0, (136.0, 68.0)),
        ((100.0, 200.0), 68.0, (34.0, 68.0)),
        ((200.0, 100.0), 300.0, (600.0, 300.0)),
        ((200.0, 100.0), 0.0, (0.0, 0.0)),
    ] {
        for transpose in [false, true] {
            for content in [None, Some(Length::Content)] {
                for fit in [None, Some(ImageFit::Contain), Some(ImageFit::Cover)] {
                    for scale in [1.0, 2.0] {
                        let (source, width, height, expected) = if transpose {
                            (
                                (source.1, source.0),
                                Some(Length::Px(fixed)),
                                content.clone(),
                                (expected.1, expected.0),
                            )
                        } else {
                            (source, content.clone(), Some(Length::Px(fixed)), expected)
                        };
                        let image = make_element(
                            "image",
                            ElementKind::Image,
                            Attrs {
                                image_size: Some(source),
                                image_fit: fit,
                                width,
                                height,
                                ..Attrs::default()
                            },
                        );
                        let id = image.id;
                        let mut tree = ElementTree::new();
                        tree.set_root_id(id);
                        tree.insert(image);
                        layout_tree(
                            &mut tree,
                            Constraint::new(2000.0, 2000.0),
                            scale,
                            &MockTextMeasurer,
                        );
                        let image = tree.get(&id).unwrap();
                        for frame in [image.layout.measured_frame, image.layout.frame] {
                            let frame = frame.unwrap();
                            assert_eq!(
                                (frame.width, frame.height),
                                (expected.0 * scale, expected.1 * scale),
                                "source={source:?}, transpose={transpose}, fit={fit:?}, scale={scale}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn test_image_single_pixel_axis_sizes_parent_and_positions_sibling() {
    for kind in [ElementKind::Row, ElementKind::Column] {
        let parent = make_element("parent", kind, Attrs::default());
        let parent_id = parent.id;
        let image = make_element(
            "image",
            ElementKind::Image,
            Attrs {
                image_size: Some((200.0, 200.0)),
                height: Some(Length::Px(68.0)),
                ..Attrs::default()
            },
        );
        let image_id = image.id;
        let sibling = make_element("sibling", ElementKind::El, fixed_box_attrs(10.0, 10.0));
        let sibling_id = sibling.id;
        let mut tree = ElementTree::new();
        tree.set_root_id(parent_id);
        for element in [parent, image, sibling] {
            tree.insert(element);
        }
        tree.set_children(&parent_id, vec![image_id, sibling_id])
            .unwrap();
        layout_tree(
            &mut tree,
            Constraint::new(800.0, 600.0),
            1.0,
            &MockTextMeasurer,
        );

        let parent = tree.get(&parent_id).unwrap();
        let sibling = tree.get(&sibling_id).unwrap().layout.frame.unwrap();
        let (expected_size, expected_position) = if kind == ElementKind::Row {
            ((78.0, 68.0), (68.0, 0.0))
        } else {
            ((68.0, 78.0), (0.0, 68.0))
        };
        for frame in [parent.layout.measured_frame, parent.layout.frame] {
            let frame = frame.unwrap();
            assert_eq!((frame.width, frame.height), expected_size);
        }
        assert_eq!((sibling.x, sibling.y), expected_position);
    }
}

#[test]
fn test_image_single_pixel_axis_applies_ratio_inside_padding_and_border() {
    // Horizontal insets = 3 + 7 + 2*2 = 14; vertical = 4 + 8 + 2*2 = 16.
    for (width, height, expected) in [
        (None, Some(Length::Px(68.0)), (118.0, 68.0)),
        (Some(Length::Px(118.0)), None, (118.0, 68.0)),
        (None, Some(Length::Px(10.0)), (14.0, 10.0)),
        (Some(Length::Px(10.0)), None, (10.0, 16.0)),
    ] {
        for scale in [1.0, 2.0] {
            let image = make_element(
                "image",
                ElementKind::Image,
                Attrs {
                    image_size: Some((200.0, 100.0)),
                    width: width.clone(),
                    height: height.clone(),
                    padding: Some(Padding::Sides {
                        top: 4.0,
                        right: 3.0,
                        bottom: 8.0,
                        left: 7.0,
                    }),
                    border_width: Some(BorderWidth::Uniform(2.0)),
                    ..Attrs::default()
                },
            );
            let id = image.id;
            let mut tree = ElementTree::new();
            tree.set_root_id(id);
            tree.insert(image);
            layout_tree(
                &mut tree,
                Constraint::new(800.0, 600.0),
                scale,
                &MockTextMeasurer,
            );
            let image = tree.get(&id).unwrap();
            for frame in [image.layout.measured_frame, image.layout.frame] {
                let frame = frame.unwrap();
                assert_eq!(
                    (frame.width, frame.height),
                    (expected.0 * scale, expected.1 * scale)
                );
            }
        }
    }
}

#[test]
fn test_image_aspect_ratio_inference_rejects_unknown_or_invalid_dimensions() {
    let attrs = fixed_height_attrs(68.0);
    for source in [
        None,
        Some((0.0, 200.0)),
        Some((200.0, 0.0)),
        Some((-1.0, 200.0)),
        Some((f64::NAN, 200.0)),
        Some((200.0, f64::INFINITY)),
        Some((f64::MAX, f64::MIN_POSITIVE)),
    ] {
        assert!(image_aspect_ratio_size(&attrs, source, LayoutInsets::default()).is_none());
    }
    for fixed in [f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(
            image_aspect_ratio_size(
                &fixed_height_attrs(fixed),
                Some((200.0, 100.0)),
                LayoutInsets::default(),
            )
            .is_none()
        );
    }
}

#[test]
fn test_media_sizing_outside_single_pixel_axis_is_unchanged() {
    for (kind, width, height, expected) in [
        (ElementKind::Image, None, None, (200.0, 100.0)),
        (
            ElementKind::Image,
            Some(Length::Px(80.0)),
            Some(Length::Px(68.0)),
            (80.0, 68.0),
        ),
        (
            ElementKind::Image,
            Some(Length::Content),
            Some(Length::Content),
            (200.0, 100.0),
        ),
        (
            ElementKind::Image,
            Some(Length::Fill),
            Some(Length::Px(68.0)),
            (800.0, 68.0),
        ),
        (
            ElementKind::Image,
            None,
            Some(Length::FillWeighted(2.0)),
            (200.0, 600.0),
        ),
        (
            ElementKind::Image,
            None,
            Some(Length::Min(
                Box::new(Length::Px(68.0)),
                Box::new(Length::Content),
            )),
            (200.0, 68.0),
        ),
        (
            ElementKind::Image,
            Some(Length::Max(
                Box::new(Length::Px(68.0)),
                Box::new(Length::Content),
            )),
            None,
            (200.0, 100.0),
        ),
        (
            ElementKind::Video,
            None,
            Some(Length::Px(68.0)),
            (200.0, 68.0),
        ),
    ] {
        let image = make_element(
            "media",
            kind,
            Attrs {
                image_size: Some((200.0, 100.0)),
                width,
                height,
                ..Attrs::default()
            },
        );
        let id = image.id;
        let mut tree = ElementTree::new();
        tree.set_root_id(id);
        tree.insert(image);
        layout_tree(
            &mut tree,
            Constraint::new(800.0, 600.0),
            1.0,
            &MockTextMeasurer,
        );
        let frame = tree.get(&id).unwrap().layout.frame.unwrap();
        assert_eq!((frame.width, frame.height), expected);
    }
}
