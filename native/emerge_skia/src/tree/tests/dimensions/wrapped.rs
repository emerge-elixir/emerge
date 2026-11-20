//! Literal multiline oracle: group by minimum outer widths, then feed each
//! line to the independent f64 scalar-sum model. No production slots/breaks.
use super::*;

fn policy(rng: &mut Rng) -> Expr {
    let w = Fill(Some([0.25, 1., 2., 4.][rng.next(4)]));
    let (a, b) = (Px(rng.next(81) as f64), Px(rng.next(81) as f64));
    match rng.next(9) {
        0 => w,
        1 => pair(true, w, a),
        2 => pair(false, a, w),
        3 => pair(true, a, pair(false, b, w)),
        4 => pair(false, pair(true, a, w), b),
        5 => a,
        6 => Content,
        7 => pair(true, w, Content),
        _ => Auto,
    }
}

fn expected(case: &Case) -> Vec<(f64, f64, f64)> {
    let z = case.display;
    let gap = case.gap * z;
    let inner = ((case.host - 2. * case.pad) * z).max(0.);
    let mut y = case.pad * z;
    let mut boxes = Vec::new();
    let mut kids = case.kids.clone();
    for c in &mut kids {
        // In the generated grammar only the bare and cap-only fills have an
        // automatic floor. Substitute padded content *inside* the cap.
        // For these fixed-content children a content cap equals that floor.
        match &c.expr {
            f @ Fill(_) => c.expr = pair(false, f.clone(), Content),
            Min(f, cap) if matches!(**f, Fill(_)) => {
                c.expr = pair(true, pair(false, *f.clone(), Content), *cap.clone())
            }
            _ => (),
        }
    }
    let mut rest = kids.as_slice();
    while !rest.is_empty() {
        let n = (2..=rest.len())
            .find(|&n| {
                rest[..n].iter().map(|c| c.extent(z, |_| 0.)).sum::<f64>() + gap * (n - 1) as f64
                    > inner
            })
            .map_or(rest.len(), |n| n - 1);
        let (line, tail) = rest.split_at(n);
        let single = Case {
            kids: line.to_vec(),
            host: case.host,
            pad: case.pad,
            gap: case.gap,
            display: z,
        };
        boxes.extend(
            single
                .expected(RowWidth)
                .into_iter()
                .map(|(x, w)| (x, y, w)),
        );
        y += gap + line.iter().map(|c| 12. * c.zoom * z).fold(0., f64::max);
        rest = tail;
    }
    boxes
}

#[test]
fn seeded_wrapped_widths_match_line_math_and_fresh_trees() {
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
        live.upload(&case.under(2, 1)).unwrap();
        for step in 0..8 {
            let i = rng.next(case.kids.len());
            let k = &mut case.kids[i];
            match step {
                1 | 6 => {
                    case.host = if step == 1 { 0. } else { rng.next(240) as f64 };
                    live.patch(&set(1, 2, case.root(1))).unwrap();
                }
                2 | 7 => {
                    k.expr = policy(&mut rng);
                    live.patch(&set(k.id, 4, k.attrs(1))).unwrap();
                }
                3 => {
                    k.demand = rng.next(91) as f64;
                    live.patch(&set(k.id + 100, 6, block(&[px(1, k.demand)])))
                        .unwrap();
                }
                4 => {
                    case.kids.reverse();
                    live.patch(&order(1, 2, &case.ids())).unwrap();
                }
                5 => {
                    k.zoom = [0.5, 1., 2.][rng.next(3)];
                    live.patch(&set(k.id, 4, k.attrs(1))).unwrap();
                    case.display = 1.5;
                }
                _ => (),
            }
            case.settle(&mut live).unwrap();
            let mut cold = Tree::default();
            cold.upload(&case.under(2, 1)).unwrap();
            case.settle(&mut cold).unwrap();
            for (id, f) in cold.frames() {
                assert_eq!(
                    live.layers.frames[live.elements.ids[&id]], f,
                    "seed {seed} step {step} id {id}"
                );
            }
            for (k, (x, y, w)) in case.kids.iter().zip(expected(&case)) {
                let f = live.layers.frames[live.elements.ids[&k.id]];
                for (actual, want) in [(f.x, x), (f.y, y), (f.width, w)] {
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
