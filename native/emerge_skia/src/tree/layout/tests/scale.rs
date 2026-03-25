use super::super::*;
use super::common::*;
use crate::events::test_support::AnimatedNearbyHitCase;
use crate::tree::animation::{AnimationCurve, AnimationRepeat, AnimationSpec};
use crate::tree::attrs::{BorderRadius, BorderStyle, FontStyle, FontWeight};

#[test]
fn test_layout_with_scale() {
    let mut tree = ElementTree::new();

    // Element with width=100px, height=50px, padding=10px, font_size=16
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(50.0));
    attrs.padding = Some(Padding::Uniform(10.0));
    attrs.font_size = Some(16.0);

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    // With scale=2.0, frame pixel values should double
    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        2.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.frame.unwrap();
    // width: 100 * 2 = 200
    // height: 50 * 2 = 100
    assert_eq!(frame.width, 200.0);
    assert_eq!(frame.height, 100.0);

    // base_attrs should remain unchanged (original unscaled values)
    assert_eq!(root.base_attrs.padding, Some(Padding::Uniform(10.0)));
    assert_eq!(root.base_attrs.font_size, Some(16.0));

    // attrs should be scaled (for render to read)
    assert_eq!(root.attrs.padding, Some(Padding::Uniform(20.0)));
    assert_eq!(root.attrs.font_size, Some(32.0));
}

#[test]
fn test_layout_with_scale_scales_font_spacing() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.content = Some("a b".to_string());
    attrs.font_size = Some(10.0);
    attrs.font_letter_spacing = Some(2.0);
    attrs.font_word_spacing = Some(3.0);

    let el = make_element("root", ElementKind::Text, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        2.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    assert_eq!(root.base_attrs.font_letter_spacing, Some(2.0));
    assert_eq!(root.base_attrs.font_word_spacing, Some(3.0));
    assert_eq!(root.attrs.font_letter_spacing, Some(4.0));
    assert_eq!(root.attrs.font_word_spacing, Some(6.0));
}

#[test]
fn test_layout_scale_minimum_maximum() {
    let mut tree = ElementTree::new();

    // Element with width=minimum(100, fill), height=maximum(200, fill)
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Minimum(100.0, Box::new(Length::Fill)));
    attrs.height = Some(Length::Maximum(200.0, Box::new(Length::Fill)));

    let el = make_element("root", ElementKind::El, attrs);
    let root_id = el.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(el);

    // With scale=2.0:
    // width: minimum(200, fill) -> fill=800, clamped to min 200 -> 800
    // height: maximum(400, fill) -> fill=600, clamped to max 400 -> 400
    layout_tree(
        &mut tree,
        Constraint::new(800.0, 600.0),
        2.0,
        &MockTextMeasurer,
    );

    let root = tree.get(&root_id).unwrap();
    let frame = root.frame.unwrap();
    assert_eq!(frame.width, 800.0); // fill = 800, min 200 doesn't apply
    assert_eq!(frame.height, 400.0); // fill = 600, clamped to max 400
}

#[test]
fn test_mouse_over_styles_are_applied_in_layout_pass() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(40.0));
    attrs.background = Some(crate::tree::attrs::Background::Color(
        crate::tree::attrs::Color::Rgb {
            r: 10,
            g: 20,
            b: 30,
        },
    ));
    attrs.mouse_over = Some(MouseOverAttrs {
        background: Some(crate::tree::attrs::Background::Color(
            crate::tree::attrs::Color::Rgb {
                r: 200,
                g: 100,
                b: 50,
            },
        )),
        border_radius: Some(BorderRadius::Uniform(6.0)),
        border_width: Some(BorderWidth::Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        }),
        border_style: Some(BorderStyle::Dashed),
        font: Some(Font::Atom("display".to_string())),
        font_weight: Some(FontWeight("bold".to_string())),
        font_style: Some(FontStyle("italic".to_string())),
        font_size: Some(22.0),
        font_underline: Some(true),
        font_strike: Some(true),
        font_letter_spacing: Some(3.0),
        font_word_spacing: Some(4.0),
        text_align: Some(TextAlign::Center),
        move_x: Some(5.0),
        alpha: Some(0.5),
        ..Default::default()
    });
    attrs.mouse_over_active = Some(true);

    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(root);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let updated = tree.get(&root_id).unwrap();
    assert_eq!(
        updated.attrs.border_radius,
        Some(BorderRadius::Uniform(6.0))
    );
    assert_eq!(
        updated.attrs.border_width,
        Some(BorderWidth::Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        })
    );
    assert_eq!(updated.attrs.border_style, Some(BorderStyle::Dashed));
    assert_eq!(updated.attrs.font, Some(Font::Atom("display".to_string())));
    assert_eq!(
        updated.attrs.font_weight,
        Some(FontWeight("bold".to_string()))
    );
    assert_eq!(
        updated.attrs.font_style,
        Some(FontStyle("italic".to_string()))
    );
    assert_eq!(updated.attrs.font_size, Some(22.0));
    assert_eq!(updated.attrs.font_underline, Some(true));
    assert_eq!(updated.attrs.font_strike, Some(true));
    assert_eq!(updated.attrs.font_letter_spacing, Some(3.0));
    assert_eq!(updated.attrs.font_word_spacing, Some(4.0));
    assert_eq!(updated.attrs.text_align, Some(TextAlign::Center));
    assert_eq!(updated.attrs.move_x, Some(5.0));
    assert_eq!(updated.attrs.alpha, Some(0.5));
    assert_eq!(
        updated.attrs.background,
        Some(crate::tree::attrs::Background::Color(
            crate::tree::attrs::Color::Rgb {
                r: 200,
                g: 100,
                b: 50
            }
        ))
    );
}

#[test]
fn test_interaction_style_merge_order_prefers_mouse_down_on_conflict() {
    let mut tree = ElementTree::new();

    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(100.0));
    attrs.height = Some(Length::Px(40.0));
    attrs.mouse_over = Some(MouseOverAttrs {
        border_width: Some(BorderWidth::Uniform(1.0)),
        border_style: Some(BorderStyle::Dashed),
        border_color: Some(crate::tree::attrs::Color::Rgb {
            r: 160,
            g: 90,
            b: 70,
        }),
        box_shadows: Some(vec![crate::tree::attrs::BoxShadow {
            offset_x: 0.0,
            offset_y: 1.0,
            blur: 4.0,
            size: 0.0,
            color: crate::tree::attrs::Color::Named("black".to_string()),
            inset: false,
        }]),
        font: Some(Font::Atom("hover".to_string())),
        font_size: Some(18.0),
        text_align: Some(TextAlign::Left),
        move_x: Some(5.0),
        ..Default::default()
    });
    attrs.focused = Some(MouseOverAttrs {
        border_width: Some(BorderWidth::Uniform(2.0)),
        border_style: Some(BorderStyle::Solid),
        border_color: Some(crate::tree::attrs::Color::Rgb {
            r: 80,
            g: 160,
            b: 90,
        }),
        font: Some(Font::Atom("focus".to_string())),
        font_weight: Some(FontWeight("bold".to_string())),
        font_style: Some(FontStyle("italic".to_string())),
        font_size: Some(24.0),
        font_color: Some(crate::tree::attrs::Color::Rgb {
            r: 220,
            g: 240,
            b: 255,
        }),
        box_shadows: Some(vec![crate::tree::attrs::BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 8.0,
            size: 2.0,
            color: crate::tree::attrs::Color::Named("cyan".to_string()),
            inset: false,
        }]),
        text_align: Some(TextAlign::Center),
        alpha: Some(0.8),
        ..Default::default()
    });
    attrs.mouse_down = Some(MouseOverAttrs {
        border_width: Some(BorderWidth::Uniform(3.0)),
        border_style: Some(BorderStyle::Dotted),
        border_color: Some(crate::tree::attrs::Color::Rgb {
            r: 70,
            g: 90,
            b: 180,
        }),
        box_shadows: Some(vec![crate::tree::attrs::BoxShadow {
            offset_x: 0.0,
            offset_y: 1.0,
            blur: 6.0,
            size: 1.0,
            color: crate::tree::attrs::Color::Named("white".to_string()),
            inset: true,
        }]),
        font: Some(Font::String("pressed".to_string())),
        font_size: Some(30.0),
        text_align: Some(TextAlign::Right),
        move_y: Some(-2.0),
        ..Default::default()
    });
    attrs.mouse_over_active = Some(true);
    attrs.focused_active = Some(true);
    attrs.mouse_down_active = Some(true);

    let root = make_element("root", ElementKind::TextInput, attrs);
    let root_id = root.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(root);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let updated = tree.get(&root_id).unwrap();
    assert_eq!(updated.attrs.font_size, Some(30.0));
    assert_eq!(
        updated.attrs.border_color,
        Some(crate::tree::attrs::Color::Rgb {
            r: 70,
            g: 90,
            b: 180
        })
    );
    assert_eq!(
        updated.attrs.font_color,
        Some(crate::tree::attrs::Color::Rgb {
            r: 220,
            g: 240,
            b: 255
        })
    );
    assert_eq!(updated.attrs.border_width, Some(BorderWidth::Uniform(3.0)));
    assert_eq!(updated.attrs.border_style, Some(BorderStyle::Dotted));
    assert_eq!(
        updated.attrs.font,
        Some(Font::String("pressed".to_string()))
    );
    assert_eq!(
        updated.attrs.font_weight,
        Some(FontWeight("bold".to_string()))
    );
    assert_eq!(
        updated.attrs.font_style,
        Some(FontStyle("italic".to_string()))
    );
    assert_eq!(updated.attrs.text_align, Some(TextAlign::Right));
    assert_eq!(updated.attrs.move_x, Some(5.0));
    assert_eq!(updated.attrs.move_y, Some(-2.0));
    assert_eq!(updated.attrs.alpha, Some(0.8));
    let shadow = updated
        .attrs
        .box_shadows
        .as_ref()
        .and_then(|shadows| shadows.first())
        .expect("mouse_down style should win shadow conflicts");
    assert!(shadow.inset);
    assert_eq!(shadow.blur, 6.0);
    assert_eq!(
        shadow.color,
        crate::tree::attrs::Color::Named("white".to_string())
    );
}

#[test]
fn test_scale_attrs_scales_border_shadow_motion_and_scroll_fields() {
    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Minimum(
        10.0,
        Box::new(Length::Maximum(20.0, Box::new(Length::Px(30.0)))),
    ));
    attrs.border_width = Some(BorderWidth::Sides {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    });
    attrs.border_radius = Some(crate::tree::attrs::BorderRadius::Corners {
        tl: 2.0,
        tr: 4.0,
        br: 6.0,
        bl: 8.0,
    });
    attrs.box_shadows = Some(vec![crate::tree::attrs::BoxShadow {
        offset_x: 1.0,
        offset_y: -2.0,
        blur: 3.0,
        size: 4.0,
        color: crate::tree::attrs::Color::Named("black".to_string()),
        inset: false,
    }]);
    attrs.move_x = Some(3.0);
    attrs.move_y = Some(-2.0);
    attrs.scroll_x = Some(5.0);
    attrs.scroll_y_max = Some(11.0);

    let scaled = scale_attrs(&attrs, 1.5);

    assert_eq!(
        scaled.width,
        Some(Length::Minimum(
            15.0,
            Box::new(Length::Maximum(30.0, Box::new(Length::Px(45.0)))),
        ))
    );
    assert_eq!(
        scaled.border_width,
        Some(BorderWidth::Sides {
            top: 1.5,
            right: 3.0,
            bottom: 4.5,
            left: 6.0,
        })
    );
    assert_eq!(
        scaled.border_radius,
        Some(crate::tree::attrs::BorderRadius::Corners {
            tl: 3.0,
            tr: 6.0,
            br: 9.0,
            bl: 12.0,
        })
    );

    let shadow = scaled
        .box_shadows
        .as_ref()
        .and_then(|shadows| shadows.first())
        .expect("scaled box shadow should exist");
    assert_eq!(shadow.offset_x, 1.5);
    assert_eq!(shadow.offset_y, -3.0);
    assert_eq!(shadow.blur, 4.5);
    assert_eq!(shadow.size, 6.0);

    assert_eq!(scaled.move_x, Some(4.5));
    assert_eq!(scaled.move_y, Some(-3.0));
    assert_eq!(scaled.scroll_x, Some(7.5));
    assert_eq!(scaled.scroll_y_max, Some(16.5));
}

#[test]
fn test_scale_attrs_scales_mouse_over_numeric_fields() {
    let mut attrs = Attrs::default();
    attrs.mouse_over = Some(MouseOverAttrs {
        border_radius: Some(BorderRadius::Corners {
            tl: 1.0,
            tr: 2.0,
            br: 3.0,
            bl: 4.0,
        }),
        border_width: Some(BorderWidth::Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        }),
        border_style: Some(BorderStyle::Dashed),
        font: Some(Font::Atom("display".to_string())),
        font_weight: Some(FontWeight("bold".to_string())),
        font_style: Some(FontStyle("italic".to_string())),
        font_size: Some(12.0),
        font_letter_spacing: Some(1.0),
        font_word_spacing: Some(2.0),
        text_align: Some(TextAlign::Center),
        move_x: Some(-3.0),
        move_y: Some(4.0),
        box_shadows: Some(vec![crate::tree::attrs::BoxShadow {
            offset_x: 1.0,
            offset_y: -2.0,
            blur: 3.0,
            size: 4.0,
            color: crate::tree::attrs::Color::Named("black".to_string()),
            inset: false,
        }]),
        ..Default::default()
    });
    attrs.focused = Some(MouseOverAttrs {
        border_radius: Some(BorderRadius::Uniform(5.0)),
        border_width: Some(BorderWidth::Uniform(2.0)),
        border_style: Some(BorderStyle::Dotted),
        font: Some(Font::String("mono".to_string())),
        font_weight: Some(FontWeight("bold".to_string())),
        font_style: Some(FontStyle("italic".to_string())),
        font_size: Some(10.0),
        alpha: Some(0.5),
        text_align: Some(TextAlign::Right),
        box_shadows: Some(vec![crate::tree::attrs::BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 6.0,
            size: 1.0,
            color: crate::tree::attrs::Color::Named("blue".to_string()),
            inset: false,
        }]),
        ..Default::default()
    });
    attrs.mouse_down = Some(MouseOverAttrs {
        border_radius: Some(BorderRadius::Uniform(2.0)),
        border_width: Some(BorderWidth::Uniform(1.5)),
        border_style: Some(BorderStyle::Solid),
        font: Some(Font::Atom("serif".to_string())),
        font_weight: Some(FontWeight("bold".to_string())),
        font_style: Some(FontStyle("italic".to_string())),
        move_x: Some(3.0),
        move_y: Some(-2.0),
        text_align: Some(TextAlign::Left),
        box_shadows: Some(vec![crate::tree::attrs::BoxShadow {
            offset_x: 0.5,
            offset_y: 1.5,
            blur: 2.0,
            size: 0.5,
            color: crate::tree::attrs::Color::Named("white".to_string()),
            inset: true,
        }]),
        ..Default::default()
    });

    let scaled = scale_attrs(&attrs, 2.0);
    let hover = scaled
        .mouse_over
        .as_ref()
        .expect("scaled mouse_over attrs should exist");

    assert_eq!(
        hover.border_radius,
        Some(BorderRadius::Corners {
            tl: 2.0,
            tr: 4.0,
            br: 6.0,
            bl: 8.0,
        })
    );
    assert_eq!(
        hover.border_width,
        Some(BorderWidth::Sides {
            top: 2.0,
            right: 4.0,
            bottom: 6.0,
            left: 8.0,
        })
    );
    assert_eq!(hover.border_style, Some(BorderStyle::Dashed));
    assert_eq!(hover.font, Some(Font::Atom("display".to_string())));
    assert_eq!(hover.font_weight, Some(FontWeight("bold".to_string())));
    assert_eq!(hover.font_style, Some(FontStyle("italic".to_string())));
    assert_eq!(hover.font_size, Some(24.0));
    assert_eq!(hover.font_letter_spacing, Some(2.0));
    assert_eq!(hover.font_word_spacing, Some(4.0));
    assert_eq!(hover.text_align, Some(TextAlign::Center));
    assert_eq!(hover.move_x, Some(-6.0));
    assert_eq!(hover.move_y, Some(8.0));
    let hover_shadow = hover
        .box_shadows
        .as_ref()
        .and_then(|shadows| shadows.first())
        .expect("hover shadow should scale");
    assert_eq!(hover_shadow.offset_x, 2.0);
    assert_eq!(hover_shadow.offset_y, -4.0);
    assert_eq!(hover_shadow.blur, 6.0);
    assert_eq!(hover_shadow.size, 8.0);

    let focused = scaled
        .focused
        .as_ref()
        .expect("scaled focused attrs should exist");
    assert_eq!(focused.border_radius, Some(BorderRadius::Uniform(10.0)));
    assert_eq!(focused.border_width, Some(BorderWidth::Uniform(4.0)));
    assert_eq!(focused.border_style, Some(BorderStyle::Dotted));
    assert_eq!(focused.font, Some(Font::String("mono".to_string())));
    assert_eq!(focused.font_weight, Some(FontWeight("bold".to_string())));
    assert_eq!(focused.font_style, Some(FontStyle("italic".to_string())));
    assert_eq!(focused.font_size, Some(20.0));
    assert_eq!(focused.alpha, Some(0.5));
    assert_eq!(focused.text_align, Some(TextAlign::Right));
    let focused_shadow = focused
        .box_shadows
        .as_ref()
        .and_then(|shadows| shadows.first())
        .expect("focused shadow should scale");
    assert_eq!(focused_shadow.blur, 12.0);
    assert_eq!(focused_shadow.size, 2.0);

    let mouse_down = scaled
        .mouse_down
        .as_ref()
        .expect("scaled mouse_down attrs should exist");
    assert_eq!(mouse_down.border_radius, Some(BorderRadius::Uniform(4.0)));
    assert_eq!(mouse_down.border_width, Some(BorderWidth::Uniform(3.0)));
    assert_eq!(mouse_down.border_style, Some(BorderStyle::Solid));
    assert_eq!(mouse_down.font, Some(Font::Atom("serif".to_string())));
    assert_eq!(mouse_down.font_weight, Some(FontWeight("bold".to_string())));
    assert_eq!(mouse_down.font_style, Some(FontStyle("italic".to_string())));
    assert_eq!(mouse_down.move_x, Some(6.0));
    assert_eq!(mouse_down.move_y, Some(-4.0));
    assert_eq!(mouse_down.text_align, Some(TextAlign::Left));
    let mouse_down_shadow = mouse_down
        .box_shadows
        .as_ref()
        .and_then(|shadows| shadows.first())
        .expect("mouse_down shadow should scale");
    assert_eq!(mouse_down_shadow.offset_x, 1.0);
    assert_eq!(mouse_down_shadow.offset_y, 3.0);
    assert_eq!(mouse_down_shadow.blur, 4.0);
    assert_eq!(mouse_down_shadow.size, 1.0);
    assert!(mouse_down_shadow.inset);
}

#[test]
fn test_scale_attrs_scales_animation_keyframe_numeric_fields() {
    let mut from = Attrs::default();
    from.width = Some(Length::Px(20.0));
    from.padding = Some(Padding::Uniform(4.0));
    from.move_x = Some(3.0);

    let mut to = Attrs::default();
    to.width = Some(Length::Px(40.0));
    to.padding = Some(Padding::Uniform(8.0));
    to.move_x = Some(9.0);

    let mut attrs = Attrs::default();
    attrs.animate = Some(AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 240.0,
        curve: AnimationCurve::Linear,
        repeat: AnimationRepeat::Loop,
    });

    let scaled = scale_attrs(&attrs, 2.0);
    let animate = scaled.animate.expect("scaled animation spec should exist");

    assert_eq!(animate.keyframes[0].width, Some(Length::Px(40.0)));
    assert_eq!(animate.keyframes[0].padding, Some(Padding::Uniform(8.0)));
    assert_eq!(animate.keyframes[0].move_x, Some(6.0));
    assert_eq!(animate.keyframes[1].width, Some(Length::Px(80.0)));
    assert_eq!(animate.keyframes[1].padding, Some(Padding::Uniform(16.0)));
    assert_eq!(animate.keyframes[1].move_x, Some(18.0));
}

#[test]
fn test_layout_uses_first_animation_keyframe_for_static_frames() {
    let mut tree = ElementTree::new();

    let mut from = Attrs::default();
    from.width = Some(Length::Px(100.0));
    from.height = Some(Length::Px(30.0));
    from.move_x = Some(12.0);

    let mut to = Attrs::default();
    to.width = Some(Length::Px(160.0));
    to.height = Some(Length::Px(30.0));
    to.move_x = Some(36.0);

    let mut attrs = Attrs::default();
    attrs.width = Some(Length::Px(40.0));
    attrs.height = Some(Length::Px(30.0));
    attrs.animate = Some(AnimationSpec {
        keyframes: vec![from, to],
        duration_ms: 180.0,
        curve: AnimationCurve::EaseInOut,
        repeat: AnimationRepeat::Once,
    });

    let root = make_element("root", ElementKind::El, attrs);
    let root_id = root.id.clone();
    tree.root = Some(root_id.clone());
    tree.insert(root);

    layout_tree(
        &mut tree,
        Constraint::new(300.0, 200.0),
        1.0,
        &MockTextMeasurer,
    );

    let updated = tree.get(&root_id).unwrap();
    let frame = updated.frame.unwrap();

    assert_eq!(updated.attrs.width, Some(Length::Px(100.0)));
    assert_eq!(updated.attrs.move_x, Some(12.0));
    assert_eq!(frame.width, 100.0);
}

#[test]
fn test_animated_nearby_hit_case_layout_geometry_matches_probe_story() {
    let case = AnimatedNearbyHitCase::width_move_in_front();

    let initial_tree = case.tree_at(0, false);
    let mid_tree = case.tree_at(500, false);
    let late_tree = case.tree_at(1000, false);

    let initial = initial_tree.get(&case.target_id).unwrap().frame.unwrap();
    let mid = mid_tree.get(&case.target_id).unwrap().frame.unwrap();
    let late = late_tree.get(&case.target_id).unwrap().frame.unwrap();
    let initial_move_x = initial_tree
        .get(&case.target_id)
        .unwrap()
        .attrs
        .move_x
        .unwrap();
    let mid_move_x = mid_tree.get(&case.target_id).unwrap().attrs.move_x.unwrap();
    let late_move_x = late_tree
        .get(&case.target_id)
        .unwrap()
        .attrs
        .move_x
        .unwrap();

    assert_eq!(initial.x, 16.0);
    assert_eq!(initial.width, 96.0);
    assert_eq!(initial.x + initial_move_x as f32, 0.0);

    assert_eq!(mid.x, 1.0);
    assert_eq!(mid.width, 126.0);
    assert_eq!(mid.x + mid_move_x as f32, 6.0);

    assert_eq!(late.x, -14.0);
    assert_eq!(late.width, 156.0);
    assert_eq!(late.x + late_move_x as f32, 12.0);
}
