//! A single edge with two simultaneous policies. Never mount the multichild
//! generator below an El: even when one axis has zero room the edge is real.
use super::*;

#[derive(Clone, Debug)]
struct Wrapper {
    host: [Expr; 2],
    child: [Expr; 2],
    demand: [f64; 2],
    inset: f64,
    pad: f64,
    zoom: f64,
    display: f64,
    id: Option<u64>,
}
fn attrs(policy: &[Expr; 2], pad: f64, zoom: f64) -> Bytes {
    block(
        &[
            Some([vec![3, 0], pad.to_be_bytes().to_vec()].concat()),
            Some(number(78, zoom)),
            policy[0].field(1),
            policy[1].field(2),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
    )
}
impl Wrapper {
    fn leaf(&self) -> Bytes {
        block(&[px(1, self.demand[0]), px(2, self.demand[1])])
    }
    fn children(&self) -> Vec<Bytes> {
        self.id.map_or_else(Vec::new, |id| {
            vec![
                node(id, 4, attrs(&self.child, self.pad, self.zoom), &[id + 1]),
                node(id + 1, 6, self.leaf(), &[]),
            ]
        })
    }
    fn full(&self) -> Bytes {
        upload(
            &[
                vec![node(
                    1,
                    4,
                    attrs(&self.host, self.inset, 1.),
                    self.id.as_slice(),
                )],
                self.children(),
            ]
            .concat(),
        )
    }
    fn replace_child(&self) -> Bytes {
        patch(
            1,
            1,
            &[vec![edges(1, 4, self.id.as_slice())], self.children()].concat(),
        )
    }
    fn check(&self, tree: &Tree, viewport: [f32; 2]) -> Vec<Rect> {
        let d = self.display;
        let z = d * self.zoom;
        let start = self.inset * d;
        let inset = self.pad * z;
        let (mut host, mut child) = ([0.; 2], [0.; 2]);
        for axis in 0..2 {
            let content = self.demand[axis] * z + 2. * inset;
            let natural = self
                .id
                .map_or(0., |_| self.child[axis].eval(content, z, |_| content));
            host[axis] =
                self.host[axis].eval(natural + 2. * start, d, |_| f64::from(viewport[axis]));
            let available = (host[axis] - 2. * start).max(0.);
            child[axis] = self.child[axis].eval(content, z, |_| available);
        }
        let mut want = vec![(1, [0., 0., host[0], host[1]])];
        if let Some(id) = self.id {
            want.extend([
                (id, [start, start, child[0], child[1]]),
                (
                    id + 1,
                    [
                        start + inset,
                        start + inset,
                        self.demand[0] * z,
                        self.demand[1] * z,
                    ],
                ),
            ]);
        }
        assert_eq!(tree.frames().count(), want.len());
        want.iter()
            .map(|(id, expected)| {
                let actual = tree.layers.frames[tree.elements.ids[id]];
                for (x, e) in [actual.x, actual.y, actual.width, actual.height]
                    .into_iter()
                    .zip(*expected)
                {
                    assert!(
                        (f64::from(x) - e).abs() <= 8. * f64::from(f32::EPSILON) * e.abs().max(1.),
                        "#{id}: {actual:?} expected {expected:?}"
                    );
                }
                actual
            })
            .collect()
    }
}
#[test]
fn seeded_el_edits_match_two_axis_math_and_fresh_trees() {
    for seed in seeds() {
        let mut rng = Rng(seed);
        let mut case = Wrapper {
            host: [Px(120.), Px(80.)],
            child: [rng.expr(3), rng.expr(3)],
            demand: [30., 16.],
            inset: 6.,
            pad: rng.next(4) as f64,
            zoom: 1.,
            display: 1.,
            id: Some(2),
        };
        let mut live = Tree::default();
        let mut viewport = [512., 512.];
        for step in 0..18 {
            let before = case.id.unwrap_or(0);
            let patch_child = |c: &Wrapper| set(c.id.unwrap(), 4, attrs(&c.child, c.pad, c.zoom));
            let patch_host = |c: &Wrapper| set(1, 4, attrs(&c.host, c.inset, 1.));
            let update = match step {
                0 => case.full(),
                1 | 9 => {
                    case.demand = [rng.next(180) as f64, rng.next(140) as f64];
                    set(before + 1, 6, case.leaf())
                }
                2 | 10 => {
                    case.child = [rng.expr(4), rng.expr(4)];
                    patch_child(&case)
                }
                3 => {
                    case.inset = rng.next(12) as f64;
                    case.host = [Auto, Content];
                    patch_host(&case)
                }
                4 => {
                    case.zoom = [0.5, 1., 1.5, 2.][rng.next(4)];
                    case.pad = rng.next(6) as f64;
                    patch_child(&case)
                }
                5 => {
                    case.display = [0.5, 1., 1.5, 2.][rng.next(4)];
                    Vec::new()
                }
                6 => {
                    case.host = [Px(0.), Px(0.)];
                    patch_host(&case)
                }
                7 => {
                    case.host = [Px(160.), Px(120.)];
                    patch_host(&case)
                }
                8 => {
                    case.id = Some(10);
                    case.replace_child()
                }
                11 => {
                    case.id = None;
                    patch(before, 0, &[])
                }
                12 => {
                    case.id = Some(2);
                    case.replace_child()
                }
                14 => {
                    case.host = [rng.expr(4), rng.expr(4)];
                    patch_host(&case)
                }
                15..=17 => {
                    viewport = match step {
                        15 => [rng.next(160) as f32 + 0.5, rng.next(120) as f32 + 0.25],
                        16 => [0., 0.],
                        _ => [120., 80.],
                    };
                    Vec::new()
                }
                _ => {
                    let saved = case.child.clone();
                    case.child = [rng.expr(4), rng.expr(4)];
                    live.patch(&patch_child(&case)).unwrap();
                    case.child = saved;
                    patch_child(&case)
                }
            };
            // Seed and step replay the entire sparse prefix, not just this cold tree.
            let check = || {
                if step == 0 {
                    live.upload(&update)
                } else {
                    live.patch(&update)
                }
                .unwrap();
                live.refresh(viewport[0], viewport[1], case.display as f32)
                    .unwrap();
                let mut cold = Tree::default();
                cold.upload(&case.full()).unwrap();
                cold.refresh(viewport[0], viewport[1], case.display as f32)
                    .unwrap();
                assert_eq!(case.check(&live, viewport), case.check(&cold, viewport));
            };
            if let Err(reason) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(check)) {
                eprintln!("seed {seed} ElChild step {step}: {case:?}");
                std::panic::resume_unwind(reason)
            }
        }
    }
}
