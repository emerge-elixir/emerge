use super::*;

fn reversed(l: &Expr) -> Expr {
    match l {
        Min(a, b) | Max(a, b) => pair(matches!(l, Min(..)), reversed(b), reversed(a)),
        leaf => leaf.clone(),
    }
}

#[test]
fn commutation_and_idempotence_preserve_slots_on_either_axis() {
    for seed in 0..128 {
        let mut rng = Rng(seed);
        let l = rng.expr(4);
        let mut c = specimen(l.clone());
        c.host = rng.next(201) as f64;
        c.kids[0].pad = 2.;
        c.gap = 1.5;
        for axis in [1, 2] {
            c.kids[0].expr = l.clone();
            let original = c.cold(axis).unwrap();
            for equivalent in [
                reversed(&l),
                pair(true, l.clone(), l.clone()),
                pair(false, l.clone(), l.clone()),
            ] {
                c.kids[0].expr = equivalent;
                assert_eq!(c.cold(axis).unwrap(), original, "seed {seed} axis {axis}");
            }
        }
    }
}

#[test]
fn a_capped_maximum_weight_can_hand_the_slope_to_a_subnormal_weight() {
    let tiny = f64::from_bits(1);
    let a = pair(
        false,
        pair(true, Fill(Some(f64::MAX)), Px(30.)),
        Fill(Some(tiny)),
    );
    let mut c = specimen(a);
    c.kids[1].expr = Fill(Some(tiny));
    c.kids.push(child(4, Fill(Some(3. * tiny)), 0.));
    for axis in [1, 2] {
        let mut r = Raster::default();
        r.tree.upload(&c.upload(axis)).unwrap();
        for (room, expected) in [
            (20., [20., 0., 0.]),
            (120., [30., 22.5, 67.5]),
            (200., [40., 40., 120.]),
        ] {
            c.host = room;
            r.tree
                .patch(&set(1, weighted::host(axis), c.root(axis)))
                .unwrap();
            c.settle(&mut r.tree).unwrap();
            assert_eq!([2, 3, 4].map(|id| weighted::span(&r, axis, id).1), expected);
        }
    }
}

#[test]
fn malformed_subexpressions_never_replace_a_preceding_valid_edit() {
    for axis in [1, 2] {
        let mut r = Raster::default();
        r.tree
            .upload(&content_bounds::doc(axis, vec![axis, 4, 1, 0], 20.))
            .unwrap();
        draw(&mut r);
        let (x, y) = if axis == 1 { (35., 5.) } else { (5., 35.) };
        let screen = r.pixels().to_vec();
        let target = r.target_at(x, y).unwrap();
        assert_eq!(target.1, 3);
        r.tree.patch(&set(4, 6, block(&[px(axis, 90.)]))).unwrap();
        let pending = r.tree.encode();
        for seed in 0..96 {
            let mut rng = Rng(seed);
            let mut l = rng.expr(4).bytes();
            let take = rng.next(l.len());
            l.truncate(take);
            let bad = match seed % 4 {
                0 => l,
                1 => vec![6],
                2 => number(2, [f64::NAN, -1., f64::INFINITY][rng.next(3)]),
                _ => number(3, [0., f64::NEG_INFINITY, f64::NAN][rng.next(3)]),
            };
            let good = rng.expr(3).bytes();
            // Truncate only the final operand, never a prefix followed by valid
            // bytes that could be interpreted as the missing operand.
            let (left, right) = if seed % 4 == 0 || rng.next(2) == 0 {
                (good, bad)
            } else {
                (bad, good)
            };
            let expr = [
                vec![axis, if rng.next(2) == 0 { 4 } else { 5 }],
                left,
                right,
            ]
            .concat();
            let err = r.tree.patch(&set(2, 4, block(&[expr]))).unwrap_err();
            assert!(
                err.contains(
                    [
                        "truncated EMRG data",
                        "unsupported length 6",
                        "expected finite non-negative size",
                        "fill weight must be finite and positive"
                    ][(seed % 4) as usize]
                ),
                "seed {seed}: {err}"
            );
            assert_eq!(r.tree.encode(), pending);
            assert!(r.tree.layers.worksets_empty());
            assert_eq!(r.pixels(), screen);
            assert_eq!(r.target_at(x, y), Some(target));
        }
        draw(&mut r);
        assert_eq!(weighted::span(&r, axis, 2).1, 90.);
        assert_eq!(r.target_at(x, y).unwrap().1, 2);
    }
}
