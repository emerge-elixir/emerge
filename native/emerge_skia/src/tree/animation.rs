mod admission;
pub mod change;
mod content;
mod fields;
pub(crate) mod frame;
use frame::{RuntimeAuthority, RuntimeIdentity};
pub(crate) mod groups;
pub mod lengths;
mod regular;
pub mod source;
pub(crate) mod timing;

use super::layout::projection::ProjectionError;
use admission::AdmissionMap;
use groups::RunGeneration;
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Instant;

use super::attrs::{
    Attrs, Background, BorderRadius, BorderWidth, BoxShadow, Color, Length, Padding,
};
use super::element::{ElementTree, NodeId};
use super::invalidation::{TreeInvalidation, animation_attrs_affect_registry_refresh};

#[derive(Clone, Debug, PartialEq)]
pub enum AnimationCurve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnimationRepeat {
    Once,
    Times(u32),
    Loop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationSpec {
    pub keyframes: Vec<Attrs>,
    pub duration_ms: f64,
    pub curve: AnimationCurve,
    pub repeat: AnimationRepeat,
}

#[derive(Clone, Copy, Debug)]
pub struct AnimationRuntimeEntry {
    pub spec_hash: u64,
    pub started_at: Instant,
}

#[derive(Clone, Debug)]
struct OwnedAnimationEntry {
    spec: Arc<AnimationSpec>,
    source: Option<Arc<source::PresentationSource>>,
    pending: fields::FieldMask,
    handoffs: Vec<Arc<regular::SelectedFields>>,
    fields: fields::FieldMask,
    mount: u64,
    clock: AnimationRuntimeEntry,
    generation: RunGeneration,
}

#[derive(Clone, Debug)]
pub struct EnterAnimationRuntimeEntry {
    mount: u64,
    pub(crate) generation: RunGeneration,
    fields: fields::FieldMask,
    pub spec: Arc<AnimationSpec>,
    pub started_at: Instant,
    pub presentation_anchor_pending: bool,
}

#[derive(Clone, Debug)]
pub struct ExitAnimationRuntimeEntry {
    mount: u64,
    source: Option<Arc<source::PresentationSource>>,
    fields: fields::FieldMask,
    pub(crate) generation: RunGeneration,
    pub spec: Arc<AnimationSpec>,
    pub started_at: Instant,
    pub capture_scale: f32,
    pub presentation_anchor_pending: bool,
}

#[derive(Debug, Default)]
pub struct AnimationRuntime {
    identity: Option<RuntimeAuthority>,
    last_generation: u64,
    synced_sample_time: Option<Instant>,
    admission_error: Option<ProjectionError>,
    groups: groups::GroupLedger,
    animate_entries: AdmissionMap<NodeId, OwnedAnimationEntry>,
    enter_entries: AdmissionMap<NodeId, EnterAnimationRuntimeEntry>,
    exit_entries: AdmissionMap<NodeId, ExitAnimationRuntimeEntry>,
    changes: AdmissionMap<(NodeId, change::Field), change::ChangeEntry>,
    content_nodes: HashSet<NodeId>,
    last_seen_revision: u64,
    synced: bool,
    last_root: Option<(NodeId, u64)>,
    #[cfg(test)]
    sync_node_visits: usize,
}

impl Clone for AnimationRuntime {
    fn clone(&self) -> Self {
        Self {
            identity: self.identity.as_ref().map(|_| RuntimeAuthority::default()),
            last_generation: self.last_generation,
            synced_sample_time: self.synced_sample_time,
            admission_error: self.admission_error.clone(),
            groups: self.groups.clone(),
            animate_entries: self.animate_entries.clone(),
            enter_entries: self.enter_entries.clone(),
            exit_entries: self.exit_entries.clone(),
            changes: self.changes.clone(),
            content_nodes: self.content_nodes.clone(),
            last_seen_revision: self.last_seen_revision,
            synced: self.synced,
            last_root: self.last_root,
            #[cfg(test)]
            sync_node_visits: self.sync_node_visits,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnimationSample {
    pub attrs: Attrs,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimationLayoutEffect {
    pub id: NodeId,
    pub invalidation: TreeInvalidation,
    pub registry_refresh: bool,
    pub layout_scale_dirty: bool,
    pub transform_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnimationOverlayResult {
    pub(crate) preparation_error: Option<super::layout::projection::ProjectionError>,
    pub active: bool,
    pub invalidation: TreeInvalidation,
    pub effects: Vec<AnimationLayoutEffect>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnimationSyncResult {
    pub completed: AnimationOverlayResult,
}

#[derive(Debug, Default)]
pub(crate) struct AnimationFrameSamples {
    pub(crate) geometry: Option<lengths::GeometryDelta>,
    pub(crate) samples: HashMap<NodeId, AnimationSample>,
    pub(crate) result: AnimationOverlayResult,
}

impl AnimationOverlayResult {
    fn record_sample(&mut self, id: NodeId, sample: &AnimationSample) {
        self.active |= sample.active;
        if let Some(effect) = animation_attrs_layout_effect(id, &sample.attrs) {
            self.record_effect(effect);
        }
    }

    pub(crate) fn record_effect(&mut self, effect: AnimationLayoutEffect) {
        self.invalidation.add(effect.invalidation);

        if effect.invalidation.is_dirty() {
            self.effects.push(effect);
        }
    }
}

impl AnimationSyncResult {
    fn record_completed_enter(&mut self, id: NodeId, spec: &AnimationSpec) {
        if let Some(effect) = animation_spec_layout_effect(id, spec) {
            self.completed.record_effect(effect);
        }
    }
}

impl AnimationRuntime {
    fn admission_versions(&self) -> [admission::Version; 6] {
        let [groups, owners] = self.groups.versions();
        [
            self.animate_entries.version(),
            self.enter_entries.version(),
            self.exit_entries.version(),
            self.changes.version(),
            groups,
            owners,
        ]
    }

    pub fn sync_with_tree(
        &mut self,
        tree: &ElementTree,
        started_at: Instant,
    ) -> AnimationSyncResult {
        let before_epoch = (
            self.synced_sample_time,
            self.last_seen_revision,
            self.last_root,
            self.last_generation,
            self.admission_error.clone(),
        );
        let before_counts = (
            self.animate_entries.len(),
            self.enter_entries.len(),
            self.exit_entries.len(),
            self.changes.len(),
        );
        self.synced_sample_time = Some(started_at);
        let result = self.try_sync_with_tree(tree, started_at);
        if before_counts
            != (
                self.animate_entries.len(),
                self.enter_entries.len(),
                self.exit_entries.len(),
                self.changes.len(),
            )
            || before_epoch
                != (
                    self.synced_sample_time,
                    self.last_seen_revision,
                    self.last_root,
                    self.last_generation,
                    result.as_ref().err().cloned(),
                )
            || self.identity.is_none()
        {
            self.rotate_identity();
        }
        self.admission_error = result.as_ref().err().cloned();
        result.unwrap_or_else(|error| AnimationSyncResult {
            completed: AnimationOverlayResult {
                preparation_error: Some(error),
                ..Default::default()
            },
        })
    }

    fn rotate_identity(&mut self) {
        self.identity = Some(RuntimeAuthority::default());
    }
    fn allocate_generation(&mut self) -> Result<RunGeneration, ProjectionError> {
        let next = self
            .last_generation
            .checked_add(1)
            .ok_or(ProjectionError::GenerationExhausted)?;
        self.last_generation = next;
        Ok(RunGeneration(next))
    }

    fn retained_group(
        &self,
        node: NodeId,
        kind: groups::Owner,
        started: Instant,
        tree: &ElementTree,
    ) -> Option<groups::GroupKey> {
        let generation = match kind {
            groups::Owner::Regular => self.animate_entries.get(&node).map(|e| e.generation),
            groups::Owner::Enter => self.enter_entries.get(&node).map(|e| e.generation),
            groups::Owner::Exit => self.exit_entries.get(&node).map(|e| e.generation),
            groups::Owner::Change(field) => self.changes.get(&(node, field)).map(|e| e.generation),
        };
        generation.zip(tree.get(&node)).and_then(|(generation, n)| {
            self.groups.group(groups::OwnerKey {
                node,
                mount: n.lifecycle.mounted_at_revision,
                kind,
                generation,
                started,
            })
        })
    }

    fn retains_geometry(
        &self,
        node: NodeId,
        kind: groups::Owner,
        started: Instant,
        tree: &ElementTree,
    ) -> bool {
        self.retained_group(node, kind, started, tree).is_some()
    }

    fn owner_phases(
        &self,
        owner: groups::OwnerKey,
        fields: fields::FieldMask,
        active: bool,
        now: Option<Instant>,
        blocked: fields::FieldMask,
    ) -> fields::PhaseMasks {
        let group = self.groups.group(owner);
        fields::PhaseMasks::new(
            fields,
            active,
            group.is_some(),
            group
                .and_then(|key| self.groups.barrier(key))
                .zip(now)
                .is_some_and(|(deadline, now)| deadline <= now),
            blocked,
        )
    }
    fn change_phases(
        &self,
        key: (NodeId, change::Field),
        entry: &change::ChangeEntry,
        active: bool,
        now: Option<Instant>,
    ) -> fields::PhaseMasks {
        let field = fields::FieldMask::field(key.1);
        self.owner_phases(
            groups::OwnerKey {
                node: key.0,
                mount: entry.mount,
                kind: groups::Owner::Change(key.1),
                generation: entry.generation,
                started: entry.started_at,
            },
            field,
            active,
            now,
            if entry.pending {
                field
            } else {
                fields::FieldMask::default()
            },
        )
    }

    fn enter_phases(
        &self,
        tree: &ElementTree,
        node: NodeId,
        entry: &EnterAnimationRuntimeEntry,
        now: Instant,
    ) -> fields::PhaseMasks {
        debug_assert!(
            tree.get(&node)
                .is_some_and(|node| node.lifecycle.mounted_at_revision == entry.mount)
        );
        self.owner_phases(
            groups::OwnerKey {
                node,
                mount: entry.mount,
                kind: groups::Owner::Enter,
                generation: entry.generation,
                started: entry.started_at,
            },
            entry.fields,
            entry_is_active(&entry.spec, entry.started_at, now),
            Some(now),
            fields::FieldMask::default(),
        )
    }

    fn completed_enter_ids(
        &self,
        tree: &ElementTree,
        now: Instant,
        releasing: &HashSet<groups::GroupKey>,
    ) -> Vec<NodeId> {
        self.enter_entries
            .iter()
            .filter_map(|(id, entry)| {
                let retained = self
                    .retained_group(*id, groups::Owner::Enter, entry.started_at, tree)
                    .is_some_and(|key| !releasing.contains(&key));
                (!tree.get(id).is_some_and(|node| node.is_live())
                    || (!entry_is_active(&entry.spec, entry.started_at, now) && !retained))
                    .then_some(*id)
            })
            .collect()
    }
    fn check_enter_handoff_capacity(
        &self,
        tree: &ElementTree,
        completed: &[NodeId],
    ) -> Result<(), ProjectionError> {
        let needed = completed
            .iter()
            .filter(|id| {
                tree.get(id).is_some_and(|node| {
                    node.is_live() && self.regular_handoff_needs_generation(node)
                })
            })
            .count();
        self.last_generation
            .checked_add(needed as u64)
            .ok_or(ProjectionError::GenerationExhausted)?;
        Ok(())
    }

    fn try_sync_with_tree(
        &mut self,
        tree: &ElementTree,
        started_at: Instant,
    ) -> Result<AnimationSyncResult, ProjectionError> {
        self.animate_entries.retain(|id, entry| {
            tree.get(id).is_some_and(|element| {
                element.is_live()
                    && element.spec.declared.animate.is_some()
                    && element.lifecycle.mounted_at_revision == entry.mount
            })
        });
        self.enter_entries.retain(|id, entry| {
            tree.get(id).is_some_and(|element| {
                element.is_live() && element.lifecycle.mounted_at_revision == entry.mount
            })
        });
        self.exit_entries.retain(|id, entry| {
            tree.get(id).is_some_and(|element| {
                element.is_ghost_root()
                    && element.lifecycle.ghost_exit_animation.is_some()
                    && element.lifecycle.mounted_at_revision == entry.mount
            })
        });

        let mut change_effects = self.sync_changes(tree, started_at)?;
        self.sync_groups(tree, started_at);
        self.handoff_completed_enter_paint(tree, started_at);
        for effect in self.finish_unheld_changes(tree, started_at) {
            change_effects.record_effect(effect);
        }
        let root = tree.root_id().and_then(|id| {
            tree.get(&id)
                .map(|node| (id, node.lifecycle.mounted_at_revision))
        });
        if self.synced && self.last_seen_revision == tree.revision() && self.last_root == root {
            let mut result = self.finish_active_enters(tree, started_at)?;
            for effect in change_effects.effects {
                result.completed.record_effect(effect);
            }
            self.sync_regular_handoffs(tree, started_at)?;
            self.sync_groups(tree, started_at);
            return Ok(result);
        }

        let mut sync_result = AnimationSyncResult::default();

        self.content_nodes.clear();
        for (id, element) in tree.iter_node_pairs() {
            if element.is_live() && content::has_policy(&element.spec.declared) {
                self.content_nodes.insert(id);
            }
            #[cfg(test)]
            {
                self.sync_node_visits += 1;
            }
            change::validate_policies(&element.spec.declared)
                .map_err(|_| ProjectionError::InvalidOwnership(id))?;
            for spec in [
                element.spec.declared.animate.as_ref(),
                element.spec.declared.animate_enter.as_ref(),
                element.spec.declared.animate_exit.as_ref(),
                element.lifecycle.ghost_exit_animation.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                validate_animation_lengths(spec)
                    .map_err(|_| ProjectionError::UnsupportedLength(id))?;
                if !timing::valid_duration(spec.duration_ms, &spec.repeat, started_at) {
                    return Err(ProjectionError::InvalidTiming(id));
                }
            }
            if element.is_ghost_root() {
                if let Some(spec) = element.lifecycle.ghost_exit_animation.as_ref()
                    && !self.exit_entries.contains_key(&id)
                {
                    let generation = self.allocate_generation()?;
                    let fields = fields::FieldMask::from_spec(spec);
                    let frame = element.layout.render_frame.or(element.layout.frame);
                    let preserve=[
                        (change::Field::Width,super::layout::dimensions::Axis::Width,element.spec.declared.width.as_ref(),frame.map(|frame|frame.width)),
                        (change::Field::Height,super::layout::dimensions::Axis::Height,element.spec.declared.height.as_ref(),frame.map(|frame|frame.height)),
                    ].map(|(field,axis,length,visible)| {
                        fields.intersects(fields::FieldMask::field(field)) && (
                            element.layout.dimension_samples.as_ref().and_then(|samples|samples.get(axis)).is_some()
                            || !matches!(length,Some(super::attrs::Length::Px(px)) if visible==Some(*px as f32))
                        )
                    });
                    let source = if preserve.into_iter().any(|preserve| preserve) {
                        source::PresentationSource::capture(tree, &id).map(|mut source| {
                            for (sampled, preserve) in source.sampled.iter_mut().zip(preserve) {
                                *sampled |= preserve;
                            }
                            Arc::new(source)
                        })
                    } else {
                        None
                    };
                    self.exit_entries.insert(
                        id,
                        ExitAnimationRuntimeEntry {
                            mount: element.lifecycle.mounted_at_revision,
                            source,
                            fields,
                            generation,
                            spec: Arc::new(spec.clone()),
                            started_at,
                            capture_scale: element.lifecycle.ghost_capture_scale.unwrap_or(1.0),
                            presentation_anchor_pending: true,
                        },
                    );
                }
                continue;
            }

            if !element.is_live() {
                continue;
            }

            if tree.was_mounted_after(&id, self.last_seen_revision)
                && self
                    .enter_entries
                    .get(&id)
                    .is_none_or(|entry| entry.mount != element.lifecycle.mounted_at_revision)
                && self
                    .animate_entries
                    .get(&id)
                    .is_none_or(|entry| entry.mount != element.lifecycle.mounted_at_revision)
            {
                self.animate_entries.remove(&id);

                if let Some(spec) = element.spec.declared.animate_enter.as_ref() {
                    let generation = self.allocate_generation()?;
                    self.enter_entries.insert(
                        id,
                        EnterAnimationRuntimeEntry {
                            mount: element.lifecycle.mounted_at_revision,
                            generation,
                            fields: fields::FieldMask::from_spec(spec),
                            spec: Arc::new(spec.clone()),
                            started_at,
                            presentation_anchor_pending: true,
                        },
                    );
                } else {
                    self.enter_entries.remove(&id);
                }
            }

            self.sync_regular_node(tree, id, started_at)?;
        }

        let completed = self.finish_active_enters(tree, started_at)?;
        for effect in completed.completed.effects {
            sync_result.completed.record_effect(effect);
        }
        for effect in change_effects.effects {
            sync_result.completed.record_effect(effect);
        }
        self.sync_groups(tree, started_at);
        self.last_seen_revision = tree.revision();
        self.synced = true;
        self.last_root = root;
        Ok(sync_result)
    }

    fn finish_active_enters(
        &mut self,
        tree: &ElementTree,
        now: Instant,
    ) -> Result<AnimationSyncResult, ProjectionError> {
        let completed = self.completed_enter_ids(tree, now, &HashSet::new());
        self.check_enter_handoff_capacity(tree, &completed)?;
        completed
            .into_iter()
            .try_fold(AnimationSyncResult::default(), |mut result, id| {
                if let Some(entry) = self.enter_entries.remove(&id)
                    && let Some(node) = tree.get(&id).filter(|node| node.is_live())
                {
                    result.record_completed_enter(id, &entry.spec);
                    self.handoff_changes(tree, id, &entry.spec, now);
                    let _ = node;
                    self.sync_regular_node_from(tree, id, now, Some(&entry.spec))?;
                }
                Ok(result)
            })
    }

    pub(super) fn preflight_commit(&self, tree: &ElementTree) -> Result<(), ProjectionError> {
        if let Some(error) = &self.admission_error {
            return Err(error.clone());
        }
        if let Some(now) = self.synced_sample_time {
            let ready = self.groups.ready(now);
            self.check_enter_handoff_capacity(tree, &self.completed_enter_ids(tree, now, &ready))?;
        }
        Ok(())
    }
    /// Commit only after a successfully prepared live layout. Enter handoffs get
    /// a follow-up frame, so terminal-symbol release and intentional base/regular
    /// handoff are distinct publications, with the terminal footprint as source.
    pub(crate) fn finish_prepared_frame(
        &mut self,
        tree: &mut ElementTree,
    ) -> Result<bool, ProjectionError> {
        if let Some(error) = &self.admission_error {
            return Err(error.clone());
        }
        if let Some(token) = tree.applied_animation_frame.0.as_ref() {
            token.validate(tree)?;
            token.validate_owner(Some(self))?;
        } else {
            return if tree
                .length_runtime
                .as_ref()
                .is_some_and(|s| s.release().is_some())
            {
                Err(ProjectionError::StalePreparation)
            } else {
                Ok(false)
            };
        }
        let Some(state) = tree.length_runtime.as_ref() else {
            self.commit_admissions();
            tree.finish_patch_frame();
            return Ok(self.commit_ghost_cleanup(tree));
        };
        state.validate_delta()?;
        let Some(receipt) = state.release() else {
            if let Some(state) = tree.length_runtime.as_mut() {
                state.commit_delta();
            }
            if tree
                .length_runtime
                .as_ref()
                .is_some_and(|state| state.is_empty())
                && self.groups.is_empty()
            {
                tree.clear_length_runtime();
            }
            self.commit_admissions();
            tree.finish_patch_frame();
            return Ok(self.commit_ghost_cleanup(tree));
        };
        state.validate_release(tree, self)?;
        let completed = self.completed_enter_ids(tree, receipt.sample_time, &receipt.groups);
        self.check_enter_handoff_capacity(tree, &completed)?;
        if let Some(state) = tree.length_runtime.as_mut() {
            state.commit_delta();
        }
        let released = tree
            .length_runtime
            .as_mut()
            .and_then(|state| state.take_release());
        let Some(released) = released else {
            self.commit_admissions();
            tree.finish_patch_frame();
            return Ok(self.commit_ghost_cleanup(tree));
        };
        tree.finish_patch_frame();
        let now = released.sample_time;
        self.groups.commit(&released.groups);
        let _ = self.finish_unheld_changes(tree, now);
        let handoffs = self.finish_active_enters(tree, now)?;
        tree.pending_patch_effects
            .invalidation
            .add(handoffs.completed.invalidation);
        for effect in &handoffs.completed.effects {
            tree.mark_layout_dirty_for_invalidation(&effect.id, effect.invalidation);
        }
        self.sync_groups(tree, now);
        if let Some(state) = tree.length_runtime.as_mut() {
            state.finish_release(&released);
        }
        if tree
            .length_runtime
            .as_ref()
            .is_some_and(|state| state.is_empty())
            && self.groups.is_empty()
        {
            tree.clear_length_runtime();
        }
        self.commit_admissions();
        Ok(self.commit_ghost_cleanup(tree) || !handoffs.completed.effects.is_empty())
    }

    fn commit_ghost_cleanup(&mut self, tree: &mut ElementTree) -> bool {
        self.compact_regular_sources();
        self.commit_admissions();
        let ghosts = tree
            .applied_animation_frame
            .0
            .take()
            .map(|token| token.ghosts)
            .unwrap_or_default();
        if self.retire_ghosts(tree, ghosts) {
            tree.pending_patch_effects
                .invalidation
                .add(TreeInvalidation::Structure);
            self.commit_admissions();
            true
        } else {
            false
        }
    }

    pub(crate) fn has_pending_admissions(&self) -> bool {
        self.animate_entries.has_staged()
            || self.enter_entries.has_staged()
            || self.exit_entries.has_staged()
            || self.changes.has_staged()
            || self.groups.has_staged()
    }
    fn commit_admissions(&mut self) {
        self.animate_entries.commit();
        self.enter_entries.commit();
        self.exit_entries.commit();
        self.changes.commit();
        self.groups.commit_admissions();
    }

    pub fn is_empty(&self) -> bool {
        self.animate_entries.is_empty()
            && self.enter_entries.is_empty()
            && self.exit_entries.is_empty()
            && self.changes.is_empty()
    }

    pub fn anchor_pending_transient_entries_to_present(&mut self, presented_at: Instant) {
        let enters: Vec<_> = self
            .enter_entries
            .iter()
            .filter_map(|(id, e)| e.presentation_anchor_pending.then_some(*id))
            .collect();
        let exits: Vec<_> = self
            .exit_entries
            .iter()
            .filter_map(|(id, e)| e.presentation_anchor_pending.then_some(*id))
            .collect();
        let anchor = |started: &mut Instant, pending: &mut bool| {
            let changed = *started != presented_at;
            *started = presented_at;
            *pending = false;
            changed
        };
        // Non-short-circuit OR: every pending entry must be anchored.
        let enter_changed = enters.into_iter().fold(false, |changed, id| {
            self.enter_entries.get_mut(&id).is_some_and(|entry| {
                anchor(
                    &mut entry.started_at,
                    &mut entry.presentation_anchor_pending,
                )
            }) | changed
        });
        let exit_changed = exits.into_iter().fold(false, |changed, id| {
            self.exit_entries.get_mut(&id).is_some_and(|entry| {
                anchor(
                    &mut entry.started_at,
                    &mut entry.presentation_anchor_pending,
                )
            }) | changed
        });
        if (enter_changed || exit_changed) && self.identity.is_some() {
            self.rotate_identity();
        }
    }

    pub fn has_transient_entries(&self) -> bool {
        !self.enter_entries.is_empty() || !self.exit_entries.is_empty()
    }

    pub fn active_node_ids(&self) -> Vec<NodeId> {
        let mut seen = HashSet::new();
        self.animate_entries
            .keys()
            .chain(self.enter_entries.keys())
            .chain(self.exit_entries.keys())
            .chain(self.changes.keys().map(|(id, _)| id))
            .copied()
            .filter(|id| seen.insert(*id))
            .collect()
    }

    pub fn animate_entry(&self, id: &NodeId) -> Option<&AnimationRuntimeEntry> {
        self.animate_entries
            .get(id)
            .filter(|entry| !entry.fields.is_empty())
            .map(|entry| &entry.clock)
    }

    pub fn enter_entry(&self, id: &NodeId) -> Option<&EnterAnimationRuntimeEntry> {
        self.enter_entries.get(id)
    }

    pub fn exit_entry(&self, id: &NodeId) -> Option<&ExitAnimationRuntimeEntry> {
        self.exit_entries.get(id)
    }

    fn ghost_retirements(&self, tree: &ElementTree) -> Vec<groups::OwnerKey> {
        let Some(now) = self.synced_sample_time else {
            return Vec::new();
        };
        self.exit_entries
            .iter()
            .filter_map(|(id, entry)| {
                let group = self.retained_group(*id, groups::Owner::Exit, entry.started_at, tree);
                (!entry_is_active(&entry.spec, entry.started_at, now)
                    && group.is_none_or(|key| {
                        self.groups
                            .barrier(key)
                            .is_some_and(|deadline| deadline <= now)
                    }))
                .then_some(groups::OwnerKey {
                    node: *id,
                    mount: entry.mount,
                    kind: groups::Owner::Exit,
                    generation: entry.generation,
                    started: entry.started_at,
                })
            })
            .collect()
    }
    fn retire_ghosts(&mut self, tree: &mut ElementTree, ghosts: Vec<groups::OwnerKey>) -> bool {
        if ghosts.is_empty() {
            return false;
        }
        self.rotate_identity();
        for ghost in ghosts {
            assert!(
                self.exit_entries
                    .get(&ghost.node)
                    .is_some_and(|entry| entry.mount == ghost.mount
                        && entry.generation == ghost.generation
                        && entry.started_at == ghost.started)
                    && tree
                        .get(&ghost.node)
                        .is_some_and(|node| node.is_ghost_root()
                            && node.lifecycle.mounted_at_revision == ghost.mount),
                "preflighted ghost identity under exclusive access"
            );
            self.exit_entries.remove(&ghost.node);
            crate::tree::patch::remove_subtree(tree, &ghost.node);
        }
        true
    }
    #[cfg(test)]
    fn prune_completed_exit_ghosts(
        &mut self,
        tree: &mut ElementTree,
        sample_time: Option<Instant>,
    ) -> bool {
        self.synced_sample_time = sample_time;
        self.retire_ghosts(tree, self.ghost_retirements(tree))
    }
}

fn entry_is_active(spec: &AnimationSpec, started_at: Instant, now: Instant) -> bool {
    let entry = AnimationRuntimeEntry {
        spec_hash: 0,
        started_at,
    };
    timing::position(spec, Some(&entry), Some(now)).is_some_and(|p| p.active)
}

pub fn spec_fingerprint(spec: &AnimationSpec) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{spec:?}").hash(&mut hasher);
    hasher.finish()
}

pub fn scale_animation_spec(spec: &AnimationSpec, scale: f64) -> AnimationSpec {
    AnimationSpec {
        keyframes: spec
            .keyframes
            .iter()
            .map(|keyframe| scale_animation_keyframe(keyframe, scale))
            .collect(),
        duration_ms: spec.duration_ms,
        curve: spec.curve.clone(),
        repeat: spec.repeat.clone(),
    }
}

/// Exit ghosts start from the current rendered attrs so interrupted enter or
/// interaction animations do not jump back to the declared exit keyframe.
pub fn retarget_exit_animation_spec_to_current_visual(
    mut spec: AnimationSpec,
    current: &Attrs,
) -> AnimationSpec {
    if let Some(first) = spec.keyframes.first_mut() {
        retarget_exit_keyframe_to_current_visual(first, current);
    }

    spec
}

fn retarget_exit_keyframe_to_current_visual(first: &mut Attrs, current: &Attrs) {
    macro_rules! retarget_clone {
        ($field:ident) => {
            if first.$field.is_some() {
                if let Some(value) = current.$field.clone() {
                    first.$field = Some(value);
                }
            }
        };
    }

    macro_rules! retarget_copy {
        ($field:ident) => {
            if first.$field.is_some() {
                if let Some(value) = current.$field {
                    first.$field = Some(value);
                }
            }
        };
    }

    retarget_clone!(width);
    retarget_clone!(height);
    retarget_clone!(padding);
    retarget_copy!(spacing);
    retarget_copy!(spacing_x);
    retarget_copy!(spacing_y);
    retarget_copy!(align_x);
    retarget_copy!(align_y);
    retarget_clone!(background);
    retarget_clone!(border_radius);
    retarget_clone!(border_width);
    retarget_clone!(border_color);
    retarget_clone!(box_shadows);
    retarget_copy!(font_size);
    retarget_clone!(font_color);
    retarget_copy!(font_letter_spacing);
    retarget_copy!(font_word_spacing);
    retarget_clone!(svg_color);
    retarget_copy!(layout_scale);
    retarget_copy!(layout_rotate);
    retarget_copy!(move_x);
    retarget_copy!(move_y);
    retarget_copy!(rotate);
    retarget_copy!(scale);
    retarget_copy!(alpha);
}

pub(crate) fn sample_animation_overlays(
    tree: &ElementTree,
    runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> AnimationFrameSamples {
    if let Some(error) = runtime.and_then(|runtime| runtime.admission_error.clone()) {
        return AnimationFrameSamples {
            result: AnimationOverlayResult {
                preparation_error: Some(error),
                ..Default::default()
            },
            ..Default::default()
        };
    }

    tree.iter_node_pairs().fold(
        AnimationFrameSamples::default(),
        |mut frame_samples, (id, element)| {
            if let Some(sample) = sample_animation_for_element(element, runtime, sample_time) {
                frame_samples.result.record_sample(id, &sample);
                frame_samples.samples.insert(id, sample);
            }

            frame_samples
        },
    )
}

pub(crate) fn sample_animation_overlays_for_ids(
    tree: &ElementTree,
    runtime: &AnimationRuntime,
    ids: &[NodeId],
    sample_time: Option<Instant>,
) -> AnimationFrameSamples {
    if let Some(error) = runtime.admission_error.clone() {
        return AnimationFrameSamples {
            result: AnimationOverlayResult {
                preparation_error: Some(error),
                ..Default::default()
            },
            ..Default::default()
        };
    }

    ids.iter()
        .fold(AnimationFrameSamples::default(), |mut frame_samples, id| {
            if let Some(element) = tree.get(id)
                && let Some(sample) =
                    sample_animation_for_element(element, Some(runtime), sample_time)
            {
                frame_samples.result.record_sample(*id, &sample);
                frame_samples.samples.insert(*id, sample);
            }

            frame_samples
        })
}

fn sample_animation_for_element(
    element: &super::element::Element,
    runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
) -> Option<AnimationSample> {
    let Some(runtime) = runtime else {
        return element
            .spec
            .declared
            .animate
            .as_ref()
            .map(|spec| sample_animation_spec(spec, None, sample_time));
    };
    selected_runs(element, Some(runtime), sample_time, false)
        .into_iter()
        .fold(None, |result, run| {
            let clock = AnimationRuntimeEntry {
                spec_hash: run.revision,
                started_at: run.started,
            };
            let mut sample = sample_animation_spec(run.spec, Some(&clock), sample_time);
            let phases = if let groups::Owner::Change(field) = run.owner {
                runtime.change_phases(
                    (element.id, field),
                    runtime
                        .changes
                        .get(&(element.id, field))
                        .expect("selected change"),
                    sample.active,
                    sample_time,
                )
            } else {
                runtime.owner_phases(
                    run.owner_key(element),
                    run.fields,
                    sample.active,
                    sample_time,
                    fields::FieldMask::default(),
                )
            };
            let mask = if matches!(run.owner, groups::Owner::Regular | groups::Owner::Exit) {
                run.fields
            } else {
                phases.writes()
            };
            if mask.is_empty() {
                return result;
            }
            sample.attrs = mask.select(&sample.attrs);
            let mut result = result.unwrap_or_default();
            apply_sample_attrs(&mut result.attrs, &sample.attrs);
            result.active |= sample.active;
            Some(result)
        })
}

fn runs_for_element<'a>(
    element: &'a super::element::Element,
    runtime: Option<&'a AnimationRuntime>,
    now: Option<Instant>,
) -> Vec<lengths::RunRef<'a>> {
    selected_runs(element, runtime, now, false)
}
fn runs_for_element_including_finished<'a>(
    element: &'a super::element::Element,
    runtime: Option<&'a AnimationRuntime>,
    now: Option<Instant>,
) -> Vec<lengths::RunRef<'a>> {
    selected_runs(element, runtime, now, true)
}
fn selected_runs<'a>(
    element: &'a super::element::Element,
    runtime: Option<&'a AnimationRuntime>,
    now: Option<Instant>,
    include_finished: bool,
) -> Vec<lengths::RunRef<'a>> {
    let Some(runtime) = runtime else {
        return Vec::new();
    };
    let now = now.unwrap_or_else(Instant::now);
    let primary = if let Some(entry) = runtime.exit_entry(&element.id) {
        Some(lengths::RunRef {
            fields: entry.fields,
            spec: &entry.spec,
            started: entry.started_at,
            owner: groups::Owner::Exit,
            revision: 0,
            generation: entry.generation,
            source: entry.source.as_deref(),
        })
    } else {
        runtime
            .enter_entry(&element.id)
            .filter(|e| {
                include_finished
                    || entry_is_active(&e.spec, e.started_at, now)
                    || runtime
                        .groups
                        .group(groups::OwnerKey {
                            node: element.id,
                            mount: element.lifecycle.mounted_at_revision,
                            kind: groups::Owner::Enter,
                            started: e.started_at,
                            generation: e.generation,
                        })
                        .is_some()
            })
            .map(|entry| lengths::RunRef {
                fields: entry.fields,
                spec: &entry.spec,
                started: entry.started_at,
                owner: groups::Owner::Enter,
                revision: 0,
                generation: entry.generation,
                source: None,
            })
    };
    primary
        .into_iter()
        .chain(runtime.regular_runs(element, now))
        .chain(change::Field::ALL.into_iter().filter_map(|field| {
            runtime
                .changes
                .get(&(element.id, field))
                .filter(|e| !e.pending)
                .map(|e| lengths::RunRef {
                    fields: fields::FieldMask::field(field),
                    spec: &e.spec,
                    started: e.started_at,
                    owner: groups::Owner::Change(field),
                    revision: 0,
                    generation: e.generation,
                    source: Some(&e.source),
                })
        }))
        .collect()
}

#[cfg(test)]
pub fn apply_animation_overlays(
    tree: &mut ElementTree,
    runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
    scale: f32,
) -> AnimationOverlayResult {
    tree.iter_nodes_mut()
        .fold(AnimationOverlayResult::default(), |mut result, element| {
            apply_animation_overlay_to_element(&mut result, element, runtime, sample_time, scale);
            result
        })
}

#[cfg(test)]
pub fn apply_animation_overlays_to_active(
    tree: &mut ElementTree,
    runtime: &AnimationRuntime,
    sample_time: Option<Instant>,
    scale: f32,
) -> AnimationOverlayResult {
    runtime.active_node_ids().into_iter().fold(
        AnimationOverlayResult::default(),
        |mut result, id| {
            if let Some(element) = tree.get_mut(&id) {
                apply_animation_overlay_to_element(
                    &mut result,
                    element,
                    Some(runtime),
                    sample_time,
                    scale,
                );
            }
            result
        },
    )
}

#[cfg(test)]
fn apply_animation_overlay_to_element(
    result: &mut AnimationOverlayResult,
    element: &mut super::element::Element,
    runtime: Option<&AnimationRuntime>,
    sample_time: Option<Instant>,
    scale: f32,
) {
    if let Some(sample) = runtime
        .and_then(|state| state.exit_entry(&element.id))
        .map(|entry| sample_exit_animation_spec(entry, sample_time, scale))
        .filter(|sample| sample.active)
    {
        apply_sample_attrs(&mut element.layout.effective, &sample.attrs);
        result.record_sample(element.id, &sample);
        return;
    }

    if let Some(sample) = runtime
        .and_then(|state| state.enter_entry(&element.id))
        .map(|entry| sample_enter_animation_spec(entry, sample_time, scale as f64))
        .filter(|sample| sample.active)
    {
        apply_sample_attrs(&mut element.layout.effective, &sample.attrs);
        result.record_sample(element.id, &sample);
        return;
    }

    let Some(spec) = element.layout.effective.animate.as_ref() else {
        return;
    };

    let sample = sample_animation_spec(
        spec,
        runtime.and_then(|state| state.animate_entry(&element.id)),
        sample_time,
    );
    apply_sample_attrs(&mut element.layout.effective, &sample.attrs);
    result.record_sample(element.id, &sample);
}

fn animation_attrs_layout_effect(id: NodeId, attrs: &Attrs) -> Option<AnimationLayoutEffect> {
    let invalidation = classify_animation_sample_attrs(attrs);
    invalidation.is_dirty().then_some(AnimationLayoutEffect {
        id,
        invalidation,
        registry_refresh: animation_attrs_affect_registry_refresh(attrs),
        layout_scale_dirty: attrs.layout_scale.is_some(),
        transform_only: animation_attrs_are_transform_only(attrs),
    })
}

fn animation_spec_layout_effect(id: NodeId, spec: &AnimationSpec) -> Option<AnimationLayoutEffect> {
    let invalidation =
        spec.keyframes
            .iter()
            .fold(TreeInvalidation::None, |mut invalidation, attrs| {
                invalidation.add(classify_animation_sample_attrs(attrs));
                invalidation
            });

    invalidation.is_dirty().then(|| AnimationLayoutEffect {
        id,
        invalidation,
        registry_refresh: spec
            .keyframes
            .iter()
            .any(animation_attrs_affect_registry_refresh),
        layout_scale_dirty: spec
            .keyframes
            .iter()
            .any(|attrs| attrs.layout_scale.is_some()),
        transform_only: spec
            .keyframes
            .iter()
            .all(animation_attrs_are_transform_only),
    })
}

pub(super) fn animation_spec_is_compositor_only(spec: &AnimationSpec) -> bool {
    !spec.keyframes.is_empty()
        && spec
            .keyframes
            .iter()
            .all(animation_attrs_are_transform_only)
}

fn animation_attrs_are_transform_only(attrs: &Attrs) -> bool {
    classify_animation_sample_attrs(attrs) == TreeInvalidation::Paint
        && attrs.background.is_none()
        && attrs.border_radius.is_none()
        && attrs.border_style.is_none()
        && attrs.border_color.is_none()
        && attrs.box_shadows.is_none()
        && attrs.font_color.is_none()
        && attrs.svg_color.is_none()
        && attrs.font_underline.is_none()
        && attrs.font_strike.is_none()
        && attrs.video_target.is_none()
}

pub fn classify_animation_sample_attrs(attrs: &Attrs) -> TreeInvalidation {
    let mut invalidation = TreeInvalidation::None;

    if attrs.background.is_some()
        || attrs.border_radius.is_some()
        || attrs.border_style.is_some()
        || attrs.border_color.is_some()
        || attrs.box_shadows.is_some()
        || attrs.font_color.is_some()
        || attrs.svg_color.is_some()
        || attrs.font_underline.is_some()
        || attrs.font_strike.is_some()
        || attrs.video_target.is_some()
        || attrs.move_x.is_some()
        || attrs.move_y.is_some()
        || attrs.rotate.is_some()
        || attrs.scale.is_some()
        || attrs.alpha.is_some()
    {
        invalidation.add(TreeInvalidation::Paint);
    }

    if attrs.align_x.is_some() || attrs.align_y.is_some() {
        invalidation.add(TreeInvalidation::Resolve);
    }

    if attrs.width.is_some()
        || attrs.height.is_some()
        || attrs.layout_scale.is_some()
        || attrs.layout_rotate.is_some()
        || attrs.padding.is_some()
        || attrs.spacing.is_some()
        || attrs.spacing_x.is_some()
        || attrs.spacing_y.is_some()
        || attrs.scrollbar_y.is_some()
        || attrs.scrollbar_x.is_some()
        || attrs.ghost_scrollbar_y.is_some()
        || attrs.ghost_scrollbar_x.is_some()
        || attrs.scroll_x.is_some()
        || attrs.scroll_y.is_some()
        || attrs.clip_nearby.is_some()
        || attrs.border_width.is_some()
        || attrs.font_size.is_some()
        || attrs.font.is_some()
        || attrs.font_weight.is_some()
        || attrs.font_style.is_some()
        || attrs.font_letter_spacing.is_some()
        || attrs.font_word_spacing.is_some()
        || attrs.image_src.is_some()
        || attrs.image_fit.is_some()
        || attrs.image_size.is_some()
        || attrs.text_align.is_some()
        || attrs.content.is_some()
        || attrs.snap_layout.is_some()
        || attrs.snap_text_metrics.is_some()
        || attrs.space_evenly.is_some()
    {
        invalidation.add(TreeInvalidation::Measure);
    }

    invalidation
}

#[cfg(test)]
fn sample_enter_animation_spec(
    entry: &EnterAnimationRuntimeEntry,
    sample_time: Option<Instant>,
    scale: f64,
) -> AnimationSample {
    let scaled_spec = scale_animation_spec(&entry.spec, scale);
    let runtime_entry = AnimationRuntimeEntry {
        spec_hash: 0,
        started_at: entry.started_at,
    };

    sample_animation_spec(&scaled_spec, Some(&runtime_entry), sample_time)
}

#[cfg(test)]
fn sample_exit_animation_spec(
    entry: &ExitAnimationRuntimeEntry,
    sample_time: Option<Instant>,
    current_scale: f32,
) -> AnimationSample {
    let scaled_spec = scale_animation_spec(
        &entry.spec,
        (current_scale / entry.capture_scale.max(f32::EPSILON)) as f64,
    );
    let runtime_entry = AnimationRuntimeEntry {
        spec_hash: 0,
        started_at: entry.started_at,
    };

    sample_animation_spec(&scaled_spec, Some(&runtime_entry), sample_time)
}

pub fn sample_animation_spec(
    spec: &AnimationSpec,
    entry: Option<&AnimationRuntimeEntry>,
    sample_time: Option<Instant>,
) -> AnimationSample {
    let Some(position) = timing::position(spec, entry, sample_time) else {
        return AnimationSample::default();
    };
    let attrs = if !position.active {
        spec.keyframes.last().cloned().unwrap_or_default()
    } else {
        interpolate_attrs(
            &spec.keyframes[position.index],
            &spec.keyframes[position.index + 1],
            position.eased,
        )
    };
    AnimationSample {
        attrs,
        active: position.active,
    }
}

fn scale_animation_keyframe(attrs: &Attrs, scale: f64) -> Attrs {
    Attrs {
        width: attrs.width.as_ref().map(|value| scale_length(value, scale)),
        height: attrs
            .height
            .as_ref()
            .map(|value| scale_length(value, scale)),
        padding: attrs
            .padding
            .as_ref()
            .map(|value| scale_padding(value, scale)),
        spacing: attrs.spacing.map(|value| value * scale),
        spacing_x: attrs.spacing_x.map(|value| value * scale),
        spacing_y: attrs.spacing_y.map(|value| value * scale),
        align_x: attrs.align_x,
        align_y: attrs.align_y,
        background: attrs.background.clone(),
        border_radius: attrs
            .border_radius
            .as_ref()
            .map(|value| scale_border_radius(value, scale)),
        border_width: attrs
            .border_width
            .as_ref()
            .map(|value| scale_border_width(value, scale)),
        border_color: attrs.border_color.clone(),
        box_shadows: attrs.box_shadows.as_ref().map(|shadows| {
            shadows
                .iter()
                .map(|shadow| BoxShadow {
                    offset_x: shadow.offset_x * scale,
                    offset_y: shadow.offset_y * scale,
                    blur: shadow.blur * scale,
                    size: shadow.size * scale,
                    color: shadow.color.clone(),
                    inset: shadow.inset,
                })
                .collect()
        }),
        font_size: attrs.font_size.map(|value| value * scale),
        font_color: attrs.font_color.clone(),
        font_letter_spacing: attrs.font_letter_spacing.map(|value| value * scale),
        font_word_spacing: attrs.font_word_spacing.map(|value| value * scale),
        svg_color: attrs.svg_color.clone(),
        layout_scale: attrs.layout_scale,
        layout_rotate: attrs.layout_rotate,
        move_x: attrs.move_x.map(|value| value * scale),
        move_y: attrs.move_y.map(|value| value * scale),
        rotate: attrs.rotate,
        scale: attrs.scale,
        alpha: attrs.alpha,
        ..Attrs::default()
    }
}

fn interpolate_attrs(from: &Attrs, to: &Attrs, t: f64) -> Attrs {
    Attrs {
        width: interpolate_opt_ref(
            from.width.as_ref(),
            to.width.as_ref(),
            t,
            interpolate_length,
        ),
        height: interpolate_opt_ref(
            from.height.as_ref(),
            to.height.as_ref(),
            t,
            interpolate_length,
        ),
        padding: interpolate_opt_ref(
            from.padding.as_ref(),
            to.padding.as_ref(),
            t,
            interpolate_padding,
        ),
        spacing: interpolate_opt_copy(from.spacing, to.spacing, t, lerp_f64),
        spacing_x: interpolate_opt_copy(from.spacing_x, to.spacing_x, t, lerp_f64),
        spacing_y: interpolate_opt_copy(from.spacing_y, to.spacing_y, t, lerp_f64),
        align_x: interpolate_opt_copy(from.align_x, to.align_x, t, interpolate_discrete),
        align_y: interpolate_opt_copy(from.align_y, to.align_y, t, interpolate_discrete),
        background: interpolate_opt_ref(
            from.background.as_ref(),
            to.background.as_ref(),
            t,
            interpolate_background,
        ),
        border_radius: interpolate_opt_ref(
            from.border_radius.as_ref(),
            to.border_radius.as_ref(),
            t,
            interpolate_border_radius,
        ),
        border_width: interpolate_opt_ref(
            from.border_width.as_ref(),
            to.border_width.as_ref(),
            t,
            interpolate_border_width,
        ),
        border_color: interpolate_opt_ref(
            from.border_color.as_ref(),
            to.border_color.as_ref(),
            t,
            interpolate_color,
        ),
        box_shadows: interpolate_opt_ref(
            from.box_shadows.as_ref(),
            to.box_shadows.as_ref(),
            t,
            |from, to, t| interpolate_box_shadows(from, to, t),
        ),
        font_size: interpolate_opt_copy(from.font_size, to.font_size, t, lerp_f64),
        font_color: interpolate_opt_ref(
            from.font_color.as_ref(),
            to.font_color.as_ref(),
            t,
            interpolate_color,
        ),
        font_letter_spacing: interpolate_opt_copy(
            from.font_letter_spacing,
            to.font_letter_spacing,
            t,
            lerp_f64,
        ),
        font_word_spacing: interpolate_opt_copy(
            from.font_word_spacing,
            to.font_word_spacing,
            t,
            lerp_f64,
        ),
        svg_color: interpolate_opt_ref(
            from.svg_color.as_ref(),
            to.svg_color.as_ref(),
            t,
            interpolate_color,
        ),
        layout_scale: interpolate_opt_copy(from.layout_scale, to.layout_scale, t, lerp_f64),
        layout_rotate: interpolate_opt_copy(from.layout_rotate, to.layout_rotate, t, lerp_f64),
        move_x: interpolate_opt_copy(from.move_x, to.move_x, t, lerp_f64),
        move_y: interpolate_opt_copy(from.move_y, to.move_y, t, lerp_f64),
        rotate: interpolate_opt_copy(from.rotate, to.rotate, t, lerp_f64),
        scale: interpolate_opt_copy(from.scale, to.scale, t, lerp_f64),
        alpha: interpolate_opt_copy(from.alpha, to.alpha, t, lerp_f64),
        ..Attrs::default()
    }
}

pub(crate) fn apply_sample_attrs(attrs: &mut Attrs, sample: &Attrs) {
    if let Some(value) = sample.width.clone() {
        attrs.width = Some(value);
    }
    if let Some(value) = sample.height.clone() {
        attrs.height = Some(value);
    }
    if let Some(value) = sample.padding.clone() {
        attrs.padding = Some(value);
    }
    if let Some(value) = sample.spacing {
        attrs.spacing = Some(value);
    }
    if let Some(value) = sample.spacing_x {
        attrs.spacing_x = Some(value);
    }
    if let Some(value) = sample.spacing_y {
        attrs.spacing_y = Some(value);
    }
    if let Some(value) = sample.align_x {
        attrs.align_x = Some(value);
    }
    if let Some(value) = sample.align_y {
        attrs.align_y = Some(value);
    }
    if let Some(value) = sample.background.clone() {
        attrs.background = Some(value);
    }
    if let Some(value) = sample.border_radius.clone() {
        attrs.border_radius = Some(value);
    }
    if let Some(value) = sample.border_width.clone() {
        attrs.border_width = Some(value);
    }
    if let Some(value) = sample.border_color.clone() {
        attrs.border_color = Some(value);
    }
    if let Some(value) = sample.box_shadows.clone() {
        attrs.box_shadows = Some(value);
    }
    if let Some(value) = sample.font_size {
        attrs.font_size = Some(value);
    }
    if let Some(value) = sample.font_color.clone() {
        attrs.font_color = Some(value);
    }
    if let Some(value) = sample.font_letter_spacing {
        attrs.font_letter_spacing = Some(value);
    }
    if let Some(value) = sample.font_word_spacing {
        attrs.font_word_spacing = Some(value);
    }
    if let Some(value) = sample.svg_color.clone() {
        attrs.svg_color = Some(value);
    }
    if let Some(value) = sample.layout_scale {
        attrs.layout_scale = Some(value);
    }
    if let Some(value) = sample.layout_rotate {
        attrs.layout_rotate = Some(value);
    }
    if let Some(value) = sample.move_x {
        attrs.move_x = Some(value);
    }
    if let Some(value) = sample.move_y {
        attrs.move_y = Some(value);
    }
    if let Some(value) = sample.rotate {
        attrs.rotate = Some(value);
    }
    if let Some(value) = sample.scale {
        attrs.scale = Some(value);
    }
    if let Some(value) = sample.alpha {
        attrs.alpha = Some(value);
    }
}

fn interpolate_opt_copy<T: Copy, U, F>(
    from: Option<T>,
    to: Option<T>,
    t: f64,
    interpolate: F,
) -> Option<U>
where
    F: Fn(T, T, f64) -> U,
{
    match (from, to) {
        (Some(from), Some(to)) => Some(interpolate(from, to, t)),
        _ => None,
    }
}

fn interpolate_opt_ref<T, U, F>(
    from: Option<&T>,
    to: Option<&T>,
    t: f64,
    interpolate: F,
) -> Option<U>
where
    F: Fn(&T, &T, f64) -> U,
{
    match (from, to) {
        (Some(from), Some(to)) => Some(interpolate(from, to, t)),
        _ => None,
    }
}

fn interpolate_discrete<T: Copy>(from: T, to: T, t: f64) -> T {
    if t < 0.5 { from } else { to }
}

fn lerp_f64(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

pub(crate) fn validate_animated_length(length: &Length) -> Result<(), String> {
    if matches!(length, Length::Min(..) | Length::Max(..)) {
        Err(
            "cannot animate min/max length expressions; use pixels, content, fill or weighted fill"
                .into(),
        )
    } else {
        Ok(())
    }
}
pub(crate) fn validate_animation_lengths(spec: &AnimationSpec) -> Result<(), String> {
    spec.keyframes
        .iter()
        .flat_map(|frame| frame.width.iter().chain(frame.height.iter()))
        .try_for_each(validate_animated_length)
}

fn interpolate_length(from: &Length, to: &Length, t: f64) -> Length {
    match (from, to) {
        (Length::Fill, Length::Fill) => Length::Fill,
        (Length::Content, Length::Content) => Length::Content,
        (Length::Px(from), Length::Px(to)) => Length::Px(lerp_f64(*from, *to, t)),
        (Length::FillWeighted(from), Length::FillWeighted(to)) => {
            Length::FillWeighted(lerp_f64(*from, *to, t))
        }
        _ => from.clone(),
    }
}

fn interpolate_padding(from: &Padding, to: &Padding, t: f64) -> Padding {
    match (from, to) {
        (Padding::Uniform(from), Padding::Uniform(to)) => Padding::Uniform(lerp_f64(*from, *to, t)),
        (
            Padding::Sides {
                top: from_top,
                right: from_right,
                bottom: from_bottom,
                left: from_left,
            },
            Padding::Sides {
                top: to_top,
                right: to_right,
                bottom: to_bottom,
                left: to_left,
            },
        ) => Padding::Sides {
            top: lerp_f64(*from_top, *to_top, t),
            right: lerp_f64(*from_right, *to_right, t),
            bottom: lerp_f64(*from_bottom, *to_bottom, t),
            left: lerp_f64(*from_left, *to_left, t),
        },
        _ => from.clone(),
    }
}

fn interpolate_border_radius(from: &BorderRadius, to: &BorderRadius, t: f64) -> BorderRadius {
    match (from, to) {
        (BorderRadius::Uniform(from), BorderRadius::Uniform(to)) => {
            BorderRadius::Uniform(lerp_f64(*from, *to, t))
        }
        (
            BorderRadius::Corners {
                tl: from_tl,
                tr: from_tr,
                br: from_br,
                bl: from_bl,
            },
            BorderRadius::Corners {
                tl: to_tl,
                tr: to_tr,
                br: to_br,
                bl: to_bl,
            },
        ) => BorderRadius::Corners {
            tl: lerp_f64(*from_tl, *to_tl, t),
            tr: lerp_f64(*from_tr, *to_tr, t),
            br: lerp_f64(*from_br, *to_br, t),
            bl: lerp_f64(*from_bl, *to_bl, t),
        },
        _ => from.clone(),
    }
}

fn interpolate_border_width(from: &BorderWidth, to: &BorderWidth, t: f64) -> BorderWidth {
    match (from, to) {
        (BorderWidth::Uniform(from), BorderWidth::Uniform(to)) => {
            BorderWidth::Uniform(lerp_f64(*from, *to, t))
        }
        (
            BorderWidth::Sides {
                top: from_top,
                right: from_right,
                bottom: from_bottom,
                left: from_left,
            },
            BorderWidth::Sides {
                top: to_top,
                right: to_right,
                bottom: to_bottom,
                left: to_left,
            },
        ) => BorderWidth::Sides {
            top: lerp_f64(*from_top, *to_top, t),
            right: lerp_f64(*from_right, *to_right, t),
            bottom: lerp_f64(*from_bottom, *to_bottom, t),
            left: lerp_f64(*from_left, *to_left, t),
        },
        _ => from.clone(),
    }
}

fn interpolate_background(from: &Background, to: &Background, t: f64) -> Background {
    match (from, to) {
        (Background::Color(from), Background::Color(to)) => {
            Background::Color(interpolate_color(from, to, t))
        }
        (
            Background::Image {
                source: from_source,
                fit: from_fit,
            },
            Background::Image {
                source: to_source,
                fit: to_fit,
            },
        ) if from_source == to_source && from_fit == to_fit => from.clone(),
        _ => from.clone(),
    }
}

fn interpolate_box_shadows(from: &[BoxShadow], to: &[BoxShadow], t: f64) -> Vec<BoxShadow> {
    from.iter()
        .zip(to.iter())
        .map(|(from, to)| BoxShadow {
            offset_x: lerp_f64(from.offset_x, to.offset_x, t),
            offset_y: lerp_f64(from.offset_y, to.offset_y, t),
            blur: lerp_f64(from.blur, to.blur, t),
            size: lerp_f64(from.size, to.size, t),
            color: interpolate_color(&from.color, &to.color, t),
            inset: from.inset,
        })
        .collect()
}

fn interpolate_color(from: &Color, to: &Color, t: f64) -> Color {
    use crate::tree::attrs::SolidColor;
    match (from, to) {
        (
            Color::Gradient {
                colors: a,
                angle: aa,
            },
            Color::Gradient {
                colors: b,
                angle: ba,
            },
        ) => {
            if a.len() != b.len() {
                return if t >= 1.0 { to.clone() } else { from.clone() };
            }
            Color::Gradient {
                colors: a
                    .iter()
                    .zip(b.iter())
                    .map(|(a, b)| interpolate_stop(a, b, t))
                    .collect(),
                angle: (1.0 - t) * aa + t * ba,
            }
        }
        (Color::Gradient { colors, angle }, solid) => {
            let Ok(solid) = SolidColor::try_from(solid.clone()) else {
                return from.clone();
            };
            Color::Gradient {
                colors: colors
                    .iter()
                    .map(|c| interpolate_stop(c, &solid, t))
                    .collect(),
                angle: *angle,
            }
        }
        (solid, Color::Gradient { colors, angle }) => {
            let Ok(solid) = SolidColor::try_from(solid.clone()) else {
                return from.clone();
            };
            Color::Gradient {
                colors: colors
                    .iter()
                    .map(|c| interpolate_stop(&solid, c, t))
                    .collect(),
                angle: *angle,
            }
        }
        _ => {
            match (
                SolidColor::try_from(from.clone()),
                SolidColor::try_from(to.clone()),
            ) {
                (Ok(a), Ok(b)) => interpolate_stop(&a, &b, t).into(),
                _ => from.clone(),
            }
        }
    }
}

fn interpolate_stop(
    from: &crate::tree::attrs::SolidColor,
    to: &crate::tree::attrs::SolidColor,
    t: f64,
) -> crate::tree::attrs::SolidColor {
    let from = color_to_rgba(from);
    let to = color_to_rgba(to);
    crate::tree::attrs::SolidColor::Rgba {
        r: lerp_channel(from.0, to.0, t),
        g: lerp_channel(from.1, to.1, t),
        b: lerp_channel(from.2, to.2, t),
        a: lerp_channel(from.3, to.3, t),
    }
}

fn color_to_rgba(color: &crate::tree::attrs::SolidColor) -> (u8, u8, u8, u8) {
    use crate::tree::attrs::SolidColor;
    match color {
        SolidColor::Rgb { r, g, b } => (*r, *g, *b, 255),
        SolidColor::Rgba { r, g, b, a } => (*r, *g, *b, *a),
        SolidColor::Named(n) => named_color_rgba(n),
    }
}

fn named_color_rgba(name: &str) -> (u8, u8, u8, u8) {
    match name {
        "white" => (255, 255, 255, 255),
        "black" => (0, 0, 0, 255),
        "red" => (255, 0, 0, 255),
        "green" => (0, 255, 0, 255),
        "blue" => (0, 0, 255, 255),
        "cyan" => (0, 255, 255, 255),
        "magenta" => (255, 0, 255, 255),
        "yellow" => (255, 255, 0, 255),
        "orange" => (255, 165, 0, 255),
        "purple" => (128, 0, 128, 255),
        "pink" => (255, 192, 203, 255),
        "gray" | "grey" => (128, 128, 128, 255),
        "navy" => (0, 0, 128, 255),
        "teal" => (0, 128, 128, 255),
        _ => (255, 255, 255, 255),
    }
}

fn lerp_channel(from: u8, to: u8, t: f64) -> u8 {
    lerp_f64(from as f64, to as f64, t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn scale_length(value: &Length, scale: f64) -> Length {
    match value {
        Length::Fill => Length::Fill,
        Length::Content => Length::Content,
        Length::Px(value) => Length::Px(value * scale),
        Length::FillWeighted(value) => Length::FillWeighted(*value),
        Length::Min(left, right) => Length::Min(
            Box::new(scale_length(left, scale)),
            Box::new(scale_length(right, scale)),
        ),
        Length::Max(left, right) => Length::Max(
            Box::new(scale_length(left, scale)),
            Box::new(scale_length(right, scale)),
        ),
    }
}

fn scale_padding(value: &Padding, scale: f64) -> Padding {
    match value {
        Padding::Uniform(value) => Padding::Uniform(value * scale),
        Padding::Sides {
            top,
            right,
            bottom,
            left,
        } => Padding::Sides {
            top: top * scale,
            right: right * scale,
            bottom: bottom * scale,
            left: left * scale,
        },
    }
}

fn scale_border_radius(value: &BorderRadius, scale: f64) -> BorderRadius {
    match value {
        BorderRadius::Uniform(value) => BorderRadius::Uniform(value * scale),
        BorderRadius::Corners { tl, tr, br, bl } => BorderRadius::Corners {
            tl: tl * scale,
            tr: tr * scale,
            br: br * scale,
            bl: bl * scale,
        },
    }
}

fn scale_border_width(value: &BorderWidth, scale: f64) -> BorderWidth {
    match value {
        BorderWidth::Uniform(value) => BorderWidth::Uniform(value * scale),
        BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        } => BorderWidth::Sides {
            top: top * scale,
            right: right * scale,
            bottom: bottom * scale,
            left: left * scale,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::element::{Element, ElementKind, GhostAttachment, NodeResidency};

    fn move_x_spec(
        from_x: f64,
        to_x: f64,
        duration_ms: f64,
        repeat: AnimationRepeat,
    ) -> AnimationSpec {
        let from = Attrs {
            move_x: Some(from_x),
            ..Attrs::default()
        };

        let to = Attrs {
            move_x: Some(to_x),
            ..Attrs::default()
        };

        AnimationSpec {
            keyframes: vec![from, to],
            duration_ms,
            curve: AnimationCurve::Linear,
            repeat,
        }
    }

    fn alpha_spec(from_alpha: f64, to_alpha: f64, duration_ms: f64) -> AnimationSpec {
        let from = Attrs {
            alpha: Some(from_alpha),
            ..Attrs::default()
        };

        let to = Attrs {
            alpha: Some(to_alpha),
            ..Attrs::default()
        };

        AnimationSpec {
            keyframes: vec![from, to],
            duration_ms,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        }
    }

    fn tree_with_element(
        attrs: Attrs,
        tree_revision: u64,
        mounted_at_revision: u64,
    ) -> (ElementTree, NodeId) {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut element = Element::with_attrs(id, ElementKind::El, Vec::new(), attrs);
        element.lifecycle.mounted_at_revision = mounted_at_revision;

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);
        tree.set_revision(tree_revision);
        (tree, id)
    }

    fn tree_with_exit_ghost() -> (ElementTree, NodeId, NodeId) {
        let root_id = NodeId::from_term_bytes(vec![10]);
        let ghost_id = NodeId::from_term_bytes(vec![11]);

        let mut root = Element::with_attrs(root_id, ElementKind::El, Vec::new(), Attrs::default());
        root.children = vec![ghost_id];

        let ghost_attrs = Attrs {
            alpha: Some(1.0),
            ..Attrs::default()
        };
        let mut ghost =
            Element::with_attrs(ghost_id, ElementKind::El, Vec::new(), ghost_attrs.clone());
        ghost.spec.declared = ghost_attrs;
        ghost.lifecycle.residency = NodeResidency::Ghost;
        ghost.lifecycle.ghost_attachment = Some(GhostAttachment::Child {
            parent_id: root_id,
            live_index: 0,
            seq: 0,
        });
        ghost.lifecycle.ghost_capture_scale = Some(1.0);
        ghost.lifecycle.ghost_exit_animation = Some(alpha_spec(1.0, 0.0, 100.0));

        let mut tree = ElementTree::new();
        tree.insert(root);
        tree.insert(ghost);
        tree.set_root_id(root_id);
        (tree, root_id, ghost_id)
    }

    #[test]
    fn sample_animation_spec_loops_with_time() {
        let from = Attrs {
            move_x: Some(0.0),
            ..Attrs::default()
        };

        let to = Attrs {
            move_x: Some(10.0),
            ..Attrs::default()
        };

        let spec = AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 100.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Loop,
        };
        let start = Instant::now();
        let entry = AnimationRuntimeEntry {
            spec_hash: spec_fingerprint(&spec),
            started_at: start,
        };

        let sample = sample_animation_spec(
            &spec,
            Some(&entry),
            Some(start + std::time::Duration::from_millis(150)),
        );

        assert_eq!(sample.attrs.move_x, Some(5.0));
        assert!(sample.active);
    }

    #[test]
    fn sample_animation_spec_clamps_once_to_last_keyframe() {
        let from = Attrs {
            alpha: Some(0.0),
            ..Attrs::default()
        };

        let to = Attrs {
            alpha: Some(1.0),
            ..Attrs::default()
        };

        let spec = AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 100.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        };
        let start = Instant::now();
        let entry = AnimationRuntimeEntry {
            spec_hash: spec_fingerprint(&spec),
            started_at: start,
        };

        let sample = sample_animation_spec(
            &spec,
            Some(&entry),
            Some(start + std::time::Duration::from_millis(250)),
        );

        assert_eq!(sample.attrs.alpha, Some(1.0));
        assert!(!sample.active);
    }

    #[test]
    fn sample_animation_spec_interpolates_layout_scale_and_rotate() {
        let from = Attrs {
            layout_scale: Some(1.0),
            layout_rotate: Some(0.0),
            ..Attrs::default()
        };

        let to = Attrs {
            layout_scale: Some(2.0),
            layout_rotate: Some(90.0),
            ..Attrs::default()
        };

        let spec = AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 100.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        };
        let start = Instant::now();
        let entry = AnimationRuntimeEntry {
            spec_hash: spec_fingerprint(&spec),
            started_at: start,
        };

        let sample = sample_animation_spec(
            &spec,
            Some(&entry),
            Some(start + std::time::Duration::from_millis(50)),
        );

        assert_eq!(sample.attrs.layout_scale, Some(1.5));
        assert_eq!(sample.attrs.layout_rotate, Some(45.0));
        assert_eq!(
            classify_animation_sample_attrs(&sample.attrs),
            TreeInvalidation::Measure
        );
    }

    #[test]
    fn scale_animation_spec_preserves_layout_transform_fields() {
        let from = Attrs {
            layout_scale: Some(1.25),
            layout_rotate: Some(15.0),
            width: Some(Length::Px(20.0)),
            ..Attrs::default()
        };

        let to = Attrs {
            layout_scale: Some(1.5),
            layout_rotate: Some(45.0),
            width: Some(Length::Px(40.0)),
            ..Attrs::default()
        };

        let scaled = scale_animation_spec(
            &AnimationSpec {
                keyframes: vec![from, to],
                duration_ms: 100.0,
                curve: AnimationCurve::Linear,
                repeat: AnimationRepeat::Once,
            },
            2.0,
        );

        assert_eq!(scaled.keyframes[0].layout_scale, Some(1.25));
        assert_eq!(scaled.keyframes[1].layout_scale, Some(1.5));
        assert_eq!(scaled.keyframes[0].layout_rotate, Some(15.0));
        assert_eq!(scaled.keyframes[1].layout_rotate, Some(45.0));
        assert_eq!(scaled.keyframes[0].width, Some(Length::Px(40.0)));
        assert_eq!(scaled.keyframes[1].width, Some(Length::Px(80.0)));
    }

    #[test]
    fn sync_with_tree_starts_enter_animation_for_newly_mounted_nodes() {
        let attrs = Attrs {
            animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
            ..Attrs::default()
        };
        let (tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        assert!(runtime.enter_entry(&id).is_some());
        assert!(
            runtime
                .enter_entry(&id)
                .expect("enter entry should exist")
                .presentation_anchor_pending
        );
        assert!(runtime.animate_entry(&id).is_none());
        assert_eq!(runtime.last_seen_revision, 1);
    }

    #[test]
    fn transient_enter_animation_anchors_to_first_presented_frame_once() {
        let attrs = Attrs {
            animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
            ..Attrs::default()
        };
        let (tree, id) = tree_with_element(attrs, 1, 1);
        let patch_time = Instant::now();
        let first_presented = patch_time + std::time::Duration::from_millis(24);
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, patch_time);
        runtime.anchor_pending_transient_entries_to_present(first_presented);
        runtime.anchor_pending_transient_entries_to_present(
            first_presented + std::time::Duration::from_millis(16),
        );

        let entry = runtime
            .enter_entry(&id)
            .expect("enter entry should stay active");
        assert_eq!(entry.started_at, first_presented);
        assert!(!entry.presentation_anchor_pending);

        let sample = sample_enter_animation_spec(
            entry,
            Some(first_presented + std::time::Duration::from_millis(50)),
            1.0,
        );
        assert_eq!(sample.attrs.alpha, Some(0.5));
    }

    #[test]
    fn presentation_anchoring_visits_every_pending_enter_and_exit() {
        let started_at = Instant::now();
        let presented_at = started_at + std::time::Duration::from_millis(24);
        let (enter_tree, enter_id) = tree_with_element(
            Attrs {
                animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
                ..Attrs::default()
            },
            1,
            1,
        );
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&enter_tree, started_at);
        let enter = runtime.enter_entry(&enter_id).unwrap().clone();
        let (exit_tree, _, exit_id) = tree_with_exit_ghost();
        let mut exits = AnimationRuntime::default();
        exits.sync_with_tree(&exit_tree, started_at);
        let exit = exits.exit_entry(&exit_id).unwrap().clone();
        for id in 100..104 {
            runtime.enter_entries.insert(NodeId(id), enter.clone());
            runtime.exit_entries.insert(NodeId(id + 100), exit.clone());
        }
        // Clearing pending is still required when the timestamp already matches.
        runtime
            .enter_entries
            .get_mut(&NodeId(100))
            .unwrap()
            .started_at = presented_at;
        runtime
            .exit_entries
            .get_mut(&NodeId(200))
            .unwrap()
            .started_at = presented_at;
        runtime.anchor_pending_transient_entries_to_present(presented_at);
        runtime.anchor_pending_transient_entries_to_present(
            presented_at + std::time::Duration::from_millis(16),
        );
        assert_eq!(runtime.enter_entries.len(), 5);
        assert_eq!(runtime.exit_entries.len(), 4);
        for (_, entry) in runtime.enter_entries.iter() {
            assert_eq!(entry.started_at, presented_at);
            assert!(!entry.presentation_anchor_pending);
        }
        for (_, entry) in runtime.exit_entries.iter() {
            assert_eq!(entry.started_at, presented_at);
            assert!(!entry.presentation_anchor_pending);
        }
    }

    #[test]
    fn sync_with_tree_does_not_start_enter_when_attr_is_added_later() {
        let (mut tree, id) = tree_with_element(Attrs::default(), 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        tree.set_revision(2);
        let element = tree.get_mut(&id).expect("element should exist");
        element.spec.declared.animate_enter = Some(alpha_spec(0.0, 1.0, 100.0));
        element.layout.effective.animate_enter = element.spec.declared.animate_enter.clone();

        runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(16));

        assert!(runtime.enter_entry(&id).is_none());
    }

    #[test]
    fn enter_animation_captures_spec_at_mount_time() {
        let attrs = Attrs {
            animate_enter: Some(move_x_spec(0.0, 100.0, 100.0, AnimationRepeat::Once)),
            ..Attrs::default()
        };
        let (mut tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        let element = tree.get_mut(&id).expect("element should exist");
        element.spec.declared.animate_enter =
            Some(move_x_spec(0.0, 200.0, 100.0, AnimationRepeat::Once));
        element.layout.effective.animate_enter = element.spec.declared.animate_enter.clone();

        let sample = sample_enter_animation_spec(
            runtime.enter_entry(&id).expect("enter entry should exist"),
            Some(start + std::time::Duration::from_millis(50)),
            1.0,
        );

        assert_eq!(sample.attrs.move_x, Some(50.0));
    }

    #[test]
    fn completed_enter_hands_off_to_base_attrs_when_no_animate_is_present() {
        let attrs = Attrs {
            animate_enter: Some(move_x_spec(0.0, 100.0, 100.0, AnimationRepeat::Once)),
            ..Attrs::default()
        };
        let (mut tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);
        let sync = runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(150));

        assert_eq!(sync.completed.invalidation, TreeInvalidation::Paint);
        assert_eq!(sync.completed.effects.len(), 1);
        assert_eq!(sync.completed.effects[0].id, id);
        assert!(runtime.enter_entry(&id).is_none());
        assert!(runtime.animate_entry(&id).is_none());

        let result = apply_animation_overlays(
            &mut tree,
            Some(&runtime),
            Some(start + std::time::Duration::from_millis(150)),
            1.0,
        );

        assert!(!result.active);
        assert_eq!(tree.get(&id).unwrap().layout.effective.move_x, None);
    }

    #[test]
    fn completed_enter_starts_regular_animation_from_zero_progress() {
        let attrs = Attrs {
            animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
            animate: Some(move_x_spec(10.0, 30.0, 100.0, AnimationRepeat::Loop)),
            ..Attrs::default()
        };
        let (mut tree, id) = tree_with_element(attrs, 1, 1);
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);
        runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(150));

        assert!(runtime.enter_entry(&id).is_none());
        assert!(runtime.animate_entry(&id).is_some());

        let result = apply_animation_overlays(
            &mut tree,
            Some(&runtime),
            Some(start + std::time::Duration::from_millis(150)),
            1.0,
        );

        assert!(result.active);
        assert_eq!(tree.get(&id).unwrap().layout.effective.move_x, Some(10.0));
    }

    #[test]
    fn sync_with_tree_starts_exit_runtime_for_ghost_roots() {
        let (tree, _root_id, ghost_id) = tree_with_exit_ghost();
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);

        assert!(runtime.exit_entry(&ghost_id).is_some());
        assert!(
            runtime
                .exit_entry(&ghost_id)
                .expect("exit entry should exist")
                .presentation_anchor_pending
        );
    }

    #[test]
    fn transient_exit_animation_anchors_to_first_presented_frame_once() {
        let (tree, _root_id, ghost_id) = tree_with_exit_ghost();
        let patch_time = Instant::now();
        let first_presented = patch_time + std::time::Duration::from_millis(24);
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, patch_time);
        runtime.anchor_pending_transient_entries_to_present(first_presented);
        runtime.anchor_pending_transient_entries_to_present(
            first_presented + std::time::Duration::from_millis(16),
        );

        let entry = runtime
            .exit_entry(&ghost_id)
            .expect("exit entry should stay active");
        assert_eq!(entry.started_at, first_presented);
        assert!(!entry.presentation_anchor_pending);

        let sample = sample_exit_animation_spec(
            entry,
            Some(first_presented + std::time::Duration::from_millis(50)),
            1.0,
        );
        assert_eq!(sample.attrs.alpha, Some(0.5));
    }

    #[test]
    fn rejected_publication_cannot_prune_an_expired_ghost_and_cleanup_drains() {
        use crate::tree::layout::{
            Constraint, layout_and_refresh_default_with_animation, prepare_frame_attrs_for_update,
        };
        let (mut tree, _, ghost) = tree_with_exit_ghost();
        let mut runtime = AnimationRuntime::default();
        let start = Instant::now();
        let constraint = Constraint::new(600.0, 600.0);
        layout_and_refresh_default_with_animation(&mut tree, constraint, 1.0, &mut runtime, start)
            .unwrap();
        let before = tree.get(&ghost).unwrap().layout.frame;
        let generation = runtime.exit_entries.committed(&ghost).unwrap().generation;
        let now = start + std::time::Duration::from_millis(100);
        runtime.sync_with_tree(&tree, now);
        let mut rejected =
            prepare_frame_attrs_for_update(&mut tree, 1.0, Some(&mut runtime), Some(now));
        rejected.animation_result.preparation_error = Some(ProjectionError::InvalidContext);
        assert!(
            rejected
                .publish(&mut tree, &mut runtime, constraint, true, None)
                .is_err()
        );
        assert_eq!(before, tree.get(&ghost).unwrap().layout.frame);
        assert_eq!(
            generation,
            runtime.exit_entries.committed(&ghost).unwrap().generation
        );
        let terminal = layout_and_refresh_default_with_animation(
            &mut tree,
            constraint,
            1.0,
            &mut runtime,
            now,
        )
        .unwrap();
        assert!(
            terminal.animations_active,
            "cleanup needs a final publication"
        );
        assert!(tree.get(&ghost).is_none());
        assert!(runtime.exit_entries.is_empty());
        assert_eq!(
            tree.pending_patch_effects.invalidation,
            TreeInvalidation::Structure
        );
        let cleanup = layout_and_refresh_default_with_animation(
            &mut tree,
            constraint,
            1.0,
            &mut runtime,
            now,
        )
        .unwrap();
        assert!(!cleanup.animations_active);
        assert!(tree.pending_patch_effects.invalidation.is_none());
        assert!(tree.pending_patch_effects.sources.is_empty());
    }

    #[test]
    fn failed_admission_retry_preserves_already_admitted_enter_clock_and_spec() {
        let attrs = Attrs {
            animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
            ..Default::default()
        };
        let (mut tree, first) = tree_with_element(attrs.clone(), 1, 1);
        let second = NodeId(9000);
        let mut node = Element::with_attrs(second, ElementKind::El, vec![], attrs);
        node.lifecycle.mounted_at_revision = 1;
        tree.insert(node);
        tree.set_children(&first, vec![second]).unwrap();
        let start = Instant::now();
        let mut runtime = AnimationRuntime {
            last_generation: u64::MAX - 1,
            ..Default::default()
        };
        assert_eq!(
            runtime
                .sync_with_tree(&tree, start)
                .completed
                .preparation_error,
            Some(ProjectionError::GenerationExhausted)
        );
        let admitted = runtime.enter_entries.get(&first).unwrap().clone();
        assert_eq!(
            runtime
                .sync_with_tree(&tree, start + std::time::Duration::from_millis(10))
                .completed
                .preparation_error,
            Some(ProjectionError::GenerationExhausted)
        );
        let retry = runtime.enter_entries.get(&first).unwrap();
        assert_eq!(admitted.generation, retry.generation);
        assert_eq!(admitted.started_at, retry.started_at);
        assert!(Arc::ptr_eq(&admitted.spec, &retry.spec));
        assert!(runtime.enter_entries.committed(&first).is_none());
    }

    #[test]
    fn remounted_enter_and_exit_ids_never_keep_the_old_mount_clock() {
        for exiting in [false, true] {
            let (mut tree, node) = if exiting {
                let (tree, _, ghost) = tree_with_exit_ghost();
                (tree, ghost)
            } else {
                tree_with_element(
                    Attrs {
                        animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
                        ..Default::default()
                    },
                    1,
                    1,
                )
            };
            let start = Instant::now();
            let mut runtime = AnimationRuntime::default();
            runtime.sync_with_tree(&tree, start);
            runtime.commit_admissions();
            let generation = if exiting {
                runtime.exit_entries.get(&node).unwrap().generation
            } else {
                runtime.enter_entries.get(&node).unwrap().generation
            };
            tree.get_mut(&node).unwrap().lifecycle.mounted_at_revision = 2;
            tree.set_revision(2);
            let now = start + std::time::Duration::from_millis(25);
            assert!(
                runtime
                    .sync_with_tree(&tree, now)
                    .completed
                    .preparation_error
                    .is_none()
            );
            if exiting {
                let next = runtime.exit_entries.get(&node).unwrap();
                assert_ne!(next.generation, generation);
                assert_eq!(next.started_at, now);
                assert_eq!(next.mount, 2);
            } else {
                let next = runtime.enter_entries.get(&node).unwrap();
                assert_ne!(next.generation, generation);
                assert_eq!(next.started_at, now);
                assert_eq!(next.mount, 2);
            }
        }
    }

    #[test]
    fn prune_completed_exit_ghosts_removes_finished_ghost_subtree() {
        let (mut tree, root_id, ghost_id) = tree_with_exit_ghost();
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();

        runtime.sync_with_tree(&tree, start);
        assert!(!runtime.prune_completed_exit_ghosts(
            &mut tree,
            Some(start + std::time::Duration::from_millis(50))
        ));
        assert!(tree.get(&ghost_id).is_some());

        assert!(runtime.prune_completed_exit_ghosts(
            &mut tree,
            Some(start + std::time::Duration::from_millis(150))
        ));
        assert!(tree.get(&ghost_id).is_none());
        assert!(runtime.exit_entry(&ghost_id).is_none());
        assert!(tree.get(&root_id).unwrap().children.is_empty());
    }

    #[test]
    fn gradient_interpolation_keeps_all_stops_and_rejects_native_count_mismatch() {
        let from = Background::Color(crate::tree::attrs::Color::Gradient {
            colors: (vec![
                Color::Named("black".into()),
                Color::Rgba {
                    r: 20,
                    g: 40,
                    b: 60,
                    a: 0,
                },
                Color::Named("red".into()),
            ])
            .into_iter()
            .map(|c| c.try_into().expect("solid stop"))
            .collect(),
            angle: -90.0,
        });
        let to = Background::Color(crate::tree::attrs::Color::Gradient {
            colors: (vec![
                Color::Named("white".into()),
                Color::Rgba {
                    r: 40,
                    g: 80,
                    b: 120,
                    a: 200,
                },
                Color::Named("blue".into()),
            ])
            .into_iter()
            .map(|c| c.try_into().expect("solid stop"))
            .collect(),
            angle: 450.0,
        });
        assert_eq!(
            interpolate_background(&from, &to, 0.5),
            Background::Color(crate::tree::attrs::Color::Gradient {
                colors: ([
                    Color::Rgba {
                        r: 128,
                        g: 128,
                        b: 128,
                        a: 255
                    },
                    Color::Rgba {
                        r: 30,
                        g: 60,
                        b: 90,
                        a: 100
                    },
                    Color::Rgba {
                        r: 128,
                        g: 0,
                        b: 128,
                        a: 255
                    }
                ])
                .into_iter()
                .map(|c| c.try_into().expect("solid stop"))
                .collect(),
                angle: 180.0
            })
        );
        for (t, source) in [(0.0, &from), (1.0, &to)] {
            let Background::Color(Color::Gradient { colors, angle }) =
                interpolate_background(&from, &to, t)
            else {
                panic!("expected gradient")
            };
            let Background::Color(Color::Gradient {
                colors: expected,
                angle: expected_angle,
            }) = source
            else {
                unreachable!()
            };
            assert_eq!(angle, *expected_angle);
            assert_eq!(
                colors.iter().map(color_to_rgba).collect::<Vec<_>>(),
                expected.iter().map(color_to_rgba).collect::<Vec<_>>()
            );
        }
        let mismatch = Background::Color(crate::tree::attrs::Color::Gradient {
            colors: (vec![Color::Named("black".into()); 2])
                .into_iter()
                .map(|c| c.try_into().expect("solid stop"))
                .collect(),
            angle: 0.0,
        });
        assert_eq!(interpolate_background(&from, &mismatch, 0.5), from);
        let huge = |angle| {
            Background::Color(crate::tree::attrs::Color::Gradient {
                colors: (vec![Color::Named("black".into()); 2])
                    .into_iter()
                    .map(|c| c.try_into().expect("solid stop"))
                    .collect(),
                angle,
            })
        };
        let Background::Color(Color::Gradient { angle, .. }) =
            interpolate_background(&huge(-f64::MAX), &huge(f64::MAX), 0.5)
        else {
            unreachable!()
        };
        assert_eq!(angle, 0.0);
    }
    #[test]
    fn universal_color_animation_lifts_solids_and_interpolates_every_shadow() {
        use crate::tree::attrs::SolidColor;
        let solid = Color::Named("black".into());
        let gradient = Color::Gradient {
            colors: [
                SolidColor::Named("red".into()),
                SolidColor::Rgba {
                    r: 0,
                    g: 255,
                    b: 0,
                    a: 128,
                },
                SolidColor::Rgba {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 0,
                },
            ]
            .into(),
            angle: 90.0,
        };
        let bounds = crate::tree::geometry::Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        for (from, to) in [(&solid, &gradient), (&gradient, &solid)] {
            assert_eq!(
                interpolate_color(from, to, 0.0).render(bounds),
                from.render(bounds)
            );
            assert_eq!(
                interpolate_color(from, to, 1.0).render(bounds),
                to.render(bounds)
            );
            let Color::Gradient { colors, angle } = interpolate_color(from, to, 0.5) else {
                panic!("gradient")
            };
            assert_eq!(angle, 90.0);
            assert_eq!(
                colors.iter().map(color_to_rgba).collect::<Vec<_>>(),
                [(128, 0, 0, 255), (0, 128, 0, 192), (0, 0, 128, 128)]
            );
        }
        let shadow = |color| BoxShadow {
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 4.0,
            size: 2.0,
            inset: false,
            color,
        };
        let a = [shadow(solid.clone()), shadow(gradient.clone())];
        let b = [shadow(gradient.clone()), shadow(solid.clone())];
        let result = interpolate_box_shadows(&a, &b, 0.5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].color, result[1].color);
        let mismatch = Color::Gradient {
            colors: [SolidColor::Named("white".into()); 1].into(),
            angle: 0.0,
        };
        assert_eq!(interpolate_color(&gradient, &mismatch, 0.5), gradient);
        assert_eq!(interpolate_color(&gradient, &mismatch, 1.0), mismatch);
    }
    #[test]
    fn transient_pulses_visit_owners_not_the_static_model() {
        let (mut tree, id) = tree_with_element(
            Attrs {
                animate_enter: Some(alpha_spec(0.0, 1.0, 100.0)),
                animate: Some(alpha_spec(0.5, 1.0, 200.0)),
                ..Attrs::default()
            },
            1,
            1,
        );
        for n in 100..1100 {
            tree.insert(Element::with_attrs(
                NodeId::from_wire_u64(n),
                ElementKind::El,
                Vec::new(),
                Attrs::default(),
            ));
        }
        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, start);
        assert_eq!(runtime.sync_node_visits, 1001);
        for ms in [10, 20, 70, 99] {
            runtime.sync_with_tree(&tree, start + std::time::Duration::from_millis(ms));
            assert!(runtime.enter_entry(&id).is_some());
        }
        let end = start + std::time::Duration::from_millis(100);
        let handoff = runtime.sync_with_tree(&tree, end);
        assert!(handoff.completed.invalidation.is_dirty());
        assert!(runtime.enter_entry(&id).is_none());
        assert_eq!(runtime.animate_entry(&id).unwrap().started_at, end);
        assert_eq!(runtime.sync_node_visits, 1001);
        tree.bump_revision();
        runtime.sync_with_tree(&tree, end);
        assert_eq!(runtime.sync_node_visits, 2002);
    }

    #[test]
    fn empty_runtime_does_not_rescan_an_unchanged_model() {
        let (mut tree, _) = tree_with_element(Attrs::default(), 1, 1);
        let mut runtime = AnimationRuntime::default();
        let now = Instant::now();
        for _ in 0..10 {
            runtime.sync_with_tree(&tree, now);
        }
        assert_eq!(runtime.sync_node_visits, 1);
        // A newly mounted root is an O(1) identity change, even for direct callers.
        tree.stamp_all_mounted_at_revision(2);
        runtime.sync_with_tree(&tree, now);
        assert_eq!(runtime.sync_node_visits, 2);
    }
}
