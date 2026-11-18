use super::*;

#[test]
fn equivalent_cards_share_intrinsic_values_but_not_offers_hits_or_presses() {
    let scene = |size| {
        upload(&[
            node(1, 1, block(&[]), &[2, 3]),
            node(2, 3, block(&[px(1, 50.), number(16, 16.)]), &[4]),
            node(3, 3, block(&[px(1, 70.), number(16, size)]), &[5]),
            node(4, 4, block(&[vec![1, 0], vec![40, 1]]), &[6]),
            node(5, 4, block(&[vec![1, 0], vec![40, 1]]), &[7]),
            node(6, 5, text("Pay"), &[]),
            node(7, 5, text("Pay"), &[]),
        ])
    };
    let mut r = Raster::default();
    r.tree.upload(&scene(16.)).unwrap();
    draw(&mut r);
    assert_eq!(r.tree.layers.text.shapes, 1);
    // Four distinct non-text descriptors: one card, two columns, one row.
    assert_eq!(r.tree.layers.intrinsic.misses, 4);
    let a = r.target_at(5., 5.).unwrap().0;
    let b = r.target_at(55., 5.).unwrap().0;
    assert_ne!(a, b);
    assert_eq!((bounds(&r, 4).width, bounds(&r, 5).width), (50., 70.));
    r.tree
        .patch(&set(3, 3, block(&[px(1, 70.), number(16, 24.)])))
        .unwrap();
    topology::cold_match(&mut r, &scene(24.));
    assert_eq!(r.tree.layers.text.shapes, 2);
    assert!(bounds(&r, 7).height > bounds(&r, 6).height);
    assert_eq!(r.target_at(55., 5.).unwrap().0, b);
    r.draw(56., 20., 1.).unwrap();
    assert!(r.target_at(57., 5.).is_none());
    assert_eq!(r.tree.layers.text.shapes, 2);
}

#[test]
fn kept_branch_moves_and_promotes_with_its_settled_glyphs_and_final_depths() {
    let mut r = Raster::default();
    let card = node(17, 4, block(&[vec![1, 0], vec![40, 1]]), &[18]);
    let label = node(18, 5, text("Pay"), &[]);
    let left = |kids: &[u64]| node(2, 3, block(&[px(1, 60.), number(16, 24.)]), kids);
    let right = |size, kids: &[u64]| node(3, 3, block(&[px(1, 80.), number(16, size)]), kids);
    let root = node(1, 1, block(&[]), &[2, 3]);
    r.tree
        .upload(&upload(&[
            root.clone(),
            left(&[17]),
            right(12., &[]),
            card.clone(),
            label.clone(),
        ]))
        .unwrap();
    draw(&mut r);
    let pressed = r.target_at(5., 5.).unwrap().0;
    let slot = r.tree.elements.ids[&17];
    r.tree
        .patch(&patch(
            1,
            1,
            &[edges(2, 3, &[]), edges(1, 1, &[2, 3]), edges(3, 3, &[17])],
        ))
        .unwrap();
    assert_eq!(r.target_at(5., 5.).unwrap().0, pressed);
    assert!(r.tree.layers.worksets_empty());
    topology::cold_match(
        &mut r,
        &upload(&[
            root,
            left(&[]),
            right(12., &[17]),
            card.clone(),
            label.clone(),
        ]),
    );
    assert_eq!(r.target_at(65., 5.).unwrap().0, pressed);
    assert_eq!(bounds(&r, 17).width, 80.);
    assert_eq!(r.tree.layers.text.shapes, 2);
    r.tree.patch(&patch(1, 17, &[])).unwrap();
    topology::cold_match(&mut r, &upload(&[card, label]));
    assert_eq!(r.target_at(5., 5.).unwrap().0, pressed);
    assert_eq!(r.tree.root, Some(slot));
    assert_eq!(bounds(&r, 17).width, 180.);
    assert_eq!(r.tree.elements.depth(r.tree.elements.ids[&18]), 1);
    assert_eq!(r.tree.layers.text.shapes, 3); // the viewport supplies a third strike
    r.tree.patch(&patch(17, 17, &[])).unwrap();
    draw(&mut r);
    assert!(r.tree.layers.trace.is_empty());
}

#[test]
fn keep_fields_read_the_last_accepted_declaration_and_validate_closed_height() {
    let pad = |n: f64| block(&[[3, 0].into_iter().chain(n.to_be_bytes()).collect()]);
    let mut t = Tree::default();
    t.upload(&upload(&[
        node(1, 3, block(&[]), &[3]),
        node(3, 3, pad(4.), &[7]),
        node(7, 5, text("a"), &[]),
    ]))
    .unwrap();
    t.patch(&set(3, 3, pad(12.))).unwrap();
    t.patch(&patch(
        3,
        3,
        &[edges(3, 3, &[7]), attr_record(7, 5, text("b"))],
    ))
    .unwrap();
    t.refresh(100., 100., 1.).unwrap();
    assert_eq!(t.layers.frames[t.elements.ids[&7]].x, 12.);
    t.patch(&set(3, 3, block(&[]))).unwrap();
    t.refresh(100., 100., 1.).unwrap();
    assert_eq!(t.layers.frames[t.elements.ids[&7]].x, 0.);
    // Wrap a kept two-level branch below the maximum depth. The explicit
    // path alone fits; only the cached height uncovers the invalid last edge.
    let mut nodes = vec![edges(1, 3, &[100])];
    nodes.extend((100..100 + MAX_DEPTH as u64 - 1).map(|i| {
        node(
            i,
            3,
            block(&[]),
            &[if i == 100 + MAX_DEPTH as u64 - 2 {
                3
            } else {
                i + 1
            }],
        )
    }));
    let before = t.encode();
    assert_eq!(
        t.patch(&patch(1, 1, &nodes)).unwrap_err(),
        "tree depth exceeds 512"
    );
    assert_eq!(t.encode(), before);
}
