use super::*;

mod inline;

fn axes(w: f64, h: f64) -> Bytes {
    block(&[px(1, w), px(2, h)])
}
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}
fn hit(r: &Raster, at: [f32; 2], id: u64, local: [f64; 2]) {
    let (target, x, y) = r.hit_at(at[0], at[1]).expect("installed listener");
    assert_eq!(target, id);
    assert!(
        (x - local[0]).abs() < 1e-4 && (y - local[1]).abs() < 1e-4,
        "{x},{y} expected {local:?}"
    );
}
fn chip(tag: u8, degrees: f64) -> Bytes {
    block(&[
        px(1, 60.),
        px(2, 20.),
        number(tag, degrees),
        color([255, 0, 0]),
        vec![40, 1],
    ])
}
fn marker() -> Bytes {
    block(&[px(1, 8.), px(2, 8.), color([0, 0, 255]), vec![40, 1]])
}
#[test]
fn only_layout_turns_reserve_the_aabb_and_only_successful_submit_advances_hits() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, axes(160., 120.), &[2, 3]),
            node(2, 4, chip(33, 90.), &[]),
            node(3, 4, marker(), &[]),
        ]))
        .unwrap();
    let old = draw(&mut r).to_vec();
    assert_eq!(bounds(&r, 2), rect(0., 0., 60., 20.));
    assert_eq!(bounds(&r, 3).x, 60.);
    hit(&r, [30., 2.], 2, [22., 10.]);
    r.tree.patch(&set(2, 4, chip(79, -270.))).unwrap();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("not submitted".into()))
            .is_err()
    );
    assert_eq!(bounds(&r, 2), rect(0., 0., 20., 60.));
    assert_eq!(bounds(&r, 3).x, 20.);
    assert_eq!(r.pixels(), old);
    hit(&r, [30., 2.], 2, [22., 10.]);
    assert_ne!(draw(&mut r), old);
    hit(&r, [6., 55.], 2, [55., 14.]);
    hit(&r, [22., 2.], 3, [2., 2.]);
    for (d, expected) in [
        (45., 80. * std::f32::consts::FRAC_1_SQRT_2),
        (-90., 20.),
        (180., 60.),
        (360., 60.),
    ] {
        r.tree.patch(&set(2, 4, chip(79, d))).unwrap();
        draw(&mut r);
        assert!((bounds(&r, 3).x - expected).abs() < 1e-4);
        check_fresh(&r);
    }
}
#[test]
fn root_offers_swap_on_quarter_turns_and_descendants_keep_the_local_box_on_resize() {
    let mut t = Tree::default();
    let root = |d| block(&[vec![1, 0], vec![2, 0], number(79, d)]);
    t.upload(&upload(&[
        node(1, 4, root(90.), &[2]),
        node(
            2,
            4,
            block(&[px(1, 30.), px(2, 10.), vec![5, 2], vec![6, 2], vec![40, 1]]),
            &[],
        ),
    ]))
    .unwrap();
    for (d, w, h, z) in [
        (90., 300., 500., 1.),
        (-90., 180., 90., 1.5),
        (0., 180., 90., 1.5),
        (90., 180., 90., 1.5),
    ] {
        t.patch(&set(1, 4, root(d))).unwrap();
        t.refresh(w, h, z).unwrap();
        let k = t.elements.ids[&1];
        assert_eq!(t.layers.frames[k], rect(0., 0., w, h));
        let (rw, rh) = if d == 0. { (w, h) } else { (h, w) };
        let (dx, dy) = ((w - rw) * 0.5, (h - rh) * 0.5);
        assert_eq!(t.layers.paint[k].frame, rect(dx, dy, rw, rh));
        assert_eq!(
            t.layers.frames[t.elements.ids[&2]],
            rect(dx + rw - 30. * z, dy + rh - 10. * z, 30. * z, 10. * z)
        );
    }
}
#[test]
fn a_turned_subtree_respects_stationary_and_moving_clips_even_when_its_owner_has_no_ink() {
    let mut r = Raster::default();
    let root = block(&[px(1, 46.), px(2, 54.), vec![40, 1]]);
    let host = |d, z| {
        block(&[
            px(1, 60.),
            px(2, 20.),
            number(33, d),
            number(34, z),
            number(32, 30.),
        ])
    };
    let leaf = block(&[
        px(1, 60.),
        px(2, 20.),
        number(34, -1.),
        vec![40, 1],
        color([255, 0, 0]),
    ]);
    r.tree
        .upload(&upload(&[
            node(1, 1, root, &[2]),
            node(2, 4, host(90., 1.), &[3]),
            node(3, 4, leaf, &[]),
        ]))
        .unwrap();
    draw(&mut r);
    hit(&r, [26., 14.], 3, [56., 6.]);
    hit(&r, [41., 20.], 1, [41., 20.]);
    assert!(r.hit_at(30., 60.).is_none());
    for (d, z) in [(45., 2.), (90., 0.), (-90., 1.), (0., 1.)] {
        r.tree.patch(&set(2, 4, host(d, z))).unwrap();
        draw(&mut r);
        assert!(
            r.tree
                .layers
                .trace
                .iter()
                .all(|(p, _, _)| matches!(p, PaintContext | Compose))
        );
        if z == 0. {
            hit(&r, [26., 14.], 1, [26., 14.]);
        }
        check_fresh(&r);
    }
}
#[test]
fn early_rotated_charges_split_fills_break_lines_and_follow_inherited_height() {
    let mut r = Raster::default();
    let shell = |h| block(&[px(1, 100.), px(2, h)]);
    let turn = block(&[px(1, 40.), vec![2, 0], number(79, 90.)]);
    let row = block(&[vec![1, 0], vec![2, 0], number(4, 4.)]);
    r.tree
        .upload(&upload(&[
            node(1, 4, shell(30.), &[2]),
            node(2, 1, row, &[3, 4]),
            node(3, 4, turn.clone(), &[]),
            node(4, 4, block(&[vec![1, 0], px(2, 8.)]), &[]),
        ]))
        .unwrap();
    for h in [30., 50., 0., 30.] {
        r.tree.patch(&set(1, 4, shell(h))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3), rect(0., 0., h as f32, 40.));
        assert_eq!(bounds(&r, 4), rect(h as f32 + 4., 0., 96. - h as f32, 8.));
        check_fresh(&r);
    }
    r.tree.patch(&set(1, 4, block(&[px(1, 100.)]))).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 4), rect(44., 0., 56., 8.));
    check_fresh(&r);
    let mut r = Raster::default();
    let line = |w| block(&[px(1, w), px(2, 28.), number(4, 2.)]);
    r.tree
        .upload(&upload(&[
            node(1, 2, line(49.), &[2, 3]),
            node(2, 4, turn, &[]),
            node(3, 4, axes(20., 6.), &[]),
        ]))
        .unwrap();
    for (w, at) in [(49., [0., 42.]), (50., [30., 0.]), (49., [0., 42.])] {
        r.tree.patch(&set(1, 2, line(w))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3), rect(at[0], at[1], 20., 6.));
        check_fresh(&r);
    }
}
#[test]
fn equal_unallotted_extents_do_not_hide_a_changed_rotated_fill_charge() {
    let mut r = Raster::default();
    let card = |angle, h| block(&[px(1, 40.), h, number(79, angle)]);
    r.tree
        .upload(&upload(&[
            node(1, 1, axes(160., 100.), &[2, 3]),
            node(2, 4, card(0., vec![2, 0]), &[4]),
            node(4, 4, axes(40., 40.), &[]),
            node(3, 4, marker(), &[]),
        ]))
        .unwrap();
    for (d, h, x) in [
        (0., vec![2, 0], 40.),
        (90., vec![2, 0], 100.),
        (90., bounded::bound(2, 4, 70., None, false), 70.),
        (0., vec![2, 0], 40.),
    ] {
        r.tree.patch(&set(2, 4, card(d, h))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3).x, x);
        check_fresh(&r);
    }
}
#[test]
fn a_turned_content_cap_uses_the_local_basis_even_if_its_occupied_width_is_unchanged() {
    let mut r = Raster::default();
    let cap = block(&[vec![1, 4, 1, 0], px(2, 20.), number(79, 90.)]);
    r.tree
        .upload(&upload(&[
            node(1, 1, axes(160., 80.), &[2, 4]),
            node(2, 4, cap, &[3]),
            node(3, 4, axes(60., 8.), &[]),
            node(4, 4, block(&[vec![1, 0], px(2, 8.)]), &[]),
        ]))
        .unwrap();
    for w in [60., 30., 60.] {
        r.tree.patch(&set(3, 4, axes(w, 8.))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 2), rect(0., 0., 20., w as f32));
        assert_eq!(bounds(&r, 4), rect(w as f32, 0., 160. - w as f32, 8.));
        check_fresh(&r);
    }
}
#[test]
fn a_column_share_uses_the_turned_childs_local_content_cap() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, axes(80., 100.), &[2, 4]),
            node(
                2,
                4,
                block(&[px(1, 20.), vec![2, 4, 1, 0], number(79, 90.)]),
                &[3],
            ),
            node(3, 4, axes(8., 40.), &[]),
            node(4, 4, block(&[px(1, 8.), vec![2, 0]]), &[]),
        ]))
        .unwrap();
    for (content, share) in [(40., 40.), (12., 12.), (60., 50.), (40., 40.)] {
        r.tree.patch(&set(3, 4, axes(8., content))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 2), rect(0., 0., share, 20.));
        // The next fill is allocated what the first didn't use, but it
        // starts below the 20px occupied height, not below the local share.
        assert_eq!(bounds(&r, 4), rect(0., 20., 8., 100. - share));
        check_fresh(&r);
    }
}
#[test]
fn wrapping_columns_break_at_occupied_height_and_turned_paragraphs_clear_document_floats() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 14, axes(90., 64.), &[2, 3]),
            node(2, 4, chip(79, 90.), &[]),
            node(3, 4, marker(), &[]),
        ]))
        .unwrap();
    for (h, at) in [(64., [20., 0.]), (68., [0., 60.]), (64., [20., 0.])] {
        r.tree.patch(&set(1, 14, axes(90., h))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3), rect(at[0], at[1], 8., 8.));
        hit(&r, [at[0] + 2., at[1] + 3.], 3, [2., 3.]);
        check_fresh(&r);
    }
    let mut r = Raster::default();
    let paragraph = |angle| block(&[px(1, 44.), px(2, 44.), number(79, angle)]);
    r.tree
        .upload(&upload(&[
            node(1, 8, axes(96., 120.), &[2, 3, 5]),
            node(2, 4, document::margin(0, 20., 48.), &[]),
            node(3, 7, paragraph(0.), &[4]),
            node(4, 5, text("A"), &[]),
            node(5, 4, marker(), &[]),
        ]))
        .unwrap();
    // The square has the same occupied extents in both states. Its role, not
    // a size change, determines whether the document's float bands enter it.
    for (d, y, text_x, next) in [(0., 0., 20., 48.), (90., 48., 0., 92.), (0., 0., 20., 48.)] {
        r.tree.patch(&set(3, 7, paragraph(d))).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3), rect(0., y, 44., 44.));
        assert_eq!(bounds(&r, 4).x, text_x);
        assert_eq!(bounds(&r, 5).y, next);
        check_fresh(&r);
    }
}
pub(super) fn split_prose() -> Bytes {
    upload(&[
        node(1, 1, axes(160., 120.), &[2, 3]),
        node(
            2,
            4,
            block(&[px(1, 48.), number(79, 90.), vec![40, 1], color([255, 0, 0])]),
            &[4],
        ),
        node(4, 7, block(&[px(1, 48.)]), &[5]),
        node(5, 5, text("AA BB CC DD EE"), &[]),
        node(
            3,
            4,
            block(&[vec![1, 0], px(2, 8.), vec![40, 1], color([0, 0, 255])]),
            &[],
        ),
    ])
}

#[test]
fn a_row_consumes_the_independently_measured_charge_without_reopening_flow() {
    let mut r = Raster::default();
    r.tree.upload(&split_prose()).unwrap();
    for (words, lines) in [
        ("AA BB CC DD EE", 3.),
        ("AA BB", 1.),
        ("M M MMMM", 2.),
        ("MM MM MM", 3.),
        ("AA BB", 1.),
        ("AA BB CC DD EE", 3.),
    ] {
        let before = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(5, 5, text(words))).unwrap();
        draw(&mut r);
        assert_eq!(r.tree.layers.paragraphs.solves - before, 1);
        let h = r.tree.layers.runs[r.tree.elements.ids[&5]].metrics.height * lines;
        assert_eq!(bounds(&r, 2), rect(0., 0., h, 48.));
        assert_eq!(bounds(&r, 3), rect(h, 0., 160. - h, 8.));
        hit(&r, [4., 6.], 2, [6., f64::from(h - 4.)]);
        hit(&r, [h + 4., 2.], 3, [4., 2.]);
        let trace = &r.tree.layers.trace;
        let index = |pass, id| {
            trace
                .iter()
                .position(|&(p, n, _)| p == pass && n == id)
                .unwrap()
        };
        assert!(index(Measure, 1) < index(Flow, 1));
        assert!(index(Flow, 1) < index(Reflow, 4));
        assert_eq!(
            trace
                .iter()
                .filter(|&&(p, n, _)| n == 4 && p == Reflow)
                .count(),
            1
        );
        assert!(r.tree.layers.worksets_empty());
        check_fresh(&r);
    }
    // A descendant edit can change whether the charge needs lines without
    // changing the enclosing El's intrinsic measurement or its declaration.
    let line = r.tree.layers.runs[r.tree.elements.ids[&5]].metrics.height;
    for (para, charge) in [
        (axes(48., line.into()), line),
        (block(&[px(1, 48.)]), line * 3.),
    ] {
        r.tree.patch(&set(4, 7, para)).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 3), rect(charge, 0., 160. - charge, 8.));
        check_fresh(&r);
    }
    // A leading fill must get its new share before the turned box is placed.
    r.tree.patch(&order(1, 1, &[3, 2])).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).x, 160. - bounds(&r, 2).width);
    assert!(!ran(&r, Reflow, 4));
    check_fresh(&r);
    // This non-fill's resolved local width is intrinsic, not the occupied charge.
    r.tree.patch(&set(2, 4, block(&[number(79, 90.)]))).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 3).width, 160. - line * 3.);
    check_fresh(&r);
    let wrap = upload(&[
        node(8, 2, axes(160., 120.), &[9]),
        node(9, 7, block(&[px(1, 48.), number(79, 90.)]), &[10]),
        node(10, 5, text("AA BB CC DD EE"), &[]),
    ]);
    r.tree.patch(&at(wrap, 1)).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 9).width, line * 3.);
    check_fresh(&r);
}

#[test]
fn moving_a_kept_branch_changes_which_row_has_to_wait_for_its_lines() {
    let mut r = Raster::default();
    let turned = || block(&[px(1, 48.), number(79, 90.)]);
    r.tree
        .upload(&upload(&[
            node(1, 3, axes(160., 140.), &[2, 3]),
            node(2, 1, axes(160., 64.), &[4, 7]),
            node(3, 1, axes(128., 64.), &[8]),
            node(4, 4, turned(), &[5]),
            node(5, 7, block(&[]), &[6]),
            node(6, 5, text("AA BB CC DD EE"), &[]),
            node(7, 4, block(&[vec![1, 0], px(2, 8.)]), &[]),
            node(8, 4, marker(), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let line = r.tree.layers.runs[r.tree.elements.ids[&6]].metrics.height;
    for (split, words, lines) in [(false, "AA BB", 1.), (true, "AA BB CC DD EE", 3.)] {
        let moved = patch(
            1,
            1,
            &[
                edges(1, 3, &[2, 3]),
                edges(2, 1, if split { &[4, 7] } else { &[7] }),
                edges(3, 1, if split { &[8] } else { &[4, 8] }),
            ],
        );
        // Two accepted edits, one layout against the final parent links.
        r.tree.patch(&set(6, 5, text(words))).unwrap();
        r.tree.patch(&moved).unwrap();
        draw(&mut r);
        let charge = lines * line;
        assert!(ran(&r, Reflow, 5));
        assert_eq!(
            bounds(&r, 7).width,
            if split { 160. - charge } else { 160. }
        );
        assert_eq!(bounds(&r, 8).x, if split { 0. } else { charge });
        assert!(r.tree.layers.worksets_empty());
        check_fresh(&r);
    }
}

#[test]
fn a_changed_share_does_not_change_the_target_until_install() {
    let mut r = Raster::default();
    r.tree.upload(&split_prose()).unwrap();
    let old = draw(&mut r).to_vec();
    let edge = bounds(&r, 3).x;
    hit(&r, [32., 4.], 2, [4., f64::from(edge - 32.)]);
    r.tree.patch(&set(5, 5, text("AA BB"))).unwrap();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("not submitted".into()))
            .is_err()
    );
    let new_edge = bounds(&r, 3).x;
    assert!(new_edge < 32. && 32. < edge);
    assert_eq!(r.pixels(), old);
    hit(&r, [32., 4.], 2, [4., f64::from(edge - 32.)]);
    assert_ne!(draw(&mut r), old);
    hit(&r, [32., 4.], 3, [f64::from(32. - new_edge), 4.]);
}

#[test]
fn a_leading_fill_paragraph_reflows_at_the_remainder_not_the_previous_share() {
    let mut r = Raster::default();
    let turned = |w| block(&[px(1, w), number(79, 90.)]);
    let phrase = "AA BB CC DD EE";
    r.tree
        .upload(&upload(&[
            node(1, 1, axes(108., 128.), &[2, 4]),
            node(2, 7, block(&[vec![1, 0]]), &[3]),
            node(3, 5, text(phrase), &[]),
            node(4, 4, turned(48.), &[5]),
            node(5, 7, block(&[]), &[6]),
            node(6, 5, text(phrase), &[]),
        ]))
        .unwrap();
    for (w, lines) in [(48., 3.), (160., 1.), (48., 3.)] {
        r.tree.patch(&set(4, 4, turned(w))).unwrap();
        draw(&mut r);
        let line = r.tree.layers.runs[r.tree.elements.ids[&6]].metrics.height;
        let charge = lines * line;
        assert_eq!(bounds(&r, 4), rect(108. - charge, 0., charge, w as f32));
        assert_eq!(bounds(&r, 2).width, 108. - charge);
        assert_eq!(bounds(&r, 2).height, if w == 48. { 3. } else { 2. } * line);
        check_fresh(&r);
    }
    // Nested independent charges settle inside-out, without reopening Flow.
    let nested = patch(
        5,
        7,
        &[
            node(7, 1, block(&[px(1, 48.)]), &[8, 9]),
            node(8, 7, turned(24.), &[6]),
            node(9, 4, block(&[vec![1, 0]]), &[]),
        ],
    );
    r.tree.patch(&nested).unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 4).width, bounds(&r, 2).width), (24., 84.));
    check_fresh(&r);
}

#[test]
fn a_nonfill_intrinsic_local_width_is_independent_of_the_final_batch_split() {
    let mut r = Raster::default();
    let automatic = || block(&[number(79, 90.)]);
    let doc = upload(&[
        node(1, 1, axes(160., 120.), &[2, 3]),
        node(2, 4, automatic(), &[4]),
        node(4, 7, block(&[px(1, 48.)]), &[5]),
        node(5, 5, text("AA BB CC DD EE"), &[]),
        node(3, 4, marker(), &[]),
    ]);
    r.tree.upload(&doc).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 3).x, bounds(&r, 2).width);
    assert!(bounds(&r, 3).x > 32.);
    let fills = block(&[vec![1, 0], px(2, 8.)]);
    r.tree.patch(&set(3, 4, fills.clone())).unwrap();
    draw(&mut r);
    let charge = bounds(&r, 2).width;
    assert_eq!(bounds(&r, 3).width, 160. - charge);
    // Both replacements describe a single final split.
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 3]),
                attr_record(3, 4, fills),
                attr_record(2, 4, chip(79, 90.)),
            ],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 3), rect(20., 0., 140., 8.));
    check_fresh(&r);
    r.tree.patch(&set(2, 4, automatic())).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 3).x, charge);
    check_fresh(&r);
}

#[test]
fn a_closed_turned_branch_moves_between_offers_and_detaches_without_leaking_its_plane() {
    let mut r = Raster::default();
    let move_to = |to| {
        patch(
            1,
            1,
            &[
                edges(1, 1, &[2, 3]),
                edges(2, 4, if to == 2 { &[4] } else { &[] }),
                edges(3, 4, if to == 3 { &[4] } else { &[] }),
            ],
        )
    };
    r.tree
        .upload(&upload(&[
            node(1, 1, axes(180., 80.), &[2, 3]),
            node(2, 4, axes(70., 30.), &[4]),
            node(3, 4, axes(70., 48.), &[]),
            node(
                4,
                4,
                block(&[px(1, 40.), vec![2, 0], number(79, 90.)]),
                &[5],
            ),
            node(
                5,
                4,
                block(&[
                    vec![1, 0],
                    vec![2, 0],
                    number(34, -1.),
                    color([255, 0, 0]),
                    vec![40, 1],
                ]),
                &[],
            ),
        ]))
        .unwrap();
    draw(&mut r);
    hit(&r, [4., 36.], 5, [4., 4.]);
    let attachment = r.target_at(4., 36.).unwrap().0;
    r.tree.patch(&move_to(3)).unwrap();
    draw(&mut r);
    hit(&r, [74., 36.], 5, [4., 4.]);
    assert_eq!(bounds(&r, 4), rect(70., 0., 48., 40.));
    check_fresh(&r);
    r.tree.patch(&move_to(0)).unwrap();
    draw(&mut r);
    assert!(r.hit_at(74., 36.).is_none());
    let branch = node(
        4,
        4,
        block(&[px(1, 40.), vec![2, 0], number(79, 90.)]),
        &[5],
    );
    let leaf = node(
        5,
        4,
        block(&[
            vec![1, 0],
            vec![2, 0],
            number(34, -1.),
            color([255, 0, 0]),
            vec![40, 1],
        ]),
        &[],
    );
    r.tree
        .patch(&patch(2, 2, &[edges(2, 4, &[4]), branch, leaf]))
        .unwrap();
    draw(&mut r);
    hit(&r, [4., 36.], 5, [4., 4.]);
    assert_ne!(attachment, r.target_at(4., 36.).unwrap().0);
    check_fresh(&r);
}
#[test]
fn a_rounded_inline_mask_stays_with_the_wrapper_not_its_separately_turned_text() {
    let mut r = Raster::default();
    let turn = block(&[
        text("MMMM")[2..].to_vec(),
        number(31, 16.),
        number(33, 180.),
        vec![40, 1],
    ]);
    let wrapper = |radius, angle| {
        block(&[
            [vec![13, 0], f64::to_be_bytes(radius).to_vec()].concat(),
            number(79, angle),
        ])
    };
    r.tree
        .upload(&upload(&[
            node(1, 7, axes(160., 90.), &[2]),
            node(2, 4, wrapper(0., 0.), &[3]),
            node(3, 5, turn, &[]),
        ]))
        .unwrap();
    let first = draw(&mut r).to_vec();
    let f = bounds(&r, 2);
    let x = (f.x + f.width).ceil() as usize + 2;
    assert_eq!(r.hit_at(x as f32, f.y + f.height / 2.).unwrap().0, 3);
    let outside =
        |bytes: &[u8]| (0..50).any(|y| (x..x + 14).any(|i| bytes[(y * 180 + i) * 4 + 3] > 0));
    assert!(outside(&first));
    r.tree.patch(&set(2, 4, wrapper(8., 0.))).unwrap();
    let clipped = draw(&mut r).to_vec();
    assert!(!outside(&clipped));
    assert!(r.hit_at(x as f32, f.y + f.height / 2.).is_none());
    assert_eq!(r.hit_at(f.width - 10., f.y + f.height / 2.).unwrap().0, 3);
    check_fresh(&r);
    // Turn the wrapper and its mask together. The translated, half-turned
    // Text remains inside that mask, not clipped at its old device position.
    r.tree.patch(&set(2, 4, wrapper(8., 90.))).unwrap();
    assert_ne!(draw(&mut r), clipped);
    let turned = bounds(&r, 2);
    assert_eq!((turned.width, turned.height), (f.height, f.width));
    let middle = turned.width / 2.;
    assert!(r.hit_at(middle, turned.height + 2.).is_none());
    assert_eq!(r.hit_at(middle, turned.height - 10.).unwrap().0, 3);
    check_fresh(&r);
    r.tree.patch(&set(2, 4, wrapper(0., 0.))).unwrap();
    assert_eq!(draw(&mut r), first);
}
#[test]
fn snap_flags_roundtrip_without_changing_fractional_geometry_ink_or_pass_inputs() {
    let mut r = Raster::default();
    let flags = |n| {
        block(&[
            vec![28, n],
            vec![29, n],
            px(1, 60.5),
            number(31, 0.25),
            px(2, 22.75),
        ])
    };
    r.tree
        .upload(&upload(&[
            node(1, 4, flags(0), &[2]),
            node(2, 5, text("Fraction"), &[]),
        ]))
        .unwrap();
    let first = r.draw(100., 100., 1.5).unwrap().to_vec();
    let bytes = upload(&[
        node(1, 4, flags(1), &[2]),
        node(2, 5, text("Fraction"), &[]),
    ]);
    r.tree.patch(&set(1, 4, flags(1))).unwrap();
    assert_eq!(r.tree.encode(), bytes);
    assert_eq!(r.draw(100., 100., 1.5).unwrap(), first);
    assert_eq!(bounds(&r, 1), rect(0., 0., 90.75, 34.125));
    assert!(r.tree.layers.trace.is_empty());
    for bad in [
        vec![28, 2],
        vec![29, 2],
        vec![28],
        number(79, f64::NAN),
        number(34, f64::INFINITY),
    ] {
        assert!(r.tree.patch(&set(1, 4, block(&[bad]))).is_err());
        assert_eq!(r.tree.encode(), bytes);
    }
}
