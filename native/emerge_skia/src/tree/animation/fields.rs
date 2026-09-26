//! Canonical animation write sets. Wire/API spellings are not ownership identities.
use super::{Attrs, change::Field};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FieldMask(u32);
impl FieldMask {
    const ALIGN_X: Self = Self(1 << 22);
    const ALIGN_Y: Self = Self(1 << 23);
    pub fn field(field: Field) -> Self {
        // Uniform spacing and per-axis spacing write the same allocation inputs.
        let canonical = if field == Field::SpacingXy {
            Field::Spacing
        } else {
            field
        };
        Self(1 << canonical as u32)
    }
    pub fn from_attrs(attrs: &Attrs) -> Self {
        let alignment = Self(
            if attrs.align_x.is_some() {
                Self::ALIGN_X.0
            } else {
                0
            } | if attrs.align_y.is_some() {
                Self::ALIGN_Y.0
            } else {
                0
            },
        );
        Field::ALL
            .into_iter()
            .filter(|f| f.present(attrs))
            .fold(alignment, |mask, field| mask.union(Self::field(field)))
    }
    pub fn from_spec(spec: &super::AnimationSpec) -> Self {
        spec.keyframes.iter().fold(Self::default(), |mask, attrs| {
            mask.union(Self::from_attrs(attrs))
        })
    }
    pub fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn layout(self) -> Self {
        let layout = [
            Field::Width,
            Field::Height,
            Field::Padding,
            Field::Spacing,
            Field::BorderWidth,
            Field::FontSize,
            Field::FontLetterSpacing,
            Field::FontWordSpacing,
            Field::LayoutScale,
            Field::LayoutRotate,
        ]
        .into_iter()
        .fold(Self::default(), |mask, field| {
            mask.union(Self::field(field))
        });
        Self(self.0 & layout.union(Self::ALIGN_X).union(Self::ALIGN_Y).0)
    }
    pub fn blocked_by(self, other: Self) -> Self {
        let mut blocked = Self(self.0 & other.0);
        for (layout, paint) in [
            (Field::LayoutScale, Field::Scale),
            (Field::LayoutRotate, Field::Rotate),
        ] {
            let layout = Self::field(layout);
            let paint = Self::field(paint);
            if other.intersects(layout) {
                blocked = blocked.union(Self(self.0 & paint.0));
            }
            if other.intersects(paint) {
                blocked = blocked.union(Self(self.0 & layout.0));
            }
        }
        blocked
    }
    pub fn select(self, attrs: &Attrs) -> Attrs {
        Field::ALL
            .into_iter()
            .filter(|field| self.intersects(Self::field(*field)))
            .fold(
                Attrs {
                    align_x: attrs.align_x.filter(|_| self.intersects(Self::ALIGN_X)),
                    align_y: attrs.align_y.filter(|_| self.intersects(Self::ALIGN_Y)),
                    ..Attrs::default()
                },
                |mut selected, field| {
                    super::apply_sample_attrs(&mut selected, &field.extract(attrs));
                    selected
                },
            )
    }
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
    pub fn conflicts(self, other: Self) -> bool {
        self.intersects(other)
            || [
                (Field::LayoutScale, Field::Scale),
                (Field::LayoutRotate, Field::Rotate),
            ]
            .into_iter()
            .any(|(layout, paint)| {
                let layout = Self::field(layout);
                let paint = Self::field(paint);
                (self.intersects(layout) && other.intersects(paint))
                    || (self.intersects(paint) && other.intersects(layout))
            })
    }
}

/// Disjoint candidate write phases; these do not introduce a second clock or spec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PhaseMasks {
    pub moving: FieldMask,
    pub held: FieldMask,
    pub pending: FieldMask,
    pub releasing: FieldMask,
}
impl PhaseMasks {
    pub fn new(
        fields: FieldMask,
        active: bool,
        retained: bool,
        releasing: bool,
        blocked: FieldMask,
    ) -> Self {
        let pending = fields.blocked_by(blocked);
        let available = fields.without(pending);
        let mut phases = Self {
            pending,
            ..Default::default()
        };
        if active {
            phases.moving = available;
        } else if retained {
            if releasing {
                phases.releasing = available.layout();
            } else {
                phases.held = available.layout();
            }
        }
        phases
    }
    pub fn writes(self) -> FieldMask {
        self.moving.union(self.held).union(self.releasing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::animation::{
        AnimationCurve, AnimationRepeat, AnimationSpec,
        change::{ChangePolicy, validate_policies},
    };
    use std::sync::Arc;
    #[test]
    fn explicit_alignment_is_part_of_the_held_native_layout_mask() {
        let attrs = Attrs {
            align_x: Some(crate::tree::attrs::AlignX::Right),
            align_y: Some(crate::tree::attrs::AlignY::Bottom),
            alpha: Some(0.4),
            ..Default::default()
        };
        let mask = FieldMask::from_attrs(&attrs);
        let held = PhaseMasks::new(mask, false, true, false, FieldMask::default())
            .writes()
            .select(&attrs);
        assert_eq!(held.align_x, attrs.align_x);
        assert_eq!(held.align_y, attrs.align_y);
        assert!(held.alpha.is_none());
    }
    #[test]
    fn phase_masks_are_disjoint_and_only_geometry_is_held_or_releasing() {
        let width = FieldMask::field(Field::Width);
        let alpha = FieldMask::field(Field::Alpha);
        let all = width.union(alpha);
        let moving = PhaseMasks::new(all, true, true, false, FieldMask::default());
        assert_eq!(moving.moving, all);
        let held = PhaseMasks::new(all, false, true, false, FieldMask::default());
        assert_eq!(held.held, width);
        assert_eq!(held.writes(), width);
        let releasing = PhaseMasks::new(all, false, true, true, FieldMask::default());
        assert_eq!(releasing.releasing, width);
        assert!(releasing.held.is_empty());
        let blocked = PhaseMasks::new(all, true, false, false, width);
        assert_eq!(blocked.pending, width);
        assert_eq!(blocked.moving, alpha);
        assert!(!blocked.pending.intersects(blocked.writes()));
    }
    #[test]
    fn conflicting_transforms_block_without_aliasing_distinct_writes() {
        let paint = FieldMask::field(Field::Scale);
        let layout = FieldMask::field(Field::LayoutScale);
        let phase = PhaseMasks::new(paint, true, false, false, layout);
        assert_eq!(phase.pending, paint);
        assert!(phase.writes().is_empty());
        assert!(
            layout
                .layout()
                .select(&Attrs {
                    layout_scale: Some(2.0),
                    scale: Some(3.0),
                    ..Default::default()
                })
                .scale
                .is_none()
        );
    }
    fn policy(field: Field) -> ChangePolicy {
        ChangePolicy {
            field,
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
        }
    }
    #[test]
    fn spacing_aliases_have_one_write_set_but_paint_and_layout_transforms_are_distinct() {
        assert_eq!(
            FieldMask::field(Field::Spacing),
            FieldMask::field(Field::SpacingXy)
        );
        let layout = FieldMask::field(Field::LayoutScale);
        let paint = FieldMask::field(Field::Scale);
        assert!(!layout.intersects(paint));
        assert!(layout.conflicts(paint));
        assert!(paint.conflicts(layout));
        assert!(!FieldMask::field(Field::Alpha).conflicts(FieldMask::field(Field::Width)));
    }
    #[test]
    fn duplicate_alias_policies_and_cross_transform_policies_are_rejected() {
        for (a, b, attrs) in [
            (
                Field::Spacing,
                Field::SpacingXy,
                Attrs {
                    spacing: Some(10.0),
                    spacing_x: Some(20.0),
                    spacing_y: Some(20.0),
                    ..Default::default()
                },
            ),
            (
                Field::LayoutScale,
                Field::Scale,
                Attrs {
                    layout_scale: Some(2.0),
                    scale: Some(2.0),
                    ..Default::default()
                },
            ),
            (
                Field::LayoutRotate,
                Field::Rotate,
                Attrs {
                    layout_rotate: Some(10.0),
                    rotate: Some(10.0),
                    ..Default::default()
                },
            ),
        ] {
            for policies in [vec![policy(a), policy(b)], vec![policy(b), policy(a)]] {
                assert!(
                    validate_policies(&Attrs {
                        animate_change: Some(Arc::new(policies)),
                        ..attrs.clone()
                    })
                    .is_err()
                );
            }
        }
    }
    #[test]
    fn regular_change_overlap_uses_canonical_writes_and_checks_every_keyframe() {
        let explicit = AnimationSpec {
            keyframes: vec![
                Attrs::default(),
                Attrs {
                    spacing: Some(20.0),
                    ..Default::default()
                },
            ],
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        };
        assert!(
            validate_policies(&Attrs {
                spacing_x: Some(10.0),
                animate: Some(explicit),
                animate_change: Some(Arc::new(vec![policy(Field::SpacingXy)])),
                ..Default::default()
            })
            .is_err()
        );
    }
    #[test]
    fn disjoint_regular_and_change_fields_remain_legal() {
        let explicit = AnimationSpec {
            keyframes: vec![
                Attrs {
                    alpha: Some(0.0),
                    ..Default::default()
                },
                Attrs {
                    alpha: Some(1.0),
                    ..Default::default()
                },
            ],
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        };
        assert!(
            validate_policies(&Attrs {
                width: Some(crate::tree::attrs::Length::Fill),
                animate: Some(explicit),
                animate_change: Some(Arc::new(vec![policy(Field::Width)])),
                ..Default::default()
            })
            .is_ok()
        );
    }
}
