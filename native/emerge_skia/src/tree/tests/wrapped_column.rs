use super::*;

fn gaps(x: f64, y: f64) -> Bytes {
    [vec![36], x.to_be_bytes().to_vec(), y.to_be_bytes().to_vec()].concat()
}
fn rect(w: f64, h: f64) -> Bytes {
    block(&[px(1, w), px(2, h)])
}
fn attrs(width: Bytes, height: Bytes) -> Bytes {
    block(&[width, height, gaps(6., 4.)])
}
fn header(h: f64) -> Bytes {
    rect(64., h)
}

pub(super) fn shared() -> Bytes {
    upload(&[
        node(1, 1, header(44.), &[2, 5]),
        node(2, 14, attrs(vec![1, 1], vec![2, 0]), &[3, 4]),
        node(3, 6, rect(12., 20.), &[]),
        node(
            4,
            4,
            block(&[px(1, 12.), px(2, 20.), vec![40, 1], color([0, 0, 255])]),
            &[],
        ),
        node(5, 2, block(&[vec![1, 0], number(4, 4.)]), &[6, 7]),
        node(6, 6, rect(24., 8.), &[]),
        node(7, 6, rect(24., 8.), &[]),
    ])
}

#[test]
fn an_independent_height_supplies_packed_width_before_its_sibling_flows() {
    let mut r = Raster::default();
    r.tree.upload(&shared()).unwrap();
    for h in [44., 43., 44.] {
        r.tree.patch(&set(1, 1, header(h))).unwrap();
        draw(&mut r);
        let (w, peer_h, origin) = if h == 44. {
            (12., 8., [0., 24.])
        } else {
            (30., 20., [18., 0.])
        };
        assert_eq!(
            (bounds(&r, 2).width, bounds(&r, 5).x, bounds(&r, 5).height),
            (w, w, peer_h)
        );
        assert_eq!([bounds(&r, 4).x, bounds(&r, 4).y], origin);
        let mut cold = Raster::default();
        cold.tree.upload(&r.tree.encode()).unwrap();
        assert_eq!(draw(&mut cold), r.pixels());
        assert_eq!(bounds(&cold, 5), bounds(&r, 5));
    }
    // Removing the Row height uses the unallotted one-column basis. Neither
    // that basis nor the empty card's zero content height reads the split.
    for edit in [
        set(1, 1, block(&[px(1, 64.)])),
        set(4, 4, block(&[px(1, 12.)])),
    ] {
        r.tree.patch(&edit).unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 2).width, bounds(&r, 5).x), (12., 12.));
        check_fresh(&r);
    }
}

pub(super) fn shared_column() -> Bytes {
    upload(&[
        node(1, 1, rect(64., 60.), &[2, 6]),
        node(2, 3, block(&[vec![1, 1], px(2, 60.)]), &[3, 4]),
        node(3, 4, rect(0., 16.), &[]),
        node(4, 14, attrs(vec![1, 1], vec![2, 0]), &[5, 9]),
        node(5, 6, rect(12., 20.), &[]),
        node(9, 6, rect(12., 20.), &[]),
        node(6, 2, block(&[vec![1, 0], number(4, 4.)]), &[7, 8]),
        node(7, 6, rect(24., 8.), &[]),
        node(8, 6, rect(24., 8.), &[]),
    ])
}
#[test]
fn a_declared_column_share_reaches_an_earlier_width_consumer_without_retry() {
    let mut r = Raster::default();
    r.tree.upload(&shared_column()).unwrap();
    for (header, w, peer) in [(16., 12., 8.), (17., 30., 20.), (16., 12., 8.)] {
        r.tree.patch(&set(3, 4, rect(0., header))).unwrap();
        draw(&mut r);
        assert_eq!(
            (
                bounds(&r, 2).width,
                bounds(&r, 4).width,
                bounds(&r, 6).x,
                bounds(&r, 6).height
            ),
            (w, w, w, peer)
        );
        let mut cold = Raster::default();
        cold.tree.upload(&r.tree.encode()).unwrap();
        draw(&mut cold);
        for (id, frame) in cold.tree.frames() {
            assert_eq!(bounds(&r, id), frame);
        }
    }
    // The other sibling has independent, empty content instead of a declaration.
    r.tree.patch(&set(3, 4, block(&[vec![2, 1]]))).unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 4).height, bounds(&r, 6).x), (60., 12.));
    check_fresh(&r);
}

// min(px(cap), fill()) and min(px(cap), max(content(), fill())) have the
// same intrinsic clamp. Only the latter reads the packed width in Flow.
pub(super) fn reader_attrs(cap: f64, reads_content: bool) -> Bytes {
    let basis = if reads_content {
        vec![5, 1, 0]
    } else {
        vec![0]
    };
    let width = [vec![1, 4], number(2, cap), basis].concat();
    block(&[width, vec![2, 0], vec![5, 2], gaps(6., 4.)])
}
pub(super) fn reader_column() -> Bytes {
    let card = |rgb| block(&[px(1, 12.), px(2, 20.), color(rgb), vec![40, 1]]);
    upload(&[
        node(1, 3, rect(16., 60.), &[2, 3]),
        node(2, 4, rect(1., 17.), &[]),
        node(3, 14, reader_attrs(80., false), &[4, 5]),
        node(4, 4, card([255, 0, 0]), &[]),
        node(5, 4, card([0, 0, 255]), &[]),
    ])
}

#[test]
fn a_sparse_width_edit_can_add_or_remove_the_only_column_share_reader() {
    let mut r = Raster::default();
    r.tree.upload(&reader_column()).unwrap();
    draw(&mut r);
    let column = r.tree.elements.ids[&1];
    let edit = |cap, reads| set(3, 14, reader_attrs(cap, reads));
    let rect = |x, width| Rect {
        x,
        y: 17.,
        width,
        height: 43.,
    };
    assert_eq!(bounds(&r, 3), rect(0., 16.));
    assert_eq!(r.hit_at(6., 22.), Some((4, 6., 5.)));
    for (cap, reads, transition) in [(80., true, true), (79., true, false), (79., false, true)] {
        let old_pixels = r.pixels().to_vec();
        let old_hit = r.hit_at(6., 22.);
        let passes = r.tree.layers.invalidation_passes;
        r.tree.patch(&edit(cap, reads)).unwrap();
        assert!(r.tree.layers.worksets_empty());
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused handoff".into()))
            .unwrap_err();
        let (x, w, hit) = if reads {
            (-14., 30., (5, 2., 5.))
        } else {
            (0., 16., (4, 6., 5.))
        };
        assert_eq!(bounds(&r, 3), rect(x, w));
        assert_eq!(r.tree.layers.has_height_group(column), reads);
        assert_eq!(ran(&r, Context, 1), transition);
        assert_eq!(r.tree.layers.invalidation_passes, passes + 1);
        assert_eq!(r.pixels(), old_pixels);
        assert_eq!(r.hit_at(6., 22.), old_hit);
        draw(&mut r);
        assert_eq!(r.hit_at(6., 22.), Some(hit));
        check_fresh(&r);
    }
    // No transient reader when both edits are accepted before compute.
    r.tree.patch(&edit(79., true)).unwrap();
    r.tree.patch(&edit(79., false)).unwrap();
    assert!(r.tree.layers.worksets_empty());
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
    assert!(!r.tree.layers.has_height_group(column));
}

#[test]
fn swapping_weights_at_an_unchanged_group_unit_updates_packed_width_once() {
    let mut r = Raster::default();
    r.tree.upload(&shared_column()).unwrap();
    draw(&mut r);
    let header = |weight| set(3, 4, block(&[px(1, 0.), weighted::weight(2, weight)]));
    let cards = |weight| set(4, 14, attrs(vec![1, 1], weighted::weight(2, weight)));
    for (a, b, h, w) in [(3., 1., 15., 30.), (1., 3., 45., 12.), (3., 1., 15., 30.)] {
        let n = r.tree.layers.invalidation_passes;
        r.tree.patch(&header(a)).unwrap();
        r.tree.patch(&cards(b)).unwrap();
        assert!(r.tree.layers.worksets_empty());
        draw(&mut r);
        assert_eq!(r.tree.layers.invalidation_passes, n + 1);
        assert_eq!(
            (bounds(&r, 4).height, bounds(&r, 2).width, bounds(&r, 6).x),
            (h, w, w)
        );
    }
}

#[test]
fn no_packed_width_satisfies_both_assignments_in_a_content_width_column() {
    let root = |w| block(&[w, px(2, 60.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, root(px(1, 30.)), &[2, 7]),
            node(2, 1, block(&[vec![1, 0]]), &[3, 4]),
            node(3, 6, block(&[vec![1, 0]]), &[]),
            node(4, 2, block(&[vec![1, 0]]), &[5, 6]),
            node(5, 6, rect(10., 10.), &[]),
            node(6, 6, rect(10., 10.), &[]),
            node(
                7,
                14,
                block(&[vec![1, 1], vec![2, 0], vec![40, 1]]),
                &[8, 9],
            ),
            node(8, 6, rect(30., 21.), &[]),
            node(
                9,
                4,
                block(&[px(1, 30.), px(2, 21.), vec![40, 1], color([0, 0, 255])]),
                &[],
            ),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(
        (
            bounds(&r, 2).height,
            bounds(&r, 7).height,
            bounds(&r, 7).width
        ),
        (20., 40., 60.)
    );
    let old = r.pixels().to_vec();
    let old_hit = r.hit_at(6., 35.);
    r.tree.patch(&set(1, 3, root(px(1, 60.)))).unwrap();
    let pending = r.tree.encode();
    assert!(
        r.tree
            .patch(&set(1, 3, root(vec![1, 1])))
            .unwrap_err()
            .contains("before its height is assigned")
    );
    assert_eq!(r.tree.encode(), pending);
    assert!(r.tree.layers.worksets_empty());
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("test handoff".into()))
            .is_err()
    );
    assert_eq!(r.pixels(), old);
    assert_eq!(r.hit_at(6., 35.), old_hit);
    draw(&mut r);
    assert_eq!(
        (
            bounds(&r, 2).height,
            bounds(&r, 7).height,
            bounds(&r, 7).width
        ),
        (10., 50., 30.)
    );
    assert_eq!(r.hit_at(6., 35.), Some((9, 6., 4.)));
}

#[test]
fn a_nested_columns_late_width_moves_the_following_column() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 14, attrs(px(1, 64.), px(2, 44.)), &[2, 5, 6]),
            node(2, 14, attrs(vec![1, 1], vec![2, 0]), &[3, 4]),
            node(3, 6, rect(12., 20.), &[]),
            node(4, 6, rect(12., 20.), &[]),
            node(5, 6, rect(8., 20.), &[]),
            node(6, 6, rect(8., 8.), &[]),
        ]))
        .unwrap();
    for w in [12., 14., 12.] {
        r.tree.patch(&set(3, 6, rect(w, 20.))).unwrap();
        draw(&mut r);
        assert_eq!(
            (
                bounds(&r, 2).height,
                bounds(&r, 2).width,
                bounds(&r, 4).x,
                bounds(&r, 6).x
            ),
            (20., w as f32 + 18., w as f32 + 6., w as f32 + 24.)
        );
    }
}

#[test]
fn root_resize_and_fractional_alignment_use_current_offers() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 14, attrs(vec![1, 1], vec![2, 0]), &[2, 3]),
            node(2, 6, block(&[px(1, 12.), px(2, 20.), vec![5, 1]]), &[]),
            node(3, 6, rect(13., 20.), &[]),
        ]))
        .unwrap();
    for (h, w, x) in [
        (44., 13., 0.5),
        (43., 31., 0.),
        (0., 31., 0.),
        (44., 13., 0.5),
    ] {
        r.settle(64., h, 1.).unwrap();
        assert_eq!((bounds(&r, 1).width, bounds(&r, 2).x), (w, x));
    }
}

#[test]
fn cross_alignment_is_placement_only_and_uses_each_groups_own_maximum() {
    for (kind, axis) in [(1, 6), (3, 5), (2, 6), (14, 5)] {
        let vertical = axis == 5;
        let wh = |main, across| {
            if vertical {
                rect(across, main)
            } else {
                rect(main, across)
            }
        };
        let item = |align| block(&[px(1, 16.), px(2, 16.), vec![axis, align]]);
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, wh(36., 64.), &[2, 3, 4, 5]),
                node(2, 4, item(0), &[]),
                node(3, 6, wh(20., 32.), &[]),
                node(4, 4, item(0), &[]),
                node(5, 6, wh(20., 24.), &[]),
            ]))
            .unwrap();
        draw(&mut r);
        r.tree.patch(&set(2, 4, item(1))).unwrap();
        r.tree.patch(&set(4, 4, item(2))).unwrap();
        draw(&mut r);
        let cross = |id| {
            if vertical {
                bounds(&r, id).x
            } else {
                bounds(&r, id).y
            }
        };
        assert_eq!(
            (cross(2), cross(4)),
            if matches!(kind, 2 | 14) {
                (8., 40.)
            } else {
                (24., 48.)
            }
        );
        assert!(
            r.tree
                .layers
                .trace
                .iter()
                .all(|(p, _, _)| !matches!(p, Measure | Flow | Reflow))
        );
        // Move both closed children: their former cross alignment is now
        // read by the new parent's main-axis placement.
        let opposite = if vertical { 1 } else { 3 };
        r.tree
            .patch(&patch(1, 10, &[node(10, opposite, wh(36., 64.), &[2, 4])]))
            .unwrap();
        draw(&mut r);
        for (id, pos) in [(2, 16.), (4, 48.)] {
            let b = bounds(&r, id);
            assert_eq!(if vertical { b.x } else { b.y }, pos);
        }
        // The El reads the kept child's alignment. On promotion there is
        // no parent, but its declared alignment is not stripped.
        r.tree
            .patch(&patch(10, 11, &[node(11, 4, wh(36., 64.), &[2])]))
            .unwrap();
        draw(&mut r);
        let b = bounds(&r, 2);
        assert_eq!(if vertical { b.x } else { b.y }, 24.);
        r.tree.patch(&patch(11, 2, &[])).unwrap();
        draw(&mut r);
        let b = bounds(&r, 2);
        assert_eq!([b.x, b.y], [0., 0.]);
        let accepted = upload(&[node(2, 4, item(1), &[])]);
        assert_eq!(r.tree.encode(), accepted);
        // Leaf child arity is still checked even when the kept child is aligned.
        assert_eq!(
            r.tree
                .patch(&patch(2, 12, &[node(12, 6, block(&[]), &[2])]))
                .unwrap_err(),
            "leaf has children"
        );
        assert_eq!(r.tree.encode(), accepted);
    }
}

#[test]
fn inherited_alignment_respects_an_explicit_leading_override_and_signed_overflow() {
    let mut r = Raster::default();
    let host = |right: f64| {
        block(&[
            px(1, 40.),
            px(2, 16.),
            vec![5, 2],
            [
                vec![3, 2],
                0f64.to_be_bytes().to_vec(),
                right.to_be_bytes().to_vec(),
                0f64.to_be_bytes().to_vec(),
                0f64.to_be_bytes().to_vec(),
            ]
            .concat(),
        ])
    };
    r.tree
        .upload(&upload(&[
            node(3, 3, rect(40., 40.), &[1]),
            node(1, 4, host(0.), &[2]),
            node(2, 6, rect(44., 24.), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 2).x, bounds(&r, 2).y), (-4., 0.));
    r.tree.patch(&set(1, 4, host(6.))).unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 2).x, -10.);
    r.tree
        .patch(&set(2, 6, block(&[px(1, 44.), px(2, 24.), vec![5, 0]])))
        .unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 2).x, bounds(&r, 2).y), (0., 0.));
}
