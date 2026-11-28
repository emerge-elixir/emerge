use super::*;

#[test]
fn malformed_mounts_and_over_budget_bands_leave_both_accepted_trees_intact() {
    let good = upload(&[node(1, 4, chip(40., 40., [8, 9, 10], true), &[])]);
    let mut w = Publisher::new(0);
    let mut t = Raster::default();
    let mut pinned = Tree::for_session(0);
    t.tree.upload(&good).unwrap();
    w.apply(&good, 1.).unwrap().install(&mut pinned);
    draw(&mut t);
    pinned.refresh(50., 50., 1.).unwrap();
    let pin = pinned.seal().unwrap();
    let n = MAX_DEPTH as u64 + 1;
    let trunk = |id, refs: &[(u8, u64)], opacity| {
        mounted(
            node(
                id,
                4,
                if opacity {
                    block(&[number(35, 0.5)])
                } else {
                    block(&[])
                },
                &[],
            ),
            refs,
        )
    };
    let too_deep = patch(
        1,
        1,
        &(1..=n + 1)
            .map(|id| trunk(id, &if id <= n { vec![(6, id + 1)] } else { vec![] }, false))
            .collect::<Vec<_>>(),
    );
    let leaves = (n..n + 4096).map(|id| (6, id)).collect::<Vec<_>>();
    let too_many_groups = patch(
        1,
        1,
        &(1..n)
            .map(|id| {
                if id < n - 1 {
                    trunk(id, &[(6, id + 1)], true)
                } else {
                    trunk(id, &leaves, true)
                }
            })
            .chain(leaves.iter().map(|&(_, id)| node(id, 4, block(&[]), &[])))
            .collect::<Vec<_>>(),
    );
    let mut short = patch(1, 1, &[mounted(edges(1, 4, &[]), &[(6, 2)])]);
    short.pop();
    for (bad, why) in [
        (
            patch(1, 1, &[mounted(edges(1, 4, &[]), &[(255, 2)])]),
            "invalid Nearby slot 255",
        ),
        (short, "truncated EMRG data"),
        (
            patch(
                1,
                1,
                &[
                    mounted(edges(1, 4, &[2]), &[(3, 2)]),
                    node(2, 4, block(&[]), &[]),
                ],
            ),
            "cycle or duplicate child",
        ),
        (too_deep, "tree depth exceeds"),
        (
            too_many_groups,
            "Nearby opacity groups exceed storage limit",
        ),
    ] {
        let a = t.tree.patch(&bad).unwrap_err();
        assert!(a.contains(why), "{a}, expected {why}");
        let a = w.apply(&bad, 1.).err().unwrap();
        assert!(a.contains(why), "{a}, expected {why}");
        assert_eq!(t.tree.encode(), good);
        assert_eq!(t.hit_at(1., 1.), Some((1, 1., 1.)));
        assert_eq!(pin.hits.target_at(pin.viewport, 1., 1.).unwrap().1, 1);
    }
    w.apply(&set(1, 4, chip(40., 40., [8, 9, 10], true)), 1.)
        .unwrap()
        .install(&mut pinned);
    assert_eq!(pinned.encode(), good);
}

#[test]
fn late_anchor_proof_follows_attrs_only_edits_and_closed_branch_moves() {
    let fill = block(&[vec![1, 0], px(2, 10.)]);
    let turn = block(&[px(1, 20.), vec![2, 0], number(79, 90.)]);
    let fixed = || block(&[px(1, 20.), px(2, 10.)]);
    let paragraph = || node(4, 7, fixed(), &[]);
    let head = |s, attrs| mounted(node(1, s, attrs, &[]), &[(6, 2)]);
    for (start, bad, why) in [
        // The panel alone changes; its host isn't named by the patch.
        (
            upload(&[head(14, block(&[])), node(2, 4, fixed(), &[])]),
            set(2, 4, fill.clone()),
            "width needs an anchor known before Place",
        ),
        (
            upload(&[head(4, block(&[])), node(2, 4, fixed(), &[])]),
            set(2, 4, turn.clone()),
            "height-dependent width needs a definite anchor height",
        ),
        // The host alone changes and its panel is a kept declaration.
        (
            upload(&[head(14, fixed()), node(2, 4, fill, &[])]),
            set(1, 14, block(&[])),
            "width needs an anchor known before Place",
        ),
        // Float -> paragraph continuation, and moving a closed El -> Text
        // branch into a paragraph. Neither edit names the panel.
        (
            upload(&[
                node(1, 7, fixed(), &[2]),
                mounted(node(2, 4, block(&[vec![5, 0]]), &[3]), &[(2, 5)]),
                node(3, 5, text("a"), &[]),
                node(5, 4, fixed(), &[]),
            ]),
            set(2, 4, block(&[])),
            "not a paragraph continuation",
        ),
        (
            upload(&[
                node(1, 1, fixed(), &[2, 4]),
                node(2, 4, fixed(), &[3]),
                mounted(node(3, 5, text("story"), &[]), &[(2, 5)]),
                node(5, 4, fixed(), &[]),
                paragraph(),
            ]),
            patch(1, 1, &[edges(1, 1, &[4]), edges(4, 7, &[2])]),
            "not a paragraph continuation",
        ),
        // In shared document flow a declared paragraph can grow with lines.
        (
            upload(&[
                node(1, 8, fixed(), &[2]),
                mounted(node(2, 7, fixed(), &[]), &[(6, 3)]),
                node(3, 4, fixed(), &[]),
            ]),
            set(3, 4, turn),
            "before document reflow",
        ),
    ] {
        let mut r = Raster::default();
        let mut live = Tree::for_session(0);
        let mut w = Publisher::new(0);
        r.tree.upload(&start).unwrap();
        w.apply(&start, 1.).unwrap().install(&mut live);
        let before = draw(&mut r).to_vec();
        let a = r.tree.patch(&bad).unwrap_err();
        assert!(a.contains(why), "{a}, expected {why}");
        let a = w.apply(&bad, 1.).err().unwrap();
        assert!(a.contains(why), "{a}, expected {why}");
        assert_eq!(r.tree.encode(), start);
        assert_eq!(live.encode(), start);
        assert!(draw(&mut r) == before);
    }
}

#[test]
fn mounted_paragraph_and_early_height_projection_update_forward_only() {
    let host = |w, h| block(&[px(1, w), px(2, h)]);
    let mut r = Raster::default();
    r.tree
        .upload(&upload(&[
            mounted(node(1, 4, host(90., 40.), &[]), &[(4, 2), (3, 4)]),
            node(2, 7, block(&[vec![1, 0]]), &[3]),
            node(3, 5, text("AA BB CC DD EE FF"), &[]),
            node(4, 4, block(&[px(1, 20.), vec![2, 0], number(79, 90.)]), &[]),
        ]))
        .unwrap();
    let mut prev = None;
    for (w, h, rewrap) in [
        (90., 40., true),
        (45., 40., true),
        (45., 24., false),
        (90., 40., true),
    ] {
        let solves = r.tree.layers.paragraphs.solves;
        r.tree.patch(&set(1, 4, host(w, h))).unwrap();
        draw(&mut r);
        assert_eq!(
            r.tree.layers.paragraphs.solves - solves,
            usize::from(rewrap)
        );
        let text = bounds(&r, 2);
        assert_eq!((text.width, text.y), (w as f32, h as f32));
        if let Some(prev) = prev
            && w == 45.
            && rewrap
        {
            assert!(text.height > prev);
        }
        prev = Some(text.height);
        assert_eq!(
            bounds(&r, 4),
            Rect {
                x: w as f32,
                y: 0.,
                width: h as f32,
                height: 20.
            }
        );
        fresh(&r, &[(10., 65.), (103., 6.)]);
    }
}
