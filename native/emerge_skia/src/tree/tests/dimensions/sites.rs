//! Width × height on every mounted kind, not just transpositions of flat lines.
use super::*;

impl Expr {
    fn has_fill(&self) -> bool {
        match self {
            Fill(_) => true,
            Min(a, b) | Max(a, b) => a.has_fill() || b.has_fill(),
            _ => false,
        }
    }
    fn allowed(&self, axis: u8, parent: Option<u8>) -> bool {
        parent != Some(2) || !self.has_fill() || (axis == 1 && matches!(self, Fill(_)))
    }
    fn single(&self, natural: f32, host: f64, independent_offer: bool) -> f32 {
        let eval = |unit| self.eval(f64::from(natural), 1., |_| unit);
        // One child: if a main-axis slot exists it either takes all the room
        // or reaches its minimum/maximum. Roots and cross-axis fills read the offer directly.
        if independent_offer {
            eval(host) as f32
        } else {
            host.clamp(eval(0.), eval(f64::INFINITY)) as f32
        }
    }
}

#[test]
fn width_by_height_at_roots_and_all_supported_parent_child_sites() {
    let lengths = [
        Auto,
        Px(27.),
        Content,
        Fill(None),
        Fill(Some(1.)),
        Fill(Some(0.5)),
        pair(true, Px(15.), Px(90.)),
        pair(false, Content, Px(20.)),
        pair(true, Content, Content),
        pair(false, Fill(Some(2.)), Px(9.)),
        pair(true, Content, Fill(None)),
        pair(false, Fill(None), pair(true, Px(75.), Fill(Some(3.)))),
    ];
    let styles = |w: &Expr, h: &Expr, kind| {
        let mut fields = vec![[vec![3, 0], 2f64.to_be_bytes().to_vec()].concat()];
        if kind == 5 {
            fields.push(vec![21, 0, 1, b'i']);
        }
        fields.extend(w.field(1));
        fields.extend(h.field(2));
        block(&fields)
    };
    for parent in [None, Some(1), Some(2), Some(3), Some(4)] {
        for kind in 1..=6 {
            let (w, h) = (80., 60.);
            let id = if parent.is_some() { 2 } else { 1 };
            let mut nodes: Vec<_> = parent
                .into_iter()
                .map(|p| node(1, p, block(&[px(1, w), px(2, h)]), &[id]))
                .collect();
            nodes.push(node(id, kind, styles(&Content, &Content, kind), &[]));
            let mut t = Tree::default();
            t.upload(&upload(&nodes)).unwrap();
            t.refresh(w as f32, h as f32, 1.).unwrap();
            let key = t.elements.ids[&id];
            let natural = t.layers.frames[key];
            if kind == 5 {
                assert!(natural.width > 4. && natural.height > 4.);
            } else {
                assert_eq!((natural.width, natural.height), (4., 4.));
            }
            let mut accepted = t.encode();
            for width in &lengths {
                for height in &lengths {
                    let edit = set(id, kind, styles(width, height, kind));
                    let result = t.patch(&edit);
                    if !width.allowed(1, parent) || !height.allowed(2, parent) {
                        assert!(result.unwrap_err().contains("WrappedRow child"));
                        assert_eq!(t.encode(), accepted);
                        continue;
                    }
                    result.unwrap();
                    t.refresh(w as f32, h as f32, 1.).unwrap();
                    let b = t.layers.frames[key];
                    assert_eq!((b.x, b.y), (0., 0.));
                    let expected = (
                        width.single(natural.width, w, matches!(parent, None | Some(3 | 4))),
                        height.single(natural.height, h, matches!(parent, None | Some(1 | 4))),
                    );
                    assert_eq!(
                        (b.width, b.height),
                        expected,
                        "{parent:?} → {kind:?}, {width:?} × {height:?}"
                    );
                    accepted = t.encode();
                }
            }
        }
    }
}

#[test]
fn a_mixed_wrapped_height_reads_reflow_only_when_there_is_a_slot() {
    let mut t = Tree::default();
    let chip = || block(&[px(1, 48.), px(2, 20.)]);
    t.upload(&upload(&[
        node(
            1,
            3,
            block(&[px(1, 100.), px(2, 120.), number(4, 4.)]),
            &[2, 3],
        ),
        node(
            2,
            2,
            block(&[vec![1, 0], vec![2, 4, 1, 0], number(4, 4.)]),
            &[4, 5],
        ),
        node(4, 6, chip(), &[]),
        node(5, 6, chip(), &[]),
        node(3, 4, block(&[vec![2, 0]]), &[6]),
        node(6, 6, block(&[px(2, 12.)]), &[]),
    ]))
    .unwrap();
    for w in [100., 99.5, 80., 100.5] {
        for (h, definite) in [(120., true), (44., true), (0., true), (0., false)] {
            for min in [true, false] {
                let root = block(&[
                    px(1, w),
                    if definite { px(2, h) } else { vec![2, 1] },
                    number(4, 4.),
                ]);
                t.patch(&set(1, 3, root)).unwrap();
                t.patch(&set(
                    2,
                    2,
                    block(&[
                        vec![1, 0],
                        vec![2, if min { 4 } else { 5 }, 1, 0],
                        number(4, 4.),
                    ]),
                ))
                .unwrap();
                t.refresh(140., 120., 1.).unwrap();
                let frames = |id| t.layers.frames[t.elements.ids[&id]];
                let content: f32 = if w >= 100. { 20. } else { 44. };
                let share = (h as f32 - 4.).max(0.) / 2.;
                let a = if !definite {
                    20.
                } else if min {
                    content.min(share)
                } else {
                    content.max(share)
                };
                let b = if definite {
                    (2. * share - a).max(0.)
                } else {
                    12.
                };
                assert_eq!(frames(1).height, if definite { h as f32 } else { 36. });
                assert_eq!(frames(2).height, a);
                assert_eq!((frames(3).y, frames(3).height), (a + 4., b));
                assert_eq!(frames(5).y, if w >= 100. { 0. } else { 24. });
            }
        }
    }
}

#[test]
fn a_mixed_root_uses_the_viewport_not_weight_times_viewport_and_reacts_to_unchanged_size_policy_metadata()
 {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let shape = weighted::host(axis);
        let cap = pair(true, Content, Fill(Some(3.)));
        let floor = pair(false, Content, Fill(Some(3.)));
        let root = |l: &Expr| weighted::size(axis, l.field(axis).unwrap());
        r.tree
            .upload(&upload(&[
                node(1, shape, root(&cap), &[2]),
                node(2, 6, block(&[px(axis, 80.)]), &[]),
            ]))
            .unwrap();
        for (l, host, want) in [
            (&cap, 50., 50.),
            (&floor, 50., 80.),
            (&floor, 100., 100.),
            (&cap, 100., 80.),
        ] {
            r.tree.patch(&set(1, shape, root(l))).unwrap();
            r.tree.refresh(host, host, 1.).unwrap();
            assert_eq!(weighted::span(&r, axis, 1), (0., want));
        }
    }
}

#[test]
fn a_closed_mixed_child_checks_its_new_site_and_scale_without_installing_a_refused_candidate() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        let host = weighted::host(axis);
        let other = weighted::host(3 - axis);
        let sty = |n| {
            weighted::panel(
                axis,
                2,
                pair(true, Fill(None), pair(true, Content, Px(n)))
                    .field(axis)
                    .unwrap(),
            )
        };
        r.tree
            .upload(&upload(&[
                node(1, other, block(&[]), &[2, 3]),
                node(2, host, block(&[px(axis, 120.)]), &[4, 5]),
                node(4, 4, sty(20.), &[6]),
                node(6, 6, block(&[px(axis, 90.)]), &[]),
                node(5, 4, weighted::panel(axis, 3, vec![axis, 0]), &[]),
                node(3, host, block(&[px(axis, 120.), number(78, 2.)]), &[]),
            ]))
            .unwrap();
        let visible = draw(&mut r).to_vec();
        let (x, y) = if axis == 1 { (35., 5.) } else { (5., 35.) };
        let target = r.target_at(x, y).unwrap();
        assert_eq!(target.1, 5);
        r.tree.patch(&set(4, 4, sty(2e38))).unwrap();
        let pending = r.tree.encode();
        assert!(
            r.tree
                .patch(&patch(1, 90, &[node(90, 2, block(&[]), &[4])]))
                .unwrap_err()
                .contains("WrappedRow child")
        );
        let moved = patch(
            1,
            1,
            &[
                edges(1, other, &[2, 3]),
                edges(2, host, &[5]),
                edges(3, host, &[4]),
            ],
        );
        assert_eq!(
            r.tree.patch(&moved).unwrap_err(),
            "scaled length exceeds device range"
        );
        assert_eq!(r.tree.encode(), pending);
        assert_eq!(
            r.prepare(180., 170., 1.)
                .unwrap()
                .submit(|_, _| Err("refused mixed split".into()))
                .unwrap_err(),
            "refused mixed split"
        );
        assert_eq!(r.pixels(), visible);
        assert_eq!(r.target_at(x, y), Some(target));
        draw(&mut r);
        assert_eq!(r.target_at(x, y).unwrap().1, 4);
        r.tree.patch(&set(4, 4, sty(20.))).unwrap();
        r.tree.patch(&moved).unwrap();
        draw(&mut r);
        assert_eq!(weighted::span(&r, axis, 4).1, 40.);
    }
}
