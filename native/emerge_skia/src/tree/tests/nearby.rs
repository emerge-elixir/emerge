use super::*;

mod admission;
mod inline;
mod ordering;
mod schedule;
mod scopes;

pub(super) fn mounted(mut record: Bytes, refs: &[(u8, u64)]) -> Bytes {
    assert_eq!(record.split_off(record.len() - 2), [0, 0]);
    record.extend((refs.len() as u16).to_be_bytes());
    for &(tag, id) in refs {
        record.push(tag);
        record.extend(id.to_be_bytes());
    }
    record
}
pub(super) fn chip(w: f64, h: f64, rgb: [u8; 3], click: bool) -> Bytes {
    block(&[px(1, w), px(2, h), color(rgb), vec![40, u8::from(click)]])
}
fn padding(t: f64, l: f64) -> Bytes {
    [3, 1]
        .into_iter()
        .chain([t, 0., 0., l].into_iter().flat_map(f64::to_be_bytes))
        .collect()
}
fn pixel(r: &Raster, x: usize, y: usize) -> [u8; 4] {
    let at = 4 * (180 * y + x);
    r.pixels()[at..at + 4].try_into().unwrap()
}
fn fresh(r: &Raster, points: &[(f32, f32)]) {
    let mut cold = Raster::default();
    let wire = r.tree.encode();
    cold.tree.upload(&wire).unwrap();
    assert_eq!(wire, cold.tree.encode());
    assert!(draw(&mut cold) == r.pixels());
    for (id, b) in cold.tree.frames() {
        assert_eq!(bounds(r, id), b);
    }
    for &(x, y) in points {
        assert_eq!(r.hit_at(x, y), cold.hit_at(x, y));
    }
}
#[test]
fn six_anchor_sites_offer_only_their_active_axes_and_never_charge_the_host() {
    let panel = |fill| {
        block(&[
            if fill { vec![1, 0] } else { px(1, 20.) },
            if fill { vec![2, 0] } else { px(2, 10.) },
            vec![5, 1],
            vec![6, 1],
            vec![40, 1],
            color([255, 0, 0]),
        ])
    };
    let order = |tag| mounted(edges(2, 1, &[]), &[(tag, 3)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, block(&[padding(20., 10.)]), &[2]),
            mounted(
                node(2, 1, block(&[px(1, 80.), px(2, 40.), padding(8., 6.)]), &[]),
                &[(6, 3)],
            ),
            node(3, 4, panel(false), &[4]),
            node(4, 4, block(&[px(1, 20.), px(2, 10.)]), &[]),
        ]))
        .unwrap();
    for fill in [false, true] {
        r.tree.patch(&set(3, 4, panel(fill))).unwrap();
        for (tag, x, y, w, h) in if fill {
            [
                (6, 10., 20., 80., 40.),
                (1, 10., 20., 80., 40.),
                (2, 10., 10., 80., 10.),
                (4, 10., 60., 80., 10.),
                (3, 90., 20., 20., 40.),
                (5, -10., 20., 20., 40.),
            ]
        } else {
            [
                (6, 40., 35., 20., 10.),
                (1, 40., 35., 20., 10.),
                (2, 40., 10., 20., 10.),
                (4, 40., 60., 20., 10.),
                (3, 90., 35., 20., 10.),
                (5, -10., 35., 20., 10.),
            ]
        } {
            r.tree.patch(&patch(2, 2, &[order(tag)])).unwrap();
            draw(&mut r);
            assert_eq!(
                bounds(&r, 3),
                Rect {
                    x,
                    y,
                    width: w,
                    height: h
                },
                "slot {tag} fill {fill}"
            );
            assert_eq!(
                bounds(&r, 1),
                Rect {
                    x: 0.,
                    y: 0.,
                    width: 90.,
                    height: 60.
                }
            );
            let (tx, ty) = ((x + 2.).max(1.), y + 2.);
            assert_eq!(r.hit_at(tx, ty), Some((3, f64::from(tx - x), 2.)));
            fresh(&r, &[(tx, ty), (120., 80.)]);
        }
    }
    // Geometric slot change, unlike a pure reorder, changes the fill offer.
    r.tree.patch(&patch(2, 2, &[order(6)])).unwrap();
    draw(&mut r);
    assert!(ran(&r, Flow, 3) && ran(&r, Place, 3));
    assert!(!ran(&r, Measure, 1) && !ran(&r, Reflow, 2));
}

#[test]
fn roles_reorders_and_cancelled_edits_are_compared_at_settle() {
    let root = |refs: &[(u8, u64)]| patch(1, 1, &[mounted(edges(1, 4, &[2]), refs)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            mounted(
                node(1, 4, chip(60., 60., [1, 1, 1], true), &[2]),
                &[(1, 3), (6, 4), (6, 5)],
            ),
            node(2, 4, chip(10., 10., [2, 0, 0], true), &[]),
            node(3, 4, chip(30., 30., [3, 0, 0], true), &[]),
            node(4, 4, chip(8., 8., [4, 0, 0], true), &[]),
            node(5, 4, chip(8., 8., [5, 0, 0], true), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    for (x, y, id) in [(2, 2, 5), (9, 9, 2), (15, 15, 3), (40, 40, 1)] {
        assert_eq!(pixel(&r, x, y)[0], id);
        assert_eq!(r.hit_at(x as f32, y as f32).unwrap().0, u64::from(id));
    }
    let ab = root(&[(1, 3), (6, 4), (6, 5)]);
    let ba = root(&[(1, 3), (6, 5), (6, 4)]);
    for (p, winner) in [(&ba, 4), (&ab, 5)] {
        r.tree.patch(p).unwrap();
        draw(&mut r);
        assert_eq!(r.tree.layers.trace, [(Compose, 1, Outputs::Both)]);
        assert_eq!(r.hit_at(2., 2.).unwrap().0, winner);
        fresh(&r, &[(2., 2.), (9., 9.), (15., 15.), (40., 40.)]);
    }
    r.tree.patch(&ba).unwrap();
    r.tree.patch(&ab).unwrap();
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
    // Preserve the edge tag on target replacement, then drop it on removal.
    r.tree
        .patch(&patch(
            5,
            7,
            &[node(7, 4, chip(10., 10., [7, 0, 0], true), &[])],
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(r.hit_at(2., 2.).unwrap().0, 7);
    r.tree.patch(&patch(7, 0, &[])).unwrap();
    draw(&mut r);
    assert_eq!(r.hit_at(2., 2.).unwrap().0, 4);
    // Exchange an ordinary child and a mounted root without new identities.
    let changes = mounted(edges(1, 4, &[4]), &[(1, 3), (6, 2)]);
    r.tree.patch(&patch(1, 1, &[changes])).unwrap();
    draw(&mut r);
    assert_eq!(r.hit_at(2., 2.).unwrap().0, 2);
    fresh(&r, &[(2., 2.), (9., 9.), (15., 15.)]);
}

#[test]
fn empty_boundary_changes_descendant_order_but_plain_wrappers_do_not() {
    let sized = || block(&[px(1, 20.), px(2, 20.)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 1, block(&[]), &[2, 4]),
            mounted(node(2, 4, sized(), &[]), &[(6, 3)]),
            node(3, 4, chip(40., 20., [1, 2, 3], true), &[]),
            node(4, 4, sized(), &[5]),
            node(5, 4, sized(), &[6]),
            mounted(node(6, 4, sized(), &[]), &[(6, 7)]),
            node(7, 4, chip(20., 20., [7, 8, 9], true), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let at = (25., 5.);
    assert_eq!(r.hit_at(at.0, at.1).unwrap().0, 7);
    let add = patch(
        4,
        4,
        &[
            mounted(edges(4, 4, &[5]), &[(6, 8)]),
            node(8, 6, block(&[]), &[]),
        ],
    );
    for (edit, winner) in [(&add, 3), (&patch(8, 0, &[]), 7)] {
        r.tree.patch(edit).unwrap();
        draw(&mut r);
        assert_eq!(r.hit_at(at.0, at.1).unwrap().0, winner);
        assert_eq!(pixel(&r, 25, 5)[0], if winner == 3 { 1 } else { 7 });
        fresh(&r, &[at]);
    }
}

#[test]
fn rotated_escape_leaves_an_ordinary_clip_but_not_an_opted_in_clip() {
    let corridor = |clip| block(&[px(1, 40.), px(2, 40.), vec![77, clip]]);
    let panel = |click, alpha| {
        block(&[
            px(1, 20.),
            px(2, 20.),
            vec![40, click],
            number(35, alpha),
            color([255, 0, 0]),
        ])
    };
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(
                1,
                4,
                block(&[
                    px(1, 160.),
                    px(2, 120.),
                    padding(30., 30.),
                    vec![40, 1],
                    color([0, 0, 255]),
                ]),
                &[2],
            ),
            node(2, 3, corridor(0), &[3]),
            mounted(
                node(3, 4, block(&[px(1, 60.), px(2, 20.), number(33, 90.)]), &[]),
                &[(3, 4)],
            ),
            node(4, 4, panel(1, 1.), &[]),
        ]))
        .unwrap();
    let points = [(65., 75.), (80., 80.)];
    draw(&mut r);
    assert_eq!(pixel(&r, 65, 75), [255, 0, 0, 255]);
    let (id, x, y) = r.hit_at(65., 75.).unwrap();
    assert_eq!(id, 4);
    assert!((x - 5.).abs() < 0.001 && (y - 5.).abs() < 0.001);
    r.tree.patch(&set(2, 3, corridor(1))).unwrap();
    assert!(
        r.prepare(180., 170., 1.)
            .unwrap()
            .submit(|_, _| Err("rejected".into()))
            .is_err()
    );
    assert_eq!(pixel(&r, 65, 75), [255, 0, 0, 255]);
    assert_eq!(r.hit_at(65., 75.).unwrap().0, 4);
    draw(&mut r);
    assert_eq!(pixel(&r, 65, 75), [0, 0, 255, 255]);
    assert_eq!(r.hit_at(65., 75.).unwrap().0, 1);
    fresh(&r, &points);
    r.tree.patch(&set(2, 3, corridor(0))).unwrap();
    for alpha in [1., 0.] {
        r.tree.patch(&set(4, 4, panel(0, alpha))).unwrap();
        draw(&mut r);
        assert_eq!(
            pixel(&r, 65, 75),
            if alpha == 1. {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        );
        assert!(r.hit_at(65., 75.).is_none());
        assert_eq!(r.hit_at(80., 80.).unwrap().0, 1);
        fresh(&r, &points);
    }
}

#[test]
fn overlapping_menus_share_their_reopened_opacity_ancestry() {
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            node(1, 4, chip(80., 80., [255; 3], true), &[2]),
            node(2, 1, block(&[number(35, 0.5)]), &[3, 5]),
            mounted(node(3, 4, block(&[px(1, 20.), px(2, 20.)]), &[]), &[(6, 4)]),
            node(4, 4, chip(60., 20., [255, 0, 0], true), &[]),
            mounted(node(5, 4, block(&[px(1, 20.), px(2, 20.)]), &[]), &[(6, 6)]),
            node(6, 4, chip(30., 20., [0, 0, 255], true), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(pixel(&r, 5, 5), [255, 127, 127, 255]);
    assert_eq!(pixel(&r, 25, 5), [127, 127, 255, 255]);
    assert_eq!(r.hit_at(25., 5.), Some((6, 5., 5.)));
    r.tree
        .patch(&set(
            5,
            4,
            block(&[px(1, 20.), px(2, 20.), number(35, 0.5)]),
        ))
        .unwrap();
    draw(&mut r);
    assert_eq!(pixel(&r, 25, 5), [191, 127, 191, 255]);
    fresh(&r, &[(25., 5.)]);
}

#[test]
fn tagged_order_proofs_apply_to_detached_and_chunked_live_input() {
    let good = upload(&[
        mounted(node(1, 6, block(&[]), &[]), &[(6, 2), (6, 3)]),
        node(2, 6, block(&[]), &[]),
        node(3, 6, block(&[]), &[]),
    ]);
    let storage = storage::Storage::default();
    let mut writer = Publisher::with_storage(80, storage.clone());
    let mut live = Tree::for_session(0);
    let mut t = Tree::default();
    let mut stream = Stream::bounded(good.len(), &storage).unwrap();
    for (i, v) in good.chunks(1).enumerate() {
        stream.push(i, v).unwrap();
    }
    let mut draft = writer.prepare(stream.finish().unwrap(), 1.).unwrap();
    while !draft.apply_slice() {}
    draft.check_extent().unwrap();
    draft.publish().install(&mut live);
    writer.release(draft, true);
    t.upload(&good).unwrap();
    assert_eq!(t.encode(), good);
    assert_eq!(live.encode(), good);
    for bad in [
        mounted(edges(1, 6, &[]), &[(7, 2)]),
        mounted(edges(1, 6, &[]), &[(0, 2)]),
        mounted(edges(1, 6, &[]), &[(2, 1)]),
        mounted(edges(1, 6, &[]), &[(6, 2), (1, 2)]),
        mounted(edges(1, 6, &[]), &[(6, 999)]),
        mounted(edges(1, 6, &[2]), &[(1, 3)]),
    ] {
        let bad = patch(1, 1, &[bad]);
        assert!(t.patch(&bad).is_err());
        assert!(writer.apply(&bad, 1.).is_err());
        assert_eq!(t.encode(), good);
        assert_eq!(live.encode(), good);
    }
    let keep = set(3, 6, block(&[vec![40, 1]]));
    let k = live.root.unwrap();
    let old = live
        .elements
        .attrs
        .version
        .as_ref()
        .unwrap()
        .get(k)
        .unwrap()
        .children
        .clone();
    writer.apply(&keep, 1.).unwrap().install(&mut live);
    t.patch(&keep).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &old,
        &live
            .elements
            .attrs
            .version
            .as_ref()
            .unwrap()
            .get(k)
            .unwrap()
            .children
    ));
    assert_eq!(live.encode(), t.encode());
}
