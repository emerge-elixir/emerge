use super::{weighted::panel, *};

pub(super) fn mixed(op: u8) -> Bytes {
    [vec![2, op, 1], number(3, 3.)].concat()
}
fn host(h: f64) -> Bytes {
    block(&[
        px(1, 40.),
        px(2, h),
        [3, 0].into_iter().chain(6f64.to_be_bytes()).collect(),
        number(4, 4.),
        vec![40, 1],
    ])
}
pub(super) fn line() -> Bytes {
    upload(&[
        node(1, 1, host(120.), &[2, 3]),
        node(2, 4, panel(2, 2, mixed(4)), &[4]),
        node(4, 6, block(&[px(2, 30.)]), &[]),
        node(3, 4, panel(2, 3, vec![2, 0]), &[5]),
        node(5, 5, text("i"), &[]),
    ])
}

#[test]
fn policy_only_edits_reach_the_row_height_offer_not_intrinsic_measure_or_width_sharing() {
    let mut r = Raster::default();
    r.tree.upload(&line()).unwrap();
    draw(&mut r);
    let heights = |r: &Raster| [2, 3].map(|id| bounds(r, id).height);
    assert_eq!(heights(&r), [30., 108.]);
    let floor = set(2, 4, panel(2, 2, mixed(5)));
    r.tree.patch(&floor).unwrap();
    assert!(r.tree.layers.worksets_empty());
    draw(&mut r);
    assert_eq!(heights(&r), [108., 108.]);
    assert!(!ran(&r, Place, 1) && ran(&r, Place, 2) && !ran(&r, Place, 3));
    assert!(
        !r.tree
            .layers
            .trace
            .iter()
            .any(|(p, _, _)| matches!(p, Measure | Flow | Reflow))
    );
    r.tree.patch(&set(4, 6, block(&[px(2, 150.)]))).unwrap();
    draw(&mut r);
    assert_eq!(heights(&r), [150., 108.]);
    assert!(!ran(&r, Measure, 5));
    for w in [1., f64::from_bits(1), f64::MAX] {
        r.tree
            .patch(&set(3, 4, panel(2, 3, weighted::weight(2, w))))
            .unwrap();
        draw(&mut r);
        assert!(r.tree.layers.trace.is_empty());
    }
    r.tree.patch(&set(1, 1, host(10.))).unwrap();
    draw(&mut r);
    assert_eq!(heights(&r), [150., 0.]);
    r.tree.patch(&set(2, 4, panel(2, 2, mixed(4)))).unwrap();
    draw(&mut r);
    assert_eq!(heights(&r), [0., 0.]);
    r.tree.patch(&set(1, 1, host(120.))).unwrap();
    draw(&mut r);
    assert_eq!(heights(&r), [108., 108.]);
}

#[test]
fn a_closed_move_switches_between_pre_and_post_wrap_at_identical_width_and_scale() {
    closed_move(1);
}
pub(super) fn closed_move(independent_host: u8) {
    let mut r = Raster::default();
    let wrap = |masked| {
        block(&[
            px(1, 76.),
            [vec![2, 4, 4, 0], number(2, 90.), number(2, masked)].concat(),
            number(4, 4.),
            color([255, 0, 0]),
            vec![40, 1],
        ])
    };
    let chip = || block(&[px(1, 38.), px(2, 12.)]);
    r.tree
        .upload(&upload(&[
            node(
                1,
                3,
                block(&[px(1, 100.), number(4, 4.)]),
                &[10, 20, 30, 40],
            ),
            node(10, 3, block(&[px(1, 80.), vec![2, 1]]), &[2]),
            node(2, 2, wrap(120.), &[3, 4]),
            node(3, 6, chip(), &[]),
            node(4, 6, chip(), &[]),
            node(20, independent_host, block(&[px(1, 80.), vec![2, 1]]), &[]),
            node(
                30,
                independent_host,
                block(&[px(1, 40.), number(78, 2.)]),
                &[],
            ),
            node(40, 2, block(&[number(78, 2.)]), &[]),
        ]))
        .unwrap();
    let old = draw(&mut r).to_vec();
    let target = r.target_at(5., 5.).unwrap();
    let heights = |r: &Raster| [10, 20, 2].map(|id| bounds(r, id).height);
    assert_eq!(heights(&r), [12., 0., 12.]);
    let transfer = |dest, kind| {
        patch(
            1,
            1,
            &[
                edges(1, 3, &[10, 20, 30, 40]),
                edges(10, 3, &[]),
                edges(dest, kind, &[2]),
            ],
        )
    };
    r.tree.patch(&set(2, 2, wrap(2e38))).unwrap();
    let pending = r.tree.encode();
    assert_eq!(
        r.tree.patch(&transfer(40, 2)).unwrap_err(),
        "scaled length exceeds device range"
    );
    assert_eq!(
        r.tree.patch(&transfer(30, independent_host)).unwrap_err(),
        "scaled length exceeds device range"
    );
    assert_eq!(r.tree.encode(), pending);
    assert!(r.tree.layers.worksets_empty());
    r.tree.patch(&transfer(20, independent_host)).unwrap();
    assert_eq!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("refused height handoff".into()))
            .unwrap_err(),
        "refused height handoff"
    );
    assert_eq!(heights(&r), [0., 28., 28.]);
    assert_eq!(bounds(&r, 4).y, 20.);
    assert_eq!(r.target_at(5., 5.), Some(target));
    assert!(r.target_at(5., 25.).is_none());
    assert_eq!(r.pixels(), old);
    draw(&mut r);
    assert_eq!(r.target_at(5., 25.), Some((target.0, 2, 5., 21.)));
    r.tree
        .patch(&patch(
            1,
            1,
            &[
                edges(1, 3, &[10, 20, 30, 40]),
                edges(10, 3, &[2]),
                edges(20, independent_host, &[]),
            ],
        ))
        .unwrap();
    assert_eq!(draw(&mut r), old);
    assert_eq!(heights(&r), [12., 0., 12.]);
    assert_eq!(r.target_at(5., 5.), Some(target));
}

#[test]
fn changing_site_and_policy_before_one_refresh_uses_the_first_height_basis() {
    edit_and_move(1);
}
pub(super) fn edit_and_move(independent_host: u8) {
    let wrap = |independent| {
        let mut cap = vec![2, 4];
        cap.extend(number(2, 90.));
        cap.extend(if independent { &[4, 1, 0][..] } else { &[0] });
        block(&[
            px(1, 76.),
            cap,
            number(4, 4.),
            color([255, 0, 0]),
            vec![40, 1],
        ])
    };
    let kids = |yes| if yes { &[2][..] } else { &[] };
    let full = |independent: bool| {
        upload(&[
            node(1, 3, block(&[px(1, 76.)]), &[10, 20]),
            node(10, 3, block(&[px(1, 76.), vec![2, 1]]), kids(!independent)),
            node(
                20,
                independent_host,
                block(&[px(1, 76.), vec![2, 1]]),
                kids(independent),
            ),
            node(2, 2, wrap(independent), &[3, 4]),
            node(3, 6, block(&[px(1, 38.), px(2, 12.)]), &[]),
            node(4, 6, block(&[px(1, 38.), px(2, 12.)]), &[]),
        ])
    };
    // The two policies have exactly the same natural bounds (0..90). Only
    // the old Column used pre-wrap 12; the independent host needs post-wrap 28.
    for edit_first in [false, true] {
        let mut r = Raster::default();
        r.tree.upload(&full(false)).unwrap();
        for independent in [true, false] {
            draw(&mut r);
            let edits = [
                set(2, 2, wrap(independent)),
                patch(
                    1,
                    1,
                    &[
                        edges(1, 3, &[10, 20]),
                        edges(10, 3, kids(!independent)),
                        edges(20, independent_host, kids(independent)),
                    ],
                ),
            ];
            for i in if edit_first { [0, 1] } else { [1, 0] } {
                r.tree.patch(&edits[i]).unwrap();
            }
            assert!(r.tree.layers.worksets_empty());
            let mut cold = Raster::default();
            cold.tree.upload(&full(independent)).unwrap();
            draw(&mut r);
            draw(&mut cold);
            assert_eq!(bounds(&r, 2).height, if independent { 28. } else { 12. });
            assert_eq!(r.pixels(), cold.pixels());
            assert_eq!(r.hit_at(5., 20.), cold.hit_at(5., 20.));
        }
    }
}

#[test]
fn nested_columns_report_demand_up_and_subdivide_only_the_placed_height_down() {
    let mut r = Raster::default();
    let wrap = || block(&[vec![1, 0], number(4, 4.)]);
    let row = |w, policy: Option<Bytes>| {
        let mut fields = vec![px(1, w)];
        fields.extend(policy);
        block(&fields)
    };
    // A: width fill, height fill Column. Fixed wrapped header followed by
    // a capped fill child. B: 12px wide, 20px tall. The natural Row needs
    // the Column's unallotted height, not its previous placed height.
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[px(1, 120.), px(2, 80.)]), &[9, 10]),
            node(9, 6, block(&[px(2, 8.)]), &[]),
            node(10, 1, row(92., Some(vec![2, 0])), &[2, 3]),
            node(2, 3, block(&[vec![1, 0], vec![2, 0]]), &[4, 5]),
            node(4, 2, wrap(), &[6, 7]),
            node(6, 6, block(&[px(1, 38.), px(2, 12.)]), &[]),
            node(7, 6, block(&[px(1, 38.), px(2, 12.)]), &[]),
            node(5, 4, block(&[mixed(4)]), &[8]),
            node(8, 6, block(&[px(2, 6.)]), &[]),
            node(3, 6, block(&[px(1, 12.), px(2, 20.)]), &[]),
        ]))
        .unwrap();
    let check = |r: &Raster, h, header, capped| {
        assert_eq!(
            [10, 2, 4, 5, 3].map(|id| bounds(r, id).height),
            [h, h, header, capped, 20.]
        );
        assert_eq!(bounds(r, 5).y, 8. + header);
    };
    draw(&mut r);
    check(&r, 72., 12., 6.);
    for natural in [None, Some(vec![2, 1])] {
        r.tree
            .patch(&set(10, 1, row(92., natural.clone())))
            .unwrap();
        draw(&mut r);
        check(&r, 20., 12., 6.);
        r.tree
            .patch(&set(10, 1, row(91., natural.clone())))
            .unwrap();
        draw(&mut r);
        check(&r, 34., 28., 6.);
        // Only a nested chip width changes; Column width and measured height stay equal.
        r.tree
            .patch(&set(6, 6, block(&[px(1, 36.), px(2, 12.)])))
            .unwrap();
        draw(&mut r);
        check(&r, 20., 12., 6.);
        r.tree
            .patch(&set(6, 6, block(&[px(1, 38.), px(2, 12.)])))
            .unwrap();
        draw(&mut r);
        check(&r, 34., 28., 6.);
    }
    r.tree
        .patch(&set(10, 1, row(91., Some(vec![2, 0]))))
        .unwrap();
    r.tree.patch(&set(9, 6, block(&[px(2, 80.)]))).unwrap();
    draw(&mut r);
    assert_eq!(
        [10, 2, 4, 5].map(|id| bounds(&r, id).height),
        [0., 0., 28., 0.]
    );
    r.tree.patch(&set(9, 6, block(&[px(2, 8.)]))).unwrap();
    draw(&mut r);
    check(&r, 72., 28., 6.);
}
