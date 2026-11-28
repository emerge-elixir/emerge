//! Paragraph-local fragments, in declaration order. Reflow writes relative
//! coordinates once; Place, drawing and hit testing read the same result.
use super::{
    text::{Run, TextStore},
    *,
};

mod floats;
mod turn;
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
impl Fragment {
    // A wrapped continuation uses only its own height on this line. For a
    // layout turn, align the occupied footprint rather than the unturned ink.
    fn offset_y(item: &[Self], elements: &Elements, line_height: f32) -> f32 {
        let first = &item[0];
        let align = elements.attrs[first.owner].align[1].unwrap_or_default();
        if align == Alignment::Start {
            return 0.;
        }
        let height = if turn::mixes_axes(elements, first.owner, first.source) {
            let ink = union(first.bounds, item.last().unwrap().bounds);
            turn::slot(elements, first.owner, first.source, ink).height
        } else {
            first.bounds.height
        };
        align.offset(line_height, height).max(0.)
    }
    fn shift(&mut self, dx: f32, dy: f32) {
        self.baseline[0] += dx;
        self.baseline[1] += dy;
        self.bounds.x += dx;
        self.bounds.y += dy;
    }
}
#[derive(Default)]
pub(super) struct Paragraphs {
    pub hosts: usize,
    pub words: usize,
    solutions: SecondaryMap<Key, Solution>,
    data: Pool<Fragment>,
    sites: SecondaryMap<Key, Site>,
    scratch: Vec<Fragment>,
    pub floats: Floats,
    #[cfg(test)]
    pub solves: usize,
    #[cfg(test)]
    pub frame: u64,
}
// An inline declaration owns a consecutive slice of the paragraph's words.
// Keep the slice with its settled site; selecting late spans must not walk
// earlier declarations or their words. Floats and empty owners have no words.
#[derive(Default)]
struct Site {
    frame: Rect,
    words: std::ops::Range<u32>,
}
#[derive(Clone, Copy, Default)]
struct Solution {
    revision: u64,
    has_floats: bool,
    span: Span<Fragment>,
    // A clean unexcluded paragraph can move vertically without breaking lines.
    clear_height: Option<f32>,
    #[cfg(test)]
    solved_frame: Option<(u64, f32)>,
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
        self.sites.insert(
            c,
            Site {
                frame: site,
                ..Site::default()
            },
        );
    }
    pub fn site(&self, c: Key) -> Rect {
        self.sites.get(c).map_or(Rect::default(), |s| s.frame)
    }
    pub fn revision(&self, k: Key) -> u64 {
        self.solutions.get(k).map_or(0, |s| s.revision)
    }
    pub fn has_floats(&self, k: Key) -> bool {
        self.solutions.get(k).is_some_and(|s| s.has_floats)
    }
    pub fn words_at(&self, owner: Key) -> std::ops::Range<u32> {
        self.sites[owner].words.clone()
    }
    pub fn fragments(&self, k: Key) -> &[Fragment] {
        self.solutions
            .get(k)
            .map_or(&[], |s| self.data.get(&s.span))
    }
    #[cfg(test)]
    pub fn assert_final_width(&self, k: Key, inner: f32) {
        if let Some((frame, w)) = self.solutions.get(k).and_then(|s| s.solved_frame)
            && frame == self.frame
        {
            assert_eq!(w, inner, "paragraph was not solved at its final width");
        }
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
        #[cfg(test)]
        {
            assert_ne!(
                self.solutions
                    .get(k)
                    .and_then(|s| s.solved_frame.map(|(n, _)| n)),
                Some(self.frame),
                "paragraph {} solved twice in one layout",
                e.slots[k].id
            );
            self.solves += 1;
        }
        let band = Band::new(origin, left, inner, gap);
        self.scratch.clear();
        let mut flow = Lines::new(
            e,
            &mut self.scratch,
            &mut self.floats,
            band,
            top,
            line_gap,
            align,
        );
        let mut has_floats = false;
        for &owner in e.children(k) {
            if let Some(side) = e.float_side(owner) {
                has_floats = true;
                let rect = flow.add_float(side, float_size(owner));
                self.sites.insert(
                    owner,
                    Site {
                        frame: rect,
                        ..Site::default()
                    },
                );
                continue;
            }
            self.sites.insert(owner, Site::default());
            let Some(source) = e.span_source(owner) else {
                continue;
            };
            self.sites.insert(source, Site::default());
            let phrase = Phrase {
                s: e.text(source),
                source,
                owner,
                run: runs[source],
                pad: insets(e, owner, source),
            };
            if turn::mixes_axes(e, owner, source) {
                flow.turned_span(phrase, text);
            } else {
                flow.span(phrase, text);
            }
        }
        flow.finish_line(flow.words.len());
        let height = flow.bottom - top;
        // Store unturned frames for the same pivots used in drawing and hits.
        // Alignment has already shifted each fragment into its final line.
        let mut first = 0;
        for span in self.scratch.chunk_by(|a, b| a.owner == b.owner) {
            let (owner, source) = (span[0].owner, span[0].source);
            let ink = span.iter().map(|f| f.bounds).reduce(union).unwrap();
            self.sites[source].frame = ink;
            if source != owner {
                self.sites[owner].frame = turn::owner_box(e, owner, source, ink);
            }
            let end = first + span.len() as u32;
            self.sites[owner].words = first..end;
            first = end;
        }
        let changed = self.fragments(k) != self.scratch;
        let solution = self
            .solutions
            .entry(k)
            .expect("live paragraph")
            .or_default();
        solution.has_floats = has_floats;
        if changed {
            solution.revision += 1;
            self.data.release(solution.span);
            solution.span = self.data.insert(&self.scratch);
        }
        #[cfg(test)]
        {
            solution.solved_frame = Some((self.frame, inner));
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
fn insets(e: &Elements, owner: Key, source: Key) -> [f32; 4] {
    if owner == source {
        [0.; 4]
    } else {
        e.attrs[owner]
            .sizing
            .padding
            .map(|v| scale::pixels(v, e.slots[owner].admitted.scale))
    }
}
#[derive(Clone, Copy)]
struct Phrase<'a> {
    s: &'a str,
    source: Key,
    owner: Key,
    run: Run,
    pad: [f32; 4],
}
const EPS: f32 = 0.001;
struct Lines<'a> {
    elements: &'a Elements,
    words: &'a mut Vec<Fragment>,
    floats: &'a mut Floats,
    band: Band,
    x: f32,
    y: f32,
    h: f32,
    bottom: f32,
    line_gap: f32,
    align: Alignment,
    align_items_y: bool,
    start: usize,
    text_right: f32,
    band_right: f32,
}
impl<'a> Lines<'a> {
    fn new(
        elements: &'a Elements,
        words: &'a mut Vec<Fragment>,
        floats: &'a mut Floats,
        band: Band,
        y: f32,
        line_gap: f32,
        align: Alignment,
    ) -> Self {
        let x = band.limits(floats, y, 1.).0;
        Self {
            start: words.len(),
            elements,
            words,
            floats,
            band,
            y,
            x,
            h: 0.,
            bottom: y,
            line_gap,
            align,
            align_items_y: false,
            text_right: x,
            band_right: band.right,
        }
    }
    fn band(&self, y: f32, height: f32) -> (f32, f32) {
        self.band.limits(self.floats, y, height)
    }
    fn next_bottom(&self, y: f32) -> Option<f32> {
        self.band.next_bottom(self.floats, y)
    }
    fn finish_line(&mut self, end: usize) {
        let dx = self
            .align
            .offset((self.band_right - self.text_right).max(0.), 0.);
        let line = &mut self.words[self.start..end];
        if self.align_items_y {
            for item in line.chunk_by_mut(|a, b| a.owner == b.owner) {
                let dy = Fragment::offset_y(item, self.elements, self.h);
                for word in item {
                    word.shift(dx, dy);
                }
            }
        } else if dx != 0. {
            for word in line {
                word.shift(dx, 0.);
            }
        }
        self.start = end;
        self.align_items_y = false;
    }
    fn occupy(&mut self, align_y: Alignment, height: f32, text_right: f32, band_right: f32) {
        self.align_items_y |= align_y != Alignment::Start;
        self.h = self.h.max(height);
        self.bottom = self.bottom.max(coordinate(self.y + height));
        self.text_right = text_right;
        self.band_right = band_right;
    }
    fn newline(&mut self, y: f32, end: usize) {
        self.finish_line(end);
        self.y = y;
        self.h = 0.;
        self.x = self.band(y, 1.).0;
    }
    fn advance_line(&mut self) {
        self.newline(self.y + self.h + self.line_gap, self.words.len());
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
    // `end` excludes the pending item, whose words may already be shaped in
    // the reserved scratch. Only committed words participate in line alignment.
    fn room(&mut self, end: usize, height: f32, atomic: bool, need: impl Fn(bool) -> f32) -> f32 {
        loop {
            let (left, right) = self.band(self.y, self.h.max(height));
            self.x = self.x.max(left);
            let width = need(end == self.start);
            let empty_band = right - left <= EPS;
            if (empty_band || (atomic && end == self.start && !self.band.fits(width, left, right)))
                && let Some(y) = self.next_bottom(self.y)
            {
                self.newline(y, end);
            } else if (self.x > left + EPS || (atomic && end != self.start))
                && self.x + width > right
            {
                self.newline(self.y + self.h + self.line_gap, end);
            } else {
                return right;
            }
        }
    }
    fn span(
        &mut self,
        Phrase {
            s,
            source,
            owner,
            pad: [t, r, b, l],
            run,
        }: Phrase<'_>,
        text: &TextStore,
    ) {
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
        let align_y = self.elements.attrs[owner].align[1].unwrap_or_default();
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
            let opening = |empty| if first || empty { l } else { 0. };
            let right = self.room(self.words.len(), height, false, |empty| {
                opening(empty) + w + r
            });
            let opening = opening(self.words.len() == self.start);
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
            self.occupy(align_y, height, self.x + r, right);
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
