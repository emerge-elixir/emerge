use super::*;

// EMRG length tag 1 = width (Row), tag 2 = height (Column).
pub(super) fn host(axis: u8) -> u8 {
    if axis == 1 { 1 } else { 3 }
}
// Keep both lanes at their original positions, but put them under a parent
// that cannot itself host a compound fill width or a fill height: a 90px
// WrappedRow stacks the width lanes; a natural WrappedRow puts the height
// lanes on a single line. Unlike Row, neither can offer a fill height.
pub(super) fn wrong_site(axis: u8) -> (u8, Bytes) {
    if axis == 1 {
        (2, block(&[px(1, 90.)]))
    } else {
        (2, block(&[]))
    }
}
pub(super) fn weight(axis: u8, n: f64) -> Bytes {
    [axis, 3].into_iter().chain(n.to_be_bytes()).collect()
}
pub(super) fn size(axis: u8, length: Bytes) -> Bytes {
    block(&[length, px(3 - axis, 12.)])
}
pub(super) fn panel(axis: u8, id: u64, length: Bytes) -> Bytes {
    block(&[
        length,
        px(3 - axis, 12.),
        color(if id == 2 { [255, 0, 0] } else { [0, 0, 255] }),
        vec![40, 1],
    ])
}
pub(super) fn edit(axis: u8, id: u64, n: f64) -> Bytes {
    set(id, 4, panel(axis, id, weight(axis, n)))
}
pub(super) fn pair(axis: u8, second: Bytes) -> Bytes {
    upload(&[
        node(1, host(axis), size(axis, px(axis, 90.)), &[2, 3]),
        node(2, 4, panel(axis, 2, vec![axis, 0]), &[4]),
        node(3, 4, panel(axis, 3, second), &[]),
        node(4, 5, text("Pay"), &[]),
    ])
}
pub(super) fn span(r: &Raster, axis: u8, id: u64) -> (f32, f32) {
    let f = bounds(r, id);
    if axis == 1 {
        (f.x, f.width)
    } else {
        (f.y, f.height)
    }
}

#[test]
fn weight_only_edits_invalidate_the_parent_split_not_intrinsic_boxes() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        r.tree.upload(&pair(axis, vec![axis, 0])).unwrap();
        let equal = draw(&mut r).to_vec();
        assert_eq!(span(&r, axis, 3), (45., 45.));
        r.tree.patch(&edit(axis, 3, 1.)).unwrap(); // fill() → fill(1)
        assert_eq!(draw(&mut r), equal);
        assert!(r.tree.layers.trace.is_empty());
        r.tree.patch(&edit(axis, 3, 2.)).unwrap();
        assert!(r.tree.layers.worksets_empty());
        assert_ne!(draw(&mut r), equal);
        assert_eq!(span(&r, axis, 3), (30., 60.));
        assert!(!r.tree.layers.trace.iter().any(|(p, _, _)| *p == Measure));
        assert!(ran(&r, if axis == 1 { Flow } else { Place }, 1));
        if axis == 2 {
            assert!(
                !r.tree
                    .layers
                    .trace
                    .iter()
                    .any(|(p, _, _)| matches!(p, Flow | Reflow))
            );
        }
        r.tree.patch(&edit(axis, 3, 3.)).unwrap();
        r.tree.patch(&edit(axis, 3, 2.)).unwrap();
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
        r.tree
            .patch(&set(1, host(axis), size(axis, vec![axis, 1])))
            .unwrap();
        draw(&mut r);
        let natural = span(&r, axis, 4).1;
        assert!(natural > 0.);
        assert_eq!(span(&r, axis, 2), (0., natural));
        assert_eq!(span(&r, axis, 3), (natural, 0.));
        r.tree.patch(&edit(axis, 2, 7.)).unwrap();
        draw(&mut r);
        assert!(
            r.tree.layers.trace.is_empty(),
            "content host doesn't read weights"
        );
    }
}

#[test]
fn weights_keep_device_edge_rounding_and_extreme_finite_ratios_stay_finite() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        r.tree.upload(&pair(axis, vec![axis, 0])).unwrap();
        let tiny = f64::from_bits(1);
        let large = f64::from(f32::MAX);
        for (a, b, first, second) in [
            (0.25, 0.75, 22.5, 67.5),
            (tiny, tiny, 45., 45.),
            (tiny, 2. * tiny, 30., 60.),
            (f64::MAX, f64::MAX, 45., 45.),
            (large, large, 45., 45.),
            (f64::MAX, tiny, 90., 0.),
            (tiny, f64::MAX, 0., 90.),
        ] {
            r.tree.patch(&edit(axis, 2, a)).unwrap();
            r.tree.patch(&edit(axis, 3, b)).unwrap();
            draw(&mut r);
            assert_eq!(span(&r, axis, 2), (0., first));
            assert_eq!(span(&r, axis, 3), (first, second));
        }
        let doc = upload(&[
            node(1, host(axis), size(axis, px(axis, 101.)), &[2, 3, 5]),
            node(2, 4, size(axis, weight(axis, 0.1)), &[]),
            node(3, 4, size(axis, weight(axis, 1.1)), &[]),
            node(5, 4, size(axis, weight(axis, 2.7)), &[]),
        ]);
        r.tree.patch(&at(doc, 1)).unwrap();
        draw(&mut r);
        let unit = 101f32 / (0.1f32 + 1.1 + 2.7);
        assert_eq!(span(&r, axis, 3), (unit * 0.1, unit * 1.1));
    }
}

#[test]
fn nested_weighted_columns_charge_the_reflowed_nonfill_sibling_once() {
    let root = |w| block(&[px(1, w), weight(2, 7.), number(4, 4.)]);
    let chip = || block(&[px(1, 48.), px(2, 20.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, root(100.), &[2, 3, 4]),
            node(2, 2, block(&[vec![1, 0], number(4, 4.)]), &[5, 6]),
            node(5, 4, chip(), &[]),
            node(6, 4, chip(), &[]),
            node(3, 4, size(2, vec![2, 0]), &[]),
            node(
                4,
                3,
                block(&[vec![1, 0], weight(2, 3.), number(78, 2.)]),
                &[7, 8],
            ),
            node(7, 4, size(2, weight(2, 1.)), &[]),
            node(8, 4, size(2, weight(2, 2.)), &[]),
        ]))
        .unwrap();
    for (w, h, wrap, unit) in [
        (100., 100., 20., 18.),
        (80., 100., 44., 12.),
        (80., 40., 44., 0.),
        (100., 100., 20., 18.),
    ] {
        r.tree.patch(&set(1, 3, root(w))).unwrap();
        r.draw(100., h, 1.).unwrap();
        assert_eq!(span(&r, 2, 3), (wrap + 4., unit));
        let y = wrap + 8. + unit;
        assert_eq!(span(&r, 2, 4), (y, 3. * unit));
        assert_eq!(span(&r, 2, 7), (y, unit));
        assert_eq!(span(&r, 2, 8), (y + unit, 2. * unit));
    }
}

#[test]
fn rejection_preserves_pending_weights_and_closed_moves_use_the_new_hosts_split() {
    for axis in [1, 2] {
        let line = host(axis);
        let (cross, root) = wrong_site(axis);
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, cross, root, &[2, 3]),
                node(2, line, size(axis, px(axis, 90.)), &[10, 20]),
                node(3, line, size(axis, px(axis, 60.)), &[30]),
                node(10, 4, panel(axis, 10, vec![axis, 0]), &[11]),
                node(11, 5, text("Pay"), &[]),
                node(20, 4, size(axis, vec![axis, 0]), &[]),
                node(30, 4, size(axis, vec![axis, 0]), &[]),
            ]))
            .unwrap();
        let old = draw(&mut r).to_vec();
        let pressed = r.target_at(5., 5.).unwrap().0;
        // A bare numeric width is legal on a wrapped line now. Include a real
        // (nonbinding here) pixel cap for the unsupported width-site checks.
        let policy = |w| {
            if axis == 1 {
                bounded::bound(1, 4, 300., Some(w), false)
            } else {
                weight(2, w)
            }
        };
        r.tree
            .patch(&set(10, 4, panel(axis, 10, policy(3.))))
            .unwrap();
        let pending = r.tree.encode();
        for w in [0., -0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                r.tree.patch(&edit(axis, 10, w)).unwrap_err(),
                "fill weight must be finite and positive"
            );
        }
        assert_eq!(
            r.tree
                .patch(&set(10, 4, block(&[vec![axis, 3, 0]])))
                .unwrap_err(),
            "truncated EMRG data"
        );
        // The kept branch has accepted a weighted policy, even though the last
        // draw used fill(). It can't move directly under the wrapping host,
        // as a definition with :same or a bare ref. Promotion is legal.
        for bad in [
            patch(2, 10, &[]),
            patch(1, 90, &[node(90, 2, block(&[]), &[10])]),
            patch(
                1,
                1,
                &[
                    edges(1, cross, &[2, 3, 10]),
                    edges(2, line, &[20]),
                    edges(10, 4, &[11]),
                ],
            ),
            patch(3, 9, &[node(9, 4, size(axis, policy(1.)), &[])]),
        ] {
            assert!(r.tree.patch(&bad).unwrap_err().contains("WrappedRow child"));
        }
        assert_eq!(r.tree.encode(), pending);
        assert!(r.tree.layers.worksets_empty());
        assert_eq!(r.pixels(), old);
        assert_eq!(r.target_at(5., 5.).unwrap().0, pressed);
        r.tree
            .patch(&patch(
                1,
                1,
                &[
                    edges(1, cross, &[2, 3]),
                    edges(2, line, &[20]),
                    edges(3, line, &[30, 10]),
                ],
            ))
            .unwrap();
        let err = r
            .prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused split".into()))
            .unwrap_err();
        assert_eq!(err, "refused split");
        assert_eq!(r.pixels(), old);
        assert_eq!(r.target_at(5., 5.).unwrap().0, pressed);
        assert_eq!(span(&r, axis, 10), (15., 45.));
        draw(&mut r);
        let (x, y) = if axis == 1 { (20., 17.) } else { (17., 20.) };
        assert_eq!(r.target_at(x, y), Some((pressed, 10, 5., 5.)));
        assert!(r.target_at(5., 5.).is_none());
        assert_eq!(r.tree.layers.text.shapes, 1);
        r.tree.patch(&patch(1, 10, &[])).unwrap();
        draw(&mut r);
        assert_eq!(
            span(&r, axis, 10),
            (0., if axis == 1 { 180. } else { 170. })
        );
        assert_eq!(r.target_at(5., 5.), Some((pressed, 10, 5., 5.)));
    }
}
