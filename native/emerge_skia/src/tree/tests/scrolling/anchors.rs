use super::*;

fn radius(r: f64) -> Bytes {
    [13, 0].into_iter().chain(r.to_be_bytes()).collect()
}
fn sample(scene: &paint::Scene, x: usize, y: usize) -> ([u8; 4], Option<(u64, f64, f64)>) {
    let off = 4 * (y * scene.viewport.width as usize + x);
    (
        scene.rasterize().unwrap()[off..off + 4].try_into().unwrap(),
        scene
            .hits
            .target_at(scene.viewport, x as f32, y as f32)
            .map(|(_, id, x, y)| (id, x, y)),
    )
}

pub(in crate::tree::tests) fn corner_scene() -> Bytes {
    // Only the corner of the first row reaches this rounded 40x40 port. The
    // menu itself is entirely outside it; a submenu covers only its left half.
    // A tall ordinary wrapper keeps the same ordinal selected across the cut.
    upload(&[
        node(1, 3, box_attrs(80., 80.), &[2, 8]),
        node(
            2,
            3,
            block(&[px(1, 40.), px(2, 40.), vec![7, 1], radius(20.)]),
            &[3],
        ),
        node(3, 4, box_attrs(40., 80.), &[4]),
        mounted(
            node(4, 4, block(&[px(1, 4.), px(2, 4.), number(32, 38.)]), &[]),
            &[(4, 5)],
        ),
        mounted(
            node(
                5,
                4,
                block(&[
                    px(1, 20.),
                    px(2, 10.),
                    number(32, 6.),
                    color([0, 255, 0]),
                    vec![40, 1],
                ]),
                &[],
            ),
            &[(6, 6)],
        ),
        node(
            6,
            4,
            block(&[px(1, 5.), px(2, 10.), color([0, 0, 255]), vec![40, 1]]),
            &[],
        ),
        node(
            8,
            4,
            block(&[px(1, 80.), px(2, 40.), color([255, 255, 0]), vec![40, 1]]),
            &[],
        ),
    ])
}

#[test]
fn a_corner_can_hide_the_anchor_without_changing_the_selected_row() {
    let bytes = corner_scene();
    let mut fast = Tree::for_session(0);
    Publisher::new(0)
        .apply(&bytes, 1.)
        .unwrap()
        .install(&mut fast);
    let mut slow = Tree::default();
    slow.layers.scroll.exhaustive = true;
    slow.upload(&bytes).unwrap();
    let mut held = None;
    for (i, offset) in [0., 6., 6.25, 8., 0., 6.25].into_iter().enumerate() {
        for t in [&mut fast, &mut slow] {
            if i > 0 {
                recordings::move_to(t, 2, offset, i as u64);
            }
            t.refresh(80., 80., 1.).unwrap();
        }
        let a = fast.seal().unwrap();
        let b = slow.seal().unwrap();
        let visible = offset > 6.; // at (4,32) the host just touches the radius-20 circle
        let y = 46;
        for x in [2, 9] {
            let (rgba, target) = if !visible {
                ([255, 255, 0, 255], (8, x as f64, 6.))
            } else {
                (
                    if x == 2 {
                        [0, 0, 255, 255]
                    } else {
                        [0, 255, 0, 255]
                    },
                    (
                        if x == 2 { 6 } else { 5 },
                        x as f64,
                        y as f64 - (48. - offset),
                    ),
                )
            };
            assert_eq!(sample(&a, x, y), (rgba, Some(target)), "at offset {offset}");
            assert_eq!(
                sample(&b, x, y),
                (rgba, Some(target)),
                "exhaustive at {offset}"
            );
        }
        if i == 2 {
            held = Some((a.clone(), a.rasterize().unwrap()));
        }
        if i == 3 {
            assert_eq!(fast.output.recorded(), [0; 2]);
        }
    }
    let (pinned, px) = held.unwrap();
    assert_eq!(pinned.rasterize().unwrap(), px);
    assert_eq!(sample(&pinned, 2, 46).1, Some((6, 2., 4.25)));
}

#[test]
fn a_port_in_an_escaped_menu_tests_its_own_rows_not_the_original_window() {
    let port = |h| block(&[px(1, 40.), px(2, h), vec![7, 1]]);
    let bytes = upload(&[
        node(1, 3, box_attrs(90., 90.), &[2, 9]),
        node(2, 3, port(30.), &[3, 4]),
        mounted(node(3, 4, box_attrs(40., 15.), &[]), &[(4, 5)]),
        node(4, 4, box_attrs(40., 75.), &[]),
        node(
            5,
            3,
            block(&[px(1, 40.), px(2, 15.), vec![7, 1], number(32, 30.)]),
            &[6, 7],
        ),
        mounted(node(6, 4, box_attrs(40., 10.), &[]), &[(3, 8)]),
        node(7, 4, box_attrs(40., 40.), &[]),
        // A listenerless front root must disappear as a blocker, too.
        node(
            8,
            4,
            block(&[px(1, 14.), px(2, 10.), number(32, 10.), color([0, 255, 0])]),
            &[],
        ),
        node(
            9,
            4,
            block(&[px(1, 90.), px(2, 60.), color([255, 255, 0]), vec![40, 1]]),
            &[],
        ),
    ]);
    for exhaustive in [false, true] {
        let mut r = Raster::default();
        r.tree.layers.scroll.exhaustive = exhaustive;
        r.tree.upload(&bytes).unwrap();
        r.draw(90., 90., 1.).unwrap();
        for (i, (outer, inner)) in [
            (0., 0.),
            (0., 10.),
            (14., 9.),
            (15., 9.),
            (15., 4.),
            (14., 4.),
            (0., 0.),
        ]
        .into_iter()
        .enumerate()
        {
            if i != 0 {
                recordings::move_to(&mut r.tree, 2, outer, i as u64);
                recordings::move_to(&mut r.tree, 5, inner, i as u64);
                r.draw(90., 90., 1.).unwrap();
            }
            let y = (57. - outer - inner) as usize;
            let active = outer < 15. && inner < 10.;
            let rgb = if active {
                [0, 255, 0, 255]
            } else {
                [255, 255, 0, 255]
            };
            assert_eq!(
                &r.pixels()[4 * (90 * y + 45)..4 * (90 * y + 46)],
                &rgb,
                "at {outer},{inner} exhaustive={exhaustive}"
            );
            assert_eq!(
                r.hit_at(45., y as f32),
                (!active).then_some((9, 45., y as f64 - 30.))
            );
        }
    }
}

#[test]
fn a_turned_anchor_must_reach_the_corner_not_just_overlap_its_bounds() {
    let host = |offset| {
        block(&[
            px(1, 10.),
            px(2, 10.),
            number(31, offset),
            number(32, offset),
            number(33, 45.),
        ])
    };
    let bytes = upload(&[
        node(
            1,
            3,
            block(&[
                px(1, 40.),
                px(2, 40.),
                vec![7, 1],
                vec![40, 1],
                color([255, 255, 0]),
            ]),
            &[2],
        ),
        mounted(node(2, 4, host(-9.), &[]), &[(6, 3)]),
        node(
            3,
            4,
            block(&[
                px(1, 20.),
                px(2, 20.),
                number(31, 30.),
                vec![40, 1],
                color([0, 255, 0]),
            ]),
            &[],
        ),
    ]);
    for exhaustive in [false, true] {
        let mut r = Raster::default();
        r.tree.layers.scroll.exhaustive = exhaustive;
        r.tree.upload(&bytes).unwrap();
        for offset in [-9., -8., -9.] {
            r.tree.patch(&set(2, 4, host(offset))).unwrap();
            r.draw(60., 60., 1.).unwrap();
            let pixel = &r.pixels()[4 * (60 * 25 + 17)..4 * (60 * 25 + 18)];
            let (id, x, y) = r.hit_at(17., 25.).unwrap();
            if offset == -9. {
                assert_eq!(pixel, [255, 255, 0, 255]);
                assert_eq!((id, x, y), (1, 17., 25.));
            } else {
                assert_eq!(pixel, [0, 255, 0, 255]);
                assert_eq!(id, 3);
                assert!(
                    (x - (48. / 2f64.sqrt() - 25.)).abs() < 1e-4
                        && (y - (8. / 2f64.sqrt() + 5.)).abs() < 1e-4
                );
            }
        }
    }
}

#[test]
fn a_point_anchor_at_the_viewport_edge_can_move_between_content_and_a_stationary_mount() {
    // The host has no width. Its origin is at x=40; its entire menu projects
    // back to x=16..36. Neither the menu nor the host's height admits the row.
    let menu = |rgb| {
        block(&[
            px(1, 20.),
            px(2, 8.),
            number(31, -24.),
            color(rgb),
            vec![40, 1],
        ])
    };
    let bytes = upload(&[
        node(
            1,
            3,
            block(&[
                px(1, 80.),
                px(2, 40.),
                vec![7, 1],
                color([255, 255, 0]),
                vec![40, 1],
            ]),
            &[2],
        ),
        mounted(
            node(2, 4, block(&[px(1, 0.), px(2, 8.), number(31, 40.)]), &[]),
            &[(6, 3)],
        ),
        node(3, 4, menu([0, 255, 0]), &[]),
    ]);
    for exhaustive in [false, true] {
        let mut r = Raster::default();
        r.tree.layers.scroll.exhaustive = exhaustive;
        r.tree.upload(&bytes).unwrap();
        let expect = |r: &mut Raster, w: usize, visible: bool, rgb: [u8; 3]| {
            r.draw(w as f32, 40., 1.).unwrap();
            let i = 4 * (4 * w + 20);
            assert_eq!(
                &r.pixels()[i..i + 3],
                if visible { rgb } else { [255, 255, 0] }
            );
            assert_eq!(
                r.hit_at(20., 4.),
                Some(if visible { (3, 4., 4.) } else { (1, 20., 4.) })
            );
        };
        expect(&mut r, 41, true, [0, 255, 0]);
        expect(&mut r, 40, false, [0, 255, 0]);
        let red = [255, 0, 0];
        r.tree.patch(&set(3, 4, menu(red))).unwrap();
        expect(&mut r, 40, false, red);
        r.tree
            .patch(&patch(1, 1, &[mounted(edges(1, 3, &[]), &[(6, 2)])]))
            .unwrap();
        expect(&mut r, 40, true, red);
        r.tree.patch(&order(1, 3, &[2])).unwrap();
        expect(&mut r, 40, false, red);
        expect(&mut r, 41, true, red);
    }
}

#[test]
fn changing_an_ancestor_radius_changes_the_exposed_port_not_the_menu_shape() {
    let wrapper = |r| block(&[px(1, 40.), px(2, 40.), radius(r)]);
    let bytes = upload(&[
        node(1, 3, box_attrs(80., 80.), &[2, 7]),
        node(2, 4, wrapper(1.), &[3]),
        node(3, 3, block(&[px(1, 40.), px(2, 40.), vec![7, 1]]), &[4]),
        mounted(node(4, 4, block(&[px(1, 3.), px(2, 3.)]), &[]), &[(6, 5)]),
        node(
            5,
            4,
            block(&[
                px(1, 20.),
                px(2, 12.),
                number(32, 40.),
                color([0, 255, 0]),
                vec![40, 1],
            ]),
            &[],
        ),
        node(7, 4, box_attrs(80., 40.), &[]),
    ]);
    let mut r = Raster::default();
    r.tree.upload(&bytes).unwrap();
    for radius in [1., 20., 1.] {
        r.tree.patch(&set(2, 4, wrapper(radius))).unwrap();
        r.draw(80., 80., 1.).unwrap();
        let (rgb, id) = if radius == 1. {
            ([0, 255, 0, 255], 5)
        } else {
            ([1, 2, 3, 255], 7)
        };
        assert_eq!(
            &r.pixels()[4 * (80 * 44 + 9)..4 * (80 * 44 + 10)],
            &rgb,
            "radius {radius}"
        );
        assert_eq!(r.hit_at(9., 44.), Some((id, 9., 4.)));
    }
}
