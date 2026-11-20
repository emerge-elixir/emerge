use super::*;

pub(super) fn chip(w: f64) -> Bytes {
    block(&[px(1, w), px(2, 12.)])
}
pub(super) fn nested(a: f64, b: f64) -> Bytes {
    let wrap = || block(&[vec![1, 0], number(4, 4.)]);
    upload(&[
        node(1, 2, wrap(), &[2, 3]),
        node(2, 2, wrap(), &[4, 5]),
        node(4, 6, chip(a), &[]),
        node(5, 6, chip(b), &[]),
        node(3, 4, block(&[weighted::weight(1, 3.)]), &[6]),
        node(6, 6, chip(20.), &[]),
    ])
}

#[test]
fn measures_padding_and_gaps_choose_line_membership_before_shares() {
    let host = |w, right, gap| {
        let pad = [0., right, 0., 4.].into_iter().flat_map(f64::to_be_bytes);
        block(&[
            px(1, w),
            [3, 2].into_iter().chain(pad).collect(),
            number(4, gap),
        ])
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 2, host(52., 4., 4.), &[2, 3]),
            node(2, 4, block(&[vec![1, 0]]), &[4]),
            node(3, 4, block(&[vec![1, 0]]), &[5]),
            node(4, 6, chip(20.), &[]),
            node(5, 6, chip(20.), &[]),
        ]))
        .unwrap();
    for (w, right, gap, measured, weight, aw, bx, by, bw) in [
        (52., 4., 4., 20., 1., 20., 28., 0., 20.), // exact fit
        (52., 4., 4., 21., 1., 44., 4., 16., 44.), // measure only
        (52., 5., 4., 20., 1., 43., 4., 16., 43.), // padding only relative to exact fit
        (52., 4., 5., 20., 1., 44., 4., 17., 44.), // gap only
        (108., 4., 4., 20., 1., 48., 56., 0., 48.),
        (108., 4., 4., 20., 3., 24., 32., 0., 72.), // weights, same line
    ] {
        r.tree.patch(&set(1, 2, host(w, right, gap))).unwrap();
        r.tree.patch(&set(4, 6, chip(measured))).unwrap();
        r.tree
            .patch(&set(3, 4, block(&[weighted::weight(1, weight)])))
            .unwrap();
        draw(&mut r);
        let b = bounds(&r, 3);
        assert_eq!((bounds(&r, 2).width, b.x, b.y, b.width), (aw, bx, by, bw));
    }
}

#[test]
fn a_new_minimum_at_equal_preferred_width_can_move_a_peer_to_another_line() {
    let mut r = Raster::default();
    r.tree.upload(&nested(20., 20.)).unwrap();
    for (a, b, viewport, scale, aw, bx, by, bw, nested_h) in [
        (20., 20., 50., 1., 20., 24., 0., 26., 28.),
        (10., 30., 50., 1., 50., 0., 16., 50., 12.),
        (10., 30., 54., 1., 30., 34., 0., 20., 28.),
        (10., 30., 81., 1.5, 45., 51., 0., 30., 42.),
        (20., 20., 50., 1., 20., 24., 0., 26., 28.),
        (20., 20., 0., 1., 20., 0., 32., 20., 28.),
        (20., 20., 100., 1., 24., 28., 0., 72., 28.),
    ] {
        r.tree.patch(&set(4, 6, chip(a))).unwrap();
        r.tree.patch(&set(5, 6, chip(b))).unwrap();
        assert!(r.tree.layers.worksets_empty());
        r.tree.refresh(viewport, 80., scale).unwrap();
        let (aa, bb) = (bounds(&r, 2), bounds(&r, 3));
        assert_eq!(
            (aa.width, aa.height, bb.x, bb.y, bb.width),
            (aw, nested_h, bx, by, bw)
        );
        let mut cold = Tree::default();
        cold.upload(&nested(a, b)).unwrap();
        cold.refresh(viewport, 80., scale).unwrap();
        for (id, frame) in cold.frames() {
            assert_eq!(bounds(&r, id), frame);
        }
    }
}

pub(super) fn content_bound(op: u8, second_chip: f64) -> Bytes {
    upload(&[
        node(1, 2, block(&[vec![1, 0], number(4, 4.)]), &[2, 3]),
        node(
            2,
            2,
            block(&[
                vec![1, op, 1, 0],
                number(4, 4.),
                color([255, 0, 0]),
                vec![40, 1],
            ]),
            &[4, 5],
        ),
        node(4, 6, chip(20.), &[]),
        node(5, 6, chip(second_chip), &[]),
        node(3, 4, weighted::panel(1, 3, vec![1, 0]), &[6]),
        node(6, 6, chip(20.), &[]),
    ])
}

#[test]
fn a_preferred_only_edit_survives_a_refusal_without_publishing_early() {
    for (op, w, chip2, x, old_local, next_peer) in [
        (4, 100., 10., 42., 4., (48., 0.)), // capped, same line
        (5, 67., 19., 50., 3., (0., 16.)),  // floored, new line
    ] {
        let mut r = Raster::default();
        r.tree.upload(&content_bound(op, chip2)).unwrap();
        let old = r.draw(w, 40., 1.).unwrap().to_vec();
        let old_hit = Some((3, old_local, 6.));
        assert_eq!(r.hit_at(x, 6.), old_hit);
        r.tree.patch(&set(5, 6, chip(20.))).unwrap();
        let pending = r.tree.encode();
        for field in [
            vec![1, 4, 1, 5, 1, 0], // nested content bound
            vec![1, 4, 4, 1, 1, 0], // content is a folded expression, not a leaf
            vec![1, 4, 1, 5, 0, 0], // two fills
            vec![2, 4, 1, 0],       // height
        ] {
            assert!(
                r.tree
                    .patch(&set(2, 2, block(&[field])))
                    .unwrap_err()
                    .contains("WrappedRow child")
            );
            assert_eq!(r.tree.encode(), pending);
        }
        assert!(r.tree.layers.worksets_empty());
        let err = r
            .prepare(w, 40., 1.)
            .unwrap()
            .submit(|_, _| Err("refused handoff".into()))
            .unwrap_err();
        assert_eq!(err, "refused handoff");
        assert!(ran(&r, Flow, 1) && !ran(&r, Measure, 3));
        let peer = bounds(&r, 3);
        assert_eq!((peer.x, peer.y), next_peer);
        assert_eq!(r.pixels(), old);
        assert_eq!(r.hit_at(x, 6.), old_hit);
        r.draw(w, 40., 1.).unwrap();
        assert_eq!(r.hit_at(x, 6.), Some((2, f64::from(x), 6.)));
    }
}

#[test]
fn moving_a_closed_pixel_floor_changes_only_the_minimum_its_new_parent_reads() {
    // At 34, a 10px floor beside a 20px peer fits exactly. Insert an El
    // between the inner WrappedRow and the floored card: that El now offers
    // width independently and contributes the card's natural 20px minimum.
    let mut t = Tree::default();
    let fill = || block(&[vec![1, 0]]);
    let floor = || block(&[bounded::bound(1, 5, 10., None, true)]);
    t.upload(&upload(&[
        node(1, 2, block(&[vec![1, 0], number(4, 4.)]), &[2, 3]),
        node(2, 2, fill(), &[4]),
        node(4, 4, floor(), &[5]),
        node(5, 6, chip(20.), &[]),
        node(3, 4, fill(), &[6]),
        node(6, 6, chip(20.), &[]),
    ]))
    .unwrap();
    let interpose = patch(4, 7, &[node(7, 4, fill(), &[4])]);
    let promote = patch(7, 4, &[]);
    for (wire, aw, bx, by) in [
        (&[][..], 10., 14., 0.),
        (&interpose, 34., 0., 16.),
        (&promote, 10., 14., 0.),
    ] {
        if !wire.is_empty() {
            t.patch(wire).unwrap();
        }
        assert!(t.layers.worksets_empty());
        t.refresh(34., 60., 1.).unwrap();
        let f = |id| t.layers.frames[t.elements.ids[&id]];
        assert_eq!((f(2).width, f(3).x, f(3).y), (aw, bx, by));
        let mut cold = Tree::default();
        cold.upload(&t.encode()).unwrap();
        cold.refresh(34., 60., 1.).unwrap();
        for (id, frame) in cold.frames() {
            assert_eq!(f(id), frame);
        }
    }
}

#[test]
fn content_floor_measure_follows_the_final_parent_and_operator() {
    // The outer host reads the inner WrappedRow's minimum. Switching a
    // grandchild's operator, or moving it behind an El, changes that minimum
    // without changing any preferred width. Check untouched descendant bytes.
    let mut t = Tree::default();
    let fill = || block(&[vec![1, 0]]);
    let style = |width| block(&[width, number(4, 4.)]);
    let policy = |op| set(4, 2, style(vec![1, op, 1, 0]));
    t.upload(&upload(&[
        node(1, 2, style(vec![1, 0]), &[2, 3]),
        node(2, 2, fill(), &[4]),
        node(3, 4, fill(), &[5]),
        node(5, 6, chip(20.), &[]),
        node(4, 2, style(vec![1, 5, 1, 0]), &[6, 7]),
        node(6, 6, chip(20.), &[]),
        node(7, 6, chip(20.), &[]),
    ]))
    .unwrap();
    let interpose = || patch(4, 8, &[node(8, 4, fill(), &[4])]);
    let promote = || patch(8, 4, &[]);
    for (edits, bx, by) in [
        (vec![], 0., 16.),
        (vec![policy(4)], 35.5, 0.),
        (vec![policy(5)], 0., 16.),
        (vec![interpose()], 35.5, 0.),
        (vec![promote()], 0., 16.),
        (vec![interpose(), set(4, 2, style(vec![1, 0]))], 35.5, 0.),
        (vec![policy(5), promote()], 0., 16.),
    ] {
        for wire in edits {
            t.patch(&wire).unwrap();
        }
        assert!(t.layers.worksets_empty());
        t.refresh(67., 40., 1.).unwrap();
        let peer = t.layers.frames[t.elements.ids[&3]];
        assert_eq!((peer.x, peer.y), (bx, by));
        let mut cold = Tree::default();
        cold.upload(&t.encode()).unwrap();
        cold.refresh(67., 40., 1.).unwrap();
        for (id, frame) in cold.frames() {
            assert_eq!(t.layers.frames[t.elements.ids[&id]], frame);
        }
    }
}

#[test]
fn explicit_zero_changes_line_membership_at_equal_folded_bounds() {
    let mut r = Raster::default();
    let cap = || bounded::bound(1, 4, 30., None, false);
    let explicit = || nested::range(1, 0., 30., 1.);
    r.tree
        .upload(&upload(&[
            node(1, 2, block(&[px(1, 24.), number(4, 4.)]), &[2, 3]),
            node(2, 4, block(&[cap()]), &[4]),
            node(4, 6, chip(20.), &[]),
            node(3, 4, block(&[vec![1, 0]]), &[5]),
            node(5, 6, chip(20.), &[]),
        ]))
        .unwrap();
    for (width, aw, bx, by) in [
        (cap(), 24., 0., 16.),
        (explicit(), 0., 4., 0.),
        (cap(), 24., 0., 16.),
    ] {
        r.tree.patch(&set(2, 4, block(&[width]))).unwrap();
        draw(&mut r);
        let b = bounds(&r, 3);
        assert_eq!((bounds(&r, 2).width, b.x, b.y), (aw, bx, by));
    }
    let pending = r.tree.encode();
    // Masking content or a second fill with a zero cap does not make the
    // declaration a pixel bound on one fill. Equal output isn't admission.
    for mixed in [vec![4, 1, 0], vec![5, 1, 0], vec![5, 0, 0]] {
        let width = [vec![1, 4], number(2, 0.), mixed].concat();
        assert!(
            r.tree
                .patch(&set(2, 4, block(&[width])))
                .unwrap_err()
                .contains("WrappedRow child")
        );
        assert_eq!(r.tree.encode(), pending);
    }
}

#[test]
fn an_auto_column_can_consume_the_line_without_changing_its_measured_width() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 2, block(&[px(1, 100.), number(4, 4.)]), &[2, 3]),
            node(2, 3, block(&[]), &[4]),
            node(4, 4, block(&[]), &[5]),
            node(5, 6, chip(20.), &[]),
            node(3, 4, block(&[vec![1, 0]]), &[6]),
            node(6, 6, chip(20.), &[]),
        ]))
        .unwrap();
    for (id, kind, style, aw, bx, by, bw) in [
        (4, 4, block(&[]), 20., 24., 0., 76.),
        (4, 4, block(&[vec![1, 0]]), 100., 0., 16., 100.),
        (2, 3, block(&[vec![1, 1]]), 20., 24., 0., 76.),
        (2, 3, block(&[]), 100., 0., 16., 100.),
        (4, 4, block(&[]), 20., 24., 0., 76.),
    ] {
        r.tree.patch(&set(id, kind, style)).unwrap();
        draw(&mut r);
        let b = bounds(&r, 3);
        assert_eq!((bounds(&r, 2).width, b.x, b.y, b.width), (aw, bx, by, bw));
    }
}
