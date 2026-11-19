use super::{
    weighted::{host, panel, size, span, weight},
    *,
};

pub(super) fn bound(axis: u8, tag: u8, px: f64, w: Option<f64>, reverse: bool) -> Bytes {
    let px = number(2, px);
    let fill = w.map_or_else(|| vec![0], |w| number(3, w));
    let operands = if reverse { [fill, px] } else { [px, fill] };
    [axis, tag]
        .into_iter()
        .chain(operands.into_iter().flatten())
        .collect()
}

#[test]
fn cap_only_edits_move_the_peer_even_at_equal_intrinsic_and_weight() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let attrs = |n, w, rev| panel(axis, 3, bound(axis, 4, n, w, rev));
        let doc = |a| {
            upload(&[
                node(1, host(axis), size(axis, px(axis, 120.)), &[3, 2]),
                node(3, 4, a, &[]),
                node(2, 4, panel(axis, 2, vec![axis, 0]), &[4]),
                node(4, 5, text("Pay"), &[]),
            ])
        };
        r.tree.upload(&doc(attrs(20., None, false))).unwrap();
        let old = draw(&mut r).to_vec();
        let (x, y) = if axis == 1 { (25., 5.) } else { (5., 25.) };
        let pressed = r.target_at(x, y).unwrap();
        assert_eq!((pressed.1, pressed.2, pressed.3), (2, 5., 5.));
        let edit = |n, w, rev| set(3, 4, attrs(n, w, rev));
        r.tree.patch(&edit(20., Some(1.), true)).unwrap();
        assert_eq!(
            r.tree.encode(),
            doc(attrs(20., Some(1.), true)),
            "preserve spelling and operand order"
        );
        assert_eq!(draw(&mut r), old);
        assert!(r.tree.layers.trace.is_empty());
        r.tree.patch(&edit(30., None, false)).unwrap();
        assert!(r.tree.layers.worksets_empty());
        let err = r
            .prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused cap edit".into()))
            .unwrap_err();
        assert_eq!(err, "refused cap edit");
        assert_eq!(span(&r, axis, 2), (30., 90.));
        assert!(!ran(&r, Measure, 2) && !ran(&r, Measure, 4));
        assert_eq!(r.pixels(), old);
        assert_eq!(r.target_at(x, y), Some(pressed));
        assert_ne!(draw(&mut r), old);
        assert_eq!(r.target_at(x, y).unwrap().1, 3);
        for p in [edit(40., Some(2.), true), edit(30., None, false)] {
            r.tree.patch(&p).unwrap();
        }
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
        for w in [f64::MAX, f64::from_bits(1)] {
            r.tree.patch(&edit(20., Some(w), false)).unwrap();
            r.tree
                .patch(&set(2, 4, panel(axis, 2, weight(axis, w))))
                .unwrap();
            draw(&mut r);
            assert_eq!(span(&r, axis, 2), (20., 100.));
        }
        assert_eq!(r.tree.layers.text.shapes, 1);
    }
}

#[test]
fn the_parent_reads_a_childs_scaled_bound_even_when_both_measured_extents_stay_zero() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let a = |s| block(&[bound(axis, 4, 12., Some(3.), false), number(78, s)]);
        r.tree
            .upload(&upload(&[
                node(1, host(axis), size(axis, px(axis, 120.)), &[2, 3]),
                node(2, 6, a(1.), &[]),
                node(3, 4, panel(axis, 3, vec![axis, 0]), &[]),
            ]))
            .unwrap();
        for (s, extent) in [(1., 12.), (1.5, 18.), (0.5, 6.), (1., 12.)] {
            r.tree.patch(&set(2, 6, a(s))).unwrap();
            draw(&mut r);
            assert_eq!(span(&r, axis, 3), (extent, 120. - extent));
            if s != 1. {
                assert!(
                    !ran(&r, Measure, 1),
                    "not propagated via a changed intrinsic box"
                );
            }
        }
    }
}

#[test]
fn proportional_fill_redistributes_across_multiple_bounds_in_either_order() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let cap = |n, w| bound(axis, 4, n, Some(w), false);
        let floor = |n, w| bound(axis, 5, n, Some(w), true);
        let fill = |w| weight(axis, w);
        let tiny = f64::from_bits(1);
        let huge = f64::MAX;
        // Later limits may bind only after an earlier one does. Conversely,
        // a provisional floor and cap must not both freeze if one can relax.
        for (lengths, expected) in [
            ([cap(10., 1.), cap(45., 1.), fill(1.)], [10., 45., 65.]),
            ([cap(20., 2.), fill(1.), fill(3.)], [20., 25., 75.]),
            ([floor(50., 1.), cap(10., 1.), fill(1.)], [55., 10., 55.]),
            ([cap(36., 1.), floor(60., 1.), fill(1.)], [30., 60., 30.]),
            ([cap(10., 1.), cap(20., 2.), cap(30., 3.)], [10., 20., 30.]),
            ([floor(80., 0.5), floor(90., 1.5), fill(1.)], [80., 90., 0.]),
            (
                [cap(10., huge), fill(tiny), fill(3. * tiny)],
                [10., 27.5, 82.5],
            ),
            (
                [floor(20., tiny), cap(40., huge), fill(tiny)],
                [40., 40., 40.],
            ),
            ([cap(0., huge), cap(5., tiny), fill(tiny)], [0., 5., 115.]),
        ] {
            for ids in [[2, 3, 4], [4, 3, 2]] {
                let mut doc = vec![node(1, host(axis), size(axis, px(axis, 120.)), &ids)];
                doc.extend(
                    lengths
                        .iter()
                        .enumerate()
                        .map(|(i, l)| node(i as u64 + 2, 4, size(axis, l.clone()), &[])),
                );
                let bytes = upload(&doc);
                if r.tree.root.is_none() {
                    r.tree.upload(&bytes).unwrap();
                } else {
                    r.tree.patch(&at(bytes, 1)).unwrap();
                }
                r.tree.refresh(180., 170., 1.).unwrap();
                let mut origin = 0.;
                for id in ids {
                    let extent = expected[id as usize - 2];
                    assert_eq!(
                        span(&r, axis, id),
                        (origin, extent),
                        "axis {axis}: {expected:?}"
                    );
                    origin += extent;
                }
            }
        }
    }
}

#[test]
fn bounded_roots_nested_hosts_and_content_bases_use_their_own_extent() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let root = |length| size(axis, length);
        r.tree
            .upload(&upload(&[
                node(
                    1,
                    host(axis),
                    root(bound(axis, 4, 80., Some(7.), true)),
                    &[2, 3],
                ),
                node(
                    2,
                    host(axis),
                    size(axis, bound(axis, 4, 20., Some(3.), false)),
                    &[4, 5],
                ),
                node(3, 4, size(axis, vec![axis, 0]), &[6]),
                node(4, 4, size(axis, vec![axis, 0]), &[7]),
                node(5, 4, size(axis, weight(axis, 3.)), &[8]),
                node(6, 6, size(axis, px(axis, 12.)), &[]),
                node(7, 6, size(axis, px(axis, 10.)), &[]),
                node(8, 6, size(axis, px(axis, 30.)), &[]),
            ]))
            .unwrap();
        for (policy, viewport, outer, a, b) in [
            (bound(axis, 4, 80., Some(7.), true), 120., 80., 20., 60.),
            (bound(axis, 4, 80., Some(7.), true), 16., 16., 12., 4.),
            (bound(axis, 5, 140., Some(7.), false), 120., 140., 20., 120.),
            (vec![axis, 1], 120., 32., 20., 12.),
        ] {
            r.tree.patch(&set(1, host(axis), root(policy))).unwrap();
            r.tree.refresh(viewport, viewport, 1.).unwrap();
            assert_eq!(span(&r, axis, 1), (0., outer));
            assert_eq!(span(&r, axis, 2), (0., a));
            assert_eq!(span(&r, axis, 3), (a, b));
            assert_eq!(span(&r, axis, 4), (0., a / 4.));
            assert_eq!(span(&r, axis, 5), (a / 4., 3. * a / 4.));
        }
    }
}

#[test]
fn a_floor_can_exceed_its_share_even_after_a_nonfill_sibling_wraps_or_uses_all_the_room() {
    let mut r = Raster::default();
    let root = |w| block(&[px(1, w), vec![2, 0], number(4, 4.)]);
    let chip = || block(&[px(1, 48.), px(2, 20.)]);
    r.tree
        .upload(&upload(&[
            node(1, 3, root(100.), &[2, 3, 4]),
            node(2, 2, block(&[vec![1, 0], number(4, 4.)]), &[5, 6]),
            node(5, 4, chip(), &[]),
            node(6, 4, chip(), &[]),
            node(3, 4, panel(2, 3, bound(2, 5, 18., None, true)), &[]),
            node(4, 4, panel(2, 4, weight(2, 3.)), &[]),
        ]))
        .unwrap();
    for (w, h, wrap, b) in [
        (100., 100., 20., 54.),
        (80., 100., 44., 30.),
        (80., 40., 44., 0.),
        (100., 100., 20., 54.),
    ] {
        r.tree.patch(&set(1, 3, root(w))).unwrap();
        r.tree.refresh(100., h, 1.).unwrap();
        assert_eq!(span(&r, 2, 2), (0., wrap));
        assert_eq!(span(&r, 2, 3), (wrap + 4., 18.));
        assert_eq!(span(&r, 2, 4), (wrap + 26., b));
    }
}

#[test]
fn admission_proves_the_bounded_leaf_at_its_final_site_before_publishing_anything() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let line = host(axis);
        let cross = host(3 - axis);
        r.tree
            .upload(&upload(&[
                node(1, cross, block(&[]), &[2, 3]),
                node(2, line, size(axis, px(axis, 90.)), &[10, 20]),
                node(
                    3,
                    line,
                    block(&[px(axis, 30.), px(3 - axis, 12.), number(78, 2.)]),
                    &[30],
                ),
                node(10, 4, panel(axis, 10, vec![axis, 0]), &[11]),
                node(11, 5, text("Pay"), &[]),
                node(20, 4, size(axis, vec![axis, 0]), &[]),
                node(30, 4, size(axis, vec![axis, 0]), &[]),
            ]))
            .unwrap();
        let old = draw(&mut r).to_vec();
        let pressed = r.target_at(5., 5.).unwrap().0;
        let attrs = |l| set(10, 4, panel(axis, 10, l));
        r.tree
            .patch(&attrs(bound(axis, 4, 2e38, Some(3.), false)))
            .unwrap();
        let pending = r.tree.encode();
        let transfer = patch(
            1,
            1,
            &[
                edges(1, cross, &[2, 3]),
                edges(2, line, &[20]),
                edges(3, line, &[30, 10]),
            ],
        );
        assert_eq!(
            r.tree.patch(&transfer).unwrap_err(),
            "scaled length exceeds device range"
        );
        assert_eq!(
            r.tree.refresh(180., 170., 2.).unwrap_err(),
            "scaled length exceeds device range"
        );
        for p in [
            patch(1, 10, &[]),
            patch(2, 10, &[]),
            patch(
                3,
                31,
                &[node(31, 4, size(axis, bound(axis, 5, 5., None, true)), &[])],
            ),
        ] {
            assert!(r.tree.patch(&p).unwrap_err().contains("requires a root"));
        }
        for (l, err) in [
            (vec![axis, 4, 0, 6], "unsupported length 6"),
            (vec![axis, 5, 0, 255], "unsupported length 255"),
            (vec![axis, 4, 5, 0, 0], "truncated EMRG data"),
            (vec![axis, 5, 0], "truncated EMRG data"),
            (
                bound(axis, 4, f64::NAN, None, false),
                "expected finite non-negative size",
            ),
            (
                bound(axis, 5, 20., Some(0.), true),
                "fill weight must be finite and positive",
            ),
        ] {
            // Put the malformed value last: a missing operand cannot read
            // the next attribute as a length tag.
            assert_eq!(r.tree.patch(&set(10, 4, block(&[l]))).unwrap_err(), err);
        }
        assert_eq!(r.tree.encode(), pending);
        assert!(r.tree.layers.worksets_empty());
        assert_eq!(r.pixels(), old);
        assert_eq!(r.target_at(5., 5.).unwrap().0, pressed);
        r.tree
            .patch(&attrs(bound(axis, 4, 10., Some(3.), true)))
            .unwrap();
        r.tree.patch(&transfer).unwrap();
        draw(&mut r);
        assert_eq!(span(&r, axis, 10), (40., 20.));
        let (x, y) = if axis == 1 { (45., 17.) } else { (17., 45.) };
        assert_eq!(r.target_at(x, y), Some((pressed, 10, 2.5, 2.5)));
    }
}
