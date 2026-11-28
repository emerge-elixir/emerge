//! Layout reserves the AABB of the unrotated border box. The coefficients
//! apply to lengths only; paint uses the signed angle about the same center.
use super::*;

/// Declaration-only facts on the edge. A natural paragraph height needs its
/// local width first; that width need not depend on its parent's split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Dependencies {
    pub height_reader: bool,
    pub reflows_height: bool,
    // These depend on this node's *incoming* height. A fixed-height wrapper
    // cuts the path, unlike `height_reader` which also marks interior work.
    projects_offer: bool,
    pub fit_after_share: bool,
    // A pack which will read the Column share while deriving content width.
    packed_share: bool,
    pub width_sensitive_slot: bool,
    pub width_sensitive_fit: bool,
    pub late_nonfill: bool,
    pub fills_width: bool,
    pub constrained_measure: bool,
}
impl Dependencies {
    pub fn union(self, child: Self) -> Self {
        Self {
            height_reader: self.height_reader || child.height_reader,
            reflows_height: self.reflows_height || child.reflows_height,
            projects_offer: self.projects_offer || child.projects_offer,
            fit_after_share: self.fit_after_share || child.fit_after_share,
            packed_share: self.packed_share || child.packed_share,
            width_sensitive_slot: self.width_sensitive_slot || child.width_sensitive_slot,
            width_sensitive_fit: self.width_sensitive_fit || child.width_sensitive_fit,
            late_nonfill: self.late_nonfill || child.late_nonfill,
            fills_width: self.fills_width || child.fills_width,
            constrained_measure: self.constrained_measure || child.constrained_measure,
        }
    }
}
impl Subtree {
    pub fn with_layout(mut self, shape: Shape, a: &Attrs) -> Result<Self, String> {
        self.group_runs =
            self.child_group_runs + if a.opacity.is_some() { self.escapes } else { 0 };
        let kids = self.children;
        let b = a.sizing;
        let local_reflows = !matches!(b.height, Length::Px(_))
            && match shape {
                Shape::WrappedRow | Shape::WrappedColumn | Shape::Paragraph | Shape::TextColumn => {
                    true
                }
                Shape::El | Shape::Row | Shape::Column => kids.reflows_height,
                Shape::Image => b.height_from_width(),
                Shape::Text | Shape::Empty => false,
            };
        let mixes = a.rotation.mixes_axes();
        let late_turn = mixes && local_reflows && !b.height.independent_fill();
        let pack = shape.measures_packed_width(b.width) && b.height.limits_column_height();
        // Multiple cards can change columns. One card can change packed width
        // only if it projects its incoming share, not merely because something
        // inside it reads a height (e.g. an inline turn inside a paragraph).
        let variable = |l: Length| l.natural_bounds().floor != l.natural_bounds().cap;
        let packed_share = b.height.weight().is_some()
            && variable(b.height)
            && (!b.width.definite() || b.width.reads_fill_content())
            && ((pack && (self.direct > 1 || kids.projects_offer))
                || ((shape.passes_cross_height() || shape == Shape::Column) && kids.packed_share));
        let reads_width = b.width.weight().is_some()
            || (shape == Shape::Paragraph && !b.width.definite())
            || (matches!(shape, Shape::Column | Shape::TextColumn)
                && b.width == Length::Auto
                && kids.fills_width);
        if shape == Shape::Column
            && (!b.width.definite() || b.width.reads_fill_content())
            && b.height.definite()
            && kids.packed_share
            && kids.width_sensitive_slot
        {
            return Err("WrappedColumn packed width is needed before its height is assigned; a Column share depends on that same width".into());
        }
        let constrained_measure = kids.constrained_measure
            || (shape.allots_width() && kids.late_nonfill)
            || pack
            || (kids.height_reader && (shape == Shape::Column || shape.passes_cross_height()));
        // Needing a width to measure a height is not the same as needing
        // *this offer*. A filling El over a fixed-width paragraph is an
        // independent height slot. A WrappedRow can also read width through
        // membership, without a single width-sensitive child.
        let slot_reads_width = match shape {
            Shape::Paragraph | Shape::TextColumn => self.direct != 0,
            Shape::WrappedRow => self.direct > 1 || kids.width_sensitive_fit,
            Shape::El | Shape::Row | Shape::WrappedColumn => kids.width_sensitive_fit,
            Shape::Column => kids.width_sensitive_slot,
            Shape::Image => b.height_from_width(),
            Shape::Empty | Shape::Text => false,
        };
        let width_sensitive_fit = reads_width
            && variable(b.width)
            && ((!matches!(b.height, Length::Px(_)) && slot_reads_width) || mixes);
        let share = b.height.weight().is_some() && variable(b.height);
        let passes_height = shape.passes_cross_height() || shape.allots_height();
        let projects_offer = share
            && (shape.projects_height(a)
                || packed_share
                || (passes_height && !matches!(b.width, Length::Px(_)) && kids.projects_offer));
        // E.g. the Image consumes some of a Row's width; its filling prose
        // peer cannot be measured before the height that sizes that Image.
        let fit_after_share = share
            && passes_height
            && (kids.fit_after_share
                || (kids.projects_offer
                    && slot_reads_width
                    && (shape.allots_width() || !matches!(b.width, Length::Px(_)))));
        self.own = Dependencies {
            projects_offer,
            fit_after_share,
            reflows_height: local_reflows
                || (mixes && (!matches!(b.width, Length::Px(_)) || b.height.weight().is_some())),
            packed_share,
            width_sensitive_fit,
            width_sensitive_slot: !b.height.independent_fill() && width_sensitive_fit,
            late_nonfill: late_turn && b.width.weight().is_none() && !fit_after_share,
            constrained_measure,
            fills_width: b.width.weight().is_some(),
            height_reader: kids.height_reader || shape.projects_height(a) || packed_share,
        };
        self.reads_anchor = [
            reads_width || (shape == Shape::Text && b.width == Length::Auto),
            self.own.height_reader && share,
        ];
        if self.mount_needs[0]
            && (shape.packs_width(b.width) || (shape == Shape::Image && b.width_from_height()))
        {
            return Err("Nearby width needs an anchor known before Place".into());
        }
        if self.mount_needs[1] && !matches!(b.height, Length::Px(_)) {
            return Err("Nearby height-dependent width needs a definite anchor height".into());
        }
        Ok(self)
    }
}

impl Elements {
    pub(super) fn reads_column_share(&self, k: Key, column: BoxStyle) -> bool {
        let m = &self.slots[k];
        m.shape.reads_group_height(&self.attrs[k], column)
            || (m.stats.children.height_reader && self.attrs[k].sizing.height.weight().is_some())
    }
    // A fixed outer width alone doesn't prove that a paragraph *inside* the
    // card has its final width. A filling wrapper may pass its pending share
    // to an Image that consumes width beside that paragraph.
    pub(super) fn fit_reads_height_offer(&self, k: Key) -> bool {
        let m = &self.slots[k];
        let b = self.attrs[k].sizing;
        m.stats.own.fit_after_share
            || (m.stats.own.projects_offer && !matches!(b.width, Length::Px(_)))
    }
    pub(super) fn measures_at_width(&self, p: Key, k: Key) -> bool {
        let m = &self.slots[p];
        (m.shape.allots_width() && self.slots[k].stats.own.late_nonfill)
            || (m.shape == Shape::WrappedColumn
                && (m.shape.measures_packed_width(self.attrs[p].sizing.width)
                    || m.stats.children.height_reader)
                && self.slots[k].stats.own.reflows_height)
            || (m.shape.passes_cross_height()
                && m.stats.children.height_reader
                && self.slots[k].stats.own.reflows_height)
            || (m.shape == Shape::Column
                && m.stats.children.height_reader
                && !self.slots[k].shape.independent_height_slot(&self.attrs[k]))
    }
    pub(super) fn is_row_charge(&self, k: Key) -> bool {
        let m = &self.slots[k];
        m.stats.own.late_nonfill && self.parent_shape(k).is_some_and(Shape::allots_width)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rotation {
    cos: f32,
    sin: f32,
}
impl Default for Rotation {
    fn default() -> Self {
        Self { cos: 1., sin: 0. }
    }
}
impl Rotation {
    pub fn degrees(angle: f64) -> (f32, Self) {
        let d = (angle % 360.) as f32;
        let quarter = (d / 90.).round();
        if (d - quarter * 90.).abs() <= 0.0001 {
            let swaps = (quarter as i32) % 2 != 0;
            (
                quarter * 90. % 360.,
                if swaps {
                    Self { cos: 0., sin: 1. }
                } else {
                    Self::default()
                },
            )
        } else {
            let (s, c) = d.to_radians().sin_cos();
            (
                d,
                Self {
                    cos: c.abs(),
                    sin: s.abs(),
                },
            )
        }
    }
    pub fn mixes_axes(self) -> bool {
        self.sin != 0.
    }
    pub fn swaps_axes(self) -> bool {
        self.cos == 0.
    }
    pub fn extent(self, w: f32, h: f32) -> (f32, f32) {
        if !self.mixes_axes() {
            (w, h)
        } else if self.swaps_axes() {
            (h, w)
        } else {
            (
                (self.cos * w + self.sin * h).min(f32::MAX),
                (self.sin * w + self.cos * h).min(f32::MAX),
            )
        }
    }
    /// Project in place about the same center used by the signed paint pose.
    pub fn about_center(self, local: Rect) -> Rect {
        let slot = self.slot(local);
        Rect {
            x: coordinate(local.x + (local.width - slot.width) * 0.5),
            y: coordinate(local.y + (local.height - slot.height) * 0.5),
            ..slot
        }
    }
    pub fn slot(self, local: Rect) -> Rect {
        let (width, height) = self.extent(local.width, local.height);
        Rect {
            width,
            height,
            ..local
        }
    }
}
