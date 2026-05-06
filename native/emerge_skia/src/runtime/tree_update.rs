use std::{collections::HashMap, sync::Arc, time::Instant};

use crate::{
    actors::{AnimationFrameTraceSeed, AnimationPulseTrace, TreeMsg},
    assets,
    events::RegistryRebuildPayload,
    stats::{RendererStatsCollector, earliest_pipeline_instant},
    tree::{
        animation::AnimationRuntime,
        element::ElementTree,
        invalidation::{
            RefreshAvailability, RefreshDecision, TreeInvalidation, decide_refresh_action,
        },
        layout::{
            FrameAttrsPreparation, LayoutOutput, layout_and_refresh_default,
            layout_and_refresh_prepared_default, prepare_animation_frame_attrs_for_update,
            prepare_frame_attrs_for_update, prepared_root_has_frame,
            refresh_prepared_default_reusing_clean_registry, refresh_reusing_clean_registry,
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeUpdateDecodePolicy {
    LogAndContinue,
    ReturnErr,
}

pub struct TreeUpdateOptions<'a> {
    pub stats: Option<&'a Arc<RendererStatsCollector>>,
    pub decode_policy: TreeUpdateDecodePolicy,
}

impl<'a> TreeUpdateOptions<'a> {
    pub fn new(
        stats: Option<&'a Arc<RendererStatsCollector>>,
        decode_policy: TreeUpdateDecodePolicy,
    ) -> Self {
        Self {
            stats,
            decode_policy,
        }
    }
}

pub enum TreeUpdateEffect {
    Stop,
    Skip,
    RegistryUpdate {
        rebuild: RegistryRebuildPayload,
    },
    Layout {
        output: Box<LayoutOutput>,
        pipeline_submitted_at: Option<Instant>,
        tree_batch_started_at: Instant,
        animation_trace: Option<AnimationFrameTraceSeed>,
    },
}

pub struct TreeUpdateEngine {
    tree: ElementTree,
    width: f32,
    height: f32,
    scale: f32,
    cached_rebuild: Option<RegistryRebuildPayload>,
    animation_runtime: AnimationRuntime,
    latest_animation_sample_time: Option<Instant>,
}

impl TreeUpdateEngine {
    pub fn new(initial_tree: ElementTree, initial_width: u32, initial_height: u32) -> Self {
        Self {
            tree: initial_tree,
            width: (initial_width as f32).max(1.0),
            height: (initial_height as f32).max(1.0),
            scale: 1.0,
            cached_rebuild: None,
            animation_runtime: AnimationRuntime::default(),
            latest_animation_sample_time: None,
        }
    }

    pub fn tree(&self) -> &ElementTree {
        &self.tree
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn size(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    pub fn animation_runtime_is_empty(&self) -> bool {
        self.animation_runtime.is_empty()
    }

    pub fn process_messages(
        &mut self,
        messages: Vec<TreeMsg>,
        options: TreeUpdateOptions<'_>,
    ) -> Result<TreeUpdateEffect, String> {
        let mut flat = Vec::new();
        messages
            .into_iter()
            .for_each(|msg| push_tree_message_flat(msg, &mut flat));

        if flat.is_empty() {
            return Ok(TreeUpdateEffect::Skip);
        }

        let tree_batch_started_at = Instant::now();
        let mut scroll_acc = HashMap::new();
        let mut thumb_drag_x_acc = HashMap::new();
        let mut thumb_drag_y_acc = HashMap::new();
        let mut hover_x_state = HashMap::new();
        let mut hover_y_state = HashMap::new();
        let mut mouse_over_active_state = HashMap::new();
        let mut mouse_down_active_state = HashMap::new();
        let mut focused_active_state = HashMap::new();
        let mut patch_processing_started_ats = Vec::new();
        let mut pipeline_submitted_at = None;
        let mut invalidation = TreeInvalidation::None;
        let mut registry_requested = false;
        let mut animation_sample_time = self.latest_animation_sample_time;
        let mut animation_trace_previous_sample_time = animation_sample_time;
        let mut animation_trace_pulse: Option<(AnimationPulseTrace, Instant, Instant)> = None;
        let mut animation_presented_at = None;
        let mut animation_predicted_next_present_at = None;
        let mut animation_sample_requested = false;

        for message in flat {
            match message {
                TreeMsg::Stop => return Ok(TreeUpdateEffect::Stop),
                TreeMsg::Batch(_) => {
                    unreachable!("tree batches must be flattened before processing")
                }
                TreeMsg::UploadTree {
                    bytes,
                    submitted_at,
                } => {
                    pipeline_submitted_at =
                        earliest_pipeline_instant(pipeline_submitted_at, submitted_at);
                    match crate::tree::deserialize::decode_tree(&bytes) {
                        Ok(decoded) => {
                            self.tree.replace_with_uploaded(decoded);
                            invalidation.add(TreeInvalidation::Structure);
                        }
                        Err(err) => {
                            handle_message_error(
                                options.decode_policy,
                                format!("tree upload failed: {err}"),
                            )?;
                        }
                    }
                }
                TreeMsg::PatchTree {
                    bytes,
                    submitted_at,
                } => {
                    pipeline_submitted_at =
                        earliest_pipeline_instant(pipeline_submitted_at, submitted_at);
                    let patch_started_at = Instant::now();
                    let patches = match crate::tree::patch::decode_patches(&bytes) {
                        Ok(patches) => patches,
                        Err(err) => {
                            record_patch_process_stat(options.stats, patch_started_at);
                            handle_message_error(
                                options.decode_policy,
                                format!("tree patch decode failed: {err}"),
                            )?;
                            continue;
                        }
                    };
                    match crate::tree::patch::apply_patches(&mut self.tree, patches) {
                        Ok(patch_invalidation) => {
                            invalidation.add(patch_invalidation);
                        }
                        Err(err) => {
                            record_patch_process_stat(options.stats, patch_started_at);
                            handle_message_error(
                                options.decode_policy,
                                format!("tree patch apply failed: {err}"),
                            )?;
                            continue;
                        }
                    }
                    patch_processing_started_ats.push(patch_started_at);
                }
                TreeMsg::Resize {
                    width,
                    height,
                    scale,
                } => {
                    self.width = width.max(1.0);
                    self.height = height.max(1.0);
                    self.scale = scale;
                    invalidation.add(TreeInvalidation::Measure);
                }
                TreeMsg::ScrollRequest { element_id, dx, dy } => {
                    let entry = scroll_acc.entry(element_id).or_insert((0.0, 0.0));
                    entry.0 += dx;
                    entry.1 += dy;
                }
                TreeMsg::ScrollbarThumbDragX { element_id, dx } => {
                    let entry = thumb_drag_x_acc.entry(element_id).or_insert(0.0);
                    *entry += dx;
                }
                TreeMsg::ScrollbarThumbDragY { element_id, dy } => {
                    let entry = thumb_drag_y_acc.entry(element_id).or_insert(0.0);
                    *entry += dy;
                }
                TreeMsg::SetScrollbarXHover {
                    element_id,
                    hovered,
                } => {
                    hover_x_state.insert(element_id, hovered);
                }
                TreeMsg::SetScrollbarYHover {
                    element_id,
                    hovered,
                } => {
                    hover_y_state.insert(element_id, hovered);
                }
                TreeMsg::SetMouseOverActive { element_id, active } => {
                    crate::debug_trace::hover_trace!(
                        "tree_msg",
                        "set_mouse_over_active id={:?} active={}",
                        element_id.0,
                        active
                    );
                    mouse_over_active_state.insert(element_id, active);
                }
                TreeMsg::SetMouseDownActive { element_id, active } => {
                    mouse_down_active_state.insert(element_id, active);
                }
                TreeMsg::SetFocusedActive { element_id, active } => {
                    focused_active_state.insert(element_id, active);
                }
                TreeMsg::SetTextInputContent {
                    element_id,
                    content,
                } => {
                    invalidation.add(self.tree.set_text_input_content(&element_id, content));
                }
                TreeMsg::SetTextInputRuntime {
                    element_id,
                    focused,
                    cursor,
                    selection_anchor,
                    preedit,
                    preedit_cursor,
                } => {
                    invalidation.add(self.tree.set_text_input_runtime(
                        &element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    ));
                }
                TreeMsg::AnimationPulse {
                    presented_at,
                    predicted_next_present_at,
                    trace,
                } => {
                    crate::debug_trace::hover_trace!(
                        "tree_pulse",
                        "presented_at={:?} predicted_next={:?}",
                        presented_at,
                        predicted_next_present_at
                    );
                    animation_trace_previous_sample_time = animation_sample_time;
                    animation_sample_time = Some(animation_pulse_sample_time(
                        animation_sample_time,
                        presented_at,
                        predicted_next_present_at,
                    ));
                    if let Some(trace) = trace {
                        animation_trace_pulse =
                            Some((trace, presented_at, predicted_next_present_at));
                    }
                    animation_presented_at = Some(presented_at);
                    animation_predicted_next_present_at = Some(predicted_next_present_at);
                    animation_sample_requested = true;
                }
                TreeMsg::RebuildRegistry => {
                    registry_requested = true;
                }
                TreeMsg::AssetStateChanged => {
                    invalidation.add(TreeInvalidation::Measure);
                }
            }
        }

        if let (Some(stats), Some(submitted_at)) = (options.stats, pipeline_submitted_at) {
            stats.record_pipeline_submit_to_tree_start(submitted_at, tree_batch_started_at);
        }

        for (id, (dx, dy)) in scroll_acc {
            invalidation.add(self.tree.apply_scroll(&id, dx, dy));
        }
        for (id, dx) in thumb_drag_x_acc {
            invalidation.add(self.tree.apply_scroll_x(&id, dx));
        }
        for (id, dy) in thumb_drag_y_acc {
            invalidation.add(self.tree.apply_scroll_y(&id, dy));
        }
        for (id, hovered) in hover_x_state {
            invalidation.add(self.tree.set_scrollbar_x_hover(&id, hovered));
        }
        for (id, hovered) in hover_y_state {
            invalidation.add(self.tree.set_scrollbar_y_hover(&id, hovered));
        }
        for (id, active) in &mouse_over_active_state {
            invalidation.add(self.tree.set_mouse_over_active(id, *active));
        }
        for (id, active) in mouse_down_active_state {
            invalidation.add(self.tree.set_mouse_down_active(&id, active));
        }
        for (id, active) in focused_active_state {
            invalidation.add(self.tree.set_focused_active(&id, active));
        }

        let update_started_at = Instant::now();
        let mut plan = FrameUpdatePlan::new(invalidation);
        let should_sync_animations = animation_sample_requested
            || !self.animation_runtime.is_empty()
            || plan.invalidation.requires_recompute();
        let sample_time =
            should_sync_animations.then(|| animation_sample_time.unwrap_or_else(Instant::now));
        let had_animation_runtime = !self.animation_runtime.is_empty();
        let had_transient_animations = self.animation_runtime.has_transient_entries();

        if let Some(sample_time) = sample_time {
            if animation_sample_requested && let Some(presented_at) = animation_presented_at {
                self.animation_runtime
                    .anchor_pending_transient_entries_to_present(presented_at);
            }
            self.latest_animation_sample_time = Some(sample_time);
            crate::debug_trace::hover_trace!(
                "tree_plan",
                "sample_time={:?} cached_rebuild={} invalidation={:?} registry_requested={}",
                sample_time,
                self.cached_rebuild.is_some(),
                plan.invalidation,
                registry_requested
            );
            self.animation_runtime
                .sync_with_tree(&self.tree, sample_time);
            if self
                .animation_runtime
                .prune_completed_exit_ghosts(&mut self.tree, Some(sample_time))
            {
                plan.invalidation.add(TreeInvalidation::Structure);
            }
        }

        let should_prepare_frame = plan.invalidation.is_dirty()
            || animation_sample_requested
            || !self.animation_runtime.is_empty();

        if should_prepare_frame {
            self.tree.set_layout_cache_stats_enabled(
                options
                    .stats
                    .is_some_and(|stats| stats.layout_cache_enabled()),
            );
            let preparation = if animation_sample_requested
                && !plan.invalidation.is_dirty()
                && !self.animation_runtime.is_empty()
                && !had_transient_animations
            {
                prepare_animation_frame_attrs_for_update(
                    &mut self.tree,
                    self.scale,
                    &self.animation_runtime,
                    sample_time,
                )
            } else {
                prepare_frame_attrs_for_update(
                    &mut self.tree,
                    self.scale,
                    (!self.animation_runtime.is_empty()).then_some(&self.animation_runtime),
                    sample_time,
                )
            };
            let dynamic_invalidation = preparation.animation_result.invalidation;
            plan.animations_active = preparation.animation_result.active;
            plan.invalidation.add(dynamic_invalidation);

            if animation_sample_requested
                && had_animation_runtime
                && !plan.animations_active
                && dynamic_invalidation.is_none()
            {
                plan.invalidation.add(TreeInvalidation::Paint);
            }

            plan.preparation = Some(preparation);
        }

        plan.action = decide_refresh_action(
            plan.invalidation,
            registry_requested,
            RefreshAvailability {
                has_cached_rebuild: self.cached_rebuild.is_some(),
                has_root_frame: plan.preparation.as_ref().map_or_else(
                    || tree_has_root_frame(&self.tree),
                    |preparation| prepared_root_has_frame(&self.tree, preparation),
                ),
            },
        );
        let animation_trace = sample_time
            .filter(|_| plan.animations_active || animation_sample_requested)
            .map(|sample_time| {
                let (pulse, presented_at, predicted_next_present_at) = animation_trace_pulse
                    .map_or(
                        (
                            None,
                            animation_presented_at,
                            animation_predicted_next_present_at,
                        ),
                        |(trace, presented_at, predicted_next_present_at)| {
                            (
                                Some(trace),
                                Some(presented_at),
                                Some(predicted_next_present_at),
                            )
                        },
                    );

                AnimationFrameTraceSeed {
                    sequence: pulse.map(|trace| trace.sequence),
                    pulse_sent_at: pulse.map(|trace| trace.sent_at),
                    tree_started_at: tree_batch_started_at,
                    presented_at,
                    predicted_next_present_at,
                    sample_time,
                    previous_sample_time: animation_trace_previous_sample_time,
                    animations_active: plan.animations_active,
                    pulse_requested_sample: animation_sample_requested,
                }
            });

        let effect = match plan.action {
            RefreshDecision::Skip => {
                self.clear_latest_sample_time_if_inactive(plan.animations_active);
                TreeUpdateEffect::Skip
            }
            RefreshDecision::UseCachedRebuild => {
                self.clear_latest_sample_time_if_inactive(plan.animations_active);
                self.cached_rebuild
                    .clone()
                    .map_or(TreeUpdateEffect::Skip, |rebuild| {
                        TreeUpdateEffect::RegistryUpdate { rebuild }
                    })
            }
            RefreshDecision::RefreshOnly => {
                assets::ensure_tree_sources(&self.tree);
                let update = if let Some(preparation) = plan.preparation {
                    refresh_prepared_default_reusing_clean_registry(
                        &mut self.tree,
                        preparation,
                        self.cached_rebuild.as_ref(),
                    )
                } else {
                    self.tree.set_layout_cache_stats_enabled(
                        options
                            .stats
                            .is_some_and(|stats| stats.layout_cache_enabled()),
                    );
                    self.tree.reset_layout_cache_stats();
                    let output = refresh_reusing_clean_registry(
                        &mut self.tree,
                        self.cached_rebuild.as_ref(),
                    );
                    crate::tree::layout::LayoutUpdateOutput {
                        output,
                        layout_performed: false,
                    }
                };

                if let Some(stats) = options.stats {
                    stats.record_refresh(update_started_at.elapsed());
                }

                self.layout_effect(
                    update.output,
                    pipeline_submitted_at,
                    tree_batch_started_at,
                    animation_trace,
                )
            }
            RefreshDecision::Recompute => {
                assets::ensure_tree_sources(&self.tree);

                let constraint = crate::tree::layout::Constraint::new(self.width, self.height);
                let update = if let Some(preparation) = plan.preparation {
                    layout_and_refresh_prepared_default(&mut self.tree, constraint, preparation)
                } else {
                    self.tree.set_layout_cache_stats_enabled(
                        options
                            .stats
                            .is_some_and(|stats| stats.layout_cache_enabled()),
                    );
                    let output = layout_and_refresh_default(&mut self.tree, constraint, self.scale);
                    crate::tree::layout::LayoutUpdateOutput {
                        output,
                        layout_performed: true,
                    }
                };

                if let Some(stats) = options.stats {
                    if update.layout_performed {
                        stats.record_layout(update_started_at.elapsed());
                        stats.record_layout_cache(self.tree.layout_cache_stats());
                    } else {
                        stats.record_refresh(update_started_at.elapsed());
                    }
                }

                self.layout_effect(
                    update.output,
                    pipeline_submitted_at,
                    tree_batch_started_at,
                    animation_trace,
                )
            }
        };

        record_patch_process_stats(options.stats, patch_processing_started_ats);
        Ok(effect)
    }

    fn layout_effect(
        &mut self,
        output: LayoutOutput,
        pipeline_submitted_at: Option<Instant>,
        tree_batch_started_at: Instant,
        animation_trace: Option<AnimationFrameTraceSeed>,
    ) -> TreeUpdateEffect {
        let animations_active = output.animations_active;
        if output.event_rebuild_changed {
            self.cached_rebuild.replace(output.event_rebuild.clone());
        }
        trace_tree_snapshots(&self.tree);
        self.clear_latest_sample_time_if_inactive(animations_active);
        TreeUpdateEffect::Layout {
            output: Box::new(output),
            pipeline_submitted_at,
            tree_batch_started_at,
            animation_trace,
        }
    }

    fn clear_latest_sample_time_if_inactive(&mut self, animations_active: bool) {
        if self.animation_runtime.is_empty() || !animations_active {
            self.latest_animation_sample_time = None;
        }
    }
}

pub fn animation_pulse_sample_time(
    previous_sample_time: Option<Instant>,
    presented_at: Instant,
    predicted_next_present_at: Instant,
) -> Instant {
    let sample_time = predicted_next_present_at.max(presented_at);
    previous_sample_time.map_or(sample_time, |previous| sample_time.max(previous))
}

#[derive(Debug)]
struct FrameUpdatePlan {
    invalidation: TreeInvalidation,
    animations_active: bool,
    action: RefreshDecision,
    preparation: Option<FrameAttrsPreparation>,
}

impl FrameUpdatePlan {
    fn new(invalidation: TreeInvalidation) -> Self {
        Self {
            invalidation,
            animations_active: false,
            action: RefreshDecision::Skip,
            preparation: None,
        }
    }
}

fn tree_has_root_frame(tree: &ElementTree) -> bool {
    tree.root_id()
        .and_then(|root_id| tree.get(&root_id).and_then(|element| element.layout.frame))
        .is_some()
}

fn handle_message_error(policy: TreeUpdateDecodePolicy, message: String) -> Result<(), String> {
    match policy {
        TreeUpdateDecodePolicy::LogAndContinue => {
            eprintln!("{message}");
            Ok(())
        }
        TreeUpdateDecodePolicy::ReturnErr => Err(message),
    }
}

fn record_patch_process_stat(
    stats: Option<&Arc<RendererStatsCollector>>,
    patch_processing_started_at: Instant,
) {
    if let Some(stats) = stats {
        stats.record_patch_tree_process(patch_processing_started_at.elapsed());
    }
}

fn record_patch_process_stats(
    stats: Option<&Arc<RendererStatsCollector>>,
    patch_processing_started_ats: Vec<Instant>,
) {
    patch_processing_started_ats
        .into_iter()
        .for_each(|started_at| record_patch_process_stat(stats, started_at));
}

pub(crate) fn push_tree_message_flat(msg: TreeMsg, out: &mut Vec<TreeMsg>) {
    match msg {
        TreeMsg::Batch(messages) => messages
            .into_iter()
            .for_each(|nested| push_tree_message_flat(nested, out)),
        other => out.push(other),
    }
}

#[cfg(feature = "hover-trace")]
fn trace_tree_snapshots(tree: &ElementTree) {
    for (id, x, y, w, h, move_x) in trace_element_snapshots(tree) {
        crate::debug_trace::hover_trace!(
            "tree_snapshot",
            "id={:?} frame=({x:.2},{y:.2},{w:.2},{h:.2}) move_x={:.2} visual_x={:.2}",
            id.0,
            move_x.unwrap_or(0.0),
            x + move_x.unwrap_or(0.0) as f32
        );
    }
}

#[cfg(not(feature = "hover-trace"))]
fn trace_tree_snapshots(_tree: &ElementTree) {}

#[cfg(feature = "hover-trace")]
fn trace_element_snapshots(
    tree: &ElementTree,
) -> Vec<(
    crate::tree::element::NodeId,
    f32,
    f32,
    f32,
    f32,
    Option<f64>,
)> {
    tree.iter_node_pairs()
        .filter_map(|(id, element)| {
            element.layout.frame.map(|frame| {
                (
                    *id,
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    element.layout.effective.move_x,
                )
            })
        })
        .collect()
}
