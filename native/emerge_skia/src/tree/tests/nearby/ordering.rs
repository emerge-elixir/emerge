use super::*;

// Only the test constructs stacking addresses and sorts. With all boxes at
// (0,0) a packed item's position also determines observable input precedence.
fn addresses(refs: &[Vec<(u8, u64)>], id: u64, layer: u128, bit: u32, out: &mut Vec<(u128, u64)>) {
    let kids = &refs[id as usize - 1];
    let boundary = kids.iter().any(|&(t, _)| t == 6);
    out.push((layer, id));
    for tag in [1, 0, 6] {
        for &(_, c) in kids.iter().filter(|&&(t, _)| t == tag) {
            addresses(
                refs,
                c,
                if tag == 6 {
                    layer | (1u128 << bit)
                } else {
                    layer
                },
                bit - u32::from(boundary),
                out,
            );
        }
    }
}
#[test]
fn native_bands_match_an_independent_address_sort_including_escapes_out_of_behinds() {
    let mut state = 20261123u32;
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state as usize
    };
    for size in 2..100 {
        let mut refs = vec![Vec::new(); size];
        for i in 1..size {
            let p = random() % i;
            let choices = [0, 1, 6, 6, 1, 0, 6];
            let mut tag = choices[random() % choices.len()];
            if tag == 0 && refs[p].iter().any(|&(t, _)| t == 0) {
                tag = 6;
            }
            refs[p].push((tag, i as u64 + 1));
        }
        let tree = upload(
            &refs
                .iter()
                .enumerate()
                .map(|(i, kids)| {
                    let ordinary = kids
                        .iter()
                        .filter_map(|&(t, c)| (t == 0).then_some(c))
                        .collect::<Vec<_>>();
                    let mounts = kids
                        .iter()
                        .copied()
                        .filter(|&(t, _)| t != 0)
                        .collect::<Vec<_>>();
                    mounted(
                        node(
                            i as u64 + 1,
                            4,
                            chip(20., 20., [i as u8 + 1, 0, 0], true),
                            &ordinary,
                        ),
                        &mounts,
                    )
                })
                .collect::<Vec<_>>(),
        );
        let mut oracle = Vec::new();
        addresses(&refs, 1, 0, 127, &mut oracle);
        oracle.sort_by_key(|p| p.0);
        let mut t = Tree::default();
        t.upload(&tree).unwrap();
        t.refresh(20., 20., 1.).unwrap();
        let frame = t.seal().unwrap();
        assert!(
            frame.hits.ids().eq(oracle.iter().map(|&(_, id)| id)),
            "{refs:?}"
        );
        let winner = oracle.last().unwrap().1;
        assert_eq!(frame.rasterize().unwrap()[0], winner as u8);
        assert_eq!(
            frame.hits.target_at(frame.viewport, 1., 1.).unwrap().1,
            winner
        );
    }
}

#[test]
fn same_host_definition_order_wins_not_the_slot_number() {
    let mut r = Raster::default();
    let root = |order| mounted(edges(1, 4, &[]), order);
    r.tree
        .upload(&upload(&[
            mounted(
                node(1, 4, chip(50., 30., [1, 2, 3], true), &[]),
                &[(6, 2), (4, 3)],
            ),
            node(2, 4, chip(20., 45., [255, 0, 0], true), &[]),
            node(3, 4, chip(20., 20., [0, 0, 255], true), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    assert_eq!(pixel(&r, 5, 35), [0, 0, 255, 255]);
    assert_eq!(r.hit_at(5., 35.), Some((3, 5., 5.)));
    r.tree
        .patch(&patch(1, 1, &[root(&[(4, 3), (6, 2)])]))
        .unwrap();
    draw(&mut r);
    assert_eq!(pixel(&r, 5, 35), [255, 0, 0, 255]);
    assert_eq!(r.hit_at(5., 35.), Some((2, 5., 35.)));
    fresh(&r, &[(5., 35.), (40., 25.)]);
}

#[test]
fn interleaving_behinds_with_escapes_changes_the_wire_but_neither_output() {
    let mut r = Raster::default();
    let bounds = || chip(20., 20., [255, 0, 0], true);
    let ab = [(1, 2), (1, 3), (6, 4), (6, 5)];
    r.tree
        .upload(&upload(&[
            mounted(node(1, 4, bounds(), &[]), &ab),
            node(2, 4, bounds(), &[]),
            node(3, 4, bounds(), &[]),
            node(4, 4, bounds(), &[]),
            node(5, 4, bounds(), &[]),
        ]))
        .unwrap();
    draw(&mut r);
    let before = r.tree.encode();
    let interleaved = [(6, 4), (1, 2), (6, 5), (1, 3)];
    r.tree
        .patch(&patch(1, 1, &[mounted(edges(1, 4, &[]), &interleaved)]))
        .unwrap();
    draw(&mut r);
    assert_ne!(r.tree.encode(), before);
    assert!(r.tree.layers.trace.is_empty(), "{:?}", r.tree.layers.trace);
    assert_eq!(r.hit_at(2., 2.), Some((5, 2., 2.)));
    fresh(&r, &[(2., 2.)]);
}

#[test]
fn only_the_reordered_nonempty_port_is_repacked_through_an_explicit_guard() {
    let mut w = Publisher::new(80);
    let mut t = Tree::for_session(0);
    let transparent = || block(&[px(1, 20.), px(2, 20.), vec![40, 1]]);
    let root = |tags: &[(u8, u64)]| patch(1, 1, &[mounted(edges(1, 4, &[]), tags)]);
    let ab = [(1, 2), (1, 3), (6, 4), (6, 5)];
    let ba = [(1, 3), (1, 2), (6, 4), (6, 5)];
    let frame = |t: &mut Tree| {
        t.refresh(50., 50., 1.).unwrap();
        t.seal().unwrap()
    };
    w.apply(
        &upload(&[
            mounted(
                node(
                    1,
                    4,
                    block(&[px(1, 40.), px(2, 40.), vec![77, 1], number(35, 0.5)]),
                    &[],
                ),
                &ab,
            ),
            node(2, 4, chip(20., 20., [255, 0, 0], false), &[]),
            node(3, 4, chip(20., 20., [0, 255, 0], false), &[]),
            node(4, 4, transparent(), &[]),
            node(5, 4, transparent(), &[]),
        ]),
        1.,
    )
    .unwrap()
    .install(&mut t);
    let a = frame(&mut t);
    let first = w.apply(&root(&ba), 1.).unwrap();
    let undo = w.apply(&root(&ab), 1.).unwrap();
    first.install(&mut t);
    undo.install(&mut t);
    let same = frame(&mut t);
    assert!(t.layers.trace.is_empty());
    assert!(std::sync::Arc::ptr_eq(&a.drawing, &same.drawing));
    w.apply(&root(&ba), 1.).unwrap().install(&mut t);
    let green = frame(&mut t);
    assert_eq!(t.layers.trace, [(Compose, 1, Outputs::Draw)]);
    assert!(std::sync::Arc::ptr_eq(&a.hits, &green.hits));
    assert_ne!(a.rasterize().unwrap()[..4], green.rasterize().unwrap()[..4]);
    w.apply(&root(&[(1, 3), (1, 2), (6, 5), (6, 4)]), 1.)
        .unwrap()
        .install(&mut t);
    let b = frame(&mut t);
    assert_eq!(t.layers.trace, [(Compose, 1, Outputs::Hit)]);
    assert!(std::sync::Arc::ptr_eq(&green.drawing, &b.drawing));
    assert_eq!(a.hits.target_at(a.viewport, 1., 1.).unwrap().1, 5);
    assert_eq!(b.hits.target_at(b.viewport, 1., 1.).unwrap().1, 4);
}
