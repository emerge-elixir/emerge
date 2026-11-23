//! Paragraph-local fragments, in declaration order. Reflow writes relative
//! coordinates once; Place, drawing and hit testing read the same result.
use super::{
    layers::Size,
    text::{Run, TextStore},
    *,
};

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
    pub count: usize,
    pub words: usize,
    spans: SecondaryMap<Key, Span<Fragment>>,
    data: Pool<Fragment>,
    sites: SecondaryMap<Key, Rect>,
    scratch: Vec<Fragment>,
    // Each side stacks monotonically, even when the two sides overlap.
    floats: [Vec<Rect>; 2],
}
impl Paragraphs {
    pub fn reserve(
        &mut self,
        keys: usize,
        owners: usize,
        words: usize,
        b: Budget,
    ) -> Result<(), String> {
        self.spans
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        self.sites
            .try_set_capacity(keys)
            .map_err(pool::capacity_error)?;
        if words > self.scratch.capacity() {
            pool::reserve(&mut self.scratch, b.capacity(words))?;
        }
        for side in &mut self.floats {
            if owners > side.capacity() {
                pool::reserve(side, b.capacity(owners))?;
            }
        }
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
        if let Some(span) = self.spans.remove(k) {
            self.data.release(span);
        }
        self.sites.remove(k);
    }
    pub fn site(&self, c: Key) -> Rect {
        self.sites.get(c).copied().unwrap_or_default()
    }
    pub fn fragments(&self, k: Key) -> &[Fragment] {
        self.spans.get(k).map_or(&[], |s| self.data.get(s))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn layout(
        &mut self,
        e: &Elements,
        k: Key,
        inner: f32,
        gap: [f32; 2],
        align: Alignment,
        runs: &SecondaryMap<Key, Run>,
        text: &TextStore,
        fit: impl Fn(Key) -> Size,
    ) -> f32 {
        let scale = e.slots[k].admitted.scale;
        let [top, _, _, left] = e.attrs[k].sizing.padding.map(|v| scale::pixels(v, scale));
        self.scratch.clear();
        for side in &mut self.floats {
            side.clear();
        }
        let mut flow = Lines {
            words: &mut self.scratch,
            floats: &mut self.floats,
            left,
            right: left + inner,
            y: top,
            x: left,
            h: 0.,
            bottom: top,
            gap,
            align,
            start: 0,
            text_right: left,
            band_right: left + inner,
        };
        for &owner in e.children(k) {
            if let Some(side) = e.float_side(owner) {
                let size = fit(owner);
                let rect = flow.add_float(side == Alignment::End, size);
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
        if self.fragments(k) != self.scratch {
            if let Some(old) = self.spans.remove(k) {
                self.data.release(old);
            }
            self.spans.insert(k, self.data.insert(&self.scratch));
        }
        height
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
    floats: &'a mut [Vec<Rect>; 2],
    left: f32,
    right: f32,
    x: f32,
    y: f32,
    h: f32,
    bottom: f32,
    gap: [f32; 2],
    align: Alignment,
    start: usize,
    text_right: f32,
    band_right: f32,
}
impl Lines<'_> {
    fn band(&self, y: f32, height: f32) -> (f32, f32) {
        let active = |side: usize| {
            let floats = &self.floats[side];
            let start = floats.partition_point(|r| r.y + r.height <= y);
            floats[start..]
                .iter()
                .take_while(move |r| r.y < y + height.max(1.))
        };
        let l = active(0)
            .map(|r| (r.x + r.width + self.gap[0]).min(self.right))
            .fold(self.left, f32::max);
        let r = active(1)
            .map(|r| (r.x - self.gap[0]).max(self.left))
            .fold(self.right, f32::min);
        (l, r.max(l))
    }
    fn next_bottom(&self, y: f32) -> Option<f32> {
        self.floats
            .iter()
            .filter_map(|s| s.get(s.partition_point(|r| r.y + r.height <= y + EPS)))
            .map(|r| r.y + r.height)
            .min_by(f32::total_cmp)
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
        self.newline(self.y + self.h + self.gap[1]);
    }
    fn add_float(&mut self, right: bool, size: Size) -> Rect {
        if self.h > 0. && self.x > self.band(self.y, 1.).0 + EPS {
            self.advance_line();
        }
        let mut y = self.floats[usize::from(right)]
            .last()
            .map_or(self.y, |r| self.y.max(r.y + r.height));
        let (l, r) = loop {
            let (l, r) = self.band(y, size.h);
            if size.w.min(self.right - self.left) <= r - l + EPS {
                break (l, r);
            }
            if let Some(b) = self.next_bottom(y) {
                y = b;
            } else {
                break (l, r);
            }
        };
        let rect = Rect {
            x: if right { (r - size.w).max(l) } else { l },
            y,
            width: size.w,
            height: size.h,
        };
        self.bottom = self.bottom.max(y + size.h);
        self.floats[usize::from(right)].push(rect);
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
