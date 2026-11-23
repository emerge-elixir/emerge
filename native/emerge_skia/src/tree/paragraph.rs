//! Paragraph-local fragments, in declaration order. Reflow writes relative
//! coordinates once; Place, drawing and hit testing read the same result.
use super::{
    text::{Run, TextStore},
    *,
};

mod floats;
use floats::Floats;
pub(super) use floats::{Band, FloatSize};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Fragment {
    pub source: Key,
    pub owner: Key,
    pub first: u16,
    pub end: u16,
    /// Origin of the glyph baseline; no left-overhang correction in prose.
    pub baseline: [f32; 2],
    pub width: f32,
    pub bounds: Rect,
}
#[derive(Default)]
pub(super) struct Paragraphs {
    pub hosts: usize,
    pub words: usize,
    solutions: SecondaryMap<Key, Solution>,
    data: Pool<Fragment>,
    sites: SecondaryMap<Key, Rect>,
    scratch: Vec<Fragment>,
    pub floats: Floats,
}
#[derive(Clone, Copy, Default)]
struct Solution {
    span: Span<Fragment>,
    // A clean unexcluded paragraph can move vertically without breaking lines.
    clear_height: Option<f32>,
}
impl Paragraphs {
    pub fn reserve(
        &mut self,
        keys: usize,
        owners: usize,
        words: usize,
        b: Budget,
    ) -> Result<(), String> {
        self.solutions
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        self.sites
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        if words > self.scratch.capacity() {
            pool::reserve(&mut self.scratch, b.capacity(words))?;
        }
        self.floats.reserve(owners, b)?;
        // Old fragments can belong to a detached or not-yet-reflowed paragraph.
        // Provision both initial and replacement line sets on first admission.
        let old = if self.data.capacity() == 0 {
            words
        } else {
            self.data.len()
        };
        self.data.ensure(old + words, owners, b)
    }
    pub fn evict(&mut self, k: Key) {
        if let Some(solution) = self.solutions.remove(k) {
            self.data.release(solution.span);
        }
        self.sites.remove(k);
    }
    pub fn set_site(&mut self, c: Key, site: Rect) {
        self.sites.insert(c, site);
    }
    pub fn site(&self, c: Key) -> Rect {
        self.sites.get(c).copied().unwrap_or_default()
    }
    pub fn fragments(&self, k: Key) -> &[Fragment] {
        self.solutions
            .get(k)
            .map_or(&[], |s| self.data.get(&s.span))
    }
    pub fn invalidate_lines(&mut self, k: Key) {
        if let Some(s) = self.solutions.get_mut(k) {
            s.clear_height = None;
        }
    }
    /// Returns the content height and whether line inputs were rebuilt. A local
    /// float cannot outlive this paragraph's content height. Thus when both the
    /// old and new incoming bands are clear, clean relative sites need no replay.
    #[allow(clippy::too_many_arguments)]
    pub fn layout(
        &mut self,
        e: &Elements,
        k: Key,
        inner: f32,
        document_origin: Option<[f32; 2]>,
        align: Alignment,
        runs: &SecondaryMap<Key, Run>,
        text: &TextStore,
        float_size: impl Fn(Key) -> FloatSize,
    ) -> (f32, bool) {
        let size = e.attrs[k].sizing;
        let scale = e.slots[k].admitted.scale;
        let [top, _, _, left] = size.padding.map(|v| scale::pixels(v, scale));
        let [gap, line_gap] = size.spacing.map(|v| scale::pixels(v, scale));
        let origin = document_origin.unwrap_or([0.; 2]);
        if document_origin.is_none() {
            self.floats.clear();
        }
        let clear = document_origin.is_some() && self.floats.bottom() <= origin[1] + top;
        if clear && let Some(h) = self.solutions.get(k).and_then(|s| s.clear_height) {
            return (h, false);
        }
        let band = Band::new(origin, left, inner, gap);
        let left = band.limits(&self.floats, top, 1.).0;
        self.scratch.clear();
        let mut flow = Lines {
            words: &mut self.scratch,
            floats: &mut self.floats,
            band,
            y: top,
            x: left,
            h: 0.,
            bottom: top,
            line_gap,
            align,
            start: 0,
            text_right: left,
            band_right: band.right,
        };
        for &owner in e.children(k) {
            if let Some(side) = e.float_side(owner) {
                let rect = flow.add_float(side, float_size(owner));
                self.sites.insert(owner, rect);
                continue;
            }
            self.sites.insert(owner, Rect::default());
            let source = match e.slots[owner].shape {
                Shape::Text => owner,
                Shape::El => match e.children(owner).first() {
                    Some(&c) if e.slots[c].shape == Shape::Text => c,
                    _ => continue,
                },
                _ => continue,
            };
            self.sites.insert(source, Rect::default());
            let insets = if owner == source {
                [0.; 4]
            } else {
                e.attrs[owner]
                    .sizing
                    .padding
                    .map(|v| scale::pixels(v, e.slots[owner].admitted.scale))
            };
            flow.span(e.text(source), source, owner, insets, runs[source], text);
        }
        flow.finish_line();
        let height = flow.bottom - top;
        for f in &self.scratch {
            let bounds = self.site(f.owner);
            let bounds = if bounds == Rect::default() {
                f.bounds
            } else {
                union(bounds, f.bounds)
            };
            self.sites.insert(f.owner, bounds);
            if f.source != f.owner {
                self.sites.insert(f.source, bounds);
            }
        }
        let changed = self.fragments(k) != self.scratch;
        let solution = self
            .solutions
            .entry(k)
            .expect("live paragraph")
            .or_default();
        if changed {
            self.data.release(solution.span);
            solution.span = self.data.insert(&self.scratch);
        }
        solution.clear_height = clear.then_some(height);
        (height, true)
    }
}
pub(super) fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}
const EPS: f32 = 0.001;
struct Lines<'a> {
    words: &'a mut Vec<Fragment>,
    floats: &'a mut Floats,
    band: Band,
    x: f32,
    y: f32,
    h: f32,
    bottom: f32,
    line_gap: f32,
    align: Alignment,
    start: usize,
    text_right: f32,
    band_right: f32,
}
impl Lines<'_> {
    fn band(&self, y: f32, height: f32) -> (f32, f32) {
        self.band.limits(self.floats, y, height)
    }
    fn next_bottom(&self, y: f32) -> Option<f32> {
        self.band.next_bottom(self.floats, y)
    }
    fn finish_line(&mut self) {
        let dx = self
            .align
            .offset((self.band_right - self.text_right).max(0.), 0.);
        if dx != 0. {
            for f in &mut self.words[self.start..] {
                f.baseline[0] += dx;
                f.bounds.x += dx;
            }
        }
        self.start = self.words.len();
    }
    fn newline(&mut self, y: f32) {
        self.finish_line();
        self.y = y;
        self.h = 0.;
        self.x = self.band(y, 1.).0;
    }
    fn advance_line(&mut self) {
        self.newline(self.y + self.h + self.line_gap);
    }
    fn add_float(&mut self, side: Alignment, size: FloatSize) -> Rect {
        if self.h > 0. && self.x > self.band(self.y, 1.).0 + EPS {
            self.advance_line();
        }
        let rect = self.band.place(self.floats, side, self.y, size);
        self.bottom = self.bottom.max(rect.y + rect.height);
        self.x = self.x.max(self.band(self.y, 1.).0);
        rect
    }
    fn space(&mut self, w: f32, closing: f32, carry: bool, height: f32) {
        let (l, r) = self.band(self.y, self.h.max(height));
        self.x = self.x.max(l);
        if self.x > l + EPS && self.x + w + closing > r {
            self.advance_line();
            if !carry {
                return;
            }
        }
        self.x += w;
    }
    fn span(
        &mut self,
        s: &str,
        source: Key,
        owner: Key,
        pad: [f32; 4],
        run: Run,
        text: &TextStore,
    ) {
        let [t, r, b, l] = pad;
        let mut chars = s.chars().enumerate().peekable();
        let mut words = std::iter::from_fn(|| {
            let (first, _) = chars.find(|(_, c)| !c.is_whitespace())?;
            while chars.next_if(|(_, c)| !c.is_whitespace()).is_some() {}
            Some(
                first
                    ..chars
                        .peek()
                        .map_or_else(|| text.glyphs.get(&run.span).len(), |(i, _)| *i),
            )
        })
        .peekable();
        let has_words = words.peek().is_some();
        if !has_words && source == owner {
            return;
        }
        let leading = has_words && s.starts_with(char::is_whitespace);
        let space = text.space_width(run);
        let height = if has_words { run.metrics.height } else { 0. } + t + b;
        if leading {
            self.space(space, r, true, height);
        }
        let leading_origin = (self.x - space, self.y);
        let mut first = true;
        loop {
            let range = words.next().unwrap_or(0..0);
            let w = text.word_width(run, range.clone());
            let (opening, right) = loop {
                let (left, right) = self.band(self.y, self.h.max(height));
                self.x = self.x.max(left);
                if right - left <= EPS
                    && let Some(y) = self.next_bottom(self.y)
                {
                    self.newline(y);
                    continue;
                }
                let continues = !first && self.words.len() > self.start;
                let opening = if continues { 0. } else { l };
                if self.x > left + EPS && self.x + opening + w + r > right {
                    self.advance_line();
                    continue;
                }
                break (opening, right);
            };
            let x = if first && leading && self.y == leading_origin.1 {
                leading_origin.0
            } else {
                self.x
            };
            self.x += opening;
            self.words.push(Fragment {
                owner,
                source,
                first: range.start as u16,
                end: range.end as u16,
                baseline: [self.x, self.y + t + run.metrics.ascent],
                width: w,
                bounds: Rect {
                    x,
                    y: self.y,
                    width: self.x + w + r - x,
                    height,
                },
            });
            self.x += w;
            self.text_right = self.x + r;
            self.band_right = right;
            self.h = self.h.max(height);
            self.bottom = self.bottom.max(self.y + height);
            first = false;
            if words.peek().is_none() {
                break;
            }
            self.space(space, r, false, 0.);
        }
        self.x += r;
        if has_words && s.ends_with(char::is_whitespace) {
            self.space(space, 0., true, 0.);
            let f = self.words.last_mut().unwrap();
            if owner != source && f.bounds.y == self.y {
                f.bounds.width = self.x - f.bounds.x;
                self.text_right = self.x;
            }
        }
    }
}
