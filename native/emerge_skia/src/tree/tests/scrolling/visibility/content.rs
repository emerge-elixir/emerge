use super::*;

#[test]
fn visible_rows_retain_glyphs_and_bound_bitmap_fits_on_return() {
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
    let bytes = upload(&nodes);
    let mut live = Tree::for_session(0);
    let mut writer = Publisher::new(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut live);
    let mut r = Raster::with_tree(live);
    let mut held = None;
    let mut oracle = reference(&bytes);
    let mut loader = images::image_loader(true, 2);
    let mut full_loader = images::image_loader(true, 2);
    for _ in &rows {
        images::load(&mut r.tree, &mut loader);
        images::load(&mut oracle.tree, &mut full_loader);
    }
    for (i, pos) in [0., 1., 0.5, 0.].into_iter().enumerate() {
        if i != 0 {
            for t in [&mut r.tree, &mut oracle.tree] {
                assert!(set_input(t, 1, [0., pos], i as u64, None));
            }
        }
        r.settle(100., 80., 1.).unwrap();
        let returning = i == 3;
        assert_eq!(r.tree.output.fits.additions, if returning { 0 } else { 4 });
        if returning {
            assert_eq!(r.tree.output.packed_children, [0; 2]);
            assert_eq!(r.tree.output.recorded(), [0; 2]);
            assert_eq!(r.tree.output.assembly().1, [0; 3]);
        }
        oracle.settle(100., 80., 1.).unwrap();
        if r.tree.waiting_for_pixels() {
            images::load(&mut r.tree, &mut loader);
        }
        if oracle.tree.waiting_for_pixels() {
            images::load(&mut oracle.tree, &mut full_loader);
        }
        assert!(!r.tree.waiting_for_pixels());
        r.draw(100., 80., 1.).unwrap();
        oracle.draw(100., 80., 1.).unwrap();
        assert!(r.pixels() == oracle.pixels());
        let (commands, glyphs, _, hits) = r.pack_counts();
        assert_eq!((commands, glyphs, hits), (9, 8, 6));
        if i == 0 {
            held = Some((r.tree.seal().unwrap(), r.pixels().to_vec()));
        }
        let first = (96. * pos) as usize;
        assert_eq!(r.hit_at(65., 5.), Some((rows[first], 65., 5.)));
    }
    // Supersede an outstanding fit with a scroll, resize and source edit. A
    // failure for the stale job cannot poison the retained installed sample.
    let replace = |id, w, h, name| set(id, 9, block(&[px(1, w), px(2, h), images::path(name)]));
    let edits = [
        replace(rows[0] + 2, 5., 3., images::GRID),
        replace(rows[99] + 2, 20., 10., images::GRID),
    ];
    for edit in &edits {
        writer.apply(edit, 1.).unwrap().install(&mut r.tree);
        oracle.tree.patch(edit).unwrap();
        images::load(&mut r.tree, &mut loader);
        images::load(&mut oracle.tree, &mut full_loader);
    }
    for (r, l) in [(&mut r, &mut loader), (&mut oracle, &mut full_loader)] {
        r.settle(100., 80., 1.).unwrap();
        let (stale, request) = r.tree.next_image().unwrap();
        assert!(r.tree.waiting_for_pixels());
        set_input(&mut r.tree, 1, [0., 1.], 5, None);
        r.settle(112., 88., 1.25).unwrap();
        assert!(
            !r.tree
                .complete_image(&stale, crate::raster_assets::Source::Failed)
        );
        assert!(!l.fulfill(request, |s| {
            assert!(matches!(s, crate::raster_assets::Source::Ready(_)));
            r.tree.complete_image(&stale, s)
        }));
        while r.tree.waiting_for_pixels() {
            images::load(&mut r.tree, l);
        }
        r.draw(112., 88., 1.25).unwrap();
    }
    assert!(r.pixels() == oracle.pixels());
    for (r, l) in [(&mut r, &mut loader), (&mut oracle, &mut full_loader)] {
        set_input(&mut r.tree, 1, [0., 0.], 6, None);
        r.settle(100., 80., 1.).unwrap();
        while r.tree.waiting_for_pixels() {
            images::load(&mut r.tree, l);
        }
        r.draw(100., 80., 1.).unwrap();
    }
    let (old, before) = held.unwrap();
    assert!(r.pixels() == oracle.pixels() && r.pixels() != before);
    assert_eq!(old.rasterize().unwrap(), before);
    // A current failure is different: paint the failed occurrence, never
    // mutate the bound pixels in a reader or leave the actor waiting for it.
    let replace = replace(rows[0] + 2, 38., 19., "test_assets/alpha_inverse.png");
    writer.apply(&replace, 1.).unwrap().install(&mut r.tree);
    oracle.tree.patch(&replace).unwrap();
    images::load(&mut r.tree, &mut loader);
    images::load(&mut oracle.tree, &mut full_loader);
    for rr in [&mut r, &mut oracle] {
        rr.settle(100., 80., 1.).unwrap();
        let (job, _) = rr.tree.next_image().unwrap();
        assert!(
            rr.tree
                .complete_image(&job, crate::raster_assets::Source::Failed)
        );
        rr.draw(100., 80., 1.).unwrap();
        assert!(!rr.tree.waiting_for_pixels());
    }
    assert!(r.pixels() == oracle.pixels());
    assert_eq!(old.rasterize().unwrap(), before);
}

#[test]
fn fitted_samples_share_a_worker_target_across_shell_and_rotated_local_calls() {
    let ids = (10..18).collect::<Vec<_>>();
    let img = |w, h| block(&[px(1, w), px(2, h), images::path(images::WIDE), vec![40, 1]]);
    let bytes = upload(
        &[
            node(1, 1, block(&[px(1, 120.), px(2, 65.)]), &[2, 3]),
            node(
                2,
                3,
                block(&[
                    px(1, 64.),
                    px(2, 45.),
                    number(33, 12.),
                    number(34, 1.25),
                    vec![7, 1],
                ]),
                &ids,
            ),
            node(3, 9, img(48., 24.), &[]),
        ]
        .into_iter()
        .chain(ids.iter().map(|&id| node(id, 9, img(64., 20.), &[])))
        .collect::<Vec<_>>(),
    );
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut slow = reference(&bytes);
    for rr in [&mut fast, &mut slow] {
        let mut loader = images::image_loader(true, 1);
        for _ in 0..=ids.len() {
            images::load(&mut rr.tree, &mut loader);
        }
        rr.settle(120., 65., 1.).unwrap();
        assert!(rr.tree.waiting_for_pixels());
        images::load(&mut rr.tree, &mut loader);
        assert!(!rr.tree.waiting_for_pixels());
        rr.draw(120., 65., 1.).unwrap();
    }
    for (i, px) in [0., 0.25, 17.25, 80., 0.].into_iter().enumerate() {
        for t in [&mut fast.tree, &mut slow.tree] {
            if i != 0 {
                super::super::recordings::move_to(t, 2, px, i as u64);
            }
        }
        twins(&mut fast, &mut slow, 120, 65);
    }
}

#[test]
fn a_bitmap_may_bleed_from_a_partly_visible_host_but_not_an_empty_intersection() {
    let picture = node(
        11,
        9,
        block(&[
            px(1, 48.),
            px(2, 32.),
            images::path("sample_assets/static.jpg"),
        ]),
        &[],
    );
    for banded in [false, true] {
        let host = node(
            10,
            4,
            block(&[
                px(1, 64.),
                px(2, 20.),
                [13, 0].into_iter().chain(4f64.to_be_bytes()).collect(),
            ]),
            &[11],
        );
        let gap = node(12, 4, block(&[px(1, 64.), px(2, 80.), vec![40, 1]]), &[]);
        let bytes = upload(
            &[
                node(
                    1,
                    3,
                    block(&[px(1, 80.), px(2, 40.), vec![7, 1]]),
                    &[10, 12],
                ),
                host,
                picture.clone(),
            ]
            .into_iter()
            .chain(if banded {
                vec![mounted(gap, &[(6, 13)]), node(13, 6, block(&[]), &[])]
            } else {
                vec![gap]
            })
            .collect::<Vec<_>>(),
        );
        let mut fast = Raster::default();
        fast.tree.upload(&bytes).unwrap();
        let mut all = reference(&bytes);
        for r in [&mut fast, &mut all] {
            images::load(&mut r.tree, &mut images::loader());
        }
        for (i, y) in [0., 19., 19.75, 20., 20.25, 0.].into_iter().enumerate() {
            for r in [&mut fast, &mut all] {
                if i != 0 {
                    super::super::recordings::move_to(&mut r.tree, 1, y, i as u64);
                }
                r.draw(80., 40., 1.).unwrap();
            }
            assert_eq!(
                fast.pixels()
                    .iter()
                    .zip(all.pixels())
                    .position(|(a, b)| a != b),
                None,
                "host edge at {y}, banded={banded}"
            );
            if y == 19. || y == 20. {
                assert_eq!(all.pixels()[4 * 12 + 3] != 0, y == 19.);
            }
        }
    }
}

pub(in crate::tree::tests) fn shared_photo() -> Bytes {
    let photo = |id, w, h| {
        node(
            id,
            9,
            block(&[px(1, w), px(2, h), images::path("sample_assets/static.jpg")]),
            &[],
        )
    };
    upload(&[
        node(1, 1, box_attrs(240., 50.), &[2, 3]),
        node(
            2,
            3,
            block(&[px(1, 180.), px(2, 50.), vec![7, 1]]),
            &[7, 4, 5, 6],
        ),
        photo(3, 48., 32.),
        photo(7, 30., 20.),
        photo(4, 48., 32.),
        node(5, 4, box_attrs(10., 22.), &[]),
        photo(6, 160., 105.),
    ])
}
#[test]
fn a_larger_exposed_fit_updates_its_other_sampled_uses_but_not_a_pinned_frame() {
    for fail_growth in [false, true] {
        shared_growth(fail_growth);
    }
}
fn shared_growth(fail_growth: bool) {
    use crate::raster_assets::{Request, Source};
    let bytes = shared_photo();
    let mut t = Tree::for_session(0);
    Publisher::new(0).apply(&bytes, 1.).unwrap().install(&mut t);
    let mut f = Raster::with_tree(t);
    let mut all = reference(&bytes);
    let mut fast = images::image_loader(true, usize::from(fail_growth));
    let mut slow = images::image_loader(true, usize::from(fail_growth));
    for (r, l) in [(&mut f, &mut fast), (&mut all, &mut slow)] {
        for _ in 0..4 {
            images::load(&mut r.tree, l);
        }
    }
    let mut pinned = None;
    for (i, pos) in [0., 30., 31., 90., 0.].into_iter().enumerate() {
        for (r, l) in [(&mut f, &mut fast), (&mut all, &mut slow)] {
            if i > 0 {
                super::super::recordings::move_to(&mut r.tree, 2, pos, i as u64);
            }
            r.settle(240., 50., 1.).unwrap();
            if i == 2 && !r.tree.layers.scroll.exhaustive() {
                assert_eq!(r.tree.output.fits.additions, 0);
                assert_eq!(r.tree.output.recorded(), [0; 2]);
                assert_eq!(r.tree.output.assembly(), (0, [0; 3]));
            }
            if i < 2 {
                let (job, req) = r.tree.next_image().unwrap();
                let target = if i == 0 { (48, 32) } else { (160, 105) };
                assert!(matches!(&req, Request::Fit(_, size) if *size == target.into()));
                if i == 1 && fail_growth {
                    assert!(r.tree.complete_image(&job, Source::Failed));
                    for id in [3, 4, 6, 7] {
                        let k = r.tree.elements.ids[&id];
                        assert_eq!(
                            matches!(r.tree.elements.images.source(k), Source::Failed),
                            id != 7,
                            "hidden thumbnail keeps its source"
                        );
                    }
                } else {
                    assert!(l.fulfill(req, |s| r.tree.complete_image(&job, s)));
                }
            }
            assert!(r.tree.next_image().is_none());
            r.draw(240., 50., 1.).unwrap();
        }
        assert_eq!(
            f.pixels()
                .iter()
                .zip(all.pixels())
                .position(|(x, y)| x != y),
            None,
            "shared photo at offset {pos}"
        );
        if i == 0 {
            pinned = Some((f.tree.seal().unwrap(), f.pixels().to_vec()));
        }
    }
    let (s, initial) = pinned.unwrap();
    assert_ne!(f.pixels(), initial);
    assert_eq!(s.rasterize().unwrap(), initial);
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
