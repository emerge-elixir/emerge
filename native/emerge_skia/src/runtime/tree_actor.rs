use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Instant,
};

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::{
    RenderSender,
    actors::{
        AnimationFrameTraceSeed, AnimationPulseTrace, EventMsg, RenderMsg, TreeMsg,
        earliest_pipeline_submitted_at,
    },
    assets,
    backend::wake::BackendWakeHandle,
    events::{self, RegistryRebuildPayload},
    stats::RendererStatsCollector,
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

#[cfg(feature = "hover-trace")]
use crate::tree::element::NodeId;

pub(crate) struct TreeActorConfig {
    pub(crate) render_sender: RenderSender,
    pub(crate) event_tx: Sender<EventMsg>,
    pub(crate) render_counter: Arc<AtomicU64>,
    pub(crate) stats: Option<Arc<RendererStatsCollector>>,
    pub(crate) log_input: bool,
    pub(crate) window_wake: BackendWakeHandle,
    pub(crate) initial_width: u32,
    pub(crate) initial_height: u32,
}

#[cfg_attr(
    not(any(
        all(feature = "wayland", target_os = "linux"),
        all(feature = "drm", target_os = "linux")
    )),
    allow(dead_code)
)]
pub(crate) fn spawn_tree_actor(
    tree_rx: Receiver<TreeMsg>,
    config: TreeActorConfig,
) -> thread::JoinHandle<()> {
    spawn_tree_actor_with_initial_tree(tree_rx, config, ElementTree::new())
}

pub(crate) fn spawn_tree_actor_with_initial_tree(
    tree_rx: Receiver<TreeMsg>,
    config: TreeActorConfig,
    initial_tree: ElementTree,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let TreeActorConfig {
            render_sender,
            event_tx,
            render_counter,
            stats,
            log_input,
            window_wake,
            initial_width,
            initial_height,
        } = config;

        let mut tree = initial_tree;
        let mut width = (initial_width as f32).max(1.0);
        let mut height = (initial_height as f32).max(1.0);
        let mut scale = 1.0f32;
        let mut cached_rebuild: Option<RegistryRebuildPayload> = None;
        let mut animation_runtime = AnimationRuntime::default();
        let mut latest_animation_sample_time: Option<Instant> = None;

        loop {
            let msg = match tree_rx.recv() {
                Ok(msg) => msg,
                Err(_) => return,
            };
            let mut messages = Vec::new();
            push_tree_message_flat(msg, &mut messages);
            while let Ok(next) = tree_rx.try_recv() {
                push_tree_message_flat(next, &mut messages);
            }
            let tree_batch_started_at = Instant::now();

            let mut scroll_acc = std::collections::HashMap::new();
            let mut thumb_drag_x_acc = std::collections::HashMap::new();
            let mut thumb_drag_y_acc = std::collections::HashMap::new();
            let mut hover_x_state = std::collections::HashMap::new();
            let mut hover_y_state = std::collections::HashMap::new();
            let mut mouse_over_active_state = std::collections::HashMap::new();
            let mut mouse_down_active_state = std::collections::HashMap::new();
            let mut focused_active_state = std::collections::HashMap::new();
            let mut patch_processing_started_ats = Vec::new();
            let mut pipeline_submitted_at = None;
            let mut invalidation = TreeInvalidation::None;
            let mut registry_requested = false;
            let mut animation_sample_time = latest_animation_sample_time;
            let mut animation_trace_previous_sample_time = animation_sample_time;
            let mut animation_trace_pulse: Option<(AnimationPulseTrace, Instant, Instant)> = None;
            let mut animation_presented_at = None;
            let mut animation_predicted_next_present_at = None;
            let mut animation_sample_requested = false;

            for message in messages.iter().cloned() {
                match message {
                    TreeMsg::Stop => return,
                    TreeMsg::Batch(_) => {
                        unreachable!("tree batches must be flattened before processing")
                    }
                    TreeMsg::UploadTree {
                        bytes,
                        submitted_at,
                    } => {
                        pipeline_submitted_at =
                            earliest_pipeline_submitted_at(pipeline_submitted_at, submitted_at);
                        match crate::tree::deserialize::decode_tree(&bytes) {
                            Ok(decoded) => {
                                tree.replace_with_uploaded(decoded);
                                invalidation.add(TreeInvalidation::Structure);
                            }
                            Err(err) => {
                                eprintln!("tree upload failed: {err}");
                            }
                        }
                    }
                    TreeMsg::PatchTree {
                        bytes,
                        submitted_at,
                    } => {
                        pipeline_submitted_at =
                            earliest_pipeline_submitted_at(pipeline_submitted_at, submitted_at);
                        let patch_started_at = Instant::now();
                        let patches = match crate::tree::patch::decode_patches(&bytes) {
                            Ok(patches) => patches,
                            Err(err) => {
                                if let Some(stats) = stats.as_ref() {
                                    stats.record_patch_tree_process(patch_started_at.elapsed());
                                }
                                eprintln!("tree patch decode failed: {err}");
                                continue;
                            }
                        };
                        match crate::tree::patch::apply_patches(&mut tree, patches) {
                            Ok(patch_invalidation) => {
                                invalidation.add(patch_invalidation);
                            }
                            Err(err) => {
                                if let Some(stats) = stats.as_ref() {
                                    stats.record_patch_tree_process(patch_started_at.elapsed());
                                }
                                eprintln!("tree patch apply failed: {err}");
                                continue;
                            }
                        }
                        patch_processing_started_ats.push(patch_started_at);
                    }
                    TreeMsg::Resize {
                        width: w,
                        height: h,
                        scale: s,
                    } => {
                        width = w.max(1.0);
                        height = h.max(1.0);
                        scale = s;
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
                        invalidation.add(tree.set_text_input_content(&element_id, content));
                    }
                    TreeMsg::SetTextInputRuntime {
                        element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    } => {
                        invalidation.add(tree.set_text_input_runtime(
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

            if let (Some(stats), Some(submitted_at)) = (stats.as_ref(), pipeline_submitted_at) {
                stats.record_pipeline_submit_to_tree_start(submitted_at, tree_batch_started_at);
            }

            for (id, (dx, dy)) in scroll_acc {
                invalidation.add(tree.apply_scroll(&id, dx, dy));
            }

            for (id, dx) in thumb_drag_x_acc {
                invalidation.add(tree.apply_scroll_x(&id, dx));
            }

            for (id, dy) in thumb_drag_y_acc {
                invalidation.add(tree.apply_scroll_y(&id, dy));
            }

            for (id, hovered) in hover_x_state {
                invalidation.add(tree.set_scrollbar_x_hover(&id, hovered));
            }

            for (id, hovered) in hover_y_state {
                invalidation.add(tree.set_scrollbar_y_hover(&id, hovered));
            }

            for (id, active) in &mouse_over_active_state {
                invalidation.add(tree.set_mouse_over_active(id, *active));
            }

            for (id, active) in mouse_down_active_state {
                invalidation.add(tree.set_mouse_down_active(&id, active));
            }

            for (id, active) in focused_active_state {
                invalidation.add(tree.set_focused_active(&id, active));
            }

            let update_started_at = Instant::now();
            let mut plan = FrameUpdatePlan::new(invalidation);
            let should_sync_animations = animation_sample_requested
                || !animation_runtime.is_empty()
                || plan.invalidation.requires_recompute();
            let sample_time =
                should_sync_animations.then(|| animation_sample_time.unwrap_or_else(Instant::now));
            let had_animation_runtime = !animation_runtime.is_empty();
            let had_transient_animations = animation_runtime.has_transient_entries();

            if let Some(sample_time) = sample_time {
                if animation_sample_requested && let Some(presented_at) = animation_presented_at {
                    animation_runtime.anchor_pending_transient_entries_to_present(presented_at);
                }
                latest_animation_sample_time = Some(sample_time);
                crate::debug_trace::hover_trace!(
                    "tree_plan",
                    "sample_time={:?} cached_rebuild={} invalidation={:?} registry_requested={}",
                    sample_time,
                    cached_rebuild.is_some(),
                    plan.invalidation,
                    registry_requested
                );
                animation_runtime.sync_with_tree(&tree, sample_time);
                if animation_runtime.prune_completed_exit_ghosts(&mut tree, Some(sample_time)) {
                    plan.invalidation.add(TreeInvalidation::Structure);
                }
            }

            let should_prepare_frame = plan.invalidation.is_dirty()
                || animation_sample_requested
                || !animation_runtime.is_empty();

            if should_prepare_frame {
                tree.set_layout_cache_stats_enabled(
                    stats
                        .as_ref()
                        .is_some_and(|stats| stats.layout_cache_enabled()),
                );
                let preparation = if animation_sample_requested
                    && !plan.invalidation.is_dirty()
                    && !animation_runtime.is_empty()
                    && !had_transient_animations
                {
                    prepare_animation_frame_attrs_for_update(
                        &mut tree,
                        scale,
                        &animation_runtime,
                        sample_time,
                    )
                } else {
                    prepare_frame_attrs_for_update(
                        &mut tree,
                        scale,
                        (!animation_runtime.is_empty()).then_some(&animation_runtime),
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
                    has_cached_rebuild: cached_rebuild.is_some(),
                    has_root_frame: plan.preparation.as_ref().map_or_else(
                        || tree_has_root_frame(&tree),
                        |preparation| prepared_root_has_frame(&tree, preparation),
                    ),
                },
            );
            let animation_frame_trace = sample_time
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

            match plan.action {
                RefreshDecision::Skip => {
                    if animation_runtime.is_empty() || !plan.animations_active {
                        latest_animation_sample_time = None;
                    }
                    record_patch_process_stats(stats.as_ref(), patch_processing_started_ats);
                    continue;
                }
                RefreshDecision::UseCachedRebuild => {
                    if let Some(rebuild) = cached_rebuild.clone() {
                        send_registry_update(&event_tx, rebuild, log_input);
                    }
                    if animation_runtime.is_empty() || !plan.animations_active {
                        latest_animation_sample_time = None;
                    }
                    record_patch_process_stats(stats.as_ref(), patch_processing_started_ats);
                    continue;
                }
                RefreshDecision::RefreshOnly => {
                    assets::ensure_tree_sources(&tree);
                    let update = if let Some(preparation) = plan.preparation {
                        refresh_prepared_default_reusing_clean_registry(
                            &mut tree,
                            preparation,
                            cached_rebuild.as_ref(),
                        )
                    } else {
                        tree.set_layout_cache_stats_enabled(
                            stats
                                .as_ref()
                                .is_some_and(|stats| stats.layout_cache_enabled()),
                        );
                        tree.reset_layout_cache_stats();
                        let output =
                            refresh_reusing_clean_registry(&mut tree, cached_rebuild.as_ref());
                        crate::tree::layout::LayoutUpdateOutput {
                            output,
                            layout_performed: false,
                        }
                    };

                    if let Some(stats) = stats.as_ref() {
                        stats.record_refresh(update_started_at.elapsed());
                    }

                    let animations_active = update.output.animations_active;
                    publish_layout_output(
                        LayoutOutputPublishTargets {
                            event_tx: &event_tx,
                            render_sender: &render_sender,
                            render_counter: &render_counter,
                            window_wake: &window_wake,
                            cached_rebuild: &mut cached_rebuild,
                            stats: stats.as_ref(),
                            log_input,
                        },
                        update.output,
                        pipeline_submitted_at,
                        pipeline_submitted_at.map(|_| tree_batch_started_at),
                        animation_frame_trace,
                    );

                    trace_tree_snapshots(&tree);

                    if animation_runtime.is_empty() || !animations_active {
                        latest_animation_sample_time = None;
                    }

                    record_patch_process_stats(stats.as_ref(), patch_processing_started_ats);
                }
                RefreshDecision::Recompute => {
                    assets::ensure_tree_sources(&tree);

                    let constraint = crate::tree::layout::Constraint::new(width, height);
                    let update = if let Some(preparation) = plan.preparation {
                        layout_and_refresh_prepared_default(&mut tree, constraint, preparation)
                    } else {
                        tree.set_layout_cache_stats_enabled(
                            stats
                                .as_ref()
                                .is_some_and(|stats| stats.layout_cache_enabled()),
                        );
                        let output = layout_and_refresh_default(&mut tree, constraint, scale);
                        crate::tree::layout::LayoutUpdateOutput {
                            output,
                            layout_performed: true,
                        }
                    };

                    if let Some(stats) = stats.as_ref() {
                        if update.layout_performed {
                            stats.record_layout(update_started_at.elapsed());
                            stats.record_layout_cache(tree.layout_cache_stats());
                        } else {
                            stats.record_refresh(update_started_at.elapsed());
                        }
                    }
                    let animations_active = update.output.animations_active;
                    publish_layout_output(
                        LayoutOutputPublishTargets {
                            event_tx: &event_tx,
                            render_sender: &render_sender,
                            render_counter: &render_counter,
                            window_wake: &window_wake,
                            cached_rebuild: &mut cached_rebuild,
                            stats: stats.as_ref(),
                            log_input,
                        },
                        update.output,
                        pipeline_submitted_at,
                        pipeline_submitted_at.map(|_| tree_batch_started_at),
                        animation_frame_trace,
                    );

                    trace_tree_snapshots(&tree);

                    if animation_runtime.is_empty() || !animations_active {
                        latest_animation_sample_time = None;
                    }

                    record_patch_process_stats(stats.as_ref(), patch_processing_started_ats);
                }
            }
        }
    })
}

fn animation_pulse_sample_time(
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

fn record_patch_process_stats(
    stats: Option<&Arc<RendererStatsCollector>>,
    patch_processing_started_ats: Vec<Instant>,
) {
    if let Some(stats) = stats {
        patch_processing_started_ats
            .into_iter()
            .for_each(|started_at| stats.record_patch_tree_process(started_at.elapsed()));
    }
}

pub(crate) fn send_registry_update(
    event_tx: &Sender<EventMsg>,
    rebuild: events::RegistryRebuildPayload,
    log_input: bool,
) {
    match event_tx.try_send(EventMsg::RegistryUpdate { rebuild }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            if log_input {
                eprintln!("event channel full, dropping registry update");
            }
            crate::debug_trace::hover_trace!(
                "event_channel",
                "event channel full, dropping registry update"
            );
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

struct LayoutOutputPublishTargets<'a> {
    event_tx: &'a Sender<EventMsg>,
    render_sender: &'a RenderSender,
    render_counter: &'a Arc<AtomicU64>,
    window_wake: &'a BackendWakeHandle,
    cached_rebuild: &'a mut Option<RegistryRebuildPayload>,
    stats: Option<&'a Arc<RendererStatsCollector>>,
    log_input: bool,
}

fn publish_layout_output(
    targets: LayoutOutputPublishTargets<'_>,
    output: LayoutOutput,
    pipeline_submitted_at: Option<Instant>,
    pipeline_tree_started_at: Option<Instant>,
    animation_trace: Option<AnimationFrameTraceSeed>,
) {
    if output.event_rebuild_changed {
        targets.cached_rebuild.replace(output.event_rebuild.clone());
        send_registry_update(targets.event_tx, output.event_rebuild, targets.log_input);
    }

    let version = targets.render_counter.fetch_add(1, Ordering::Relaxed) + 1;
    let render_queued_at = Instant::now();
    let pipeline_render_queued_at = pipeline_submitted_at.map(|_| render_queued_at);
    if let (Some(stats), Some(tree_started_at), Some(render_queued_at)) = (
        targets.stats,
        pipeline_tree_started_at,
        pipeline_render_queued_at,
    ) {
        stats.record_pipeline_tree(tree_started_at, render_queued_at);
    }
    targets.render_sender.send_latest(RenderMsg::Scene {
        scene: Box::new(output.scene),
        version,
        pipeline_submitted_at,
        pipeline_render_queued_at,
        animation_trace: animation_trace.map(|trace| Box::new(trace.queued_at(render_queued_at))),
        animate: output.animations_active,
        ime_enabled: output.ime_enabled,
        ime_cursor_area: output.ime_cursor_area,
        ime_text_state: Box::new(output.ime_text_state),
    });

    targets.window_wake.request_redraw();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderSender, render_scene::RenderScene, tree::element::NodeId};
    use crossbeam_channel::bounded;
    use std::sync::{Arc, atomic::AtomicU64};

    #[test]
    fn animation_pulse_sample_time_never_regresses() {
        let base = Instant::now();
        let previous = base + std::time::Duration::from_millis(67);
        let presented_at = base + std::time::Duration::from_millis(20);
        let predicted_next_present_at = base + std::time::Duration::from_millis(36);

        assert_eq!(
            animation_pulse_sample_time(Some(previous), presented_at, predicted_next_present_at),
            previous
        );
    }

    #[test]
    fn animation_pulse_sample_time_uses_predicted_present_without_previous() {
        let base = Instant::now();
        let presented_at = base;
        let predicted_next_present_at = base + std::time::Duration::from_millis(16);

        assert_eq!(
            animation_pulse_sample_time(None, presented_at, predicted_next_present_at),
            predicted_next_present_at
        );
    }

    #[test]
    fn publish_layout_output_preserves_cached_registry_when_output_is_clean() {
        let (event_tx, event_rx) = bounded(1);
        let (render_tx, render_rx) = bounded(1);
        let render_sender = RenderSender {
            tx: render_tx,
            drop_rx: render_rx.clone(),
            log_render: false,
        };
        let render_counter = Arc::new(AtomicU64::new(0));
        let window_wake = BackendWakeHandle::noop();
        let focused_id = NodeId::from_term_bytes(vec![42]);
        let mut cached_rebuild = Some(RegistryRebuildPayload {
            focused_id: Some(focused_id),
            ..Default::default()
        });

        publish_layout_output(
            LayoutOutputPublishTargets {
                event_tx: &event_tx,
                render_sender: &render_sender,
                render_counter: &render_counter,
                window_wake: &window_wake,
                cached_rebuild: &mut cached_rebuild,
                stats: None,
                log_input: false,
            },
            LayoutOutput {
                scene: RenderScene::default(),
                event_rebuild: RegistryRebuildPayload::default(),
                event_rebuild_changed: false,
                ime_enabled: false,
                ime_cursor_area: None,
                ime_text_state: None,
                animations_active: false,
            },
            Some(Instant::now()),
            Some(Instant::now()),
            None,
        );

        assert_eq!(
            cached_rebuild
                .as_ref()
                .and_then(|rebuild| rebuild.focused_id),
            Some(focused_id)
        );
        assert!(event_rx.try_recv().is_err());

        match render_rx
            .try_recv()
            .expect("scene should still be published")
        {
            RenderMsg::Scene {
                version,
                pipeline_submitted_at,
                pipeline_render_queued_at,
                ..
            } => {
                assert_eq!(version, 1);
                assert!(pipeline_submitted_at.is_some());
                assert!(pipeline_render_queued_at.is_some());
            }
            RenderMsg::Stop => panic!("expected scene render message"),
        }
    }
}

#[cfg(feature = "hover-trace")]
fn trace_element_snapshots(tree: &ElementTree) -> Vec<(NodeId, f32, f32, f32, f32, Option<f64>)> {
    tree.nodes
        .values()
        .filter(|element| {
            element.layout.effective.on_mouse_move.unwrap_or(false)
                || element.layout.effective.mouse_over.is_some()
                || element.runtime.mouse_over_active
        })
        .filter_map(|element| {
            element.layout.frame.map(|frame| {
                (
                    element.id.clone(),
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
