//! Layout reserves the AABB of the unrotated border box. The coefficients
//! apply to lengths only; paint uses the signed angle about the same center.
use super::*;

/// Declaration-only facts on the edge. A natural paragraph height needs its
/// local width first; that width need not depend on its parent's split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Dependencies {
    pub height_reader: bool,
    reflows_height: bool,
    late_turn: bool,
    pub late_nonfill: bool,
    // A late non-fill which cannot finish before a Row hands out width shares.
    unready_nonfill: bool,
    pub fills_width: bool,
    pub contains_row_split: bool,
}
impl Dependencies {
    pub fn union(self, child: Self) -> Self {
        Self {
            height_reader: self.height_reader || child.height_reader,
            reflows_height: self.reflows_height || child.reflows_height,
            late_turn: self.late_turn || child.late_turn,
            late_nonfill: self.late_nonfill || child.late_nonfill,
            unready_nonfill: self.unready_nonfill || child.unready_nonfill,
            fills_width: self.fills_width || child.fills_width,
            contains_row_split: self.contains_row_split || child.contains_row_split,
        }
    }
}
impl Subtree {
    pub(super) fn splits_after_reflow(self, shape: Shape) -> bool {
        shape == Shape::Row && self.children.late_nonfill && self.children.fills_width
    }
    pub fn with_layout(mut self, shape: Shape, a: &Attrs) -> Result<Self, String> {
        let kids = self.children;
        let b = a.sizing;
        if kids.late_turn {
            // Place can use the final fit. A Row split can also use it if
            // every late charge can reflow independently before that split.
            let can_resolve = matches!(b.width, Length::Px(_))
                && match shape {
                    Shape::Row => !kids.fills_width || !kids.unready_nonfill,
                    Shape::WrappedRow => !kids.late_nonfill,
                    Shape::El
                    | Shape::Column
                    | Shape::WrappedColumn
                    | Shape::Paragraph
                    | Shape::TextColumn => true,
                    _ => false,
                };
            if !can_resolve {
                return Err("rotated width is needed before child reflow; declare an independent height or use a placement-only site".into());
            }
        }
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
        let late_nonfill = late_turn && b.width.weight().is_none();
        let contains_row_split = kids.contains_row_split || self.splits_after_reflow(shape);
        self.own = Dependencies {
            reflows_height: local_reflows
                || (mixes && (!matches!(b.width, Length::Px(_)) || b.height.weight().is_some())),
            late_turn,
            late_nonfill,
            // For an early charge, width must be known without the Row's
            // offer and height must not change in Place. A nested dependent
            // split would require another scheduling boundary.
            unready_nonfill: late_nonfill
                && (!matches!(b.width, Length::Px(_))
                    || b.height.weight().is_some()
                    || contains_row_split),
            contains_row_split,
            fills_width: b.width.weight().is_some(),
            height_reader: kids.height_reader || shape.projects_height(a),
        };
        Ok(self)
    }
}

impl Elements {
    pub(super) fn is_row_charge(&self, k: Key) -> bool {
        let m = &self.slots[k];
        m.stats.own.late_nonfill
            && m.parent.is_some_and(|p| {
                let host = &self.slots[p];
                host.stats.splits_after_reflow(host.shape)
            })
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
    pub fn slot(self, local: Rect) -> Rect {
        let (width, height) = self.extent(local.width, local.height);
        Rect {
            width,
            height,
            ..local
        }
    }
}
