use super::*;

#[test]
fn svg_gradient_tint_multiplies_source_alpha_and_reuses_untinted_variants() {
    let _guard = tests::vector_cache_test_lock();
    tests::reset_vector_cache_test_state();
    let id = "universal-gradient-alpha";
    tests::cache_test_svg_asset(
        id,
        2,
        2,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="1" height="1" fill="red"/><rect y="1" width="1" height="1" fill="blue" opacity="0.5"/><rect x="1" y="1" width="1" height="1" fill="green" opacity="0.25"/></svg>"##,
    );
    let bounds = GeometryRect {
        x: 2.0,
        y: 2.0,
        width: 8.0,
        height: 6.0,
    };
    let gradient = RenderColor::linear([0xff000080, 0x00ff00c0, 0x0000ff40], 30.0, bounds);
    let render = |primitive| tests::render_commands_to_pixels(12, 10, vec![primitive]);
    let paint = render(DrawPrimitive::Rect(2.0, 2.0, 8.0, 6.0, gradient.clone()));
    for fit in [
        ImageFit::Contain,
        ImageFit::Cover,
        ImageFit::Repeat,
        ImageFit::RepeatX,
        ImageFit::RepeatY,
    ] {
        let image = |color| DrawPrimitive::Image(2.0, 2.0, 8.0, 6.0, id.into(), fit, color);
        let mask = render(image(None));
        let actual = render(image(Some(gradient.clone())));
        for ((actual, paint), mask) in actual
            .chunks_exact(4)
            .zip(paint.chunks_exact(4))
            .zip(mask.chunks_exact(4))
        {
            for channel in 0..4 {
                let expected = (u16::from(paint[channel]) * u16::from(mask[3]) + 127) / 255;
                assert!(
                    (i32::from(actual[channel]) - i32::from(expected)).abs() <= 2,
                    "{fit:?}: {actual:?} vs {expected}, source={mask:?}"
                );
            }
        }
        let count = vector_rasterization_count();
        let changed = render(image(Some(RenderColor::linear(
            [0xffff0080, 0xff00ffff],
            90.0,
            bounds,
        ))));
        assert_ne!(actual, changed);
        assert_eq!(
            count,
            vector_rasterization_count(),
            "tint must not rerasterize the SVG"
        );
        let transparent = render(image(Some(RenderColor::linear(
            [0xffffff00, 0xff000000],
            0.0,
            bounds,
        ))));
        assert!(transparent.iter().all(|b| *b == 0));
    }
    remove_asset(id);
}

#[test]
fn gradient_svg_profiles_a_bounded_layer_and_preserves_grayscale_holes() {
    let _guard = tests::vector_cache_test_lock();
    let id = "universal-gradient-policy";
    tests::cache_test_svg_asset(
        id,
        2,
        2,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="1" height="2" fill="red"/></svg>"##,
    );
    let bounds = GeometryRect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
    };
    let gradient = RenderColor::linear([0xff0000ff, 0x0000ff00], 0.0, bounds);
    let scene = RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                20.0,
                20.0,
                RenderColor::linear([0x222222ff, 0xeeeeeeff], 0.0, bounds),
            )),
            RenderNode::Primitive(DrawPrimitive::Image(
                0.0,
                0.0,
                20.0,
                20.0,
                id.into(),
                ImageFit::Cover,
                Some(gradient),
            )),
        ],
    };
    let timings = tests::render_scene_graph_profiled(20, 20, scene.clone());
    let detail = timings.draw_detail.unwrap();
    assert_eq!(detail.layer_detail.tinted_image_layers, 1);
    assert!(detail.layer_detail.max_tinted_image_area_px <= 400);
    let renderer = SceneRenderer::new();
    let policy = renderer
        .render_grayscale_dither_policy(20, 20, &RenderState::new(scene, Color::WHITE, 1, false))
        .unwrap();
    let dithers = |x: usize, y: usize| {
        let i = y * 20 + x;
        policy[i / 8] & (1 << (7 - i % 8)) != 0
    };
    assert!(
        dithers(15, 10),
        "the SVG hole must not protect the destination"
    );
    assert!(!dithers(1, 10), "visible vector coverage remains protected");
    remove_asset(id);
}

#[test]
fn gradient_text_and_border_brushes_remain_in_their_grayscale_roles() {
    let bounds = GeometryRect {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 40.0,
    };
    let color = RenderColor::linear([0xff0000ff, 0x0000ffff], 0.0, bounds);
    let scene = RenderScene {
        nodes: vec![
            RenderNode::Primitive(DrawPrimitive::Rect(
                0.0,
                0.0,
                64.0,
                40.0,
                RenderColor::linear([0x222222ff, 0xeeeeeeff], 0.0, bounds),
            )),
            RenderNode::Primitive(DrawPrimitive::Border(
                0.0,
                0.0,
                64.0,
                40.0,
                0.0,
                4.0,
                color.clone(),
                BorderStyle::Solid,
            )),
            RenderNode::Primitive(DrawPrimitive::TextWithFont(
                10.0,
                30.0,
                "O".into(),
                26.0,
                color,
                "sans-serif".into(),
                400,
                false,
            )),
        ],
    };
    let renderer = SceneRenderer::new();
    let mask = renderer
        .render_grayscale_dither_policy(64, 40, &RenderState::new(scene, Color::WHITE, 1, false))
        .unwrap();
    let protected = |x: usize, y: usize| {
        let i = y * 64 + x;
        mask[i / 8] & (1 << (7 - i % 8)) == 0
    };
    assert!(protected(1, 20));
    assert!(!protected(50, 20));
    assert!(
        (8..35)
            .flat_map(|y| (8..35).map(move |x| (x, y)))
            .any(|(x, y)| protected(x, y))
    );
}
