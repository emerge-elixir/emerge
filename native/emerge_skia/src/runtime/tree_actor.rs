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
    actors::{EventMsg, RenderMsg, TreeMsg},
    assets,
    backend::wake::BackendWakeHandle,
    events::{self, RegistryRebuildPayload},
    stats::RendererStatsCollector,
    tree::{
        animation::AnimationRuntime,
        element::ElementTree,
        layout::{layout_and_refresh_default, layout_and_refresh_default_with_animation},
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefreshDecision {
    Skip,
    UseCachedRebuild,
    Recompute,
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
        let mut pending_msg: Option<TreeMsg> = None;

        loop {
            let msg = match pending_msg.take() {
                Some(msg) => msg,
                None => match tree_rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => return,
                },
            };
            let mut messages = Vec::new();
            push_tree_message_flat(msg, &mut messages);
            let animation_only_batch = batch_is_animation_only(&messages);
            while let Ok(next) = tree_rx.try_recv() {
                let mut next_messages = Vec::new();
                push_tree_message_flat(next, &mut next_messages);

                if batch_is_animation_only(&next_messages) != animation_only_batch {
                    pending_msg = Some(TreeMsg::Batch(next_messages));
                    break;
                }

                messages.extend(next_messages);
            }

            let mut scroll_acc = std::collections::HashMap::new();
            let mut thumb_drag_x_acc = std::collections::HashMap::new();
            let mut thumb_drag_y_acc = std::collections::HashMap::new();
            let mut hover_x_state = std::collections::HashMap::new();
            let mut hover_y_state = std::collections::HashMap::new();
            let mut mouse_over_active_state = std::collections::HashMap::new();
            let mut mouse_down_active_state = std::collections::HashMap::new();
            let mut focused_active_state = std::collections::HashMap::new();
            let mut patch_processing_started_ats = Vec::new();
            let mut tree_changed = false;
            let mut registry_requested = false;
            let mut animation_sample_time = latest_animation_sample_time;

            for message in messages.iter().cloned() {
                match message {
                    TreeMsg::Stop => return,
                    TreeMsg::Batch(_) => {
                        unreachable!("tree batches must be flattened before processing")
                    }
                    TreeMsg::UploadTree { bytes } => {
                        match crate::tree::deserialize::decode_tree(&bytes) {
                            Ok(decoded) => {
                                tree.replace_with_uploaded(decoded);
                                tree_changed = true;
                            }
                            Err(err) => {
                                eprintln!("tree upload failed: {err}");
                            }
                        }
                    }
                    TreeMsg::PatchTree { bytes } => {
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
                        if let Err(err) = crate::tree::patch::apply_patches(&mut tree, patches) {
                            if let Some(stats) = stats.as_ref() {
                                stats.record_patch_tree_process(patch_started_at.elapsed());
                            }
                            eprintln!("tree patch apply failed: {err}");
                            continue;
                        }
                        patch_processing_started_ats.push(patch_started_at);
                        tree_changed = true;
                    }
                    TreeMsg::Resize {
                        width: w,
                        height: h,
                        scale: s,
                    } => {
                        width = w.max(1.0);
                        height = h.max(1.0);
                        scale = s;
                        tree_changed = true;
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
                        let changed = tree.set_text_input_content(&element_id, content);
                        tree_changed |= changed;
                    }
                    TreeMsg::SetTextInputRuntime {
                        element_id,
                        focused,
                        cursor,
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    } => {
                        let changed = tree.set_text_input_runtime(
                            &element_id,
                            focused,
                            cursor,
                            selection_anchor,
                            preedit,
                            preedit_cursor,
                        );
                        tree_changed |= changed;
                    }
                    TreeMsg::AnimationPulse {
                        presented_at,
                        predicted_next_present_at,
                    } => {
                        crate::debug_trace::hover_trace!(
                            "tree_pulse",
                            "presented_at={:?} predicted_next={:?}",
                            presented_at,
                            predicted_next_present_at
                        );
                        animation_sample_time = Some(predicted_next_present_at.max(presented_at));
                        tree_changed = true;
                    }
                    TreeMsg::RebuildRegistry => {
                        registry_requested = true;
                    }
                    TreeMsg::AssetStateChanged => {
                        tree_changed = true;
                    }
                }
            }

            for (id, (dx, dy)) in scroll_acc {
                let changed = tree.apply_scroll(&id, dx, dy);
                tree_changed |= changed;
            }

            for (id, dx) in thumb_drag_x_acc {
                let changed = tree.apply_scroll_x(&id, dx);
                tree_changed |= changed;
            }

            for (id, dy) in thumb_drag_y_acc {
                let changed = tree.apply_scroll_y(&id, dy);
                tree_changed |= changed;
            }

            for (id, hovered) in hover_x_state {
                tree_changed |= tree.set_scrollbar_x_hover(&id, hovered);
            }

            for (id, hovered) in hover_y_state {
                tree_changed |= tree.set_scrollbar_y_hover(&id, hovered);
            }

            for (id, active) in &mouse_over_active_state {
                tree_changed |= tree.set_mouse_over_active(id, *active);
            }

            for (id, active) in mouse_down_active_state {
                tree_changed |= tree.set_mouse_down_active(&id, active);
            }

            for (id, active) in focused_active_state {
                tree_changed |= tree.set_focused_active(&id, active);
            }

            match decide_refresh_action(tree_changed, registry_requested, cached_rebuild.is_some())
            {
                RefreshDecision::Skip => {
                    if let Some(stats) = stats.as_ref() {
                        patch_processing_started_ats
                            .into_iter()
                            .for_each(|started_at| {
                                stats.record_patch_tree_process(started_at.elapsed())
                            });
                    }
                    continue;
                }
                RefreshDecision::UseCachedRebuild => {
                    if let Some(rebuild) = cached_rebuild.clone() {
                        send_registry_update(&event_tx, rebuild, log_input);
                    }
                    if let Some(stats) = stats.as_ref() {
                        patch_processing_started_ats
                            .into_iter()
                            .for_each(|started_at| {
                                stats.record_patch_tree_process(started_at.elapsed())
                            });
                    }
                    continue;
                }
                RefreshDecision::Recompute => {
                    assets::ensure_tree_sources(&tree);

                    let constraint = crate::tree::layout::Constraint::new(width, height);
                    let sample_time = animation_sample_time.unwrap_or_else(Instant::now);
                    latest_animation_sample_time = Some(sample_time);
                    crate::debug_trace::hover_trace!(
                        "tree_recompute",
                        "sample_time={:?} cached_rebuild={} tree_changed={} registry_requested={}",
                        sample_time,
                        cached_rebuild.is_some(),
                        tree_changed,
                        registry_requested
                    );
                    animation_runtime.sync_with_tree(&tree, sample_time);
                    let _ =
                        animation_runtime.prune_completed_exit_ghosts(&mut tree, Some(sample_time));
                    let layout_started_at = Instant::now();
                    let output = if animation_runtime.is_empty() {
                        layout_and_refresh_default(&mut tree, constraint, scale)
                    } else {
                        layout_and_refresh_default_with_animation(
                            &mut tree,
                            constraint,
                            scale,
                            &animation_runtime,
                            sample_time,
                        )
                    };
                    if let Some(stats) = stats.as_ref() {
                        stats.record_layout(layout_started_at.elapsed());
                    }
                    cached_rebuild = Some(output.event_rebuild.clone());
                    send_registry_update(&event_tx, output.event_rebuild, log_input);

                    let version = render_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    render_sender.send_latest(RenderMsg::Scene {
                        scene: Box::new(output.scene),
                        version,
                        animate: output.animations_active,
                        ime_enabled: output.ime_enabled,
                        ime_cursor_area: output.ime_cursor_area,
                        ime_text_state: Box::new(output.ime_text_state),
                    });

                    window_wake.request_redraw();

                    #[cfg(feature = "hover-trace")]
                    {
                        for (id, x, y, w, h, move_x) in trace_element_snapshots(&tree) {
                            crate::debug_trace::hover_trace!(
                                "tree_snapshot",
                                "id={:?} frame=({x:.2},{y:.2},{w:.2},{h:.2}) move_x={:.2} visual_x={:.2}",
                                id.0,
                                move_x.unwrap_or(0.0),
                                x + move_x.unwrap_or(0.0) as f32
                            );
                        }
                    }

                    if animation_runtime.is_empty() || !output.animations_active {
                        latest_animation_sample_time = None;
                    }

                    if let Some(stats) = stats.as_ref() {
                        patch_processing_started_ats
                            .into_iter()
                            .for_each(|started_at| {
                                stats.record_patch_tree_process(started_at.elapsed())
                            });
                    }
                }
            }
        }
    })
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

pub(crate) fn push_tree_message_flat(msg: TreeMsg, out: &mut Vec<TreeMsg>) {
    match msg {
        TreeMsg::Batch(messages) => messages
            .into_iter()
            .for_each(|nested| push_tree_message_flat(nested, out)),
        other => out.push(other),
    }
}

fn is_animation_pulse(msg: &TreeMsg) -> bool {
    matches!(msg, TreeMsg::AnimationPulse { .. })
}

fn batch_is_animation_only(messages: &[TreeMsg]) -> bool {
    !messages.is_empty() && messages.iter().all(is_animation_pulse)
}

fn decide_refresh_action(
    tree_changed: bool,
    registry_requested: bool,
    has_cached_rebuild: bool,
) -> RefreshDecision {
    if tree_changed {
        RefreshDecision::Recompute
    } else if registry_requested && has_cached_rebuild {
        RefreshDecision::UseCachedRebuild
    } else if registry_requested {
        RefreshDecision::Recompute
    } else {
        RefreshDecision::Skip
    }
}

#[cfg(feature = "hover-trace")]
fn trace_element_snapshots(tree: &ElementTree) -> Vec<(NodeId, f32, f32, f32, f32, Option<f64>)> {
    tree.nodes
        .values()
        .filter(|element| {
            element.attrs.on_mouse_move.unwrap_or(false)
                || element.attrs.mouse_over.is_some()
                || element.attrs.mouse_over_active.unwrap_or(false)
        })
        .filter_map(|element| {
            element.frame.map(|frame| {
                (
                    element.id.clone(),
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    element.attrs.move_x,
                )
            })
        })
        .collect()
}
