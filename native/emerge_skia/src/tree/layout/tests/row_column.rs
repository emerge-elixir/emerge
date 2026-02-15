use super::super::*;
use super::common::*;

#[test]
fn test_layout_row_fill_portion_with_content_parent() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Content);
    row_attrs.height = Some(Length::Px(30.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("AAAA".to_string());
        a.font_size = Some(10.0);
        a.width = Some(Length::FillPortion(2.0));
        a
    });
    let child2 = make_element("c2", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("BB".to_string());
        a.font_size = Some(10.0);
        a.width = Some(Length::FillPortion(1.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.width, 32.0); // 4 chars * 8px
    assert_eq!(c2_frame.width, 16.0); // 2 chars * 8px
}

#[test]
fn test_layout_column_fill_portion_with_content_parent() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(120.0));
    col_attrs.height = Some(Length::Content);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("Hi".to_string());
        a.font_size = Some(12.0);
        a.height = Some(Length::FillPortion(2.0));
        a
    });
    let child2 = make_element("c2", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("Yo".to_string());
        a.font_size = Some(14.0);
        a.height = Some(Length::FillPortion(1.0));
        a
    });

    let col_id = col.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    col.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.height, 12.0);
    assert_eq!(c2_frame.height, 14.0);
}

#[test]
fn test_layout_row_spacing_xy_uses_horizontal() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.spacing_x = Some(12.0);
    row_attrs.spacing_y = Some(30.0);

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(10.0));
        a.height = Some(Length::Px(10.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(10.0));
        a.height = Some(Length::Px(10.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id);
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(200.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.x, 0.0);
    assert_eq!(c2_frame.x, 22.0); // 10 + spacing_x 12
}

#[test]
fn test_layout_column_spacing_xy_uses_vertical() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.spacing_x = Some(5.0);
    col_attrs.spacing_y = Some(14.0);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(10.0));
        a.height = Some(Length::Px(10.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(10.0));
        a.height = Some(Length::Px(10.0));
        a
    });

    let col_id = col.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    col.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(col_id);
    tree.insert(col);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(200.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 24.0); // 10 + spacing_y 14
}

#[test]
fn test_layout_text_column_stacks_like_column() {
    let mut tree = ElementTree::new();

    let mut text_col_attrs = Attrs::default();
    text_col_attrs.width = Some(Length::Px(100.0));
    text_col_attrs.spacing = Some(12.0);

    let mut text_col = make_element("text_col", ElementKind::TextColumn, text_col_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(20.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(30.0));
        a
    });

    let text_col_id = text_col.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    text_col.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(text_col_id.clone());
    tree.insert(text_col);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let text_col_frame = tree.get(&text_col_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 32.0); // 20 + spacing 12
    assert_eq!(text_col_frame.height, 62.0); // 20 + 12 + 30
}

#[test]
fn test_layout_wrapped_row_spacing_xy_uses_vertical_between_lines() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(50.0));
    row_attrs.spacing_x = Some(5.0);
    row_attrs.spacing_y = Some(7.0);

    let mut row = make_element("row", ElementKind::WrappedRow, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(40.0));
        a.height = Some(Length::Px(10.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(40.0));
        a.height = Some(Length::Px(10.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id);
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(200.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 17.0); // 10 + spacing_y 7
}

#[test]
fn test_layout_row_space_evenly_distribution() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(200.0));
    row_attrs.height = Some(Length::Px(20.0));
    row_attrs.space_evenly = Some(true);

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(20.0));
        a.height = Some(Length::Px(20.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(20.0));
        a.height = Some(Length::Px(20.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(20.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();
    let c3_id = child3.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
    tree.root = Some(row_id);
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);
    tree.insert(child3);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.x, 0.0);
    assert_eq!(c2_frame.x, 90.0);
    assert_eq!(c3_frame.x, 180.0);
}

#[test]
fn test_layout_column_space_evenly_distribution() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(50.0));
    col_attrs.height = Some(Length::Px(200.0));
    col_attrs.space_evenly = Some(true);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(20.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(20.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let col_id = col.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();
    let c3_id = child3.id.clone();

    col.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
    tree.root = Some(col_id);
    tree.insert(col);
    tree.insert(child1);
    tree.insert(child2);
    tree.insert(child3);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 300.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 90.0);
    assert_eq!(c3_frame.y, 180.0);
}

#[test]
fn test_layout_row_space_evenly_ignored_for_content_parent() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Content);
    row_attrs.space_evenly = Some(true);

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(20.0));
        a.height = Some(Length::Px(10.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(20.0));
        a.height = Some(Length::Px(10.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id);
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 100.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.x, 0.0);
    assert_eq!(c2_frame.x, 20.0);
}

#[test]
fn test_layout_row() {
    let mut tree = ElementTree::new();

    // Create row with two children
    let mut row_attrs = Attrs::default();
    row_attrs.spacing = Some(10.0);

    let mut row = make_element("row", ElementKind::Row, row_attrs);
    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.x, 0.0);
    assert_eq!(c2_frame.x, 60.0); // 50 + 10 spacing
}

#[test]
fn test_layout_column_fill() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.height = Some(Length::Px(100.0));

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Fill);
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Fill);
        a
    });

    let col_id = col.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    col.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    // Both children should split the 100px height equally
    assert_eq!(c1_frame.height, 50.0);
    assert_eq!(c2_frame.height, 50.0);
    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 50.0);
}

#[test]
fn test_layout_row_with_max_width_child() {
    let mut tree = ElementTree::new();

    // Row with two children: one fill, one max(100, fill)
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Fill); // Row needs explicit fill to expand
    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(30.0));
        a
    });

    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Maximum(100.0, Box::new(Length::Fill)));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(400.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    // Both children are fill, so they split 400px = 200px each
    // But c2 has max(100), so it gets clamped to 100px
    assert_eq!(c1_frame.width, 200.0);
    assert_eq!(c2_frame.width, 100.0);
}

#[test]
fn test_wrapped_row_height_with_wrapping() {
    let mut tree = ElementTree::new();

    // Create a wrapped row with 3 children, each 50px wide
    // Container is 100px wide, so items should wrap:
    // Line 1: child1, child2 (50 + 10 spacing + 50 = 110 > 100, so child2 wraps)
    // Actually with 100px width: child1 (50) fits, child2 (50+10=60) would make 110, wraps
    // Line 1: child1 (50px)
    // Line 2: child2 (50px)
    // Line 3: child3 (50px)
    // Total height = 3 * 30 + 2 * 10 spacing = 110px

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(100.0));
    row_attrs.spacing = Some(10.0);

    let mut row = make_element("row", ElementKind::WrappedRow, row_attrs);

    // Children 50px wide, 30px tall each
    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();
    let c3_id = child3.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);
    tree.insert(child3);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    // Check wrapped row height
    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    // With 100px width, children wrap: each on its own line
    // 3 lines * 30px height + 2 * 10px spacing = 110px
    assert_eq!(row_frame.height, 110.0);

    // Check child positions
    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    // All children should be at x=0 (each on its own line)
    assert_eq!(c1_frame.x, 0.0);
    assert_eq!(c2_frame.x, 0.0);
    assert_eq!(c3_frame.x, 0.0);

    // Y positions: 0, 40 (30+10), 80 (30+10+30+10)
    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 40.0);
    assert_eq!(c3_frame.y, 80.0);
}

#[test]
fn test_wrapped_row_two_items_per_line() {
    let mut tree = ElementTree::new();

    // Container 120px wide with 10px spacing
    // Children 50px wide each
    // Two children fit per line: 50 + 10 + 50 = 110 < 120
    // With 4 children: 2 lines
    // Total height = 2 * 30 + 1 * 10 spacing = 70px

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(120.0));
    row_attrs.spacing = Some(10.0);

    let mut row = make_element("row", ElementKind::WrappedRow, row_attrs);

    let children: Vec<_> = (0..4)
        .map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(50.0));
                a.height = Some(Length::Px(30.0));
                a
            })
        })
        .collect();

    let child_ids: Vec<_> = children.iter().map(|c| c.id.clone()).collect();
    let row_id = row.id.clone();
    row.children = child_ids.clone();

    tree.root = Some(row_id.clone());
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

    // Check wrapped row height: 2 lines * 30px + 1 * 10px spacing = 70px
    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    assert_eq!(row_frame.height, 70.0);

    // Check child positions
    // Line 1: c0 at x=0, c1 at x=60
    // Line 2: c2 at x=0, c3 at x=60
    let c0_frame = tree.get(&child_ids[0]).unwrap().frame.unwrap();
    let c1_frame = tree.get(&child_ids[1]).unwrap().frame.unwrap();
    let c2_frame = tree.get(&child_ids[2]).unwrap().frame.unwrap();
    let c3_frame = tree.get(&child_ids[3]).unwrap().frame.unwrap();

    assert_eq!(c0_frame.x, 0.0);
    assert_eq!(c0_frame.y, 0.0);
    assert_eq!(c1_frame.x, 60.0);
    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.x, 0.0);
    assert_eq!(c2_frame.y, 40.0);
    assert_eq!(c3_frame.x, 60.0);
    assert_eq!(c3_frame.y, 40.0);
}

#[test]
fn test_column_with_wrapped_row_pushes_siblings() {
    let mut tree = ElementTree::new();

    // Column containing:
    // 1. A wrapped_row (100px wide, 3 children 50px each -> wraps to 3 lines = 110px tall)
    // 2. An element (40px tall)
    //
    // The element should be pushed down by the wrapped_row's actual height (110px),
    // not its initial intrinsic height (30px).

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(100.0));
    col_attrs.spacing = Some(10.0);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    // Wrapped row with 100px width constraint from parent
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Fill);
    row_attrs.spacing = Some(10.0);

    let mut wrapped_row = make_element("wrapped_row", ElementKind::WrappedRow, row_attrs);

    // Three children that will each wrap to their own line
    let chip1 = make_element("chip1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let chip2 = make_element("chip2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let chip3 = make_element("chip3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    // Element below the wrapped row
    let below_el = make_element("below", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(40.0));
        a
    });

    let col_id = col.id.clone();
    let row_id = wrapped_row.id.clone();
    let chip1_id = chip1.id.clone();
    let chip2_id = chip2.id.clone();
    let chip3_id = chip3.id.clone();
    let below_id = below_el.id.clone();

    wrapped_row.children = vec![chip1_id.clone(), chip2_id.clone(), chip3_id.clone()];
    col.children = vec![row_id.clone(), below_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(wrapped_row);
    tree.insert(chip1);
    tree.insert(chip2);
    tree.insert(chip3);
    tree.insert(below_el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    // Check wrapped_row height (3 lines * 30px + 2 * 10px spacing = 110px)
    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    assert_eq!(row_frame.height, 110.0);
    assert_eq!(row_frame.y, 0.0);

    // Check that the element below is positioned after the wrapped_row
    // y = wrapped_row.height (110) + spacing (10) = 120
    let below_frame = tree.get(&below_id).unwrap().frame.unwrap();
    assert_eq!(below_frame.y, 120.0);
    assert_eq!(below_frame.height, 40.0);

    // Column should encompass both children
    let col_frame = tree.get(&col_id).unwrap().frame.unwrap();
    // Total: 110 (wrapped_row) + 10 (spacing) + 40 (below) = 160
    assert_eq!(col_frame.height, 160.0);
}

#[test]
fn test_content_height_column_repositions_bottom_aligned_child_after_expansion() {
    let mut tree = ElementTree::new();

    // Content-height column with a top child that expands during resolve
    // and a bottom-aligned child that should stay at the visual bottom.
    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(20.0));
    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let mut row = make_element("top_row", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a
    });

    let mut para = make_element("para", ElementKind::Paragraph, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(8.0);
        a
    });

    let txt = make_element("txt", ElementKind::Text, text_attrs("AA BB"));

    let bottom = make_element("bottom", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(10.0));
        a.align_y = Some(AlignY::Bottom);
        a
    });

    let col_id = col.id.clone();
    let row_id = row.id.clone();
    let para_id = para.id.clone();
    let txt_id = txt.id.clone();
    let bottom_id = bottom.id.clone();

    para.children = vec![txt_id.clone()];
    row.children = vec![para_id.clone()];
    col.children = vec![row_id.clone(), bottom_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(row);
    tree.insert(para);
    tree.insert(txt);
    tree.insert(bottom);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    assert_eq!(row_frame.height, 40.0);

    let bottom_frame = tree.get(&bottom_id).unwrap().frame.unwrap();
    // Bottom child should render below expanded top content.
    assert_eq!(bottom_frame.y, 40.0);

    let col_frame = tree.get(&col_id).unwrap().frame.unwrap();
    assert_eq!(col_frame.height, 50.0);
}

#[test]
fn test_content_height_column_applies_spacing_between_top_and_bottom_zones() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(20.0));
    col_attrs.spacing = Some(16.0);
    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let mut row = make_element("top_row", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a
    });

    let mut para = make_element("para", ElementKind::Paragraph, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(8.0);
        a
    });

    let txt = make_element("txt", ElementKind::Text, text_attrs("AA BB"));

    let bottom = make_element("bottom", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(10.0));
        a.align_y = Some(AlignY::Bottom);
        a
    });

    let col_id = col.id.clone();
    let row_id = row.id.clone();
    let para_id = para.id.clone();
    let txt_id = txt.id.clone();
    let bottom_id = bottom.id.clone();

    para.children = vec![txt_id.clone()];
    row.children = vec![para_id.clone()];
    col.children = vec![row_id.clone(), bottom_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(row);
    tree.insert(para);
    tree.insert(txt);
    tree.insert(bottom);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    assert_eq!(row_frame.height, 40.0);

    let bottom_frame = tree.get(&bottom_id).unwrap().frame.unwrap();
    // Bottom child should appear after top content + column spacing.
    assert_eq!(bottom_frame.y, 56.0);

    let col_frame = tree.get(&col_id).unwrap().frame.unwrap();
    // 40 (top) + 16 (zone spacing) + 10 (bottom)
    assert_eq!(col_frame.height, 66.0);
}

#[test]
fn test_row_expands_height_when_child_paragraph_wraps() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(50.0));
    col_attrs.spacing = Some(10.0);
    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Fill);
    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let mut para_attrs = Attrs::default();
    para_attrs.width = Some(Length::Fill);
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);

    let txt = make_element("txt", ElementKind::Text, text_attrs("AAAA BBBB"));

    let below = make_element("below", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(20.0));
        a
    });

    let col_id = col.id.clone();
    let row_id = row.id.clone();
    let para_id = para.id.clone();
    let txt_id = txt.id.clone();
    let below_id = below.id.clone();

    para.children = vec![txt_id.clone()];
    row.children = vec![para_id.clone()];
    col.children = vec![row_id.clone(), below_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(row);
    tree.insert(para);
    tree.insert(txt);
    tree.insert(below);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    // Row should match wrapped paragraph: 2 lines * 16px = 32px
    assert_eq!(row_frame.height, 32.0);

    let below_frame = tree.get(&below_id).unwrap().frame.unwrap();
    // below y = row height (32) + spacing (10)
    assert_eq!(below_frame.y, 42.0);
}

#[test]
fn test_row_with_fill_height_does_not_expand_for_wrapped_paragraph_child() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(50.0));
    col_attrs.height = Some(Length::Px(40.0));
    col_attrs.spacing = Some(4.0);
    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let top = make_element("top", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(8.0));
        a
    });

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Fill);
    row_attrs.height = Some(Length::Fill);
    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let mut para_attrs = Attrs::default();
    para_attrs.width = Some(Length::Fill);
    let mut para = make_element("para", ElementKind::Paragraph, para_attrs);

    let txt = make_element("txt", ElementKind::Text, text_attrs("AAAA BBBB"));

    let footer = make_element("footer", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(8.0));
        a
    });

    let col_id = col.id.clone();
    let top_id = top.id.clone();
    let row_id = row.id.clone();
    let para_id = para.id.clone();
    let txt_id = txt.id.clone();
    let footer_id = footer.id.clone();

    para.children = vec![txt_id.clone()];
    row.children = vec![para_id.clone()];
    col.children = vec![top_id.clone(), row_id.clone(), footer_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(top);
    tree.insert(row);
    tree.insert(para);
    tree.insert(txt);
    tree.insert(footer);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    // Column allocates: 40 - top(8) - footer(8) - 2 spacings(8) = 16
    // Fill-height row should remain constrained to its allocated slot.
    assert_eq!(row_frame.height, 16.0);

    let footer_frame = tree.get(&footer_id).unwrap().frame.unwrap();
    // footer y = top(8) + spacing(4) + row(16) + spacing(4) = 32
    assert_eq!(footer_frame.y, 32.0);
}

#[test]
fn test_row_fill_portion_distribution() {
    let mut tree = ElementTree::new();

    // Row with 300px width, containing:
    // - child1: fillPortion(1) -> 1/6 of 300 = 50px
    // - child2: fillPortion(2) -> 2/6 of 300 = 100px
    // - child3: fillPortion(3) -> 3/6 of 300 = 150px
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(300.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(1.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(2.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(3.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();
    let c3_id = child3.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);
    tree.insert(child3);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    // Total portions = 1 + 2 + 3 = 6
    // c1: 300 * 1/6 = 50
    // c2: 300 * 2/6 = 100
    // c3: 300 * 3/6 = 150
    assert_eq!(c1_frame.width, 50.0);
    assert_eq!(c2_frame.width, 100.0);
    assert_eq!(c3_frame.width, 150.0);

    // Check positions
    assert_eq!(c1_frame.x, 0.0);
    assert_eq!(c2_frame.x, 50.0);
    assert_eq!(c3_frame.x, 150.0);
}

#[test]
fn test_row_fill_portion_with_fixed() {
    let mut tree = ElementTree::new();

    // Row with 400px width, containing:
    // - child1: 100px fixed
    // - child2: fillPortion(1) -> 1/3 of remaining 300 = 100px
    // - child3: fillPortion(2) -> 2/3 of remaining 300 = 200px
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(400.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(100.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(1.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(2.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();
    let c3_id = child3.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);
    tree.insert(child3);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    // Remaining = 400 - 100 = 300
    // c1: 100px fixed
    // c2: 300 * 1/3 = 100
    // c3: 300 * 2/3 = 200
    assert_eq!(c1_frame.width, 100.0);
    assert_eq!(c2_frame.width, 100.0);
    assert_eq!(c3_frame.width, 200.0);
}

#[test]
fn test_column_fill_portion_distribution() {
    let mut tree = ElementTree::new();

    // Column with 300px height, containing:
    // - child1: fillPortion(1) -> 1/6 of 300 = 50px
    // - child2: fillPortion(2) -> 2/6 of 300 = 100px
    // - child3: fillPortion(3) -> 3/6 of 300 = 150px
    let mut col_attrs = Attrs::default();
    col_attrs.height = Some(Length::Px(300.0));

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::FillPortion(1.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::FillPortion(2.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::FillPortion(3.0));
        a
    });

    let col_id = col.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();
    let c3_id = child3.id.clone();

    col.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(child1);
    tree.insert(child2);
    tree.insert(child3);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    // Total portions = 1 + 2 + 3 = 6
    // c1: 300 * 1/6 = 50
    // c2: 300 * 2/6 = 100
    // c3: 300 * 3/6 = 150
    assert_eq!(c1_frame.height, 50.0);
    assert_eq!(c2_frame.height, 100.0);
    assert_eq!(c3_frame.height, 150.0);

    // Check positions
    assert_eq!(c1_frame.y, 0.0);
    assert_eq!(c2_frame.y, 50.0);
    assert_eq!(c3_frame.y, 150.0);
}

#[test]
fn test_fill_and_fill_portion_mixed() {
    let mut tree = ElementTree::new();

    // Row with 400px, containing:
    // - child1: fill (= fillPortion(1))
    // - child2: fillPortion(3)
    // Total portions = 1 + 3 = 4
    // c1: 400 * 1/4 = 100
    // c2: 400 * 3/4 = 300
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(400.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill); // Equivalent to FillPortion(1)
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(3.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let c1_id = child1.id.clone();
    let c2_id = child2.id.clone();

    row.children = vec![c1_id.clone(), c2_id.clone()];
    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(child1);
    tree.insert(child2);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

    assert_eq!(c1_frame.width, 100.0);
    assert_eq!(c2_frame.width, 300.0);
}

#[test]
fn test_row_self_alignment_zones() {
    let mut tree = ElementTree::new();

    // Row with 300px width, 3 children:
    // - left-aligned child (50px)
    // - center-aligned child (50px)
    // - right-aligned child (50px)
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(300.0));
    row_attrs.height = Some(Length::Px(50.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let left_child = make_element("left", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a.align_x = Some(AlignX::Left);
        a
    });

    let center_child = make_element("center", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a.align_x = Some(AlignX::Center);
        a
    });

    let right_child = make_element("right", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(30.0));
        a.align_x = Some(AlignX::Right);
        a
    });

    let row_id = row.id.clone();
    let left_id = left_child.id.clone();
    let center_id = center_child.id.clone();
    let right_id = right_child.id.clone();

    row.children = vec![left_id.clone(), center_id.clone(), right_id.clone()];

    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(left_child);
    tree.insert(center_child);
    tree.insert(right_child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let left_frame = tree.get(&left_id).unwrap().frame.unwrap();
    let center_frame = tree.get(&center_id).unwrap().frame.unwrap();
    let right_frame = tree.get(&right_id).unwrap().frame.unwrap();

    // Left child at x=0
    assert_eq!(left_frame.x, 0.0);

    // Right child at far right: 300 - 50 = 250
    assert_eq!(right_frame.x, 250.0);

    // Center child in the middle of remaining space
    // Remaining space: 0+50 to 250 = 200px gap
    // Center of gap: 50 + (200 - 50) / 2 = 50 + 75 = 125
    assert_eq!(center_frame.x, 125.0);
}

#[test]
fn test_column_self_alignment_zones() {
    let mut tree = ElementTree::new();

    // Column with 300px height, 3 children:
    // - top-aligned child (50px)
    // - center-aligned child (50px)
    // - bottom-aligned child (50px)
    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(100.0));
    col_attrs.height = Some(Length::Px(300.0));

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let top_child = make_element("top", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(50.0));
        a.align_y = Some(AlignY::Top);
        a
    });

    let center_child = make_element("center", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(50.0));
        a.align_y = Some(AlignY::Center);
        a
    });

    let bottom_child = make_element("bottom", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(50.0));
        a.align_y = Some(AlignY::Bottom);
        a
    });

    let col_id = col.id.clone();
    let top_id = top_child.id.clone();
    let center_id = center_child.id.clone();
    let bottom_id = bottom_child.id.clone();

    col.children = vec![top_id.clone(), center_id.clone(), bottom_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(top_child);
    tree.insert(center_child);
    tree.insert(bottom_child);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let top_frame = tree.get(&top_id).unwrap().frame.unwrap();
    let center_frame = tree.get(&center_id).unwrap().frame.unwrap();
    let bottom_frame = tree.get(&bottom_id).unwrap().frame.unwrap();

    // Top child at y=0
    assert_eq!(top_frame.y, 0.0);

    // Bottom child at far bottom: 300 - 50 = 250
    assert_eq!(bottom_frame.y, 250.0);

    // Center child in the middle of remaining space
    // Remaining space: 0+50 to 250 = 200px gap
    // Center of gap: 50 + (200 - 50) / 2 = 50 + 75 = 125
    assert_eq!(center_frame.y, 125.0);
}

#[test]
fn test_row_with_mixed_alignments_and_vertical() {
    let mut tree = ElementTree::new();

    // Row with children at different horizontal and vertical alignments
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(200.0));
    row_attrs.height = Some(Length::Px(100.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    // Left-aligned, top-aligned
    let left_top = make_element("lt", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(40.0));
        a.height = Some(Length::Px(30.0));
        a.align_x = Some(AlignX::Left);
        a.align_y = Some(AlignY::Top);
        a
    });

    // Right-aligned, bottom-aligned
    let right_bottom = make_element("rb", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(40.0));
        a.height = Some(Length::Px(30.0));
        a.align_x = Some(AlignX::Right);
        a.align_y = Some(AlignY::Bottom);
        a
    });

    let row_id = row.id.clone();
    let lt_id = left_top.id.clone();
    let rb_id = right_bottom.id.clone();

    row.children = vec![lt_id.clone(), rb_id.clone()];

    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(left_top);
    tree.insert(right_bottom);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let lt_frame = tree.get(&lt_id).unwrap().frame.unwrap();
    let rb_frame = tree.get(&rb_id).unwrap().frame.unwrap();

    // Left-top: x=0, y=0
    assert_eq!(lt_frame.x, 0.0);
    assert_eq!(lt_frame.y, 0.0);

    // Right-bottom: x=160 (200-40), y=70 (100-30)
    assert_eq!(rb_frame.x, 160.0);
    assert_eq!(rb_frame.y, 70.0);
}

#[test]
fn test_row_fill_portion_with_minimum_wrapper_clamps_individual_child() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(300.0));
    row_attrs.height = Some(Length::Px(40.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let min_fill = make_element("min_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Minimum(180.0, Box::new(Length::FillPortion(1.0))));
        a.height = Some(Length::Px(20.0));
        a
    });
    let plain_fill = make_element("plain_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillPortion(1.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let row_id = row.id.clone();
    let min_fill_id = min_fill.id.clone();
    let plain_fill_id = plain_fill.id.clone();
    row.children = vec![min_fill_id.clone(), plain_fill_id.clone()];

    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(min_fill);
    tree.insert(plain_fill);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let first = tree.get(&min_fill_id).unwrap().frame.unwrap();
    let second = tree.get(&plain_fill_id).unwrap().frame.unwrap();

    // Base fill share is 150/150, but Minimum(180, fill_portion(1)) clamps first child.
    assert_eq!(first.width, 180.0);
    assert_eq!(second.width, 150.0);
    assert_eq!(second.x, 180.0);
}

#[test]
fn test_column_fill_portion_with_maximum_wrapper_clamps_individual_child() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(100.0));
    col_attrs.height = Some(Length::Px(300.0));

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let max_fill = make_element("max_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.height = Some(Length::Maximum(60.0, Box::new(Length::FillPortion(1.0))));
        a.width = Some(Length::Px(40.0));
        a
    });
    let plain_fill = make_element("plain_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.height = Some(Length::FillPortion(1.0));
        a.width = Some(Length::Px(40.0));
        a
    });

    let col_id = col.id.clone();
    let max_fill_id = max_fill.id.clone();
    let plain_fill_id = plain_fill.id.clone();
    col.children = vec![max_fill_id.clone(), plain_fill_id.clone()];

    tree.root = Some(col_id.clone());
    tree.insert(col);
    tree.insert(max_fill);
    tree.insert(plain_fill);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let first = tree.get(&max_fill_id).unwrap().frame.unwrap();
    let second = tree.get(&plain_fill_id).unwrap().frame.unwrap();

    // Base fill share is 150/150, but Maximum(60, fill_portion(1)) clamps first child.
    assert_eq!(first.height, 60.0);
    assert_eq!(second.height, 150.0);
    assert_eq!(second.y, 60.0);
}
