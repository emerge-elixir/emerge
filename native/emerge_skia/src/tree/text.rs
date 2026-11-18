//! Glyph and position calculation reads immutable Skia strike metrics. Preparing
//! a strike provisions font inputs, not the width, bounds or layout of any string.
use super::{
    Family::*,
    Rect,
    pool::{Budget, Pool, Span},
};
use skia_safe::{Font, FontHinting, FontMgr, Point, Typeface, font::Edging};
use skrifa::{FontRef, MetadataProvider, charmap::Charmap};
use slotmap::{SlotMap, new_key_type};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::OnceLock,
};
new_key_type! { struct SharedKey; }
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Family {
    #[default]
    Sans,
    Mono,
}
impl Family {
    fn bytes(self) -> &'static [u8] {
        match self {
            Sans => include_bytes!("../fonts/inter/Inter-Regular.ttf"),
            Mono => include_bytes!("../fonts/jetbrains-mono/JetBrainsMonoNL-Regular.ttf"),
        }
    }
    fn cmap(self) -> &'static Charmap<'static> {
        static MAPS: [OnceLock<Charmap>; 2] = [OnceLock::new(), OnceLock::new()];
        MAPS[self as usize]
            .get_or_init(|| FontRef::new(self.bytes()).expect("bundled font").charmap())
    }
    pub(super) fn font(self, size: f32) -> Font {
        static FACES: [OnceLock<Typeface>; 2] = [OnceLock::new(), OnceLock::new()];
        let face = FACES[self as usize].get_or_init(|| {
            FontMgr::new()
                .new_from_data(self.bytes(), 0)
                .expect("bundled typeface")
        });
        let mut f = Font::from_typeface(face.clone(), size);
        f.set_subpixel(true);
        f.set_linear_metrics(true);
        f.set_baseline_snap(true);
        f.set_edging(Edging::SubpixelAntiAlias);
        f.set_hinting(FontHinting::Normal);
        f
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Glyph {
    pub id: u16,
    pub pos: Point,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Metrics {
    pub family: Family,
    pub size: f32,
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub left: f32,
    pub ink: Rect,
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Run {
    pub metrics: Metrics,
    pub span: Span<Glyph>,
    pub revision: u64,
    pub invalid: bool,
    shared: Option<SharedKey>,
}
struct SharedRun {
    text: Span<u8>,
    glyphs: Span<Glyph>,
    metrics: Metrics,
    refs: usize,
}
struct Strike {
    family: Family,
    size: f32,
    scale: f32,
    widths: Vec<f32>,
    bounds: Vec<skia_safe::Rect>,
    ascent: f32,
    descent: f32,
}
impl Strike {
    fn new(family: Family, size: f32) -> Result<Self, String> {
        // Same path canonicalization as SkStrikeSpec::MakeCanonicalized. Sum raw
        // advances before scaling bounds/width; positions scale each advance first.
        let path = size > 256.;
        let mut font = family.font(if path { 64. } else { size });
        let scale = if path { size / 64. } else { 1. };
        if path {
            font.set_embedded_bitmaps(false);
            font.set_force_auto_hinting(false);
            font.set_hinting(FontHinting::None);
            font.set_edging(Edging::AntiAlias);
        }
        let count = font.typeface().count_glyphs();
        let (mut ids, mut widths, mut bounds) = (Vec::new(), Vec::new(), Vec::new());
        super::pool::reserve(&mut ids, count)?;
        super::pool::reserve(&mut widths, count)?;
        super::pool::reserve(&mut bounds, count)?;
        ids.extend((0..count).map(|i| i as u16));
        widths.resize(count, 0.);
        bounds.resize(count, skia_safe::Rect::default());
        font.get_widths_bounds(&ids, Some(&mut widths), Some(&mut bounds), None);
        let (_, m) = font.metrics();
        family.cmap();
        Ok(Self {
            family,
            size,
            scale,
            widths,
            bounds,
            ascent: m.ascent * scale,
            descent: m.descent * scale,
        })
    }
    fn shape(&self, text: &str, data: &mut [Glyph]) -> Metrics {
        let mut ink = skia_safe::Rect::default();
        let (mut width, mut pos) = (0., 0.);
        let cmap = self.family.cmap();
        for (i, (ch, g)) in text.chars().zip(data).enumerate() {
            let id = cmap.map(ch).map_or(0, |v| v.to_u32() as u16);
            *g = Glyph {
                id,
                pos: Point::new(pos, 0.),
            };
            let mut rect = self.bounds[id as usize];
            rect.offset((width, 0.));
            if i == 0 {
                ink = rect
            } else {
                ink.join(rect)
            }
            let w = self.widths[id as usize];
            width += w;
            pos += w * self.scale;
        }
        width *= self.scale;
        let left = (-ink.left * self.scale).max(0.);
        let right = ink.right * self.scale;
        let x = ink.left * self.scale;
        let y = ink.top * self.scale;
        Metrics {
            family: self.family,
            size: self.size,
            width: width + left + (right - width).max(0.),
            height: self.descent - self.ascent,
            ascent: -self.ascent,
            left,
            ink: Rect {
                x,
                y,
                width: right - x,
                height: ink.bottom * self.scale - y,
            },
        }
    }
}
// Strike resources are admitted before mutation. Count distinct declarations,
// not mounts; changing a size over time must not retain unbounded font tables.
const SIZE_LIMIT: usize = 64;
#[derive(Clone, Copy)]
pub(super) struct Sizes {
    entries: [(f32, usize); SIZE_LIMIT],
}
impl Default for Sizes {
    fn default() -> Self {
        Self {
            entries: [(0., 0); SIZE_LIMIT],
        }
    }
}
impl Sizes {
    pub fn add(&mut self, size: f32) -> Result<(), String> {
        let i = self
            .entries
            .iter()
            .position(|&(s, n)| n != 0 && s == size)
            .or_else(|| self.entries.iter().position(|&(_, n)| n == 0))
            .ok_or("too many distinct declared font sizes")?;
        self.entries[i] = (size, self.entries[i].1 + 1);
        Ok(())
    }
    pub fn remove(&mut self, size: f32) {
        let entry = self
            .entries
            .iter_mut()
            .find(|(s, n)| *n != 0 && *s == size)
            .expect("declared font size");
        entry.1 -= 1;
    }
    fn devices(&self, scale: f32) -> impl Iterator<Item = f32> + '_ {
        std::iter::once(16.).chain(
            self.entries
                .iter()
                .filter(|(_, n)| *n != 0)
                .map(move |(s, _)| s * scale),
        )
    }
}
#[derive(Default)]
pub(super) struct TextStore {
    pub sizes: Sizes,
    strikes: Vec<Strike>,
    pub glyphs: Pool<Glyph>,
    strings: Pool<u8>,
    values: SlotMap<SharedKey, SharedRun>,
    lookup: Vec<Option<SharedKey>>,
    #[cfg(test)]
    pub shapes: usize,
}
impl TextStore {
    pub fn provision(&mut self, sizes: Sizes, scale: f32, has_text: bool) -> Result<(), String> {
        if has_text {
            if !sizes.devices(scale).all(f32::is_finite) {
                return Err("font size exceeds device range".into());
            }
            let initial = self.strikes.len();
            for size in sizes.devices(scale) {
                for family in [Sans, Mono] {
                    if self
                        .strikes
                        .iter()
                        .any(|s| s.family == family && s.size == size)
                    {
                        continue;
                    }
                    let next = self
                        .strikes
                        .try_reserve(1)
                        .map_err(super::pool::capacity_error)
                        .and_then(|_| Strike::new(family, size));
                    match next {
                        Ok(s) => self.strikes.push(s),
                        Err(err) => {
                            self.strikes.truncate(initial);
                            return Err(err);
                        }
                    }
                }
            }
        }
        if self.strikes.len() > 2 * (SIZE_LIMIT + 1) {
            self.strikes
                .retain(|s| sizes.devices(scale).any(|v| v == s.size));
        }
        self.sizes = sizes;
        Ok(())
    }
    pub fn ensure(&mut self, peak: usize, blocks: usize, b: Budget) -> Result<(), String> {
        let n = blocks + 1;
        if self.values.capacity() < n {
            self.values
                .try_reserve(b.capacity(n) - self.values.len())
                .map_err(super::pool::capacity_error)?;
        }
        if self.lookup.len() < 2 * n {
            let cap = b.capacity(2 * n);
            super::pool::reserve(&mut self.lookup, cap)?;
            self.lookup.resize(cap, None);
        }
        self.strings.ensure(4 * peak, n, b)?;
        self.glyphs.ensure(peak, n, b)
    }
    pub fn measure(
        &mut self,
        text: &str,
        n: usize,
        family: Family,
        size: f32,
        run: &mut Run,
    ) -> bool {
        let mut h = DefaultHasher::new();
        (text, family as u8, size.to_bits()).hash(&mut h);
        let slot = h.finish() as usize % self.lookup.len();
        let key = if let Some(k) = self.lookup[slot].filter(|&k| {
            self.values.get(k).is_some_and(|s| {
                s.metrics.family == family
                    && s.metrics.size == size
                    && self.strings.get(&s.text) == text.as_bytes()
            })
        }) {
            self.values[k].refs += 1;
            k
        } else {
            let strike = self
                .strikes
                .iter()
                .find(|s| s.family == family && s.size == size)
                .expect("provisioned strike");
            let glyphs = self.glyphs.alloc(n);
            let metrics = strike.shape(text, self.glyphs.write(glyphs));
            #[cfg(test)]
            {
                self.shapes += 1;
            }
            let k = self.values.insert(SharedRun {
                text: self.strings.insert(text.as_bytes()),
                glyphs,
                metrics,
                refs: 1,
            });
            self.lookup[slot] = Some(k);
            k
        };
        let s = &self.values[key];
        let changed = run.revision == 0
            || (run.shared != Some(key)
                && (s.metrics != run.metrics
                    || self.glyphs.get(&s.glyphs) != self.glyphs.get(&run.span)));
        let old = *run;
        *run = Run {
            shared: Some(key),
            metrics: s.metrics,
            span: s.glyphs,
            revision: old.revision + u64::from(changed),
            invalid: false,
        };
        self.release(old);
        changed
    }
    pub fn release(&mut self, run: Run) {
        if let Some(k) = run.shared {
            self.values[k].refs -= 1;
            if self.values[k].refs == 0 {
                let s = self.values.remove(k).unwrap();
                self.glyphs.release(s.glyphs);
                self.strings.release(s.text);
            }
        }
    }
}
pub struct TextMeasurer;
impl TextMeasurer {
    pub fn dimensions(text: &str, size: f32) -> (f32, f32, f32, f32) {
        let f = Sans.font(size);
        let (advance, _) = f.measure_str(text, None);
        let (_, m) = f.metrics();
        (advance, m.descent - m.ascent, -m.ascent, m.descent)
    }
}
