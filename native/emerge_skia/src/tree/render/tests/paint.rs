use super::super::paint::SCROLLBAR_COLOR;
use super::common::*;
use super::*;
use crate::tree::geometry::{ClipShape, CornerRadii, Rect};
use resvg::usvg;

const DEMO_STATIC_JPEG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../priv/demo_images/static.jpg"
));

fn insert_test_svg_asset(id: &str, svg: &str) {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();

    let tree = usvg::Tree::from_str(svg, &options).expect("test SVG should parse");
    crate::renderer::insert_vector_asset(id, tree).expect("test SVG should insert");
}

fn point_in_rounded_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, radius: f32) -> bool {
    if w <= 0.0 || h <= 0.0 {
        return false;
    }

    let r = radius.max(0.0).min((w * 0.5).min(h * 0.5));
    let left = x;
    let right = x + w;
    let top = y;
    let bottom = y + h;

    if px < left || px > right || py < top || py > bottom {
        return false;
    }

    if r <= 0.0 {
        return true;
    }

    if (px >= left + r && px <= right - r) || (py >= top + r && py <= bottom - r) {
        return true;
    }

    let cx = if px < left + r { left + r } else { right - r };
    let cy = if py < top + r { top + r } else { bottom - r };
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= r * r
}

fn point_in_inset_rounded_rect(
    px: f32,
    py: f32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    inset: f32,
) -> bool {
    let inset = inset.max(0.0);
    let inset_x = x + inset;
    let inset_y = y + inset;
    let inset_w = (w - inset * 2.0).max(0.0);
    let inset_h = (h - inset * 2.0).max(0.0);
    let inset_r = (radius - inset).max(0.0);
    point_in_rounded_rect(px, py, inset_x, inset_y, inset_w, inset_h, inset_r)
}

fn build_scroll_panel_with_cards(
    panel_attrs: Attrs,
    panel_frame: Frame,
    cards: Vec<(u8, Attrs, Frame)>,
) -> ElementTree {
    let panel_id = ElementId::from_term_bytes(vec![90]);
    let mut panel = Element::with_attrs(panel_id.clone(), ElementKind::El, Vec::new(), panel_attrs);
    panel.frame = Some(panel_frame);

    let mut tree = ElementTree::new();
    tree.root = Some(panel_id.clone());

    let child_ids = cards
        .iter()
        .map(|(id_byte, _attrs, _frame)| ElementId::from_term_bytes(vec![*id_byte]))
        .collect::<Vec<_>>();
    panel.children = child_ids.clone();
    tree.insert(panel);

    for ((id_byte, attrs, frame), child_id) in cards.into_iter().zip(child_ids.into_iter()) {
        let mut child = Element::with_attrs(child_id, ElementKind::El, Vec::new(), attrs);
        child.frame = Some(frame);
        tree.insert(child);
        let _ = id_byte;
    }

    tree
}

fn demo_glow_card_attrs(glow_color: Color, size: f64) -> Attrs {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgba {
        r: 45,
        g: 45,
        b: 68,
        a: 255,
    }));
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: size * 2.0,
        size,
        color: glow_color,
        inset: false,
    }]);
    attrs
}

fn demo_glow_card_attrs_without_glow() -> Attrs {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgba {
        r: 45,
        g: 45,
        b: 68,
        a: 255,
    }));
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    attrs
}

fn demo_combined_glow_card_attrs() -> Attrs {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgba {
            r: 67,
            g: 97,
            b: 238,
            a: 255,
        },
        to: Color::Rgba {
            r: 114,
            g: 9,
            b: 183,
            a: 255,
        },
        angle: 135.0,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(10.0));
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_color = Some(Color::Named("cyan".to_string()));
    attrs.border_style = Some(BorderStyle::Dotted);
    attrs.box_shadows = Some(vec![
        BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 6.0,
            size: 3.0,
            color: Color::Named("magenta".to_string()),
            inset: false,
        },
        BoxShadow {
            offset_x: 2.0,
            offset_y: 2.0,
            blur: 8.0,
            size: 0.0,
            color: Color::Named("purple".to_string()),
            inset: true,
        },
    ]);
    attrs
}

fn demo_inset_glow_dotted_card_attrs(with_glow: bool) -> Attrs {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgba {
        r: 45,
        g: 45,
        b: 68,
        a: 255,
    }));
    attrs.border_radius = Some(BorderRadius::Uniform(10.0));
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_color = Some(Color::Named("cyan".to_string()));
    attrs.border_style = Some(BorderStyle::Dotted);

    let mut shadows = Vec::new();
    if with_glow {
        shadows.push(BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 6.0,
            size: 3.0,
            color: Color::Named("magenta".to_string()),
            inset: false,
        });
    }
    shadows.push(BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 0.0,
        color: Color::Named("purple".to_string()),
        inset: true,
    });
    attrs.box_shadows = Some(shadows);
    attrs
}

#[test]
fn test_render_image_source_pending_emits_loading_placeholder() {
    let id = ElementId::from_term_bytes(vec![9]);
    let mut attrs = Attrs::default();
    attrs.image_src = Some(ImageSource::Logical("images/photo.jpg".to_string()));
    attrs.image_fit = Some(ImageFit::Contain);

    let mut element = Element::with_attrs(id.clone(), ElementKind::Image, Vec::new(), attrs);
    element.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 90.0,
        content_width: 120.0,
        content_height: 90.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);

    let draws = observe_tree(&tree);

    assert!(
        draws
            .iter()
            .any(|draw| matches!(draw.primitive, DrawPrimitive::ImageLoading(_, _, _, _)))
    );
}

#[test]
fn test_render_image_cover_border_has_no_inner_gap_from_background() {
    let image_id = "paint_image_cover_border_no_inner_gap";
    crate::renderer::insert_raster_asset(image_id, DEMO_STATIC_JPEG)
        .expect("test JPEG should insert");

    let outer_x: f32 = 12.0;
    let outer_y: f32 = 8.0;
    let outer_w: f32 = 54.0;
    let outer_h: f32 = 38.0;
    let border: f32 = 1.5;
    let radius: f32 = 9.0;

    let inner_x = outer_x + border;
    let inner_y = outer_y + border;
    let inner_w = outer_w - border * 2.0;
    let inner_h = outer_h - border * 2.0;
    let inner_r = (radius - border).max(0.0);

    let render_scene = |background: Color| {
        let mut attrs = Attrs::default();
        attrs.background = Some(Background::Color(background));
        attrs.image_src = Some(ImageSource::Id(image_id.to_string()));
        attrs.image_fit = Some(ImageFit::Cover);
        attrs.border_width = Some(BorderWidth::Uniform(border as f64));
        attrs.border_radius = Some(BorderRadius::Uniform(radius as f64));
        attrs.border_color = Some(Color::Rgba {
            r: 214,
            g: 220,
            b: 236,
            a: 221,
        });

        let tree = build_image_tree_with_frame(
            attrs,
            Frame {
                x: outer_x,
                y: outer_y,
                width: outer_w,
                height: outer_h,
                content_width: outer_w,
                content_height: outer_h,
            },
        );

        render_tree_to_pixels(80, 60, &tree).1
    };

    let dark_bg_pixels = render_scene(Color::Rgb { r: 5, g: 7, b: 11 });
    let bright_bg_pixels = render_scene(Color::Rgb {
        r: 245,
        g: 232,
        b: 122,
    });

    let mut band_count = 0usize;
    let mut changed_count = 0usize;
    let mut max_channel_diff = 0u8;

    for y in 0..60u32 {
        for x in 0..80u32 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let in_inner_near_edge = point_in_inset_rounded_rect(
                px, py, inner_x, inner_y, inner_w, inner_h, inner_r, 0.05,
            );
            let in_inner_deep = point_in_inset_rounded_rect(
                px, py, inner_x, inner_y, inner_w, inner_h, inner_r, 1.25,
            );

            if !(in_inner_near_edge && !in_inner_deep) {
                continue;
            }

            let (dr, dg, db, _da) = rgba_at(&dark_bg_pixels, 80, x, y);
            let (br, bg, bb, _ba) = rgba_at(&bright_bg_pixels, 80, x, y);

            let local_max = dr.abs_diff(br).max(dg.abs_diff(bg)).max(db.abs_diff(bb));
            max_channel_diff = max_channel_diff.max(local_max);

            band_count += 1;
            if local_max > 8 {
                changed_count += 1;
            }
        }
    }

    assert!(band_count > 0, "expected non-empty inner edge band");
    assert!(
        max_channel_diff <= 8,
        "expected inner edge pixels to stay background-invariant, max channel diff was {}",
        max_channel_diff
    );
    assert!(
        changed_count <= 2,
        "expected <=2 significantly changed inner-edge pixels, got {} of {}",
        changed_count,
        band_count
    );
}

#[test]
fn test_render_nested_image_cover_has_no_inner_gap_from_parent_background() {
    let image_id = "paint_nested_image_cover_no_inner_gap";
    let mut src = vec![0u8; 24 * 16 * 4];
    for px in src.chunks_exact_mut(4) {
        px[0] = 36;
        px[1] = 216;
        px[2] = 72;
        px[3] = 255;
    }
    crate::renderer::insert_test_raster_asset_rgba(image_id, 24, 16, &src)
        .expect("test raster asset should insert");

    let outer_x: f32 = 12.0;
    let outer_y: f32 = 8.0;
    let outer_w: f32 = 280.0;
    let outer_h: f32 = 120.0;
    let border: f32 = 1.0;
    let radius: f32 = 8.0;

    let inner_x = outer_x + border;
    let inner_y = outer_y + border;
    let inner_w = outer_w - border * 2.0;
    let inner_h = outer_h - border * 2.0;

    let render_scene = |background: Color| {
        let mut parent_attrs = Attrs::default();
        parent_attrs.background = Some(Background::Color(background));
        parent_attrs.border_width = Some(BorderWidth::Uniform(border as f64));
        parent_attrs.border_radius = Some(BorderRadius::Uniform(radius as f64));
        parent_attrs.border_color = Some(Color::Rgba {
            r: 214,
            g: 220,
            b: 236,
            a: 220,
        });

        let mut child_attrs = Attrs::default();
        child_attrs.image_src = Some(ImageSource::Id(image_id.to_string()));
        child_attrs.image_fit = Some(ImageFit::Cover);

        let tree = build_tree_with_image_child_frame(
            parent_attrs,
            Frame {
                x: outer_x,
                y: outer_y,
                width: outer_w,
                height: outer_h,
                content_width: outer_w,
                content_height: outer_h,
            },
            child_attrs,
            Frame {
                x: inner_x,
                y: inner_y,
                width: inner_w,
                height: inner_h,
                content_width: inner_w,
                content_height: inner_h,
            },
        );

        render_tree_to_pixels(320, 160, &tree).1
    };

    let dark_bg_pixels = render_scene(Color::Rgb {
        r: 24,
        g: 24,
        b: 36,
    });
    let bright_bg_pixels = render_scene(Color::Rgb {
        r: 245,
        g: 232,
        b: 122,
    });

    let mut border_band_count = 0usize;
    let mut changed_count = 0usize;
    let mut max_channel_diff = 0u8;
    let mut worst = None;
    let mut max_straight_edge_diff = 0u8;
    let mut worst_straight = None;

    for y in 0..160u32 {
        for x in 0..320u32 {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let in_outer =
                point_in_rounded_rect(px, py, outer_x, outer_y, outer_w, outer_h, radius);
            let away_from_outer_aa = point_in_inset_rounded_rect(
                px, py, outer_x, outer_y, outer_w, outer_h, radius, 0.15,
            );
            let inside_inner_content = point_in_inset_rounded_rect(
                px,
                py,
                outer_x,
                outer_y,
                outer_w,
                outer_h,
                radius,
                border + 0.15,
            );

            if !(in_outer && away_from_outer_aa && !inside_inner_content) {
                continue;
            }

            let (dr, dg, db, _da) = rgba_at(&dark_bg_pixels, 320, x, y);
            let (br, bg, bb, _ba) = rgba_at(&bright_bg_pixels, 320, x, y);

            let local_max = dr.abs_diff(br).max(dg.abs_diff(bg)).max(db.abs_diff(bb));
            if local_max >= max_channel_diff {
                max_channel_diff = local_max;
                worst = Some((x, y, (dr, dg, db), (br, bg, bb)));
            }

            let on_top_or_bottom = x as f32 >= outer_x + radius + 6.0
                && x as f32 <= outer_x + outer_w - radius - 6.0
                && (y as f32 <= outer_y + 2.0 || y as f32 >= outer_y + outer_h - 3.0);
            let on_left_or_right = y as f32 >= outer_y + radius + 6.0
                && y as f32 <= outer_y + outer_h - radius - 6.0
                && (x as f32 <= outer_x + 2.0 || x as f32 >= outer_x + outer_w - 3.0);

            if (on_top_or_bottom || on_left_or_right) && local_max >= max_straight_edge_diff {
                max_straight_edge_diff = local_max;
                worst_straight = Some((x, y, (dr, dg, db), (br, bg, bb)));
            }

            border_band_count += 1;
            if local_max > 8 {
                changed_count += 1;
            }
        }
    }

    assert!(
        border_band_count > 0,
        "expected non-empty border band sample"
    );
    assert!(
        max_straight_edge_diff <= 8,
        "expected nested image straight border edges to stay background-invariant, max straight-edge diff: {}, worst straight pixel: {:?}, overall max diff: {}, worst pixel: {:?}",
        max_straight_edge_diff,
        worst_straight,
        max_channel_diff,
        worst
    );
    assert!(
        max_channel_diff <= 24,
        "expected rounded-corner leakage to stay bounded, overall max diff was {}, worst pixel: {:?}",
        max_channel_diff,
        worst
    );
    assert!(
        changed_count <= 2,
        "expected <=2 significantly changed nested border-band pixels, got {} of {}",
        changed_count,
        border_band_count
    );
}

#[test]
fn test_render_nested_image_contain_has_no_right_gap_when_touching_horizontal_edges() {
    let image_id = "paint_nested_image_contain_no_right_gap";
    let mut src = vec![0u8; 24 * 16 * 4];
    for px in src.chunks_exact_mut(4) {
        px[0] = 36;
        px[1] = 216;
        px[2] = 72;
        px[3] = 255;
    }
    crate::renderer::insert_test_raster_asset_rgba(image_id, 24, 16, &src)
        .expect("test raster asset should insert");

    for (frame_w, frame_h, label) in [(140.0f32, 240.0f32, "tall"), (180.0, 180.0, "square")] {
        let outer_x: f32 = 12.0;
        let outer_y: f32 = 8.0;
        let outer_w: f32 = frame_w;
        let outer_h: f32 = frame_h;
        let border: f32 = 1.0;
        let radius: f32 = 8.0;

        let inner_x = outer_x + border;
        let inner_y = outer_y + border;
        let inner_w = outer_w - border * 2.0;
        let inner_h = outer_h - border * 2.0;

        let src_w = 24.0f32;
        let src_h = 16.0f32;
        let scale = (inner_w / src_w).min(inner_h / src_h);
        let draw_h = src_h * scale;
        let draw_y = inner_y + (inner_h - draw_h) * 0.5;

        let render_scene = |background: Color| {
            let mut parent_attrs = Attrs::default();
            parent_attrs.background = Some(Background::Color(background));
            parent_attrs.border_width = Some(BorderWidth::Uniform(border as f64));
            parent_attrs.border_radius = Some(BorderRadius::Uniform(radius as f64));
            parent_attrs.border_color = Some(Color::Rgba {
                r: 214,
                g: 220,
                b: 236,
                a: 220,
            });

            let mut child_attrs = Attrs::default();
            child_attrs.image_src = Some(ImageSource::Id(image_id.to_string()));
            child_attrs.image_fit = Some(ImageFit::Contain);

            let tree = build_tree_with_image_child_frame(
                parent_attrs,
                Frame {
                    x: outer_x,
                    y: outer_y,
                    width: outer_w,
                    height: outer_h,
                    content_width: outer_w,
                    content_height: outer_h,
                },
                child_attrs,
                Frame {
                    x: inner_x,
                    y: inner_y,
                    width: inner_w,
                    height: inner_h,
                    content_width: inner_w,
                    content_height: inner_h,
                },
            );

            render_tree_to_pixels((frame_w as u32) + 40, (frame_h as u32) + 40, &tree).1
        };

        let dark_bg_pixels = render_scene(Color::Rgb {
            r: 24,
            g: 24,
            b: 36,
        });
        let bright_bg_pixels = render_scene(Color::Rgb {
            r: 245,
            g: 232,
            b: 122,
        });

        let width = (frame_w as u32) + 40;
        let height = (frame_h as u32) + 40;
        let mut sample_count = 0usize;
        let mut max_channel_diff = 0u8;

        for y in 0..height {
            for x in 0..width {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;

                let in_outer =
                    point_in_rounded_rect(px, py, outer_x, outer_y, outer_w, outer_h, radius);
                let away_from_outer_aa = point_in_inset_rounded_rect(
                    px, py, outer_x, outer_y, outer_w, outer_h, radius, 0.15,
                );
                let in_right_band = px >= outer_x + outer_w - border - 0.2;
                let in_image_vertical = py >= draw_y + 8.0 && py <= draw_y + draw_h - 8.0;

                if !(in_outer && away_from_outer_aa && in_right_band && in_image_vertical) {
                    continue;
                }

                let (dr, dg, db, _da) = rgba_at(&dark_bg_pixels, width, x, y);
                let (br, bg, bb, _ba) = rgba_at(&bright_bg_pixels, width, x, y);
                let local_max = dr.abs_diff(br).max(dg.abs_diff(bg)).max(db.abs_diff(bb));
                max_channel_diff = max_channel_diff.max(local_max);
                sample_count += 1;
            }
        }

        assert!(
            sample_count > 0,
            "expected non-empty right-edge sample for {} frame",
            label
        );
        assert!(
            max_channel_diff <= 8,
            "expected contain right edge to stay background-invariant for {} frame, max diff was {}",
            label,
            max_channel_diff
        );
    }
}

#[test]
fn test_render_background_image_pending_uses_self_clip() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Image {
        source: ImageSource::Logical("images/background_pending_clip.png".to_string()),
        fit: ImageFit::Cover,
    });
    attrs.border_radius = Some(BorderRadius::Corners {
        tl: 6.0,
        tr: 10.0,
        br: 12.0,
        bl: 8.0,
    });

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::ImageLoading(0.0, 0.0, 100.0, 50.0)
        )
    });

    assert_eq!(background.clips.len(), 1);
    assert_eq!(
        background.clips[0].shape,
        ClipShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            },
            radii: Some(CornerRadii {
                tl: 6.0,
                tr: 10.0,
                br: 12.0,
                bl: 8.0,
            }),
        }
    );
}

#[test]
fn test_render_background_image_failed_uses_self_clip() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Image {
        source: ImageSource::Logical("images/background_failed_clip.png".to_string()),
        fit: ImageFit::Cover,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(9.0));

    let tree = build_tree_with_attrs(attrs);
    crate::assets::resolve_tree_sources_sync(&tree, None)
        .expect("background image source resolution should complete");

    let draws = observe_tree(&tree);
    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::ImageFailed(0.0, 0.0, 100.0, 50.0)
        )
    });

    assert_eq!(background.clips.len(), 1);
    assert_eq!(
        background.clips[0].shape,
        ClipShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            },
            radii: Some(CornerRadii {
                tl: 9.0,
                tr: 9.0,
                br: 9.0,
                bl: 9.0,
            }),
        }
    );
}

#[test]
fn test_render_svg_source_with_color_emits_tinted_image_command() {
    let image_id = "paint_svg_tinted_image_command";
    insert_test_svg_asset(
        image_id,
        r##"
        <svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" viewBox="0 0 2 2">
          <rect x="0" y="0" width="2" height="2" fill="#ff0000" />
        </svg>
        "##,
    );

    let id = ElementId::from_term_bytes(vec![19]);
    let mut attrs = Attrs::default();
    attrs.image_src = Some(ImageSource::Id(image_id.to_string()));
    attrs.image_fit = Some(ImageFit::Contain);
    attrs.svg_expected = Some(true);
    attrs.svg_color = Some(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    });

    let mut element = Element::with_attrs(id.clone(), ElementKind::Image, Vec::new(), attrs);
    element.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
        content_width: 20.0,
        content_height: 20.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);

    let draws = observe_tree(&tree);

    assert!(draws.iter().any(|draw| {
        matches!(
            &draw.primitive,
            DrawPrimitive::Image(_, _, _, _, asset_id, ImageFit::Contain, Some(0xFFFFFFFF)) if asset_id == image_id
        )
    }));
}

#[test]
fn test_render_svg_source_rejects_raster_asset_ids() {
    let image_id = "paint_svg_rejects_raster_asset_id";
    crate::renderer::insert_raster_asset(image_id, DEMO_STATIC_JPEG)
        .expect("test JPEG should insert");

    let id = ElementId::from_term_bytes(vec![20]);
    let mut attrs = Attrs::default();
    attrs.image_src = Some(ImageSource::Id(image_id.to_string()));
    attrs.image_fit = Some(ImageFit::Contain);
    attrs.svg_expected = Some(true);

    let mut element = Element::with_attrs(id.clone(), ElementKind::Image, Vec::new(), attrs);
    element.frame = Some(Frame {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
        content_width: 20.0,
        content_height: 20.0,
    });

    let mut tree = ElementTree::new();
    tree.root = Some(id);
    tree.insert(element);

    let draws = observe_tree(&tree);

    assert!(draws.iter().any(|draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::ImageFailed(0.0, 0.0, 20.0, 20.0)
        )
    }));
}

#[test]
fn test_render_scrollbar_y_thumb() {
    let mut attrs = Attrs::default();
    attrs.scrollbar_y = Some(true);
    attrs.scroll_y = Some(50.0);
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 150.0,
    };
    let tree = build_tree_with_frame(attrs, frame);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 100.0, 50.0, 0x000000FF)
        )
    });
    let thumb = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::RoundedRect(95.0, 13.0, 5.0, 24.0, 2.5, SCROLLBAR_COLOR)
        )
    });

    let expected_clip = ClipShape {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        },
        radii: Some(CornerRadii {
            tl: 8.0,
            tr: 8.0,
            br: 8.0,
            bl: 8.0,
        }),
    };

    assert_eq!(background.clips.len(), 1);
    assert_eq!(background.clips[0].shape, expected_clip);
    let thumb_clip_scopes = clip_scope_chain(&trace, thumb);
    assert_eq!(thumb_clip_scopes.len(), 1);
    assert_eq!(
        clip_scope_shapes(thumb_clip_scopes[0]).expect("thumb should be inside a clip scope"),
        &[expected_clip]
    );
    assert_eq!(thumb.clips[0].shape, expected_clip);
    assert!(thumb.clips[0].shape.rect.width < 10_000.0);
    assert!(thumb.clips[0].shape.rect.height < 10_000.0);
}

#[test]
fn test_render_scrollbar_x_thumb() {
    let mut attrs = Attrs::default();
    attrs.scrollbar_x = Some(true);
    attrs.scroll_x = Some(30.0);
    attrs.border_radius = Some(BorderRadius::Corners {
        tl: 4.0,
        tr: 6.0,
        br: 12.0,
        bl: 8.0,
    });
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 40.0,
        content_width: 160.0,
        content_height: 40.0,
    };
    let tree = build_tree_with_frame(attrs, frame);
    let trace = trace_tree(&tree);
    let draws = &trace.draws;

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(0.0, 0.0, 80.0, 40.0, 0x000000FF)
        )
    });

    let thumb = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::RoundedRect(15.0, 35.0, 40.0, 5.0, 2.5, SCROLLBAR_COLOR)
        )
    });
    let expected_clip = ClipShape {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
        },
        radii: Some(CornerRadii {
            tl: 4.0,
            tr: 6.0,
            br: 12.0,
            bl: 8.0,
        }),
    };

    assert_eq!(background.clips.len(), 1);
    assert_eq!(background.clips[0].shape, expected_clip);
    let thumb_clip_scopes = clip_scope_chain(&trace, thumb);
    assert_eq!(thumb_clip_scopes.len(), 1);
    assert_eq!(
        clip_scope_shapes(thumb_clip_scopes[0]).expect("thumb should be inside a clip scope"),
        &[expected_clip]
    );
    assert_eq!(thumb.clips[0].shape, expected_clip);
}

#[test]
fn test_render_scrollbar_hover_uses_wider_thumb() {
    let mut attrs = Attrs::default();
    attrs.scrollbar_y = Some(true);
    attrs.scroll_y = Some(50.0);
    attrs.scrollbar_hover_axis = Some(crate::tree::attrs::ScrollbarHoverAxis::Y);
    let frame = Frame {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 50.0,
        content_width: 100.0,
        content_height: 150.0,
    };
    let tree = build_tree_with_frame(attrs, frame);
    let draws = observe_tree(&tree);

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::RoundedRect(93.0, 13.0, 7.0, 24.0, 3.5, SCROLLBAR_COLOR)
        )
    });
}

#[test]
fn test_render_border_uniform_emits_border_cmd() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_color = Some(Color::Named("red".to_string()));
    attrs.border_radius = Some(BorderRadius::Uniform(4.0));

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Border(_, _, _, _, 4.0, 2.0, 0xFF0000FF, BorderStyle::Solid)
        )
    });
}

#[test]
fn test_render_border_edges_emits_border_edges_cmd() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    });
    attrs.border_color = Some(Color::Named("red".to_string()));

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::BorderEdges(
                _,
                _,
                _,
                _,
                _,
                1.0,
                2.0,
                3.0,
                4.0,
                0xFF0000FF,
                BorderStyle::Solid
            )
        )
    });
}

#[test]
fn test_render_border_dashed_passes_style() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    attrs.border_style = Some(BorderStyle::Dashed);
    attrs.border_color = Some(Color::Named("white".to_string()));

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Border(_, _, _, _, _, 2.0, 0xFFFFFFFF, BorderStyle::Dashed)
        )
    });
}

#[test]
fn test_render_shadow_emits_before_background() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Named("white".to_string())));
    attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 2.0,
        offset_y: 2.0,
        blur: 8.0,
        size: 4.0,
        color: Color::Named("black".to_string()),
        inset: false,
    }]);

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Shadow(..))
    });
    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(..) | DrawPrimitive::RoundedRect(..)
        )
    });

    assert!(
        paints_before(shadow, background),
        "shadow should render before background"
    );
}

#[test]
fn test_render_inset_shadow_emits_after_background() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Named("white".to_string())));
    attrs.box_shadows = Some(vec![BoxShadow {
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 10.0,
        size: 0.0,
        color: Color::Named("black".to_string()),
        inset: true,
    }]);

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let background = only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Rect(..) | DrawPrimitive::RoundedRect(..)
        )
    });
    let inset_shadow = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::InsetShadow(..))
    });

    assert!(
        paints_before(background, inset_shadow),
        "inset shadow should render after background"
    );
}

#[test]
fn test_glow_cards_in_scroll_y_panel_bleed_horizontally_at_outer_grid_edges() {
    let panel_frame = Frame {
        x: 20.0,
        y: 20.0,
        width: 980.0,
        height: 360.0,
        content_width: 980.0,
        content_height: 520.0,
    };

    let mut panel_attrs = Attrs::default();
    panel_attrs.scrollbar_y = Some(true);
    panel_attrs.background = Some(Background::Color(Color::Rgb {
        r: 35,
        g: 35,
        b: 55,
    }));
    panel_attrs.border_radius = Some(BorderRadius::Uniform(12.0));

    let cards = vec![
        (
            91,
            demo_glow_card_attrs(Color::Named("cyan".to_string()), 2.0),
            Frame {
                x: 40.0,
                y: 40.0,
                width: 300.0,
                height: 110.0,
                content_width: 300.0,
                content_height: 110.0,
            },
        ),
        (
            92,
            demo_glow_card_attrs(Color::Named("cyan".to_string()), 4.0),
            Frame {
                x: 352.0,
                y: 40.0,
                width: 300.0,
                height: 110.0,
                content_width: 300.0,
                content_height: 110.0,
            },
        ),
        (
            93,
            demo_glow_card_attrs(Color::Named("pink".to_string()), 7.0),
            Frame {
                x: 664.0,
                y: 40.0,
                width: 300.0,
                height: 110.0,
                content_width: 300.0,
                content_height: 110.0,
            },
        ),
        (
            94,
            demo_glow_card_attrs(Color::Named("blue".to_string()), 5.0),
            Frame {
                x: 40.0,
                y: 162.0,
                width: 300.0,
                height: 110.0,
                content_width: 300.0,
                content_height: 110.0,
            },
        ),
        (
            95,
            solid_fill_attrs((45, 45, 68)),
            Frame {
                x: 352.0,
                y: 162.0,
                width: 300.0,
                height: 110.0,
                content_width: 300.0,
                content_height: 110.0,
            },
        ),
        (
            96,
            demo_combined_glow_card_attrs(),
            Frame {
                x: 664.0,
                y: 162.0,
                width: 300.0,
                height: 110.0,
                content_width: 300.0,
                content_height: 110.0,
            },
        ),
    ];

    let tree = build_scroll_panel_with_cards(panel_attrs, panel_frame, cards);
    let (_output, pixels) = render_tree_to_pixels(1040, 420, &tree);

    let left_soft_alpha = rgba_at(&pixels, 1040, 36, 95).3;
    let left_blue_alpha = rgba_at(&pixels, 1040, 36, 217).3;
    let right_combined_alpha = rgba_at(&pixels, 1040, 968, 217).3;

    assert!(
        left_soft_alpha > 0,
        "expected left glow on the first glow card to bleed into panel padding"
    );
    assert!(
        left_blue_alpha > 0,
        "expected left glow on the second-row first-column glow card to bleed into panel padding"
    );
    assert!(
        right_combined_alpha > 0,
        "expected right glow on the bottom-right combined card to bleed into panel padding"
    );
}

#[test]
fn test_demo_like_nested_glow_cards_bleed_into_scroll_panel_padding_and_trailing_space() {
    let build_tree = |with_glow: bool| {
        let panel_id = ElementId::from_term_bytes(vec![100]);
        let column_id = ElementId::from_term_bytes(vec![101]);
        let glow_row_id = ElementId::from_term_bytes(vec![102]);
        let combined_row_id = ElementId::from_term_bytes(vec![103]);

        let panel_frame = Frame {
            x: 20.0,
            y: 20.0,
            width: 980.0,
            height: 760.0,
            content_width: 980.0,
            content_height: 920.0,
        };
        let column_frame = Frame {
            x: 36.0,
            y: 36.0,
            width: 948.0,
            height: 860.0,
            content_width: 948.0,
            content_height: 860.0,
        };
        let glow_row_frame = Frame {
            x: 36.0,
            y: 120.0,
            width: 948.0,
            height: 232.0,
            content_width: 948.0,
            content_height: 232.0,
        };
        let combined_row_frame = Frame {
            x: 36.0,
            y: 520.0,
            width: 948.0,
            height: 232.0,
            content_width: 948.0,
            content_height: 232.0,
        };

        let mut panel_attrs = Attrs::default();
        panel_attrs.scrollbar_y = Some(true);
        panel_attrs.padding = Some(Padding::Uniform(16.0));
        panel_attrs.background = Some(Background::Color(Color::Rgb {
            r: 35,
            g: 35,
            b: 55,
        }));
        panel_attrs.border_radius = Some(BorderRadius::Uniform(12.0));

        let column_attrs = Attrs::default();
        let glow_row_attrs = Attrs::default();
        let combined_row_attrs = Attrs::default();

        let glow_cards = vec![
            (
                110,
                if with_glow {
                    demo_glow_card_attrs(Color::Named("cyan".to_string()), 2.0)
                } else {
                    demo_glow_card_attrs_without_glow()
                },
                Frame {
                    x: 36.0,
                    y: 120.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                111,
                demo_glow_card_attrs(Color::Named("cyan".to_string()), 4.0),
                Frame {
                    x: 348.0,
                    y: 120.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                112,
                demo_glow_card_attrs(Color::Named("pink".to_string()), 7.0),
                Frame {
                    x: 660.0,
                    y: 120.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                113,
                if with_glow {
                    demo_glow_card_attrs(Color::Named("blue".to_string()), 5.0)
                } else {
                    demo_glow_card_attrs_without_glow()
                },
                Frame {
                    x: 36.0,
                    y: 242.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                114,
                demo_glow_card_attrs(Color::Named("purple".to_string()), 3.0),
                Frame {
                    x: 348.0,
                    y: 242.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                115,
                demo_glow_card_attrs(Color::Named("green".to_string()), 4.0),
                Frame {
                    x: 660.0,
                    y: 242.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
        ];

        let combined_cards = vec![
            (
                120,
                demo_glow_card_attrs_without_glow(),
                Frame {
                    x: 36.0,
                    y: 520.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                121,
                demo_combined_glow_card_attrs(),
                Frame {
                    x: 348.0,
                    y: 520.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                122,
                demo_glow_card_attrs_without_glow(),
                Frame {
                    x: 660.0,
                    y: 520.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                123,
                demo_glow_card_attrs_without_glow(),
                Frame {
                    x: 36.0,
                    y: 642.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                124,
                demo_glow_card_attrs_without_glow(),
                Frame {
                    x: 348.0,
                    y: 642.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
            (
                125,
                demo_inset_glow_dotted_card_attrs(with_glow),
                Frame {
                    x: 660.0,
                    y: 642.0,
                    width: 300.0,
                    height: 110.0,
                    content_width: 300.0,
                    content_height: 110.0,
                },
            ),
        ];

        let mut panel =
            Element::with_attrs(panel_id.clone(), ElementKind::El, Vec::new(), panel_attrs);
        panel.frame = Some(panel_frame);
        panel.children = vec![column_id.clone()];

        let mut column = Element::with_attrs(
            column_id.clone(),
            ElementKind::Column,
            Vec::new(),
            column_attrs,
        );
        column.frame = Some(column_frame);
        column.children = vec![glow_row_id.clone(), combined_row_id.clone()];

        let mut glow_row = Element::with_attrs(
            glow_row_id.clone(),
            ElementKind::WrappedRow,
            Vec::new(),
            glow_row_attrs,
        );
        glow_row.frame = Some(glow_row_frame);
        glow_row.children = glow_cards
            .iter()
            .map(|(id, _, _)| ElementId::from_term_bytes(vec![*id]))
            .collect();

        let mut combined_row = Element::with_attrs(
            combined_row_id.clone(),
            ElementKind::WrappedRow,
            Vec::new(),
            combined_row_attrs,
        );
        combined_row.frame = Some(combined_row_frame);
        combined_row.children = combined_cards
            .iter()
            .map(|(id, _, _)| ElementId::from_term_bytes(vec![*id]))
            .collect();

        let mut tree = ElementTree::new();
        tree.root = Some(panel_id);
        tree.insert(panel);
        tree.insert(column);
        tree.insert(glow_row);
        tree.insert(combined_row);

        for (id, attrs, frame) in glow_cards.into_iter().chain(combined_cards.into_iter()) {
            let id = ElementId::from_term_bytes(vec![id]);
            let mut child = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
            child.frame = Some(frame);
            tree.insert(child);
        }

        tree
    };

    let with_glow_tree = build_tree(true);
    let without_glow_tree = build_tree(false);
    let with_glow_trace = trace_tree(&with_glow_tree);
    let with_glow = render_tree_to_pixels(1040, 820, &with_glow_tree).1;
    let without_glow = render_tree_to_pixels(1040, 820, &without_glow_tree).1;

    let first_left_glow = only_draw(&with_glow_trace.draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Shadow(36.0, 120.0, 300.0, 110.0, 0.0, 0.0, 4.0, 2.0, 8.0, _)
        )
    });
    assert!(
        clip_scope_chain(&with_glow_trace, first_left_glow)
            .into_iter()
            .filter_map(clip_scope_shapes)
            .flatten()
            .all(|clip| !(clip.rect.x == 36.0 && clip.rect.width == 948.0)),
        "outer glow should not inherit the wrapped content clip"
    );

    let diff_at = |x: u32, y: u32| {
        let (wr, wg, wb, wa) = rgba_at(&with_glow, 1040, x, y);
        let (nr, ng, nb, na) = rgba_at(&without_glow, 1040, x, y);
        wr.abs_diff(nr)
            .max(wg.abs_diff(ng))
            .max(wb.abs_diff(nb))
            .max(wa.abs_diff(na))
    };

    let left_soft_diff = diff_at(32, 175);
    let left_blue_diff = diff_at(32, 297);
    let right_combined_diff = diff_at(966, 697);

    assert!(
        left_soft_diff > 6,
        "expected left glow on the first glow card to affect panel padding pixels, diff={}",
        left_soft_diff
    );
    assert!(
        left_blue_diff > 6,
        "expected left glow on the second-row first-column glow card to affect panel padding pixels, diff={}",
        left_blue_diff
    );
    assert!(
        right_combined_diff > 6,
        "expected right glow on the bottom-right combined card to affect trailing-space pixels, diff={}",
        right_combined_diff
    );
}

#[test]
fn test_render_no_border_without_color() {
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Uniform(2.0));
    // No border_color set

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    assert!(!draws.iter().any(|draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::Border(..) | DrawPrimitive::BorderEdges(..)
        )
    }));
}

#[test]
fn test_render_gradient_with_rounded_corners_uses_self_clip() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb {
            r: 67,
            g: 97,
            b: 238,
        },
        to: Color::Rgb {
            r: 114,
            g: 9,
            b: 183,
        },
        angle: 135.0,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(10.0));

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let gradient = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Gradient(..))
    });

    assert_eq!(gradient.clips.len(), 1);
    assert_eq!(
        gradient.clips[0].shape,
        ClipShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            },
            radii: Some(CornerRadii {
                tl: 10.0,
                tr: 10.0,
                br: 10.0,
                bl: 10.0,
            }),
        }
    );
}

#[test]
fn test_render_gradient_without_radius_has_no_self_clip() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb { r: 0, g: 0, b: 0 },
        to: Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        angle: 90.0,
    });
    // No border_radius set

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let gradient = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Gradient(..))
    });

    assert!(gradient.clips.is_empty());
}

#[test]
fn test_render_gradient_with_per_corner_radius_uses_self_clip() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb { r: 0, g: 0, b: 0 },
        to: Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        angle: 0.0,
    });
    attrs.border_radius = Some(BorderRadius::Corners {
        tl: 10.0,
        tr: 5.0,
        br: 10.0,
        bl: 5.0,
    });

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let gradient = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::Gradient(..))
    });

    assert_eq!(gradient.clips.len(), 1);
    assert_eq!(
        gradient.clips[0].shape,
        ClipShape {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            },
            radii: Some(CornerRadii {
                tl: 10.0,
                tr: 5.0,
                br: 10.0,
                bl: 5.0,
            }),
        }
    );
}

#[test]
fn test_render_gradient_with_per_corner_radius_clips_corner_pixels() {
    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Gradient {
        from: Color::Rgb { r: 0, g: 0, b: 0 },
        to: Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
        angle: 0.0,
    });
    attrs.border_radius = Some(BorderRadius::Corners {
        tl: 10.0,
        tr: 5.0,
        br: 10.0,
        bl: 5.0,
    });

    let tree = build_tree_with_attrs(attrs);
    let (_output, pixels) = render_tree_to_pixels(100, 50, &tree);

    assert_eq!(
        rgba_at(&pixels, 100, 0, 0).3,
        0,
        "top-left pixel should be clipped out by the rounded corner"
    );
    assert!(
        rgba_at(&pixels, 100, 20, 10).3 > 0,
        "interior pixel should remain painted"
    );
}

#[test]
fn test_render_uniform_pill_border_matches_clamped_rounded_clip() {
    let outer_x: f32 = 8.0;
    let outer_y: f32 = 12.0;
    let outer_w: f32 = 104.0;
    let outer_h: f32 = 36.0;
    let border: f32 = 1.0;
    let expected_radius = (outer_w * 0.5).min(outer_h * 0.5);

    let mut attrs = Attrs::default();
    attrs.background = Some(Background::Color(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    }));
    attrs.border_width = Some(BorderWidth::Uniform(border as f64));
    attrs.border_color = Some(Color::Rgb {
        r: 214,
        g: 220,
        b: 236,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(999.0));

    let tree = build_tree_with_frame(
        attrs,
        Frame {
            x: outer_x,
            y: outer_y,
            width: outer_w,
            height: outer_h,
            content_width: outer_w,
            content_height: outer_h,
        },
    );
    let (_output, pixels) = render_tree_to_pixels(120, 60, &tree);

    let outside_corner_samples = [(12, 13), (107, 13), (12, 46), (107, 46)];
    for (x, y) in outside_corner_samples {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        assert!(
            !point_in_rounded_rect(px, py, outer_x, outer_y, outer_w, outer_h, expected_radius),
            "sample ({}, {}) should sit outside the clamped pill shape",
            x,
            y,
        );

        assert!(
            rgba_at(&pixels, 120, x, y).3 <= 8,
            "expected rounded(999) border to stay inside the clamped pill clip at ({}, {})",
            x,
            y,
        );
    }

    let border_samples = [(20, 13), (99, 13), (20, 46), (99, 46)];
    for (x, y) in border_samples {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        assert!(
            point_in_rounded_rect(px, py, outer_x, outer_y, outer_w, outer_h, expected_radius)
                && !point_in_inset_rounded_rect(
                    px,
                    py,
                    outer_x,
                    outer_y,
                    outer_w,
                    outer_h,
                    expected_radius,
                    border,
                ),
            "sample ({}, {}) should land inside the visible border band",
            x,
            y,
        );

        assert!(
            rgba_at(&pixels, 120, x, y).3 >= 96,
            "expected visible border coverage at ({}, {})",
            x,
            y,
        );
    }

    let fill = rgba_at(&pixels, 120, 60, 30);
    assert!(fill.3 >= 240, "expected opaque fill at pill center");
}

#[test]
fn test_render_border_edges_asymmetric_widths() {
    // Regression: thick top/bottom, thin sides should emit correct per-edge widths
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 4.0,
        right: 1.0,
        bottom: 4.0,
        left: 1.0,
    });
    attrs.border_color = Some(Color::Rgb {
        r: 120,
        g: 200,
        b: 160,
    });
    attrs.border_radius = Some(BorderRadius::Uniform(8.0));

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let edges_cmd = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::BorderEdges(..))
    });

    match edges_cmd.primitive {
        DrawPrimitive::BorderEdges(_, _, _, _, radius, top, right, bottom, left, _, _) => {
            assert_eq!(top, 4.0);
            assert_eq!(right, 1.0);
            assert_eq!(bottom, 4.0);
            assert_eq!(left, 1.0);
            assert_eq!(radius, 8.0, "border radius should be passed through");
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_border_edges_bottom_only() {
    // Regression: bottom-only border should emit BorderEdges with zero for other sides
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 0.0,
        right: 0.0,
        bottom: 3.0,
        left: 0.0,
    });
    attrs.border_color = Some(Color::Rgb {
        r: 200,
        g: 180,
        b: 100,
    });

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    let edges_cmd = only_draw(&draws, |draw| {
        matches!(draw.primitive, DrawPrimitive::BorderEdges(..))
    });

    match edges_cmd.primitive {
        DrawPrimitive::BorderEdges(_, _, _, _, radius, top, right, bottom, left, _, _) => {
            assert_eq!(top, 0.0);
            assert_eq!(right, 0.0);
            assert_eq!(bottom, 3.0);
            assert_eq!(left, 0.0);
            assert_eq!(radius, 0.0, "no border radius set");
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_render_border_edges_with_style() {
    // Per-edge borders should forward the border style
    let mut attrs = Attrs::default();
    attrs.border_width = Some(BorderWidth::Sides {
        top: 2.0,
        right: 2.0,
        bottom: 2.0,
        left: 2.0,
    });
    attrs.border_color = Some(Color::Named("white".to_string()));
    attrs.border_style = Some(BorderStyle::Dashed);

    let tree = build_tree_with_attrs(attrs);
    let draws = observe_tree(&tree);

    only_draw(&draws, |draw| {
        matches!(
            draw.primitive,
            DrawPrimitive::BorderEdges(
                _,
                _,
                _,
                _,
                _,
                2.0,
                2.0,
                2.0,
                2.0,
                0xFFFFFFFF,
                BorderStyle::Dashed
            )
        )
    });
}
