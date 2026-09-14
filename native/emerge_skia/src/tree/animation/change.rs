//! Change is admission/source selection; sampling remains the ordinary once spec.
use super::*;
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Width,
    Height,
    Padding,
    Spacing,
    SpacingXy,
    Background,
    BorderRadius,
    BorderWidth,
    BorderColor,
    BoxShadow,
    FontSize,
    FontColor,
    FontLetterSpacing,
    FontWordSpacing,
    SvgColor,
    LayoutScale,
    LayoutRotate,
    MoveX,
    MoveY,
    Rotate,
    Scale,
    Alpha,
}
impl Field {
    pub const ALL: [Self; 22] = [
        Self::Width,
        Self::Height,
        Self::Padding,
        Self::Spacing,
        Self::SpacingXy,
        Self::Background,
        Self::BorderRadius,
        Self::BorderWidth,
        Self::BorderColor,
        Self::BoxShadow,
        Self::FontSize,
        Self::FontColor,
        Self::FontLetterSpacing,
        Self::FontWordSpacing,
        Self::SvgColor,
        Self::LayoutScale,
        Self::LayoutRotate,
        Self::MoveX,
        Self::MoveY,
        Self::Rotate,
        Self::Scale,
        Self::Alpha,
    ];
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "width" => Some(Self::Width),
            "height" => Some(Self::Height),
            "padding" => Some(Self::Padding),
            "spacing" => Some(Self::Spacing),
            "spacing_xy" => Some(Self::SpacingXy),
            "background" => Some(Self::Background),
            "border_radius" => Some(Self::BorderRadius),
            "border_width" => Some(Self::BorderWidth),
            "border_color" => Some(Self::BorderColor),
            "box_shadow" => Some(Self::BoxShadow),
            "font_size" => Some(Self::FontSize),
            "font_color" => Some(Self::FontColor),
            "font_letter_spacing" => Some(Self::FontLetterSpacing),
            "font_word_spacing" => Some(Self::FontWordSpacing),
            "svg_color" => Some(Self::SvgColor),
            "layout_scale" => Some(Self::LayoutScale),
            "layout_rotate" => Some(Self::LayoutRotate),
            "move_x" => Some(Self::MoveX),
            "move_y" => Some(Self::MoveY),
            "rotate" => Some(Self::Rotate),
            "scale" => Some(Self::Scale),
            "alpha" => Some(Self::Alpha),
            _ => None,
        }
    }
    pub fn extract(self, attrs: &Attrs) -> Attrs {
        match self {
            Self::Width => Attrs {
                width: attrs.width.clone(),
                ..Attrs::default()
            },
            Self::Height => Attrs {
                height: attrs.height.clone(),
                ..Attrs::default()
            },
            Self::Padding => Attrs {
                padding: attrs.padding.clone(),
                ..Attrs::default()
            },
            Self::Spacing => Attrs {
                spacing: attrs.spacing,
                ..Attrs::default()
            },
            Self::SpacingXy => Attrs {
                spacing_x: attrs.spacing_x,
                spacing_y: attrs.spacing_y,
                ..Attrs::default()
            },
            Self::Background => Attrs {
                background: attrs.background.clone(),
                ..Attrs::default()
            },
            Self::BorderRadius => Attrs {
                border_radius: attrs.border_radius.clone(),
                ..Attrs::default()
            },
            Self::BorderWidth => Attrs {
                border_width: attrs.border_width.clone(),
                ..Attrs::default()
            },
            Self::BorderColor => Attrs {
                border_color: attrs.border_color.clone(),
                ..Attrs::default()
            },
            Self::BoxShadow => Attrs {
                box_shadows: attrs.box_shadows.clone(),
                ..Attrs::default()
            },
            Self::FontSize => Attrs {
                font_size: attrs.font_size,
                ..Attrs::default()
            },
            Self::FontColor => Attrs {
                font_color: attrs.font_color.clone(),
                ..Attrs::default()
            },
            Self::FontLetterSpacing => Attrs {
                font_letter_spacing: attrs.font_letter_spacing,
                ..Attrs::default()
            },
            Self::FontWordSpacing => Attrs {
                font_word_spacing: attrs.font_word_spacing,
                ..Attrs::default()
            },
            Self::SvgColor => Attrs {
                svg_color: attrs.svg_color.clone(),
                ..Attrs::default()
            },
            Self::LayoutScale => Attrs {
                layout_scale: attrs.layout_scale,
                ..Attrs::default()
            },
            Self::LayoutRotate => Attrs {
                layout_rotate: attrs.layout_rotate,
                ..Attrs::default()
            },
            Self::MoveX => Attrs {
                move_x: attrs.move_x,
                ..Attrs::default()
            },
            Self::MoveY => Attrs {
                move_y: attrs.move_y,
                ..Attrs::default()
            },
            Self::Rotate => Attrs {
                rotate: attrs.rotate,
                ..Attrs::default()
            },
            Self::Scale => Attrs {
                scale: attrs.scale,
                ..Attrs::default()
            },
            Self::Alpha => Attrs {
                alpha: attrs.alpha,
                ..Attrs::default()
            },
        }
    }
    pub fn present(self, attrs: &Attrs) -> bool {
        match self {
            Self::Width => attrs.width.is_some(),
            Self::Height => attrs.height.is_some(),
            Self::Padding => attrs.padding.is_some(),
            Self::Spacing => attrs.spacing.is_some(),
            Self::SpacingXy => attrs.spacing_x.is_some() || attrs.spacing_y.is_some(),
            Self::Background => attrs.background.is_some(),
            Self::BorderRadius => attrs.border_radius.is_some(),
            Self::BorderWidth => attrs.border_width.is_some(),
            Self::BorderColor => attrs.border_color.is_some(),
            Self::BoxShadow => attrs.box_shadows.is_some(),
            Self::FontSize => attrs.font_size.is_some(),
            Self::FontColor => attrs.font_color.is_some(),
            Self::FontLetterSpacing => attrs.font_letter_spacing.is_some(),
            Self::FontWordSpacing => attrs.font_word_spacing.is_some(),
            Self::SvgColor => attrs.svg_color.is_some(),
            Self::LayoutScale => attrs.layout_scale.is_some(),
            Self::LayoutRotate => attrs.layout_rotate.is_some(),
            Self::MoveX => attrs.move_x.is_some(),
            Self::MoveY => attrs.move_y.is_some(),
            Self::Rotate => attrs.rotate.is_some(),
            Self::Scale => attrs.scale.is_some(),
            Self::Alpha => attrs.alpha.is_some(),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ChangePolicy {
    pub field: Field,
    pub duration_ms: f64,
    pub curve: AnimationCurve,
}
#[derive(Clone, Debug)]
pub(super) struct ChangeEntry {
    pub resolved_target: Option<crate::tree::layout::dimensions::AxisFootprint>,
    pub generation: RunGeneration,
    pub spec: Arc<AnimationSpec>,
    pub started_at: Instant,
    pub mount: u64,
    pub source: Arc<source::PresentationSource>,
    pub pending: bool,
}

pub(crate) fn validate_policies(attrs: &Attrs) -> Result<(), String> {
    let mut seen = fields::FieldMask::default();
    for policy in attrs.animate_change.as_deref().into_iter().flatten() {
        let length = match policy.field {
            Field::Width => attrs.width.as_ref(),
            Field::Height => attrs.height.as_ref(),
            _ => None,
        };
        if let Some(length) = length {
            super::validate_animated_length(length)?;
        }
        let writes = fields::FieldMask::field(policy.field);
        if seen.conflicts(writes)
            || !policy.duration_ms.is_finite()
            || policy.duration_ms <= 0.0
            || !policy.field.present(attrs)
        {
            return Err("invalid animate_change policy or missing target".into());
        }
        seen = seen.union(writes);
        if attrs.animate.as_ref().is_some_and(|spec| {
            spec.keyframes
                .iter()
                .any(|frame| fields::FieldMask::from_attrs(frame).conflicts(writes))
        }) {
            return Err("animate and animate_change cannot own the same field".into());
        }
    }
    Ok(())
}

pub(crate) fn validate_transition(before: &Attrs, after: &Attrs) -> Result<(), String> {
    validate_policies(after)?;
    for policy in after.animate_change.as_deref().into_iter().flatten() {
        let a = policy.field.extract(before);
        let b = policy.field.extract(after);
        if !policy.field.present(&a) {
            continue;
        }
        for length in a
            .width
            .iter()
            .chain(a.height.iter())
            .chain(b.width.iter())
            .chain(b.height.iter())
        {
            super::validate_animated_length(length)?;
        }
        let compatible = match policy.field {
            Field::Padding => matches!(
                (&a.padding, &b.padding),
                (Some(Padding::Uniform(_)), Some(Padding::Uniform(_)))
                    | (Some(Padding::Sides { .. }), Some(Padding::Sides { .. }))
            ),
            Field::BorderRadius => matches!(
                (&a.border_radius, &b.border_radius),
                (
                    Some(BorderRadius::Uniform(_)),
                    Some(BorderRadius::Uniform(_))
                ) | (
                    Some(BorderRadius::Corners { .. }),
                    Some(BorderRadius::Corners { .. })
                )
            ),
            Field::BorderWidth => matches!(
                (&a.border_width, &b.border_width),
                (Some(BorderWidth::Uniform(_)), Some(BorderWidth::Uniform(_)))
                    | (
                        Some(BorderWidth::Sides { .. }),
                        Some(BorderWidth::Sides { .. })
                    )
            ),
            Field::Background => match (&a.background, &b.background) {
                (Some(Background::Color(a)), Some(Background::Color(b))) => colors_compatible(a, b),
                (
                    Some(Background::Image { source: a, fit: b }),
                    Some(Background::Image { source: c, fit: d }),
                ) => a == c && b == d,
                _ => false,
            },
            Field::FontColor | Field::BorderColor | Field::SvgColor => {
                let a = a
                    .font_color
                    .as_ref()
                    .or(a.border_color.as_ref())
                    .or(a.svg_color.as_ref());
                let b = b
                    .font_color
                    .as_ref()
                    .or(b.border_color.as_ref())
                    .or(b.svg_color.as_ref());
                a.zip(b).is_some_and(|(a, b)| colors_compatible(a, b))
            }
            Field::BoxShadow => a
                .box_shadows
                .as_ref()
                .zip(b.box_shadows.as_ref())
                .is_some_and(|(a, b)| {
                    a.len() == b.len()
                        && a.iter()
                            .zip(b)
                            .all(|(a, b)| colors_compatible(&a.color, &b.color))
                }),
            _ => true,
        };
        if !compatible {
            return Err(format!(
                "incompatible animate_change values for {:?}",
                policy.field
            ));
        }
    }
    Ok(())
}
fn colors_compatible(a: &Color, b: &Color) -> bool {
    match (a, b) {
        (Color::Gradient { colors: a, .. }, Color::Gradient { colors: b, .. }) => {
            a.len() == b.len()
        }
        _ => true,
    }
}

impl AnimationRuntime {
    /// Admit/cancel first, refresh barriers, then finish unheld clocks. Pruning
    /// earlier can lose a boundary-time join or retain an orphaned held source.
    pub(super) fn finish_unheld_changes(
        &mut self,
        tree: &ElementTree,
        now: Instant,
    ) -> Vec<AnimationLayoutEffect> {
        let completed: Vec<_> = self
            .changes
            .iter()
            .filter_map(|(key, entry)| {
                (!entry.pending
                    && !entry_is_active(&entry.spec, entry.started_at, now)
                    && !self.retains_geometry(
                        key.0,
                        groups::Owner::Change(key.1),
                        entry.started_at,
                        tree,
                    ))
                .then_some(*key)
            })
            .collect();
        completed
            .into_iter()
            .filter_map(|key| {
                self.changes
                    .remove(&key)
                    .and_then(|entry| animation_spec_layout_effect(key.0, &entry.spec))
            })
            .collect()
    }
    pub(super) fn sync_changes(
        &mut self,
        tree: &ElementTree,
        now: Instant,
    ) -> Result<AnimationOverlayResult, ProjectionError> {
        let mut completed = AnimationOverlayResult::default();
        self.changes.retain(|(id, field), entry| {
            let keep = tree.get(id).is_some_and(|node| {
                node.is_live()
                    && node.lifecycle.mounted_at_revision == entry.mount
                    && node
                        .spec
                        .declared
                        .animate_change
                        .as_ref()
                        .is_some_and(|policies| policies.iter().any(|p| p.field == *field))
            });
            if !keep && let Some(effect) = animation_spec_layout_effect(*id, &entry.spec) {
                completed.record_effect(effect);
            }
            keep
        });
        for (id, original) in &tree.pending_patch_effects.sources {
            if let Some(node) = tree.get(id) {
                validate_policies(&node.spec.declared)
                    .map_err(|_| ProjectionError::InvalidOwnership(*id))?;
            }
            let Some(node) = tree.get(id).filter(|node| {
                node.is_live()
                    && node.lifecycle.mounted_at_revision == original.mounted_at
                    && node.lifecycle.mounted_at_revision <= self.last_seen_revision
            }) else {
                continue;
            };
            let Some(policies) = &node.spec.declared.animate_change else {
                continue;
            };
            let mut source = original.clone();
            // Implicit lengths have a concrete presentation but no scalar declaration.
            for (i, axis) in [
                crate::tree::layout::dimensions::Axis::Width,
                crate::tree::layout::dimensions::Axis::Height,
            ]
            .into_iter()
            .enumerate()
            {
                let missing = if i == 0 {
                    source.effective.width.is_none()
                } else {
                    source.effective.height.is_none()
                };
                if missing && let Some(fp) = source.dimensions[i] {
                    source.sampled[i] = true;
                    if axis == crate::tree::layout::dimensions::Axis::Width {
                        source.effective.width =
                            Some(Length::Px((fp.visible * source.scale) as f64));
                    } else {
                        source.effective.height =
                            Some(Length::Px((fp.visible * source.scale) as f64));
                    }
                }
            }
            let source = Arc::new(source);
            let raw = scale_animation_keyframe(&source.effective, 1.0 / source.scale as f64);
            for policy in policies.iter() {
                if !timing::valid_duration(policy.duration_ms, &AnimationRepeat::Once, now) {
                    return Err(ProjectionError::InvalidTiming(*id));
                }
                let target = policy.field.extract(&node.spec.declared);
                let key = (*id, policy.field);
                // Identical content declarations are decided against native
                // resolved destinations during measured preparation, not here.
                if super::content::axis(policy.field, &node.spec.declared).is_some()
                    && policy.field.extract(&original.declared) == target
                {
                    continue;
                }
                if let Some(base) = self.changes.committed(&key).filter(|entry| {
                    entry.mount == node.lifecycle.mounted_at_revision
                        && entry.spec.keyframes.last() == Some(&target)
                }) {
                    // Preserve staged clock/source handoffs on this same generation.
                    // Only a replacement/cancellation is undone by returning to A.
                    if self
                        .changes
                        .get(&key)
                        .is_none_or(|entry| entry.generation != base.generation)
                    {
                        let owner = groups::OwnerKey {
                            node: *id,
                            mount: base.mount,
                            kind: groups::Owner::Change(policy.field),
                            generation: base.generation,
                            started: base.started_at,
                        };
                        self.changes.restore(&key);
                        self.groups.restore_owner(owner);
                    }
                    continue;
                }
                if policy.field.extract(&original.declared) == target {
                    // A staged first run may disappear before ever being presented.
                    // A different committed owner still needs a real interruption.
                    if self.changes.committed(&key).is_none() {
                        self.changes.remove(&key);
                        continue;
                    }
                }
                if self
                    .changes
                    .get(&(*id, policy.field))
                    .is_some_and(|entry| entry.spec.keyframes.last() == Some(&target))
                {
                    continue;
                }
                let from = policy.field.extract(&raw);
                if !policy.field.present(&from) {
                    continue;
                }
                for length in from.width.iter().chain(from.height.iter()) {
                    super::validate_animated_length(length)
                        .map_err(|_| ProjectionError::UnsupportedLength(*id))?;
                }
                let pending = self.enter_entries.get(id).is_some_and(|entry| {
                    let phase = self.enter_phases(tree, *id, entry, now);
                    phase
                        .writes()
                        .conflicts(super::fields::FieldMask::field(policy.field))
                });
                let generation = self.allocate_generation()?;
                self.changes.insert(
                    (*id, policy.field),
                    ChangeEntry {
                        resolved_target: None,
                        generation,
                        spec: Arc::new(AnimationSpec {
                            keyframes: vec![from, target],
                            duration_ms: policy.duration_ms,
                            curve: policy.curve.clone(),
                            repeat: AnimationRepeat::Once,
                        }),
                        source: Arc::clone(&source),
                        started_at: now,
                        mount: node.lifecycle.mounted_at_revision,
                        pending,
                    },
                );
            }
        }
        Ok(completed)
    }
    pub(super) fn handoff_changes(
        &mut self,
        tree: &ElementTree,
        id: NodeId,
        enter: &AnimationSpec,
        now: Instant,
    ) {
        self.handoff_change_fields(
            tree,
            id,
            enter,
            super::fields::FieldMask::from_spec(enter),
            now,
        );
    }
    pub(super) fn handoff_completed_enter_paint(&mut self, tree: &ElementTree, now: Instant) {
        let ready: Vec<_> = self
            .enter_entries
            .iter()
            .filter_map(|(id, entry)| {
                let all = entry.fields;
                let phases = self.enter_phases(tree, *id, entry, now);
                let completed = all.without(phases.writes());
                (!completed.is_empty()).then(|| (*id, Arc::clone(&entry.spec), completed))
            })
            .collect();
        for (id, spec, fields) in ready {
            self.handoff_change_fields(tree, id, &spec, fields, now);
        }
    }
    fn handoff_change_fields(
        &mut self,
        tree: &ElementTree,
        id: NodeId,
        enter: &AnimationSpec,
        fields: super::fields::FieldMask,
        now: Instant,
    ) {
        if !Field::ALL.into_iter().any(|field| {
            fields.intersects(super::fields::FieldMask::field(field))
                && self
                    .changes
                    .get(&(id, field))
                    .is_some_and(|entry| entry.pending)
        }) {
            return;
        }
        let Some(mut source) = source::PresentationSource::capture(tree, &id) else {
            return;
        };
        if let Some(last) = enter.keyframes.last() {
            let last = fields.select(last);
            apply_sample_attrs(
                &mut source.effective,
                &scale_animation_keyframe(&last, source.scale as f64),
            );
            for (i, axis) in [
                crate::tree::layout::dimensions::Axis::Width,
                crate::tree::layout::dimensions::Axis::Height,
            ]
            .into_iter()
            .enumerate()
            {
                if (i == 0 && last.width.is_some()) || (i == 1 && last.height.is_some()) {
                    source.dimensions[i] = tree
                        .length_runtime
                        .as_ref()
                        .and_then(|r| r.terminal(id, axis));
                    source.sampled[i] = source.dimensions[i].is_some();
                }
            }
        }
        let raw = scale_animation_keyframe(&source.effective, 1.0 / source.scale as f64);
        let source = Arc::new(source);
        for field in Field::ALL {
            if !fields.intersects(super::fields::FieldMask::field(field))
                || !self
                    .changes
                    .get(&(id, field))
                    .is_some_and(|entry| entry.pending)
            {
                continue;
            }
            if let Some(entry) = self.changes.get_mut(&(id, field)) {
                Arc::make_mut(&mut entry.spec).keyframes[0] = field.extract(&raw);
                entry.source = Arc::clone(&source);
                entry.started_at = now;
                entry.pending = false;
            }
        }
    }
}
