//! Deterministic driver around native preparation/layout/refresh, not an animation engine.
//! Oracle copies and retained observations exist only in tests.
use super::super::inspection::{Inspection, QueryKind};
use super::*;
use crate::events::registry_builder::assert_registry_rebuild_payloads_equivalent;
use crate::tree::{
    element::{Frame, NodeLayoutState, NodeRefreshState},
    invalidation::layout_model_attrs_changed,
    layout::{
        LayoutOutput, layout_and_refresh_prepared_default,
        prepare_animation_frame_attrs_for_update, prepare_dirty_frame_attrs_for_update,
        prepare_frame_attrs_for_update,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Full,
    Active,
    Dirty,
}
#[derive(Clone, Debug)]
pub(super) enum Event {
    Attrs(NodeId, Box<Attrs>),
    Viewport { width: f32, height: f32, scale: f32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Injection {
    None,
    Query(QueryKind),
    BeforeLayout,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AttemptError {
    Native(ProjectionError),
    BeforeLayout,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Pose {
    pub mount: u64,
    pub frame: Option<Frame>,
    pub render_frame: Option<Frame>,
    pub effective: Attrs,
    pub footprints: [Option<AxisFootprint>; 2],
    pub sampled: [bool; 2],
    pub scroll: [f32; 4],
}
pub(super) type Poses = HashMap<NodeId, Pose>;
pub(super) fn poses(tree: &ElementTree) -> Poses {
    tree.iter_nodes()
        .map(|node| {
            (
                node.id,
                Pose {
                    mount: node.lifecycle.mounted_at_revision,
                    frame: node.layout.frame,
                    render_frame: node.layout.render_frame,
                    effective: node.layout.effective.clone(),
                    footprints: [Axis::Width, Axis::Height]
                        .map(|axis| capture_axis(tree, &node.id, axis)),
                    sampled: [Axis::Width, Axis::Height].map(|axis| {
                        node.layout
                            .dimension_samples
                            .as_ref()
                            .is_some_and(|s| s.get(axis).is_some())
                    }),
                    scroll: [
                        node.layout.scroll_x,
                        node.layout.scroll_y,
                        node.layout.scroll_x_max,
                        node.layout.scroll_y_max,
                    ],
                },
            )
        })
        .collect()
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Source {
    mount: u64,
    model: u64,
    declared: Attrs,
    effective: Attrs,
    scale: f32,
    dimensions: [Option<AxisFootprint>; 2],
    sampled: [bool; 2],
}
impl From<&source::PresentationSource> for Source {
    fn from(s: &source::PresentationSource) -> Self {
        Self {
            mount: s.mounted_at,
            model: s.model,
            declared: s.declared.clone(),
            effective: s.effective.clone(),
            scale: s.scale,
            dimensions: s.dimensions,
            sampled: s.sampled,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Change {
    pub generation: RunGeneration,
    pub started: Instant,
    pub target: Attrs,
    pub pending: bool,
    pub phases: fields::PhaseMasks,
    pub source: Source,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RegularFields {
    fields: fields::FieldMask,
    pending: fields::FieldMask,
    source: Option<Source>,
    handoffs: Vec<(RunGeneration, Instant, fields::FieldMask, Option<Source>)>,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Owners {
    regular_fields: HashMap<NodeId, RegularFields>,
    pub regular: HashMap<NodeId, (RunGeneration, Instant, u64)>,
    pub enters: HashMap<NodeId, (RunGeneration, Instant)>,
    pub exits: HashMap<NodeId, (RunGeneration, Instant)>,
    pub changes: HashMap<(NodeId, change::Field), Change>,
    pub groups: usize,
    pub membership: HashMap<groups::GroupKey, groups::GroupObservation>,
    pub generation_cursor: u64,
}
impl Owners {
    fn capture(rt: &AnimationRuntime) -> Self {
        Self {
            regular_fields: rt
                .animate_entries
                .iter()
                .map(|(id, entry)| {
                    (
                        *id,
                        RegularFields {
                            fields: entry.fields,
                            pending: entry.pending,
                            source: entry.source.as_deref().map(Source::from),
                            handoffs: entry
                                .handoffs
                                .iter()
                                .map(|lane| {
                                    (
                                        lane.generation,
                                        lane.clock.started_at,
                                        lane.fields,
                                        lane.source.as_deref().map(Source::from),
                                    )
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            regular: rt
                .animate_entries
                .iter()
                .map(|(id, e)| (*id, (e.generation, e.clock.started_at, e.clock.spec_hash)))
                .collect(),
            enters: rt
                .enter_entries
                .iter()
                .map(|(id, e)| (*id, (e.generation, e.started_at)))
                .collect(),
            exits: rt
                .exit_entries
                .iter()
                .map(|(id, e)| (*id, (e.generation, e.started_at)))
                .collect(),
            changes: rt
                .changes
                .iter()
                .map(|(key, e)| {
                    (
                        *key,
                        Change {
                            generation: e.generation,
                            started: e.started_at,
                            target: e.spec.keyframes.last().unwrap().clone(),
                            pending: e.pending,
                            phases: rt.change_phases(
                                *key,
                                e,
                                rt.synced_sample_time
                                    .is_some_and(|now| entry_is_active(&e.spec, e.started_at, now)),
                                rt.synced_sample_time,
                            ),
                            source: Source::from(e.source.as_ref()),
                        },
                    )
                })
                .collect(),
            groups: rt.groups.len(),
            membership: rt.groups.observation(),
            generation_cursor: rt.last_generation,
        }
    }
}
#[derive(Clone)]
pub(super) struct Publication {
    pub sequence: u64,
    pub at: Duration,
    pub pose: Poses,
    pub owners: Owners,
    pub output: Arc<LayoutOutput>,
}
impl Publication {
    pub fn assert_same(&self, other: &Self) {
        assert_eq!(self.sequence, other.sequence);
        assert_eq!(self.at, other.at);
        assert_eq!(self.pose, other.pose);
        assert_eq!(self.owners, other.owners);
        assert_eq!(self.output.scene, other.output.scene);
        assert_eq!(
            self.output.animations_active,
            other.output.animations_active
        );
        assert_registry_rebuild_payloads_equivalent(
            &self.output.event_rebuild,
            &other.output.event_rebuild,
        );
    }
}
pub(super) struct Attempt {
    pub number: u64,
    pub at: Duration,
    pub events: Vec<Event>,
    pub revision: u64,
    pub mode: Mode,
    pub published_before: Option<Publication>,
    pub before_queries: Poses,
    pub owners_after_sync: Owners,
    pub after_prepare: Poses,
    pub sources_after_prepare: HashMap<NodeId, Source>,
    pub inspection: Inspection,
    pub with_samples: Option<Poses>,
    pub without_samples: Option<Poses>,
    pub after_layout: Option<Poses>,
    pub published_after: Option<Publication>,
    pub result: Result<bool, AttemptError>,
}
pub(super) struct Driver {
    pub tree: ElementTree,
    pub runtime: AnimationRuntime,
    pub mode: Mode,
    epoch: Instant,
    constraint: Constraint,
    scale: f32,
    attempts: u64,
    published: Option<Publication>,
}
impl Driver {
    pub fn new(mut tree: ElementTree, epoch: Instant, mode: Mode) -> Self {
        tree.animation_inspection = Some(std::cell::RefCell::new(Box::default()));
        Self {
            tree,
            runtime: AnimationRuntime::default(),
            mode,
            epoch,
            constraint: Constraint::new(600.0, 600.0),
            scale: 1.0,
            attempts: 0,
            published: None,
        }
    }
    pub fn step(&mut self, micros: u64, events: Vec<Event>) -> Attempt {
        self.attempt(micros, events, Injection::None)
    }
    pub fn attempt(&mut self, micros: u64, events: Vec<Event>, injection: Injection) -> Attempt {
        self.attempts += 1;
        let at = Duration::from_micros(micros);
        let now = self.epoch + at;
        let published_before = self.published.clone();
        let dirty: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::Attrs(id, attrs) => {
                    self.tree.capture_animation_source(id);
                    let node = self.tree.get_mut(id).unwrap();
                    let model_changed = layout_model_attrs_changed(&node.spec.declared, attrs);
                    node.spec.declared = (**attrs).clone();
                    if model_changed {
                        self.tree.layout_model_epoch += 1;
                    }
                    self.tree.set_revision(self.tree.revision() + 1);
                    self.tree.pending_patch_effects.model_changed |= model_changed;
                    self.tree
                        .pending_patch_effects
                        .invalidation
                        .add(TreeInvalidation::Measure);
                    self.tree.mark_measure_dirty(id);
                    Some(*id)
                }
                Event::Viewport {
                    width,
                    height,
                    scale,
                } => {
                    self.constraint = Constraint::new(*width, *height);
                    self.scale = *scale;
                    self.tree.mark_all_measure_dirty();
                    None
                }
            })
            .collect();
        self.tree.animation_constraint = Some(self.constraint);
        let before_queries = poses(&self.tree);
        let sync = self.runtime.sync_with_tree(&self.tree, now);
        for effect in sync.completed.effects {
            self.tree
                .mark_layout_dirty_for_invalidation(&effect.id, effect.invalidation);
        }
        let owners_after_sync = Owners::capture(&self.runtime);
        let mode = if self
            .tree
            .root_id()
            .and_then(|id| self.tree.get(&id))
            .is_none_or(|n| n.layout.frame.is_none())
        {
            Mode::Full
        } else if self.mode == Mode::Active
            && dirty
                .iter()
                .any(|id| !self.runtime.active_node_ids().contains(id))
        {
            Mode::Dirty
        } else {
            self.mode
        };
        self.tree
            .animation_inspection
            .as_ref()
            .unwrap()
            .borrow_mut()
            .fail_query = match injection {
            Injection::Query(kind) => Some(kind),
            _ => None,
        };
        let prep = match mode {
            Mode::Full => prepare_frame_attrs_for_update(
                &mut self.tree,
                self.scale,
                Some(&mut self.runtime),
                Some(now),
            ),
            Mode::Active => prepare_animation_frame_attrs_for_update(
                &mut self.tree,
                self.scale,
                &mut self.runtime,
                Some(now),
            ),
            Mode::Dirty => prepare_dirty_frame_attrs_for_update(
                &mut self.tree,
                self.scale,
                Some(&mut self.runtime),
                Some(now),
                &dirty,
            ),
        };
        let inspection = self
            .tree
            .animation_inspection
            .as_ref()
            .map(|cell| cell.borrow().as_ref().clone())
            .unwrap_or_default();
        let after_prepare = poses(&self.tree);
        let sources_after_prepare = self
            .tree
            .pending_patch_effects
            .sources
            .iter()
            .map(|(id, s)| (*id, Source::from(s)))
            .collect();
        let valid_queries = inspection.queries.iter().all(|q| q.result.is_ok());
        let with_samples = valid_queries.then_some(()).and_then(|_| {
            inspection
                .candidate
                .as_ref()
                .or(inspection.frozen.as_deref())
                .map(|p| native_pose(&self.tree, p, self.constraint, self.scale))
        });
        // Use the evaluator's actual combined release input, not a guessed endpoint.
        let without_samples = valid_queries.then_some(()).and_then(|_| {
            inspection
                .queries
                .iter()
                .find(|q| q.kind == QueryKind::Release)
                .map(|q| native_pose(&self.tree, &q.projection, self.constraint, self.scale))
        });
        let mut after_layout = None;
        let result = if let Some(error) = prep.animation_result.preparation_error.clone() {
            Err(AttemptError::Native(error))
        } else if injection == Injection::BeforeLayout {
            Err(AttemptError::BeforeLayout)
        } else {
            prep.apply(&mut self.tree, Some(&self.runtime))
                .map_err(AttemptError::Native)
                .and_then(|applied| {
                    let mut output = layout_and_refresh_prepared_default(
                        &mut self.tree,
                        self.constraint,
                        applied,
                    )
                    .output;
                    after_layout = Some(poses(&self.tree));
                    self.runtime
                        .finish_prepared_frame(&mut self.tree)
                        .map(|followup| {
                            output.animations_active |= followup;
                            let active = output.animations_active;
                            self.published = Some(Publication {
                                sequence: self.published.as_ref().map_or(1, |p| p.sequence + 1),
                                at,
                                pose: poses(&self.tree),
                                owners: Owners::capture(&self.runtime),
                                output: Arc::new(output),
                            });
                            active
                        })
                        .map_err(AttemptError::Native)
                })
        };
        Attempt {
            number: self.attempts,
            at,
            events,
            revision: self.tree.revision(),
            mode,
            published_before,
            before_queries,
            owners_after_sync,
            after_prepare,
            sources_after_prepare,
            inspection,
            with_samples,
            without_samples,
            after_layout,
            published_after: self.published.clone(),
            result,
        }
    }
    pub fn attrs(&self, id: NodeId) -> Attrs {
        self.tree.get(&id).unwrap().spec.declared.clone()
    }
}

/// Fresh ordinary-layout control. Copies/clears are deliberately test-only.
fn native_pose(
    tree: &ElementTree,
    projection: &Projection,
    constraint: Constraint,
    scale: f32,
) -> Poses {
    let mut probe = tree.clone();
    probe.animation_inspection = None;
    probe.length_runtime = None;
    probe.pending_patch_effects = Default::default();
    for node in probe.iter_nodes_mut() {
        node.layout = NodeLayoutState::default();
        node.refresh = NodeRefreshState::default();
        node.spec.declared.animate = None;
        node.spec.declared.animate_enter = None;
        node.spec.declared.animate_exit = None;
        node.spec.declared.animate_change = None;
        node.lifecycle.ghost_exit_animation = None;
        if let Some(p) = projection.nodes.get(&node.id) {
            apply_sample_attrs(&mut node.spec.declared, &p.attrs);
        }
    }
    // Resolve current native roles/scales before validating scoped samples (a
    // float is not identifiable from freshly reset effective attrs alone).
    crate::tree::layout::layout_tree_default(&mut probe, constraint, scale);
    for (id, node) in &projection.nodes {
        for (axis, value) in [(Axis::Width, node.width), (Axis::Height, node.height)] {
            if let Some(fp) = value {
                set_axis_sample(&mut probe, id, axis, Some(fp)).unwrap();
            }
        }
    }
    crate::tree::layout::layout_tree_default(&mut probe, constraint, scale);
    poses(&probe)
}
