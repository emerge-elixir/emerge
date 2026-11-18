use super::*;

fn children(parent: u64, kind: u8, ids: &[u64], defs: &[Bytes]) -> Bytes {
    let mut records = vec![edges(parent, kind, ids)];
    records.extend_from_slice(defs);
    patch(parent, parent, &records)
}
fn remove(id: u64) -> Bytes {
    patch(id, 0, &[])
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
pub(super) fn cold_match(r: &mut Raster, full: &[u8]) {
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
        children(1, 1, &[2, 3, 4], &[box_node(3, 20., [0, 255, 0])]),
        set(4, 4, box_attrs(40., yellow)),
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
        order(1, 1, &[4, 2]),
        set(4, 4, box_attrs(40., blue)),
        order(1, 1, &[2, 4]),
        set(4, 4, box_attrs(40., yellow)),
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
        children(
            1,
            1,
            &[2],
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
    r.tree.patch(&remove(2)).unwrap();
    r.tree.patch(&subtree("WW")).unwrap();
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
    r.tree.patch(&set(3, 5, text("i"))).unwrap();
    r.tree.patch(&remove(2)).unwrap();
    assert!(r.tree.patch(&remove(3)).is_err());
    r.tree
        .patch(&set(1, 1, block(&[px(1, 110.), number(16, 30.)])))
        .unwrap();
    draw(&mut r);
    assert_eq!(r.tree.frames().count(), 1);
    assert!(r.hit_at(5., 5.).is_none());
    assert!(r.tree.patch(&set(3, 5, text("absent"))).is_err());
    r.tree.patch(&subtree("i")).unwrap();
    cold_match(&mut r, &doc(30., "i"));
    assert_eq!(r.tree.elements.ids[&3], slot);
    assert_eq!(r.tree.layers.runs[slot].metrics.size, 30.);
    assert!(ran(&r, Measure, 3));
    // An incompatible declaration is cold, including its typed payload.
    let green = box_node(3, 20., [0, 255, 0]);
    r.tree.patch(&remove(3)).unwrap();
    r.tree
        .patch(&children(2, 4, &[3], std::slice::from_ref(&green)))
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
            .patch(&children(1, kind, &[4], std::slice::from_ref(&fill)))
            .unwrap();
        cold_match(
            &mut r,
            &upload(&[node(1, kind, attrs.clone(), &[4]), fill.clone()]),
        );
        assert_eq!(bounds(&r, 4).width, 100.);
        if kind != 4 {
            r.tree
                .patch(&children(1, kind, &[3, 4], std::slice::from_ref(&fixed)))
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
    let nested = children(1, 1, &[2, 7, 4], &[leaf, fresh.clone()]);
    r.tree.patch(&nested).unwrap();
    let before = r.tree.encode();
    let pending = r.tree.elements.touched.clone();
    for bad in [
        patch(1, 2, &[fresh]),                       // unreachable record
        patch(1, 2, &[node(2, 6, block(&[]), &[])]), // incompatible live kind
        patch(0, 8, &[node(8, 6, block(&[]), &[])]),
        children(9, 5, &[8], &[node(8, 6, block(&[]), &[])]), // leaf arity
        order(7, 3, &[2]),                                    // outside capture
        patch(8, 0, &[]),                                     // absent target
        order(1, 1, &[2, 7, 7]),
        order(1, 1, &[7, 9]), // #7 closes over #9
        patch(1, 1, &[edges(1, 1, &[7, 9]), attr_record(7, 3, block(&[]))]),
        patch(
            7,
            7,
            &[edges(7, 3, &[9]), attr_record(9, 5, block(&[vec![65]]))],
        ),
        patch(7, 8, &[edges(8, 3, &[9])]), // fresh attrs: :same
        patch(7, 8, &[attr_record(8, 3, block(&[]))]), // fresh children: :same
        vec![3, 0],
    ] {
        assert!(r.tree.patch(&bad).is_err());
        assert_eq!(r.tree.encode(), before);
        assert_eq!(r.tree.elements.touched, pending);
        assert!(r.tree.layers.worksets_empty());
    }
    let mixed = patch(
        1,
        1,
        &[
            attr_record(9, 5, text("new")),
            node(8, 6, block(&[]), &[]),
            edges(1, 1, &[4, 7]),
            edges(7, 3, &[9, 8]),
        ],
    );
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
    t.patch(&children(
        last,
        3,
        &[last + 1],
        &[node(last + 1, 3, block(&[]), &[])],
    ))
    .unwrap();
    let before = t.encode();
    assert_eq!(
        t.patch(&children(
            last + 1,
            3,
            &[last + 2],
            &[node(last + 2, 3, block(&[]), &[])]
        ))
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
            .patch(&patch(id - 1, id, &[box_node(id, 20., [0, 0, 255])]))
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
