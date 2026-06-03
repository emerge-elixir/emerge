use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    actors::{AnimationFrameTraceSeed, AnimationPulseTrace, TreeMsg},
    assets,
    events::RegistryRebuildPayload,
    stats::{RendererStatsCollector, earliest_pipeline_instant},
    tree::{
        animation::AnimationRuntime,
        element::{ElementTree, NodeId},
        invalidation::{
            RefreshAvailability, RefreshDecision, TreeInvalidation, decide_refresh_action,
        },
        layout::{
            FrameAttrsPreparation, LayoutOutput, layout_and_refresh_default,
            layout_and_refresh_prepared_default_reusing_clean_registry,
            layout_and_refresh_prepared_default_reusing_clean_registry_timed,
            mark_animation_effects_dirty_for_update, prepare_animation_frame_attrs_for_update,
            prepare_dirty_frame_attrs_with_subtrees_for_update, prepare_frame_attrs_for_update,
            prepared_root_has_frame, refresh_prepared_default_reusing_clean_registry,
            refresh_reusing_clean_registry,
        },
        patch::Patch,
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
        let mut frame_attr_dirty_ids = Vec::new();
        let mut frame_attr_dirty_subtree_roots = Vec::new();
        let mut frame_attr_dirty_ids_complete = true;
        let mut patch_processing_started_ats = Vec::new();
        let mut patch_decode_durations = Vec::new();
        let mut patch_apply_durations = Vec::new();
        let mut pipeline_submitted_at = None;
        let mut invalidation = TreeInvalidation::None;
        let mut registry_requested = false;
        let mut animation_sample_time = self.latest_animation_sample_time;
        let mut animation_trace_previous_sample_time = animation_sample_time;
        let mut animation_trace_pulse: Option<(AnimationPulseTrace, Instant, Instant)> = None;
        let mut animation_presented_at = None;
        let mut animation_predicted_next_present_at = None;
        let mut animation_sample_requested = false;
        let mut animation_sync_requested = false;

        for message in flat {
            registry_requested |= message.requires_listener_registry_response();

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
                    let decode_started_at = Instant::now();
                    let patches = match crate::tree::patch::decode_patches(&bytes) {
                        Ok(patches) => {
                            patch_decode_durations.push(decode_started_at.elapsed());
                            patches
                        }
                        Err(err) => {
                            patch_decode_durations.push(decode_started_at.elapsed());
                            record_patch_process_stat(options.stats, patch_started_at);
                            handle_message_error(
                                options.decode_policy,
                                format!("tree patch decode failed: {err}"),
                            )?;
                            continue;
                        }
                    };
                    animation_sync_requested |=
                        patches_may_start_animation_runtime(&self.tree, &patches);
                    let patch_frame_attr_dirty = patch_frame_attr_dirty_hints(&self.tree, &patches);
                    let apply_started_at = Instant::now();
                    let apply_result = crate::tree::patch::apply_patches(&mut self.tree, patches);
                    patch_apply_durations.push(apply_started_at.elapsed());
                    match apply_result {
                        Ok(patch_invalidation) => {
                            invalidation.add(patch_invalidation);
                            if patch_frame_attr_dirty.complete {
                                extend_frame_attr_dirty_ids(
                                    &mut frame_attr_dirty_ids,
                                    patch_frame_attr_dirty.ids,
                                );
                                extend_frame_attr_dirty_ids(
                                    &mut frame_attr_dirty_subtree_roots,
                                    patch_frame_attr_dirty.subtree_roots,
                                );
                            } else {
                                frame_attr_dirty_ids_complete = false;
                            }
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
                    let next_width = width.max(1.0);
                    let next_height = height.max(1.0);
                    let changed = (self.width - next_width).abs() > f32::EPSILON
                        || (self.height - next_height).abs() > f32::EPSILON
                        || (self.scale - scale).abs() > f32::EPSILON;

                    self.width = next_width;
                    self.height = next_height;
                    self.scale = scale;

                    if changed || self.cached_rebuild.is_none() {
                        invalidation.add(TreeInvalidation::Measure);
                    }
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
                    let update_invalidation =
                        self.tree.set_text_input_content(&element_id, content);
                    record_frame_attr_dirty_id(
                        &mut frame_attr_dirty_ids,
                        element_id,
                        update_invalidation,
                    );
                    invalidation.add(update_invalidation);
                }
                TreeMsg::SetTextInputRuntime {
                    element_id,
                    focused,
                    cursor,
                    selection_anchor,
                    preedit,
                    preedit_cursor,
                } => {
                    let update_invalidation = self.tree.set_text_input_runtime(
                        &element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    );
                    record_frame_attr_dirty_id(
                        &mut frame_attr_dirty_ids,
                        element_id,
                        update_invalidation,
                    );
                    invalidation.add(update_invalidation);
                }
                TreeMsg::SetSliderValue { element_id, value } => {
                    let update_invalidation = self.tree.set_slider_value(&element_id, value);
                    record_frame_attr_dirty_id(
                        &mut frame_attr_dirty_ids,
                        element_id,
                        update_invalidation,
                    );
                    invalidation.add(update_invalidation);
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
            let update_invalidation = self.tree.set_mouse_over_active(id, *active);
            record_frame_attr_dirty_id(&mut frame_attr_dirty_ids, *id, update_invalidation);
            invalidation.add(update_invalidation);
        }
        for (id, active) in mouse_down_active_state {
            let update_invalidation = self.tree.set_mouse_down_active(&id, active);
            record_frame_attr_dirty_id(&mut frame_attr_dirty_ids, id, update_invalidation);
            invalidation.add(update_invalidation);
        }
        for (id, active) in focused_active_state {
            let update_invalidation = self.tree.set_focused_active(&id, active);
            record_frame_attr_dirty_id(&mut frame_attr_dirty_ids, id, update_invalidation);
            invalidation.add(update_invalidation);
        }

        if tree_scale_changed(&self.tree, self.scale) {
            invalidation.add(TreeInvalidation::Measure);
        }

        let update_started_at = Instant::now();
        let patch_frame_active = !patch_processing_started_ats.is_empty();
        let mut patch_animation_sync_duration = None;
        let mut patch_prepare_duration = None;
        let mut patch_layout_duration = None;
        let mut patch_refresh_duration = None;
        let mut patch_refresh_traversal_duration = None;
        let mut patch_refresh_registry_post_duration = None;
        let mut plan = FrameUpdatePlan::new(invalidation);
        let should_sync_animations = animation_sample_requested
            || animation_sync_requested
            || !self.animation_runtime.is_empty()
            || plan.invalidation.requires_recompute();
        let sample_time =
            should_sync_animations.then(|| animation_sample_time.unwrap_or_else(Instant::now));
        let had_transient_animations = self.animation_runtime.has_transient_entries();

        if let Some(sample_time) = sample_time {
            let animation_sync_started_at = patch_frame_active.then(Instant::now);
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
            let animation_sync = self
                .animation_runtime
                .sync_with_tree(&self.tree, sample_time);
            if animation_sync.completed.invalidation.is_dirty() {
                mark_animation_effects_dirty_for_update(&mut self.tree, &animation_sync.completed);
                plan.invalidation.add(animation_sync.completed.invalidation);
            }
            if self
                .animation_runtime
                .prune_completed_exit_ghosts(&mut self.tree, Some(sample_time))
            {
                plan.invalidation.add(TreeInvalidation::Structure);
            }
            if let Some(started_at) = animation_sync_started_at {
                patch_animation_sync_duration = Some(started_at.elapsed());
            }
        }

        let should_prepare_frame = plan.invalidation.requires_recompute()
            || animation_sample_requested
            || !self.animation_runtime.is_empty()
            || (!frame_attr_dirty_ids.is_empty() && plan.invalidation.can_refresh_only())
            || (!frame_attr_dirty_subtree_roots.is_empty() && plan.invalidation.can_refresh_only())
            || (!frame_attr_dirty_ids_complete && plan.invalidation.can_refresh_only());

        if should_prepare_frame {
            self.tree.set_layout_cache_stats_enabled(
                options
                    .stats
                    .is_some_and(|stats| stats.layout_cache_enabled()),
            );
            let can_prepare_dirty_incrementally = plan.invalidation.can_refresh_only()
                && frame_attr_dirty_ids_complete
                && !had_transient_animations;
            let can_prepare_patch_recompute_incrementally = patch_frame_active
                && plan.invalidation.requires_recompute()
                && frame_attr_dirty_ids_complete
                && frame_attr_dirty_ids.is_empty()
                && (!frame_attr_dirty_subtree_roots.is_empty()
                    || !self.animation_runtime.is_empty());
            let prepare_started_at = patch_frame_active.then(Instant::now);
            let preparation = if animation_sample_requested
                && !plan.invalidation.is_dirty()
                && !self.animation_runtime.is_empty()
            {
                prepare_animation_frame_attrs_for_update(
                    &mut self.tree,
                    self.scale,
                    &self.animation_runtime,
                    sample_time,
                )
            } else if can_prepare_dirty_incrementally || can_prepare_patch_recompute_incrementally {
                prepare_dirty_frame_attrs_with_subtrees_for_update(
                    &mut self.tree,
                    self.scale,
                    (!self.animation_runtime.is_empty()).then_some(&self.animation_runtime),
                    sample_time,
                    &frame_attr_dirty_ids,
                    &frame_attr_dirty_subtree_roots,
                )
            } else {
                prepare_frame_attrs_for_update(
                    &mut self.tree,
                    self.scale,
                    (!self.animation_runtime.is_empty()).then_some(&self.animation_runtime),
                    sample_time,
                )
            };
            if let Some(started_at) = prepare_started_at {
                patch_prepare_duration = Some(started_at.elapsed());
            }
            let dynamic_invalidation = preparation.animation_result.invalidation;
            plan.animations_active = preparation.animation_result.active;
            plan.invalidation.add(dynamic_invalidation);

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
                let refresh_started_at = patch_frame_active.then(Instant::now);
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
                if let Some(started_at) = refresh_started_at {
                    patch_refresh_duration = Some(started_at.elapsed());
                }

                if let Some(stats) = options.stats {
                    stats.record_refresh(update_started_at.elapsed());
                }

                let mut output = update.output;
                self.force_cached_registry_publish_if_requested(registry_requested, &mut output);
                self.layout_effect(
                    output,
                    pipeline_submitted_at,
                    tree_batch_started_at,
                    animation_trace,
                )
            }
            RefreshDecision::Recompute => {
                assets::ensure_tree_sources(&self.tree);

                let constraint = crate::tree::layout::Constraint::new(self.width, self.height);
                let (update, timed_layout) = if let Some(preparation) = plan.preparation {
                    if options.stats.is_some() {
                        let (update, timing) =
                            layout_and_refresh_prepared_default_reusing_clean_registry_timed(
                                &mut self.tree,
                                constraint,
                                preparation,
                                self.cached_rebuild.as_ref(),
                            );
                        (update, Some(timing))
                    } else {
                        (
                            layout_and_refresh_prepared_default_reusing_clean_registry(
                                &mut self.tree,
                                constraint,
                                preparation,
                                self.cached_rebuild.as_ref(),
                            ),
                            None,
                        )
                    }
                } else {
                    self.tree.set_layout_cache_stats_enabled(
                        options
                            .stats
                            .is_some_and(|stats| stats.layout_cache_enabled()),
                    );
                    let output = layout_and_refresh_default(&mut self.tree, constraint, self.scale);
                    (
                        crate::tree::layout::LayoutUpdateOutput {
                            output,
                            layout_performed: true,
                        },
                        None,
                    )
                };

                if let Some(stats) = options.stats {
                    if let Some(timing) = timed_layout {
                        if patch_frame_active {
                            patch_layout_duration = Some(timing.layout);
                            patch_refresh_duration = Some(timing.refresh);
                            patch_refresh_traversal_duration = Some(timing.refresh_traversal);
                            patch_refresh_registry_post_duration =
                                Some(timing.refresh_registry_post);
                        }
                        if update.layout_performed {
                            stats.record_layout(timing.layout);
                            stats.record_layout_cache(self.tree.layout_cache_stats());
                        }
                        stats.record_refresh(timing.refresh);
                    } else if update.layout_performed {
                        stats.record_layout(update_started_at.elapsed());
                        stats.record_layout_cache(self.tree.layout_cache_stats());
                    } else {
                        stats.record_refresh(update_started_at.elapsed());
                    }
                }

                let mut output = update.output;
                self.force_cached_registry_publish_if_requested(registry_requested, &mut output);
                self.layout_effect(
                    output,
                    pipeline_submitted_at,
                    tree_batch_started_at,
                    animation_trace,
                )
            }
        };

        record_patch_process_stats(options.stats, patch_processing_started_ats);
        record_patch_stage_stats(
            options.stats,
            PatchStageDurations {
                decode: &patch_decode_durations,
                apply: &patch_apply_durations,
                animation_sync: patch_animation_sync_duration,
                prepare: patch_prepare_duration,
                layout: patch_layout_duration,
                refresh: patch_refresh_duration,
                refresh_traversal: patch_refresh_traversal_duration,
                refresh_registry_post: patch_refresh_registry_post_duration,
            },
        );
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

    fn force_cached_registry_publish_if_requested(
        &self,
        registry_requested: bool,
        output: &mut LayoutOutput,
    ) {
        if !registry_requested || output.event_rebuild_changed {
            return;
        }

        if let Some(rebuild) = self.cached_rebuild.as_ref() {
            output.event_rebuild = rebuild.clone();
            output.event_rebuild_changed = true;
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

fn patches_may_start_animation_runtime(tree: &ElementTree, patches: &[Patch]) -> bool {
    patches.iter().any(|patch| match patch {
        Patch::InsertSubtree { subtree, .. } | Patch::InsertNearbySubtree { subtree, .. } => {
            subtree.iter_node_pairs().any(|(_, element)| {
                element.spec.declared.animate.is_some()
                    || element.spec.declared.animate_enter.is_some()
                    || element.spec.declared.animate_exit.is_some()
            })
        }
        Patch::Remove { id } => tree
            .get(id)
            .is_some_and(|element| element.spec.declared.animate_exit.is_some()),
        Patch::SetAttrs { .. } | Patch::SetChildren { .. } | Patch::SetNearbyMounts { .. } => false,
    })
}

#[derive(Clone, Debug, Default)]
struct PatchFrameAttrDirtyHints {
    ids: Vec<NodeId>,
    subtree_roots: Vec<NodeId>,
    complete: bool,
}

fn patch_frame_attr_dirty_hints(tree: &ElementTree, patches: &[Patch]) -> PatchFrameAttrDirtyHints {
    patches.iter().fold(
        PatchFrameAttrDirtyHints {
            complete: true,
            ..Default::default()
        },
        |mut hints, patch| {
            match patch {
                Patch::SetAttrs { id, .. } => hints.ids.push(*id),
                Patch::InsertSubtree { subtree, .. }
                | Patch::InsertNearbySubtree { subtree, .. } => {
                    if let Some(root_id) = subtree.root_id() {
                        hints.subtree_roots.push(root_id);
                    } else {
                        hints.complete = false;
                    }
                }
                Patch::Remove { id }
                    if tree
                        .get(id)
                        .is_some_and(|element| element.spec.declared.animate_exit.is_some()) => {}
                Patch::SetChildren { .. }
                | Patch::SetNearbyMounts { .. }
                | Patch::Remove { .. } => {
                    hints.complete = false;
                }
            }
            hints
        },
    )
}

fn record_frame_attr_dirty_id(
    frame_attr_dirty_ids: &mut Vec<NodeId>,
    id: NodeId,
    invalidation: TreeInvalidation,
) {
    if invalidation.can_refresh_only() && !frame_attr_dirty_ids.contains(&id) {
        frame_attr_dirty_ids.push(id);
    }
}

fn extend_frame_attr_dirty_ids(frame_attr_dirty_ids: &mut Vec<NodeId>, ids: Vec<NodeId>) {
    ids.into_iter().for_each(|id| {
        record_frame_attr_dirty_id(frame_attr_dirty_ids, id, TreeInvalidation::Paint)
    });
}

fn tree_scale_changed(tree: &ElementTree, scale: f32) -> bool {
    (tree.current_scale() - scale.max(f32::EPSILON)).abs() > f32::EPSILON
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

struct PatchStageDurations<'a> {
    decode: &'a [Duration],
    apply: &'a [Duration],
    animation_sync: Option<Duration>,
    prepare: Option<Duration>,
    layout: Option<Duration>,
    refresh: Option<Duration>,
    refresh_traversal: Option<Duration>,
    refresh_registry_post: Option<Duration>,
}

fn record_patch_stage_stats(
    stats: Option<&Arc<RendererStatsCollector>>,
    durations: PatchStageDurations<'_>,
) {
    let Some(stats) = stats else {
        return;
    };

    durations
        .decode
        .iter()
        .for_each(|duration| stats.record_patch_tree_decode(*duration));
    durations
        .apply
        .iter()
        .for_each(|duration| stats.record_patch_tree_apply(*duration));
    if let Some(duration) = durations.animation_sync {
        stats.record_patch_tree_animation_sync(duration);
    }
    if let Some(duration) = durations.prepare {
        stats.record_patch_tree_prepare(duration);
    }
    if let Some(duration) = durations.layout {
        stats.record_patch_tree_layout(duration);
    }
    if let Some(duration) = durations.refresh {
        stats.record_patch_tree_refresh(duration);
    }
    if let Some(duration) = durations.refresh_traversal {
        stats.record_patch_tree_refresh_traversal(duration);
    }
    if let Some(duration) = durations.refresh_registry_post {
        stats.record_patch_tree_refresh_registry_post(duration);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::render_scene::{RenderNode, RenderScene};
    use crate::tree::{
        animation::{AnimationCurve, AnimationRepeat, AnimationSpec},
        attrs::{Attrs, Background, Color, Length},
        element::{Element, ElementKind, Frame, NearbySlot},
        serialize::encode_tree,
    };

    fn enter_move_x_spec(from_x: f64, to_x: f64, duration_ms: f64) -> AnimationSpec {
        AnimationSpec {
            keyframes: vec![
                Attrs {
                    move_x: Some(from_x),
                    ..Attrs::default()
                },
                Attrs {
                    move_x: Some(to_x),
                    ..Attrs::default()
                },
            ],
            duration_ms,
            curve: AnimationCurve::EaseIn,
            repeat: AnimationRepeat::Once,
        }
    }

    fn encode_length_px_attr(out: &mut Vec<u8>, tag: u8, value: f64) {
        out.push(tag);
        out.push(2);
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn encode_f64_attr(out: &mut Vec<u8>, tag: u8, value: f64) {
        out.push(tag);
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn encoded_move_x_keyframe(value: f64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes());
        encode_f64_attr(&mut out, 31, value);
        out
    }

    fn encoded_size_attrs_raw(width: f64, height: f64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&2u16.to_be_bytes());
        encode_length_px_attr(&mut out, 1, width);
        encode_length_px_attr(&mut out, 2, height);
        out
    }

    fn encoded_enter_move_x_attrs_raw(from_x: f64, to_x: f64, duration_ms: f64) -> Vec<u8> {
        let from = encoded_move_x_keyframe(from_x);
        let to = encoded_move_x_keyframe(to_x);
        let mut spec = Vec::new();
        spec.extend_from_slice(&2u16.to_be_bytes());
        spec.extend_from_slice(&(from.len() as u32).to_be_bytes());
        spec.extend_from_slice(&from);
        spec.extend_from_slice(&(to.len() as u32).to_be_bytes());
        spec.extend_from_slice(&to);
        spec.extend_from_slice(&duration_ms.to_be_bytes());
        spec.extend_from_slice(&7u16.to_be_bytes());
        spec.extend_from_slice(b"ease_in");
        spec.push(0);

        let mut out = Vec::new();
        out.extend_from_slice(&4u16.to_be_bytes());
        encode_length_px_attr(&mut out, 1, 120.0);
        encode_length_px_attr(&mut out, 2, 240.0);
        out.push(12);
        out.push(0);
        out.push(1);
        out.extend_from_slice(&[32, 64, 96, 255]);
        out.push(66);
        out.extend_from_slice(&(spec.len() as u32).to_be_bytes());
        out.extend_from_slice(&spec);
        out
    }

    fn host_only_tree_at_start() -> ElementTree {
        let host_id = NodeId::from_term_bytes(vec![10]);
        let host = Element::with_attrs(
            host_id,
            ElementKind::El,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(360.0)),
                height: Some(Length::Px(240.0)),
                ..Attrs::default()
            },
        );

        let mut tree = ElementTree::new();
        tree.insert(host);
        tree.set_root_id(host_id);
        let revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(revision);
        tree
    }

    fn looping_move_x_spec() -> AnimationSpec {
        AnimationSpec {
            keyframes: vec![
                Attrs {
                    move_x: Some(0.0),
                    ..Attrs::default()
                },
                Attrs {
                    move_x: Some(10.0),
                    ..Attrs::default()
                },
            ],
            duration_ms: 1_000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Loop,
        }
    }

    fn animated_host_tree_at_start() -> ElementTree {
        let host_id = NodeId::from_term_bytes(vec![20]);
        let animated_id = NodeId::from_term_bytes(vec![21]);
        let host = Element::with_attrs(
            host_id,
            ElementKind::El,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(300.0)),
                height: Some(Length::Px(120.0)),
                ..Attrs::default()
            },
        );

        let animated = Element::with_attrs(
            animated_id,
            ElementKind::El,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(20.0)),
                height: Some(Length::Px(20.0)),
                animate: Some(looping_move_x_spec()),
                ..Attrs::default()
            },
        );

        let mut tree = ElementTree::new();
        tree.insert(host);
        tree.insert(animated);
        tree.set_children(&host_id, vec![animated_id]).unwrap();
        tree.set_root_id(host_id);
        let revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(revision);
        tree
    }

    fn encoded_insert_child_with_incomplete_hints_patch() -> Vec<u8> {
        let host_id = NodeId::from_term_bytes(vec![20]);
        let animated_id = NodeId::from_term_bytes(vec![21]);
        let inserted_id = NodeId::from_term_bytes(vec![22]);
        let attrs_raw = encoded_size_attrs_raw(100.0, 30.0);
        let attrs = crate::tree::attrs::decode_attrs(&attrs_raw).unwrap();
        let inserted = Element::with_attrs(inserted_id, ElementKind::El, attrs_raw, attrs);
        let mut subtree = ElementTree::new();
        subtree.insert(inserted);
        subtree.set_root_id(inserted_id);
        let tree_bytes = encode_tree(&subtree);

        let mut out = Vec::new();
        out.push(3);
        out.extend_from_slice(&host_id.to_wire_u64().to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&(tree_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&tree_bytes);

        out.push(2);
        out.extend_from_slice(&host_id.to_wire_u64().to_be_bytes());
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(&animated_id.to_wire_u64().to_be_bytes());
        out.extend_from_slice(&inserted_id.to_wire_u64().to_be_bytes());
        out
    }

    fn encoded_insert_nearby_sidepane_patch() -> Vec<u8> {
        let sidepane_id = NodeId::from_term_bytes(vec![11]);
        let attrs_raw = encoded_enter_move_x_attrs_raw(500.0, 0.0, 125.0);
        let attrs = crate::tree::attrs::decode_attrs(&attrs_raw).unwrap();
        let sidepane = Element::with_attrs(sidepane_id, ElementKind::El, attrs_raw, attrs);
        let mut subtree = ElementTree::new();
        subtree.insert(sidepane);
        subtree.set_root_id(sidepane_id);
        let tree_bytes = encode_tree(&subtree);

        let mut out = Vec::new();
        out.push(6);
        out.extend_from_slice(
            &NodeId::from_term_bytes(vec![10])
                .to_wire_u64()
                .to_be_bytes(),
        );
        out.extend_from_slice(&0u16.to_be_bytes());
        out.push(NearbySlot::InFront.tag());
        out.extend_from_slice(&(tree_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&tree_bytes);
        out
    }

    fn sidepane_enter_tree_at_start() -> ElementTree {
        let host_id = NodeId::from_term_bytes(vec![10]);
        let sidepane_id = NodeId::from_term_bytes(vec![11]);

        let mut host = Element::with_attrs(
            host_id,
            ElementKind::El,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(360.0)),
                height: Some(Length::Px(240.0)),
                ..Attrs::default()
            },
        );
        host.nearby.set(NearbySlot::InFront, Some(sidepane_id));

        let sidepane = Element::with_attrs(
            sidepane_id,
            ElementKind::El,
            Vec::new(),
            Attrs {
                width: Some(Length::Px(120.0)),
                height: Some(Length::Px(240.0)),
                background: Some(Background::Color(Color::Rgba {
                    r: 32,
                    g: 64,
                    b: 96,
                    a: 255,
                })),
                animate_enter: Some(enter_move_x_spec(500.0, 0.0, 125.0)),
                ..Attrs::default()
            },
        );

        let mut tree = ElementTree::new();
        tree.insert(host);
        tree.insert(sidepane);
        tree.set_root_id(host_id);
        let revision = tree.bump_revision();
        tree.stamp_all_mounted_at_revision(revision);
        tree
    }

    fn layout_output(effect: TreeUpdateEffect) -> Box<LayoutOutput> {
        match effect {
            TreeUpdateEffect::Layout { output, .. } => output,
            _ => panic!("expected layout effect"),
        }
    }

    fn transform_txs(scene: &RenderScene) -> Vec<f32> {
        fn collect(nodes: &[RenderNode], txs: &mut Vec<f32>) {
            for node in nodes {
                match node {
                    RenderNode::ShadowPass { children }
                    | RenderNode::Clip { children, .. }
                    | RenderNode::RelaxedClip { children, .. }
                    | RenderNode::Alpha { children, .. } => collect(children, txs),
                    RenderNode::Transform {
                        transform,
                        children,
                    } => {
                        txs.push(transform.tx);
                        collect(children, txs);
                    }
                    RenderNode::PaintLayer(layer) => {
                        collect(&layer.own_nodes, txs);
                        layer
                            .child_refs
                            .iter()
                            .for_each(|child| collect(&child.nodes, txs));
                    }
                    RenderNode::Primitive(_) => {}
                }
            }
        }

        let mut txs = Vec::new();
        collect(&scene.nodes, &mut txs);
        txs
    }

    fn assert_scene_has_translate_x(scene: &RenderScene, min: f32, max: f32) {
        let txs = transform_txs(scene);
        assert!(
            txs.iter().any(|tx| *tx >= min && *tx <= max),
            "expected translate x in [{min}, {max}], got {txs:?}"
        );
    }

    fn assert_scene_has_no_visible_translate_x(scene: &RenderScene) {
        let txs = transform_txs(scene);
        assert!(
            txs.iter().all(|tx| tx.abs() <= 0.5),
            "expected no visible translate x, got {txs:?}"
        );
    }

    fn cached_layer_content_generation(engine: &TreeUpdateEngine, id: NodeId) -> Option<u64> {
        engine
            .tree()
            .get(&id)
            .and_then(|element| {
                element
                    .refresh
                    .render_layer_cache
                    .borrow()
                    .as_ref()
                    .cloned()
            })
            .map(|cache| cache.layer.content_generation)
    }

    fn cached_layer_own_nodes_ptr(engine: &TreeUpdateEngine, id: NodeId) -> Option<usize> {
        engine
            .tree()
            .get(&id)
            .and_then(|element| {
                element
                    .refresh
                    .render_layer_cache
                    .borrow()
                    .as_ref()
                    .cloned()
            })
            .map(|cache| std::sync::Arc::as_ptr(&cache.layer.own_nodes) as usize)
    }

    fn scrollable_tree_at_start() -> ElementTree {
        let id = NodeId::from_term_bytes(vec![1]);
        let attrs = Attrs {
            width: Some(Length::Px(100.0)),
            height: Some(Length::Px(100.0)),
            scrollbar_y: Some(true),
            scroll_y: Some(0.0),
            scroll_y_max: Some(100.0),
            ..Attrs::default()
        };
        let mut element = Element::with_attrs(id, ElementKind::El, Vec::new(), attrs);
        element.layout.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.insert(element);
        tree.set_root_id(id);
        tree
    }

    #[test]
    fn inserted_nearby_enter_patch_renders_first_frame_at_start_transform() {
        let mut engine = TreeUpdateEngine::new(host_only_tree_at_start(), 360, 240);
        let _ = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::Resize {
                        width: 360.0,
                        height: 240.0,
                        scale: 1.0,
                    }],
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap(),
        );

        let first = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::PatchTree {
                        bytes: encoded_insert_nearby_sidepane_patch(),
                        submitted_at: Some(Instant::now()),
                    }],
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap(),
        );

        assert!(first.animations_active);
        assert_scene_has_translate_x(&first.scene, 499.0, 501.0);
    }

    #[test]
    fn completed_enter_animation_refreshes_final_base_attrs_before_stopping() {
        let mut engine = TreeUpdateEngine::new(sidepane_enter_tree_at_start(), 360, 240);

        let initial = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::Resize {
                        width: 360.0,
                        height: 240.0,
                        scale: 1.0,
                    }],
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap(),
        );
        assert!(initial.animations_active);
        assert_scene_has_translate_x(&initial.scene, 499.0, 501.0);
        let sidepane_id = NodeId::from_term_bytes(vec![11]);
        let initial_paint_generation = engine
            .tree()
            .get(&sidepane_id)
            .expect("sidepane should exist")
            .refresh
            .paint_generation;

        let first_presented = Instant::now() + Duration::from_secs(1);
        let mid = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::AnimationPulse {
                        presented_at: first_presented,
                        predicted_next_present_at: first_presented + Duration::from_millis(64),
                        trace: None,
                    }],
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap(),
        );
        assert!(mid.animations_active);
        assert_scene_has_translate_x(&mid.scene, 300.0, 500.0);
        assert_eq!(
            engine
                .tree()
                .get(&sidepane_id)
                .expect("sidepane should exist")
                .refresh
                .paint_generation,
            initial_paint_generation
        );
        let mid_layer_generation = cached_layer_content_generation(&engine, sidepane_id).expect(
            "translated sidepane should cache stable payload content after first clean pulse",
        );
        let mid_layer_own_nodes_ptr = cached_layer_own_nodes_ptr(&engine, sidepane_id).expect(
            "translated sidepane should cache stable payload nodes after first clean pulse",
        );

        let late_mid = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::AnimationPulse {
                        presented_at: first_presented + Duration::from_millis(64),
                        predicted_next_present_at: first_presented + Duration::from_millis(96),
                        trace: None,
                    }],
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap(),
        );
        assert!(late_mid.animations_active);
        assert_scene_has_translate_x(&late_mid.scene, 200.0, 350.0);
        assert_eq!(
            cached_layer_content_generation(&engine, sidepane_id),
            Some(mid_layer_generation)
        );
        assert_eq!(
            cached_layer_own_nodes_ptr(&engine, sidepane_id),
            Some(mid_layer_own_nodes_ptr)
        );

        let final_output = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::AnimationPulse {
                        presented_at: first_presented + Duration::from_millis(96),
                        predicted_next_present_at: first_presented + Duration::from_millis(150),
                        trace: None,
                    }],
                    TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr),
                )
                .unwrap(),
        );

        assert!(!final_output.animations_active);
        assert!(engine.animation_runtime_is_empty());
        assert_eq!(
            engine
                .tree()
                .get(&sidepane_id)
                .expect("sidepane should exist")
                .refresh
                .paint_generation,
            initial_paint_generation
        );
        assert_eq!(
            engine
                .tree()
                .get(&sidepane_id)
                .and_then(|element| element.layout.effective.move_x),
            None
        );
        assert_scene_has_no_visible_translate_x(&final_output.scene);
    }

    #[test]
    fn incomplete_recompute_patch_hints_fall_back_to_full_attr_prepare() {
        let inserted_id = NodeId::from_term_bytes(vec![22]);
        let mut engine = TreeUpdateEngine::new(animated_host_tree_at_start(), 300, 120);
        let options = TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);
        let initial = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::Resize {
                        width: 600.0,
                        height: 240.0,
                        scale: 2.0,
                    }],
                    options,
                )
                .unwrap(),
        );
        assert!(initial.animations_active);

        let options = TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);
        let _ = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::PatchTree {
                        bytes: encoded_insert_child_with_incomplete_hints_patch(),
                        submitted_at: None,
                    }],
                    options,
                )
                .unwrap(),
        );

        let inserted = engine
            .tree()
            .get(&inserted_id)
            .expect("inserted child exists");
        assert_eq!(
            inserted.layout.effective.width,
            Some(Length::Px(200.0)),
            "inserted subtree attrs should be prepared at the active scale"
        );
        assert_eq!(
            inserted.layout.frame.map(|frame| frame.width),
            Some(200.0),
            "inserted subtree layout should use scaled attrs"
        );
    }

    #[test]
    fn refresh_only_message_with_pending_scale_change_forces_recompute() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut engine = TreeUpdateEngine::new(scrollable_tree_at_start(), 100, 100);
        let options = TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);

        let _ = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::Resize {
                        width: 100.0,
                        height: 100.0,
                        scale: 1.0,
                    }],
                    options,
                )
                .unwrap(),
        );
        assert_eq!(engine.tree().current_scale(), 1.0);
        assert_eq!(
            engine.tree().get(&id).unwrap().layout.frame.unwrap().width,
            100.0
        );

        engine.scale = 2.0;
        let options = TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);
        let _ = layout_output(
            engine
                .process_messages(
                    vec![TreeMsg::SetScrollbarYHover {
                        element_id: id,
                        hovered: true,
                    }],
                    options,
                )
                .unwrap(),
        );

        assert_eq!(engine.tree().current_scale(), 2.0);
        assert_eq!(
            engine.tree().get(&id).unwrap().layout.frame.unwrap().width,
            200.0
        );
    }

    #[test]
    fn blocked_scroll_request_publishes_cached_registry_response() {
        let id = NodeId::from_term_bytes(vec![1]);
        let mut engine = TreeUpdateEngine::new(scrollable_tree_at_start(), 100, 100);
        let options = TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);

        assert!(matches!(
            engine.process_messages(vec![TreeMsg::RebuildRegistry], options),
            Ok(TreeUpdateEffect::Layout { .. })
        ));

        let options = TreeUpdateOptions::new(None, TreeUpdateDecodePolicy::ReturnErr);
        let effect = engine.process_messages(
            vec![TreeMsg::ScrollRequest {
                element_id: id,
                dx: 0.0,
                dy: 10.0,
            }],
            options,
        );

        assert!(matches!(
            effect,
            Ok(TreeUpdateEffect::RegistryUpdate { .. })
        ));
    }
}
