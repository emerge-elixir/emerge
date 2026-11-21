//! Cross heights on several lines. Group fixed outer widths independently,
//! evaluate authored expressions directly in f64, and compare every frame to
//! a fresh tree as well. No production breaks, height constraints or folds.
use super::*;

fn host(c: &Case, h: &Expr) -> Bytes {
    let mut fields = vec![
        px(1, c.host),
        number(4, c.gap),
        [vec![3, 0], c.pad.to_be_bytes().to_vec()].concat(),
    ];
    fields.extend(h.field(2));
    block(&fields)
}
fn document(c: &Case, h: &Expr) -> Bytes {
    upload(
        &std::iter::once(node(1, 2, host(c, h), &c.ids()))
            .chain(c.kids.iter().flat_map(|k| {
                [
                    node(k.id, 4, k.attrs(2), &[k.id + 100]),
                    node(k.id + 100, 6, block(&[px(2, k.demand)]), &[]),
                ]
            }))
            .collect::<Vec<_>>(),
    )
}

fn verify(c: &Case, policy: &Expr, natural: bool, t: &Tree) {
    let z = c.display;
    let (pad, gap) = (c.pad * z, c.gap * z);
    let width = |k: &Child| 12. * k.zoom * z;
    let kid_h = |k: &Child, offer| k.extent(z, |_| offer);
    let highest = c
        .kids
        .iter()
        .map(|k| kid_h(k, (k.demand + 2. * k.pad) * k.zoom * z))
        .fold(0., f64::max);
    let initial = highest + 2. * pad;
    let natural_offer = (policy.eval(initial, z, |_| initial) - 2. * pad).max(0.);
    let mut remaining = &c.kids[..];
    let lines: Vec<_> = std::iter::from_fn(|| {
        if remaining.is_empty() {
            return None;
        }
        let n = (2..=remaining.len())
            .find(|&i| {
                remaining[..i].iter().map(width).sum::<f64>() + (i - 1) as f64 * gap
                    > ((c.host - 2. * c.pad) * z).max(0.)
            })
            .map_or(remaining.len(), |i| i - 1);
        let (line, tail) = remaining.split_at(n);
        remaining = tail;
        Some(line)
    })
    .collect();
    let tallest = |line: &[Child], offer| line.iter().map(|k| kid_h(k, offer)).fold(0., f64::max);
    let packed = 2. * pad
        + lines.iter().map(|l| tallest(l, natural_offer)).sum::<f64>()
        + lines.len().saturating_sub(1) as f64 * gap;
    let outer = policy.eval(packed, z, |_| 512.);
    let offer = if natural {
        natural_offer
    } else {
        (outer - 2. * pad).max(0.)
    };
    let check = |actual: f32, want: f64| {
        assert!(
            (f64::from(actual) - want).abs() <= 8. * f64::from(f32::EPSILON) * want.abs().max(1.),
            "{actual} vs {want}, {policy:?}, {c:?}"
        )
    };
    check(t.layers.frames[t.elements.ids[&1]].height, outer);
    let mut y = pad;
    for line in lines {
        let mut x = pad;
        for k in line {
            let f = t.layers.frames[t.elements.ids[&k.id]];
            for (a, b) in [
                (f.x, x),
                (f.y, y),
                (f.width, width(k)),
                (f.height, kid_h(k, offer)),
            ] {
                check(a, b);
            }
            x += width(k) + gap;
        }
        y += tallest(line, offer) + gap;
    }
}

#[test]
fn seeded_wrapped_heights_match_cross_offers_and_fresh_trees() {
    for seed in seeds() {
        let mut rng = Rng(seed);
        let mut c = specimen(Auto);
        c.kids = (2..5 + rng.next(3))
            .map(|i| Child {
                id: i as u64,
                expr: rng.expr(3),
                demand: rng.next(37) as f64,
                zoom: [0.5, 1., 1.5][rng.next(3)],
                pad: rng.next(4) as f64,
            })
            .collect();
        c.pad = rng.next(5) as f64;
        c.gap = rng.next(6) as f64;
        let mut live = Tree::default();
        for (step, (h, natural)) in [
            (Content, true),
            (Px(54.), false),
            (Fill(Some(3.)), false),
            (pair(true, Content, Px(27.)), true),
            (Auto, true),
            (pair(true, Content, Fill(None)), false),
            (pair(false, Px(30.), Content), true),
            (Px(0.), false),
        ]
        .into_iter()
        .enumerate()
        {
            c.host = rng.next(69) as f64;
            if step == 0 {
                live.upload(&document(&c, &h)).unwrap();
            } else {
                let i = rng.next(c.kids.len());
                let k = &mut c.kids[i];
                k.expr = rng.expr(4);
                k.zoom = [0.5, 1., 2.][rng.next(3)];
                k.demand = rng.next(51) as f64;
                live.patch(&set(k.id, 4, k.attrs(2))).unwrap();
                live.patch(&set(k.id + 100, 6, block(&[px(2, k.demand)])))
                    .unwrap();
                c.kids.reverse();
                live.patch(&order(1, 2, &c.ids())).unwrap();
                live.patch(&set(1, 2, host(&c, &h))).unwrap();
                c.display = [1., 1.5, 2.][rng.next(3)];
            }
            c.settle(&mut live).unwrap();
            verify(&c, &h, natural, &live);
            let mut cold = Tree::default();
            cold.upload(&document(&c, &h)).unwrap();
            c.settle(&mut cold).unwrap();
            for (id, f) in cold.frames() {
                assert_eq!(
                    live.layers.frames[live.elements.ids[&id]], f,
                    "seed {seed} step {step} id {id}"
                );
            }
        }
    }
}
