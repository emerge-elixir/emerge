//! Direct nonfloating paragraphs read earlier document siblings, not just
//! their own width. Resolve them in document order during the same Reflow
//! pass, after their glyphs, widths and local float fits are available.
use super::*;
use paragraph::{Band, FloatSize};

// A document offers ordinary blocks an unbounded height, not a Column share.
pub(super) const BLOCK_HEIGHT: Constraint = Constraint::Available(f32::MAX);

// Search with the unallotted, parent-facing demand. In particular, the
// measured height of a quarter turn comes from its local *width*. An
// Image's intrinsic width survives aspect projection in its minimum.
fn float_demand(e: &Elements, c: Key, local: Demand) -> [f32; 2] {
    let a = &e.attrs[c];
    let w = if e.slots[c].shape == Shape::Image && a.sizing.width_from_height() {
        local.min_w
    } else {
        local.w
    };
    let (w, h) = a.rotation.extent(w, local.h);
    [w, h]
}
pub(super) fn float_edge(e: &Elements, c: Key, local: Demand) -> Constraint {
    Constraint::Available(float_demand(e, c, local)[1])
}
fn float_size(e: &Elements, c: Key, local: Demand, fits: &SecondaryMap<Key, Fit>) -> FloatSize {
    let wanted = float_demand(e, c, local);
    let size =
        Fit::local_at(fits, e, c, Constraint::Available(wanted[1])).turned(e.attrs[c].rotation);
    FloatSize {
        wanted,
        outer: [size.w, size.h],
    }
}
impl Layers {
    fn lines(
        &mut self,
        e: &Elements,
        k: Key,
        document_origin: Option<[f32; 2]>,
        basis: Basis,
    ) -> f32 {
        let a = &e.attrs[k];
        let align = a
            .text_align
            .or(a.align[0])
            .unwrap_or(self.context[k].text_align);
        let inner = basis.width(self, k).inner;
        let fits = match basis {
            Basis::Independent => &self.constrained.fits,
            Basis::Offered => &self.fit,
        };
        let (height, rebuilt) = self.paragraphs.layout(
            e,
            k,
            inner,
            document_origin,
            align,
            &self.runs,
            &self.text,
            |c| float_size(e, c, self.local_measure[c], fits),
        );
        if rebuilt {
            self.enqueue(Compose, k, e);
            self.enqueue(Place, k, e);
        }
        height
    }
    fn document_paragraph(&mut self, e: &Elements, k: Key, origin: [f32; 2], basis: Basis) -> f32 {
        let fit = self.lines(e, k, Some(origin), basis);
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
            w: basis.width(self, k).outer,
            h,
            min_h: h,
        };
        basis.record(
            self,
            k,
            Fit {
                local: size,
                size: size.turned(e.attrs[k].rotation),
                row_offer: 0.,
            },
        );
        h
    }
    fn document_block(
        &mut self,
        e: &Elements,
        c: Key,
        left: f32,
        inner: f32,
        y: f32,
        basis: Basis,
    ) -> Rect {
        // Exclusions belong to the document's axes. A turned paragraph is
        // a block with its own lines, not a continuation of those float bands.
        let paragraph = e.slots[c].shape == Shape::Paragraph && !e.attrs[c].rotation.mixes_axes();
        let (w, y) = if paragraph {
            (basis.width(self, c).outer, y)
        } else {
            (
                Fit::width_at(basis.fits(self), e, c, BLOCK_HEIGHT),
                y.max(self.paragraphs.floats.bottom()),
            )
        };
        let x = left + e.alignment(c, 0).offset(inner, w);
        let h = if paragraph {
            self.document_paragraph(e, c, [x, y], basis)
        } else {
            Fit::local_at(basis.fits(self), e, c, BLOCK_HEIGHT)
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
        let h = self.layers.lines(e, k, None, self.basis);
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
        let inner = self.basis.width(l, k).inner;
        let band = Band::new([0.; 2], left, inner, self.gap[0]);
        let mut y = top;
        for (i, &c) in e.children(k).iter().enumerate() {
            if i != 0 {
                y = (y + self.gap[1]).min(f32::MAX);
            }
            let site = if let Some(side) = e.float_side(c) {
                let size = float_size(e, c, l.local_measure[c], self.basis.fits(l));
                band.place(&mut l.paragraphs.floats, side, y, size)
            } else {
                let site = l.document_block(e, c, left, inner, y, self.basis);
                y = (site.y + site.height).min(f32::MAX);
                site
            };
            l.paragraphs.set_site(c, site);
        }
        let height = y.max(l.paragraphs.floats.bottom()) - top;
        self.box_height(height).max(self.measured_height).into()
    }
}
