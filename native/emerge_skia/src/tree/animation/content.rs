//! Admission of content policies whose declaration is unchanged but native size changed.
//! Uses the existing renderer-local length workspace and ordinary change owners.
use super::*;
use crate::tree::layout::{
    FontContext, TextMeasurer,
    dimensions::{Axis, AxisFootprint},
    projection::QueryContext,
};

#[derive(Debug)]
pub(super) struct InputAttempt {
    pub model: u64,
    pub context: Arc<QueryContext>,
    pub admitted_at: Instant,
    pub checked: bool,
    pub has_presentation: bool,
}

pub(super) fn axis(field: change::Field, attrs: &Attrs) -> Option<Axis> {
    match field {
        change::Field::Width if attrs.width == Some(Length::Content) => Some(Axis::Width),
        change::Field::Height if attrs.height == Some(Length::Content) => Some(Axis::Height),
        _ => None,
    }
}
pub(super) fn has_policy(attrs: &Attrs) -> bool {
    attrs
        .animate_change
        .as_deref()
        .into_iter()
        .flatten()
        .any(|p| axis(p.field, attrs).is_some())
}
fn same_inputs(a: &QueryContext, b: &QueryContext) -> bool {
    a.constraint == b.constraint
        && a.scale == b.scale
        && a.inherited == b.inherited
        && a.metrics_epoch == b.metrics_epoch
        && a.fonts == b.fonts
        && a.images == b.images
        && a.seeds.len() == b.seeds.len()
        && a.seeds.iter().all(|(id, seed)| {
            b.seeds
                .get(id)
                .is_some_and(|other| seed.runtime == other.runtime)
        })
    // Scroll ranges/offset clamping are layout outputs, not new intrinsic targets.
}
impl AnimationRuntime {
    pub(crate) fn resolve_content_changes<M: TextMeasurer>(
        &mut self,
        tree: &mut ElementTree,
        now: Instant,
        scale: f32,
        inherited: &FontContext,
        measurer: &M,
    ) -> Result<(), ProjectionError> {
        if let Some(error) = self.admission_error.clone() {
            return Err(error);
        }
        if self.content_nodes.is_empty() {
            if let Some(state) = tree.length_runtime.as_mut() {
                state.content_inputs = None;
            }
            return Ok(());
        }
        let published = tree.publication.0;
        let eligible: Vec<_> = self
            .content_nodes
            .iter()
            .copied()
            .filter(|id| {
                tree.get(id).is_some_and(|node| {
                    node.is_live()
                        && has_policy(&node.spec.declared)
                        && node.layout.frame.is_some()
                        && published.is_some_and(|published| {
                            node.lifecycle.mounted_at_revision <= published.revision
                        })
                })
            })
            .collect();
        if eligible.is_empty() && tree.animation_constraint.is_none() {
            return Ok(());
        }
        let mut state = tree.length_runtime.take().unwrap_or_default();
        let result = (|| {
            let context = state.query_context(tree, scale, inherited, measurer)?;
            let environment_changed = state
                .content_inputs
                .as_ref()
                .is_none_or(|old| !same_inputs(&old.context, &context));
            if !environment_changed
                && state
                    .content_inputs
                    .as_ref()
                    .is_some_and(|old| old.model == tree.layout_model_epoch && old.checked)
            {
                return Ok(());
            }
            let admitted_at = state
                .content_inputs
                .as_ref()
                .filter(|old| {
                    old.model == tree.layout_model_epoch && same_inputs(&old.context, &context)
                })
                .map_or(now, |old| old.admitted_at);
            state.content_inputs = Some(InputAttempt {
                model: tree.layout_model_epoch,
                context: Arc::clone(&context),
                admitted_at,
                checked: false,
                has_presentation: !eligible.is_empty(),
            });
            let mut candidates: Vec<_> = eligible
                .iter()
                .filter_map(|id| tree.get(id).map(|node| (*id, node)))
                .filter(|(id, node)| {
                    environment_changed
                        || node.layout.measure_dirty
                        || node.layout.measure_descendant_dirty
                        || node.layout.resolve_dirty
                        || tree.pending_patch_effects.sources.contains_key(id)
                })
                .flat_map(|(id, node)| {
                    node.spec
                        .declared
                        .animate_change
                        .as_deref()
                        .into_iter()
                        .flatten()
                        .filter_map(move |p| {
                            axis(p.field, &node.spec.declared).map(|axis| (id, axis, p.clone()))
                        })
                })
                .collect();
            // Stable admission identities independent of hash/query iteration order.
            candidates.sort_unstable_by_key(|(id, axis, _)| (id.0, *axis == Axis::Height));
            if !candidates.is_empty() {
                // All watched content axes are unanimated in this query. In particular,
                // descendants cannot feed their current samples back into an ancestor's
                // destination. Other declared layout and live metric facts remain native.
                let mut projection = state.content_projection(tree, self, now)?;
                for id in &eligible {
                    let node = tree.get(id).ok_or(ProjectionError::UnknownNode(*id))?;
                    for policy in node
                        .spec
                        .declared
                        .animate_change
                        .as_deref()
                        .into_iter()
                        .flatten()
                    {
                        if let Some(axis) = axis(policy.field, &node.spec.declared) {
                            let projected = projection.nodes.entry(*id).or_default();
                            match axis {
                                Axis::Width => projected.width = None,
                                Axis::Height => projected.height = None,
                            }
                            apply_sample_attrs(
                                &mut projected.attrs,
                                &policy.field.extract(&node.spec.declared),
                            );
                        }
                    }
                }
                let projection = Arc::new(projection);
                let endpoints: Vec<_> = candidates
                    .iter()
                    .map(|(id, axis, _)| (*id, *axis))
                    .collect();
                let targets = state.content_targets(
                    tree,
                    Arc::clone(&context),
                    projection,
                    &endpoints,
                    measurer,
                )?;
                for (id, axis, policy) in candidates {
                    let Some(target) = targets.get(&(id, axis)).copied() else {
                        return Err(ProjectionError::MissingLayout(id, axis));
                    };
                    if let Some(source) = tree
                        .pending_patch_effects
                        .sources
                        .get(&id)
                        .cloned()
                        .or_else(|| source::PresentationSource::capture(tree, &id))
                    {
                        self.admit_content_target(
                            tree,
                            &state,
                            id,
                            axis,
                            &policy,
                            target,
                            &source,
                            now,
                            admitted_at,
                        )?;
                    }
                }
            }
            state.content_inputs = Some(InputAttempt {
                model: tree.layout_model_epoch,
                context,
                admitted_at,
                checked: true,
                has_presentation: !eligible.is_empty(),
            });
            Ok(())
        })();
        if !state.is_empty() || result.is_err() {
            tree.length_runtime = Some(state);
        }
        if result.is_ok() {
            self.sync_groups(tree, now);
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn admit_content_target(
        &mut self,
        tree: &ElementTree,
        state: &lengths::LengthRuntime,
        id: NodeId,
        axis: Axis,
        policy: &change::ChangePolicy,
        target: AxisFootprint,
        source: &source::PresentationSource,
        now: Instant,
        admitted_at: Instant,
    ) -> Result<(), ProjectionError> {
        let key = (id, policy.field);
        let matches = |old: AxisFootprint| old.release_matches(target);
        if let Some(base) = self
            .changes
            .committed(&key)
            .filter(|e| e.resolved_target.is_some_and(matches))
        {
            if self
                .changes
                .get(&key)
                .is_none_or(|entry| entry.generation != base.generation)
            {
                let owner = groups::OwnerKey {
                    node: id,
                    mount: base.mount,
                    kind: groups::Owner::Change(policy.field),
                    generation: base.generation,
                    started: base.started_at,
                };
                self.changes.restore(&key);
                self.groups.restore_owner(owner);
            }
            return Ok(());
        }
        if let Some(entry) = self.changes.get(&key) {
            if entry.resolved_target.is_some_and(matches) {
                return Ok(());
            }
            if entry.resolved_target.is_none()
                && (state.terminal(id, axis).is_some_and(matches)
                    || self.changes.committed(&key).is_none())
            {
                let mut entry = entry.clone();
                entry.resolved_target = Some(target);
                self.changes.insert(key, entry);
                return Ok(());
            }
        } else if source.dimensions[usize::from(axis == Axis::Height)].is_some_and(matches) {
            return Ok(());
        }
        // Failed first admission followed by a return to the published destination.
        if self.changes.committed(&key).is_none()
            && source.dimensions[usize::from(axis == Axis::Height)].is_some_and(matches)
        {
            self.changes.remove(&key);
            return Ok(());
        }
        if !timing::valid_duration(policy.duration_ms, &AnimationRepeat::Once, now) {
            return Err(ProjectionError::InvalidTiming(id));
        }
        let Some(from) = source.dimensions[usize::from(axis == Axis::Height)] else {
            return Ok(());
        };
        let mut source = source.clone();
        source.sampled[usize::from(axis == Axis::Height)] = true;
        let mut first = Attrs::default();
        match axis {
            Axis::Width => first.width = Some(Length::Px(from.visible as f64)),
            Axis::Height => first.height = Some(Length::Px(from.visible as f64)),
        }
        let pending = self.enter_entries.get(&id).is_some_and(|entry| {
            self.enter_phases(tree, id, entry, now)
                .writes()
                .conflicts(fields::FieldMask::field(policy.field))
        });
        let generation = self.allocate_generation()?;
        self.changes.insert(
            key,
            change::ChangeEntry {
                generation,
                resolved_target: Some(target),
                spec: Arc::new(AnimationSpec {
                    keyframes: vec![
                        first,
                        policy.field.extract(
                            &tree
                                .get(&id)
                                .ok_or(ProjectionError::UnknownNode(id))?
                                .spec
                                .declared,
                        ),
                    ],
                    duration_ms: policy.duration_ms,
                    curve: policy.curve.clone(),
                    repeat: AnimationRepeat::Once,
                }),
                started_at: admitted_at,
                mount: source.mounted_at,
                source: Arc::new(source),
                pending,
            },
        );
        Ok(())
    }
}
