use super::*;

fn install(t: &mut Tree) -> paint::Scene {
    t.layout(120., 45., 1.).unwrap();
    assert!(t.collect_if_available());
    t.seal().unwrap()
}
fn row(id: u64, click: bool, ink: u8) -> Bytes {
    block(&[
        px(1, 60.),
        px(2, 20.),
        color([ink, id as u8, 40]),
        vec![40, u8::from(click)],
    ])
}
fn document() -> Bytes {
    let port = block(&[px(1, 60.), px(2, 45.), vec![7, 1]]);
    let kids = |start: u64| (start..start + 12).map(|i| i * 2).collect::<Vec<_>>();
    let rows = kids(10).into_iter().chain(kids(30)).flat_map(|id| {
        [
            node(id, 4, row(id, true, 100), &[id + 1]),
            node(id + 1, 5, text("ink"), &[]),
        ]
    });
    upload(
        &[
            node(1, 1, block(&[px(1, 120.), px(2, 45.)]), &[2, 3]),
            node(2, 3, port.clone(), &kids(10)),
            node(3, 3, port, &kids(30)),
        ]
        .into_iter()
        .chain(rows)
        .collect::<Vec<_>>(),
    )
}
pub(in crate::tree::tests) fn move_to(t: &mut Tree, port: u64, px: f64, serial: u64) {
    let range = t.layers.scroll.view(t.elements.ids[&port]).range[1];
    assert!(set_input(
        t,
        port,
        [0., px / f64::from(range)],
        serial,
        None
    ));
}
fn hit(s: &paint::Scene, x: f32, y: f32) -> Option<(u64, f64, f64)> {
    s.hits
        .target_at(s.viewport, x, y)
        .map(|(_, id, x, y)| (id, x, y))
}
#[test]
fn translation_seals_only_bindings_and_pinned_frames_keep_their_pixels_and_hits() {
    let bytes = document();
    let mut writer = Publisher::new(0);
    let mut t = Tree::for_session(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut t);
    let start = install(&mut t);
    let mut exhaustive = Tree::default();
    exhaustive.layers.scroll.exhaustive = true;
    exhaustive.upload(&bytes).unwrap();
    let oracle = install(&mut exhaustive);
    assert_eq!(start.rasterize().unwrap(), oracle.rasterize().unwrap());
    drop(oracle);
    let keys = [2, 3].map(|id| t.elements.ids[&id]);
    let mut frames = vec![start];
    for (i, (port, offset)) in [(2, 30.), (2, 31.), (3, 2.), (3, 3.)]
        .into_iter()
        .enumerate()
    {
        move_to(&mut t, port, offset, i as u64 + 1);
        let next = install(&mut t);
        let prev = frames.last().unwrap();
        assert_eq!(
            t.layers
                .trace
                .iter()
                .filter(|(p, _, _)| *p == Project)
                .map(|(_, id, _)| *id)
                .collect::<Vec<_>>(),
            [port]
        );
        assert!(std::sync::Arc::ptr_eq(&next.drawing, &prev.drawing));
        assert!(!prev.same_drawing(&next));
        if i == 1 || i == 3 {
            assert_eq!(t.output.assembly(), (0, [0; 3]));
            assert_eq!(t.output.recorded(), [0; 2]);
            assert_eq!(t.output.packed_children, [0; 2]);
            for k in keys {
                for s in 0..2 {
                    assert!(next.shares_order(prev, k, s));
                }
            }
        }
        move_to(&mut exhaustive, port, offset, i as u64 + 1);
        let reference = install(&mut exhaustive);
        assert_eq!(next.rasterize().unwrap(), reference.rasterize().unwrap());
        assert_ne!(prev.rasterize().unwrap(), next.rasterize().unwrap());
        for x in [7., 52., 66., 113.] {
            for y in [3., 15., 43.] {
                assert_eq!(hit(&next, x, y), hit(&reference, x, y));
            }
        }
        frames.push(next);
    }
    // All five map versions, but only one drawing, are held. Settlement may
    // advance; a rejected seal must not steal a binding from a reader.
    move_to(&mut t, 2, 32., 5);
    t.layout(120., 45., 1.).unwrap();
    assert!(!t.collect_if_available());
    assert!(t.seal().is_none());
    assert_eq!(hit(&frames[0], 7., 3.), Some((20, 7., 3.)));
    assert_eq!(hit(&frames[4], 7., 3.), Some((22, 7., 14.)));
    frames.remove(1);
    assert!(t.collect_if_available());
    let next = t.seal().unwrap();
    drop(frames);
    assert_eq!(hit(&next, 7., 3.), Some((22, 7., 15.)));
}
#[test]
fn nested_windows_and_escape_bands_share_their_translation_boundaries() {
    let port = |h| block(&[px(1, 100.), px(2, h), vec![7, 1], vec![40, 1]]);
    let ids = (10..20).collect::<Vec<_>>();
    let bytes = upload(
        &[
            mounted(node(1, 3, port(45.), &[2, 3, 4]), &[(6, 80)]),
            node(2, 4, row(2, true, 50), &[]),
            node(3, 3, port(50.), &ids),
            node(
                4,
                4,
                block(&[px(1, 100.), px(2, 90.), color([40, 80, 120])]),
                &[],
            ),
            node(
                80,
                4,
                block(&[px(1, 3.), px(2, 3.), vec![40, 1], color([0, 255, 255])]),
                &[],
            ),
            mounted(
                node(
                    90,
                    4,
                    block(&[px(1, 15.), px(2, 12.), vec![40, 1], color([255, 0, 255])]),
                    &[],
                ),
                &[(6, 91)],
            ),
            node(
                91,
                4,
                block(&[px(1, 4.), px(2, 4.), vec![40, 1], color([255, 255, 0])]),
                &[],
            ),
        ]
        .into_iter()
        .chain(ids.iter().map(|&id| {
            let r = node(id, 4, row(id, true, 90), &[]);
            if id == 11 { mounted(r, &[(6, 90)]) } else { r }
        }))
        .collect::<Vec<_>>(),
    );
    let mut f = Tree::for_session(0);
    Publisher::new(0).apply(&bytes, 1.).unwrap().install(&mut f);
    let mut slow = Tree::default();
    slow.layers.scroll.exhaustive = true;
    slow.upload(&bytes).unwrap();
    let old = install(&mut f);
    let pixels = old.rasterize().unwrap();
    let mut last = old.clone();
    for (i, (port, offset)) in [
        (1, 8.),
        (1, 9.),
        (3, 5.),
        (3, 6.),
        (1, 70.),
        (1, 8.),
        (3, 66.),
    ]
    .into_iter()
    .enumerate()
    {
        let reference = install(&mut slow);
        drop(reference);
        move_to(&mut f, port, offset, i as u64 + 1);
        move_to(&mut slow, port, offset, i as u64 + 1);
        let next = install(&mut f);
        if matches!(i, 1 | 3) {
            assert!(std::sync::Arc::ptr_eq(&last.drawing, &next.drawing));
            assert_eq!(f.output.assembly(), (0, [0; 3]));
            assert_eq!(f.output.packed_children, [0; 2]);
            assert_eq!(f.output.recorded(), [0; 2]);
            assert!(
                f.layers
                    .trace
                    .iter()
                    .all(|(p, k, _)| matches!(p, Project | PaintContext) && [1, 3].contains(k))
            );
        }
        let reference = install(&mut slow);
        assert!(
            next.rasterize().unwrap() == reference.rasterize().unwrap(),
            "nested raster {i}"
        );
        for x in (0..100).step_by(3) {
            for y in (0..45).step_by(2) {
                let (x, y) = (x as f32 + 0.5, y as f32 + 0.25);
                assert_eq!(
                    hit(&next, x, y),
                    hit(&reference, x, y),
                    "nested {i} at {x},{y}"
                );
                let region = |s: &paint::Scene| {
                    s.hits.region_at(s.viewport, x, y).map(|(p, a)| {
                        let [u, v] = p.point(x, y);
                        (p.id, a, [u - p.frame.x, v - p.frame.y])
                    })
                };
                assert_eq!(region(&next), region(&reference), "region {i} at {x},{y}");
            }
        }
        assert_eq!(hit(&next, 1., 1.).map(|h| h.0), Some(80));
        last = next;
    }
    assert_eq!(old.rasterize().unwrap(), pixels);
}
#[test]
fn an_export_breaks_the_ordinary_order_only_at_its_ordinal() {
    let ids = (10..34).collect::<Vec<_>>();
    for (angle, delegated, nested) in [
        (0., false, false),
        (0., true, false),
        (6., true, false),
        (0., false, true),
        (0., true, true),
        (6., true, true),
    ] {
        let host = if delegated { 2 } else { 1 };
        let attrs = block(&[
            px(1, 80.),
            px(2, 45.),
            vec![7, 1],
            number(33, angle),
            [13, 0].into_iter().chain(5f64.to_be_bytes()).collect(),
            number(35, 0.75),
        ]);
        let nodes = if delegated {
            vec![
                node(1, 4, attrs, &[2]),
                node(2, 3, block(&[px(1, 80.)]), &ids),
            ]
        } else {
            vec![node(1, 3, attrs, &ids)]
        };
        let bytes = upload(
            &nodes
                .into_iter()
                .chain(ids.iter().map(|&id| {
                    let row = node(
                        id,
                        4,
                        row(id, true, 160),
                        if nested && id == 17 { &[99] } else { &[] },
                    );
                    if !nested && id == 17 {
                        mounted(row, &[(6, 100)])
                    } else {
                        row
                    }
                }))
                .chain(nested.then(|| {
                    mounted(
                        node(
                            99,
                            4,
                            block(&[px(1, 40.), px(2, 16.), color([80, 40, 20])]),
                            &[],
                        ),
                        &[(6, 100)],
                    )
                }))
                .chain([node(
                    100,
                    4,
                    block(&[px(1, 20.), px(2, 8.), color([0, 255, 100]), vec![40, 1]]),
                    &[],
                )])
                .collect::<Vec<_>>(),
        );
        let mut fast = Tree::for_session(0);
        Publisher::new(0)
            .apply(&bytes, 1.)
            .unwrap()
            .install(&mut fast);
        let mut slow = Tree::default();
        slow.layers.scroll.exhaustive = true;
        slow.upload(&bytes).unwrap();
        let mut last = install(&mut fast);
        let initial = install(&mut slow);
        assert!(last.rasterize().unwrap() == initial.rasterize().unwrap());
        drop(initial);
        for (i, offset) in [30., 31., 119., 140., 141., 240., 241., 30.]
            .into_iter()
            .enumerate()
        {
            move_to(&mut fast, 1, offset, i as u64 + 1);
            move_to(&mut slow, 1, offset, i as u64 + 1);
            let next = install(&mut fast);
            // At 119 -> 140 an ordinary row enters while the exporter stays.
            if matches!(i, 0 | 1 | 3 | 4 | 6 | 7) {
                assert_eq!(
                    fast.layers
                        .trace
                        .iter()
                        .filter(|(p, _, _)| *p == Project)
                        .map(|(_, id, _)| *id)
                        .collect::<Vec<_>>(),
                    [1]
                );
                assert!(
                    std::sync::Arc::ptr_eq(&last.drawing, &next.drawing),
                    "{host} {i}"
                );
                assert_eq!(fast.output.packed_children, [0; 2]);
            }
            if matches!(i, 1 | 4 | 6 | 7) {
                for side in 0..2 {
                    assert!(next.shares_order(&last, fast.elements.ids[&1], side));
                }
                assert_eq!(fast.output.recorded(), [0; 2]);
                assert_eq!(fast.output.assembly().1, [0; 3]);
            }
            let reference = install(&mut slow);
            let a = next.rasterize().unwrap();
            let b = reference.rasterize().unwrap();
            assert_eq!(
                a.iter().zip(b).filter(|(x, y)| **x != *y).count(),
                0,
                "angle {angle}, host {host}, offset {offset}"
            );
            for x in (0..100).step_by(3) {
                for y in (0..45).step_by(3) {
                    let (x, y) = (x as f32 + 0.25, y as f32 + 0.5);
                    assert_eq!(
                        hit(&next, x, y),
                        hit(&reference, x, y),
                        "{angle} {host} {offset} {x} {y}"
                    );
                }
            }
            last = next;
        }
    }
}
#[test]
fn overlapping_local_ink_blends_inside_the_stationary_rounded_clip() {
    let ids = (10..18).collect::<Vec<_>>();
    let b = upload(
        &std::iter::once(node(
            1,
            3,
            block(&[
                px(1, 64.),
                px(2, 45.),
                vec![7, 1],
                number(33, 6.),
                [13, 0].into_iter().chain(9f64.to_be_bytes()).collect(),
            ]),
            &ids,
        ))
        .chain(ids.iter().flat_map(|&id| {
            [
                node(
                    id,
                    4,
                    block(&[
                        px(1, 64.),
                        px(2, 20.),
                        color([10, 80, 255]),
                        number(35, 0.625),
                    ]),
                    &[id + 20],
                ),
                node(
                    id + 20,
                    4,
                    block(&[px(1, 62.), px(2, 14.), color([180, 80, 20]), vec![40, 1]]),
                    &[],
                ),
            ]
        }))
        .collect::<Vec<_>>(),
    );
    let mut f = Tree::for_session(0);
    Publisher::new(0).apply(&b, 1.).unwrap().install(&mut f);
    let mut all = Tree::default();
    all.layers.scroll.exhaustive = true;
    all.upload(&b).unwrap();
    for (i, px) in [0., 1., 2., 60., 0.].into_iter().enumerate() {
        if i > 0 {
            for t in [&mut f, &mut all] {
                move_to(t, 1, px, i as u64);
            }
        }
        let a = install(&mut f);
        let b = install(&mut all);
        assert!(
            a.rasterize().unwrap() == b.rasterize().unwrap(),
            "layer at {px}"
        );
    }
}
#[test]
fn disjoint_runs_split_around_exports_and_remounts_do_not_revive_capture() {
    let ids = (10..30).collect::<Vec<_>>();
    let popup = |id, rgb| {
        node(
            id,
            4,
            block(&[px(1, 8.), px(2, 8.), color(rgb), vec![40, 1]]),
            &[],
        )
    };
    let bytes = upload(
        &std::iter::once(node(
            1,
            3,
            block(&[px(1, 80.), px(2, 45.), vec![7, 1]]),
            &ids,
        ))
        .chain(ids.iter().map(|&id| {
            let i = id - 10;
            let y = if i % 2 == 1 { 9. - 20. * i as f64 } else { 0. };
            let attrs = block(&[
                px(1, 40.),
                px(2, 20.),
                number(31, (i % 4) as f64 * 8.),
                number(32, y),
                color([id as u8 * 6, 10, 120]),
                vec![40, 1],
            ]);
            let row = node(id, 4, attrs, &[]);
            if matches!(id, 13 | 23) {
                mounted(row, &[(6, id + 100)])
            } else {
                row
            }
        }))
        .chain([popup(113, [255, 0, 80]), popup(123, [0, 240, 40])])
        .collect::<Vec<_>>(),
    );
    let mut writer = Publisher::new(0);
    let mut fast = Tree::for_session(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut fast);
    let mut slow = Tree::default();
    slow.layers.scroll.exhaustive = true;
    slow.upload(&bytes).unwrap();
    let old = install(&mut fast);
    drop(install(&mut slow));
    let pix = old.rasterize().unwrap();
    let cap = old.hits.listener_at(old.viewport, 10., 11.).unwrap();
    assert_eq!(cap.id, 123);
    let k = fast.elements.ids[&1];
    assert!(fast.layers.scroll.ranges_rev(&fast.elements, k, k).count() > 4);
    let compare = |a: &paint::Scene, b: &paint::Scene| {
        assert_eq!(a.packed_counts(), b.packed_counts());
        assert!(a.rasterize().unwrap() == b.rasterize().unwrap());
        for y in (0..45).step_by(3) {
            for x in (0..80).step_by(2) {
                assert_eq!(
                    hit(a, x as f32 + 0.25, y as f32 + 0.5),
                    hit(b, x as f32 + 0.25, y as f32 + 0.5)
                );
            }
        }
    };
    let mut last = old.clone();
    for (i, px) in [1., 2., 125., 126., 1., 0.].into_iter().enumerate() {
        for t in [&mut fast, &mut slow] {
            move_to(t, 1, px, i as u64 + 1);
        }
        let a = install(&mut fast);
        if matches!(i, 0 | 1 | 3 | 5) {
            assert!(std::sync::Arc::ptr_eq(&a.drawing, &last.drawing));
            assert_eq!(fast.output.assembly(), (0, [0; 3]));
        }
        let b = install(&mut slow);
        compare(&a, &b);
        last = a;
    }
    for edit in [
        patch(123, 0, &[]),
        patch(
            23,
            23,
            &[
                mounted(edges(23, 4, &[]), &[(6, 123)]),
                popup(123, [110, 80, 250]),
            ],
        ),
    ] {
        writer.apply(&edit, 1.).unwrap().install(&mut fast);
        slow.patch(&edit).unwrap();
        let a = install(&mut fast);
        compare(&a, &install(&mut slow));
        assert!(a.hits.captured(cap.key, 10., 11.).is_none());
        last = a;
    }
    let current = last.hits.listener_at(last.viewport, 10., 11.).unwrap();
    assert_eq!(current.id, cap.id);
    assert_ne!(current.key, cap.key);
    assert_eq!(
        old.hits.captured(cap.key, 13., 15.).unwrap().local,
        [5., 6.]
    );
    assert_eq!(old.rasterize().unwrap(), pix);
    assert_ne!(last.rasterize().unwrap(), pix);
}
#[test]
fn inherited_gradient_versions_survive_scroll_reorder_and_replacement() {
    let ids = (10..18).collect::<Vec<_>>();
    let ink = |low, high| {
        block(&[
            px(1, 100.),
            px(2, 45.),
            vec![7, 1],
            number(16, 15.),
            vec![
                17, 3, 0, 0, 0, 2, 0, low, 30, 0, 0, high, 50, 255, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ])
    };
    let phrase = |id| node(id + 20, 5, text("A B C"), &[]);
    let bytes = upload(
        &std::iter::once(node(1, 3, ink(255, 80), &ids))
            .chain(ids.iter().flat_map(|&id| {
                [
                    node(
                        id,
                        7,
                        block(&[px(1, 100.), px(2, 20.), vec![40, 1]]),
                        &[id + 20],
                    ),
                    phrase(id),
                ]
            }))
            .collect::<Vec<_>>(),
    );
    let mut writer = Publisher::new(0);
    let mut fast = Tree::for_session(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut fast);
    let mut slow = Tree::default();
    slow.layers.scroll.exhaustive = true;
    slow.upload(&bytes).unwrap();
    let old = install(&mut fast);
    let pix = old.rasterize().unwrap();
    assert_eq!(pix, install(&mut slow).rasterize().unwrap());
    for t in [&mut fast, &mut slow] {
        move_to(t, 1, 105., 1);
        drop(install(t));
    }
    let order = ids.iter().rev().copied().collect::<Vec<_>>();
    for edit in [
        set(1, 3, ink(0, 22)),
        patch(1, 1, &[edges(1, 3, &order)]),
        set(30, 5, text("XYZ")),
    ] {
        writer.apply(&edit, 1.).unwrap().install(&mut fast);
        slow.patch(&edit).unwrap();
        let current = install(&mut fast);
        let reference = install(&mut slow);
        assert!(current.rasterize().unwrap() == reference.rasterize().unwrap());
        assert_eq!(hit(&current, 7., 9.), hit(&reference, 7., 9.));
    }
    for t in [&mut fast, &mut slow] {
        move_to(t, 1, 0., 2);
    }
    let next = install(&mut fast);
    let reference = install(&mut slow);
    let changed = next.rasterize().unwrap();
    assert_eq!(changed, reference.rasterize().unwrap());
    assert_ne!(changed, pix);
    assert_eq!(hit(&next, 7., 9.), Some((17, 7., 9.)));
    assert_eq!(old.rasterize().unwrap(), pix);
    assert_eq!(hit(&old, 7., 9.), Some((10, 7., 9.)));
}
// Move behind panels from a sibling onto successive ancestors of the same
// glyph run. The tree does not grow, but the owner of the combined ink changes.
#[test]
fn regrouping_does_not_keep_a_glyph_program_at_every_former_boundary() {
    const LEAF: u64 = 19;
    const BANK: u64 = 40;
    let fixed = || block(&[px(1, 120.), px(2, 30.)]);
    let panel = |i| 100 + i;
    let remaining = |end| (2..end).map(|i| (1, panel(i))).collect::<Vec<_>>();
    let bytes = upload(
        &std::iter::once(node(
            1,
            3,
            block(&[px(1, 120.), px(2, 45.), vec![7, 1]]),
            &[2, BANK],
        ))
        .chain((2..LEAF).map(|i| node(i, 4, fixed(), &[i + 1])))
        .chain([
            node(
                LEAF,
                5,
                block(&[text(&"abcdef".repeat(180))[2..].to_vec(), vec![40, 1]]),
                &[],
            ),
            mounted(node(BANK, 4, fixed(), &[]), &remaining(LEAF)),
        ])
        .chain((2..LEAF).map(|i| {
            node(
                panel(i),
                4,
                block(&[px(1, 3.), px(2, 4.), color([24, i as u8, 26]), vec![40, 1]]),
                &[],
            )
        }))
        .collect::<Vec<_>>(),
    );
    let mut live = Tree::for_session(0);
    let mut writer = Publisher::new(0);
    writer.apply(&bytes, 1.).unwrap().install(&mut live);
    let mut detached = Tree::with_headroom(0);
    let mut oracle = Tree::default();
    oracle.layers.scroll.exhaustive = true;
    for t in [&mut detached, &mut oracle] {
        t.upload(&bytes).unwrap();
    }
    let listener = |s: &paint::Scene| [hit(s, 1., 2.), hit(s, 7., 9.), hit(s, 1., 32.)];
    let mut pins = Vec::new();
    for i in (2..=LEAF).rev() {
        if i != LEAF {
            let edit = patch(
                1,
                1,
                &std::iter::once(edges(1, 3, &[2, BANK]))
                    .chain((2..i).map(|k| edges(k, 4, &[k + 1])))
                    .chain([
                        mounted(edges(i, 4, &[i + 1]), &[(1, panel(i))]),
                        mounted(edges(BANK, 4, &[]), &remaining(i)),
                    ])
                    .collect::<Vec<_>>(),
            );
            writer.apply(&edit, 1.).unwrap().install(&mut live);
            for t in [&mut detached, &mut oracle] {
                t.patch(&edit).unwrap();
            }
        }
        let reference = install(&mut oracle);
        let pixels = reference.rasterize().unwrap();
        let hits = listener(&reference);
        assert!(hits[1].is_some());
        let frame = install(&mut live);
        for s in [&frame, &install(&mut detached)] {
            assert!(s.rasterize().unwrap() == pixels, "regroup {i}");
            assert_eq!(listener(s), hits);
        }
        if pins.len() < 4 {
            pins.push((frame, pixels, hits));
        }
    }
    for (s, pixels, hits) in pins {
        assert_eq!(s.rasterize().unwrap(), pixels);
        assert_eq!(listener(&s), hits);
    }
}
#[test]
fn hidden_replacement_and_hit_only_edit_replace_only_the_affected_program() {
    let mut w = Publisher::new(0);
    let mut t = Tree::for_session(0);
    w.apply(&document(), 1.).unwrap().install(&mut t);
    let original = install(&mut t);
    let key = t.elements.ids[&2];
    let pix = original.rasterize().unwrap();
    w.apply(&set(20, 4, row(20, false, 100)), 1.)
        .unwrap()
        .install(&mut t);
    let no_listener = install(&mut t);
    assert_eq!(t.output.recorded()[0], 0);
    assert!(original.shares_order(&no_listener, key, 0));
    assert!(!original.shares_order(&no_listener, key, 1));
    assert!(original.same_drawing(&no_listener));
    assert_eq!(pix, no_listener.rasterize().unwrap());
    assert!(hit(&no_listener, 5., 6.).is_none());
    assert_eq!(hit(&original, 5., 6.), Some((20, 5., 6.)));
    drop(no_listener);
    move_to(&mut t, 2, 190., 1);
    let far = install(&mut t);
    w.apply(&set(20, 4, row(20, true, 200)), 1.)
        .unwrap()
        .install(&mut t);
    let still_far = install(&mut t);
    assert!(still_far.same_drawing(&far));
    assert_eq!(t.output.recorded(), [0; 2]);
    move_to(&mut t, 2, 0., 2);
    let home = install(&mut t);
    assert_eq!(t.output.recorded(), [1, 1]);
    assert_ne!(pix, home.rasterize().unwrap());
    assert_eq!(hit(&home, 5., 6.), Some((20, 5., 6.)));
    assert_eq!(pix, original.rasterize().unwrap());
}
