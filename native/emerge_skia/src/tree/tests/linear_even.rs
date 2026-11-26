use super::*;

fn host(axis: u8, length: Bytes, gap: f64, flag: u8) -> Bytes {
    block(&[length, px(3 - axis, 32.), number(4, gap), vec![37, flag]])
}
fn axes(r: &Raster, id: u64, axis: u8) -> (f32, f32, f32) {
    let f = bounds(r, id);
    if axis == 1 {
        (f.x, f.y, f.width)
    } else {
        (f.y, f.x, f.height)
    }
}

#[test]
fn wrapping_charges_the_authored_break_gap_but_not_the_even_share_gap() {
    for (kind, axis) in [(2, 1), (14, 2)] {
        let root = |size, gap, flag| host(axis, px(axis, size), gap, flag);
        let fill = block(&[vec![axis, 0], px(3 - axis, 8.)]);
        let basis = block(&[px(axis, 20.)]);
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, root(44., 4., 0), &[2, 3]),
                node(2, 4, fill.clone(), &[4]),
                node(3, 4, fill, &[5]),
                node(4, 6, basis.clone(), &[]),
                node(5, 6, basis, &[]),
            ]))
            .unwrap();
        for (size, gap, flag, first, second) in [
            (44., 4., 0, 20., (24., 0., 20.)),
            (44., 4., 1, 22., (22., 0., 22.)),
            (44., 5., 1, 44., (0., 13., 44.)),
            (44., 4., 1, 22., (22., 0., 22.)),
            (43., 4., 1, 43., (0., 12., 43.)),
            (44., 4., 0, 20., (24., 0., 20.)),
        ] {
            r.tree.patch(&set(1, kind, root(size, gap, flag))).unwrap();
            draw(&mut r);
            assert_eq!(axes(&r, 2, axis), (0., 0., first));
            assert_eq!(axes(&r, 3, axis), second);
            check_fresh(&r);
        }
    }
}

#[test]
fn only_the_active_main_axis_ignores_child_alignment_even_at_the_same_extent() {
    for (kind, axis, wrapped) in [(1, 1, false), (3, 2, false), (2, 1, true), (14, 2, true)] {
        let fixed = px(axis, 100.);
        let floor = [vec![axis, 5, 1], number(2, 100.)].concat();
        let item = |cross, main_align, cross_align| {
            block(&[
                px(axis, 20.),
                px(3 - axis, cross),
                vec![4 + axis, main_align],
                vec![7 - axis, cross_align],
            ])
        };
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host(axis, fixed.clone(), 0., 1), &[2, 3, 4]),
                node(2, 6, item(8., 2, 0), &[]),
                node(3, 6, item(20., 0, 0), &[]),
                node(4, 6, item(20., 0, 0), &[]),
            ]))
            .unwrap();
        draw(&mut r);
        assert_eq!(axes(&r, 3, axis).0, 40.);
        r.tree.patch(&set(2, 6, item(8., 1, 0))).unwrap();
        draw(&mut r);
        assert!(!ran(&r, Place, 1));
        r.tree.patch(&set(2, 6, item(8., 2, 2))).unwrap();
        draw(&mut r);
        let cross = if wrapped { 12. } else { 24. };
        assert_eq!(axes(&r, 2, axis), (0., cross, 20.));
        for (l, flag, positions) in [
            (floor, 1, [80., 0., 20.]),
            (fixed.clone(), 1, [0., 40., 80.]),
            (fixed, 0, [80., 0., 20.]),
        ] {
            r.tree
                .patch(&set(1, kind, host(axis, l, 0., flag)))
                .unwrap();
            draw(&mut r);
            assert_eq!([2, 3, 4].map(|id| axes(&r, id, axis).0), positions);
            assert_eq!(axes(&r, 2, axis).1, cross);
            check_fresh(&r);
        }
        let pending = r.tree.encode();
        assert_eq!(
            r.tree
                .patch(&set(1, kind, host(axis, px(axis, 100.), 0., 2)))
                .unwrap_err(),
            "invalid space_evenly flag 2"
        );
        assert_eq!(r.tree.encode(), pending);
    }
}

#[test]
fn even_groups_use_the_same_forward_step_rounding_on_both_axes() {
    // After the large card, adding extent=1 and gap=1 separately would lose both.
    for (kind, axis) in [(1, 1), (3, 2), (2, 1), (14, 2)] {
        let mut r = Raster::default();
        let root = host(axis, px(axis, 16_777_218.), 0., 1);
        let nodes = std::iter::once(node(1, kind, root, &[2, 3, 4]))
            .chain([16_777_216., 1., 0.].into_iter().enumerate().map(|(i, n)| {
                node(
                    i as u64 + 2,
                    6,
                    block(&[px(axis, n), px(3 - axis, 1.)]),
                    &[],
                )
            }))
            .collect::<Vec<_>>();
        r.tree.upload(&upload(&nodes)).unwrap();
        draw(&mut r);
        assert_eq!(axes(&r, 4, axis).0, 16_777_218., "kind {kind}");
    }
}

#[test]
fn the_trailing_inset_moves_unaligned_children_in_every_even_group() {
    for (kind, axis) in [(1, 1), (3, 2), (2, 1), (14, 2)] {
        let root = |end, flag| {
            let mut insets = [4f64; 4];
            insets[usize::from(axis)] = end; // right or bottom
            block(&[
                px(axis, 100.),
                px(3 - axis, 32.),
                number(4, 4.),
                [3, 2]
                    .into_iter()
                    .chain(insets.into_iter().flat_map(f64::to_be_bytes))
                    .collect(),
                vec![37, flag],
            ])
        };
        let nodes = std::iter::once(node(1, kind, root(4., 0), &[2, 3, 4]))
            .chain((2..=4).map(|id| node(id, 6, block(&[px(axis, 20.), px(3 - axis, 8.)]), &[])))
            .collect::<Vec<_>>();
        let mut r = Raster::default();
        r.tree.upload(&upload(&nodes)).unwrap();
        draw(&mut r);
        for (end, mode, pos) in [
            (4., 1, [4., 40., 76.]),
            (10., 1, [4., 37., 70.]),
            (4., 1, [4., 40., 76.]),
            (4., 0, [4., 28., 52.]),
        ] {
            r.tree.patch(&set(1, kind, root(end, mode))).unwrap();
            draw(&mut r);
            assert_eq!([2, 3, 4].map(|id| axes(&r, id, axis).0), pos);
            if axis == 2 {
                assert!(!ran(&r, Flow, 1) && !ran(&r, Context, 1));
            }
            check_fresh(&r);
        }
    }
}

#[test]
fn failed_wrapped_handoff_keeps_the_old_line_origin_and_install_uses_the_sealed_one() {
    for (kind, axis) in [(2, 1), (14, 2)] {
        let root = |flag| host(axis, px(axis, 60.), 4., flag);
        let card = block(&[
            px(axis, 20.),
            px(3 - axis, 8.),
            color([255, 0, 0]),
            vec![40, 1],
        ]);
        let bytes = upload(
            &std::iter::once(node(1, kind, root(0), &[2, 3, 4, 5, 6]))
                .chain((2..=6).map(|id| node(id, 4, card.clone(), &[])))
                .collect::<Vec<_>>(),
        );
        let mut r = Raster::default();
        r.tree.upload(&bytes).unwrap();
        let a = draw(&mut r).to_vec();
        let point = if axis == 1 { (42., 14.) } else { (14., 42.) };
        let hit = |r: &Raster| r.hit_at(point.0, point.1);
        let old = if axis == 1 {
            Some((5, 18., 2.))
        } else {
            Some((5, 2., 18.))
        };
        assert_eq!(hit(&r), old);
        r.tree.patch(&set(1, kind, root(1))).unwrap();
        assert!(r.tree.layers.worksets_empty());
        assert!(
            r.prepare(180., 170., 1.)
                .unwrap()
                .submit(|_, _| Err("handoff".into()))
                .is_err()
        );
        assert_eq!(r.pixels(), a);
        assert_eq!(hit(&r), old);
        r.settle(180., 170., 1.).unwrap();
        r.tree.patch(&set(1, kind, root(0))).unwrap();
        r.install();
        assert_eq!(hit(&r), Some((5, 2., 2.)));
        assert_eq!(draw(&mut r), a);
        assert_eq!(hit(&r), old);
    }
}

pub(super) fn outer(flag: u8) -> Bytes {
    set(
        2,
        3,
        block(&[vec![1, 1], px(2, 60.), number(4, 1.), vec![37, flag]]),
    )
}
#[test]
fn early_and_final_column_shares_use_the_same_charge_before_the_peer_flows() {
    for flag in [0, 1] {
        check_early_share(flag);
    }
}
fn check_early_share(inner_flag: u8) {
    let mut r = Raster::default();
    r.tree.upload(&wrapped_column::shared_column()).unwrap();
    r.tree
        .patch(&set(
            4,
            14,
            block(&[
                vec![1, 1],
                vec![2, 0],
                [
                    vec![36],
                    6f64.to_be_bytes().to_vec(),
                    4f64.to_be_bytes().to_vec(),
                ]
                .concat(),
                vec![37, inner_flag],
            ]),
        ))
        .unwrap();
    for (flag, height, width, y, peer) in [
        (0, 43., 30., 17., 20.),
        (1, 44., 12., 16., 8.),
        (0, 43., 30., 17., 20.),
    ] {
        let n = r.tree.layers.invalidation_passes;
        for edit in [outer(1 - flag), outer(flag)] {
            r.tree.patch(&edit).unwrap();
        }
        assert!(r.tree.layers.worksets_empty());
        draw(&mut r);
        assert_eq!(r.tree.layers.invalidation_passes, n + 1);
        assert_eq!(axes(&r, 4, 2), (y, 0., height));
        assert_eq!(bounds(&r, 2).width, width);
        assert_eq!(
            (bounds(&r, 6).x, bounds(&r, 6).width, bounds(&r, 6).height),
            (width, 64. - width, peer)
        );
        check_fresh(&r);
    }
    r.tree.patch(&set(3, 4, block(&[vec![2, 1]]))).unwrap();
    draw(&mut r);
    assert_eq!((bounds(&r, 4).width, bounds(&r, 6).x), (12., 12.));
    check_fresh(&r);
}
