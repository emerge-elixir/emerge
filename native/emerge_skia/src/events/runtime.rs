//! # Event Runtime
//!
//! This module runs the event actor side of the event system.
//!
//! It is responsible for:
//!
//! - receiving backend input
//! - forwarding raw observer input
//! - dispatching listener input against base + overlay listener state
//! - managing transient runtime interaction state
//! - buffering listener-lane input while listener data is stale
//! - installing fresh rebuild payloads from the tree actor
//!
//! Dispatch uses:
//!
//! - `base_registry` for listener state rebuilt from the retained tree
//! - `overlay_registry` for transient runtime follow-up listeners
//! - `LayeredRegistryView` to read both in one precedence order without
//!   materializing a merged registry
use std::{
    collections::{HashMap, VecDeque},
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use rustler::LocalPid;

use crate::{
    actors::{EventMsg, TreeMsg},
    backend::wake::BackendWakeHandle,
    clipboard::{ClipboardManager, ClipboardTarget},
    input::{ACTION_PRESS, InputEvent, InputHandler, SCROLL_LINE_PIXELS},
    keys::CanonicalKey,
    tree::{
        element::{ElementId, TextInputContentOrigin},
        scrollbar::ScrollbarAxis,
    },
};

use super::{
    CursorIcon, ElementEventKind, RegistryRebuildPayload, TextInputState, blur_atom, change_atom,
    click_atom, focus_atom, key_down_atom, key_press_atom, key_up_atom, mouse_down_atom,
    mouse_enter_atom, mouse_leave_atom, mouse_move_atom, mouse_up_atom, press_atom,
    registry_builder::{
        self, GestureAxis, ListenerAction, ListenerComputeCtx, ListenerInput, ListenerMatcherKind,
        RuntimeChange, RuntimeOverlayState,
    },
    scrollbar::ScrollbarNode,
    send_element_event, send_element_event_with_string_payload, send_input_event, swipe_down_atom,
    swipe_left_atom, swipe_right_atom, swipe_up_atom, virtual_key_hold_atom,
};

const PENDING_TEXT_PATCH_TTL: Duration = Duration::from_millis(50);
const DRAG_VELOCITY_SAMPLE_MIN_DT: Duration = Duration::from_millis(4);
const DRAG_VELOCITY_FILTER_ALPHA: f32 = 0.25;
const ADAPTIVE_SCROLL_FRICTION: f32 = 0.015;
const ADAPTIVE_SCROLL_INFLEXION: f32 = 0.35;
const ADAPTIVE_SCROLL_DECELERATION_RATE: f32 = 2.358_201_7;
const ADAPTIVE_SCROLL_PHYSICAL_COEFF: f32 = 51_890.203;
const ADAPTIVE_SCROLL_MIN_VELOCITY: f32 = 500.0;
const ADAPTIVE_SCROLL_MAX_VELOCITY: f32 = 6_000.0;
const ADAPTIVE_SCROLL_STOP_TOLERANCE: f32 = 0.5;
const ADAPTIVE_SCROLL_WATCHDOG_MAX_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PresentTimingState {
    presented_at: Instant,
    predicted_next_present_at: Instant,
}

#[derive(Clone, Debug, PartialEq)]
struct DragMotionState {
    element_id: ElementId,
    axis: ScrollbarAxis,
    last_pointer_axis: f32,
    last_sample_at: Instant,
    velocity_px_per_sec: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct AdaptiveScrollSimulation {
    initial_position: f32,
    initial_velocity: f32,
    duration_secs: f32,
    distance: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextCommitSuppression {
    element_id: ElementId,
    key: CanonicalKey,
}

impl AdaptiveScrollSimulation {
    fn new(initial_position: f32, initial_velocity: f32) -> Option<Self> {
        let abs_velocity = initial_velocity.abs();
        if abs_velocity < f32::EPSILON {
            return None;
        }

        let reference_velocity =
            ADAPTIVE_SCROLL_FRICTION * ADAPTIVE_SCROLL_PHYSICAL_COEFF / ADAPTIVE_SCROLL_INFLEXION;
        let android_duration = (abs_velocity / reference_velocity)
            .powf(1.0 / (ADAPTIVE_SCROLL_DECELERATION_RATE - 1.0));
        let duration_secs =
            ADAPTIVE_SCROLL_DECELERATION_RATE * ADAPTIVE_SCROLL_INFLEXION * android_duration;

        if !duration_secs.is_finite() || duration_secs <= 0.0 {
            return None;
        }

        let distance = initial_velocity * duration_secs / ADAPTIVE_SCROLL_DECELERATION_RATE;
        Some(Self {
            initial_position,
            initial_velocity,
            duration_secs,
            distance,
        })
    }

    fn x(&self, elapsed_secs: f32) -> f32 {
        let t = (elapsed_secs / self.duration_secs).clamp(0.0, 1.0);
        self.initial_position
            + self.distance * (1.0 - (1.0 - t).powf(ADAPTIVE_SCROLL_DECELERATION_RATE))
    }

    fn dx(&self, elapsed_secs: f32) -> f32 {
        let t = (elapsed_secs / self.duration_secs).clamp(0.0, 1.0);
        self.initial_velocity * (1.0 - t).powf(ADAPTIVE_SCROLL_DECELERATION_RATE - 1.0)
    }

    fn is_done(&self, elapsed_secs: f32) -> bool {
        elapsed_secs >= self.duration_secs
    }
}

#[derive(Clone, Debug, PartialEq)]
struct InertialScrollState {
    element_id: ElementId,
    axis: ScrollbarAxis,
    simulation: AdaptiveScrollSimulation,
    started_at: Instant,
    last_sample_position: f32,
    watchdog_deadline: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingTextPatch {
    content: String,
    expires_at: Instant,
}

/// Deferred effects collected from one listener dispatch.
///
/// Listener actions are processed in two phases:
///
/// - immediate side effects that must happen during collection
///   - Elixir event forwarding
///   - clipboard writes
/// - deferred effects that are flushed after collection
///   - runtime state changes
///   - tree messages
///
/// If a listener emits Elixir events but no tree messages, flushing may inject
/// `TreeMsg::RebuildRegistry` so the tree actor will send fresh listener data.
/// Plain `MouseMove` events are exempt so cursor motion can stay latest-wins
/// without forcing a rebuild cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DispatchMode {
    Normal,
    CursorRevalidate,
}

#[derive(Default)]
struct PendingDispatchEffects {
    tree_msgs: Vec<TreeMsg>,
    runtime_changes: Vec<RuntimeChange>,
    synthetic_inputs: Vec<Vec<InputEvent>>,
    elixir_event_requires_rebuild: bool,
    requested_cursor: Option<CursorIcon>,
}

impl PendingDispatchEffects {
    fn collect(
        mut self,
        runtime: &mut DirectEventRuntime,
        action: ListenerAction,
        dispatch_mode: DispatchMode,
    ) -> Self {
        match action {
            ListenerAction::TreeMsg(msg) => self.tree_msgs.push(msg),
            ListenerAction::RuntimeChange(change) => self.runtime_changes.push(change),
            ListenerAction::SyntheticInput(events) => self.synthetic_inputs.push(events),
            ListenerAction::SetCursor(icon) => self.requested_cursor = Some(icon),
            ListenerAction::ElixirEvent(event) => {
                if dispatch_mode == DispatchMode::CursorRevalidate
                    && event.kind == ElementEventKind::MouseMove
                {
                    return self;
                }

                if event.kind != ElementEventKind::MouseMove {
                    self.elixir_event_requires_rebuild = true;
                }
                runtime.send_elixir_event(event);
            }
            ListenerAction::ClipboardWrite { target, text } => {
                runtime.clipboard.set_text(target, &text);
            }
            ListenerAction::Semantic(_) => {
                unreachable!("listener compute must resolve semantic actions")
            }
        }

        self
    }

    fn flush(
        mut self,
        runtime: &mut DirectEventRuntime,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
        dispatch_mode: DispatchMode,
    ) {
        runtime.apply_runtime_changes_and_recompose_if_needed(self.runtime_changes);
        if let Some(icon) = self.requested_cursor {
            runtime.apply_cursor_request(icon);
        }

        #[cfg(not(feature = "hover-trace"))]
        let _ = dispatch_mode;

        for events in self.synthetic_inputs {
            runtime.inject_synthetic_inputs(events, tree_tx, log_render);
        }

        if self.elixir_event_requires_rebuild && self.tree_msgs.is_empty() {
            self.tree_msgs.push(TreeMsg::RebuildRegistry);
        }

        crate::debug_trace::hover_trace!(
            "event_dispatch",
            "mode={} hover_msgs={:?} requested_rebuild={} stale_before_flush={}",
            dispatch_mode_label(dispatch_mode),
            hover_msgs_from_tree_msgs(&self.tree_msgs)
                .iter()
                .map(|(id, active)| (id.0.clone(), *active))
                .collect::<Vec<_>>(),
            self.tree_msgs
                .iter()
                .any(|msg| matches!(msg, TreeMsg::RebuildRegistry)),
            runtime.listener_lane.is_stale()
        );

        if self.elixir_event_requires_rebuild || !self.tree_msgs.is_empty() {
            send_tree_messages(tree_tx, self.tree_msgs, log_render);
            runtime.listener_lane.mark_stale();
        }
    }
}

/// Runtime dispatch context passed into listener computation.
///
/// This exposes:
///
/// - focused element state
/// - current text input runtime state
/// - clipboard access
/// - base-only and layered redispatch helpers
struct RuntimeListenerComputeCtx<'a> {
    base_registry: &'a registry_builder::Registry,
    overlay_registry: &'a registry_builder::Registry,
    focused_id: Option<&'a ElementId>,
    hovered_id: Option<&'a ElementId>,
    text_states: &'a HashMap<ElementId, TextInputState>,
    text_commit_suppressions: &'a mut Vec<TextCommitSuppression>,
    clipboard: &'a mut ClipboardManager,
}

impl ListenerComputeCtx for RuntimeListenerComputeCtx<'_> {
    fn focused_id(&self) -> Option<&ElementId> {
        self.focused_id
    }

    fn hover_owner(&self) -> Option<&ElementId> {
        self.hovered_id
    }

    fn text_input_state(&self, element_id: &ElementId) -> Option<TextInputState> {
        self.text_states.get(element_id).cloned()
    }

    fn clipboard_text(&mut self, target: ClipboardTarget) -> Option<String> {
        self.clipboard.get_text(target)
    }

    fn take_text_commit_suppression(&mut self, element_id: &ElementId) -> bool {
        self.text_commit_suppressions
            .iter()
            .position(|suppression| &suppression.element_id == element_id)
            .map(|index| {
                self.text_commit_suppressions.remove(index);
            })
            .is_some()
    }

    fn dispatch_base(&mut self, input: &ListenerInput) -> Vec<ListenerAction> {
        self.base_registry.view().first_match(input, &[], self)
    }

    fn dispatch_effective_skip(
        &mut self,
        input: &ListenerInput,
        skip_matchers: &[ListenerMatcherKind],
    ) -> Vec<ListenerAction> {
        registry_builder::LayeredRegistryView::new(self.overlay_registry, self.base_registry)
            .first_match(input, skip_matchers, self)
    }

    fn base_first_match_listener(
        &self,
        input: &ListenerInput,
        skip_matchers: &[ListenerMatcherKind],
    ) -> Option<registry_builder::Listener> {
        self.base_registry
            .view()
            .matching_listener(input, skip_matchers)
            .cloned()
    }

    fn base_source_listener(
        &self,
        element_id: &ElementId,
        matcher_kind: ListenerMatcherKind,
    ) -> Option<registry_builder::Listener> {
        self.base_registry
            .view()
            .find_precedence(|listener| {
                listener.element_id.as_ref() == Some(element_id)
                    && listener.matcher.kind() == matcher_kind
            })
            .cloned()
    }
}

/// Freshness state for the listener-matching path.
///
/// While stale, listener input is buffered and coalesced until a fresh
/// `RegistryUpdate` is installed. Raw observer input forwarding continues
/// independently.
#[derive(Clone, Debug, Default)]
struct ListenerLaneState {
    stale: bool,
    buffered_inputs: Vec<InputEvent>,
}

impl ListenerLaneState {
    fn initially_stale() -> Self {
        Self {
            stale: true,
            buffered_inputs: Vec::new(),
        }
    }

    fn is_stale(&self) -> bool {
        self.stale
    }

    fn mark_stale(&mut self) {
        self.stale = true;
    }

    fn buffer_input(&mut self, event: InputEvent) {
        self.buffered_inputs.push(event);
        let mut buffered = std::mem::take(&mut self.buffered_inputs);
        self.buffered_inputs = coalesce_input_events(&mut buffered);
    }

    fn mark_fresh_and_take_buffered(&mut self) -> Vec<InputEvent> {
        self.stale = false;
        std::mem::take(&mut self.buffered_inputs)
    }
}

/// In-memory event actor runtime.
///
/// This holds:
///
/// - rebuilt listener state from the tree actor
/// - transient runtime interaction state
/// - text/scrollbar reconciliation state
/// - freshness/buffering state for listener dispatch
struct DirectEventRuntime {
    base_registry: registry_builder::Registry,
    runtime_overlay: RuntimeOverlayState,
    overlay_registry: registry_builder::Registry,
    listener_lane: ListenerLaneState,
    last_focus_on_mount_revision: u64,
    focused_id: Option<ElementId>,
    text_states: HashMap<ElementId, TextInputState>,
    text_commit_suppressions: Vec<TextCommitSuppression>,
    pending_text_patches: HashMap<ElementId, VecDeque<PendingTextPatch>>,
    scrollbar_nodes: HashMap<(ElementId, ScrollbarAxis), ScrollbarNode>,
    input_handler: InputHandler,
    input_target: Option<LocalPid>,
    clipboard: ClipboardManager,
    backend_cursor_tx: Option<Sender<CursorIcon>>,
    backend_wake: BackendWakeHandle,
    last_cursor_pos: Option<(f32, f32)>,
    cursor_in_window: bool,
    hovered_id: Option<ElementId>,
    current_cursor_icon: CursorIcon,
    virtual_key_deadline: Option<Instant>,
    last_present_timing: Option<PresentTimingState>,
    drag_motion: Option<DragMotionState>,
    inertial_scroll: Option<InertialScrollState>,
    suppress_drag_release_inertia: bool,
    scroll_line_pixels: f32,
}

impl DirectEventRuntime {
    #[cfg(test)]
    fn new(system_clipboard: bool) -> Self {
        Self::new_with_backend_cursor(system_clipboard, None, BackendWakeHandle::noop())
    }

    fn new_with_backend_cursor(
        system_clipboard: bool,
        backend_cursor_tx: Option<Sender<CursorIcon>>,
        backend_wake: BackendWakeHandle,
    ) -> Self {
        let base_registry = registry_builder::Registry::default();
        let runtime_overlay = RuntimeOverlayState::default();
        let overlay_registry =
            registry_builder::build_runtime_overlay_registry(&base_registry, &runtime_overlay);

        Self {
            base_registry,
            runtime_overlay,
            overlay_registry,
            listener_lane: ListenerLaneState::initially_stale(),
            last_focus_on_mount_revision: 0,
            focused_id: None,
            text_states: HashMap::new(),
            text_commit_suppressions: Vec::new(),
            pending_text_patches: HashMap::new(),
            scrollbar_nodes: HashMap::new(),
            input_handler: InputHandler::new(),
            input_target: None,
            clipboard: ClipboardManager::new(system_clipboard),
            backend_cursor_tx,
            backend_wake,
            last_cursor_pos: None,
            cursor_in_window: false,
            hovered_id: None,
            current_cursor_icon: CursorIcon::Default,
            virtual_key_deadline: None,
            last_present_timing: None,
            drag_motion: None,
            inertial_scroll: None,
            suppress_drag_release_inertia: false,
            scroll_line_pixels: SCROLL_LINE_PIXELS,
        }
    }

    fn set_scroll_line_pixels(&mut self, scroll_line_pixels: f32) {
        self.scroll_line_pixels = scroll_line_pixels;
    }

    fn set_input_mask(&mut self, mask: u32) {
        self.input_handler.set_mask(mask);
    }

    fn set_input_target(&mut self, target: Option<LocalPid>) {
        self.input_target = target;
    }

    fn note_present_timing(&mut self, presented_at: Instant, predicted_next_present_at: Instant) {
        self.last_present_timing = Some(PresentTimingState {
            presented_at,
            predicted_next_present_at,
        });
    }

    fn cancel_inertial_scroll(&mut self) {
        self.inertial_scroll = None;
    }

    fn cancel_inertial_scroll_for_input(&mut self, event: &InputEvent) {
        if matches!(
            event,
            InputEvent::CursorEntered { entered: false } | InputEvent::Focused { focused: false }
        ) && self.drag_motion.is_some()
        {
            self.suppress_drag_release_inertia = true;
        }

        let should_cancel = matches!(
            event,
            InputEvent::CursorButton {
                action: crate::input::ACTION_PRESS,
                ..
            } | InputEvent::CursorScroll { .. }
                | InputEvent::CursorScrollLines { .. }
                | InputEvent::CursorEntered { entered: false }
                | InputEvent::Focused { focused: false }
        );

        if should_cancel {
            self.cancel_inertial_scroll();
        }
    }

    fn pointer_axis_value(axis: ScrollbarAxis, x: f32, y: f32) -> f32 {
        match axis {
            ScrollbarAxis::X => x,
            ScrollbarAxis::Y => y,
        }
    }

    fn drag_axis_from_gesture(axis: GestureAxis) -> ScrollbarAxis {
        match axis {
            GestureAxis::Horizontal => ScrollbarAxis::X,
            GestureAxis::Vertical => ScrollbarAxis::Y,
        }
    }

    fn sync_drag_motion_start(
        &mut self,
        element_id: ElementId,
        locked_axis: GestureAxis,
        last_x: f32,
        last_y: f32,
        now: Instant,
    ) {
        let axis = Self::drag_axis_from_gesture(locked_axis);
        self.drag_motion = Some(DragMotionState {
            element_id,
            axis,
            last_pointer_axis: Self::pointer_axis_value(axis, last_x, last_y),
            last_sample_at: now,
            velocity_px_per_sec: 0.0,
        });
    }

    fn update_drag_motion(&mut self, last_x: f32, last_y: f32, now: Instant) {
        let Some(motion) = self.drag_motion.as_mut() else {
            return;
        };

        let next_axis = Self::pointer_axis_value(motion.axis, last_x, last_y);
        let dt = now.saturating_duration_since(motion.last_sample_at);
        if dt >= DRAG_VELOCITY_SAMPLE_MIN_DT {
            let dt_secs = dt.as_secs_f32();
            if dt_secs > 0.0 {
                let instantaneous = (next_axis - motion.last_pointer_axis) / dt_secs;
                let filtered = motion.velocity_px_per_sec;
                motion.velocity_px_per_sec = if filtered == 0.0 {
                    instantaneous
                } else {
                    filtered + (instantaneous - filtered) * DRAG_VELOCITY_FILTER_ALPHA
                };
                motion.last_sample_at = now;
            }
        }
        motion.last_pointer_axis = next_axis;
    }

    fn edge_blocks_inertial_scroll(
        &self,
        element_id: &ElementId,
        axis: ScrollbarAxis,
        velocity: f32,
    ) -> bool {
        let key = scrollbar_key(element_id, axis);
        let Some(node) = self.scrollbar_nodes.get(&key) else {
            return false;
        };

        if node.scroll_range <= f32::EPSILON {
            return true;
        }

        (node.scroll_offset <= f32::EPSILON && velocity > 0.0)
            || ((node.scroll_offset - node.scroll_range).abs() < f32::EPSILON && velocity < 0.0)
    }

    fn next_inertial_watchdog_deadline(&self, now: Instant) -> Instant {
        self.last_present_timing
            .map(|timing| {
                timing
                    .predicted_next_present_at
                    .min(now + ADAPTIVE_SCROLL_WATCHDOG_MAX_DELAY)
            })
            .unwrap_or(now + ADAPTIVE_SCROLL_WATCHDOG_MAX_DELAY)
    }

    fn clamp_inertial_scroll_delta(
        &self,
        element_id: &ElementId,
        axis: ScrollbarAxis,
        delta: f32,
    ) -> (f32, bool) {
        let Some(node) = self.scrollbar_nodes.get(&scrollbar_key(element_id, axis)) else {
            return (delta, false);
        };

        let clamped = if delta > 0.0 {
            delta.min(node.scroll_offset.max(0.0))
        } else if delta < 0.0 {
            -((-delta).min((node.scroll_range - node.scroll_offset).max(0.0)))
        } else {
            0.0
        };

        (
            clamped,
            (clamped - delta).abs() > ADAPTIVE_SCROLL_STOP_TOLERANCE,
        )
    }

    fn maybe_start_inertial_scroll(&mut self, now: Instant) {
        if self.suppress_drag_release_inertia {
            self.drag_motion = None;
            self.suppress_drag_release_inertia = false;
            return;
        }

        let Some(motion) = self.drag_motion.take() else {
            return;
        };

        let velocity = motion
            .velocity_px_per_sec
            .clamp(-ADAPTIVE_SCROLL_MAX_VELOCITY, ADAPTIVE_SCROLL_MAX_VELOCITY);

        if velocity.abs() < ADAPTIVE_SCROLL_MIN_VELOCITY
            || self.edge_blocks_inertial_scroll(&motion.element_id, motion.axis, velocity)
        {
            return;
        }

        let Some(simulation) = AdaptiveScrollSimulation::new(0.0, velocity) else {
            return;
        };

        self.inertial_scroll = Some(InertialScrollState {
            element_id: motion.element_id,
            axis: motion.axis,
            simulation,
            started_at: now,
            last_sample_position: 0.0,
            watchdog_deadline: self.next_inertial_watchdog_deadline(now),
        });
        self.backend_wake.request_redraw();
    }

    fn step_inertial_scroll(&mut self, now: Instant, tree_tx: &Sender<TreeMsg>, log_render: bool) {
        let Some(mut inertia) = self.inertial_scroll.take() else {
            return;
        };

        let elapsed_secs = now
            .saturating_duration_since(inertia.started_at)
            .as_secs_f32();
        let position = inertia.simulation.x(elapsed_secs);
        let delta = position - inertia.last_sample_position;

        if delta.abs() <= ADAPTIVE_SCROLL_STOP_TOLERANCE {
            if inertia.simulation.is_done(elapsed_secs) {
                return;
            }

            inertia.last_sample_position = position;
            inertia.watchdog_deadline = self.next_inertial_watchdog_deadline(now);
            self.inertial_scroll = Some(inertia);
            self.backend_wake.request_redraw();
            return;
        }

        let current_velocity = inertia.simulation.dx(elapsed_secs);
        if self.edge_blocks_inertial_scroll(&inertia.element_id, inertia.axis, current_velocity) {
            return;
        }

        let (delta, hit_boundary) =
            self.clamp_inertial_scroll_delta(&inertia.element_id, inertia.axis, delta);

        if delta.abs() > f32::EPSILON {
            send_tree(
                tree_tx,
                TreeMsg::ScrollRequest {
                    element_id: inertia.element_id.clone(),
                    dx: if inertia.axis == ScrollbarAxis::X {
                        delta
                    } else {
                        0.0
                    },
                    dy: if inertia.axis == ScrollbarAxis::Y {
                        delta
                    } else {
                        0.0
                    },
                },
                log_render,
            );
        }

        if hit_boundary || inertia.simulation.is_done(elapsed_secs) {
            return;
        }

        inertia.last_sample_position = position;
        inertia.watchdog_deadline = self.next_inertial_watchdog_deadline(now);
        self.inertial_scroll = Some(inertia);
        self.backend_wake.request_redraw();
    }

    fn next_timer_deadline(&self) -> Option<Instant> {
        self.virtual_key_deadline
            .into_iter()
            .chain(
                self.inertial_scroll
                    .as_ref()
                    .map(|inertia| inertia.watchdog_deadline),
            )
            .min()
    }

    fn handle_input_event(
        &mut self,
        event: InputEvent,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        let event = event.normalize_scroll_with_line_pixels(self.scroll_line_pixels);
        self.record_pointer_snapshot(&event);
        self.cancel_inertial_scroll_for_input(&event);
        crate::debug_trace::hover_trace!(
            "event_input",
            "event={:?} stale={} buffered={}",
            event,
            self.listener_lane.is_stale(),
            self.listener_lane.buffered_inputs.len()
        );
        forward_observer_input(&event, &self.input_handler, &self.input_target);
        self.clear_text_commit_suppressions_for_event(&event);

        if self.listener_lane.is_stale() {
            self.listener_lane.buffer_input(event);
            return;
        }

        self.dispatch_event(event, tree_tx, log_render, DispatchMode::Normal);
    }

    fn inject_synthetic_inputs(
        &mut self,
        events: Vec<InputEvent>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        for event in events {
            self.handle_input_event(event, tree_tx, log_render);
        }
    }

    fn handle_registry_update(
        &mut self,
        rebuild: RegistryRebuildPayload,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        let _stale_before_install = self.listener_lane.is_stale();
        self.listener_lane.stale = false;
        self.install_rebuild(rebuild, tree_tx, log_render);
        let _stale_after_install = self.listener_lane.is_stale();
        if self.listener_lane.is_stale() {
            return;
        }

        let buffered = self.listener_lane.mark_fresh_and_take_buffered();
        let _buffered_count = buffered.len();
        self.replay_buffered(buffered, tree_tx, log_render);
        let _stale_after_replay = self.listener_lane.is_stale();

        if !self.listener_lane.is_stale() && !self.has_active_pointer_overlay() {
            self.redispatch_last_cursor_pos(tree_tx, log_render);
        }
    }

    fn install_rebuild(
        &mut self,
        rebuild: RegistryRebuildPayload,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        let RegistryRebuildPayload {
            base_registry,
            text_inputs,
            scrollbars,
            focused_id,
            focus_on_mount,
        } = rebuild;

        self.prune_expired_pending_text_patches();
        self.base_registry = base_registry;
        self.scrollbar_nodes = scrollbars;

        self.reconcile_runtime_overlay(&text_inputs);
        self.recompose_overlay_registry();
        self.focused_id = focused_id;
        self.text_commit_suppressions.retain(|suppression| {
            self.focused_id
                .as_ref()
                .is_some_and(|focused_id| focused_id == &suppression.element_id)
        });

        let mut changed_tree = reconcile_text_input_states(
            &text_inputs,
            &mut self.text_states,
            &mut self.pending_text_patches,
            &self.focused_id,
            tree_tx,
            log_render,
        );

        if let Some(target) = focus_on_mount
            && target.mounted_at_revision > self.last_focus_on_mount_revision
        {
            self.last_focus_on_mount_revision = target.mounted_at_revision;
            changed_tree |= apply_focus_to(
                Some(target.element_id),
                &target.reveal_scrolls,
                &mut self.focused_id,
                &self.input_target,
                &mut self.text_states,
                tree_tx,
                log_render,
            );
        }

        if changed_tree {
            self.listener_lane.mark_stale();
        }
    }

    fn replay_buffered(
        &mut self,
        events: Vec<InputEvent>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        for event in events {
            if self.listener_lane.is_stale() {
                self.listener_lane.buffer_input(event);
                continue;
            }
            let dispatch_mode = match event {
                InputEvent::CursorPos { .. } => DispatchMode::CursorRevalidate,
                _ => DispatchMode::Normal,
            };
            self.dispatch_event(event, tree_tx, log_render, dispatch_mode);
        }
    }

    fn recompose_overlay_registry(&mut self) {
        self.overlay_registry = registry_builder::build_runtime_overlay_registry(
            &self.base_registry,
            &self.runtime_overlay,
        );
    }

    fn next_event_timeout(&self) -> Option<Duration> {
        self.next_timer_deadline().map(|deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(60))
        })
    }

    fn handle_timers(&mut self, tree_tx: &Sender<TreeMsg>, log_render: bool) {
        let now = Instant::now();

        if self
            .virtual_key_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.handle_virtual_key_timer(tree_tx, log_render);
        }

        if self
            .inertial_scroll
            .as_ref()
            .is_some_and(|inertia| inertia.watchdog_deadline <= now)
        {
            self.step_inertial_scroll(now, tree_tx, log_render);
        }
    }

    fn handle_present_timing(
        &mut self,
        presented_at: Instant,
        predicted_next_present_at: Instant,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        self.note_present_timing(presented_at, predicted_next_present_at);
        if self.inertial_scroll.is_none() {
            return;
        }

        self.step_inertial_scroll(presented_at, tree_tx, log_render);
    }

    fn handle_virtual_key_timer(&mut self, tree_tx: &Sender<TreeMsg>, log_render: bool) {
        let Some(tracker) = self.runtime_overlay.virtual_key.clone() else {
            self.virtual_key_deadline = None;
            return;
        };

        match tracker.phase {
            registry_builder::VirtualKeyPhase::Armed => match tracker.hold {
                crate::tree::attrs::VirtualKeyHoldMode::None => {
                    self.virtual_key_deadline = None;
                }
                crate::tree::attrs::VirtualKeyHoldMode::Event => {
                    self.runtime_overlay.virtual_key = None;
                    self.virtual_key_deadline = None;
                    self.recompose_overlay_registry();
                    self.apply_listener_actions(
                        vec![ListenerAction::ElixirEvent(registry_builder::ElixirEvent {
                            element_id: tracker.element_id,
                            kind: ElementEventKind::VirtualKeyHold,
                            payload: None,
                        })],
                        tree_tx,
                        log_render,
                        DispatchMode::Normal,
                    );
                }
                crate::tree::attrs::VirtualKeyHoldMode::Repeat => {
                    if let Some(active) = self.runtime_overlay.virtual_key.as_mut() {
                        active.phase = registry_builder::VirtualKeyPhase::Repeating;
                    }

                    self.virtual_key_deadline =
                        Some(Instant::now() + Duration::from_millis(u64::from(tracker.repeat_ms)));
                    self.recompose_overlay_registry();
                    self.inject_synthetic_inputs(
                        registry_builder::synthetic_input_sequence_for_virtual_key_tap(
                            &tracker.tap,
                        ),
                        tree_tx,
                        log_render,
                    );
                }
            },
            registry_builder::VirtualKeyPhase::Repeating => {
                self.virtual_key_deadline =
                    Some(Instant::now() + Duration::from_millis(u64::from(tracker.repeat_ms)));
                self.inject_synthetic_inputs(
                    registry_builder::synthetic_input_sequence_for_virtual_key_tap(&tracker.tap),
                    tree_tx,
                    log_render,
                );
            }
            registry_builder::VirtualKeyPhase::Cancelled => {
                self.virtual_key_deadline = None;
            }
        }
    }

    fn record_pointer_snapshot(&mut self, event: &InputEvent) {
        match event {
            InputEvent::CursorPos { x, y }
            | InputEvent::CursorScroll { x, y, .. }
            | InputEvent::CursorScrollLines { x, y, .. }
            | InputEvent::CursorButton { x, y, .. } => {
                self.last_cursor_pos = Some((*x, *y));
                self.cursor_in_window = true;
            }
            InputEvent::CursorEntered { entered } => {
                self.cursor_in_window = *entered;

                if !entered {
                    self.current_cursor_icon = CursorIcon::Default;
                }
            }
            _ => {}
        }
    }

    fn redispatch_last_cursor_pos(&mut self, tree_tx: &Sender<TreeMsg>, log_render: bool) {
        if !self.cursor_in_window {
            return;
        }

        let Some((x, y)) = self.last_cursor_pos else {
            return;
        };

        self.dispatch_event(
            InputEvent::CursorPos { x, y },
            tree_tx,
            log_render,
            DispatchMode::CursorRevalidate,
        );
    }

    fn has_active_pointer_overlay(&self) -> bool {
        self.runtime_overlay.click_press.is_some()
            || self.runtime_overlay.virtual_key.is_some()
            || !matches!(
                self.runtime_overlay.drag,
                registry_builder::DragTrackerState::Inactive
            )
            || self.runtime_overlay.swipe.is_some()
            || self.runtime_overlay.scrollbar.is_some()
            || self.runtime_overlay.text_drag.is_some()
    }

    fn apply_cursor_request(&mut self, icon: CursorIcon) {
        if self.current_cursor_icon != icon {
            self.current_cursor_icon = icon;

            if let Some(cursor_tx) = self.backend_cursor_tx.as_ref() {
                let _ = cursor_tx.send(icon);
            }

            self.backend_wake.request_redraw();
        }
    }

    fn should_preserve_registry_transitions(&self) -> bool {
        self.cursor_in_window
            && self.last_cursor_pos.is_some()
            && !self.has_active_pointer_overlay()
    }

    fn dispatch_event(
        &mut self,
        event: InputEvent,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
        dispatch_mode: DispatchMode,
    ) {
        let input = ListenerInput::Raw(event);
        let actions = {
            let mut ctx = RuntimeListenerComputeCtx {
                base_registry: &self.base_registry,
                overlay_registry: &self.overlay_registry,
                focused_id: self.focused_id.as_ref(),
                hovered_id: self.hovered_id.as_ref(),
                text_states: &self.text_states,
                text_commit_suppressions: &mut self.text_commit_suppressions,
                clipboard: &mut self.clipboard,
            };
            registry_builder::LayeredRegistryView::new(&self.overlay_registry, &self.base_registry)
                .first_match(&input, &[], &mut ctx)
        };

        if !actions.is_empty() {
            self.apply_listener_actions(actions, tree_tx, log_render, dispatch_mode);
        }
    }

    fn apply_listener_actions(
        &mut self,
        actions: Vec<ListenerAction>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
        dispatch_mode: DispatchMode,
    ) {
        // Apply the ordered action list produced by one matched listener.
        // Tree messages and runtime changes are collected first so they can be
        // flushed in a controlled order after action collection completes.
        actions
            .into_iter()
            .fold(PendingDispatchEffects::default(), |effects, action| {
                effects.collect(self, action, dispatch_mode)
            })
            .flush(self, tree_tx, log_render, dispatch_mode);
    }

    fn apply_runtime_changes_and_recompose_if_needed(
        &mut self,
        runtime_changes: Vec<RuntimeChange>,
    ) {
        // Runtime changes may add or remove overlay listeners, so overlay
        // listener state is rebuilt after the batch if any change requires it.
        let recompose = runtime_changes
            .iter()
            .any(RuntimeChange::requires_registry_recompose);

        runtime_changes
            .into_iter()
            .for_each(|change| self.apply_runtime_change(change));

        if recompose {
            self.recompose_overlay_registry();
        }
    }

    fn send_elixir_event(&self, event: registry_builder::ElixirEvent) {
        let Some(pid) = self.input_target else {
            return;
        };

        let atom = event_kind_to_atom(event.kind);
        match event.payload.as_deref() {
            Some(value) => {
                send_element_event_with_string_payload(pid, &event.element_id, atom, value)
            }
            None => send_element_event(pid, &event.element_id, atom),
        }
    }

    fn apply_runtime_change(&mut self, change: RuntimeChange) {
        let now = Instant::now();
        match change {
            RuntimeChange::StartClickPressTracker {
                element_id,
                matcher_kind,
                emit_click,
                emit_press_pointer,
            } => {
                self.runtime_overlay.click_press = Some(registry_builder::ClickPressTracker {
                    element_id,
                    matcher_kind,
                    emit_click,
                    emit_press_pointer,
                });
            }
            RuntimeChange::StartVirtualKeyTracker { tracker } => {
                self.virtual_key_deadline = match tracker.hold {
                    crate::tree::attrs::VirtualKeyHoldMode::Repeat
                    | crate::tree::attrs::VirtualKeyHoldMode::Event => {
                        Some(Instant::now() + Duration::from_millis(u64::from(tracker.hold_ms)))
                    }
                    crate::tree::attrs::VirtualKeyHoldMode::None => None,
                };
                self.runtime_overlay.virtual_key = Some(tracker);
            }
            RuntimeChange::StartKeyPressTracker { tracker } => {
                if !self.runtime_overlay.key_presses.contains(&tracker) {
                    self.runtime_overlay.key_presses.push(tracker);
                }
            }
            RuntimeChange::StartDragTracker {
                element_id,
                matcher_kind,
                origin_x,
                origin_y,
                swipe_handlers,
            } => {
                self.suppress_drag_release_inertia = false;
                self.runtime_overlay.drag = registry_builder::DragTrackerState::Candidate {
                    element_id,
                    matcher_kind,
                    origin_x,
                    origin_y,
                    swipe_handlers,
                };
            }
            RuntimeChange::PromoteDragTracker {
                element_id,
                matcher_kind,
                last_x,
                last_y,
                locked_axis,
            } => {
                self.suppress_drag_release_inertia = false;
                self.cancel_inertial_scroll();
                self.runtime_overlay.drag = registry_builder::DragTrackerState::Active {
                    element_id: element_id.clone(),
                    matcher_kind,
                    last_x,
                    last_y,
                    locked_axis,
                };
                self.sync_drag_motion_start(element_id, locked_axis, last_x, last_y, now);
            }
            RuntimeChange::ClearDragTracker => {
                self.runtime_overlay.drag = registry_builder::DragTrackerState::Inactive;
                self.maybe_start_inertial_scroll(now);
            }
            RuntimeChange::UpdateDragTrackerPointer { last_x, last_y } => {
                if let registry_builder::DragTrackerState::Active {
                    last_x: ref mut current_x,
                    last_y: ref mut current_y,
                    ..
                } = self.runtime_overlay.drag
                {
                    *current_x = last_x;
                    *current_y = last_y;
                }
                self.update_drag_motion(last_x, last_y, now);
            }
            RuntimeChange::ClearClickPressTracker => {
                self.runtime_overlay.click_press = None;
            }
            RuntimeChange::StartSwipeTracker { tracker } => {
                self.runtime_overlay.swipe = Some(tracker);
            }
            RuntimeChange::ClearSwipeTracker => {
                self.runtime_overlay.swipe = None;
            }
            RuntimeChange::CancelVirtualKeyTracker => {
                if let Some(ref mut tracker) = self.runtime_overlay.virtual_key {
                    tracker.phase = registry_builder::VirtualKeyPhase::Cancelled;
                }
                self.virtual_key_deadline = None;
            }
            RuntimeChange::ClearVirtualKeyTracker => {
                self.runtime_overlay.virtual_key = None;
                self.virtual_key_deadline = None;
            }
            RuntimeChange::ClearKeyPressTrackersForKey { key } => {
                self.runtime_overlay
                    .key_presses
                    .retain(|tracker| tracker.key != key);
            }
            RuntimeChange::ClearKeyPressTrackers => {
                self.runtime_overlay.key_presses.clear();
            }
            RuntimeChange::StartScrollbarDrag { tracker } => {
                self.cancel_inertial_scroll();
                self.runtime_overlay.scrollbar = Some(tracker);
            }
            RuntimeChange::UpdateScrollbarDragCurrentScroll { current_scroll } => {
                if let Some(ref mut tracker) = self.runtime_overlay.scrollbar {
                    tracker.current_scroll = current_scroll;
                }
            }
            RuntimeChange::ClearScrollbarDrag => {
                self.runtime_overlay.scrollbar = None;
            }
            RuntimeChange::StartTextDragTracker {
                element_id,
                matcher_kind,
            } => {
                self.runtime_overlay.text_drag = Some(registry_builder::TextDragTracker {
                    element_id,
                    matcher_kind,
                });
            }
            RuntimeChange::ClearTextDragTracker => {
                self.runtime_overlay.text_drag = None;
            }
            RuntimeChange::SetTextInputState { element_id, state } => {
                self.apply_text_input_state(&element_id, state);
            }
            RuntimeChange::ArmTextCommitSuppression { element_id, key } => {
                self.text_commit_suppressions
                    .push(TextCommitSuppression { element_id, key });
            }
            RuntimeChange::ExpectTextInputPatchValue {
                element_id,
                content,
            } => {
                self.enqueue_pending_text_patch(element_id, content);
            }
            RuntimeChange::SetHoverOwner { element_id } => {
                self.hovered_id = element_id;
            }
        }
    }

    fn apply_text_input_state(&mut self, element_id: &ElementId, state: TextInputState) {
        self.text_states.insert(element_id.clone(), state);
    }

    fn clear_text_commit_suppressions_for_event(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Key { key, action, .. } if *action == ACTION_PRESS => {
                self.text_commit_suppressions
                    .retain(|suppression| suppression.key == *key);
            }
            InputEvent::Focused { focused: false } => {
                self.text_commit_suppressions.clear();
            }
            _ => {}
        }
    }

    fn enqueue_pending_text_patch(&mut self, element_id: ElementId, content: String) {
        let now = Instant::now();
        let queue = self.pending_text_patches.entry(element_id).or_default();
        prune_expired_pending_text_patch_queue(queue, now);

        if let Some(existing) = queue.back_mut()
            && existing.content == content
        {
            existing.expires_at = now + PENDING_TEXT_PATCH_TTL;
        } else {
            queue.push_back(PendingTextPatch {
                content,
                expires_at: now + PENDING_TEXT_PATCH_TTL,
            });
        }
    }

    fn prune_expired_pending_text_patches(&mut self) {
        let now = Instant::now();
        self.pending_text_patches.retain(|_, queue| {
            prune_expired_pending_text_patch_queue(queue, now);
            !queue.is_empty()
        });
    }

    fn reconcile_runtime_overlay(&mut self, text_inputs: &HashMap<ElementId, TextInputState>) {
        if let Some(click_press) = self.runtime_overlay.click_press.as_ref()
            && !base_has_source_listener(
                &self.base_registry,
                &click_press.element_id,
                click_press.matcher_kind,
            )
        {
            self.runtime_overlay.click_press = None;
        }

        if self.runtime_overlay.virtual_key.is_none() {
            self.virtual_key_deadline = None;
        }

        self.runtime_overlay.key_presses.retain(|tracker| {
            registry_builder::base_has_key_press_source(&self.base_registry, tracker)
        });

        match self.runtime_overlay.drag {
            registry_builder::DragTrackerState::Inactive => {}
            registry_builder::DragTrackerState::Candidate {
                ref element_id,
                matcher_kind,
                ..
            }
            | registry_builder::DragTrackerState::Active {
                ref element_id,
                matcher_kind,
                ..
            } => {
                if !base_has_source_listener(&self.base_registry, element_id, matcher_kind) {
                    self.runtime_overlay.drag = registry_builder::DragTrackerState::Inactive;
                    self.drag_motion = None;
                }
            }
        }

        if self.drag_motion.as_ref().is_some_and(|motion| {
            !base_has_source_listener(
                &self.base_registry,
                &motion.element_id,
                ListenerMatcherKind::CursorButtonLeftPressInside,
            )
        }) {
            self.drag_motion = None;
        }

        if let Some(swipe) = self.runtime_overlay.swipe.as_ref()
            && !base_has_source_listener(&self.base_registry, &swipe.element_id, swipe.matcher_kind)
        {
            self.runtime_overlay.swipe = None;
        }

        if let Some(text_drag) = self.runtime_overlay.text_drag.as_ref()
            && !text_inputs.contains_key(&text_drag.element_id)
        {
            self.runtime_overlay.text_drag = None;
        }

        if let Some(ref mut tracker) = self.runtime_overlay.scrollbar {
            let key = scrollbar_key(&tracker.element_id, tracker.axis);
            if let Some(node) = self.scrollbar_nodes.get(&key).copied() {
                tracker.track_start = node.track_start;
                tracker.track_len = node.track_len;
                tracker.thumb_len = node.thumb_len;
                tracker.scroll_range = node.scroll_range;
                tracker.current_scroll = node.scroll_offset;
                tracker.pointer_offset = tracker.pointer_offset.clamp(0.0, node.thumb_len);
                tracker.screen_to_local = node.screen_to_local;
            } else {
                self.runtime_overlay.scrollbar = None;
            }
        }

        if self.inertial_scroll.as_ref().is_some_and(|inertia| {
            !self
                .scrollbar_nodes
                .contains_key(&scrollbar_key(&inertia.element_id, inertia.axis))
        }) {
            self.inertial_scroll = None;
        }

        self.hovered_id = self
            .base_registry
            .view()
            .find_precedence(|listener| {
                listener.matcher.kind() == ListenerMatcherKind::HoverLeaveCurrentOwner
            })
            .and_then(|listener| listener.element_id.clone());
    }
}

fn base_has_source_listener(
    base: &registry_builder::Registry,
    element_id: &ElementId,
    matcher_kind: ListenerMatcherKind,
) -> bool {
    base.view().any_precedence(|listener| {
        listener.element_id.as_ref() == Some(element_id) && listener.matcher.kind() == matcher_kind
    })
}

fn scrollbar_key(element_id: &ElementId, axis: ScrollbarAxis) -> (ElementId, ScrollbarAxis) {
    (element_id.clone(), axis)
}

fn send_tree(tree_tx: &Sender<TreeMsg>, msg: TreeMsg, log_render: bool) {
    match tree_tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking send");
            }
            let _ = tree_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn send_tree_messages(tree_tx: &Sender<TreeMsg>, msgs: Vec<TreeMsg>, log_render: bool) {
    match msgs.len() {
        0 => {}
        1 => send_tree(tree_tx, msgs.into_iter().next().unwrap(), log_render),
        _ => send_tree(tree_tx, TreeMsg::Batch(msgs), log_render),
    }
}

#[cfg(feature = "hover-trace")]
fn dispatch_mode_label(mode: DispatchMode) -> &'static str {
    match mode {
        DispatchMode::Normal => "normal",
        DispatchMode::CursorRevalidate => "cursor_revalidate",
    }
}

#[cfg(feature = "hover-trace")]
fn hover_msgs_from_tree_msgs(msgs: &[TreeMsg]) -> Vec<(ElementId, bool)> {
    let mut out = Vec::new();
    for msg in msgs {
        match msg {
            TreeMsg::SetMouseOverActive { element_id, active } => {
                out.push((element_id.clone(), *active));
            }
            TreeMsg::Batch(nested) => out.extend(hover_msgs_from_tree_msgs(nested)),
            _ => {}
        }
    }
    out
}

fn event_kind_to_atom(kind: ElementEventKind) -> rustler::Atom {
    match kind {
        ElementEventKind::Click => click_atom(),
        ElementEventKind::Press => press_atom(),
        ElementEventKind::SwipeUp => swipe_up_atom(),
        ElementEventKind::SwipeDown => swipe_down_atom(),
        ElementEventKind::SwipeLeft => swipe_left_atom(),
        ElementEventKind::SwipeRight => swipe_right_atom(),
        ElementEventKind::KeyDown => key_down_atom(),
        ElementEventKind::KeyUp => key_up_atom(),
        ElementEventKind::KeyPress => key_press_atom(),
        ElementEventKind::VirtualKeyHold => virtual_key_hold_atom(),
        ElementEventKind::MouseDown => mouse_down_atom(),
        ElementEventKind::MouseUp => mouse_up_atom(),
        ElementEventKind::MouseEnter => mouse_enter_atom(),
        ElementEventKind::MouseLeave => mouse_leave_atom(),
        ElementEventKind::MouseMove => mouse_move_atom(),
        ElementEventKind::Focus => focus_atom(),
        ElementEventKind::Blur => blur_atom(),
        ElementEventKind::Change => change_atom(),
    }
}

fn send_runtime_update(
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    element_id: &ElementId,
    state: &TextInputState,
) -> bool {
    send_tree(
        tree_tx,
        TreeMsg::SetTextInputRuntime {
            element_id: element_id.clone(),
            focused: state.focused,
            cursor: Some(state.cursor),
            selection_anchor: state.selection_anchor,
            preedit: state.preedit.clone(),
            preedit_cursor: state.preedit_cursor,
        },
        log_render,
    );
    true
}

fn send_content_update(
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    element_id: &ElementId,
    content: String,
) -> bool {
    send_tree(
        tree_tx,
        TreeMsg::SetTextInputContent {
            element_id: element_id.clone(),
            content,
        },
        log_render,
    );
    true
}

fn prune_expired_pending_text_patch_queue(queue: &mut VecDeque<PendingTextPatch>, now: Instant) {
    while queue
        .front()
        .is_some_and(|pending| pending.expires_at <= now)
    {
        queue.pop_front();
    }
}

fn consume_pending_text_patch_match(
    pending_text_patches: &mut HashMap<ElementId, VecDeque<PendingTextPatch>>,
    element_id: &ElementId,
    content: &str,
) -> bool {
    let matched = pending_text_patches
        .get_mut(element_id)
        .and_then(|queue| {
            let match_index = queue
                .iter()
                .position(|pending| pending.content == content)?;
            (0..=match_index).for_each(|_| {
                queue.pop_front();
            });
            Some(())
        })
        .is_some();

    if pending_text_patches
        .get(element_id)
        .is_some_and(|queue| queue.is_empty())
    {
        pending_text_patches.remove(element_id);
    }

    matched
}

fn reconcile_text_input_states(
    text_inputs: &HashMap<ElementId, TextInputState>,
    states: &mut HashMap<ElementId, TextInputState>,
    pending_text_patches: &mut HashMap<ElementId, VecDeque<PendingTextPatch>>,
    focused: &Option<ElementId>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
) -> bool {
    fn text_input_runtime_mismatch(rebuild: &TextInputState, state: &TextInputState) -> bool {
        rebuild.focused != state.focused
            || rebuild.cursor != state.cursor
            || rebuild.selection_anchor != state.selection_anchor
            || rebuild.preedit != state.preedit
            || rebuild.preedit_cursor != state.preedit_cursor
    }

    fn reconcile_focused_text_input(
        element_id: &ElementId,
        rebuild_state: &TextInputState,
        state: &mut TextInputState,
        pending_text_patches: &mut HashMap<ElementId, VecDeque<PendingTextPatch>>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) -> bool {
        fn preserve_runtime_focused_text_input(
            element_id: &ElementId,
            rebuild_state: &TextInputState,
            state: &mut TextInputState,
            tree_tx: &Sender<TreeMsg>,
            log_render: bool,
        ) -> bool {
            let mut changed_tree = false;

            state.copy_rebuild_metadata_from(rebuild_state);
            if rebuild_state.content_origin == TextInputContentOrigin::Event
                && state.content != rebuild_state.content
            {
                changed_tree |=
                    send_content_update(tree_tx, log_render, element_id, state.content.clone());
            }

            state.focused = true;
            state.normalize_runtime();

            if text_input_runtime_mismatch(rebuild_state, state) {
                changed_tree |= send_runtime_update(tree_tx, log_render, element_id, state);
            }

            changed_tree
        }

        fn accept_tree_patch_focused_text_input(
            element_id: &ElementId,
            rebuild_state: &TextInputState,
            state: &mut TextInputState,
            tree_tx: &Sender<TreeMsg>,
            log_render: bool,
        ) -> bool {
            *state = rebuild_state.clone();
            state.focused = true;
            state.normalize_runtime();

            text_input_runtime_mismatch(rebuild_state, state)
                && send_runtime_update(tree_tx, log_render, element_id, state)
        }

        let preserve_runtime = match rebuild_state.content_origin {
            TextInputContentOrigin::Event => true,
            TextInputContentOrigin::TreePatch => consume_pending_text_patch_match(
                pending_text_patches,
                element_id,
                &rebuild_state.content,
            ),
        };

        if preserve_runtime {
            preserve_runtime_focused_text_input(
                element_id,
                rebuild_state,
                state,
                tree_tx,
                log_render,
            )
        } else {
            pending_text_patches.remove(element_id);
            accept_tree_patch_focused_text_input(
                element_id,
                rebuild_state,
                state,
                tree_tx,
                log_render,
            )
        }
    }

    fn reset_unfocused_text_input_from_rebuild(
        state: &mut TextInputState,
        rebuild_state: &TextInputState,
    ) {
        *state = rebuild_state.clone();
        let cursor = state.cursor;
        let selection_anchor = state.selection_anchor;
        let preedit = state.preedit.clone();
        let preedit_cursor = state.preedit_cursor;
        state.set_runtime(
            false,
            Some(cursor),
            selection_anchor,
            preedit,
            preedit_cursor,
        );
    }

    let mut changed_tree = false;

    for (id, rebuild_state) in text_inputs {
        let id = id.clone();
        let should_focus = focused.as_ref().is_some_and(|focused_id| focused_id == &id);

        let state = states
            .entry(id.clone())
            .or_insert_with(|| rebuild_state.clone());

        if should_focus {
            changed_tree |= reconcile_focused_text_input(
                &id,
                rebuild_state,
                state,
                pending_text_patches,
                tree_tx,
                log_render,
            );
        } else {
            reset_unfocused_text_input_from_rebuild(state, rebuild_state);
        }
    }

    states.retain(|id, _| text_inputs.contains_key(id));
    pending_text_patches.retain(|id, queue| {
        text_inputs.contains_key(id)
            && focused.as_ref().is_some_and(|focused_id| focused_id == id)
            && !queue.is_empty()
    });
    changed_tree
}

fn apply_focus_to(
    next_focus: Option<ElementId>,
    reveal_scrolls: &[registry_builder::FocusRevealScroll],
    focused: &mut Option<ElementId>,
    target: &Option<LocalPid>,
    states: &mut HashMap<ElementId, TextInputState>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
) -> bool {
    fn blur_previous_focus(
        prev_id: ElementId,
        target: &Option<LocalPid>,
        states: &mut HashMap<ElementId, TextInputState>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) -> bool {
        if let Some(pid) = target.as_ref().copied() {
            send_element_event(pid, &prev_id, blur_atom());
        }

        send_tree(
            tree_tx,
            TreeMsg::SetFocusedActive {
                element_id: prev_id.clone(),
                active: false,
            },
            log_render,
        );

        let mut changed_tree = true;
        if let Some(state) = states.get_mut(&prev_id) {
            let cursor = state.cursor;
            state.set_runtime(false, Some(cursor), None, None, None);
            changed_tree |= send_runtime_update(tree_tx, log_render, &prev_id, state);
        }

        changed_tree
    }

    fn focus_next_element(
        next_id: ElementId,
        target: &Option<LocalPid>,
        states: &mut HashMap<ElementId, TextInputState>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) -> bool {
        if let Some(pid) = target.as_ref().copied() {
            send_element_event(pid, &next_id, focus_atom());
        }

        send_tree(
            tree_tx,
            TreeMsg::SetFocusedActive {
                element_id: next_id.clone(),
                active: true,
            },
            log_render,
        );

        let mut changed_tree = true;
        if let Some(state) = states.get_mut(&next_id) {
            let cursor = state.cursor;
            let selection_anchor = state.selection_anchor;
            let preedit = state.preedit.clone();
            let preedit_cursor = state.preedit_cursor;
            state.set_runtime(
                true,
                Some(cursor),
                selection_anchor,
                preedit,
                preedit_cursor,
            );
            changed_tree |= send_runtime_update(tree_tx, log_render, &next_id, state);
        }

        changed_tree
    }

    fn emit_reveal_scroll_requests(
        reveal_scrolls: &[registry_builder::FocusRevealScroll],
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) -> bool {
        reveal_scrolls.iter().fold(false, |_, reveal| {
            send_tree(
                tree_tx,
                TreeMsg::ScrollRequest {
                    element_id: reveal.element_id.clone(),
                    dx: reveal.dx,
                    dy: reveal.dy,
                },
                log_render,
            );
            true
        })
    }

    let previous_focus = focused.clone();
    *focused = next_focus.clone();

    if previous_focus == next_focus {
        return false;
    }

    let mut changed_tree = false;

    if let Some(prev_id) = previous_focus {
        changed_tree |= blur_previous_focus(prev_id, target, states, tree_tx, log_render);
    }

    if let Some(next_id) = next_focus {
        changed_tree |= focus_next_element(next_id, target, states, tree_tx, log_render);
    }

    changed_tree |= emit_reveal_scroll_requests(reveal_scrolls, tree_tx, log_render);

    changed_tree
}

fn coalesce_input_events(events: &mut Vec<InputEvent>) -> Vec<InputEvent> {
    let mut coalesced = Vec::new();
    let mut last_cursor: Option<InputEvent> = None;
    let mut scroll_acc: Option<(f32, f32, f32, f32)> = None;
    let mut last_resize: Option<InputEvent> = None;

    for event in events.drain(..) {
        let event = event.normalize_scroll();
        match event {
            InputEvent::CursorPos { .. } => {
                last_cursor = Some(event);
            }
            InputEvent::CursorScroll { dx, dy, x, y } => {
                scroll_acc = Some(match scroll_acc {
                    Some((acc_dx, acc_dy, _, _)) => (acc_dx + dx, acc_dy + dy, x, y),
                    None => (dx, dy, x, y),
                });
            }
            InputEvent::Resized { .. } => {
                last_resize = Some(event);
            }
            other => coalesced.push(other),
        }
    }

    if let Some((dx, dy, x, y)) = scroll_acc {
        coalesced.push(InputEvent::CursorScroll { dx, dy, x, y });
    }
    if let Some(resize) = last_resize {
        coalesced.push(resize);
    }
    if let Some(cursor) = last_cursor {
        coalesced.push(cursor);
    }

    coalesced
}

fn drain_fresh_input_events(
    initial_event: InputEvent,
    event_rx: &Receiver<EventMsg>,
    pending_message: &mut Option<EventMsg>,
) -> Vec<InputEvent> {
    if !matches!(initial_event, InputEvent::CursorPos { .. }) {
        return vec![initial_event];
    }

    let mut events = vec![initial_event];
    while let Ok(message) = event_rx.try_recv() {
        match message {
            EventMsg::InputEvent(event @ InputEvent::CursorPos { .. }) => events.push(event),
            other => {
                *pending_message = Some(other);
                break;
            }
        }
    }

    coalesce_input_events(&mut events)
}

fn forward_observer_input(
    event: &InputEvent,
    input_handler: &InputHandler,
    target: &Option<LocalPid>,
) {
    if input_handler.accepts(event)
        && let Some(pid) = target.as_ref()
    {
        send_input_event(*pid, event);
    }
}

pub(crate) fn spawn_event_actor(
    event_rx: Receiver<EventMsg>,
    tree_tx: Sender<TreeMsg>,
    backend_cursor_tx: Option<Sender<CursorIcon>>,
    backend_wake: BackendWakeHandle,
    scroll_line_pixels: f32,
    log_render: bool,
    system_clipboard: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut runtime = DirectEventRuntime::new_with_backend_cursor(
            system_clipboard,
            backend_cursor_tx,
            backend_wake,
        );
        runtime.set_scroll_line_pixels(scroll_line_pixels);
        let mut pending_message: Option<EventMsg> = None;

        loop {
            let message = match pending_message.take() {
                Some(message) => Some(message),
                None => match runtime.next_event_timeout() {
                    Some(timeout) => match event_rx.recv_timeout(timeout) {
                        Ok(message) => Some(message),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    },
                    None => match event_rx.recv() {
                        Ok(message) => Some(message),
                        Err(_) => return,
                    },
                },
            };

            let Some(message) = message else {
                runtime.handle_timers(&tree_tx, log_render);
                continue;
            };

            match message {
                EventMsg::InputEvent(event) => {
                    let events = drain_fresh_input_events(event, &event_rx, &mut pending_message);
                    for event in events {
                        runtime.handle_input_event(event, &tree_tx, log_render);
                    }
                }
                EventMsg::RegistryUpdate { rebuild } => {
                    let rebuild = if runtime.should_preserve_registry_transitions() {
                        rebuild
                    } else {
                        coalesce_registry_updates(rebuild, &event_rx, &mut pending_message).0
                    };
                    runtime.handle_registry_update(rebuild, &tree_tx, log_render)
                }
                EventMsg::PresentTiming {
                    presented_at,
                    predicted_next_present_at,
                } => runtime.handle_present_timing(
                    presented_at,
                    predicted_next_present_at,
                    &tree_tx,
                    log_render,
                ),
                EventMsg::SetInputMask(mask) => runtime.set_input_mask(mask),
                EventMsg::SetInputTarget(target) => runtime.set_input_target(target),
                EventMsg::Stop => return,
            }
        }
    })
}

fn coalesce_registry_updates(
    mut rebuild: RegistryRebuildPayload,
    event_rx: &Receiver<EventMsg>,
    pending_message: &mut Option<EventMsg>,
) -> (RegistryRebuildPayload, usize) {
    let mut coalesced_count = 0;
    while let Ok(message) = event_rx.try_recv() {
        match message {
            EventMsg::RegistryUpdate {
                rebuild: newer_rebuild,
            } => {
                rebuild = newer_rebuild;
                coalesced_count += 1;
            }
            other => {
                *pending_message = Some(other);
                break;
            }
        }
    }

    (rebuild, coalesced_count)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};

    use super::*;
    use crate::events::registry_builder::{self, FocusRevealScroll};
    use crate::events::test_support::AnimatedNearbyHitCase;
    use crate::events::{CursorIcon, FocusOnMountTarget, RegistryRebuildPayload};
    use crate::input::{ACTION_PRESS, ACTION_RELEASE};
    use crate::keys::CanonicalKey;
    use crate::tree::animation::{
        AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec,
    };
    use crate::tree::attrs::TextAlign;
    use crate::tree::attrs::{
        AlignX, AlignY, Attrs, KeyBindingMatch, KeyBindingSpec, Length, MouseOverAttrs,
        VirtualKeyHoldMode, VirtualKeySpec, VirtualKeyTapAction,
    };
    use crate::tree::element::ElementId;
    use crate::tree::element::{
        Element, ElementKind, ElementTree, Frame, NearbySlot, TextInputContentOrigin,
    };
    use crate::tree::layout::{Constraint, layout_and_refresh_default_with_animation};
    use crate::tree::render::render_tree;
    use crossbeam_channel::{bounded, unbounded};
    use std::time::{Duration, Instant};

    fn make_text_input_state(
        content: &str,
        cursor: u32,
        selection_anchor: Option<u32>,
        focused: bool,
    ) -> TextInputState {
        make_text_input_state_with_origin(
            content,
            TextInputContentOrigin::TreePatch,
            cursor,
            selection_anchor,
            focused,
        )
    }

    fn make_text_input_state_with_origin(
        content: &str,
        content_origin: TextInputContentOrigin,
        cursor: u32,
        selection_anchor: Option<u32>,
        focused: bool,
    ) -> TextInputState {
        TextInputState {
            content: content.to_string(),
            content_origin,
            content_len: content.chars().count() as u32,
            cursor,
            selection_anchor,
            preedit: None,
            preedit_cursor: None,
            focused,
            emit_change: false,
            multiline: false,
            frame_x: 0.0,
            frame_y: 0.0,
            frame_width: 100.0,
            frame_height: 20.0,
            inset_top: 0.0,
            inset_left: 0.0,
            inset_bottom: 0.0,
            inset_right: 0.0,
            screen_to_local: Some(crate::tree::transform::Affine2::identity()),
            text_align: TextAlign::Left,
            font_family: "default".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }

    fn drain_msgs(rx: &Receiver<TreeMsg>) -> Vec<TreeMsg> {
        let mut out = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            push_tree_msg_flat(msg, &mut out);
        }
        out
    }

    fn make_key_down_binding(key: CanonicalKey) -> KeyBindingSpec {
        KeyBindingSpec {
            route: format!("key_down:{}", key.atom_name()),
            key,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }
    }

    fn rebuild_with_focus(id: u8) -> RegistryRebuildPayload {
        RegistryRebuildPayload {
            focused_id: Some(ElementId::from_term_bytes(vec![id])),
            ..RegistryRebuildPayload::default()
        }
    }

    fn focus_on_mount_target(id: u8, mounted_at_revision: u64) -> FocusOnMountTarget {
        FocusOnMountTarget {
            element_id: ElementId::from_term_bytes(vec![id]),
            reveal_scrolls: Vec::new(),
            mounted_at_revision,
        }
    }

    #[test]
    fn coalesce_registry_updates_keeps_latest_consecutive_rebuild() {
        let (event_tx, event_rx) = bounded(8);
        event_tx
            .send(EventMsg::RegistryUpdate {
                rebuild: rebuild_with_focus(2),
            })
            .unwrap();
        event_tx
            .send(EventMsg::RegistryUpdate {
                rebuild: rebuild_with_focus(3),
            })
            .unwrap();

        let mut pending = None;
        let (rebuild, coalesced) =
            coalesce_registry_updates(rebuild_with_focus(1), &event_rx, &mut pending);

        assert_eq!(
            rebuild.focused_id,
            Some(ElementId::from_term_bytes(vec![3]))
        );
        assert_eq!(coalesced, 2);
        assert!(pending.is_none());
    }

    #[test]
    fn coalesce_registry_updates_stops_at_non_registry_message() {
        let (event_tx, event_rx) = bounded(8);
        event_tx
            .send(EventMsg::RegistryUpdate {
                rebuild: rebuild_with_focus(2),
            })
            .unwrap();
        event_tx
            .send(EventMsg::InputEvent(InputEvent::CursorEntered {
                entered: false,
            }))
            .unwrap();
        event_tx
            .send(EventMsg::RegistryUpdate {
                rebuild: rebuild_with_focus(3),
            })
            .unwrap();

        let mut pending = None;
        let (rebuild, coalesced) =
            coalesce_registry_updates(rebuild_with_focus(1), &event_rx, &mut pending);

        assert_eq!(
            rebuild.focused_id,
            Some(ElementId::from_term_bytes(vec![2]))
        );
        assert_eq!(coalesced, 1);
        assert!(matches!(
            pending,
            Some(EventMsg::InputEvent(InputEvent::CursorEntered {
                entered: false
            }))
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::RegistryUpdate { rebuild })
                if rebuild.focused_id == Some(ElementId::from_term_bytes(vec![3]))
        ));
    }

    fn drain_raw_msgs(rx: &Receiver<TreeMsg>) -> Vec<TreeMsg> {
        let mut out = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            out.push(msg);
        }
        out
    }

    fn probe_point(case: &AnimatedNearbyHitCase, label: &str) -> (f32, f32) {
        case.probes
            .iter()
            .find(|probe| probe.label == label)
            .map(|probe| probe.point)
            .expect("probe should exist")
    }

    fn push_tree_msg_flat(msg: TreeMsg, out: &mut Vec<TreeMsg>) {
        match msg {
            TreeMsg::Batch(messages) => {
                for nested in messages {
                    push_tree_msg_flat(nested, out);
                }
            }
            other => out.push(other),
        }
    }

    fn with_interaction(element: Element) -> Element {
        with_interaction_rect(element, 0.0, 0.0, 100.0, 40.0)
    }

    fn with_interaction_rect(
        mut element: Element,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Element {
        element.frame = Some(Frame {
            x,
            y,
            width,
            height,
            content_width: width,
            content_height: height,
        });
        element
    }

    fn make_element(id: u8, kind: ElementKind, attrs: Attrs) -> Element {
        Element::with_attrs(
            ElementId::from_term_bytes(vec![id]),
            kind,
            Vec::new(),
            attrs,
        )
    }

    fn with_frame(mut element: Element, frame: Frame) -> Element {
        element.frame = Some(frame);
        element
    }

    fn animated_width_move_rebuild_at(
        sample_ms: u64,
        hover_active: bool,
    ) -> RegistryRebuildPayload {
        let host_id = ElementId::from_term_bytes(vec![130]);
        let overlay_id = ElementId::from_term_bytes(vec![131]);

        let mut tree = ElementTree::new();

        let mut host_attrs = Attrs::default();
        host_attrs.width = Some(Length::Px(128.0));
        host_attrs.height = Some(Length::Px(82.0));
        let mut host = make_element(130, ElementKind::El, host_attrs);
        host.nearby
            .set(NearbySlot::InFront, Some(overlay_id.clone()));

        let mut from = Attrs::default();
        from.width = Some(Length::Px(96.0));
        from.move_x = Some(-16.0);

        let mut to = Attrs::default();
        to.width = Some(Length::Px(156.0));
        to.move_x = Some(26.0);

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.width = Some(Length::Px(128.0));
        overlay_attrs.height = Some(Length::Px(82.0));
        overlay_attrs.align_x = Some(AlignX::Center);
        overlay_attrs.align_y = Some(AlignY::Center);
        overlay_attrs.on_mouse_move = Some(true);
        overlay_attrs.mouse_over = Some(MouseOverAttrs::default());
        overlay_attrs.mouse_over_active = Some(hover_active);
        overlay_attrs.animate = Some(AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        });

        let overlay = make_element(131, ElementKind::El, overlay_attrs);

        tree.root = Some(host_id);
        tree.insert(host);
        tree.insert(overlay);

        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, start);
        layout_and_refresh_default_with_animation(
            &mut tree,
            Constraint::new(128.0, 82.0),
            1.0,
            &runtime,
            start + Duration::from_millis(sample_ms),
        )
        .event_rebuild
    }

    #[test]
    fn listener_lane_state_buffers_and_releases_coalesced_inputs() {
        let mut lane = ListenerLaneState::initially_stale();
        lane.buffer_input(InputEvent::CursorPos { x: 1.0, y: 1.0 });
        lane.buffer_input(InputEvent::CursorPos { x: 3.0, y: 4.0 });
        lane.buffer_input(InputEvent::CursorScroll {
            dx: 1.0,
            dy: -2.0,
            x: 3.0,
            y: 4.0,
        });
        lane.buffer_input(InputEvent::CursorScroll {
            dx: 2.0,
            dy: 1.0,
            x: 5.0,
            y: 6.0,
        });

        let buffered = lane.mark_fresh_and_take_buffered();
        assert_eq!(buffered.len(), 2);
        assert!(matches!(
            buffered[0],
            InputEvent::CursorScroll { dx, dy, x, y }
                if (dx - 3.0).abs() < f32::EPSILON
                    && (dy + 1.0).abs() < f32::EPSILON
                    && (x - 5.0).abs() < f32::EPSILON
                    && (y - 6.0).abs() < f32::EPSILON
        ));
        assert!(
            matches!(buffered[1], InputEvent::CursorPos { x, y } if (x - 3.0).abs() < f32::EPSILON && (y - 4.0).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn listener_lane_state_coalesces_resize_events_to_latest() {
        let mut lane = ListenerLaneState::initially_stale();
        lane.buffer_input(InputEvent::Resized {
            width: 320,
            height: 180,
            scale_factor: 1.0,
        });
        lane.buffer_input(InputEvent::Resized {
            width: 640,
            height: 360,
            scale_factor: 1.5,
        });
        lane.buffer_input(InputEvent::CursorPos { x: 10.0, y: 20.0 });

        let buffered = lane.mark_fresh_and_take_buffered();
        assert_eq!(buffered.len(), 2);
        assert!(matches!(
            buffered[0],
            InputEvent::Resized {
                width: 640,
                height: 360,
                scale_factor
            } if (scale_factor - 1.5).abs() < f32::EPSILON
        ));
        assert!(matches!(
            buffered[1],
            InputEvent::CursorPos { x, y }
                if (x - 10.0).abs() < f32::EPSILON && (y - 20.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn drain_fresh_input_events_coalesces_cursor_bursts_and_preserves_next_message() {
        let (event_tx, event_rx) = bounded(8);
        event_tx
            .send(EventMsg::InputEvent(InputEvent::CursorPos {
                x: 2.0,
                y: 3.0,
            }))
            .unwrap();
        event_tx
            .send(EventMsg::InputEvent(InputEvent::CursorPos {
                x: 4.0,
                y: 5.0,
            }))
            .unwrap();
        event_tx
            .send(EventMsg::InputEvent(InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 4.0,
                y: 5.0,
            }))
            .unwrap();

        let mut pending = None;
        let drained = drain_fresh_input_events(
            InputEvent::CursorPos { x: 1.0, y: 1.0 },
            &event_rx,
            &mut pending,
        );

        assert_eq!(drained.len(), 1);
        assert!(matches!(
            drained[0],
            InputEvent::CursorPos { x, y }
                if (x - 4.0).abs() < f32::EPSILON && (y - 5.0).abs() < f32::EPSILON
        ));
        assert!(matches!(
            pending,
            Some(EventMsg::InputEvent(InputEvent::CursorButton { button, action, .. }))
                if button == "left" && action == crate::input::ACTION_PRESS
        ));
    }

    #[test]
    fn apply_focus_to_switches_focus_and_emits_reveal_scrolls() {
        let previous_id = ElementId::from_term_bytes(vec![210]);
        let next_id = ElementId::from_term_bytes(vec![211]);
        let scroll_id = ElementId::from_term_bytes(vec![212]);

        let mut previous = make_text_input_state("prev", 4, None, true);
        previous.selection_anchor = Some(1);
        previous.preedit = Some("x".to_string());
        previous.preedit_cursor = Some((0, 1));
        let next = make_text_input_state("next", 0, None, false);

        let mut sessions =
            HashMap::from([(previous_id.clone(), previous), (next_id.clone(), next)]);
        let mut focused = Some(previous_id.clone());
        let (tree_tx, tree_rx) = bounded(32);

        assert!(apply_focus_to(
            Some(next_id.clone()),
            &[FocusRevealScroll {
                element_id: scroll_id.clone(),
                dx: 12.0,
                dy: -8.0,
            }],
            &mut focused,
            &None,
            &mut sessions,
            &tree_tx,
            false,
        ));

        assert_eq!(focused, Some(next_id.clone()));
        assert!(
            !sessions
                .get(&previous_id)
                .expect("previous session")
                .focused
        );
        assert!(sessions.get(&next_id).expect("next session").focused);

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == previous_id && !*active
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == next_id && *active
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == scroll_id
                    && (*dx - 12.0).abs() < f32::EPSILON
                    && (*dy + 8.0).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn install_rebuild_reconciles_text_sessions_and_registry() {
        let input_id = ElementId::from_term_bytes(vec![1]);
        let descriptor = make_text_input_state("hello", 2, None, true);
        let base_registry = registry_builder::Registry::default();
        let rebuild = RegistryRebuildPayload {
            base_registry,
            text_inputs: HashMap::from([(input_id.clone(), descriptor.clone())]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        let state = runtime.text_states.get(&input_id).expect("state created");
        assert_eq!(state, &descriptor);
        assert_eq!(runtime.focused_id, Some(input_id));
        assert!(drain_msgs(&tree_rx).is_empty());
        assert!(!runtime.listener_lane.is_stale());
    }

    #[test]
    fn install_rebuild_applies_focus_on_mount_once_for_new_target() {
        let input_id = ElementId::from_term_bytes(vec![9]);
        let rebuild = RegistryRebuildPayload {
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state("hello", 5, None, false),
            )]),
            focus_on_mount: Some(focus_on_mount_target(9, 1)),
            ..RegistryRebuildPayload::default()
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);

        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);

        assert_eq!(runtime.focused_id, Some(input_id.clone()));
        assert_eq!(runtime.last_focus_on_mount_revision, 1);
        assert!(
            runtime
                .text_states
                .get(&input_id)
                .expect("text state should exist")
                .focused
        );

        let first_msgs = drain_msgs(&tree_rx);
        assert!(first_msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == input_id && *active
        )));

        runtime.handle_registry_update(rebuild, &tree_tx, false);

        assert!(
            drain_msgs(&tree_rx).is_empty(),
            "same mount revision should not re-trigger focus_on_mount"
        );
    }

    #[test]
    fn install_rebuild_focus_on_mount_steals_existing_focus() {
        let previous_id = ElementId::from_term_bytes(vec![10]);
        let next_id = ElementId::from_term_bytes(vec![11]);
        let initial_rebuild = RegistryRebuildPayload {
            text_inputs: HashMap::from([(
                previous_id.clone(),
                make_text_input_state("prev", 4, None, true),
            )]),
            focused_id: Some(previous_id.clone()),
            ..RegistryRebuildPayload::default()
        };
        let mount_rebuild = RegistryRebuildPayload {
            text_inputs: HashMap::from([
                (
                    previous_id.clone(),
                    make_text_input_state("prev", 4, None, true),
                ),
                (
                    next_id.clone(),
                    make_text_input_state("next", 0, None, false),
                ),
            ]),
            focused_id: Some(previous_id.clone()),
            focus_on_mount: Some(focus_on_mount_target(11, 2)),
            ..RegistryRebuildPayload::default()
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);

        runtime.handle_registry_update(initial_rebuild, &tree_tx, false);
        drain_msgs(&tree_rx);

        runtime.handle_registry_update(mount_rebuild, &tree_tx, false);

        assert_eq!(runtime.focused_id, Some(next_id.clone()));
        assert_eq!(runtime.last_focus_on_mount_revision, 2);
        assert!(
            !runtime
                .text_states
                .get(&previous_id)
                .expect("previous text state should exist")
                .focused
        );
        assert!(
            runtime
                .text_states
                .get(&next_id)
                .expect("next text state should exist")
                .focused
        );

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == previous_id && !*active
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == next_id && *active
        )));
    }

    #[test]
    fn direct_runtime_dispatches_mouse_down_style_activation() {
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs::default());
        let element = with_interaction(make_element(20, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseDownActive { element_id, active }
                if *element_id == ElementId::from_term_bytes(vec![20]) && *active
        )));
    }

    #[test]
    fn direct_runtime_on_press_inside_release_clears_mouse_down_style() {
        let element_id = ElementId::from_term_bytes(vec![21]);

        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        attrs.mouse_down = Some(MouseOverAttrs::default());
        let element = with_interaction(make_element(21, ElementKind::El, attrs.clone()));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        let press_msgs = drain_msgs(&tree_rx);
        assert!(press_msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseDownActive { element_id: msg_id, active }
                if *msg_id == element_id && *active
        )));

        let mut active_attrs = attrs;
        active_attrs.mouse_down_active = Some(true);
        active_attrs.focused_active = Some(true);
        let active_element = with_interaction(make_element(21, ElementKind::El, active_attrs));
        let active_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[active_element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: Some(element_id.clone()),
            focus_on_mount: None,
        };
        runtime.handle_registry_update(active_rebuild, &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseDownActive { element_id: msg_id, active }
                if *msg_id == element_id && !*active
        )));
    }

    #[test]
    fn direct_runtime_dispatches_concrete_tab_focus_transition() {
        let mut first_attrs = Attrs::default();
        first_attrs.on_focus = Some(true);
        first_attrs.focused_active = Some(true);
        let first = with_interaction(make_element(30, ElementKind::El, first_attrs));

        let mut second_attrs = Attrs::default();
        second_attrs.on_focus = Some(true);
        let second = with_interaction(make_element(31, ElementKind::El, second_attrs));

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[first, second]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![30])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::Tab,
                action: crate::input::ACTION_PRESS,
                mods: 0,
            },
            &tree_tx,
            false,
        );

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == ElementId::from_term_bytes(vec![30]) && !*active
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetFocusedActive { element_id, active }
                if *element_id == ElementId::from_term_bytes(vec![31]) && *active
        )));
    }

    #[test]
    fn direct_runtime_hover_without_press_does_not_scroll() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(100.0);
        let element = with_interaction(make_element(40, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 15.0, y: 10.0 }, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(
            drain_msgs(&tree_rx)
                .iter()
                .all(|msg| !matches!(msg, TreeMsg::ScrollRequest { .. }))
        );
    }

    #[test]
    fn direct_runtime_on_press_without_scroll_match_stays_press_only_until_release() {
        let element_id = ElementId::from_term_bytes(vec![44]);

        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(44, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: Some(element_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 25.0, y: 10.0 }, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_RELEASE,
                mods: 0,
                x: 25.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(
            msgs.iter()
                .any(|msg| matches!(msg, TreeMsg::RebuildRegistry)),
            "pointer press release should still emit press and request rebuild"
        );
        assert!(matches!(
            runtime.runtime_overlay.drag,
            registry_builder::DragTrackerState::Inactive
        ));
        assert!(runtime.runtime_overlay.click_press.is_none());
    }

    #[test]
    fn direct_runtime_scrollable_only_element_drag_scrolls_after_threshold() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(100.0);
        let element = with_interaction(make_element(43, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 24.0, y: 10.0 }, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 30.0, y: 10.0 }, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == ElementId::from_term_bytes(vec![43])
                    && (*dx - 6.0).abs() < f32::EPSILON
                    && dy.abs() < f32::EPSILON
        )));
    }

    #[test]
    fn direct_runtime_clear_drag_tracker_starts_inertia_from_sampled_velocity() {
        let element_id = ElementId::from_term_bytes(vec![211]);
        let mut runtime = DirectEventRuntime::new(false);
        let now = Instant::now();

        runtime.runtime_overlay.drag = registry_builder::DragTrackerState::Active {
            element_id: element_id.clone(),
            matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
            last_x: 10.0,
            last_y: 10.0,
            locked_axis: GestureAxis::Horizontal,
        };
        runtime.sync_drag_motion_start(
            element_id.clone(),
            GestureAxis::Horizontal,
            10.0,
            10.0,
            now,
        );
        runtime.update_drag_motion(40.0, 10.0, now + Duration::from_millis(20));

        runtime.apply_runtime_change(RuntimeChange::ClearDragTracker);

        let inertia = runtime
            .inertial_scroll
            .as_ref()
            .expect("release velocity should start inertia");
        assert_eq!(inertia.element_id, element_id);
        assert_eq!(inertia.axis, ScrollbarAxis::X);
        assert!(inertia.simulation.initial_velocity.abs() >= ADAPTIVE_SCROLL_MIN_VELOCITY);
    }

    #[test]
    fn direct_runtime_present_timing_steps_inertia_with_adaptive_decay() {
        let element_id = ElementId::from_term_bytes(vec![212]);
        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        let now = Instant::now();

        runtime.scrollbar_nodes.insert(
            scrollbar_key(&element_id, ScrollbarAxis::X),
            ScrollbarNode {
                axis: ScrollbarAxis::X,
                track_rect: crate::tree::geometry::Rect::default(),
                thumb_rect: crate::tree::geometry::Rect::default(),
                track_start: 0.0,
                track_len: 80.0,
                thumb_start: 0.0,
                thumb_len: 20.0,
                scroll_offset: 20.0,
                scroll_range: 100.0,
                screen_to_local: None,
            },
        );
        runtime.inertial_scroll = Some(InertialScrollState {
            element_id: element_id.clone(),
            axis: ScrollbarAxis::X,
            simulation: AdaptiveScrollSimulation::new(0.0, 1_000.0)
                .expect("simulation should start"),
            started_at: now,
            last_sample_position: 0.0,
            watchdog_deadline: now + Duration::from_millis(18),
        });

        runtime.handle_present_timing(
            now + Duration::from_millis(10),
            now + Duration::from_millis(18),
            &tree_tx,
            false,
        );

        let first_msgs = drain_msgs(&tree_rx);
        let first_dx = first_msgs
            .iter()
            .find_map(|msg| match msg {
                TreeMsg::ScrollRequest {
                    element_id: id,
                    dx,
                    dy,
                } if *id == element_id && dy.abs() < f32::EPSILON => Some(*dx),
                _ => None,
            })
            .expect("first present should emit inertial scroll");
        assert!(first_dx > 0.0);

        runtime.handle_present_timing(
            now + Duration::from_millis(20),
            now + Duration::from_millis(28),
            &tree_tx,
            false,
        );

        let second_msgs = drain_msgs(&tree_rx);
        let second_dx = second_msgs
            .iter()
            .find_map(|msg| match msg {
                TreeMsg::ScrollRequest {
                    element_id: id,
                    dx,
                    dy,
                } if *id == element_id && dy.abs() < f32::EPSILON => Some(*dx),
                _ => None,
            })
            .expect("second present should emit inertial scroll");
        assert!(second_dx > 0.0);
        assert!(second_dx < first_dx);

        let inertia = runtime
            .inertial_scroll
            .as_ref()
            .expect("inertia should continue after one frame");
        assert!(inertia.last_sample_position > first_dx);
        assert_eq!(inertia.watchdog_deadline, now + Duration::from_millis(28));
    }

    #[test]
    fn direct_runtime_watchdog_timer_steps_inertia_without_present_timing() {
        let element_id = ElementId::from_term_bytes(vec![213]);
        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        let now = Instant::now();

        runtime.scrollbar_nodes.insert(
            scrollbar_key(&element_id, ScrollbarAxis::Y),
            ScrollbarNode {
                axis: ScrollbarAxis::Y,
                track_rect: crate::tree::geometry::Rect::default(),
                thumb_rect: crate::tree::geometry::Rect::default(),
                track_start: 0.0,
                track_len: 80.0,
                thumb_start: 0.0,
                thumb_len: 20.0,
                scroll_offset: 20.0,
                scroll_range: 100.0,
                screen_to_local: None,
            },
        );
        runtime.last_present_timing = Some(PresentTimingState {
            presented_at: now - Duration::from_millis(16),
            predicted_next_present_at: now - Duration::from_millis(1),
        });
        runtime.inertial_scroll = Some(InertialScrollState {
            element_id: element_id.clone(),
            axis: ScrollbarAxis::Y,
            simulation: AdaptiveScrollSimulation::new(0.0, -800.0)
                .expect("simulation should start"),
            started_at: now - Duration::from_millis(12),
            last_sample_position: 0.0,
            watchdog_deadline: now - Duration::from_millis(1),
        });

        runtime.handle_timers(&tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id: id, dx, dy }
                if *id == element_id && dx.abs() < f32::EPSILON && *dy < -1.0
        )));
    }

    #[test]
    fn direct_runtime_pointer_press_and_blur_leave_cancel_inertia() {
        let element_id = ElementId::from_term_bytes(vec![214]);
        let (tree_tx, _tree_rx) = bounded(8);
        let mut runtime = DirectEventRuntime::new(false);
        let now = Instant::now();

        let make_inertia = || InertialScrollState {
            element_id: element_id.clone(),
            axis: ScrollbarAxis::X,
            simulation: AdaptiveScrollSimulation::new(0.0, 900.0).expect("simulation should start"),
            started_at: now,
            last_sample_position: 0.0,
            watchdog_deadline: now + Duration::from_millis(16),
        };

        runtime.inertial_scroll = Some(make_inertia());
        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 0.0,
                y: 0.0,
            },
            &tree_tx,
            false,
        );
        assert!(runtime.inertial_scroll.is_none());

        runtime.inertial_scroll = Some(make_inertia());
        runtime.handle_input_event(InputEvent::Focused { focused: false }, &tree_tx, false);
        assert!(runtime.inertial_scroll.is_none());

        runtime.inertial_scroll = Some(make_inertia());
        runtime.handle_input_event(
            InputEvent::CursorEntered { entered: false },
            &tree_tx,
            false,
        );
        assert!(runtime.inertial_scroll.is_none());
    }

    #[test]
    fn direct_runtime_nested_drag_scroll_prefers_child_over_parent() {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(10.0);
        parent_attrs.scroll_y_max = Some(100.0);
        let mut parent = with_interaction(make_element(73, ElementKind::El, parent_attrs));
        parent.children = vec![ElementId::from_term_bytes(vec![74])];

        let mut child_attrs = Attrs::default();
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(20.0);
        child_attrs.scroll_y_max = Some(100.0);
        let child = with_interaction(make_element(74, ElementKind::El, child_attrs));

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[parent, child]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );
        let _ = drain_msgs(&tree_rx);

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 24.0 }, &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 30.0 }, &tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == ElementId::from_term_bytes(vec![74])
                    && dx.abs() < f32::EPSILON
                    && (*dy - 6.0).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn direct_runtime_wheel_scroll_propagates_to_parent_when_child_direction_blocked() {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(10.0);
        parent_attrs.scroll_y_max = Some(100.0);
        let mut parent = with_interaction(make_element(75, ElementKind::El, parent_attrs));
        parent.children = vec![ElementId::from_term_bytes(vec![76])];

        let mut child_attrs = Attrs::default();
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(100.0);
        child_attrs.scroll_y_max = Some(100.0);
        let child = with_interaction(make_element(76, ElementKind::El, child_attrs));

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[parent, child]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorScroll {
                dx: 0.0,
                dy: -6.0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == ElementId::from_term_bytes(vec![75])
                    && dx.abs() < f32::EPSILON
                    && (*dy + 6.0).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn direct_runtime_batches_multiple_tree_messages_from_single_scroll_dispatch() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_x_max = Some(50.0);
        attrs.scroll_y_max = Some(40.0);
        let element = with_interaction(make_element(88, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorScroll {
                dx: -12.0,
                dy: -6.0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );

        let raw = drain_raw_msgs(&tree_rx);
        assert!(matches!(raw.as_slice(), [TreeMsg::Batch(msgs)] if msgs.len() == 2));
        let flat: Vec<_> = raw
            .into_iter()
            .flat_map(|msg| {
                let mut out = Vec::new();
                push_tree_msg_flat(msg, &mut out);
                out
            })
            .collect();
        assert!(flat.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == ElementId::from_term_bytes(vec![88])
                    && (*dx + 12.0).abs() < f32::EPSILON
                    && dy.abs() < f32::EPSILON
        )));
        assert!(flat.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == ElementId::from_term_bytes(vec![88])
                    && dx.abs() < f32::EPSILON
                    && (*dy + 6.0).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn direct_runtime_drag_scroll_propagates_to_parent_when_child_direction_blocked() {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(10.0);
        parent_attrs.scroll_y_max = Some(100.0);
        let mut parent = with_interaction(make_element(77, ElementKind::El, parent_attrs));
        parent.children = vec![ElementId::from_term_bytes(vec![78])];

        let mut child_attrs = Attrs::default();
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(100.0);
        child_attrs.scroll_y_max = Some(100.0);
        let child = with_interaction(make_element(78, ElementKind::El, child_attrs));

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[parent, child]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );
        let _ = drain_msgs(&tree_rx);

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 24.0 }, &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 18.0 }, &tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollRequest { element_id, dx, dy }
                if *element_id == ElementId::from_term_bytes(vec![77])
                    && dx.abs() < f32::EPSILON
                    && (*dy + 6.0).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn direct_runtime_scrollbar_thumb_press_and_move_drags_thumb() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(20.0);
        let element = with_frame(
            with_interaction(make_element(79, ElementKind::El, attrs)),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );
        let mut tree = ElementTree::new();
        tree.root = Some(ElementId::from_term_bytes(vec![79]));
        tree.insert(element);
        let rebuild = render_tree(&tree).event_rebuild;

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 96.0,
                y: 12.0,
            },
            &tree_tx,
            false,
        );

        let msgs = drain_msgs(&tree_rx);
        assert!(runtime.runtime_overlay.scrollbar.is_some());
        assert!(msgs.is_empty());

        let rebuild = render_tree(&tree).event_rebuild;
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 96.0, y: 20.0 }, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::ScrollbarThumbDragY { element_id, .. }
                if *element_id == ElementId::from_term_bytes(vec![79])
        )));
    }

    #[test]
    fn direct_runtime_release_then_move_does_not_start_drag_scroll() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(100.0);
        let element = with_interaction(make_element(41, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &tree_tx,
            false,
        );
        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(
            msgs.iter()
                .any(|msg| matches!(msg, TreeMsg::RebuildRegistry))
        );

        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 40.0, y: 10.0 }, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(
            msgs.iter()
                .all(|msg| !matches!(msg, TreeMsg::ScrollRequest { .. }))
        );
    }

    #[test]
    fn direct_runtime_elixir_only_mouse_move_stays_fresh_without_rebuild() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_move = Some(true);
        let element = with_interaction(make_element(42, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(
            msgs.iter()
                .all(|msg| !matches!(msg, TreeMsg::RebuildRegistry))
        );
    }

    #[test]
    fn direct_runtime_text_commit_updates_content() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        let element = with_interaction(make_element(50, ElementKind::TextInput, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![50]),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![50])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::TextCommit {
                text: "c".to_string(),
                mods: 0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == ElementId::from_term_bytes(vec![50]) && content == "abc"
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputRuntime { element_id, cursor, .. }
                if *element_id == ElementId::from_term_bytes(vec![50]) && *cursor == Some(3)
        )));
        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![50]))
            .expect("session updated after commit");
        assert_eq!(session.content, "abc");
        assert_eq!(session.cursor, 3);
    }

    #[test]
    fn direct_runtime_key_down_binding_suppresses_buffered_text_commit() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        attrs.focused_active = Some(true);
        attrs.on_key_down = Some(vec![make_key_down_binding(CanonicalKey::A)]);
        let element = with_interaction(make_element(150, ElementKind::TextInput, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![150]),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![150])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::A,
                action: ACTION_PRESS,
                mods: 0,
            },
            &tree_tx,
            false,
        );
        assert!(runtime.listener_lane.is_stale());
        let _ = drain_msgs(&tree_rx);

        runtime.handle_input_event(
            InputEvent::TextCommit {
                text: "a".to_string(),
                mods: 0,
            },
            &tree_tx,
            false,
        );
        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::A,
                action: ACTION_RELEASE,
                mods: 0,
            },
            &tree_tx,
            false,
        );

        runtime.handle_registry_update(rebuild, &tree_tx, false);

        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![150]))
            .expect("session preserved after suppression");
        assert_eq!(session.content, "ab");
        assert_eq!(session.cursor, 2);

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, .. }
                if *element_id == ElementId::from_term_bytes(vec![150])
        )));
    }

    #[test]
    fn direct_runtime_multiline_enter_inserts_newline() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        let element = with_interaction(make_element(151, ElementKind::Multiline, attrs));
        let mut state = make_text_input_state("ab", 2, None, true);
        state.multiline = true;
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(ElementId::from_term_bytes(vec![151]), state)]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![151])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::Enter,
                action: ACTION_PRESS,
                mods: 0,
            },
            &tree_tx,
            false,
        );

        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![151]))
            .expect("multiline session updated");
        assert_eq!(session.content, "ab\n");
        assert_eq!(session.cursor, 3);
    }

    #[test]
    fn direct_runtime_multiline_enter_suppresses_following_text_commit_newline() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        let element = with_interaction(make_element(153, ElementKind::Multiline, attrs));
        let mut state = make_text_input_state("ab", 2, None, true);
        state.multiline = true;
        let element_id = ElementId::from_term_bytes(vec![153]);
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(element_id.clone(), state)]),
            scrollbars: HashMap::new(),
            focused_id: Some(element_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::Enter,
                action: ACTION_PRESS,
                mods: 0,
            },
            &tree_tx,
            false,
        );

        let _ = drain_msgs(&tree_rx);

        runtime.handle_input_event(
            InputEvent::TextCommit {
                text: "\n".to_string(),
                mods: 0,
            },
            &tree_tx,
            false,
        );

        let session = runtime
            .text_states
            .get(&element_id)
            .expect("multiline session preserved after text commit");
        assert_eq!(session.content, "ab\n");
        assert_eq!(session.cursor, 3);

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id: msg_id, content }
                if *msg_id == element_id && content == "ab\n\n"
        )));
    }

    #[test]
    fn direct_runtime_multiline_key_down_binding_suppresses_enter_default() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        attrs.focused_active = Some(true);
        attrs.on_key_down = Some(vec![make_key_down_binding(CanonicalKey::Enter)]);
        let element = with_interaction(make_element(152, ElementKind::Multiline, attrs));
        let mut state = make_text_input_state("ab", 2, None, true);
        state.multiline = true;
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(ElementId::from_term_bytes(vec![152]), state)]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![152])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::Enter,
                action: ACTION_PRESS,
                mods: 0,
            },
            &tree_tx,
            false,
        );

        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![152]))
            .expect("multiline session preserved");
        assert_eq!(session.content, "ab");

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, .. }
                if *element_id == ElementId::from_term_bytes(vec![152])
        )));
    }

    #[test]
    fn direct_runtime_virtual_key_release_commits_text_to_focused_input() {
        let mut text_attrs = Attrs::default();
        text_attrs.content = Some("ab".to_string());
        text_attrs.text_input_focused = Some(true);
        text_attrs.text_input_cursor = Some(2);
        let text_input = with_interaction(make_element(80, ElementKind::TextInput, text_attrs));

        let mut key_attrs = Attrs::default();
        key_attrs.virtual_key = Some(VirtualKeySpec {
            tap: VirtualKeyTapAction::Text("c".to_string()),
            hold: VirtualKeyHoldMode::None,
            hold_ms: 350,
            repeat_ms: 40,
        });
        let soft_key = with_interaction_rect(
            make_element(81, ElementKind::El, key_attrs),
            0.0,
            50.0,
            100.0,
            40.0,
        );

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[text_input, soft_key]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![80]),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![80])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 60.0,
            },
            &tree_tx,
            false,
        );
        let _ = drain_msgs(&tree_rx);
        assert!(runtime.runtime_overlay.virtual_key.is_some());

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 60.0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        assert!(runtime.runtime_overlay.virtual_key.is_none());

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == ElementId::from_term_bytes(vec![80]) && content == "abc"
        )));

        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![80]))
            .expect("session updated after virtual key tap");
        assert_eq!(session.content, "abc");
        assert_eq!(session.cursor, 3);
    }

    #[test]
    fn direct_runtime_virtual_key_text_and_key_respects_key_down_suppression() {
        let mut text_attrs = Attrs::default();
        text_attrs.content = Some("ab".to_string());
        text_attrs.text_input_focused = Some(true);
        text_attrs.text_input_cursor = Some(2);
        text_attrs.focused_active = Some(true);
        text_attrs.on_key_down = Some(vec![make_key_down_binding(CanonicalKey::A)]);
        let text_input = with_interaction(make_element(180, ElementKind::TextInput, text_attrs));

        let mut key_attrs = Attrs::default();
        key_attrs.virtual_key = Some(VirtualKeySpec {
            tap: VirtualKeyTapAction::TextAndKey {
                text: "a".to_string(),
                key: CanonicalKey::A,
                mods: 0,
            },
            hold: VirtualKeyHoldMode::None,
            hold_ms: 350,
            repeat_ms: 40,
        });
        let soft_key = with_interaction_rect(
            make_element(181, ElementKind::El, key_attrs),
            0.0,
            50.0,
            100.0,
            40.0,
        );

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[text_input, soft_key]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![180]),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![180])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 60.0,
            },
            &tree_tx,
            false,
        );
        let _ = drain_msgs(&tree_rx);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 60.0,
            },
            &tree_tx,
            false,
        );

        runtime.handle_registry_update(rebuild, &tree_tx, false);

        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![180]))
            .expect("session preserved after suppressed virtual key tap");
        assert_eq!(session.content, "ab");

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, .. }
                if *element_id == ElementId::from_term_bytes(vec![180])
        )));
    }

    #[test]
    fn direct_runtime_virtual_key_repeat_stops_after_slide_off_until_repress() {
        let mut text_attrs = Attrs::default();
        text_attrs.content = Some("ab".to_string());
        text_attrs.text_input_focused = Some(true);
        text_attrs.text_input_cursor = Some(2);
        let text_input = with_interaction(make_element(82, ElementKind::TextInput, text_attrs));

        let mut key_attrs = Attrs::default();
        key_attrs.virtual_key = Some(VirtualKeySpec {
            tap: VirtualKeyTapAction::Text("x".to_string()),
            hold: VirtualKeyHoldMode::Repeat,
            hold_ms: 350,
            repeat_ms: 40,
        });
        let soft_key = with_interaction_rect(
            make_element(83, ElementKind::El, key_attrs),
            0.0,
            50.0,
            100.0,
            40.0,
        );

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[
                text_input.clone(),
                soft_key.clone(),
            ]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![82]),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![82])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 60.0,
            },
            &tree_tx,
            false,
        );
        assert!(matches!(
            runtime
                .runtime_overlay
                .virtual_key
                .as_ref()
                .map(|tracker| tracker.phase),
            Some(registry_builder::VirtualKeyPhase::Armed)
        ));

        runtime.handle_virtual_key_timer(&tree_tx, false);

        assert!(matches!(
            runtime
                .runtime_overlay
                .virtual_key
                .as_ref()
                .map(|tracker| tracker.phase),
            Some(registry_builder::VirtualKeyPhase::Repeating)
        ));

        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == ElementId::from_term_bytes(vec![82]) && content == "abx"
        )));

        let repeat_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[text_input, soft_key]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![82]),
                make_text_input_state_with_origin(
                    "abx",
                    TextInputContentOrigin::Event,
                    3,
                    None,
                    true,
                ),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![82])),
            focus_on_mount: None,
        };
        runtime.handle_registry_update(repeat_rebuild, &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 140.0, y: 60.0 }, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 60.0 }, &tree_tx, false);

        assert!(matches!(
            runtime
                .runtime_overlay
                .virtual_key
                .as_ref()
                .map(|tracker| tracker.phase),
            Some(registry_builder::VirtualKeyPhase::Cancelled)
        ));
        assert!(runtime.virtual_key_deadline.is_none());

        runtime.handle_virtual_key_timer(&tree_tx, false);
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == ElementId::from_term_bytes(vec![82]) && content == "abxx"
        )));

        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![82]))
            .expect("session retained after repeat cancel");
        assert_eq!(session.content, "abx");
    }

    #[test]
    fn direct_runtime_backspace_updates_content() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        let element = with_interaction(make_element(51, ElementKind::TextInput, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![51]),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![51])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::Key {
                key: CanonicalKey::Backspace,
                action: crate::input::ACTION_PRESS,
                mods: 0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == ElementId::from_term_bytes(vec![51]) && content == "a"
        )));
        let session = runtime
            .text_states
            .get(&ElementId::from_term_bytes(vec![51]))
            .expect("session updated after backspace");
        assert_eq!(session.content, "a");
        assert_eq!(session.cursor, 1);
    }

    #[test]
    fn direct_runtime_focused_text_commit_survives_followup_rebuild() {
        let input_id = ElementId::from_term_bytes(vec![53]);
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        let element = with_interaction(make_element(53, ElementKind::TextInput, attrs));

        let rebuild_ab = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element.clone()]),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state("ab", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
            focus_on_mount: None,
        };

        let rebuild_abc = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state("abc", 3, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild_ab.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild_ab, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::TextCommit {
                text: "c".to_string(),
                mods: 0,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == input_id.clone() && content == "abc"
        )));

        runtime.handle_registry_update(rebuild_abc, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == input_id.clone() && content != "abc"
        )));

        runtime.handle_input_event(
            InputEvent::TextCommit {
                text: "d".to_string(),
                mods: 0,
            },
            &tree_tx,
            false,
        );

        let session = runtime
            .text_states
            .get(&input_id)
            .expect("session updated after rebuild");
        assert_eq!(session.content, "abcd");
        assert_eq!(session.cursor, 4);
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == input_id.clone() && content == "abcd"
        )));
    }

    #[test]
    fn focused_tree_patch_matching_pending_value_preserves_runtime_content() {
        let input_id = ElementId::from_term_bytes(vec![55]);
        let rebuild_ab = RegistryRebuildPayload {
            base_registry: registry_builder::Registry::default(),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state_with_origin(
                    "ab",
                    TextInputContentOrigin::TreePatch,
                    2,
                    None,
                    true,
                ),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.focused_id = Some(input_id.clone());
        runtime.text_states.insert(
            input_id.clone(),
            make_text_input_state("abc", 3, None, true),
        );
        runtime.pending_text_patches.insert(
            input_id.clone(),
            VecDeque::from([
                PendingTextPatch {
                    content: "ab".to_string(),
                    expires_at: Instant::now() + PENDING_TEXT_PATCH_TTL,
                },
                PendingTextPatch {
                    content: "abc".to_string(),
                    expires_at: Instant::now() + PENDING_TEXT_PATCH_TTL,
                },
            ]),
        );

        runtime.handle_registry_update(rebuild_ab, &tree_tx, false);

        let session = runtime
            .text_states
            .get(&input_id)
            .expect("session preserved");
        assert_eq!(session.content, "abc");
        assert_eq!(session.cursor, 3);
        assert_eq!(
            runtime
                .pending_text_patches
                .get(&input_id)
                .expect("latest pending retained")
                .iter()
                .map(|pending| pending.content.as_str())
                .collect::<Vec<_>>(),
            vec!["abc"]
        );
        assert!(drain_msgs(&tree_rx).iter().all(|msg| !matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == input_id && content == "abc"
        )));
    }

    #[test]
    fn focused_tree_patch_non_pending_value_is_accepted() {
        let input_id = ElementId::from_term_bytes(vec![56]);
        let rebuild_remote = RegistryRebuildPayload {
            base_registry: registry_builder::Registry::default(),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state_with_origin(
                    "server",
                    TextInputContentOrigin::TreePatch,
                    6,
                    None,
                    true,
                ),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.focused_id = Some(input_id.clone());
        runtime.text_states.insert(
            input_id.clone(),
            make_text_input_state("abc", 3, None, true),
        );
        runtime.pending_text_patches.insert(
            input_id.clone(),
            VecDeque::from([PendingTextPatch {
                content: "abc".to_string(),
                expires_at: Instant::now() + PENDING_TEXT_PATCH_TTL,
            }]),
        );

        runtime.handle_registry_update(rebuild_remote, &tree_tx, false);

        let session = runtime
            .text_states
            .get(&input_id)
            .expect("session accepted");
        assert_eq!(session.content, "server");
        assert_eq!(session.content_origin, TextInputContentOrigin::TreePatch);
        assert!(!runtime.pending_text_patches.contains_key(&input_id));
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn focused_tree_patch_accepts_value_after_pending_expiration() {
        let input_id = ElementId::from_term_bytes(vec![57]);
        let rebuild_remote = RegistryRebuildPayload {
            base_registry: registry_builder::Registry::default(),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state_with_origin(
                    "server",
                    TextInputContentOrigin::TreePatch,
                    6,
                    None,
                    true,
                ),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.focused_id = Some(input_id.clone());
        runtime.text_states.insert(
            input_id.clone(),
            make_text_input_state("abc", 3, None, true),
        );
        runtime.pending_text_patches.insert(
            input_id.clone(),
            VecDeque::from([PendingTextPatch {
                content: "abc".to_string(),
                expires_at: Instant::now() - Duration::from_millis(1),
            }]),
        );

        runtime.handle_registry_update(rebuild_remote, &tree_tx, false);

        assert_eq!(
            runtime
                .text_states
                .get(&input_id)
                .expect("expired pending should not block")
                .content,
            "server"
        );
        assert!(!runtime.pending_text_patches.contains_key(&input_id));
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn unfocused_rebuild_clears_pending_text_patches() {
        let input_id = ElementId::from_term_bytes(vec![58]);
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::Registry::default(),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state_with_origin(
                    "abc",
                    TextInputContentOrigin::TreePatch,
                    3,
                    None,
                    false,
                ),
            )]),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, _tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.pending_text_patches.insert(
            input_id.clone(),
            VecDeque::from([PendingTextPatch {
                content: "abc".to_string(),
                expires_at: Instant::now() + PENDING_TEXT_PATCH_TTL,
            }]),
        );

        runtime.handle_registry_update(rebuild, &tree_tx, false);

        assert!(!runtime.pending_text_patches.contains_key(&input_id));
    }

    #[test]
    fn direct_runtime_delete_surrounding_updates_content() {
        let mut attrs = Attrs::default();
        attrs.content = Some("abcd".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        let element = with_interaction(make_element(54, ElementKind::TextInput, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![54]),
                make_text_input_state("abcd", 2, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(ElementId::from_term_bytes(vec![54])),
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::DeleteSurrounding {
                before_length: 1,
                after_length: 1,
            },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == ElementId::from_term_bytes(vec![54]) && content == "ad"
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputRuntime { element_id, cursor, .. }
                if *element_id == ElementId::from_term_bytes(vec![54]) && *cursor == Some(1)
        )));
    }

    #[test]
    fn direct_runtime_hover_leave_clears_hover_state_after_rebuild() {
        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);
        let element = with_interaction(make_element(52, ElementKind::El, attrs.clone()));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);
        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == ElementId::from_term_bytes(vec![52]) && *active
        )));

        attrs.mouse_over_active = Some(true);
        let active_element = with_interaction(make_element(52, ElementKind::El, attrs));
        let active_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[active_element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };
        runtime.handle_registry_update(active_rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::CursorEntered { entered: false },
            &tree_tx,
            false,
        );

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == ElementId::from_term_bytes(vec![52]) && !*active
        )));
    }

    #[test]
    fn direct_runtime_parent_hover_activates_through_mouse_move_child() {
        let parent_id = ElementId::from_term_bytes(vec![62]);
        let child_id = ElementId::from_term_bytes(vec![63]);

        let mut parent_attrs = Attrs::default();
        parent_attrs.mouse_over = Some(MouseOverAttrs::default());
        parent_attrs.mouse_over_active = Some(false);
        let mut parent = with_interaction(make_element(62, ElementKind::El, parent_attrs));
        parent.children = vec![child_id.clone()];

        let mut child_attrs = Attrs::default();
        child_attrs.on_mouse_move = Some(true);
        let child = with_interaction(make_element(63, ElementKind::El, child_attrs));

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[parent, child]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        assert!(runtime.listener_lane.is_stale());
        assert_eq!(runtime.hovered_id, Some(parent_id.clone()));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == parent_id && *active
        )));
        assert!(msgs.iter().all(|msg| {
            !matches!(
                msg,
                TreeMsg::SetMouseOverActive { element_id, .. } if *element_id == child_id
            )
        }));
    }

    #[test]
    fn direct_runtime_child_hover_beats_parent_hover() {
        let parent_id = ElementId::from_term_bytes(vec![64]);
        let child_id = ElementId::from_term_bytes(vec![65]);

        let mut parent_attrs = Attrs::default();
        parent_attrs.mouse_over = Some(MouseOverAttrs::default());
        parent_attrs.mouse_over_active = Some(false);
        let mut parent = with_interaction(make_element(64, ElementKind::El, parent_attrs));
        parent.children = vec![child_id.clone()];

        let mut child_attrs = Attrs::default();
        child_attrs.mouse_over = Some(MouseOverAttrs::default());
        child_attrs.mouse_over_active = Some(false);
        let child = with_interaction(make_element(65, ElementKind::El, child_attrs));

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[parent, child]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        assert!(runtime.listener_lane.is_stale());
        assert_eq!(runtime.hovered_id, Some(child_id.clone()));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == child_id && *active
        )));
        assert!(msgs.iter().all(|msg| {
            !matches!(
                msg,
                TreeMsg::SetMouseOverActive { element_id, .. } if *element_id == parent_id
            )
        }));
    }

    #[test]
    fn direct_runtime_hover_handoff_switches_from_parent_to_child() {
        let parent_id = ElementId::from_term_bytes(vec![66]);
        let child_id = ElementId::from_term_bytes(vec![67]);

        let mut parent_attrs = Attrs::default();
        parent_attrs.mouse_over = Some(MouseOverAttrs::default());
        parent_attrs.mouse_over_active = Some(false);
        let mut parent = with_interaction(make_element(66, ElementKind::El, parent_attrs.clone()));
        parent.children = vec![child_id.clone()];

        let mut child_attrs = Attrs::default();
        child_attrs.mouse_over = Some(MouseOverAttrs::default());
        child_attrs.mouse_over_active = Some(false);
        let child = with_interaction_rect(
            make_element(67, ElementKind::El, child_attrs.clone()),
            40.0,
            0.0,
            40.0,
            40.0,
        );

        let initial_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[
                parent.clone(),
                child.clone(),
            ]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(initial_rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);

        assert_eq!(runtime.hovered_id, Some(parent_id.clone()));
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == parent_id && *active
        )));

        let mut active_parent_attrs = parent_attrs;
        active_parent_attrs.mouse_over_active = Some(true);
        let mut active_parent =
            with_interaction(make_element(66, ElementKind::El, active_parent_attrs));
        active_parent.children = vec![child_id.clone()];
        let active_child = with_interaction_rect(
            make_element(67, ElementKind::El, child_attrs),
            40.0,
            0.0,
            40.0,
            40.0,
        );
        let active_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[active_parent, active_child]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };
        runtime.handle_registry_update(active_rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 50.0, y: 10.0 }, &tree_tx, false);

        let hover_msgs: Vec<_> = drain_msgs(&tree_rx)
            .into_iter()
            .filter_map(|msg| match msg {
                TreeMsg::SetMouseOverActive { element_id, active } => Some((element_id, active)),
                _ => None,
            })
            .collect();
        assert!(runtime.listener_lane.is_stale());
        assert_eq!(runtime.hovered_id, Some(child_id.clone()));
        assert_eq!(hover_msgs, vec![(parent_id, false), (child_id, true)]);
    }

    #[test]
    fn direct_runtime_registry_rebuild_replays_static_cursor_into_new_hover_target() {
        let element_id = ElementId::from_term_bytes(vec![57]);

        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);

        let initial = with_interaction_rect(
            make_element(57, ElementKind::El, attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[initial]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let moved = with_interaction_rect(
            make_element(57, ElementKind::El, attrs),
            60.0,
            0.0,
            40.0,
            40.0,
        );
        let moved_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[moved]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 70.0, y: 10.0 }, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_registry_update(moved_rebuild, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id: id, active }
                if *id == element_id && *active
        )));
    }

    #[test]
    fn direct_runtime_updates_cursor_icon_for_text_pressable_mouse_down_and_fallback() {
        let text_input = with_interaction_rect(
            make_element(170, ElementKind::TextInput, Attrs::default()),
            0.0,
            0.0,
            40.0,
            40.0,
        );

        let mut pressable_attrs = Attrs::default();
        pressable_attrs.on_press = Some(true);
        let pressable = with_interaction_rect(
            make_element(171, ElementKind::El, pressable_attrs),
            60.0,
            0.0,
            40.0,
            40.0,
        );

        let mut mouse_down_attrs = Attrs::default();
        mouse_down_attrs.on_mouse_down = Some(true);
        let mouse_down_only = with_interaction_rect(
            make_element(172, ElementKind::El, mouse_down_attrs),
            120.0,
            0.0,
            40.0,
            40.0,
        );

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[
                text_input,
                pressable,
                mouse_down_only,
            ]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);
        assert_eq!(runtime.current_cursor_icon, CursorIcon::Text);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 70.0, y: 10.0 }, &tree_tx, false);
        assert_eq!(runtime.current_cursor_icon, CursorIcon::Pointer);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 130.0, y: 10.0 }, &tree_tx, false);
        assert_eq!(runtime.current_cursor_icon, CursorIcon::Pointer);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 190.0, y: 10.0 }, &tree_tx, false);
        assert_eq!(runtime.current_cursor_icon, CursorIcon::Default);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn direct_runtime_sends_cursor_icons_to_backend_transport_once_per_change() {
        let text_input = with_interaction_rect(
            make_element(173, ElementKind::TextInput, Attrs::default()),
            0.0,
            0.0,
            40.0,
            40.0,
        );

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[text_input]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let (cursor_tx, cursor_rx) = unbounded();
        let mut runtime = DirectEventRuntime::new_with_backend_cursor(
            false,
            Some(cursor_tx),
            BackendWakeHandle::noop(),
        );
        runtime.handle_registry_update(rebuild, &tree_tx, false);

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);
        assert_eq!(cursor_rx.try_recv().ok(), Some(CursorIcon::Text));
        assert!(cursor_rx.try_recv().is_err());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);
        assert!(cursor_rx.try_recv().is_err());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_input_event(
            InputEvent::CursorEntered { entered: false },
            &tree_tx,
            false,
        );
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);
        assert_eq!(cursor_rx.try_recv().ok(), Some(CursorIcon::Text));
        assert!(cursor_rx.try_recv().is_err());
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn direct_runtime_cursor_revalidate_updates_icon_without_synthetic_mouse_move() {
        let initial = with_interaction_rect(
            make_element(172, ElementKind::El, Attrs::default()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let initial_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[initial]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let mut moved_attrs = Attrs::default();
        moved_attrs.on_mouse_move = Some(true);
        moved_attrs.on_press = Some(true);
        let moved = with_interaction_rect(
            make_element(172, ElementKind::El, moved_attrs),
            60.0,
            0.0,
            40.0,
            40.0,
        );
        let moved_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[moved]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(initial_rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 70.0, y: 10.0 }, &tree_tx, false);

        assert_eq!(runtime.current_cursor_icon, CursorIcon::Default);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_registry_update(moved_rebuild, &tree_tx, false);

        assert_eq!(runtime.current_cursor_icon, CursorIcon::Pointer);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn direct_runtime_registry_rebuild_replays_static_cursor_out_of_old_hover_target() {
        let element_id = ElementId::from_term_bytes(vec![58]);

        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);
        let element = with_interaction_rect(
            make_element(58, ElementKind::El, attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id: id, active }
                if *id == element_id && *active
        )));

        let mut active_attrs = attrs.clone();
        active_attrs.mouse_over_active = Some(true);
        let active_element = with_interaction_rect(
            make_element(58, ElementKind::El, active_attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let active_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[active_element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };
        runtime.handle_registry_update(active_rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        let moved_away = with_interaction_rect(
            make_element(58, ElementKind::El, active_attrs),
            60.0,
            0.0,
            40.0,
            40.0,
        );
        let moved_away_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[moved_away]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        runtime.handle_registry_update(moved_away_rebuild, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id: id, active }
                if *id == element_id && !*active
        )));
    }

    #[test]
    fn direct_runtime_animated_in_front_overlay_activates_hover_right_of_initial_position() {
        let overlay_id = ElementId::from_term_bytes(vec![131]);
        let initial_rebuild = animated_width_move_rebuild_at(0, false);
        let mid_rebuild = animated_width_move_rebuild_at(500, false);
        let late_rebuild = animated_width_move_rebuild_at(1000, true);

        let (tree_tx, tree_rx) = bounded(128);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(initial_rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 130.0, y: 41.0 }, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_registry_update(mid_rebuild, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == overlay_id && *active
        )));

        runtime.listener_lane.stale = false;
        runtime.handle_registry_update(late_rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
    }

    #[test]
    fn sampled_hit_case_static_cursor_activates_when_target_enters_newly_occupied_probes() {
        let case = AnimatedNearbyHitCase::width_move_in_front();

        for label in ["newly_occupied_inside_host", "newly_occupied_outside_host"] {
            let point = probe_point(&case, label);
            let (tree_tx, tree_rx) = bounded(128);
            let mut runtime = DirectEventRuntime::new(false);

            runtime.handle_registry_update(case.rebuild_at(0, false), &tree_tx, false);
            runtime.handle_input_event(
                InputEvent::CursorPos {
                    x: point.0,
                    y: point.1,
                },
                &tree_tx,
                false,
            );

            match label {
                "newly_occupied_inside_host" => {
                    assert!(
                        !runtime.listener_lane.is_stale(),
                        "probe {label} should stay fresh for mousemove-only target"
                    );
                    assert!(
                        drain_msgs(&tree_rx)
                            .iter()
                            .all(|msg| !matches!(msg, TreeMsg::RebuildRegistry)),
                        "probe {label} should not request rebuild at 0ms"
                    );
                }
                "newly_occupied_outside_host" => {
                    assert!(
                        !runtime.listener_lane.is_stale(),
                        "probe {label} should start outside"
                    );
                    assert!(
                        drain_msgs(&tree_rx).is_empty(),
                        "probe {label} should not trigger at 0ms"
                    );
                }
                _ => unreachable!("unexpected probe label"),
            }

            runtime.handle_registry_update(case.rebuild_at(500, false), &tree_tx, false);

            let msgs = drain_msgs(&tree_rx);
            let activations = msgs
                .iter()
                .filter(|msg| {
                    matches!(
                        msg,
                        TreeMsg::SetMouseOverActive { element_id, active }
                            if *element_id == case.target_id && *active
                    )
                })
                .count();

            assert_eq!(
                activations, 1,
                "probe {label} should activate hover exactly once"
            );
            assert!(
                runtime.listener_lane.is_stale(),
                "probe {label} should mark lane stale after activation"
            );
        }
    }

    #[test]
    fn sampled_hit_case_static_cursor_does_not_duplicate_hover_when_target_stays_inside() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let point = probe_point(&case, "newly_occupied_outside_host");
        let (tree_tx, tree_rx) = bounded(128);
        let mut runtime = DirectEventRuntime::new(false);

        runtime.handle_registry_update(case.rebuild_at(0, false), &tree_tx, false);
        runtime.handle_input_event(
            InputEvent::CursorPos {
                x: point.0,
                y: point.1,
            },
            &tree_tx,
            false,
        );
        let _ = drain_msgs(&tree_rx);

        runtime.handle_registry_update(case.rebuild_at(500, false), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.listener_lane.stale = false;

        runtime.handle_registry_update(case.rebuild_at(1000, true), &tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        let activations = msgs
            .iter()
            .filter(|msg| {
                matches!(
                    msg,
                    TreeMsg::SetMouseOverActive { element_id, active }
                        if *element_id == case.target_id && *active
                )
            })
            .count();

        assert_eq!(activations, 0);
        assert!(!runtime.listener_lane.is_stale());
    }

    #[test]
    fn sampled_hit_case_static_cursor_clears_when_target_leaves_probe() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        let point = probe_point(&case, "newly_occupied_outside_host");
        let (tree_tx, tree_rx) = bounded(128);
        let mut runtime = DirectEventRuntime::new(false);

        runtime.handle_registry_update(case.rebuild_at(1000, true), &tree_tx, false);
        runtime.handle_input_event(
            InputEvent::CursorPos {
                x: point.0,
                y: point.1,
            },
            &tree_tx,
            false,
        );
        let _ = drain_msgs(&tree_rx);
        runtime.listener_lane.stale = false;

        runtime.handle_registry_update(case.rebuild_at(0, true), &tree_tx, false);

        let msgs = drain_msgs(&tree_rx);
        let clears = msgs
            .iter()
            .filter(|msg| {
                matches!(
                    msg,
                    TreeMsg::SetMouseOverActive { element_id, active }
                        if *element_id == case.target_id && !*active
                )
            })
            .count();

        assert_eq!(clears, 1);
        assert!(runtime.listener_lane.is_stale());
    }

    #[test]
    fn direct_runtime_registry_rebuild_skips_cursor_replay_when_pointer_left_window() {
        let element_id = ElementId::from_term_bytes(vec![59]);

        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);
        let element = with_interaction_rect(
            make_element(59, ElementKind::El, attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);
        assert!(runtime.listener_lane.is_stale());
        let _ = drain_msgs(&tree_rx);

        let mut active_attrs = attrs;
        active_attrs.mouse_over_active = Some(true);
        let active_element = with_interaction_rect(
            make_element(59, ElementKind::El, active_attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let active_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[active_element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };
        runtime.handle_registry_update(active_rebuild, &tree_tx, false);

        runtime.handle_input_event(
            InputEvent::CursorEntered { entered: false },
            &tree_tx,
            false,
        );
        assert!(runtime.listener_lane.is_stale());
        let _ = drain_msgs(&tree_rx);

        let inactive_element = with_interaction_rect(
            make_element(59, ElementKind::El, {
                let mut attrs = active_attrs;
                attrs.mouse_over_active = Some(false);
                attrs
            }),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let inactive_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[inactive_element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };
        runtime.handle_registry_update(inactive_rebuild, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().all(|msg| {
            !matches!(
                msg,
                TreeMsg::SetMouseOverActive { element_id: id, active }
                    if *id == element_id && *active
            )
        }));
    }

    #[test]
    fn direct_runtime_registry_rebuild_suppresses_synthetic_mouse_move() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_move = Some(true);

        let initial = with_interaction_rect(
            make_element(60, ElementKind::El, attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[initial]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let moved = with_interaction_rect(
            make_element(60, ElementKind::El, attrs),
            60.0,
            0.0,
            40.0,
            40.0,
        );
        let moved_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[moved]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 70.0, y: 10.0 }, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_registry_update(moved_rebuild, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn direct_runtime_registry_rebuild_keeps_hovered_cursor_fresh_when_move_is_synthetic() {
        let element_id = ElementId::from_term_bytes(vec![61]);

        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.on_mouse_move = Some(true);
        attrs.mouse_over_active = Some(false);

        let initial = with_interaction_rect(
            make_element(61, ElementKind::El, attrs.clone()),
            0.0,
            0.0,
            40.0,
            40.0,
        );
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[initial]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let moved = with_interaction_rect(
            make_element(61, ElementKind::El, attrs.clone()),
            60.0,
            0.0,
            40.0,
            40.0,
        );
        let moved_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[moved]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        runtime.handle_input_event(InputEvent::CursorPos { x: 70.0, y: 10.0 }, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());

        runtime.handle_registry_update(moved_rebuild.clone(), &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id: id, active }
                if *id == element_id && *active
        )));

        let mut hovered_attrs = attrs;
        hovered_attrs.mouse_over_active = Some(true);
        let hovered = with_interaction_rect(
            make_element(61, ElementKind::El, hovered_attrs),
            60.0,
            0.0,
            40.0,
            40.0,
        );
        let hovered_rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[hovered]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        runtime.listener_lane.stale = false;
        runtime.handle_registry_update(hovered_rebuild, &tree_tx, false);

        assert!(!runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).is_empty());
    }

    #[test]
    fn direct_runtime_unhovered_scrollable_elsewhere_does_not_mask_menu_hover_leave() {
        let mut menu_attrs = Attrs::default();
        menu_attrs.mouse_over = Some(MouseOverAttrs::default());
        menu_attrs.mouse_over_active = Some(true);
        let menu = with_interaction_rect(
            make_element(54, ElementKind::El, menu_attrs),
            0.0,
            0.0,
            100.0,
            40.0,
        );

        let plain = with_interaction_rect(
            make_element(55, ElementKind::El, Attrs::default()),
            0.0,
            45.0,
            100.0,
            10.0,
        );

        let mut scrollable_attrs = Attrs::default();
        scrollable_attrs.scrollbar_y = Some(true);
        scrollable_attrs.scroll_y = Some(10.0);
        let scrollable = with_frame(
            with_interaction_rect(
                make_element(56, ElementKind::El, scrollable_attrs),
                0.0,
                60.0,
                100.0,
                40.0,
            ),
            Frame {
                x: 0.0,
                y: 60.0,
                width: 100.0,
                height: 40.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );

        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[menu, plain, scrollable]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
            focus_on_mount: None,
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 50.0 }, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        assert!(drain_msgs(&tree_rx).iter().any(|msg| matches!(
            msg,
            TreeMsg::SetMouseOverActive { element_id, active }
                if *element_id == ElementId::from_term_bytes(vec![54]) && !*active
        )));
    }
}
