//! Cost the normalized commands that the painter will actually see (including
//! outlined glyphs and inherited paints). Resource roots have their own local
//! coordinates; visit them at each use, not once at identity in the definitions.
use super::{MAX_EFFECT_PIXELS, policy};
use usvg::{Group, Node, NonZeroRect, Paint, Transform};

#[derive(Default)]
pub(super) struct Estimate {
    pub work: usize,
    pub area: f64,
}
impl Estimate {
    pub fn new(tree: &usvg::Tree) -> Option<Self> {
        let mut result = Self::default();
        result.group(tree.root(), Transform::identity(), 0)?;
        Some(result)
    }
    fn add(&mut self, n: usize) -> Option<()> {
        self.work = self.work.checked_add(n)?;
        (self.work <= policy::WORK).then_some(())
    }
    fn layer(&mut self, rect: NonZeroRect, t: Transform) -> Option<()> {
        let r = rect.transform(t)?;
        // The frontend outsets layer bounds for antialias coverage. Count the
        // full unclipped extent, including a generous round-out on both axes.
        self.area += (f64::from(r.width()).ceil() + 6.) * (f64::from(r.height()).ceil() + 6.);
        (self.area <= MAX_EFFECT_PIXELS).then_some(())
    }
    fn group(&mut self, g: &Group, parent: Transform, depth: u32) -> Option<()> {
        if depth > 2 * policy::DEPTH {
            return None;
        }
        self.add(1)?;
        let t = parent.pre_concat(g.transform());
        if g.should_isolate()
            && let Some(r) = g.layer_bounding_box()
        {
            self.layer(r, t)?;
        }
        for f in g.filters() {
            self.layer(f.rect(), t)?;
            for p in f.primitives() {
                self.layer(p.rect(), t)?;
            }
        }
        for m in std::iter::successors(g.mask(), |m| m.mask()) {
            self.layer(m.rect(), t)?;
            self.group(m.root(), t, depth + 1)?;
        }
        for c in std::iter::successors(g.clip_path(), |c| c.clip_path()) {
            if let Some(r) = g.layer_bounding_box() {
                self.layer(r, t)?;
            }
            self.group(c.root(), t.pre_concat(c.transform()), depth + 1)?;
        }
        for n in g.children() {
            match n {
                Node::Group(child) => self.group(child, t, depth + 1)?,
                Node::Text(text) => self.group(text.flattened(), t, depth + 1)?,
                Node::Image(_) => return None,
                Node::Path(p) => {
                    self.add(1 + p.data().len())?;
                    if let Some(d) = p.stroke().and_then(|s| s.dasharray()) {
                        self.add(d.len())?;
                    }
                    for paint in [p.fill().map(|f| f.paint()), p.stroke().map(|s| s.paint())]
                        .into_iter()
                        .flatten()
                    {
                        match paint {
                            Paint::Color(_) => (),
                            Paint::LinearGradient(g) => self.add(g.stops().len())?,
                            Paint::RadialGradient(g) => self.add(g.stops().len())?,
                            Paint::Pattern(p) => {
                                let pt = t.pre_concat(p.transform());
                                self.layer(p.rect(), pt)?;
                                self.group(p.root(), pt, depth + 1)?;
                            }
                        }
                    }
                }
            }
        }
        Some(())
    }
}
