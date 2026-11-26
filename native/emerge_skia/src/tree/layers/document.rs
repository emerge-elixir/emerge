//! Direct nonfloating paragraphs read earlier document siblings, not just
//! their own width. Resolve them in document order during the same Reflow
//! pass, after their glyphs, widths and local float fits are available.
use super::*;
use paragraph::{Band, FloatSize};

// A document offers ordinary blocks an unbounded height, not a Column share.
pub(super) const BLOCK_HEIGHT: Constraint = Constraint::Available(f32::MAX);

fn float_size(
    e: &Elements,
    c: Key,
    m: Demand,
    local_h: f32,
    fits: &SecondaryMap<Key, Fit>,
) -> FloatSize {
    let edge = Constraint::Available(local_h);
    let size = Fit::local_at(fits, e, c, edge).turned(e.attrs[c].rotation);
    FloatSize {
        wanted: [m.w, m.h],
        outer: [size.w, size.h],
    }
}
impl Layers {
    fn lines(&mut self, e: &Elements, k: Key, document_origin: Option<[f32; 2]>) -> f32 {
        let a = &e.attrs[k];
        let align = a
            .text_align
            .or(a.align[0])
            .unwrap_or(self.context[k].text_align);
        let (height, rebuilt) = self.paragraphs.layout(
            e,
            k,
            self.width[k].inner,
            document_origin,
            align,
            &self.runs,
            &self.text,
            |c| float_size(e, c, self.measure[c], self.local_measure[c].h, &self.fit),
        );
        if rebuilt {
            self.enqueue(Compose, k, e);
            self.enqueue(Place, k, e);
        }
        height
    }
    fn document_paragraph(&mut self, e: &Elements, k: Key, origin: [f32; 2]) -> f32 {
        let fit = self.lines(e, k, Some(origin));
        let b = e.attrs[k].sizing;
        let scale = e.slots[k].admitted.scale;
        let content = fit + pixels(b.padding[0], scale) + pixels(b.padding[2], scale);
        // Shared flow grows even a shorter declared paragraph to contain its
        // text and local floats. Incoming floats aren't charged to its height.
        let natural = b
            .height
            .resolve(content, || scale)
            .max(self.local_measure[k].h);
        let initial = BLOCK_HEIGHT.resolve_height(e, k, natural);
        // Insets alone must not expand a zero-height, empty paragraph.
        let h = if fit > 0. {
            initial.max(content)
        } else {
            initial
        };
        let size = Size {
            w: self.width[k].outer,
            h,
            min_h: h,
        };
        self.fit.insert(
            k,
            Fit {
                local: size,
                size: size.turned(e.attrs[k].rotation),
                row_offer: 0.,
            },
        );
        h
    }
    fn document_block(&mut self, e: &Elements, c: Key, left: f32, inner: f32, y: f32) -> Rect {
        // Exclusions belong to the document's axes. A turned paragraph is
        // a block with its own lines, not a continuation of those float bands.
        let paragraph = e.slots[c].shape == Shape::Paragraph && !e.attrs[c].rotation.mixes_axes();
        let (w, y) = if paragraph {
            (self.width[c].outer, y)
        } else {
            (
                Fit::width_at(&self.fit, e, c, BLOCK_HEIGHT),
                y.max(self.paragraphs.floats.bottom()),
            )
        };
        let x = left + e.alignment(c, 0).offset(inner, w);
        let h = if paragraph {
            self.document_paragraph(e, c, [x, y])
        } else {
            Fit::local_at(&self.fit, e, c, BLOCK_HEIGHT)
                .turned(e.attrs[c].rotation)
                .h
        };
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }
}
impl ReflowCx<'_> {
    pub fn paragraph(&mut self, e: &Elements, k: Key) -> Heights {
        let h = self.layers.lines(e, k, None);
        // A capped or fill-height float keeps its intrinsic outer box even
        // when its lines wrap. Its descendants still use the resolved lines.
        if e.is_prose_float(k) && !self.height.grows_with_content(e.raw(k)) {
            self.measured_height.into()
        } else {
            self.box_height(h).max(self.measured_height).into()
        }
    }
    pub fn document(&mut self, e: &Elements, k: Key) -> Heights {
        let l = &mut self.layers;
        l.paragraphs.floats.clear();
        let [top, _, _, left] = self.padding;
        let inner = l.width[k].inner;
        let band = Band::new([0.; 2], left, inner, self.gap[0]);
        let mut y = top;
        for (i, &c) in e.children(k).iter().enumerate() {
            if i != 0 {
                y = (y + self.gap[1]).min(f32::MAX);
            }
            let site = if let Some(side) = e.float_side(c) {
                let size = float_size(e, c, l.measure[c], l.local_measure[c].h, &l.fit);
                band.place(&mut l.paragraphs.floats, side, y, size)
            } else {
                let site = l.document_block(e, c, left, inner, y);
                y = (site.y + site.height).min(f32::MAX);
                site
            };
            l.paragraphs.set_site(c, site);
        }
        let height = y.max(l.paragraphs.floats.bottom()) - top;
        self.box_height(height).max(self.measured_height).into()
    }
}
