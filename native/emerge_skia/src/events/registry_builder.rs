//! # Direct Listener Registry Builder
//!
//! This module defines the initial direct-listener registry model and the first
//! node-level builder entrypoint.
//!
//! ## Where it is used
//!
//! - The tree actor builds a base registry during the same walk that produces
//!   draw commands.
//! - The tree actor sends that registry to the event actor through
//!   `EventMsg::RegistryUpdate`.
//! - The event actor resolves input by walking deterministic listener buckets.
//!
//! ## Why this exists
//!
//! It keeps event handling direct and declarative:
//!
//! `input -> listener match -> actions`
//!
//! This avoids trigger/job translation layers and keeps runtime handlers focused
//! on orchestration.
//!
//! ## Current scope
//!
//! This first iteration focuses on **element -> listeners** for:
//! - pointer/hover/mouse-down style behavior
//! - pointer tracker bootstrap for click/press followups
//! - local wheel-scroll listeners
//! - focused text-input change listeners (`on_change` gated emission)
//! - focused and pointer text-input command listeners (`cut`/`paste`)
//! - focus transition scaffolding (pointer focus target + focused Tab cycle)
//!
//! Remaining runtime integration still lands in later iterations.
//!
//! ## Slot-based assembly
//!
//! `listeners_for_element` uses a fixed slot table. Each slot corresponds to
//! one matcher/bucket pair and aggregates actions from multiple attribute
//! contributors.
//!
//! This avoids same-matcher collisions under `first matched wins` semantics.
//! For example, `on_mouse_down` and `mouse_down` style both contribute to the
//! same primary left-press slot, yielding one listener with multiple actions.

use crate::actors::TreeMsg;
use crate::input::{
    ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
};
use crate::tree::element::{Element, ElementId, ElementKind};
use crate::tree::interaction::Rect;

use super::{
    TextInputCommandRequest, TextInputEditRequest, dispatch_outcome::ElementEventKind, text_ops,
};

/// Deterministic listener passes.
///
/// Buckets represent execution passes, not event categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BucketId {
    /// Default pass used for most input processing.
    Primary,
    /// Leave transition pass, used when runtime must resolve leave listeners.
    CursorLeave,
    /// Enter transition pass, used when runtime must resolve enter listeners.
    CursorEnter,
}

impl BucketId {
    /// Total number of buckets.
    pub const COUNT: usize = BucketId::CursorEnter as usize + 1;

    /// Converts this bucket id to an index in `Registry::buckets`.
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Full dispatch registry consumed by the event actor.
///
/// Storage shape is intentionally simple:
/// - `Registry { buckets: Vec<Bucket> }`
/// - `Bucket { listeners: Vec<Listener> }`
#[derive(Clone, Debug)]
pub struct Registry {
    /// All deterministic dispatch passes.
    pub buckets: Vec<Bucket>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            buckets: vec![Bucket::default(); BucketId::COUNT],
        }
    }
}

impl Registry {
    /// Returns an immutable reference to a bucket by id.
    #[inline]
    pub fn bucket(&self, id: BucketId) -> &Bucket {
        &self.buckets[id.index()]
    }

    /// Returns a mutable reference to a bucket by id.
    #[inline]
    pub fn bucket_mut(&mut self, id: BucketId) -> &mut Bucket {
        &mut self.buckets[id.index()]
    }

    /// Appends a listener at the end of a bucket.
    ///
    /// Use this for normal base-registry ordering.
    pub fn push_listener(&mut self, bucket: BucketId, listener: Listener) {
        self.bucket_mut(bucket).listeners.push(listener);
    }

    /// Inserts a listener at the front of a bucket.
    ///
    /// Use this for temporary runtime listeners that must take precedence over
    /// the current stack.
    pub fn prepend_listener(&mut self, bucket: BucketId, listener: Listener) {
        self.bucket_mut(bucket).listeners.insert(0, listener);
    }
}

/// Pointer click/press tracker state used to rematerialize release followups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickPressTracker {
    pub element_id: ElementId,
    pub matcher_kind: ListenerMatcherKind,
    pub emit_click: bool,
    pub emit_press_pointer: bool,
}

/// Drag tracker lifecycle state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DragTrackerState {
    #[default]
    Inactive,
    Candidate {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
    },
    Active {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
    },
}

/// Runtime overlay state used to compose an effective registry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeOverlayState {
    pub click_press: Option<ClickPressTracker>,
    pub drag: DragTrackerState,
}

/// Compose effective registry from base listeners and runtime overlay state.
pub fn compose_effective_registry(base: &Registry, runtime: &RuntimeOverlayState) -> Registry {
    let mut effective = base.clone();

    if let Some(listener) = runtime
        .click_press
        .as_ref()
        .and_then(|tracker| click_press_release_followup(base, tracker))
    {
        effective.prepend_listener(BucketId::Primary, listener);
    }

    if let Some(listener) = drag_active_release_followup(base, &runtime.drag) {
        effective.prepend_listener(BucketId::Primary, listener);
    }

    effective
}

fn drag_active_release_followup(base: &Registry, drag: &DragTrackerState) -> Option<Listener> {
    let (element_id, matcher_kind) = match drag {
        DragTrackerState::Active {
            element_id,
            matcher_kind,
        } => (element_id, *matcher_kind),
        DragTrackerState::Inactive | DragTrackerState::Candidate { .. } => return None,
    };

    let source = source_listener(base, element_id, matcher_kind)?;
    let rect = press_rect_from_source(source)?;
    let actions = vec![
        ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
        ListenerAction::RuntimeChange(RuntimeChange::SetDragConsumed(false)),
    ];
    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseInside { rect },
        compute: ListenerCompute::Static { actions },
    })
}

fn click_press_release_followup(base: &Registry, tracker: &ClickPressTracker) -> Option<Listener> {
    let source = source_listener(base, &tracker.element_id, tracker.matcher_kind)?;
    let rect = press_rect_from_source(source)?;

    let actions: Vec<ListenerAction> = [
        tracker.emit_click.then(|| {
            ListenerAction::ElixirEvent(ElixirEvent {
                element_id: tracker.element_id.clone(),
                kind: ElementEventKind::Click,
                payload: None,
            })
        }),
        tracker.emit_press_pointer.then(|| {
            ListenerAction::ElixirEvent(ElixirEvent {
                element_id: tracker.element_id.clone(),
                kind: ElementEventKind::Press,
                payload: None,
            })
        }),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!actions.is_empty()).then(|| Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseInside { rect },
        compute: ListenerCompute::Static { actions },
    })
}

fn source_listener<'a>(
    base: &'a Registry,
    element_id: &ElementId,
    matcher_kind: ListenerMatcherKind,
) -> Option<&'a Listener> {
    base.buckets
        .iter()
        .flat_map(|bucket| bucket.listeners.iter())
        .find(|listener| {
            listener.element_id.as_ref() == Some(element_id)
                && listener.matcher.kind() == matcher_kind
        })
}

fn press_rect_from_source(source: &Listener) -> Option<Rect> {
    match source.matcher {
        ListenerMatcher::CursorButtonLeftPressInside { rect } => Some(rect),
        _ => None,
    }
}

/// Ordered listener stack for one deterministic pass.
#[derive(Clone, Debug, Default)]
pub struct Bucket {
    /// Listeners evaluated in order; first match wins for this bucket invocation.
    pub listeners: Vec<Listener>,
}

/// Declarative listener record.
///
/// A listener is intentionally minimal:
/// - `element_id` carries source identity for runtime followup rebinding
/// - `matcher` decides whether this listener applies to the current input
/// - `compute` produces final sink actions from the matched input
#[derive(Clone, Debug, Default)]
pub struct Listener {
    /// Optional source element id for this listener.
    pub element_id: Option<ElementId>,
    /// Match rule for this listener.
    pub matcher: ListenerMatcher,
    /// Computation that generates output actions for this listener.
    pub compute: ListenerCompute,
}

impl Listener {
    /// Compute output actions for this listener from a matched input event.
    pub fn compute_actions(&self, input: &InputEvent) -> Vec<ListenerAction> {
        self.compute.compute(input)
    }
}

/// Matcher shape for listener evaluation.
///
/// The first iteration includes concrete pointer/hover variants needed by
/// `listeners_for_element`.
#[derive(Clone, Debug, Default)]
pub enum ListenerMatcher {
    /// Match left-button press when pointer is inside `rect`.
    CursorButtonLeftPressInside { rect: Rect },
    /// Match left-button release when pointer is inside `rect`.
    CursorButtonLeftReleaseInside { rect: Rect },
    /// Match cursor position updates inside `rect`.
    CursorPosInside { rect: Rect },
    /// Match scroll wheel updates inside `rect`.
    CursorScrollInside { rect: Rect },
    /// Match Enter key press when Ctrl/Alt/Meta are not held.
    KeyEnterPressNoCtrlAltMeta,
    /// Match Tab key press when Shift/Ctrl/Alt/Meta are not held.
    KeyTabPressNoShiftCtrlAltMeta,
    /// Match Shift+Tab key press when Ctrl/Alt/Meta are not held.
    KeyShiftTabPressNoCtrlAltMeta,
    /// Match X key press when Ctrl or Meta is held.
    KeyXPressCtrlOrMeta,
    /// Match V key press when Ctrl or Meta is held.
    KeyVPressCtrlOrMeta,
    /// Match Backspace key press.
    KeyBackspacePress,
    /// Match Delete key press.
    KeyDeletePress,
    /// Match text commit events when Ctrl/Meta are not held.
    TextCommitNoCtrlMeta,
    /// Match middle-button press when pointer is inside `rect`.
    CursorButtonMiddlePressInside { rect: Rect },
    /// Match window focus gained notifications.
    WindowFocused,
    /// Match window focus lost notifications.
    WindowBlurred,
    /// Match leaving `rect` via cursor movement outside, or window-leave.
    CursorLeaveBoundary { rect: Rect },
    /// Placeholder variant used while more matcher variants are introduced.
    #[default]
    Unspecified,
}

/// Stable matcher identity for source lookup.
///
/// Equality is by enum variant only; payload is intentionally ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListenerMatcherKind {
    CursorButtonLeftPressInside,
    CursorButtonLeftReleaseInside,
    CursorButtonMiddlePressInside,
    CursorPosInside,
    CursorScrollInside,
    KeyEnterPressNoCtrlAltMeta,
    KeyTabPressNoShiftCtrlAltMeta,
    KeyShiftTabPressNoCtrlAltMeta,
    KeyXPressCtrlOrMeta,
    KeyVPressCtrlOrMeta,
    KeyBackspacePress,
    KeyDeletePress,
    TextCommitNoCtrlMeta,
    WindowFocused,
    WindowBlurred,
    CursorLeaveBoundary,
    Unspecified,
}

impl ListenerMatcher {
    /// Returns matcher identity (variant/discriminant only).
    pub fn kind(&self) -> ListenerMatcherKind {
        match self {
            ListenerMatcher::CursorButtonLeftPressInside { .. } => {
                ListenerMatcherKind::CursorButtonLeftPressInside
            }
            ListenerMatcher::CursorButtonLeftReleaseInside { .. } => {
                ListenerMatcherKind::CursorButtonLeftReleaseInside
            }
            ListenerMatcher::CursorButtonMiddlePressInside { .. } => {
                ListenerMatcherKind::CursorButtonMiddlePressInside
            }
            ListenerMatcher::CursorPosInside { .. } => ListenerMatcherKind::CursorPosInside,
            ListenerMatcher::CursorScrollInside { .. } => ListenerMatcherKind::CursorScrollInside,
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyEnterPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta => {
                ListenerMatcherKind::KeyTabPressNoShiftCtrlAltMeta
            }
            ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyShiftTabPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyXPressCtrlOrMeta => ListenerMatcherKind::KeyXPressCtrlOrMeta,
            ListenerMatcher::KeyVPressCtrlOrMeta => ListenerMatcherKind::KeyVPressCtrlOrMeta,
            ListenerMatcher::KeyBackspacePress => ListenerMatcherKind::KeyBackspacePress,
            ListenerMatcher::KeyDeletePress => ListenerMatcherKind::KeyDeletePress,
            ListenerMatcher::TextCommitNoCtrlMeta => ListenerMatcherKind::TextCommitNoCtrlMeta,
            ListenerMatcher::WindowFocused => ListenerMatcherKind::WindowFocused,
            ListenerMatcher::WindowBlurred => ListenerMatcherKind::WindowBlurred,
            ListenerMatcher::CursorLeaveBoundary { .. } => ListenerMatcherKind::CursorLeaveBoundary,
            ListenerMatcher::Unspecified => ListenerMatcherKind::Unspecified,
        }
    }

    /// Returns whether this matcher accepts the given input event.
    pub fn matches(&self, input: &InputEvent) -> bool {
        match self {
            ListenerMatcher::CursorButtonLeftPressInside { rect } => {
                matches!(
                    input,
                    InputEvent::CursorButton {
                        button,
                        action,
                        x,
                        y,
                        ..
                    } if button == "left" && *action == ACTION_PRESS && rect.contains(*x, *y)
                )
            }
            ListenerMatcher::CursorButtonLeftReleaseInside { rect } => {
                matches!(
                    input,
                    InputEvent::CursorButton {
                        button,
                        action,
                        x,
                        y,
                        ..
                    } if button == "left" && *action == ACTION_RELEASE && rect.contains(*x, *y)
                )
            }
            ListenerMatcher::CursorButtonMiddlePressInside { rect } => {
                matches!(
                    input,
                    InputEvent::CursorButton {
                        button,
                        action,
                        x,
                        y,
                        ..
                    } if button == "middle" && *action == ACTION_PRESS && rect.contains(*x, *y)
                )
            }
            ListenerMatcher::CursorPosInside { rect } => {
                matches!(input, InputEvent::CursorPos { x, y } if rect.contains(*x, *y))
            }
            ListenerMatcher::CursorScrollInside { rect } => matches!(
                input,
                InputEvent::CursorScroll { x, y, .. } | InputEvent::CursorScrollLines { x, y, .. }
                    if rect.contains(*x, *y)
            ),
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta => matches!(
                input,
                InputEvent::Key { key, action, mods }
                    if *action == ACTION_PRESS
                        && key.eq_ignore_ascii_case("enter")
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta => matches!(
                input,
                InputEvent::Key { key, action, mods }
                    if *action == ACTION_PRESS
                        && key.eq_ignore_ascii_case("tab")
                        && (*mods & (MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta => matches!(
                input,
                InputEvent::Key { key, action, mods }
                    if *action == ACTION_PRESS
                        && key.eq_ignore_ascii_case("tab")
                        && (*mods & MOD_SHIFT) != 0
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyXPressCtrlOrMeta => matches!(
                input,
                InputEvent::Key { key, action, mods }
                    if *action == ACTION_PRESS
                        && key.eq_ignore_ascii_case("x")
                        && (*mods & (MOD_CTRL | MOD_META)) != 0
            ),
            ListenerMatcher::KeyVPressCtrlOrMeta => matches!(
                input,
                InputEvent::Key { key, action, mods }
                    if *action == ACTION_PRESS
                        && key.eq_ignore_ascii_case("v")
                        && (*mods & (MOD_CTRL | MOD_META)) != 0
            ),
            ListenerMatcher::KeyBackspacePress => matches!(
                input,
                InputEvent::Key { key, action, .. }
                    if *action == ACTION_PRESS && key.eq_ignore_ascii_case("backspace")
            ),
            ListenerMatcher::KeyDeletePress => matches!(
                input,
                InputEvent::Key { key, action, .. }
                    if *action == ACTION_PRESS && key.eq_ignore_ascii_case("delete")
            ),
            ListenerMatcher::TextCommitNoCtrlMeta => matches!(
                input,
                InputEvent::TextCommit { mods, .. } if (*mods & (MOD_CTRL | MOD_META)) == 0
            ),
            ListenerMatcher::WindowFocused => {
                matches!(input, InputEvent::Focused { focused } if *focused)
            }
            ListenerMatcher::WindowBlurred => {
                matches!(input, InputEvent::Focused { focused } if !*focused)
            }
            ListenerMatcher::CursorLeaveBoundary { rect } => match input {
                InputEvent::CursorPos { x, y } => !rect.contains(*x, *y),
                InputEvent::CursorEntered { entered } => !*entered,
                _ => false,
            },
            ListenerMatcher::Unspecified => false,
        }
    }
}

/// Final listener sinks.
///
/// A matched listener always resolves into one or more of these sink actions.
#[derive(Clone, Debug)]
pub enum ListenerAction {
    /// Message forwarded to the tree actor.
    TreeMsg(TreeMsg),
    /// Event-runtime transient mutation.
    RuntimeChange(RuntimeChange),
    /// Event forwarded to Elixir-side consumers.
    ElixirEvent(ElixirEvent),
}

/// Transient event-runtime state changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeChange {
    /// Begin click/press followup tracking for pointer interaction.
    StartClickPressTracker {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        emit_click: bool,
        emit_press_pointer: bool,
    },
    /// Begin drag threshold tracking.
    StartDragTracker,
    /// End drag tracking on pointer release.
    ClearDragTracker,
    /// Toggle hover runtime state.
    SetMouseOverActive { element_id: ElementId, active: bool },
    /// Toggle mouse-down style runtime state.
    SetMouseDownActive { element_id: ElementId, active: bool },
    /// Toggle focused style runtime state.
    SetFocusedActive { element_id: ElementId, active: bool },
    /// Request focus transition to an explicit element (or none).
    RequestFocusSet { next: Option<ElementId> },
    /// Request a focus-cycle transition from the current focused element.
    RequestFocusCycle { reverse: bool },
    /// Request a text-input command operation (cut/paste) in runtime.
    TextInputCommand {
        element_id: ElementId,
        request: TextInputCommandRequest,
        emit_change: bool,
    },
    /// Start or stop text-input drag selection target tracking.
    SetTextInputDragTarget(Option<ElementId>),
    /// Mark whether pointer interaction consumed click-like semantics.
    SetDragConsumed(bool),
}

/// Elixir-facing element event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElixirEvent {
    /// Target element id.
    pub element_id: ElementId,
    /// Logical event kind.
    pub kind: ElementEventKind,
    /// Optional string payload.
    pub payload: Option<String>,
}

/// Computes output sink actions for a matched listener.
///
/// This is where input-dependent outputs are generated.
#[derive(Clone, Debug, Default)]
pub enum ListenerCompute {
    /// Fixed action list independent of input payload.
    Static { actions: Vec<ListenerAction> },
    /// Build `TreeMsg::ScrollRequest` actions from `CursorScroll` deltas.
    ScrollTreeMsgsFromCursorScroll {
        element_id: ElementId,
        allow_x: bool,
        allow_y: bool,
    },
    /// Apply a fixed text-edit request and emit tree/runtime updates.
    TextInputEditToTreeAndMaybeChange {
        element_id: ElementId,
        content: String,
        cursor: u32,
        selection_anchor: Option<u32>,
        focused: bool,
        emit_change: bool,
        request: TextInputEditRequest,
    },
    /// Apply text-commit insertion and emit tree/runtime updates.
    TextCommitToTreeAndMaybeChange {
        element_id: ElementId,
        content: String,
        cursor: u32,
        selection_anchor: Option<u32>,
        focused: bool,
        emit_change: bool,
    },
    /// Placeholder variant used while more compute variants are introduced.
    #[default]
    Unspecified,
}

impl ListenerCompute {
    /// Compute final sink actions from the matched input.
    pub fn compute(&self, input: &InputEvent) -> Vec<ListenerAction> {
        match self {
            ListenerCompute::Static { actions } => actions.clone(),
            ListenerCompute::ScrollTreeMsgsFromCursorScroll {
                element_id,
                allow_x,
                allow_y,
            } => {
                let (dx, dy) = match input {
                    InputEvent::CursorScroll { dx, dy, .. }
                    | InputEvent::CursorScrollLines { dx, dy, .. } => (*dx, *dy),
                    _ => return Vec::new(),
                };

                let mut out = Vec::new();
                if *allow_x && dx.abs() > f32::EPSILON {
                    out.push(ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                        element_id: element_id.clone(),
                        dx,
                        dy: 0.0,
                    }));
                }
                if *allow_y && dy.abs() > f32::EPSILON {
                    out.push(ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                        element_id: element_id.clone(),
                        dx: 0.0,
                        dy,
                    }));
                }
                out
            }
            ListenerCompute::TextInputEditToTreeAndMaybeChange {
                element_id,
                content,
                cursor,
                selection_anchor,
                focused,
                emit_change,
                request,
            } => text_ops::apply_edit_request(content, *cursor, *selection_anchor, request)
                .map(|(next_content, next_cursor)| {
                    text_edit_actions(
                        element_id,
                        *focused,
                        *emit_change,
                        next_content,
                        next_cursor,
                    )
                })
                .unwrap_or_default(),
            ListenerCompute::TextCommitToTreeAndMaybeChange {
                element_id,
                content,
                cursor,
                selection_anchor,
                focused,
                emit_change,
            } => match input {
                InputEvent::TextCommit { text, mods } if (*mods & (MOD_CTRL | MOD_META)) == 0 => {
                    let filtered: String = text.chars().filter(|ch| !ch.is_control()).collect();
                    text_ops::apply_insert(content, *cursor, *selection_anchor, &filtered)
                        .map(|(next_content, next_cursor)| {
                            text_edit_actions(
                                element_id,
                                *focused,
                                *emit_change,
                                next_content,
                                next_cursor,
                            )
                        })
                        .unwrap_or_default()
                }
                _ => Vec::new(),
            },
            ListenerCompute::Unspecified => Vec::new(),
        }
    }
}

fn text_edit_actions(
    element_id: &ElementId,
    focused: bool,
    emit_change: bool,
    next_content: String,
    next_cursor: u32,
) -> Vec<ListenerAction> {
    [
        Some(ListenerAction::TreeMsg(TreeMsg::SetTextInputContent {
            element_id: element_id.clone(),
            content: next_content.clone(),
        })),
        Some(ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime {
            element_id: element_id.clone(),
            focused,
            cursor: Some(next_cursor),
            selection_anchor: None,
            preedit: None,
            preedit_cursor: None,
        })),
        emit_change.then(|| {
            ListenerAction::ElixirEvent(ElixirEvent {
                element_id: element_id.clone(),
                kind: ElementEventKind::Change,
                payload: Some(next_content),
            })
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Convenience tuple produced by element-level builder functions.
pub type BucketedListener = (BucketId, Listener);

/// Slot builder for one deterministic matcher/bucket position.
type ElementSlotBuilder = fn(&Element) -> Option<BucketedListener>;

/// Deterministic slot order for base element listener assembly.
///
/// Reordering this table changes behavior.
const ELEMENT_LISTENER_SLOTS: &[ElementSlotBuilder] = &[
    slot_primary_left_press,
    slot_primary_left_release,
    slot_text_commit,
    slot_key_backspace_press,
    slot_key_delete_press,
    slot_key_cut_press,
    slot_key_paste_press,
    slot_middle_paste_primary_press,
    slot_key_tab_forward,
    slot_key_tab_reverse,
    slot_key_enter_press,
    slot_window_focused,
    slot_window_blurred,
    slot_primary_cursor_pos,
    slot_cursor_enter,
    slot_cursor_leave,
    slot_primary_scroll,
];

/// Return pointer hit rect only when interaction data exists and is visible.
///
/// Pointer-driven slots use this gate; non-pointer features should not.
fn pointer_hit_rect(element: &Element) -> Option<Rect> {
    let interaction = element.interaction?;
    interaction.visible.then_some(interaction.hit_rect)
}

#[derive(Clone, Debug)]
struct FocusedTextInputState {
    element_id: ElementId,
    content: String,
    cursor: u32,
    selection_anchor: Option<u32>,
    focused: bool,
    emit_change: bool,
}

fn focused_text_input_state(element: &Element) -> Option<FocusedTextInputState> {
    if element.kind != ElementKind::TextInput {
        return None;
    }

    let focused = element.attrs.text_input_focused.unwrap_or(false);
    if !focused {
        return None;
    }

    let content = element.attrs.content.clone().unwrap_or_default();
    let content_len = text_ops::text_char_len(&content);
    let cursor = element
        .attrs
        .text_input_cursor
        .unwrap_or(content_len)
        .min(content_len);
    let selection_anchor = element
        .attrs
        .text_input_selection_anchor
        .map(|anchor| anchor.min(content_len))
        .filter(|anchor| *anchor != cursor);

    Some(FocusedTextInputState {
        element_id: element.id.clone(),
        content,
        cursor,
        selection_anchor,
        focused,
        emit_change: element.attrs.on_change.unwrap_or(false),
    })
}

fn text_input_emit_change(element: &Element) -> Option<bool> {
    (element.kind == ElementKind::TextInput).then_some(element.attrs.on_change.unwrap_or(false))
}

fn is_focusable(element: &Element) -> bool {
    element.kind == ElementKind::TextInput
        || element.attrs.on_press.unwrap_or(false)
        || element.attrs.on_focus.unwrap_or(false)
        || element.attrs.on_blur.unwrap_or(false)
}

/// Build first-iteration listeners for one element.
///
/// Current coverage:
/// - `on_mouse_down`, `on_mouse_up`, `on_mouse_move` (primary bucket)
/// - hover enter/leave style transitions (`mouse_over` + `mouse_over_active`)
/// - mouse-down style transitions (`mouse_down` + `mouse_down_active`)
/// - pointer tracker bootstrap for `on_click` and pointer `on_press`
/// - focused Enter-key `on_press` listeners
/// - focused window `on_focus`/`on_blur` listeners
/// - focus-transition scaffolding (`RequestFocusSet`, focused Tab cycle)
/// - focused text-input edit listeners with `on_change`-gated change emission
/// - text-input command listeners for cut/paste command requests
/// - local wheel-scroll listeners for scrollable elements
pub fn listeners_for_element(element: &Element) -> Vec<BucketedListener> {
    ELEMENT_LISTENER_SLOTS
        .iter()
        .flat_map(|build| build(element))
        .collect()
}

/// Build a base registry from a list of elements.
pub fn registry_for_elements(elements: &[Element]) -> Registry {
    elements.iter().flat_map(listeners_for_element).fold(
        Registry::default(),
        |mut registry, (bucket, listener)| {
            registry.push_listener(bucket, listener);
            registry
        },
    )
}

/// Build primary left-press listener.
///
/// Aggregates actions from mouse events, mouse-down style activation, and
/// click/press tracker bootstrap.
fn slot_primary_left_press(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let matcher = ListenerMatcher::CursorButtonLeftPressInside { rect: hit_rect };
    let actions: Vec<ListenerAction> = [
        mouse_events::left_press_actions(element),
        mouse_down_style::left_press_actions(element),
        focus_transitions::left_press_actions(element),
        click_press_tracker::left_press_actions(element, matcher.kind()),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher,
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build primary left-release listener.
///
/// Aggregates actions from mouse-up event emission and mouse-down style clear.
fn slot_primary_left_release(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let actions: Vec<ListenerAction> = [
        mouse_events::left_release_actions(element),
        mouse_down_style::left_release_actions(element),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::CursorButtonLeftReleaseInside { rect: hit_rect },
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build primary cursor-position listener.
///
/// Emits move actions for `on_mouse_move`.
fn slot_primary_cursor_pos(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let actions = mouse_events::cursor_pos_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::CursorPosInside { rect: hit_rect },
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build Enter key press listener for focused `on_press` behavior.
fn slot_key_enter_press(element: &Element) -> Option<BucketedListener> {
    let actions = on_press_keyboard::enter_press_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::KeyEnterPressNoCtrlAltMeta,
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build Tab key (forward) listener for currently focused focusable elements.
fn slot_key_tab_forward(element: &Element) -> Option<BucketedListener> {
    let actions = focus_transitions::tab_forward_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta,
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build Shift+Tab key listener for currently focused focusable elements.
fn slot_key_tab_reverse(element: &Element) -> Option<BucketedListener> {
    let actions = focus_transitions::tab_reverse_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta,
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build text-commit listener for focused text inputs.
fn slot_text_commit(element: &Element) -> Option<BucketedListener> {
    let state = focused_text_input_state(element)?;

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(state.element_id.clone()),
            matcher: ListenerMatcher::TextCommitNoCtrlMeta,
            compute: ListenerCompute::TextCommitToTreeAndMaybeChange {
                element_id: state.element_id,
                content: state.content,
                cursor: state.cursor,
                selection_anchor: state.selection_anchor,
                focused: state.focused,
                emit_change: state.emit_change,
            },
        },
    ))
}

/// Build Backspace-key listener for focused text inputs.
fn slot_key_backspace_press(element: &Element) -> Option<BucketedListener> {
    let state = focused_text_input_state(element)?;

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(state.element_id.clone()),
            matcher: ListenerMatcher::KeyBackspacePress,
            compute: ListenerCompute::TextInputEditToTreeAndMaybeChange {
                element_id: state.element_id,
                content: state.content,
                cursor: state.cursor,
                selection_anchor: state.selection_anchor,
                focused: state.focused,
                emit_change: state.emit_change,
                request: TextInputEditRequest::Backspace,
            },
        },
    ))
}

/// Build Delete-key listener for focused text inputs.
fn slot_key_delete_press(element: &Element) -> Option<BucketedListener> {
    let state = focused_text_input_state(element)?;

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(state.element_id.clone()),
            matcher: ListenerMatcher::KeyDeletePress,
            compute: ListenerCompute::TextInputEditToTreeAndMaybeChange {
                element_id: state.element_id,
                content: state.content,
                cursor: state.cursor,
                selection_anchor: state.selection_anchor,
                focused: state.focused,
                emit_change: state.emit_change,
                request: TextInputEditRequest::Delete,
            },
        },
    ))
}

/// Build Ctrl/Meta+X cut command listener for focused text inputs.
fn slot_key_cut_press(element: &Element) -> Option<BucketedListener> {
    let state = focused_text_input_state(element)?;
    let actions = text_input_commands::command_actions(
        &state.element_id,
        TextInputCommandRequest::Cut,
        state.emit_change,
    );

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(state.element_id),
            matcher: ListenerMatcher::KeyXPressCtrlOrMeta,
            compute: ListenerCompute::Static { actions },
        },
    ))
}

/// Build Ctrl/Meta+V paste command listener for focused text inputs.
fn slot_key_paste_press(element: &Element) -> Option<BucketedListener> {
    let state = focused_text_input_state(element)?;
    let actions = text_input_commands::command_actions(
        &state.element_id,
        TextInputCommandRequest::Paste,
        state.emit_change,
    );

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(state.element_id),
            matcher: ListenerMatcher::KeyVPressCtrlOrMeta,
            compute: ListenerCompute::Static { actions },
        },
    ))
}

/// Build middle-button paste-primary command listener for text inputs.
fn slot_middle_paste_primary_press(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let emit_change = text_input_emit_change(element)?;
    let actions = text_input_commands::command_actions(
        &element.id,
        TextInputCommandRequest::PastePrimary,
        emit_change,
    );

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorButtonMiddlePressInside { rect: hit_rect },
            compute: ListenerCompute::Static { actions },
        },
    ))
}

/// Build window-focused listener for focused elements with `on_focus`.
fn slot_window_focused(element: &Element) -> Option<BucketedListener> {
    let actions = focus_events::window_focused_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::WindowFocused,
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build window-blurred listener for focused elements (`on_blur` and focused style clear).
fn slot_window_blurred(element: &Element) -> Option<BucketedListener> {
    let actions = focus_events::window_blurred_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::Primary,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::WindowBlurred,
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build cursor-enter listener.
///
/// Emits enter actions only when hover is currently inactive.
fn slot_cursor_enter(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let actions = hover::enter_actions(element);

    (!actions.is_empty()).then(|| {
        (
            BucketId::CursorEnter,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::CursorPosInside { rect: hit_rect },
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build cursor-leave listener.
///
/// Aggregates hover leave actions and mouse-down style clear into one listener.
fn slot_cursor_leave(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let actions: Vec<ListenerAction> = [
        hover::leave_actions(element),
        mouse_down_style::leave_actions(element),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!actions.is_empty()).then(|| {
        (
            BucketId::CursorLeave,
            Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::CursorLeaveBoundary { rect: hit_rect },
                compute: ListenerCompute::Static { actions },
            },
        )
    })
}

/// Build primary scroll listener.
///
/// Emits wheel-scroll compute only when an enabled axis has scroll extent.
fn slot_primary_scroll(element: &Element) -> Option<BucketedListener> {
    let hit_rect = pointer_hit_rect(element)?;
    let compute = scroll_wheel::scroll_compute(element)?;

    Some((
        BucketId::Primary,
        Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorScrollInside { rect: hit_rect },
            compute,
        },
    ))
}

/// Mouse event action contributors (`on_mouse_down`, `on_mouse_up`, `on_mouse_move`).
mod mouse_events {
    use super::*;

    pub(super) fn left_press_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let attrs = &element.attrs;
        let on_mouse_down = attrs.on_mouse_down.unwrap_or(false);
        on_mouse_down
            .then(|| {
                vec![ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::MouseDown,
                    payload: None,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn left_release_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let on_mouse_up = element.attrs.on_mouse_up.unwrap_or(false);

        on_mouse_up
            .then(|| {
                vec![ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::MouseUp,
                    payload: None,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn cursor_pos_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let on_mouse_move = element.attrs.on_mouse_move.unwrap_or(false);

        on_mouse_move
            .then(|| {
                vec![ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::MouseMove,
                    payload: None,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Hover action contributors (`on_mouse_enter`, `on_mouse_leave`, `mouse_over`).
mod hover {
    use super::*;

    pub(super) fn enter_actions(element: &Element) -> Vec<ListenerAction> {
        let attrs = &element.attrs;
        let element_id = element.id.clone();
        let has_hover_style = attrs.mouse_over.is_some();
        let hover_active = attrs.mouse_over_active.unwrap_or(false);
        let on_mouse_enter = attrs.on_mouse_enter.unwrap_or(false);
        let on_mouse_leave = attrs.on_mouse_leave.unwrap_or(false);
        let track_hover_active = has_hover_style || on_mouse_enter || on_mouse_leave;

        if hover_active {
            return Vec::new();
        }

        [
            on_mouse_enter.then(|| {
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element_id.clone(),
                    kind: ElementEventKind::MouseEnter,
                    payload: None,
                })
            }),
            track_hover_active.then(|| {
                ListenerAction::RuntimeChange(RuntimeChange::SetMouseOverActive {
                    element_id: element_id.clone(),
                    active: true,
                })
            }),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub(super) fn leave_actions(element: &Element) -> Vec<ListenerAction> {
        let attrs = &element.attrs;
        let element_id = element.id.clone();
        let has_hover_style = attrs.mouse_over.is_some();
        let hover_active = attrs.mouse_over_active.unwrap_or(false);
        let on_mouse_leave = attrs.on_mouse_leave.unwrap_or(false);
        let on_mouse_enter = attrs.on_mouse_enter.unwrap_or(false);
        let track_hover_active = has_hover_style || on_mouse_enter || on_mouse_leave;

        if !hover_active {
            return Vec::new();
        }

        [
            on_mouse_leave.then(|| {
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element_id.clone(),
                    kind: ElementEventKind::MouseLeave,
                    payload: None,
                })
            }),
            track_hover_active.then(|| {
                ListenerAction::RuntimeChange(RuntimeChange::SetMouseOverActive {
                    element_id: element_id.clone(),
                    active: false,
                })
            }),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Mouse-down style contributors (`mouse_down`, `mouse_down_active`).
mod mouse_down_style {
    use super::*;

    fn has_and_active(element: &Element) -> (bool, bool) {
        let attrs = &element.attrs;
        let has_mouse_down_style = attrs.mouse_down.is_some();
        let mouse_down_active = attrs.mouse_down_active.unwrap_or(false);
        (has_mouse_down_style, mouse_down_active)
    }

    pub(super) fn left_press_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let (has_mouse_down_style, mouse_down_active) = has_and_active(element);

        (has_mouse_down_style && !mouse_down_active)
            .then(|| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::SetMouseDownActive {
                        element_id,
                        active: true,
                    },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn left_release_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let (has_mouse_down_style, mouse_down_active) = has_and_active(element);

        (has_mouse_down_style && mouse_down_active)
            .then(|| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::SetMouseDownActive {
                        element_id,
                        active: false,
                    },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn leave_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let (has_mouse_down_style, mouse_down_active) = has_and_active(element);

        (has_mouse_down_style && mouse_down_active)
            .then(|| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::SetMouseDownActive {
                        element_id,
                        active: false,
                    },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Focus-transition scaffolding contributors (pointer press + Tab cycle).
mod focus_transitions {
    use super::*;

    pub(super) fn left_press_actions(element: &Element) -> Vec<ListenerAction> {
        let focused_active = element.attrs.focused_active.unwrap_or(false);
        (is_focusable(element) && !focused_active)
            .then(|| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::RequestFocusSet {
                        next: Some(element.id.clone()),
                    },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn tab_forward_actions(element: &Element) -> Vec<ListenerAction> {
        let focused_active = element.attrs.focused_active.unwrap_or(false);
        (is_focusable(element) && focused_active)
            .then(|| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::RequestFocusCycle { reverse: false },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn tab_reverse_actions(element: &Element) -> Vec<ListenerAction> {
        let focused_active = element.attrs.focused_active.unwrap_or(false);
        (is_focusable(element) && focused_active)
            .then(|| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::RequestFocusCycle { reverse: true },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Click/press tracker bootstrap contributors (`on_click`, pointer `on_press`).
mod click_press_tracker {
    use super::*;

    pub(super) fn left_press_actions(
        element: &Element,
        matcher_kind: ListenerMatcherKind,
    ) -> Vec<ListenerAction> {
        let attrs = &element.attrs;
        let emit_click = attrs.on_click.unwrap_or(false);
        let emit_press_pointer = attrs.on_press.unwrap_or(false);
        let element_id = element.id.clone();

        (emit_click || emit_press_pointer)
            .then(|| {
                vec![
                    ListenerAction::RuntimeChange(RuntimeChange::StartClickPressTracker {
                        element_id,
                        matcher_kind,
                        emit_click,
                        emit_press_pointer,
                    }),
                    ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker),
                ]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Keyboard `on_press` contributor (focused Enter key press).
mod on_press_keyboard {
    use super::*;

    pub(super) fn enter_press_actions(element: &Element) -> Vec<ListenerAction> {
        let attrs = &element.attrs;
        let emit_press = attrs.on_press.unwrap_or(false) && attrs.focused_active.unwrap_or(false);

        emit_press
            .then(|| {
                vec![ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element.id.clone(),
                    kind: ElementEventKind::Press,
                    payload: None,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Text-input command contributors (cut/paste variants).
mod text_input_commands {
    use super::*;

    pub(super) fn command_actions(
        element_id: &ElementId,
        request: TextInputCommandRequest,
        emit_change: bool,
    ) -> Vec<ListenerAction> {
        vec![ListenerAction::RuntimeChange(
            RuntimeChange::TextInputCommand {
                element_id: element_id.clone(),
                request,
                emit_change,
            },
        )]
    }
}

/// Focus event/style contributors (`on_focus`, `on_blur`, `focused`).
mod focus_events {
    use super::*;

    pub(super) fn window_focused_actions(element: &Element) -> Vec<ListenerAction> {
        let attrs = &element.attrs;
        let focused_active = attrs.focused_active.unwrap_or(false);
        let on_focus = attrs.on_focus.unwrap_or(false);

        (focused_active && on_focus)
            .then(|| {
                vec![ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element.id.clone(),
                    kind: ElementEventKind::Focus,
                    payload: None,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn window_blurred_actions(element: &Element) -> Vec<ListenerAction> {
        let attrs = &element.attrs;
        let focused_active = attrs.focused_active.unwrap_or(false);
        if !focused_active {
            return Vec::new();
        }

        let on_blur = attrs.on_blur.unwrap_or(false);
        [
            on_blur.then(|| {
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element.id.clone(),
                    kind: ElementEventKind::Blur,
                    payload: None,
                })
            }),
            Some(ListenerAction::RuntimeChange(
                RuntimeChange::SetFocusedActive {
                    element_id: element.id.clone(),
                    active: false,
                },
            )),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Wheel-scroll compute contributor (`scrollbar_x/y`, `scroll_x_max/y_max`).
mod scroll_wheel {
    use super::*;

    pub(super) fn scroll_compute(element: &Element) -> Option<ListenerCompute> {
        let attrs = &element.attrs;
        let scrollbar_x = attrs.scrollbar_x.unwrap_or(false);
        let scrollbar_y = attrs.scrollbar_y.unwrap_or(false);
        let scroll_x_max = attrs.scroll_x_max.unwrap_or(0.0);
        let scroll_y_max = attrs.scroll_y_max.unwrap_or(0.0);

        let allow_x = scrollbar_x && scroll_x_max > 0.0;
        let allow_y = scrollbar_y && scroll_y_max > 0.0;

        (allow_x || allow_y).then(|| ListenerCompute::ScrollTreeMsgsFromCursorScroll {
            element_id: element.id.clone(),
            allow_x,
            allow_y,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::actors::TreeMsg;
    use crate::input::{
        ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
    };
    use crate::tree::attrs::{Attrs, MouseOverAttrs};
    use crate::tree::element::{Element, ElementId, ElementKind};
    use crate::tree::interaction::{ElementInteraction, Rect};

    use super::{
        BucketId, ClickPressTracker, DragTrackerState, ElixirEvent, ListenerAction,
        ListenerCompute, ListenerMatcher, ListenerMatcherKind, RuntimeChange, RuntimeOverlayState,
        compose_effective_registry, listeners_for_element, registry_for_elements,
    };
    use crate::events::{TextInputCommandRequest, dispatch_outcome::ElementEventKind};

    fn make_element(id: u8, attrs: Attrs) -> Element {
        Element::with_attrs(
            ElementId::from_term_bytes(vec![id]),
            ElementKind::El,
            Vec::new(),
            attrs,
        )
    }

    fn make_text_input_element(id: u8, attrs: Attrs) -> Element {
        Element::with_attrs(
            ElementId::from_term_bytes(vec![id]),
            ElementKind::TextInput,
            Vec::new(),
            attrs,
        )
    }

    fn build_interaction(visible: bool) -> ElementInteraction {
        ElementInteraction {
            visible,
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
        }
    }

    fn with_interaction(mut element: Element, visible: bool) -> Element {
        element.interaction = Some(build_interaction(visible));
        element
    }

    fn first_matching_actions(
        registry: &super::Registry,
        bucket: BucketId,
        input: &InputEvent,
    ) -> Vec<ListenerAction> {
        registry
            .bucket(bucket)
            .listeners
            .iter()
            .find(|listener| listener.matcher.matches(input))
            .map(|listener| listener.compute_actions(input))
            .unwrap_or_default()
    }

    #[test]
    fn listeners_for_element_returns_empty_for_invisible_nodes() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_down = Some(true);
        let element = with_interaction(make_element(1, attrs), false);

        let listeners = listeners_for_element(&element);
        assert!(listeners.is_empty());
    }

    #[test]
    fn listeners_for_element_returns_empty_when_interaction_missing() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_down = Some(true);
        let element = make_element(1, attrs);

        let listeners = listeners_for_element(&element);
        assert!(listeners.is_empty());
    }

    #[test]
    fn listeners_for_element_builds_primary_pointer_listeners() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_down = Some(true);
        attrs.on_mouse_up = Some(true);
        attrs.on_mouse_move = Some(true);
        let element = with_interaction(make_element(2, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 3);
        assert_eq!(listeners[0].0, BucketId::Primary);
        assert_eq!(listeners[1].0, BucketId::Primary);
        assert_eq!(listeners[2].0, BucketId::Primary);

        let down_input = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };
        let up_input = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };
        let move_input = InputEvent::CursorPos { x: 10.0, y: 10.0 };

        let down_actions = listeners[0].1.compute_actions(&down_input);
        let up_actions = listeners[1].1.compute_actions(&up_input);
        let move_actions = listeners[2].1.compute_actions(&move_input);

        assert!(matches!(
            down_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseDown,
                ..
            })]
        ));
        assert!(matches!(
            up_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseUp,
                ..
            })]
        ));
        assert!(matches!(
            move_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseMove,
                ..
            })]
        ));
    }

    #[test]
    fn listeners_for_element_builds_enter_listener_when_hover_inactive() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_enter = Some(true);
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);
        let element = with_interaction(make_element(3, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::CursorEnter);
        assert!(matches!(
            listeners[0].1.matcher,
            ListenerMatcher::CursorPosInside { .. }
        ));

        let actions = listeners[0]
            .1
            .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseEnter,
                ..
            })
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetMouseOverActive { active: true, .. })
        ));
    }

    #[test]
    fn listeners_for_element_builds_leave_listener_when_hover_active() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_leave = Some(true);
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(true);
        let element = with_interaction(make_element(4, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::CursorLeave);
        assert!(matches!(
            listeners[0].1.matcher,
            ListenerMatcher::CursorLeaveBoundary { .. }
        ));

        let actions = listeners[0]
            .1
            .compute_actions(&InputEvent::CursorPos { x: 120.0, y: 10.0 });
        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseLeave,
                ..
            })
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetMouseOverActive { active: false, .. })
        ));
    }

    #[test]
    fn listeners_for_element_event_only_hover_tracks_active_for_leave() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_enter = Some(true);
        attrs.on_mouse_leave = Some(true);
        attrs.mouse_over_active = Some(false);
        let element = with_interaction(make_element(22, attrs), true);

        let listeners = listeners_for_element(&element);
        let enter_listener = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
            })
            .expect("expected enter listener");
        let actions = enter_listener
            .1
            .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseEnter,
                ..
            })
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetMouseOverActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![22]) && active
        ));
    }

    #[test]
    fn listeners_for_element_event_only_leave_emits_event_and_clears_hover_active() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_leave = Some(true);
        attrs.mouse_over_active = Some(true);
        let element = with_interaction(make_element(23, attrs), true);

        let listeners = listeners_for_element(&element);
        let leave_listener = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorLeaveBoundary { .. }
                )
            })
            .expect("expected leave listener");
        let actions = leave_listener
            .1
            .compute_actions(&InputEvent::CursorEntered { entered: false });

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseLeave,
                ..
            })
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetMouseOverActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![23]) && !active
        ));
    }

    #[test]
    fn cursor_leave_boundary_matcher_matches_window_leave_and_outside_pos() {
        let rect = build_interaction(true).hit_rect;
        let matcher = ListenerMatcher::CursorLeaveBoundary { rect };

        assert!(matcher.matches(&InputEvent::CursorPos { x: 120.0, y: 10.0 }));
        assert!(matcher.matches(&InputEvent::CursorEntered { entered: false }));
        assert!(!matcher.matches(&InputEvent::CursorPos { x: 20.0, y: 10.0 }));
        assert!(!matcher.matches(&InputEvent::CursorEntered { entered: true }));
    }

    #[test]
    fn matcher_kind_uses_variant_identity_only() {
        let a = ListenerMatcher::CursorButtonLeftPressInside {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        };
        let b = ListenerMatcher::CursorButtonLeftPressInside {
            rect: Rect {
                x: 50.0,
                y: 50.0,
                width: 20.0,
                height: 20.0,
            },
        };
        let c = ListenerMatcher::CursorButtonLeftReleaseInside {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        };

        assert_eq!(a.kind(), ListenerMatcherKind::CursorButtonLeftPressInside);
        assert_eq!(a.kind(), b.kind());
        assert_ne!(a.kind(), c.kind());
    }

    #[test]
    fn listeners_for_element_mouse_down_style_inactive_adds_press_activate() {
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(false);
        let element = with_interaction(make_element(5, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::Primary);
        assert!(matches!(
            listeners[0].1.matcher,
            ListenerMatcher::CursorButtonLeftPressInside { .. }
        ));
        assert_eq!(
            listeners[0].1.element_id,
            Some(ElementId::from_term_bytes(vec![5]))
        );

        let actions = listeners[0].1.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::SetMouseDownActive { element_id, active })]
                if *element_id == ElementId::from_term_bytes(vec![5]) && *active
        ));
    }

    #[test]
    fn listeners_for_element_merges_mouse_down_event_and_style_into_single_press_listener() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_down = Some(true);
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(false);
        let element = with_interaction(make_element(10, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::Primary);
        assert!(matches!(
            listeners[0].1.matcher,
            ListenerMatcher::CursorButtonLeftPressInside { .. }
        ));

        let actions = listeners[0].1.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseDown,
                ..
            })
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetMouseDownActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![10]) && active
        ));
    }

    #[test]
    fn listeners_for_element_merges_press_slot_actions_in_builder_order() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_down = Some(true);
        attrs.on_click = Some(true);
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(false);
        let element = with_interaction(make_element(11, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::Primary);

        let actions = listeners[0].1.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert_eq!(actions.len(), 4);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseDown,
                ..
            })
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetMouseDownActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![11]) && active
        ));
        assert!(matches!(
            actions[2],
            ListenerAction::RuntimeChange(RuntimeChange::StartClickPressTracker {
                ref element_id,
                emit_click,
                emit_press_pointer,
                ..
            }) if *element_id == ElementId::from_term_bytes(vec![11])
                && emit_click
                && !emit_press_pointer
        ));
        assert!(matches!(
            actions[3],
            ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker)
        ));
    }

    #[test]
    fn listeners_for_element_mouse_down_style_active_adds_release_and_leave_clear() {
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(true);
        let element = with_interaction(make_element(6, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 2);

        let release_listener = listeners
            .iter()
            .find(|(bucket, listener)| {
                *bucket == BucketId::Primary
                    && matches!(
                        listener.matcher,
                        ListenerMatcher::CursorButtonLeftReleaseInside { .. }
                    )
            })
            .expect("release clear listener missing");

        let release_actions = release_listener
            .1
            .compute_actions(&InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            });
        assert!(matches!(
            release_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::SetMouseDownActive { element_id, active })]
                if *element_id == ElementId::from_term_bytes(vec![6]) && !*active
        ));

        let leave_listener = listeners
            .iter()
            .find(|(bucket, listener)| {
                *bucket == BucketId::CursorLeave
                    && matches!(
                        listener.matcher,
                        ListenerMatcher::CursorLeaveBoundary { .. }
                    )
            })
            .expect("leave clear listener missing");

        let leave_actions = leave_listener
            .1
            .compute_actions(&InputEvent::CursorEntered { entered: false });
        assert!(matches!(
            leave_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::SetMouseDownActive { element_id, active })]
                if *element_id == ElementId::from_term_bytes(vec![6]) && !*active
        ));
    }

    #[test]
    fn listeners_for_element_on_click_starts_click_and_drag_trackers() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(7, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::Primary);

        let matcher_kind = listeners[0].1.matcher.kind();
        let actions = listeners[0].1.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::RuntimeChange(RuntimeChange::StartClickPressTracker {
                ref element_id,
                matcher_kind: kind,
                emit_click,
                emit_press_pointer,
            }) if *element_id == ElementId::from_term_bytes(vec![7])
                && kind == matcher_kind
                && emit_click
                && !emit_press_pointer
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker)
        ));
    }

    #[test]
    fn compose_effective_registry_rematerializes_click_release_followup_from_source() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(27, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![27]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: true,
                emit_press_pointer: false,
            }),
            drag: DragTrackerState::Inactive,
        };
        let effective = compose_effective_registry(&base, &runtime);

        let actions = first_matching_actions(
            &effective,
            BucketId::Primary,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                element_id,
                kind: ElementEventKind::Click,
                payload: None,
            })] if *element_id == ElementId::from_term_bytes(vec![27])
        ));
    }

    #[test]
    fn compose_effective_registry_drops_click_followup_when_source_listener_missing() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(28, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![99]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: true,
                emit_press_pointer: false,
            }),
            drag: DragTrackerState::Inactive,
        };
        let effective = compose_effective_registry(&base, &runtime);

        let actions = first_matching_actions(
            &effective,
            BucketId::Primary,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn compose_effective_registry_drag_active_release_precedes_and_suppresses_click_followup() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(29, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![29]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: true,
                emit_press_pointer: false,
            }),
            drag: DragTrackerState::Active {
                element_id: ElementId::from_term_bytes(vec![29]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
            },
        };
        let effective = compose_effective_registry(&base, &runtime);

        let actions = first_matching_actions(
            &effective,
            BucketId::Primary,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
        );

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker)
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetDragConsumed(false))
        ));
        assert!(
            actions
                .iter()
                .all(|action| !matches!(action, ListenerAction::ElixirEvent(_)))
        );
    }

    #[test]
    fn listeners_for_element_on_press_starts_pointer_press_and_drag_trackers() {
        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        let element = with_interaction(make_element(8, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);

        let matcher_kind = listeners[0].1.matcher.kind();
        let actions = listeners[0].1.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert_eq!(actions.len(), 3);
        assert!(matches!(
            actions[0],
            ListenerAction::RuntimeChange(RuntimeChange::RequestFocusSet { ref next })
                if *next == Some(ElementId::from_term_bytes(vec![8]))
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::StartClickPressTracker {
                ref element_id,
                matcher_kind: kind,
                emit_click,
                emit_press_pointer,
            }) if *element_id == ElementId::from_term_bytes(vec![8])
                && kind == matcher_kind
                && !emit_click
                && emit_press_pointer
        ));
        assert!(matches!(
            actions[2],
            ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker)
        ));
    }

    #[test]
    fn listeners_for_element_on_press_focused_adds_key_enter_listener() {
        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(12, attrs), true);

        let listeners = listeners_for_element(&element);
        let key_listener = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::KeyEnterPressNoCtrlAltMeta
                )
            })
            .expect("expected key-enter listener");

        let actions = key_listener.1.compute_actions(&InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                element_id,
                kind: ElementEventKind::Press,
                payload: None,
            })] if *element_id == ElementId::from_term_bytes(vec![12])
        ));
    }

    #[test]
    fn listeners_for_element_on_press_not_focused_omits_key_enter_listener() {
        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        attrs.focused_active = Some(false);
        let element = with_interaction(make_element(13, attrs), true);

        let listeners = listeners_for_element(&element);
        assert!(listeners.iter().all(|(_, listener)| !matches!(
            listener.matcher,
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta
        )));
    }

    #[test]
    fn listeners_for_focusable_pointer_press_requests_focus_set() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.focused_active = Some(false);
        let element = with_interaction(make_element(24, attrs), true);

        let listeners = listeners_for_element(&element);
        let press_listener = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorButtonLeftPressInside { .. }
                )
            })
            .expect("expected left-press listener");
        let actions = press_listener.1.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert!(actions.iter().any(|action| {
            matches!(
                action,
                ListenerAction::RuntimeChange(RuntimeChange::RequestFocusSet { next })
                    if *next == Some(ElementId::from_term_bytes(vec![24]))
            )
        }));
    }

    #[test]
    fn listeners_for_focused_focusable_add_tab_cycle_listeners() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(25, attrs), true);

        let listeners = listeners_for_element(&element);
        let tab_forward = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta
                )
            })
            .expect("expected forward-tab listener");
        let tab_reverse = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta
                )
            })
            .expect("expected reverse-tab listener");

        let forward_actions = tab_forward.1.compute_actions(&InputEvent::Key {
            key: "tab".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        });
        let reverse_actions = tab_reverse.1.compute_actions(&InputEvent::Key {
            key: "tab".to_string(),
            action: ACTION_PRESS,
            mods: MOD_SHIFT,
        });

        assert!(matches!(
            forward_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::RequestFocusCycle { reverse })]
                if !reverse
        ));
        assert!(matches!(
            reverse_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::RequestFocusCycle { reverse })]
                if *reverse
        ));
    }

    #[test]
    fn listeners_for_unfocused_focusable_omit_tab_cycle_listeners() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.focused_active = Some(false);
        let element = with_interaction(make_element(26, attrs), true);

        let listeners = listeners_for_element(&element);
        assert!(listeners.iter().all(|(_, listener)| {
            !matches!(
                listener.matcher,
                ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta
                    | ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta
            )
        }));
    }

    #[test]
    fn key_enter_press_matcher_blocks_ctrl_alt_meta_and_allows_shift() {
        let matcher = ListenerMatcher::KeyEnterPressNoCtrlAltMeta;

        assert!(matcher.matches(&InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        }));
        assert!(matcher.matches(&InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: crate::input::MOD_SHIFT,
        }));

        assert!(!matcher.matches(&InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        }));
        assert!(!matcher.matches(&InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: MOD_ALT,
        }));
        assert!(!matcher.matches(&InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: MOD_META,
        }));
    }

    #[test]
    fn tab_matchers_enforce_expected_modifier_behavior() {
        assert!(
            ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta.matches(&InputEvent::Key {
                key: "tab".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            })
        );
        assert!(
            !ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta.matches(&InputEvent::Key {
                key: "tab".to_string(),
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            })
        );

        assert!(
            ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta.matches(&InputEvent::Key {
                key: "tab".to_string(),
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            })
        );
        assert!(
            !ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta.matches(&InputEvent::Key {
                key: "tab".to_string(),
                action: ACTION_PRESS,
                mods: MOD_SHIFT | MOD_CTRL,
            })
        );
    }

    #[test]
    fn key_x_and_v_matchers_require_ctrl_or_meta() {
        assert!(
            ListenerMatcher::KeyXPressCtrlOrMeta.matches(&InputEvent::Key {
                key: "x".to_string(),
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            })
        );
        assert!(
            ListenerMatcher::KeyXPressCtrlOrMeta.matches(&InputEvent::Key {
                key: "X".to_string(),
                action: ACTION_PRESS,
                mods: MOD_META | MOD_ALT,
            })
        );
        assert!(
            !ListenerMatcher::KeyXPressCtrlOrMeta.matches(&InputEvent::Key {
                key: "x".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            })
        );

        assert!(
            ListenerMatcher::KeyVPressCtrlOrMeta.matches(&InputEvent::Key {
                key: "v".to_string(),
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            })
        );
        assert!(
            ListenerMatcher::KeyVPressCtrlOrMeta.matches(&InputEvent::Key {
                key: "V".to_string(),
                action: ACTION_PRESS,
                mods: MOD_META,
            })
        );
        assert!(
            !ListenerMatcher::KeyVPressCtrlOrMeta.matches(&InputEvent::Key {
                key: "v".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            })
        );
    }

    #[test]
    fn middle_press_inside_matcher_requires_middle_press_inside_rect() {
        let rect = build_interaction(true).hit_rect;
        let matcher = ListenerMatcher::CursorButtonMiddlePressInside { rect };

        assert!(matcher.matches(&InputEvent::CursorButton {
            button: "middle".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        }));
        assert!(!matcher.matches(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        }));
        assert!(!matcher.matches(&InputEvent::CursorButton {
            button: "middle".to_string(),
            action: ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        }));
        assert!(!matcher.matches(&InputEvent::CursorButton {
            button: "middle".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 200.0,
            y: 10.0,
        }));
    }

    #[test]
    fn text_commit_matcher_blocks_ctrl_or_meta_and_accepts_plain_commit() {
        let matcher = ListenerMatcher::TextCommitNoCtrlMeta;

        assert!(matcher.matches(&InputEvent::TextCommit {
            text: "a".to_string(),
            mods: 0,
        }));
        assert!(!matcher.matches(&InputEvent::TextCommit {
            text: "a".to_string(),
            mods: MOD_CTRL,
        }));
        assert!(!matcher.matches(&InputEvent::TextCommit {
            text: "a".to_string(),
            mods: MOD_META,
        }));
    }

    #[test]
    fn key_backspace_and_delete_matchers_match_expected_keys_only() {
        assert!(
            ListenerMatcher::KeyBackspacePress.matches(&InputEvent::Key {
                key: "backspace".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            })
        );
        assert!(
            !ListenerMatcher::KeyBackspacePress.matches(&InputEvent::Key {
                key: "delete".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            })
        );

        assert!(ListenerMatcher::KeyDeletePress.matches(&InputEvent::Key {
            key: "delete".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        }));
        assert!(!ListenerMatcher::KeyDeletePress.matches(&InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        }));
    }

    #[test]
    fn listeners_for_focused_text_input_add_text_edit_slots() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        attrs.on_change = Some(true);
        let element = make_text_input_element(17, attrs);

        let listeners = listeners_for_element(&element);
        assert!(listeners.iter().any(|(_, listener)| matches!(
            listener.matcher,
            ListenerMatcher::TextCommitNoCtrlMeta
        )));
        assert!(
            listeners.iter().any(|(_, listener)| matches!(
                listener.matcher,
                ListenerMatcher::KeyBackspacePress
            ))
        );
        assert!(
            listeners
                .iter()
                .any(|(_, listener)| matches!(listener.matcher, ListenerMatcher::KeyDeletePress))
        );
        assert!(
            listeners.iter().any(|(_, listener)| matches!(
                listener.matcher,
                ListenerMatcher::KeyXPressCtrlOrMeta
            ))
        );
        assert!(
            listeners.iter().any(|(_, listener)| matches!(
                listener.matcher,
                ListenerMatcher::KeyVPressCtrlOrMeta
            ))
        );

        let commit_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::TextCommitNoCtrlMeta))
            .expect("expected text-commit listener");
        let commit_actions = commit_listener.1.compute_actions(&InputEvent::TextCommit {
            text: "x".to_string(),
            mods: 0,
        });
        assert_eq!(commit_actions.len(), 3);
        assert!(matches!(
            commit_actions[0],
            ListenerAction::TreeMsg(TreeMsg::SetTextInputContent {
                ref element_id,
                ref content,
            }) if *element_id == ElementId::from_term_bytes(vec![17]) && content == "abx"
        ));
        assert!(matches!(
            commit_actions[1],
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime {
                ref element_id,
                focused,
                cursor,
                selection_anchor,
                ref preedit,
                preedit_cursor,
            }) if *element_id == ElementId::from_term_bytes(vec![17])
                && focused
                && cursor == Some(3)
                && selection_anchor.is_none()
                && preedit.is_none()
                && preedit_cursor.is_none()
        ));
        assert!(matches!(
            commit_actions[2],
            ListenerAction::ElixirEvent(ElixirEvent {
                ref element_id,
                kind: ElementEventKind::Change,
                payload: Some(ref payload),
            }) if *element_id == ElementId::from_term_bytes(vec![17]) && payload == "abx"
        ));

        let cut_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::KeyXPressCtrlOrMeta))
            .expect("expected cut command listener");
        let cut_actions = cut_listener.1.compute_actions(&InputEvent::Key {
            key: "x".to_string(),
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        });
        assert!(matches!(
            cut_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::TextInputCommand {
                element_id,
                request,
                emit_change,
            })]
                if *element_id == ElementId::from_term_bytes(vec![17])
                    && *request == TextInputCommandRequest::Cut
                    && *emit_change
        ));

        let paste_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::KeyVPressCtrlOrMeta))
            .expect("expected paste command listener");
        let paste_actions = paste_listener.1.compute_actions(&InputEvent::Key {
            key: "v".to_string(),
            action: ACTION_PRESS,
            mods: MOD_META,
        });
        assert!(matches!(
            paste_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::TextInputCommand {
                element_id,
                request,
                emit_change,
            })]
                if *element_id == ElementId::from_term_bytes(vec![17])
                    && *request == TextInputCommandRequest::Paste
                    && *emit_change
        ));
    }

    #[test]
    fn listeners_for_focused_text_input_without_on_change_emits_no_change_event() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(2);
        attrs.on_change = Some(false);
        let element = make_text_input_element(18, attrs);

        let listeners = listeners_for_element(&element);
        let commit_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::TextCommitNoCtrlMeta))
            .expect("expected text-commit listener");
        let commit_actions = commit_listener.1.compute_actions(&InputEvent::TextCommit {
            text: "x".to_string(),
            mods: 0,
        });

        assert_eq!(commit_actions.len(), 2);
        assert!(
            commit_actions
                .iter()
                .all(|action| !matches!(action, ListenerAction::ElixirEvent(_)))
        );

        let cut_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::KeyXPressCtrlOrMeta))
            .expect("expected cut command listener");
        let cut_actions = cut_listener.1.compute_actions(&InputEvent::Key {
            key: "x".to_string(),
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        });
        assert!(matches!(
            cut_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::TextInputCommand {
                element_id,
                request,
                emit_change,
            })]
                if *element_id == ElementId::from_term_bytes(vec![18])
                    && *request == TextInputCommandRequest::Cut
                    && !*emit_change
        ));
    }

    #[test]
    fn listeners_for_unfocused_text_input_omit_text_edit_slots() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(false);
        attrs.text_input_cursor = Some(2);
        attrs.on_change = Some(true);
        let element = make_text_input_element(19, attrs);

        let listeners = listeners_for_element(&element);
        assert!(listeners.iter().all(|(_, listener)| {
            !matches!(
                listener.matcher,
                ListenerMatcher::TextCommitNoCtrlMeta
                    | ListenerMatcher::KeyBackspacePress
                    | ListenerMatcher::KeyDeletePress
                    | ListenerMatcher::KeyXPressCtrlOrMeta
                    | ListenerMatcher::KeyVPressCtrlOrMeta
            )
        }));
    }

    #[test]
    fn listeners_for_text_input_with_interaction_add_middle_paste_primary_listener() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(false);
        attrs.on_change = Some(true);
        let element = with_interaction(make_text_input_element(21, attrs), true);

        let listeners = listeners_for_element(&element);
        let middle_listener = listeners
            .iter()
            .find(|(_, listener)| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorButtonMiddlePressInside { .. }
                )
            })
            .expect("expected middle paste-primary listener");

        let actions = middle_listener
            .1
            .compute_actions(&InputEvent::CursorButton {
                button: "middle".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            });
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::TextInputCommand {
                element_id,
                request,
                emit_change,
            })]
                if *element_id == ElementId::from_term_bytes(vec![21])
                    && *request == TextInputCommandRequest::PastePrimary
                    && *emit_change
        ));
    }

    #[test]
    fn backspace_listener_emits_no_actions_when_cursor_at_start_without_selection() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(true);
        attrs.text_input_cursor = Some(0);
        attrs.on_change = Some(true);
        let element = make_text_input_element(20, attrs);

        let listeners = listeners_for_element(&element);
        let backspace_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::KeyBackspacePress))
            .expect("expected backspace listener");
        let actions = backspace_listener.1.compute_actions(&InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        });
        assert!(actions.is_empty());
    }

    #[test]
    fn window_focus_and_blur_matchers_match_focus_events() {
        assert!(ListenerMatcher::WindowFocused.matches(&InputEvent::Focused { focused: true }));
        assert!(!ListenerMatcher::WindowFocused.matches(&InputEvent::Focused { focused: false }));

        assert!(ListenerMatcher::WindowBlurred.matches(&InputEvent::Focused { focused: false }));
        assert!(!ListenerMatcher::WindowBlurred.matches(&InputEvent::Focused { focused: true }));
    }

    #[test]
    fn listeners_for_element_on_focus_focused_adds_window_focus_listener() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(14, attrs), true);

        let listeners = listeners_for_element(&element);
        let focus_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::WindowFocused))
            .expect("expected window focus listener");

        let actions = focus_listener
            .1
            .compute_actions(&InputEvent::Focused { focused: true });
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                element_id,
                kind: ElementEventKind::Focus,
                payload: None,
            })] if *element_id == ElementId::from_term_bytes(vec![14])
        ));
    }

    #[test]
    fn listeners_for_element_on_blur_focused_adds_window_blur_listener() {
        let mut attrs = Attrs::default();
        attrs.on_blur = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(15, attrs), true);

        let listeners = listeners_for_element(&element);
        let blur_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::WindowBlurred))
            .expect("expected window blur listener");

        let actions = blur_listener
            .1
            .compute_actions(&InputEvent::Focused { focused: false });
        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent {
                ref element_id,
                kind: ElementEventKind::Blur,
                payload: None,
            }) if *element_id == ElementId::from_term_bytes(vec![15])
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::RuntimeChange(RuntimeChange::SetFocusedActive { ref element_id, active })
                if *element_id == ElementId::from_term_bytes(vec![15]) && !active
        ));
    }

    #[test]
    fn listeners_for_element_focused_style_active_adds_window_blur_style_clear() {
        let mut attrs = Attrs::default();
        attrs.focused = Some(MouseOverAttrs::default());
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(16, attrs), true);

        let listeners = listeners_for_element(&element);
        let blur_listener = listeners
            .iter()
            .find(|(_, listener)| matches!(listener.matcher, ListenerMatcher::WindowBlurred))
            .expect("expected window blur listener");

        let actions = blur_listener
            .1
            .compute_actions(&InputEvent::Focused { focused: false });
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::SetFocusedActive { element_id, active })]
                if *element_id == ElementId::from_term_bytes(vec![16]) && !*active
        ));
    }

    #[test]
    fn listeners_for_element_scrollable_adds_cursor_scroll_listener() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_x_max = Some(50.0);
        attrs.scroll_y_max = Some(40.0);
        let element = with_interaction(make_element(9, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].0, BucketId::Primary);
        assert!(matches!(
            listeners[0].1.matcher,
            ListenerMatcher::CursorScrollInside { .. }
        ));

        let actions = listeners[0].1.compute_actions(&InputEvent::CursorScroll {
            dx: 3.0,
            dy: -2.0,
            x: 10.0,
            y: 10.0,
        });

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                ref element_id,
                dx,
                dy,
            }) if *element_id == ElementId::from_term_bytes(vec![9]) && (dx - 3.0).abs() < f32::EPSILON && dy.abs() < f32::EPSILON
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                ref element_id,
                dx,
                dy,
            }) if *element_id == ElementId::from_term_bytes(vec![9]) && dx.abs() < f32::EPSILON && (dy + 2.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn listener_compute_scroll_builds_tree_messages_from_input_deltas() {
        let element_id = ElementId::from_term_bytes(vec![9]);
        let compute = ListenerCompute::ScrollTreeMsgsFromCursorScroll {
            element_id: element_id.clone(),
            allow_x: true,
            allow_y: true,
        };

        let actions = compute.compute(&InputEvent::CursorScroll {
            dx: 12.0,
            dy: -6.0,
            x: 5.0,
            y: 5.0,
        });

        assert_eq!(actions.len(), 2);
        assert!(matches!(
            actions[0],
            ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                ref element_id,
                dx,
                dy,
            }) if *element_id == ElementId::from_term_bytes(vec![9]) && (dx - 12.0).abs() < f32::EPSILON && dy.abs() < f32::EPSILON
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                ref element_id,
                dx,
                dy,
            }) if *element_id == ElementId::from_term_bytes(vec![9]) && dx.abs() < f32::EPSILON && (dy + 6.0).abs() < f32::EPSILON
        ));
    }
}
