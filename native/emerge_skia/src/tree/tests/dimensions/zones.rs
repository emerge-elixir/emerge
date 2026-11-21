//! Independent model: choose the longest fitting prefix, then position each
//! item from the extents of the members before/after it in its own zone.
use super::*;

#[derive(Clone, Copy, Debug)]
struct Item {
    id: u64,
    main: f64,
    cross: f64,
    zone: u8,
    align: u8,
}
impl Item {
    fn raw(self, axis: u8) -> Bytes {
        block(&[
            px(axis, self.main),
            px(3 - axis, self.cross),
            vec![4 + axis, self.zone],
            vec![7 - axis, self.align],
        ])
    }
}
fn span(items: &[Item], zone: u8) -> f64 {
    let matches = items.iter().filter(|c| c.zone == zone);
    matches.clone().map(|c| c.main).sum::<f64>() + matches.count().saturating_sub(1) as f64 * 2.
}
fn model(mut rest: &[Item], room: f64, wrap: bool) -> Vec<(u64, [f64; 2])> {
    let mut across = 1.;
    let mut out = Vec::new();
    while !rest.is_empty() {
        let count = (2..=rest.len())
            .find(|&n| {
                wrap && rest[..n].iter().map(|c| c.main).sum::<f64>() + 2. * (n - 1) as f64 > room
            })
            .map_or(rest.len(), |n| n - 1);
        let (line, tail) = rest.split_at(count);
        let max = line.iter().map(|c| c.cross).fold(0., f64::max);
        let slot = if wrap { max } else { 30. };
        let left = span(line, 0);
        let right = room - span(line, 2);
        let center = left + ((right - left - span(line, 1)) / 2.).max(0.);
        out.extend(line.iter().enumerate().map(|(i, c)| {
            let before = line[..i]
                .iter()
                .filter(|p| p.zone == c.zone)
                .map(|p| p.main + 2.)
                .sum::<f64>();
            let base = [0., center, right][c.zone as usize];
            (
                c.id,
                [
                    1. + base + before,
                    across + (slot - c.cross) * f64::from(c.align) / 2.,
                ],
            )
        }));
        across += max + 2.;
        rest = tail;
    }
    out
}

#[test]
fn seeded_linear_zones_match_positions_and_fresh_trees() {
    for seed in seeds() {
        for (axis, kind, wrap) in [(1, 1, false), (2, 3, false), (1, 2, true), (2, 14, true)] {
            let mut rng = Rng(seed);
            let mut items = (2..2 + rng.next(8))
                .map(|id| Item {
                    id: id as u64,
                    main: rng.next(85) as f64 / 2.,
                    cross: rng.next(75) as f64 / 2.,
                    zone: rng.next(3) as u8,
                    align: rng.next(3) as u8,
                })
                .collect::<Vec<_>>();
            let mut extent = 64.;
            let mut scale = 1.;
            let root = |extent| {
                block(&[
                    px(axis, extent),
                    px(3 - axis, 32.),
                    [vec![3, 0], 1f64.to_be_bytes().to_vec()].concat(),
                    number(4, 2.),
                ])
            };
            let full = |extent, items: &[Item]| {
                upload(
                    &std::iter::once(node(
                        1,
                        kind,
                        root(extent),
                        &items.iter().map(|c| c.id).collect::<Vec<_>>(),
                    ))
                    .chain(items.iter().map(|c| node(c.id, 6, c.raw(axis), &[])))
                    .collect::<Vec<_>>(),
                )
            };
            let mut live = Tree::default();
            live.upload(&full(extent, &items)).unwrap();
            for step in 0..6 {
                match step {
                    1 | 2 if !items.is_empty() => {
                        let i = rng.next(items.len());
                        let c = &mut items[i];
                        if step == 1 {
                            c.zone = rng.next(3) as u8
                        } else {
                            c.main = rng.next(90) as f64 / 2.
                        };
                        live.patch(&set(c.id, 6, c.raw(axis))).unwrap();
                    }
                    3 => {
                        items.reverse();
                        live.patch(&order(
                            1,
                            kind,
                            &items.iter().map(|c| c.id).collect::<Vec<_>>(),
                        ))
                        .unwrap();
                    }
                    4 | 5 => {
                        extent = if step == 4 { 0. } else { rng.next(130) as f64 };
                        scale = 1.5;
                        live.patch(&set(1, kind, root(extent))).unwrap();
                    }
                    _ => (),
                }
                live.refresh(180., 170., scale).unwrap();
                let mut cold = Tree::default();
                cold.upload(&full(extent, &items)).unwrap();
                cold.refresh(180., 170., scale).unwrap();
                for (id, f) in cold.frames() {
                    assert_eq!(
                        f, live.layers.frames[live.elements.ids[&id]],
                        "seed {seed} step {step} kind {kind} id {id}"
                    );
                }
                for (id, want) in model(&items, (extent - 2.).max(0.), wrap) {
                    let f = live.layers.frames[live.elements.ids[&id]];
                    let got = if axis == 1 { [f.x, f.y] } else { [f.y, f.x] };
                    assert_eq!(
                        got,
                        want.map(|v| v as f32 * scale),
                        "seed {seed} step {step} kind {kind} id {id}: {items:?}"
                    );
                }
            }
        }
    }
}
