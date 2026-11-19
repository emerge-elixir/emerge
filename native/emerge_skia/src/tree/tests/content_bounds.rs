use super::{
    weighted::{host, panel, size, span},
    *,
};

fn op(tag: u8, a: Bytes, b: Bytes) -> Bytes {
    [tag].into_iter().chain(a).chain(b).collect()
}
fn field(axis: u8, l: Bytes) -> Bytes {
    [axis].into_iter().chain(l).collect()
}
fn p(n: f64) -> Bytes {
    number(2, n)
}
pub(super) fn bound(axis: u8, tag: u8, n: f64) -> Bytes {
    field(axis, op(tag, p(n), vec![1]))
}
pub(super) fn doc(axis: u8, a: Bytes, demand: f64) -> Bytes {
    upload(&[
        node(1, host(axis), size(axis, px(axis, 240.)), &[2, 3]),
        node(2, 4, panel(axis, 2, a), &[4]),
        node(4, 6, size(axis, px(axis, demand)), &[]),
        node(3, 4, panel(axis, 3, vec![axis, 0]), &[5]),
        node(5, 5, text("Pay"), &[]),
    ])
}

#[test]
fn only_a_changed_bounded_extent_reaches_the_peer_and_only_submission_changes_the_hit() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let floor = || bound(axis, 5, 140.);
        r.tree.upload(&doc(axis, floor(), 80.)).unwrap();
        let visible = draw(&mut r).to_vec();
        assert_eq!(span(&r, axis, 2), (0., 140.));
        assert_eq!(span(&r, axis, 3), (140., 100.));
        let (x, y) = if axis == 1 { (160., 5.) } else { (5., 160.) };
        let pressed = r.target_at(x, y).unwrap();
        assert_eq!(pressed.1, 3);
        let demand = |n| set(4, 6, size(axis, px(axis, n)));
        r.tree.patch(&demand(90.)).unwrap();
        assert_eq!(draw(&mut r), visible);
        assert!(!ran(&r, Measure, 1));
        assert!(!ran(&r, if axis == 1 { Flow } else { Place }, 1));
        let same = field(axis, op(5, op(5, vec![1], p(50.)), p(140.)));
        r.tree
            .patch(&set(2, 4, panel(axis, 2, same.clone())))
            .unwrap();
        assert_eq!(r.tree.encode(), doc(axis, same, 90.));
        assert_eq!(draw(&mut r), visible);
        assert!(r.tree.layers.trace.is_empty());
        r.tree.patch(&demand(180.)).unwrap();
        assert!(r.tree.layers.worksets_empty());
        let why = r
            .prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused bounded content".into()))
            .unwrap_err();
        assert_eq!(why, "refused bounded content");
        assert_eq!(span(&r, axis, 3), (180., 60.));
        assert!(!ran(&r, Measure, 5));
        assert_eq!(r.pixels(), visible);
        assert_eq!(r.target_at(x, y), Some(pressed));
        assert_ne!(draw(&mut r), visible);
        assert_eq!(r.target_at(x, y).unwrap().1, 2);
        r.tree
            .patch(&set(2, 4, panel(axis, 2, bound(axis, 4, 100.))))
            .unwrap();
        r.tree.patch(&set(2, 4, panel(axis, 2, floor()))).unwrap();
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
        assert_eq!(r.tree.layers.text.shapes, 1);
    }
}

#[test]
fn intrinsic_host_bounds_do_not_grant_viewport_offers_or_turn_its_children_into_a_definite_split() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let line = host(axis);
        r.tree
            .upload(&upload(&[
                node(1, line, size(axis, bound(axis, 5, 140.)), &[2, 3]),
                node(2, 4, size(axis, vec![axis, 0]), &[4]),
                node(4, 6, size(axis, px(axis, 80.)), &[]),
                node(3, 4, size(axis, weighted::weight(axis, 3.)), &[5]),
                node(5, 6, size(axis, px(axis, 10.)), &[]),
            ]))
            .unwrap();
        let resize = |r: &mut Raster, l, extent| {
            r.tree.patch(&set(1, line, size(axis, l))).unwrap();
            for viewport in [250., 50.] {
                r.tree.refresh(viewport, viewport, 1.).unwrap();
                assert_eq!(span(r, axis, 1), (0., extent));
                assert_eq!(span(r, axis, 2), (0., 80.));
                assert_eq!(span(r, axis, 3), (80., 10.));
            }
        };
        resize(&mut r, bound(axis, 5, 140.), 140.);
        resize(&mut r, bound(axis, 4, 60.), 60.);
        resize(
            &mut r,
            field(axis, op(5, p(50.), op(4, p(20.), vec![1]))),
            50.,
        );
        resize(&mut r, vec![axis, 1], 90.);
        r.tree
            .patch(&set(1, line, size(axis, bound(axis, 5, 0.))))
            .unwrap();
        r.tree.refresh(50., 50., 1.).unwrap();
        assert!(r.tree.layers.trace.is_empty());
    }
}

#[test]
fn a_closed_content_box_can_move_under_an_el_but_its_masked_pixel_must_fit_the_new_scale() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let root = host(3 - axis);
        let a = |cap, masked| {
            panel(
                axis,
                10,
                field(axis, op(4, op(4, p(masked), p(cap)), op(5, vec![1], p(8.)))),
            )
        };
        r.tree
            .upload(&upload(&[
                node(1, root, block(&[]), &[2, 3]),
                node(2, 4, block(&[]), &[10]),
                node(10, 4, a(10., 30.), &[11]),
                node(11, 6, size(axis, px(axis, 50.)), &[]),
                node(3, 4, block(&[number(78, 2.)]), &[]),
            ]))
            .unwrap();
        let screen = draw(&mut r).to_vec();
        let target = r.target_at(5., 5.).unwrap();
        assert_eq!(target.1, 10);
        r.tree.patch(&set(10, 4, a(20.5, 2e38))).unwrap();
        let pending = r.tree.encode();
        let moved = patch(
            1,
            1,
            &[
                edges(1, root, &[2, 3]),
                edges(2, 4, &[]),
                edges(3, 4, &[10]),
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
        assert_eq!(r.tree.encode(), pending);
        assert_eq!(r.pixels(), screen);
        assert_eq!(r.target_at(5., 5.), Some(target));
        draw(&mut r);
        assert_eq!(span(&r, axis, 10).1, 20.5);
        r.tree.patch(&set(10, 4, a(20.5, 30.))).unwrap();
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
        r.tree.patch(&moved).unwrap();
        draw(&mut r);
        assert_eq!(span(&r, axis, 10).1, 41.);
        assert_eq!(r.target_at(5., 5.), Some((target.0, 10, 2.5, 2.5)));
        r.tree.patch(&patch(1, 10, &[])).unwrap();
        draw(&mut r);
        assert_eq!(span(&r, axis, 10), (0., 20.5));
        assert_eq!(r.target_at(5., 5.), Some(target));
    }
}
