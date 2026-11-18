use super::{
    weighted::{host, panel, size, span},
    *,
};

fn op(tag: u8, a: Bytes, b: Bytes) -> Bytes {
    [tag].into_iter().chain(a).chain(b).collect()
}
fn field(axis: u8, expr: Bytes) -> Bytes {
    [axis].into_iter().chain(expr).collect()
}
fn p(n: f64) -> Bytes {
    number(2, n)
}
pub(super) fn range(axis: u8, floor: f64, cap: f64, w: f64) -> Bytes {
    field(axis, op(4, p(cap), op(5, p(floor), number(3, w))))
}
pub(super) fn deep(axis: u8, depth: usize, right: bool) -> Bytes {
    field(
        axis,
        (0..depth).fold(vec![0], |l, i| {
            if right {
                op(4, p(20. + i as f64), l)
            } else {
                op(4, l, p(20. + i as f64))
            }
        }),
    )
}

#[test]
fn equivalent_trees_preserve_wire_bytes_and_do_not_invalidate_but_a_new_split_waits_for_submit() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let edit = |v| set(3, 4, panel(axis, 3, v));
        r.tree
            .upload(&weighted::pair(axis, range(axis, 20., 50., 3.)))
            .unwrap();
        let old = draw(&mut r).to_vec();
        assert_eq!(span(&r, axis, 3), (40., 50.));
        // Change operand order, association and a masked constant, not the function.
        let equiv = field(
            axis,
            op(5, op(4, number(3, 3.), op(4, p(50.), p(700.))), p(20.)),
        );
        r.tree.patch(&edit(equiv.clone())).unwrap();
        assert_eq!(
            r.tree.elements.raw(r.tree.elements.ids[&3]),
            panel(axis, 3, equiv.clone())
        );
        let mut cold = Raster::default();
        cold.tree.upload(&weighted::pair(axis, equiv)).unwrap();
        assert_eq!(r.tree.encode(), cold.tree.encode());
        assert_eq!(draw(&mut r), old);
        assert!(r.tree.layers.trace.is_empty());
        let (x, y) = if axis == 1 { (42., 5.) } else { (5., 42.) };
        assert_eq!(r.target_at(x, y).unwrap().1, 3);
        r.tree.patch(&edit(range(axis, 20., 50., 1.))).unwrap();
        assert!(r.tree.layers.worksets_empty());
        let e = r
            .prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused nested split".into()))
            .unwrap_err();
        assert_eq!(e, "refused nested split");
        assert_eq!(span(&r, axis, 3), (45., 45.));
        assert!(!r.tree.layers.trace.iter().any(|(p, _, _)| *p == Measure));
        assert_eq!(r.pixels(), old);
        assert_eq!(r.target_at(x, y).unwrap().1, 3);
        assert_ne!(draw(&mut r), old);
        assert_eq!(r.target_at(x, y).unwrap().1, 2);
        // Outside operation wins for crossing bounds, even when the available
        // space would have placed the raw offer below both or above both.
        for (expr, a, b) in [
            (range(axis, 80., 20., 2.), 70., 20.),
            (field(axis, op(5, p(80.), op(4, p(20.), vec![0]))), 10., 80.),
        ] {
            r.tree.patch(&edit(expr)).unwrap();
            draw(&mut r);
            assert_eq!(span(&r, axis, 2), (0., a));
            assert_eq!(span(&r, axis, 3), (a, b));
        }
        r.tree
            .patch(&edit(field(
                axis,
                op(4, op(5, number(3, f64::MAX), p(80.)), p(80.)),
            )))
            .unwrap();
        draw(&mut r);
        assert!(
            r.tree.layers.trace.is_empty(),
            "weight hidden by a constant"
        );
    }
}

#[test]
fn nested_root_content_and_scaled_child_resolve_the_same_interval_at_different_bases() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let root = |v| size(axis, v);
        let child = |s| block(&[range(axis, 10., 20.5, 3.), number(78, s)]);
        let policy = || range(axis, 20., 70., 2.);
        r.tree
            .upload(&upload(&[
                node(1, host(axis), root(policy()), &[2, 3]),
                node(2, host(axis), child(1.), &[4, 5]),
                node(3, 4, size(axis, vec![axis, 0]), &[6]),
                node(4, 4, size(axis, vec![axis, 0]), &[]),
                node(5, 4, size(axis, vec![axis, 0]), &[]),
                node(6, 6, size(axis, px(axis, 12.)), &[]),
            ]))
            .unwrap();
        for (l, viewport, scale, outer, a, b) in [
            (policy(), 100., 1., 70., 20.5, 49.5),
            (policy(), 100., 1.5, 70., 30.75, 39.25),
            (policy(), 4., 1.5, 20., 15., 5.),
            // No viewport share: intrinsic zero is floored at 15; B needs 12.
            (vec![axis, 1], 100., 1.5, 27., 15., 12.),
            (policy(), 100., 1., 70., 20.5, 49.5),
        ] {
            r.tree.patch(&set(1, host(axis), root(l))).unwrap();
            r.tree.patch(&set(2, host(axis), child(scale))).unwrap();
            r.tree.refresh(viewport, viewport, 1.).unwrap();
            assert_eq!(span(&r, axis, 1), (0., outer));
            assert_eq!(span(&r, axis, 2), (0., a));
            assert_eq!(span(&r, axis, 3), (a, b));
            assert_eq!(span(&r, axis, 5), (a / 2., a / 2.));
        }
    }
}

#[test]
fn masked_pixels_are_still_checked_at_the_final_site_and_fail_atomically() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let line = host(axis);
        let cross = host(3 - axis);
        r.tree
            .upload(&upload(&[
                node(1, cross, block(&[]), &[2, 3]),
                node(2, line, size(axis, px(axis, 90.)), &[10, 20]),
                node(3, line, block(&[px(axis, 60.), number(78, 2.)]), &[]),
                node(10, 4, panel(axis, 10, vec![axis, 0]), &[]),
                node(20, 4, panel(axis, 2, vec![axis, 0]), &[]),
            ]))
            .unwrap();
        let visible = draw(&mut r).to_vec();
        let (x, y) = if axis == 1 { (30., 5.) } else { (5., 30.) };
        let target = r.target_at(x, y).unwrap();
        assert_eq!(target.1, 10);
        let set_cap = |masked| {
            set(
                10,
                4,
                panel(
                    axis,
                    10,
                    field(axis, op(4, vec![0], op(4, p(20.), p(masked)))),
                ),
            )
        };
        r.tree.patch(&set_cap(2e38)).unwrap();
        let pending = r.tree.encode();
        let moved = patch(
            1,
            1,
            &[
                edges(1, cross, &[2, 3]),
                edges(2, line, &[20]),
                edges(3, line, &[10]),
            ],
        );
        assert_eq!(
            r.tree.patch(&moved).unwrap_err(),
            "scaled length exceeds device range"
        );
        assert_eq!(
            r.tree.refresh(180., 170., 2.).unwrap_err(),
            "scaled length exceeds device range"
        );
        assert!(
            r.tree
                .patch(&patch(1, 10, &[]))
                .unwrap_err()
                .contains("requires a root")
        );
        assert_eq!(r.tree.encode(), pending);
        assert_eq!(r.pixels(), visible);
        assert_eq!(r.target_at(x, y), Some(target));
        draw(&mut r);
        assert_eq!(r.target_at(x, y).unwrap().1, 20);
        // Replacing only the masked leaf must change admission, not schedule layout.
        r.tree.patch(&set_cap(100.)).unwrap();
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
        r.tree.patch(&moved).unwrap();
        draw(&mut r);
        assert_eq!(span(&r, axis, 10).1, 40.);
    }
}

#[test]
fn prefix_depth_and_bad_nested_operands_reject_without_publishing() {
    let mut r = Raster::default();
    let axis = 1;
    let edit = |l| set(3, 4, block(&[l]));
    r.tree
        .upload(&weighted::pair(axis, deep(axis, 64, true)))
        .unwrap();
    r.tree.patch(&edit(deep(axis, 64, false))).unwrap();
    let pending = r.tree.encode();
    for (l, why) in [
        (deep(axis, 65, true), "length expression depth exceeds 64"),
        (deep(axis, 65, false), "length expression depth exceeds 64"),
        (
            field(
                axis,
                op(4, op(5, p(8.), vec![0]), op(4, p(9.), number(3, 3.))),
            ),
            "unsupported length 4",
        ),
        (
            field(axis, op(4, op(5, p(8.), vec![1]), vec![0])),
            "unsupported length 5",
        ),
        (
            field(axis, op(4, op(5, p(8.), p(9.)), p(7.))),
            "unsupported length 4",
        ),
        (
            field(axis, op(5, op(4, p(f64::NAN), p(4.)), vec![0])),
            "expected finite non-negative size",
        ),
        (
            field(axis, op(4, p(3.), op(5, p(1.), number(3, 0.)))),
            "fill weight must be finite and positive",
        ),
        (
            field(axis, op(5, p(8.), op(4, p(9.), vec![2, 0, 0]))),
            "truncated EMRG data",
        ),
        (field(axis, op(5, p(8.), vec![4, 0])), "truncated EMRG data"),
    ] {
        assert_eq!(r.tree.patch(&edit(l)).unwrap_err(), why);
    }
    assert_eq!(r.tree.encode(), pending);
    draw(&mut r);
    assert_eq!(span(&r, axis, 3), (70., 20.));
}
