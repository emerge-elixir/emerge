//! # Registry Builder
//!
//! This module builds listener registries from two sources:
//!
//! - the retained UI tree (`base_registry`)
//! - transient runtime interaction state (`overlay_registry`)
//!
//! Dispatch itself remains simple: input is matched in precedence order and the
//! first matching listener wins.
//!
//! ## Responsibilities
//!
//! This module defines:
//!
//! - the registry storage and read/write abstractions used by the event system
//! - element listener assembly from retained tree state
//! - overlay listener assembly from transient runtime state
//! - listener matchers, computed actions, and semantic action resolution
//!
//! ## Storage Model
//!
//! `Registry` stores listeners from lowest precedence to highest precedence.
//! Reads happen through `RegistryView`, which iterates in precedence order.
//!
//! Builders should read in precedence order as well. `PrecedenceEmitter` exists
//! so builder code can be written top-to-bottom in that order while the
//! underlying registry keeps a `Vec` layout that is efficient for append and
//! reverse scan.
//!
//! ## Slot-based assembly
//!
//! Element listener assembly uses fixed slots. Each slot corresponds to one
//! matcher position and aggregates actions from multiple attribute contributors.
//!
//! This avoids same-matcher collisions under first-match semantics. For
//! example, `on_mouse_down` and `mouse_down` style activation both contribute to
//! the same left-press listener slot.

use std::collections::HashMap;
#[cfg(test)]
use std::collections::HashSet;

use crate::actors::TreeMsg;
use crate::clipboard::ClipboardTarget;
use crate::input::{
    ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
    SCROLL_LINE_PIXELS,
};
use crate::keys::CanonicalKey;
use crate::tree::attrs::{
    KeyBindingMatch, KeyBindingSpec, VirtualKeyHoldMode, VirtualKeyTapAction,
};
use crate::tree::element::{
    Element, ElementId, ElementKind, ElementTree, RetainedChildMode, RetainedPaintPhase,
};
use crate::tree::geometry::{CornerRadii, Rect, ShapeBounds, clamp_radii, point_hits_shape};
use crate::tree::scene::ResolvedNodeState;
use crate::tree::scrollbar::ScrollbarAxis;
use crate::tree::transform::{Affine2, InteractionClip, Point};

use super::{
    CursorIcon, ElementEventKind, FocusOnMountTarget, RegistryRebuildPayload,
    TextInputCommandRequest, TextInputEditRequest, TextInputPreeditRequest, TextInputState,
    scrollbar::{ScrollbarHitArea, ScrollbarNode, scrollbar_node_from_metrics},
    text_ops,
};

const RUNTIME_DRAG_DEADZONE: f32 = 10.0;
const GESTURE_AXIS_DOMINANCE_RATIO: f32 = 1.25;
const GESTURE_AXIS_MIN_LEAD: f32 = 6.0;

/// Listener registry consumed by the event actor.
///
/// Storage is optimized for the hot dispatch path:
///
/// - listeners are stored in a contiguous `Vec`
/// - storage order is low precedence -> high precedence
/// - the end of the vec is the logical top of the stack
///
/// Builder code should not depend on raw storage order. Use:
///
/// - `Registry::in_precedence_order(...)` when constructing listeners
/// - `Registry::view()` when reading them in dispatch order
#[derive(Clone, Debug, Default)]
pub struct Registry {
    listeners: Vec<Listener>,
}

impl Registry {
    /// Emit one precedence-ordered listener block into the registry.
    ///
    /// The closure should read from highest precedence to lowest precedence.
    /// Internally the appended storage slice is reversed so the underlying vec
    /// remains low-to-high precedence with the top of stack at the end.
    pub fn in_precedence_order<R>(
        &mut self,
        build: impl FnOnce(&mut PrecedenceEmitter<'_>) -> R,
    ) -> R {
        let start = self.listeners.len();
        let result = build(&mut PrecedenceEmitter {
            listeners: &mut self.listeners,
        });
        self.listeners[start..].reverse();
        result
    }

    /// Returns a precedence-ordered read view over the registry.
    pub fn view(&self) -> RegistryView<'_> {
        RegistryView {
            listeners: &self.listeners,
        }
    }

    fn extend_storage_from(&mut self, other: &Registry) {
        self.listeners.extend(other.listeners.iter().cloned());
    }

    #[cfg(test)]
    fn precedence_listeners(&self) -> Vec<Listener> {
        self.view().iter_precedence().cloned().collect()
    }
}

/// Builder sink for emitting listeners in precedence order.
///
/// The emitter lets builder code read naturally from highest precedence to
/// lowest precedence. `Registry::in_precedence_order(...)` then reverses the
/// appended storage slice so the underlying registry keeps its low-to-high
/// storage layout.
pub struct PrecedenceEmitter<'a> {
    listeners: &'a mut Vec<Listener>,
}

impl PrecedenceEmitter<'_> {
    pub fn emit(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    pub fn emit_all(&mut self, listeners: impl IntoIterator<Item = Listener>) {
        self.listeners.extend(listeners);
    }

    pub fn emit_opt(&mut self, listener: Option<Listener>) {
        if let Some(listener) = listener {
            self.emit(listener);
        }
    }
}

/// Precedence-ordered read view over one registry.
///
/// This hides the registry's physical storage order and exposes the logical
/// dispatch order used by first-match listener resolution.
#[derive(Clone, Copy)]
pub struct RegistryView<'a> {
    listeners: &'a [Listener],
}

impl<'a> RegistryView<'a> {
    pub fn iter_precedence(&self) -> impl Iterator<Item = &'a Listener> + 'a {
        self.listeners.iter().rev()
    }

    pub fn any_precedence(&self, predicate: impl FnMut(&Listener) -> bool) -> bool {
        self.iter_precedence().any(predicate)
    }

    pub fn find_precedence(
        &self,
        mut predicate: impl FnMut(&Listener) -> bool,
    ) -> Option<&'a Listener> {
        self.iter_precedence().find(|listener| predicate(listener))
    }

    pub fn matching_listener(
        &self,
        input: &ListenerInput,
        skip_matchers: &[ListenerMatcherKind],
    ) -> Option<&'a Listener> {
        self.find_precedence(|listener| {
            !skip_matchers.contains(&listener.matcher.kind())
                && listener.matcher.matches_input(input)
        })
    }

    pub fn first_match<C: ListenerComputeCtx>(
        &self,
        input: &ListenerInput,
        skip_matchers: &[ListenerMatcherKind],
        ctx: &mut C,
    ) -> Vec<ListenerAction> {
        self.matching_listener(input, skip_matchers)
            .cloned()
            .map(|listener| listener.compute_listener_input_with_ctx(input, ctx))
            .unwrap_or_default()
    }
}

/// Precedence-ordered read view over a higher-priority registry layered above a
/// lower-priority registry.
///
/// The event runtime uses this to dispatch against one combined precedence
/// order without materializing a separate merged registry on every overlay
/// rebuild.
#[derive(Clone, Copy)]
pub struct LayeredRegistryView<'a> {
    higher: &'a Registry,
    lower: &'a Registry,
}

impl<'a> LayeredRegistryView<'a> {
    pub fn new(higher: &'a Registry, lower: &'a Registry) -> Self {
        Self { higher, lower }
    }

    pub fn matching_listener(
        &self,
        input: &ListenerInput,
        skip_matchers: &[ListenerMatcherKind],
    ) -> Option<&'a Listener> {
        self.higher
            .view()
            .iter_precedence()
            .chain(self.lower.view().iter_precedence())
            .find(|listener| {
                !skip_matchers.contains(&listener.matcher.kind())
                    && listener.matcher.matches_input(input)
            })
    }

    pub fn first_match<C: ListenerComputeCtx>(
        &self,
        input: &ListenerInput,
        skip_matchers: &[ListenerMatcherKind],
        ctx: &mut C,
    ) -> Vec<ListenerAction> {
        self.matching_listener(input, skip_matchers)
            .cloned()
            .map(|listener| listener.compute_listener_input_with_ctx(input, ctx))
            .unwrap_or_default()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualKeyPhase {
    Armed,
    Repeating,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualKeyTracker {
    pub element_id: ElementId,
    pub region: PointerRegion,
    pub tap: VirtualKeyTapAction,
    pub hold: VirtualKeyHoldMode,
    pub hold_ms: u32,
    pub repeat_ms: u32,
    pub phase: VirtualKeyPhase,
}

/// Pointer-sensitive region backed by element interaction geometry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PointerRegion {
    visible: bool,
    local_shape: ShapeBounds,
    screen_to_local: Option<Affine2>,
    screen_bounds: Rect,
    clip_chain: Vec<InteractionClip>,
}

impl PointerRegion {
    fn for_state(state: &ResolvedNodeState) -> Self {
        let local_shape = state.self_shape;
        Self {
            visible: state.visible && state.interaction_inverse.is_some(),
            local_shape,
            screen_to_local: state.interaction_inverse,
            screen_bounds: state.interaction_transform.map_rect_aabb(local_shape.rect),
            clip_chain: state.interaction_clips.clone(),
        }
    }

    fn for_subregion(state: &ResolvedNodeState, bounds: Rect, radii: Option<CornerRadii>) -> Self {
        let local_shape = ShapeBounds {
            rect: bounds,
            radii: radii.map(|value| clamp_radii(bounds, value)),
        };
        Self {
            visible: state.visible && state.interaction_inverse.is_some(),
            local_shape,
            screen_to_local: state.interaction_inverse,
            screen_bounds: state.interaction_transform.map_rect_aabb(local_shape.rect),
            clip_chain: state.interaction_clips.clone(),
        }
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        if !self.visible || !self.screen_bounds.contains(x, y) {
            return false;
        }

        if self
            .clip_chain
            .iter()
            .any(|clip| !clip.contains_screen(x, y))
        {
            return false;
        }

        let Some(screen_to_local) = self.screen_to_local else {
            return false;
        };
        let local = screen_to_local.map_point(Point { x, y });
        point_hits_shape(self.local_shape, local.x, local.y)
    }
}

/// Precomputed scroll requests needed to reveal a focus target.
#[derive(Clone, Debug, PartialEq)]
pub struct FocusRevealScroll {
    pub element_id: ElementId,
    pub dx: f32,
    pub dy: f32,
}

#[derive(Clone, Debug, Default)]
struct FocusBuildState {
    focused_id: Option<ElementId>,
    first_focusable: Option<ElementId>,
    first_focusable_reveal_scrolls: Vec<FocusRevealScroll>,
    last_focusable: Option<ElementId>,
    last_focusable_reveal_scrolls: Vec<FocusRevealScroll>,
    by_id: HashMap<ElementId, ElementFocusMeta>,
}

#[derive(Clone, Debug, Default)]
struct ElementFocusMeta {
    is_currently_focused: bool,
    self_reveal_scrolls: Vec<FocusRevealScroll>,
    tab_next: Option<ElementId>,
    tab_next_reveal_scrolls: Vec<FocusRevealScroll>,
    tab_prev: Option<ElementId>,
    tab_prev_reveal_scrolls: Vec<FocusRevealScroll>,
}

#[derive(Clone, Debug)]
struct FocusEntry {
    element_id: ElementId,
    is_currently_focused: bool,
    self_reveal_scrolls: Vec<FocusRevealScroll>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KeyPressFollowup {
    ElixirEvent {
        element_id: ElementId,
        route: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyPressTracker {
    pub source_element_id: Option<ElementId>,
    pub key: CanonicalKey,
    pub mods: u8,
    pub match_mode: KeyBindingMatch,
    pub followups: Vec<KeyPressFollowup>,
}

#[derive(Default)]
pub(crate) struct RegistryBuildAcc {
    current_revision: u64,
    registry: Registry,
    text_inputs: HashMap<ElementId, TextInputState>,
    scrollbars: HashMap<(ElementId, ScrollbarAxis), ScrollbarNode>,
    focused_id: Option<ElementId>,
    focus_entries: Vec<FocusEntry>,
    focus_on_mount: Option<FocusOnMountTarget>,
}

impl RegistryBuildAcc {
    pub(crate) fn for_tree(tree: &ElementTree) -> Self {
        Self {
            current_revision: tree.revision(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollContext {
    id: ElementId,
    viewport: Rect,
    scroll_x: f32,
    scroll_y: f32,
    max_x: f32,
    max_y: f32,
}

#[derive(Clone, Debug)]
struct DeferredSubtree {
    element_id: ElementId,
    scroll_contexts: Vec<ScrollContext>,
    scene_ctx: crate::tree::scene::SceneContext,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwipeHandlers {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl SwipeHandlers {
    fn any(self) -> bool {
        self.up || self.down || self.left || self.right
    }

    fn any_for_axis(self, axis: GestureAxis) -> bool {
        match axis {
            GestureAxis::Horizontal => self.left || self.right,
            GestureAxis::Vertical => self.up || self.down,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureAxis {
    Horizontal,
    Vertical,
}

/// Drag tracker lifecycle state.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum DragTrackerState {
    #[default]
    Inactive,
    Candidate {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        origin_x: f32,
        origin_y: f32,
        swipe_handlers: SwipeHandlers,
    },
    Active {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        last_x: f32,
        last_y: f32,
        locked_axis: GestureAxis,
    },
}

/// Scrollbar drag tracker state used to rematerialize thumb-drag followups.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollbarDragTracker {
    pub element_id: ElementId,
    pub axis: ScrollbarAxis,
    pub track_start: f32,
    pub track_len: f32,
    pub thumb_len: f32,
    pub pointer_offset: f32,
    pub scroll_range: f32,
    pub current_scroll: f32,
    pub screen_to_local: Option<Affine2>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarPressSpec {
    axis: ScrollbarAxis,
    area: ScrollbarHitArea,
    track_start: f32,
    track_len: f32,
    thumb_start: f32,
    thumb_len: f32,
    scroll_offset: f32,
    scroll_range: f32,
    screen_to_local: Option<Affine2>,
}

/// Text-selection drag tracker state used to rematerialize cursor followups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextDragTracker {
    pub element_id: ElementId,
    pub matcher_kind: ListenerMatcherKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwipeTracker {
    pub element_id: ElementId,
    pub matcher_kind: ListenerMatcherKind,
    pub origin_x: f32,
    pub origin_y: f32,
    pub locked_axis: GestureAxis,
    pub handlers: SwipeHandlers,
}

/// Transient runtime interaction state used to rebuild overlay listeners.
///
/// This state does not come from the retained tree. It is produced by in-flight
/// interaction, such as click/press tracking, drag tracking, scrollbar thumb
/// dragging, and text selection dragging.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeOverlayState {
    pub click_press: Option<ClickPressTracker>,
    pub virtual_key: Option<VirtualKeyTracker>,
    pub key_presses: Vec<KeyPressTracker>,
    pub drag: DragTrackerState,
    pub swipe: Option<SwipeTracker>,
    pub scrollbar: Option<ScrollbarDragTracker>,
    pub text_drag: Option<TextDragTracker>,
}

fn emit_runtime_overlay_listeners(
    base: &Registry,
    runtime: &RuntimeOverlayState,
    out: &mut PrecedenceEmitter<'_>,
) {
    // Reordering these emissions changes runtime precedence. This function is
    // the overlay-side precedence table in code form.
    out.emit_opt(runtime_scroll_input_splitter_listener(base));
    out.emit(runtime_pointer_lifecycle_splitter_listener());
    out.emit_opt(runtime_drag_active_release_clear_listener(
        base,
        &runtime.drag,
    ));
    out.emit_opt(
        runtime
            .virtual_key
            .as_ref()
            .map(runtime_virtual_key_release_listener),
    );
    out.emit_all(runtime_key_press_release_listeners(
        base,
        &runtime.key_presses,
    ));
    out.emit_opt(
        runtime
            .swipe
            .as_ref()
            .and_then(|tracker| runtime_swipe_release_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .click_press
            .as_ref()
            .and_then(|tracker| runtime_click_press_release_listener(base, tracker)),
    );
    out.emit_opt(runtime_drag_candidate_release_anywhere_clear_listener(
        base,
        &runtime.drag,
    ));
    out.emit_opt(
        runtime
            .virtual_key
            .as_ref()
            .map(runtime_virtual_key_release_anywhere_clear_listener),
    );
    out.emit_opt(
        runtime
            .click_press
            .as_ref()
            .and_then(|tracker| runtime_click_press_release_anywhere_clear_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .virtual_key
            .as_ref()
            .and_then(runtime_virtual_key_leave_cancel_listener),
    );
    out.emit_opt(runtime_drag_active_scroll_move_listener(
        base,
        &runtime.drag,
    ));
    out.emit_opt(runtime_drag_candidate_threshold_listener(
        base,
        &runtime.drag,
    ));
    out.emit_opt(runtime_drag_window_blur_clear_listener(base, &runtime.drag));
    out.emit_opt(
        runtime
            .virtual_key
            .as_ref()
            .map(runtime_virtual_key_window_blur_clear_listener),
    );
    out.emit_opt(runtime_key_press_window_blur_clear_listener(
        &runtime.key_presses,
    ));
    out.emit_opt(
        runtime
            .swipe
            .as_ref()
            .and_then(|tracker| runtime_swipe_window_blur_clear_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .click_press
            .as_ref()
            .and_then(|tracker| runtime_click_press_window_blur_clear_listener(base, tracker)),
    );
    out.emit_opt(runtime_scrollbar_drag_release_listener(&runtime.scrollbar));
    out.emit_opt(runtime_scrollbar_drag_move_listener(&runtime.scrollbar));
    out.emit_opt(
        runtime
            .text_drag
            .as_ref()
            .and_then(|tracker| runtime_text_drag_release_clear_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .text_drag
            .as_ref()
            .and_then(|tracker| runtime_text_drag_cursor_move_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .text_drag
            .as_ref()
            .and_then(|tracker| runtime_text_drag_window_blur_clear_listener(base, tracker)),
    );
    out.emit_opt(runtime_drag_window_leave_clear_listener(
        base,
        &runtime.drag,
    ));
    out.emit_opt(
        runtime
            .virtual_key
            .as_ref()
            .map(runtime_virtual_key_window_leave_clear_listener),
    );
    out.emit_opt(
        runtime
            .swipe
            .as_ref()
            .and_then(|tracker| runtime_swipe_window_leave_clear_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .click_press
            .as_ref()
            .and_then(|tracker| runtime_click_press_window_leave_clear_listener(base, tracker)),
    );
    out.emit_opt(
        runtime
            .text_drag
            .as_ref()
            .and_then(|tracker| runtime_text_drag_window_leave_clear_listener(base, tracker)),
    );
}

/// Build runtime overlay listeners from transient runtime state.
#[cfg(test)]
pub fn runtime_listeners_for_overlay(
    base: &Registry,
    runtime: &RuntimeOverlayState,
) -> Vec<Listener> {
    build_runtime_overlay_registry(base, runtime).precedence_listeners()
}

/// Build a registry containing only runtime overlay listeners.
pub(crate) fn build_runtime_overlay_registry(
    base: &Registry,
    runtime: &RuntimeOverlayState,
) -> Registry {
    let mut registry = Registry::default();
    registry.in_precedence_order(|out| emit_runtime_overlay_listeners(base, runtime, out));
    registry
}

/// Compose a test-only combined registry from base listeners and runtime
/// overlay state.
#[cfg(test)]
pub fn compose_combined_registry(base: &Registry, runtime: &RuntimeOverlayState) -> Registry {
    let overlay_registry = build_runtime_overlay_registry(base, runtime);
    let mut registry = Registry::default();
    registry.extend_storage_from(base);
    registry.extend_storage_from(&overlay_registry);
    registry
}

fn runtime_drag_active_release_clear_listener(
    base: &Registry,
    drag: &DragTrackerState,
) -> Option<Listener> {
    let (element_id, matcher_kind) = match drag {
        DragTrackerState::Active {
            element_id,
            matcher_kind,
            ..
        } => (element_id, *matcher_kind),
        DragTrackerState::Inactive | DragTrackerState::Candidate { .. } => return None,
    };

    runtime_source_listener(base, element_id, matcher_kind)?;
    let actions = vec![
        ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
        ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
    ];
    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static { actions },
    })
}

fn runtime_drag_candidate_release_anywhere_clear_listener(
    base: &Registry,
    drag: &DragTrackerState,
) -> Option<Listener> {
    let (element_id, matcher_kind) = match drag {
        DragTrackerState::Candidate {
            element_id,
            matcher_kind,
            ..
        } => (element_id, *matcher_kind),
        DragTrackerState::Inactive | DragTrackerState::Active { .. } => return None,
    };

    runtime_source_listener(base, element_id, matcher_kind)?;
    let actions = vec![
        ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
        ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
    ];
    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static { actions },
    })
}

fn runtime_drag_window_blur_clear_listener(
    base: &Registry,
    drag: &DragTrackerState,
) -> Option<Listener> {
    let (element_id, matcher_kind) = match drag {
        DragTrackerState::Candidate {
            element_id,
            matcher_kind,
            ..
        }
        | DragTrackerState::Active {
            element_id,
            matcher_kind,
            ..
        } => (element_id, *matcher_kind),
        DragTrackerState::Inactive => return None,
    };

    runtime_source_listener(base, element_id, matcher_kind)?;
    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::Static {
            actions: vec![
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
            ],
        },
    })
}

fn runtime_drag_window_leave_clear_listener(
    base: &Registry,
    drag: &DragTrackerState,
) -> Option<Listener> {
    let (element_id, matcher_kind) = match drag {
        DragTrackerState::Candidate {
            element_id,
            matcher_kind,
            ..
        }
        | DragTrackerState::Active {
            element_id,
            matcher_kind,
            ..
        } => (element_id, *matcher_kind),
        DragTrackerState::Inactive => return None,
    };

    runtime_source_listener(base, element_id, matcher_kind)?;
    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::WindowCursorLeft,
        compute: ListenerCompute::Static {
            actions: vec![
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
            ],
        },
    })
}

fn runtime_drag_active_scroll_move_listener(
    base: &Registry,
    drag: &DragTrackerState,
) -> Option<Listener> {
    let (element_id, matcher_kind, last_x, last_y, locked_axis) = match drag {
        DragTrackerState::Active {
            element_id,
            matcher_kind,
            last_x,
            last_y,
            locked_axis,
        } => (element_id, *matcher_kind, *last_x, *last_y, *locked_axis),
        DragTrackerState::Inactive | DragTrackerState::Candidate { .. } => return None,
    };

    runtime_source_listener(base, element_id, matcher_kind)?;
    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::CursorPosAnywhere,
        compute: ListenerCompute::RedispatchScrollFromCursorMove {
            last_x,
            last_y,
            locked_axis,
        },
    })
}

fn base_has_directional_scroll_listener(base: &Registry) -> bool {
    base.view().any_precedence(|listener| {
        matches!(
            listener.matcher,
            ListenerMatcher::CursorScrollInsideDirection { .. }
        )
    })
}

fn runtime_scroll_input_splitter_listener(base: &Registry) -> Option<Listener> {
    base_has_directional_scroll_listener(base).then_some(Listener {
        element_id: None,
        matcher: ListenerMatcher::CursorScrollAny,
        compute: ListenerCompute::RedispatchScrollInput,
    })
}

fn runtime_pointer_lifecycle_splitter_listener() -> Listener {
    Listener {
        element_id: None,
        matcher: ListenerMatcher::RawPointerLifecycle,
        compute: ListenerCompute::RedispatchPointerLifecycle,
    }
}

fn runtime_drag_candidate_threshold_listener(
    base: &Registry,
    drag: &DragTrackerState,
) -> Option<Listener> {
    let (element_id, matcher_kind, origin_x, origin_y, swipe_handlers) = match drag {
        DragTrackerState::Candidate {
            element_id,
            matcher_kind,
            origin_x,
            origin_y,
            swipe_handlers,
        } => (
            element_id,
            *matcher_kind,
            *origin_x,
            *origin_y,
            *swipe_handlers,
        ),
        DragTrackerState::Inactive | DragTrackerState::Active { .. } => return None,
    };

    runtime_source_listener(base, element_id, matcher_kind)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::CursorPosDistanceFromPointExceeded {
            origin_x,
            origin_y,
            threshold: RUNTIME_DRAG_DEADZONE,
        },
        compute: ListenerCompute::PromoteDragTrackerFromCursorPos {
            element_id: element_id.clone(),
            matcher_kind,
            origin_x,
            origin_y,
            swipe_handlers,
        },
    })
}

fn runtime_scrollbar_drag_release_listener(
    scrollbar: &Option<ScrollbarDragTracker>,
) -> Option<Listener> {
    scrollbar.as_ref().map(|tracker| Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearScrollbarDrag,
            )],
        },
    })
}

fn runtime_scrollbar_drag_move_listener(
    scrollbar: &Option<ScrollbarDragTracker>,
) -> Option<Listener> {
    scrollbar.as_ref().map(|tracker| Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorPosAnywhere,
        compute: ListenerCompute::ScrollbarDragMove {
            tracker: tracker.clone(),
        },
    })
}

fn runtime_click_press_release_listener(
    base: &Registry,
    tracker: &ClickPressTracker,
) -> Option<Listener> {
    let source = runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;
    let region = runtime_press_region_from_source(source)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseInside { region },
        compute: ListenerCompute::ClickPressReleaseFollowupToBase {
            element_id: tracker.element_id.clone(),
            emit_click: tracker.emit_click,
            emit_press_pointer: tracker.emit_press_pointer,
        },
    })
}

fn runtime_swipe_release_listener(base: &Registry, tracker: &SwipeTracker) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::SwipeReleaseFollowupToBase {
            tracker: tracker.clone(),
        },
    })
}

fn runtime_swipe_window_blur_clear_listener(
    base: &Registry,
    tracker: &SwipeTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearSwipeTracker,
            )],
        },
    })
}

fn runtime_swipe_window_leave_clear_listener(
    base: &Registry,
    tracker: &SwipeTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowCursorLeft,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearSwipeTracker,
            )],
        },
    })
}

fn runtime_click_press_release_anywhere_clear_listener(
    base: &Registry,
    tracker: &ClickPressTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static {
            actions: vec![
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
            ],
        },
    })
}

fn runtime_click_press_window_blur_clear_listener(
    base: &Registry,
    tracker: &ClickPressTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::Static {
            actions: vec![
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
            ],
        },
    })
}

fn runtime_click_press_window_leave_clear_listener(
    base: &Registry,
    tracker: &ClickPressTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowCursorLeft,
        compute: ListenerCompute::Static {
            actions: vec![
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
            ],
        },
    })
}

pub(crate) fn synthetic_input_sequence_for_virtual_key_tap(
    tap: &VirtualKeyTapAction,
) -> Vec<InputEvent> {
    match tap {
        VirtualKeyTapAction::Text(text) => vec![InputEvent::TextCommit {
            text: text.clone(),
            mods: 0,
        }],
        VirtualKeyTapAction::Key { key, mods } => vec![
            InputEvent::Key {
                key: *key,
                action: ACTION_PRESS,
                mods: *mods,
            },
            InputEvent::Key {
                key: *key,
                action: ACTION_RELEASE,
                mods: *mods,
            },
        ],
        VirtualKeyTapAction::TextAndKey { text, key, mods } => vec![
            InputEvent::Key {
                key: *key,
                action: ACTION_PRESS,
                mods: *mods,
            },
            InputEvent::TextCommit {
                text: text.clone(),
                mods: *mods,
            },
            InputEvent::Key {
                key: *key,
                action: ACTION_RELEASE,
                mods: *mods,
            },
        ],
    }
}

fn runtime_virtual_key_release_listener(tracker: &VirtualKeyTracker) -> Listener {
    let mut actions = Vec::new();

    if tracker.phase == VirtualKeyPhase::Armed {
        actions.push(ListenerAction::SyntheticInput(
            synthetic_input_sequence_for_virtual_key_tap(&tracker.tap),
        ));
    }

    actions.push(ListenerAction::RuntimeChange(
        RuntimeChange::ClearVirtualKeyTracker,
    ));

    Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseInside {
            region: tracker.region.clone(),
        },
        compute: ListenerCompute::DispatchBaseThenStatic { actions },
    }
}

fn runtime_virtual_key_release_anywhere_clear_listener(tracker: &VirtualKeyTracker) -> Listener {
    Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearVirtualKeyTracker,
            )],
        },
    }
}

fn runtime_virtual_key_leave_cancel_listener(tracker: &VirtualKeyTracker) -> Option<Listener> {
    matches!(
        tracker.phase,
        VirtualKeyPhase::Armed | VirtualKeyPhase::Repeating
    )
    .then(|| Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorLocationLeaveBoundary {
            region: tracker.region.clone(),
        },
        compute: ListenerCompute::DispatchBaseThenStatic {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::CancelVirtualKeyTracker,
            )],
        },
    })
}

fn runtime_virtual_key_window_blur_clear_listener(tracker: &VirtualKeyTracker) -> Listener {
    Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::DispatchBaseThenStatic {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearVirtualKeyTracker,
            )],
        },
    }
}

fn runtime_virtual_key_window_leave_clear_listener(tracker: &VirtualKeyTracker) -> Listener {
    Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowCursorLeft,
        compute: ListenerCompute::DispatchBaseThenStatic {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearVirtualKeyTracker,
            )],
        },
    }
}

fn runtime_key_press_release_listeners(
    base: &Registry,
    trackers: &[KeyPressTracker],
) -> Vec<Listener> {
    trackers
        .iter()
        .filter(|tracker| base_has_key_press_source(base, tracker))
        .fold(
            Vec::<(CanonicalKey, Vec<KeyPressTracker>)>::new(),
            |mut acc, tracker| {
                if let Some((_, grouped)) = acc.iter_mut().find(|(key, _)| *key == tracker.key) {
                    grouped.push(tracker.clone());
                } else {
                    acc.push((tracker.key, vec![tracker.clone()]));
                }

                acc
            },
        )
        .into_iter()
        .map(|(key, trackers)| Listener {
            element_id: trackers
                .iter()
                .find_map(|tracker| tracker.source_element_id.clone()),
            matcher: ListenerMatcher::KeyReleaseTracked { key },
            compute: ListenerCompute::KeyPressReleaseFollowupToBase { key, trackers },
        })
        .collect()
}

fn runtime_key_press_window_blur_clear_listener(trackers: &[KeyPressTracker]) -> Option<Listener> {
    (!trackers.is_empty()).then_some(Listener {
        element_id: trackers
            .iter()
            .find_map(|tracker| tracker.source_element_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::DispatchBaseThenStatic {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearKeyPressTrackers,
            )],
        },
    })
}

fn runtime_text_drag_release_clear_listener(
    base: &Registry,
    tracker: &TextDragTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearTextDragTracker,
            )],
        },
    })
}

fn runtime_text_drag_cursor_move_listener(
    base: &Registry,
    tracker: &TextDragTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::CursorPosAnywhere,
        compute: ListenerCompute::StaticWithTextInputCursorRuntime {
            actions: Vec::new(),
            element_id: tracker.element_id.clone(),
            extend_selection: true,
        },
    })
}

fn runtime_text_drag_window_blur_clear_listener(
    base: &Registry,
    tracker: &TextDragTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearTextDragTracker,
            )],
        },
    })
}

fn runtime_text_drag_window_leave_clear_listener(
    base: &Registry,
    tracker: &TextDragTracker,
) -> Option<Listener> {
    runtime_source_listener(base, &tracker.element_id, tracker.matcher_kind)?;

    Some(Listener {
        element_id: Some(tracker.element_id.clone()),
        matcher: ListenerMatcher::WindowCursorLeft,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::RuntimeChange(
                RuntimeChange::ClearTextDragTracker,
            )],
        },
    })
}

#[derive(Clone, Debug)]
pub(crate) struct PointerDragBootstrap {
    element_id: ElementId,
    matcher_kind: ListenerMatcherKind,
    swipe_handlers: SwipeHandlers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextInputKeyEditKind {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

fn runtime_source_listener<'a>(
    base: &'a Registry,
    element_id: &ElementId,
    matcher_kind: ListenerMatcherKind,
) -> Option<&'a Listener> {
    base.view().find_precedence(|listener| {
        listener.element_id.as_ref() == Some(element_id) && listener.matcher.kind() == matcher_kind
    })
}

fn runtime_press_region_from_source(source: &Listener) -> Option<PointerRegion> {
    match &source.matcher {
        ListenerMatcher::CursorButtonLeftPressInside { region } => Some(region.clone()),
        _ => None,
    }
}

pub trait ListenerComputeCtx {
    fn focused_id(&self) -> Option<&ElementId> {
        None
    }

    fn hover_owner(&self) -> Option<&ElementId> {
        None
    }

    fn text_input_state(&self, _element_id: &ElementId) -> Option<TextInputState> {
        None
    }

    fn clipboard_text(&mut self, _target: ClipboardTarget) -> Option<String> {
        None
    }

    fn take_text_commit_suppression(&mut self, _element_id: &ElementId) -> bool {
        false
    }

    fn dispatch_base(&mut self, _input: &ListenerInput) -> Vec<ListenerAction> {
        Vec::new()
    }

    fn dispatch_effective_skip(
        &mut self,
        _input: &ListenerInput,
        _skip_matchers: &[ListenerMatcherKind],
    ) -> Vec<ListenerAction> {
        Vec::new()
    }

    fn base_first_match_listener(
        &self,
        _input: &ListenerInput,
        _skip_matchers: &[ListenerMatcherKind],
    ) -> Option<Listener> {
        None
    }

    fn base_source_listener(
        &self,
        _element_id: &ElementId,
        _matcher_kind: ListenerMatcherKind,
    ) -> Option<Listener> {
        None
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct NoopListenerComputeCtx;

#[cfg(test)]
impl ListenerComputeCtx for NoopListenerComputeCtx {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    XNeg,
    XPos,
    YNeg,
    YPos,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScrollbarHoverCompute {
    element_id: ElementId,
    current_axis: Option<ScrollbarAxis>,
    x_region: Option<PointerRegion>,
    y_region: Option<PointerRegion>,
}

#[derive(Clone, Debug)]
pub enum ListenerInput {
    Raw(InputEvent),
    PointerLeave {
        x: f32,
        y: f32,
        window_left: bool,
    },
    PointerEnter {
        x: f32,
        y: f32,
    },
    ScrollDirection {
        direction: ScrollDirection,
        dx: f32,
        dy: f32,
        x: f32,
        y: f32,
    },
}

impl ListenerInput {
    fn raw(&self) -> Option<&InputEvent> {
        match self {
            ListenerInput::Raw(input) => Some(input),
            _ => None,
        }
    }
}

/// Declarative listener record.
///
/// A listener is intentionally minimal:
/// - `element_id` carries source identity for runtime followup rebinding
/// - `matcher` decides whether this listener applies to the current input
/// - `compute` produces final sink actions from the matched input
#[derive(Clone, Debug)]
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
    #[cfg(test)]
    pub fn compute_actions(&self, input: &InputEvent) -> Vec<ListenerAction> {
        let mut ctx = NoopListenerComputeCtx;
        self.compute.compute(input, &mut ctx)
    }

    #[cfg(test)]
    pub fn compute_listener_input_actions(&self, input: &ListenerInput) -> Vec<ListenerAction> {
        let mut ctx = NoopListenerComputeCtx;
        self.compute.compute_input(input, &mut ctx)
    }

    /// Compute output actions for this listener from a matched input event using runtime state.
    #[cfg(test)]
    pub fn compute_actions_with_ctx<C: ListenerComputeCtx>(
        &self,
        input: &InputEvent,
        ctx: &mut C,
    ) -> Vec<ListenerAction> {
        self.compute.compute(input, ctx)
    }

    pub fn compute_listener_input_with_ctx<C: ListenerComputeCtx>(
        &self,
        input: &ListenerInput,
        ctx: &mut C,
    ) -> Vec<ListenerAction> {
        self.compute.compute_input(input, ctx)
    }
}

/// Matcher shape for listener evaluation.
///
/// The first iteration includes concrete pointer/hover variants needed by
/// `listeners_for_element`.
#[derive(Clone, Debug)]
pub enum ListenerMatcher {
    /// Match left-button press when pointer is inside `region`.
    CursorButtonLeftPressInside { region: PointerRegion },
    /// Match left-button release when pointer is inside `region`.
    CursorButtonLeftReleaseInside { region: PointerRegion },
    /// Match any left-button release regardless of pointer position.
    CursorButtonLeftReleaseAnywhere,
    /// Match cursor position updates inside `region`.
    CursorPosInside { region: PointerRegion },
    /// Match semantic pointer-enter dispatch inside `region`.
    PointerEnterInside { region: PointerRegion },
    /// Match any cursor position update regardless of pointer position.
    CursorPosAnywhere,
    /// Match cursor movement once distance from `origin` exceeds `threshold`.
    CursorPosDistanceFromPointExceeded {
        origin_x: f32,
        origin_y: f32,
        threshold: f32,
    },
    /// Match any scroll input regardless of position.
    CursorScrollAny,
    /// Match raw cursor position / left release / window leave for lifecycle splitting.
    RawPointerLifecycle,
    /// Match scroll wheel updates inside `region` for one direction only.
    CursorScrollInsideDirection {
        region: PointerRegion,
        direction: ScrollDirection,
    },
    /// Match Enter key press when Ctrl/Alt/Meta are not held.
    KeyEnterPressNoCtrlAltMeta,
    /// Match Left key press when Ctrl/Alt/Meta are not held.
    KeyLeftPressNoCtrlAltMeta,
    /// Match Right key press when Ctrl/Alt/Meta are not held.
    KeyRightPressNoCtrlAltMeta,
    /// Match Home key press when Ctrl/Alt/Meta are not held.
    KeyHomePressNoCtrlAltMeta,
    /// Match End key press when Ctrl/Alt/Meta are not held.
    KeyEndPressNoCtrlAltMeta,
    /// Match Up key press when Ctrl/Alt/Meta are not held.
    KeyUpPressNoCtrlAltMeta,
    /// Match Down key press when Ctrl/Alt/Meta are not held.
    KeyDownPressNoCtrlAltMeta,
    /// Match Tab key press when Shift/Ctrl/Alt/Meta are not held.
    KeyTabPressNoShiftCtrlAltMeta,
    /// Match Shift+Tab key press when Ctrl/Alt/Meta are not held.
    KeyShiftTabPressNoCtrlAltMeta,
    /// Match A key press when Ctrl or Meta is held.
    KeyAPressCtrlOrMeta,
    /// Match C key press when Ctrl or Meta is held.
    KeyCPressCtrlOrMeta,
    /// Match X key press when Ctrl or Meta is held.
    KeyXPressCtrlOrMeta,
    /// Match V key press when Ctrl or Meta is held.
    KeyVPressCtrlOrMeta,
    /// Match Backspace key press.
    KeyBackspacePress,
    /// Match Delete key press.
    KeyDeletePress,
    /// Match a focused user key-down binding.
    KeyDownBinding {
        key: CanonicalKey,
        mods: u8,
        match_mode: KeyBindingMatch,
    },
    /// Match a focused user key-up binding.
    KeyUpBinding {
        key: CanonicalKey,
        mods: u8,
        match_mode: KeyBindingMatch,
    },
    /// Match a key release for a tracked key regardless of modifiers.
    KeyReleaseTracked { key: CanonicalKey },
    /// Match text commit events when Ctrl/Meta are not held.
    TextCommitNoCtrlMeta,
    /// Match text preedit events.
    TextPreeditAny,
    /// Match text preedit clear events.
    TextPreeditClear,
    /// Match IME delete-surrounding requests.
    TextDeleteSurroundingAny,
    /// Match middle-button press when pointer is inside `region`.
    CursorButtonMiddlePressInside { region: PointerRegion },
    /// Match window focus lost notifications.
    WindowBlurred,
    /// Match window-level cursor-leave notifications.
    WindowCursorLeft,
    /// Match window resize notifications.
    WindowResized,
    /// Match leaving `region` via cursor or left-button location changes, or window-leave.
    CursorLocationLeaveBoundary { region: PointerRegion },
    /// Source-only hover leave listener for the current hover owner.
    HoverLeaveCurrentOwner,
}

/// Stable matcher identity for source lookup.
///
/// Equality is by enum variant only; payload is intentionally ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListenerMatcherKind {
    CursorButtonLeftPressInside,
    CursorButtonLeftReleaseInside,
    CursorButtonLeftReleaseAnywhere,
    CursorButtonMiddlePressInside,
    CursorPosInside,
    PointerEnterInside,
    CursorPosAnywhere,
    CursorPosDistanceFromPointExceeded,
    CursorScrollAny,
    RawPointerLifecycle,
    CursorScrollInsideDirection,
    KeyEnterPressNoCtrlAltMeta,
    KeyLeftPressNoCtrlAltMeta,
    KeyRightPressNoCtrlAltMeta,
    KeyHomePressNoCtrlAltMeta,
    KeyEndPressNoCtrlAltMeta,
    KeyUpPressNoCtrlAltMeta,
    KeyDownPressNoCtrlAltMeta,
    KeyTabPressNoShiftCtrlAltMeta,
    KeyShiftTabPressNoCtrlAltMeta,
    KeyAPressCtrlOrMeta,
    KeyCPressCtrlOrMeta,
    KeyXPressCtrlOrMeta,
    KeyVPressCtrlOrMeta,
    KeyBackspacePress,
    KeyDeletePress,
    KeyDownBinding,
    KeyUpBinding,
    KeyReleaseTracked,
    TextCommitNoCtrlMeta,
    TextPreeditAny,
    TextPreeditClear,
    TextDeleteSurroundingAny,
    WindowBlurred,
    WindowCursorLeft,
    WindowResized,
    CursorLocationLeaveBoundary,
    HoverLeaveCurrentOwner,
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
            ListenerMatcher::CursorButtonLeftReleaseAnywhere => {
                ListenerMatcherKind::CursorButtonLeftReleaseAnywhere
            }
            ListenerMatcher::CursorButtonMiddlePressInside { .. } => {
                ListenerMatcherKind::CursorButtonMiddlePressInside
            }
            ListenerMatcher::CursorPosInside { .. } => ListenerMatcherKind::CursorPosInside,
            ListenerMatcher::PointerEnterInside { .. } => ListenerMatcherKind::PointerEnterInside,
            ListenerMatcher::CursorPosAnywhere => ListenerMatcherKind::CursorPosAnywhere,
            ListenerMatcher::CursorPosDistanceFromPointExceeded { .. } => {
                ListenerMatcherKind::CursorPosDistanceFromPointExceeded
            }
            ListenerMatcher::CursorScrollAny => ListenerMatcherKind::CursorScrollAny,
            ListenerMatcher::RawPointerLifecycle => ListenerMatcherKind::RawPointerLifecycle,
            ListenerMatcher::CursorScrollInsideDirection { .. } => {
                ListenerMatcherKind::CursorScrollInsideDirection
            }
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyEnterPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyLeftPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyLeftPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyRightPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyRightPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyHomePressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyHomePressNoCtrlAltMeta
            }
            ListenerMatcher::KeyEndPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyEndPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyUpPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyUpPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyDownPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyDownPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta => {
                ListenerMatcherKind::KeyTabPressNoShiftCtrlAltMeta
            }
            ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta => {
                ListenerMatcherKind::KeyShiftTabPressNoCtrlAltMeta
            }
            ListenerMatcher::KeyAPressCtrlOrMeta => ListenerMatcherKind::KeyAPressCtrlOrMeta,
            ListenerMatcher::KeyCPressCtrlOrMeta => ListenerMatcherKind::KeyCPressCtrlOrMeta,
            ListenerMatcher::KeyXPressCtrlOrMeta => ListenerMatcherKind::KeyXPressCtrlOrMeta,
            ListenerMatcher::KeyVPressCtrlOrMeta => ListenerMatcherKind::KeyVPressCtrlOrMeta,
            ListenerMatcher::KeyBackspacePress => ListenerMatcherKind::KeyBackspacePress,
            ListenerMatcher::KeyDeletePress => ListenerMatcherKind::KeyDeletePress,
            ListenerMatcher::KeyDownBinding { .. } => ListenerMatcherKind::KeyDownBinding,
            ListenerMatcher::KeyUpBinding { .. } => ListenerMatcherKind::KeyUpBinding,
            ListenerMatcher::KeyReleaseTracked { .. } => ListenerMatcherKind::KeyReleaseTracked,
            ListenerMatcher::TextCommitNoCtrlMeta => ListenerMatcherKind::TextCommitNoCtrlMeta,
            ListenerMatcher::TextPreeditAny => ListenerMatcherKind::TextPreeditAny,
            ListenerMatcher::TextPreeditClear => ListenerMatcherKind::TextPreeditClear,
            ListenerMatcher::TextDeleteSurroundingAny => {
                ListenerMatcherKind::TextDeleteSurroundingAny
            }
            ListenerMatcher::WindowBlurred => ListenerMatcherKind::WindowBlurred,
            ListenerMatcher::WindowCursorLeft => ListenerMatcherKind::WindowCursorLeft,
            ListenerMatcher::WindowResized => ListenerMatcherKind::WindowResized,
            ListenerMatcher::CursorLocationLeaveBoundary { .. } => {
                ListenerMatcherKind::CursorLocationLeaveBoundary
            }
            ListenerMatcher::HoverLeaveCurrentOwner => ListenerMatcherKind::HoverLeaveCurrentOwner,
        }
    }

    /// Returns whether this matcher accepts the given input event.
    #[cfg(test)]
    pub fn matches(&self, input: &InputEvent) -> bool {
        self.matches_input(&ListenerInput::Raw(input.clone()))
    }

    pub fn matches_input(&self, input: &ListenerInput) -> bool {
        match self {
            ListenerMatcher::CursorButtonLeftPressInside { region } => {
                matches!(
                    input.raw(),
                    Some(InputEvent::CursorButton {
                        button,
                        action,
                        x,
                        y,
                        ..
                    }) if button == "left" && *action == ACTION_PRESS && region.contains(*x, *y)
                )
            }
            ListenerMatcher::CursorButtonLeftReleaseInside { region } => {
                matches!(
                    input.raw(),
                    Some(InputEvent::CursorButton {
                        button,
                        action,
                        x,
                        y,
                        ..
                    }) if button == "left" && *action == ACTION_RELEASE && region.contains(*x, *y)
                )
            }
            ListenerMatcher::CursorButtonLeftReleaseAnywhere => matches!(
                input.raw(),
                Some(InputEvent::CursorButton {
                    button,
                    action,
                    ..
                }) if button == "left" && *action == ACTION_RELEASE
            ),
            ListenerMatcher::CursorButtonMiddlePressInside { region } => {
                matches!(
                    input.raw(),
                    Some(InputEvent::CursorButton {
                        button,
                        action,
                        x,
                        y,
                        ..
                    }) if button == "middle" && *action == ACTION_PRESS && region.contains(*x, *y)
                )
            }
            ListenerMatcher::CursorPosInside { region } => matches!(
                input.raw(),
                Some(InputEvent::CursorPos { x, y }) if region.contains(*x, *y)
            ),
            ListenerMatcher::PointerEnterInside { region } => matches!(
                input,
                ListenerInput::PointerEnter { x, y } if region.contains(*x, *y)
            ),
            ListenerMatcher::CursorPosAnywhere => {
                matches!(input.raw(), Some(InputEvent::CursorPos { .. }))
            }
            ListenerMatcher::CursorPosDistanceFromPointExceeded {
                origin_x,
                origin_y,
                threshold,
            } => matches!(input.raw(), Some(InputEvent::CursorPos { x, y }) if {
                let dx = *x - *origin_x;
                let dy = *y - *origin_y;
                let threshold_sq = *threshold * *threshold;
                dx * dx + dy * dy >= threshold_sq
            }),
            ListenerMatcher::CursorScrollAny => matches!(
                input.raw(),
                Some(InputEvent::CursorScroll { .. } | InputEvent::CursorScrollLines { .. })
            ),
            ListenerMatcher::RawPointerLifecycle => {
                matches!(
                    input.raw(),
                    Some(InputEvent::CursorPos { .. })
                        | Some(InputEvent::CursorEntered { entered: false })
                ) || matches!(
                    input.raw(),
                    Some(InputEvent::CursorButton {
                            button,
                            action: ACTION_RELEASE,
                            ..
                        }) if button == "left"
                )
            }
            ListenerMatcher::CursorScrollInsideDirection { region, direction } => matches!(
                input,
                ListenerInput::ScrollDirection {
                    direction: matched,
                    x,
                    y,
                    ..
                } if matched == direction && region.contains(*x, *y)
            ),
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::Enter
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyLeftPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::ArrowLeft
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyRightPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::ArrowRight
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyHomePressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::Home
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyEndPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::End
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyUpPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::ArrowUp
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyDownPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::ArrowDown
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::Tab
                        && (*mods & (MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::Tab
                        && (*mods & MOD_SHIFT) != 0
                        && (*mods & (MOD_CTRL | MOD_ALT | MOD_META)) == 0
            ),
            ListenerMatcher::KeyAPressCtrlOrMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::A
                        && (*mods & (MOD_CTRL | MOD_META)) != 0
            ),
            ListenerMatcher::KeyCPressCtrlOrMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::C
                        && (*mods & (MOD_CTRL | MOD_META)) != 0
            ),
            ListenerMatcher::KeyXPressCtrlOrMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::X
                        && (*mods & (MOD_CTRL | MOD_META)) != 0
            ),
            ListenerMatcher::KeyVPressCtrlOrMeta => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, mods })
                    if *action == ACTION_PRESS
                        && *key == CanonicalKey::V
                        && (*mods & (MOD_CTRL | MOD_META)) != 0
            ),
            ListenerMatcher::KeyBackspacePress => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, .. })
                    if *action == ACTION_PRESS && *key == CanonicalKey::Backspace
            ),
            ListenerMatcher::KeyDeletePress => matches!(
                input.raw(),
                Some(InputEvent::Key { key, action, .. })
                    if *action == ACTION_PRESS && *key == CanonicalKey::Delete
            ),
            ListenerMatcher::KeyDownBinding {
                key,
                mods,
                match_mode,
            } => matches!(
                input.raw(),
                Some(InputEvent::Key {
                    key: input_key,
                    action,
                    mods: input_mods,
                }) if *action == ACTION_PRESS
                    && *input_key == *key
                    && key_modifiers_match(*input_mods, *mods, *match_mode)
            ),
            ListenerMatcher::KeyUpBinding {
                key,
                mods,
                match_mode,
            } => matches!(
                input.raw(),
                Some(InputEvent::Key {
                    key: input_key,
                    action,
                    mods: input_mods,
                }) if *action == ACTION_RELEASE
                    && *input_key == *key
                    && key_modifiers_match(*input_mods, *mods, *match_mode)
            ),
            ListenerMatcher::KeyReleaseTracked { key } => matches!(
                input.raw(),
                Some(InputEvent::Key {
                    key: input_key,
                    action,
                    ..
                }) if *action == ACTION_RELEASE && *input_key == *key
            ),
            ListenerMatcher::TextCommitNoCtrlMeta => matches!(
                input.raw(),
                Some(InputEvent::TextCommit { mods, .. }) if (*mods & (MOD_CTRL | MOD_META)) == 0
            ),
            ListenerMatcher::TextPreeditAny => {
                matches!(input.raw(), Some(InputEvent::TextPreedit { .. }))
            }
            ListenerMatcher::TextPreeditClear => {
                matches!(input.raw(), Some(InputEvent::TextPreeditClear))
            }
            ListenerMatcher::TextDeleteSurroundingAny => {
                matches!(input.raw(), Some(InputEvent::DeleteSurrounding { .. }))
            }
            ListenerMatcher::WindowBlurred => {
                matches!(input.raw(), Some(InputEvent::Focused { focused }) if !*focused)
            }
            ListenerMatcher::WindowCursorLeft => {
                matches!(input.raw(), Some(InputEvent::CursorEntered { entered }) if !*entered)
            }
            ListenerMatcher::WindowResized => {
                matches!(input.raw(), Some(InputEvent::Resized { .. }))
            }
            ListenerMatcher::CursorLocationLeaveBoundary { region } => match input {
                ListenerInput::PointerLeave { x, y, window_left } => {
                    *window_left || !region.contains(*x, *y)
                }
                _ => false,
            },
            ListenerMatcher::HoverLeaveCurrentOwner => false,
        }
    }
}

fn key_modifiers_match(actual: u8, required: u8, match_mode: KeyBindingMatch) -> bool {
    match match_mode {
        KeyBindingMatch::Exact => actual == required,
        KeyBindingMatch::All => actual & required == required,
    }
}

fn key_press_followup_actions(tracker: &KeyPressTracker) -> Vec<ListenerAction> {
    tracker
        .followups
        .iter()
        .map(|followup| match followup {
            KeyPressFollowup::ElixirEvent { element_id, route } => {
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element_id.clone(),
                    kind: ElementEventKind::KeyPress,
                    payload: Some(route.clone()),
                })
            }
        })
        .collect()
}

fn listener_compute_contains_key_press_tracker(
    compute: &ListenerCompute,
    tracker: &KeyPressTracker,
) -> bool {
    let actions = match compute {
        ListenerCompute::Static { actions }
        | ListenerCompute::DispatchBaseThenStatic { actions } => Some(actions.as_slice()),
        _ => None,
    };

    actions
        .into_iter()
        .flatten()
        .any(|action| matches!(action, ListenerAction::RuntimeChange(RuntimeChange::StartKeyPressTracker { tracker: existing }) if existing == tracker))
}

pub(crate) fn base_has_key_press_source(base: &Registry, tracker: &KeyPressTracker) -> bool {
    base.view().any_precedence(|listener| {
        listener_compute_contains_key_press_tracker(&listener.compute, tracker)
    })
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
    /// Synthetic raw input re-injected through the normal runtime pipeline.
    SyntheticInput(Vec<InputEvent>),
    /// Request a cursor icon update from the event runtime.
    SetCursor(CursorIcon),
    /// Event forwarded to Elixir-side consumers.
    ElixirEvent(ElixirEvent),
    /// Clipboard write performed by the runtime after dispatch.
    ClipboardWrite {
        target: ClipboardTarget,
        text: String,
    },
    /// Semantic action expanded into final outputs during listener compute.
    Semantic(SemanticAction),
}

/// Semantic listener outputs that need live runtime context to expand.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticAction {
    /// Apply a precomputed focus transition.
    FocusTo {
        next: Option<ElementId>,
        reveal_scrolls: Vec<FocusRevealScroll>,
    },
    /// Request a text-input command operation.
    TextInputCommand {
        element_id: ElementId,
        request: TextInputCommandRequest,
    },
    /// Request a text-input edit operation.
    TextInputEdit {
        element_id: ElementId,
        request: TextInputEditRequest,
    },
    /// Request a text-input cursor operation.
    TextInputCursor {
        element_id: ElementId,
        x: f32,
        y: f32,
        extend_selection: bool,
    },
    /// Request a text-input preedit operation.
    TextInputPreedit {
        element_id: ElementId,
        request: TextInputPreeditRequest,
    },
}

/// Transient event-runtime state changes.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeChange {
    /// Begin click/press followup tracking for pointer interaction.
    StartClickPressTracker {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        emit_click: bool,
        emit_press_pointer: bool,
    },
    /// Begin virtual-key press tracking.
    StartVirtualKeyTracker { tracker: VirtualKeyTracker },
    /// Begin completed key-press followup tracking.
    StartKeyPressTracker { tracker: KeyPressTracker },
    /// Begin drag threshold tracking.
    StartDragTracker {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        origin_x: f32,
        origin_y: f32,
        swipe_handlers: SwipeHandlers,
    },
    /// Promote drag threshold tracking to an active drag followup.
    PromoteDragTracker {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        last_x: f32,
        last_y: f32,
        locked_axis: GestureAxis,
    },
    /// Begin text-selection drag tracking.
    StartTextDragTracker {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
    },
    /// End drag tracking on pointer release.
    ClearDragTracker,
    /// Update active drag pointer position after a cursor move.
    UpdateDragTrackerPointer { last_x: f32, last_y: f32 },
    /// Drop click/press release followup tracking.
    ClearClickPressTracker,
    /// Begin swipe followup tracking.
    StartSwipeTracker { tracker: SwipeTracker },
    /// Drop swipe followup tracking.
    ClearSwipeTracker,
    /// Cancel the active virtual-key gesture until release.
    CancelVirtualKeyTracker,
    /// Drop virtual-key tracking entirely.
    ClearVirtualKeyTracker,
    /// Drop completed key-press tracking for one key.
    ClearKeyPressTrackersForKey { key: CanonicalKey },
    /// Drop all completed key-press tracking.
    ClearKeyPressTrackers,
    /// Begin scrollbar thumb-drag tracking.
    StartScrollbarDrag { tracker: ScrollbarDragTracker },
    /// Update current scrollbar drag scroll position.
    UpdateScrollbarDragCurrentScroll { current_scroll: f32 },
    /// End scrollbar thumb-drag tracking.
    ClearScrollbarDrag,
    /// End text-selection drag tracking.
    ClearTextDragTracker,
    /// Mirror full text input state into runtime state.
    SetTextInputState {
        element_id: ElementId,
        state: TextInputState,
    },
    /// Suppress the next keydown-derived text commit for one text input.
    ArmTextCommitSuppression {
        element_id: ElementId,
        key: CanonicalKey,
    },
    /// Track an expected content value coming back from an Elixir tree patch.
    ExpectTextInputPatchValue {
        element_id: ElementId,
        content: String,
    },
    /// Update the runtime's current hover owner.
    SetHoverOwner { element_id: Option<ElementId> },
}

impl RuntimeChange {
    pub fn requires_registry_recompose(&self) -> bool {
        matches!(
            self,
            RuntimeChange::StartClickPressTracker { .. }
                | RuntimeChange::StartVirtualKeyTracker { .. }
                | RuntimeChange::StartKeyPressTracker { .. }
                | RuntimeChange::StartDragTracker { .. }
                | RuntimeChange::PromoteDragTracker { .. }
                | RuntimeChange::StartTextDragTracker { .. }
                | RuntimeChange::ClearDragTracker
                | RuntimeChange::UpdateDragTrackerPointer { .. }
                | RuntimeChange::ClearClickPressTracker
                | RuntimeChange::StartSwipeTracker { .. }
                | RuntimeChange::ClearSwipeTracker
                | RuntimeChange::CancelVirtualKeyTracker
                | RuntimeChange::ClearVirtualKeyTracker
                | RuntimeChange::ClearKeyPressTrackersForKey { .. }
                | RuntimeChange::ClearKeyPressTrackers
                | RuntimeChange::StartScrollbarDrag { .. }
                | RuntimeChange::UpdateScrollbarDragCurrentScroll { .. }
                | RuntimeChange::ClearScrollbarDrag
                | RuntimeChange::ClearTextDragTracker
        )
    }
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
#[derive(Clone, Debug)]
pub enum ListenerCompute {
    /// Fixed action list independent of input payload.
    Static { actions: Vec<ListenerAction> },
    /// Dispatch base listeners, then append fixed actions.
    DispatchBaseThenStatic { actions: Vec<ListenerAction> },
    /// Fixed actions plus left-press runtime bootstrap from matching input.
    StaticWithLeftPressRuntimeAugment {
        actions: Vec<ListenerAction>,
        pointer_drag: Option<PointerDragBootstrap>,
        text_cursor_element_id: Option<ElementId>,
        text_drag: Option<TextDragTracker>,
    },
    /// Fixed actions plus a text-input cursor action derived from matched input.
    StaticWithTextInputCursorRuntime {
        actions: Vec<ListenerAction>,
        element_id: ElementId,
        extend_selection: bool,
    },
    /// Emit pointer click/press followups, then redispatch raw release into the base registry.
    ClickPressReleaseFollowupToBase {
        element_id: ElementId,
        emit_click: bool,
        emit_press_pointer: bool,
    },
    /// Redispatch base release listeners, then emit a completed swipe gesture.
    SwipeReleaseFollowupToBase { tracker: SwipeTracker },
    /// Redispatch base key-up listeners, optionally emit completed key-press actions, then clear tracking.
    KeyPressReleaseFollowupToBase {
        key: CanonicalKey,
        trackers: Vec<KeyPressTracker>,
    },
    /// Promote drag threshold tracking using cursor-move payload.
    PromoteDragTrackerFromCursorPos {
        element_id: ElementId,
        matcher_kind: ListenerMatcherKind,
        origin_x: f32,
        origin_y: f32,
        swipe_handlers: SwipeHandlers,
    },
    /// Split one physical scroll input into directional redispatches.
    RedispatchScrollInput,
    /// Split one raw pointer lifecycle input into synthetic leave/raw/enter passes.
    RedispatchPointerLifecycle,
    /// Build one `TreeMsg::ScrollRequest` from a directional scroll input.
    ScrollTreeMsgFromCursorScrollDirection {
        element_id: ElementId,
        direction: ScrollDirection,
    },
    /// Build `TreeMsg::Resize` from a resize input.
    WindowResizeToTree,
    /// Emit a fixed key-scroll tree request.
    KeyScrollToTree {
        element_id: ElementId,
        dx: f32,
        dy: f32,
    },
    /// Redispatch drag movement as a synthetic axis-locked scroll input and update pointer position.
    RedispatchScrollFromCursorMove {
        last_x: f32,
        last_y: f32,
        locked_axis: GestureAxis,
    },
    /// Start scrollbar drag tracking from a thumb or track press.
    ScrollbarPressToRuntime {
        element_id: ElementId,
        spec: ScrollbarPressSpec,
    },
    /// Emit scrollbar drag tree updates and update current scroll position.
    ScrollbarDragMove { tracker: ScrollbarDragTracker },
    /// Build a key-driven text cursor edit action from live text-input state.
    TextInputKeyEditToRuntime {
        element_id: ElementId,
        kind: TextInputKeyEditKind,
    },
    /// Build a fixed text-edit action only when it changes content/state.
    TextInputEditToRuntimeMaybe {
        element_id: ElementId,
        request: TextInputEditRequest,
    },
    /// Build a text-commit insertion action.
    TextCommitToRuntime { element_id: ElementId },
    /// Build a text preedit action from matched IME input.
    TextInputPreeditToRuntime { element_id: ElementId },
    /// Build an IME delete-surrounding edit action.
    TextDeleteSurroundingToRuntime { element_id: ElementId },
    /// Raw cursor position actions plus element-local scrollbar hover transitions.
    RawCursorPosWithScrollbarHover {
        actions: Vec<ListenerAction>,
        scrollbar_hover: Option<ScrollbarHoverCompute>,
    },
    /// Pointer-leave actions plus scrollbar hover clear transitions.
    PointerLeaveWithScrollbarHover {
        actions: Vec<ListenerAction>,
        scrollbar_hover: Option<ScrollbarHoverCompute>,
    },
}

impl ListenerCompute {
    /// Compute final sink actions from the matched input.
    #[cfg(test)]
    pub fn compute<C: ListenerComputeCtx>(
        &self,
        input: &InputEvent,
        ctx: &mut C,
    ) -> Vec<ListenerAction> {
        self.compute_input(&ListenerInput::Raw(input.clone()), ctx)
    }

    pub fn compute_input<C: ListenerComputeCtx>(
        &self,
        input: &ListenerInput,
        ctx: &mut C,
    ) -> Vec<ListenerAction> {
        let actions = match self {
            ListenerCompute::Static { actions } => actions.clone(),
            ListenerCompute::DispatchBaseThenStatic { actions } => ctx
                .dispatch_base(input)
                .into_iter()
                .chain(actions.iter().cloned())
                .collect(),
            ListenerCompute::StaticWithLeftPressRuntimeAugment {
                actions,
                pointer_drag,
                text_cursor_element_id,
                text_drag,
            } => match input.raw() {
                Some(InputEvent::CursorButton {
                    button,
                    action,
                    x,
                    y,
                    mods,
                    ..
                }) if button == "left" && *action == ACTION_PRESS => actions
                    .iter()
                    .cloned()
                    .chain(pointer_drag.as_ref().map(|pointer_drag| {
                        ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker {
                            element_id: pointer_drag.element_id.clone(),
                            matcher_kind: pointer_drag.matcher_kind,
                            origin_x: *x,
                            origin_y: *y,
                            swipe_handlers: pointer_drag.swipe_handlers,
                        })
                    }))
                    .chain(text_cursor_element_id.as_ref().map(|element_id| {
                        ListenerAction::Semantic(SemanticAction::TextInputCursor {
                            element_id: element_id.clone(),
                            x: *x,
                            y: *y,
                            extend_selection: *mods & MOD_SHIFT != 0,
                        })
                    }))
                    .chain(text_drag.as_ref().map(|text_drag| {
                        ListenerAction::RuntimeChange(RuntimeChange::StartTextDragTracker {
                            element_id: text_drag.element_id.clone(),
                            matcher_kind: text_drag.matcher_kind,
                        })
                    }))
                    .collect(),
                _ => actions.clone(),
            },
            ListenerCompute::StaticWithTextInputCursorRuntime {
                actions,
                element_id,
                extend_selection,
            } => actions
                .iter()
                .cloned()
                .chain(text_cursor_action_from_input(
                    input.raw(),
                    element_id,
                    *extend_selection,
                ))
                .collect(),
            ListenerCompute::ClickPressReleaseFollowupToBase {
                element_id,
                emit_click,
                emit_press_pointer,
            } => match input.raw() {
                Some(InputEvent::CursorButton { button, action, .. })
                    if button == "left" && *action == ACTION_RELEASE =>
                {
                    ctx.dispatch_base(input)
                        .into_iter()
                        .chain((*emit_click).then(|| {
                            ListenerAction::ElixirEvent(ElixirEvent {
                                element_id: element_id.clone(),
                                kind: ElementEventKind::Click,
                                payload: None,
                            })
                        }))
                        .chain((*emit_press_pointer).then(|| {
                            ListenerAction::ElixirEvent(ElixirEvent {
                                element_id: element_id.clone(),
                                kind: ElementEventKind::Press,
                                payload: None,
                            })
                        }))
                        .chain([
                            ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                            ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                        ])
                        .collect()
                }
                _ => Vec::new(),
            },
            ListenerCompute::SwipeReleaseFollowupToBase { tracker } => match input.raw() {
                Some(InputEvent::CursorButton {
                    button,
                    action,
                    x,
                    y,
                    ..
                }) if button == "left" && *action == ACTION_RELEASE => ctx
                    .dispatch_base(input)
                    .into_iter()
                    .chain(swipe_event_from_release(tracker, *x, *y).map(|kind| {
                        ListenerAction::ElixirEvent(ElixirEvent {
                            element_id: tracker.element_id.clone(),
                            kind,
                            payload: None,
                        })
                    }))
                    .chain([ListenerAction::RuntimeChange(
                        RuntimeChange::ClearSwipeTracker,
                    )])
                    .collect(),
                _ => Vec::new(),
            },
            ListenerCompute::KeyPressReleaseFollowupToBase { key, trackers } => match input.raw() {
                Some(InputEvent::Key {
                    key: input_key,
                    action,
                    mods,
                }) if *action == ACTION_RELEASE && *input_key == *key => ctx
                    .dispatch_base(input)
                    .into_iter()
                    .chain(trackers.iter().flat_map(|tracker| {
                        if key_modifiers_match(*mods, tracker.mods, tracker.match_mode) {
                            key_press_followup_actions(tracker)
                        } else {
                            Vec::new()
                        }
                    }))
                    .chain([ListenerAction::RuntimeChange(
                        RuntimeChange::ClearKeyPressTrackersForKey { key: *key },
                    )])
                    .collect(),
                _ => Vec::new(),
            },
            ListenerCompute::PromoteDragTrackerFromCursorPos {
                element_id,
                matcher_kind,
                origin_x,
                origin_y,
                swipe_handlers,
            } => match input {
                ListenerInput::Raw(InputEvent::CursorPos { x, y }) => {
                    let dx = *x - *origin_x;
                    let dy = *y - *origin_y;

                    let Some(locked_axis) = gesture_axis_intent_from_delta(dx, dy) else {
                        return Vec::new();
                    };

                    if drag_scroll_can_activate_on_axis(locked_axis, dx, dy, *x, *y, ctx) {
                        vec![
                            ListenerAction::RuntimeChange(RuntimeChange::PromoteDragTracker {
                                element_id: element_id.clone(),
                                matcher_kind: *matcher_kind,
                                last_x: *x,
                                last_y: *y,
                                locked_axis,
                            }),
                            ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                        ]
                    } else if swipe_handlers.any_for_axis(locked_axis) {
                        vec![
                            ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                            ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                            ListenerAction::RuntimeChange(RuntimeChange::StartSwipeTracker {
                                tracker: SwipeTracker {
                                    element_id: element_id.clone(),
                                    matcher_kind: *matcher_kind,
                                    origin_x: *origin_x,
                                    origin_y: *origin_y,
                                    locked_axis,
                                    handlers: *swipe_handlers,
                                },
                            }),
                        ]
                    } else {
                        vec![ListenerAction::RuntimeChange(
                            RuntimeChange::ClearDragTracker,
                        )]
                    }
                }
                _ => Vec::new(),
            },
            ListenerCompute::RedispatchScrollInput => match input.raw() {
                Some(input) => redispatch_scroll_components_from_input(input, ctx),
                None => Vec::new(),
            },
            ListenerCompute::RedispatchPointerLifecycle => match input.raw() {
                Some(input) => redispatch_pointer_lifecycle_from_input(input, ctx),
                None => Vec::new(),
            },
            ListenerCompute::ScrollTreeMsgFromCursorScrollDirection {
                element_id,
                direction,
            } => scroll_tree_actions_from_directional_input(input, element_id, *direction),
            ListenerCompute::WindowResizeToTree => match input.raw() {
                Some(InputEvent::Resized {
                    width,
                    height,
                    scale_factor,
                }) => vec![ListenerAction::TreeMsg(TreeMsg::Resize {
                    width: *width as f32,
                    height: *height as f32,
                    scale: *scale_factor,
                })],
                _ => Vec::new(),
            },
            ListenerCompute::KeyScrollToTree { element_id, dx, dy } => {
                vec![ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                    element_id: element_id.clone(),
                    dx: *dx,
                    dy: *dy,
                })]
            }
            ListenerCompute::RedispatchScrollFromCursorMove {
                last_x,
                last_y,
                locked_axis,
            } => match input.raw() {
                Some(input) => {
                    drag_scroll_actions_from_input(input, *last_x, *last_y, *locked_axis, ctx)
                }
                None => Vec::new(),
            },
            ListenerCompute::ScrollbarPressToRuntime { element_id, spec } => match input.raw() {
                Some(input) => scrollbar_press_actions_from_input(input, element_id, *spec),
                None => Vec::new(),
            },
            ListenerCompute::ScrollbarDragMove { tracker } => match input.raw() {
                Some(input) => scrollbar_drag_move_actions_from_input(input, tracker),
                None => Vec::new(),
            },
            ListenerCompute::TextInputKeyEditToRuntime { element_id, kind } => ctx
                .text_input_state(element_id)
                .and_then(|snapshot| text_key_edit_request(&snapshot, *kind, input.raw()?))
                .map(|request| {
                    vec![ListenerAction::Semantic(SemanticAction::TextInputEdit {
                        element_id: element_id.clone(),
                        request,
                    })]
                })
                .unwrap_or_default(),
            ListenerCompute::TextInputEditToRuntimeMaybe {
                element_id,
                request,
            } => ctx
                .text_input_state(element_id)
                .and_then(|snapshot| {
                    text_ops::apply_edit_request(
                        &snapshot.content,
                        snapshot.cursor,
                        snapshot.selection_anchor,
                        request,
                    )
                })
                .map(|_| {
                    vec![ListenerAction::Semantic(SemanticAction::TextInputEdit {
                        element_id: element_id.clone(),
                        request: request.clone(),
                    })]
                })
                .unwrap_or_default(),
            ListenerCompute::TextCommitToRuntime { element_id } => match input {
                ListenerInput::Raw(InputEvent::TextCommit { text, mods })
                    if (*mods & (MOD_CTRL | MOD_META)) == 0 =>
                {
                    match ctx.text_input_state(element_id) {
                        None => Vec::new(),
                        Some(snapshot) if ctx.take_text_commit_suppression(element_id) => {
                            Vec::new()
                        }
                        Some(snapshot) => {
                            let filtered = sanitize_text_input_text(text, snapshot.multiline);
                            if filtered.is_empty() {
                                Vec::new()
                            } else {
                                vec![ListenerAction::Semantic(SemanticAction::TextInputEdit {
                                    element_id: element_id.clone(),
                                    request: TextInputEditRequest::Insert(filtered),
                                })]
                            }
                        }
                    }
                }
                _ => Vec::new(),
            },
            ListenerCompute::TextInputPreeditToRuntime { element_id } => {
                text_preedit_action_from_input(input.raw(), element_id)
                    .into_iter()
                    .collect()
            }
            ListenerCompute::TextDeleteSurroundingToRuntime { element_id } => {
                text_delete_surrounding_action_from_input(input.raw(), element_id)
                    .into_iter()
                    .collect()
            }
            ListenerCompute::RawCursorPosWithScrollbarHover {
                actions,
                scrollbar_hover,
            } => match input.raw() {
                Some(InputEvent::CursorPos { x, y }) => actions
                    .iter()
                    .cloned()
                    .chain(scrollbar_hover.iter().flat_map(|scrollbar_hover| {
                        scrollbar_hover_delta_actions(scrollbar_hover, Some((*x, *y)))
                    }))
                    .collect(),
                _ => Vec::new(),
            },
            ListenerCompute::PointerLeaveWithScrollbarHover {
                actions,
                scrollbar_hover,
            } => match input {
                ListenerInput::PointerLeave { .. } => actions
                    .iter()
                    .cloned()
                    .chain(scrollbar_hover.iter().flat_map(|scrollbar_hover| {
                        scrollbar_hover_delta_actions(scrollbar_hover, None)
                    }))
                    .collect(),
                _ => Vec::new(),
            },
        };

        resolve_listener_actions(actions, ctx)
    }
}

fn text_cursor_action_from_input(
    input: Option<&InputEvent>,
    element_id: &ElementId,
    extend_selection: bool,
) -> Option<ListenerAction> {
    let action = match input? {
        InputEvent::CursorButton {
            action, x, y, mods, ..
        } if *action == ACTION_PRESS => ListenerAction::Semantic(SemanticAction::TextInputCursor {
            element_id: element_id.clone(),
            x: *x,
            y: *y,
            extend_selection: extend_selection || (*mods & MOD_SHIFT != 0),
        }),
        InputEvent::CursorPos { x, y } => {
            ListenerAction::Semantic(SemanticAction::TextInputCursor {
                element_id: element_id.clone(),
                x: *x,
                y: *y,
                extend_selection,
            })
        }
        _ => return None,
    };

    Some(action)
}

fn text_key_edit_request(
    snapshot: &TextInputState,
    kind: TextInputKeyEditKind,
    input: &InputEvent,
) -> Option<TextInputEditRequest> {
    let InputEvent::Key { mods, .. } = input else {
        return None;
    };

    let extend_selection = *mods & MOD_SHIFT != 0;
    let content_len = text_ops::text_char_len(&snapshot.content);
    let has_selection = snapshot
        .selection_anchor
        .is_some_and(|anchor| anchor != snapshot.cursor);

    match kind {
        TextInputKeyEditKind::Left => {
            let can_move = if extend_selection {
                snapshot.cursor > 0
            } else {
                snapshot.cursor > 0 || has_selection
            };
            can_move.then_some(TextInputEditRequest::MoveLeft { extend_selection })
        }
        TextInputKeyEditKind::Right => {
            let can_move = if extend_selection {
                snapshot.cursor < content_len
            } else {
                snapshot.cursor < content_len || has_selection
            };
            can_move.then_some(TextInputEditRequest::MoveRight { extend_selection })
        }
        TextInputKeyEditKind::Up => (snapshot.multiline
            && snapshot.move_vertical_target(-1) != snapshot.cursor)
            .then_some(TextInputEditRequest::MoveUp { extend_selection }),
        TextInputKeyEditKind::Down => (snapshot.multiline
            && snapshot.move_vertical_target(1) != snapshot.cursor)
            .then_some(TextInputEditRequest::MoveDown { extend_selection }),
        TextInputKeyEditKind::Home => {
            let target = snapshot.move_home_target();
            let can_move = if extend_selection {
                target != snapshot.cursor
            } else {
                target != snapshot.cursor || has_selection
            };
            can_move.then_some(TextInputEditRequest::MoveHome { extend_selection })
        }
        TextInputKeyEditKind::End => {
            let target = snapshot.move_end_target();
            let can_move = if extend_selection {
                target != snapshot.cursor
            } else {
                target != snapshot.cursor || has_selection
            };
            can_move.then_some(TextInputEditRequest::MoveEnd { extend_selection })
        }
    }
}

fn text_preedit_action_from_input(
    input: Option<&InputEvent>,
    element_id: &ElementId,
) -> Option<ListenerAction> {
    let request = match input? {
        InputEvent::TextPreedit { text, cursor } => {
            if text.is_empty() {
                TextInputPreeditRequest::Clear
            } else {
                TextInputPreeditRequest::Set {
                    text: text.clone(),
                    cursor: *cursor,
                }
            }
        }
        InputEvent::TextPreeditClear => TextInputPreeditRequest::Clear,
        _ => return None,
    };

    Some(ListenerAction::Semantic(SemanticAction::TextInputPreedit {
        element_id: element_id.clone(),
        request,
    }))
}

fn text_delete_surrounding_action_from_input(
    input: Option<&InputEvent>,
    element_id: &ElementId,
) -> Option<ListenerAction> {
    let (before_length, after_length) = match input? {
        InputEvent::DeleteSurrounding {
            before_length,
            after_length,
        } => (*before_length, *after_length),
        _ => return None,
    };

    Some(ListenerAction::Semantic(SemanticAction::TextInputEdit {
        element_id: element_id.clone(),
        request: TextInputEditRequest::DeleteSurrounding {
            before_length,
            after_length,
        },
    }))
}

fn scroll_component(
    direction: ScrollDirection,
    delta: f32,
    x: f32,
    y: f32,
) -> Option<ListenerInput> {
    (delta.abs() > f32::EPSILON).then_some(match direction {
        ScrollDirection::XNeg | ScrollDirection::XPos => ListenerInput::ScrollDirection {
            direction,
            dx: delta,
            dy: 0.0,
            x,
            y,
        },
        ScrollDirection::YNeg | ScrollDirection::YPos => ListenerInput::ScrollDirection {
            direction,
            dx: 0.0,
            dy: delta,
            x,
            y,
        },
    })
}

fn split_scroll_delta_components(dx: f32, dy: f32, x: f32, y: f32) -> Vec<ListenerInput> {
    [
        scroll_component(
            if dx < 0.0 {
                ScrollDirection::XNeg
            } else {
                ScrollDirection::XPos
            },
            dx,
            x,
            y,
        ),
        scroll_component(
            if dy < 0.0 {
                ScrollDirection::YNeg
            } else {
                ScrollDirection::YPos
            },
            dy,
            x,
            y,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn split_scroll_components(input: &InputEvent) -> Vec<ListenerInput> {
    match input {
        InputEvent::CursorScroll { dx, dy, x, y }
        | InputEvent::CursorScrollLines { dx, dy, x, y } => {
            split_scroll_delta_components(*dx, *dy, *x, *y)
        }
        _ => Vec::new(),
    }
}

fn scroll_component_for_axis(
    axis: GestureAxis,
    dx: f32,
    dy: f32,
    x: f32,
    y: f32,
) -> Option<ListenerInput> {
    match axis {
        GestureAxis::Horizontal => scroll_component(
            if dx < 0.0 {
                ScrollDirection::XNeg
            } else {
                ScrollDirection::XPos
            },
            dx,
            x,
            y,
        ),
        GestureAxis::Vertical => scroll_component(
            if dy < 0.0 {
                ScrollDirection::YNeg
            } else {
                ScrollDirection::YPos
            },
            dy,
            x,
            y,
        ),
    }
}

fn redispatch_scroll_components_from_input<C: ListenerComputeCtx>(
    input: &InputEvent,
    ctx: &mut C,
) -> Vec<ListenerAction> {
    split_scroll_components(input)
        .into_iter()
        .flat_map(|component| ctx.dispatch_base(&component))
        .collect()
}

fn redispatch_scroll_component_for_axis<C: ListenerComputeCtx>(
    axis: GestureAxis,
    dx: f32,
    dy: f32,
    x: f32,
    y: f32,
    ctx: &mut C,
) -> Vec<ListenerAction> {
    scroll_component_for_axis(axis, dx, dy, x, y)
        .into_iter()
        .flat_map(|component| ctx.dispatch_base(&component))
        .collect()
}

fn drag_scroll_can_activate_on_axis<C: ListenerComputeCtx>(
    axis: GestureAxis,
    dx: f32,
    dy: f32,
    x: f32,
    y: f32,
    ctx: &mut C,
) -> bool {
    scroll_component_for_axis(axis, dx, dy, x, y)
        .is_some_and(|component| !ctx.dispatch_base(&component).is_empty())
}

fn redispatch_pointer_lifecycle_from_input<C: ListenerComputeCtx>(
    input: &InputEvent,
    ctx: &mut C,
) -> Vec<ListenerAction> {
    let skip = [ListenerMatcherKind::RawPointerLifecycle];

    fn dispatch_sequence<C: ListenerComputeCtx>(
        ctx: &mut C,
        skip: &[ListenerMatcherKind],
        leave_input: ListenerInput,
        raw_input: ListenerInput,
        enter_input: Option<ListenerInput>,
    ) -> Vec<ListenerAction> {
        let current_hover_id = ctx.hover_owner().cloned();
        let next_hover_listener = enter_input
            .as_ref()
            .and_then(|input| ctx.base_first_match_listener(input, &[]));
        let next_hover_id = next_hover_listener
            .as_ref()
            .and_then(|listener| listener.element_id.clone());
        let hover_owner_changed = current_hover_id != next_hover_id;

        let mut out = ctx.dispatch_effective_skip(&leave_input, skip);

        if hover_owner_changed
            && let Some(current_hover_id) = current_hover_id.as_ref()
            && let Some(listener) = ctx.base_source_listener(
                current_hover_id,
                ListenerMatcherKind::HoverLeaveCurrentOwner,
            )
        {
            out.extend(listener.compute_listener_input_with_ctx(&leave_input, ctx));
        }

        out.extend(ctx.dispatch_effective_skip(&raw_input, skip));

        if hover_owner_changed {
            if let (Some(enter_input), Some(listener)) = (enter_input.as_ref(), next_hover_listener)
            {
                out.extend(listener.compute_listener_input_with_ctx(enter_input, ctx));
            }

            out.push(ListenerAction::RuntimeChange(
                RuntimeChange::SetHoverOwner {
                    element_id: next_hover_id,
                },
            ));
        }

        out
    }

    match input {
        InputEvent::CursorPos { x, y } => dispatch_sequence(
            ctx,
            &skip,
            ListenerInput::PointerLeave {
                x: *x,
                y: *y,
                window_left: false,
            },
            ListenerInput::Raw(input.clone()),
            Some(ListenerInput::PointerEnter { x: *x, y: *y }),
        ),
        InputEvent::CursorButton {
            button,
            action,
            x,
            y,
            ..
        } if button == "left" && *action == ACTION_RELEASE => dispatch_sequence(
            ctx,
            &skip,
            ListenerInput::PointerLeave {
                x: *x,
                y: *y,
                window_left: false,
            },
            ListenerInput::Raw(input.clone()),
            Some(ListenerInput::PointerEnter { x: *x, y: *y }),
        ),
        InputEvent::CursorEntered { entered } if !*entered => dispatch_sequence(
            ctx,
            &skip,
            ListenerInput::PointerLeave {
                x: 0.0,
                y: 0.0,
                window_left: true,
            },
            ListenerInput::Raw(input.clone()),
            None,
        ),
        _ => Vec::new(),
    }
}

fn scroll_tree_actions_from_directional_input(
    input: &ListenerInput,
    element_id: &ElementId,
    direction: ScrollDirection,
) -> Vec<ListenerAction> {
    match input {
        ListenerInput::ScrollDirection {
            direction: matched_direction,
            dx,
            dy,
            ..
        } if *matched_direction == direction => {
            vec![ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                element_id: element_id.clone(),
                dx: *dx,
                dy: *dy,
            })]
        }
        _ => Vec::new(),
    }
}

fn scrollbar_hover_compute_for_element(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<ScrollbarHoverCompute> {
    let state = state?;
    let (scrollbar_x, scrollbar_y) = live_scrollbar_nodes_for_element(element, state);
    if scrollbar_x.is_none() && scrollbar_y.is_none() {
        return None;
    }

    let current_axis = match element.attrs.scrollbar_hover_axis {
        Some(crate::tree::attrs::ScrollbarHoverAxis::X) => Some(ScrollbarAxis::X),
        Some(crate::tree::attrs::ScrollbarHoverAxis::Y) => Some(ScrollbarAxis::Y),
        None => None,
    };

    Some(ScrollbarHoverCompute {
        element_id: element.id.clone(),
        current_axis,
        x_region: scrollbar_x
            .and_then(|scrollbar| pointer_region_for_subregion(state, scrollbar.thumb_rect)),
        y_region: scrollbar_y
            .and_then(|scrollbar| pointer_region_for_subregion(state, scrollbar.thumb_rect)),
    })
}

fn active_scrollbar_hover_compute_for_element(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<ScrollbarHoverCompute> {
    scrollbar_hover_compute_for_element(element, state)
        .filter(|compute| compute.current_axis.is_some())
}

fn scrollbar_hover_axis_at_position(
    compute: &ScrollbarHoverCompute,
    x: f32,
    y: f32,
) -> Option<ScrollbarAxis> {
    if compute
        .x_region
        .as_ref()
        .is_some_and(|region| region.contains(x, y))
    {
        Some(ScrollbarAxis::X)
    } else if compute
        .y_region
        .as_ref()
        .is_some_and(|region| region.contains(x, y))
    {
        Some(ScrollbarAxis::Y)
    } else {
        None
    }
}

fn scrollbar_hover_delta_actions(
    compute: &ScrollbarHoverCompute,
    position: Option<(f32, f32)>,
) -> Vec<ListenerAction> {
    let next_axis = position.and_then(|(x, y)| scrollbar_hover_axis_at_position(compute, x, y));
    if next_axis == compute.current_axis {
        return Vec::new();
    }

    fn scrollbar_hover_axis_action(
        element_id: &ElementId,
        axis: Option<ScrollbarAxis>,
        hovered: bool,
    ) -> Option<ListenerAction> {
        match axis {
            Some(ScrollbarAxis::X) => Some(ListenerAction::TreeMsg(TreeMsg::SetScrollbarXHover {
                element_id: element_id.clone(),
                hovered,
            })),
            Some(ScrollbarAxis::Y) => Some(ListenerAction::TreeMsg(TreeMsg::SetScrollbarYHover {
                element_id: element_id.clone(),
                hovered,
            })),
            None => None,
        }
    }

    [
        scrollbar_hover_axis_action(&compute.element_id, compute.current_axis, false),
        scrollbar_hover_axis_action(&compute.element_id, next_axis, true),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn drag_scroll_actions_from_input<C: ListenerComputeCtx>(
    input: &InputEvent,
    last_x: f32,
    last_y: f32,
    locked_axis: GestureAxis,
    ctx: &mut C,
) -> Vec<ListenerAction> {
    let InputEvent::CursorPos { x, y } = input else {
        return Vec::new();
    };

    let dx = *x - last_x;
    let dy = *y - last_y;

    let moved = dx != 0.0 || dy != 0.0;
    let actions = if moved {
        redispatch_scroll_component_for_axis(locked_axis, dx, dy, *x, *y, ctx)
    } else {
        Vec::new()
    };

    actions
        .into_iter()
        .chain(moved.then_some(ListenerAction::RuntimeChange(
            RuntimeChange::UpdateDragTrackerPointer {
                last_x: *x,
                last_y: *y,
            },
        )))
        .collect()
}

fn gesture_axis_intent_from_delta(dx: f32, dy: f32) -> Option<GestureAxis> {
    let abs_x = dx.abs();
    let abs_y = dy.abs();

    if abs_x >= abs_y * GESTURE_AXIS_DOMINANCE_RATIO && abs_x - abs_y >= GESTURE_AXIS_MIN_LEAD {
        Some(GestureAxis::Horizontal)
    } else if abs_y >= abs_x * GESTURE_AXIS_DOMINANCE_RATIO
        && abs_y - abs_x >= GESTURE_AXIS_MIN_LEAD
    {
        Some(GestureAxis::Vertical)
    } else {
        None
    }
}

fn swipe_event_from_release(tracker: &SwipeTracker, x: f32, y: f32) -> Option<ElementEventKind> {
    let delta = match tracker.locked_axis {
        GestureAxis::Horizontal => x - tracker.origin_x,
        GestureAxis::Vertical => y - tracker.origin_y,
    };

    if delta.abs() < RUNTIME_DRAG_DEADZONE {
        None
    } else {
        match tracker.locked_axis {
            GestureAxis::Horizontal => {
                if delta > 0.0 {
                    tracker
                        .handlers
                        .right
                        .then_some(ElementEventKind::SwipeRight)
                } else {
                    tracker.handlers.left.then_some(ElementEventKind::SwipeLeft)
                }
            }
            GestureAxis::Vertical => {
                if delta > 0.0 {
                    tracker.handlers.down.then_some(ElementEventKind::SwipeDown)
                } else {
                    tracker.handlers.up.then_some(ElementEventKind::SwipeUp)
                }
            }
        }
    }
}

fn resolve_listener_actions<C: ListenerComputeCtx>(
    actions: Vec<ListenerAction>,
    ctx: &mut C,
) -> Vec<ListenerAction> {
    let mut state = SemanticComputeState::new(ctx);
    actions.into_iter().fold(Vec::new(), |mut out, action| {
        state.append_resolved_action(action, &mut out);
        out
    })
}

enum FocusTransition {
    Blur(ElementId),
    Focus(ElementId),
}

struct SemanticComputeState<'a, C> {
    ctx: &'a mut C,
    focused_id: Option<ElementId>,
    snapshots: HashMap<ElementId, Option<TextInputState>>,
    clipboard: HashMap<ClipboardTarget, Option<String>>,
}

impl<'a, C: ListenerComputeCtx> SemanticComputeState<'a, C> {
    fn new(ctx: &'a mut C) -> Self {
        Self {
            focused_id: ctx.focused_id().cloned(),
            ctx,
            snapshots: HashMap::new(),
            clipboard: HashMap::new(),
        }
    }

    fn snapshot(&mut self, element_id: &ElementId) -> Option<&mut TextInputState> {
        self.snapshots
            .entry(element_id.clone())
            .or_insert_with(|| self.ctx.text_input_state(element_id))
            .as_mut()
    }

    fn clipboard_text(&mut self, target: ClipboardTarget) -> Option<String> {
        if let Some(text) = self.clipboard.get(&target) {
            return text.clone();
        }

        let text = self.ctx.clipboard_text(target);
        self.clipboard.insert(target, text.clone());
        text
    }

    fn set_clipboard(&mut self, target: ClipboardTarget, text: String) {
        self.clipboard
            .insert(target, if text.is_empty() { None } else { Some(text) });
    }

    fn note_final_action(&mut self, action: &ListenerAction) {
        match action {
            ListenerAction::ClipboardWrite { target, text } => {
                self.set_clipboard(*target, text.clone());
            }
            ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id, active }) => {
                if *active {
                    self.focused_id = Some(element_id.clone());
                } else if self.focused_id.as_ref() == Some(element_id) {
                    self.focused_id = None;
                }
            }
            ListenerAction::TreeMsg(TreeMsg::SetTextInputContent {
                element_id,
                content,
            }) => {
                if let Some(snapshot) = self.snapshot(element_id) {
                    set_text_input_content_snapshot(snapshot, content.clone());
                }
            }
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime {
                element_id,
                focused,
                cursor,
                selection_anchor,
                preedit,
                preedit_cursor,
            }) => {
                if let Some(snapshot) = self.snapshot(element_id) {
                    set_text_input_runtime_snapshot(
                        snapshot,
                        *focused,
                        *cursor,
                        *selection_anchor,
                        preedit.clone(),
                        *preedit_cursor,
                    );
                }
            }
            _ => {}
        }
    }

    fn append_resolved_action(&mut self, action: ListenerAction, out: &mut Vec<ListenerAction>) {
        match action {
            ListenerAction::Semantic(semantic) => {
                out.extend(self.resolve_semantic_action(semantic))
            }
            other => {
                self.note_final_action(&other);
                out.push(other);
            }
        }
    }

    fn finish_with_primary_selection_write(
        &mut self,
        runtime_actions: Vec<ListenerAction>,
        primary: Option<String>,
    ) -> Vec<ListenerAction> {
        runtime_actions
            .into_iter()
            .chain(primary.filter(|text| !text.is_empty()).map(|text| {
                self.set_clipboard(ClipboardTarget::Primary, text.clone());
                ListenerAction::ClipboardWrite {
                    target: ClipboardTarget::Primary,
                    text,
                }
            }))
            .collect()
    }

    fn append_focus_transition(
        &mut self,
        transition: FocusTransition,
        out: &mut Vec<ListenerAction>,
    ) {
        match transition {
            FocusTransition::Blur(prev_id) => {
                out.extend([
                    ListenerAction::ElixirEvent(ElixirEvent {
                        element_id: prev_id.clone(),
                        kind: ElementEventKind::Blur,
                        payload: None,
                    }),
                    ListenerAction::TreeMsg(TreeMsg::SetFocusedActive {
                        element_id: prev_id.clone(),
                        active: false,
                    }),
                ]);

                if let Some(snapshot) = self.snapshot(&prev_id) {
                    let cursor = snapshot.cursor;
                    set_text_input_runtime_snapshot(
                        snapshot,
                        false,
                        Some(cursor),
                        None,
                        None,
                        None,
                    );
                    out.extend(text_runtime_actions(&prev_id, snapshot));
                }
            }
            FocusTransition::Focus(next_id) => {
                out.extend([
                    ListenerAction::ElixirEvent(ElixirEvent {
                        element_id: next_id.clone(),
                        kind: ElementEventKind::Focus,
                        payload: None,
                    }),
                    ListenerAction::TreeMsg(TreeMsg::SetFocusedActive {
                        element_id: next_id.clone(),
                        active: true,
                    }),
                ]);

                if let Some(snapshot) = self.snapshot(&next_id) {
                    let cursor = snapshot.cursor;
                    let selection_anchor = snapshot.selection_anchor;
                    let preedit = snapshot.preedit.clone();
                    let preedit_cursor = snapshot.preedit_cursor;
                    set_text_input_runtime_snapshot(
                        snapshot,
                        true,
                        Some(cursor),
                        selection_anchor,
                        preedit,
                        preedit_cursor,
                    );
                    out.extend(text_runtime_actions(&next_id, snapshot));
                }
            }
        }
    }

    fn resolve_semantic_action(&mut self, action: SemanticAction) -> Vec<ListenerAction> {
        match action {
            SemanticAction::FocusTo {
                next,
                reveal_scrolls,
            } => self.resolve_focus_to(next, reveal_scrolls),
            SemanticAction::TextInputCommand {
                element_id,
                request,
            } => self.resolve_text_command(element_id, request),
            SemanticAction::TextInputEdit {
                element_id,
                request,
            } => self.resolve_text_edit(element_id, request),
            SemanticAction::TextInputCursor {
                element_id,
                x,
                y,
                extend_selection,
            } => self.resolve_text_cursor(element_id, x, y, extend_selection),
            SemanticAction::TextInputPreedit {
                element_id,
                request,
            } => self.resolve_text_preedit(element_id, request),
        }
    }

    fn resolve_focus_to(
        &mut self,
        next: Option<ElementId>,
        reveal_scrolls: Vec<FocusRevealScroll>,
    ) -> Vec<ListenerAction> {
        let previous = self.focused_id.clone();
        if previous == next {
            return Vec::new();
        }
        self.focused_id = next.clone();

        [
            previous.map(FocusTransition::Blur),
            next.map(FocusTransition::Focus),
        ]
        .into_iter()
        .flatten()
        .fold(Vec::new(), |mut out, transition| {
            self.append_focus_transition(transition, &mut out);
            out
        })
        .into_iter()
        .chain(reveal_scrolls.into_iter().map(|reveal| {
            ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                element_id: reveal.element_id,
                dx: reveal.dx,
                dy: reveal.dy,
            })
        }))
        .collect()
    }

    fn resolve_text_cursor(
        &mut self,
        element_id: ElementId,
        x: f32,
        y: f32,
        extend_selection: bool,
    ) -> Vec<ListenerAction> {
        let Some((runtime_actions, primary)) = ({
            let snapshot = match self.snapshot(&element_id) {
                Some(snapshot) => snapshot,
                None => return Vec::new(),
            };
            let next_cursor = cursor_from_click_point(snapshot, x, y);
            if !move_snapshot_cursor(snapshot, next_cursor, extend_selection) {
                None
            } else {
                Some((
                    text_runtime_actions(&element_id, snapshot),
                    extend_selection.then(|| selection_text(snapshot)).flatten(),
                ))
            }
        }) else {
            return Vec::new();
        };

        self.finish_with_primary_selection_write(runtime_actions, primary)
    }

    fn resolve_text_command(
        &mut self,
        element_id: ElementId,
        request: TextInputCommandRequest,
    ) -> Vec<ListenerAction> {
        match request {
            TextInputCommandRequest::SelectAll => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };

                    let len = text_ops::text_char_len(&snapshot.content);
                    let mut changed = if len == 0 {
                        snapshot.selection_anchor.take().is_some()
                    } else {
                        let changed_cursor = snapshot.cursor != len;
                        let changed_anchor = snapshot.selection_anchor != Some(0);
                        snapshot.cursor = len;
                        snapshot.selection_anchor = Some(0);
                        changed_cursor || changed_anchor
                    };

                    if clear_preedit_snapshot(snapshot) {
                        changed = true;
                    }
                    sync_snapshot_descriptor(snapshot);

                    changed.then(|| {
                        (
                            text_runtime_actions(&element_id, snapshot),
                            selection_text(snapshot),
                        )
                    })
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputCommandRequest::Copy => {
                let selection = self
                    .snapshot(&element_id)
                    .and_then(|snapshot| selection_text(snapshot))
                    .filter(|text| !text.is_empty());

                let Some(selection) = selection else {
                    return Vec::new();
                };

                self.set_clipboard(ClipboardTarget::Clipboard, selection.clone());
                self.set_clipboard(ClipboardTarget::Primary, selection.clone());
                vec![
                    ListenerAction::ClipboardWrite {
                        target: ClipboardTarget::Clipboard,
                        text: selection.clone(),
                    },
                    ListenerAction::ClipboardWrite {
                        target: ClipboardTarget::Primary,
                        text: selection,
                    },
                ]
            }
            TextInputCommandRequest::Cut => {
                let Some((selected, content_actions)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let Some((next_content, next_cursor, selected)) =
                        text_ops::cut_selection_content(
                            &snapshot.content,
                            snapshot.cursor,
                            snapshot.selection_anchor,
                        )
                    else {
                        return Vec::new();
                    };

                    apply_content_change_snapshot(snapshot, next_content, next_cursor);
                    let change_payload = snapshot.emit_change.then(|| snapshot.content.clone());
                    Some((
                        selected,
                        content_change_actions(&element_id, snapshot, change_payload),
                    ))
                }) else {
                    return Vec::new();
                };

                self.set_clipboard(ClipboardTarget::Clipboard, selected.clone());
                self.set_clipboard(ClipboardTarget::Primary, selected.clone());

                [
                    ListenerAction::ClipboardWrite {
                        target: ClipboardTarget::Clipboard,
                        text: selected.clone(),
                    },
                    ListenerAction::ClipboardWrite {
                        target: ClipboardTarget::Primary,
                        text: selected,
                    },
                ]
                .into_iter()
                .chain(content_actions)
                .collect()
            }
            TextInputCommandRequest::Paste => {
                self.resolve_text_paste(element_id, ClipboardTarget::Clipboard)
            }
            TextInputCommandRequest::PastePrimary => {
                self.resolve_text_paste(element_id, ClipboardTarget::Primary)
            }
        }
    }

    fn resolve_text_paste(
        &mut self,
        element_id: ElementId,
        target: ClipboardTarget,
    ) -> Vec<ListenerAction> {
        let Some(pasted) = self.clipboard_text(target) else {
            return Vec::new();
        };
        let Some(snapshot) = self.snapshot(&element_id) else {
            return Vec::new();
        };
        let pasted = sanitize_text_input_text(&pasted, snapshot.multiline);
        if pasted.is_empty() {
            return Vec::new();
        }
        let Some((next_content, next_cursor)) = text_ops::apply_insert(
            &snapshot.content,
            snapshot.cursor,
            snapshot.selection_anchor,
            &pasted,
        ) else {
            return Vec::new();
        };

        apply_content_change_snapshot(snapshot, next_content, next_cursor);
        let change_payload = snapshot.emit_change.then(|| snapshot.content.clone());
        content_change_actions(&element_id, snapshot, change_payload)
    }

    fn resolve_text_edit(
        &mut self,
        element_id: ElementId,
        request: TextInputEditRequest,
    ) -> Vec<ListenerAction> {
        match request {
            TextInputEditRequest::MoveLeft { extend_selection } => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let next_cursor = if !extend_selection {
                        if let Some((start, _)) = selected_range(snapshot) {
                            start
                        } else {
                            snapshot.cursor.saturating_sub(1)
                        }
                    } else {
                        snapshot.cursor.saturating_sub(1)
                    };

                    if !move_snapshot_cursor(snapshot, next_cursor, extend_selection) {
                        None
                    } else {
                        Some((
                            text_runtime_actions(&element_id, snapshot),
                            extend_selection.then(|| selection_text(snapshot)).flatten(),
                        ))
                    }
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputEditRequest::MoveRight { extend_selection } => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let len = text_ops::text_char_len(&snapshot.content);
                    let next_cursor = if !extend_selection {
                        if let Some((_, end)) = selected_range(snapshot) {
                            end
                        } else {
                            (snapshot.cursor + 1).min(len)
                        }
                    } else {
                        (snapshot.cursor + 1).min(len)
                    };

                    if !move_snapshot_cursor(snapshot, next_cursor, extend_selection) {
                        None
                    } else {
                        Some((
                            text_runtime_actions(&element_id, snapshot),
                            extend_selection.then(|| selection_text(snapshot)).flatten(),
                        ))
                    }
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputEditRequest::MoveHome { extend_selection } => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    if !move_snapshot_cursor(
                        snapshot,
                        snapshot.move_home_target(),
                        extend_selection,
                    ) {
                        None
                    } else {
                        Some((
                            text_runtime_actions(&element_id, snapshot),
                            extend_selection.then(|| selection_text(snapshot)).flatten(),
                        ))
                    }
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputEditRequest::MoveEnd { extend_selection } => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    if !move_snapshot_cursor(snapshot, snapshot.move_end_target(), extend_selection)
                    {
                        None
                    } else {
                        Some((
                            text_runtime_actions(&element_id, snapshot),
                            extend_selection.then(|| selection_text(snapshot)).flatten(),
                        ))
                    }
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputEditRequest::MoveUp { extend_selection } => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let next_cursor = snapshot.move_vertical_target(-1);
                    if !move_snapshot_cursor(snapshot, next_cursor, extend_selection) {
                        None
                    } else {
                        Some((
                            text_runtime_actions(&element_id, snapshot),
                            extend_selection.then(|| selection_text(snapshot)).flatten(),
                        ))
                    }
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputEditRequest::MoveDown { extend_selection } => {
                let Some((runtime_actions, primary)) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let next_cursor = snapshot.move_vertical_target(1);
                    if !move_snapshot_cursor(snapshot, next_cursor, extend_selection) {
                        None
                    } else {
                        Some((
                            text_runtime_actions(&element_id, snapshot),
                            extend_selection.then(|| selection_text(snapshot)).flatten(),
                        ))
                    }
                }) else {
                    return Vec::new();
                };

                self.finish_with_primary_selection_write(runtime_actions, primary)
            }
            TextInputEditRequest::Backspace => {
                let Some(actions) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let Some((next_content, next_cursor)) = text_ops::apply_backspace(
                        &snapshot.content,
                        snapshot.cursor,
                        snapshot.selection_anchor,
                    ) else {
                        return Vec::new();
                    };

                    apply_content_change_snapshot(snapshot, next_content, next_cursor);
                    let change_payload = snapshot.emit_change.then(|| snapshot.content.clone());
                    Some(content_change_actions(
                        &element_id,
                        snapshot,
                        change_payload,
                    ))
                }) else {
                    return Vec::new();
                };
                actions
            }
            TextInputEditRequest::Delete => {
                let Some(actions) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let Some((next_content, next_cursor)) = text_ops::apply_delete(
                        &snapshot.content,
                        snapshot.cursor,
                        snapshot.selection_anchor,
                    ) else {
                        return Vec::new();
                    };

                    apply_content_change_snapshot(snapshot, next_content, next_cursor);
                    let change_payload = snapshot.emit_change.then(|| snapshot.content.clone());
                    Some(content_change_actions(
                        &element_id,
                        snapshot,
                        change_payload,
                    ))
                }) else {
                    return Vec::new();
                };
                actions
            }
            TextInputEditRequest::DeleteSurrounding {
                before_length,
                after_length,
            } => {
                let Some(actions) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let Some((next_content, next_cursor)) = text_ops::apply_delete_surrounding(
                        &snapshot.content,
                        snapshot.cursor,
                        before_length,
                        after_length,
                    ) else {
                        return Vec::new();
                    };

                    apply_content_change_snapshot(snapshot, next_content, next_cursor);
                    let change_payload = snapshot.emit_change.then(|| snapshot.content.clone());
                    Some(content_change_actions(
                        &element_id,
                        snapshot,
                        change_payload,
                    ))
                }) else {
                    return Vec::new();
                };
                actions
            }
            TextInputEditRequest::Insert(text) => {
                let Some(actions) = ({
                    let snapshot = match self.snapshot(&element_id) {
                        Some(snapshot) => snapshot,
                        None => return Vec::new(),
                    };
                    let Some((next_content, next_cursor)) = text_ops::apply_insert(
                        &snapshot.content,
                        snapshot.cursor,
                        snapshot.selection_anchor,
                        &text,
                    ) else {
                        return Vec::new();
                    };

                    apply_content_change_snapshot(snapshot, next_content, next_cursor);
                    let change_payload = snapshot.emit_change.then(|| snapshot.content.clone());
                    Some(content_change_actions(
                        &element_id,
                        snapshot,
                        change_payload,
                    ))
                }) else {
                    return Vec::new();
                };
                actions
            }
        }
    }

    fn resolve_text_preedit(
        &mut self,
        element_id: ElementId,
        request: TextInputPreeditRequest,
    ) -> Vec<ListenerAction> {
        let Some(snapshot) = self.snapshot(&element_id) else {
            return Vec::new();
        };

        let changed = match request {
            TextInputPreeditRequest::Set { text, cursor } => {
                let next_preedit = if text.is_empty() { None } else { Some(text) };
                let next_cursor =
                    TextInputState::normalize_preedit_cursor(next_preedit.as_deref(), cursor);
                let mut changed = false;
                if snapshot.preedit != next_preedit {
                    snapshot.preedit = next_preedit;
                    changed = true;
                }
                if snapshot.preedit_cursor != next_cursor {
                    snapshot.preedit_cursor = next_cursor;
                    changed = true;
                }
                changed
            }
            TextInputPreeditRequest::Clear => clear_preedit_snapshot(snapshot),
        };

        if changed {
            sync_snapshot_descriptor(snapshot);
            text_runtime_actions(&element_id, snapshot)
        } else {
            Vec::new()
        }
    }
}

fn sync_snapshot_descriptor(snapshot: &mut TextInputState) {
    snapshot.sync_content_metadata();
}

fn clear_preedit_snapshot(snapshot: &mut TextInputState) -> bool {
    snapshot.clear_preedit()
}

fn set_text_input_content_snapshot(snapshot: &mut TextInputState, content: String) -> bool {
    let changed = snapshot.set_content(content);
    snapshot.content_origin = crate::tree::element::TextInputContentOrigin::Event;
    changed
}

fn set_text_input_runtime_snapshot(
    snapshot: &mut TextInputState,
    focused: bool,
    cursor: Option<u32>,
    selection_anchor: Option<u32>,
    preedit: Option<String>,
    preedit_cursor: Option<(u32, u32)>,
) -> bool {
    snapshot.set_runtime(focused, cursor, selection_anchor, preedit, preedit_cursor)
}

fn selected_range(snapshot: &TextInputState) -> Option<(u32, u32)> {
    snapshot.selected_range()
}

fn selection_text(snapshot: &TextInputState) -> Option<String> {
    snapshot.selection_text()
}

fn apply_content_change_snapshot(
    snapshot: &mut TextInputState,
    next_content: String,
    next_cursor: u32,
) {
    snapshot.apply_content_change(next_content, next_cursor);
    snapshot.content_origin = crate::tree::element::TextInputContentOrigin::Event;
}

fn move_snapshot_cursor(
    snapshot: &mut TextInputState,
    next_cursor: u32,
    extend_selection: bool,
) -> bool {
    snapshot.move_cursor(next_cursor, extend_selection)
}

fn text_runtime_tree_action(element_id: &ElementId, snapshot: &TextInputState) -> ListenerAction {
    ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime {
        element_id: element_id.clone(),
        focused: snapshot.focused,
        cursor: Some(snapshot.cursor),
        selection_anchor: snapshot.selection_anchor,
        preedit: snapshot.preedit.clone(),
        preedit_cursor: snapshot.preedit_cursor,
    })
}

fn text_runtime_mirror_change(element_id: &ElementId, snapshot: &TextInputState) -> ListenerAction {
    ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState {
        element_id: element_id.clone(),
        state: snapshot.clone(),
    })
}

fn text_runtime_actions(element_id: &ElementId, snapshot: &TextInputState) -> Vec<ListenerAction> {
    vec![
        text_runtime_tree_action(element_id, snapshot),
        text_runtime_mirror_change(element_id, snapshot),
    ]
}

fn content_change_actions(
    element_id: &ElementId,
    snapshot: &TextInputState,
    change_payload: Option<String>,
) -> Vec<ListenerAction> {
    [
        ListenerAction::TreeMsg(TreeMsg::SetTextInputContent {
            element_id: element_id.clone(),
            content: snapshot.content.clone(),
        }),
        text_runtime_tree_action(element_id, snapshot),
    ]
    .into_iter()
    .chain(snapshot.emit_change.then(|| {
        ListenerAction::RuntimeChange(RuntimeChange::ExpectTextInputPatchValue {
            element_id: element_id.clone(),
            content: snapshot.content.clone(),
        })
    }))
    .chain(change_payload.into_iter().map(|payload| {
        ListenerAction::ElixirEvent(ElixirEvent {
            element_id: element_id.clone(),
            kind: ElementEventKind::Change,
            payload: Some(payload),
        })
    }))
    .chain([text_runtime_mirror_change(element_id, snapshot)])
    .collect()
}

fn sanitize_single_line_text(text: &str) -> String {
    text.chars()
        .filter_map(|ch| {
            if ch == '\n' || ch == '\r' || ch == '\t' {
                Some(' ')
            } else if ch.is_control() {
                None
            } else {
                Some(ch)
            }
        })
        .collect()
}

fn sanitize_multiline_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter_map(|ch| {
            if ch == '\t' {
                Some(' ')
            } else if ch == '\n' || !ch.is_control() {
                Some(ch)
            } else {
                None
            }
        })
        .collect()
}

fn sanitize_text_input_text(text: &str, multiline: bool) -> String {
    if multiline {
        sanitize_multiline_text(text)
    } else {
        sanitize_single_line_text(text)
    }
}

fn cursor_from_click_point(snapshot: &TextInputState, x: f32, y: f32) -> u32 {
    snapshot.cursor_from_click_point(x, y)
}

fn scrollbar_press_actions_from_input(
    input: &InputEvent,
    element_id: &ElementId,
    spec: ScrollbarPressSpec,
) -> Vec<ListenerAction> {
    let InputEvent::CursorButton {
        button,
        action,
        x,
        y,
        ..
    } = input
    else {
        return Vec::new();
    };
    if button != "left" || *action != ACTION_PRESS {
        return Vec::new();
    }

    let Some(pointer_axis) = scrollbar_pointer_axis(spec.axis, spec.screen_to_local, *x, *y) else {
        return Vec::new();
    };
    let (pointer_offset, target_scroll) = match spec.area {
        ScrollbarHitArea::Thumb => (
            (pointer_axis - spec.thumb_start).clamp(0.0, spec.thumb_len),
            spec.scroll_offset,
        ),
        ScrollbarHitArea::Track => {
            let pointer_offset = spec.thumb_len / 2.0;
            let target_scroll = tree_scrollbar_target_from_pointer(
                pointer_axis,
                spec.track_start,
                spec.track_len,
                pointer_offset,
                spec.scroll_range,
            );
            (pointer_offset, target_scroll)
        }
    };

    let tracker = ScrollbarDragTracker {
        element_id: element_id.clone(),
        axis: spec.axis,
        track_start: spec.track_start,
        track_len: spec.track_len,
        thumb_len: spec.thumb_len,
        pointer_offset,
        scroll_range: spec.scroll_range,
        current_scroll: target_scroll,
        screen_to_local: spec.screen_to_local,
    };

    let delta = spec.scroll_offset - target_scroll;

    [ListenerAction::RuntimeChange(
        RuntimeChange::StartScrollbarDrag { tracker },
    )]
    .into_iter()
    .chain(
        (spec.area == ScrollbarHitArea::Track && delta.abs() >= f32::EPSILON)
            .then_some(scrollbar_drag_tree_action(element_id, spec.axis, delta)),
    )
    .collect()
}

fn scrollbar_drag_move_actions_from_input(
    input: &InputEvent,
    tracker: &ScrollbarDragTracker,
) -> Vec<ListenerAction> {
    let InputEvent::CursorPos { x, y } = input else {
        return Vec::new();
    };

    let Some(pointer_axis) = scrollbar_pointer_axis(tracker.axis, tracker.screen_to_local, *x, *y)
    else {
        return Vec::new();
    };
    let target_scroll = tree_scrollbar_target_from_pointer(
        pointer_axis,
        tracker.track_start,
        tracker.track_len,
        tracker.pointer_offset,
        tracker.scroll_range,
    );
    let delta = tracker.current_scroll - target_scroll;
    if delta.abs() < f32::EPSILON {
        return Vec::new();
    }

    vec![
        scrollbar_drag_tree_action(&tracker.element_id, tracker.axis, delta),
        ListenerAction::RuntimeChange(RuntimeChange::UpdateScrollbarDragCurrentScroll {
            current_scroll: target_scroll,
        }),
    ]
}

fn scrollbar_pointer_axis(
    axis: ScrollbarAxis,
    screen_to_local: Option<Affine2>,
    x: f32,
    y: f32,
) -> Option<f32> {
    let local = screen_to_local?.map_point(Point { x, y });
    Some(match axis {
        ScrollbarAxis::X => local.x,
        ScrollbarAxis::Y => local.y,
    })
}

fn scrollbar_drag_tree_action(
    element_id: &ElementId,
    axis: ScrollbarAxis,
    delta: f32,
) -> ListenerAction {
    ListenerAction::TreeMsg(match axis {
        ScrollbarAxis::X => TreeMsg::ScrollbarThumbDragX {
            element_id: element_id.clone(),
            dx: delta,
        },
        ScrollbarAxis::Y => TreeMsg::ScrollbarThumbDragY {
            element_id: element_id.clone(),
            dy: delta,
        },
    })
}

fn tree_scrollbar_target_from_pointer(
    pointer_axis: f32,
    track_start: f32,
    track_len: f32,
    pointer_offset: f32,
    scroll_range: f32,
) -> f32 {
    if track_len <= 0.0 || scroll_range <= 0.0 {
        return 0.0;
    }

    let min = track_start;
    let max = track_start + track_len;
    let next_thumb_start = (pointer_axis - pointer_offset).clamp(min, max);
    let ratio = if track_len > 0.0 {
        (next_thumb_start - track_start) / track_len
    } else {
        0.0
    };
    (ratio * scroll_range).clamp(0.0, scroll_range)
}

/// Slot builder for one deterministic element listener position.
type ElementSlotBuilder = fn(&Element, Option<&ResolvedNodeState>) -> Option<Listener>;

/// Deterministic slot order for base element listener assembly.
///
/// Reordering this table changes behavior.
const ELEMENT_LISTENER_SLOTS: &[ElementSlotBuilder] = &[
    slot_scrollbar_thumb_press_y,
    slot_scrollbar_thumb_press_x,
    slot_scrollbar_track_press_y,
    slot_scrollbar_track_press_x,
    slot_primary_left_release,
    slot_mouse_down_release_anywhere,
    slot_text_commit,
    slot_text_preedit,
    slot_text_preedit_clear,
    slot_text_delete_surrounding,
    slot_key_backspace_press,
    slot_key_delete_press,
    slot_key_left_press,
    slot_key_right_press,
    slot_key_up_press,
    slot_key_down_press,
    slot_key_home_press,
    slot_key_end_press,
    slot_key_select_all_press,
    slot_key_copy_press,
    slot_key_cut_press,
    slot_key_paste_press,
    slot_multiline_enter_press,
    slot_key_enter_press,
    slot_mouse_down_window_blur_clear,
];

/// Return pointer region only when interaction data exists and is visible.
///
/// Pointer-driven slots use this gate; non-pointer features should not.
fn pointer_region_for_element(state: &ResolvedNodeState) -> Option<PointerRegion> {
    state.visible.then_some(PointerRegion::for_state(state))
}

fn pointer_region_for_subregion(state: &ResolvedNodeState, bounds: Rect) -> Option<PointerRegion> {
    state
        .visible
        .then_some(PointerRegion::for_subregion(state, bounds, None))
}

fn scrollbar_nodes_for_state(
    state: &ResolvedNodeState,
) -> (Option<ScrollbarNode>, Option<ScrollbarNode>) {
    let scrollbar_x = state
        .scrollbar_x
        .map(|metrics| scrollbar_node_from_metrics(metrics, 0.0, 0.0, state.interaction_inverse));
    let scrollbar_y = state
        .scrollbar_y
        .map(|metrics| scrollbar_node_from_metrics(metrics, 0.0, 0.0, state.interaction_inverse));
    (scrollbar_x, scrollbar_y)
}

fn live_scrollbar_nodes_for_element(
    element: &Element,
    state: &ResolvedNodeState,
) -> (Option<ScrollbarNode>, Option<ScrollbarNode>) {
    let (scrollbar_x, scrollbar_y) = scrollbar_nodes_for_state(state);
    (
        element
            .attrs
            .scrollbar_x
            .unwrap_or(false)
            .then_some(scrollbar_x)
            .flatten(),
        element
            .attrs
            .scrollbar_y
            .unwrap_or(false)
            .then_some(scrollbar_y)
            .flatten(),
    )
}

fn focused_text_input_id(element: &Element) -> Option<ElementId> {
    if !element.kind.is_text_input_family() {
        return None;
    }

    let focused = element.attrs.text_input_focused.unwrap_or(false);
    if !focused {
        return None;
    }

    Some(element.id.clone())
}

fn text_input_emit_change(element: &Element) -> Option<bool> {
    element
        .kind
        .is_text_input_family()
        .then_some(element.attrs.on_change.unwrap_or(false))
}

fn cursor_icon_for_element(element: &Element) -> Option<CursorIcon> {
    if element.kind.is_text_input_family() {
        Some(CursorIcon::Text)
    } else if element.attrs.on_click.unwrap_or(false)
        || element.attrs.on_press.unwrap_or(false)
        || element.attrs.on_mouse_down.unwrap_or(false)
        || has_swipe_listener(element)
        || element.attrs.virtual_key.is_some()
    {
        Some(CursorIcon::Pointer)
    } else {
        None
    }
}

fn swipe_handlers_for_element(element: &Element) -> SwipeHandlers {
    let attrs = &element.attrs;
    SwipeHandlers {
        up: attrs.on_swipe_up.unwrap_or(false),
        down: attrs.on_swipe_down.unwrap_or(false),
        left: attrs.on_swipe_left.unwrap_or(false),
        right: attrs.on_swipe_right.unwrap_or(false),
    }
}

fn has_swipe_listener(element: &Element) -> bool {
    swipe_handlers_for_element(element).any()
}

fn tracks_hover_inside(element: &Element) -> bool {
    let attrs = &element.attrs;
    attrs.mouse_over.is_some()
        || attrs.on_mouse_enter.unwrap_or(false)
        || attrs.on_mouse_leave.unwrap_or(false)
}

fn owns_steady_cursor_inside(element: &Element, has_scrollbar_hover: bool) -> bool {
    let attrs = &element.attrs;
    cursor_icon_for_element(element).is_some()
        || attrs.on_mouse_move.unwrap_or(false)
        || tracks_hover_inside(element)
        || attrs.on_mouse_down.unwrap_or(false)
        || attrs.on_mouse_up.unwrap_or(false)
        || attrs.mouse_down.is_some()
        || attrs.mouse_down_active.unwrap_or(false)
        || is_focusable(element)
        || has_scrollbar_hover
}

fn is_focusable(element: &Element) -> bool {
    element.attrs.virtual_key.is_none()
        && (element.kind.is_text_input_family()
            || element.attrs.on_press.unwrap_or(false)
            || element.attrs.on_focus.unwrap_or(false)
            || element.attrs.on_blur.unwrap_or(false)
            || element.attrs.on_key_down.is_some()
            || element.attrs.on_key_up.is_some()
            || element.attrs.on_key_press.is_some())
}

fn padding_sides(element: &Element) -> (f32, f32, f32, f32) {
    match element.attrs.padding.as_ref() {
        Some(crate::tree::attrs::Padding::Uniform(v)) => {
            (*v as f32, *v as f32, *v as f32, *v as f32)
        }
        Some(crate::tree::attrs::Padding::Sides {
            left,
            top,
            right,
            bottom,
        }) => (*left as f32, *top as f32, *right as f32, *bottom as f32),
        None => (0.0, 0.0, 0.0, 0.0),
    }
}

fn focus_reveal_scrolls_for_contexts(
    element_id: &ElementId,
    element_rect: Rect,
    contexts: &[ScrollContext],
) -> Vec<FocusRevealScroll> {
    fn apply_focus_reveal_context(
        element_id: &ElementId,
        adjusted: Rect,
        context: &ScrollContext,
    ) -> (Rect, Option<FocusRevealScroll>) {
        if context.id == *element_id {
            return (adjusted, None);
        }

        let mut scroll_delta_x = 0.0;
        if context.max_x > 0.0 {
            let viewport_left = context.viewport.x;
            let viewport_right = context.viewport.x + context.viewport.width;
            let element_left = adjusted.x;
            let element_right = adjusted.x + adjusted.width;

            let mut desired_scroll_x = context.scroll_x;
            if element_left < viewport_left {
                desired_scroll_x += element_left - viewport_left;
            } else if element_right > viewport_right {
                desired_scroll_x += element_right - viewport_right;
            }

            desired_scroll_x = desired_scroll_x.clamp(0.0, context.max_x);
            scroll_delta_x = desired_scroll_x - context.scroll_x;
        }

        let mut scroll_delta_y = 0.0;
        if context.max_y > 0.0 {
            let viewport_top = context.viewport.y;
            let viewport_bottom = context.viewport.y + context.viewport.height;
            let element_top = adjusted.y;
            let element_bottom = adjusted.y + adjusted.height;

            let mut desired_scroll_y = context.scroll_y;
            if element_top < viewport_top {
                desired_scroll_y += element_top - viewport_top;
            } else if element_bottom > viewport_bottom {
                desired_scroll_y += element_bottom - viewport_bottom;
            }

            desired_scroll_y = desired_scroll_y.clamp(0.0, context.max_y);
            scroll_delta_y = desired_scroll_y - context.scroll_y;
        }

        if scroll_delta_x.abs() > f32::EPSILON || scroll_delta_y.abs() > f32::EPSILON {
            (
                Rect {
                    x: adjusted.x - scroll_delta_x,
                    y: adjusted.y - scroll_delta_y,
                    ..adjusted
                },
                Some(FocusRevealScroll {
                    element_id: context.id.clone(),
                    dx: -scroll_delta_x,
                    dy: -scroll_delta_y,
                }),
            )
        } else {
            (adjusted, None)
        }
    }

    contexts
        .iter()
        .rev()
        .fold(
            (element_rect, Vec::new()),
            |(adjusted, mut requests), context| {
                let (adjusted, request) = apply_focus_reveal_context(element_id, adjusted, context);
                requests.extend(request);
                (adjusted, requests)
            },
        )
        .1
}

fn focus_to_action(
    next: Option<ElementId>,
    reveal_scrolls: Vec<FocusRevealScroll>,
) -> ListenerAction {
    ListenerAction::Semantic(SemanticAction::FocusTo {
        next,
        reveal_scrolls,
    })
}

fn focus_to_element_action(
    focus_meta: &ElementFocusMeta,
    element_id: &ElementId,
) -> ListenerAction {
    focus_to_action(
        Some(element_id.clone()),
        focus_meta.self_reveal_scrolls.clone(),
    )
}

fn emit_element_listeners_with_focus_meta(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    focus_meta: Option<&ElementFocusMeta>,
    out: &mut PrecedenceEmitter<'_>,
) {
    // Reordering these emissions changes per-element precedence. This function
    // is the element-side precedence table in code form.
    if element.kind.is_text_input_family() {
        emit_key_binding_listeners_for_element(element, out);
    }
    out.emit_all(
        ELEMENT_LISTENER_SLOTS
            .iter()
            .filter_map(|build| build(element, state)),
    );
    emit_cursor_state_listeners(element, state, out);
    out.emit_opt(slot_primary_left_press(element, state, focus_meta));
    emit_scroll_listeners_for_element(element, state, out);
    emit_key_scroll_listeners_for_element(element, out);
    if !element.kind.is_text_input_family() {
        emit_key_binding_listeners_for_element(element, out);
    }
    out.emit_opt(slot_middle_paste_primary_press(element, state, focus_meta));
    if state.is_some_and(|state| state.front_nearby_root) {
        emit_front_nearby_blockers_for_element(element, state, out);
    }
}

fn emit_cursor_state_listeners(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    out: &mut PrecedenceEmitter<'_>,
) {
    out.emit_opt(slot_hover_pointer_enter(element, state));
    out.emit_opt(slot_cursor_pos_inside(element, state));
    out.emit_opt(slot_cursor_pos_outside(element, state));
    out.emit_opt(slot_hover_leave_owner(element, state));
}

fn emit_focus_cycle_listeners_for_state(state: &FocusBuildState, out: &mut PrecedenceEmitter<'_>) {
    let (next, next_reveal_scrolls, previous, previous_reveal_scrolls, element_id) =
        if let Some(focused_id) = state.focused_id.as_ref() {
            let Some(meta) = state.by_id.get(focused_id) else {
                return;
            };
            let Some(next) = meta.tab_next.clone() else {
                return;
            };
            let Some(previous) = meta.tab_prev.clone() else {
                return;
            };
            (
                next,
                meta.tab_next_reveal_scrolls.clone(),
                previous,
                meta.tab_prev_reveal_scrolls.clone(),
                Some(focused_id.clone()),
            )
        } else {
            let Some(next) = state.first_focusable.clone() else {
                return;
            };
            let Some(previous) = state.last_focusable.clone() else {
                return;
            };
            (
                next,
                state.first_focusable_reveal_scrolls.clone(),
                previous,
                state.last_focusable_reveal_scrolls.clone(),
                None,
            )
        };

    out.emit(Listener {
        element_id: element_id.clone(),
        matcher: ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta,
        compute: ListenerCompute::Static {
            actions: vec![focus_to_action(Some(next), next_reveal_scrolls)],
        },
    });
    out.emit(Listener {
        element_id,
        matcher: ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta,
        compute: ListenerCompute::Static {
            actions: vec![focus_to_action(Some(previous), previous_reveal_scrolls)],
        },
    });
}

fn focused_window_blur_listener(state: &FocusBuildState) -> Option<Listener> {
    state.focused_id.as_ref().map(|focused_id| Listener {
        element_id: Some(focused_id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::Static {
            actions: vec![focus_to_action(None, Vec::new())],
        },
    })
}

fn focus_build_state_from_entries(entries: &[FocusEntry]) -> FocusBuildState {
    let focused_index = entries.iter().position(|entry| entry.is_currently_focused);
    let focused_id = focused_index.map(|index| entries[index].element_id.clone());

    let first_focusable = entries.first().map(|entry| entry.element_id.clone());
    let first_focusable_reveal_scrolls = entries
        .first()
        .map(|entry| entry.self_reveal_scrolls.clone())
        .unwrap_or_default();
    let last_focusable = entries.last().map(|entry| entry.element_id.clone());
    let last_focusable_reveal_scrolls = entries
        .last()
        .map(|entry| entry.self_reveal_scrolls.clone())
        .unwrap_or_default();

    let by_id = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let tab_next_entry = entries.get((index + 1) % entries.len());
            let tab_prev_entry = if index == 0 {
                entries.last()
            } else {
                entries.get(index - 1)
            };

            (
                entry.element_id.clone(),
                ElementFocusMeta {
                    is_currently_focused: focused_index == Some(index),
                    self_reveal_scrolls: entry.self_reveal_scrolls.clone(),
                    tab_next: tab_next_entry.map(|next| next.element_id.clone()),
                    tab_next_reveal_scrolls: tab_next_entry
                        .map(|next| next.self_reveal_scrolls.clone())
                        .unwrap_or_default(),
                    tab_prev: tab_prev_entry.map(|prev| prev.element_id.clone()),
                    tab_prev_reveal_scrolls: tab_prev_entry
                        .map(|prev| prev.self_reveal_scrolls.clone())
                        .unwrap_or_default(),
                },
            )
        })
        .collect();

    FocusBuildState {
        focused_id,
        first_focusable,
        first_focusable_reveal_scrolls,
        last_focusable,
        last_focusable_reveal_scrolls,
        by_id,
    }
}

fn consider_focus_on_mount_candidate(
    acc: &mut RegistryBuildAcc,
    element: &Element,
    focus_meta: &ElementFocusMeta,
) {
    if !element.attrs.focus_on_mount.unwrap_or(false)
        || acc.current_revision == 0
        || element.mounted_at_revision != acc.current_revision
    {
        return;
    }

    let should_replace = match acc.focus_on_mount.as_ref() {
        Some(current) => element.mounted_at_revision > current.mounted_at_revision,
        None => true,
    };

    if should_replace {
        acc.focus_on_mount = Some(FocusOnMountTarget {
            element_id: element.id.clone(),
            reveal_scrolls: focus_meta.self_reveal_scrolls.clone(),
            mounted_at_revision: element.mounted_at_revision,
        });
    }
}

fn local_focus_meta_for_element(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    scroll_contexts: &[ScrollContext],
) -> (Option<ElementFocusMeta>, Vec<ScrollContext>) {
    let mut next_scroll_contexts = scroll_contexts.to_vec();
    let Some(state) = state else {
        return (None, next_scroll_contexts);
    };

    let self_rect = Rect::from_frame(state.adjusted_frame);
    if let Some(frame) = element.frame {
        let (left, top, right, bottom) = padding_sides(element);
        let content_rect = Rect {
            x: self_rect.x + left,
            y: self_rect.y + top,
            width: (self_rect.width - left - right).max(0.0),
            height: (self_rect.height - top - bottom).max(0.0),
        };

        let scroll_x_enabled = element.attrs.scrollbar_x.unwrap_or(false);
        let scroll_y_enabled = element.attrs.scrollbar_y.unwrap_or(false);
        let max_x = if scroll_x_enabled {
            element
                .attrs
                .scroll_x_max
                .unwrap_or((frame.content_width - frame.width).max(0.0) as f64) as f32
        } else {
            0.0
        }
        .max(0.0);
        let max_y = if scroll_y_enabled {
            element
                .attrs
                .scroll_y_max
                .unwrap_or((frame.content_height - frame.height).max(0.0) as f64) as f32
        } else {
            0.0
        }
        .max(0.0);
        let current_scroll_x = if scroll_x_enabled {
            (element.attrs.scroll_x.unwrap_or(0.0) as f32).clamp(0.0, max_x)
        } else {
            0.0
        };
        let current_scroll_y = if scroll_y_enabled {
            (element.attrs.scroll_y.unwrap_or(0.0) as f32).clamp(0.0, max_y)
        } else {
            0.0
        };

        if scroll_x_enabled || scroll_y_enabled {
            next_scroll_contexts.push(ScrollContext {
                id: element.id.clone(),
                viewport: content_rect,
                scroll_x: current_scroll_x,
                scroll_y: current_scroll_y,
                max_x,
                max_y,
            });
        }
    }

    let focus_meta = is_focusable(element).then(|| ElementFocusMeta {
        is_currently_focused: element.attrs.focused_active.unwrap_or(false),
        self_reveal_scrolls: focus_reveal_scrolls_for_contexts(
            &element.id,
            self_rect,
            &next_scroll_contexts,
        ),
        ..Default::default()
    });

    (focus_meta, next_scroll_contexts)
}

pub(crate) fn accumulate_element_rebuild(
    acc: &mut RegistryBuildAcc,
    element: &Element,
    state: Option<&ResolvedNodeState>,
    scroll_contexts: &[ScrollContext],
) -> Vec<ScrollContext> {
    if acc.focused_id.is_none() && element.attrs.focused_active.unwrap_or(false) {
        acc.focused_id = Some(element.id.clone());
    }

    let (local_focus_meta, next_scroll_contexts) =
        local_focus_meta_for_element(element, state, scroll_contexts);

    if let Some(state) = state {
        let adjusted_rect = Rect::from_frame(state.adjusted_frame);

        let (scrollbar_x, scrollbar_y) = live_scrollbar_nodes_for_element(element, state);

        if let Some(scrollbar) = scrollbar_x {
            let previous = acc
                .scrollbars
                .insert((element.id.clone(), ScrollbarAxis::X), scrollbar);
            debug_assert!(
                previous.is_none(),
                "duplicate horizontal scrollbar rebuild state"
            );
        }

        if let Some(scrollbar) = scrollbar_y {
            let previous = acc
                .scrollbars
                .insert((element.id.clone(), ScrollbarAxis::Y), scrollbar);
            debug_assert!(
                previous.is_none(),
                "duplicate vertical scrollbar rebuild state"
            );
        }

        if element.kind.is_text_input_family() {
            let previous = acc.text_inputs.insert(
                element.id.clone(),
                super::text_input_state(element, adjusted_rect, state.interaction_inverse),
            );
            debug_assert!(previous.is_none(), "duplicate text input rebuild state");
        }
    }

    if let Some(focus_meta) = local_focus_meta.as_ref() {
        acc.focus_entries.push(FocusEntry {
            element_id: element.id.clone(),
            is_currently_focused: focus_meta.is_currently_focused,
            self_reveal_scrolls: focus_meta.self_reveal_scrolls.clone(),
        });

        consider_focus_on_mount_candidate(acc, element, focus_meta);
    }

    acc.registry.in_precedence_order(|out| {
        emit_element_listeners_with_focus_meta(element, state, local_focus_meta.as_ref(), out)
    });

    next_scroll_contexts
}

fn accumulate_subtree_rebuild_local(
    tree: &ElementTree,
    element_id: &ElementId,
    acc: &mut RegistryBuildAcc,
    scroll_contexts: &[ScrollContext],
    scene_ctx: crate::tree::scene::SceneContext,
) -> Vec<DeferredSubtree> {
    let Some(element) = tree.get(element_id) else {
        return Vec::new();
    };

    let state = crate::tree::scene::resolve_node_state(element, scene_ctx);
    let next_scroll_contexts =
        accumulate_element_rebuild(acc, element, state.as_ref(), scroll_contexts);

    let mut deferred = Vec::new();

    for mount in element.local_nearby_mounts() {
        deferred.extend(accumulate_subtree_rebuild_local(
            tree,
            &mount.id,
            acc,
            scroll_contexts,
            state
                .clone()
                .map(|resolved| {
                    crate::tree::scene::child_context(resolved, RetainedPaintPhase::BehindContent)
                })
                .unwrap_or_default(),
        ));
    }

    let child_scene_ctx = state
        .clone()
        .map(|resolved| crate::tree::scene::child_context(resolved, RetainedPaintPhase::Children))
        .unwrap_or_default();
    element.for_each_retained_child(tree, |child| match child.mode {
        RetainedChildMode::Scope | RetainedChildMode::InlineEventOnly => {
            deferred.extend(accumulate_subtree_rebuild_local(
                tree,
                child.id,
                acc,
                &next_scroll_contexts,
                child_scene_ctx.clone(),
            ));
        }
    });

    for mount in element.escape_nearby_mounts() {
        deferred.push(DeferredSubtree {
            element_id: mount.id.clone(),
            scroll_contexts: scroll_contexts.to_vec(),
            scene_ctx: state
                .clone()
                .map(|resolved| {
                    crate::tree::scene::child_context(
                        resolved,
                        RetainedPaintPhase::Overlay(mount.slot),
                    )
                })
                .unwrap_or_default(),
        });
    }

    deferred
}

fn drain_deferred_subtrees(
    tree: &ElementTree,
    acc: &mut RegistryBuildAcc,
    deferred: Vec<DeferredSubtree>,
) {
    for subtree in deferred {
        let child_deferred = accumulate_subtree_rebuild_local(
            tree,
            &subtree.element_id,
            acc,
            &subtree.scroll_contexts,
            subtree.scene_ctx,
        );
        drain_deferred_subtrees(tree, acc, child_deferred);
    }
}

pub(crate) fn accumulate_subtree_rebuild(
    tree: &ElementTree,
    element_id: &ElementId,
    acc: &mut RegistryBuildAcc,
    scroll_contexts: &[ScrollContext],
    scene_ctx: crate::tree::scene::SceneContext,
) {
    let deferred =
        accumulate_subtree_rebuild_local(tree, element_id, acc, scroll_contexts, scene_ctx);
    drain_deferred_subtrees(tree, acc, deferred);
}

pub(crate) fn build_registry_rebuild(tree: &ElementTree) -> RegistryRebuildPayload {
    let mut acc = RegistryBuildAcc::for_tree(tree);

    if let Some(root) = tree.root.as_ref() {
        accumulate_subtree_rebuild(
            tree,
            root,
            &mut acc,
            &[],
            crate::tree::scene::SceneContext::default(),
        );
    }

    finalize_registry_rebuild(acc)
}

pub(crate) fn finalize_registry_rebuild(acc: RegistryBuildAcc) -> RegistryRebuildPayload {
    let focus_state = focus_build_state_from_entries(&acc.focus_entries);
    let mut low_registry = Registry::default();
    let mut high_registry = Registry::default();

    low_registry.in_precedence_order(|out| {
        emit_window_listeners(out);
        out.emit_opt(focused_window_blur_listener(&focus_state));
    });

    high_registry
        .in_precedence_order(|out| emit_focus_cycle_listeners_for_state(&focus_state, out));

    low_registry.extend_storage_from(&acc.registry);
    low_registry.extend_storage_from(&high_registry);

    RegistryRebuildPayload {
        base_registry: low_registry,
        text_inputs: acc.text_inputs,
        scrollbars: acc.scrollbars,
        focused_id: acc.focused_id,
        focus_on_mount: acc.focus_on_mount,
    }
}

#[cfg(test)]
fn root_ids_for_elements(elements: &[Element]) -> Vec<ElementId> {
    let child_ids: HashSet<ElementId> = elements
        .iter()
        .flat_map(|element| {
            element
                .children
                .iter()
                .cloned()
                .chain(element.nearby.iter().map(|mount| mount.id.clone()))
        })
        .collect();

    elements
        .iter()
        .filter(|element| !child_ids.contains(&element.id))
        .map(|element| element.id.clone())
        .collect()
}

/// Build first-iteration listeners for one element.
///
/// Current coverage:
/// - `on_mouse_down`, `on_mouse_up`, `on_mouse_move`
/// - hover enter/leave style transitions (`mouse_over` + `mouse_over_active`)
/// - mouse-down style transitions (`mouse_down` + `mouse_down_active`)
/// - pointer tracker bootstrap for `on_click`, pointer `on_press`, and pointer swipe listeners
/// - focused Enter-key `on_press` listeners
/// - concrete pointer focus transitions (`FocusTo`)
/// - focused text-input edit listeners with `on_change`-gated change emission
/// - text-input command listeners for cut/paste command requests
/// - local wheel-scroll listeners for scrollable elements
#[cfg(test)]
pub fn listeners_for_element(element: &Element) -> Vec<Listener> {
    let state = crate::tree::scene::resolve_node_state(
        element,
        crate::tree::scene::SceneContext::default(),
    );
    let (focus_meta, _) = local_focus_meta_for_element(element, state.as_ref(), &[]);
    let mut registry = Registry::default();
    registry.in_precedence_order(|out| {
        emit_element_listeners_with_focus_meta(element, state.as_ref(), focus_meta.as_ref(), out)
    });
    registry.precedence_listeners()
}

/// Build a base registry from a list of elements.
///
/// `elements` must already be in paint order (`parent`, then `children` in declared order).
#[cfg(test)]
pub fn registry_for_elements(elements: &[Element]) -> Registry {
    let mut tree = ElementTree::new();
    for element in elements {
        tree.insert(element.clone());
    }
    let root_ids = root_ids_for_elements(elements);
    tree.root = root_ids.first().cloned();
    let mut acc = RegistryBuildAcc::for_tree(&tree);

    for root_id in &root_ids {
        accumulate_subtree_rebuild(
            &tree,
            root_id,
            &mut acc,
            &[],
            crate::tree::scene::SceneContext::default(),
        );
    }

    finalize_registry_rebuild(acc).base_registry
}

/// Build window-level listeners that do not belong to any single element.
fn emit_window_listeners(out: &mut PrecedenceEmitter<'_>) {
    out.emit(Listener {
        element_id: None,
        matcher: ListenerMatcher::WindowResized,
        compute: ListenerCompute::WindowResizeToTree,
    });
    out.emit(Listener {
        element_id: None,
        matcher: ListenerMatcher::CursorPosAnywhere,
        compute: ListenerCompute::Static {
            actions: vec![ListenerAction::SetCursor(CursorIcon::Default)],
        },
    });
}

/// Build window-level listeners that do not belong to any single element.
#[cfg(test)]
pub fn window_listeners() -> Vec<Listener> {
    let mut registry = Registry::default();
    registry.in_precedence_order(emit_window_listeners);
    registry.precedence_listeners()
}

fn key_scroll_listener(
    source_element_id: Option<ElementId>,
    matcher: ListenerMatcher,
    target_id: &ElementId,
    dx: f32,
    dy: f32,
) -> Listener {
    Listener {
        element_id: source_element_id,
        matcher,
        compute: ListenerCompute::KeyScrollToTree {
            element_id: target_id.clone(),
            dx,
            dy,
        },
    }
}

fn emit_key_scroll_listeners_for_element(element: &Element, out: &mut PrecedenceEmitter<'_>) {
    out.emit_all(
        scroll_wheel::scroll_directions_for_element(element)
            .into_iter()
            .map(|direction| {
                let (matcher, dx, dy) = match direction {
                    ScrollDirection::XNeg => (
                        ListenerMatcher::KeyRightPressNoCtrlAltMeta,
                        -SCROLL_LINE_PIXELS,
                        0.0,
                    ),
                    ScrollDirection::XPos => (
                        ListenerMatcher::KeyLeftPressNoCtrlAltMeta,
                        SCROLL_LINE_PIXELS,
                        0.0,
                    ),
                    ScrollDirection::YNeg => (
                        ListenerMatcher::KeyDownPressNoCtrlAltMeta,
                        0.0,
                        -SCROLL_LINE_PIXELS,
                    ),
                    ScrollDirection::YPos => (
                        ListenerMatcher::KeyUpPressNoCtrlAltMeta,
                        0.0,
                        SCROLL_LINE_PIXELS,
                    ),
                };

                key_scroll_listener(Some(element.id.clone()), matcher, &element.id, dx, dy)
            }),
    );
}

fn emit_key_binding_listeners_for_element(element: &Element, out: &mut PrecedenceEmitter<'_>) {
    if !element.attrs.focused_active.unwrap_or(false) {
        return;
    }

    let mut slots = Vec::new();

    element
        .attrs
        .on_key_down
        .as_ref()
        .into_iter()
        .flatten()
        .for_each(|binding| {
            push_user_key_slot_action(
                &mut slots,
                UserKeySlotPhase::Down,
                binding,
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element.id.clone(),
                    kind: ElementEventKind::KeyDown,
                    payload: Some(binding.route.clone()),
                }),
            );

            if binding_arms_text_commit_suppression(element, binding) {
                push_user_key_slot_action(
                    &mut slots,
                    UserKeySlotPhase::Down,
                    binding,
                    ListenerAction::RuntimeChange(RuntimeChange::ArmTextCommitSuppression {
                        element_id: element.id.clone(),
                        key: binding.key,
                    }),
                );
            }
        });

    element
        .attrs
        .on_key_press
        .as_ref()
        .into_iter()
        .flatten()
        .for_each(|binding| {
            push_user_key_slot_action(
                &mut slots,
                UserKeySlotPhase::Down,
                binding,
                ListenerAction::RuntimeChange(RuntimeChange::StartKeyPressTracker {
                    tracker: key_press_tracker_for_binding(element, binding),
                }),
            );
        });

    element
        .attrs
        .on_key_up
        .as_ref()
        .into_iter()
        .flatten()
        .for_each(|binding| {
            push_user_key_slot_action(
                &mut slots,
                UserKeySlotPhase::Up,
                binding,
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id: element.id.clone(),
                    kind: ElementEventKind::KeyUp,
                    payload: Some(binding.route.clone()),
                }),
            );
        });

    out.emit_all(
        slots
            .into_iter()
            .map(|slot| user_key_slot_listener(element, slot)),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UserKeySlotPhase {
    Down,
    Up,
}

#[derive(Clone, Debug)]
struct UserKeySlot {
    phase: UserKeySlotPhase,
    key: CanonicalKey,
    mods: u8,
    match_mode: KeyBindingMatch,
    actions: Vec<ListenerAction>,
}

fn push_user_key_slot_action(
    slots: &mut Vec<UserKeySlot>,
    phase: UserKeySlotPhase,
    binding: &KeyBindingSpec,
    action: ListenerAction,
) {
    if let Some(slot) = slots.iter_mut().find(|slot| {
        slot.phase == phase
            && slot.key == binding.key
            && slot.mods == binding.mods
            && slot.match_mode == binding.match_mode
    }) {
        slot.actions.push(action);
    } else {
        slots.push(UserKeySlot {
            phase,
            key: binding.key,
            mods: binding.mods,
            match_mode: binding.match_mode,
            actions: vec![action],
        });
    }
}

fn key_press_tracker_for_binding(element: &Element, binding: &KeyBindingSpec) -> KeyPressTracker {
    KeyPressTracker {
        source_element_id: Some(element.id.clone()),
        key: binding.key,
        mods: binding.mods,
        match_mode: binding.match_mode,
        followups: vec![KeyPressFollowup::ElixirEvent {
            element_id: element.id.clone(),
            route: binding.route.clone(),
        }],
    }
}

fn user_key_slot_listener(element: &Element, slot: UserKeySlot) -> Listener {
    let matcher = match slot.phase {
        UserKeySlotPhase::Down => ListenerMatcher::KeyDownBinding {
            key: slot.key,
            mods: slot.mods,
            match_mode: slot.match_mode,
        },
        UserKeySlotPhase::Up => ListenerMatcher::KeyUpBinding {
            key: slot.key,
            mods: slot.mods,
            match_mode: slot.match_mode,
        },
    };

    Listener {
        element_id: Some(element.id.clone()),
        matcher,
        compute: ListenerCompute::Static {
            actions: slot.actions,
        },
    }
}

fn binding_arms_text_commit_suppression(element: &Element, binding: &KeyBindingSpec) -> bool {
    if !element.kind.is_text_input_family() || (binding.mods & (MOD_CTRL | MOD_META)) != 0 {
        return false;
    }

    matches!(
        binding.key,
        CanonicalKey::A
            | CanonicalKey::B
            | CanonicalKey::C
            | CanonicalKey::D
            | CanonicalKey::E
            | CanonicalKey::F
            | CanonicalKey::G
            | CanonicalKey::H
            | CanonicalKey::I
            | CanonicalKey::J
            | CanonicalKey::K
            | CanonicalKey::L
            | CanonicalKey::M
            | CanonicalKey::N
            | CanonicalKey::O
            | CanonicalKey::P
            | CanonicalKey::Q
            | CanonicalKey::R
            | CanonicalKey::S
            | CanonicalKey::T
            | CanonicalKey::U
            | CanonicalKey::V
            | CanonicalKey::W
            | CanonicalKey::X
            | CanonicalKey::Y
            | CanonicalKey::Z
            | CanonicalKey::Digit0
            | CanonicalKey::Digit1
            | CanonicalKey::Digit2
            | CanonicalKey::Digit3
            | CanonicalKey::Digit4
            | CanonicalKey::Digit5
            | CanonicalKey::Digit6
            | CanonicalKey::Digit7
            | CanonicalKey::Digit8
            | CanonicalKey::Digit9
            | CanonicalKey::Minus
            | CanonicalKey::Equal
            | CanonicalKey::Plus
            | CanonicalKey::Asterisk
            | CanonicalKey::LeftBracket
            | CanonicalKey::RightBracket
            | CanonicalKey::Backslash
            | CanonicalKey::Semicolon
            | CanonicalKey::Apostrophe
            | CanonicalKey::Grave
            | CanonicalKey::Comma
            | CanonicalKey::Period
            | CanonicalKey::Slash
            | CanonicalKey::Space
            | CanonicalKey::Tab
    ) || (element.kind == ElementKind::Multiline && binding.key == CanonicalKey::Enter)
}

fn slot_scrollbar_thumb_press_y(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let (_, scrollbar_y) = live_scrollbar_nodes_for_element(element, state?);
    let scrollbar = scrollbar_y?;
    Some(scrollbar_press_listener(
        element,
        state,
        scrollbar,
        ScrollbarHitArea::Thumb,
        scrollbar.thumb_rect,
    ))
}

fn slot_scrollbar_thumb_press_x(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let (scrollbar_x, _) = live_scrollbar_nodes_for_element(element, state?);
    let scrollbar = scrollbar_x?;
    Some(scrollbar_press_listener(
        element,
        state,
        scrollbar,
        ScrollbarHitArea::Thumb,
        scrollbar.thumb_rect,
    ))
}

fn slot_scrollbar_track_press_y(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let (_, scrollbar_y) = live_scrollbar_nodes_for_element(element, state?);
    let scrollbar = scrollbar_y?;
    Some(scrollbar_press_listener(
        element,
        state,
        scrollbar,
        ScrollbarHitArea::Track,
        scrollbar.track_rect,
    ))
}

fn slot_scrollbar_track_press_x(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let (scrollbar_x, _) = live_scrollbar_nodes_for_element(element, state?);
    let scrollbar = scrollbar_x?;
    Some(scrollbar_press_listener(
        element,
        state,
        scrollbar,
        ScrollbarHitArea::Track,
        scrollbar.track_rect,
    ))
}

fn scrollbar_press_listener(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    scrollbar: ScrollbarNode,
    area: ScrollbarHitArea,
    rect: Rect,
) -> Listener {
    let region = pointer_region_for_subregion(state.expect("scrollbar press needs state"), rect)
        .expect("scrollbar press needs interaction");
    Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftPressInside { region },
        compute: ListenerCompute::ScrollbarPressToRuntime {
            element_id: element.id.clone(),
            spec: ScrollbarPressSpec {
                axis: scrollbar.axis,
                area,
                track_start: scrollbar.track_start,
                track_len: scrollbar.track_len,
                thumb_start: scrollbar.thumb_start,
                thumb_len: scrollbar.thumb_len,
                scroll_offset: scrollbar.scroll_offset,
                scroll_range: scrollbar.scroll_range,
                screen_to_local: scrollbar.screen_to_local,
            },
        },
    }
}

/// Build primary left-press listener.
///
/// Aggregates actions from mouse events, mouse-down style activation, and
/// click/press tracker bootstrap.
fn slot_primary_left_press(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    focus_meta: Option<&ElementFocusMeta>,
) -> Option<Listener> {
    let region = pointer_region_for_element(state?)?;
    let matcher = ListenerMatcher::CursorButtonLeftPressInside {
        region: region.clone(),
    };
    let matcher_kind = matcher.kind();
    let actions: Vec<_> = mouse_events::left_press_actions(element)
        .into_iter()
        .chain(mouse_down_style::left_press_actions(element))
        .chain(virtual_key::left_press_actions(element, &region))
        .chain(
            focus_meta
                .filter(|focus_meta| is_focusable(element) && !focus_meta.is_currently_focused)
                .map(|focus_meta| focus_to_element_action(focus_meta, &element.id)),
        )
        .chain(click_press_tracker::left_press_actions(
            element,
            matcher_kind,
        ))
        .collect();
    let pointer_drag = click_press_tracker::left_press_drag_bootstrap(element, matcher_kind);
    let text_cursor_element_id = element
        .kind
        .is_text_input_family()
        .then_some(element.id.clone());
    let text_drag = element
        .kind
        .is_text_input_family()
        .then_some(TextDragTracker {
            element_id: element.id.clone(),
            matcher_kind,
        });

    (!actions.is_empty()
        || pointer_drag.is_some()
        || text_cursor_element_id.is_some()
        || text_drag.is_some())
    .then(|| {
        let compute =
            if pointer_drag.is_some() || text_cursor_element_id.is_some() || text_drag.is_some() {
                ListenerCompute::StaticWithLeftPressRuntimeAugment {
                    actions,
                    pointer_drag,
                    text_cursor_element_id,
                    text_drag,
                }
            } else {
                ListenerCompute::Static { actions }
            };

        Listener {
            element_id: Some(element.id.clone()),
            matcher,
            compute,
        }
    })
}

/// Build Left-key listener for focused text inputs.
fn slot_key_left_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    slot_text_key_edit(
        element,
        ListenerMatcher::KeyLeftPressNoCtrlAltMeta,
        TextInputKeyEditKind::Left,
    )
}

/// Build Right-key listener for focused text inputs.
fn slot_key_right_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    slot_text_key_edit(
        element,
        ListenerMatcher::KeyRightPressNoCtrlAltMeta,
        TextInputKeyEditKind::Right,
    )
}

fn slot_key_up_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    slot_text_key_edit(
        element,
        ListenerMatcher::KeyUpPressNoCtrlAltMeta,
        TextInputKeyEditKind::Up,
    )
}

fn slot_key_down_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    slot_text_key_edit(
        element,
        ListenerMatcher::KeyDownPressNoCtrlAltMeta,
        TextInputKeyEditKind::Down,
    )
}

/// Build Home-key listener for focused text inputs.
fn slot_key_home_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    slot_text_key_edit(
        element,
        ListenerMatcher::KeyHomePressNoCtrlAltMeta,
        TextInputKeyEditKind::Home,
    )
}

/// Build End-key listener for focused text inputs.
fn slot_key_end_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    slot_text_key_edit(
        element,
        ListenerMatcher::KeyEndPressNoCtrlAltMeta,
        TextInputKeyEditKind::End,
    )
}

fn slot_text_key_edit(
    element: &Element,
    matcher: ListenerMatcher,
    kind: TextInputKeyEditKind,
) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher,
        compute: ListenerCompute::TextInputKeyEditToRuntime { element_id, kind },
    })
}

fn slot_multiline_enter_press(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    if element.kind != ElementKind::Multiline || !element.attrs.text_input_focused.unwrap_or(false)
    {
        return None;
    }

    Some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::KeyEnterPressNoCtrlAltMeta,
        compute: ListenerCompute::Static {
            actions: vec![
                ListenerAction::Semantic(SemanticAction::TextInputEdit {
                    element_id: element.id.clone(),
                    request: TextInputEditRequest::Insert("\n".to_string()),
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ArmTextCommitSuppression {
                    element_id: element.id.clone(),
                    key: CanonicalKey::Enter,
                }),
            ],
        },
    })
}

/// Build primary left-release listener.
///
/// Emits `on_mouse_up` for the element under the release location and clears
/// mouse-down style when that release is also inside the element.
fn slot_primary_left_release(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let region = pointer_region_for_element(state?)?;
    let actions: Vec<ListenerAction> = [
        mouse_events::left_release_actions(element),
        mouse_down_style::left_release_actions(element),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!actions.is_empty()).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseInside { region },
        compute: ListenerCompute::Static { actions },
    })
}

fn slot_mouse_down_release_anywhere(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let actions = mouse_down_style::left_release_actions(element);

    (!actions.is_empty()).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::CursorButtonLeftReleaseAnywhere,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build the inside-cursor listener.
///
/// Emits all element behavior that depends on the cursor currently being inside
/// the element, including move, cursor ownership, and scrollbar hover.
fn slot_cursor_pos_inside(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let state = state?;
    let region = pointer_region_for_element(state)?;
    let scrollbar_hover = scrollbar_hover_compute_for_element(element, Some(state));
    let has_scrollbar_hover = scrollbar_hover.is_some();
    let cursor_icon = cursor_icon_for_element(element).or_else(|| {
        owns_steady_cursor_inside(element, has_scrollbar_hover).then_some(CursorIcon::Default)
    });
    let actions: Vec<ListenerAction> = mouse_events::cursor_pos_actions(element)
        .into_iter()
        .chain(cursor_icon.map(ListenerAction::SetCursor))
        .collect();

    (!actions.is_empty() || has_scrollbar_hover).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::CursorPosInside { region },
        compute: ListenerCompute::RawCursorPosWithScrollbarHover {
            actions,
            scrollbar_hover,
        },
    })
}

fn slot_hover_pointer_enter(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let state = state?;
    let region = pointer_region_for_element(state)?;
    let actions = hover::inside_actions(element);

    tracks_hover_inside(element).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::PointerEnterInside { region },
        compute: ListenerCompute::Static { actions },
    })
}

/// Build Enter key press listener for focused `on_press` behavior.
fn slot_key_enter_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    let actions = on_press_keyboard::enter_press_actions(element);

    (!actions.is_empty()).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::KeyEnterPressNoCtrlAltMeta,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build text-commit listener for focused text inputs.
fn slot_text_commit(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::TextCommitNoCtrlMeta,
        compute: ListenerCompute::TextCommitToRuntime { element_id },
    })
}

/// Build Backspace-key listener for focused text inputs.
fn slot_key_backspace_press(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::KeyBackspacePress,
        compute: ListenerCompute::TextInputEditToRuntimeMaybe {
            element_id,
            request: TextInputEditRequest::Backspace,
        },
    })
}

/// Build Delete-key listener for focused text inputs.
fn slot_key_delete_press(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::KeyDeletePress,
        compute: ListenerCompute::TextInputEditToRuntimeMaybe {
            element_id,
            request: TextInputEditRequest::Delete,
        },
    })
}

/// Build Ctrl/Meta+A select-all command listener for focused text inputs.
fn slot_key_select_all_press(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;
    let actions =
        text_input_commands::command_actions(&element_id, TextInputCommandRequest::SelectAll);

    Some(Listener {
        element_id: Some(element_id),
        matcher: ListenerMatcher::KeyAPressCtrlOrMeta,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build Ctrl/Meta+C copy command listener for focused text inputs.
fn slot_key_copy_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;
    let actions = text_input_commands::command_actions(&element_id, TextInputCommandRequest::Copy);

    Some(Listener {
        element_id: Some(element_id),
        matcher: ListenerMatcher::KeyCPressCtrlOrMeta,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build Ctrl/Meta+X cut command listener for focused text inputs.
fn slot_key_cut_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;
    let actions = text_input_commands::command_actions(&element_id, TextInputCommandRequest::Cut);

    Some(Listener {
        element_id: Some(element_id),
        matcher: ListenerMatcher::KeyXPressCtrlOrMeta,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build Ctrl/Meta+V paste command listener for focused text inputs.
fn slot_key_paste_press(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;
    let actions = text_input_commands::command_actions(&element_id, TextInputCommandRequest::Paste);

    Some(Listener {
        element_id: Some(element_id),
        matcher: ListenerMatcher::KeyVPressCtrlOrMeta,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build middle-button paste-primary command listener for text inputs.
fn slot_middle_paste_primary_press(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    focus_meta: Option<&ElementFocusMeta>,
) -> Option<Listener> {
    let region = pointer_region_for_element(state?)?;
    text_input_emit_change(element)?;
    let actions: Vec<_> = focus_meta
        .filter(|focus_meta| !focus_meta.is_currently_focused)
        .map(|focus_meta| focus_to_element_action(focus_meta, &element.id))
        .into_iter()
        .chain(text_input_commands::command_actions(
            &element.id,
            TextInputCommandRequest::PastePrimary,
        ))
        .collect();

    Some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::CursorButtonMiddlePressInside { region },
        compute: ListenerCompute::StaticWithTextInputCursorRuntime {
            actions,
            element_id: element.id.clone(),
            extend_selection: false,
        },
    })
}

/// Build IME preedit listener for focused text inputs.
fn slot_text_preedit(element: &Element, _state: Option<&ResolvedNodeState>) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::TextPreeditAny,
        compute: ListenerCompute::TextInputPreeditToRuntime { element_id },
    })
}

/// Build IME preedit-clear listener for focused text inputs.
fn slot_text_preedit_clear(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::TextPreeditClear,
        compute: ListenerCompute::TextInputPreeditToRuntime { element_id },
    })
}

/// Build IME delete-surrounding listener for focused text inputs.
fn slot_text_delete_surrounding(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let element_id = focused_text_input_id(element)?;

    Some(Listener {
        element_id: Some(element_id.clone()),
        matcher: ListenerMatcher::TextDeleteSurroundingAny,
        compute: ListenerCompute::TextDeleteSurroundingToRuntime { element_id },
    })
}

/// Build the outside-cursor listener.
///
/// Aggregates behavior that depends on the cursor being outside the element,
/// including mouse-down clear and scrollbar hover clear.
fn slot_cursor_pos_outside(
    element: &Element,
    state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let state = state?;
    let region = pointer_region_for_element(state)?;
    let actions = mouse_down_style::leave_actions(element);
    let scrollbar_hover = active_scrollbar_hover_compute_for_element(element, Some(state));

    (!actions.is_empty() || scrollbar_hover.is_some()).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::CursorLocationLeaveBoundary { region },
        compute: ListenerCompute::PointerLeaveWithScrollbarHover {
            actions,
            scrollbar_hover,
        },
    })
}

fn slot_hover_leave_owner(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let actions = hover::leave_actions(element);

    (!actions.is_empty()).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::HoverLeaveCurrentOwner,
        compute: ListenerCompute::Static { actions },
    })
}

/// Build primary scroll listeners.
fn emit_scroll_listeners_for_element(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    out: &mut PrecedenceEmitter<'_>,
) {
    let Some(region) = state.and_then(pointer_region_for_element) else {
        return;
    };

    out.emit_all(
        scroll_wheel::scroll_directions_for_element(element)
            .into_iter()
            .map(|direction| Listener {
                element_id: Some(element.id.clone()),
                matcher: ListenerMatcher::CursorScrollInsideDirection {
                    region: region.clone(),
                    direction,
                },
                compute: ListenerCompute::ScrollTreeMsgFromCursorScrollDirection {
                    element_id: element.id.clone(),
                    direction,
                },
            }),
    );
}

fn emit_front_nearby_blockers_for_element(
    element: &Element,
    state: Option<&ResolvedNodeState>,
    out: &mut PrecedenceEmitter<'_>,
) {
    let Some(region) = state.and_then(pointer_region_for_element) else {
        return;
    };

    let blocker = || ListenerCompute::Static {
        actions: Vec::new(),
    };
    let cursor_blocker = || ListenerCompute::Static {
        actions: vec![ListenerAction::SetCursor(CursorIcon::Default)],
    };

    out.emit_all([
        Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorButtonLeftPressInside {
                region: region.clone(),
            },
            compute: blocker(),
        },
        Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorButtonLeftReleaseInside {
                region: region.clone(),
            },
            compute: blocker(),
        },
        Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorButtonMiddlePressInside {
                region: region.clone(),
            },
            compute: blocker(),
        },
        Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorPosInside {
                region: region.clone(),
            },
            compute: cursor_blocker(),
        },
    ]);

    out.emit_all(
        [
            ScrollDirection::XNeg,
            ScrollDirection::XPos,
            ScrollDirection::YNeg,
            ScrollDirection::YPos,
        ]
        .into_iter()
        .map(|direction| Listener {
            element_id: Some(element.id.clone()),
            matcher: ListenerMatcher::CursorScrollInsideDirection {
                region: region.clone(),
                direction,
            },
            compute: blocker(),
        }),
    );
}

fn slot_mouse_down_window_blur_clear(
    element: &Element,
    _state: Option<&ResolvedNodeState>,
) -> Option<Listener> {
    let actions = mouse_down_style::window_blur_actions(element);

    (!actions.is_empty()).then_some(Listener {
        element_id: Some(element.id.clone()),
        matcher: ListenerMatcher::WindowBlurred,
        compute: ListenerCompute::Static { actions },
    })
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

    pub(super) fn inside_actions(element: &Element) -> Vec<ListenerAction> {
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
                ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive {
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
                ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive {
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
                vec![ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
                    element_id,
                    active: true,
                })]
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
                vec![ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
                    element_id,
                    active: false,
                })]
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
                vec![ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
                    element_id,
                    active: false,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn window_blur_actions(element: &Element) -> Vec<ListenerAction> {
        let element_id = element.id.clone();
        let (has_mouse_down_style, mouse_down_active) = has_and_active(element);

        (has_mouse_down_style && mouse_down_active)
            .then(|| {
                vec![ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
                    element_id,
                    active: false,
                })]
            })
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Click/press tracker bootstrap contributors (`on_click`, pointer `on_press`, swipe, drag-scrollable containers).
mod virtual_key {
    use super::*;

    pub(super) fn left_press_actions(
        element: &Element,
        region: &PointerRegion,
    ) -> Vec<ListenerAction> {
        element
            .attrs
            .virtual_key
            .as_ref()
            .map(|spec| {
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::StartVirtualKeyTracker {
                        tracker: VirtualKeyTracker {
                            element_id: element.id.clone(),
                            region: region.clone(),
                            tap: spec.tap.clone(),
                            hold: spec.hold,
                            hold_ms: spec.hold_ms,
                            repeat_ms: spec.repeat_ms,
                            phase: VirtualKeyPhase::Armed,
                        },
                    },
                )]
            })
            .unwrap_or_default()
    }
}

/// Click/press tracker bootstrap contributors (`on_click`, pointer `on_press`, swipe, drag-scrollable containers).
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
                vec![ListenerAction::RuntimeChange(
                    RuntimeChange::StartClickPressTracker {
                        element_id,
                        matcher_kind,
                        emit_click,
                        emit_press_pointer,
                    },
                )]
            })
            .into_iter()
            .flatten()
            .collect()
    }

    pub(super) fn left_press_drag_bootstrap(
        element: &Element,
        matcher_kind: ListenerMatcherKind,
    ) -> Option<PointerDragBootstrap> {
        let attrs = &element.attrs;
        let swipe_handlers = swipe_handlers_for_element(element);
        (attrs.on_click.unwrap_or(false)
            || attrs.on_press.unwrap_or(false)
            || swipe_handlers.any()
            || !scroll_wheel::scroll_directions_for_element(element).is_empty())
        .then(|| PointerDragBootstrap {
            element_id: element.id.clone(),
            matcher_kind,
            swipe_handlers,
        })
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
    ) -> Vec<ListenerAction> {
        vec![ListenerAction::Semantic(SemanticAction::TextInputCommand {
            element_id: element_id.clone(),
            request,
        })]
    }
}

/// Wheel-scroll compute contributor (`scrollbar_x/y`, `scroll_x_max/y_max`).
mod scroll_wheel {
    use super::*;

    pub(super) fn scroll_directions_for_element(element: &Element) -> Vec<ScrollDirection> {
        let attrs = &element.attrs;
        let scroll_x = attrs.scroll_x.unwrap_or(0.0) as f32;
        let scroll_y = attrs.scroll_y.unwrap_or(0.0) as f32;
        let scroll_x_max = attrs.scroll_x_max.unwrap_or(0.0) as f32;
        let scroll_y_max = attrs.scroll_y_max.unwrap_or(0.0) as f32;

        [
            (attrs.scrollbar_x.unwrap_or(false) && scroll_x < scroll_x_max)
                .then_some(ScrollDirection::XNeg),
            (attrs.scrollbar_x.unwrap_or(false) && scroll_x > 0.0).then_some(ScrollDirection::XPos),
            (attrs.scrollbar_y.unwrap_or(false) && scroll_y < scroll_y_max)
                .then_some(ScrollDirection::YNeg),
            (attrs.scrollbar_y.unwrap_or(false) && scroll_y > 0.0).then_some(ScrollDirection::YPos),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::actors::TreeMsg;
    use crate::clipboard::ClipboardTarget;
    use crate::events::test_support::{
        AnimatedNearbyHitCase, SampledRegistrySource, assert_registry_probe_matrix,
    };
    use crate::input::{
        ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
        SCROLL_LINE_PIXELS,
    };
    use crate::keys::CanonicalKey;
    use crate::tree::animation::{
        AnimationCurve, AnimationRepeat, AnimationRuntime, AnimationSpec,
    };
    use crate::tree::attrs::TextAlign;
    use crate::tree::attrs::{
        AlignX, AlignY, Attrs, KeyBindingMatch, KeyBindingSpec, Length, MouseOverAttrs,
        ScrollbarHoverAxis, VirtualKeyHoldMode, VirtualKeySpec, VirtualKeyTapAction,
    };
    use crate::tree::element::{Element, ElementId, ElementKind, ElementTree, Frame, NearbySlot};
    use crate::tree::geometry::{ClipShape, CornerRadii, Rect, ShapeBounds, clamp_radii};
    use crate::tree::layout::{
        Constraint, layout_and_refresh_default_with_animation, layout_tree_default_with_animation,
    };
    use crate::tree::scrollbar::ScrollbarAxis;
    use crate::tree::transform::{Affine2, InteractionClip};
    use std::time::{Duration, Instant};

    use super::{
        ClickPressTracker, DragTrackerState, ElixirEvent, GestureAxis, KeyPressFollowup,
        KeyPressTracker, Listener, ListenerAction, ListenerCompute, ListenerComputeCtx,
        ListenerInput, ListenerMatcher, ListenerMatcherKind, NoopListenerComputeCtx, PointerRegion,
        RuntimeChange, RuntimeOverlayState, ScrollDirection, ScrollbarDragTracker,
        ScrollbarHitArea, ScrollbarPressSpec, SwipeHandlers, SwipeTracker, TextDragTracker,
        VirtualKeyPhase, VirtualKeyTracker, compose_combined_registry, listeners_for_element,
        registry_for_elements, runtime_listeners_for_overlay, window_listeners,
    };
    use crate::events::{CursorIcon, ElementEventKind, RegistryRebuildPayload, TextInputState};

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

    fn build_pointer_region(visible: bool) -> PointerRegion {
        let rect = if visible {
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            }
        } else {
            Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }
        };

        PointerRegion {
            visible,
            local_shape: ShapeBounds { rect, radii: None },
            screen_to_local: Some(Affine2::identity()),
            screen_bounds: rect,
            clip_chain: Vec::new(),
        }
    }

    fn build_clipped_rounded_region() -> PointerRegion {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };

        PointerRegion {
            visible: true,
            local_shape: ShapeBounds { rect, radii: None },
            screen_to_local: Some(Affine2::identity()),
            screen_bounds: rect,
            clip_chain: vec![InteractionClip::new(
                ClipShape {
                    rect,
                    radii: Some(CornerRadii {
                        tl: 10.0,
                        tr: 10.0,
                        br: 10.0,
                        bl: 10.0,
                    }),
                },
                Affine2::identity(),
            )],
        }
    }

    fn build_pointer_subregion(
        region: PointerRegion,
        bounds: Rect,
        radii: Option<CornerRadii>,
    ) -> PointerRegion {
        PointerRegion {
            local_shape: ShapeBounds {
                rect: bounds,
                radii: radii.map(|value| clamp_radii(bounds, value)),
            },
            screen_bounds: bounds,
            ..region
        }
    }

    fn with_interaction(mut element: Element, visible: bool) -> Element {
        let rect = if visible {
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            }
        } else {
            Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }
        };
        let frame = Frame {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            content_width: rect.width,
            content_height: rect.height,
        };
        element.frame = Some(frame);
        element
    }

    fn with_interaction_rect(mut element: Element, visible: bool, hit_rect: Rect) -> Element {
        let rect = if visible {
            hit_rect
        } else {
            Rect {
                width: 0.0,
                height: 0.0,
                ..hit_rect
            }
        };
        let frame = Frame {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            content_width: rect.width,
            content_height: rect.height,
        };
        element.frame = Some(frame);
        element
    }

    fn with_frame(mut element: Element, frame: Frame) -> Element {
        element.frame = Some(frame);
        element
    }

    fn rebuild_payload_for_tree(tree: &ElementTree) -> RegistryRebuildPayload {
        let mut acc = super::RegistryBuildAcc::for_tree(tree);
        let root_id = tree.root.as_ref().expect("tree should have a root");

        super::accumulate_subtree_rebuild(
            tree,
            root_id,
            &mut acc,
            &[],
            crate::tree::scene::SceneContext::default(),
        );

        super::finalize_registry_rebuild(acc)
    }

    fn animated_width_move_registry_at(sample_ms: u64) -> super::Registry {
        let host_id = ElementId::from_term_bytes(vec![120]);
        let overlay_id = ElementId::from_term_bytes(vec![121]);

        let mut tree = crate::tree::element::ElementTree::new();

        let mut host_attrs = Attrs::default();
        host_attrs.width = Some(Length::Px(128.0));
        host_attrs.height = Some(Length::Px(82.0));
        let mut host = make_element(120, host_attrs);
        host.frame = None;
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
        overlay_attrs.animate = Some(AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        });

        let overlay = make_element(121, overlay_attrs);

        tree.root = Some(host_id.clone());
        tree.insert(host);
        tree.insert(overlay);

        let start = Instant::now();
        let mut runtime = AnimationRuntime::default();
        runtime.sync_with_tree(&tree, start);
        let _ = layout_tree_default_with_animation(
            &mut tree,
            Constraint::new(128.0, 82.0),
            1.0,
            &runtime,
            start + Duration::from_millis(sample_ms),
        );

        let elements: Vec<_> = tree.nodes.values().cloned().collect();
        registry_for_elements(&elements)
    }

    fn animated_width_move_render_registry_at(sample_ms: u64) -> super::Registry {
        let host_id = ElementId::from_term_bytes(vec![122]);
        let overlay_id = ElementId::from_term_bytes(vec![123]);

        let mut tree = crate::tree::element::ElementTree::new();

        let mut host_attrs = Attrs::default();
        host_attrs.width = Some(Length::Px(128.0));
        host_attrs.height = Some(Length::Px(82.0));
        let mut host = make_element(122, host_attrs);
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
        overlay_attrs.animate = Some(AnimationSpec {
            keyframes: vec![from, to],
            duration_ms: 1000.0,
            curve: AnimationCurve::Linear,
            repeat: AnimationRepeat::Once,
        });

        let overlay = make_element(123, overlay_attrs);

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
        .base_registry
    }

    #[derive(Default)]
    struct TestComputeCtx {
        focused_id: Option<ElementId>,
        hovered_id: Option<ElementId>,
        text_inputs: HashMap<ElementId, TextInputState>,
        clipboard: HashMap<ClipboardTarget, Option<String>>,
        base_registry: Option<super::Registry>,
        combined_registry: Option<super::Registry>,
    }

    impl ListenerComputeCtx for TestComputeCtx {
        fn focused_id(&self) -> Option<&ElementId> {
            self.focused_id.as_ref()
        }

        fn hover_owner(&self) -> Option<&ElementId> {
            self.hovered_id.as_ref()
        }

        fn text_input_state(&self, element_id: &ElementId) -> Option<TextInputState> {
            self.text_inputs.get(element_id).cloned()
        }

        fn clipboard_text(&mut self, target: ClipboardTarget) -> Option<String> {
            self.clipboard.get(&target).cloned().flatten()
        }

        fn dispatch_base(&mut self, input: &ListenerInput) -> Vec<ListenerAction> {
            let Some(registry) = self.base_registry.clone() else {
                return Vec::new();
            };
            registry.view().first_match(input, &[], self)
        }

        fn dispatch_effective_skip(
            &mut self,
            input: &ListenerInput,
            skip_matchers: &[ListenerMatcherKind],
        ) -> Vec<ListenerAction> {
            let Some(registry) = self.combined_registry.clone() else {
                return Vec::new();
            };
            registry.view().first_match(input, skip_matchers, self)
        }

        fn base_first_match_listener(
            &self,
            input: &ListenerInput,
            skip_matchers: &[ListenerMatcherKind],
        ) -> Option<Listener> {
            self.base_registry
                .as_ref()?
                .view()
                .matching_listener(input, skip_matchers)
                .cloned()
        }

        fn base_source_listener(
            &self,
            element_id: &ElementId,
            matcher_kind: ListenerMatcherKind,
        ) -> Option<Listener> {
            self.base_registry
                .as_ref()?
                .view()
                .find_precedence(|listener| {
                    listener.element_id.as_ref() == Some(element_id)
                        && listener.matcher.kind() == matcher_kind
                })
                .cloned()
        }
    }

    fn make_text_input_state(
        content: &str,
        cursor: u32,
        selection_anchor: Option<u32>,
        focused: bool,
        emit_change: bool,
    ) -> TextInputState {
        TextInputState {
            content: content.to_string(),
            content_origin: crate::tree::element::TextInputContentOrigin::TreePatch,
            content_len: content.chars().count() as u32,
            cursor,
            selection_anchor,
            preedit: None,
            preedit_cursor: None,
            focused,
            emit_change,
            multiline: false,
            frame_x: 0.0,
            frame_y: 0.0,
            frame_width: 100.0,
            frame_height: 20.0,
            inset_top: 0.0,
            inset_left: 0.0,
            inset_bottom: 0.0,
            inset_right: 0.0,
            screen_to_local: Some(Affine2::identity()),
            text_align: TextAlign::Left,
            font_family: "Arial".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }

    fn listener_matching(
        listeners: &[Listener],
        predicate: impl Fn(&Listener) -> bool,
    ) -> &Listener {
        listeners
            .iter()
            .find(|listener| predicate(listener))
            .expect("expected matching listener")
    }

    fn first_matching_actions_with_ctx(
        registry: &super::Registry,
        input: &InputEvent,
        ctx: &mut TestComputeCtx,
    ) -> Vec<ListenerAction> {
        if ctx.base_registry.is_none() {
            ctx.base_registry = Some(registry.clone());
        }
        if ctx.combined_registry.is_none() {
            ctx.combined_registry = Some(registry.clone());
        }
        registry
            .view()
            .find_precedence(|listener| listener.matcher.matches(input))
            .map(|listener| listener.compute_actions_with_ctx(input, ctx))
            .unwrap_or_default()
    }

    fn first_matching_actions(
        registry: &super::Registry,
        input: &InputEvent,
    ) -> Vec<ListenerAction> {
        let mut ctx = TestComputeCtx {
            base_registry: Some(registry.clone()),
            combined_registry: Some(registry.clone()),
            ..Default::default()
        };
        first_matching_actions_with_ctx(registry, input, &mut ctx)
    }

    fn actions_without_cursor(actions: &[ListenerAction]) -> Vec<ListenerAction> {
        actions
            .iter()
            .filter(|action| !matches!(action, ListenerAction::SetCursor(_)))
            .cloned()
            .collect()
    }

    fn cursor_actions(actions: &[ListenerAction]) -> Vec<CursorIcon> {
        actions
            .iter()
            .filter_map(|action| match action {
                ListenerAction::SetCursor(icon) => Some(*icon),
                _ => None,
            })
            .collect()
    }

    fn first_matching_listener_input_actions_with_ctx(
        registry: &super::Registry,
        input: &ListenerInput,
        ctx: &mut TestComputeCtx,
    ) -> Vec<ListenerAction> {
        if ctx.base_registry.is_none() {
            ctx.base_registry = Some(registry.clone());
        }
        if ctx.combined_registry.is_none() {
            ctx.combined_registry = Some(registry.clone());
        }
        registry
            .view()
            .find_precedence(|listener| listener.matcher.matches_input(input))
            .map(|listener| listener.compute_listener_input_with_ctx(input, ctx))
            .unwrap_or_default()
    }

    fn first_matching_listener_input_actions(
        registry: &super::Registry,
        input: &ListenerInput,
    ) -> Vec<ListenerAction> {
        let mut ctx = TestComputeCtx {
            base_registry: Some(registry.clone()),
            combined_registry: Some(registry.clone()),
            ..Default::default()
        };
        first_matching_listener_input_actions_with_ctx(registry, input, &mut ctx)
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

        let down_actions = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        })
        .compute_actions(&down_input);
        let up_actions = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftReleaseInside { .. }
            )
        })
        .compute_actions(&up_input);
        let move_actions = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        })
        .compute_actions(&move_input);

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
            actions_without_cursor(&move_actions).as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::MouseMove,
                ..
            })]
        ));
        assert_eq!(cursor_actions(&move_actions), vec![CursorIcon::Pointer]);
    }

    #[test]
    fn listeners_for_element_builds_inside_listener_when_hover_inactive() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_enter = Some(true);
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);
        let element = with_interaction(make_element(3, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 2);
        let enter_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::PointerEnterInside { .. })
        });
        let inside_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        });

        let actions = enter_listener
            .compute_listener_input_actions(&ListenerInput::PointerEnter { x: 10.0, y: 10.0 });
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
            ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive { active: true, .. })
        ));

        let raw_actions =
            inside_listener.compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert!(actions_without_cursor(&raw_actions).is_empty());
        assert_eq!(cursor_actions(&raw_actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn listeners_for_element_builds_leave_listener_when_hover_active() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_leave = Some(true);
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(true);
        let element = with_interaction(make_element(4, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 3);
        let leave_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::HoverLeaveCurrentOwner)
        });

        let actions = leave_listener.compute_listener_input_actions(&ListenerInput::PointerLeave {
            x: 120.0,
            y: 10.0,
            window_left: false,
        });
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
            ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive { active: false, .. })
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
        let enter_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::PointerEnterInside { .. })
        });
        let actions = enter_listener
            .compute_listener_input_actions(&ListenerInput::PointerEnter { x: 10.0, y: 10.0 });

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
            ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![22]) && active
        ));

        let release_actions = enter_listener
            .compute_listener_input_actions(&ListenerInput::PointerEnter { x: 10.0, y: 10.0 });
        assert_eq!(release_actions.len(), 2);
    }

    #[test]
    fn listeners_for_element_hover_style_without_mouse_move_still_activates_inside() {
        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(false);
        let element = with_interaction(make_element(24, attrs), true);

        let listeners = listeners_for_element(&element);
        let enter_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::PointerEnterInside { .. })
        });

        let actions = enter_listener
            .compute_listener_input_actions(&ListenerInput::PointerEnter { x: 10.0, y: 10.0 });

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![24]) && active
        ));

        let inside_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        });
        let raw_actions =
            inside_listener.compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert!(actions_without_cursor(&raw_actions).is_empty());
        assert_eq!(cursor_actions(&raw_actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn listeners_for_element_active_hover_keeps_inside_listener_for_default_cursor() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_enter = Some(true);
        attrs.mouse_over = Some(MouseOverAttrs::default());
        attrs.mouse_over_active = Some(true);
        let element = with_interaction(make_element(25, attrs), true);

        let listeners = listeners_for_element(&element);
        let inside_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        });
        let actions = inside_listener.compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert!(actions_without_cursor(&actions).is_empty());
        assert_eq!(cursor_actions(&actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn listeners_for_element_event_only_leave_emits_event_and_clears_hover_active() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_leave = Some(true);
        attrs.mouse_over_active = Some(true);
        let element = with_interaction(make_element(23, attrs), true);

        let listeners = listeners_for_element(&element);
        let leave_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::HoverLeaveCurrentOwner)
        });
        let actions = leave_listener.compute_listener_input_actions(&ListenerInput::PointerLeave {
            x: 0.0,
            y: 0.0,
            window_left: true,
        });

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
            ListenerAction::TreeMsg(TreeMsg::SetMouseOverActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![23]) && !active
        ));

        let release_actions =
            leave_listener.compute_listener_input_actions(&ListenerInput::PointerLeave {
                x: 120.0,
                y: 10.0,
                window_left: false,
            });
        assert_eq!(release_actions.len(), 2);
    }

    #[test]
    fn pointer_matchers_respect_clipped_rounded_interaction() {
        let region = build_clipped_rounded_region();
        let matcher = ListenerMatcher::CursorButtonLeftPressInside { region };

        assert!(!matcher.matches(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 2.0,
            y: 2.0,
        }));
        assert!(matcher.matches(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 2.0,
        }));
    }

    #[test]
    fn matcher_kind_uses_variant_identity_only() {
        let region = build_pointer_region(true);
        let a = ListenerMatcher::CursorButtonLeftPressInside {
            region: region.clone(),
        };
        let b = ListenerMatcher::CursorButtonLeftPressInside {
            region: build_pointer_subregion(
                build_pointer_region(true),
                Rect {
                    x: 50.0,
                    y: 50.0,
                    width: 20.0,
                    height: 20.0,
                },
                None,
            ),
        };
        let c = ListenerMatcher::CursorButtonLeftReleaseInside { region };

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
        assert_eq!(listeners.len(), 2);
        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });
        assert_eq!(
            press_listener.element_id,
            Some(ElementId::from_term_bytes(vec![5]))
        );

        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active })]
                if element_id == &ElementId::from_term_bytes(vec![5]) && *active
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
        assert_eq!(listeners.len(), 2);
        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });

        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
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
            ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
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
        assert_eq!(listeners.len(), 2);

        let actions = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        })
        .compute_actions(&InputEvent::CursorButton {
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
            ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive {
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
            ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker {
                ref element_id,
                matcher_kind,
                origin_x,
                origin_y,
                ..
            }) if *element_id == ElementId::from_term_bytes(vec![11])
                && matcher_kind == ListenerMatcherKind::CursorButtonLeftPressInside
                && origin_x == 10.0
                && origin_y == 10.0
        ));
    }

    #[test]
    fn listeners_for_element_mouse_down_style_active_adds_release_and_leave_clear() {
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(true);
        let element = with_interaction(make_element(6, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 5);

        let release_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftReleaseAnywhere
            )
        });

        let release_actions = release_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });
        assert!(matches!(
            release_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active })]
                if element_id == &ElementId::from_term_bytes(vec![6]) && !*active
        ));

        let leave_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorLocationLeaveBoundary { .. }
            )
        });

        let leave_actions =
            leave_listener.compute_listener_input_actions(&ListenerInput::PointerLeave {
                x: 0.0,
                y: 0.0,
                window_left: true,
            });
        assert!(matches!(
            leave_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active })]
                if element_id == &ElementId::from_term_bytes(vec![6]) && !*active
        ));

        let blur_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::WindowBlurred)
        });

        let blur_actions = blur_listener.compute_actions(&InputEvent::Focused { focused: false });
        assert!(matches!(
            blur_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active })]
                if element_id == &ElementId::from_term_bytes(vec![6]) && !*active
        ));
    }

    #[test]
    fn registry_for_elements_keeps_mouse_up_targeted_and_mouse_down_clear_anywhere() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_up = Some(true);
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(true);
        let element = with_interaction(make_element(60, attrs), true);
        let registry = registry_for_elements(&[element]);

        let inside_actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
        );
        assert!(matches!(
            inside_actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent { kind, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active }),
            ] if *kind == ElementEventKind::MouseUp
                && *element_id == ElementId::from_term_bytes(vec![60])
                && !*active
        ));

        let outside_actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 120.0,
                y: 10.0,
            },
        );
        assert!(matches!(
            outside_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active })]
                if *element_id == ElementId::from_term_bytes(vec![60]) && !*active
        ));
    }

    #[test]
    fn registry_for_elements_front_nearby_blocker_suppresses_underlying_mouse_down() {
        let mut host = with_interaction_rect(
            make_element(80, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );
        host.children = vec![ElementId::from_term_bytes(vec![81])];
        host.nearby.set(
            NearbySlot::InFront,
            Some(ElementId::from_term_bytes(vec![82])),
        );

        let mut underlying_attrs = Attrs::default();
        underlying_attrs.on_mouse_down = Some(true);
        let underlying = with_interaction_rect(
            make_element(81, underlying_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );

        let overlay = with_interaction_rect(
            make_element(82, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[host, underlying, overlay]);

        let covered_actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 50.0,
                y: 10.0,
            },
        );
        assert!(covered_actions.is_empty());

        let uncovered_actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 120.0,
                y: 10.0,
            },
        );
        assert!(matches!(
            uncovered_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == ElementId::from_term_bytes(vec![81])
                    && *kind == ElementEventKind::MouseDown
        ));
    }

    #[test]
    fn registry_for_elements_front_nearby_real_listener_precedes_blocker() {
        let mut host = with_interaction_rect(
            make_element(83, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );
        host.children = vec![ElementId::from_term_bytes(vec![84])];
        host.nearby.set(
            NearbySlot::InFront,
            Some(ElementId::from_term_bytes(vec![85])),
        );

        let mut underlying_attrs = Attrs::default();
        underlying_attrs.on_mouse_down = Some(true);
        let underlying = with_interaction_rect(
            make_element(84, underlying_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_mouse_down = Some(true);
        let overlay = with_interaction_rect(
            make_element(85, overlay_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[host, underlying, overlay]);

        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 50.0,
                y: 10.0,
            },
        );
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == ElementId::from_term_bytes(vec![85])
                    && *kind == ElementEventKind::MouseDown
        ));
    }

    #[test]
    fn registry_for_elements_clip_nearby_clips_escape_overlay_interaction() {
        let mut host_attrs = Attrs::default();
        host_attrs.clip_nearby = Some(true);
        let mut host = with_interaction_rect(
            make_element(86, host_attrs),
            true,
            Rect {
                x: 50.0,
                y: 50.0,
                width: 100.0,
                height: 40.0,
            },
        );
        host.nearby.set(
            NearbySlot::Above,
            Some(ElementId::from_term_bytes(vec![87])),
        );

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_mouse_down = Some(true);
        let overlay = with_interaction_rect(
            make_element(87, overlay_attrs),
            true,
            Rect {
                x: 50.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[host, overlay]);
        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 60.0,
                y: 30.0,
            },
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn registry_for_elements_earlier_child_escape_beats_later_normal_sibling() {
        let host_id = ElementId::from_term_bytes(vec![141]);
        let later_id = ElementId::from_term_bytes(vec![142]);
        let overlay_id = ElementId::from_term_bytes(vec![143]);

        let mut root = with_frame(
            make_element(140, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 120.0,
                content_width: 220.0,
                content_height: 120.0,
            },
        );
        root.children = vec![host_id.clone(), later_id.clone()];

        let mut host = with_interaction_rect(
            make_element(141, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 40.0,
            },
        );
        host.nearby.set(NearbySlot::Below, Some(overlay_id.clone()));

        let mut later_attrs = Attrs::default();
        later_attrs.on_mouse_down = Some(true);
        let later = with_interaction_rect(
            make_element(142, later_attrs),
            true,
            Rect {
                x: 0.0,
                y: 48.0,
                width: 220.0,
                height: 40.0,
            },
        );

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_mouse_down = Some(true);
        let overlay = with_interaction_rect(
            make_element(143, overlay_attrs),
            true,
            Rect {
                x: 100.0,
                y: 48.0,
                width: 60.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[root, host, later, overlay]);
        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 110.0,
                y: 60.0,
            },
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == overlay_id && *kind == ElementEventKind::MouseDown
        ));
    }

    #[test]
    fn registry_for_elements_ancestor_in_front_beats_descendant_below() {
        let parent_id = ElementId::from_term_bytes(vec![145]);
        let ancestor_overlay_id = ElementId::from_term_bytes(vec![146]);
        let descendant_overlay_id = ElementId::from_term_bytes(vec![147]);

        let mut root = with_frame(
            make_element(144, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 120.0,
                content_width: 220.0,
                content_height: 120.0,
            },
        );
        root.children = vec![parent_id.clone()];
        root.nearby
            .set(NearbySlot::InFront, Some(ancestor_overlay_id.clone()));

        let mut parent = with_interaction_rect(
            make_element(145, Attrs::default()),
            true,
            Rect {
                x: 60.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );
        parent
            .nearby
            .set(NearbySlot::Below, Some(descendant_overlay_id.clone()));

        let mut ancestor_overlay_attrs = Attrs::default();
        ancestor_overlay_attrs.on_mouse_down = Some(true);
        let ancestor_overlay = with_interaction_rect(
            make_element(146, ancestor_overlay_attrs),
            true,
            Rect {
                x: 80.0,
                y: 48.0,
                width: 60.0,
                height: 40.0,
            },
        );

        let mut descendant_overlay_attrs = Attrs::default();
        descendant_overlay_attrs.on_mouse_down = Some(true);
        let descendant_overlay = with_interaction_rect(
            make_element(147, descendant_overlay_attrs),
            true,
            Rect {
                x: 80.0,
                y: 48.0,
                width: 60.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[root, parent, ancestor_overlay, descendant_overlay]);
        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 90.0,
                y: 60.0,
            },
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == ancestor_overlay_id && *kind == ElementEventKind::MouseDown
        ));
    }

    #[test]
    fn registry_for_elements_focus_order_follows_paint_order_with_escape_overlay() {
        let root_id = ElementId::from_term_bytes(vec![149]);
        let host_id = ElementId::from_term_bytes(vec![150]);
        let sibling_id = ElementId::from_term_bytes(vec![151]);
        let overlay_id = ElementId::from_term_bytes(vec![152]);

        let mut root = with_frame(
            make_element(149, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 120.0,
                content_width: 220.0,
                content_height: 120.0,
            },
        );
        root.children = vec![host_id.clone(), sibling_id.clone()];

        let mut host_attrs = Attrs::default();
        host_attrs.on_focus = Some(true);
        host_attrs.focused_active = Some(true);
        let mut host = with_interaction_rect(
            make_element(150, host_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 40.0,
            },
        );
        host.nearby.set(NearbySlot::Below, Some(overlay_id.clone()));

        let mut sibling_attrs = Attrs::default();
        sibling_attrs.on_focus = Some(true);
        let sibling = with_interaction_rect(
            make_element(151, sibling_attrs),
            true,
            Rect {
                x: 0.0,
                y: 48.0,
                width: 120.0,
                height: 40.0,
            },
        );

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_focus = Some(true);
        let overlay = with_interaction_rect(
            make_element(152, overlay_attrs),
            true,
            Rect {
                x: 100.0,
                y: 48.0,
                width: 60.0,
                height: 40.0,
            },
        );

        let mut tree = ElementTree::new();
        tree.root = Some(root_id);
        tree.insert(root);
        tree.insert(host);
        tree.insert(sibling);
        tree.insert(overlay);

        let mut acc = super::RegistryBuildAcc::for_tree(&tree);
        super::accumulate_subtree_rebuild(
            &tree,
            tree.root.as_ref().expect("tree should have a root"),
            &mut acc,
            &[],
            crate::tree::scene::SceneContext::default(),
        );

        let focus_ids: Vec<_> = acc
            .focus_entries
            .iter()
            .map(|entry| entry.element_id.clone())
            .collect();
        assert_eq!(
            focus_ids,
            vec![host_id.clone(), sibling_id.clone(), overlay_id.clone()]
        );

        let focus_state = super::focus_build_state_from_entries(&acc.focus_entries);
        assert_eq!(
            focus_state
                .by_id
                .get(&host_id)
                .and_then(|meta| meta.tab_next.clone()),
            Some(sibling_id.clone())
        );
        assert_eq!(
            focus_state
                .by_id
                .get(&sibling_id)
                .and_then(|meta| meta.tab_next.clone()),
            Some(overlay_id.clone())
        );
    }

    #[test]
    fn registry_for_elements_front_nearby_blocker_suppresses_underlying_mouse_move() {
        let mut host = with_interaction_rect(
            make_element(86, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );
        host.children = vec![ElementId::from_term_bytes(vec![87])];
        host.nearby.set(
            NearbySlot::InFront,
            Some(ElementId::from_term_bytes(vec![88])),
        );

        let mut underlying_attrs = Attrs::default();
        underlying_attrs.on_mouse_move = Some(true);
        let underlying = with_interaction_rect(
            make_element(87, underlying_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );

        let overlay = with_interaction_rect(
            make_element(88, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[host, underlying, overlay]);

        let covered_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 50.0, y: 10.0 });
        assert!(actions_without_cursor(&covered_actions).is_empty());
        assert_eq!(cursor_actions(&covered_actions), vec![CursorIcon::Default]);

        let uncovered_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 120.0, y: 10.0 });
        assert!(matches!(
            actions_without_cursor(&uncovered_actions).as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == ElementId::from_term_bytes(vec![87])
                    && *kind == ElementEventKind::MouseMove
        ));
        assert_eq!(
            cursor_actions(&uncovered_actions),
            vec![CursorIcon::Default]
        );
    }

    #[test]
    fn registry_for_elements_front_nearby_real_move_listener_precedes_root_blocker() {
        let mut host = with_interaction_rect(
            make_element(89, Attrs::default()),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );
        host.children = vec![ElementId::from_term_bytes(vec![90])];
        host.nearby.set(
            NearbySlot::InFront,
            Some(ElementId::from_term_bytes(vec![91])),
        );

        let mut underlying_attrs = Attrs::default();
        underlying_attrs.on_mouse_move = Some(true);
        let underlying = with_interaction_rect(
            make_element(90, underlying_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 150.0,
                height: 40.0,
            },
        );

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_mouse_move = Some(true);
        overlay_attrs.mouse_over = Some(MouseOverAttrs::default());
        let overlay = with_interaction_rect(
            make_element(91, overlay_attrs),
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
        );

        let registry = registry_for_elements(&[host, underlying, overlay]);
        let actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 50.0, y: 10.0 });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                if *element_id == ElementId::from_term_bytes(vec![91])
                    && *kind == ElementEventKind::MouseMove
        ));
    }

    #[test]
    fn registry_for_elements_in_front_overlay_matches_right_of_initial_position() {
        let host_id = ElementId::from_term_bytes(vec![110]);
        let under_id = ElementId::from_term_bytes(vec![111]);
        let overlay_id = ElementId::from_term_bytes(vec![112]);

        let mut host = with_frame(
            make_element(110, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 128.0,
                height: 82.0,
                content_width: 128.0,
                content_height: 82.0,
            },
        );
        host.children = vec![under_id.clone()];
        host.nearby
            .set(NearbySlot::InFront, Some(overlay_id.clone()));

        let mut under_attrs = Attrs::default();
        under_attrs.on_mouse_move = Some(true);
        let underlying = with_frame(
            make_element(111, under_attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 128.0,
                height: 82.0,
                content_width: 128.0,
                content_height: 82.0,
            },
        );

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_mouse_move = Some(true);
        overlay_attrs.mouse_over = Some(MouseOverAttrs::default());
        let overlay = with_frame(
            make_element(112, overlay_attrs),
            Frame {
                x: 6.0,
                y: 0.0,
                width: 126.0,
                height: 82.0,
                content_width: 126.0,
                content_height: 82.0,
            },
        );

        let registry = registry_for_elements(&[host, underlying, overlay]);

        let inside_host_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 110.0, y: 41.0 });
        let overflow_strip_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 130.0, y: 41.0 });

        assert!(matches!(
            inside_host_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                if *element_id == overlay_id && *kind == ElementEventKind::MouseMove
        ));
        assert!(matches!(
            overflow_strip_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                if *element_id == overlay_id && *kind == ElementEventKind::MouseMove
        ));
        assert!(inside_host_actions.iter().all(|action| !matches!(
            action,
            ListenerAction::ElixirEvent(ElixirEvent { element_id, .. }) if *element_id == host_id
        )));
    }

    #[test]
    fn registry_for_elements_in_front_descendant_blocker_does_not_beat_overlay_hover_listener() {
        let overlay_id = ElementId::from_term_bytes(vec![141]);
        let child_id = ElementId::from_term_bytes(vec![142]);

        let mut host = with_frame(
            make_element(140, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 80.0,
                content_width: 160.0,
                content_height: 80.0,
            },
        );
        host.nearby
            .set(NearbySlot::InFront, Some(overlay_id.clone()));

        let mut overlay_attrs = Attrs::default();
        overlay_attrs.on_mouse_move = Some(true);
        overlay_attrs.mouse_over = Some(MouseOverAttrs::default());
        let mut overlay = with_frame(
            make_element(141, overlay_attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 60.0,
                content_width: 120.0,
                content_height: 60.0,
            },
        );
        overlay.children = vec![child_id.clone()];

        let child = with_frame(
            make_element(142, Attrs::default()),
            Frame {
                x: 20.0,
                y: 10.0,
                width: 60.0,
                height: 20.0,
                content_width: 60.0,
                content_height: 20.0,
            },
        );

        let registry = registry_for_elements(&[host, overlay, child]);
        let actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 30.0, y: 15.0 });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                if *element_id == overlay_id && *kind == ElementEventKind::MouseMove
        ));
    }

    #[test]
    fn registry_for_elements_nested_overlay_wrapper_does_not_beat_target_hover_listener() {
        let overlay_id = ElementId::from_term_bytes(vec![150]);
        let wrapper_id = ElementId::from_term_bytes(vec![151]);
        let target_id = ElementId::from_term_bytes(vec![152]);

        let mut host = with_frame(
            make_element(149, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 180.0,
                height: 100.0,
                content_width: 180.0,
                content_height: 100.0,
            },
        );
        host.nearby
            .set(NearbySlot::InFront, Some(overlay_id.clone()));

        let mut overlay = with_frame(
            make_element(150, Attrs::default()),
            Frame {
                x: 20.0,
                y: 10.0,
                width: 140.0,
                height: 70.0,
                content_width: 140.0,
                content_height: 70.0,
            },
        );
        overlay.children = vec![wrapper_id.clone()];

        let mut wrapper = with_frame(
            make_element(151, Attrs::default()),
            Frame {
                x: 30.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
                content_width: 100.0,
                content_height: 40.0,
            },
        );
        wrapper.children = vec![target_id.clone()];

        let mut target_attrs = Attrs::default();
        target_attrs.on_mouse_move = Some(true);
        target_attrs.mouse_over = Some(MouseOverAttrs::default());
        let target = with_frame(
            make_element(152, target_attrs),
            Frame {
                x: 40.0,
                y: 25.0,
                width: 80.0,
                height: 20.0,
                content_width: 80.0,
                content_height: 20.0,
            },
        );

        let registry = registry_for_elements(&[host, overlay, wrapper, target]);
        let actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 50.0, y: 30.0 });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                if *element_id == target_id && *kind == ElementEventKind::MouseMove
        ));
    }

    #[test]
    fn sampled_hit_case_layout_registry_matches_expected_winners() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        assert_registry_probe_matrix(&case, SampledRegistrySource::LayoutOnly);
    }

    #[test]
    fn animated_in_front_overlay_matches_points_right_of_initial_position_after_growth() {
        let initial_registry = animated_width_move_registry_at(0);
        let mid_registry = animated_width_move_registry_at(500);
        let late_registry = animated_width_move_registry_at(1000);

        let initial_inside = first_matching_actions(
            &initial_registry,
            &InputEvent::CursorPos { x: 110.0, y: 41.0 },
        );
        let initial_overflow = first_matching_actions(
            &initial_registry,
            &InputEvent::CursorPos { x: 130.0, y: 41.0 },
        );
        let mid_inside =
            first_matching_actions(&mid_registry, &InputEvent::CursorPos { x: 110.0, y: 41.0 });
        let mid_overflow =
            first_matching_actions(&mid_registry, &InputEvent::CursorPos { x: 130.0, y: 41.0 });
        let late_inside =
            first_matching_actions(&late_registry, &InputEvent::CursorPos { x: 110.0, y: 41.0 });
        let late_overflow =
            first_matching_actions(&late_registry, &InputEvent::CursorPos { x: 130.0, y: 41.0 });

        assert_eq!(cursor_actions(&initial_inside), vec![CursorIcon::Default]);
        assert!(actions_without_cursor(&initial_inside).is_empty());
        assert_eq!(cursor_actions(&initial_overflow), vec![CursorIcon::Default]);
        assert!(actions_without_cursor(&initial_overflow).is_empty());

        for actions in [mid_inside, mid_overflow, late_inside, late_overflow] {
            assert!(matches!(
                actions.as_slice(),
                [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                    if *element_id == ElementId::from_term_bytes(vec![121])
                        && *kind == ElementEventKind::MouseMove
            ));
        }
    }

    #[test]
    fn sampled_hit_case_render_rebuild_registry_matches_expected_winners() {
        let case = AnimatedNearbyHitCase::width_move_in_front();
        assert_registry_probe_matrix(&case, SampledRegistrySource::RenderRebuild);
    }

    #[test]
    fn render_event_rebuild_matches_points_right_of_initial_position_after_growth() {
        let initial_registry = animated_width_move_render_registry_at(0);
        let mid_registry = animated_width_move_render_registry_at(500);
        let late_registry = animated_width_move_render_registry_at(1000);

        let initial_inside = first_matching_actions(
            &initial_registry,
            &InputEvent::CursorPos { x: 110.0, y: 41.0 },
        );
        let initial_overflow = first_matching_actions(
            &initial_registry,
            &InputEvent::CursorPos { x: 130.0, y: 41.0 },
        );
        let mid_inside =
            first_matching_actions(&mid_registry, &InputEvent::CursorPos { x: 110.0, y: 41.0 });
        let mid_overflow =
            first_matching_actions(&mid_registry, &InputEvent::CursorPos { x: 130.0, y: 41.0 });
        let late_inside =
            first_matching_actions(&late_registry, &InputEvent::CursorPos { x: 110.0, y: 41.0 });
        let late_overflow =
            first_matching_actions(&late_registry, &InputEvent::CursorPos { x: 130.0, y: 41.0 });

        assert_eq!(cursor_actions(&initial_inside), vec![CursorIcon::Default]);
        assert!(actions_without_cursor(&initial_inside).is_empty());
        assert_eq!(cursor_actions(&initial_overflow), vec![CursorIcon::Default]);
        assert!(actions_without_cursor(&initial_overflow).is_empty());

        for actions in [mid_inside, mid_overflow, late_inside, late_overflow] {
            assert!(matches!(
                actions.as_slice(),
                [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. }), ..]
                    if *element_id == ElementId::from_term_bytes(vec![123])
                        && *kind == ElementEventKind::MouseMove
            ));
        }
    }

    #[test]
    fn listeners_for_element_on_click_starts_click_and_drag_trackers() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(7, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 2);

        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });
        let matcher_kind = press_listener.matcher.kind();
        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
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
            ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker {
                ref element_id,
                matcher_kind: kind,
                origin_x,
                origin_y,
                ..
            }) if *element_id == ElementId::from_term_bytes(vec![7])
                && kind == matcher_kind
                && origin_x == 10.0
                && origin_y == 10.0
        ));

        let move_actions = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        })
        .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert!(actions_without_cursor(&move_actions).is_empty());
        assert_eq!(cursor_actions(&move_actions), vec![CursorIcon::Pointer]);
    }

    #[test]
    fn listeners_for_scrollable_element_start_drag_tracker_without_click_handlers() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(10.0);
        attrs.scroll_y_max = Some(100.0);
        let element = with_interaction(make_element(70, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 5);

        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });

        let matcher_kind = press_listener.matcher.kind();
        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker {
                element_id,
                matcher_kind: kind,
                origin_x,
                origin_y,
                ..
            })] if element_id == &ElementId::from_term_bytes(vec![70])
                && *kind == matcher_kind
                && *origin_x == 10.0
                && *origin_y == 10.0
        ));
    }

    #[test]
    fn runtime_listeners_for_overlay_orders_runtime_followups_before_release_followup() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(30, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![30]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: true,
                emit_press_pointer: false,
            }),
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![30]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                swipe_handlers: SwipeHandlers::default(),
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };

        let listeners = runtime_listeners_for_overlay(&base, &runtime);
        assert!(listeners.iter().any(|listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorPosDistanceFromPointExceeded {
                    origin_x,
                    origin_y,
                    threshold,
                } if origin_x == 10.0 && origin_y == 10.0 && threshold == 10.0
            )
        }));
        assert!(listeners.iter().any(|listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftReleaseInside { .. }
            )
        }));
        assert!(listeners.iter().any(|listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftReleaseAnywhere
            )
        }));
        assert!(
            listeners
                .iter()
                .any(|listener| { matches!(listener.matcher, ListenerMatcher::WindowCursorLeft) })
        );
    }

    #[test]
    fn compose_combined_registry_click_release_followup_redispatches_base_release() {
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
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::Click,
                    payload: None,
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
            ] if *element_id == ElementId::from_term_bytes(vec![27])
        ));
    }

    #[test]
    fn compose_combined_registry_drops_click_followup_when_source_listener_missing() {
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
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &mut ctx,
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn compose_combined_registry_on_press_release_includes_base_mouse_down_clear() {
        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        attrs.mouse_down = Some(MouseOverAttrs::default());
        attrs.mouse_down_active = Some(true);
        let element = with_interaction(make_element(91, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![91]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: false,
                emit_press_pointer: true,
            }),
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::TreeMsg(TreeMsg::SetMouseDownActive { element_id, active }),
                ListenerAction::ElixirEvent(ElixirEvent { kind, .. }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
            ] if *element_id == ElementId::from_term_bytes(vec![91])
                && !*active
                && *kind == ElementEventKind::Press
        ));
    }

    #[test]
    fn compose_combined_registry_drag_active_release_precedes_and_suppresses_click_followup() {
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
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Active {
                element_id: ElementId::from_term_bytes(vec![29]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                last_x: 10.0,
                last_y: 10.0,
                locked_axis: GestureAxis::Horizontal,
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);

        let actions = first_matching_actions(
            &combined,
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
            ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker)
        ));
        assert!(
            actions
                .iter()
                .all(|action| !matches!(action, ListenerAction::ElixirEvent(_)))
        );
    }

    #[test]
    fn compose_combined_registry_drag_candidate_threshold_without_scroll_match_clears_drag_only() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        let element = with_interaction(make_element(31, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![31]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: true,
                emit_press_pointer: false,
            }),
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![31]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                swipe_handlers: SwipeHandlers::default(),
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 25.0, y: 10.0 },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(
                RuntimeChange::ClearDragTracker
            )]
        ));
    }

    #[test]
    fn compose_combined_registry_drag_candidate_threshold_promotes_drag_when_scroll_matches() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(100.0);
        let element = with_interaction(make_element(31, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: Some(ClickPressTracker {
                element_id: ElementId::from_term_bytes(vec![31]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                emit_click: true,
                emit_press_pointer: false,
            }),
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![31]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                swipe_handlers: SwipeHandlers::default(),
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 25.0, y: 10.0 },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::RuntimeChange(RuntimeChange::PromoteDragTracker {
                    element_id,
                    matcher_kind,
                    locked_axis,
                    ..
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
            ] if *element_id == ElementId::from_term_bytes(vec![31])
                && *matcher_kind == ListenerMatcherKind::CursorButtonLeftPressInside
                && *locked_axis == GestureAxis::Horizontal
        ));
    }

    #[test]
    fn listeners_for_element_on_swipe_starts_drag_tracker_without_click_press_tracker() {
        let mut attrs = Attrs::default();
        attrs.on_swipe_right = Some(true);
        let element = with_interaction(make_element(71, attrs), true);

        let listeners = listeners_for_element(&element);
        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });

        let matcher_kind = press_listener.matcher.kind();
        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker {
                element_id,
                matcher_kind: kind,
                origin_x,
                origin_y,
                swipe_handlers,
            })] if element_id == &ElementId::from_term_bytes(vec![71])
                && *kind == matcher_kind
                && *origin_x == 10.0
                && *origin_y == 10.0
                && !swipe_handlers.up
                && !swipe_handlers.down
                && !swipe_handlers.left
                && swipe_handlers.right
        ));
        assert_eq!(
            cursor_actions(
                &listener_matching(&listeners, |listener| {
                    matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
                })
                .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 })
            ),
            vec![CursorIcon::Pointer]
        );
    }

    #[test]
    fn compose_combined_registry_drag_candidate_threshold_starts_swipe_when_enabled() {
        let mut attrs = Attrs::default();
        attrs.on_swipe_right = Some(true);
        let element = with_interaction(make_element(72, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![72]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                swipe_handlers: SwipeHandlers {
                    right: true,
                    ..SwipeHandlers::default()
                },
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 25.0, y: 10.0 },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                ListenerAction::RuntimeChange(RuntimeChange::StartSwipeTracker { tracker }),
            ] if tracker.element_id == ElementId::from_term_bytes(vec![72])
                && tracker.matcher_kind == ListenerMatcherKind::CursorButtonLeftPressInside
                && (tracker.origin_x - 10.0).abs() < f32::EPSILON
                && (tracker.origin_y - 10.0).abs() < f32::EPSILON
                && tracker.locked_axis == GestureAxis::Horizontal
                && tracker.handlers.right
        ));
    }

    #[test]
    fn compose_combined_registry_drag_candidate_threshold_waits_for_clear_axis_intent() {
        let mut attrs = Attrs::default();
        attrs.on_swipe_right = Some(true);
        let element = with_interaction(make_element(75, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![75]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                swipe_handlers: SwipeHandlers {
                    right: true,
                    ..SwipeHandlers::default()
                },
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let ambiguous = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 28.0, y: 24.0 },
            &mut ctx,
        );

        assert!(ambiguous.is_empty());

        let resolved = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 34.0, y: 16.0 },
            &mut ctx,
        );

        assert!(matches!(
            resolved.as_slice(),
            [
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                ListenerAction::RuntimeChange(RuntimeChange::StartSwipeTracker { tracker }),
            ] if tracker.element_id == ElementId::from_term_bytes(vec![75])
                && tracker.locked_axis == GestureAxis::Horizontal
                && tracker.handlers.right
        ));
    }

    #[test]
    fn compose_combined_registry_drag_candidate_threshold_prefers_horizontal_swipe_over_vertical_parent_scroll()
     {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(10.0);
        parent_attrs.scroll_y_max = Some(100.0);
        let mut parent = with_frame(
            with_interaction(make_element(76, parent_attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 180.0,
                height: 180.0,
                content_width: 180.0,
                content_height: 360.0,
            },
        );
        parent.children = vec![ElementId::from_term_bytes(vec![77])];

        let mut child_attrs = Attrs::default();
        child_attrs.on_swipe_left = Some(true);
        child_attrs.on_swipe_right = Some(true);
        let child = with_frame(
            with_interaction(make_element(77, child_attrs), true),
            Frame {
                x: 20.0,
                y: 20.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 100.0,
            },
        );

        let base = registry_for_elements(&[parent, child]);
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![77]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 40.0,
                origin_y: 40.0,
                swipe_handlers: SwipeHandlers {
                    left: true,
                    right: true,
                    ..SwipeHandlers::default()
                },
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 62.0, y: 48.0 },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
                ListenerAction::RuntimeChange(RuntimeChange::ClearDragTracker),
                ListenerAction::RuntimeChange(RuntimeChange::StartSwipeTracker { tracker }),
            ] if tracker.element_id == ElementId::from_term_bytes(vec![77])
                && tracker.locked_axis == GestureAxis::Horizontal
                && tracker.handlers.left
                && tracker.handlers.right
        ));
    }

    #[test]
    fn compose_combined_registry_drag_candidate_threshold_prefers_vertical_parent_scroll_over_horizontal_swipe()
     {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(10.0);
        parent_attrs.scroll_y_max = Some(100.0);
        let mut parent = with_frame(
            with_interaction(make_element(78, parent_attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 180.0,
                height: 180.0,
                content_width: 180.0,
                content_height: 360.0,
            },
        );
        parent.children = vec![ElementId::from_term_bytes(vec![79])];

        let mut child_attrs = Attrs::default();
        child_attrs.on_swipe_left = Some(true);
        child_attrs.on_swipe_right = Some(true);
        let child = with_frame(
            with_interaction(make_element(79, child_attrs), true),
            Frame {
                x: 20.0,
                y: 20.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 100.0,
            },
        );

        let base = registry_for_elements(&[parent, child]);
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Candidate {
                element_id: ElementId::from_term_bytes(vec![79]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 40.0,
                origin_y: 40.0,
                swipe_handlers: SwipeHandlers {
                    left: true,
                    right: true,
                    ..SwipeHandlers::default()
                },
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 48.0, y: 64.0 },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::RuntimeChange(RuntimeChange::PromoteDragTracker {
                    element_id,
                    matcher_kind,
                    locked_axis,
                    ..
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearClickPressTracker),
            ] if *element_id == ElementId::from_term_bytes(vec![79])
                && *matcher_kind == ListenerMatcherKind::CursorButtonLeftPressInside
                && *locked_axis == GestureAxis::Vertical
        ));
    }

    #[test]
    fn compose_combined_registry_swipe_release_emits_direction_and_base_mouse_up() {
        let mut attrs = Attrs::default();
        attrs.on_swipe_right = Some(true);
        attrs.on_mouse_up = Some(true);
        let element = with_interaction(make_element(73, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: Some(SwipeTracker {
                element_id: ElementId::from_term_bytes(vec![73]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                locked_axis: GestureAxis::Horizontal,
                handlers: SwipeHandlers {
                    right: true,
                    ..SwipeHandlers::default()
                },
            }),
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 35.0,
                y: 10.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent {
                    kind: ElementEventKind::MouseUp,
                    ..
                }),
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::SwipeRight,
                    payload: None,
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearSwipeTracker),
            ] if *element_id == ElementId::from_term_bytes(vec![73])
        ));
    }

    #[test]
    fn compose_combined_registry_swipe_release_uses_locked_axis_even_with_large_off_axis_delta() {
        let mut attrs = Attrs::default();
        attrs.on_swipe_right = Some(true);
        let element = with_interaction(make_element(74, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: Some(SwipeTracker {
                element_id: ElementId::from_term_bytes(vec![74]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                locked_axis: GestureAxis::Horizontal,
                handlers: SwipeHandlers {
                    right: true,
                    ..SwipeHandlers::default()
                },
            }),
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 30.0,
                y: 48.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::SwipeRight,
                    payload: None,
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearSwipeTracker),
            ] if *element_id == ElementId::from_term_bytes(vec![74])
        ));
    }

    #[test]
    fn compose_combined_registry_swipe_release_ignores_short_locked_axis_displacement() {
        let mut attrs = Attrs::default();
        attrs.on_swipe_right = Some(true);
        let element = with_interaction(make_element(80, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: Some(SwipeTracker {
                element_id: ElementId::from_term_bytes(vec![80]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                origin_x: 10.0,
                origin_y: 10.0,
                locked_axis: GestureAxis::Horizontal,
                handlers: SwipeHandlers {
                    right: true,
                    ..SwipeHandlers::default()
                },
            }),
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 18.0,
                y: 40.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(
                RuntimeChange::ClearSwipeTracker
            )]
        ));
    }

    #[test]
    fn listeners_for_element_on_press_starts_pointer_press_and_drag_trackers() {
        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        let element = with_interaction(make_element(8, attrs), true);

        let listeners = listeners_for_element(&element);
        assert_eq!(listeners.len(), 2);

        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });
        let matcher_kind = press_listener.matcher.kind();
        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
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
                ref element_id,
                kind: ElementEventKind::Focus,
                payload: None,
            }) if *element_id == ElementId::from_term_bytes(vec![8])
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::TreeMsg(TreeMsg::SetFocusedActive {
                ref element_id,
                active,
            }) if *element_id == ElementId::from_term_bytes(vec![8]) && active
        ));
        assert!(matches!(
            actions[2],
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
            actions[3],
            ListenerAction::RuntimeChange(RuntimeChange::StartDragTracker {
                ref element_id,
                matcher_kind: kind,
                origin_x,
                origin_y,
                ..
            }) if *element_id == ElementId::from_term_bytes(vec![8])
                && kind == matcher_kind
                && origin_x == 10.0
                && origin_y == 10.0
        ));

        let move_actions = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        })
        .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert!(actions_without_cursor(&move_actions).is_empty());
        assert_eq!(cursor_actions(&move_actions), vec![CursorIcon::Pointer]);
    }

    #[test]
    fn listeners_for_element_on_press_focused_adds_key_enter_listener() {
        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(12, attrs), true);

        let listeners = listeners_for_element(&element);
        let key_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::KeyEnterPressNoCtrlAltMeta
            )
        });

        let actions = key_listener.compute_actions(&InputEvent::Key {
            key: CanonicalKey::Enter,
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
        assert!(listeners.iter().all(|listener| !matches!(
            listener.matcher,
            ListenerMatcher::KeyEnterPressNoCtrlAltMeta
        )));
    }

    #[test]
    fn listeners_for_element_virtual_key_starts_tracker_and_never_focuses() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.virtual_key = Some(VirtualKeySpec {
            tap: VirtualKeyTapAction::Text("a".to_string()),
            hold: VirtualKeyHoldMode::None,
            hold_ms: 350,
            repeat_ms: 40,
        });
        let element = with_interaction(make_element(73, attrs), true);

        let listeners = listeners_for_element(&element);
        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });
        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::StartVirtualKeyTracker { tracker })]
                if tracker.element_id == ElementId::from_term_bytes(vec![73])
                    && tracker.phase == VirtualKeyPhase::Armed
        ));

        let move_actions = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        })
        .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 });

        assert_eq!(actions_without_cursor(&move_actions).len(), 0);
        assert_eq!(cursor_actions(&move_actions), vec![CursorIcon::Pointer]);
    }

    #[test]
    fn runtime_listeners_for_overlay_virtual_key_release_dispatches_synthetic_input() {
        let base = registry_for_elements(&[]);
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: Some(VirtualKeyTracker {
                element_id: ElementId::from_term_bytes(vec![74]),
                region: PointerRegion {
                    visible: true,
                    local_shape: ShapeBounds {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 100.0,
                            height: 40.0,
                        },
                        radii: None,
                    },
                    screen_to_local: Some(Affine2::identity()),
                    screen_bounds: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 40.0,
                    },
                    clip_chain: Vec::new(),
                },
                tap: VirtualKeyTapAction::Text("a".to_string()),
                hold: VirtualKeyHoldMode::None,
                hold_ms: 350,
                repeat_ms: 40,
                phase: VirtualKeyPhase::Armed,
            }),
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };

        let listeners = runtime_listeners_for_overlay(&base, &runtime);
        assert!(listeners.iter().any(|listener| matches!(
            listener.matcher,
            ListenerMatcher::CursorButtonLeftReleaseInside { .. }
        )));
        assert!(listeners.iter().any(|listener| matches!(
            listener.matcher,
            ListenerMatcher::CursorLocationLeaveBoundary { .. }
        )));

        let release_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftReleaseInside { .. }
            )
        });
        let actions = release_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::SyntheticInput(events),
                ListenerAction::RuntimeChange(RuntimeChange::ClearVirtualKeyTracker),
            ] if matches!(events.as_slice(), [InputEvent::TextCommit { text, mods }] if text == "a" && *mods == 0)
        ));
    }

    #[test]
    fn listeners_for_element_focused_key_bindings_emit_key_events() {
        let mut attrs = Attrs::default();
        attrs.focused_active = Some(true);
        attrs.on_key_down = Some(vec![KeyBindingSpec {
            route: "key_down:enter:exact:0".to_string(),
            key: CanonicalKey::Enter,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }]);
        attrs.on_key_up = Some(vec![KeyBindingSpec {
            route: "key_up:escape:exact:2".to_string(),
            key: CanonicalKey::Escape,
            mods: MOD_CTRL,
            match_mode: KeyBindingMatch::Exact,
        }]);
        let element = with_interaction(make_element(37, attrs), true);

        let listeners = listeners_for_element(&element);

        let key_down_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::KeyDownBinding {
                    key: CanonicalKey::Enter,
                    mods: 0,
                    match_mode: KeyBindingMatch::Exact,
                }
            )
        });

        let key_down_actions = key_down_listener.compute_actions(&InputEvent::Key {
            key: CanonicalKey::Enter,
            action: ACTION_PRESS,
            mods: 0,
        });

        assert!(matches!(
            key_down_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                element_id,
                kind: ElementEventKind::KeyDown,
                payload,
            })] if *element_id == ElementId::from_term_bytes(vec![37])
                && payload.as_deref() == Some("key_down:enter:exact:0")
        ));

        let key_up_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::KeyUpBinding {
                    key: CanonicalKey::Escape,
                    mods: MOD_CTRL,
                    match_mode: KeyBindingMatch::Exact,
                }
            )
        });

        let key_up_actions = key_up_listener.compute_actions(&InputEvent::Key {
            key: CanonicalKey::Escape,
            action: ACTION_RELEASE,
            mods: MOD_CTRL,
        });

        assert!(matches!(
            key_up_actions.as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent {
                element_id,
                kind: ElementEventKind::KeyUp,
                payload,
            })] if *element_id == ElementId::from_term_bytes(vec![37])
                && payload.as_deref() == Some("key_up:escape:exact:2")
        ));
    }

    #[test]
    fn listeners_for_element_unfocused_key_bindings_are_omitted() {
        let mut attrs = Attrs::default();
        attrs.focused_active = Some(false);
        attrs.on_key_down = Some(vec![KeyBindingSpec {
            route: "key_down:enter:exact:0".to_string(),
            key: CanonicalKey::Enter,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }]);
        let element = with_interaction(make_element(38, attrs), true);

        let listeners = listeners_for_element(&element);
        assert!(listeners.iter().all(|listener| {
            !matches!(listener.matcher, ListenerMatcher::KeyDownBinding { .. })
        }));
    }

    #[test]
    fn listeners_for_element_key_down_and_key_press_share_one_slot() {
        let mut attrs = Attrs::default();
        attrs.focused_active = Some(true);
        attrs.on_key_down = Some(vec![KeyBindingSpec {
            route: "key_down:space:exact:0".to_string(),
            key: CanonicalKey::Space,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }]);
        attrs.on_key_press = Some(vec![KeyBindingSpec {
            route: "key_press:space:exact:0".to_string(),
            key: CanonicalKey::Space,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }]);
        let element = with_interaction(make_element(39, attrs), true);

        let listeners = listeners_for_element(&element);
        let listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::KeyDownBinding {
                    key: CanonicalKey::Space,
                    mods: 0,
                    match_mode: KeyBindingMatch::Exact,
                }
            )
        });

        let actions = listener.compute_actions(&InputEvent::Key {
            key: CanonicalKey::Space,
            action: ACTION_PRESS,
            mods: 0,
        });

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent {
                    element_id,
                    kind: ElementEventKind::KeyDown,
                    payload,
                }),
                ListenerAction::RuntimeChange(RuntimeChange::StartKeyPressTracker { tracker }),
            ] if *element_id == ElementId::from_term_bytes(vec![39])
                && payload.as_deref() == Some("key_down:space:exact:0")
                && tracker.key == CanonicalKey::Space
                && tracker.source_element_id == Some(ElementId::from_term_bytes(vec![39]))
                && matches!(
                    tracker.followups.as_slice(),
                    [KeyPressFollowup::ElixirEvent { route, .. }]
                        if route == "key_press:space:exact:0"
                )
        ));
    }

    #[test]
    fn key_press_release_followup_redispatches_key_up_before_key_press() {
        let mut attrs = Attrs::default();
        attrs.focused_active = Some(true);
        attrs.on_key_up = Some(vec![KeyBindingSpec {
            route: "key_up:space:exact:0".to_string(),
            key: CanonicalKey::Space,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }]);
        attrs.on_key_press = Some(vec![KeyBindingSpec {
            route: "key_press:space:exact:0".to_string(),
            key: CanonicalKey::Space,
            mods: 0,
            match_mode: KeyBindingMatch::Exact,
        }]);
        let element = with_interaction(make_element(40, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: vec![KeyPressTracker {
                source_element_id: Some(ElementId::from_term_bytes(vec![40])),
                key: CanonicalKey::Space,
                mods: 0,
                match_mode: KeyBindingMatch::Exact,
                followups: vec![KeyPressFollowup::ElixirEvent {
                    element_id: ElementId::from_term_bytes(vec![40]),
                    route: "key_press:space:exact:0".to_string(),
                }],
            }],
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            combined_registry: Some(combined.clone()),
            ..Default::default()
        };

        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::Key {
                key: CanonicalKey::Space,
                action: ACTION_RELEASE,
                mods: 0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent {
                    kind: ElementEventKind::KeyUp,
                    payload: key_up_route,
                    ..
                }),
                ListenerAction::ElixirEvent(ElixirEvent {
                    kind: ElementEventKind::KeyPress,
                    payload: key_press_route,
                    ..
                }),
                ListenerAction::RuntimeChange(RuntimeChange::ClearKeyPressTrackersForKey { key }),
            ] if key_up_route.as_deref() == Some("key_up:space:exact:0")
                && key_press_route.as_deref() == Some("key_press:space:exact:0")
                && *key == CanonicalKey::Space
        ));
    }

    #[test]
    fn listeners_for_focusable_pointer_press_emits_focus_to() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.focused_active = Some(false);
        let element = with_interaction(make_element(24, attrs), true);

        let listeners = listeners_for_element(&element);
        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });
        let actions = press_listener.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        });

        assert!(actions.iter().any(|action| {
            matches!(
                action,
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id, active })
                    if element_id == &ElementId::from_term_bytes(vec![24]) && *active
            )
        }));
    }

    #[test]
    fn registry_for_elements_adds_concrete_tab_focus_transitions() {
        let mut focused_attrs = Attrs::default();
        focused_attrs.on_focus = Some(true);
        focused_attrs.focused_active = Some(true);
        let focused = with_interaction(make_element(25, focused_attrs), true);

        let mut next_attrs = Attrs::default();
        next_attrs.on_focus = Some(true);
        let next = with_interaction(make_element(26, next_attrs), true);

        let registry = registry_for_elements(&[focused, next]);
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![25])),
            ..Default::default()
        };
        let forward_actions = first_matching_actions_with_ctx(
            &registry,
            &InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: 0,
            },
            &mut ctx,
        );
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![25])),
            ..Default::default()
        };
        let reverse_actions = first_matching_actions_with_ctx(
            &registry,
            &InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            },
            &mut ctx,
        );

        assert!(matches!(
            forward_actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent { element_id: previous, kind: ElementEventKind::Blur, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: previous_tree, active: false }),
                ListenerAction::ElixirEvent(ElixirEvent { element_id: next, kind: ElementEventKind::Focus, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: next_tree, active: true }),
            ] if *previous == ElementId::from_term_bytes(vec![25])
                && *previous_tree == ElementId::from_term_bytes(vec![25])
                && *next == ElementId::from_term_bytes(vec![26])
                && *next_tree == ElementId::from_term_bytes(vec![26])
        ));
        assert!(matches!(
            reverse_actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent { element_id: previous, kind: ElementEventKind::Blur, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: previous_tree, active: false }),
                ListenerAction::ElixirEvent(ElixirEvent { element_id: next, kind: ElementEventKind::Focus, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: next_tree, active: true }),
            ] if *previous == ElementId::from_term_bytes(vec![25])
                && *previous_tree == ElementId::from_term_bytes(vec![25])
                && *next == ElementId::from_term_bytes(vec![26])
                && *next_tree == ElementId::from_term_bytes(vec![26])
        ));
    }

    #[test]
    fn rebuild_payload_focus_on_mount_ignores_existing_node_when_attr_is_added_later() {
        let root_id = ElementId::from_term_bytes(vec![27]);
        let field_id = ElementId::from_term_bytes(vec![28]);

        let mut tree = ElementTree::new();
        tree.set_revision(2);

        let mut root = with_frame(
            make_element(27, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 80.0,
                content_width: 160.0,
                content_height: 80.0,
            },
        );
        root.mounted_at_revision = 1;
        root.children = vec![field_id.clone()];

        let mut field_attrs = Attrs::default();
        field_attrs.focus_on_mount = Some(true);
        let mut field = with_frame(
            make_text_input_element(28, field_attrs),
            Frame {
                x: 12.0,
                y: 10.0,
                width: 80.0,
                height: 24.0,
                content_width: 80.0,
                content_height: 24.0,
            },
        );
        field.mounted_at_revision = 1;

        tree.root = Some(root_id);
        tree.insert(root);
        tree.insert(field);

        let rebuild = rebuild_payload_for_tree(&tree);

        assert!(
            rebuild.focus_on_mount.is_none(),
            "existing retained nodes should not autofocus when the attr is toggled on later"
        );
    }

    #[test]
    fn rebuild_payload_focus_on_mount_prefers_newly_mounted_target() {
        let root_id = ElementId::from_term_bytes(vec![29]);
        let existing_id = ElementId::from_term_bytes(vec![30]);
        let new_id = ElementId::from_term_bytes(vec![31]);

        let mut tree = ElementTree::new();
        tree.set_revision(2);

        let mut root = with_frame(
            make_element(29, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 180.0,
                height: 90.0,
                content_width: 180.0,
                content_height: 90.0,
            },
        );
        root.mounted_at_revision = 1;
        root.children = vec![existing_id.clone(), new_id.clone()];

        let mut existing_attrs = Attrs::default();
        existing_attrs.focus_on_mount = Some(true);
        let mut existing = with_frame(
            make_text_input_element(30, existing_attrs),
            Frame {
                x: 10.0,
                y: 10.0,
                width: 70.0,
                height: 24.0,
                content_width: 70.0,
                content_height: 24.0,
            },
        );
        existing.mounted_at_revision = 1;

        let mut new_attrs = Attrs::default();
        new_attrs.focus_on_mount = Some(true);
        let mut new_field = with_frame(
            make_text_input_element(31, new_attrs),
            Frame {
                x: 10.0,
                y: 44.0,
                width: 70.0,
                height: 24.0,
                content_width: 70.0,
                content_height: 24.0,
            },
        );
        new_field.mounted_at_revision = 2;

        tree.root = Some(root_id);
        tree.insert(root);
        tree.insert(existing);
        tree.insert(new_field);

        let rebuild = rebuild_payload_for_tree(&tree);

        assert!(matches!(
            rebuild.focus_on_mount.as_ref(),
            Some(target)
                if target.element_id == new_id && target.mounted_at_revision == 2
        ));
    }

    #[test]
    fn rebuild_payload_focus_on_mount_keeps_first_candidate_in_same_revision() {
        let root_id = ElementId::from_term_bytes(vec![32]);
        let first_id = ElementId::from_term_bytes(vec![33]);
        let second_id = ElementId::from_term_bytes(vec![34]);

        let mut tree = ElementTree::new();
        tree.set_revision(3);

        let mut root = with_frame(
            make_element(32, Attrs::default()),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
                content_width: 200.0,
                content_height: 100.0,
            },
        );
        root.mounted_at_revision = 1;
        root.children = vec![first_id.clone(), second_id.clone()];

        let mut first_attrs = Attrs::default();
        first_attrs.focus_on_mount = Some(true);
        let mut first = with_frame(
            make_text_input_element(33, first_attrs),
            Frame {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 24.0,
                content_width: 80.0,
                content_height: 24.0,
            },
        );
        first.mounted_at_revision = 3;

        let mut second_attrs = Attrs::default();
        second_attrs.focus_on_mount = Some(true);
        let mut second = with_frame(
            make_text_input_element(34, second_attrs),
            Frame {
                x: 10.0,
                y: 44.0,
                width: 80.0,
                height: 24.0,
                content_width: 80.0,
                content_height: 24.0,
            },
        );
        second.mounted_at_revision = 3;

        tree.root = Some(root_id);
        tree.insert(root);
        tree.insert(first);
        tree.insert(second);

        let rebuild = rebuild_payload_for_tree(&tree);

        assert!(matches!(
            rebuild.focus_on_mount.as_ref(),
            Some(target)
                if target.element_id == first_id && target.mounted_at_revision == 3
        ));
    }

    #[test]
    fn registry_for_elements_without_focus_adds_global_tab_fallbacks() {
        let mut first_attrs = Attrs::default();
        first_attrs.on_focus = Some(true);
        let first = with_interaction(make_element(27, first_attrs), true);

        let mut last_attrs = Attrs::default();
        last_attrs.on_focus = Some(true);
        let last = with_interaction(make_element(28, last_attrs), true);

        let registry = registry_for_elements(&[first, last]);
        let mut ctx = TestComputeCtx::default();
        let forward_actions = first_matching_actions_with_ctx(
            &registry,
            &InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: 0,
            },
            &mut ctx,
        );
        let mut ctx = TestComputeCtx::default();
        let reverse_actions = first_matching_actions_with_ctx(
            &registry,
            &InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            },
            &mut ctx,
        );

        assert!(matches!(
            forward_actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent { element_id, kind: ElementEventKind::Focus, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: tree_id, active: true }),
            ] if *element_id == ElementId::from_term_bytes(vec![27])
                && *tree_id == ElementId::from_term_bytes(vec![27])
        ));
        assert!(matches!(
            reverse_actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent { element_id, kind: ElementEventKind::Focus, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: tree_id, active: true }),
            ] if *element_id == ElementId::from_term_bytes(vec![28])
                && *tree_id == ElementId::from_term_bytes(vec![28])
        ));
    }

    #[test]
    fn key_enter_press_matcher_blocks_ctrl_alt_meta_and_allows_shift() {
        let matcher = ListenerMatcher::KeyEnterPressNoCtrlAltMeta;

        assert!(matcher.matches(&InputEvent::Key {
            key: CanonicalKey::Enter,
            action: ACTION_PRESS,
            mods: 0,
        }));
        assert!(matcher.matches(&InputEvent::Key {
            key: CanonicalKey::Enter,
            action: ACTION_PRESS,
            mods: crate::input::MOD_SHIFT,
        }));

        assert!(!matcher.matches(&InputEvent::Key {
            key: CanonicalKey::Enter,
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        }));
        assert!(!matcher.matches(&InputEvent::Key {
            key: CanonicalKey::Enter,
            action: ACTION_PRESS,
            mods: MOD_ALT,
        }));
        assert!(!matcher.matches(&InputEvent::Key {
            key: CanonicalKey::Enter,
            action: ACTION_PRESS,
            mods: MOD_META,
        }));
    }

    #[test]
    fn tab_matchers_enforce_expected_modifier_behavior() {
        assert!(
            ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta.matches(&InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: 0,
            })
        );
        assert!(
            !ListenerMatcher::KeyTabPressNoShiftCtrlAltMeta.matches(&InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            })
        );

        assert!(
            ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta.matches(&InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            })
        );
        assert!(
            !ListenerMatcher::KeyShiftTabPressNoCtrlAltMeta.matches(&InputEvent::Key {
                key: CanonicalKey::Tab,
                action: ACTION_PRESS,
                mods: MOD_SHIFT | MOD_CTRL,
            })
        );
    }

    #[test]
    fn key_x_and_v_matchers_require_ctrl_or_meta() {
        assert!(
            ListenerMatcher::KeyXPressCtrlOrMeta.matches(&InputEvent::Key {
                key: CanonicalKey::X,
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            })
        );
        assert!(
            ListenerMatcher::KeyXPressCtrlOrMeta.matches(&InputEvent::Key {
                key: CanonicalKey::X,
                action: ACTION_PRESS,
                mods: MOD_META | MOD_ALT,
            })
        );
        assert!(
            !ListenerMatcher::KeyXPressCtrlOrMeta.matches(&InputEvent::Key {
                key: CanonicalKey::X,
                action: ACTION_PRESS,
                mods: 0,
            })
        );

        assert!(
            ListenerMatcher::KeyVPressCtrlOrMeta.matches(&InputEvent::Key {
                key: CanonicalKey::V,
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            })
        );
        assert!(
            ListenerMatcher::KeyVPressCtrlOrMeta.matches(&InputEvent::Key {
                key: CanonicalKey::V,
                action: ACTION_PRESS,
                mods: MOD_META,
            })
        );
        assert!(
            !ListenerMatcher::KeyVPressCtrlOrMeta.matches(&InputEvent::Key {
                key: CanonicalKey::V,
                action: ACTION_PRESS,
                mods: 0,
            })
        );
    }

    #[test]
    fn middle_press_inside_matcher_requires_middle_press_inside_rect() {
        let region = build_pointer_region(true);
        let matcher = ListenerMatcher::CursorButtonMiddlePressInside { region };

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
                key: CanonicalKey::Backspace,
                action: ACTION_PRESS,
                mods: 0,
            })
        );
        assert!(
            !ListenerMatcher::KeyBackspacePress.matches(&InputEvent::Key {
                key: CanonicalKey::Delete,
                action: ACTION_PRESS,
                mods: 0,
            })
        );

        assert!(ListenerMatcher::KeyDeletePress.matches(&InputEvent::Key {
            key: CanonicalKey::Delete,
            action: ACTION_PRESS,
            mods: 0,
        }));
        assert!(!ListenerMatcher::KeyDeletePress.matches(&InputEvent::Key {
            key: CanonicalKey::Backspace,
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
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::TextCommitNoCtrlMeta))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::KeyBackspacePress))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::KeyDeletePress))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::KeyXPressCtrlOrMeta))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::KeyVPressCtrlOrMeta))
        );
        assert!(listeners.iter().any(|listener| matches!(
            listener.matcher,
            ListenerMatcher::KeyLeftPressNoCtrlAltMeta
        )));
        assert!(listeners.iter().any(|listener| matches!(
            listener.matcher,
            ListenerMatcher::KeyRightPressNoCtrlAltMeta
        )));
        assert!(listeners.iter().any(|listener| matches!(
            listener.matcher,
            ListenerMatcher::KeyHomePressNoCtrlAltMeta
        )));
        assert!(
            listeners.iter().any(|listener| matches!(
                listener.matcher,
                ListenerMatcher::KeyEndPressNoCtrlAltMeta
            ))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::KeyAPressCtrlOrMeta))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::KeyCPressCtrlOrMeta))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::TextPreeditAny))
        );
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::TextPreeditClear))
        );

        let commit_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::TextCommitNoCtrlMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, None, true, true),
            )]),
            ..Default::default()
        };
        let commit_actions = commit_listener.compute_actions_with_ctx(
            &InputEvent::TextCommit {
                text: "x".to_string(),
                mods: 0,
            },
            &mut ctx,
        );
        assert_eq!(commit_actions.len(), 5);
        assert!(commit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputContent { element_id, content })
                if *element_id == ElementId::from_term_bytes(vec![17]) && content == "abx"
        )));
        assert!(commit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::ExpectTextInputPatchValue {
                element_id,
                content,
            }) if *element_id == ElementId::from_term_bytes(vec![17]) && content == "abx"
        )));
        assert!(commit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![17]) && state.content == "abx"
        )));
        assert!(commit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![17]) && state.cursor == 3
        )));

        let cut_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyXPressCtrlOrMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, Some(0), true, true),
            )]),
            ..Default::default()
        };
        let cut_actions = cut_listener.compute_actions_with_ctx(
            &InputEvent::Key {
                key: CanonicalKey::X,
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            },
            &mut ctx,
        );
        assert_eq!(cut_actions.len(), 7);
        assert!(cut_actions.iter().any(|action| matches!(
            action,
            ListenerAction::ClipboardWrite { target: ClipboardTarget::Clipboard, text }
                if text == "ab"
        )));
        assert!(cut_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputContent { element_id, content })
                if *element_id == ElementId::from_term_bytes(vec![17]) && content.is_empty()
        )));
        assert!(cut_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![17]) && state.content.is_empty()
        )));
        assert!(cut_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::ExpectTextInputPatchValue {
                element_id,
                content,
            }) if *element_id == ElementId::from_term_bytes(vec![17]) && content.is_empty()
        )));
        assert!(cut_actions.iter().any(|action| matches!(
            action,
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::Change,
                ..
            })
        )));

        let paste_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyVPressCtrlOrMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, None, true, true),
            )]),
            clipboard: HashMap::from([(ClipboardTarget::Clipboard, Some("zz".to_string()))]),
            ..Default::default()
        };
        let paste_actions = paste_listener.compute_actions_with_ctx(
            &InputEvent::Key {
                key: CanonicalKey::V,
                action: ACTION_PRESS,
                mods: MOD_META,
            },
            &mut ctx,
        );
        assert_eq!(paste_actions.len(), 5);
        assert!(paste_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputContent { element_id, content })
                if *element_id == ElementId::from_term_bytes(vec![17]) && content == "abzz"
        )));
        assert!(paste_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::ExpectTextInputPatchValue {
                element_id,
                content,
            }) if *element_id == ElementId::from_term_bytes(vec![17]) && content == "abzz"
        )));
        assert!(paste_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![17]) && state.content == "abzz"
        )));
        assert!(paste_actions.iter().any(|action| matches!(
            action,
            ListenerAction::ElixirEvent(ElixirEvent {
                kind: ElementEventKind::Change,
                ..
            })
        )));

        let left_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyLeftPressNoCtrlAltMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, None, true, true),
            )]),
            ..Default::default()
        };
        let left_actions = left_listener.compute_actions_with_ctx(
            &InputEvent::Key {
                key: CanonicalKey::ArrowLeft,
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
            },
            &mut ctx,
        );
        assert_eq!(left_actions.len(), 3);
        assert!(left_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime { element_id, selection_anchor, .. })
                if *element_id == ElementId::from_term_bytes(vec![17])
                    && *selection_anchor == Some(2)
        )));
        assert!(left_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![17])
                    && state.selection_anchor == Some(2)
        )));
        assert!(left_actions.iter().any(|action| matches!(
            action,
            ListenerAction::ClipboardWrite { target: ClipboardTarget::Primary, text }
                if text == "b"
        )));

        let select_all_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyAPressCtrlOrMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, None, true, true),
            )]),
            ..Default::default()
        };
        let select_all_actions = select_all_listener.compute_actions_with_ctx(
            &InputEvent::Key {
                key: CanonicalKey::A,
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            },
            &mut ctx,
        );
        assert_eq!(select_all_actions.len(), 3);
        assert!(select_all_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime { element_id, selection_anchor, .. })
                if *element_id == ElementId::from_term_bytes(vec![17])
                    && *selection_anchor == Some(0)
        )));
        assert!(select_all_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![17])
                    && state.selection_anchor == Some(0)
        )));
        assert!(select_all_actions.iter().any(|action| matches!(
            action,
            ListenerAction::ClipboardWrite { target: ClipboardTarget::Primary, text }
                if text == "ab"
        )));

        let copy_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyCPressCtrlOrMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, Some(0), true, true),
            )]),
            ..Default::default()
        };
        let copy_actions = copy_listener.compute_actions_with_ctx(
            &InputEvent::Key {
                key: CanonicalKey::C,
                action: ACTION_PRESS,
                mods: MOD_META,
            },
            &mut ctx,
        );
        assert!(matches!(
            copy_actions.as_slice(),
            [
                ListenerAction::ClipboardWrite { target: ClipboardTarget::Clipboard, text },
                ListenerAction::ClipboardWrite { target: ClipboardTarget::Primary, text: primary },
            ] if text == "ab" && primary == "ab"
        ));

        let preedit_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::TextPreeditAny)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![17])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![17]),
                make_text_input_state("ab", 2, None, true, true),
            )]),
            ..Default::default()
        };
        let preedit_actions = preedit_listener.compute_actions_with_ctx(
            &InputEvent::TextPreedit {
                text: "xy".to_string(),
                cursor: Some((1, 1)),
            },
            &mut ctx,
        );
        assert_eq!(preedit_actions.len(), 2);
        assert!(preedit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime {
                element_id,
                preedit,
                preedit_cursor,
                ..
            }) if *element_id == ElementId::from_term_bytes(vec![17])
                && preedit.as_deref() == Some("xy")
                && *preedit_cursor == Some((1, 1))
        )));
        assert!(preedit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState {
                element_id,
                state,
            }) if *element_id == ElementId::from_term_bytes(vec![17])
                && state.preedit.as_deref() == Some("xy")
                && state.preedit_cursor == Some((1, 1))
        )));
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
        let commit_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::TextCommitNoCtrlMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![18])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![18]),
                make_text_input_state("ab", 2, None, true, false),
            )]),
            ..Default::default()
        };
        let commit_actions = commit_listener.compute_actions_with_ctx(
            &InputEvent::TextCommit {
                text: "x".to_string(),
                mods: 0,
            },
            &mut ctx,
        );

        assert_eq!(commit_actions.len(), 3);
        assert!(
            commit_actions
                .iter()
                .all(|action| !matches!(action, ListenerAction::ElixirEvent(_)))
        );
        assert!(commit_actions.iter().any(|action| matches!(
            action,
            ListenerAction::RuntimeChange(RuntimeChange::SetTextInputState { element_id, state })
                if *element_id == ElementId::from_term_bytes(vec![18]) && state.content == "abx"
        )));

        let cut_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyXPressCtrlOrMeta)
        });
        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![18])),
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![18]),
                make_text_input_state("ab", 2, Some(0), true, false),
            )]),
            ..Default::default()
        };
        let cut_actions = cut_listener.compute_actions_with_ctx(
            &InputEvent::Key {
                key: CanonicalKey::X,
                action: ACTION_PRESS,
                mods: MOD_CTRL,
            },
            &mut ctx,
        );
        assert!(
            cut_actions
                .iter()
                .all(|action| !matches!(action, ListenerAction::ElixirEvent(_)))
        );
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
        assert!(listeners.iter().all(|listener| {
            !matches!(
                listener.matcher,
                ListenerMatcher::TextCommitNoCtrlMeta
                    | ListenerMatcher::KeyBackspacePress
                    | ListenerMatcher::KeyDeletePress
                    | ListenerMatcher::KeyLeftPressNoCtrlAltMeta
                    | ListenerMatcher::KeyRightPressNoCtrlAltMeta
                    | ListenerMatcher::KeyHomePressNoCtrlAltMeta
                    | ListenerMatcher::KeyEndPressNoCtrlAltMeta
                    | ListenerMatcher::KeyAPressCtrlOrMeta
                    | ListenerMatcher::KeyCPressCtrlOrMeta
                    | ListenerMatcher::KeyXPressCtrlOrMeta
                    | ListenerMatcher::KeyVPressCtrlOrMeta
                    | ListenerMatcher::TextPreeditAny
                    | ListenerMatcher::TextPreeditClear
            )
        }));
    }

    #[test]
    fn listeners_for_text_input_left_press_sets_cursor_and_starts_text_drag() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(false);
        let element = with_interaction(make_text_input_element(32, attrs), true);

        let listeners = listeners_for_element(&element);
        let press_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonLeftPressInside { .. }
            )
        });

        let mut ctx = TestComputeCtx {
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![32]),
                make_text_input_state("ab", 0, None, false, false),
            )]),
            ..Default::default()
        };
        let actions = press_listener.compute_actions_with_ctx(
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: MOD_SHIFT,
                x: 24.0,
                y: 10.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent { ref element_id, kind: ElementEventKind::Focus, .. })
                if *element_id == ElementId::from_term_bytes(vec![32])
        ));
        assert!(matches!(
            actions[1],
            ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { ref element_id, active })
                if *element_id == ElementId::from_term_bytes(vec![32]) && active
        ));
        assert!(actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime { element_id, .. })
                if *element_id == ElementId::from_term_bytes(vec![32])
        )));
        assert!(matches!(
            actions.last().expect("text drag action"),
            ListenerAction::RuntimeChange(RuntimeChange::StartTextDragTracker {
                element_id,
                matcher_kind,
            }) if *element_id == ElementId::from_term_bytes(vec![32])
                && *matcher_kind == ListenerMatcherKind::CursorButtonLeftPressInside
        ));
    }

    #[test]
    fn listeners_for_text_input_with_interaction_add_middle_paste_primary_listener() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        attrs.text_input_focused = Some(false);
        attrs.on_change = Some(true);
        let element = with_interaction(make_text_input_element(21, attrs), true);

        let listeners = listeners_for_element(&element);
        let middle_listener = listener_matching(&listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorButtonMiddlePressInside { .. }
            )
        });

        let mut ctx = TestComputeCtx {
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![21]),
                make_text_input_state("ab", 0, None, false, true),
            )]),
            clipboard: HashMap::from([(ClipboardTarget::Primary, Some("zz".to_string()))]),
            ..Default::default()
        };
        let actions = middle_listener.compute_actions_with_ctx(
            &InputEvent::CursorButton {
                button: "middle".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            &mut ctx,
        );
        assert!(actions.len() >= 4);
        assert!(matches!(
            actions[0],
            ListenerAction::ElixirEvent(ElixirEvent { ref element_id, kind: ElementEventKind::Focus, .. })
                if *element_id == ElementId::from_term_bytes(vec![21])
        ));
        assert!(actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputContent { element_id, .. })
                if *element_id == ElementId::from_term_bytes(vec![21])
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime { element_id, .. })
                if *element_id == ElementId::from_term_bytes(vec![21])
        )));
    }

    #[test]
    fn runtime_listeners_for_overlay_text_drag_adds_move_and_clear_followups() {
        let mut attrs = Attrs::default();
        attrs.content = Some("ab".to_string());
        let element = with_interaction(make_text_input_element(33, attrs), true);
        let base = registry_for_elements(&[element]);

        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: Some(TextDragTracker {
                element_id: ElementId::from_term_bytes(vec![33]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
            }),
        };

        let listeners = runtime_listeners_for_overlay(&base, &runtime);
        assert_eq!(listeners.len(), 5);
        assert!(matches!(
            listeners[0].matcher,
            ListenerMatcher::RawPointerLifecycle
        ));
        assert!(matches!(
            listeners[1].matcher,
            ListenerMatcher::CursorButtonLeftReleaseAnywhere
        ));
        assert!(matches!(
            listeners[2].matcher,
            ListenerMatcher::CursorPosAnywhere
        ));
        assert!(matches!(
            listeners[3].matcher,
            ListenerMatcher::WindowBlurred
        ));
        assert!(matches!(
            listeners[4].matcher,
            ListenerMatcher::WindowCursorLeft
        ));

        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            text_inputs: HashMap::from([(
                ElementId::from_term_bytes(vec![33]),
                make_text_input_state("ab", 0, None, true, false),
            )]),
            ..Default::default()
        };
        let move_actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 18.0, y: 9.0 },
            &mut ctx,
        );
        assert!(matches!(
            move_actions.first(),
            Some(ListenerAction::TreeMsg(TreeMsg::SetTextInputRuntime { element_id, .. }))
                if *element_id == ElementId::from_term_bytes(vec![33])
        ));
    }

    #[test]
    fn runtime_listeners_for_overlay_drops_text_drag_followups_when_source_listener_missing() {
        let base = registry_for_elements(&[]);
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: None,
            text_drag: Some(TextDragTracker {
                element_id: ElementId::from_term_bytes(vec![34]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
            }),
        };

        let listeners = runtime_listeners_for_overlay(&base, &runtime);
        assert_eq!(listeners.len(), 1);
        assert!(matches!(
            listeners[0].matcher,
            ListenerMatcher::RawPointerLifecycle
        ));
    }

    #[test]
    fn window_listeners_emit_resize_tree_message() {
        let listeners = window_listeners();
        assert_eq!(listeners.len(), 2);
        let resize_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::WindowResized)
        });
        let cursor_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosAnywhere)
        });

        let actions = resize_listener.compute_actions(&InputEvent::Resized {
            width: 800,
            height: 600,
            scale_factor: 1.5,
        });
        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::Resize { width, height, scale })]
                if (*width - 800.0).abs() < f32::EPSILON
                    && (*height - 600.0).abs() < f32::EPSILON
                    && (*scale - 1.5).abs() < f32::EPSILON
        ));
        assert!(matches!(
            cursor_listener
                .compute_actions(&InputEvent::CursorPos { x: 10.0, y: 10.0 })
                .as_slice(),
            [ListenerAction::SetCursor(CursorIcon::Default)]
        ));
    }

    #[test]
    fn listeners_for_element_adds_key_scroll_listeners_from_scroll_position() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(50.0);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(0.0);
        attrs.scroll_y_max = Some(40.0);
        let element = with_interaction(make_element(40, attrs), true);

        let listeners = listeners_for_element(&element);
        let key_listeners: Vec<_> = listeners
            .iter()
            .filter(|listener| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::KeyLeftPressNoCtrlAltMeta
                        | ListenerMatcher::KeyRightPressNoCtrlAltMeta
                        | ListenerMatcher::KeyDownPressNoCtrlAltMeta
                )
            })
            .collect();

        assert_eq!(key_listeners.len(), 3);
        assert!(key_listeners.iter().any(|listener| matches!(
            listener.compute_actions(&InputEvent::Key {
                key: CanonicalKey::ArrowLeft,
                action: ACTION_PRESS,
                mods: 0,
            })
            .as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest { element_id, dx, dy })]
                if element_id == &ElementId::from_term_bytes(vec![40])
                    && (dx - SCROLL_LINE_PIXELS).abs() < f32::EPSILON
                    && dy.abs() < f32::EPSILON
        )));
        assert!(key_listeners.iter().any(|listener| matches!(
            listener.compute_actions(&InputEvent::Key {
                key: CanonicalKey::ArrowRight,
                action: ACTION_PRESS,
                mods: 0,
            })
            .as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest { element_id, dx, dy })]
                if element_id == &ElementId::from_term_bytes(vec![40])
                    && (dx + SCROLL_LINE_PIXELS).abs() < f32::EPSILON
                    && dy.abs() < f32::EPSILON
        )));
        assert!(key_listeners.iter().any(|listener| matches!(
            listener.compute_actions(&InputEvent::Key {
                key: CanonicalKey::ArrowDown,
                action: ACTION_PRESS,
                mods: 0,
            })
            .as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest { element_id, dx, dy })]
                if element_id == &ElementId::from_term_bytes(vec![40])
                    && dx.abs() < f32::EPSILON
                    && (dy + SCROLL_LINE_PIXELS).abs() < f32::EPSILON
        )));
    }

    #[test]
    fn listeners_for_element_scrollbar_hover_uses_move_and_active_leave_only() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(10.0);
        let element = with_frame(
            with_interaction(make_element(45, attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );

        let listeners = listeners_for_element(&element);
        assert!(!listeners.iter().any(|listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorLocationLeaveBoundary { .. }
            )
        }));

        let move_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosInside { .. })
        });
        let move_actions =
            move_listener.compute_actions(&InputEvent::CursorPos { x: 96.0, y: 10.0 });
        assert!(matches!(
            actions_without_cursor(&move_actions).as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetScrollbarYHover { element_id, hovered })]
                if *element_id == ElementId::from_term_bytes(vec![45]) && *hovered
        ));
        assert_eq!(cursor_actions(&move_actions), vec![CursorIcon::Default]);

        let mut hovered_attrs = Attrs::default();
        hovered_attrs.scrollbar_y = Some(true);
        hovered_attrs.scroll_y = Some(10.0);
        hovered_attrs.scrollbar_hover_axis = Some(ScrollbarHoverAxis::Y);
        let hovered_element = with_frame(
            with_interaction(make_element(46, hovered_attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );
        let hovered_listeners = listeners_for_element(&hovered_element);
        let leave = listener_matching(&hovered_listeners, |listener| {
            matches!(
                listener.matcher,
                ListenerMatcher::CursorLocationLeaveBoundary { .. }
            )
        });
        let leave_actions = leave.compute_listener_input_actions(&ListenerInput::PointerLeave {
            x: 0.0,
            y: 0.0,
            window_left: true,
        });
        assert!(matches!(
            leave_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetScrollbarYHover { element_id, hovered })]
                if *element_id == ElementId::from_term_bytes(vec![46]) && !*hovered
        ));
    }

    #[test]
    fn registry_for_elements_nested_scrolled_child_hover_uses_screen_space_position() {
        let wrapper_id = ElementId::from_term_bytes(vec![92]);
        let target_id = ElementId::from_term_bytes(vec![93]);

        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(20.0);
        parent_attrs.scroll_y_max = Some(120.0);
        let mut parent = with_frame(
            make_element(91, parent_attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 60.0,
                content_width: 120.0,
                content_height: 180.0,
            },
        );
        parent.children = vec![wrapper_id.clone()];

        let mut wrapper = with_frame(
            make_element(92, Attrs::default()),
            Frame {
                x: 0.0,
                y: 30.0,
                width: 120.0,
                height: 60.0,
                content_width: 120.0,
                content_height: 60.0,
            },
        );
        wrapper.children = vec![target_id.clone()];

        let mut target_attrs = Attrs::default();
        target_attrs.on_mouse_move = Some(true);
        let target = with_frame(
            make_element(93, target_attrs),
            Frame {
                x: 0.0,
                y: 40.0,
                width: 120.0,
                height: 20.0,
                content_width: 120.0,
                content_height: 20.0,
            },
        );

        let registry = registry_for_elements(&[parent, wrapper, target]);
        let hit_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 10.0, y: 25.0 });
        let miss_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 10.0, y: 45.0 });

        assert!(matches!(
            actions_without_cursor(&hit_actions).as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == target_id && *kind == ElementEventKind::MouseMove
        ));
        assert_eq!(cursor_actions(&hit_actions), vec![CursorIcon::Default]);
        assert!(
            actions_without_cursor(&miss_actions).is_empty(),
            "screen-space hover should miss the target at its pre-scroll position"
        );
        assert_eq!(cursor_actions(&miss_actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn registry_for_elements_translated_hover_uses_visual_position() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_move = Some(true);
        attrs.move_x = Some(40.0);
        attrs.move_y = Some(15.0);
        let element = with_frame(
            make_element(96, attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
                content_width: 100.0,
                content_height: 20.0,
            },
        );

        let registry = registry_for_elements(&[element]);
        let hit_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 50.0, y: 20.0 });
        let miss_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 10.0, y: 10.0 });

        assert!(matches!(
            actions_without_cursor(&hit_actions).as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == ElementId::from_term_bytes(vec![96])
                    && *kind == ElementEventKind::MouseMove
        ));
        assert_eq!(cursor_actions(&hit_actions), vec![CursorIcon::Default]);
        assert!(
            actions_without_cursor(&miss_actions).is_empty(),
            "pointer matching should miss the pre-transform position"
        );
        assert_eq!(cursor_actions(&miss_actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn registry_for_elements_rotated_hover_uses_visual_rotation() {
        let mut attrs = Attrs::default();
        attrs.on_mouse_move = Some(true);
        attrs.rotate = Some(90.0);
        let element = with_frame(
            make_element(97, attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
                content_width: 100.0,
                content_height: 20.0,
            },
        );

        let registry = registry_for_elements(&[element]);
        let hit_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 50.0, y: 50.0 });
        let miss_actions =
            first_matching_actions(&registry, &InputEvent::CursorPos { x: 90.0, y: 10.0 });

        assert!(matches!(
            actions_without_cursor(&hit_actions).as_slice(),
            [ListenerAction::ElixirEvent(ElixirEvent { element_id, kind, .. })]
                if *element_id == ElementId::from_term_bytes(vec![97])
                    && *kind == ElementEventKind::MouseMove
        ));
        assert_eq!(cursor_actions(&hit_actions), vec![CursorIcon::Default]);
        assert!(
            actions_without_cursor(&miss_actions).is_empty(),
            "pointer matching should respect the rotated visual footprint"
        );
        assert_eq!(cursor_actions(&miss_actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn registry_for_elements_scrolled_child_scrollbar_hover_uses_screen_space_thumb_rect() {
        let child_id = ElementId::from_term_bytes(vec![95]);

        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(40.0);
        parent_attrs.scroll_y_max = Some(160.0);
        let mut parent = with_frame(
            make_element(94, parent_attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 80.0,
                content_width: 120.0,
                content_height: 240.0,
            },
        );
        parent.children = vec![child_id.clone()];

        let mut child_attrs = Attrs::default();
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(10.0);
        child_attrs.scroll_y_max = Some(100.0);
        let child = with_frame(
            make_element(95, child_attrs),
            Frame {
                x: 10.0,
                y: 60.0,
                width: 80.0,
                height: 40.0,
                content_width: 80.0,
                content_height: 180.0,
            },
        );

        let parent_state = crate::tree::scene::resolve_node_state(
            &parent,
            crate::tree::scene::SceneContext::default(),
        )
        .expect("parent state should resolve");
        let child_state = crate::tree::scene::resolve_node_state(
            &child,
            crate::tree::scene::child_context(
                parent_state,
                crate::tree::element::RetainedPaintPhase::Children,
            ),
        )
        .expect("child state should resolve");
        let thumb = super::scrollbar_nodes_for_state(&child_state)
            .1
            .expect("child scrollbar should exist")
            .thumb_rect;

        let registry = registry_for_elements(&[parent, child]);
        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorPos {
                x: thumb.x + thumb.width / 2.0,
                y: thumb.y + thumb.height / 2.0,
            },
        );

        assert!(matches!(
            actions_without_cursor(&actions).as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetScrollbarYHover { element_id, hovered })]
                if *element_id == child_id && *hovered
        ));
        assert_eq!(cursor_actions(&actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn registry_for_elements_transformed_scrollbar_hover_uses_visual_thumb_rect() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(10.0);
        attrs.scroll_y_max = Some(100.0);
        attrs.move_x = Some(30.0);
        attrs.rotate = Some(90.0);
        let element = with_frame(
            make_element(98, attrs),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );

        let state = crate::tree::scene::resolve_node_state(
            &element,
            crate::tree::scene::SceneContext::default(),
        )
        .expect("state should resolve");
        let thumb = super::scrollbar_nodes_for_state(&state)
            .1
            .expect("scrollbar should exist")
            .thumb_rect;
        let screen_thumb = state.interaction_transform.map_rect_aabb(thumb);

        let registry = registry_for_elements(&[element]);
        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorPos {
                x: screen_thumb.x + screen_thumb.width / 2.0,
                y: screen_thumb.y + screen_thumb.height / 2.0,
            },
        );

        assert!(matches!(
            actions_without_cursor(&actions).as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::SetScrollbarYHover { element_id, hovered })]
                if *element_id == ElementId::from_term_bytes(vec![98]) && *hovered
        ));
        assert_eq!(cursor_actions(&actions), vec![CursorIcon::Default]);
    }

    #[test]
    fn listeners_for_element_scrollbar_press_slots_start_drag_runtime() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(20.0);
        let element = with_frame(
            with_interaction(make_element(47, attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );
        let listeners = listeners_for_element(&element);
        let thumb = listener_matching(&listeners, |listener| {
            matches!(
                listener.compute,
                ListenerCompute::ScrollbarPressToRuntime {
                    spec: ScrollbarPressSpec {
                        axis: ScrollbarAxis::Y,
                        area: ScrollbarHitArea::Thumb,
                        ..
                    },
                    ..
                }
            )
        });
        let thumb_actions = thumb.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 96.0,
            y: 12.0,
        });
        assert!(matches!(
            thumb_actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::StartScrollbarDrag { tracker })]
                if tracker.element_id == ElementId::from_term_bytes(vec![47])
                    && tracker.axis == ScrollbarAxis::Y
        ));

        let track = listener_matching(&listeners, |listener| {
            matches!(
                listener.compute,
                ListenerCompute::ScrollbarPressToRuntime {
                    spec: ScrollbarPressSpec {
                        axis: ScrollbarAxis::Y,
                        area: ScrollbarHitArea::Track,
                        ..
                    },
                    ..
                }
            )
        });
        let track_actions = track.compute_actions(&InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 96.0,
            y: 45.0,
        });
        assert!(matches!(
            track_actions[0],
            ListenerAction::RuntimeChange(RuntimeChange::StartScrollbarDrag { .. })
        ));
        assert!(track_actions.iter().any(|action| matches!(
            action,
            ListenerAction::TreeMsg(TreeMsg::ScrollbarThumbDragY { element_id, .. })
                if *element_id == ElementId::from_term_bytes(vec![47])
        )));
    }

    #[test]
    fn scrollbar_thumb_press_precedes_generic_left_press_listener() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(20.0);
        let element = with_frame(
            with_interaction(make_element(92, attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
                content_width: 100.0,
                content_height: 200.0,
            },
        );

        let registry = registry_for_elements(&[element]);
        let actions = first_matching_actions(
            &registry,
            &InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 96.0,
                y: 12.0,
            },
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::StartScrollbarDrag { tracker })]
                if tracker.element_id == ElementId::from_term_bytes(vec![92])
                    && tracker.axis == ScrollbarAxis::Y
        ));
    }

    #[test]
    fn compose_combined_registry_drag_active_scroll_move_emits_scroll_and_updates_pointer() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(100.0);
        let element = with_frame(
            with_interaction(make_element(48, attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
                content_width: 220.0,
                content_height: 40.0,
            },
        );
        let base = registry_for_elements(&[element]);
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Active {
                element_id: ElementId::from_term_bytes(vec![48]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                last_x: 10.0,
                last_y: 10.0,
                locked_axis: GestureAxis::Horizontal,
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            ..Default::default()
        };
        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 24.0, y: 12.0 },
            &mut ctx,
        );
        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::TreeMsg(TreeMsg::ScrollRequest { element_id, dx, dy }),
                ListenerAction::RuntimeChange(RuntimeChange::UpdateDragTrackerPointer {
                    last_x,
                    last_y,
                }),
            ] if *element_id == ElementId::from_term_bytes(vec![48])
                && (*dx - 14.0).abs() < f32::EPSILON
                && dy.abs() < f32::EPSILON
                && (*last_x - 24.0).abs() < f32::EPSILON
                && (*last_y - 12.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn compose_combined_registry_drag_active_scroll_move_ignores_off_axis_delta_after_lock() {
        let mut attrs = Attrs::default();
        attrs.on_click = Some(true);
        attrs.scrollbar_x = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(100.0);
        let element = with_frame(
            with_interaction(make_element(81, attrs), true),
            Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
                content_width: 220.0,
                content_height: 40.0,
            },
        );
        let base = registry_for_elements(&[element]);
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Active {
                element_id: ElementId::from_term_bytes(vec![81]),
                matcher_kind: ListenerMatcherKind::CursorButtonLeftPressInside,
                last_x: 10.0,
                last_y: 10.0,
                locked_axis: GestureAxis::Horizontal,
            },
            swipe: None,
            scrollbar: None,
            text_drag: None,
        };
        let combined = compose_combined_registry(&base, &runtime);
        let mut ctx = TestComputeCtx {
            base_registry: Some(base.clone()),
            ..Default::default()
        };
        let actions = first_matching_actions_with_ctx(
            &combined,
            &InputEvent::CursorPos { x: 10.0, y: 24.0 },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::RuntimeChange(RuntimeChange::UpdateDragTrackerPointer {
                last_x,
                last_y,
            })] if (*last_x - 10.0).abs() < f32::EPSILON && (*last_y - 24.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn runtime_listeners_for_overlay_scrollbar_drag_emit_move_and_clear_followups() {
        let runtime = RuntimeOverlayState {
            click_press: None,
            virtual_key: None,
            key_presses: Vec::new(),
            drag: DragTrackerState::Inactive,
            swipe: None,
            scrollbar: Some(ScrollbarDragTracker {
                element_id: ElementId::from_term_bytes(vec![49]),
                axis: ScrollbarAxis::Y,
                track_start: 0.0,
                track_len: 30.0,
                thumb_len: 10.0,
                pointer_offset: 5.0,
                scroll_range: 90.0,
                current_scroll: 30.0,
                screen_to_local: Some(Affine2::identity()),
            }),
            text_drag: None,
        };
        let listeners = runtime_listeners_for_overlay(&registry_for_elements(&[]), &runtime);
        assert!(
            listeners
                .iter()
                .any(|listener| matches!(listener.matcher, ListenerMatcher::CursorPosAnywhere))
        );
        assert!(listeners.iter().any(|listener| matches!(
            listener.matcher,
            ListenerMatcher::CursorButtonLeftReleaseAnywhere
        )));

        let move_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorPosAnywhere)
        });
        let move_actions =
            move_listener.compute_actions(&InputEvent::CursorPos { x: 96.0, y: 20.0 });
        assert!(matches!(
            move_actions.as_slice(),
            [
                ListenerAction::TreeMsg(TreeMsg::ScrollbarThumbDragY { element_id, .. }),
                ListenerAction::RuntimeChange(RuntimeChange::UpdateScrollbarDragCurrentScroll { current_scroll }),
            ] if *element_id == ElementId::from_term_bytes(vec![49])
                && (*current_scroll - 45.0).abs() < f32::EPSILON
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
        let backspace_listener = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::KeyBackspacePress)
        });
        let actions = backspace_listener.compute_actions(&InputEvent::Key {
            key: CanonicalKey::Backspace,
            action: ACTION_PRESS,
            mods: 0,
        });
        assert!(actions.is_empty());
    }

    #[test]
    fn window_focus_and_blur_matchers_match_focus_events() {
        assert!(ListenerMatcher::WindowBlurred.matches(&InputEvent::Focused { focused: false }));
        assert!(!ListenerMatcher::WindowBlurred.matches(&InputEvent::Focused { focused: true }));
    }

    #[test]
    fn registry_for_elements_with_focused_node_adds_window_blur_focus_clear_listener() {
        let mut attrs = Attrs::default();
        attrs.on_focus = Some(true);
        attrs.focused_active = Some(true);
        let element = with_interaction(make_element(15, attrs), true);

        let registry = registry_for_elements(&[element]);
        let blur_listener = registry
            .view()
            .find_precedence(|listener| matches!(listener.matcher, ListenerMatcher::WindowBlurred))
            .expect("expected window blur listener");

        let mut ctx = TestComputeCtx {
            focused_id: Some(ElementId::from_term_bytes(vec![15])),
            ..Default::default()
        };
        let actions = blur_listener
            .compute_actions_with_ctx(&InputEvent::Focused { focused: false }, &mut ctx);
        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::ElixirEvent(ElixirEvent { element_id, kind: ElementEventKind::Blur, .. }),
                ListenerAction::TreeMsg(TreeMsg::SetFocusedActive { element_id: tree_id, active: false }),
            ] if element_id == &ElementId::from_term_bytes(vec![15])
                && tree_id == &ElementId::from_term_bytes(vec![15])
        ));
    }

    #[test]
    fn registry_for_elements_without_focused_node_omits_window_blur_focus_clear_listener() {
        let mut attrs = Attrs::default();
        attrs.focused = Some(MouseOverAttrs::default());
        attrs.focused_active = Some(false);
        let element = with_interaction(make_element(16, attrs), true);

        let registry = registry_for_elements(&[element]);
        assert!(
            registry
                .view()
                .iter_precedence()
                .all(|listener| !matches!(listener.matcher, ListenerMatcher::WindowBlurred))
        );
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
        assert_eq!(listeners.len(), 5);
        let scroll_listeners: Vec<_> = listeners
            .iter()
            .filter(|listener| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorScrollInsideDirection { .. }
                )
            })
            .collect();
        assert_eq!(scroll_listeners.len(), 2);

        let x_actions = scroll_listeners
            .iter()
            .find(|listener| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorScrollInsideDirection {
                        direction: ScrollDirection::XNeg,
                        ..
                    }
                )
            })
            .expect("expected x-negative scroll listener")
            .compute_listener_input_actions(&ListenerInput::ScrollDirection {
                direction: ScrollDirection::XNeg,
                dx: -3.0,
                dy: 0.0,
                x: 10.0,
                y: 10.0,
            });
        assert!(matches!(
            x_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                element_id,
                dx,
                dy,
            })] if *element_id == ElementId::from_term_bytes(vec![9]) && (*dx + 3.0).abs() < f32::EPSILON && dy.abs() < f32::EPSILON
        ));

        let y_actions = scroll_listeners
            .iter()
            .find(|listener| {
                matches!(
                    listener.matcher,
                    ListenerMatcher::CursorScrollInsideDirection {
                        direction: ScrollDirection::YNeg,
                        ..
                    }
                )
            })
            .expect("expected y-negative scroll listener")
            .compute_listener_input_actions(&ListenerInput::ScrollDirection {
                direction: ScrollDirection::YNeg,
                dx: 0.0,
                dy: -2.0,
                x: 10.0,
                y: 10.0,
            });
        assert!(matches!(
            y_actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                element_id,
                dx,
                dy,
            })] if *element_id == ElementId::from_term_bytes(vec![9]) && dx.abs() < f32::EPSILON && (dy + 2.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn listeners_for_element_omits_blocked_scroll_directions() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_x = Some(10.0);
        attrs.scroll_x_max = Some(10.0);
        attrs.scroll_y = Some(0.0);
        attrs.scroll_y_max = Some(20.0);
        let element = with_interaction(make_element(90, attrs), true);

        let directions: Vec<_> = listeners_for_element(&element)
            .into_iter()
            .filter_map(|listener| match listener.matcher {
                ListenerMatcher::CursorScrollInsideDirection { direction, .. } => Some(direction),
                _ => None,
            })
            .collect();

        assert_eq!(
            directions,
            vec![ScrollDirection::XPos, ScrollDirection::YNeg]
        );
    }

    #[test]
    fn registry_for_elements_nested_child_scroll_listener_precedes_parent() {
        let mut parent_attrs = Attrs::default();
        parent_attrs.scrollbar_y = Some(true);
        parent_attrs.scroll_y = Some(10.0);
        parent_attrs.scroll_y_max = Some(100.0);
        let mut parent = with_interaction(make_element(71, parent_attrs), true);
        parent.children = vec![ElementId::from_term_bytes(vec![72])];

        let mut child_attrs = Attrs::default();
        child_attrs.scrollbar_y = Some(true);
        child_attrs.scroll_y = Some(20.0);
        child_attrs.scroll_y_max = Some(100.0);
        let child = with_interaction(make_element(72, child_attrs), true);

        let registry = registry_for_elements(&[parent, child]);
        let actions = first_matching_listener_input_actions(
            &registry,
            &ListenerInput::ScrollDirection {
                direction: ScrollDirection::YNeg,
                dx: 0.0,
                dy: -6.0,
                x: 10.0,
                y: 10.0,
            },
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest { element_id, dx, dy })]
                if *element_id == ElementId::from_term_bytes(vec![72])
                    && dx.abs() < f32::EPSILON
                    && (*dy + 6.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn listener_compute_scroll_builds_tree_message_from_directional_input() {
        let element_id = ElementId::from_term_bytes(vec![9]);
        let compute = ListenerCompute::ScrollTreeMsgFromCursorScrollDirection {
            element_id: element_id.clone(),
            direction: ScrollDirection::YNeg,
        };

        let actions = compute.compute_input(
            &ListenerInput::ScrollDirection {
                direction: ScrollDirection::YNeg,
                dx: 0.0,
                dy: -6.0,
                x: 5.0,
                y: 5.0,
            },
            &mut NoopListenerComputeCtx,
        );

        assert!(matches!(
            actions.as_slice(),
            [ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                element_id,
                dx,
                dy,
            })] if *element_id == ElementId::from_term_bytes(vec![9]) && dx.abs() < f32::EPSILON && (dy + 6.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn runtime_scroll_splitter_redispatches_both_components() {
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_x_max = Some(50.0);
        attrs.scroll_y_max = Some(40.0);
        let element = with_interaction(make_element(91, attrs), true);
        let base = registry_for_elements(&[element]);
        let listeners = runtime_listeners_for_overlay(&base, &RuntimeOverlayState::default());
        let splitter = listener_matching(&listeners, |listener| {
            matches!(listener.matcher, ListenerMatcher::CursorScrollAny)
        });

        let mut ctx = TestComputeCtx {
            base_registry: Some(base),
            ..Default::default()
        };
        let actions = splitter.compute_actions_with_ctx(
            &InputEvent::CursorScroll {
                dx: -12.0,
                dy: -6.0,
                x: 5.0,
                y: 5.0,
            },
            &mut ctx,
        );

        assert!(matches!(
            actions.as_slice(),
            [
                ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                    element_id,
                    dx,
                    dy,
                }),
                ListenerAction::TreeMsg(TreeMsg::ScrollRequest {
                    element_id: second_id,
                    dx: dx2,
                    dy: dy2,
                }),
            ] if *element_id == ElementId::from_term_bytes(vec![91])
                && *second_id == ElementId::from_term_bytes(vec![91])
                && (*dx + 12.0).abs() < f32::EPSILON
                && dy.abs() < f32::EPSILON
                && dx2.abs() < f32::EPSILON
                && (*dy2 + 6.0).abs() < f32::EPSILON
        ));
    }
}
