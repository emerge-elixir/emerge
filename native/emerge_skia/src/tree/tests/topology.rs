use super::*;

pub(super) fn insert(parent: u64, index: u16, nodes: &[Bytes]) -> Bytes {
    let sub = upload(nodes);
    [3].into_iter()
        .chain(parent.to_be_bytes())
        .chain(index.to_be_bytes())
        .chain((sub.len() as u32).to_be_bytes())
        .chain(sub)
        .collect()
}
pub(super) fn remove(id: u64) -> Bytes {
    [4].into_iter().chain(id.to_be_bytes()).collect()
}
fn box_attrs(width: f64, rgb: [u8; 3]) -> Bytes {
    block(&[px(1, width), px(2, 20.), color(rgb), vec![40, 1]])
}
fn box_node(id: u64, width: f64, rgb: [u8; 3]) -> Bytes {
    node(id, 4, box_attrs(width, rgb), &[])
}
fn line(c: [u8; 3]) -> Bytes {
    upload(&[
        node(1, 1, block(&[px(1, 80.)]), &[2, 4]),
        box_node(2, 20., [255, 0, 0]),
        box_node(4, 40., c),
    ])
}
fn cold_match(r: &mut Raster, full: &[u8]) {
    let mut cold = Raster::default();
    cold.tree.upload(full).unwrap();
    assert_eq!(draw(r), draw(&mut cold));
    let mut frames: Vec<_> = r.tree.frames().collect();
    frames.sort_by_key(|x| x.0);
    let mut expected: Vec<_> = cold.tree.frames().collect();
    expected.sort_by_key(|x| x.0);
    assert_eq!(frames, expected);
    for x in [0., 5., 20., 25., 45., 65., 80., 120.] {
        assert_eq!(r.hit_at(x, 5.), cold.hit_at(x, 5.));
    }
    assert_eq!(r.tree.encode(), cold.tree.encode());
}
#[test]
fn apply_the_entire_patch_cycle_then_invalidate_once() {
    let mut r = Raster::default();
    let blue = [0, 0, 255];
    let yellow = [255, 255, 0];
    r.tree.upload(&line(blue)).unwrap();
    assert!(r.tree.layers.worksets_empty());
    draw(&mut r);
    let c = r.tree.elements.ids[&4];
    let press = r.target_at(25., 5.).unwrap().0;
    let pass = r.tree.layers.invalidation_passes;
    for p in [
        insert(1, 1, &[box_node(3, 20., [0, 255, 0])]),
        set(4, box_attrs(40., yellow)),
        remove(3),
    ] {
        r.tree.patch(&p).unwrap();
        assert!(r.tree.layers.worksets_empty());
        assert_eq!(r.tree.layers.invalidation_passes, pass);
        assert_eq!(r.target_at(25., 5.).unwrap().0, press);
    }
    cold_match(&mut r, &line(yellow));
    assert_eq!(r.tree.layers.invalidation_passes, pass + 1);
    assert_eq!(
        r.tree.layers.trace,
        [(Compose, 4, Outputs::Draw), (Compose, 1, Outputs::Draw)]
    );
    assert_eq!(r.tree.elements.ids[&4], c);
    // Net-zero declaration and order edits have no downstream work either.
    for p in [
        order(1, &[4, 2]),
        set(4, box_attrs(40., blue)),
        order(1, &[2, 4]),
        set(4, box_attrs(40., yellow)),
    ] {
        r.tree.patch(&p).unwrap();
    }
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
}
#[test]
fn detached_cache_reuses_glyphs_not_attachment_or_inherited_inputs() {
    let doc = |size, s| {
        upload(&[
            node(1, 1, block(&[px(1, 110.), number(16, size)]), &[2]),
            node(2, 4, block(&[vec![40, 1]]), &[3]),
            node(3, 5, text(s), &[]),
        ])
    };
    let subtree = |s| {
        insert(
            1,
            u16::MAX,
            &[
                node(2, 4, block(&[vec![40, 1]]), &[3]),
                node(3, 5, text(s), &[]),
            ],
        )
    };
    let mut r = Raster::default();
    r.tree.upload(&doc(16., "WW")).unwrap();
    draw(&mut r);
    let slot = r.tree.elements.ids[&3];
    let rev = r.tree.layers.runs[slot].revision;
    let target = r.target_at(5., 5.).unwrap().0;
    let old = r.pixels().to_vec();
    // Remove and reattach without an intervening draw: pixels may be accepted,
    // but a press held against the old attachment must not match its new one.
    let draws = r.draws;
    r.tree.patch(&[remove(2), subtree("WW")].concat()).unwrap();
    assert!(r.has_target(target));
    assert_eq!(draw(&mut r), old);
    assert_eq!(r.draws, draws);
    assert!(!r.has_target(target));
    assert!(!ran(&r, Measure, 3));
    assert_eq!(r.tree.layers.runs[slot].revision, rev);
    // The same hit identity is retired even if we actually draw the absence;
    // the detached glyph cache is still reusable when the same wire ID returns.
    r.tree.patch(&remove(2)).unwrap();
    assert!(draw(&mut r).iter().all(|&v| v == 0));
    r.tree.patch(&subtree("WW")).unwrap();
    assert_eq!(draw(&mut r), old);
    assert_eq!(r.tree.elements.ids[&3], slot);
    assert!(!ran(&r, Measure, 3));
    // Edit while attached, detach before that edit is computed, change the
    // inherited font while absent, then reinsert with the pending declaration.
    r.tree.patch(&set(3, text("i"))).unwrap();
    r.tree.patch(&[remove(2), remove(3)].concat()).unwrap();
    r.tree
        .patch(&set(1, block(&[px(1, 110.), number(16, 30.)])))
        .unwrap();
    draw(&mut r);
    assert_eq!(r.tree.frames().count(), 1);
    assert!(r.hit_at(5., 5.).is_none());
    assert!(r.tree.patch(&set(3, text("absent"))).is_err());
    r.tree.patch(&subtree("i")).unwrap();
    cold_match(&mut r, &doc(30., "i"));
    assert_eq!(r.tree.elements.ids[&3], slot);
    assert_eq!(r.tree.layers.runs[slot].metrics.size, 30.);
    assert!(ran(&r, Measure, 3));
    // An incompatible declaration is cold, including its typed payload.
    let green = box_node(3, 20., [0, 255, 0]);
    r.tree
        .patch(&[remove(3), insert(2, 0, std::slice::from_ref(&green))].concat())
        .unwrap();
    cold_match(
        &mut r,
        &upload(&[
            node(1, 1, block(&[px(1, 110.), number(16, 30.)]), &[2]),
            node(2, 4, block(&[vec![40, 1]]), &[3]),
            green,
        ]),
    );
    assert_ne!(r.tree.elements.ids[&3], slot);
    assert!(r.tree.layers.runs.get(slot).is_none());
}
#[test]
fn membership_supplies_fill_offers_even_when_parent_bounds_are_equal() {
    let attrs = block(&[px(1, 100.)]);
    let fixed = box_node(3, 20., [255, 0, 0]);
    let fill = node(4, 4, block(&[vec![1, 0], px(2, 20.), vec![40, 1]]), &[]);
    for kind in [1, 2, 3, 4] {
        let base = upload(&[node(1, kind, attrs.clone(), &[])]);
        let mut r = Raster::default();
        r.tree.upload(&base).unwrap();
        draw(&mut r);
        r.tree
            .patch(&insert(1, 99, std::slice::from_ref(&fill)))
            .unwrap();
        cold_match(
            &mut r,
            &upload(&[node(1, kind, attrs.clone(), &[4]), fill.clone()]),
        );
        assert_eq!(bounds(&r, 4).width, 100.);
        if kind != 4 {
            r.tree
                .patch(&insert(1, 0, std::slice::from_ref(&fixed)))
                .unwrap();
            cold_match(
                &mut r,
                &upload(&[
                    node(1, kind, attrs.clone(), &[3, 4]),
                    fixed.clone(),
                    fill.clone(),
                ]),
            );
            r.tree.patch(&remove(3)).unwrap();
        }
        r.tree.patch(&remove(4)).unwrap();
        cold_match(&mut r, &base);
    }
}
#[test]
fn mixed_batches_resolve_intermediate_ids_and_reject_atomically() {
    let mut r = Raster::default();
    r.tree.upload(&line([0, 0, 255])).unwrap();
    draw(&mut r);
    let fresh = node(7, 3, block(&[]), &[9]);
    let leaf = node(9, 5, text("old"), &[]);
    let nested = insert(1, 1, &[fresh.clone(), leaf]);
    r.tree.patch(&nested).unwrap();
    let before = r.tree.encode();
    let pending = r.tree.elements.touched.clone();
    for bad in [
        insert(1, 0, std::slice::from_ref(&fresh)),
        insert(1, 0, &[node(2, 6, block(&[]), &[])]),
        insert(0, 0, &[node(8, 6, block(&[]), &[])]),
        insert(9, 0, &[node(8, 6, block(&[]), &[])]),
        insert(8, 0, &[node(8, 6, block(&[]), &[])]),
        remove(1),
        order(1, &[2, 7, 7]),
        order(1, &[2, 9, 4]),
        [set(9, block(&[vec![65]])), set(9, text("valid"))].concat(),
        vec![3, 0],
    ] {
        assert!(
            r.tree
                .patch(&[set(4, box_attrs(80., [0, 0, 0])), bad].concat())
                .is_err()
        );
        assert_eq!(r.tree.encode(), before);
        assert_eq!(r.tree.elements.touched, pending);
        assert!(r.tree.layers.worksets_empty());
    }
    let mixed = [
        set(9, text("new")),
        insert(7, 0, &[node(8, 6, block(&[]), &[])]),
        remove(2),
        order(1, &[4, 7]),
        order(7, &[9, 8]),
    ]
    .concat();
    r.tree.patch(&mixed).unwrap();
    cold_match(
        &mut r,
        &upload(&[
            node(1, 1, block(&[px(1, 80.)]), &[4, 7]),
            box_node(4, 40., [0, 0, 255]),
            node(7, 3, block(&[]), &[9, 8]),
            node(9, 5, text("new"), &[]),
            node(8, 6, block(&[]), &[]),
        ]),
    );
}
#[test]
fn live_depth_limit_and_churn_through_fresh_ids() {
    let mut t = Tree::with_headroom(0);
    let deep: Vec<_> = (1..=MAX_DEPTH as u64)
        .map(|id| {
            let next = [id + 1];
            node(
                id,
                3,
                block(&[]),
                if id == MAX_DEPTH as u64 { &[] } else { &next },
            )
        })
        .collect();
    t.upload(&upload(&deep)).unwrap();
    let last = MAX_DEPTH as u64;
    t.patch(&insert(last, 0, &[node(last + 1, 3, block(&[]), &[])]))
        .unwrap();
    let before = t.encode();
    assert_eq!(
        t.patch(&insert(last + 1, 0, &[node(last + 2, 3, block(&[]), &[])]))
            .unwrap_err(),
        "tree depth exceeds 512"
    );
    assert_eq!(t.encode(), before);
    t.refresh(40., 40., 1.).unwrap();
    let mut r = Raster::with_tree(Tree::with_headroom(0));
    r.tree
        .upload(&upload(&[
            node(1, 4, block(&[]), &[2]),
            box_node(2, 20., [0, 0, 255]),
        ]))
        .unwrap();
    draw(&mut r);
    for id in 3..203 {
        r.tree
            .patch(
                &[
                    insert(1, 0, &[box_node(id, 20., [0, 0, 255])]),
                    remove(id - 1),
                ]
                .concat(),
            )
            .unwrap();
        assert_eq!(r.tree.elements.slots.len(), 2);
        if id % 11 == 0 {
            draw(&mut r);
            assert_eq!(r.hit_at(5., 5.), Some((id, 5., 5.)));
        }
    }
    draw(&mut r);
    assert_eq!(r.hit_at(5., 5.), Some((202, 5., 5.)));
}
