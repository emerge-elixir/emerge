use super::super::*;
use super::common::*;

struct ExactAssetsIds {
    weather_row_id: ElementId,
    svg_row_id: ElementId,
    weather_card_ids: Vec<ElementId>,
    svg_card_ids: Vec<ElementId>,
}

fn insert_text_node(tree: &mut ElementTree, id: &str, content: &str, font_size: f64) -> ElementId {
    let mut attrs = text_attrs(content);
    attrs.font_size = Some(font_size);
    let element = make_element(id, ElementKind::Text, attrs);
    let element_id = element.id.clone();
    tree.insert(element);
    element_id
}

fn insert_badge_node(
    tree: &mut ElementTree,
    id: &str,
    label: &str,
    padding: (f64, f64, f64, f64),
    font_size: f64,
) -> ElementId {
    let text_id = insert_text_node(tree, &format!("{id}_text"), label, font_size);

    let mut badge = make_element(id, ElementKind::El, {
        let mut a = Attrs::default();
        a.padding = Some(Padding::Sides {
            top: padding.0,
            right: padding.1,
            bottom: padding.2,
            left: padding.3,
        });
        a
    });
    let badge_id = badge.id.clone();
    badge.children = vec![text_id];
    tree.insert(badge);
    badge_id
}

fn insert_temp_line_node(
    tree: &mut ElementTree,
    id: &str,
    label: &str,
    primary: &str,
    secondary: &str,
) -> ElementId {
    let label_id = insert_text_node(tree, &format!("{id}_label"), label, 9.0);
    let primary_id = insert_text_node(tree, &format!("{id}_primary"), primary, 15.0);
    let secondary_id = insert_text_node(tree, &format!("{id}_secondary"), secondary, 11.0);

    let mut row = make_element(id, ElementKind::Row, {
        let mut a = Attrs::default();
        a.spacing = Some(6.0);
        a
    });
    let row_id = row.id.clone();
    row.children = vec![label_id, primary_id, secondary_id];
    tree.insert(row);
    row_id
}

fn insert_weather_day_card_node(
    tree: &mut ElementTree,
    id: &str,
    day: &str,
    condition: &str,
    high_c: &str,
    high_f: &str,
    low_c: &str,
    low_f: &str,
    precip: &str,
) -> ElementId {
    let day_id = insert_text_node(tree, &format!("{id}_day"), day, 12.0);

    let icon = make_element(&format!("{id}_icon"), ElementKind::Image, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Fill);
        a
    });
    let icon_id = icon.id.clone();
    tree.insert(icon);

    let mut icon_wrap = make_element(&format!("{id}_icon_wrap"), ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(58.0));
        a.height = Some(Length::Px(58.0));
        a.padding = Some(Padding::Uniform(8.0));
        a
    });
    let icon_wrap_id = icon_wrap.id.clone();
    icon_wrap.children = vec![icon_id];
    tree.insert(icon_wrap);

    let condition_id = insert_text_node(tree, &format!("{id}_condition"), condition, 11.0);
    let hi_id = insert_temp_line_node(tree, &format!("{id}_hi"), "HI", high_c, high_f);
    let lo_id = insert_temp_line_node(tree, &format!("{id}_lo"), "LO", low_c, low_f);
    let precip_id = insert_badge_node(
        tree,
        &format!("{id}_precip"),
        precip,
        (3.0, 8.0, 3.0, 8.0),
        9.0,
    );

    let mut column = make_element(&format!("{id}_column"), ElementKind::Column, {
        let mut a = Attrs::default();
        a.spacing = Some(8.0);
        a
    });
    let column_id = column.id.clone();
    column.children = vec![day_id, icon_wrap_id, condition_id, hi_id, lo_id, precip_id];
    tree.insert(column);

    let mut card = make_element(id, ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(118.0));
        a.padding = Some(Padding::Uniform(10.0));
        a.spacing = Some(8.0);
        a.border_width = Some(BorderWidth::Uniform(1.0));
        a
    });
    let card_id = card.id.clone();
    card.children = vec![column_id];
    tree.insert(card);
    card_id
}

fn insert_svg_scale_card_node(
    tree: &mut ElementTree,
    id: &str,
    label: &str,
    note: &str,
) -> ElementId {
    let title_text_id = insert_text_node(tree, &format!("{id}_title_text"), label, 12.0);
    let mut title_fill = make_element(&format!("{id}_title_fill"), ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a
    });
    let title_fill_id = title_fill.id.clone();
    title_fill.children = vec![title_text_id];
    tree.insert(title_fill);

    let badge_id = insert_badge_node(
        tree,
        &format!("{id}_badge"),
        "SVG",
        (4.0, 8.0, 4.0, 8.0),
        10.0,
    );

    let mut title_row = make_element(&format!("{id}_title_row"), ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(8.0);
        a
    });
    let title_row_id = title_row.id.clone();
    title_row.children = vec![title_fill_id, badge_id];
    tree.insert(title_row);

    let note_id = insert_text_node(tree, &format!("{id}_note"), note, 10.0);

    let size_box_ids: Vec<_> = [(24.0, "24px"), (48.0, "48px"), (80.0, "80px")]
        .into_iter()
        .enumerate()
        .map(|(index, (size, label_text))| {
            let icon = make_element(&format!("{id}_size{index}_icon"), ElementKind::Image, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(size));
                a.height = Some(Length::Px(size));
                a
            });
            let icon_id = icon.id.clone();
            tree.insert(icon);

            let text_id =
                insert_text_node(tree, &format!("{id}_size{index}_text"), label_text, 10.0);

            let mut content =
                make_element(&format!("{id}_size{index}_content"), ElementKind::Column, {
                    let mut a = Attrs::default();
                    a.spacing = Some(8.0);
                    a
                });
            let content_id = content.id.clone();
            content.children = vec![icon_id, text_id];
            tree.insert(content);

            let mut box_el = make_element(&format!("{id}_size{index}_box"), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(86.0));
                a.height = Some(Length::Px(118.0));
                a.padding = Some(Padding::Uniform(8.0));
                a
            });
            let box_id = box_el.id.clone();
            box_el.children = vec![content_id];
            tree.insert(box_el);
            box_id
        })
        .collect();

    let mut sizes_row = make_element(&format!("{id}_sizes_row"), ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(8.0);
        a
    });
    let sizes_row_id = sizes_row.id.clone();
    sizes_row.children = size_box_ids;
    tree.insert(sizes_row);

    let mut column = make_element(&format!("{id}_column"), ElementKind::Column, {
        let mut a = Attrs::default();
        a.spacing = Some(10.0);
        a
    });
    let column_id = column.id.clone();
    column.children = vec![title_row_id, note_id, sizes_row_id];
    tree.insert(column);

    let mut card = make_element(id, ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(300.0));
        a.padding = Some(Padding::Uniform(12.0));
        a.spacing = Some(10.0);
        a
    });
    let card_id = card.id.clone();
    card.children = vec![column_id];
    tree.insert(card);
    card_id
}

fn build_exact_demo_assets_tree() -> (ElementTree, ExactAssetsIds) {
    let mut tree = ElementTree::new();

    let mut root = make_element("root", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(20.0));
        a.spacing = Some(16.0);
        a
    });

    let header = make_element("header", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(82.0));
        a
    });

    let mut body = make_element("body", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Fill);
        a.spacing = Some(16.0);
        a
    });

    let menu = make_element("menu", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(220.0));
        a.height = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(12.0));
        a
    });

    let content_panel = make_element("content_panel", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(16.0));
        a.scrollbar_y = Some(true);
        a
    });

    let mut page = make_element("page", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(16.0);
        a
    });

    let assets_title_id = insert_text_node(&mut tree, "assets_title", "Assets", 22.0);
    let assets_intro_id = insert_text_node(
        &mut tree,
        "assets_intro",
        "Assets resolve from otp_app priv or runtime paths, then render through image/2, Background helpers, startup-loaded font assets, and vector SVG icons.",
        12.0,
    );
    let svg_weather_title_id =
        insert_text_node(&mut tree, "svg_weather_title", "SVG Weather", 18.0);
    let svg_weather_intro_id = insert_text_node(
        &mut tree,
        "svg_weather_intro",
        "A hardcoded seven-day forecast using local SVG icons. Temperatures lead with Celsius and keep Fahrenheit as the quieter secondary scale.",
        12.0,
    );

    let mut weather_widget = make_element("weather_widget", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(16.0));
        a.spacing = Some(14.0);
        a.border_width = Some(BorderWidth::Uniform(1.0));
        a
    });

    let left_title_id = insert_text_node(&mut tree, "weather_title", "Weekly forecast", 22.0);
    let left_intro_id = insert_text_node(
        &mut tree,
        "weather_intro",
        "North Shore boardwalk · local SVG weather icons rendered with image/2",
        12.0,
    );
    let badge_svg_id = insert_badge_node(
        &mut tree,
        "weather_badge_svg",
        "SVG via image/2",
        (4.0, 8.0, 4.0, 8.0),
        10.0,
    );
    let badge_c_id = insert_badge_node(
        &mut tree,
        "weather_badge_c",
        "C primary",
        (4.0, 8.0, 4.0, 8.0),
        10.0,
    );
    let badge_f_id = insert_badge_node(
        &mut tree,
        "weather_badge_f",
        "F secondary",
        (4.0, 8.0, 4.0, 8.0),
        10.0,
    );

    let mut left_badges = make_element("weather_left_badges", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(8.0);
        a
    });
    let left_badges_id = left_badges.id.clone();
    left_badges.children = vec![badge_svg_id, badge_c_id, badge_f_id];
    tree.insert(left_badges);

    let mut left_column = make_element("weather_left_column", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(6.0);
        a
    });
    let left_column_id = left_column.id.clone();
    left_column.children = vec![left_title_id, left_intro_id, left_badges_id];
    tree.insert(left_column);

    let sample_badge_id = insert_badge_node(
        &mut tree,
        "weather_sample_badge",
        "Hardcoded sample",
        (5.0, 10.0, 5.0, 10.0),
        11.0,
    );
    let summary_text_id = insert_text_node(
        &mut tree,
        "weather_summary",
        "3 sunny, 2 cloudy, 2 rainy across the week",
        11.0,
    );

    let mut right_column = make_element("weather_right_column", ElementKind::Column, {
        let mut a = Attrs::default();
        a.spacing = Some(8.0);
        a
    });
    let right_column_id = right_column.id.clone();
    right_column.children = vec![sample_badge_id, summary_text_id];
    tree.insert(right_column);

    let mut top_row = make_element("weather_top_row", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(12.0);
        a
    });
    let top_row_id = top_row.id.clone();
    top_row.children = vec![left_column_id, right_column_id];
    tree.insert(top_row);

    let mut weather_shell = make_element("weather_shell", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(10.0));
        a
    });

    let mut weather_row = make_element("weather_row", ElementKind::WrappedRow, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing_x = Some(10.0);
        a.spacing_y = Some(10.0);
        a
    });

    let weather_card_specs = [
        ("Mon", "Sunny", "22C", "72F", "13C", "55F", "precip 5%"),
        ("Tue", "Cloudy", "19C", "66F", "12C", "54F", "precip 20%"),
        ("Wed", "Rain", "16C", "61F", "10C", "50F", "precip 70%"),
        ("Thu", "Cloudy", "18C", "64F", "11C", "52F", "precip 25%"),
        ("Fri", "Sunny", "24C", "75F", "14C", "57F", "precip 5%"),
        ("Sat", "Rain", "17C", "63F", "9C", "48F", "precip 80%"),
        ("Sun", "Sunny", "23C", "73F", "13C", "55F", "precip 10%"),
    ];

    let weather_card_ids: Vec<_> = weather_card_specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            insert_weather_day_card_node(
                &mut tree,
                &format!("weather_card_{index}"),
                spec.0,
                spec.1,
                spec.2,
                spec.3,
                spec.4,
                spec.5,
                spec.6,
            )
        })
        .collect();

    let weather_row_id = weather_row.id.clone();
    weather_row.children = weather_card_ids.clone();
    tree.insert(weather_row);

    let weather_shell_id = weather_shell.id.clone();
    weather_shell.children = vec![weather_row_id.clone()];
    tree.insert(weather_shell);

    let weather_column = make_element("weather_column", ElementKind::Column, {
        let mut a = Attrs::default();
        a.spacing = Some(14.0);
        a
    });
    let mut weather_column = weather_column;
    let weather_column_id = weather_column.id.clone();
    weather_column.children = vec![top_row_id, weather_shell_id.clone()];
    tree.insert(weather_column);

    let weather_widget_id = weather_widget.id.clone();
    weather_widget.children = vec![weather_column_id];
    tree.insert(weather_widget);

    let svg_scaling_title_id =
        insert_text_node(&mut tree, "svg_scaling_title", "SVG scaling", 12.0);
    let svg_scaling_intro_id = insert_text_node(
        &mut tree,
        "svg_scaling_intro",
        "The same icon files stay crisp across compact forecast markers and larger showcase sizes.",
        11.0,
    );

    let mut centered_wrapper = make_element("centered_wrapper", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Maximum(960.0, Box::new(Length::Fill)));
        a.align_x = Some(AlignX::Center);
        a
    });

    let mut svg_row = make_element("svg_row", ElementKind::WrappedRow, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing_x = Some(12.0);
        a.spacing_y = Some(12.0);
        a
    });

    let svg_specs = [
        (
            "Sun",
            "Bright icon reused from forecast cells to oversized hero scale.",
        ),
        (
            "Cloud",
            "Soft neutral linework rendered across compact and roomy card slots.",
        ),
        (
            "Rain",
            "Same source reused for small forecast markers and larger detail art.",
        ),
    ];
    let svg_card_ids: Vec<_> = svg_specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            insert_svg_scale_card_node(&mut tree, &format!("svg_card_{index}"), spec.0, spec.1)
        })
        .collect();

    let svg_row_id = svg_row.id.clone();
    svg_row.children = svg_card_ids.clone();
    tree.insert(svg_row);

    let centered_wrapper_id = centered_wrapper.id.clone();
    centered_wrapper.children = vec![svg_row_id.clone()];
    tree.insert(centered_wrapper);

    let mut svg_section = make_element("svg_section", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(12.0);
        a
    });
    let svg_section_id = svg_section.id.clone();
    svg_section.children = vec![
        svg_scaling_title_id,
        svg_scaling_intro_id,
        centered_wrapper_id,
    ];
    tree.insert(svg_section);

    let footer = make_element("footer", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(180.0));
        a
    });

    let header_id = header.id.clone();
    let body_id = body.id.clone();
    let menu_id = menu.id.clone();
    let content_panel_id = content_panel.id.clone();
    let page_id = page.id.clone();
    let footer_id = footer.id.clone();

    page.children = vec![
        assets_title_id,
        assets_intro_id,
        svg_weather_title_id,
        svg_weather_intro_id,
        weather_widget_id,
        svg_section_id,
    ];
    tree.insert(page);

    let mut content_panel = content_panel;
    content_panel.children = vec![page_id];
    tree.insert(content_panel);

    body.children = vec![menu_id, content_panel_id];
    tree.insert(body);

    root.children = vec![header_id, body_id, footer_id];
    tree.root = Some(root.id.clone());
    tree.insert(root);
    tree.insert(header);
    tree.insert(menu);
    tree.insert(footer);

    (
        tree,
        ExactAssetsIds {
            weather_row_id,
            svg_row_id,
            weather_card_ids,
            svg_card_ids,
        },
    )
}

#[test]
fn test_layout_row_weighted_fill_with_content_parent() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Content);
    row_attrs.height = Some(Length::Px(30.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("AAAA".to_string());
        a.font_size = Some(10.0);
        a.width = Some(Length::FillWeighted(2.0));
        a
    });
    let child2 = make_element("c2", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("BB".to_string());
        a.font_size = Some(10.0);
        a.width = Some(Length::FillWeighted(1.0));
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
fn test_layout_column_weighted_fill_with_content_parent() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(120.0));
    col_attrs.height = Some(Length::Content);

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("Hi".to_string());
        a.font_size = Some(12.0);
        a.height = Some(Length::FillWeighted(2.0));
        a
    });
    let child2 = make_element("c2", ElementKind::Text, {
        let mut a = Attrs::default();
        a.content = Some("Yo".to_string());
        a.font_size = Some(14.0);
        a.height = Some(Length::FillWeighted(1.0));
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
fn test_row_padding_stays_symmetric_with_explicit_width_padded_children() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.padding = Some(Padding::Uniform(10.0));
    row_attrs.spacing = Some(10.0);

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(20.0));
        a.padding = Some(Padding::Uniform(2.0));
        a
    });

    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(10.0));
        a.height = Some(Length::Px(20.0));
        a
    });

    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::Px(20.0));
        a.padding = Some(Padding::Uniform(2.0));
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

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
    let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
    let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

    assert_eq!(row_frame.width, 150.0);
    assert_eq!(row_frame.height, 40.0);
    assert_eq!(c1_frame.x, row_frame.x + 10.0);
    assert_eq!(c2_frame.x, c1_frame.x + c1_frame.width + 10.0);
    assert_eq!(c3_frame.x, c2_frame.x + c2_frame.width + 10.0);
    assert_eq!(
        row_frame.x + row_frame.width - (c3_frame.x + c3_frame.width),
        10.0
    );
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
fn test_wrapped_row_inside_fill_chain_wraps_cards() {
    let mut tree = ElementTree::new();

    let mut root = make_element("root", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(840.0));
        a
    });

    let mut column = make_element("column", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a
    });

    let mut wrapper = make_element("wrapper", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Maximum(960.0, Box::new(Length::Fill)));
        a.align_x = Some(AlignX::Center);
        a
    });

    let mut wrapped_row = make_element("wrapped", ElementKind::WrappedRow, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing_x = Some(12.0);
        a.spacing_y = Some(12.0);
        a
    });

    let cards: Vec<_> = (0..3)
        .map(|i| {
            make_element(&format!("card{i}"), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(300.0));
                a.height = Some(Length::Px(120.0));
                a
            })
        })
        .collect();

    let root_id = root.id.clone();
    let column_id = column.id.clone();
    let wrapper_id = wrapper.id.clone();
    let row_id = wrapped_row.id.clone();
    let card_ids: Vec<_> = cards.iter().map(|card| card.id.clone()).collect();

    wrapped_row.children = card_ids.clone();
    wrapper.children = vec![row_id.clone()];
    column.children = vec![wrapper_id.clone()];
    root.children = vec![column_id.clone()];

    tree.root = Some(root_id.clone());
    tree.insert(root);
    tree.insert(column);
    tree.insert(wrapper);
    tree.insert(wrapped_row);
    for card in cards {
        tree.insert(card);
    }

    layout_tree(
        &mut tree,
        Constraint::new(840.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let wrapper_frame = tree.get(&wrapper_id).unwrap().frame.unwrap();
    assert_eq!(wrapper_frame.width, 840.0);

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    assert_eq!(row_frame.width, 840.0);
    assert_eq!(row_frame.height, 252.0);

    let first = tree.get(&card_ids[0]).unwrap().frame.unwrap();
    let second = tree.get(&card_ids[1]).unwrap().frame.unwrap();
    let third = tree.get(&card_ids[2]).unwrap().frame.unwrap();

    assert_eq!(first.x, 0.0);
    assert_eq!(second.x, 312.0);
    assert_eq!(third.x, 0.0);
    assert_eq!(third.y, 132.0);
}

#[test]
fn test_wrapped_row_with_decorated_fixed_cards_wraps_by_occupied_width() {
    let mut tree = ElementTree::new();

    let mut wrapped_row = make_element("wrapped", ElementKind::WrappedRow, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(490.0));
        a.spacing_x = Some(10.0);
        a.spacing_y = Some(10.0);
        a
    });

    let cards: Vec<_> = (0..4)
        .map(|i| {
            make_element(&format!("card{i}"), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(118.0));
                a.height = Some(Length::Px(60.0));
                a.padding = Some(Padding::Uniform(10.0));
                a.border_width = Some(BorderWidth::Uniform(1.0));
                a
            })
        })
        .collect();

    let row_id = wrapped_row.id.clone();
    let card_ids: Vec<_> = cards.iter().map(|card| card.id.clone()).collect();

    wrapped_row.children = card_ids.clone();
    tree.root = Some(row_id.clone());
    tree.insert(wrapped_row);
    for card in cards {
        tree.insert(card);
    }

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
    assert_eq!(row_frame.height, 130.0);

    let first = tree.get(&card_ids[0]).unwrap().frame.unwrap();
    let second = tree.get(&card_ids[1]).unwrap().frame.unwrap();
    let third = tree.get(&card_ids[2]).unwrap().frame.unwrap();
    let fourth = tree.get(&card_ids[3]).unwrap().frame.unwrap();

    assert_eq!(first.x, 0.0);
    assert_eq!(second.x, 128.0);
    assert_eq!(third.x, 256.0);
    assert_eq!(fourth.x, 0.0);
    assert_eq!(fourth.y, 70.0);
}

#[test]
fn test_row_weighted_fill_subtracts_decorated_fixed_outer_width() {
    let mut tree = ElementTree::new();

    let mut row = make_element("row", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(400.0));
        a
    });

    let fixed = make_element("fixed", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(100.0));
        a.height = Some(Length::Px(30.0));
        a.padding = Some(Padding::Uniform(10.0));
        a.border_width = Some(BorderWidth::Uniform(5.0));
        a
    });
    let fill_a = make_element("fill_a", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(1.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let fill_b = make_element("fill_b", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(2.0));
        a.height = Some(Length::Px(30.0));
        a
    });

    let row_id = row.id.clone();
    let fixed_id = fixed.id.clone();
    let fill_a_id = fill_a.id.clone();
    let fill_b_id = fill_b.id.clone();

    row.children = vec![fixed_id.clone(), fill_a_id.clone(), fill_b_id.clone()];

    tree.root = Some(row_id.clone());
    tree.insert(row);
    tree.insert(fixed);
    tree.insert(fill_a);
    tree.insert(fill_b);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        1.0,
        &MockTextMeasurer,
    );

    let fixed_frame = tree.get(&fixed_id).unwrap().frame.unwrap();
    let fill_a_frame = tree.get(&fill_a_id).unwrap().frame.unwrap();
    let fill_b_frame = tree.get(&fill_b_id).unwrap().frame.unwrap();

    assert_eq!(fixed_frame.x, 0.0);
    assert_eq!(fill_a_frame.x, 100.0);
    assert_eq!(fill_a_frame.width, 100.0);
    assert_eq!(fill_b_frame.x, 200.0);
    assert_eq!(fill_b_frame.width, 200.0);
}

#[test]
fn test_exact_demo_assets_tree_wraps_cards_at_fresh_narrow_width() {
    let (mut tree, ids) = build_exact_demo_assets_tree();

    layout_tree(
        &mut tree,
        Constraint::new(1007.0, 974.0),
        1.0,
        &MockTextMeasurer,
    );

    let weather_row_frame = tree.get(&ids.weather_row_id).unwrap().frame.unwrap();
    let weather_first = tree.get(&ids.weather_card_ids[0]).unwrap().frame.unwrap();
    let weather_last = tree.get(&ids.weather_card_ids[6]).unwrap().frame.unwrap();
    let svg_row_frame = tree.get(&ids.svg_row_id).unwrap().frame.unwrap();
    let svg_first = tree.get(&ids.svg_card_ids[0]).unwrap().frame.unwrap();
    let svg_last = tree.get(&ids.svg_card_ids[2]).unwrap().frame.unwrap();

    assert!(
        weather_row_frame.width < 700.0,
        "weather row width stayed too wide at {}",
        weather_row_frame.width
    );
    assert!(
        weather_last.y > weather_first.y,
        "weather cards stayed on one line: first_y={}, last_y={}",
        weather_first.y,
        weather_last.y
    );
    assert!(
        svg_row_frame.width < 760.0,
        "svg row width stayed too wide at {}",
        svg_row_frame.width
    );
    assert!(
        svg_last.y > svg_first.y,
        "svg cards stayed on one line: first_y={}, last_y={}",
        svg_first.y,
        svg_last.y
    );
}

#[test]
fn test_exact_demo_assets_tree_wraps_after_wide_to_narrow_relayout() {
    let (mut tree, ids) = build_exact_demo_assets_tree();

    layout_tree(
        &mut tree,
        Constraint::new(1490.0, 924.0),
        1.0,
        &MockTextMeasurer,
    );

    let weather_first_wide = tree.get(&ids.weather_card_ids[0]).unwrap().frame.unwrap();
    let weather_last_wide = tree.get(&ids.weather_card_ids[6]).unwrap().frame.unwrap();
    let svg_first_wide = tree.get(&ids.svg_card_ids[0]).unwrap().frame.unwrap();
    let svg_last_wide = tree.get(&ids.svg_card_ids[2]).unwrap().frame.unwrap();

    assert_eq!(weather_last_wide.y, weather_first_wide.y);
    assert_eq!(svg_last_wide.y, svg_first_wide.y);

    layout_tree(
        &mut tree,
        Constraint::new(1007.0, 974.0),
        1.0,
        &MockTextMeasurer,
    );

    let weather_first_narrow = tree.get(&ids.weather_card_ids[0]).unwrap().frame.unwrap();
    let weather_last_narrow = tree.get(&ids.weather_card_ids[6]).unwrap().frame.unwrap();
    let svg_first_narrow = tree.get(&ids.svg_card_ids[0]).unwrap().frame.unwrap();
    let svg_last_narrow = tree.get(&ids.svg_card_ids[2]).unwrap().frame.unwrap();

    assert!(
        weather_last_narrow.y > weather_first_narrow.y,
        "weather cards did not rewrap after resize: first_y={}, last_y={}",
        weather_first_narrow.y,
        weather_last_narrow.y
    );
    assert!(
        svg_last_narrow.y > svg_first_narrow.y,
        "svg cards did not rewrap after resize: first_y={}, last_y={}",
        svg_first_narrow.y,
        svg_last_narrow.y
    );
}

#[test]
fn test_demo_assets_fill_chain_keeps_wrapped_rows_within_content_panel() {
    let mut tree = ElementTree::new();

    let mut root = make_element("root", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(1024.0));
        a.height = Some(Length::Px(768.0));
        a.padding = Some(Padding::Uniform(20.0));
        a.spacing = Some(16.0);
        a
    });

    let header = make_element("header", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(80.0));
        a
    });

    let mut body = make_element("body", ElementKind::Row, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Fill);
        a.spacing = Some(16.0);
        a
    });

    let menu = make_element("menu", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(220.0));
        a.height = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(12.0));
        a
    });

    let mut content_panel = make_element("content_panel", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(16.0));
        a.scrollbar_y = Some(true);
        a
    });

    let mut page = make_element("page", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(16.0);
        a
    });

    let mut weather_widget = make_element("weather_widget", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(16.0));
        a.border_width = Some(BorderWidth::Uniform(1.0));
        a
    });

    let mut weather_shell = make_element("weather_shell", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.padding = Some(Padding::Uniform(10.0));
        a
    });

    let mut weather_row = make_element("weather_row", ElementKind::WrappedRow, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing_x = Some(10.0);
        a.spacing_y = Some(10.0);
        a
    });

    let weather_cards: Vec<_> = (0..7)
        .map(|i| {
            make_element(&format!("weather_card{i}"), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(118.0));
                a.padding = Some(Padding::Uniform(10.0));
                a.border_width = Some(BorderWidth::Uniform(1.0));
                a.height = Some(Length::Px(60.0));
                a
            })
        })
        .collect();

    let mut svg_section = make_element("svg_section", ElementKind::Column, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing = Some(12.0);
        a
    });

    let mut centered_wrapper = make_element("centered_wrapper", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Maximum(960.0, Box::new(Length::Fill)));
        a.align_x = Some(AlignX::Center);
        a
    });

    let mut svg_row = make_element("svg_row", ElementKind::WrappedRow, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.spacing_x = Some(12.0);
        a.spacing_y = Some(12.0);
        a
    });

    let svg_cards: Vec<_> = (0..3)
        .map(|i| {
            make_element(&format!("svg_card{i}"), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(300.0));
                a.height = Some(Length::Px(140.0));
                a
            })
        })
        .collect();

    let footer = make_element("footer", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill);
        a.height = Some(Length::Px(180.0));
        a
    });

    let root_id = root.id.clone();
    let header_id = header.id.clone();
    let body_id = body.id.clone();
    let menu_id = menu.id.clone();
    let content_panel_id = content_panel.id.clone();
    let page_id = page.id.clone();
    let weather_widget_id = weather_widget.id.clone();
    let weather_shell_id = weather_shell.id.clone();
    let weather_row_id = weather_row.id.clone();
    let weather_card_ids: Vec<_> = weather_cards.iter().map(|card| card.id.clone()).collect();
    let svg_section_id = svg_section.id.clone();
    let centered_wrapper_id = centered_wrapper.id.clone();
    let svg_row_id = svg_row.id.clone();
    let svg_card_ids: Vec<_> = svg_cards.iter().map(|card| card.id.clone()).collect();
    let footer_id = footer.id.clone();

    weather_row.children = weather_card_ids.clone();
    weather_shell.children = vec![weather_row_id.clone()];
    weather_widget.children = vec![weather_shell_id.clone()];
    svg_row.children = svg_card_ids.clone();
    centered_wrapper.children = vec![svg_row_id.clone()];
    svg_section.children = vec![centered_wrapper_id.clone()];
    page.children = vec![weather_widget_id.clone(), svg_section_id.clone()];
    content_panel.children = vec![page_id.clone()];
    body.children = vec![menu_id.clone(), content_panel_id.clone()];
    root.children = vec![header_id.clone(), body_id.clone(), footer_id.clone()];

    tree.root = Some(root_id.clone());
    tree.insert(root);
    tree.insert(header);
    tree.insert(body);
    tree.insert(menu);
    tree.insert(content_panel);
    tree.insert(page);
    tree.insert(weather_widget);
    tree.insert(weather_shell);
    tree.insert(weather_row);
    for card in weather_cards {
        tree.insert(card);
    }
    tree.insert(svg_section);
    tree.insert(centered_wrapper);
    tree.insert(svg_row);
    for card in svg_cards {
        tree.insert(card);
    }
    tree.insert(footer);

    let weather_card_intrinsic = measure_element(
        &mut tree,
        &weather_card_ids[0],
        &MockTextMeasurer,
        &FontContext::default(),
    );
    assert_eq!(weather_card_intrinsic.width, 118.0);
    assert_eq!(weather_card_intrinsic.height, 60.0);

    layout_tree(
        &mut tree,
        Constraint::new(1024.0, 768.0),
        1.0,
        &MockTextMeasurer,
    );

    let content_panel_frame = tree.get(&content_panel_id).unwrap().frame.unwrap();
    let page_frame = tree.get(&page_id).unwrap().frame.unwrap();
    let weather_widget_frame = tree.get(&weather_widget_id).unwrap().frame.unwrap();
    let weather_shell_frame = tree.get(&weather_shell_id).unwrap().frame.unwrap();
    let weather_row_frame = tree.get(&weather_row_id).unwrap().frame.unwrap();
    let centered_wrapper_frame = tree.get(&centered_wrapper_id).unwrap().frame.unwrap();
    let svg_row_frame = tree.get(&svg_row_id).unwrap().frame.unwrap();

    assert_eq!(content_panel_frame.width, 748.0);
    assert_eq!(page_frame.width, 716.0);
    assert_eq!(weather_widget_frame.width, 716.0);
    assert_eq!(weather_shell_frame.width, 682.0);
    assert_eq!(weather_row_frame.width, 662.0);
    assert_eq!(centered_wrapper_frame.width, 716.0);
    assert_eq!(svg_row_frame.width, 716.0);

    let weather_card_0 = tree.get(&weather_card_ids[0]).unwrap().frame.unwrap();
    let weather_card_4 = tree.get(&weather_card_ids[4]).unwrap().frame.unwrap();
    let weather_card_5 = tree.get(&weather_card_ids[5]).unwrap().frame.unwrap();
    let weather_card_6 = tree.get(&weather_card_ids[6]).unwrap().frame.unwrap();

    assert_eq!(weather_card_0.x, weather_row_frame.x);
    assert_eq!(weather_card_4.x, weather_row_frame.x + 4.0 * 128.0);
    assert_eq!(weather_card_5.x, weather_row_frame.x);
    assert_eq!(weather_card_5.y, weather_card_0.y + 70.0);
    assert_eq!(weather_card_6.x, weather_row_frame.x + 128.0);
    assert_eq!(weather_card_6.y, weather_card_0.y + 70.0);

    let svg_card_0 = tree.get(&svg_card_ids[0]).unwrap().frame.unwrap();
    let svg_card_1 = tree.get(&svg_card_ids[1]).unwrap().frame.unwrap();
    let svg_card_2 = tree.get(&svg_card_ids[2]).unwrap().frame.unwrap();

    assert_eq!(svg_card_0.x, centered_wrapper_frame.x);
    assert_eq!(svg_card_1.x, centered_wrapper_frame.x + 312.0);
    assert_eq!(svg_card_2.x, centered_wrapper_frame.x);
    assert_eq!(svg_card_2.y, svg_card_0.y + 152.0);
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
fn test_row_weighted_fill_distribution() {
    let mut tree = ElementTree::new();

    // Row with 300px width, containing:
    // - child1: weighted fill 1 -> 1/6 of 300 = 50px
    // - child2: weighted fill 2 -> 2/6 of 300 = 100px
    // - child3: weighted fill 3 -> 3/6 of 300 = 150px
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(300.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(1.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(2.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(3.0));
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
fn test_row_weighted_fill_with_fixed() {
    let mut tree = ElementTree::new();

    // Row with 400px width, containing:
    // - child1: 100px fixed
    // - child2: weighted fill 1 -> 1/3 of remaining 300 = 100px
    // - child3: weighted fill 2 -> 2/3 of remaining 300 = 200px
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
        a.width = Some(Length::FillWeighted(1.0));
        a.height = Some(Length::Px(30.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(2.0));
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
fn test_column_weighted_fill_distribution() {
    let mut tree = ElementTree::new();

    // Column with 300px height, containing:
    // - child1: weighted fill 1 -> 1/6 of 300 = 50px
    // - child2: weighted fill 2 -> 2/6 of 300 = 100px
    // - child3: weighted fill 3 -> 3/6 of 300 = 150px
    let mut col_attrs = Attrs::default();
    col_attrs.height = Some(Length::Px(300.0));

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::FillWeighted(1.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::FillWeighted(2.0));
        a
    });
    let child3 = make_element("c3", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Px(50.0));
        a.height = Some(Length::FillWeighted(3.0));
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
fn test_fill_and_weighted_fill_mixed() {
    let mut tree = ElementTree::new();

    // Row with 400px, containing:
    // - child1: fill (= weighted fill 1)
    // - child2: weighted fill 3
    // Total portions = 1 + 3 = 4
    // c1: 400 * 1/4 = 100
    // c2: 400 * 3/4 = 300
    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(400.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let child1 = make_element("c1", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Fill); // Equivalent to weighted fill 1
        a.height = Some(Length::Px(30.0));
        a
    });
    let child2 = make_element("c2", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(3.0));
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
fn test_row_weighted_fill_with_minimum_wrapper_clamps_individual_child() {
    let mut tree = ElementTree::new();

    let mut row_attrs = Attrs::default();
    row_attrs.width = Some(Length::Px(300.0));
    row_attrs.height = Some(Length::Px(40.0));

    let mut row = make_element("row", ElementKind::Row, row_attrs);

    let min_fill = make_element("min_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::Minimum(180.0, Box::new(Length::FillWeighted(1.0))));
        a.height = Some(Length::Px(20.0));
        a
    });
    let plain_fill = make_element("plain_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.width = Some(Length::FillWeighted(1.0));
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

    // Base fill share is 150/150, but Minimum(180, weighted fill 1) clamps first child.
    assert_eq!(first.width, 180.0);
    assert_eq!(second.width, 150.0);
    assert_eq!(second.x, 180.0);
}

#[test]
fn test_column_weighted_fill_with_maximum_wrapper_clamps_individual_child() {
    let mut tree = ElementTree::new();

    let mut col_attrs = Attrs::default();
    col_attrs.width = Some(Length::Px(100.0));
    col_attrs.height = Some(Length::Px(300.0));

    let mut col = make_element("col", ElementKind::Column, col_attrs);

    let max_fill = make_element("max_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.height = Some(Length::Maximum(60.0, Box::new(Length::FillWeighted(1.0))));
        a.width = Some(Length::Px(40.0));
        a
    });
    let plain_fill = make_element("plain_fill", ElementKind::El, {
        let mut a = Attrs::default();
        a.height = Some(Length::FillWeighted(1.0));
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

    // Base fill share is 150/150, but Maximum(60, weighted fill 1) clamps first child.
    assert_eq!(first.height, 60.0);
    assert_eq!(second.height, 150.0);
    assert_eq!(second.y, 60.0);
}
