//! Shared mixed-length sampling on the ordinary layout evaluator.
#[cfg(test)]
pub(crate) mod inspection;
use super::*;
use crate::tree::layout::{
    FontContext, TextMeasurer,
    dimensions::{Axis, AxisFootprint},
    projection::{
        EndpointResolver, NodeProjection, Projection, ProjectionError, QueryContext, QuerySeed,
    },
};
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ProjectionScope {
    Group(groups::GroupKey),
    Owner(groups::OwnerKey),
    Release,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct RequestKey {
    scope: ProjectionScope,
    source_at: Option<Instant>,
}
type BoundaryQuery = (Arc<Projection>, Vec<(NodeId, Axis)>);

#[derive(Clone, Copy)]
pub(super) struct RunRef<'a> {
    pub fields: super::fields::FieldMask,
    pub spec: &'a AnimationSpec,
    pub started: Instant,
    pub owner: groups::Owner,
    pub revision: u64,
    pub generation: RunGeneration,
    pub source: Option<&'a source::PresentationSource>,
}
impl RunRef<'_> {
    pub(super) fn owner_key(self, node: &crate::tree::element::Element) -> groups::OwnerKey {
        groups::OwnerKey {
            node: node.id,
            mount: node.lifecycle.mounted_at_revision,
            kind: self.owner,
            generation: self.generation,
            started: self.started,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct RunKey {
    owner: groups::Owner,
    revision: u64,
    generation: RunGeneration,
    started: Instant,
    mount: u64,
    segment: usize,
    cycle: u64,
}
type ClockInputs = HashMap<(NodeId, Axis), Arc<ClockInput>>;
#[derive(Clone, Debug)]
struct Track {
    // ClockInput has no forecast field: query evidence cannot retain its history.
    input: Arc<ClockInput>,
    forecast: Option<Arc<ClockInputs>>,
}
impl std::ops::Deref for Track {
    type Target = ClockInput;
    fn deref(&self) -> &Self::Target {
        &self.input
    }
}
impl std::ops::DerefMut for Track {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.input)
    }
}
#[derive(Clone, Debug)]
struct ClockInput {
    evidence: Arc<crate::tree::layout::projection::NativeGoalReceipt>,
    key: RunKey,
    from: AxisFootprint,
    to: AxisFootprint,
    anchor: f64,
    projection: Arc<Projection>,
    context: Arc<QueryContext>,
    model: u64,
    evaluated_at: Instant,
    end_ms: f64,
    hold_interval: Option<(Instant, Instant)>,
}
impl ClockInput {
    fn same_run(&self, run: RunRef<'_>, mount: u64) -> bool {
        self.key.owner == run.owner
            && self.key.started == run.started
            && self.key.revision == run.revision
            && self.key.generation == run.generation
            && self.key.mount == mount
    }
    fn sample(
        &self,
        run: RunRef<'_>,
        p: timing::SegmentPosition,
        now: Instant,
    ) -> Result<AxisFootprint, ProjectionError> {
        let progress = if let Some((start, end)) = self.hold_interval {
            let duration = end.saturating_duration_since(start).as_secs_f64();
            if duration == 0.0 {
                1.0
            } else {
                (now.saturating_duration_since(start).as_secs_f64() / duration).clamp(0.0, 1.0)
            }
        } else {
            timing::remaining_progress(&run.spec.curve, self.anchor, p.progress)
        };
        self.from
            .interpolate(self.to, progress as f32)
            .map_err(ProjectionError::InvalidSample)
    }
}
#[derive(Debug, Default)]
struct GeometryIdentity;

#[derive(Clone, Debug, PartialEq)]
struct FrameStamp {
    root: Option<(NodeId, u64)>,
    revision: u64,
    model: u64,
    constraint: Option<crate::tree::layout::Constraint>,
}
impl FrameStamp {
    fn capture(tree: &ElementTree) -> Self {
        Self {
            root: tree.root_id().and_then(|id| {
                tree.get(&id)
                    .map(|node| (id, node.lifecycle.mounted_at_revision))
            }),
            revision: tree.revision(),
            model: tree.layout_model_epoch,
            constraint: tree.animation_constraint,
        }
    }
}

#[derive(Debug)]
struct PreparedField {
    key: RunKey,
    terminal: AxisFootprint,
}

/// Affine receipt: immutable versions plus exact retiring track identities.
/// No spec/source clones, and no mutable workspace shared across renderers.
#[derive(Debug)]
pub(crate) struct PreparedRelease {
    pub(super) sample_time: Instant,
    pub(super) groups: HashSet<groups::GroupKey>,
    ticket: groups::GroupTicket,
    runtime_identity: Arc<RuntimeIdentity>,
    generation_cursor: u64,
    geometry_identity: Arc<GeometryIdentity>,
    stamp: FrameStamp,
    context: Option<Arc<QueryContext>>,
    fields: HashMap<(NodeId, Axis), PreparedField>,
}
impl PreparedRelease {
    fn new(
        tree: &ElementTree,
        runtime: Option<&AnimationRuntime>,
        state: &LengthRuntime,
        sample_time: Instant,
        groups: HashSet<groups::GroupKey>,
        fields: HashMap<(NodeId, Axis), PreparedField>,
    ) -> Result<Self, ProjectionError> {
        let runtime = runtime.ok_or(ProjectionError::StalePreparation)?;
        if runtime.synced_sample_time != Some(sample_time) || runtime.admission_error.is_some() {
            return Err(ProjectionError::StalePreparation);
        }
        let completed = runtime.completed_enter_ids(tree, sample_time, &groups);
        runtime.check_enter_handoff_capacity(tree, &completed)?;
        Ok(Self {
            sample_time,
            ticket: runtime.groups.ticket(&groups)?,
            groups,
            runtime_identity: runtime
                .identity
                .as_ref()
                .map(|id| Arc::clone(id))
                .ok_or(ProjectionError::StalePreparation)?,
            generation_cursor: runtime.last_generation,
            geometry_identity: Arc::clone(&state.identity),
            stamp: FrameStamp::capture(tree),
            context: state.context.clone(),
            fields,
        })
    }
    fn validate(
        &self,
        tree: &ElementTree,
        runtime: &AnimationRuntime,
        state: &LengthRuntime,
        delta: Option<&GeometryDelta>,
    ) -> Result<(), ProjectionError> {
        let context_matches = match (&self.context, &state.context) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if self.stamp != FrameStamp::capture(tree)
            || !context_matches
            || !Arc::ptr_eq(&self.geometry_identity, &state.identity)
            || runtime
                .identity
                .as_ref()
                .is_none_or(|id| !Arc::ptr_eq(id, &self.runtime_identity))
            || runtime.last_generation != self.generation_cursor
            || runtime.synced_sample_time != Some(self.sample_time)
            || runtime.admission_error.is_some()
            || !self.ticket.valid_for(&runtime.groups)
            || !self.fields.iter().all(|(id, expected)| {
                delta
                    .or(state.applied_delta.as_deref())
                    .and_then(|delta| delta.updates.get(id))
                    .or_else(|| state.tracks.get(id))
                    .is_some_and(|track| track.key == expected.key && track.to == expected.terminal)
            })
        {
            return Err(ProjectionError::StalePreparation);
        }
        Ok(())
    }
}
/// Sparse presentation writes returned by preparation, owned by the frame until apply.
#[derive(Debug)]
pub(crate) struct GeometryDelta {
    frame_context: Option<Arc<QueryContext>>,
    /// Native transport certified the pending declared role before installation.
    prepared_roles: HashSet<(NodeId, Axis)>,
    updates: HashMap<(NodeId, Axis), Arc<Track>>,
    active: HashSet<(NodeId, Axis)>,
    samples: Vec<((NodeId, Axis), Option<AxisFootprint>)>,
    expected: HashMap<(NodeId, Axis), Arc<Track>>,
    identity: Arc<GeometryIdentity>,
    receipt: Option<PreparedRelease>,
}
impl GeometryDelta {
    pub(crate) fn context(&self) -> Option<Arc<QueryContext>> {
        self.frame_context.clone()
    }
    pub(crate) fn validate(
        &self,
        tree: &ElementTree,
        runtime: Option<&AnimationRuntime>,
    ) -> Result<(), ProjectionError> {
        let state = tree
            .length_runtime
            .as_ref()
            .ok_or(ProjectionError::StalePreparation)?;
        if !Arc::ptr_eq(&self.identity, &state.identity)
            || self.expected.len() != state.tracks.len()
            || !self.expected.iter().all(|(key, old)| {
                state
                    .tracks
                    .get(key)
                    .is_some_and(|track| Arc::ptr_eq(track, old))
            })
        {
            return Err(ProjectionError::StalePreparation);
        }
        if let Some(receipt) = &self.receipt {
            receipt.validate(
                tree,
                runtime.ok_or(ProjectionError::StalePreparation)?,
                state,
                Some(self),
            )?;
        }
        for &((id, axis), sample) in &self.samples {
            crate::tree::layout::dimensions::validate_prepared_axis_sample(
                tree,
                &id,
                axis,
                sample,
                self.prepared_roles.contains(&(id, axis)),
            )
            .map_err(ProjectionError::InvalidSample)?;
        }
        Ok(())
    }
    pub(crate) fn apply(
        mut self,
        tree: &mut ElementTree,
        runtime: Option<&AnimationRuntime>,
    ) -> Result<(), ProjectionError> {
        self.validate(tree, runtime)?;
        for &((id, axis), sample) in &self.samples {
            crate::tree::layout::dimensions::set_prepared_axis_sample(
                tree,
                &id,
                axis,
                sample,
                self.prepared_roles.contains(&(id, axis)),
            )
            .map_err(ProjectionError::InvalidSample)?;
        }
        let state = tree
            .length_runtime
            .as_mut()
            .ok_or(ProjectionError::StalePreparation)?;
        state.pending_release = self.receipt.take();
        state.applied_delta = Some(Box::new(self));
        Ok(())
    }
}
#[derive(Default)]
pub struct LengthRuntime {
    identity: Arc<GeometryIdentity>,
    resolver: EndpointResolver,
    pending_release: Option<PreparedRelease>,
    applied_delta: Option<Box<GeometryDelta>>,
    tracks: HashMap<(NodeId, Axis), Arc<Track>>,
    model: Option<u64>,
    seed_ids: Vec<NodeId>,
    image_sources: Vec<super::super::attrs::ImageSource>,
    image_nodes: Vec<NodeId>,
    context: Option<Arc<QueryContext>>,
    pub(super) content_inputs: Option<super::content::InputAttempt>,
}
impl Clone for LengthRuntime {
    fn clone(&self) -> Self {
        Self {
            tracks: self.tracks.clone(),
            ..Self::default()
        }
    }
}
impl std::fmt::Debug for LengthRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LengthRuntime")
            .field("tracks", &self.tracks.len())
            .field("queries", &self.resolver.stats())
            .finish()
    }
}

/// Only direct pixels have a context-independent numeric interpolation path.
/// Matching symbolic shapes (including weighted bounds) still require complete
/// endpoint footprints: interpolating their parameters is a different trajectory.
pub(crate) fn direct_numeric(from: &Length, to: &Length) -> bool {
    matches!((from, to), (Length::Px(_), Length::Px(_)))
}

fn length(attrs: &Attrs, axis: Axis) -> Option<&Length> {
    match axis {
        Axis::Width => attrs.width.as_ref(),
        Axis::Height => attrs.height.as_ref(),
    }
}
fn clear_length(attrs: &mut Attrs, axis: Axis) {
    match axis {
        Axis::Width => attrs.width = None,
        Axis::Height => attrs.height = None,
    }
}
fn set_projection_sample(node: &mut NodeProjection, axis: Axis, fp: AxisFootprint) {
    clear_length(&mut node.attrs, axis);
    match axis {
        Axis::Width => node.width = Some(fp),
        Axis::Height => node.height = Some(fp),
    }
}
fn boundary(run: RunRef<'_>, ms: f64) -> Option<Instant> {
    std::time::Duration::try_from_secs_f64(ms / 1000.0)
        .ok()
        .and_then(|d| run.started.checked_add(d))
}

fn has_structural_source(tree: &ElementTree, id: NodeId, axis: Axis) -> bool {
    tree.pending_patch_effects
        .sources
        .get(&id)
        .is_some_and(|source| {
            tree.get(&id)
                .is_some_and(|node| node.lifecycle.mounted_at_revision == source.mounted_at)
                && (!source.valid_transport_origin() || source.attachment_changed(tree, &id, axis))
        })
}

pub(crate) fn resolve_frame<M: TextMeasurer>(
    tree: &mut ElementTree,
    runtime: Option<&AnimationRuntime>,
    now: Option<Instant>,
    scale: f32,
    inherited: &FontContext,
    measurer: &M,
    frame: &mut AnimationFrameSamples,
) {
    if frame.result.preparation_error.is_some() {
        return;
    }
    if tree.length_runtime.as_ref().is_none_or(|state| {
        state.tracks.is_empty() && state.pending_release.is_none() && state.applied_delta.is_none()
    }) && runtime.is_none_or(|state| {
        state
            .groups
            .ready(now.unwrap_or_else(Instant::now))
            .is_empty()
    }) && !frame
        .samples
        .keys()
        .filter_map(|id| tree.get(id))
        .any(|node| {
            super::runs_for_element(node, runtime, now)
                .into_iter()
                .any(|run| {
                    let entry = AnimationRuntimeEntry {
                        spec_hash: run.revision,
                        started_at: run.started,
                    };
                    timing::position(run.spec, Some(&entry), now).is_some_and(|p| {
                        p.active
                            && [Axis::Width, Axis::Height].into_iter().any(|axis| {
                                if !run.fields.intersects(super::fields::FieldMask::field(
                                    if axis == Axis::Width {
                                        super::change::Field::Width
                                    } else {
                                        super::change::Field::Height
                                    },
                                )) {
                                    return false;
                                }
                                length(&run.spec.keyframes[p.index], axis)
                                    .zip(length(&run.spec.keyframes[p.index + 1], axis))
                                    .is_some_and(|(a, b)| {
                                        !direct_numeric(a, b)
                                            || has_structural_source(tree, node.id, axis)
                                            || run.source.is_some_and(|s| {
                                                s.sampled[if axis == Axis::Width { 0 } else { 1 }]
                                            })
                                    })
                            })
                    })
                })
        })
    {
        tree.animation_error = None;
        return;
    }
    let mut state = tree.length_runtime.take().unwrap_or_default();
    #[cfg(test)]
    if let Some(cell) = tree.animation_inspection.as_ref() {
        let mut trace = cell.borrow_mut();
        if trace.queries.is_empty() {
            trace.before = state.resolver.stats();
        }
        trace.cached_targets = state
            .tracks
            .iter()
            .map(|(key, track)| (*key, track.to))
            .collect();
    }
    let result = state.resolve(tree, runtime, now, scale, inherited, measurer, frame);
    #[cfg(test)]
    if let Some(cell) = tree.animation_inspection.as_ref() {
        let mut trace = cell.borrow_mut();
        trace.after = state.resolver.stats();
    }
    tree.animation_error = result
        .as_ref()
        .err()
        .map(|error| format!("animation endpoint: {error:?}"));
    frame.result.preparation_error = result.err();
    if !state.is_empty() || state.pending_release.is_some() || frame.geometry.is_some() {
        tree.length_runtime = Some(state);
    }
}

impl LengthRuntime {
    #[cfg(any(test, feature = "bench-diagnostics"))]
    pub(crate) fn query_stats(&self) -> crate::tree::layout::projection::ProjectionStats {
        self.resolver.stats()
    }
    pub(super) fn query_context<M: TextMeasurer>(
        &mut self,
        tree: &ElementTree,
        scale: f32,
        inherited: &FontContext,
        measurer: &M,
    ) -> Result<Arc<QueryContext>, ProjectionError> {
        let constraint = tree
            .animation_constraint
            .ok_or(ProjectionError::InvalidContext)?;
        if self.model != Some(tree.layout_model_epoch) {
            self.seed_ids = tree
                .iter_nodes()
                .filter(|node| crate::tree::layout::projection::seed_candidate(node))
                .map(|node| node.id)
                .collect();
            let mut seen = HashSet::new();
            let images: Vec<_> = tree
                .iter_nodes()
                .filter_map(|node| {
                    node.spec
                        .declared
                        .image_src
                        .clone()
                        .map(|source| (node.id, source))
                })
                .collect();
            self.image_nodes = images.iter().map(|(id, _)| *id).collect();
            self.image_sources = images
                .into_iter()
                .map(|(_, source)| source)
                .filter(|source| seen.insert(source.clone()))
                .collect();
            self.model = Some(tree.layout_model_epoch);
        }
        let images: HashMap<_, _> = self
            .image_sources
            .iter()
            .filter_map(|source| {
                measurer
                    .image_dimensions(source, false)
                    .map(|size| (source.clone(), size))
            })
            .collect();
        let images = self
            .context
            .as_ref()
            .filter(|old| *old.images == images)
            .map(|old| Arc::clone(&old.images))
            .unwrap_or_else(|| Arc::new(images));
        let candidate = QueryContext {
            constraint,
            scale,
            inherited: inherited.clone(),
            metrics_epoch: measurer.metrics_epoch(),
            fonts: measurer.font_snapshot(),
            images,
            seeds: self
                .seed_ids
                .iter()
                .filter_map(|id| tree.get(id).map(|node| (*id, QuerySeed::capture(node))))
                .collect(),
        };
        let context = self
            .context
            .as_ref()
            .filter(|old| ***old == candidate)
            .cloned()
            .unwrap_or_else(|| Arc::new(candidate));
        self.context = Some(Arc::clone(&context));
        Ok(context)
    }
    pub(crate) fn content_frame_context(&self) -> Option<Arc<QueryContext>> {
        self.content_inputs
            .as_ref()
            .filter(|input| input.has_presentation)
            .and(self.context.as_ref())
            .cloned()
    }
    pub(crate) fn media_invalidations(
        &self,
        tree: &ElementTree,
        context: &QueryContext,
    ) -> Vec<NodeId> {
        self.image_nodes.iter().copied().filter(|id| {
            let Some(node)=tree.get(id) else {return false;};
            let Some(cache)=node.layout.intrinsic_measure_cache.as_ref() else {return false;};
            matches!(&cache.key,crate::tree::element::IntrinsicMeasureCacheKey::Media {image_src:Some(source),image_size:None,resolved_source_size,..} if *resolved_source_size!=context.images.get(source).copied())
        }).collect()
    }
    pub(super) fn content_projection(
        &self,
        tree: &ElementTree,
        runtime: &AnimationRuntime,
        now: Instant,
    ) -> Result<Projection, ProjectionError> {
        let samples = super::sample_animation_overlays_for_ids(
            tree,
            runtime,
            &runtime.active_node_ids(),
            Some(now),
        );
        let mut projection = Projection {
            nodes: samples
                .samples
                .into_iter()
                .map(|(id, sample)| {
                    (
                        id,
                        NodeProjection {
                            attrs: layout_attrs(&sample.attrs),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        };
        for (&(id, axis), track) in &self.tracks {
            let Some(node) = tree.get(&id) else {
                continue;
            };
            if super::content::has_policy(&node.spec.declared)
                && super::content::axis(
                    if axis == Axis::Width {
                        super::change::Field::Width
                    } else {
                        super::change::Field::Height
                    },
                    &node.spec.declared,
                )
                .is_some()
            {
                continue;
            }
            let Some(run) = super::runs_for_element(node, Some(runtime), Some(now))
                .into_iter()
                .find(|run| track.same_run(*run, node.lifecycle.mounted_at_revision))
            else {
                continue;
            };
            let clock = AnimationRuntimeEntry {
                spec_hash: run.revision,
                started_at: run.started,
            };
            if let Some(p) = timing::position(run.spec, Some(&clock), Some(now))
                && p.index == track.key.segment
            {
                set_projection_sample(
                    projection.nodes.entry(id).or_default(),
                    axis,
                    track.sample(run, p, now)?,
                );
            }
        }
        Ok(projection)
    }
    pub(super) fn content_targets<M: TextMeasurer>(
        &mut self,
        tree: &ElementTree,
        context: Arc<QueryContext>,
        projection: Arc<Projection>,
        endpoints: &[(NodeId, Axis)],
        measurer: &M,
    ) -> Result<HashMap<(NodeId, Axis), AxisFootprint>, ProjectionError> {
        #[cfg(test)]
        if let Some(cell) = tree.animation_inspection.as_ref() {
            let mut trace = cell.borrow_mut();
            if trace.queries.is_empty() {
                trace.before = self.resolver.stats();
            }
        }
        let result = self.resolver.resolve(
            tree,
            tree.layout_model_epoch,
            Arc::clone(&projection),
            context,
            endpoints,
            measurer,
        );
        #[cfg(test)]
        let result = inspection::observe_query(
            tree,
            || inspection::QueryKind::ContentTarget,
            &projection,
            result,
        );
        #[cfg(test)]
        if let Some(cell) = tree.animation_inspection.as_ref() {
            cell.borrow_mut().after = self.resolver.stats();
        }
        result
    }

    /// Validate the original native goal under its joint clock/environment inputs
    /// before releasing against a changed context. Declarations replay privately;
    /// seed membership changes require an opaque original-evaluation receipt.
    fn context_release<M: TextMeasurer>(
        &mut self,
        tree: &ElementTree,
        context: &QueryContext,
        current: &HashMap<(NodeId, Axis), AxisFootprint>,
        terminal: &HashMap<(NodeId, Axis), AxisFootprint>,
        measurer: &M,
    ) -> Result<HashSet<(NodeId, Axis)>, ProjectionError> {
        let sources = &tree.pending_patch_effects.sources;
        let valid_sources = !sources.is_empty()
            && sources.iter().all(|(id, source)| {
                tree.get(id)
                    .is_some_and(|node| node.lifecycle.mounted_at_revision == source.mounted_at)
            });
        let mut model_preimages = HashMap::new();
        let requests = terminal
            .iter()
            .filter_map(|(key, target)| {
                let track = self.tracks.get(key)?;
                let sample = current.get(key)?;
                let model_changed = track.model != tree.layout_model_epoch;
                let previous_model = tree.publication.0.is_some_and(|published| {
                    published.model == track.model
                        && published.structure == tree.layout_structure_epoch
                }) && valid_sources
                    && sources.values().all(|source| source.model == track.model);
                ((!model_changed || previous_model)
                    && !sample.release_matches(*target)
                    && sample.release_matches(track.to))
                .then_some((
                    *key,
                    Arc::clone(&track.context),
                    Arc::clone(&track.projection),
                    model_changed,
                ))
            })
            .fold(
                HashMap::<
                    (usize, usize, bool),
                    (
                        Arc<QueryContext>,
                        Arc<Projection>,
                        bool,
                        Vec<(NodeId, Axis)>,
                    ),
                >::new(),
                |mut requests, (key, context, projection, model_changed)| {
                    requests
                        .entry((
                            Arc::as_ptr(&context) as usize,
                            Arc::as_ptr(&projection) as usize,
                            model_changed,
                        ))
                        .or_insert_with(|| (context, projection, model_changed, Vec::new()))
                        .3
                        .push(key);
                    requests
                },
            );
        requests
            .into_values()
            .filter(|(previous, _, model_changed, _)| {
                (*model_changed && context.same_environment(previous))
                    || context.replayable_change_from(previous)
                    || (!context.same_seed_membership(previous)
                        && context.recorded_change_from(previous))
            })
            .map(|(previous, projection, model_changed, endpoints)| {
                if !context.same_seed_membership(&previous) {
                    return endpoints
                        .into_iter()
                        .map(|key| {
                            let track = &self.tracks[&key];
                            let native = track.evidence.native(
                                track.model,
                                &track.projection,
                                &track.context,
                                key,
                                track.key.mount,
                            );
                            #[cfg(test)]
                            let native = inspection::observe_query(
                                tree,
                                || inspection::QueryKind::HistoricalTarget,
                                &track.projection,
                                native.map(|fp| HashMap::from([(key, fp)])),
                            )
                            .and_then(|values| {
                                values
                                    .get(&key)
                                    .copied()
                                    .ok_or(ProjectionError::MissingLayout(key.0, key.1))
                            });
                            if !current[&key].release_matches(native?) {
                                return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
                            }
                            Ok(key)
                        })
                        .collect::<Result<Vec<_>, _>>();
                }
                // Replay the original owner query, including its original clock
                // forecasts. Current release inputs plus old viewport/model facts
                // are not a historical state when clocks moved at the same time.
                let release = &projection;
                let release = if model_changed {
                    model_preimages
                        .entry(Arc::as_ptr(release) as usize)
                        .or_insert_with(|| {
                            let nodes = release
                                .nodes
                                .iter()
                                .map(|(id, node)| {
                                    (
                                        *id,
                                        NodeProjection {
                                            model: sources
                                                .get(id)
                                                .map(|source| Arc::new(source.declared.clone())),
                                            ..node.clone()
                                        },
                                    )
                                })
                                .chain(
                                    sources
                                        .iter()
                                        .filter(|(id, _)| !release.nodes.contains_key(id))
                                        .map(|(id, source)| {
                                            (
                                                *id,
                                                NodeProjection {
                                                    model: Some(Arc::new(source.declared.clone())),
                                                    ..Default::default()
                                                },
                                            )
                                        }),
                                )
                                .collect();
                            Arc::new(Projection { nodes })
                        })
                } else {
                    release
                };
                #[cfg(test)]
                let previous_for_kind = Arc::clone(&previous);
                let result = self.resolver.resolve(
                    tree,
                    tree.layout_model_epoch,
                    Arc::clone(release),
                    previous,
                    &endpoints,
                    measurer,
                );
                #[cfg(test)]
                let result = inspection::observe_query(
                    tree,
                    || {
                        if model_changed {
                            inspection::QueryKind::PreviousModel
                        } else if context.viewport_change_from(&previous_for_kind) {
                            inspection::QueryKind::PreviousViewport
                        } else {
                            inspection::QueryKind::PreviousInputs
                        }
                    },
                    release,
                    result,
                );
                let values = result?;
                endpoints
                    .into_iter()
                    .map(|key| {
                        let native = values
                            .get(&key)
                            .ok_or(ProjectionError::MissingLayout(key.0, key.1))?;
                        if !current[&key].release_matches(*native) {
                            return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
                        }
                        Ok(key)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|groups| groups.into_iter().flatten().collect())
    }

    /// Reusing a continuous anchor must not launder an invalid cached endpoint.
    /// Batch witnesses by shared query inputs in the one renderer-local workspace.
    fn verify_continuations<M: TextMeasurer>(
        &mut self,
        tree: &ElementTree,
        tracks: &mut ContinuedTracks,
        next_targets: &HashMap<(NodeId, Axis), AxisFootprint>,
        measurer: &M,
    ) -> Result<(), ProjectionError> {
        let batches = tracks.iter().fold(
            HashMap::<(usize, usize), Vec<(NodeId, Axis)>>::new(),
            |mut batches, (key, (track, proof))| {
                batches
                    .entry((
                        Arc::as_ptr(proof) as usize,
                        Arc::as_ptr(&track.context) as usize,
                    ))
                    .or_default()
                    .push(*key);
                batches
            },
        );
        let mut dependent = HashSet::new();
        for keys in batches.into_values() {
            let (track, proof) = &tracks[&keys[0]];
            // Certify the original cached destination first. Otherwise a bad
            // cache that coincidentally matches new sibling geometry could be
            // mistaken for independence.
            if let Some((_, original)) = &proof.independent {
                let result = self.resolver.resolve(
                    tree,
                    tree.layout_model_epoch,
                    Arc::clone(original),
                    Arc::clone(&track.context),
                    &keys,
                    measurer,
                );
                #[cfg(test)]
                let result = inspection::observe_query(
                    tree,
                    || inspection::QueryKind::PriorTarget,
                    original,
                    result,
                );
                let values = result?;
                for key in &keys {
                    if values
                        .get(key)
                        .is_none_or(|fp| !fp.release_matches(tracks[key].0.to))
                    {
                        return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
                    }
                }
            }
            let before = keys
                .iter()
                .map(|key| (*key, tracks[key].0.to))
                .chain(proof.pixels.iter().copied())
                .collect::<Vec<_>>();
            // Check both the complete foreign sample and the dependent target.
            // Pixel-looking geometry alone does not prove identical allocation
            // or descendant behavior (imposed sizing and policy bits matter).
            let after = proof.next.as_ref().map(|(projection, pixels)| {
                (
                    Arc::clone(projection),
                    keys.iter()
                        .map(|key| (*key, next_targets[key]))
                        .chain(pixels.iter().copied())
                        .collect::<Vec<_>>(),
                )
            });
            for (key, input) in &proof.foreign {
                let native = input.evidence.native(
                    input.model,
                    &input.projection,
                    &input.context,
                    *key,
                    input.key.mount,
                )?;
                if input.model != tree.layout_model_epoch
                    || !Arc::ptr_eq(&input.context, &track.context)
                {
                    #[cfg(test)]
                    let native = inspection::observe_query(
                        tree,
                        || inspection::QueryKind::HistoricalTarget,
                        &input.projection,
                        Ok(HashMap::from([(*key, native)])),
                    )
                    .and_then(|values| {
                        values
                            .get(key)
                            .copied()
                            .ok_or(ProjectionError::MissingLayout(key.0, key.1))
                    })?;
                    if !input.to.release_matches(native) {
                        return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
                    }
                }
            }
            let foreign = proof
                .foreign
                .iter()
                .filter(|(_, input)| {
                    input.model == tree.layout_model_epoch
                        && Arc::ptr_eq(&input.context, &track.context)
                })
                .fold(
                    HashMap::<
                        (usize, usize),
                        (
                            Arc<Projection>,
                            Arc<QueryContext>,
                            Vec<((NodeId, Axis), AxisFootprint)>,
                        ),
                    >::new(),
                    |mut batches, (key, track)| {
                        batches
                            .entry((
                                Arc::as_ptr(&track.projection) as usize,
                                Arc::as_ptr(&track.context) as usize,
                            ))
                            .or_insert_with(|| {
                                (
                                    Arc::clone(&track.projection),
                                    Arc::clone(&track.context),
                                    Vec::new(),
                                )
                            })
                            .2
                            .push((*key, track.to));
                        batches
                    },
                );
            for (projection, context, expected, _foreign) in
                std::iter::once((Arc::clone(&proof.projection), before))
                    .chain(after)
                    .map(|(projection, expected)| {
                        (projection, Arc::clone(&track.context), expected, false)
                    })
                    .chain(
                        foreign
                            .into_values()
                            .map(|(projection, context, expected)| {
                                (projection, context, expected, true)
                            }),
                    )
            {
                let endpoints: Vec<_> = expected.iter().map(|(key, _)| *key).collect();
                let result = self.resolver.resolve(
                    tree,
                    tree.layout_model_epoch,
                    Arc::clone(&projection),
                    context,
                    &endpoints,
                    measurer,
                );
                #[cfg(test)]
                let result = inspection::observe_query(
                    tree,
                    || {
                        if _foreign {
                            inspection::QueryKind::ForeignTarget
                        } else if Arc::ptr_eq(&projection, &proof.projection) {
                            if proof.independent.is_some() {
                                inspection::QueryKind::IndependentContext
                            } else {
                                inspection::QueryKind::PriorTarget
                            }
                        } else {
                            inspection::QueryKind::PixelContinuation
                        }
                    },
                    &projection,
                    result,
                );
                let native = result?;
                for (key, expected) in expected {
                    if native
                        .get(&key)
                        .is_none_or(|fp| !fp.release_matches(expected))
                    {
                        if !_foreign
                            && Arc::ptr_eq(&projection, &proof.projection)
                            && proof.independent.is_some()
                            && keys.contains(&key)
                        {
                            // A genuine allocation dependency is not corrupted
                            // evidence. Decline continuation and use normal retarget
                            // or release validation, while checking every other witness.
                            dependent.insert(key);
                        } else {
                            return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
                        }
                    }
                }
            }
        }
        tracks.retain(|key, _| !dependent.contains(key));
        Ok(())
    }

    /// Existing retarget work belongs to the admitted owner, not to whichever
    /// siblings still happen to be selected. Cancellation cannot truncate it.
    pub(super) fn hold_deadline(&self, owner: groups::OwnerKey, now: Instant) -> Option<Instant> {
        [Axis::Width, Axis::Height]
            .into_iter()
            .filter_map(|axis| {
                let track = self.tracks.get(&(owner.node, axis))?;
                (track.key.owner == owner.kind
                    && track.key.mount == owner.mount
                    && track.key.generation == owner.generation
                    && track.key.started == owner.started)
                    .then_some(track.hold_interval)
                    .flatten()
                    .map(|(_, end)| end)
                    .filter(|end| *end > now)
            })
            .max()
    }

    pub(crate) fn commit_delta(&mut self) {
        if let Some(delta) = self.applied_delta.take() {
            self.tracks.retain(|key, _| delta.active.contains(key));
            self.tracks.extend(delta.updates);
        }
    }
    pub(crate) fn validate_delta(&self) -> Result<(), ProjectionError> {
        if self.applied_delta.as_ref().is_some_and(|delta| {
            delta.expected.len() != self.tracks.len()
                || !delta.expected.iter().all(|(key, old)| {
                    self.tracks
                        .get(key)
                        .is_some_and(|track| Arc::ptr_eq(track, old))
                })
        }) {
            return Err(ProjectionError::StalePreparation);
        }
        Ok(())
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.content_inputs.is_none()
    }
    pub(crate) fn validate_release(
        &self,
        tree: &ElementTree,
        runtime: &AnimationRuntime,
    ) -> Result<(), ProjectionError> {
        if let Some(receipt) = &self.pending_release {
            receipt.validate(tree, runtime, self, None)?;
        }
        Ok(())
    }
    pub(crate) fn release(&self) -> Option<&PreparedRelease> {
        self.pending_release.as_ref()
    }
    pub(crate) fn take_release(&mut self) -> Option<PreparedRelease> {
        self.pending_release.take()
    }
    pub(crate) fn finish_release(&mut self, release: &PreparedRelease) {
        self.tracks.retain(|key, track| {
            release
                .fields
                .get(key)
                .is_none_or(|field| field.key != track.key)
        });
    }
    pub(crate) fn terminal(&self, id: NodeId, axis: Axis) -> Option<AxisFootprint> {
        self.applied_delta
            .as_ref()
            .and_then(|delta| delta.updates.get(&(id, axis)))
            .or_else(|| self.tracks.get(&(id, axis)))
            .map(|t| t.to)
    }
    #[allow(clippy::too_many_arguments)]
    fn resolve<M: TextMeasurer>(
        &mut self,
        tree: &mut ElementTree,
        runtime: Option<&AnimationRuntime>,
        now: Option<Instant>,
        scale: f32,
        inherited: &FontContext,
        measurer: &M,
        frame: &mut AnimationFrameSamples,
    ) -> Result<(), ProjectionError> {
        // No receipt from a prior uncommitted preparation may survive a failure.
        self.pending_release = None;
        self.applied_delta = None;
        let expected = self.tracks.clone();
        let runs: Vec<_> = frame
            .samples
            .keys()
            .filter_map(|id| tree.get(id))
            .flat_map(|node| {
                super::runs_for_element(node, runtime, now)
                    .into_iter()
                    .map(move |run| (node.id, node.lifecycle.mounted_at_revision, run))
            })
            .collect();
        let now = now.unwrap_or_else(Instant::now);
        let group_for = |id, mount, run: RunRef<'_>| {
            runtime.and_then(|state| {
                state.groups.group(groups::OwnerKey {
                    node: id,
                    mount,
                    kind: run.owner,
                    generation: run.generation,
                    started: run.started,
                })
            })
        };
        let positions: Vec<_> = runs
            .iter()
            .filter_map(|&(id, mount, run)| {
                let entry = AnimationRuntimeEntry {
                    spec_hash: run.revision,
                    started_at: run.started,
                };
                timing::position(run.spec, Some(&entry), Some(now)).map(|p| (id, mount, run, p))
            })
            .collect();
        let ready = runtime
            .map(|state| state.groups.ready(now))
            .unwrap_or_default();
        let tracks = &self.tracks;
        let source_tree: &ElementTree = tree;
        let fields: Vec<_> = positions
            .iter()
            .filter(|(id, mount, run, p)| p.active || group_for(*id, *mount, *run).is_some())
            .flat_map(|&(id, mount, run, p)| {
                [Axis::Width, Axis::Height]
                    .into_iter()
                    .filter_map(move |axis| {
                        if !run.fields.intersects(super::fields::FieldMask::field(
                            if axis == Axis::Width {
                                super::change::Field::Width
                            } else {
                                super::change::Field::Height
                            },
                        )) {
                            return None;
                        }
                        let a = length(&run.spec.keyframes[p.index], axis)?;
                        let b = length(&run.spec.keyframes[p.index + 1], axis)?;
                        let captured_mixed = run.source.is_some_and(|source| {
                            source.sampled[if axis == Axis::Width { 0 } else { 1 }]
                        });
                        (!direct_numeric(a, b)
                            || has_structural_source(source_tree, id, axis)
                            || captured_mixed
                            || tracks
                                .get(&(id, axis))
                                .is_some_and(|track| track.same_run(run, mount)))
                        .then_some((
                            id,
                            axis,
                            run,
                            p,
                            RunKey {
                                owner: run.owner,
                                revision: run.revision,
                                generation: run.generation,
                                started: run.started,
                                mount,
                                segment: p.index,
                                cycle: p.start_ms.to_bits(),
                            },
                        ))
                    })
            })
            .collect();
        let release_fields: HashSet<_> = fields
            .iter()
            .filter_map(|&(id, axis, run, _, key)| {
                group_for(id, key.mount, run)
                    .is_some_and(|group| ready.contains(&group))
                    .then_some((id, axis))
            })
            .collect();
        if fields.is_empty() {
            let receipt = if ready.is_empty() {
                None
            } else {
                Some(PreparedRelease::new(
                    tree,
                    runtime,
                    self,
                    now,
                    ready,
                    HashMap::new(),
                )?)
            };
            frame.geometry = Some(GeometryDelta {
                frame_context: None,
                prepared_roles: HashSet::new(),
                updates: HashMap::new(),
                active: HashSet::new(),
                expected,
                receipt,
                identity: Arc::clone(&self.identity),
                samples: self
                    .tracks
                    .keys()
                    .filter(|(id, _)| tree.get(id).is_some())
                    .map(|key| (*key, None))
                    .collect(),
            });
            return Ok(());
        }
        let context = self.query_context(tree, scale, inherited, measurer)?;
        let mut frozen = Projection {
            nodes: frame
                .samples
                .iter()
                .map(|(id, sample)| {
                    (
                        *id,
                        NodeProjection {
                            attrs: layout_attrs(&sample.attrs),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        };
        // Freeze every moving/held peer once, before refreshing any group.
        let mut current: HashMap<_, _> = fields
            .iter()
            .filter_map(|&(id, axis, run, p, key)| {
                self.tracks
                    .get(&(id, axis))
                    .filter(|track| track.key == key)
                    .map(|track| track.sample(run, p, now).map(|fp| ((id, axis), fp)))
            })
            .collect::<Result<_, _>>()?;
        for (&(id, axis), &fp) in &current {
            set_projection_sample(frozen.nodes.entry(id).or_default(), axis, fp);
        }
        fields.iter().try_for_each(|(id, _, _, _, key)| {
            if tree
                .pending_patch_effects
                .sources
                .get(id)
                .is_some_and(|source| {
                    source.mounted_at == key.mount && !source.valid_transport_origin()
                })
            {
                Err(ProjectionError::StalePreparation)
            } else {
                Ok(())
            }
        })?;
        let transports = fields
            .iter()
            .filter_map(|&(id, axis, run, p, key)| {
                let source = tree.pending_patch_effects.sources.get(&id)?;
                (source.attachment_changed(tree, &id, axis) && source.mounted_at == key.mount)
                    .then_some((id, axis, run, p, key, source))
            })
            .collect::<Vec<_>>();
        let mut transported_releases = HashSet::new();
        if !transports.is_empty() {
            let mut projection = frozen.clone();
            for &(id, axis, run, p, key, source) in &transports {
                let old = self.tracks.get(&(id, axis));
                if let Some(old) = old {
                    let native = old.evidence.native(
                        old.model,
                        &old.projection,
                        &old.context,
                        (id, axis),
                        old.key.mount,
                    )?;
                    if !native.release_matches(old.to) {
                        return Err(ProjectionError::ReleaseMismatch(id, axis));
                    }
                }
                let fp = if release_fields.contains(&(id, axis)) {
                    let old = old
                        .filter(|old| old.same_run(run, key.mount))
                        .ok_or(ProjectionError::MissingLayout(id, axis))?;
                    let sample = old.sample(run, p, now)?;
                    if !sample.release_matches(old.to) {
                        return Err(ProjectionError::ReleaseMismatch(id, axis));
                    }
                    transported_releases.insert((id, axis));
                    sample
                } else {
                    source.dimensions[if axis == Axis::Width { 0 } else { 1 }]
                        .ok_or(ProjectionError::MissingLayout(id, axis))?
                };
                let node = projection.nodes.entry(id).or_default();
                clear_length(&mut node.attrs, axis);
                match axis {
                    Axis::Width => node.width = None,
                    Axis::Height => node.height = None,
                };
                Arc::make_mut(node.transport.get_or_insert_with(|| Arc::new([None, None])))
                    [if axis == Axis::Width { 0 } else { 1 }] =
                    Some(crate::tree::layout::dimensions::TransportSource {
                        footprint: fp,
                        scale: source.scale,
                    });
            }
            let projection = Arc::new(projection);
            let endpoints = transports
                .iter()
                .map(|(id, axis, ..)| (*id, *axis))
                .collect::<Vec<_>>();
            let result = self.resolver.resolve(
                tree,
                tree.layout_model_epoch,
                Arc::clone(&projection),
                Arc::clone(&context),
                &endpoints,
                measurer,
            );
            #[cfg(test)]
            let result = inspection::observe_query(
                tree,
                || inspection::QueryKind::Transport,
                &projection,
                result,
            );
            for (key, fp) in result? {
                current.insert(key, fp);
                set_projection_sample(frozen.nodes.entry(key.0).or_default(), key.1, fp);
            }
        }

        let prepared_roles = transports
            .iter()
            .map(|(id, axis, ..)| (*id, *axis))
            .collect::<HashSet<_>>();
        #[cfg(test)]
        if let Some(cell) = tree.animation_inspection.as_ref() {
            let mut trace = cell.borrow_mut();
            trace.frozen = Some(Arc::new(frozen.clone()));
            trace.context = Some(Arc::clone(&context));
            trace.releasing = release_fields.clone();
        }
        let project = |request: RequestKey| {
            let mut projection = frozen.clone();
            for &(id, mount, run, p) in &positions {
                let group = group_for(id, mount, run);
                let matches = match request.scope {
                    ProjectionScope::Group(wanted) => group == Some(wanted),
                    ProjectionScope::Release => group.is_some_and(|key| ready.contains(&key)),
                    ProjectionScope::Owner(owner) => {
                        group.is_none()
                            && p.active
                            && owner
                                == groups::OwnerKey {
                                    node: id,
                                    mount,
                                    kind: run.owner,
                                    generation: run.generation,
                                    started: run.started,
                                }
                    }
                };
                if !matches {
                    continue;
                }
                if request
                    .source_at
                    .is_some_and(|when| boundary(run, p.start_ms) != Some(when))
                {
                    continue;
                }
                let frame_index = if request.source_at.is_some() {
                    p.index
                } else {
                    p.index + 1
                };
                let Some(attrs) = run.spec.keyframes.get(frame_index) else {
                    continue;
                };
                let mut attrs = layout_attrs(&run.fields.select(attrs));
                // A joining member must not rewind existing mixed peers.
                if request.source_at.is_some() {
                    if current.contains_key(&(id, Axis::Width)) {
                        attrs.width = None;
                    }
                    if current.contains_key(&(id, Axis::Height)) {
                        attrs.height = None;
                    }
                }
                let node = projection.nodes.entry(id).or_default();
                if attrs.width.is_some() {
                    node.width = None;
                }
                if attrs.height.is_some() {
                    node.height = None;
                }
                apply_sample_attrs(&mut node.attrs, &attrs);
            }
            projection
        };
        // A changed foreign interval has no sample in `current`. Resolve its
        // actual current footprint before querying dependent releases. These
        // use the ordinary owner's frozen endpoint projections, not a future
        // layout graph or another owner's freshly recomputed destination.
        let boundary_tracks: HashMap<_, _> = if release_fields.is_empty() {
            HashMap::new()
        } else {
            fields
                .iter()
                .filter(|(id, _, run, _, key)| {
                    group_for(*id, key.mount, *run).is_none()
                        && (matches!(run.spec.repeat, AnimationRepeat::Loop)
                            || release_fields
                                .iter()
                                .any(|(owner, _)| continuation::is_ancestor(tree, *id, *owner)))
                })
                .filter(|(id, axis, run, _, key)| {
                    self.tracks
                        .get(&(*id, *axis))
                        .is_some_and(|old| old.key != *key && old.same_run(*run, key.mount))
                })
                .map(|&(id, axis, run, p, key)| {
                    let scope = ProjectionScope::Owner(groups::OwnerKey {
                        node: id,
                        mount: key.mount,
                        kind: run.owner,
                        generation: run.generation,
                        started: run.started,
                    });
                    let target = Arc::new(project(RequestKey {
                        scope,
                        source_at: None,
                    }));
                    let mut query = |projection: Arc<Projection>, _kind: bool| {
                        let result = self.resolver.resolve_certified(
                            tree,
                            tree.layout_model_epoch,
                            Arc::clone(&projection),
                            Arc::clone(&context),
                            &[(id, axis)],
                            measurer,
                        );
                        #[cfg(test)]
                        let result = inspection::observe_query(
                            tree,
                            || {
                                if _kind {
                                    inspection::QueryKind::Target
                                } else {
                                    inspection::QueryKind::Source
                                }
                            },
                            &projection,
                            result
                                .as_ref()
                                .map(|receipt| (***receipt).clone())
                                .map_err(Clone::clone),
                        )
                        .and(result);
                        let receipt = result?;
                        let value = receipt
                            .get(&(id, axis))
                            .copied()
                            .ok_or(ProjectionError::MissingLayout(id, axis))?;
                        Ok::<_, ProjectionError>((value, receipt))
                    };
                    let old = &self.tracks[&(id, axis)];
                    let from = if p.index > 0
                        && old.key.segment + 1 == p.index
                        && old.end_ms == p.start_ms
                    {
                        old.to
                    } else {
                        query(
                            Arc::new(project(RequestKey {
                                scope,
                                source_at: Some(
                                    boundary(run, p.start_ms)
                                        .ok_or(ProjectionError::InvalidContext)?,
                                ),
                            })),
                            false,
                        )?
                        .0
                    };
                    let (to, evidence) = query(Arc::clone(&target), true)?;
                    from.interpolate(to, 0.0)
                        .map_err(ProjectionError::InvalidSample)?;
                    Ok((
                        (id, axis),
                        Arc::new(Track {
                            forecast: None,
                            input: Arc::new(ClockInput {
                                evidence,
                                key,
                                from,
                                to,
                                anchor: 0.0,
                                projection: target,
                                context: Arc::clone(&context),
                                model: tree.layout_model_epoch,
                                evaluated_at: now,
                                end_ms: p.end_ms,
                                hold_interval: None,
                            }),
                        }),
                    ))
                })
                .collect::<Result<_, ProjectionError>>()?
        };
        let boundary_samples = if boundary_tracks.is_empty() {
            HashMap::new()
        } else {
            fields
                .iter()
                .filter_map(|&(id, axis, run, p, _)| {
                    boundary_tracks
                        .get(&(id, axis))
                        .map(|track| track.sample(run, p, now).map(|fp| ((id, axis), fp)))
                })
                .collect::<Result<HashMap<_, _>, _>>()?
        };
        // Only releases consume these current samples. Feeding them back into
        // owner endpoint queries would make destinations depend on query order.
        let project = |request: RequestKey| {
            let mut projection = project(request);
            if request.scope == ProjectionScope::Release {
                for (&(id, axis), &fp) in &boundary_samples {
                    set_projection_sample(projection.nodes.entry(id).or_default(), axis, fp);
                }
            }
            projection
        };
        // Group/request identities deduplicate in O(1), not a growing vector scan.
        let mut queries: Vec<BoundaryQuery> = Vec::new();
        let mut request_indices = HashMap::new();
        let mut prepared = HashMap::<RequestKey, Arc<Projection>>::new();
        let mut continuation_cache = HashMap::new();
        let mut independence_cache = continuation::IndependenceCache::default();
        let mut continuations = HashMap::new();
        let mut coupled_candidates = HashMap::new();
        let mut needed = Vec::new();
        for &(id, axis, run, p, key) in &fields {
            let group = group_for(id, key.mount, run);
            let scope = match group {
                Some(group) if ready.contains(&group) => ProjectionScope::Release,
                Some(group) => ProjectionScope::Group(group),
                None => ProjectionScope::Owner(groups::OwnerKey {
                    node: id,
                    mount: key.mount,
                    kind: run.owner,
                    generation: run.generation,
                    started: run.started,
                }),
            };
            let target_request = RequestKey {
                scope,
                source_at: None,
            };
            let old = self.tracks.get(&(id, axis)).filter(|t| t.key == key);
            let target = prepared
                .entry(target_request)
                .or_insert_with(|| {
                    let projection = project(target_request);
                    // One semantic comparison per request, then pointer checks per
                    // field. A large group's warm path must not compare N entries R times.
                    old.filter(|track| *track.projection == projection)
                        .map(|track| Arc::clone(&track.projection))
                        .unwrap_or_else(|| Arc::new(projection))
                })
                .clone();
            if scope != ProjectionScope::Release
                && old.is_some_and(|t| {
                    Arc::ptr_eq(&t.projection, &target)
                        && Arc::ptr_eq(&t.context, &context)
                        && t.model == tree.layout_model_epoch
                })
            {
                continue;
            }
            if let Some(old) = old.filter(|old| {
                !Arc::ptr_eq(&old.projection, &target)
                    && old.model == tree.layout_model_epoch
                    && Arc::ptr_eq(&old.context, &context)
            }) {
                let continuation = continuation_cache
                    .entry((
                        Arc::as_ptr(&old.projection) as usize,
                        Arc::as_ptr(&target) as usize,
                        old.evaluated_at,
                        scope == ProjectionScope::Release,
                    ))
                    .or_insert_with(|| {
                        numeric_context_continues(
                            tree,
                            old,
                            &target,
                            &positions,
                            now,
                            scope == ProjectionScope::Release,
                            &self.tracks,
                        )
                        .or_else(|| {
                            footprint_context_continues(
                                tree,
                                old,
                                &target,
                                &positions,
                                now,
                                &self.tracks,
                                (scope == ProjectionScope::Release).then_some(&boundary_tracks),
                            )
                        })
                    });
                if group.is_some()
                    && (p.active
                        || old.hold_interval.is_some()
                        || boundary(run, p.end_ms).is_some_and(|end| old.evaluated_at < end))
                    && let Some(proof) = continuation.as_ref()
                    && let Some(drivers) = proof.finite_feedback(tree, (id, axis), &positions)
                {
                    coupled_candidates
                        .insert((id, axis), ((Arc::clone(old), Arc::clone(proof)), drivers));
                }
                let proof = continuation
                    .as_ref()
                    .filter(|proof| proof.permits_owner(tree, (id, axis)))
                    .cloned()
                    .or_else(|| {
                        independence_cache.continuation(
                            tree,
                            ((id, axis), old, &release_fields),
                            &target,
                            (&positions, now),
                            &self.tracks,
                            (scope == ProjectionScope::Release).then_some(&boundary_tracks),
                        )
                    });
                if let Some(proof) = proof
                    && (p.active
                        || old.hold_interval.is_some()
                        || boundary(run, p.end_ms).is_some_and(|end| old.evaluated_at < end))
                {
                    continuations.insert((id, axis), (Arc::clone(old), proof));
                }
            }
            let captured = if p.index == 0 && p.start_ms == 0.0 {
                run.source
                    .and_then(|s| s.dimensions[if axis == Axis::Width { 0 } else { 1 }])
            } else {
                None
            };
            let preceding = self
                .tracks
                .get(&(id, axis))
                .filter(|t| {
                    p.index > 0
                        && t.key.segment + 1 == p.index
                        && t.same_run(run, key.mount)
                        && t.end_ms == p.start_ms
                        && t.key != key
                })
                .map(|t| t.to);
            let from = current.get(&(id, axis)).copied().or(preceding).or(captured);
            let target_index = add_query(
                &mut queries,
                &mut request_indices,
                target_request,
                target,
                (id, axis),
            );
            let source_index = if from.is_none() {
                let source_request = RequestKey {
                    scope,
                    source_at: Some(
                        boundary(run, p.start_ms).ok_or(ProjectionError::InvalidContext)?,
                    ),
                };
                let source = prepared
                    .entry(source_request)
                    .or_insert_with(|| Arc::new(project(source_request)))
                    .clone();
                Some(add_query(
                    &mut queries,
                    &mut request_indices,
                    source_request,
                    source,
                    (id, axis),
                ))
            } else {
                None
            };
            let hold_end = (!p.active)
                .then(|| {
                    // A new sibling can extend the group barrier, but not work that
                    // already has an admitted remaining interval.
                    old.and_then(|track| track.hold_interval)
                        .map(|(_, end)| end)
                        .filter(|end| *end > now)
                        .or_else(|| {
                            runtime.and_then(|state| {
                                state
                                    .groups
                                    .held_until(groups::OwnerKey {
                                        node: id,
                                        mount: key.mount,
                                        kind: run.owner,
                                        generation: run.generation,
                                        started: run.started,
                                    })
                                    .or_else(|| group.and_then(|key| state.groups.barrier(key)))
                                    .filter(|end| *end > now)
                            })
                        })
                })
                .flatten();
            needed.push((
                id,
                axis,
                key,
                p.progress,
                p.end_ms,
                from,
                target_index,
                source_index,
                hold_end,
            ));
        }
        let results: Vec<_> = queries
            .iter()
            .map(|(projection, endpoints)| {
                let result = self.resolver.resolve_certified(
                    tree,
                    tree.layout_model_epoch,
                    Arc::clone(projection),
                    Arc::clone(&context),
                    endpoints,
                    measurer,
                );
                #[cfg(test)]
                let result = inspection::observe_query(
                    tree,
                    || {
                        let request = prepared
                            .iter()
                            .find(|(_, value)| Arc::ptr_eq(value, projection))
                            .map(|(key, _)| *key);
                        match request {
                            Some(RequestKey {
                                source_at: Some(_), ..
                            }) => inspection::QueryKind::Source,
                            Some(RequestKey {
                                scope: ProjectionScope::Release,
                                ..
                            }) => inspection::QueryKind::Release,
                            _ => inspection::QueryKind::Target,
                        }
                    },
                    projection,
                    result
                        .as_ref()
                        .map(|receipt| (***receipt).clone())
                        .map_err(Clone::clone),
                )
                .and(result);
                result
            })
            .collect::<Result<_, _>>()?;
        let next_targets = needed
            .iter()
            .map(|(id, axis, _, _, _, _, index, _, _)| {
                results[*index]
                    .get(&(*id, *axis))
                    .copied()
                    .map(|target| ((*id, *axis), target))
                    .ok_or(ProjectionError::MissingLayout(*id, *axis))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        // Decide finite consumers before checking potential feedback drivers.
        // An active, held or finishing consumer may change a loop's endpoint, invalidating
        // its old continuation but not its ordinary current-presentation retarget.
        let potential_feedback: HashSet<_> = coupled_candidates
            .values()
            .flat_map(|(_, drivers)| drivers.iter().copied())
            .collect();
        let deferred: ContinuedTracks = potential_feedback
            .iter()
            .filter_map(|key| continuations.remove(key).map(|track| (*key, track)))
            .collect();
        self.verify_continuations(tree, &mut continuations, &next_targets, measurer)?;
        let mut coupled: ContinuedTracks = coupled_candidates
            .iter()
            .filter(|(key, _)| !continuations.contains_key(key))
            .map(|(key, (track, _))| (*key, track.clone()))
            .collect();
        self.verify_continuations(tree, &mut coupled, &next_targets, measurer)?;
        let feedback: HashSet<_> = coupled
            .keys()
            .flat_map(|key| coupled_candidates[key].1.iter().copied())
            .collect();
        let mut independent_drivers: ContinuedTracks = deferred
            .into_iter()
            .filter(|(key, _)| !feedback.contains(key))
            .collect();
        self.verify_continuations(tree, &mut independent_drivers, &next_targets, measurer)?;
        continuations.extend(coupled);
        continuations.extend(independent_drivers);
        let context_release = if let Some(&index) = request_indices.get(&RequestKey {
            scope: ProjectionScope::Release,
            source_at: None,
        }) {
            self.context_release(tree, &context, &current, &results[index], measurer)?
        } else {
            HashSet::new()
        };
        // Preserve the clock facts actually used by finite endpoint queries.
        // A loop can continue to a different native goal later in this same
        // preparation; its published sample then differs from the forecast
        // that the finite owner's cached target was measured against.
        let forecast_owners: HashSet<_> = fields
            .iter()
            .filter_map(|&(id, axis, run, _, key)| {
                (group_for(id, key.mount, run).is_some() && !release_fields.contains(&(id, axis)))
                    .then_some((id, axis))
            })
            .collect();
        let forecast = (!needed.is_empty() && !forecast_owners.is_empty())
            .then(|| {
                fields
                    .iter()
                    .filter(|(_, _, run, _, _)| matches!(run.spec.repeat, AnimationRepeat::Loop))
                    .filter_map(|(id, axis, _, _, _)| {
                        self.tracks
                            .get(&(*id, *axis))
                            .map(|track| ((*id, *axis), Arc::clone(&track.input)))
                    })
                    .collect::<HashMap<_, _>>()
            })
            .filter(|tracks| !tracks.is_empty())
            .map(Arc::new);
        // Build/validate the whole replacement set before installing anything live.
        let mut updates: HashMap<_, _> = needed
            .into_iter()
            .map(
                |(id, axis, key, progress, end_ms, from, target_index, source_index, hold_end)| {
                    let to = results
                        .get(target_index)
                        .and_then(|values| values.get(&(id, axis)))
                        .copied()
                        .ok_or(ProjectionError::MissingLayout(id, axis))?;
                    let from = from
                        .or_else(|| {
                            source_index.and_then(|i| results.get(i)?.get(&(id, axis)).copied())
                        })
                        .ok_or(ProjectionError::MissingLayout(id, axis))?;
                    from.interpolate(to, 0.0)
                        .map_err(ProjectionError::InvalidSample)?;
                    if release_fields.contains(&(id, axis))
                        && !context_release.contains(&(id, axis))
                        && !transported_releases.contains(&(id, axis))
                        && !continuations.contains_key(&(id, axis))
                        && current
                            .get(&(id, axis))
                            .is_some_and(|sample| !sample.release_matches(to))
                    {
                        return Err(ProjectionError::ReleaseMismatch(id, axis));
                    }
                    let anchor = if current.contains_key(&(id, axis)) {
                        progress
                    } else {
                        0.0
                    };
                    let unchanged_goal = self.tracks.get(&(id, axis)).filter(|old| {
                        old.key == key
                            && old.model != tree.layout_model_epoch
                            && Arc::ptr_eq(&old.context, &context)
                            && old.to.release_matches(to)
                            && !transports
                                .iter()
                                .any(|(node, lane, ..)| *node == id && *lane == axis)
                    });
                    if let Some(old) = unchanged_goal {
                        let native = old.evidence.native(
                            old.model,
                            &old.projection,
                            &old.context,
                            (id, axis),
                            old.key.mount,
                        )?;
                        if !native.release_matches(old.to) {
                            return Err(ProjectionError::ReleaseMismatch(id, axis));
                        }
                    }
                    let (from, anchor, hold_interval) = continuations
                        .get(&(id, axis))
                        .map(|(old, _)| (old.from, old.anchor, old.hold_interval))
                        .or_else(|| {
                            unchanged_goal.map(|old| (old.from, old.anchor, old.hold_interval))
                        })
                        .unwrap_or((from, anchor, hold_end.map(|end| (now, end))));
                    let projection = queries
                        .get(target_index)
                        .map(|q| Arc::clone(&q.0))
                        .ok_or(ProjectionError::MissingLayout(id, axis))?;
                    Ok((
                        (id, axis),
                        Arc::new(Track {
                            forecast: if forecast_owners.contains(&(id, axis)) {
                                forecast.clone()
                            } else {
                                None
                            },
                            input: Arc::new(ClockInput {
                                evidence: Arc::clone(&results[target_index]),
                                key,
                                from,
                                to,
                                anchor,
                                end_ms,
                                hold_interval,
                                projection,
                                context: Arc::clone(&context),
                                model: tree.layout_model_epoch,
                                evaluated_at: now,
                            }),
                        }),
                    ))
                },
            )
            .collect::<Result<_, ProjectionError>>()?;
        if !prepared_roles.is_empty()
            && let Some(forecast) = forecast.as_ref()
        {
            let refreshed = forecast
                .iter()
                .map(|(key, input)| {
                    let input = if prepared_roles.contains(key) {
                        updates
                            .get(key)
                            .map(|track| Arc::clone(&track.input))
                            .unwrap_or_else(|| Arc::clone(input))
                    } else {
                        Arc::clone(input)
                    };
                    (*key, input)
                })
                .collect::<HashMap<_, _>>();
            if refreshed
                .iter()
                .any(|(key, input)| !Arc::ptr_eq(input, &forecast[key]))
            {
                let refreshed = Arc::new(refreshed);
                for (key, track) in &mut updates {
                    if forecast_owners.contains(key) {
                        Arc::make_mut(track).forecast = Some(Arc::clone(&refreshed));
                    }
                }
            }
        }
        let installed: Vec<_> = fields
            .iter()
            .map(|&(id, axis, run, p, _)| {
                let track = updates
                    .get(&(id, axis))
                    .or_else(|| self.tracks.get(&(id, axis)))
                    .ok_or(ProjectionError::MissingLayout(id, axis))?;
                Ok(((id, axis), track.sample(run, p, now)?))
            })
            .collect::<Result<_, ProjectionError>>()?;
        for (key, sample) in &installed {
            if feedback.contains(key)
                && current
                    .get(key)
                    .or_else(|| boundary_samples.get(key))
                    .is_none_or(|fp| !fp.release_matches(*sample))
            {
                return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
            }
        }
        // Continued foreign anchors can change their current sample after the
        // first release query. Certify sample removal against what this transaction
        // will actually install, not a provisional peer snapshot.
        if let Some(&index) = request_indices.get(&RequestKey {
            scope: ProjectionScope::Release,
            source_at: None,
        }) {
            let (release, endpoints) = &queries[index];
            let mut actual = (**release).clone();
            for &(key, fp) in &installed {
                if !release_fields.contains(&key) {
                    set_projection_sample(actual.nodes.entry(key.0).or_default(), key.1, fp);
                }
            }
            if actual != **release {
                let actual = Arc::new(actual);
                let result = self.resolver.resolve(
                    tree,
                    tree.layout_model_epoch,
                    Arc::clone(&actual),
                    Arc::clone(&context),
                    endpoints,
                    measurer,
                );
                #[cfg(test)]
                let result = inspection::observe_query(
                    tree,
                    || inspection::QueryKind::CurrentRelease,
                    &actual,
                    result,
                );
                let values = result?;
                for key in endpoints {
                    if values
                        .get(key)
                        .is_none_or(|fp| !fp.release_matches(results[index][key]))
                    {
                        return Err(ProjectionError::ReleaseMismatch(key.0, key.1));
                    }
                }
            }
        }
        #[cfg(test)]
        if let Some(cell) = tree.animation_inspection.as_ref() {
            let mut trace = cell.borrow_mut();
            let mut candidate = frozen.clone();
            for &((id, axis), sample) in &installed {
                set_projection_sample(candidate.nodes.entry(id).or_default(), axis, sample);
            }
            trace.candidate = Some(candidate);
        }
        for (&(id, axis), sample) in installed.iter().map(|(key, fp)| (key, *fp)) {
            crate::tree::layout::dimensions::validate_prepared_axis_sample(
                tree,
                &id,
                axis,
                Some(sample),
                prepared_roles.contains(&(id, axis)),
            )
            .map_err(ProjectionError::InvalidSample)?;
        }
        let receipt = if ready.is_empty() {
            None
        } else {
            let retiring = release_fields
                .iter()
                .map(|key| {
                    let track = updates
                        .get(key)
                        .or_else(|| self.tracks.get(key))
                        .ok_or(ProjectionError::MissingLayout(key.0, key.1))?;
                    Ok((
                        *key,
                        PreparedField {
                            key: track.key,
                            terminal: track.to,
                        },
                    ))
                })
                .collect::<Result<_, ProjectionError>>()?;
            Some(PreparedRelease::new(
                tree, runtime, self, now, ready, retiring,
            )?)
        };
        let active: HashSet<_> = installed.iter().map(|(key, _)| *key).collect();
        let removed = self
            .tracks
            .keys()
            .filter(|key| !active.contains(key) && tree.get(&key.0).is_some())
            .map(|key| (*key, None));
        let samples = installed
            .iter()
            .map(|&(key, fp)| (key, (!release_fields.contains(&key)).then_some(fp)))
            .chain(removed)
            .collect();
        for ((id, axis), _) in &installed {
            if !release_fields.contains(&(*id, *axis))
                && let Some(sample) = frame.samples.get_mut(id)
            {
                clear_length(&mut sample.attrs, *axis);
            }
        }
        frame.geometry = Some(GeometryDelta {
            frame_context: Some(context),
            prepared_roles,
            updates,
            active,
            samples,
            expected,
            receipt,
            identity: Arc::clone(&self.identity),
        });
        Ok(())
    }
}
fn add_query(
    queries: &mut Vec<BoundaryQuery>,
    indices: &mut HashMap<RequestKey, usize>,
    key: RequestKey,
    projection: Arc<Projection>,
    endpoint: (NodeId, Axis),
) -> usize {
    if let Some(&i) = indices.get(&key) {
        queries[i].1.push(endpoint);
        i
    } else {
        let i = queries.len();
        queries.push((projection, vec![endpoint]));
        indices.insert(key, i);
        i
    }
}

/// Ordinary pixel samples and full samples carried out of an earlier mixed
/// segment use the same clock proof. Full samples require a native pixel
/// substitution witness; matching visible sizes alone is never sufficient.
type ContinuedTracks = HashMap<(NodeId, Axis), (Arc<Track>, Arc<ClockContinuation>)>;
type PixelWitness = (Arc<Projection>, Vec<((NodeId, Axis), AxisFootprint)>);
#[derive(Clone, Debug)]
struct ClockContinuation {
    /// Consumer-specific independence candidate and its unmodified prior input.
    independent: Option<(Vec<NodeId>, Arc<Projection>)>,
    own_axis: Option<(NodeId, Axis)>,
    projection: Arc<Projection>,
    pixels: Vec<((NodeId, Axis), AxisFootprint)>,
    next: Option<PixelWitness>,
    foreign: Vec<((NodeId, Axis), Arc<ClockInput>)>,
}
fn numeric_context_continues(
    tree: &ElementTree,
    old: &Track,
    next: &Arc<Projection>,
    positions: &[(NodeId, u64, RunRef<'_>, timing::SegmentPosition)],
    now: Instant,
    releasing: bool,
    tracks: &HashMap<(NodeId, Axis), Arc<Track>>,
) -> Option<Arc<ClockContinuation>> {
    if old.projection.nodes.len() != next.nodes.len() {
        return None;
    }
    let replacements: Vec<_> = old
        .projection
        .nodes
        .iter()
        .map(|(id, before)| {
            let after = next.nodes.get(id)?;
            if before == after {
                return Some(None);
            }
            if before.model != after.model {
                return None;
            }
            positions
                .iter()
                .filter(|(node, _, _, _)| node == id)
                .find_map(|(_, _, run, p)| {
                    if run.owner != groups::Owner::Regular || run.started > old.evaluated_at {
                        return None;
                    }
                    let entry = AnimationRuntimeEntry {
                        spec_hash: run.revision,
                        started_at: run.started,
                    };
                    let previous =
                        timing::position(run.spec, Some(&entry), Some(old.evaluated_at))?;
                    let continuous = previous.index == p.index && previous.start_ms == p.start_ms;
                    if !continuous
                        && (!releasing
                            || [Axis::Width, Axis::Height]
                                .into_iter()
                                .any(|axis| tracks.contains_key(&(*id, axis)))
                            || before.width.is_some()
                            || before.height.is_some()
                            || after.width.is_some()
                            || after.height.is_some()
                            || run.source.is_some_and(|source| {
                                source.sampled.into_iter().any(|sampled| sampled)
                            }))
                    {
                        return None;
                    }
                    // A moving anchor requires the same interval. At finite release,
                    // ordinary scalar pixel inputs may cross a boundary or skip
                    // cycles: validate both selected intervals and the cached native
                    // destination, then publish the new input without rewinding it.
                    // Retained mixed samples need additional source-boundary evidence;
                    // they are not admitted by this scalar-only release exception.
                    let selected =
                        |index| layout_attrs(&run.fields.select(&run.spec.keyframes[index]));
                    let endpoints = [p.index, p.index + 1].map(selected);
                    let previous_endpoints =
                        (!continuous).then(|| [previous.index, previous.index + 1].map(selected));
                    if !endpoints
                        .iter()
                        .chain(previous_endpoints.iter().flatten())
                        .all(|attrs| {
                            attrs.width.is_some() == endpoints[0].width.is_some()
                                && attrs.height.is_some() == endpoints[0].height.is_some()
                                && attrs
                                    .width
                                    .iter()
                                    .chain(attrs.height.iter())
                                    .all(|length| matches!(length, Length::Px(_)))
                                && *attrs
                                    == Attrs {
                                        width: attrs.width.clone(),
                                        height: attrs.height.clone(),
                                        ..Default::default()
                                    }
                        })
                    {
                        return None;
                    }
                    let sample = |time| {
                        layout_attrs(&run.fields.select(
                            &sample_animation_spec(run.spec, Some(&entry), Some(time)).attrs,
                        ))
                    };
                    let from = sample(old.evaluated_at);
                    let to = sample(now);
                    let mut expected_before = before.clone();
                    let mut expected_after = before.clone();
                    apply_sample_attrs(&mut expected_before.attrs, &from);
                    apply_sample_attrs(&mut expected_after.attrs, &to);
                    let mut native = before.clone();
                    let mut native_next = after.clone();
                    let pixels = [Axis::Width, Axis::Height]
                        .into_iter()
                        .map(|axis| {
                            let field = if axis == Axis::Width {
                                super::change::Field::Width
                            } else {
                                super::change::Field::Height
                            };
                            if !run
                                .fields
                                .intersects(super::fields::FieldMask::field(field))
                            {
                                return Some(None);
                            }
                            let (a, b) = if axis == Axis::Width {
                                (before.width, after.width)
                            } else {
                                (before.height, after.height)
                            };
                            let Some(prototype) = a.or(b) else {
                                return Some(None);
                            };
                            let (Some(Length::Px(from)), Some(Length::Px(to))) =
                                (length(&from, axis), length(&to, axis))
                            else {
                                return None;
                            };
                            let old_pixel = prototype.pixel_candidate(tree, *id, *from as f32)?;
                            let new_pixel = prototype.pixel_candidate(tree, *id, *to as f32)?;
                            if a.is_some_and(|a| !a.release_matches(old_pixel))
                                || b.is_some_and(|b| !b.release_matches(new_pixel))
                            {
                                return None;
                            }
                            if a.is_some() {
                                clear_length(&mut expected_before.attrs, axis);
                            }
                            if b.is_some() {
                                clear_length(&mut expected_after.attrs, axis);
                            }
                            match axis {
                                Axis::Width => {
                                    expected_before.width = a;
                                    expected_after.width = b;
                                    native.width = None;
                                    native.attrs.width = Some(Length::Px(*from));
                                    native_next.width = None;
                                    native_next.attrs.width = Some(Length::Px(*to));
                                }
                                Axis::Height => {
                                    expected_before.height = a;
                                    expected_after.height = b;
                                    native.height = None;
                                    native.attrs.height = Some(Length::Px(*from));
                                    native_next.height = None;
                                    native_next.attrs.height = Some(Length::Px(*to));
                                }
                            }
                            Some(Some((
                                a.map(|_| ((*id, axis), old_pixel)),
                                b.map(|_| ((*id, axis), new_pixel)),
                            )))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    (expected_before == *before && expected_after == *after).then(|| {
                        let (pixels, next_pixels): (Vec<_>, Vec<_>) =
                            pixels.into_iter().flatten().unzip();
                        // Sparse replacements only; unchanged or scalar-only nodes
                        // do not clone the shared projection tables.
                        (native != *before || native_next != *after).then(|| {
                            Box::new((
                                *id,
                                native,
                                native_next,
                                pixels.into_iter().flatten().collect::<Vec<_>>(),
                                next_pixels.into_iter().flatten().collect::<Vec<_>>(),
                            ))
                        })
                    })
                })
        })
        // Discard successful no-op nodes without an O(projection-size) buffer.
        .filter_map(|candidate| candidate.ok_or(()).transpose())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let (before_changes, after_changes, pixels, next_pixels) = replacements.into_iter().fold(
        (HashMap::new(), HashMap::new(), Vec::new(), Vec::new()),
        |(mut before_changes, mut after_changes, mut pixels, mut next_pixels), replacement| {
            let (id, before, after, old_witnesses, new_witnesses) = *replacement;
            if !old_witnesses.is_empty() {
                before_changes.insert(id, before);
            }
            if !new_witnesses.is_empty() {
                after_changes.insert(id, after);
            }
            pixels.extend(old_witnesses);
            next_pixels.extend(new_witnesses);
            (before_changes, after_changes, pixels, next_pixels)
        },
    );
    let replace = |base: &Arc<Projection>, changes: HashMap<NodeId, NodeProjection>| {
        if changes.is_empty() {
            Arc::clone(base)
        } else {
            Arc::new(Projection {
                nodes: base
                    .nodes
                    .iter()
                    .map(|(id, node)| (*id, changes.get(id).unwrap_or(node).clone()))
                    .collect(),
            })
        }
    };
    let projection = replace(&old.projection, before_changes);
    let next = (!next_pixels.is_empty()).then(|| (replace(next, after_changes), next_pixels));
    Some(Arc::new(ClockContinuation {
        independent: None,
        own_axis: None,
        projection,
        pixels,
        next,
        foreign: Vec::new(),
    }))
}

pub(super) fn layout_attrs(attrs: &Attrs) -> Attrs {
    Attrs {
        width: attrs.width.clone(),
        height: attrs.height.clone(),
        align_x: attrs.align_x,
        align_y: attrs.align_y,
        padding: attrs.padding.clone(),
        border_width: attrs.border_width.clone(),
        spacing: attrs.spacing,
        spacing_x: attrs.spacing_x,
        spacing_y: attrs.spacing_y,
        layout_scale: attrs.layout_scale,
        layout_rotate: attrs.layout_rotate,
        font_size: attrs.font_size,
        font_letter_spacing: attrs.font_letter_spacing,
        font_word_spacing: attrs.font_word_spacing,
        ..Attrs::default()
    }
}
mod continuation;
use continuation::footprint_context_continues;
#[cfg(test)]
mod tests;

/// Read-only probe counters, outside the timed publication path. Projection slots
/// count each retained Arc once, including the private workspace's last query.
#[cfg(feature = "bench-diagnostics")]
pub fn diagnostics_for_benchmark(tree: &ElementTree) -> String {
    let Some(state) = tree.length_runtime.as_ref() else {
        return format!(
            "workspace=false tracks=0 projections=0 projection_slots=0 forecast_sets=0 forecast_inputs=0 native_receipts=0 receipt_goals=0 registry_affects_visits={} retired_queries={:?}",
            tree.registry_affects_visit_count(),
            tree.animation_query_retirement
        );
    };
    let forecasts = state
        .tracks
        .values()
        .filter_map(|track| track.forecast.as_ref())
        .map(|inputs| (Arc::as_ptr(inputs), inputs.len()))
        .collect::<HashMap<_, _>>();
    let receipts = state
        .tracks
        .values()
        .flat_map(|track| {
            std::iter::once(&track.input)
                .chain(track.forecast.iter().flat_map(|inputs| inputs.values()))
        })
        .map(|input| (Arc::as_ptr(&input.evidence), input.evidence.len()))
        .collect::<HashMap<_, _>>();
    let projections = state
        .tracks
        .values()
        .flat_map(|track| {
            std::iter::once(&track.projection).chain(
                track
                    .forecast
                    .iter()
                    .flat_map(|inputs| inputs.values().map(|input| &input.projection)),
            )
        })
        .chain(state.resolver.retained_projections())
        .map(|projection| (Arc::as_ptr(projection), projection.nodes.len()))
        .collect::<HashMap<_, _>>();
    format!(
        "workspace=true tracks={} projections={} projection_slots={} forecast_sets={} forecast_inputs={} native_receipts={} receipt_goals={} queries={:?} registry_affects_visits={} retired_queries={:?}",
        state.tracks.len(),
        projections.len(),
        projections.values().sum::<usize>(),
        forecasts.len(),
        forecasts.values().sum::<usize>(),
        receipts.len(),
        receipts.values().sum::<usize>(),
        state.resolver.stats(),
        tree.registry_affects_visit_count(),
        tree.animation_query_retirement
    )
}
