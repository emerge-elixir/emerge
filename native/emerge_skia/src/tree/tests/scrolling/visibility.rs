use super::*;
mod content;

fn reference(bytes: &[u8]) -> Raster {
    let mut r = Raster::default();
    r.tree.layers.scroll.exhaustive = true;
    r.tree.upload(bytes).unwrap();
    r
}
fn twins(fast: &mut Raster, slow: &mut Raster, w: usize, h: usize) {
    let actual = fast.draw(w as f32, h as f32, 1.).unwrap();
    let expected = slow.draw(w as f32, h as f32, 1.).unwrap();
    assert_eq!(
        actual.iter().zip(expected).position(|(a, b)| a != b),
        None,
        "pixel byte mismatch"
    );
    for y in (0..h).step_by(3) {
        for x in (0..w).step_by(5) {
            let (x, y) = (x as f32 + 0.25, y as f32 + 0.5);
            assert_eq!(fast.hit_at(x, y), slow.hit_at(x, y), "target at {x},{y}");
        }
    }
}
fn row_attrs(id: u64, horizontal: bool, rounded: bool, size: f64, extra: &[Bytes]) -> Bytes {
    let (w, h) = if horizontal {
        (size, 100.)
    } else {
        (100., size)
    };
    block(
        &[px(1, w), px(2, h), color([id as u8, 20, 30]), vec![40, 1]]
            .into_iter()
            .chain(rounded.then(|| [13, 0].into_iter().chain(3f64.to_be_bytes()).collect()))
            .chain(extra.iter().cloned())
            .collect::<Vec<_>>(),
    )
}
pub(in crate::tree::tests) fn list(
    n: u64,
    horizontal: bool,
    wrapped: bool,
    rounded: bool,
) -> Bytes {
    let ids = (10..10 + n).collect::<Vec<_>>();
    let kind = if horizontal { 1 } else { 3 };
    let (w, h, axis) = if horizontal {
        (80., 100., 8)
    } else {
        (100., 80., 7)
    };
    let attrs = block(&[px(1, w), px(2, h), vec![axis, 1]]);
    let root = if wrapped {
        node(1, 4, attrs, &[2])
    } else {
        node(1, kind, attrs, &ids)
    };
    upload(
        &std::iter::once(root)
            .chain(wrapped.then(|| node(2, kind, block(&[]), &ids)))
            .chain(
                ids.iter()
                    .map(|&id| node(id, 4, row_attrs(id, horizontal, rounded, 20., &[]), &[])),
            )
            .collect::<Vec<_>>(),
    )
}
#[test]
fn warm_scroll_projects_only_the_saved_run_for_both_streams() {
    for n in [100, 10_000] {
        for (horizontal, wrapped, rounded) in [
            (false, false, false),
            (false, false, true),
            (true, false, true),
            (false, true, true),
        ] {
            let bytes = list(n, horizontal, wrapped, rounded);
            let mut r = Raster::default();
            r.tree.upload(&bytes).unwrap();
            let (w, h) = if horizontal { (80., 100.) } else { (100., 80.) };
            r.draw(w, h, 1.).unwrap();
            // The cold content edge makes storage addressable even for the
            // last, as-yet unprojected row. Its first reveal is still sparse.
            assert_eq!(
                r.tree.layers.scroll.counts.content_slots,
                n as usize + usize::from(wrapped)
            );
            assert!(
                r.tree
                    .layers
                    .projection(r.tree.elements.ids[&(n + 9)])
                    .is_none()
            );
            let range = 20. * n as f64 - 80.;
            for (i, (offset, first, count, rewrite)) in [
                (5., 0, 5, 1),
                (6., 0, 5, 0),
                (20., 1, 4, 1),
                (range, n - 4, 4, 1),
                (range / 2., (n - 4) / 2, 4, 1),
                (0., 0, 4, 1),
            ]
            .into_iter()
            .enumerate()
            {
                let mut pos = [0.; 2];
                pos[usize::from(!horizontal)] = offset / range;
                r.tree.layers.scroll.counts = Default::default();
                assert!(set_input(&mut r.tree, 1, pos, 1 + i as u64, None));
                r.draw(w, h, 1.).unwrap();
                let e = &r.tree.elements;
                let order = e.ids[&if wrapped { 2 } else { 1 }];
                let selected = r
                    .tree
                    .layers
                    .scroll
                    .children_rev(e, order)
                    .map(|&k| e.slots[k].id)
                    .collect::<Vec<_>>();
                assert_eq!(
                    selected,
                    (10 + first..10 + first + count).rev().collect::<Vec<_>>()
                );
                let work = r.tree.layers.scroll.counts;
                assert_eq!((work.content_slots, work.range_reads), (0, 0));
                assert_eq!(work.fact_roots, usize::from(!wrapped));
                assert_eq!(work.compose_reads, [usize::from(wrapped); 2]);
                let trace = &r.tree.layers.trace;
                assert!(
                    trace
                        .iter()
                        .all(|(p, _, _)| matches!(p, PaintContext | Compose | Project))
                );
                assert_eq!(trace.iter().filter(|(p, _, _)| *p == Compose).count(), 1);
                assert_eq!(
                    trace.iter().filter(|(p, _, _)| *p == PaintContext).count(),
                    1
                );
                assert_eq!(
                    trace.iter().filter(|(p, _, _)| *p == Project).count(),
                    1 + count as usize + usize::from(wrapped)
                );
                assert_eq!(
                    (work.builds, work.refits, work.refreshes, work.rewrites),
                    (0, 0, 1, rewrite)
                );
                assert!(
                    work.probes + count as usize
                        <= 4 * (n + 1).next_power_of_two().ilog2() as usize
                            + 4 * count as usize
                            + 8,
                    "{work:?}"
                );
                assert_eq!(
                    r.tree.output.packed_children,
                    [count as usize + usize::from(wrapped); 2]
                );
                assert_eq!(
                    r.pack_counts(),
                    (count as usize + 1, 0, 0, count as usize + 2)
                );
                let local = 5. + offset % 20.;
                let (x, y) = if horizontal { (local, 5.) } else { (5., local) };
                assert_eq!(r.hit_at(5., 5.), Some((10 + first, x, y)));
                let b = 4 * (w as usize * 5 + 5);
                assert_eq!(&r.pixels()[b..b + 4], &[(10 + first) as u8, 20, 30, 255]);
                if i < 2 {
                    // 5 → 6 keeps both endpoints, but moves the boundary at
                    // pixel 14 across rows 10 and 11 on the scrolling axis.
                    let (x, y) = if horizontal { (14, 5) } else { (5, 14) };
                    let b = 4 * (w as usize * y + x);
                    assert_eq!(&r.pixels()[b..b + 4], &[10 + i as u8, 20, 30, 255]);
                }
                eprintln!(
                    "rows={n} horizontal={horizontal} delegated={wrapped} rounded={rounded} offset={offset} first={first} count={count} {work:?} paint_context/compose/project={:?} packed={:?} commands/glyphs/fits/hits={:?}",
                    [PaintContext, Compose, Project]
                        .map(|pass| trace.iter().filter(|(p, _, _)| *p == pass).count()),
                    r.tree.output.packed_children,
                    r.pack_counts()
                );
            }
        }
    }
}

#[test]
fn hidden_edits_changed_reach_reorder_and_undo_match_exhaustive_projection() {
    let n = 65;
    let ids: Vec<_> = (10..10 + n).collect();
    let port = |pos| block(&[px(1, 100.), px(2, 80.), vec![7, 1], fraction(86, pos)]);
    let row = |id, extra: &[Bytes]| row_attrs(id, false, true, 10. + (id % 5) as f64, extra);
    let bytes = upload(
        &std::iter::once(node(1, 3, port(0.), &ids))
            .chain(ids.iter().map(|&id| node(id, 4, row(id, &[]), &[])))
            .collect::<Vec<_>>(),
    );
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut slow = reference(&bytes);
    twins(&mut fast, &mut slow, 100, 80);
    let y = bounds(&fast, 55).y;
    // The aggregate extent of the port needn't change when a middle row jumps
    // into the window. It creates two visible runs in the authoritative order.
    let moved = set(55, 4, row(55, &[number(32, -f64::from(y) + 5.)]));
    for edits in [
        vec![set(74, 4, row(99, &[]))],
        vec![moved.clone()],
        vec![set(1, 3, port(1.))],
        vec![set(1, 3, port(0.)), set(55, 4, row(55, &[]))],
        vec![order(1, 3, &ids.iter().rev().copied().collect::<Vec<_>>())],
        vec![order(1, 3, &ids), moved],
        vec![set(55, 4, row(55, &[]))],
    ] {
        fast.tree.layers.scroll.counts = Default::default();
        for patch in &edits {
            fast.tree.patch(patch).unwrap();
            slow.tree.patch(patch).unwrap();
        }
        twins(&mut fast, &mut slow, 100, 80);
        if edits == [set(74, 4, row(99, &[]))] {
            assert_eq!(fast.tree.layers.scroll.counts.refreshes, 0);
        }
        let mut cold = reference(&fast.tree.encode());
        twins(&mut fast, &mut cold, 100, 80);
    }
}

#[test]
fn nested_wrapped_rows_and_offscreen_reparenting_follow_the_final_order() {
    let port = |w, h, pos| block(&[px(1, w), px(2, h), vec![7, 1], fraction(86, pos)]);
    let ids = (10..60).collect::<Vec<_>>();
    let bytes = upload(
        &[
            node(1, 3, port(100., 60., 0.), &[2, 3, 4]),
            node(2, 4, row_attrs(2, false, false, 55., &[]), &[]),
            node(3, 2, port(90., 48., 0.), &ids),
            node(4, 3, port(100., 50., 0.), &[5]),
            node(5, 4, row_attrs(5, false, false, 90., &[]), &[]),
        ]
        .into_iter()
        .chain(ids.iter().map(|&id| {
            node(
                id,
                4,
                block(&[
                    px(1, 32. + (id % 3) as f64),
                    px(2, 10. + (id % 7) as f64),
                    color([id as u8, 100, 0]),
                    vec![40, 1],
                ]),
                &[],
            )
        }))
        .collect::<Vec<_>>(),
    );
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut slow = reference(&bytes);
    let other = ids
        .iter()
        .copied()
        .filter(|&id| id != 42)
        .collect::<Vec<_>>();
    let move_to = |slot| {
        patch(
            1,
            1,
            &[
                edges(1, 3, &[2, 3, 4]),
                edges(3, 2, &other),
                if slot {
                    mounted(edges(4, 3, &[5]), &[(2, 42)])
                } else {
                    edges(4, 3, &[42, 5])
                },
            ],
        )
    };
    for edits in [
        vec![],
        vec![set(3, 2, port(90., 48., 0.67))],
        vec![set(1, 3, port(100., 60., 0.55))],
        vec![move_to(false), set(1, 3, port(100., 60., 1.))],
        vec![move_to(true)],
        vec![
            patch(
                1,
                1,
                &[
                    edges(1, 3, &[2, 3, 4]),
                    edges(3, 2, &ids),
                    edges(4, 3, &[5]),
                ],
            ),
            set(1, 3, port(100., 60., 0.2)),
        ],
    ] {
        for edit in edits {
            fast.tree.patch(&edit).unwrap();
            slow.tree.patch(&edit).unwrap();
        }
        twins(&mut fast, &mut slow, 100, 60);
        let mut cold = Raster::default();
        cold.tree.upload(&fast.tree.encode()).unwrap();
        twins(&mut fast, &mut cold, 100, 60);
    }
}

#[test]
fn a_zero_overflow_inner_axis_takes_and_returns_the_same_order_at_equal_geometry() {
    let host = |enabled| block(&[px(1, 100.), px(2, 200.), vec![8, enabled]]);
    let kids = (10..20).collect::<Vec<_>>();
    let bytes = upload(
        &[
            node(
                1,
                4,
                block(&[px(1, 100.), px(2, 80.), vec![7, 1], fraction(86, 0.5)]),
                &[2],
            ),
            node(2, 3, host(0), &kids),
        ]
        .into_iter()
        .chain(
            kids.iter()
                .map(|&id| node(id, 4, row_attrs(id, false, false, 20., &[]), &[])),
        )
        .collect::<Vec<_>>(),
    );
    let mut fast = Raster::default();
    fast.tree.upload(&bytes).unwrap();
    let mut all = reference(&bytes);
    for enabled in [0, 1, 0, 1] {
        let edit = set(2, 3, host(enabled));
        fast.tree.patch(&edit).unwrap();
        all.tree.patch(&edit).unwrap();
        twins(&mut fast, &mut all, 100, 80);
        let e = &fast.tree.elements;
        let order = e.ids[&2];
        let port = e.ids[&if enabled == 0 { 1 } else { 2 }];
        assert_eq!(scroll::order_port(e, order), Some(port));
        assert_eq!(
            fast.tree
                .layers
                .scroll
                .children_rev(e, order)
                .map(|k| e.slots[*k].id)
                .collect::<Vec<_>>(),
            [16, 15, 14, 13]
        );
        assert_eq!(view(&fast, 1).offset(), [0., 60.]);
        assert_eq!(view(&fast, 2).range, [0.; 2]);
        assert_eq!(fast.hit_at(5., 5.), Some((13, 5., 5.)));
    }
}

#[test]
fn first_scroll_window_reserves_before_acceptance_even_at_unchanged_key_count() {
    let kids = (10..74).collect::<Vec<_>>();
    let plain = upload(
        &std::iter::once(node(1, 3, block(&[px(1, 100.), px(2, 80.)]), &kids))
            .chain(
                kids.iter()
                    .map(|&id| node(id, 4, row_attrs(id, false, false, 20., &[]), &[])),
            )
            .collect::<Vec<_>>(),
    );
    let turns = [
        plain,
        set(10, 4, row_attrs(10, false, true, 20., &[])),
        set(
            1,
            3,
            block(&[px(1, 100.), px(2, 80.), vec![7, 1], fraction(86, 1.)]),
        ),
    ];
    let cap = {
        let storage = storage::Storage::default();
        let mut w = Publisher::with_storage(0, storage.clone());
        let mut t = Tree::for_session(0);
        for edit in &turns {
            w.apply(edit, 1.).unwrap().install(&mut t);
        }
        storage.workspace.bytes().0
    };
    let storage = storage::Storage::with_limits(1 << 21, 2 * cap, 1 << 20);
    let mut w = Publisher::with_storage(0, storage.clone());
    let mut t = Tree::for_session(0);
    w.apply(&turns[0], 1.).unwrap().install(&mut t);
    t.refresh(100., 80., 1.).unwrap();
    let pin = t.seal().unwrap();
    w.apply(&turns[1], 1.).unwrap().install(&mut t);
    t.refresh(100., 80., 1.).unwrap();
    let installed = t.seal().unwrap();
    let accepted = t.encode();
    assert_eq!(w.apply(&turns[2], 1.).err().as_deref(), Some(storage::BUSY));
    assert!(!storage.available());
    assert_eq!(t.encode(), accepted);
    assert_eq!(
        installed
            .hits
            .target_at(installed.viewport, 5., 5.)
            .unwrap()
            .1,
        10
    );
    drop(pin);
    assert!(storage.available());
    w.apply(&turns[2], 1.).unwrap().install(&mut t);
    t.refresh(100., 80., 1.).unwrap();
    let next = t.seal().unwrap();
    assert_eq!(next.hits.target_at(next.viewport, 5., 5.).unwrap().1, 70);
    assert_eq!(
        installed
            .hits
            .target_at(installed.viewport, 5., 5.)
            .unwrap()
            .1,
        10
    );
}

#[test]
fn packing_and_replay_do_not_discover_rows_or_borrow_the_next_selection() {
    let bytes = list(100, false, true, true);
    let mut writer = Publisher::new(0);
    let mut tree = Tree::for_session(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut tree);
    tree.refresh(100., 80., 1.).unwrap();
    let old = tree.seal().unwrap();
    assert!(set_input(&mut tree, 1, [0., 1.], 1, None));
    tree.layout(100., 80., 1.).unwrap();
    let reads = tree.layers.scroll.counts.probes;
    assert!(tree.collect_if_available());
    let new = tree.seal().unwrap();
    for (scene, id) in [(&new, 106), (&old, 10)] {
        assert_eq!(
            scene
                .hits
                .target_at(scene.viewport, 5., 5.)
                .map(|(_, id, x, y)| (id, x, y)),
            Some((id, 5., 5.))
        );
        assert_eq!(
            &scene.rasterize().unwrap()[2020..2024],
            &[id as u8, 20, 30, 255]
        );
    }
    assert_eq!(tree.layers.scroll.counts.probes, reads);
}

#[test]
fn opaque_sibling_coverage_is_draw_only_and_recovers_on_reorder_or_alpha() {
    let id = |i| {
        node(
            i,
            4,
            block(&[
                px(1, 40.),
                px(2, 40.),
                color([0, i as u8 * 50, 0]),
                vec![40, 1],
            ]),
            &[],
        )
    };
    let front = |shift, alpha| {
        block(&[
            px(1, 40.),
            px(2, 40.),
            number(31, shift),
            number(35, alpha),
            color([0, 0, 255]),
            vec![40, 1],
        ])
    };
    let bytes = upload(&[
        node(1, 2, block(&[px(1, 100.), px(2, 40.)]), &[2, 3]),
        id(2),
        node(3, 4, front(-40., 1.), &[]),
    ]);
    let mut r = Raster::default();
    r.tree.upload(&bytes).unwrap();
    let mut slow = reference(&bytes);
    for (edits, hidden) in [
        (vec![], true),
        (vec![set(3, 4, front(-40., 0.5))], false),
        (vec![set(3, 4, front(-20., 1.))], false),
        (vec![set(3, 4, front(-40., 1.))], true),
        (vec![order(1, 2, &[3, 2])], false),
    ] {
        for edit in edits {
            r.tree.patch(&edit).unwrap();
            slow.tree.patch(&edit).unwrap();
        }
        twins(&mut r, &mut slow, 100, 40);
        let (fast, _, _, hits) = r.pack_counts();
        let (all, _, _, all_hits) = slow.pack_counts();
        assert_eq!(hits, all_hits);
        assert_eq!(all - fast, usize::from(hidden));
    }
}
