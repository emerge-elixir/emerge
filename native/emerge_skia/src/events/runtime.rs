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
use std::{collections::HashMap, thread};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use rustler::LocalPid;

use crate::{
    actors::{EventMsg, TreeMsg},
    clipboard::{ClipboardManager, ClipboardTarget},
    input::{InputEvent, InputHandler},
    tree::{element::ElementId, scrollbar::ScrollbarAxis},
};

use super::{
    ElementEventKind, RegistryRebuildPayload, TextInputState, blur_atom, change_atom, click_atom,
    focus_atom, mouse_down_atom, mouse_enter_atom, mouse_leave_atom, mouse_move_atom,
    mouse_up_atom, press_atom,
    registry_builder::{
        self, ListenerAction, ListenerComputeCtx, ListenerInput, ListenerMatcherKind,
        RuntimeChange, RuntimeOverlayState,
    },
    scrollbar::ScrollbarNode,
    send_element_event, send_element_event_with_string_payload, send_input_event,
};

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
/// If a listener emits Elixir events but no tree messages, flushing injects
/// `TreeMsg::RebuildRegistry` so the tree actor will send fresh listener data.
#[derive(Default)]
struct PendingDispatchEffects {
    tree_msgs: Vec<TreeMsg>,
    runtime_changes: Vec<RuntimeChange>,
    elixir_event_requires_rebuild: bool,
}

impl PendingDispatchEffects {
    fn collect(mut self, runtime: &mut DirectEventRuntime, action: ListenerAction) -> Self {
        match action {
            ListenerAction::TreeMsg(msg) => self.tree_msgs.push(msg),
            ListenerAction::RuntimeChange(change) => self.runtime_changes.push(change),
            ListenerAction::ElixirEvent(event) => {
                self.elixir_event_requires_rebuild = true;
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
    ) {
        runtime.apply_runtime_changes_and_recompose_if_needed(self.runtime_changes);

        if self.elixir_event_requires_rebuild && self.tree_msgs.is_empty() {
            self.tree_msgs.push(TreeMsg::RebuildRegistry);
        }

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
    text_states: &'a HashMap<ElementId, TextInputState>,
    clipboard: &'a mut ClipboardManager,
}

impl ListenerComputeCtx for RuntimeListenerComputeCtx<'_> {
    fn focused_id(&self) -> Option<&ElementId> {
        self.focused_id
    }

    fn text_input_state(&self, element_id: &ElementId) -> Option<TextInputState> {
        self.text_states.get(element_id).cloned()
    }

    fn clipboard_text(&mut self, target: ClipboardTarget) -> Option<String> {
        self.clipboard.get_text(target)
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
    focused_id: Option<ElementId>,
    text_states: HashMap<ElementId, TextInputState>,
    scrollbar_nodes: HashMap<(ElementId, ScrollbarAxis), ScrollbarNode>,
    input_handler: InputHandler,
    input_target: Option<LocalPid>,
    clipboard: ClipboardManager,
}

impl DirectEventRuntime {
    fn new(system_clipboard: bool) -> Self {
        let base_registry = registry_builder::Registry::default();
        let runtime_overlay = RuntimeOverlayState::default();
        let overlay_registry =
            registry_builder::build_runtime_overlay_registry(&base_registry, &runtime_overlay);

        Self {
            base_registry,
            runtime_overlay,
            overlay_registry,
            listener_lane: ListenerLaneState::initially_stale(),
            focused_id: None,
            text_states: HashMap::new(),
            scrollbar_nodes: HashMap::new(),
            input_handler: InputHandler::new(),
            input_target: None,
            clipboard: ClipboardManager::new(system_clipboard),
        }
    }

    fn set_input_mask(&mut self, mask: u32) {
        self.input_handler.set_mask(mask);
    }

    fn set_input_target(&mut self, target: Option<LocalPid>) {
        self.input_target = target;
    }

    fn handle_input_event(
        &mut self,
        event: InputEvent,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        let event = event.normalize_scroll();
        forward_observer_input(&event, &self.input_handler, &self.input_target);

        if self.listener_lane.is_stale() {
            self.listener_lane.buffer_input(event);
            return;
        }

        self.dispatch_event(event, tree_tx, log_render);
    }

    fn handle_registry_update(
        &mut self,
        rebuild: RegistryRebuildPayload,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        self.listener_lane.stale = false;
        self.install_rebuild(rebuild, tree_tx, log_render);
        if self.listener_lane.is_stale() {
            return;
        }

        let buffered = self.listener_lane.mark_fresh_and_take_buffered();
        self.replay_buffered(buffered, tree_tx, log_render);
    }

    fn install_rebuild(
        &mut self,
        rebuild: RegistryRebuildPayload,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        self.base_registry = rebuild.base_registry;
        self.scrollbar_nodes = rebuild.scrollbars;

        self.reconcile_runtime_overlay(&rebuild.text_inputs);
        self.recompose_overlay_registry();
        self.focused_id = rebuild.focused_id;

        if reconcile_text_input_states(
            &rebuild.text_inputs,
            &mut self.text_states,
            &self.focused_id,
            tree_tx,
            log_render,
        ) {
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
            self.dispatch_event(event, tree_tx, log_render);
        }
    }

    fn recompose_overlay_registry(&mut self) {
        self.overlay_registry = registry_builder::build_runtime_overlay_registry(
            &self.base_registry,
            &self.runtime_overlay,
        );
    }

    fn dispatch_event(&mut self, event: InputEvent, tree_tx: &Sender<TreeMsg>, log_render: bool) {
        let input = ListenerInput::Raw(event);
        let actions = {
            let mut ctx = RuntimeListenerComputeCtx {
                base_registry: &self.base_registry,
                overlay_registry: &self.overlay_registry,
                focused_id: self.focused_id.as_ref(),
                text_states: &self.text_states,
                clipboard: &mut self.clipboard,
            };
            registry_builder::LayeredRegistryView::new(&self.overlay_registry, &self.base_registry)
                .first_match(&input, &[], &mut ctx)
        };

        if !actions.is_empty() {
            self.apply_listener_actions(actions, tree_tx, log_render);
        }
    }

    fn apply_listener_actions(
        &mut self,
        actions: Vec<ListenerAction>,
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) {
        // Apply the ordered action list produced by one matched listener.
        // Tree messages and runtime changes are collected first so they can be
        // flushed in a controlled order after action collection completes.
        actions
            .into_iter()
            .fold(PendingDispatchEffects::default(), |effects, action| {
                effects.collect(self, action)
            })
            .flush(self, tree_tx, log_render);
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
            RuntimeChange::StartDragTracker {
                element_id,
                matcher_kind,
                origin_x,
                origin_y,
            } => {
                self.runtime_overlay.drag = registry_builder::DragTrackerState::Candidate {
                    element_id,
                    matcher_kind,
                    origin_x,
                    origin_y,
                };
            }
            RuntimeChange::PromoteDragTracker {
                element_id,
                matcher_kind,
                last_x,
                last_y,
            } => {
                self.runtime_overlay.drag = registry_builder::DragTrackerState::Active {
                    element_id,
                    matcher_kind,
                    last_x,
                    last_y,
                };
            }
            RuntimeChange::ClearDragTracker => {
                self.runtime_overlay.drag = registry_builder::DragTrackerState::Inactive;
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
            }
            RuntimeChange::ClearClickPressTracker => {
                self.runtime_overlay.click_press = None;
            }
            RuntimeChange::StartScrollbarDrag { tracker } => {
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
        }
    }

    fn apply_text_input_state(&mut self, element_id: &ElementId, state: TextInputState) {
        self.text_states.insert(element_id.clone(), state);
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
                }
            }
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
            } else {
                self.runtime_overlay.scrollbar = None;
            }
        }
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

fn event_kind_to_atom(kind: ElementEventKind) -> rustler::Atom {
    match kind {
        ElementEventKind::Click => click_atom(),
        ElementEventKind::Press => press_atom(),
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

fn reconcile_text_input_states(
    text_inputs: &HashMap<ElementId, TextInputState>,
    states: &mut HashMap<ElementId, TextInputState>,
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
        tree_tx: &Sender<TreeMsg>,
        log_render: bool,
    ) -> bool {
        let mut changed_tree = false;

        state.copy_rebuild_metadata_from(rebuild_state);
        if state.content != rebuild_state.content {
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
            changed_tree |=
                reconcile_focused_text_input(&id, rebuild_state, state, tree_tx, log_render);
        } else {
            reset_unfocused_text_input_from_rebuild(state, rebuild_state);
        }
    }

    states.retain(|id, _| text_inputs.contains_key(id));
    changed_tree
}

#[cfg(test)]
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
            other => coalesced.push(other),
        }
    }

    if let Some((dx, dy, x, y)) = scroll_acc {
        coalesced.push(InputEvent::CursorScroll { dx, dy, x, y });
    }
    if let Some(cursor) = last_cursor {
        coalesced.push(cursor);
    }

    coalesced
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
    log_render: bool,
    system_clipboard: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut runtime = DirectEventRuntime::new(system_clipboard);

        while let Ok(message) = event_rx.recv() {
            match message {
                EventMsg::InputEvent(event) => {
                    runtime.handle_input_event(event, &tree_tx, log_render)
                }
                EventMsg::RegistryUpdate { rebuild } => {
                    runtime.handle_registry_update(rebuild, &tree_tx, log_render)
                }
                EventMsg::SetInputMask(mask) => runtime.set_input_mask(mask),
                EventMsg::SetInputTarget(target) => runtime.set_input_target(target),
                EventMsg::Stop => return,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::events::registry_builder::{self, FocusRevealScroll};
    use crate::events::{RegistryRebuildPayload, build_registry_rebuild};
    use crate::tree::attrs::TextAlign;
    use crate::tree::attrs::{Attrs, MouseOverAttrs};
    use crate::tree::element::ElementId;
    use crate::tree::element::{Element, ElementKind, ElementTree, Frame};
    use crate::tree::interaction::{ElementInteraction, Rect};
    use crossbeam_channel::bounded;

    fn make_text_input_state(
        content: &str,
        cursor: u32,
        selection_anchor: Option<u32>,
        focused: bool,
    ) -> TextInputState {
        TextInputState {
            content: content.to_string(),
            content_len: content.chars().count() as u32,
            cursor,
            selection_anchor,
            preedit: None,
            preedit_cursor: None,
            focused,
            emit_change: false,
            frame_x: 0.0,
            frame_width: 100.0,
            inset_left: 0.0,
            inset_right: 0.0,
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

    fn drain_raw_msgs(rx: &Receiver<TreeMsg>) -> Vec<TreeMsg> {
        let mut out = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            out.push(msg);
        }
        out
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
        element.interaction = Some(ElementInteraction {
            visible: true,
            hit_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
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
    fn direct_runtime_dispatches_mouse_down_style_activation() {
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs::default());
        let element = with_interaction(make_element(20, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
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
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::Key {
                key: "tab".to_string(),
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
        let rebuild = build_registry_rebuild(&mut tree);

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
        assert!(matches!(runtime.runtime_overlay.scrollbar, Some(_)));
        assert!(msgs.is_empty());

        let rebuild = build_registry_rebuild(&mut tree);
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
    fn direct_runtime_elixir_only_listener_marks_stale_and_requests_rebuild() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_move = Some(true);
        let element = with_interaction(make_element(42, ElementKind::El, attrs));
        let rebuild = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
        };

        let (tree_tx, tree_rx) = bounded(32);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(InputEvent::CursorPos { x: 10.0, y: 10.0 }, &tree_tx, false);

        assert!(runtime.listener_lane.is_stale());
        let msgs = drain_msgs(&tree_rx);
        assert!(
            msgs.iter()
                .any(|msg| matches!(msg, TreeMsg::RebuildRegistry))
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
        };

        let (tree_tx, tree_rx) = bounded(64);
        let mut runtime = DirectEventRuntime::new(false);
        runtime.handle_registry_update(rebuild.clone(), &tree_tx, false);
        let _ = drain_msgs(&tree_rx);
        runtime.handle_registry_update(rebuild, &tree_tx, false);
        assert!(!runtime.listener_lane.is_stale());

        runtime.handle_input_event(
            InputEvent::Key {
                key: "backspace".to_string(),
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
        };

        let rebuild_abc = RegistryRebuildPayload {
            base_registry: registry_builder::registry_for_elements(&[element]),
            text_inputs: HashMap::from([(
                input_id.clone(),
                make_text_input_state("abc", 3, None, true),
            )]),
            scrollbars: HashMap::new(),
            focused_id: Some(input_id.clone()),
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
