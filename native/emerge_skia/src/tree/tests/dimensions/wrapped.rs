//! Literal multiline oracle: group by minimum outer widths, then feed each
//! line to the independent f64 scalar-sum model. No production slots/breaks.
use super::*;

fn policy(rng: &mut Rng) -> Expr {
    if rng.next(8) == 0 { Auto } else { rng.expr(4) }
}

// Literal extended-real model, no production interval folding. Treat fill
// as unbounded below: a finite result is a guaranteed floor, even at zero.
fn break_width(e: &Expr, preferred: f64, automatic: f64) -> f64 {
    let floor = e.eval(preferred, 1., |_| f64::NEG_INFINITY);
    if floor.is_finite() {
        floor
    } else {
        e.eval(preferred, 1., |_| automatic)
    }
}

// Some bodies can wrap below their preferred width, others cannot. Changing
// only `demand` can now change preferred width without changing the minimum.
fn first_chip(k: &Child) -> f64 {
    if k.id.is_multiple_of(2) { 20. } else { 0. }
}
fn line_tree(case: &Case, axis: u8, kind: u8) -> Bytes {
    let root = node(1, kind, case.root(axis), &case.ids());
    upload(
        &std::iter::once(root)
            .chain(case.kids.iter().flat_map(|k| {
                [
                    node(k.id, kind, k.attrs(axis), &[k.id + 200, k.id + 100]),
                    node(k.id + 200, 6, block(&[px(axis, first_chip(k))]), &[]),
                    node(k.id + 100, 6, block(&[px(axis, k.demand)]), &[]),
                ]
            }))
            .collect::<Vec<_>>(),
    )
}

fn expected(case: &Case) -> Vec<(f64, f64, f64)> {
    let z = case.display;
    let gap = case.gap * z;
    let inner = ((case.host - 2. * case.pad) * z).max(0.);
    let mut y = case.pad * z;
    let mut boxes = Vec::new();
    let kids: Vec<_> = case
        .kids
        .iter()
        .map(|k| {
            let first = first_chip(k);
            let preferred = first + k.demand + 2. * k.pad;
            let automatic = first.max(k.demand) + 2. * k.pad;
            let basis = break_width(&k.expr, preferred, automatic);
            let model = Child {
                demand: first + k.demand,
                expr: pair(false, Px(basis), k.expr.clone()),
                ..k.clone()
            };
            (basis * k.zoom * z, model)
        })
        .collect();
    let mut rest = kids.as_slice();
    while !rest.is_empty() {
        let n = (2..=rest.len())
            .find(|&n| {
                rest[..n].iter().map(|(width, _)| width).sum::<f64>() + gap * (n - 1) as f64 > inner
            })
            .map_or(rest.len(), |n| n - 1);
        let (line, tail) = rest.split_at(n);
        let single = Case {
            kids: line.iter().map(|(_, k)| k.clone()).collect(),
            host: case.host,
            pad: case.pad,
            gap: case.gap,
            even: false,
            display: z,
        };
        boxes.extend(
            single
                .expected(RowWidth)
                .into_iter()
                .map(|(x, w)| (x, y, w)),
        );
        y += gap
            + line
                .iter()
                .map(|(_, k)| 12. * k.zoom * z)
                .fold(0., f64::max);
        rest = tail;
    }
    boxes
}

#[test]
fn seeded_wrapped_widths_match_line_math_and_fresh_trees() {
    check_lines(1, 2);
}
#[test]
fn seeded_wrapped_column_heights_match_line_math_and_fresh_trees() {
    check_lines(2, 14);
}
fn check_lines(axis: u8, kind: u8) {
    for seed in seeds() {
        let mut rng = Rng(seed);
        let mut case = specimen(Auto);
        case.kids = (2..4 + rng.next(5))
            .map(|id| Child {
                id: id as u64,
                expr: policy(&mut rng),
                demand: rng.next(61) as f64,
                zoom: [0.5, 1., 1.5][rng.next(3)],
                pad: rng.next(5) as f64,
            })
            .collect();
        case.gap = rng.next(6) as f64;
        case.pad = rng.next(5) as f64;
        let mut live = Tree::default();
        live.upload(&line_tree(&case, axis, kind)).unwrap();
        for step in 0..8 {
            let i = rng.next(case.kids.len());
            let k = &mut case.kids[i];
            match step {
                1 | 6 => {
                    case.host = if step == 1 { 0. } else { rng.next(240) as f64 };
                    live.patch(&set(1, kind, case.root(axis))).unwrap();
                }
                2 | 7 => {
                    k.expr = policy(&mut rng);
                    live.patch(&set(k.id, kind, k.attrs(axis))).unwrap();
                }
                3 => {
                    k.demand = rng.next(91) as f64;
                    live.patch(&set(k.id + 100, 6, block(&[px(axis, k.demand)])))
                        .unwrap();
                }
                4 => {
                    case.kids.reverse();
                    live.patch(&order(1, kind, &case.ids())).unwrap();
                }
                5 => {
                    k.zoom = [0.5, 1., 2.][rng.next(3)];
                    live.patch(&set(k.id, kind, k.attrs(axis))).unwrap();
                    case.display = 1.5;
                }
                _ => (),
            }
            case.settle(&mut live).unwrap();
            let mut cold = Tree::default();
            cold.upload(&line_tree(&case, axis, kind)).unwrap();
            case.settle(&mut cold).unwrap();
            for (id, f) in cold.frames() {
                assert_eq!(
                    live.layers.frames[live.elements.ids[&id]], f,
                    "seed {seed} step {step} id {id}"
                );
            }
            for (k, (x, y, w)) in case.kids.iter().zip(expected(&case)) {
                let f = live.layers.frames[live.elements.ids[&k.id]];
                let actual = if axis == 1 {
                    [f.x, f.y, f.width]
                } else {
                    [f.y, f.x, f.height]
                };
                for (actual, want) in actual.into_iter().zip([x, y, w]) {
                    assert!(
                        (f64::from(actual) - want).abs()
                            <= 8. * f64::from(f32::EPSILON) * want.abs().max(1.),
                        "seed {seed} step {step}, {k:?}: {f:?} vs ({x},{y},{w}); {case:?}"
                    );
                }
            }
        }
    }
}
