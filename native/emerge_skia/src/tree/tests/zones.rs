//! A zone changes placement, not the child order, its settled size or wrap membership.
use super::*;

fn card(axis: u8, extent: f64, zone: u8, rgb: [u8; 3]) -> Bytes {
    block(&[
        px(axis, extent),
        px(3 - axis, 20.),
        vec![axis + 4, zone],
        color(rgb),
        vec![40, 1],
    ])
}
fn host(axis: u8, room: f64, right: f64) -> Bytes {
    let padding = [4., right, 4., 4.];
    block(&[
        px(axis, room),
        px(3 - axis, 100.),
        number(4, 4.),
        [3, 2]
            .into_iter()
            .chain(padding.into_iter().flat_map(f64::to_be_bytes))
            .collect(),
        vec![40, 1],
    ])
}
fn xy(r: &Raster, id: u64, axis: u8) -> [f32; 2] {
    let b = bounds(r, id);
    if axis == 1 { [b.x, b.y] } else { [b.y, b.x] }
}
#[test]
fn end_zone_is_anchored_as_a_group_in_declaration_order() {
    for (kind, axis) in [(1, 1), (3, 2)] {
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host(axis, 100., 4.), &[2, 3, 4]),
                node(2, 4, card(axis, 20., 0, [255, 0, 0]), &[]),
                node(3, 4, card(axis, 20., 0, [0, 0, 255]), &[]),
                node(4, 4, card(axis, 20., 0, [0, 255, 0]), &[]),
            ]))
            .unwrap();
        draw(&mut r);
        for (zone, origins) in [(2, [52., 4., 76.]), (0, [4., 28., 52.])] {
            let n = r.tree.layers.invalidation_passes;
            r.tree
                .patch(&set(2, 4, card(axis, 20., zone, [255, 0, 0])))
                .unwrap();
            r.tree
                .patch(&set(4, 4, card(axis, 20., zone, [0, 255, 0])))
                .unwrap();
            assert!(r.tree.layers.worksets_empty());
            draw(&mut r);
            assert_eq!(r.tree.layers.invalidation_passes, n + 1);
            assert!(
                r.tree
                    .layers
                    .trace
                    .iter()
                    .all(|(p, _, _)| !matches!(p, Measure | Flow | Reflow))
            );
            assert_eq!([2, 3, 4].map(|id| xy(&r, id, axis)[0]), origins);
            check_fresh(&r);
        }
    }
}
#[test]
fn centers_share_the_remaining_interval_and_clamp_at_the_end_of_start() {
    for (axis, kind) in [(1, 1), (2, 3)] {
        let attr = |v, z| card(axis, v, z, [0, 0, 255]);
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host(axis, 100., 4.), &[2, 3, 4, 5]),
                node(2, 4, attr(36., 0), &[]),
                node(3, 4, attr(4., 1), &[]),
                node(4, 4, attr(20., 2), &[]),
                node(5, 4, attr(4., 1), &[]),
            ]))
            .unwrap();
        for (extent, z, at) in [
            (36., 0, [4., 52., 76., 60.]),
            (20., 0, [4., 44., 76., 52.]),
            (21., 0, [4., 44.5, 76., 52.5]),
            (75., 0, [4., 79., 76., 87.]),
            (20., 1, [22., 46., 76., 54.]),
            (36., 0, [4., 52., 76., 60.]),
        ] {
            r.tree.patch(&set(2, 4, attr(extent, z))).unwrap();
            draw(&mut r);
            assert_eq!([2, 3, 4, 5].map(|id| xy(&r, id, axis)[0]), at);
            check_fresh(&r);
        }
    }
}
#[test]
fn a_content_column_anchors_its_end_after_width_reflow_and_successful_submission() {
    let host = |w| block(&[px(1, w), number(4, 4.)]);
    let chip = || block(&[px(1, 20.), px(2, 8.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 3, host(44.), &[2, 5]),
            node(2, 2, block(&[vec![1, 0], number(4, 4.)]), &[3, 4]),
            node(3, 6, chip(), &[]),
            node(4, 6, chip(), &[]),
            node(5, 4, card(2, 12., 2, [255, 0, 0]), &[]),
        ]))
        .unwrap();
    let a = draw(&mut r).to_vec();
    assert_eq!((bounds(&r, 1).height, bounds(&r, 5).y), (24., 12.));
    r.tree.patch(&set(1, 3, host(43.))).unwrap();
    let pending = r.tree.encode();
    assert_eq!(
        r.tree
            .patch(&patch(
                1,
                5,
                &[attr_record(5, 4, card(2, 12., 3, [255, 0, 0]))]
            ))
            .unwrap_err(),
        "invalid alignment 3"
    );
    assert_eq!(r.tree.encode(), pending);
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("handoff".into()))
            .is_err()
    );
    assert_eq!(r.pixels(), a);
    assert_eq!(r.hit_at(5., 15.), Some((5, 5., 3.)));
    draw(&mut r);
    assert_eq!((bounds(&r, 1).height, bounds(&r, 5).y), (36., 24.));
    assert_eq!(r.hit_at(5., 15.), None);
    assert_eq!(r.hit_at(5., 27.), Some((5, 5., 3.)));
    check_fresh(&r);
    r.tree.patch(&patch(1, 5, &[])).unwrap();
    draw(&mut r);
    assert_eq!(r.hit_at(5., 3.), Some((5, 5., 3.)));
    assert_eq!(r.hit_at(5., 27.), None);
}
#[test]
fn a_center_follows_the_resolved_width_of_an_unedited_packed_sibling() {
    let mut r = Raster::default();
    r.tree.upload(&wrapped_column::shared()).unwrap();
    r.tree
        .patch(&set(5, 2, block(&[px(1, 8.), number(4, 4.), vec![5, 1]])))
        .unwrap();
    for (h, packed, center) in [(44., 12., 34.), (43., 30., 43.), (44., 12., 34.)] {
        r.tree
            .patch(&set(1, 1, block(&[px(1, 64.), px(2, h)])))
            .unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 2).width, bounds(&r, 5).x), (packed, center));
        check_fresh(&r);
    }
}
#[test]
fn installing_settled_zones_does_not_publish_a_later_accepted_edit() {
    for kind in [1, 4] {
        // Row zone or El child alignment
        check_alignment_install(kind);
    }
}
fn check_alignment_install(kind: u8) {
    let mut r = Raster::default();
    let attrs = |z| card(1, 20., z, [255, 0, 0]);
    r.tree
        .upload(&upload(&[
            node(1, kind, host(1, 100., 4.), &[2]),
            node(2, 4, attrs(0), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let target = |r: &Raster, x| r.hit_at(x, 8.).unwrap().0;
    r.tree.patch(&set(2, 4, attrs(1))).unwrap();
    // Window workers settle/record B, then accept C without computing it
    // while waiting to submit B. Hits still belong to A until install.
    assert!(r.settle(180., 170., 1.).unwrap());
    r.tree.patch(&set(2, 4, attrs(2))).unwrap();
    let pending = r.tree.encode();
    assert!(
        r.tree
            .patch(&set(2, 4, attrs(3)))
            .unwrap_err()
            .contains("invalid alignment")
    );
    assert_eq!(r.tree.encode(), pending);
    assert_eq!([8., 44., 80.].map(|x| target(&r, x)), [2, 1, 1]);
    r.install();
    assert_eq!([8., 44., 80.].map(|x| target(&r, x)), [1, 2, 1]);
    draw(&mut r);
    assert_eq!([8., 44., 80.].map(|x| target(&r, x)), [1, 1, 2]);
    check_fresh(&r);
}
#[test]
fn changed_trailing_inset_at_equal_fit_moves_a_row_end_member() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, host(1, 100., 4.), &[2]),
            node(2, 4, card(1, 20., 2, [255, 0, 0]), &[]),
        ]))
        .unwrap();
    for (pad, x) in [(4., 76.), (10., 70.), (4., 76.)] {
        r.tree.patch(&set(1, 1, host(1, 100., pad))).unwrap();
        draw(&mut r);
        assert_eq!(xy(&r, 2, 1), [x, 4.]);
        check_fresh(&r);
    }
}
#[test]
fn centers_use_each_wrapped_groups_own_ends_and_cross_extent() {
    // First group has two centers; second has two end members interleaved
    // with center/start. Only declaration order inside a zone affects its origin.
    for (kind, axis) in [(2, 1), (14, 2)] {
        let (gx, gy): (f64, f64) = if axis == 1 { (4., 2.) } else { (2., 4.) };
        let host = block(&[
            px(axis, 64.),
            px(3 - axis, 100.),
            [
                vec![36],
                gx.to_be_bytes().to_vec(),
                gy.to_be_bytes().to_vec(),
            ]
            .concat(),
        ]);
        let item = |id, m, h, z, cross| {
            node(
                id,
                4,
                block(&[
                    px(axis, m),
                    px(3 - axis, h),
                    vec![4 + axis, z],
                    vec![7 - axis, cross],
                ]),
                &[],
            )
        };
        let ids = [2, 3, 4, 5, 6, 7, 8, 9];
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host, &ids),
                item(2, 24., 14., 0, 0),
                item(3, 8., 6., 1, 2),
                item(4, 8., 8., 1, 0),
                item(5, 8., 10., 2, 0),
                item(6, 18., 10., 2, 0),
                item(7, 12., 6., 1, 2),
                item(8, 8., 16., 0, 0),
                item(9, 8., 8., 2, 0),
            ]))
            .unwrap();
        for (order_ids, b, c) in [(ids, 30., 42.), ([2, 4, 3, 5, 6, 7, 8, 9], 42., 30.)] {
            r.tree.patch(&order(1, kind, &order_ids)).unwrap();
            draw(&mut r);
            assert_eq!(
                ids.map(|id| xy(&r, id, axis)),
                [
                    [0., 0.],
                    [b, 8.],
                    [c, 0.],
                    [56., 0.],
                    [34., 16.],
                    [15., 26.],
                    [0., 16.],
                    [56., 16.]
                ]
            );
            check_fresh(&r);
        }
    }
}
#[test]
fn centered_siblings_use_the_settled_weighted_shares_in_either_direction() {
    for (axis, kind) in [(1, 2), (2, 14)] {
        let host = block(&[px(axis, 100.), px(3 - axis, 20.), number(4, 4.)]);
        let fill = |w, z| {
            block(&[
                weighted::weight(axis, w),
                px(3 - axis, 8.),
                vec![axis + 4, z],
            ])
        };
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host, &[2, 3, 4]),
                node(2, 4, fill(3., 2), &[]),
                node(3, 4, fill(1., 1), &[]),
                node(4, 4, card(axis, 20., 0, [255, 0, 0]), &[]),
            ]))
            .unwrap();
        for (a, b, start_center, start_end) in
            [(3., 1., 24., 46.), (1., 3., 24., 82.), (3., 1., 24., 46.)]
        {
            r.tree.patch(&set(2, 4, fill(a, 2))).unwrap();
            r.tree.patch(&set(3, 4, fill(b, 1))).unwrap();
            draw(&mut r);
            assert_eq!(weighted::span(&r, axis, 2), (start_end, 18. * a as f32));
            assert_eq!(weighted::span(&r, axis, 3), (start_center, 18. * b as f32));
            assert_eq!(weighted::span(&r, axis, 4), (0., 20.));
            check_fresh(&r);
        }
    }
}
#[test]
fn a_center_begins_at_the_actual_last_start_edge_when_there_is_no_space() {
    // At this magnitude f32 rounds `pos + (width + gap)` and `(pos + gap) + width` differently.
    let mut r = Tree::default();
    let k = |id, w, z| node(id, 6, block(&[px(1, w), vec![5, z]]), &[]);
    r.upload(&upload(&[
        node(1, 1, block(&[px(1, 32.), number(4, 1.)]), &[2, 3, 4, 5]),
        k(2, 16_777_216., 0),
        k(3, 1., 0),
        k(4, 1., 0),
        k(5, 3., 1),
    ]))
    .unwrap();
    r.refresh(32., 32., 1.).unwrap();
    let b = |id| r.layers.frames[r.elements.ids[&id]];
    assert_eq!(b(5).x, b(4).x + b(4).width);
}
#[test]
fn an_aligned_capped_fill_stays_in_its_measured_group_and_keeps_its_share() {
    for (kind, axis) in [(2, 1), (14, 2)] {
        let host = |room| {
            block(&[
                px(axis, room),
                px(3 - axis, 40.),
                number(4, 4.),
                [vec![3, 0], 2f64.to_be_bytes().to_vec()].concat(),
            ])
        };
        let b = |zone| {
            block(&[
                [vec![axis, 4, 0], number(2, 12.)].concat(),
                px(3 - axis, 8.),
                vec![axis + 4, zone],
            ])
        };
        let fixed = |v| block(&[px(axis, v), px(3 - axis, 8.)]);
        let mut r = Raster::default();
        r.tree
            .upload(&upload(&[
                node(1, kind, host(80.), &[2, 3, 4]),
                node(2, 4, fixed(20.), &[]),
                node(3, 4, b(0), &[]),
                node(4, 4, fixed(28.), &[]),
            ]))
            .unwrap();
        for (room, zone, b_at, c_at) in [
            (80., 0, 26., [42., 2.]),
            (80., 2, 66., [26., 2.]),
            (55., 2, 41., [2., 14.]),
            (80., 2, 66., [26., 2.]),
        ] {
            r.tree.patch(&set(1, kind, host(room))).unwrap();
            r.tree.patch(&set(3, 4, b(zone))).unwrap();
            draw(&mut r);
            let size = bounds(&r, 3);
            assert_eq!(if axis == 1 { size.width } else { size.height }, 12.);
            assert_eq!(xy(&r, 3, axis), [b_at, 2.]);
            assert_eq!(xy(&r, 4, axis), c_at);
            check_fresh(&r);
        }
    }
}
#[test]
fn end_group_overlap_keeps_the_later_declared_paint_and_listener() {
    let mut r = Raster::default();
    let h = block(&[px(1, 30.), px(2, 30.)]);
    r.tree
        .upload(&upload(&[
            node(1, 1, h, &[2, 3]),
            node(2, 4, card(1, 20., 2, [255, 0, 0]), &[]),
            node(3, 4, card(1, 20., 0, [0, 0, 255]), &[]),
        ]))
        .unwrap();
    for (ids, target, local, rgb) in [
        ([2, 3], 3, 15., [0, 0, 255, 255]),
        ([3, 2], 2, 5., [255, 0, 0, 255]),
    ] {
        r.tree.patch(&order(1, 1, &ids)).unwrap();
        let i = 4 * (6 * 180 + 15);
        assert_eq!(&draw(&mut r)[i..i + 4], &rgb);
        assert_eq!(r.hit_at(15., 6.), Some((target, local, 6.)));
        check_fresh(&r);
    }
}
