use super::*;

#[test]
fn visible_rows_alone_copy_glyphs_and_submit_bitmap_fits() {
    let rows = (0..100).map(|i| 10 + 3 * i).collect::<Vec<_>>();
    let port = block(&[px(1, 100.), px(2, 80.), vec![7, 1]]);
    let nodes = std::iter::once(node(1, 3, port, &rows))
        .chain(rows.iter().enumerate().flat_map(|(i, &id)| {
            [
                node(
                    id,
                    1,
                    block(&[
                        px(1, 100.),
                        px(2, if i % 2 == 0 { 18. } else { 22. }),
                        [3, 0].into_iter().chain(4f64.to_be_bytes()).collect(),
                        vec![40, 1],
                    ]),
                    &[id + 1, id + 2],
                ),
                node(
                    id + 1,
                    5,
                    block(&[
                        px(1, 28.),
                        px(2, 10.),
                        number(16, 10.),
                        text("AB")[2..].to_vec(),
                    ]),
                    &[],
                ),
                node(
                    id + 2,
                    9,
                    block(&[px(1, 20.), px(2, 10.), images::path(images::WIDE)]),
                    &[],
                ),
            ]
        }))
        .collect::<Vec<_>>();
    let mut r = Raster::default();
    r.tree.upload(&upload(&nodes)).unwrap();
    let mut loader = images::image_loader(true, 2);
    for _ in &rows {
        images::load(&mut r.tree, &mut loader);
    }
    for (i, pos) in [0., 1., 0.5, 0.].into_iter().enumerate() {
        if i != 0 {
            assert!(set_input(&mut r.tree, 1, [0., pos], i as u64, None));
        }
        r.settle(100., 80., 1.).unwrap();
        assert_eq!(r.tree.output.packed_children, [12; 2]);
        if r.tree.waiting_for_pixels() {
            images::load(&mut r.tree, &mut loader);
        }
        assert!(!r.tree.waiting_for_pixels());
        r.draw(100., 80., 1.).unwrap();
        assert_eq!(r.pack_counts(), (9, 8, 4, 6));
        let first = (96. * pos) as usize;
        assert_eq!(r.hit_at(65., 5.), Some((rows[first], 65., 5.)));
    }
}

#[test]
fn paragraph_ports_select_settled_word_owners_including_floats_and_empty_exporters() {
    let attrs = |w, scroll, pos| {
        block(&[
            px(1, w),
            px(2, 50.),
            number(16, 13.),
            vec![7, scroll],
            fraction(86, pos),
            vec![40, 1],
        ])
    };
    let ids = (10..90).map(|i| 3 * i).collect::<Vec<_>>();
    let float = mounted(
        node(
            2,
            4,
            block(&[px(1, 30.), px(2, 18.), vec![5, 0], color([255, 0, 0])]),
            &[],
        ),
        &[(6, 3)],
    );
    let chip = |id| {
        node(
            id,
            4,
            block(&[px(1, 12.), px(2, 9.), vec![40, 1], color([0, 255, 0])]),
            &[],
        )
    };
    let bytes = upload(
        &[
            node(1, 7, attrs(100., 1, 0.), &[&[2][..], &ids[..]].concat()),
            float,
            chip(3),
        ]
        .into_iter()
        .chain(ids.iter().enumerate().flat_map(|(i, &id)| {
            let owner = node(id, 4, prose::wrapper(12), &[id + 1]);
            if i % 7 == 0 {
                vec![
                    mounted(owner, &[(2, id + 2)]),
                    node(
                        id + 1,
                        5,
                        text(if i == 0 { "" } else { "first second" }),
                        &[],
                    ),
                    chip(id + 2),
                ]
            } else {
                vec![owner, node(id + 1, 5, text("AAA B CC"), &[])]
            }
        }))
        .collect::<Vec<_>>(),
    );
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut slow = reference(&bytes);
    for (w, en, pos, scale) in [
        (100., 1, 0., 1.),
        (100., 1, 1., 1.),
        (100., 1, 0.413, 1.),
        (64., 1, 0.413, 1.5),
        (100., 0, 0., 1.),
        (100., 1, 0., 1.),
    ] {
        let edit = set(1, 7, attrs(w, en, pos));
        fast.tree.patch(&edit).unwrap();
        slow.tree.patch(&edit).unwrap();
        let mut cold = reference(&fast.tree.encode());
        for r in [&mut fast, &mut slow, &mut cold] {
            r.draw(150., 100., scale).unwrap();
        }
        assert!(fast.pixels() == slow.pixels() && fast.pixels() == cold.pixels());
        for y in (0..100).step_by(4) {
            for x in (0..150).step_by(3) {
                let hit = |r: &Raster| r.hit_at(x as f32 + 0.5, y as f32 + 0.5);
                assert_eq!(hit(&fast), hit(&slow));
                assert_eq!(hit(&fast), hit(&cold));
            }
        }
        if en == 1 {
            assert!(fast.pack_counts().1 < slow.pack_counts().1 / 2);
            assert!(fast.tree.output.packed_children[0] < ids.len());
        }
    }
}

#[test]
fn a_transparent_listener_refits_only_hits_and_keyed_jumps_use_unfiltered_geometry() {
    let port = |pos| block(&[px(1, 100.), px(2, 60.), vec![7, 1], vec![40, 1], pos]);
    let row = |click| block(&[px(1, 100.), px(2, 20.), vec![40, click]]);
    let ids = (10..110).collect::<Vec<_>>();
    let bytes = upload(
        &std::iter::once(node(1, 3, port(fraction(86, 0.)), &ids))
            .chain(ids.iter().map(|&id| node(id, 4, row(0), &[])))
            .collect::<Vec<_>>(),
    );
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut slow = reference(&bytes);
    twins(&mut fast, &mut slow, 100, 60);
    for edit in [
        set(57, 4, row(1)),
        set(1, 3, port(target(86, 123, 57))),
        set(57, 4, row(0)),
    ] {
        fast.tree.layers.scroll.counts = Default::default();
        let draw = fast.draws;
        for r in [&mut fast, &mut slow] {
            r.tree.patch(&edit).unwrap();
        }
        twins(&mut fast, &mut slow, 100, 60);
        if edit == set(1, 3, port(target(86, 123, 57))) {
            assert_eq!(fast.hit_at(5., 5.), Some((57, 5., 5.)));
        } else {
            assert_eq!(fast.draws, draw);
            assert_eq!(fast.tree.output.packed_children[0], 0);
            let work = fast.tree.layers.scroll.counts;
            assert_eq!((work.builds, work.refits), (0, 1));
            assert_eq!(fast.hit_at(5., 5.), Some((1, 5., 5.)));
        }
    }
}

#[test]
fn subpixel_and_distant_positive_boxes_are_not_empty_in_the_projected_plane() {
    for (spacer, height, offset) in [(20.1, 0.26, 19.7), (268435456., 4., 268435456.)] {
        let port = |y| block(&[px(1, 100.), px(2, 20.), vec![7, 1], number(39, y)]);
        let ink = |rgb| block(&[px(1, 100.), px(2, height), color(rgb), vec![40, 1]]);
        let bytes = upload(&[
            node(1, 3, port(0.), &[2, 3, 4]),
            node(2, 4, block(&[px(2, spacer)]), &[]),
            node(3, 4, ink([0, 255, 0]), &[]),
            node(4, 4, block(&[px(2, 256.)]), &[]),
        ]);
        let mut f = Raster::default();
        f.tree.upload(&bytes).unwrap();
        let mut all = reference(&bytes);
        twins(&mut f, &mut all, 100, 20);
        for edit in [set(1, 3, port(offset)), set(3, 4, ink([0, 0, 255]))] {
            f.tree.patch(&edit).unwrap();
            all.tree.patch(&edit).unwrap();
            twins(&mut f, &mut all, 100, 20);
            assert_eq!(f.hit_at(5., 0.5).unwrap().0, 3);
            assert_ne!(f.pixels()[20..24], [0; 4]);
        }
    }
}

#[test]
fn a_scaled_positive_box_can_collapse_only_in_the_unscrolled_plane() {
    let y = 1_048_576.;
    let port = |offset| block(&[px(1, 101.), px(2, 21.), vec![7, 1], number(39, offset)]);
    let bytes = upload(&[
        node(1, 3, port(0.), &[2, 3]),
        node(2, 4, box_attrs(101., y), &[]),
        node(
            3,
            4,
            block(&[
                px(1, 101.),
                px(2, 21.),
                number(34, 1. / 512.),
                color([0, 255, 0]),
                vec![40, 1],
            ]),
            &[],
        ),
    ]);
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut all = reference(&bytes);
    twins(&mut fast, &mut all, 101, 21);
    for offset in [y, 0., y] {
        let edit = set(1, 3, port(offset));
        fast.tree.patch(&edit).unwrap();
        all.tree.patch(&edit).unwrap();
        twins(&mut fast, &mut all, 101, 21);
        let expected = all.hit_at(50.5, 10.5);
        if offset != 0. {
            assert_eq!(expected, Some((3, 50.5, 10.5)));
        }
        assert_eq!(fast.hit_at(50.5, 10.5), expected);
    }
}

#[test]
fn disjoint_opaque_covers_never_occlude_the_listener_or_the_hole_between_them() {
    let cover = |id, shift, w| {
        node(
            id,
            4,
            block(&[px(1, w), px(2, 20.), number(31, shift), color([0, 0, 255])]),
            &[],
        )
    };
    let bytes = upload(&[
        node(
            1,
            1,
            block(&[px(1, 60.), px(2, 28.), vec![8, 1]]),
            &[2, 3, 4],
        ),
        node(
            2,
            4,
            block(&[px(1, 60.), px(2, 20.), vec![40, 1], color([255, 0, 0])]),
            &[],
        ),
        cover(3, -60., 20.),
        cover(4, -40., 20.),
    ]);
    let mut f = Raster::default();
    f.tree.upload(&bytes).unwrap();
    let mut all = reference(&bytes);
    for solid in [false, true, false] {
        let cut = set(
            3,
            4,
            block(&[
                px(1, if solid { 60. } else { 20. }),
                px(2, 20.),
                number(31, -60.),
                color([0, 0, 255]),
            ]),
        );
        for r in [&mut f, &mut all] {
            r.tree.patch(&cut).unwrap();
        }
        twins(&mut f, &mut all, 60, 28);
        let b = 4 * (5 * 60 + 30);
        assert_eq!(
            f.pixels()[b..b + 4],
            if solid {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            }
        );
        assert_eq!(f.hit_at(30., 5.), Some((2, 30., 5.)));
        assert_eq!(all.pack_counts().0 - f.pack_counts().0, usize::from(solid));
    }
}
