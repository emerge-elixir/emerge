use rustler::{Atom, Encoder, LocalPid, OwnedBinary, OwnedEnv};

use crate::input::{
    ACTION_PRESS, ACTION_RELEASE, EVENT_CLICK, EVENT_FOCUSABLE, EVENT_MOUSE_DOWN,
    EVENT_MOUSE_DOWN_STYLE, EVENT_MOUSE_ENTER, EVENT_MOUSE_LEAVE, EVENT_MOUSE_MOVE,
    EVENT_MOUSE_OVER_STYLE, EVENT_MOUSE_UP, EVENT_PRESS, EVENT_SCROLL_X_NEG, EVENT_SCROLL_X_POS,
    EVENT_SCROLL_Y_NEG, EVENT_SCROLL_Y_POS, EVENT_TEXT_INPUT, InputEvent, MOD_ALT, MOD_CTRL,
    MOD_META, MOD_SHIFT, SCROLL_LINE_PIXELS,
};
use crate::tree::attrs::{BorderWidth, Font, Padding, TextAlign};
use crate::tree::element::{ElementId, ElementKind, ElementTree};
use crate::tree::interaction::{self as tree_interaction, CornerRadii, Rect};
use crate::tree::scrollbar::{self as tree_scrollbar, ScrollbarAxis};

mod dispatch_outcome;
mod registry;
mod registry_builder;
mod runtime;
mod scrollbar;
mod text_ops;
use registry::{
    DispatchCtx, DispatchJob, DispatchRuleAction, EventRegistry, NodeIdx, StyleRuntimeActionKind,
    TriggerId,
};
pub(crate) use runtime::spawn_event_actor;
use scrollbar::{
    ScrollbarDragState, ScrollbarHitArea, ScrollbarInteraction, ScrollbarThumbHover, axis_coord,
    hit_test_scrollbar, scroll_from_pointer, scrollbar_node_from_metrics, thumb_hover_from_hit,
};
pub use scrollbar::{ScrollbarHoverRequest, ScrollbarNode, ScrollbarThumbDragRequest};

const DRAG_DEADZONE: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyScrollDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug)]
struct ScrollContext {
    id: ElementId,
    viewport: Rect,
    scroll_x: f32,
    scroll_y: f32,
    max_x: f32,
    max_y: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyScrollTargets {
    pub left: Option<ElementId>,
    pub right: Option<ElementId>,
    pub up: Option<ElementId>,
    pub down: Option<ElementId>,
}

impl KeyScrollTargets {
    fn for_direction(&self, direction: KeyScrollDirection) -> Option<ElementId> {
        match direction {
            KeyScrollDirection::Left => self.left.clone(),
            KeyScrollDirection::Right => self.right.clone(),
            KeyScrollDirection::Up => self.up.clone(),
            KeyScrollDirection::Down => self.down.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollRequestMatcher {
    pub element_id: ElementId,
    pub dx: f32,
    pub dy: f32,
}

#[derive(Clone)]
pub struct EventProcessor {
    registry: Vec<EventNode>,
    dispatch_registry: EventRegistry,
    pressed_id: Option<ElementId>,
    hovered_id: Option<ElementId>,
    mouse_over_active_id: Option<ElementId>,
    mouse_down_active_id: Option<ElementId>,
    focused_id: Option<ElementId>,
    text_input_drag_id: Option<ElementId>,
    hovered_scrollbar_thumb: Option<ScrollbarThumbHover>,
    drag_start: Option<(f32, f32)>,
    drag_last_pos: Option<(f32, f32)>,
    drag_active: bool,
    drag_consumed: bool,
    scrollbar_interaction: ScrollbarInteraction,
}

#[derive(Clone, Copy, Debug, Default)]
struct DispatchApplyResult {
    mouse_button_event_emitted: bool,
    matched_rule: bool,
}

impl DispatchApplyResult {
    fn merge(&mut self, other: Self) {
        self.mouse_button_event_emitted |= other.mouse_button_event_emitted;
        self.matched_rule |= other.matched_rule;
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DispatchPreview {
    pub outcome: Option<dispatch_outcome::DispatchOutcome>,
    pub had_jobs: bool,
    pub trigger_for_stats: Option<TriggerId>,
}

#[derive(Clone, Debug, Default)]
struct CompiledDispatchJobs {
    primary_trigger: Option<TriggerId>,
    trigger_for_stats: Option<TriggerId>,
    jobs: Vec<DispatchJob>,
}

#[derive(Clone, Debug)]
pub struct EventNode {
    pub id: ElementId,
    pub hit_rect: Rect,
    pub visible: bool,
    pub flags: u16,
    pub self_rect: Rect,
    pub self_radii: Option<CornerRadii>,
    pub clip_rect: Option<Rect>,
    pub clip_radii: Option<CornerRadii>,
    pub scrollbar_x: Option<ScrollbarNode>,
    pub scrollbar_y: Option<ScrollbarNode>,
    pub key_scroll_targets: KeyScrollTargets,
    pub focus_reveal_scrolls: Vec<ScrollRequestMatcher>,
    pub text_input: Option<TextInputDescriptor>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextInputDescriptor {
    pub content: String,
    pub content_len: u32,
    pub cursor: u32,
    pub selection_anchor: Option<u32>,
    pub emit_change: bool,
    pub frame_x: f32,
    pub frame_width: f32,
    pub inset_left: f32,
    pub inset_right: f32,
    pub text_align: TextAlign,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_italic: bool,
    pub letter_spacing: f32,
    pub word_spacing: f32,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MouseOverRequest {
    SetMouseOverActive { element_id: ElementId, active: bool },
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MouseDownRequest {
    SetMouseDownActive { element_id: ElementId, active: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEditRequest {
    MoveLeft { extend_selection: bool },
    MoveRight { extend_selection: bool },
    MoveHome { extend_selection: bool },
    MoveEnd { extend_selection: bool },
    Backspace,
    Delete,
    Insert(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextInputCursorRequest {
    Set {
        element_id: ElementId,
        x: f32,
        extend_selection: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputCommandRequest {
    SelectAll,
    Copy,
    Cut,
    Paste,
    PastePrimary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputPreeditRequest {
    Set {
        text: String,
        cursor: Option<(u32, u32)>,
    },
    Clear,
}

pub fn build_event_registry(tree: &mut ElementTree) -> Vec<EventNode> {
    let Some(root) = tree.root.clone() else {
        return Vec::new();
    };

    tree_interaction::populate_interaction(tree);

    let mut registry = Vec::new();
    collect_event_nodes(tree, &root, &mut registry, &[]);
    registry
}

fn collect_event_nodes(
    tree: &ElementTree,
    id: &ElementId,
    registry: &mut Vec<EventNode>,
    scroll_contexts: &[ScrollContext],
) {
    let Some(element) = tree.get(id) else {
        return;
    };

    let mut flags = 0u16;
    if element.attrs.on_click.unwrap_or(false) {
        flags |= EVENT_CLICK;
    }
    if element.attrs.on_mouse_down.unwrap_or(false) {
        flags |= EVENT_MOUSE_DOWN;
    }
    if element.attrs.on_mouse_up.unwrap_or(false) {
        flags |= EVENT_MOUSE_UP;
    }
    if element.attrs.on_mouse_enter.unwrap_or(false) {
        flags |= EVENT_MOUSE_ENTER;
    }
    if element.attrs.on_mouse_leave.unwrap_or(false) {
        flags |= EVENT_MOUSE_LEAVE;
    }
    if element.attrs.on_mouse_move.unwrap_or(false) {
        flags |= EVENT_MOUSE_MOVE;
    }
    if element.attrs.on_press.unwrap_or(false) {
        flags |= EVENT_PRESS;
    }
    if element.attrs.mouse_over.is_some() {
        flags |= EVENT_MOUSE_OVER_STYLE;
    }
    if element.attrs.mouse_down.is_some() {
        flags |= EVENT_MOUSE_DOWN_STYLE;
    }
    if element.kind == ElementKind::TextInput {
        flags |= EVENT_TEXT_INPUT;
        flags |= EVENT_FOCUSABLE;
    }
    if element.attrs.on_press.unwrap_or(false)
        || element.attrs.on_focus.unwrap_or(false)
        || element.attrs.on_blur.unwrap_or(false)
    {
        flags |= EVENT_FOCUSABLE;
    }
    if element.attrs.scrollbar_x.unwrap_or(false) {
        let scroll_x = element.attrs.scroll_x.unwrap_or(0.0) as f32;
        let scroll_x_max = element.attrs.scroll_x_max.unwrap_or(0.0) as f32;
        if scroll_x > 0.0 {
            flags |= EVENT_SCROLL_X_NEG;
        }
        if scroll_x < scroll_x_max {
            flags |= EVENT_SCROLL_X_POS;
        }
    }
    if element.attrs.scrollbar_y.unwrap_or(false) {
        let scroll_y = element.attrs.scroll_y.unwrap_or(0.0) as f32;
        let scroll_y_max = element.attrs.scroll_y_max.unwrap_or(0.0) as f32;
        if scroll_y > 0.0 {
            flags |= EVENT_SCROLL_Y_NEG;
        }
        if scroll_y < scroll_y_max {
            flags |= EVENT_SCROLL_Y_POS;
        }
    }

    let mut next_scroll_contexts = scroll_contexts.to_vec();

    if let Some(frame) = element.frame
        && let Some(interaction) = element.interaction
    {
        let adjusted_rect = interaction.self_rect;
        let visible_rect = interaction.hit_rect;
        let visible = interaction.visible;
        let self_radii = interaction.self_radii;
        let active_clip_rect = interaction.clip_rect;
        let clip_radii = interaction.clip_radii;

        let node_offset_x = frame.x - adjusted_rect.x;
        let node_offset_y = frame.y - adjusted_rect.y;

        let scrollbar_x = tree_scrollbar::horizontal_metrics(frame, &element.attrs)
            .map(|metrics| scrollbar_node_from_metrics(metrics, node_offset_x, node_offset_y));
        let scrollbar_y = tree_scrollbar::vertical_metrics(frame, &element.attrs)
            .map(|metrics| scrollbar_node_from_metrics(metrics, node_offset_x, node_offset_y));
        let text_input = if element.kind == ElementKind::TextInput {
            Some(text_input_descriptor(element, adjusted_rect))
        } else {
            None
        };

        let padding = element.attrs.padding.as_ref();
        let (left, top, right, bottom) = match padding {
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
        };

        let content_rect = Rect {
            x: adjusted_rect.x + left,
            y: adjusted_rect.y + top,
            width: (adjusted_rect.width - left - right).max(0.0),
            height: (adjusted_rect.height - top - bottom).max(0.0),
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

        let focusable = flags & EVENT_FOCUSABLE != 0;
        let key_scroll_targets = if focusable {
            key_scroll_targets_for_contexts(&next_scroll_contexts)
        } else {
            KeyScrollTargets::default()
        };
        let focus_reveal_scrolls = if focusable {
            focus_reveal_scroll_requests(&element.id, adjusted_rect, &next_scroll_contexts)
        } else {
            Vec::new()
        };

        if flags != 0 && (visible || focusable) {
            registry.push(EventNode {
                id: element.id.clone(),
                hit_rect: visible_rect,
                visible,
                flags,
                self_rect: adjusted_rect,
                self_radii,
                clip_rect: active_clip_rect,
                clip_radii,
                scrollbar_x,
                scrollbar_y,
                key_scroll_targets,
                focus_reveal_scrolls,
                text_input,
            });
        }
    }

    for child_id in &element.children {
        collect_event_nodes(tree, child_id, registry, &next_scroll_contexts);
    }
}

fn key_scroll_targets_for_contexts(contexts: &[ScrollContext]) -> KeyScrollTargets {
    let mut targets = KeyScrollTargets::default();

    for context in contexts.iter().rev() {
        if targets.left.is_none() && context.scroll_x > 0.0 {
            targets.left = Some(context.id.clone());
        }
        if targets.right.is_none() && context.scroll_x < context.max_x {
            targets.right = Some(context.id.clone());
        }
        if targets.up.is_none() && context.scroll_y > 0.0 {
            targets.up = Some(context.id.clone());
        }
        if targets.down.is_none() && context.scroll_y < context.max_y {
            targets.down = Some(context.id.clone());
        }

        if targets.left.is_some()
            && targets.right.is_some()
            && targets.up.is_some()
            && targets.down.is_some()
        {
            break;
        }
    }

    targets
}

fn focus_reveal_scroll_requests(
    element_id: &ElementId,
    element_rect: Rect,
    contexts: &[ScrollContext],
) -> Vec<ScrollRequestMatcher> {
    let mut adjusted = element_rect;
    let mut requests = Vec::new();

    for context in contexts.iter().rev() {
        if context.id == *element_id {
            continue;
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
            requests.push(ScrollRequestMatcher {
                element_id: context.id.clone(),
                dx: -scroll_delta_x,
                dy: -scroll_delta_y,
            });
            adjusted.x -= scroll_delta_x;
            adjusted.y -= scroll_delta_y;
        }
    }

    requests
}

pub fn hit_test_with_flag(registry: &[EventNode], x: f32, y: f32, flag: u16) -> Option<ElementId> {
    for node in registry.iter().rev() {
        if node.flags & flag == 0 {
            continue;
        }
        if !point_hits_node(node, x, y) {
            continue;
        }
        return Some(node.id.clone());
    }
    None
}

fn point_hits_node(node: &EventNode, x: f32, y: f32) -> bool {
    if !node.hit_rect.contains(x, y) {
        return false;
    }
    if let (Some(rect), Some(radii)) = (node.clip_rect, node.clip_radii)
        && !tree_interaction::point_in_rounded_rect(rect, radii, x, y)
    {
        return false;
    }
    if let Some(radii) = node.self_radii
        && !tree_interaction::point_in_rounded_rect(node.self_rect, radii, x, y)
    {
        return false;
    }
    true
}

fn text_input_descriptor(
    element: &crate::tree::element::Element,
    adjusted_rect: Rect,
) -> TextInputDescriptor {
    let content = element.base_attrs.content.clone().unwrap_or_default();
    let content_len = content.chars().count() as u32;
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
    let (inset_left, inset_right) = text_content_insets(&element.attrs);
    let (font_family, font_weight, font_italic) = font_info_from_attrs(&element.attrs);

    TextInputDescriptor {
        content,
        content_len,
        cursor,
        selection_anchor,
        emit_change: element.attrs.on_change.unwrap_or(false),
        frame_x: adjusted_rect.x,
        frame_width: adjusted_rect.width,
        inset_left,
        inset_right,
        text_align: element.attrs.text_align.unwrap_or_default(),
        font_family,
        font_size: element.attrs.font_size.unwrap_or(16.0) as f32,
        font_weight,
        font_italic,
        letter_spacing: element.attrs.font_letter_spacing.unwrap_or(0.0) as f32,
        word_spacing: element.attrs.font_word_spacing.unwrap_or(0.0) as f32,
    }
}

fn text_content_insets(attrs: &crate::tree::attrs::Attrs) -> (f32, f32) {
    let (pad_left, pad_right) = match attrs.padding.as_ref() {
        Some(Padding::Uniform(v)) => (*v as f32, *v as f32),
        Some(Padding::Sides { left, right, .. }) => (*left as f32, *right as f32),
        None => (0.0, 0.0),
    };

    let (border_left, border_right) = match attrs.border_width.as_ref() {
        Some(BorderWidth::Uniform(v)) => (*v as f32, *v as f32),
        Some(BorderWidth::Sides { left, right, .. }) => (*left as f32, *right as f32),
        None => (0.0, 0.0),
    };

    (pad_left + border_left, pad_right + border_right)
}

fn font_info_from_attrs(attrs: &crate::tree::attrs::Attrs) -> (String, u16, bool) {
    let family = attrs
        .font
        .as_ref()
        .map(|font| match font {
            Font::Atom(name) | Font::String(name) => name.clone(),
        })
        .unwrap_or_else(|| "default".to_string());

    let weight = attrs
        .font_weight
        .as_ref()
        .map(|value| parse_font_weight(&value.0))
        .unwrap_or(400);

    let italic = attrs
        .font_style
        .as_ref()
        .map(|style| style.0 == "italic")
        .unwrap_or(false);

    (family, weight, italic)
}

fn parse_font_weight(value: &str) -> u16 {
    match value {
        "bold" => 700,
        "normal" => 400,
        "light" => 300,
        "thin" => 100,
        "medium" => 500,
        "semibold" | "semi_bold" => 600,
        "extrabold" | "extra_bold" => 800,
        "black" => 900,
        _ => value.parse().unwrap_or(400),
    }
}

impl EventProcessor {
    pub fn new() -> Self {
        Self {
            registry: Vec::new(),
            dispatch_registry: EventRegistry::from_event_nodes(&[]),
            pressed_id: None,
            hovered_id: None,
            mouse_over_active_id: None,
            mouse_down_active_id: None,
            focused_id: None,
            text_input_drag_id: None,
            hovered_scrollbar_thumb: None,
            drag_start: None,
            drag_last_pos: None,
            drag_active: false,
            drag_consumed: false,
            scrollbar_interaction: ScrollbarInteraction::default(),
        }
    }

    pub fn rebuild_registry(&mut self, registry: Vec<EventNode>) {
        self.registry = registry;
        self.dispatch_registry = EventRegistry::from_event_nodes(&self.registry);

        if let Some(hover) = self.hovered_scrollbar_thumb.as_ref() {
            let still_valid = self.registry.iter().any(|node| {
                if node.id != hover.id {
                    return false;
                }
                match hover.axis {
                    ScrollbarAxis::X => node.scrollbar_x.is_some(),
                    ScrollbarAxis::Y => node.scrollbar_y.is_some(),
                }
            });
            if !still_valid {
                self.hovered_scrollbar_thumb = None;
            }
        }

        if let Some(focused) = self.focused_id.as_ref() {
            let still_valid = self
                .registry
                .iter()
                .any(|node| node.id == *focused && (node.flags & EVENT_FOCUSABLE != 0));
            if !still_valid {
                self.focused_id = None;
            }
        }

        if let Some(mouse_down_id) = self.mouse_down_active_id.as_ref() {
            let still_valid = self.registry.iter().any(|node| {
                node.id == *mouse_down_id && (node.flags & EVENT_MOUSE_DOWN_STYLE != 0)
            });
            if !still_valid {
                self.mouse_down_active_id = None;
            }
        }

        if let Some(drag_id) = self.text_input_drag_id.as_ref() {
            let still_valid = self
                .registry
                .iter()
                .any(|node| node.id == *drag_id && (node.flags & EVENT_TEXT_INPUT != 0));
            if !still_valid {
                self.text_input_drag_id = None;
            }
        }

        let Some((drag_id, drag_axis)) = self
            .scrollbar_interaction
            .dragging()
            .map(|drag| (drag.id.clone(), drag.axis))
        else {
            return;
        };

        let maybe_scrollbar = self
            .registry
            .iter()
            .rev()
            .find(|node| node.id == drag_id)
            .and_then(|node| match drag_axis {
                ScrollbarAxis::X => node.scrollbar_x,
                ScrollbarAxis::Y => node.scrollbar_y,
            });

        match maybe_scrollbar {
            Some(scrollbar) => {
                if let Some(drag) = self.scrollbar_interaction.dragging_mut() {
                    drag.track_start = scrollbar.track_start;
                    drag.track_len = scrollbar.track_len;
                    drag.thumb_len = scrollbar.thumb_len;
                    drag.scroll_range = scrollbar.scroll_range;
                    drag.current_scroll = scrollbar.scroll_offset;
                    drag.pointer_offset = drag.pointer_offset.clamp(0.0, drag.thumb_len);
                }
            }
            None => {
                self.scrollbar_interaction.clear();
            }
        }
    }

    fn confirmed_pointer_release_target(&self, x: f32, y: f32) -> Option<ElementId> {
        if self.scrollbar_interaction.is_captured() || self.drag_consumed {
            return None;
        }

        let hit = hit_test_with_flag(&self.registry, x, y, EVENT_CLICK | EVENT_PRESS)?;
        let pressed_id = self.pressed_id.as_ref()?;
        if pressed_id == &hit {
            Some(pressed_id.clone())
        } else {
            None
        }
    }

    fn advance_pointer_press_state(&mut self, event: &InputEvent) {
        let InputEvent::CursorButton {
            button,
            action,
            x,
            y,
            ..
        } = event
        else {
            return;
        };

        if button != "left" {
            return;
        }

        if *action == crate::input::ACTION_PRESS {
            if hit_test_scrollbar(&self.registry, *x, *y).is_some() {
                self.scrollbar_interaction.mark_captured();
                self.pressed_id = None;
                self.drag_start = None;
                self.drag_last_pos = None;
                self.drag_active = false;
                self.drag_consumed = true;
                return;
            }

            self.scrollbar_interaction.clear();
            self.pressed_id = hit_test_with_flag(&self.registry, *x, *y, EVENT_CLICK | EVENT_PRESS);
            self.drag_start = Some((*x, *y));
            self.drag_last_pos = Some((*x, *y));
            self.drag_active = false;
            self.drag_consumed = false;
            return;
        }

        if *action == crate::input::ACTION_RELEASE {
            self.pressed_id = None;
            self.drag_start = None;
            self.drag_last_pos = None;
            self.drag_active = false;
            self.drag_consumed = false;
            self.scrollbar_interaction.clear();
        }
    }

    pub(crate) fn advance_runtime_state_after_event(&mut self, event: &InputEvent) {
        self.advance_pointer_press_state(event);
        self.advance_text_cursor_state(event);
        self.advance_scroll_state(event);
        self.advance_hover_state(event);
        self.advance_scrollbar_interaction_state(event);
        self.advance_scrollbar_hover_state(event);
        self.advance_mouse_over_state(event);
        self.advance_mouse_down_state(event);
    }

    fn advance_text_cursor_state(&mut self, event: &InputEvent) {
        match event {
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left" && *action == ACTION_PRESS => {
                self.text_input_drag_id =
                    hit_test_with_flag(&self.registry, *x, *y, EVENT_TEXT_INPUT);
            }
            InputEvent::CursorButton { button, action, .. }
                if button == "left" && *action == crate::input::ACTION_RELEASE =>
            {
                self.text_input_drag_id = None;
            }
            InputEvent::CursorEntered { entered } if !*entered => {
                self.text_input_drag_id = None;
            }
            _ => {}
        }
    }

    fn next_hover_target_for_event(&self, event: &InputEvent) -> Option<Option<ElementId>> {
        match event {
            InputEvent::CursorPos { x, y } => {
                let hover_mask = EVENT_MOUSE_ENTER
                    | EVENT_MOUSE_LEAVE
                    | EVENT_MOUSE_MOVE
                    | EVENT_MOUSE_OVER_STYLE;
                Some(hit_test_with_flag(&self.registry, *x, *y, hover_mask))
            }
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left"
                && (*action == crate::input::ACTION_PRESS
                    || *action == crate::input::ACTION_RELEASE) =>
            {
                let hover_mask = EVENT_MOUSE_ENTER
                    | EVENT_MOUSE_LEAVE
                    | EVENT_MOUSE_MOVE
                    | EVENT_MOUSE_OVER_STYLE;
                Some(hit_test_with_flag(&self.registry, *x, *y, hover_mask))
            }
            InputEvent::CursorEntered { entered } if !*entered => Some(None),
            _ => None,
        }
    }

    fn apply_hover_transition(
        &mut self,
        event: &InputEvent,
    ) -> Option<(Option<ElementId>, Option<ElementId>)> {
        let next_hover = self.next_hover_target_for_event(event)?;
        if self.hovered_id == next_hover {
            return None;
        }

        let previous = self.hovered_id.clone();
        self.hovered_id = next_hover.clone();
        Some((previous, next_hover))
    }

    fn advance_hover_state(&mut self, event: &InputEvent) {
        let _ = self.apply_hover_transition(event);
    }

    pub fn text_input_focus_request(&mut self, event: &InputEvent) -> Option<Option<ElementId>> {
        let next_focus = match event {
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left" && *action == ACTION_PRESS => {
                Some(hit_test_with_flag(&self.registry, *x, *y, EVENT_FOCUSABLE))
            }
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "middle" && *action == ACTION_PRESS => {
                Some(hit_test_with_flag(&self.registry, *x, *y, EVENT_TEXT_INPUT))
            }
            InputEvent::Key { key, action, mods }
                if *action == ACTION_PRESS && key.eq_ignore_ascii_case("tab") =>
            {
                let blocked_mods = MOD_CTRL | MOD_ALT | MOD_META;
                if *mods & blocked_mods != 0 {
                    None
                } else {
                    let reverse = *mods & MOD_SHIFT != 0;
                    self.cycle_focus(reverse).map(Some)
                }
            }
            InputEvent::Focused { focused } if !*focused => {
                self.text_input_drag_id = None;
                Some(None)
            }
            _ => None,
        }?;

        if self.focused_id == next_focus {
            return None;
        }

        self.focused_id = next_focus.clone();
        Some(next_focus)
    }

    pub fn text_input_cursor_requests(
        &mut self,
        event: &InputEvent,
    ) -> Vec<TextInputCursorRequest> {
        match event {
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                mods,
                ..
            } if button == "left" && *action == ACTION_PRESS => {
                let Some(id) = hit_test_with_flag(&self.registry, *x, *y, EVENT_TEXT_INPUT) else {
                    self.text_input_drag_id = None;
                    return Vec::new();
                };

                self.text_input_drag_id = Some(id.clone());
                let extend_selection = *mods & MOD_SHIFT != 0;
                vec![TextInputCursorRequest::Set {
                    element_id: id,
                    x: *x,
                    extend_selection,
                }]
            }
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "middle" && *action == ACTION_PRESS => {
                let Some(id) = hit_test_with_flag(&self.registry, *x, *y, EVENT_TEXT_INPUT) else {
                    return Vec::new();
                };

                vec![TextInputCursorRequest::Set {
                    element_id: id,
                    x: *x,
                    extend_selection: false,
                }]
            }
            InputEvent::CursorPos { x, .. } => {
                let Some(id) = self.text_input_drag_id.clone() else {
                    return Vec::new();
                };

                vec![TextInputCursorRequest::Set {
                    element_id: id,
                    x: *x,
                    extend_selection: true,
                }]
            }
            InputEvent::CursorButton { button, action, .. }
                if button == "left" && *action == crate::input::ACTION_RELEASE =>
            {
                self.text_input_drag_id = None;
                Vec::new()
            }
            InputEvent::CursorEntered { entered } if !*entered => {
                self.text_input_drag_id = None;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn text_input_command_request(
        &self,
        event: &InputEvent,
    ) -> Option<(ElementId, TextInputCommandRequest)> {
        match event {
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "middle" && *action == ACTION_PRESS => {
                let id = hit_test_with_flag(&self.registry, *x, *y, EVENT_TEXT_INPUT)?;
                Some((id, TextInputCommandRequest::PastePrimary))
            }
            InputEvent::Key { key, action, mods } if *action == ACTION_PRESS => {
                let focused_id = self.focused_text_input_id()?.clone();
                let has_meta = (*mods & MOD_CTRL != 0) || (*mods & MOD_META != 0);
                if !has_meta {
                    return None;
                }

                let key = key.to_ascii_lowercase();
                let request = match key.as_str() {
                    "a" => TextInputCommandRequest::SelectAll,
                    "c" => TextInputCommandRequest::Copy,
                    "x" => TextInputCommandRequest::Cut,
                    "v" => TextInputCommandRequest::Paste,
                    _ => return None,
                };

                Some((focused_id, request))
            }
            _ => None,
        }
    }

    pub fn text_input_edit_request(
        &self,
        event: &InputEvent,
    ) -> Option<(ElementId, TextInputEditRequest)> {
        let focused_id = self.focused_text_input_id()?.clone();
        let descriptor = self.text_input_descriptor(&focused_id)?;

        match event {
            InputEvent::Key { key, action, mods } if *action == ACTION_PRESS => {
                let request = self.text_input_key_edit_request(descriptor, key, *mods)?;

                Some((focused_id, request))
            }
            InputEvent::TextCommit { text, mods } => {
                if (*mods & MOD_CTRL != 0) || (*mods & MOD_META != 0) {
                    return None;
                }

                let filtered: String = text.chars().filter(|ch| !ch.is_control()).collect();
                if filtered.is_empty() {
                    return None;
                }

                Some((focused_id, TextInputEditRequest::Insert(filtered)))
            }
            _ => None,
        }
    }

    pub fn text_input_preedit_request(
        &self,
        event: &InputEvent,
    ) -> Option<(ElementId, TextInputPreeditRequest)> {
        let focused_id = self.focused_text_input_id()?.clone();

        match event {
            InputEvent::TextPreedit { text, cursor } => {
                if text.is_empty() {
                    Some((focused_id, TextInputPreeditRequest::Clear))
                } else {
                    Some((
                        focused_id,
                        TextInputPreeditRequest::Set {
                            text: text.clone(),
                            cursor: *cursor,
                        },
                    ))
                }
            }
            InputEvent::TextPreeditClear => Some((focused_id, TextInputPreeditRequest::Clear)),
            _ => None,
        }
    }

    fn text_input_descriptor(&self, id: &ElementId) -> Option<&TextInputDescriptor> {
        self.registry
            .iter()
            .find(|node| node.id == *id)
            .and_then(|node| node.text_input.as_ref())
    }

    fn focused_text_input_descriptor(&self) -> Option<&TextInputDescriptor> {
        let focused_id = self.focused_text_input_id()?;
        self.text_input_descriptor(&focused_id)
    }

    fn text_input_key_edit_request(
        &self,
        descriptor: &TextInputDescriptor,
        key: &str,
        mods: u8,
    ) -> Option<TextInputEditRequest> {
        let extend_selection = mods & MOD_SHIFT != 0;
        let has_selection = descriptor
            .selection_anchor
            .is_some_and(|anchor| anchor != descriptor.cursor);

        match key {
            "left" => {
                let can_move = if extend_selection {
                    descriptor.cursor > 0
                } else {
                    descriptor.cursor > 0 || has_selection
                };
                if can_move {
                    Some(TextInputEditRequest::MoveLeft { extend_selection })
                } else {
                    None
                }
            }
            "right" => {
                let can_move = if extend_selection {
                    descriptor.cursor < descriptor.content_len
                } else {
                    descriptor.cursor < descriptor.content_len || has_selection
                };
                if can_move {
                    Some(TextInputEditRequest::MoveRight { extend_selection })
                } else {
                    None
                }
            }
            "home" => {
                let can_move = if extend_selection {
                    descriptor.cursor > 0
                } else {
                    descriptor.cursor > 0 || has_selection
                };
                if can_move {
                    Some(TextInputEditRequest::MoveHome { extend_selection })
                } else {
                    None
                }
            }
            "end" => {
                let can_move = if extend_selection {
                    descriptor.cursor < descriptor.content_len
                } else {
                    descriptor.cursor < descriptor.content_len || has_selection
                };
                if can_move {
                    Some(TextInputEditRequest::MoveEnd { extend_selection })
                } else {
                    None
                }
            }
            "backspace" => Some(TextInputEditRequest::Backspace),
            "delete" => Some(TextInputEditRequest::Delete),
            _ => None,
        }
    }

    fn preview_next_content_for_edit(
        descriptor: &TextInputDescriptor,
        request: &TextInputEditRequest,
    ) -> Option<String> {
        text_ops::apply_edit_request(
            &descriptor.content,
            descriptor.cursor,
            descriptor.selection_anchor,
            request,
        )
        .map(|(next_content, _next_cursor)| next_content)
    }

    fn push_unique_scroll_request(
        out: &mut dispatch_outcome::DispatchOutcome,
        target: &ElementId,
        dx: f32,
        dy: f32,
    ) {
        let request = dispatch_outcome::ScrollRequestOut {
            target: dispatch_outcome::node_key(target),
            dx: dispatch_outcome::milli(dx),
            dy: dispatch_outcome::milli(dy),
        };

        if !out.scroll_requests.contains(&request) {
            out.scroll_requests.push(request);
        }
    }

    fn push_unique_change_event(
        out: &mut dispatch_outcome::DispatchOutcome,
        target: dispatch_outcome::NodeKey,
        payload: String,
    ) {
        let duplicate = out.element_events.iter().any(|event| {
            event.kind == dispatch_outcome::ElementEventKind::Change
                && event.target == target
                && event.payload.as_deref() == Some(payload.as_str())
        });

        if !duplicate {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target,
                kind: dispatch_outcome::ElementEventKind::Change,
                payload: Some(payload),
            });
        }
    }

    fn push_unique_text_command_request(
        out: &mut dispatch_outcome::DispatchOutcome,
        target: &ElementId,
        request: TextInputCommandRequest,
    ) {
        let req = dispatch_outcome::TextCommandReqOut {
            target: dispatch_outcome::node_key(target),
            request,
        };

        if !out.text_command_requests.contains(&req) {
            out.text_command_requests.push(req);
        }
    }

    fn push_unique_text_preedit_request(
        out: &mut dispatch_outcome::DispatchOutcome,
        target: &ElementId,
        request: TextInputPreeditRequest,
    ) {
        let req = dispatch_outcome::TextPreeditReqOut {
            target: dispatch_outcome::node_key(target),
            request,
        };

        if !out.text_preedit_requests.contains(&req) {
            out.text_preedit_requests.push(req);
        }
    }

    fn push_unique_text_cursor_request(
        out: &mut dispatch_outcome::DispatchOutcome,
        request: TextInputCursorRequest,
    ) {
        let req = match request {
            TextInputCursorRequest::Set {
                element_id,
                x,
                extend_selection,
            } => dispatch_outcome::TextCursorReqOut {
                target: dispatch_outcome::node_key(&element_id),
                x: dispatch_outcome::milli(x),
                extend_selection,
            },
        };

        if !out.text_cursor_requests.contains(&req) {
            out.text_cursor_requests.push(req);
        }
    }

    fn push_text_edit_request_and_change(
        &self,
        target_id: &ElementId,
        request: &TextInputEditRequest,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) {
        let target = dispatch_outcome::node_key(target_id);
        let req = dispatch_outcome::TextEditReqOut {
            target: target.clone(),
            request: request.clone(),
        };

        if !out.text_edit_requests.contains(&req) {
            out.text_edit_requests.push(req);
        }

        if let Some(descriptor) = self.text_input_descriptor(target_id)
            && descriptor.emit_change
            && let Some(next_content) = Self::preview_next_content_for_edit(descriptor, request)
        {
            Self::push_unique_change_event(out, target, next_content);
        }
    }

    pub fn focused_text_input_id(&self) -> Option<ElementId> {
        let focused_id = self.focused_id.as_ref()?.clone();
        if self.node_has_flag(&focused_id, EVENT_TEXT_INPUT) {
            Some(focused_id)
        } else {
            None
        }
    }

    pub fn focused_id(&self) -> Option<ElementId> {
        self.focused_id.clone()
    }

    pub(crate) fn set_focused_id_for_runtime(&mut self, next_focus: Option<ElementId>) {
        self.focused_id = next_focus;
        if self.focused_id.is_none() {
            self.text_input_drag_id = None;
        }
    }

    fn apply_focus_change_action(
        &self,
        next: Option<u32>,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) {
        out.focus_change = Some(next.and_then(|idx| {
            self.dispatch_registry
                .node_id(idx)
                .map(dispatch_outcome::node_key)
        }));
    }

    fn apply_scroll_action(
        &self,
        element: u32,
        dx: f32,
        dy: f32,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) {
        if let Some(target_id) = self.dispatch_registry.node_id(element) {
            Self::push_unique_scroll_request(out, target_id, dx, dy);
        }
    }

    fn element_event_kind_for_trigger(
        trigger: TriggerId,
    ) -> Option<dispatch_outcome::ElementEventKind> {
        match trigger {
            TriggerId::CursorButtonLeftPress => Some(dispatch_outcome::ElementEventKind::MouseDown),
            TriggerId::CursorButtonLeftRelease => Some(dispatch_outcome::ElementEventKind::MouseUp),
            TriggerId::CursorClickRelease => Some(dispatch_outcome::ElementEventKind::Click),
            TriggerId::CursorPressRelease => Some(dispatch_outcome::ElementEventKind::Press),
            TriggerId::CursorMove => Some(dispatch_outcome::ElementEventKind::MouseMove),
            TriggerId::CursorEnter => Some(dispatch_outcome::ElementEventKind::MouseEnter),
            TriggerId::CursorLeave => Some(dispatch_outcome::ElementEventKind::MouseLeave),
            TriggerId::KeyEnterPress => Some(dispatch_outcome::ElementEventKind::Press),
            _ => None,
        }
    }

    fn dispatch_ctx_for_event(&self, event: &InputEvent, focused: Option<NodeIdx>) -> DispatchCtx {
        let mods = match event {
            InputEvent::Key { mods, .. }
            | InputEvent::TextCommit { mods, .. }
            | InputEvent::CursorButton { mods, .. } => *mods,
            _ => 0,
        };

        DispatchCtx { mods, focused }
    }

    fn hover_target_idx_at(&self, x: f32, y: f32) -> Option<NodeIdx> {
        let hover_mask =
            EVENT_MOUSE_ENTER | EVENT_MOUSE_LEAVE | EVENT_MOUSE_MOVE | EVENT_MOUSE_OVER_STYLE;
        hit_test_with_flag(&self.registry, x, y, hover_mask)
            .as_ref()
            .and_then(|id| self.dispatch_registry.node_idx(id))
    }

    fn push_hover_transition_jobs(
        &self,
        jobs: &mut Vec<DispatchJob>,
        ctx: DispatchCtx,
        next_hover_idx: Option<NodeIdx>,
    ) {
        let previous_hover_idx = self
            .hovered_id
            .as_ref()
            .and_then(|id| self.dispatch_registry.node_idx(id));

        if previous_hover_idx == next_hover_idx {
            return;
        }

        if let Some(previous) = previous_hover_idx {
            jobs.push(DispatchJob::Targeted {
                trigger: TriggerId::CursorLeave,
                target: previous,
                ctx,
            });
        }

        if let Some(next) = next_hover_idx {
            jobs.push(DispatchJob::Targeted {
                trigger: TriggerId::CursorEnter,
                target: next,
                ctx,
            });
        }
    }

    fn push_mouse_down_clear_job_if_changed(
        &self,
        jobs: &mut Vec<DispatchJob>,
        ctx: DispatchCtx,
        next_active_idx: Option<NodeIdx>,
    ) {
        let previous_active_idx = self
            .mouse_down_active_id
            .as_ref()
            .and_then(|id| self.dispatch_registry.node_idx(id));

        if previous_active_idx == next_active_idx {
            return;
        }

        if let Some(previous) = previous_active_idx {
            jobs.push(DispatchJob::Targeted {
                trigger: TriggerId::MouseDownStyleClear,
                target: previous,
                ctx,
            });
        }
    }

    fn append_primary_dispatch_jobs(
        &self,
        event: &InputEvent,
        primary_trigger: Option<TriggerId>,
        focused_idx: Option<NodeIdx>,
        ctx: DispatchCtx,
        jobs: &mut Vec<DispatchJob>,
    ) {
        let Some(trigger) = primary_trigger else {
            return;
        };

        let mut pointer_trigger_handled = false;
        let mut release_confirmed_target: Option<ElementId> = None;
        let pointer_job = match event {
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left" && *action == ACTION_PRESS => {
                pointer_trigger_handled = true;
                if self.scrollbar_interaction.is_captured()
                    || hit_test_scrollbar(&self.registry, *x, *y).is_some()
                {
                    None
                } else {
                    Some(DispatchJob::Pointed {
                        trigger,
                        x: *x,
                        y: *y,
                        ctx,
                    })
                }
            }
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left" && *action == ACTION_RELEASE => {
                pointer_trigger_handled = true;
                let consumed_by_scrollbar = self.scrollbar_interaction.is_captured();
                if consumed_by_scrollbar {
                    None
                } else {
                    release_confirmed_target = self.confirmed_pointer_release_target(*x, *y);

                    Some(DispatchJob::Pointed {
                        trigger,
                        x: *x,
                        y: *y,
                        ctx,
                    })
                }
            }
            _ => None,
        };

        if let Some(job) = pointer_job {
            jobs.push(job);
        } else if !pointer_trigger_handled {
            let job = if let Some(target) = focused_idx {
                DispatchJob::Targeted {
                    trigger,
                    target,
                    ctx,
                }
            } else {
                DispatchJob::Untargeted { trigger, ctx }
            };
            jobs.push(job);
        }

        if let Some(target_id) = release_confirmed_target
            && let Some(target) = self.dispatch_registry.node_idx(&target_id)
        {
            if self.node_has_flag(&target_id, EVENT_CLICK) {
                jobs.push(DispatchJob::Targeted {
                    trigger: TriggerId::CursorClickRelease,
                    target,
                    ctx,
                });
            }

            if self.node_has_flag(&target_id, EVENT_PRESS) {
                jobs.push(DispatchJob::Targeted {
                    trigger: TriggerId::CursorPressRelease,
                    target,
                    ctx,
                });
            }
        }
    }

    fn append_hover_and_style_jobs(
        &self,
        event: &InputEvent,
        ctx: DispatchCtx,
        jobs: &mut Vec<DispatchJob>,
    ) {
        match event {
            InputEvent::CursorPos { x, y } => {
                let hit_idx = self.hover_target_idx_at(*x, *y);

                self.push_hover_transition_jobs(jobs, ctx, hit_idx);

                if hit_idx.is_some() {
                    jobs.push(DispatchJob::Pointed {
                        trigger: TriggerId::CursorMove,
                        x: *x,
                        y: *y,
                        ctx,
                    });
                }
            }
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left"
                && (*action == crate::input::ACTION_PRESS
                    || *action == crate::input::ACTION_RELEASE) =>
            {
                let next_hover_idx = self.hover_target_idx_at(*x, *y);
                self.push_hover_transition_jobs(jobs, ctx, next_hover_idx);

                let next_mouse_down_active_idx = if *action == crate::input::ACTION_PRESS {
                    if self.scrollbar_interaction.is_captured()
                        || hit_test_scrollbar(&self.registry, *x, *y).is_some()
                    {
                        None
                    } else {
                        hit_test_with_flag(&self.registry, *x, *y, EVENT_MOUSE_DOWN_STYLE)
                            .as_ref()
                            .and_then(|id| self.dispatch_registry.node_idx(id))
                    }
                } else {
                    None
                };

                self.push_mouse_down_clear_job_if_changed(jobs, ctx, next_mouse_down_active_idx);
            }
            InputEvent::CursorEntered { entered } if !*entered => {
                self.push_hover_transition_jobs(jobs, ctx, None);
                self.push_mouse_down_clear_job_if_changed(jobs, ctx, None);
            }
            InputEvent::Focused { focused } if !*focused => {
                self.push_mouse_down_clear_job_if_changed(jobs, ctx, None);
            }
            _ => {}
        }
    }

    fn append_scroll_runtime_jobs(
        &self,
        event: &InputEvent,
        ctx: DispatchCtx,
        jobs: &mut Vec<DispatchJob>,
    ) {
        match event {
            InputEvent::CursorScroll { .. } => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::CursorScrollDispatch,
                    ctx,
                });
            }
            InputEvent::CursorPos { .. } => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::CursorDragScroll,
                    ctx,
                });
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::ScrollbarThumbDispatch,
                    ctx,
                });
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::ScrollbarHoverDispatch,
                    ctx,
                });
            }
            InputEvent::CursorButton { button, action, .. }
                if button == "left"
                    && (*action == crate::input::ACTION_PRESS
                        || *action == crate::input::ACTION_RELEASE) =>
            {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::ScrollbarThumbDispatch,
                    ctx,
                });
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::ScrollbarHoverDispatch,
                    ctx,
                });
            }
            InputEvent::CursorEntered { entered } if !*entered => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::ScrollbarHoverDispatch,
                    ctx,
                });
            }
            _ => {}
        }
    }

    fn append_text_cursor_jobs(
        &self,
        event: &InputEvent,
        ctx: DispatchCtx,
        jobs: &mut Vec<DispatchJob>,
    ) {
        match event {
            InputEvent::CursorPos { .. } => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::TextCursorDispatch,
                    ctx,
                });
            }
            InputEvent::CursorButton { button, action, .. }
                if ((button == "left"
                    && (*action == crate::input::ACTION_PRESS
                        || *action == crate::input::ACTION_RELEASE))
                    || (button == "middle" && *action == ACTION_PRESS)) =>
            {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::TextCursorDispatch,
                    ctx,
                });
            }
            InputEvent::CursorEntered { entered } if !*entered => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::TextCursorDispatch,
                    ctx,
                });
            }
            _ => {}
        }
    }

    fn append_focus_text_jobs(
        &self,
        event: &InputEvent,
        ctx: DispatchCtx,
        jobs: &mut Vec<DispatchJob>,
    ) {
        match event {
            InputEvent::CursorButton { button, action, .. }
                if (button == "left" || button == "middle") && *action == ACTION_PRESS =>
            {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::CursorFocusDispatch,
                    ctx,
                });
                if button == "middle" {
                    jobs.push(DispatchJob::Untargeted {
                        trigger: TriggerId::TextCommandDispatch,
                        ctx,
                    });
                }
            }
            InputEvent::Key { action, .. } if *action == ACTION_PRESS => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::TextCommandDispatch,
                    ctx,
                });
            }
            InputEvent::TextCommit { .. } => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::TextEditDispatch,
                    ctx,
                });
            }
            InputEvent::TextPreedit { .. } | InputEvent::TextPreeditClear => {
                jobs.push(DispatchJob::Untargeted {
                    trigger: TriggerId::TextPreeditDispatch,
                    ctx,
                });
            }
            _ => {}
        }
    }

    fn dispatch_job_trigger(job: &DispatchJob) -> TriggerId {
        match job {
            DispatchJob::Targeted { trigger, .. }
            | DispatchJob::Pointed { trigger, .. }
            | DispatchJob::Untargeted { trigger, .. } => *trigger,
        }
    }

    fn trigger_for_compiled_jobs(
        primary_trigger: Option<TriggerId>,
        jobs: &[DispatchJob],
    ) -> Option<TriggerId> {
        jobs.first()
            .map(Self::dispatch_job_trigger)
            .or(primary_trigger)
    }

    fn compile_dispatch_jobs_for_event(
        &self,
        event: &InputEvent,
        focused: Option<&ElementId>,
    ) -> CompiledDispatchJobs {
        let focused_idx = focused.and_then(|id| self.dispatch_registry.node_idx(id));
        let ctx = self.dispatch_ctx_for_event(event, focused_idx);
        let primary_trigger = self.trigger_for_input_event(event);
        let mut jobs = Vec::new();

        self.append_primary_dispatch_jobs(event, primary_trigger, focused_idx, ctx, &mut jobs);
        self.append_hover_and_style_jobs(event, ctx, &mut jobs);
        self.append_scroll_runtime_jobs(event, ctx, &mut jobs);
        self.append_text_cursor_jobs(event, ctx, &mut jobs);
        self.append_focus_text_jobs(event, ctx, &mut jobs);

        CompiledDispatchJobs {
            primary_trigger,
            trigger_for_stats: Self::trigger_for_compiled_jobs(primary_trigger, &jobs),
            jobs,
        }
    }

    fn apply_emit_event_action(
        &self,
        trigger: TriggerId,
        element: u32,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) -> Option<dispatch_outcome::ElementEventKind> {
        let Some(kind) = Self::element_event_kind_for_trigger(trigger) else {
            return None;
        };
        let Some(target_id) = self.dispatch_registry.node_id(element) else {
            return None;
        };

        out.element_events.push(dispatch_outcome::ElementEventOut {
            target: dispatch_outcome::node_key(target_id),
            kind,
            payload: None,
        });

        Some(kind)
    }

    fn apply_dispatch_actions(
        &mut self,
        event: &InputEvent,
        trigger: TriggerId,
        actions: &[DispatchRuleAction],
        out: &mut dispatch_outcome::DispatchOutcome,
    ) -> DispatchApplyResult {
        let mut result = DispatchApplyResult::default();

        for action in actions {
            match action {
                DispatchRuleAction::FocusChange { next } => {
                    self.apply_focus_change_action(*next, out);
                }
                DispatchRuleAction::ScrollRequest { element, dx, dy } => {
                    self.apply_scroll_action(*element, *dx, *dy, out);
                }
                DispatchRuleAction::EmitElementEvent { element } => {
                    if let Some(kind) = self.apply_emit_event_action(trigger, *element, out) {
                        if kind == dispatch_outcome::ElementEventKind::MouseDown
                            || kind == dispatch_outcome::ElementEventKind::MouseUp
                        {
                            result.mouse_button_event_emitted = true;
                        }
                    }
                }
                DispatchRuleAction::StyleRuntime {
                    element,
                    kind,
                    active,
                } => {
                    if let Some(target_id) = self.dispatch_registry.node_id(*element) {
                        let style_kind = match kind {
                            StyleRuntimeActionKind::MouseOver => {
                                dispatch_outcome::StyleRuntimeKind::MouseOver
                            }
                            StyleRuntimeActionKind::MouseDown => {
                                dispatch_outcome::StyleRuntimeKind::MouseDown
                            }
                        };

                        let req = dispatch_outcome::StyleRuntimeReqOut {
                            target: dispatch_outcome::node_key(target_id),
                            kind: style_kind,
                            active: *active,
                        };

                        if !out.style_runtime_requests.contains(&req) {
                            out.style_runtime_requests.push(req);
                        }
                    }
                }
                DispatchRuleAction::EmitScrollRequestsFromEvent => {
                    for (id, dx, dy) in self.scroll_requests(event) {
                        Self::push_unique_scroll_request(out, &id, dx, dy);
                    }
                }
                DispatchRuleAction::EmitScrollbarThumbDragRequestsFromEvent => {
                    for request in self.scrollbar_thumb_drag_requests(event) {
                        let converted = match request {
                            ScrollbarThumbDragRequest::X { element_id, dx } => {
                                dispatch_outcome::ScrollbarThumbDragReqOut {
                                    target: dispatch_outcome::node_key(&element_id),
                                    axis: dispatch_outcome::ScrollbarAxisOut::X,
                                    delta: dispatch_outcome::milli(dx),
                                }
                            }
                            ScrollbarThumbDragRequest::Y { element_id, dy } => {
                                dispatch_outcome::ScrollbarThumbDragReqOut {
                                    target: dispatch_outcome::node_key(&element_id),
                                    axis: dispatch_outcome::ScrollbarAxisOut::Y,
                                    delta: dispatch_outcome::milli(dy),
                                }
                            }
                        };

                        if !out.scrollbar_thumb_drag_requests.contains(&converted) {
                            out.scrollbar_thumb_drag_requests.push(converted);
                        }
                    }
                }
                DispatchRuleAction::EmitScrollbarHoverRequestsFromEvent => {
                    for request in self.scrollbar_hover_requests(event) {
                        let converted = match request {
                            ScrollbarHoverRequest::X {
                                element_id,
                                hovered,
                            } => dispatch_outcome::ScrollbarHoverReqOut {
                                target: dispatch_outcome::node_key(&element_id),
                                axis: dispatch_outcome::ScrollbarAxisOut::X,
                                hovered,
                            },
                            ScrollbarHoverRequest::Y {
                                element_id,
                                hovered,
                            } => dispatch_outcome::ScrollbarHoverReqOut {
                                target: dispatch_outcome::node_key(&element_id),
                                axis: dispatch_outcome::ScrollbarAxisOut::Y,
                                hovered,
                            },
                        };

                        if !out.scrollbar_hover_requests.contains(&converted) {
                            out.scrollbar_hover_requests.push(converted);
                        }
                    }
                }
                DispatchRuleAction::EmitWindowResizeFromEvent => {
                    if let InputEvent::Resized {
                        width,
                        height,
                        scale_factor,
                    } = event
                    {
                        let req = dispatch_outcome::WindowResizeReqOut {
                            width: *width,
                            height: *height,
                            scale: dispatch_outcome::milli(*scale_factor),
                        };

                        if !out.window_resize_requests.contains(&req) {
                            out.window_resize_requests.push(req);
                        }
                    }
                }
                DispatchRuleAction::EmitFocusChangeFromEvent => {
                    if let Some(next_focus) = self.text_input_focus_request(event) {
                        out.focus_change =
                            Some(next_focus.as_ref().map(dispatch_outcome::node_key));

                        if let Some(focused_id) = next_focus.as_ref() {
                            for (id, dx, dy) in self.focus_reveal_scroll_requests(focused_id) {
                                Self::push_unique_scroll_request(out, &id, dx, dy);
                            }
                        }
                    }
                }
                DispatchRuleAction::EmitTextCommandRequestsFromEvent => {
                    if let Some((element_id, request)) = self.text_input_command_request(event) {
                        Self::push_unique_text_command_request(out, &element_id, request);
                    }
                }
                DispatchRuleAction::EmitTextCursorRequestsFromEvent => {
                    for request in self.text_input_cursor_requests(event) {
                        Self::push_unique_text_cursor_request(out, request);
                    }
                }
                DispatchRuleAction::EmitTextEditRequestsFromEvent => {
                    if let Some((element_id, request)) = self.text_input_edit_request(event) {
                        self.push_text_edit_request_and_change(&element_id, &request, out);
                    }
                }
                DispatchRuleAction::EmitTextPreeditRequestsFromEvent => {
                    if let Some((element_id, request)) = self.text_input_preedit_request(event) {
                        Self::push_unique_text_preedit_request(out, &element_id, request);
                    }
                }
                DispatchRuleAction::TextEdit { element, request } => {
                    if let Some(target_id) = self.dispatch_registry.node_id(*element) {
                        self.push_text_edit_request_and_change(target_id, request, out);
                    }
                }
            }
        }

        result
    }

    fn apply_compiled_dispatch_jobs(
        &mut self,
        event: &InputEvent,
        jobs: &[DispatchJob],
        out: &mut dispatch_outcome::DispatchOutcome,
    ) -> DispatchApplyResult {
        let mut combined = DispatchApplyResult::default();

        for job in jobs {
            let trigger = match job {
                DispatchJob::Targeted { trigger, .. }
                | DispatchJob::Pointed { trigger, .. }
                | DispatchJob::Untargeted { trigger, .. } => *trigger,
            };

            if let Some(actions) = self.dispatch_registry.resolve_actions_for_job(job) {
                combined.matched_rule = true;
                let actions = actions.to_vec();
                combined.merge(self.apply_dispatch_actions(event, trigger, &actions, out));
            }
        }

        combined
    }

    fn move_mouse_button_events_to_end(out: &mut dispatch_outcome::DispatchOutcome) {
        let mut deferred = Vec::new();
        out.element_events.retain(|event| {
            let is_mouse_button = matches!(
                event.kind,
                dispatch_outcome::ElementEventKind::MouseDown
                    | dispatch_outcome::ElementEventKind::MouseUp
            );

            if is_mouse_button {
                deferred.push(event.clone());
            }

            !is_mouse_button
        });
        out.element_events.extend(deferred);
    }

    fn synthesize_focus_transition_events(
        out: &mut dispatch_outcome::DispatchOutcome,
        previous_focus: Option<&ElementId>,
    ) {
        let Some(next_focus) = out.focus_change.clone() else {
            return;
        };

        let previous_focus_key = previous_focus.map(dispatch_outcome::node_key);
        if previous_focus_key == next_focus {
            return;
        }

        if let Some(prev_key) = previous_focus_key {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: prev_key,
                kind: dispatch_outcome::ElementEventKind::Blur,
                payload: None,
            });
        }

        if let Some(next_key) = next_focus {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: next_key,
                kind: dispatch_outcome::ElementEventKind::Focus,
                payload: None,
            });
        }
    }

    fn finalize_preview_dispatch_outcome(
        out: &mut dispatch_outcome::DispatchOutcome,
        previous_focus: Option<&ElementId>,
        mouse_button_event_emitted: bool,
    ) {
        if mouse_button_event_emitted {
            Self::move_mouse_button_events_to_end(out);
        }
        Self::synthesize_focus_transition_events(out, previous_focus);
    }

    fn dispatch_outcome_has_output(out: &dispatch_outcome::DispatchOutcome) -> bool {
        out.focus_change.is_some()
            || !out.element_events.is_empty()
            || !out.scroll_requests.is_empty()
            || !out.window_resize_requests.is_empty()
            || !out.text_cursor_requests.is_empty()
            || !out.text_command_requests.is_empty()
            || !out.text_edit_requests.is_empty()
            || !out.text_preedit_requests.is_empty()
            || !out.scrollbar_thumb_drag_requests.is_empty()
            || !out.scrollbar_hover_requests.is_empty()
            || !out.style_runtime_requests.is_empty()
    }

    pub(crate) fn trigger_for_input_event(&self, event: &InputEvent) -> Option<TriggerId> {
        match event {
            InputEvent::CursorButton { button, action, .. }
                if button == "left" && *action == ACTION_PRESS =>
            {
                Some(TriggerId::CursorButtonLeftPress)
            }
            InputEvent::CursorButton { button, action, .. }
                if button == "left" && *action == ACTION_RELEASE =>
            {
                Some(TriggerId::CursorButtonLeftRelease)
            }
            InputEvent::Key { key, action, .. } if *action == ACTION_PRESS => {
                if key.eq_ignore_ascii_case("left") {
                    Some(TriggerId::KeyLeftPress)
                } else if key.eq_ignore_ascii_case("right") {
                    Some(TriggerId::KeyRightPress)
                } else if key.eq_ignore_ascii_case("up") {
                    Some(TriggerId::KeyUpPress)
                } else if key.eq_ignore_ascii_case("down") {
                    Some(TriggerId::KeyDownPress)
                } else if key.eq_ignore_ascii_case("tab") {
                    Some(TriggerId::KeyTabPress)
                } else if key.eq_ignore_ascii_case("enter") {
                    Some(TriggerId::KeyEnterPress)
                } else if key.eq_ignore_ascii_case("home") {
                    Some(TriggerId::KeyHomePress)
                } else if key.eq_ignore_ascii_case("end") {
                    Some(TriggerId::KeyEndPress)
                } else if key.eq_ignore_ascii_case("backspace") {
                    Some(TriggerId::KeyBackspacePress)
                } else if key.eq_ignore_ascii_case("delete") {
                    Some(TriggerId::KeyDeletePress)
                } else {
                    None
                }
            }
            InputEvent::Resized { .. } => Some(TriggerId::WindowResizeDispatch),
            InputEvent::Focused { focused } if !*focused => Some(TriggerId::WindowFocusLost),
            _ => None,
        }
    }

    pub(crate) fn preview_dispatch_outcome(
        &self,
        event: &InputEvent,
        focused: Option<&ElementId>,
    ) -> DispatchPreview {
        let previous_focus = focused.cloned();

        let mut preview_processor = self.clone();
        let mut out = dispatch_outcome::DispatchOutcome::default();
        let compiled = preview_processor.compile_dispatch_jobs_for_event(event, focused);
        let had_jobs = !compiled.jobs.is_empty();
        let dispatch_applied =
            preview_processor.apply_compiled_dispatch_jobs(event, &compiled.jobs, &mut out);

        Self::finalize_preview_dispatch_outcome(
            &mut out,
            previous_focus.as_ref(),
            dispatch_applied.mouse_button_event_emitted,
        );

        let has_output = Self::dispatch_outcome_has_output(&out);

        let outcome = if has_output || dispatch_applied.matched_rule {
            Some(out)
        } else {
            None
        };

        DispatchPreview {
            outcome,
            had_jobs,
            trigger_for_stats: compiled.trigger_for_stats.or(compiled.primary_trigger),
        }
    }

    #[cfg(test)]
    pub(crate) fn preview_outcome_for_test(
        &self,
        event: &InputEvent,
        focused: Option<&ElementId>,
    ) -> Option<dispatch_outcome::DispatchOutcome> {
        self.preview_dispatch_outcome(event, focused).outcome
    }

    #[cfg(test)]
    pub(crate) fn dispatch_registry(&self) -> &EventRegistry {
        &self.dispatch_registry
    }

    pub fn focus_reveal_scroll_requests(&self, id: &ElementId) -> Vec<(ElementId, f32, f32)> {
        self.registry
            .iter()
            .find(|node| node.id == *id)
            .map(|node| {
                node.focus_reveal_scrolls
                    .iter()
                    .map(|request| (request.element_id.clone(), request.dx, request.dy))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn cycle_focus(&self, reverse: bool) -> Option<ElementId> {
        let focusable: Vec<ElementId> = self
            .registry
            .iter()
            .filter(|node| node.flags & EVENT_FOCUSABLE != 0)
            .map(|node| node.id.clone())
            .collect();

        if focusable.is_empty() {
            return None;
        }

        let next_index = match self
            .focused_id
            .as_ref()
            .and_then(|focused| focusable.iter().position(|id| id == focused))
        {
            Some(current) if reverse => {
                if current == 0 {
                    focusable.len() - 1
                } else {
                    current - 1
                }
            }
            Some(current) => (current + 1) % focusable.len(),
            None if reverse => focusable.len() - 1,
            None => 0,
        };

        Some(focusable[next_index].clone())
    }

    fn node_has_flag(&self, id: &ElementId, flag: u16) -> bool {
        self.registry
            .iter()
            .find(|node| node.id == *id)
            .is_some_and(|node| node.flags & flag != 0)
    }

    pub fn scrollbar_thumb_drag_requests(
        &mut self,
        event: &InputEvent,
    ) -> Vec<ScrollbarThumbDragRequest> {
        let mut requests = Vec::new();
        requests.extend(self.handle_scrollbar_button_requests(event));
        requests.extend(self.handle_scrollbar_drag_requests(event));
        requests
    }

    pub fn scrollbar_hover_requests(&mut self, event: &InputEvent) -> Vec<ScrollbarHoverRequest> {
        let Some((previous, next_hover)) = self.apply_scrollbar_hover_transition(event) else {
            return Vec::new();
        };

        let mut requests = Vec::with_capacity(2);
        if let Some(prev) = previous {
            requests.push(Self::hover_request(prev, false));
        }
        if let Some(next) = next_hover {
            requests.push(Self::hover_request(next, true));
        }
        requests
    }

    fn apply_scrollbar_hover_transition(
        &mut self,
        event: &InputEvent,
    ) -> Option<(Option<ScrollbarThumbHover>, Option<ScrollbarThumbHover>)> {
        let next_hover = self.next_scrollbar_thumb_hover(event)?;
        if self.hovered_scrollbar_thumb == next_hover {
            return None;
        }

        let previous = self.hovered_scrollbar_thumb.clone();
        self.hovered_scrollbar_thumb = next_hover.clone();
        Some((previous, next_hover))
    }

    fn advance_scrollbar_hover_state(&mut self, event: &InputEvent) {
        let _ = self.apply_scrollbar_hover_transition(event);
    }

    #[cfg(test)]
    pub fn mouse_over_requests(&mut self, event: &InputEvent) -> Vec<MouseOverRequest> {
        let Some((previous, next_active)) = self.apply_mouse_over_transition(event) else {
            return Vec::new();
        };

        let mut requests = Vec::with_capacity(2);
        if let Some(id) = previous {
            requests.push(MouseOverRequest::SetMouseOverActive {
                element_id: id,
                active: false,
            });
        }
        if let Some(id) = next_active {
            requests.push(MouseOverRequest::SetMouseOverActive {
                element_id: id,
                active: true,
            });
        }

        requests
    }

    fn next_mouse_over_active_for_event(&self, event: &InputEvent) -> Option<Option<ElementId>> {
        match event {
            InputEvent::CursorPos { x, y } => Some(hit_test_with_flag(
                &self.registry,
                *x,
                *y,
                EVENT_MOUSE_OVER_STYLE,
            )),
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left"
                && (*action == crate::input::ACTION_PRESS
                    || *action == crate::input::ACTION_RELEASE) =>
            {
                Some(hit_test_with_flag(
                    &self.registry,
                    *x,
                    *y,
                    EVENT_MOUSE_OVER_STYLE,
                ))
            }
            InputEvent::CursorEntered { entered } if !*entered => Some(None),
            _ => None,
        }
    }

    fn apply_mouse_over_transition(
        &mut self,
        event: &InputEvent,
    ) -> Option<(Option<ElementId>, Option<ElementId>)> {
        let next_active = self.next_mouse_over_active_for_event(event)?;
        if self.mouse_over_active_id == next_active {
            return None;
        }

        let previous = self.mouse_over_active_id.clone();
        self.mouse_over_active_id = next_active.clone();
        Some((previous, next_active))
    }

    fn advance_mouse_over_state(&mut self, event: &InputEvent) {
        let _ = self.apply_mouse_over_transition(event);
    }

    #[cfg(test)]
    pub fn mouse_down_requests(&mut self, event: &InputEvent) -> Vec<MouseDownRequest> {
        let Some((previous, next_active)) = self.apply_mouse_down_transition(event) else {
            return Vec::new();
        };

        let mut requests = Vec::with_capacity(2);
        if let Some(id) = previous {
            requests.push(MouseDownRequest::SetMouseDownActive {
                element_id: id,
                active: false,
            });
        }
        if let Some(id) = next_active {
            requests.push(MouseDownRequest::SetMouseDownActive {
                element_id: id,
                active: true,
            });
        }

        requests
    }

    fn next_mouse_down_active_for_event(&self, event: &InputEvent) -> Option<Option<ElementId>> {
        match event {
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left" && *action == crate::input::ACTION_PRESS => Some(
                hit_test_with_flag(&self.registry, *x, *y, EVENT_MOUSE_DOWN_STYLE),
            ),
            InputEvent::CursorButton { button, action, .. }
                if button == "left" && *action == crate::input::ACTION_RELEASE =>
            {
                Some(None)
            }
            InputEvent::CursorEntered { entered } if !*entered => Some(None),
            InputEvent::Focused { focused } if !*focused => Some(None),
            _ => None,
        }
    }

    fn apply_mouse_down_transition(
        &mut self,
        event: &InputEvent,
    ) -> Option<(Option<ElementId>, Option<ElementId>)> {
        let next_active = self.next_mouse_down_active_for_event(event)?;
        if self.mouse_down_active_id == next_active {
            return None;
        }

        let previous = self.mouse_down_active_id.clone();
        self.mouse_down_active_id = next_active.clone();
        Some((previous, next_active))
    }

    fn advance_mouse_down_state(&mut self, event: &InputEvent) {
        let _ = self.apply_mouse_down_transition(event);
    }

    fn next_scrollbar_thumb_hover(
        &self,
        event: &InputEvent,
    ) -> Option<Option<ScrollbarThumbHover>> {
        match event {
            InputEvent::CursorPos { x, y } => Some(self.current_scrollbar_thumb_hover(*x, *y)),
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left"
                && (*action == crate::input::ACTION_PRESS
                    || *action == crate::input::ACTION_RELEASE) =>
            {
                Some(self.current_scrollbar_thumb_hover(*x, *y))
            }
            InputEvent::CursorEntered { entered } if !*entered => Some(None),
            _ => None,
        }
    }

    fn current_scrollbar_thumb_hover(&self, x: f32, y: f32) -> Option<ScrollbarThumbHover> {
        if let Some(drag) = self.scrollbar_interaction.dragging() {
            return Some(ScrollbarThumbHover {
                id: drag.id.clone(),
                axis: drag.axis,
            });
        }
        hit_test_scrollbar(&self.registry, x, y).and_then(thumb_hover_from_hit)
    }

    fn hover_request(hover: ScrollbarThumbHover, hovered: bool) -> ScrollbarHoverRequest {
        match hover.axis {
            ScrollbarAxis::X => ScrollbarHoverRequest::X {
                element_id: hover.id,
                hovered,
            },
            ScrollbarAxis::Y => ScrollbarHoverRequest::Y {
                element_id: hover.id,
                hovered,
            },
        }
    }

    pub fn scroll_requests(&mut self, event: &InputEvent) -> Vec<(ElementId, f32, f32)> {
        let mut requests = Vec::new();
        requests.extend(self.handle_drag_scroll_requests(event));
        requests.extend(self.handle_scroll_requests(event));
        requests.extend(self.handle_key_scroll_requests(event));
        requests
    }

    fn advance_scroll_state(&mut self, event: &InputEvent) {
        let _ = self.update_drag_scroll_state(event);
    }

    fn handle_key_scroll_requests(&self, event: &InputEvent) -> Vec<(ElementId, f32, f32)> {
        let InputEvent::Key { key, action, mods } = event else {
            return Vec::new();
        };

        if *action != ACTION_PRESS {
            return Vec::new();
        }

        if *mods & (MOD_CTRL | MOD_ALT | MOD_META) != 0 {
            return Vec::new();
        }

        let direction = match key.as_str() {
            "left" => KeyScrollDirection::Left,
            "right" => KeyScrollDirection::Right,
            "up" => KeyScrollDirection::Up,
            "down" => KeyScrollDirection::Down,
            _ => return Vec::new(),
        };

        if let Some(descriptor) = self.focused_text_input_descriptor() {
            let text_key = match direction {
                KeyScrollDirection::Left => "left",
                KeyScrollDirection::Right => "right",
                KeyScrollDirection::Up => "up",
                KeyScrollDirection::Down => "down",
            };

            if self
                .text_input_key_edit_request(descriptor, text_key, *mods)
                .is_some()
            {
                return Vec::new();
            }
        }

        let target = if let Some(focused_id) = self.focused_id.as_ref() {
            self.registry
                .iter()
                .find(|node| node.id == *focused_id)
                .and_then(|node| node.key_scroll_targets.for_direction(direction))
                .or_else(|| self.first_visible_scroll_target(direction))
        } else {
            self.first_visible_scroll_target(direction)
        };

        let Some(target) = target else {
            return Vec::new();
        };

        let (dx, dy) = match direction {
            KeyScrollDirection::Left => (SCROLL_LINE_PIXELS, 0.0),
            KeyScrollDirection::Right => (-SCROLL_LINE_PIXELS, 0.0),
            KeyScrollDirection::Up => (0.0, SCROLL_LINE_PIXELS),
            KeyScrollDirection::Down => (0.0, -SCROLL_LINE_PIXELS),
        };

        vec![(target, dx, dy)]
    }

    fn first_visible_scroll_target(&self, direction: KeyScrollDirection) -> Option<ElementId> {
        let flag = match direction {
            KeyScrollDirection::Left => EVENT_SCROLL_X_NEG,
            KeyScrollDirection::Right => EVENT_SCROLL_X_POS,
            KeyScrollDirection::Up => EVENT_SCROLL_Y_NEG,
            KeyScrollDirection::Down => EVENT_SCROLL_Y_POS,
        };

        self.registry
            .iter()
            .find(|node| node.visible && (node.flags & flag != 0))
            .map(|node| node.id.clone())
    }

    fn handle_scrollbar_button_requests(
        &mut self,
        event: &InputEvent,
    ) -> Vec<ScrollbarThumbDragRequest> {
        self.update_scrollbar_button_state(event)
            .and_then(|(id, axis, current_scroll, target_scroll)| {
                Self::scrollbar_drag_request(&id, axis, current_scroll, target_scroll)
            })
            .map(|request| vec![request])
            .unwrap_or_default()
    }

    fn update_scrollbar_button_state(
        &mut self,
        event: &InputEvent,
    ) -> Option<(ElementId, ScrollbarAxis, f32, f32)> {
        let InputEvent::CursorButton {
            button,
            action,
            x,
            y,
            ..
        } = event
        else {
            return None;
        };

        if button != "left" {
            return None;
        }

        if *action == crate::input::ACTION_RELEASE {
            self.scrollbar_interaction.clear();
            return None;
        }

        if *action != crate::input::ACTION_PRESS {
            return None;
        }

        let Some(hit) = hit_test_scrollbar(&self.registry, *x, *y) else {
            return None;
        };

        self.drag_consumed = true;

        let pointer_axis = axis_coord(hit.axis, *x, *y);
        let (pointer_offset, target_scroll) = match hit.area {
            ScrollbarHitArea::Thumb => (
                (pointer_axis - hit.node.thumb_start).clamp(0.0, hit.node.thumb_len),
                hit.node.scroll_offset,
            ),
            ScrollbarHitArea::Track => {
                let pointer_offset = hit.node.thumb_len / 2.0;
                let target_scroll = scroll_from_pointer(
                    pointer_axis,
                    hit.node.track_start,
                    hit.node.track_len,
                    pointer_offset,
                    hit.node.scroll_range,
                );
                (pointer_offset, target_scroll)
            }
        };

        self.scrollbar_interaction.set_dragging(ScrollbarDragState {
            id: hit.id.clone(),
            axis: hit.axis,
            track_start: hit.node.track_start,
            track_len: hit.node.track_len,
            thumb_len: hit.node.thumb_len,
            pointer_offset,
            scroll_range: hit.node.scroll_range,
            current_scroll: target_scroll,
        });

        if hit.area == ScrollbarHitArea::Track {
            return Some((hit.id, hit.axis, hit.node.scroll_offset, target_scroll));
        }

        None
    }

    fn handle_scrollbar_drag_requests(
        &mut self,
        event: &InputEvent,
    ) -> Vec<ScrollbarThumbDragRequest> {
        self.update_scrollbar_drag_state(event)
            .and_then(|(id, axis, current_scroll, target_scroll)| {
                Self::scrollbar_drag_request(&id, axis, current_scroll, target_scroll)
            })
            .map(|request| vec![request])
            .unwrap_or_default()
    }

    fn update_scrollbar_drag_state(
        &mut self,
        event: &InputEvent,
    ) -> Option<(ElementId, ScrollbarAxis, f32, f32)> {
        let InputEvent::CursorPos { x, y } = event else {
            return None;
        };

        let Some(state) = self.scrollbar_interaction.dragging() else {
            return None;
        };

        let id = state.id.clone();
        let axis = state.axis;
        let track_start = state.track_start;
        let track_len = state.track_len;
        let pointer_offset = state.pointer_offset;
        let scroll_range = state.scroll_range;
        let current_scroll = state.current_scroll;

        if !self.registry.iter().any(|node| node.id == id) {
            self.scrollbar_interaction.clear();
            return None;
        }

        let pointer_axis = axis_coord(axis, *x, *y);
        let target_scroll = scroll_from_pointer(
            pointer_axis,
            track_start,
            track_len,
            pointer_offset,
            scroll_range,
        );

        if (target_scroll - current_scroll).abs() < f32::EPSILON {
            return None;
        }

        if let Some(state) = self.scrollbar_interaction.dragging_mut() {
            state.current_scroll = target_scroll;
        }

        Some((id, axis, current_scroll, target_scroll))
    }

    fn advance_scrollbar_interaction_state(&mut self, event: &InputEvent) {
        let _ = self.update_scrollbar_button_state(event);
        let _ = self.update_scrollbar_drag_state(event);
    }

    fn scrollbar_drag_request(
        id: &ElementId,
        axis: ScrollbarAxis,
        current_scroll: f32,
        target_scroll: f32,
    ) -> Option<ScrollbarThumbDragRequest> {
        let delta = current_scroll - target_scroll;
        if delta.abs() < f32::EPSILON {
            return None;
        }

        Some(match axis {
            ScrollbarAxis::X => ScrollbarThumbDragRequest::X {
                element_id: id.clone(),
                dx: delta,
            },
            ScrollbarAxis::Y => ScrollbarThumbDragRequest::Y {
                element_id: id.clone(),
                dy: delta,
            },
        })
    }

    fn handle_drag_scroll_requests(&mut self, event: &InputEvent) -> Vec<(ElementId, f32, f32)> {
        let Some((dx, dy, x, y)) = self.update_drag_scroll_state(event) else {
            return Vec::new();
        };

        let mut requests = Vec::new();

        if dx != 0.0 {
            let flag = if dx > 0.0 {
                EVENT_SCROLL_X_NEG
            } else {
                EVENT_SCROLL_X_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, x, y, flag) {
                requests.push((id, dx, 0.0));
            }
        }

        if dy != 0.0 {
            let flag = if dy > 0.0 {
                EVENT_SCROLL_Y_NEG
            } else {
                EVENT_SCROLL_Y_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, x, y, flag) {
                requests.push((id, 0.0, dy));
            }
        }

        requests
    }

    fn update_drag_scroll_state(&mut self, event: &InputEvent) -> Option<(f32, f32, f32, f32)> {
        if self.scrollbar_interaction.blocks_content_drag() {
            return None;
        }

        let InputEvent::CursorPos { x, y } = event else {
            return None;
        };

        let Some((start_x, start_y)) = self.drag_start else {
            return None;
        };

        let last_pos = self.drag_last_pos.unwrap_or((start_x, start_y));
        let dx = x - last_pos.0;
        let dy = y - last_pos.1;

        if !self.drag_active {
            let total_dx = x - start_x;
            let total_dy = y - start_y;
            let distance = (total_dx * total_dx + total_dy * total_dy).sqrt();
            if distance < DRAG_DEADZONE {
                return None;
            }
            self.drag_active = true;
            self.drag_consumed = true;
        }

        self.drag_last_pos = Some((*x, *y));

        if dx == 0.0 && dy == 0.0 {
            return None;
        }

        Some((dx, dy, *x, *y))
    }

    fn handle_scroll_requests(&mut self, event: &InputEvent) -> Vec<(ElementId, f32, f32)> {
        let InputEvent::CursorScroll { dx, dy, x, y } = event else {
            return Vec::new();
        };
        let mut requests = Vec::new();

        if *dx != 0.0 {
            let flag = if *dx > 0.0 {
                EVENT_SCROLL_X_NEG
            } else {
                EVENT_SCROLL_X_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                requests.push((id, *dx, 0.0));
            }
        }

        if *dy != 0.0 {
            let flag = if *dy > 0.0 {
                EVENT_SCROLL_Y_NEG
            } else {
                EVENT_SCROLL_Y_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                requests.push((id, 0.0, *dy));
            }
        }

        requests
    }
}

pub(crate) fn send_element_event(pid: LocalPid, element_id: &ElementId, event: Atom) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        let mut bin = OwnedBinary::new(element_id.0.len()).unwrap();
        bin.as_mut_slice().copy_from_slice(&element_id.0);
        let id_bin = bin.release(inner_env);
        (emerge_skia_event(), (id_bin, event)).encode(inner_env)
    });
}

pub(crate) fn send_element_event_with_string_payload(
    pid: LocalPid,
    element_id: &ElementId,
    event: Atom,
    value: &str,
) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        let mut bin = OwnedBinary::new(element_id.0.len()).unwrap();
        bin.as_mut_slice().copy_from_slice(&element_id.0);
        let id_bin = bin.release(inner_env);
        (emerge_skia_event(), (id_bin, event, value.to_string())).encode(inner_env)
    });
}

pub(crate) fn send_input_event(pid: LocalPid, event: &InputEvent) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (emerge_skia_event(), event).encode(inner_env)
    });
}

rustler::atoms! {
    emerge_skia_event,
    click,
    press,
    change,
    focus,
    blur,
    mouse_down,
    mouse_up,
    mouse_enter,
    mouse_leave,
    mouse_move,
}

pub(crate) fn click_atom() -> Atom {
    click()
}

pub(crate) fn press_atom() -> Atom {
    press()
}

pub(crate) fn change_atom() -> Atom {
    change()
}

pub(crate) fn focus_atom() -> Atom {
    focus()
}

pub(crate) fn blur_atom() -> Atom {
    blur()
}

pub(crate) fn mouse_down_atom() -> Atom {
    mouse_down()
}

pub(crate) fn mouse_up_atom() -> Atom {
    mouse_up()
}

pub(crate) fn mouse_enter_atom() -> Atom {
    mouse_enter()
}

pub(crate) fn mouse_leave_atom() -> Atom {
    mouse_leave()
}

pub(crate) fn mouse_move_atom() -> Atom {
    mouse_move()
}

#[cfg(test)]
mod tests {
    use super::registry::{
        DispatchCtx, DispatchJob, DispatchRuleAction, EventRegistry, ScrollDirection, TriggerId,
    };
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{Element, ElementKind, ElementTree};

    fn make_element(
        id: u8,
        attrs: Attrs,
        frame: crate::tree::element::Frame,
        children: Vec<ElementId>,
    ) -> Element {
        let mut element = Element::with_attrs(
            ElementId::from_term_bytes(vec![id]),
            ElementKind::El,
            Vec::new(),
            attrs,
        );
        element.frame = Some(frame);
        element.children = children;
        element
    }

    fn make_scrollbar_test_node(id: u8) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            visible: true,
            flags: EVENT_CLICK
                | EVENT_MOUSE_DOWN
                | EVENT_MOUSE_UP
                | EVENT_SCROLL_Y_NEG
                | EVENT_SCROLL_Y_POS,
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: Some(ScrollbarNode {
                track_rect: Rect {
                    x: 95.0,
                    y: 0.0,
                    width: 5.0,
                    height: 100.0,
                },
                thumb_rect: Rect {
                    x: 95.0,
                    y: 20.0,
                    width: 5.0,
                    height: 30.0,
                },
                track_start: 0.0,
                track_len: 70.0,
                thumb_start: 20.0,
                thumb_len: 30.0,
                scroll_offset: 40.0,
                scroll_range: 140.0,
            }),
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn make_dual_scrollbar_test_node(id: u8) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            visible: true,
            flags: EVENT_SCROLL_X_NEG
                | EVENT_SCROLL_X_POS
                | EVENT_SCROLL_Y_NEG
                | EVENT_SCROLL_Y_POS,
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: Some(ScrollbarNode {
                track_rect: Rect {
                    x: 0.0,
                    y: 95.0,
                    width: 100.0,
                    height: 5.0,
                },
                thumb_rect: Rect {
                    x: 20.0,
                    y: 95.0,
                    width: 30.0,
                    height: 5.0,
                },
                track_start: 0.0,
                track_len: 70.0,
                thumb_start: 20.0,
                thumb_len: 30.0,
                scroll_offset: 40.0,
                scroll_range: 140.0,
            }),
            scrollbar_y: Some(ScrollbarNode {
                track_rect: Rect {
                    x: 95.0,
                    y: 0.0,
                    width: 5.0,
                    height: 100.0,
                },
                thumb_rect: Rect {
                    x: 95.0,
                    y: 20.0,
                    width: 5.0,
                    height: 30.0,
                },
                track_start: 0.0,
                track_len: 70.0,
                thumb_start: 20.0,
                thumb_len: 30.0,
                scroll_offset: 40.0,
                scroll_range: 140.0,
            }),
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn make_mouse_over_node(id: u8, x: f32, y: f32, width: f32, height: f32) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x,
                y,
                width,
                height,
            },
            visible: true,
            flags: EVENT_MOUSE_OVER_STYLE,
            self_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn make_mouse_down_style_node(id: u8, x: f32, y: f32, width: f32, height: f32) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x,
                y,
                width,
                height,
            },
            visible: true,
            flags: EVENT_MOUSE_DOWN_STYLE,
            self_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn make_pointer_events_node(id: u8, x: f32, y: f32, width: f32, height: f32) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x,
                y,
                width,
                height,
            },
            visible: true,
            flags: EVENT_CLICK
                | EVENT_PRESS
                | EVENT_MOUSE_DOWN
                | EVENT_MOUSE_UP
                | EVENT_MOUSE_ENTER
                | EVENT_MOUSE_LEAVE
                | EVENT_MOUSE_MOVE,
            self_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn make_text_input_node(id: u8, x: f32, y: f32, width: f32, height: f32) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x,
                y,
                width,
                height,
            },
            visible: true,
            flags: EVENT_TEXT_INPUT | EVENT_FOCUSABLE,
            self_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: Some(TextInputDescriptor {
                content: String::new(),
                content_len: 0,
                cursor: 0,
                selection_anchor: None,
                emit_change: true,
                frame_x: x,
                frame_width: width,
                inset_left: 0.0,
                inset_right: 0.0,
                text_align: TextAlign::Left,
                font_family: "default".to_string(),
                font_size: 16.0,
                font_weight: 400,
                font_italic: false,
                letter_spacing: 0.0,
                word_spacing: 0.0,
            }),
        }
    }

    fn make_pressable_node(id: u8, x: f32, y: f32, width: f32, height: f32) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: Rect {
                x,
                y,
                width,
                height,
            },
            visible: true,
            flags: EVENT_PRESS | EVENT_FOCUSABLE,
            self_rect: Rect {
                x,
                y,
                width,
                height,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn make_scroll_target_node(id: u8, visible: bool, flags: u16) -> EventNode {
        EventNode {
            id: ElementId::from_term_bytes(vec![id]),
            hit_rect: if visible {
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                }
            } else {
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }
            },
            visible,
            flags,
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    #[test]
    fn test_dispatch_registry_focus_order_matches_current_registry_order() {
        let registry = vec![
            make_pressable_node(1, 0.0, 0.0, 20.0, 20.0),
            make_scroll_target_node(2, true, EVENT_CLICK),
            make_text_input_node(3, 0.0, 24.0, 120.0, 30.0),
            make_scroll_target_node(4, false, EVENT_FOCUSABLE),
        ];

        let expected: Vec<ElementId> = registry
            .iter()
            .filter(|node| node.flags & EVENT_FOCUSABLE != 0)
            .map(|node| node.id.clone())
            .collect();

        let dispatch_registry = EventRegistry::from_event_nodes(&registry);
        assert_eq!(dispatch_registry.focus_order_ids(), expected);
    }

    #[test]
    fn test_dispatch_registry_first_visible_scrollable_matches_existing_logic() {
        let registry = vec![
            make_scroll_target_node(1, false, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(3, true, EVENT_SCROLL_Y_POS),
        ];

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(registry.clone());

        let baseline = processor.first_visible_scroll_target(KeyScrollDirection::Down);

        let dispatch_registry = EventRegistry::from_event_nodes(&registry);
        let resolved = dispatch_registry.first_visible_scrollable_id(ScrollDirection::Down);

        assert_eq!(resolved, baseline);
    }

    #[test]
    fn test_dispatch_registry_pointer_candidates_keep_topmost_first_order() {
        let registry = vec![
            make_scroll_target_node(1, true, EVENT_CLICK),
            make_scroll_target_node(2, true, EVENT_CLICK),
            make_scroll_target_node(3, true, EVENT_CLICK),
        ];

        let top_hit = hit_test_with_flag(&registry, 10.0, 10.0, EVENT_CLICK).unwrap();
        assert_eq!(top_hit, ElementId::from_term_bytes(vec![3]));

        let dispatch_registry = EventRegistry::from_event_nodes(&registry);
        let candidate_ids: Vec<ElementId> = dispatch_registry
            .pointer_candidates(TriggerId::CursorButtonLeftPress)
            .iter()
            .filter_map(|idx| dispatch_registry.node_id(*idx).cloned())
            .collect();

        assert_eq!(candidate_ids.first(), Some(&top_hit));
        assert_eq!(
            candidate_ids,
            vec![
                ElementId::from_term_bytes(vec![3]),
                ElementId::from_term_bytes(vec![2]),
                ElementId::from_term_bytes(vec![1]),
            ]
        );
    }

    #[test]
    fn test_rebuild_registry_populates_dispatch_indexes() {
        let mut processor = EventProcessor::new();
        let registry = vec![
            make_pressable_node(1, 0.0, 0.0, 20.0, 20.0),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
        ];

        processor.rebuild_registry(registry);

        let next_focus = processor
            .dispatch_registry()
            .focus_order_next(None, false)
            .and_then(|idx| processor.dispatch_registry().node_id(idx).cloned());
        assert_eq!(next_focus, Some(ElementId::from_term_bytes(vec![1])));

        let first_scroll = processor
            .dispatch_registry()
            .first_visible_scrollable_id(ScrollDirection::Down);
        assert_eq!(first_scroll, Some(ElementId::from_term_bytes(vec![2])));
    }

    fn first_scroll_action(
        dispatch_registry: &EventRegistry,
        actions: &[DispatchRuleAction],
    ) -> Option<(ElementId, f32, f32)> {
        actions.iter().find_map(|action| match action {
            DispatchRuleAction::ScrollRequest { element, dx, dy } => dispatch_registry
                .node_id(*element)
                .cloned()
                .map(|id| (id, *dx, *dy)),
            _ => None,
        })
    }

    fn first_focus_change_action(
        dispatch_registry: &EventRegistry,
        actions: &[DispatchRuleAction],
    ) -> Option<ElementId> {
        actions.iter().find_map(|action| match action {
            DispatchRuleAction::FocusChange { next: Some(next) } => {
                dispatch_registry.node_id(*next).cloned()
            }
            _ => None,
        })
    }

    #[test]
    fn test_dispatch_registry_arrow_down_resolves_without_focus() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            make_scroll_target_node(1, false, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(3, true, EVENT_SCROLL_Y_POS),
        ]);

        let down = InputEvent::Key {
            key: "down".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };
        let baseline_requests = processor.handle_key_scroll_requests(&down);

        let actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Untargeted {
                trigger: TriggerId::KeyDownPress,
                ctx: DispatchCtx::default(),
            })
            .expect("dispatch registry should resolve a no-focus arrow rule");
        let resolved = first_scroll_action(processor.dispatch_registry(), actions)
            .expect("dispatch registry should emit a scroll action");

        assert_eq!(baseline_requests.len(), 1);
        assert_eq!(baseline_requests[0].0, resolved.0);
        assert!((baseline_requests[0].1 - resolved.1).abs() < f32::EPSILON);
        assert!((baseline_requests[0].2 - resolved.2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_dispatch_registry_arrow_uses_focused_directional_matcher_before_fallback() {
        let focused_id = ElementId::from_term_bytes(vec![10]);
        let focused_scroll_id = ElementId::from_term_bytes(vec![11]);
        let fallback_scroll_id = ElementId::from_term_bytes(vec![12]);

        let mut focused = make_pressable_node(10, 0.0, 0.0, 80.0, 30.0);
        focused.key_scroll_targets.down = Some(focused_scroll_id.clone());

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            focused,
            make_scroll_target_node(11, true, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(12, true, EVENT_SCROLL_Y_POS),
        ]);
        processor.focused_id = Some(focused_id.clone());

        let down = InputEvent::Key {
            key: "down".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };
        let baseline_requests = processor.handle_key_scroll_requests(&down);

        let focused_idx = processor
            .dispatch_registry()
            .node_idx(&focused_id)
            .expect("focused node should exist in dispatch registry");
        let actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyDownPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("dispatch registry should resolve focused directional rule");
        let resolved = first_scroll_action(processor.dispatch_registry(), actions)
            .expect("dispatch registry should emit focused scroll action");

        assert_eq!(baseline_requests.len(), 1);
        assert_eq!(baseline_requests[0].0, focused_scroll_id);
        assert_eq!(baseline_requests[0].0, resolved.0);
        assert_ne!(resolved.0, fallback_scroll_id);
    }

    #[test]
    fn test_dispatch_registry_arrow_falls_back_when_focused_has_no_directional_matcher() {
        let focused_id = ElementId::from_term_bytes(vec![20]);
        let fallback_scroll_id = ElementId::from_term_bytes(vec![21]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            make_pressable_node(20, 0.0, 0.0, 80.0, 30.0),
            make_scroll_target_node(21, true, EVENT_SCROLL_Y_POS),
        ]);
        processor.focused_id = Some(focused_id.clone());

        let down = InputEvent::Key {
            key: "down".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };
        let baseline_requests = processor.handle_key_scroll_requests(&down);

        let focused_idx = processor
            .dispatch_registry()
            .node_idx(&focused_id)
            .expect("focused node should exist in dispatch registry");
        let actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyDownPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("dispatch registry should resolve fallback arrow rule");
        let resolved = first_scroll_action(processor.dispatch_registry(), actions)
            .expect("dispatch registry should emit fallback scroll action");

        assert_eq!(baseline_requests.len(), 1);
        assert_eq!(baseline_requests[0].0, fallback_scroll_id);
        assert_eq!(baseline_requests[0].0, resolved.0);
    }

    #[test]
    fn test_dispatch_registry_tab_focus_change_resolves_next_and_prev() {
        let id1 = ElementId::from_term_bytes(vec![30]);
        let id2 = ElementId::from_term_bytes(vec![31]);
        let id3 = ElementId::from_term_bytes(vec![32]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            make_pressable_node(30, 0.0, 0.0, 80.0, 30.0),
            make_pressable_node(31, 0.0, 40.0, 80.0, 30.0),
            make_pressable_node(32, 0.0, 80.0, 80.0, 30.0),
        ]);

        processor.focused_id = Some(id2.clone());
        let baseline_next = processor.cycle_focus(false);

        let focused_idx = processor
            .dispatch_registry()
            .node_idx(&id2)
            .expect("focused node should exist in dispatch registry");
        let next_actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyTabPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("dispatch registry should resolve tab forward rule");
        let next = first_focus_change_action(processor.dispatch_registry(), next_actions)
            .expect("focus action");

        assert_eq!(baseline_next, Some(id3.clone()));
        assert_eq!(Some(next), baseline_next);

        let baseline_prev = processor.cycle_focus(true);
        let prev_actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyTabPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    mods: MOD_SHIFT,
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("dispatch registry should resolve tab reverse rule");
        let prev = first_focus_change_action(processor.dispatch_registry(), prev_actions)
            .expect("focus action");

        assert_eq!(baseline_prev, Some(id1.clone()));
        assert_eq!(Some(prev), baseline_prev);
    }

    #[test]
    fn test_dispatch_registry_enter_press_resolves_for_focused_pressable() {
        let focused_id = ElementId::from_term_bytes(vec![40]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(40, 0.0, 0.0, 80.0, 30.0)]);
        processor.focused_id = Some(focused_id.clone());

        let enter = InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };
        let predicted = processor
            .preview_outcome_for_test(&enter, processor.focused_id.as_ref())
            .expect("preview should produce enter press outcome");
        let predicted_press_target = predicted.element_events.iter().find_map(|event| {
            if event.kind == dispatch_outcome::ElementEventKind::Press {
                Some(event.target.clone())
            } else {
                None
            }
        });

        let focused_idx = processor
            .dispatch_registry()
            .node_idx(&focused_id)
            .expect("focused node should exist in dispatch registry");
        let actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyEnterPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("dispatch registry should resolve enter press rule");

        let emitted_id = actions.iter().find_map(|action| match action {
            DispatchRuleAction::EmitElementEvent { element } => {
                processor.dispatch_registry().node_id(*element).cloned()
            }
            _ => None,
        });

        assert_eq!(
            predicted_press_target,
            Some(dispatch_outcome::node_key(&focused_id.clone()))
        );
        assert_eq!(emitted_id, Some(focused_id));

        let ctrl_enter = InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        };
        let predicted_blocked = processor
            .preview_outcome_for_test(&ctrl_enter, processor.focused_id.as_ref())
            .map(|outcome| {
                outcome
                    .element_events
                    .into_iter()
                    .any(|event| event.kind == dispatch_outcome::ElementEventKind::Press)
            })
            .unwrap_or(false);
        let blocked =
            processor
                .dispatch_registry()
                .resolve_winner_for_job(&DispatchJob::Targeted {
                    trigger: TriggerId::KeyEnterPress,
                    target: focused_idx,
                    ctx: DispatchCtx {
                        mods: MOD_CTRL,
                        focused: Some(focused_idx),
                        ..DispatchCtx::default()
                    },
                });

        assert!(!predicted_blocked);
        assert_eq!(blocked, None);
    }

    #[test]
    fn test_dispatch_registry_keyboard_triggers_resolve_behavioral_actions() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            make_pressable_node(50, 0.0, 0.0, 80.0, 30.0),
            make_scroll_target_node(51, true, EVENT_SCROLL_Y_POS),
        ]);

        let focused_id = ElementId::from_term_bytes(vec![50]);
        let focused_idx = processor
            .dispatch_registry()
            .node_idx(&focused_id)
            .expect("focused pressable should exist in dispatch registry");

        let enter_actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyEnterPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("enter trigger should resolve actions for focused pressable");
        assert!(
            enter_actions
                .iter()
                .any(|action| matches!(action, DispatchRuleAction::EmitElementEvent { .. }))
        );

        let down_actions = processor
            .dispatch_registry()
            .resolve_actions_for_job(&DispatchJob::Untargeted {
                trigger: TriggerId::KeyDownPress,
                ctx: DispatchCtx::default(),
            })
            .expect("down trigger should resolve scroll actions");
        assert!(
            down_actions
                .iter()
                .any(|action| matches!(action, DispatchRuleAction::ScrollRequest { .. }))
        );
    }

    fn trigger_for_job(job: &DispatchJob) -> TriggerId {
        match job {
            DispatchJob::Targeted { trigger, .. }
            | DispatchJob::Pointed { trigger, .. }
            | DispatchJob::Untargeted { trigger, .. } => *trigger,
        }
    }

    fn compiled_job_triggers(compiled: &CompiledDispatchJobs) -> Vec<TriggerId> {
        compiled.jobs.iter().map(trigger_for_job).collect()
    }

    #[test]
    fn test_compile_dispatch_jobs_cursor_pos_pass_order_is_stable() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pointer_events_node(97, 0.0, 0.0, 100.0, 100.0)]);

        let event = InputEvent::CursorPos { x: 10.0, y: 10.0 };
        let compiled = processor.compile_dispatch_jobs_for_event(&event, None);

        assert_eq!(compiled.primary_trigger, None);
        assert_eq!(compiled.trigger_for_stats, Some(TriggerId::CursorEnter));
        assert_eq!(
            compiled_job_triggers(&compiled),
            vec![
                TriggerId::CursorEnter,
                TriggerId::CursorMove,
                TriggerId::CursorDragScroll,
                TriggerId::ScrollbarThumbDispatch,
                TriggerId::ScrollbarHoverDispatch,
                TriggerId::TextCursorDispatch,
            ]
        );
    }

    #[test]
    fn test_compile_dispatch_jobs_left_press_pass_order_is_stable() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pointer_events_node(98, 0.0, 0.0, 100.0, 100.0)]);

        let event = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };
        let compiled = processor.compile_dispatch_jobs_for_event(&event, None);

        assert_eq!(
            compiled.primary_trigger,
            Some(TriggerId::CursorButtonLeftPress)
        );
        assert_eq!(
            compiled.trigger_for_stats,
            Some(TriggerId::CursorButtonLeftPress)
        );
        assert_eq!(
            compiled_job_triggers(&compiled),
            vec![
                TriggerId::CursorButtonLeftPress,
                TriggerId::CursorEnter,
                TriggerId::ScrollbarThumbDispatch,
                TriggerId::ScrollbarHoverDispatch,
                TriggerId::TextCursorDispatch,
                TriggerId::CursorFocusDispatch,
            ]
        );
    }

    #[test]
    fn test_compile_dispatch_jobs_trigger_for_stats_uses_first_job_when_primary_missing() {
        let focused_id = ElementId::from_term_bytes(vec![99]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(99, 0.0, 0.0, 120.0, 30.0)]);

        let event = InputEvent::CursorButton {
            button: "middle".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let compiled = processor.compile_dispatch_jobs_for_event(&event, Some(&focused_id));

        assert_eq!(compiled.primary_trigger, None);
        assert_eq!(
            compiled.trigger_for_stats,
            Some(TriggerId::TextCursorDispatch)
        );
        assert_eq!(
            compiled_job_triggers(&compiled),
            vec![
                TriggerId::TextCursorDispatch,
                TriggerId::CursorFocusDispatch,
                TriggerId::TextCommandDispatch,
            ]
        );
    }

    #[test]
    fn test_compile_dispatch_jobs_for_key_event_targets_focused_node() {
        let focused_id = ElementId::from_term_bytes(vec![60]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(60, 0.0, 0.0, 160.0, 30.0)]);

        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let compiled = processor.compile_dispatch_jobs_for_event(&event, Some(&focused_id));

        assert_eq!(compiled.primary_trigger, Some(TriggerId::KeyBackspacePress));
        assert!(!compiled.jobs.is_empty());

        let expected_target = processor
            .dispatch_registry()
            .node_idx(&focused_id)
            .expect("focused node should exist in dispatch registry");

        let key_job = compiled.jobs.iter().find_map(|job| match job {
            DispatchJob::Targeted {
                trigger,
                target,
                ctx,
            } if *trigger == TriggerId::KeyBackspacePress => Some((*target, *ctx)),
            _ => None,
        });

        let Some((target, ctx)) = key_job else {
            panic!("expected targeted key job in compiled jobs");
        };

        assert_eq!(target, expected_target);
        assert_eq!(ctx.focused, Some(expected_target));
        assert_eq!(ctx.mods, 0);
    }

    #[test]
    fn test_compile_dispatch_jobs_for_left_button_creates_pointed_job() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pointer_events_node(79, 0.0, 0.0, 100.0, 100.0)]);

        let event = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 15.0,
        };

        let compiled = processor.compile_dispatch_jobs_for_event(&event, None);
        assert_eq!(
            compiled.primary_trigger,
            Some(TriggerId::CursorButtonLeftPress)
        );
        assert!(compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Pointed {
                    trigger: TriggerId::CursorButtonLeftPress,
                    x,
                    y,
                    ..
                } if *x == 10.0 && *y == 15.0
            )
        }));
    }

    #[test]
    fn test_compile_dispatch_jobs_for_middle_button_uses_supplemental_dispatches() {
        let focused_id = ElementId::from_term_bytes(vec![82]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(82, 0.0, 0.0, 120.0, 30.0)]);

        let event = InputEvent::CursorButton {
            button: "middle".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let compiled = processor.compile_dispatch_jobs_for_event(&event, Some(&focused_id));
        assert_eq!(compiled.primary_trigger, None);

        assert!(
            compiled
                .jobs
                .iter()
                .all(|job| !matches!(job, DispatchJob::Pointed { .. }))
        );
        assert!(compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Untargeted {
                    trigger: TriggerId::TextCursorDispatch,
                    ..
                }
            )
        }));
        assert!(compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Untargeted {
                    trigger: TriggerId::CursorFocusDispatch,
                    ..
                }
            )
        }));
        assert!(compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Untargeted {
                    trigger: TriggerId::TextCommandDispatch,
                    ..
                }
            )
        }));
    }

    #[test]
    fn test_compile_dispatch_jobs_for_text_commit_uses_text_edit_dispatch() {
        let focused_id = ElementId::from_term_bytes(vec![83]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(83, 0.0, 0.0, 120.0, 30.0)]);

        let event = InputEvent::TextCommit {
            text: "a".to_string(),
            mods: 0,
        };

        let compiled = processor.compile_dispatch_jobs_for_event(&event, Some(&focused_id));
        assert_eq!(compiled.primary_trigger, None);
        assert!(compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Untargeted {
                    trigger: TriggerId::TextEditDispatch,
                    ..
                }
            )
        }));
    }

    #[test]
    fn test_compile_dispatch_jobs_for_preedit_events_use_text_preedit_dispatch() {
        let focused_id = ElementId::from_term_bytes(vec![84]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(84, 0.0, 0.0, 120.0, 30.0)]);

        let preedit = InputEvent::TextPreedit {
            text: "a".to_string(),
            cursor: Some((0, 1)),
        };
        let clear = InputEvent::TextPreeditClear;

        let preedit_compiled =
            processor.compile_dispatch_jobs_for_event(&preedit, Some(&focused_id));
        assert_eq!(preedit_compiled.primary_trigger, None);
        assert!(preedit_compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Untargeted {
                    trigger: TriggerId::TextPreeditDispatch,
                    ..
                }
            )
        }));

        let clear_compiled = processor.compile_dispatch_jobs_for_event(&clear, Some(&focused_id));
        assert_eq!(clear_compiled.primary_trigger, None);
        assert!(clear_compiled.jobs.iter().any(|job| {
            matches!(
                job,
                DispatchJob::Untargeted {
                    trigger: TriggerId::TextPreeditDispatch,
                    ..
                }
            )
        }));
    }

    #[test]
    fn test_pointed_dispatch_skips_non_matching_top_hit_and_emits_mouse_down() {
        let bottom_id = ElementId::from_term_bytes(vec![80]);

        let mut bottom = make_pointer_events_node(80, 0.0, 0.0, 100.0, 100.0);
        bottom.flags = EVENT_MOUSE_DOWN;

        let mut top = make_pointer_events_node(81, 0.0, 0.0, 100.0, 100.0);
        top.flags = EVENT_CLICK;

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![bottom, top]);

        let event = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 20.0,
            y: 20.0,
        };

        let compiled = processor.compile_dispatch_jobs_for_event(&event, None);
        let mut out = dispatch_outcome::DispatchOutcome::default();
        let applied = processor.apply_compiled_dispatch_jobs(&event, &compiled.jobs, &mut out);

        assert!(applied.mouse_button_event_emitted);
        let mouse_down_events: Vec<&dispatch_outcome::ElementEventOut> = out
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::MouseDown)
            .collect();
        assert_eq!(mouse_down_events.len(), 1);
        assert_eq!(
            mouse_down_events[0].target,
            dispatch_outcome::node_key(&bottom_id)
        );
    }

    #[test]
    fn test_trigger_mapping_includes_window_dispatch_triggers() {
        let processor = EventProcessor::new();

        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 0.0,
                y: 0.0,
            }),
            Some(TriggerId::CursorButtonLeftPress)
        );
        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::CursorButton {
                button: "left".to_string(),
                action: ACTION_RELEASE,
                mods: 0,
                x: 0.0,
                y: 0.0,
            }),
            Some(TriggerId::CursorButtonLeftRelease)
        );
        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::CursorButton {
                button: "middle".to_string(),
                action: ACTION_PRESS,
                mods: 0,
                x: 0.0,
                y: 0.0,
            }),
            None
        );

        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::TextCommit {
                text: "x".to_string(),
                mods: 0,
            }),
            None
        );
        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::TextPreedit {
                text: "x".to_string(),
                cursor: None,
            }),
            None
        );
        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::TextPreeditClear),
            None
        );

        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::Focused { focused: false }),
            Some(TriggerId::WindowFocusLost)
        );
        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::Resized {
                width: 800,
                height: 600,
                scale_factor: 2.0,
            }),
            Some(TriggerId::WindowResizeDispatch)
        );
        assert_eq!(
            processor.trigger_for_input_event(&InputEvent::Focused { focused: true }),
            None
        );
    }

    #[test]
    fn test_apply_dispatch_actions_emits_text_edit_output() {
        let text_input_id = ElementId::from_term_bytes(vec![78]);
        let mut node = make_text_input_node(78, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 3;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);

        let idx = processor
            .dispatch_registry()
            .node_idx(&text_input_id)
            .expect("text input should exist in dispatch registry");
        let actions = vec![DispatchRuleAction::TextEdit {
            element: idx,
            request: TextInputEditRequest::Backspace,
        }];

        let mut out = dispatch_outcome::DispatchOutcome::default();
        let _applied = processor.apply_dispatch_actions(
            &InputEvent::Key {
                key: "backspace".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            },
            TriggerId::KeyBackspacePress,
            &actions,
            &mut out,
        );

        assert_eq!(
            out.text_edit_requests,
            vec![dispatch_outcome::TextEditReqOut {
                target: dispatch_outcome::node_key(&text_input_id),
                request: TextInputEditRequest::Backspace,
            }]
        );
        assert!(out.text_command_requests.is_empty());
        assert!(out.text_preedit_requests.is_empty());

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = out
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();
        assert_eq!(change_events.len(), 1);
        assert_eq!(
            change_events[0].target,
            dispatch_outcome::node_key(&text_input_id)
        );
        assert_eq!(change_events[0].payload.as_deref(), Some("ab"));
    }

    #[test]
    fn test_apply_dispatch_actions_emits_window_resize_output() {
        let mut processor = EventProcessor::new();
        let mut out = dispatch_outcome::DispatchOutcome::default();

        let _applied = processor.apply_dispatch_actions(
            &InputEvent::Resized {
                width: 800,
                height: 600,
                scale_factor: 1.5,
            },
            TriggerId::WindowResizeDispatch,
            &[DispatchRuleAction::EmitWindowResizeFromEvent],
            &mut out,
        );

        assert_eq!(
            out.window_resize_requests,
            vec![dispatch_outcome::WindowResizeReqOut {
                width: 800,
                height: 600,
                scale: dispatch_outcome::milli(1.5),
            }]
        );
    }

    #[test]
    fn test_preview_resize_outcome_available_before_registry_update() {
        let processor = EventProcessor::new();

        let predicted = processor
            .preview_outcome_for_test(
                &InputEvent::Resized {
                    width: 1024,
                    height: 768,
                    scale_factor: 2.0,
                },
                None,
            )
            .expect("preview should emit resize dispatch outcome");

        assert_eq!(
            predicted.window_resize_requests,
            vec![dispatch_outcome::WindowResizeReqOut {
                width: 1024,
                height: 768,
                scale: dispatch_outcome::milli(2.0),
            }]
        );
    }

    #[test]
    fn test_apply_dispatch_actions_text_edit_static_dynamic_match() {
        let text_input_id = ElementId::from_term_bytes(vec![91]);
        let mut node = make_text_input_node(91, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 3;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(text_input_id.clone());

        let idx = processor
            .dispatch_registry()
            .node_idx(&text_input_id)
            .expect("text input should exist in dispatch registry");
        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let mut static_out = dispatch_outcome::DispatchOutcome::default();
        let _ = processor.apply_dispatch_actions(
            &event,
            TriggerId::KeyBackspacePress,
            &[DispatchRuleAction::TextEdit {
                element: idx,
                request: TextInputEditRequest::Backspace,
            }],
            &mut static_out,
        );

        let mut dynamic_out = dispatch_outcome::DispatchOutcome::default();
        let _ = processor.apply_dispatch_actions(
            &event,
            TriggerId::TextEditDispatch,
            &[DispatchRuleAction::EmitTextEditRequestsFromEvent],
            &mut dynamic_out,
        );

        assert_eq!(dynamic_out, static_out);
    }

    #[test]
    fn test_preview_text_command_request_emits_select_all() {
        let focused_id = ElementId::from_term_bytes(vec![61]);
        let mut node = make_text_input_node(61, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 3;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Key {
            key: "a".to_string(),
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce command request outcome");

        assert_eq!(
            predicted.text_command_requests,
            vec![dispatch_outcome::TextCommandReqOut {
                target: dispatch_outcome::node_key(&focused_id),
                request: TextInputCommandRequest::SelectAll,
            }]
        );
    }

    #[test]
    fn test_preview_text_cursor_request_emits_expected_target_and_position() {
        let text_input_id = ElementId::from_term_bytes(vec![90]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(90, 0.0, 0.0, 160.0, 30.0)]);

        let event = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 42.0,
            y: 10.0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce text cursor request outcome");

        assert_eq!(
            predicted.text_cursor_requests,
            vec![dispatch_outcome::TextCursorReqOut {
                target: dispatch_outcome::node_key(&text_input_id),
                x: dispatch_outcome::milli(42.0),
                extend_selection: false,
            }]
        );
    }

    #[test]
    fn test_preview_text_edit_request_emits_backspace_for_focused_input() {
        let focused_id = ElementId::from_term_bytes(vec![62]);
        let mut node = make_text_input_node(62, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 3;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id);

        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce edit request outcome");

        assert_eq!(
            predicted.text_edit_requests,
            vec![dispatch_outcome::TextEditReqOut {
                target: dispatch_outcome::node_key(&ElementId::from_term_bytes(vec![62])),
                request: TextInputEditRequest::Backspace,
            }]
        );
    }

    #[test]
    fn test_preview_text_commit_emits_change_event_payload() {
        let focused_id = ElementId::from_term_bytes(vec![68]);
        let mut node = make_text_input_node(68, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "ab".to_string();
            descriptor.content_len = 2;
            descriptor.cursor = 2;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::TextCommit {
            text: "x".to_string(),
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce text commit change event");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert_eq!(change_events.len(), 1);
        assert_eq!(
            change_events[0].target,
            dispatch_outcome::node_key(&focused_id)
        );
        assert_eq!(change_events[0].payload.as_deref(), Some("abx"));
    }

    #[test]
    fn test_preview_text_commit_without_on_change_emits_edit_request_but_no_change_event() {
        let focused_id = ElementId::from_term_bytes(vec![72]);
        let mut node = make_text_input_node(72, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "ab".to_string();
            descriptor.content_len = 2;
            descriptor.cursor = 2;
            descriptor.selection_anchor = None;
            descriptor.emit_change = false;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::TextCommit {
            text: "x".to_string(),
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce text commit edit request");

        assert_eq!(
            predicted.text_edit_requests,
            vec![dispatch_outcome::TextEditReqOut {
                target: dispatch_outcome::node_key(&focused_id),
                request: TextInputEditRequest::Insert("x".to_string()),
            }]
        );
        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();
        assert!(change_events.is_empty());
    }

    #[test]
    fn test_preview_backspace_emits_change_event_payload() {
        let focused_id = ElementId::from_term_bytes(vec![69]);
        let mut node = make_text_input_node(69, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abcd".to_string();
            descriptor.content_len = 4;
            descriptor.cursor = 4;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce backspace change event");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert_eq!(change_events.len(), 1);
        assert_eq!(
            change_events[0].target,
            dispatch_outcome::node_key(&focused_id)
        );
        assert_eq!(change_events[0].payload.as_deref(), Some("abc"));
    }

    #[test]
    fn test_preview_backspace_at_start_emits_no_change_event() {
        let mut node = make_text_input_node(70, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abcd".to_string();
            descriptor.content_len = 4;
            descriptor.cursor = 0;
            descriptor.selection_anchor = None;
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(ElementId::from_term_bytes(vec![70]));

        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce backspace outcome");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert!(change_events.is_empty());
    }

    #[test]
    fn test_preview_backspace_with_selection_emits_change_event_payload() {
        let focused_id = ElementId::from_term_bytes(vec![71]);
        let mut node = make_text_input_node(71, 0.0, 0.0, 160.0, 30.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abcd".to_string();
            descriptor.content_len = 4;
            descriptor.cursor = 3;
            descriptor.selection_anchor = Some(1);
        }

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce selection delete change event");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert_eq!(change_events.len(), 1);
        assert_eq!(
            change_events[0].target,
            dispatch_outcome::node_key(&focused_id)
        );
        assert_eq!(change_events[0].payload.as_deref(), Some("ad"));
    }

    #[test]
    fn test_preview_text_preedit_request_emits_set_for_focused_input() {
        let focused_id = ElementId::from_term_bytes(vec![63]);
        let node = make_text_input_node(63, 0.0, 0.0, 160.0, 30.0);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id);

        let event = InputEvent::TextPreedit {
            text: "kana".to_string(),
            cursor: Some((1, 3)),
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce preedit request outcome");

        assert_eq!(
            predicted.text_preedit_requests,
            vec![dispatch_outcome::TextPreeditReqOut {
                target: dispatch_outcome::node_key(&ElementId::from_term_bytes(vec![63])),
                request: TextInputPreeditRequest::Set {
                    text: "kana".to_string(),
                    cursor: Some((1, 3)),
                },
            }]
        );
    }

    #[test]
    fn test_preview_enter_press_does_not_duplicate_press_event() {
        let focused_id = ElementId::from_term_bytes(vec![64]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(64, 0.0, 0.0, 100.0, 30.0)]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce enter press outcome");

        let press_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Press)
            .collect();

        assert_eq!(press_events.len(), 1);
        assert_eq!(
            press_events[0].target,
            dispatch_outcome::node_key(&focused_id)
        );
    }

    #[test]
    fn test_preview_tab_emits_focus_transition_events() {
        let first = ElementId::from_term_bytes(vec![65]);
        let second = ElementId::from_term_bytes(vec![66]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            make_pressable_node(65, 0.0, 0.0, 100.0, 30.0),
            make_pressable_node(66, 0.0, 40.0, 100.0, 30.0),
        ]);
        processor.focused_id = Some(first.clone());

        let event = InputEvent::Key {
            key: "tab".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce tab focus transition outcome");

        assert_eq!(
            predicted.focus_change,
            Some(Some(dispatch_outcome::node_key(&second)))
        );
        assert_eq!(predicted.element_events.len(), 2);
        assert_eq!(
            predicted.element_events[0],
            dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&first),
                kind: dispatch_outcome::ElementEventKind::Blur,
                payload: None,
            }
        );
        assert_eq!(
            predicted.element_events[1],
            dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&second),
                kind: dispatch_outcome::ElementEventKind::Focus,
                payload: None,
            }
        );
    }

    #[test]
    fn test_preview_window_focus_lost_emits_blur_event() {
        let focused_id = ElementId::from_term_bytes(vec![67]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(67, 0.0, 0.0, 100.0, 30.0)]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Focused { focused: false };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce blur outcome on window focus loss");

        assert_eq!(predicted.focus_change, Some(None));
        assert_eq!(predicted.element_events.len(), 1);
        assert_eq!(
            predicted.element_events[0],
            dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&focused_id),
                kind: dispatch_outcome::ElementEventKind::Blur,
                payload: None,
            }
        );
    }

    fn sequenced_outcome_for_event(
        processor: &mut EventProcessor,
        event: &InputEvent,
    ) -> dispatch_outcome::DispatchOutcome {
        let out = processor
            .preview_outcome_for_test(event, processor.focused_id.as_ref())
            .unwrap_or_default();
        processor.advance_runtime_state_after_event(event);
        out
    }

    #[test]
    fn test_preview_scrollbar_thumb_drag_sequence_regression() {
        let mut baseline_processor = EventProcessor::new();
        baseline_processor.rebuild_registry(vec![make_scrollbar_test_node(66)]);

        let mut preview_processor = baseline_processor.clone();

        let events = [
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 95.0,
                y: 80.0,
            },
            InputEvent::CursorPos { x: 95.0, y: 70.0 },
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_RELEASE,
                mods: 0,
                x: 95.0,
                y: 70.0,
            },
        ];

        for event in events {
            let expected = sequenced_outcome_for_event(&mut baseline_processor, &event);
            let predicted = preview_processor
                .preview_outcome_for_test(&event, None)
                .unwrap_or_default();

            assert_eq!(
                predicted.scrollbar_thumb_drag_requests,
                expected.scrollbar_thumb_drag_requests
            );

            let _ = sequenced_outcome_for_event(&mut preview_processor, &event);
        }
    }

    #[test]
    fn test_preview_scrollbar_hover_request_emits_expected_axis_and_target() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_scrollbar_test_node(64)]);

        let event = InputEvent::CursorPos { x: 97.0, y: 25.0 };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce scrollbar hover request outcome");

        assert_eq!(
            predicted.scrollbar_hover_requests,
            vec![dispatch_outcome::ScrollbarHoverReqOut {
                target: dispatch_outcome::node_key(&ElementId::from_term_bytes(vec![64])),
                axis: dispatch_outcome::ScrollbarAxisOut::Y,
                hovered: true,
            }]
        );
    }

    #[test]
    fn test_preview_style_runtime_mouse_over_request_emits_activate() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_mouse_over_node(65, 0.0, 0.0, 100.0, 100.0)]);

        let event = InputEvent::CursorPos { x: 10.0, y: 10.0 };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce style runtime request outcome");

        assert_eq!(
            predicted.style_runtime_requests,
            vec![dispatch_outcome::StyleRuntimeReqOut {
                target: dispatch_outcome::node_key(&ElementId::from_term_bytes(vec![65])),
                kind: dispatch_outcome::StyleRuntimeKind::MouseOver,
                active: true,
            }]
        );
    }

    #[test]
    fn test_preview_style_runtime_mouse_down_sequence_regression() {
        let mut baseline_processor = EventProcessor::new();
        baseline_processor
            .rebuild_registry(vec![make_mouse_down_style_node(67, 0.0, 0.0, 100.0, 100.0)]);

        let mut preview_processor = baseline_processor.clone();

        let events = [
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
        ];

        for event in events {
            let expected = sequenced_outcome_for_event(&mut baseline_processor, &event);
            let predicted = preview_processor
                .preview_outcome_for_test(&event, None)
                .unwrap_or_default();

            assert_eq!(
                predicted.style_runtime_requests,
                expected.style_runtime_requests
            );

            let _ = sequenced_outcome_for_event(&mut preview_processor, &event);
        }
    }

    #[test]
    fn test_preview_pointer_element_events_sequence_regression() {
        let mut baseline_processor = EventProcessor::new();
        baseline_processor
            .rebuild_registry(vec![make_pointer_events_node(68, 0.0, 0.0, 100.0, 100.0)]);

        let mut preview_processor = baseline_processor.clone();

        let events = [
            InputEvent::CursorPos { x: 10.0, y: 10.0 },
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_PRESS,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            InputEvent::CursorButton {
                button: "left".to_string(),
                action: crate::input::ACTION_RELEASE,
                mods: 0,
                x: 10.0,
                y: 10.0,
            },
            InputEvent::CursorPos { x: 140.0, y: 140.0 },
        ];

        for event in events {
            let expected = sequenced_outcome_for_event(&mut baseline_processor, &event);
            let predicted = preview_processor
                .preview_outcome_for_test(&event, None)
                .unwrap_or_default();

            assert_eq!(predicted.element_events, expected.element_events);

            let _ = sequenced_outcome_for_event(&mut preview_processor, &event);
        }
    }

    #[test]
    fn test_preview_mouse_button_payload_is_none() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pointer_events_node(72, 0.0, 0.0, 100.0, 100.0)]);

        let event = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, None)
            .expect("preview should produce mouse down outcome");

        let mouse_down_event = predicted
            .element_events
            .iter()
            .find(|event| event.kind == dispatch_outcome::ElementEventKind::MouseDown)
            .expect("mouse down event should be predicted");

        assert_eq!(mouse_down_event.payload, None);
    }

    #[test]
    fn test_preview_key_scroll_requests_are_deduped() {
        let focused_id = ElementId::from_term_bytes(vec![73]);
        let scroll_id = ElementId::from_term_bytes(vec![74]);

        let mut focused = make_pressable_node(73, 0.0, 0.0, 100.0, 30.0);
        focused.key_scroll_targets.down = Some(scroll_id.clone());

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            focused,
            make_scroll_target_node(74, true, EVENT_SCROLL_Y_POS),
        ]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Key {
            key: "down".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&event, processor.focused_id.as_ref())
            .expect("preview should produce key scroll outcome");

        assert_eq!(predicted.scroll_requests.len(), 1);
        assert_eq!(
            predicted.scroll_requests[0],
            dispatch_outcome::ScrollRequestOut {
                target: dispatch_outcome::node_key(&scroll_id),
                dx: dispatch_outcome::milli(0.0),
                dy: dispatch_outcome::milli(-SCROLL_LINE_PIXELS),
            }
        );
    }

    #[test]
    fn test_preview_tab_focus_reveal_scroll_is_deduped() {
        let first_id = ElementId::from_term_bytes(vec![75]);
        let second_id = ElementId::from_term_bytes(vec![76]);
        let scroll_id = ElementId::from_term_bytes(vec![77]);

        let first = make_pressable_node(75, 0.0, 0.0, 100.0, 30.0);
        let mut second = make_pressable_node(76, 0.0, 40.0, 100.0, 30.0);
        second.focus_reveal_scrolls.push(ScrollRequestMatcher {
            element_id: scroll_id.clone(),
            dx: 0.0,
            dy: -50.0,
        });

        let mut baseline_processor = EventProcessor::new();
        baseline_processor.rebuild_registry(vec![
            first,
            second,
            make_scroll_target_node(77, true, EVENT_SCROLL_Y_POS),
        ]);
        baseline_processor.focused_id = Some(first_id.clone());

        let preview_processor = baseline_processor.clone();

        let event = InputEvent::Key {
            key: "tab".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let predicted = preview_processor
            .preview_outcome_for_test(&event, preview_processor.focused_id.as_ref())
            .expect("preview should produce tab focus-reveal outcome");

        assert_eq!(
            predicted.focus_change,
            Some(Some(dispatch_outcome::node_key(&second_id)))
        );
        assert_eq!(predicted.scroll_requests.len(), 1);
        assert_eq!(
            predicted.scroll_requests[0],
            dispatch_outcome::ScrollRequestOut {
                target: dispatch_outcome::node_key(&scroll_id),
                dx: dispatch_outcome::milli(0.0),
                dy: dispatch_outcome::milli(-50.0),
            }
        );
    }

    #[test]
    fn test_build_event_registry_order() {
        let mut tree = ElementTree::new();

        let mut root_attrs = Attrs::default();
        root_attrs.on_click = Some(true);
        let root_id = ElementId::from_term_bytes(vec![1]);

        let mut child_attrs = Attrs::default();
        child_attrs.on_click = Some(true);
        let child_id = ElementId::from_term_bytes(vec![2]);

        let root = make_element(
            1,
            root_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 100.0,
            },
            vec![child_id.clone()],
        );

        let child = make_element(
            2,
            child_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
                content_width: 50.0,
                content_height: 50.0,
            },
            Vec::new(),
        );

        tree.root = Some(root_id);
        tree.insert(root);
        tree.insert(child);

        let registry = build_event_registry(&mut tree);
        assert_eq!(registry.len(), 2);
        assert_eq!(registry[0].id, ElementId::from_term_bytes(vec![1]));
        assert_eq!(registry[1].id, ElementId::from_term_bytes(vec![2]));

        let hit = hit_test_with_flag(&registry, 10.0, 10.0, EVENT_CLICK).unwrap();
        assert_eq!(hit, ElementId::from_term_bytes(vec![2]));
    }

    #[test]
    fn test_text_input_registry_keeps_click_and_mouse_flags() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        attrs.on_click = Some(true);
        attrs.on_mouse_enter = Some(true);
        attrs.on_mouse_leave = Some(true);
        attrs.on_mouse_move = Some(true);

        let id = ElementId::from_term_bytes(vec![9]);
        let mut input = Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        input.frame = Some(crate::tree::element::Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        tree.root = Some(id.clone());
        tree.insert(input);

        let registry = build_event_registry(&mut tree);
        assert_eq!(registry.len(), 1);

        let node = &registry[0];
        assert!(node.flags & EVENT_TEXT_INPUT != 0);
        assert!(node.flags & EVENT_CLICK != 0);
        assert!(node.flags & EVENT_MOUSE_ENTER != 0);
        assert!(node.flags & EVENT_MOUSE_LEAVE != 0);
        assert!(node.flags & EVENT_MOUSE_MOVE != 0);
        assert!(node.text_input.is_some());

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(registry);

        let middle_press = InputEvent::CursorButton {
            button: "middle".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 4.0,
            y: 4.0,
        };
        let middle_outcome = sequenced_outcome_for_event(&mut processor, &middle_press);
        assert!(
            middle_outcome
                .element_events
                .iter()
                .all(|event| event.kind != dispatch_outcome::ElementEventKind::Click)
        );

        let left_press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 4.0,
            y: 4.0,
        };
        let press_outcome = sequenced_outcome_for_event(&mut processor, &left_press);
        assert!(
            press_outcome
                .element_events
                .iter()
                .all(|event| event.kind != dispatch_outcome::ElementEventKind::Click)
        );

        let left_release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 4.0,
            y: 4.0,
        };
        let release_outcome = sequenced_outcome_for_event(&mut processor, &left_release);
        let click_targets: Vec<&dispatch_outcome::NodeKey> = release_outcome
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Click)
            .map(|event| &event.target)
            .collect();
        assert_eq!(click_targets, vec![&dispatch_outcome::node_key(&id)]);
    }

    #[test]
    fn test_hit_test_respects_clip_padding() {
        let mut tree = ElementTree::new();

        let mut root_attrs = Attrs::default();
        root_attrs.scrollbar_y = Some(true);
        root_attrs.padding = Some(crate::tree::attrs::Padding::Uniform(10.0));
        let root = make_element(
            1,
            root_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 100.0,
            },
            vec![ElementId::from_term_bytes(vec![2])],
        );

        let mut child_attrs = Attrs::default();
        child_attrs.on_click = Some(true);
        let child = make_element(
            2,
            child_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
                content_width: 50.0,
                content_height: 50.0,
            },
            Vec::new(),
        );

        tree.root = Some(ElementId::from_term_bytes(vec![1]));
        tree.insert(root);
        tree.insert(child);

        let registry = build_event_registry(&mut tree);
        assert!(hit_test_with_flag(&registry, 5.0, 5.0, EVENT_CLICK).is_none());
        assert!(hit_test_with_flag(&registry, 15.0, 15.0, EVENT_CLICK).is_some());
    }

    #[test]
    fn test_hit_test_respects_rounded_corners() {
        let mut tree = ElementTree::new();

        let mut root_attrs = Attrs::default();
        root_attrs.scrollbar_y = Some(true);
        root_attrs.border_radius = Some(crate::tree::attrs::BorderRadius::Uniform(10.0));
        let root = make_element(
            1,
            root_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 100.0,
            },
            vec![ElementId::from_term_bytes(vec![2])],
        );

        let mut child_attrs = Attrs::default();
        child_attrs.on_click = Some(true);
        let child = make_element(
            2,
            child_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
                content_width: 50.0,
                content_height: 50.0,
            },
            Vec::new(),
        );

        tree.root = Some(ElementId::from_term_bytes(vec![1]));
        tree.insert(root);
        tree.insert(child);

        let registry = build_event_registry(&mut tree);
        assert!(hit_test_with_flag(&registry, 2.0, 2.0, EVENT_CLICK).is_none());
        assert!(hit_test_with_flag(&registry, 10.0, 2.0, EVENT_CLICK).is_some());
    }

    #[test]
    fn test_scroll_flags_respect_bounds() {
        let mut tree = ElementTree::new();

        let mut root_attrs = Attrs::default();
        root_attrs.scrollbar_y = Some(true);
        root_attrs.scroll_y = Some(0.0);
        root_attrs.scroll_y_max = Some(20.0);
        let root = make_element(
            1,
            root_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 120.0,
            },
            Vec::new(),
        );

        tree.root = Some(ElementId::from_term_bytes(vec![1]));
        tree.insert(root);

        let registry = build_event_registry(&mut tree);
        assert_eq!(registry.len(), 1);
        assert!(registry[0].flags & EVENT_SCROLL_Y_NEG == 0);
        assert!(registry[0].flags & EVENT_SCROLL_Y_POS != 0);
        assert!(registry[0].scrollbar_y.is_some());
    }

    #[test]
    fn test_drag_deadzone_suppresses_click() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_scroll_target_node(1, true, EVENT_CLICK)]);

        let press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };
        let move_event = InputEvent::CursorPos { x: 25.0, y: 10.0 };
        let release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 25.0,
            y: 10.0,
        };

        let press_outcome = processor
            .preview_outcome_for_test(&press, None)
            .unwrap_or_default();
        assert!(
            press_outcome
                .element_events
                .iter()
                .all(|event| event.kind != dispatch_outcome::ElementEventKind::Click)
        );
        processor.advance_runtime_state_after_event(&press);
        assert!(processor.scroll_requests(&move_event).is_empty());
        let release_outcome = processor
            .preview_outcome_for_test(&release, None)
            .unwrap_or_default();
        assert!(
            release_outcome
                .element_events
                .iter()
                .all(|event| event.kind != dispatch_outcome::ElementEventKind::Click)
        );
        processor.advance_runtime_state_after_event(&release);
    }

    #[test]
    fn test_scrollbar_thumb_drag_emits_requests_and_suppresses_click() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_scrollbar_test_node(1)]);

        let press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 95.0,
            y: 25.0,
        };
        let drag = InputEvent::CursorPos { x: 95.0, y: 55.0 };
        let release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 95.0,
            y: 55.0,
        };

        let press_outcome = processor
            .preview_outcome_for_test(&press, None)
            .unwrap_or_default();
        assert!(press_outcome.element_events.is_empty());
        processor.advance_runtime_state_after_event(&press);

        let requests = processor.scrollbar_thumb_drag_requests(&drag);
        assert_eq!(requests.len(), 1);
        match &requests[0] {
            ScrollbarThumbDragRequest::Y { element_id, dy } => {
                assert_eq!(*element_id, ElementId::from_term_bytes(vec![1]));
                assert!((*dy + 60.0).abs() < 0.01);
            }
            _ => panic!("expected vertical thumb drag request"),
        }

        let release_outcome = processor
            .preview_outcome_for_test(&release, None)
            .unwrap_or_default();
        assert!(release_outcome.element_events.is_empty());
        processor.advance_runtime_state_after_event(&release);
        assert!(processor.scrollbar_thumb_drag_requests(&release).is_empty());
    }

    #[test]
    fn test_scrollbar_track_press_snaps_to_cursor_then_drags() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_scrollbar_test_node(1)]);

        let press_track = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 95.0,
            y: 80.0,
        };
        let drag_after_press = InputEvent::CursorPos { x: 95.0, y: 70.0 };

        let press_outcome = processor
            .preview_outcome_for_test(&press_track, None)
            .unwrap_or_default();
        assert!(press_outcome.element_events.is_empty());
        let first = processor.scrollbar_thumb_drag_requests(&press_track);
        assert_eq!(first.len(), 1);
        match &first[0] {
            ScrollbarThumbDragRequest::Y { dy, .. } => {
                assert!((*dy + 90.0).abs() < 0.01);
            }
            _ => panic!("expected vertical track snap request"),
        }

        let second = processor.scrollbar_thumb_drag_requests(&drag_after_press);
        assert_eq!(second.len(), 1);
        match &second[0] {
            ScrollbarThumbDragRequest::Y { dy, .. } => {
                assert!((*dy - 20.0).abs() < 0.01);
            }
            _ => panic!("expected vertical drag request"),
        }
    }

    #[test]
    fn test_scrollbar_hover_requests_are_axis_specific() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_scrollbar_test_node(1)];

        let hover_thumb = InputEvent::CursorPos { x: 96.0, y: 25.0 };
        let leave_window = InputEvent::CursorEntered { entered: false };

        let first = processor.scrollbar_hover_requests(&hover_thumb);
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0],
            ScrollbarHoverRequest::Y {
                element_id: ElementId::from_term_bytes(vec![1]),
                hovered: true,
            }
        );

        assert!(processor.scrollbar_hover_requests(&hover_thumb).is_empty());

        let leave = processor.scrollbar_hover_requests(&leave_window);
        assert_eq!(leave.len(), 1);
        assert_eq!(
            leave[0],
            ScrollbarHoverRequest::Y {
                element_id: ElementId::from_term_bytes(vec![1]),
                hovered: false,
            }
        );
    }

    #[test]
    fn test_scrollbar_hover_switches_between_axes_without_dual_state() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_dual_scrollbar_test_node(1)];

        let hover_y = InputEvent::CursorPos { x: 96.0, y: 25.0 };
        let hover_x = InputEvent::CursorPos { x: 25.0, y: 96.0 };

        let first = processor.scrollbar_hover_requests(&hover_y);
        assert_eq!(
            first,
            vec![ScrollbarHoverRequest::Y {
                element_id: ElementId::from_term_bytes(vec![1]),
                hovered: true,
            }]
        );

        let second = processor.scrollbar_hover_requests(&hover_x);
        assert_eq!(second.len(), 2);
        assert_eq!(
            second[0],
            ScrollbarHoverRequest::Y {
                element_id: ElementId::from_term_bytes(vec![1]),
                hovered: false,
            }
        );
        assert_eq!(
            second[1],
            ScrollbarHoverRequest::X {
                element_id: ElementId::from_term_bytes(vec![1]),
                hovered: true,
            }
        );
    }

    #[test]
    fn test_mouse_over_requests_activate_and_deactivate() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_mouse_over_node(1, 0.0, 0.0, 100.0, 100.0)];

        let enter = InputEvent::CursorPos { x: 10.0, y: 10.0 };
        let leave = InputEvent::CursorEntered { entered: false };

        let first = processor.mouse_over_requests(&enter);
        assert_eq!(
            first,
            vec![MouseOverRequest::SetMouseOverActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: true,
            }]
        );

        assert!(processor.mouse_over_requests(&enter).is_empty());

        let second = processor.mouse_over_requests(&leave);
        assert_eq!(
            second,
            vec![MouseOverRequest::SetMouseOverActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: false,
            }]
        );
    }

    #[test]
    fn test_mouse_over_requests_switch_target() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_mouse_over_node(1, 0.0, 0.0, 40.0, 40.0),
            make_mouse_over_node(2, 50.0, 0.0, 40.0, 40.0),
        ];

        let first = processor.mouse_over_requests(&InputEvent::CursorPos { x: 10.0, y: 10.0 });
        assert_eq!(
            first,
            vec![MouseOverRequest::SetMouseOverActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: true,
            }]
        );

        let second = processor.mouse_over_requests(&InputEvent::CursorPos { x: 60.0, y: 10.0 });
        assert_eq!(second.len(), 2);
        assert_eq!(
            second[0],
            MouseOverRequest::SetMouseOverActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: false,
            }
        );
        assert_eq!(
            second[1],
            MouseOverRequest::SetMouseOverActive {
                element_id: ElementId::from_term_bytes(vec![2]),
                active: true,
            }
        );
    }

    #[test]
    fn test_build_event_registry_includes_mouse_down_style_flag() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(crate::tree::attrs::MouseOverAttrs {
            alpha: Some(0.6),
            ..Default::default()
        });

        let id = ElementId::from_term_bytes(vec![13]);
        let mut el = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        el.frame = Some(crate::tree::element::Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        });

        tree.root = Some(id.clone());
        tree.insert(el);

        let registry = build_event_registry(&mut tree);
        assert_eq!(registry.len(), 1);
        assert!(registry[0].flags & EVENT_MOUSE_DOWN_STYLE != 0);
    }

    #[test]
    fn test_build_event_registry_includes_press_and_focusable_flags() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.on_press = Some(true);

        let id = ElementId::from_term_bytes(vec![21]);
        let mut el = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        el.frame = Some(crate::tree::element::Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        });

        tree.root = Some(id.clone());
        tree.insert(el);

        let registry = build_event_registry(&mut tree);
        assert_eq!(registry.len(), 1);
        assert!(registry[0].flags & EVENT_PRESS != 0);
        assert!(registry[0].flags & EVENT_FOCUSABLE != 0);
    }

    #[test]
    fn test_mouse_down_requests_activate_and_deactivate() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_mouse_down_style_node(1, 0.0, 0.0, 100.0, 100.0)];

        let press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let first = processor.mouse_down_requests(&press);
        assert_eq!(
            first,
            vec![MouseDownRequest::SetMouseDownActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: true,
            }]
        );

        assert!(processor.mouse_down_requests(&press).is_empty());

        let second = processor.mouse_down_requests(&release);
        assert_eq!(
            second,
            vec![MouseDownRequest::SetMouseDownActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: false,
            }]
        );
    }

    #[test]
    fn test_mouse_down_requests_switch_target_on_new_press() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_mouse_down_style_node(1, 0.0, 0.0, 40.0, 40.0),
            make_mouse_down_style_node(2, 50.0, 0.0, 40.0, 40.0),
        ];

        let first_press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let second_press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 60.0,
            y: 10.0,
        };

        let first = processor.mouse_down_requests(&first_press);
        assert_eq!(
            first,
            vec![MouseDownRequest::SetMouseDownActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: true,
            }]
        );

        let second = processor.mouse_down_requests(&second_press);
        assert_eq!(second.len(), 2);
        assert_eq!(
            second[0],
            MouseDownRequest::SetMouseDownActive {
                element_id: ElementId::from_term_bytes(vec![1]),
                active: false,
            }
        );
        assert_eq!(
            second[1],
            MouseDownRequest::SetMouseDownActive {
                element_id: ElementId::from_term_bytes(vec![2]),
                active: true,
            }
        );
    }

    #[test]
    fn test_text_input_focus_requests_track_target() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_text_input_node(9, 0.0, 0.0, 120.0, 30.0)];

        let press_inside = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 20.0,
            y: 10.0,
        };

        let press_outside = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 300.0,
            y: 300.0,
        };

        let first = processor.text_input_focus_request(&press_inside).unwrap();
        assert_eq!(first, Some(ElementId::from_term_bytes(vec![9])));

        assert!(processor.text_input_focus_request(&press_inside).is_none());

        let blur = processor.text_input_focus_request(&press_outside).unwrap();
        assert_eq!(blur, None);
    }

    #[test]
    fn test_press_requests_activate_on_mouse_click_for_pressable_nodes() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(4, 0.0, 0.0, 100.0, 40.0)]);

        let press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let press_outcome = processor
            .preview_outcome_for_test(&press, None)
            .unwrap_or_default();
        assert!(
            press_outcome
                .element_events
                .iter()
                .all(|event| event.kind != dispatch_outcome::ElementEventKind::Press)
        );
        processor.advance_runtime_state_after_event(&press);

        let release_outcome = processor
            .preview_outcome_for_test(&release, None)
            .unwrap_or_default();
        let press_targets: Vec<&dispatch_outcome::NodeKey> = release_outcome
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Press)
            .map(|event| &event.target)
            .collect();
        assert_eq!(
            press_targets,
            vec![&dispatch_outcome::node_key(&ElementId::from_term_bytes(
                vec![4]
            ))]
        );
        processor.advance_runtime_state_after_event(&release);
    }

    #[test]
    fn test_press_requests_activate_on_enter_for_focused_pressable_nodes() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(5, 0.0, 0.0, 100.0, 40.0)]);

        let focus_press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        let focus = processor.text_input_focus_request(&focus_press).unwrap();
        assert_eq!(focus, Some(ElementId::from_term_bytes(vec![5])));

        let enter = InputEvent::Key {
            key: "enter".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        let predicted = processor
            .preview_outcome_for_test(&enter, processor.focused_id.as_ref())
            .expect("preview should produce enter press outcome");
        let press_targets: Vec<&dispatch_outcome::NodeKey> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Press)
            .map(|event| &event.target)
            .collect();
        assert_eq!(
            press_targets,
            vec![&dispatch_outcome::node_key(&ElementId::from_term_bytes(
                vec![5]
            ))]
        );
    }

    #[test]
    fn test_focus_cycle_includes_pressable_nodes() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_text_input_node(1, 0.0, 0.0, 120.0, 30.0),
            make_pressable_node(2, 0.0, 40.0, 120.0, 30.0),
        ];

        let tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        let first = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(first, Some(ElementId::from_term_bytes(vec![1])));

        let second = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(second, Some(ElementId::from_term_bytes(vec![2])));

        let wrapped = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(wrapped, Some(ElementId::from_term_bytes(vec![1])));
    }

    #[test]
    fn test_focus_cycle_includes_clipped_focusables_and_builds_reveal_scrolls() {
        let mut tree = ElementTree::new();

        let mut root_attrs = Attrs::default();
        root_attrs.scrollbar_y = Some(true);
        root_attrs.scroll_y = Some(0.0);
        root_attrs.scroll_y_max = Some(200.0);
        let root_id = ElementId::from_term_bytes(vec![1]);

        let mut visible_attrs = Attrs::default();
        visible_attrs.on_press = Some(true);
        let visible_id = ElementId::from_term_bytes(vec![2]);

        let mut clipped_attrs = Attrs::default();
        clipped_attrs.on_press = Some(true);
        let clipped_id = ElementId::from_term_bytes(vec![3]);

        let root = make_element(
            1,
            root_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 300.0,
            },
            vec![visible_id.clone(), clipped_id.clone()],
        );

        let visible = make_element(
            2,
            visible_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 10.0,
                width: 80.0,
                height: 20.0,
                content_width: 80.0,
                content_height: 20.0,
            },
            Vec::new(),
        );

        let clipped = make_element(
            3,
            clipped_attrs,
            crate::tree::element::Frame {
                x: 0.0,
                y: 220.0,
                width: 80.0,
                height: 30.0,
                content_width: 80.0,
                content_height: 30.0,
            },
            Vec::new(),
        );

        tree.root = Some(root_id);
        tree.insert(root);
        tree.insert(visible);
        tree.insert(clipped);

        let registry = build_event_registry(&mut tree);
        let clipped_node = registry
            .iter()
            .find(|node| node.id == clipped_id)
            .expect("clipped focusable should be present in registry");

        assert!(!clipped_node.visible);
        assert!(clipped_node.flags & EVENT_FOCUSABLE != 0);
        assert_eq!(clipped_node.focus_reveal_scrolls.len(), 1);
        assert_eq!(
            clipped_node.focus_reveal_scrolls[0].element_id,
            ElementId::from_term_bytes(vec![1])
        );
        assert!((clipped_node.focus_reveal_scrolls[0].dy + 150.0).abs() < 0.01);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(registry);

        let tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        let first = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(first, Some(visible_id));

        let second = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(second, Some(clipped_id.clone()));

        let reveal = processor.focus_reveal_scroll_requests(&clipped_id);
        assert_eq!(reveal.len(), 1);
        assert_eq!(reveal[0].0, ElementId::from_term_bytes(vec![1]));
        assert!((reveal[0].2 + 150.0).abs() < 0.01);
    }

    #[test]
    fn test_key_scroll_uses_first_visible_scrollable_without_focus() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_scroll_target_node(1, false, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(3, true, EVENT_SCROLL_Y_POS),
        ];

        let down = InputEvent::Key {
            key: "down".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        assert_eq!(
            processor.scroll_requests(&down),
            vec![(
                ElementId::from_term_bytes(vec![2]),
                0.0,
                -SCROLL_LINE_PIXELS,
            )]
        );
    }

    #[test]
    fn test_key_scroll_falls_back_when_focused_node_has_no_directional_matcher() {
        let focused_id = ElementId::from_term_bytes(vec![4]);

        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_pressable_node(4, 0.0, 0.0, 100.0, 30.0),
            make_scroll_target_node(1, false, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
        ];
        processor.focused_id = Some(focused_id);

        let down = InputEvent::Key {
            key: "down".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        assert_eq!(
            processor.scroll_requests(&down),
            vec![(
                ElementId::from_term_bytes(vec![2]),
                0.0,
                -SCROLL_LINE_PIXELS,
            )]
        );
    }

    #[test]
    fn test_text_input_boundary_falls_back_to_root_scrollable_when_matcher_missing() {
        let focused_id = ElementId::from_term_bytes(vec![5]);
        let left_ancestor_id = ElementId::from_term_bytes(vec![7]);
        let fallback_id = ElementId::from_term_bytes(vec![8]);

        let mut text_node = make_text_input_node(5, 0.0, 0.0, 120.0, 30.0);
        if let Some(descriptor) = text_node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 3;
            descriptor.selection_anchor = None;
        }
        text_node.key_scroll_targets.left = Some(left_ancestor_id.clone());

        let left_ancestor_node = make_scroll_target_node(7, true, EVENT_SCROLL_X_NEG);
        let fallback_node = make_scroll_target_node(8, true, EVENT_SCROLL_X_POS);

        let mut processor = EventProcessor::new();
        processor.registry = vec![text_node, left_ancestor_node, fallback_node];
        processor.focused_id = Some(focused_id);

        let right = InputEvent::Key {
            key: "right".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        assert!(processor.text_input_edit_request(&right).is_none());
        assert_eq!(
            processor.scroll_requests(&right),
            vec![(fallback_id, -SCROLL_LINE_PIXELS, 0.0)]
        );
    }

    #[test]
    fn test_right_arrow_at_text_boundary_scrolls_using_registry_matcher() {
        let focused_id = ElementId::from_term_bytes(vec![5]);
        let ancestor_id = ElementId::from_term_bytes(vec![9]);

        let mut text_node = make_text_input_node(5, 0.0, 0.0, 120.0, 30.0);
        if let Some(descriptor) = text_node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 3;
            descriptor.selection_anchor = None;
        }
        text_node.key_scroll_targets.right = Some(ancestor_id.clone());

        let mut ancestor_node = make_scroll_target_node(9, true, EVENT_SCROLL_X_POS);
        ancestor_node.visible = true;

        let mut processor = EventProcessor::new();
        processor.registry = vec![text_node, ancestor_node];
        processor.focused_id = Some(focused_id.clone());

        let right = InputEvent::Key {
            key: "right".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        assert!(processor.text_input_edit_request(&right).is_none());
        assert_eq!(
            processor.scroll_requests(&right),
            vec![(ancestor_id, -SCROLL_LINE_PIXELS, 0.0)]
        );
    }

    #[test]
    fn test_text_input_focus_requests_cycle_forward_with_tab() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_text_input_node(1, 0.0, 0.0, 120.0, 30.0),
            make_text_input_node(2, 0.0, 40.0, 120.0, 30.0),
            make_text_input_node(3, 0.0, 80.0, 120.0, 30.0),
        ];

        let tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        let first = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(first, Some(ElementId::from_term_bytes(vec![1])));

        let second = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(second, Some(ElementId::from_term_bytes(vec![2])));

        let third = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(third, Some(ElementId::from_term_bytes(vec![3])));

        let wrapped = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(wrapped, Some(ElementId::from_term_bytes(vec![1])));
    }

    #[test]
    fn test_text_input_focus_requests_cycle_reverse_with_shift_tab() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_text_input_node(1, 0.0, 0.0, 120.0, 30.0),
            make_text_input_node(2, 0.0, 40.0, 120.0, 30.0),
            make_text_input_node(3, 0.0, 80.0, 120.0, 30.0),
        ];

        let shift_tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_SHIFT,
        };

        let first = processor.text_input_focus_request(&shift_tab).unwrap();
        assert_eq!(first, Some(ElementId::from_term_bytes(vec![3])));

        let second = processor.text_input_focus_request(&shift_tab).unwrap();
        assert_eq!(second, Some(ElementId::from_term_bytes(vec![2])));

        let third = processor.text_input_focus_request(&shift_tab).unwrap();
        assert_eq!(third, Some(ElementId::from_term_bytes(vec![1])));

        let wrapped = processor.text_input_focus_request(&shift_tab).unwrap();
        assert_eq!(wrapped, Some(ElementId::from_term_bytes(vec![3])));
    }

    #[test]
    fn test_text_input_focus_requests_ignore_tab_with_ctrl_alt_or_meta() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_text_input_node(1, 0.0, 0.0, 120.0, 30.0),
            make_text_input_node(2, 0.0, 40.0, 120.0, 30.0),
        ];

        let tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };
        let ctrl_tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_CTRL,
        };
        let alt_tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_ALT,
        };
        let meta_tab = InputEvent::Key {
            key: "tab".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_META,
        };

        let first = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(first, Some(ElementId::from_term_bytes(vec![1])));

        assert!(processor.text_input_focus_request(&ctrl_tab).is_none());
        assert!(processor.text_input_focus_request(&alt_tab).is_none());
        assert!(processor.text_input_focus_request(&meta_tab).is_none());

        let second = processor.text_input_focus_request(&tab).unwrap();
        assert_eq!(second, Some(ElementId::from_term_bytes(vec![2])));
    }

    #[test]
    fn test_text_input_cursor_click_request_returns_x_for_hit_input() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_text_input_node(3, 0.0, 0.0, 160.0, 24.0)];

        let press_inside = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 42.0,
            y: 12.0,
        };

        let requests = processor.text_input_cursor_requests(&press_inside);
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0],
            TextInputCursorRequest::Set {
                element_id: ElementId::from_term_bytes(vec![3]),
                x: 42.0,
                extend_selection: false,
            }
        );

        let move_inside = InputEvent::CursorPos { x: 55.0, y: 12.0 };
        let drag_requests = processor.text_input_cursor_requests(&move_inside);
        assert_eq!(drag_requests.len(), 1);
        assert_eq!(
            drag_requests[0],
            TextInputCursorRequest::Set {
                element_id: ElementId::from_term_bytes(vec![3]),
                x: 55.0,
                extend_selection: true,
            }
        );

        let release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 55.0,
            y: 12.0,
        };
        assert!(processor.text_input_cursor_requests(&release).is_empty());

        let press_outside = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 300.0,
            y: 12.0,
        };
        assert!(
            processor
                .text_input_cursor_requests(&press_outside)
                .is_empty()
        );
    }

    #[test]
    fn test_text_input_edit_requests_follow_focus_and_keys() {
        let mut processor = EventProcessor::new();
        let mut node = make_text_input_node(5, 0.0, 0.0, 100.0, 20.0);
        if let Some(descriptor) = node.text_input.as_mut() {
            descriptor.content = "abc".to_string();
            descriptor.content_len = 3;
            descriptor.cursor = 2;
        }
        processor.registry = vec![node];

        let focus = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };
        processor.text_input_focus_request(&focus);

        let left = InputEvent::Key {
            key: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
        };

        let (left_id, left_req) = processor.text_input_edit_request(&left).unwrap();
        assert_eq!(left_id, ElementId::from_term_bytes(vec![5]));
        assert_eq!(
            left_req,
            TextInputEditRequest::MoveLeft {
                extend_selection: false
            }
        );

        let commit = InputEvent::TextCommit {
            text: "x".to_string(),
            mods: 0,
        };
        let (_commit_id, commit_req) = processor.text_input_edit_request(&commit).unwrap();
        assert_eq!(commit_req, TextInputEditRequest::Insert("x".to_string()));

        let ctrl_commit = InputEvent::TextCommit {
            text: "x".to_string(),
            mods: crate::input::MOD_CTRL,
        };
        assert!(processor.text_input_edit_request(&ctrl_commit).is_none());

        let shift_right = InputEvent::Key {
            key: "right".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_SHIFT,
        };
        let (_, shift_req) = processor.text_input_edit_request(&shift_right).unwrap();
        assert_eq!(
            shift_req,
            TextInputEditRequest::MoveRight {
                extend_selection: true
            }
        );
    }

    #[test]
    fn test_text_input_command_requests_map_shortcuts() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_text_input_node(8, 0.0, 0.0, 140.0, 24.0)];

        let focus = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 8.0,
            y: 8.0,
        };
        processor.text_input_focus_request(&focus);

        let select_all = InputEvent::Key {
            key: "a".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_CTRL,
        };
        let (_, request) = processor.text_input_command_request(&select_all).unwrap();
        assert_eq!(request, TextInputCommandRequest::SelectAll);

        let paste = InputEvent::Key {
            key: "V".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: crate::input::MOD_META,
        };
        let (_, request) = processor.text_input_command_request(&paste).unwrap();
        assert_eq!(request, TextInputCommandRequest::Paste);

        let middle_paste = InputEvent::CursorButton {
            button: "middle".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 12.0,
            y: 10.0,
        };
        let (_, request) = processor.text_input_command_request(&middle_paste).unwrap();
        assert_eq!(request, TextInputCommandRequest::PastePrimary);
    }

    #[test]
    fn test_text_input_preedit_requests_follow_focus() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_text_input_node(7, 0.0, 0.0, 140.0, 24.0)];

        let focus = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 8.0,
            y: 8.0,
        };
        processor.text_input_focus_request(&focus);

        let preedit = InputEvent::TextPreedit {
            text: "ka".to_string(),
            cursor: Some((2, 2)),
        };

        let (id, req) = processor.text_input_preedit_request(&preedit).unwrap();
        assert_eq!(id, ElementId::from_term_bytes(vec![7]));
        assert_eq!(
            req,
            TextInputPreeditRequest::Set {
                text: "ka".to_string(),
                cursor: Some((2, 2))
            }
        );

        let clear = InputEvent::TextPreeditClear;
        let (_, clear_req) = processor.text_input_preedit_request(&clear).unwrap();
        assert_eq!(clear_req, TextInputPreeditRequest::Clear);
    }
}
