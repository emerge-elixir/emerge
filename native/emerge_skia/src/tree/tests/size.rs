use super::*;
const SHORT: &str = "AA BB";
const LONG: &str = "AA BB CC DD EE";
fn rect(w: f64, h: f64) -> Bytes {
    block(&[px(1, w), px(2, h)])
}
fn marker() -> Bytes {
    block(&[vec![1, 0], px(2, 8.), color([0, 0, 255]), vec![40, 1]])
}
fn pack(w: Bytes) -> Bytes {
    block(&[
        w,
        px(2, 60.),
        [
            vec![36],
            6f64.to_be_bytes().to_vec(),
            4f64.to_be_bytes().to_vec(),
        ]
        .concat(),
    ])
}
pub(super) fn packed() -> Bytes {
    upload(&[
        node(1, 1, rect(160., 120.), &[2, 6]),
        node(2, 14, pack(vec![1, 1]), &[3, 5]),
        node(3, 7, block(&[px(1, 48.)]), &[4]),
        node(4, 5, text(LONG), &[]),
        node(5, 4, rect(20., 20.), &[]),
        node(6, 4, marker(), &[]),
    ])
}
#[test]
fn packed_width_and_its_content_cap_change_at_the_final_paragraph_width() {
    let mut r = Raster::default();
    r.tree.upload(&packed()).unwrap();
    for (policy, s, x, lines) in [
        (vec![1, 1], LONG, 74., 3.),
        (vec![1, 1], SHORT, 48., 1.),
        (vec![1, 4, 1, 0], LONG, 74., 3.),
        (vec![1, 4, 1, 0], SHORT, 48., 1.),
        (vec![1, 1], LONG, 74., 3.),
    ] {
        let n = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(4, 5, text(s))).unwrap();
        r.tree.patch(&set(2, 14, pack(policy))).unwrap();
        draw(&mut r);
        let line = r.tree.layers.runs[r.tree.elements.ids[&4]].metrics.height;
        assert_eq!(r.tree.layers.paragraphs.solves - n, 1);
        assert_eq!(
            (bounds(&r, 3).width, bounds(&r, 3).height),
            (48., line * lines)
        );
        assert_eq!((bounds(&r, 2).width, bounds(&r, 6).x), (x, x));
        check_fresh(&r);
    }
}
#[test]
fn packed_width_projects_a_fill_in_its_own_group_after_membership_is_known() {
    // Direct turn, direct Image, or a filling El containing the Image.
    for (kind, nested) in [(4, false), (9, false), (4, true)] {
        let mut r = Raster::default();
        let floor = bounded::bound(2, 5, 0., None, false); // max(0,fill): authored zero minimum
        let attrs = if nested {
            block(&[vec![1, 1], floor])
        } else if kind == 4 {
            block(&[px(1, 24.), number(79, 90.), floor])
        } else {
            block(&[images::path(images::WIDE), floor])
        };
        let mut nodes = vec![
            node(1, 1, rect(180., 120.), &[2, 6]),
            node(2, 14, pack(vec![1, 1]), &[3, 4, 5]),
            node(3, 4, rect(8., 41.), &[]),
            node(4, kind, attrs, if nested { &[7] } else { &[] }),
            node(5, 4, rect(20., 22.), &[]),
            node(6, 4, marker(), &[]),
        ];
        if nested {
            nodes.push(node(
                7,
                9,
                block(&[images::path(images::WIDE), vec![2, 0]]),
                &[],
            ));
        }
        r.tree.upload(&upload(&nodes)).unwrap();
        if nested || kind == 9 {
            images::load(&mut r.tree, &mut images::loader());
        }
        for (header, share, cols) in [(41., 15., 2), (29., 1., 1), (41., 15., 2)] {
            r.tree.patch(&set(3, 4, rect(8., header))).unwrap();
            draw(&mut r);
            let art = if nested || kind == 9 {
                2. * share
            } else {
                share
            };
            if nested {
                assert_eq!(bounds(&r, 7).width, art);
            }
            let cross = if cols == 2 { art + 6. + 20. } else { 20. };
            assert_eq!(bounds(&r, 4).width, art);
            assert_eq!(
                (bounds(&r, 2).width, bounds(&r, 6).x, bounds(&r, 6).width),
                (cross, cross, 180. - cross)
            );
        }
    }
}

#[test]
fn a_fixed_width_pack_card_does_not_solve_its_inner_paragraph_before_the_height_share() {
    // The card's width is known, but the paragraph gets what is left of
    // the Row after projecting a height fill through the Image's aspect.
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 14, pack(vec![1, 1]), &[2, 3]),
            node(2, 4, rect(8., 16.), &[]),
            node(
                3,
                4,
                block(&[px(1, 200.), bounded::bound(2, 5, 0., None, false)]),
                &[4],
            ),
            node(4, 1, block(&[vec![1, 0], vec![2, 0]]), &[5, 6]),
            node(5, 9, block(&[images::path(images::WIDE), vec![2, 0]]), &[]),
            node(6, 7, block(&[vec![1, 0]]), &[7]),
            node(7, 5, text(LONG), &[]),
        ]))
        .unwrap();
    images::load(&mut r.tree, &mut images::loader());
    for head in [16., 8., 16.] {
        let before = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(2, 4, rect(8., head))).unwrap();
        draw(&mut r);
        let share = 60. - 4. - head as f32;
        assert_eq!(
            (bounds(&r, 5).width, bounds(&r, 6).width),
            (2. * share, 200. - 2. * share)
        );
        assert_eq!(r.tree.layers.paragraphs.solves, before + 1);
    }
}

#[test]
fn a_measured_column_share_crosses_filling_wrappers_and_tracks_weight_swaps() {
    for kind in [4, 1, 3] {
        let mut r = Raster::default();
        let wrap = |w| block(&[vec![1, 1], weighted::weight(2, w)]);
        r.tree
            .upload(&upload(&[
                node(1, 1, rect(1024., 300.), &[2, 8]),
                node(2, 3, block(&[vec![1, 1], px(2, 300.)]), &[3, 5, 7]),
                node(3, 7, block(&[px(1, 48.)]), &[4]),
                node(4, 5, text(SHORT), &[]),
                node(5, kind, wrap(2.), &[6]),
                node(6, 9, block(&[images::path(images::WIDE), vec![2, 0]]), &[]),
                node(7, 4, wrap(1.), &[]),
                node(8, 4, marker(), &[]),
            ]))
            .unwrap();
        images::load(&mut r.tree, &mut images::loader());
        for (a, b, s, lines) in [
            (2., 1., SHORT, 1.),
            (1., 2., LONG, 3.),
            (2., 1., LONG, 3.),
            (2., 1., SHORT, 1.),
        ] {
            r.tree.patch(&set(5, kind, wrap(a))).unwrap();
            r.tree.patch(&set(7, 4, wrap(b))).unwrap();
            r.tree.patch(&set(4, 5, text(s))).unwrap();
            r.tree.refresh(1024., 300., 1.).unwrap();
            let line = r.tree.layers.runs[r.tree.elements.ids[&4]].metrics.height;
            let share = (300. - line * lines) / 3. * a as f32;
            let w = share * 2.;
            let host = if kind == 4 { w } else { w.max(200.) };
            assert_eq!((bounds(&r, 6).width, bounds(&r, 6).height), (w, share));
            assert_eq!(
                (bounds(&r, 5).width, bounds(&r, 8).x),
                (host, host),
                "wrapper {kind}"
            );
        }
    }
}
#[test]
fn a_mixed_height_evaluates_its_own_site_offer_before_projecting_an_image() {
    for kind in [1, 3] {
        let mut r = Raster::default();
        let row = kind == 1;
        r.tree
            .upload(&upload(&[
                node(
                    1,
                    kind,
                    block(&[
                        if row { px(1, 600.) } else { vec![1, 1] },
                        vec![2, if row { 4 } else { 5 }, 1, 0],
                    ]),
                    &[2, 3, 4],
                ),
                node(2, 4, rect(8., 20.), &[]),
                node(3, 9, block(&[images::path(images::WIDE), vec![2, 0]]), &[]),
                node(4, 4, if row { marker() } else { rect(0., 0.) }, &[]),
            ]))
            .unwrap();
        images::load(&mut r.tree, &mut images::loader());
        for view in [60., 0., 260., 60.] {
            r.tree.refresh(600., view, 1.).unwrap();
            let inner = if row { view.min(100.) } else { view.max(120.) };
            let h = if row { inner } else { (inner - 20.).max(0.) };
            assert_eq!((bounds(&r, 1).height, bounds(&r, 3).width), (inner, 2. * h));
            if row {
                assert_eq!(bounds(&r, 4).width, 592. - 2. * h);
            } else {
                assert_eq!(bounds(&r, 1).width, 2. * h);
            }
        }
    }
}

#[test]
fn a_turned_cap_cannot_read_lines_whose_width_it_is_about_to_assign() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, rect(280., 80.), &[2, 4]),
            node(2, 7, block(&[px(1, 48.)]), &[3]),
            node(3, 5, text(SHORT), &[]),
            node(4, 1, block(&[px(1, 280.), vec![2, 0]]), &[5, 10]),
            node(
                5,
                4,
                block(&[px(1, 200.), number(79, 90.), vec![2, 4, 1, 0]]),
                &[6],
            ),
            node(6, 1, block(&[vec![1, 0], vec![2, 0]]), &[7, 8]),
            node(7, 9, block(&[images::path(images::WIDE), vec![2, 0]]), &[]),
            node(8, 7, block(&[vec![1, 0]]), &[9]),
            node(9, 5, text(&"AA ".repeat(20)), &[]),
            node(10, 4, marker(), &[]),
        ]))
        .unwrap();
    images::load(&mut r.tree, &mut images::loader());
    for (cap, s) in [
        (80., SHORT),
        (80., LONG),
        (180., SHORT),
        (180., LONG),
        (80., SHORT),
    ] {
        r.tree.patch(&set(1, 3, rect(280., cap))).unwrap();
        r.tree.patch(&set(3, 5, text(s))).unwrap();
        draw(&mut r);
        let h = (cap as f32 - bounds(&r, 2).height).min(100.); // unallotted 2:1 Image: 200 × 100
        assert_eq!(
            (
                bounds(&r, 10).x,
                bounds(&r, 5).width,
                bounds(&r, 7).width,
                bounds(&r, 8).width
            ),
            (h, h, h * 2., 200. - h * 2.)
        );
        if cap == 180. {
            assert!(bounds(&r, 8).height > h);
        } // overflowing prose cannot reopen the split
    }
}

#[test]
fn a_turned_content_cap_reads_the_settled_local_lines_before_the_row_charges_it() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, rect(160., 40.), &[2, 5]),
            node(
                2,
                4,
                block(&[number(79, 90.), px(1, 48.), vec![2, 4, 1, 0]]),
                &[3],
            ),
            node(3, 7, block(&[vec![1, 0]]), &[4]),
            node(4, 5, text(LONG), &[]),
            node(5, 4, marker(), &[]),
        ]))
        .unwrap();
    for (h, s) in [
        (40., LONG),
        (24., SHORT),
        (80., LONG),
        (0., LONG),
        (40., LONG),
    ] {
        r.tree.patch(&set(1, 1, rect(160., h))).unwrap();
        r.tree.patch(&set(4, 5, text(s))).unwrap();
        draw(&mut r);
        let lines = if s == SHORT { 1. } else { 3. };
        let cap =
            (lines * r.tree.layers.runs[r.tree.elements.ids[&4]].metrics.height).min(h as f32);
        assert_eq!(bounds(&r, 2).width, cap);
        assert_eq!((bounds(&r, 5).x, bounds(&r, 5).width), (cap, 160. - cap));
        check_fresh(&r);
    }
}

#[test]
fn source_arrival_replaces_a_width_proof_without_touching_a_clean_row() {
    // The card's unturned width comes from both the art and the unwrapped
    // text. The fill paragraph is constrained to that width by the card.
    let mut r = Raster::default();
    let nodes = || {
        upload(&[
            node(1, 3, rect(300., 160.), &[2, 9]),
            node(2, 1, rect(300., 80.), &[3, 8]),
            node(3, 3, block(&[vec![1, 1], number(79, 90.)]), &[4, 5]),
            node(4, 9, block(&[images::path(images::WIDE), px(2, 76.)]), &[]),
            node(5, 7, block(&[vec![1, 0]]), &[6]),
            node(6, 5, text(LONG), &[]),
            node(8, 4, marker(), &[]),
            node(9, 1, rect(300., 80.), &[10, 12]),
            node(10, 7, block(&[px(1, 48.), number(79, 90.)]), &[11]),
            node(11, 5, text(LONG), &[]),
            node(12, 4, marker(), &[]),
        ])
    };
    r.tree.upload(&nodes()).unwrap();
    draw(&mut r);
    assert_eq!(r.tree.layers.paragraphs.solves, 2);
    let mut l = images::loader();
    images::load(&mut r.tree, &mut l);
    draw(&mut r);
    assert_eq!(bounds(&r, 5).width, 152.);
    assert_eq!(bounds(&r, 8).x, 76. + bounds(&r, 5).height);
    assert_eq!(r.tree.layers.paragraphs.solves, 3);
    assert!(
        !r.tree
            .layers
            .trace
            .iter()
            .any(|(p, id, _)| *id >= 9 && matches!(p, Measure | Flow | Reflow))
    );
    let mut cold = Raster::default();
    cold.tree.upload(&nodes()).unwrap();
    images::load(&mut cold.tree, &mut l);
    assert_eq!(draw(&mut cold), r.pixels());
    for (id, frame) in cold.tree.frames() {
        assert_eq!(frame, bounds(&r, id));
    }
}

#[test]
fn the_intrinsic_width_floor_breaks_the_shared_wrapping_header_feedback() {
    let mut r = Raster::default();
    let col = |w| block(&[w, px(2, 100.)]);
    r.tree
        .upload(&upload(&[
            node(1, 1, rect(320., 165.), &[2, 9]),
            node(2, 3, col(vec![1, 1]), &[3, 8]),
            node(3, 2, block(&[vec![1, 0]]), &[4, 6]),
            node(4, 4, rect(75., 20.), &[5]),
            node(5, 6, block(&[]), &[]),
            node(6, 4, rect(75., 20.), &[7]),
            node(7, 6, block(&[]), &[]),
            node(8, 9, block(&[images::path(images::WIDE), vec![2, 0]]), &[]),
            node(9, 4, marker(), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 9).x, 150.); // unloaded, no aspect
    images::load(&mut r.tree, &mut images::loader());
    for (width, charge, h) in [
        (vec![1, 1], 200., 20.),
        (px(1, 120.), 120., 40.),
        (px(1, 160.), 160., 20.),
        (vec![1, 1], 200., 20.),
    ] {
        r.tree.patch(&set(2, 3, col(width))).unwrap();
        draw(&mut r);
        assert_eq!(
            (bounds(&r, 9).x, bounds(&r, 3).height, bounds(&r, 8).y),
            (charge, h, h)
        );
        assert_eq!(bounds(&r, 8).width, (100. - h) * 2.);
    }
}
#[test]
fn natural_cross_projection_reads_an_independent_siblings_settled_height() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, block(&[px(1, 180.)]), &[2, 4, 5]),
            node(2, 7, block(&[px(1, 48.)]), &[3]),
            node(3, 5, text(LONG), &[]),
            node(4, 4, block(&[px(1, 12.), number(79, 90.), vec![2, 0]]), &[]),
            node(5, 4, marker(), &[]),
        ]))
        .unwrap();
    for (s, lines) in [(LONG, 3.), (SHORT, 1.), (LONG, 3.)] {
        r.tree.patch(&set(3, 5, text(s))).unwrap();
        draw(&mut r);
        let h = r.tree.layers.runs[r.tree.elements.ids[&3]].metrics.height * lines;
        assert_eq!(
            (bounds(&r, 4).width, bounds(&r, 5).x, bounds(&r, 5).width),
            (h, 48. + h, 132. - h)
        );
        check_fresh(&r);
    }
}
#[test]
fn a_closed_pack_move_and_a_sibling_only_edit_check_the_final_column_group() {
    let mut r = Raster::default();
    let width = |w| block(&[w, px(2, 60.)]);
    let fill_w = || block(&[vec![1, 0]]);
    r.tree
        .upload(&upload(&[
            node(1, 1, rect(180., 80.), &[2, 11]),
            node(2, 3, width(vec![1, 1]), &[3, 7]),
            node(3, 1, block(&[px(1, 30.)]), &[4, 5]),
            node(4, 6, fill_w(), &[]),
            node(5, 2, fill_w(), &[13, 14]),
            node(13, 6, rect(10., 10.), &[]),
            node(14, 6, rect(10., 10.), &[]),
            node(7, 4, block(&[vec![2, 0]]), &[8]),
            node(8, 14, block(&[vec![1, 1], vec![2, 0]]), &[9, 10]),
            node(9, 4, rect(30., 21.), &[]),
            node(10, 4, rect(30., 21.), &[]),
            node(11, 3, width(px(1, 60.)), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let incoming = |to| {
        patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 11]),
                edges(2, 3, if to == 2 { &[3, 7] } else { &[3] }),
                edges(11, 3, if to == 2 { &[] } else { &[7] }),
            ],
        )
    };
    let refuse = |r: &mut Raster, edit: Bytes| {
        let accepted = r.tree.encode();
        let pixels = r.pixels().to_vec();
        let hit = r.hit_at(6., 35.);
        assert!(
            r.tree
                .patch(&edit)
                .unwrap_err()
                .contains("Column share depends on that same width")
        );
        assert_eq!(r.tree.encode(), accepted);
        assert!(r.tree.layers.worksets_empty());
        assert_eq!(r.pixels(), pixels);
        assert_eq!(r.hit_at(6., 35.), hit);
    };
    refuse(&mut r, set(3, 1, fill_w())); // the Column is outside the attrs-only target
    r.tree.patch(&incoming(11)).unwrap();
    r.tree.patch(&set(3, 1, fill_w())).unwrap();
    draw(&mut r);
    check_fresh(&r);
    refuse(&mut r, incoming(2)); // the pack, its filling wrapper and both cards are closed
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 11]),
                node(2, 3, width(px(1, 30.)), &[3, 7]),
                edges(11, 3, &[]),
            ],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 8).height, bounds(&r, 8).width), (40., 60.));
    // No packing feedback when the only card has a fixed width and height.
    r.tree.patch(&order(8, 14, &[9])).unwrap();
    r.tree.patch(&set(2, 3, width(vec![1, 1]))).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 8).width, 30.);
    check_fresh(&r);
}

#[test]
fn a_filling_wrappers_independent_paragraph_height_can_supply_a_pack_share() {
    // A width-filling wrapper does not make its fixed-width paragraph read
    // the Column's width. A second, fixed-height paragraph needs the actual
    // offer for its lines, but not to charge the slot.
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[vec![1, 1], px(2, 100.)]), &[2, 7]),
            node(2, 3, block(&[vec![1, 0]]), &[3, 5]),
            node(3, 7, block(&[px(1, 48.)]), &[4]),
            node(4, 5, text(LONG), &[]),
            node(5, 7, block(&[vec![1, 0], px(2, 4.)]), &[6]),
            node(6, 5, text("MM MM"), &[]),
            node(7, 14, block(&[vec![1, 1], vec![2, 0]]), &[8, 9]),
            node(8, 4, rect(80., 21.), &[]),
            node(9, 4, rect(80., 21.), &[]),
        ]))
        .unwrap();
    for (s, x, lines) in [(LONG, 160., 3.), (SHORT, 80., 1.), (LONG, 160., 3.)] {
        let n = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(4, 5, text(s))).unwrap();
        draw(&mut r);
        assert_eq!(r.tree.layers.paragraphs.solves - n, 2);
        let h = r.tree.layers.runs[r.tree.elements.ids[&4]].metrics.height * lines + 4.;
        assert_eq!((bounds(&r, 2).height, bounds(&r, 7).height), (h, 100. - h));
        assert_eq!((bounds(&r, 1).width, bounds(&r, 7).width), (x, x));
        check_fresh(&r);
    }
}

#[test]
fn consecutive_unbounded_document_boxes_do_not_overflow_the_enclosing_row() {
    let mut r = Raster::default();
    let nodes = std::iter::once(node(1, 1, block(&[vec![1, 1]]), &[2, 4, 6, 8]))
        .chain([2, 4, 6].into_iter().flat_map(|id| {
            [
                node(id, 8, block(&[vec![1, 1]]), &[id + 1]),
                node(
                    id + 1,
                    4,
                    block(&[px(1, 24.), vec![2, 0], number(79, 90.)]),
                    &[],
                ),
            ]
        }))
        .chain([node(8, 4, rect(8., 8.), &[])])
        .collect::<Vec<_>>();
    r.tree.upload(&upload(&nodes)).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 1).width, f32::MAX);
    assert_eq!(bounds(&r, 8).x, f32::MAX);
    for (_, f) in r.tree.frames() {
        assert!(
            [f.x, f.y, f.width, f.height]
                .into_iter()
                .all(f32::is_finite)
        );
    }
}

#[test]
fn a_kept_projected_block_moves_between_documents_and_releases_the_old_band() {
    let mut r = Raster::default();
    let doc = || block(&[content_bounds::bound(1, 4, 90.), px(2, 70.)]);
    let card = |side: Option<u8>| {
        block(
            &[
                Some(px(1, 24.)),
                Some(bounded::bound(2, 4, 80., None, false)),
                Some(number(79, 90.)),
                side.map(|s| vec![5, s]),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>(),
        )
    };
    r.tree
        .upload(&upload(&[
            node(1, 1, rect(180., 120.), &[2, 3]),
            node(2, 8, doc(), &[4, 5]),
            node(3, 8, doc(), &[7]),
            node(4, 4, card(None), &[]),
            node(5, 7, block(&[px(1, 60.)]), &[6]),
            node(6, 5, text("AA"), &[]),
            node(7, 7, block(&[px(1, 60.)]), &[8]),
            node(8, 5, text("AA"), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    for (to, side, w) in [
        (3, Some(0), 24.),
        (2, Some(2), 24.),
        (3, None, 80.),
        (2, None, 80.),
    ] {
        let (here, there): (&[u64], &[u64]) = if to == 2 {
            (&[4, 5], &[7])
        } else {
            (&[5], &[4, 7])
        };
        r.tree
            .patch(&patch(
                1,
                1,
                &[
                    edges(1, 1, &[2, 3]),
                    edges(2, 8, here),
                    edges(3, 8, there),
                    attr_record(4, 4, card(side)),
                ],
            ))
            .unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 4).width, w);
        let word = if to == 2 { 6 } else { 8 };
        assert_eq!(bounds(&r, word).y, if side.is_some() { 0. } else { 24. });
        assert_eq!(bounds(&r, if word == 6 { 8 } else { 6 }).y, 0.);
        check_fresh(&r);
    }
}

#[test]
fn document_bands_change_inside_a_constrained_turn_without_a_second_solution() {
    let mut r = Raster::default();
    let side = |h| block(&[vec![5, 0], px(1, 32.), px(2, h)]);
    r.tree
        .upload(&upload(&[
            node(1, 1, rect(160., 120.), &[2, 7]),
            node(2, 8, block(&[px(1, 64.), number(79, 90.)]), &[3, 4]),
            node(3, 4, side(40.), &[]),
            node(4, 7, block(&[]), &[5]),
            node(5, 5, text(LONG), &[]),
            node(7, 4, marker(), &[]),
        ]))
        .unwrap();
    for (h, s) in [(40., LONG), (8., SHORT), (0., LONG), (40., LONG)] {
        let old = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(3, 4, side(h))).unwrap();
        r.tree.patch(&set(5, 5, text(s))).unwrap();
        draw(&mut r);
        assert_eq!(r.tree.layers.paragraphs.solves - old, 1);
        assert_eq!(bounds(&r, 7).width, 160. - bounds(&r, 2).width);
        check_fresh(&r);
    }
}
