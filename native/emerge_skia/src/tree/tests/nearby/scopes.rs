use super::*;

fn guard(on: bool, radius: f64) -> Bytes {
    block(&[
        px(1, 40.),
        px(2, 40.),
        [13, 0].into_iter().chain(radius.to_be_bytes()).collect(),
        vec![77, u8::from(on)],
    ])
}
#[test]
fn an_offscreen_anchor_exports_past_ordinary_clips_but_images_and_outer_shadows_obey_its_guard() {
    let mut r = Raster::default();
    let paper = block(&[
        px(1, 160.),
        px(2, 120.),
        color([0, 0, 255]),
        padding(30., 30.),
        vec![40, 1],
    ]);
    let shadow = [52, 1]
        .into_iter()
        .chain([0f64, 30., 0., 0.].into_iter().flat_map(f64::to_be_bytes))
        .chain([0, 0, 255, 0, 0])
        .collect();
    let image = block(&[
        images::path(images::WIDE),
        px(1, 40.),
        px(2, 20.),
        number(31, -56.),
        shadow,
        vec![40, 1],
    ]);
    r.tree
        .upload(&upload(&[
            node(1, 4, paper, &[2]),
            node(2, 3, guard(false, 12.), &[3]),
            mounted(
                node(3, 4, block(&[px(1, 20.), px(2, 20.), number(31, 50.)]), &[]),
                &[(6, 4)],
            ),
            node(4, 9, image, &[]),
        ]))
        .unwrap();
    let mut l = images::loader();
    images::load(&mut r.tree, &mut l);
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let green = [0, 255, 0, 255];
    for (on, radius) in [(false, 12.), (true, 0.), (true, 12.), (false, 12.)] {
        r.tree.patch(&set(2, 3, guard(on, radius))).unwrap();
        draw(&mut r);
        for (x, y, expected, id) in [
            (25, 36, if on { blue } else { red }, if on { 1 } else { 4 }),
            (
                31,
                31,
                if on && radius != 0. { blue } else { red },
                if on && radius != 0. { 1 } else { 4 },
            ),
            (46, 36, red, 4),
            (46, 64, green, 1),
            (46, 74, if on { blue } else { green }, 1),
        ] {
            assert_eq!(pixel(&r, x, y), expected, "guard {on}/{radius} at {x},{y}");
            let (x, y) = (x as f32, y as f32);
            let origin = if id == 4 { (24., 30.) } else { (0., 0.) };
            assert_eq!(
                r.hit_at(x, y),
                Some((id, (x - origin.0).into(), (y - origin.1).into()))
            );
        }
        let mut cold = Raster::default();
        cold.tree.upload(&r.tree.encode()).unwrap();
        images::load(&mut cold.tree, &mut l);
        assert!(draw(&mut cold) == r.pixels());
    }
}

#[test]
fn behind_is_above_the_leaf_background_but_below_actual_glyphs_and_image_samples() {
    for kind in [5, 9] {
        let attrs = |rgb| {
            block(&[
                px(1, 60.),
                px(2, 36.),
                color(rgb),
                vec![40, 1],
                if kind == 5 {
                    vec![21, 0, 1, b'M']
                } else {
                    images::path(images::GRID)
                },
            ])
        };
        let mut r = Raster::default();
        let mut reference = Raster::default();
        reference
            .tree
            .upload(&upload(&[node(1, kind, attrs([255, 0, 0]), &[])]))
            .unwrap();
        r.tree
            .upload(&upload(&[
                mounted(node(1, kind, attrs([0, 255, 0]), &[]), &[(1, 2)]),
                node(2, 4, chip(60., 36., [255, 0, 0], false), &[]),
            ]))
            .unwrap();
        if kind == 9 {
            let mut l = images::loader();
            images::load(&mut r.tree, &mut l);
            images::load(&mut reference.tree, &mut l);
        }
        let expected = draw(&mut reference);
        assert!(draw(&mut r) == expected);
        assert_eq!(r.hit_at(5., 5.), Some((1, 5., 5.)));
        r.tree
            .patch(&patch(1, 1, &[mounted(edges(1, kind, &[]), &[(6, 2)])]))
            .unwrap();
        assert!(draw(&mut r) != expected);
        assert!(r.hit_at(5., 5.).is_none());
        assert!((0..36).all(|y| (0..60).all(|x| pixel(&r, x, y) == [255, 0, 0, 255])));
    }
}
