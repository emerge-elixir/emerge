//! Replayable model of main-axis splits and independent cross-axis offers. Uses
//! literal length math and fixed-size content, not the decoder or allocator.
use super::*;
mod laws;
mod sites;
mod unary;
mod wrapped;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
    RowWidth,
    ColumnHeight,
    ColumnWidth,
    RowHeight,
}
use Site::*;
const SITES: [Site; 4] = [RowWidth, ColumnHeight, ColumnWidth, RowHeight];
impl Site {
    fn axis(self) -> u8 {
        match self {
            RowWidth | ColumnWidth => 1,
            ColumnHeight | RowHeight => 2,
        }
    }
    fn host(self) -> u8 {
        match self {
            RowWidth | RowHeight => 1,
            ColumnWidth | ColumnHeight => 3,
        }
    }
}

#[derive(Clone, Debug)]
enum Expr {
    Auto,
    Px(f64),
    Content,
    Fill(Option<f64>),
    Min(Box<Self>, Box<Self>),
    Max(Box<Self>, Box<Self>),
}
use Expr::*;
fn pair(min: bool, a: Expr, b: Expr) -> Expr {
    if min {
        Min(Box::new(a), Box::new(b))
    } else {
        Max(Box::new(a), Box::new(b))
    }
}
impl Expr {
    fn bytes(&self) -> Bytes {
        match self {
            Auto => unreachable!("omission is not a leaf"),
            Px(n) => number(2, *n),
            Content => vec![1],
            Fill(None) => vec![0],
            Fill(Some(w)) => number(3, *w),
            Min(a, b) | Max(a, b) => [if matches!(self, Min(..)) { 4 } else { 5 }]
                .into_iter()
                .chain(a.bytes())
                .chain(b.bytes())
                .collect(),
        }
    }
    fn field(&self, axis: u8) -> Option<Bytes> {
        (!matches!(self, Auto)).then(|| [axis].into_iter().chain(self.bytes()).collect())
    }
    fn eval(&self, content: f64, scale: f64, share: impl Fn(f64) -> f64 + Copy) -> f64 {
        match self {
            Px(n) => n * scale,
            Auto | Content => content,
            Fill(w) => share(w.unwrap_or(1.)),
            Min(a, b) => a
                .eval(content, scale, share)
                .min(b.eval(content, scale, share)),
            Max(a, b) => a
                .eval(content, scale, share)
                .max(b.eval(content, scale, share)),
        }
    }
}
#[derive(Clone, Debug)]
struct Child {
    id: u64,
    expr: Expr,
    demand: f64,
    zoom: f64,
    pad: f64,
}
impl Child {
    fn attrs(&self, axis: u8) -> Bytes {
        let mut fields = vec![
            px(3 - axis, 12.),
            number(78, self.zoom),
            [vec![3, 0], self.pad.to_be_bytes().to_vec()].concat(),
        ];
        fields.extend(self.expr.field(axis));
        block(&fields)
    }
    fn extent(&self, z: f64, fill: impl Fn(f64) -> f64 + Copy) -> f64 {
        let scale = z * self.zoom;
        self.expr
            .eval((self.demand + 2. * self.pad) * scale, scale, fill)
    }
}
#[derive(Clone, Debug)]
struct Case {
    kids: Vec<Child>,
    host: f64,
    pad: f64,
    gap: f64,
    display: f64,
}
impl Case {
    fn root(&self, axis: u8) -> Bytes {
        block(&[
            px(axis, self.host),
            px(3 - axis, 12.),
            number(4, self.gap),
            [vec![3, 0], self.pad.to_be_bytes().to_vec()].concat(),
        ])
    }
    fn ids(&self) -> Vec<u64> {
        self.kids.iter().map(|c| c.id).collect()
    }
    fn upload(&self, site: Site) -> Bytes {
        self.under(site.host(), site.axis())
    }
    fn under(&self, host: u8, axis: u8) -> Bytes {
        let root = node(1, host, self.root(axis), &self.ids());
        let nodes = std::iter::once(root)
            .chain(self.kids.iter().flat_map(|c| {
                [
                    node(c.id, 4, c.attrs(axis), &[c.id + 100]),
                    node(c.id + 100, 6, block(&[px(axis, c.demand)]), &[]),
                ]
            }))
            .collect::<Vec<_>>();
        upload(&nodes)
    }
    fn settle(&self, t: &mut Tree) -> Result<(), String> {
        t.refresh(512., 512., self.display as f32)
    }
    fn expected(&self, site: Site) -> Vec<(f64, f64)> {
        let z = self.display;
        if matches!(site, ColumnWidth | RowHeight) {
            let inner = ((self.host - 2. * self.pad) * z).max(0.);
            return self
                .kids
                .iter()
                .map(|c| (self.pad * z, c.extent(z, |_| inner)))
                .collect();
        }
        let room =
            ((self.host - 2. * self.pad - self.gap * self.kids.len().saturating_sub(1) as f64) * z)
                .max(0.);
        let sum = |unit: f64| {
            self.kids
                .iter()
                .map(|c| c.extent(z, |w| w * unit))
                .sum::<f64>()
        };
        let unit = if sum(0.) >= room {
            0.
        } else if sum(f64::INFINITY) <= room {
            f64::INFINITY
        } else {
            // Test weights are dyadic in [1/4,4]. Bracket numerically, then
            // bisect the scalar sum; no breakpoint classification or folding.
            let (mut lo, mut hi) = (0., 1.);
            while sum(hi) < room {
                hi *= 2.;
            }
            for _ in 0..64 {
                let u = (lo + hi) / 2.;
                if sum(u) < room {
                    lo = u;
                } else {
                    hi = u;
                }
            }
            hi
        };
        let mut pos = self.pad * z;
        self.kids
            .iter()
            .map(|c| {
                let w = c.extent(z, |w| w * unit);
                let x = pos;
                pos += w + self.gap * z;
                (x, w)
            })
            .collect()
    }
    fn verify(&self, site: Site, t: &Tree) -> Result<Vec<(f32, f32)>, String> {
        let spans = self
            .kids
            .iter()
            .map(|c| {
                let f = t.layers.frames[t.elements.ids[&c.id]];
                if site.axis() == 1 {
                    (f.x, f.width)
                } else {
                    (f.y, f.height)
                }
            })
            .collect::<Vec<_>>();
        for ((x, w), (ex, ew)) in spans.iter().copied().zip(self.expected(site)) {
            let close = |a: f32, b: f64| {
                (f64::from(a) - b).abs() <= 8. * f64::from(f32::EPSILON) * b.abs().max(1.)
            };
            if !close(x, ex) || !close(w, ew) {
                return Err(format!("actual ({x},{w}), expected ({ex},{ew})"));
            }
        }
        Ok(spans)
    }
    fn cold(&self, site: Site) -> Result<Vec<(f32, f32)>, String> {
        let mut t = Tree::default();
        t.upload(&self.upload(site))?;
        self.settle(&mut t)?;
        self.verify(site, &t)
    }
    fn reduced_cold_failure(&self, site: Site) -> Option<Self> {
        let error = self.cold(site).err()?;
        // Do not reduce a geometry failure to e.g. a decoder rejection, or
        // present a cold case as the reproduction of an incremental-only bug.
        let reproduces = |case: &Self| {
            case.cold(site).is_err_and(|e| {
                e == error || (error.starts_with("actual (") && e.starts_with("actual ("))
            })
        };
        let mut c = self.clone();
        for id in self.ids() {
            let mut trial = c.clone();
            trial.kids.retain(|k| k.id != id);
            if !trial.kids.is_empty() && reproduces(&trial) {
                c = trial;
            }
        }
        for i in 0..c.kids.len() {
            while let Min(a, b) | Max(a, b) = c.kids[i].expr.clone() {
                let simpler = [*a, *b].into_iter().find(|l| {
                    let mut trial = c.clone();
                    trial.kids[i].expr = l.clone();
                    reproduces(&trial)
                });
                if let Some(l) = simpler {
                    c.kids[i].expr = l
                } else {
                    break;
                }
            }
        }
        Some(c)
    }
}
fn child(id: u64, l: Expr, demand: f64) -> Child {
    Child {
        id,
        expr: l,
        demand,
        zoom: 1.,
        pad: 0.,
    }
}
fn specimen(a: Expr) -> Case {
    Case {
        kids: vec![child(2, a, 20.), child(3, Fill(None), 90.)],
        host: 120.,
        gap: 0.,
        pad: 0.,
        display: 1.,
    }
}

#[test]
fn every_ordered_pair_and_leaf_on_both_axes() {
    let leaves = [
        Px(0.),
        Px(50.),
        Content,
        Fill(None),
        Fill(Some(1.)),
        Fill(Some(3.)),
    ];
    let mut failures = Vec::new();
    for site in SITES {
        for l in [Auto].into_iter().chain(leaves.clone()) {
            specimen(l).cold(site).unwrap();
        }
        for a in &leaves {
            for b in &leaves {
                for min in [true, false] {
                    let l = pair(min, a.clone(), b.clone());
                    if let Err(e) = specimen(l.clone()).cold(site) {
                        failures.push((site, l, e));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} pair failures, first: {:?}",
        failures.len(),
        failures.first()
    );
}

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) % n as u64) as usize
    }
    fn expr(&mut self, d: usize) -> Expr {
        if d != 0 && self.next(4) != 0 {
            pair(self.next(2) == 0, self.expr(d - 1), self.expr(d - 1))
        } else {
            match self.next(4) {
                0 => Px(self.next(90) as f64 / 2.),
                1 => Content,
                2 => Fill(None),
                _ => Fill(Some([0.25, 1., 2., 4.][self.next(4)])),
            }
        }
    }
}
fn seeds() -> std::ops::Range<u64> {
    let first = std::env::var("EMERGE_DIM_SEED").map_or(0, |s| s.parse().unwrap());
    let count: u64 = std::env::var("EMERGE_DIM_CASES").map_or(64, |s| s.parse().unwrap());
    first..first + count
}
#[test]
fn seeded_dimension_edits_match_math_and_fresh_trees() {
    for seed in seeds() {
        for site in SITES {
            let axis = site.axis();
            let host = site.host();
            let mut rng = Rng(seed);
            let mut c = specimen(Auto);
            c.kids = (0..2 + rng.next(4))
                .map(|i| Child {
                    id: i as u64 + 2,
                    expr: rng.expr(3),
                    demand: rng.next(121) as f64,
                    zoom: [0.5, 1., 1.5][rng.next(3)],
                    pad: rng.next(4) as f64,
                })
                .collect();
            let mut live = Tree::default();
            for step in 0..13 {
                let i = rng.next(c.kids.len());
                let k = &mut c.kids[i];
                let wire = match step {
                    0 => c.upload(site),
                    1 | 8 => {
                        k.demand = rng.next(181) as f64;
                        set(k.id + 100, 6, block(&[px(axis, k.demand)]))
                    }
                    2 => {
                        k.expr = rng.expr(4);
                        set(k.id, 4, k.attrs(axis))
                    }
                    3 => {
                        c.pad = rng.next(8) as f64;
                        c.gap = rng.next(6) as f64;
                        c.host = rng.next(240) as f64;
                        set(1, host, c.root(axis))
                    }
                    4 => {
                        k.zoom = [0.5, 1., 1.5, 2.][rng.next(4)];
                        set(k.id, 4, k.attrs(axis))
                    }
                    5 => {
                        c.kids.reverse();
                        order(1, host, &c.ids())
                    }
                    6 => {
                        c.display = [0.5, 1., 1.5, 2.][rng.next(4)];
                        Vec::new()
                    }
                    7 | 9 => {
                        c.host = if step == 7 { 0. } else { 160. };
                        set(1, host, c.root(axis))
                    }
                    10 => {
                        let extra = child(20, rng.expr(3), 30.);
                        let attrs = extra.attrs(axis);
                        c.kids.insert(i, extra);
                        patch(
                            1,
                            1,
                            &[
                                edges(1, host, &c.ids()),
                                node(20, 4, attrs, &[120]),
                                node(120, 6, block(&[px(axis, 30.)]), &[]),
                            ],
                        )
                    }
                    11 => {
                        c.kids.retain(|kid| kid.id != 20);
                        patch(20, 0, &[])
                    }
                    _ => {
                        let before = k.expr.clone();
                        k.expr = rng.expr(2);
                        live.patch(&set(k.id, 4, k.attrs(axis)))
                            .unwrap_or_else(|e| {
                                panic!("seed {seed} {site:?} step {step}, intermediate patch: {e}")
                            });
                        k.expr = before;
                        set(k.id, 4, k.attrs(axis))
                    }
                };
                let check = (|| {
                    if step == 0 {
                        live.upload(&wire)?
                    } else if !wire.is_empty() {
                        live.patch(&wire)?
                    };
                    c.settle(&mut live)?;
                    let actual = c.verify(site, &live)?;
                    let cold = c.cold(site)?;
                    if actual != cold {
                        return Err(format!("sparse {actual:?} != cold {cold:?}"));
                    }
                    Ok(())
                })();
                if let Err(e) = check {
                    panic!(
                        "seed {seed} {site:?} step {step}: {e}; case {c:?}; cold reducer: {:?}",
                        c.reduced_cold_failure(site)
                    );
                }
            }
        }
    }
}

#[test]
fn an_expression_can_change_its_active_fill_weight_twice() {
    // max(min(3u,30),u): 3u below 10, then 30, then u above 30.
    let mut c = specimen(pair(false, pair(true, Fill(Some(3.)), Px(30.)), Fill(None)));
    for site in [RowWidth, ColumnHeight] {
        for (room, a, b) in [(20., 15., 5.), (50., 30., 20.), (120., 60., 60.)] {
            c.host = room;
            assert_eq!(c.cold(site).unwrap(), [(0., a), (a, b)]);
        }
    }
}
