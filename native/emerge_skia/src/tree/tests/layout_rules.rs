use super::*;

#[test]
fn a_kept_child_receives_the_whole_width_at_an_existing_independent_site() {
    let host = |w| block(&[px(1, w), px(2, 12.)]);
    let fill = || block(&[vec![1, 0], px(2, 8.)]);
    for kind in [4, 3, 14] {
        // El, Column and WrappedColumn
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, 3, block(&[]), &[10, 20]),
                node(10, 1, host(32.), &[2, 3]),
                node(20, kind, host(32.), &[]),
                node(2, 4, fill(), &[4]),
                node(4, 6, host(8.), &[]),
                node(3, 6, fill(), &[]),
            ]))
            .unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 2).width, 16.);
        let move_to = |from_row| {
            let (row, other): (&[u64], &[u64]) = if from_row {
                (&[3], &[2])
            } else {
                (&[2, 3], &[])
            };
            patch(
                1,
                1,
                &[
                    edges(1, 3, &[10, 20]),
                    edges(10, 1, row),
                    edges(20, kind, other),
                ],
            )
        };
        r.tree.patch(&move_to(true)).unwrap(); // Neither parent's inner width changed.
        for w in [32., 0., 32.] {
            r.tree.patch(&set(20, kind, host(w))).unwrap();
            draw(&mut r);
            assert_eq!(bounds(&r, 2).width, w as f32); // Zero is an offer, not the 8px natural fit.
            check_fresh(&r);
        }
        r.tree.patch(&move_to(false)).unwrap();
        draw(&mut r);
        assert_eq!(bounds(&r, 2).width, 16.);
        check_fresh(&r);
    }
}

#[test]
fn asymmetric_padding_uses_the_host_or_the_wrapped_group_on_the_cross_axis() {
    let host = |right| {
        block(&[
            px(1, 24.),
            px(2, 16.),
            number(78, 1.5),
            [3, 2]
                .into_iter()
                .chain([1., right, 5., 7.].into_iter().flat_map(f64::to_be_bytes))
                .collect(),
        ])
    };
    for kind in [4, 1, 3, 2, 14] {
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host(3.), &[2]),
                node(
                    2,
                    4,
                    block(&[
                        px(1, 20.),
                        px(2, 12.),
                        vec![5, 2],
                        vec![6, 2],
                        color([0, 0, 255]),
                    ]),
                    &[],
                ),
            ]))
            .unwrap();
        for (right, end_x) in [(3., 1.5), (20., -19.5), (3., 1.5)] {
            r.tree.patch(&set(1, kind, host(right))).unwrap();
            draw(&mut r);
            let b = bounds(&r, 2);
            assert_eq!(b.x, if kind == 14 { 10.5 } else { end_x });
            assert_eq!(b.y, if kind == 2 { 1.5 } else { -1.5 });
            check_fresh(&r);
        }
    }
}

#[test]
fn packed_cross_extent_charges_only_between_group_maxima_in_device_order() {
    for (kind, main, cross) in [(2, 1, 2), (14, 2, 1)] {
        let item = |along, across| block(&[px(main, along), px(cross, across)]);
        let extent = |r: &Raster| {
            if cross == 1 {
                bounds(r, 1).width
            } else {
                bounds(r, 1).height
            }
        };
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(
                    1,
                    kind,
                    block(&[px(main, 12.), vec![cross, 1], number(4, 1.)]),
                    &[2, 3, 4],
                ),
                node(2, 6, item(4., 8.), &[]),
                node(3, 6, item(7., 32.), &[]),
                node(4, 6, item(12., 1.), &[]),
            ]))
            .unwrap();
        // 4 + 1 + 7 fits exactly. The cross extent is (max(8, h) + gap) + 1.
        // At 2^24 each addition of 1 rounds down; adding (gap + 1) would not.
        for (h, want) in [(32., 34.), (16_777_216., 16_777_216.), (32., 34.)] {
            r.tree.patch(&set(3, 6, item(7., h))).unwrap();
            draw(&mut r);
            assert_eq!(extent(&r), want);
            check_fresh(&r);
        }
        for (order_ids, want) in [(&[2][..], 8.), (&[][..], 0.)] {
            r.tree.patch(&order(1, kind, order_ids)).unwrap();
            draw(&mut r);
            assert_eq!(extent(&r), want);
            check_fresh(&r);
        }
    }
}
