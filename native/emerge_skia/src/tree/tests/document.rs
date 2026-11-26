use super::*;

pub(super) fn margin(side: u8, w: f64, h: f64) -> Bytes {
    block(&[
        vec![5, side],
        px(1, w),
        px(2, h),
        color([240, 40, 0]),
        vec![40, 1],
    ])
}
fn paper() -> Bytes {
    block(&[vec![1, 0], px(2, 24.), number(4, 2.)])
}
pub(super) fn story() -> Bytes {
    upload(&[
        node(
            1,
            8,
            block(&[px(1, 98.), number(4, 2.), number(16, 12.)]),
            &[2, 3, 5, 8],
        ),
        node(2, 4, margin(0, 30., 70.), &[]),
        node(3, 7, paper(), &[4]),
        node(4, 5, text("A"), &[]),
        node(5, 7, paper(), &[6]),
        node(6, 4, prose::wrapper(90), &[7]),
        node(7, 5, text("AA BB"), &[]),
        node(
            8,
            4,
            block(&[
                vec![5, 1],
                px(1, 20.),
                px(2, 10.),
                color([0, 0, 255]),
                vec![40, 1],
            ]),
            &[],
        ),
    ])
}

#[test]
fn both_paragraphs_read_the_margin_but_neither_is_charged_its_height() {
    let mut r = Raster::default();
    r.tree.upload(&story()).unwrap();
    let a = draw(&mut r).to_vec();
    let positions = |r: &Raster| {
        [4, 6, 8].map(|id| {
            let b = bounds(r, id);
            [b.x, b.y]
        })
    };
    assert_eq!(positions(&r), [[32., 2.], [32., 28.], [39., 70.]]);
    assert_eq!(r.hit_at(35., 31.), Some((6, 3., 3.)));
    let before = r.tree.layers.invalidation_passes;
    r.tree.patch(&set(6, 4, prose::wrapper(220))).unwrap();
    assert_ne!(draw(&mut r), a);
    assert_eq!(before + 1, r.tree.layers.invalidation_passes);
    assert!([1, 3, 5].iter().all(|id| !ran(&r, Reflow, *id)));
    let stable = r.pixels().to_vec();
    let before = r.tree.layers.invalidation_passes;
    r.tree.patch(&set(2, 4, margin(2, 30., 70.))).unwrap();
    r.tree.patch(&set(2, 4, margin(0, 30., 70.))).unwrap();
    assert_eq!(draw(&mut r), stable);
    assert!(r.tree.layers.trace.is_empty());
    assert_eq!(before + 1, r.tree.layers.invalidation_passes);
    for (side, height, left, clear_y) in [(2, 70., 0., 70.), (0, 18., 0., 54.), (0, 70., 32., 70.)]
    {
        r.tree.patch(&set(2, 4, margin(side, 30., height))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 6).x, left);
        assert_eq!(bounds(&r, 8).y, clear_y);
        check_fresh(&r);
    }
}
#[test]
fn failed_submission_retains_the_installed_document_even_if_another_float_edit_is_accepted() {
    let mut r = Raster::default();
    r.tree.upload(&story()).unwrap();
    let a = draw(&mut r).to_vec();
    let hit = |r: &Raster| r.hit_at(3., 31.);
    assert_eq!(hit(&r), Some((2, 3., 31.)));
    r.tree.patch(&set(2, 4, margin(2, 30., 70.))).unwrap();
    let b = r.tree.encode();
    // The kept span's CR/LF is legal only in paragraph flow, never as a block.
    r.tree.patch(&set(7, 5, text("A\nB"))).unwrap();
    let accepted = r.tree.encode();
    assert!(
        r.tree
            .patch(&patch(
                1,
                1,
                &[edges(1, 8, &[2, 3, 5, 6, 8]), edges(5, 7, &[])]
            ))
            .is_err()
    );
    assert_eq!(r.tree.encode(), accepted);
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused".into()))
            .is_err()
    );
    assert_eq!(r.pixels(), a);
    assert_eq!(hit(&r), Some((2, 3., 31.)));
    r.tree.patch(&at(b, 1)).unwrap();
    r.settle(180., 170., 1.).unwrap();
    let sealed = r.replay_drawing();
    assert!(sealed != a);
    r.tree.patch(&set(2, 4, margin(0, 30., 70.))).unwrap();
    assert!(r.replay_drawing() == sealed);
    r.install();
    assert_eq!(hit(&r), Some((6, 3., 3.)));
    assert!(r.replay_drawing() == sealed);
    assert_eq!(draw(&mut r), a);
    assert_eq!(hit(&r), Some((2, 3., 31.)));
    check_fresh(&r);
}
#[test]
fn closed_paragraph_enters_and_leaves_document_flow_at_equal_width() {
    let mut r = Raster::default();
    r.tree.upload(&story()).unwrap();
    let a = draw(&mut r).to_vec();
    // The standalone Column and TextColumn have the same inner width. The kept
    // paragraph and its words are never resent; only its incoming edge changes.
    let moved = patch(
        1,
        9,
        &[
            node(9, 3, block(&[px(1, 98.)]), &[1, 5]),
            edges(1, 8, &[2, 3, 8]),
        ],
    );
    let back = patch(9, 1, &[edges(1, 8, &[2, 3, 5, 8])]);
    for edit in [moved, back] {
        r.tree.patch(&edit).unwrap();
        draw(&mut r);
        check_fresh(&r);
    }
    assert_eq!(r.pixels(), a);
    for align in [Some(2), None, Some(1), Some(0), None] {
        let attrs = block(
            &[
                Some(vec![1, 0]),
                Some(px(2, 24.)),
                Some(number(4, 2.)),
                align.map(|side| vec![5, side]),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        );
        r.tree.patch(&set(5, 7, attrs)).unwrap();
        draw(&mut r);
        check_fresh(&r);
    }
    assert_eq!(r.pixels(), a);
}
#[test]
fn document_and_paragraph_floats_search_with_measurement_before_excluding_the_final_box() {
    let attrs = |font| {
        block(&[
            vec![5, 0],
            px(1, 50.),
            content_bounds::bound(2, 4, 50.),
            number(16, font),
        ])
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(
                1,
                8,
                block(&[px(1, 100.), px(2, 145.), number(4, 2.)]),
                &[2, 3, 4],
            ),
            node(2, 4, margin(2, 10., 26.), &[]),
            node(3, 4, margin(2, 60., 30.), &[]),
            node(4, 7, attrs(12.), &[5]),
            node(
                5,
                5,
                text("many small words that need to wrap across more than four lines"),
                &[],
            ),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 4).y, 4.);
    assert!(bounds(&r, 4).height < 26.);
    let nested = patch(
        1,
        1,
        &[
            edges(1, 8, &[2, 3, 6]),
            node(6, 7, block(&[vec![1, 0], number(4, 2.)]), &[4]),
        ],
    );
    for insertion in [None, Some(nested)] {
        if let Some(edit) = insertion {
            r.tree.patch(&edit).unwrap();
            draw(&mut r);
        }
        for (font, y) in [(28., 56.), (12., 4.)] {
            r.tree.patch(&set(4, 7, attrs(font))).unwrap();
            draw(&mut r);
            assert_eq!(bounds(&r, 4).y, y);
            let line = if font == 28. { 33.88 } else { 14.52 };
            assert!((bounds(&r, 4).height - line).abs() < 0.01);
            check_fresh(&r);
        }
    }
    // Identical intrinsic offers. Fill and min(fill,content) stay one
    // measured line tall; content and max(fill,content) grow after wrapping.
    // Even the tall outer box starts beside the narrow first float at y=4.
    for (height, grows) in [
        (vec![2, 0], false),
        (vec![2, 1], true),
        (vec![2, 4, 0, 1], false),
        (vec![2, 5, 0, 1], true),
        (vec![2, 4, 0, 1], false),
    ] {
        r.tree
            .patch(&set(
                4,
                7,
                block(&[vec![5, 0], px(1, 50.), height, number(16, 12.)]),
            ))
            .unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 4).y, 4.);
        let expected = if grows { 159.71 } else { 14.52 };
        assert!((bounds(&r, 4).height - expected).abs() < 0.01);
        check_fresh(&r);
    }
}

#[test]
fn insets_alone_do_not_expand_an_empty_explicit_paragraph_height() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 8, block(&[px(1, 120.), number(4, 2.)]), &[2, 4]),
            node(
                2,
                7,
                block(&[
                    px(2, 1.),
                    [vec![3, 0], 8f64.to_be_bytes().to_vec()].concat(),
                ]),
                &[3],
            ),
            node(3, 5, text(""), &[]),
            node(
                4,
                4,
                block(&[px(1, 20.), px(2, 8.), color([0, 0, 255])]),
                &[],
            ),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 4).y, 3.);
    for s in ["visible", ""] {
        r.tree.patch(&set(3, 5, text(s))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 4).y == 3., s.is_empty());
        check_fresh(&r);
    }
}

#[test]
fn an_ordinary_block_can_center_a_late_packed_width_but_a_float_needs_it_in_flow() {
    let attrs = |align| block(&[vec![5, align], vec![1, 1], px(2, 25.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 8, block(&[px(1, 100.)]), &[2]),
            node(2, 14, attrs(1), &[3, 5]),
            node(3, 4, block(&[px(1, 18.)]), &[4]),
            node(4, 6, block(&[px(2, 20.)]), &[]),
            node(5, 6, block(&[px(1, 16.), px(2, 16.)]), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 2).x, bounds(&r, 2).width), (33., 34.));
    r.tree.patch(&set(4, 6, block(&[px(2, 8.)]))).unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 2).x, bounds(&r, 2).width), (41., 18.));
    for edit in [set(2, 14, attrs(0)), set(1, 8, block(&[vec![1, 1]]))] {
        r.tree.patch(&edit).unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 2).x, bounds(&r, 2).width), (0., 18.));
        check_fresh(&r);
    }
}

#[test]
fn a_turned_document_block_uses_unbounded_height_even_in_a_short_document() {
    let mut r = Raster::default();
    let card = |height| {
        block(&[
            px(1, 24.),
            height,
            number(79, 90.),
            color([255, 0, 0]),
            vec![40, 1],
        ])
    };
    let cap = |h| bounded::bound(2, 4, h, None, false);
    r.tree
        .upload(&upload(&[
            node(
                1,
                8,
                block(&[px(1, 160.), px(2, 80.), vec![40, 1]]),
                &[2, 3],
            ),
            node(2, 4, card(px(2, 24.)), &[]),
            node(
                3,
                4,
                block(&[px(1, 8.), px(2, 8.), color([0, 0, 255])]),
                &[],
            ),
        ]))
        .unwrap();
    for (width, height) in [
        (24., cap(24.)),
        (80., cap(80.)),
        (f32::MAX, vec![2, 0]),
        (24., px(2, 24.)),
    ] {
        let previous = r.hit_at(40., 6.);
        r.tree.patch(&set(2, 4, card(height))).unwrap();
        r.settle(180., 170., 1.).unwrap();
        assert_eq!(r.hit_at(40., 6.), previous);
        assert_eq!((bounds(&r, 2).width, bounds(&r, 3).y), (width, 24.));
        draw(&mut r);
        if width == 80. {
            assert_eq!(r.hit_at(40., 6.), Some((2, 6., 40.)));
        }
        if width == 24. {
            assert_eq!(r.hit_at(40., 6.), Some((1, 40., 6.)));
        }
        check_fresh(&r);
    }
}

#[test]
fn document_display_scale_is_a_layout_input_and_inverse_hit_transform() {
    let d = story();
    let mut r = Raster::default();
    r.tree.upload(&d).unwrap();
    for s in [1., 1.5, 2., 1.] {
        let mut fresh = Raster::default();
        fresh.tree.upload(&d).unwrap();
        assert!(r.draw(180., 170., s).unwrap() == fresh.draw(180., 170., s).unwrap());
        assert_eq!(r.hit_at(35. * s, 31. * s), Some((6, 3., 3.)));
        assert_eq!(bounds(&r, 8).y, 70. * s);
    }
}
