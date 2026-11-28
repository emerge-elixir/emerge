use super::*;

fn strip() -> Bytes {
    block(&[vec![1, 0], px(2, 6.), color([0, 255, 0]), vec![40, 1]])
}
fn side() -> Bytes {
    block(&[
        px(1, 20.),
        vec![2, 0],
        number(79, 90.),
        color([0, 0, 200]),
        vec![40, 1],
    ])
}

#[test]
fn every_host_layer_finishes_place_before_any_layout_of_its_mounts() {
    let box_ = |id, kids: &[u64], panels: &[(u8, u64)]| {
        mounted(node(id, 3, chip(20., 20., [1; 3], true), kids), panels)
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            box_(1, &[2, 3], &[]),
            box_(2, &[], &[(6, 5)]),
            box_(3, &[4], &[]),
            box_(4, &[], &[(1, 6)]),
            box_(5, &[7], &[]),
            box_(6, &[], &[]),
            box_(7, &[], &[(4, 8)]),
            box_(8, &[], &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let rank = |id| match id {
        1..=4 => 0,
        5..=7 => 1,
        _ => 2,
    };
    assert!(
        r.tree
            .layers
            .trace
            .iter()
            .map(|&(pass, id, _)| match pass {
                PaintContext => (3, 0),
                Compose => (3, 1),
                _ => (rank(id), pass as usize),
            })
            .is_sorted(),
        "{:?}",
        r.tree.layers.trace
    );
    r.tree
        .patch(&set(8, 3, chip(30., 12., [2; 3], true)))
        .unwrap();
    draw(&mut r);
    assert!(
        r.tree
            .layers
            .trace
            .iter()
            .all(|&(p, id, _)| id == 8 || matches!(p, PaintContext | Compose))
    );
    fresh(&r, &[(1., 1.), (2., 22.)]);
}

#[test]
fn a_closed_subtree_can_change_rank_without_changing_any_ownership_depth() {
    let mut r = Raster::default();
    let mut w = Publisher::new(0);
    let mut live = Tree::for_session(0);
    let start = upload(&[
        node(1, 3, chip(64., 50., [0, 0, 0], true), &[2, 5]),
        node(2, 4, block(&[vec![1, 0]]), &[3]),
        mounted(
            node(3, 4, chip(32., 10., [255, 0, 0], true), &[]),
            &[(4, 4)],
        ),
        node(4, 4, strip(), &[]),
        node(5, 4, chip(64., 8., [0, 0, 255], true), &[]),
    ]);
    let role = patch(1, 1, &[mounted(edges(1, 3, &[5]), &[(6, 2)])]);
    let back = order(1, 3, &[2, 5]);
    for (wire, rank) in [(&start, 0), (&role, 1), (&back, 0)] {
        r.tree.patch(wire).unwrap();
        w.apply(wire, 1.).unwrap().install(&mut live);
        for t in [&mut r.tree, &mut live] {
            t.refresh(180., 170., 1.).unwrap();
            for (id, depth, extra) in [(2, 1, 0), (3, 2, 0), (4, 3, 1)] {
                assert_eq!(
                    t.elements.depths(t.elements.ids[&id]),
                    [depth, rank + extra]
                );
            }
        }
        draw(&mut r);
        fresh(&r, &[(2., 12.)]);
        assert!(live.seal().unwrap().rasterize().unwrap() == r.pixels());
    }
}

#[test]
fn mounts_follow_packed_and_aspect_width_including_on_unnamed_host_edits() {
    let mut r = Raster::default();
    r.tree.upload(&wrapped_column::shared()).unwrap();
    draw(&mut r);
    r.tree
        .patch(&patch(
            2,
            2,
            &[
                mounted(edges(2, 14, &[3, 4]), &[(6, 8)]),
                node(8, 4, strip(), &[]),
            ],
        ))
        .unwrap();
    for (h, expected) in [(44., 12.), (43., 30.), (44., 12.)] {
        r.tree
            .patch(&set(1, 1, block(&[px(1, 64.), px(2, h)])))
            .unwrap();
        draw(&mut r);
        assert_eq!((bounds(&r, 8).width, bounds(&r, 5).x), (expected, expected));
        fresh(&r, &[(expected - 1., 2.)]);
    }

    let mut r = Raster::default();
    let header = |h| block(&[px(1, 8.), px(2, h)]);
    r.tree
        .upload(&upload(&[
            node(1, 3, block(&[px(1, 160.), px(2, 48.)]), &[2, 3]),
            mounted(
                node(
                    2,
                    9,
                    block(&[images::path(images::WIDE), vec![2, 0], vec![5, 2]]),
                    &[],
                ),
                &[(6, 4)],
            ),
            node(3, 4, header(8.), &[]),
            node(4, 4, strip(), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(bounds(&r, 4).width, 64.);
    images::load(&mut r.tree, &mut images::loader());
    for (h, w) in [(8., 80.), (16., 64.), (8., 80.)] {
        r.tree.patch(&set(3, 4, header(h))).unwrap();
        draw(&mut r);
        assert_eq!(
            (bounds(&r, 2).width, bounds(&r, 4).width, bounds(&r, 4).x),
            (w, w, 160. - w)
        );
        assert_eq!(r.hit_at(162. - w, 2.), Some((4, 2., 2.)));
        let mut twin = Raster::default();
        twin.tree
            .upload(&upload(&[
                node(1, 3, block(&[px(1, 160.), px(2, 48.)]), &[2, 3]),
                node(
                    2,
                    9,
                    block(&[images::path(images::WIDE), vec![2, 0], vec![5, 2]]),
                    &[],
                ),
                node(3, 4, header(h), &[]),
            ]))
            .unwrap();
        images::load(&mut twin.tree, &mut images::loader());
        draw(&mut twin);
        for id in [1, 2, 3] {
            assert_eq!(bounds(&r, id), bounds(&twin, id));
        }
        assert_ne!(twin.tree.layers.flow_width(twin.tree.elements.ids[&2]), w);
    }
}

#[test]
fn a_packs_offer_tracks_a_reflowing_siblings_final_charge_not_the_single_column_basis() {
    let card = |id, w, h| node(id, 4, block(&[px(1, w), px(2, h)]), &[]);
    let doc = |small, mounts| {
        let gaps = [
            vec![36],
            6f64.to_be_bytes().to_vec(),
            4f64.to_be_bytes().to_vec(),
        ]
        .concat();
        let pack = node(2, 14, block(&[vec![1, 1], vec![2, 0], gaps]), &[3, 4]);
        let nodes = [
            node(1, 3, block(&[px(1, 64.), px(2, 53.)]), &[5, 2]),
            mounted(pack, if mounts { &[(6, 8), (3, 9)] } else { &[] }),
            card(3, 12., 20.),
            card(4, 12., 20.),
            node(5, 2, block(&[px(1, 20.)]), &[6, 7]),
            card(6, 12., 9.),
            card(7, small, 9.),
            node(8, 4, strip(), &[]),
            node(9, 4, side(), &[]),
        ];
        upload(&nodes[..if mounts { 9 } else { 7 }])
    };
    let mut r = Raster::default();
    let mut writer = Publisher::new(0);
    let mut live = Tree::for_session(0);
    for (i, (small, w, h)) in [(8., 12., 44.), (9., 30., 35.), (8., 12., 44.)]
        .into_iter()
        .enumerate()
    {
        let mut twin = Raster::default();
        twin.tree.upload(&doc(small, false)).unwrap();
        draw(&mut twin);
        let wire = if i == 0 {
            doc(small, true)
        } else {
            set(7, 4, block(&[px(1, small), px(2, 9.)]))
        };
        r.tree.patch(&wire).unwrap();
        writer.apply(&wire, 1.).unwrap().install(&mut live);
        draw(&mut r);
        for id in 1..=7 {
            assert_eq!(bounds(&r, id), bounds(&twin, id));
        }
        let host = bounds(&r, 2);
        assert_eq!((host.width, host.height), (w, h));
        if i == 1 {
            assert_ne!(twin.tree.layers.flow_width(twin.tree.elements.ids[&2]), w);
        }
        assert_eq!(bounds(&r, 8).width, w);
        assert_eq!(
            (bounds(&r, 9).x, bounds(&r, 9).width, bounds(&r, 9).height),
            (w, h, 20.)
        );
        assert_eq!(
            r.hit_at(w - 1., host.y + 1.),
            Some((8, f64::from(w - 1.), 1.))
        );
        live.refresh(180., 170., 1.).unwrap();
        assert!(live.seal().unwrap().rasterize().unwrap() == r.pixels());
        fresh(&r, &[(w - 1., host.y + 1.)]);
    }
}

#[test]
fn turned_side_uses_the_height_charged_by_flow_or_allocated_in_place() {
    // No measured text metrics in the expected rectangle: the ordinary child
    // (or flex remainder) establishes exactly 64, even in shared document flow.
    for site in [Shape::Paragraph, Shape::El, Shape::Column] {
        let scene = |mounted_panel: bool, h: f64| {
            let charge = if site == Shape::Column { 80. - h } else { h };
            let child = node(3, 4, block(&[px(1, 10.), px(2, charge), vec![5, 0]]), &[]);
            let (root, host) = match site {
                Shape::Paragraph => (
                    node(1, 8, block(&[px(1, 40.)]), &[2]),
                    node(2, 7, block(&[vec![1, 0], px(2, 8.)]), &[3]),
                ),
                Shape::El => (
                    node(1, 3, block(&[px(1, 40.)]), &[2]),
                    node(2, 4, block(&[vec![1, 0]]), &[3]),
                ),
                _ => (
                    node(1, 3, block(&[px(1, 40.), px(2, 80.)]), &[3, 2]),
                    node(2, 3, block(&[vec![1, 0], vec![2, 0]]), &[]),
                ),
            };
            let records = [
                root,
                mounted(host, if mounted_panel { &[(3, 4)] } else { &[] }),
                child,
                node(4, 4, side(), &[]),
            ];
            upload(&records[..if mounted_panel { 4 } else { 3 }])
        };
        let mut r = Raster::default();
        for (i, h) in [64., 32., 64.].into_iter().enumerate() {
            let mut twin = Raster::default();
            twin.tree.upload(&scene(false, h)).unwrap();
            draw(&mut twin);
            if i == 0 {
                r.tree.upload(&scene(true, h)).unwrap();
            } else {
                r.tree
                    .patch(&set(
                        3,
                        4,
                        block(&[
                            px(1, 10.),
                            px(2, if site == Shape::Column { 80. - h } else { h }),
                            vec![5, 0],
                        ]),
                    ))
                    .unwrap();
            }
            draw(&mut r);
            for id in 1..=3 {
                assert_eq!(bounds(&r, id), bounds(&twin, id));
            }
            let f = bounds(&twin, 2);
            assert_eq!(f.height, h as f32);
            assert_eq!(
                bounds(&r, 4),
                Rect {
                    x: f.x + f.width,
                    y: f.y,
                    width: f.height,
                    height: 20.
                }
            );
            let pt = (f.x + f.width + 6., f.y + 4.);
            assert_eq!(pixel(&r, pt.0 as usize, pt.1 as usize), [0, 0, 200, 255]);
            let (id, x, y) = r.hit_at(pt.0, pt.1).unwrap();
            assert_eq!((id, x, y), (4, 4., h - 6.));
            fresh(&r, &[pt]);
        }
    }
}

#[test]
fn document_height_and_multiline_span_supply_distinct_final_anchors() {
    let doc = |w, panels| {
        let ns = &[(3, 7)][..];
        let below = &[(4, 8)][..];
        let span = node(4, 4, block(&[vec![40, 1]]), &[5]);
        let p = node(2, 7, block(&[vec![1, 0], px(2, 8.)]), &[3, 4, 6]);
        let records = [
            node(1, 8, block(&[px(1, w), number(16, 12.)]), &[2]),
            mounted(p, if panels { ns } else { &[] }),
            node(3, 5, text("X "), &[]),
            mounted(span, if panels { below } else { &[] }),
            node(5, 5, text("AA BBB CC DDDDD E F"), &[]),
            node(6, 5, text(" ZZZZZ"), &[]),
            node(7, 4, side(), &[]),
            node(8, 4, strip(), &[]),
        ];
        upload(&records[..if panels { 8 } else { 6 }])
    };
    let mut r = Raster::default();
    let mut writer = Publisher::new(0);
    let mut live = Tree::for_session(0);
    for (i, w) in [160., 44., 160.].into_iter().enumerate() {
        let mut twin = Raster::default();
        twin.tree.upload(&doc(w, false)).unwrap();
        draw(&mut twin);
        let data = if i == 0 {
            doc(w, true)
        } else {
            set(1, 8, block(&[px(1, w), number(16, 12.)]))
        };
        writer.apply(&data, 1.).unwrap().install(&mut live);
        r.tree.patch(&data).unwrap();
        let before = r.tree.layers.paragraphs.solves;
        draw(&mut r);
        assert_eq!(r.tree.layers.paragraphs.solves - before, 1);
        for id in 1..=6 {
            assert_eq!(bounds(&r, id), bounds(&twin, id));
        }
        assert_eq!(r.tree.output.mounted_visits, [2, 2]);
        let fragments = |t: &Raster| {
            t.tree
                .layers
                .paragraphs
                .fragments(t.tree.elements.ids[&2])
                .iter()
                .map(|f| (t.tree.elements.slots[f.owner].id, f.bounds))
                .collect::<Vec<_>>()
        };
        assert_eq!(fragments(&r), fragments(&twin));
        if i == 1 {
            let lines = fragments(&twin)
                .into_iter()
                .filter(|(owner, _)| *owner == 4)
                .map(|(_, b)| b)
                .collect::<Vec<_>>();
            assert!(lines.chunk_by(|a, b| a.y == b.y).count() >= 3);
            assert_ne!(
                lines[0].x + lines[0].width,
                lines.last().unwrap().x + lines.last().unwrap().width
            );
        }
        let p = bounds(&twin, 2);
        let span = bounds(&twin, 4);
        assert!(p.height > 8.);
        assert_eq!(
            bounds(&r, 7),
            Rect {
                x: p.width,
                y: 0.,
                width: p.height,
                height: 20.
            }
        );
        assert_eq!(
            bounds(&r, 8),
            Rect {
                x: span.x,
                y: span.y + span.height,
                width: span.width,
                height: 6.
            }
        );
        let xy = (span.x + 2., span.y + span.height + 2.);
        assert_eq!(r.hit_at(xy.0, xy.1), Some((8, 2., 2.)));
        live.refresh(180., 170., 1.).unwrap();
        let scene = live.seal().unwrap();
        assert_eq!(scene.hits.ids().filter(|&id| id == 8).count(), 1);
        assert!(scene.rasterize().unwrap() == r.pixels());
        assert_eq!(pixel(&r, xy.0 as usize, xy.1 as usize), [0, 255, 0, 255]);
        fresh(&r, &[xy]);
    }
}
