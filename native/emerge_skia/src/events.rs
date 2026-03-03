use rustler::{Atom, Encoder, LocalPid, OwnedBinary, OwnedEnv};

use crate::input::{
    ACTION_PRESS, EVENT_CLICK, EVENT_FOCUSABLE, EVENT_MOUSE_DOWN, EVENT_MOUSE_DOWN_STYLE,
    EVENT_MOUSE_ENTER, EVENT_MOUSE_LEAVE, EVENT_MOUSE_MOVE, EVENT_MOUSE_OVER_STYLE, EVENT_MOUSE_UP,
    EVENT_PRESS, EVENT_SCROLL_X_NEG, EVENT_SCROLL_X_POS, EVENT_SCROLL_Y_NEG, EVENT_SCROLL_Y_POS,
    EVENT_TEXT_INPUT, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT, SCROLL_LINE_PIXELS,
};
use crate::tree::attrs::{BorderWidth, Font, Padding, TextAlign};
use crate::tree::element::{ElementId, ElementKind, ElementTree};
use crate::tree::scrollbar::{self as tree_scrollbar, ScrollbarAxis};

mod registry_v2;
mod runtime;
mod scrollbar;
mod dispatch_outcome;
use registry_v2::{DispatchCtx, DispatchJob, DispatchRuleAction, EventRegistryV2, TriggerId};
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
    registry_v2: EventRegistryV2,
    pressed_id: Option<ElementId>,
    pending_press_id: Option<ElementId>,
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
pub(crate) struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn from_frame(frame: crate::tree::element::Frame) -> Self {
        Self {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        }
    }

    fn intersect(self, other: Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);
        if x2 <= x1 || y2 <= y1 {
            return None;
        }
        Some(Rect {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        })
    }

    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CornerRadii {
    tl: f32,
    tr: f32,
    br: f32,
    bl: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ClipContext {
    rect: Rect,
    radii: Option<CornerRadii>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MouseOverRequest {
    SetMouseOverActive { element_id: ElementId, active: bool },
}

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

pub fn build_event_registry(tree: &ElementTree) -> Vec<EventNode> {
    let Some(root) = tree.root.as_ref() else {
        return Vec::new();
    };

    let mut registry = Vec::new();
    collect_event_nodes(tree, root, &mut registry, 0.0, 0.0, None, &[]);
    registry
}

fn collect_event_nodes(
    tree: &ElementTree,
    id: &ElementId,
    registry: &mut Vec<EventNode>,
    offset_x: f32,
    offset_y: f32,
    clip_rect: Option<ClipContext>,
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

    let mut next_clip = clip_rect;
    let mut next_scroll_contexts = scroll_contexts.to_vec();
    let mut scroll_x = 0.0;
    let mut scroll_y = 0.0;

    if let Some(frame) = element.frame {
        let frame_rect = Rect::from_frame(frame);
        let adjusted_rect = Rect {
            x: frame_rect.x - offset_x,
            y: frame_rect.y - offset_y,
            width: frame_rect.width,
            height: frame_rect.height,
        };
        let mut visible_rect = adjusted_rect;
        let active_clip_rect = clip_rect.map(|ctx| ctx.rect);
        let active_clip_radii = clip_rect.and_then(|ctx| ctx.radii);
        if let Some(active_clip) = active_clip_rect {
            if let Some(intersected) = adjusted_rect.intersect(active_clip) {
                visible_rect = intersected;
            } else {
                visible_rect = Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                };
            }
        }

        let visible = visible_rect.width > 0.0 && visible_rect.height > 0.0;

        let self_radii = radii_from_border_radius(element.attrs.border_radius.as_ref())
            .map(|radii| clamp_radii(adjusted_rect, radii));
        let clip_radii = active_clip_rect
            .and_then(|rect| active_clip_radii.map(|radii| clamp_radii(rect, radii)));
        let scrollbar_x = tree_scrollbar::horizontal_metrics(frame, &element.attrs)
            .map(|metrics| scrollbar_node_from_metrics(metrics, offset_x, offset_y));
        let scrollbar_y = tree_scrollbar::vertical_metrics(frame, &element.attrs)
            .map(|metrics| scrollbar_node_from_metrics(metrics, offset_x, offset_y));
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

        scroll_x = current_scroll_x;
        scroll_y = current_scroll_y;

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

        let clip_enabled = element.attrs.clip_x.unwrap_or(false)
            || element.attrs.clip_y.unwrap_or(false)
            || element.attrs.scrollbar_x.unwrap_or(false)
            || element.attrs.scrollbar_y.unwrap_or(false);

        if clip_enabled {
            let clip_radii = radii_from_border_radius(element.attrs.border_radius.as_ref());
            let clip_rect = match clip_rect {
                Some(active_clip) => content_rect.intersect(active_clip.rect).unwrap_or(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }),
                None => content_rect,
            };

            let clipped_radii = clip_radii.map(|radii| clamp_radii(clip_rect, radii));

            next_clip = Some(ClipContext {
                rect: clip_rect,
                radii: clipped_radii,
            });
        }
    }

    let child_offset_x = offset_x + scroll_x;
    let child_offset_y = offset_y + scroll_y;

    for child_id in &element.children {
        collect_event_nodes(
            tree,
            child_id,
            registry,
            child_offset_x,
            child_offset_y,
            next_clip,
            &next_scroll_contexts,
        );
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
        && !point_in_rounded_rect(rect, radii, x, y)
    {
        return false;
    }
    if let Some(radii) = node.self_radii
        && !point_in_rounded_rect(node.self_rect, radii, x, y)
    {
        return false;
    }
    true
}

fn radii_from_border_radius(
    radius: Option<&crate::tree::attrs::BorderRadius>,
) -> Option<CornerRadii> {
    match radius {
        Some(crate::tree::attrs::BorderRadius::Uniform(v)) => {
            let value = *v as f32;
            Some(CornerRadii {
                tl: value,
                tr: value,
                br: value,
                bl: value,
            })
        }
        Some(crate::tree::attrs::BorderRadius::Corners { tl, tr, br, bl }) => Some(CornerRadii {
            tl: *tl as f32,
            tr: *tr as f32,
            br: *br as f32,
            bl: *bl as f32,
        }),
        None => None,
    }
}

fn clamp_radii(rect: Rect, radii: CornerRadii) -> CornerRadii {
    let max_x = rect.width / 2.0;
    let max_y = rect.height / 2.0;
    let clamp = |r: f32| r.min(max_x).min(max_y).max(0.0);
    CornerRadii {
        tl: clamp(radii.tl),
        tr: clamp(radii.tr),
        br: clamp(radii.br),
        bl: clamp(radii.bl),
    }
}

fn point_in_rounded_rect(rect: Rect, radii: CornerRadii, x: f32, y: f32) -> bool {
    if !rect.contains(x, y) {
        return false;
    }

    let check_corner = |cx: f32, cy: f32, r: f32, px: f32, py: f32| {
        let dx = px - cx;
        let dy = py - cy;
        dx * dx + dy * dy <= r * r
    };

    if radii.tl > 0.0 && x < rect.x + radii.tl && y < rect.y + radii.tl {
        return check_corner(rect.x + radii.tl, rect.y + radii.tl, radii.tl, x, y);
    }
    if radii.tr > 0.0 && x > rect.x + rect.width - radii.tr && y < rect.y + radii.tr {
        return check_corner(
            rect.x + rect.width - radii.tr,
            rect.y + radii.tr,
            radii.tr,
            x,
            y,
        );
    }
    if radii.br > 0.0 && x > rect.x + rect.width - radii.br && y > rect.y + rect.height - radii.br {
        return check_corner(
            rect.x + rect.width - radii.br,
            rect.y + rect.height - radii.br,
            radii.br,
            x,
            y,
        );
    }
    if radii.bl > 0.0 && x < rect.x + radii.bl && y > rect.y + rect.height - radii.bl {
        return check_corner(
            rect.x + radii.bl,
            rect.y + rect.height - radii.bl,
            radii.bl,
            x,
            y,
        );
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
            registry_v2: EventRegistryV2::default(),
            pressed_id: None,
            pending_press_id: None,
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
        self.registry_v2 = EventRegistryV2::from_event_nodes(&self.registry);

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

    pub(crate) fn detect_click(&mut self, event: &InputEvent) -> Option<ElementId> {
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

        if *action == crate::input::ACTION_PRESS {
            if hit_test_scrollbar(&self.registry, *x, *y).is_some() {
                self.scrollbar_interaction.mark_captured();
                self.pressed_id = None;
                self.pending_press_id = None;
                self.drag_start = None;
                self.drag_last_pos = None;
                self.drag_active = false;
                self.drag_consumed = true;
                return None;
            }

            self.scrollbar_interaction.clear();
            self.pending_press_id = None;
            let hit = hit_test_with_flag(&self.registry, *x, *y, EVENT_CLICK | EVENT_PRESS);
            self.pressed_id = hit;
            self.drag_start = Some((*x, *y));
            self.drag_last_pos = Some((*x, *y));
            self.drag_active = false;
            self.drag_consumed = false;
            return None;
        }

        if *action == crate::input::ACTION_RELEASE {
            let consumed_by_scrollbar = self.scrollbar_interaction.is_captured();
            if consumed_by_scrollbar {
                self.scrollbar_interaction.suppress_release();
            } else {
                self.scrollbar_interaction.clear();
            }

            let hit = hit_test_with_flag(&self.registry, *x, *y, EVENT_CLICK | EVENT_PRESS);
            let pressed = self.pressed_id.take();
            self.drag_start = None;
            self.drag_last_pos = None;
            let was_dragged = self.drag_consumed;
            self.drag_active = false;
            self.drag_consumed = false;
            self.pending_press_id = None;
            if consumed_by_scrollbar || was_dragged {
                return None;
            }
            if let (Some(pressed_id), Some(hit_id)) = (pressed, hit)
                && pressed_id == hit_id
            {
                if self.node_has_flag(&pressed_id, EVENT_PRESS) {
                    self.pending_press_id = Some(pressed_id.clone());
                }

                if self.node_has_flag(&pressed_id, EVENT_CLICK) {
                    return Some(pressed_id);
                }
            }
        }

        None
    }

    pub(crate) fn detect_press(&mut self, event: &InputEvent) -> Option<ElementId> {
        match event {
            InputEvent::CursorButton { button, action, .. }
                if button == "left" && *action == crate::input::ACTION_RELEASE =>
            {
                self.pending_press_id.take()
            }
            InputEvent::Key { key, action, mods }
                if *action == ACTION_PRESS && key.eq_ignore_ascii_case("enter") =>
            {
                let blocked_mods = MOD_CTRL | MOD_ALT | MOD_META;
                if *mods & blocked_mods != 0 {
                    return None;
                }

                let focused_id = self.focused_id.as_ref()?.clone();
                if self.node_has_flag(&focused_id, EVENT_PRESS) {
                    Some(focused_id)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(crate) fn detect_mouse_button_event(
        &mut self,
        event: &InputEvent,
    ) -> Option<(ElementId, dispatch_outcome::ElementEventKind)> {
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

        match *action {
            crate::input::ACTION_PRESS => {
                if self.scrollbar_interaction.is_captured()
                    || hit_test_scrollbar(&self.registry, *x, *y).is_some()
                {
                    return None;
                }
            }
            crate::input::ACTION_RELEASE => {
                if self.scrollbar_interaction.take_release_suppression() {
                    return None;
                }
            }
            _ => return None,
        }

        let (flag, kind) = match *action {
            crate::input::ACTION_PRESS => {
                (EVENT_MOUSE_DOWN, dispatch_outcome::ElementEventKind::MouseDown)
            }
            crate::input::ACTION_RELEASE => (EVENT_MOUSE_UP, dispatch_outcome::ElementEventKind::MouseUp),
            _ => return None,
        };

        let hit = hit_test_with_flag(&self.registry, *x, *y, flag)?;
        Some((hit, kind))
    }

    pub(crate) fn handle_hover_event(
        &mut self,
        event: &InputEvent,
    ) -> Vec<(ElementId, dispatch_outcome::ElementEventKind)> {
        let mut emitted = Vec::new();

        match event {
            InputEvent::CursorPos { x, y } => {
                let hover_mask = EVENT_MOUSE_ENTER | EVENT_MOUSE_LEAVE | EVENT_MOUSE_MOVE;
                let hit = hit_test_with_flag(&self.registry, *x, *y, hover_mask);

                if hit != self.hovered_id {
                    if let Some(previous) = self.hovered_id.take()
                        && self.node_has_flag(&previous, EVENT_MOUSE_LEAVE)
                    {
                        emitted.push((previous, dispatch_outcome::ElementEventKind::MouseLeave));
                    }

                    if let Some(new_id) = hit.clone()
                        && self.node_has_flag(&new_id, EVENT_MOUSE_ENTER)
                    {
                        emitted.push((new_id.clone(), dispatch_outcome::ElementEventKind::MouseEnter));
                    }

                    self.hovered_id = hit;
                }

                if let Some(current) = self.hovered_id.as_ref()
                    && self.node_has_flag(current, EVENT_MOUSE_MOVE)
                {
                    emitted.push((current.clone(), dispatch_outcome::ElementEventKind::MouseMove));
                }
            }
            InputEvent::CursorEntered { entered } => {
                if !*entered
                    && let Some(previous) = self.hovered_id.take()
                    && self.node_has_flag(&previous, EVENT_MOUSE_LEAVE)
                {
                    emitted.push((previous, dispatch_outcome::ElementEventKind::MouseLeave));
                }
            }
            _ => {}
        }

        emitted
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

    fn preview_text_char_len(content: &str) -> u32 {
        content.chars().count() as u32
    }

    fn preview_char_to_byte_index(content: &str, char_index: u32) -> usize {
        content
            .char_indices()
            .nth(char_index as usize)
            .map(|(idx, _)| idx)
            .unwrap_or(content.len())
    }

    fn preview_selected_range(
        cursor: u32,
        selection_anchor: Option<u32>,
        content_len: u32,
    ) -> Option<(u32, u32)> {
        let cursor = cursor.min(content_len);
        let anchor = selection_anchor?.min(content_len);
        if anchor == cursor {
            return None;
        }

        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    fn preview_next_content_for_edit(
        descriptor: &TextInputDescriptor,
        request: &TextInputEditRequest,
    ) -> Option<String> {
        let content_len = Self::preview_text_char_len(&descriptor.content);
        let cursor = descriptor.cursor.min(content_len);
        let selection_anchor = descriptor
            .selection_anchor
            .map(|anchor| anchor.min(content_len));

        match request {
            TextInputEditRequest::Insert(text) => {
                if text.is_empty() {
                    return None;
                }

                let (start, end) =
                    Self::preview_selected_range(cursor, selection_anchor, content_len)
                        .unwrap_or((cursor, cursor));

                let mut next = descriptor.content.clone();
                let start_byte = Self::preview_char_to_byte_index(&next, start);
                let end_byte = Self::preview_char_to_byte_index(&next, end);
                next.replace_range(start_byte..end_byte, text);

                if next == descriptor.content {
                    None
                } else {
                    Some(next)
                }
            }
            TextInputEditRequest::Backspace => {
                let (start, end) =
                    Self::preview_selected_range(cursor, selection_anchor, content_len)
                        .unwrap_or((cursor, cursor));

                if start != end {
                    let mut next = descriptor.content.clone();
                    let start_byte = Self::preview_char_to_byte_index(&next, start);
                    let end_byte = Self::preview_char_to_byte_index(&next, end);
                    next.replace_range(start_byte..end_byte, "");
                    return Some(next);
                }

                if cursor == 0 {
                    return None;
                }

                let mut next = descriptor.content.clone();
                let start_byte = Self::preview_char_to_byte_index(&next, cursor - 1);
                let end_byte = Self::preview_char_to_byte_index(&next, cursor);
                next.replace_range(start_byte..end_byte, "");
                Some(next)
            }
            TextInputEditRequest::Delete => {
                let (start, end) =
                    Self::preview_selected_range(cursor, selection_anchor, content_len)
                        .unwrap_or((cursor, cursor));

                if start != end {
                    let mut next = descriptor.content.clone();
                    let start_byte = Self::preview_char_to_byte_index(&next, start);
                    let end_byte = Self::preview_char_to_byte_index(&next, end);
                    next.replace_range(start_byte..end_byte, "");
                    return Some(next);
                }

                if cursor >= content_len {
                    return None;
                }

                let mut next = descriptor.content.clone();
                let start_byte = Self::preview_char_to_byte_index(&next, cursor);
                let end_byte = Self::preview_char_to_byte_index(&next, cursor + 1);
                next.replace_range(start_byte..end_byte, "");
                Some(next)
            }
            TextInputEditRequest::MoveLeft { .. }
            | TextInputEditRequest::MoveRight { .. }
            | TextInputEditRequest::MoveHome { .. }
            | TextInputEditRequest::MoveEnd { .. } => None,
        }
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

    fn apply_v2_focus_change_action(
        &self,
        next: Option<u32>,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) {
        out.focus_change =
            Some(next.and_then(|idx| self.registry_v2.node_id(idx).map(dispatch_outcome::node_key)));
    }

    fn apply_v2_scroll_action(
        &self,
        element: u32,
        dx: f32,
        dy: f32,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) {
        if let Some(target_id) = self.registry_v2.node_id(element) {
            Self::push_unique_scroll_request(out, target_id, dx, dy);
        }
    }

    fn v2_element_event_kind_for_trigger(
        trigger: TriggerId,
    ) -> Option<dispatch_outcome::ElementEventKind> {
        match trigger {
            TriggerId::KeyEnterPress => Some(dispatch_outcome::ElementEventKind::Press),
            _ => None,
        }
    }

    fn apply_v2_emit_event_action(
        &self,
        trigger: TriggerId,
        element: u32,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) -> bool {
        let Some(kind) = Self::v2_element_event_kind_for_trigger(trigger) else {
            return false;
        };
        let Some(target_id) = self.registry_v2.node_id(element) else {
            return false;
        };

        out.element_events.push(dispatch_outcome::ElementEventOut {
            target: dispatch_outcome::node_key(target_id),
            kind,
            payload: None,
        });

        trigger == TriggerId::KeyEnterPress && kind == dispatch_outcome::ElementEventKind::Press
    }

    fn apply_v2_keyboard_focus_actions(
        &self,
        trigger: TriggerId,
        actions: &[DispatchRuleAction],
        out: &mut dispatch_outcome::DispatchOutcome,
    ) -> bool {
        let mut enter_press_emitted = false;

        for action in actions {
            match action {
                DispatchRuleAction::FocusChange { next } => {
                    self.apply_v2_focus_change_action(*next, out);
                }
                DispatchRuleAction::ScrollRequest { element, dx, dy } => {
                    self.apply_v2_scroll_action(*element, *dx, *dy, out);
                }
                DispatchRuleAction::EmitElementEvent { element } => {
                    if self.apply_v2_emit_event_action(trigger, *element, out) {
                        enter_press_emitted = true;
                    }
                }
                DispatchRuleAction::TextCommand { .. }
                | DispatchRuleAction::TextEdit { .. }
                | DispatchRuleAction::TextPreedit { .. } => {}
            }
        }

        enter_press_emitted
    }

    fn apply_v2_keyboard_focus_job_for_event(
        &self,
        event: &InputEvent,
        focused: Option<&ElementId>,
        out: &mut dispatch_outcome::DispatchOutcome,
    ) -> (Option<TriggerId>, bool) {
        let trigger = self.v2_trigger_for_input_event(event);

        let mods = match event {
            InputEvent::Key { mods, .. } => *mods,
            _ => 0,
        };

        let focused_idx = focused.and_then(|id| self.registry_v2.node_idx(id));
        let ctx = DispatchCtx {
            mods,
            focused: focused_idx,
            cursor: None,
            runtime_flags: 0,
        };

        let Some(trigger) = trigger else {
            return (None, false);
        };

        let job = if let Some(target) = focused_idx {
            DispatchJob::Targeted {
                trigger,
                target,
                ctx,
            }
        } else {
            DispatchJob::Untargeted { trigger, ctx }
        };

        let enter_press_emitted_by_v2 = self
            .registry_v2
            .resolve_actions_for_job(&job)
            .is_some_and(|actions| self.apply_v2_keyboard_focus_actions(trigger, actions, out));

        (Some(trigger), enter_press_emitted_by_v2)
    }

    pub(crate) fn v2_trigger_for_input_event(&self, event: &InputEvent) -> Option<TriggerId> {
        match event {
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
            InputEvent::TextCommit { .. } => Some(TriggerId::TextCommit),
            InputEvent::TextPreedit { .. } => Some(TriggerId::TextPreedit),
            InputEvent::TextPreeditClear => Some(TriggerId::TextPreeditClear),
            _ => None,
        }
    }

    pub(crate) fn preview_v2_keyboard_focus_outcome(
        &self,
        event: &InputEvent,
        focused: Option<&ElementId>,
    ) -> Option<dispatch_outcome::DispatchOutcome> {
        let previous_focus = focused.cloned();

        let mut out = dispatch_outcome::DispatchOutcome::default();
        let (trigger, enter_press_emitted_by_v2) =
            self.apply_v2_keyboard_focus_job_for_event(event, focused, &mut out);

        let mut shadow = self.clone();

        if let Some(clicked_id) = shadow.detect_click(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&clicked_id),
                kind: dispatch_outcome::ElementEventKind::Click,
                payload: None,
            });
        }

        let allow_fallback_enter_press =
            !(trigger == Some(TriggerId::KeyEnterPress) && enter_press_emitted_by_v2);
        if allow_fallback_enter_press && let Some(pressed_id) = shadow.detect_press(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&pressed_id),
                kind: dispatch_outcome::ElementEventKind::Press,
                payload: None,
            });
        }

        if let Some((mouse_id, kind)) = shadow.detect_mouse_button_event(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&mouse_id),
                kind,
                payload: None,
            });
        }

        for (hover_id, kind) in shadow.handle_hover_event(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&hover_id),
                kind,
                payload: None,
            });
        }

        let mut focus_transition_events_emitted = false;
        if let Some(next_focus) = shadow.text_input_focus_request(event) {
            out.focus_change = Some(next_focus.as_ref().map(dispatch_outcome::node_key));

            if previous_focus != next_focus {
                if let Some(prev_id) = previous_focus.as_ref() {
                    out.element_events.push(dispatch_outcome::ElementEventOut {
                        target: dispatch_outcome::node_key(prev_id),
                        kind: dispatch_outcome::ElementEventKind::Blur,
                        payload: None,
                    });
                }

                if let Some(next_id) = next_focus.as_ref() {
                    out.element_events.push(dispatch_outcome::ElementEventOut {
                        target: dispatch_outcome::node_key(next_id),
                        kind: dispatch_outcome::ElementEventKind::Focus,
                        payload: None,
                    });
                }

                focus_transition_events_emitted = true;
            }

            if let Some(focused_id) = next_focus.as_ref() {
                for (id, dx, dy) in shadow.focus_reveal_scroll_requests(focused_id) {
                    Self::push_unique_scroll_request(&mut out, &id, dx, dy);
                }
            }
        }

        if !focus_transition_events_emitted && let Some(next_focus) = out.focus_change.as_ref() {
            let previous_focus_key = previous_focus.as_ref().map(dispatch_outcome::node_key);
            if previous_focus_key != *next_focus {
                if let Some(prev_key) = previous_focus_key {
                    out.element_events.push(dispatch_outcome::ElementEventOut {
                        target: prev_key,
                        kind: dispatch_outcome::ElementEventKind::Blur,
                        payload: None,
                    });
                }

                if let Some(next_key) = next_focus.as_ref() {
                    out.element_events.push(dispatch_outcome::ElementEventOut {
                        target: next_key.clone(),
                        kind: dispatch_outcome::ElementEventKind::Focus,
                        payload: None,
                    });
                }
            }
        }

        for (id, dx, dy) in shadow.scroll_requests(event) {
            Self::push_unique_scroll_request(&mut out, &id, dx, dy);
        }

        for request in shadow.scrollbar_thumb_drag_requests(event) {
            match request {
                ScrollbarThumbDragRequest::X { element_id, dx } => {
                    out.scrollbar_thumb_drag_requests
                        .push(dispatch_outcome::ScrollbarThumbDragReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::X,
                            delta: dispatch_outcome::milli(dx),
                        });
                }
                ScrollbarThumbDragRequest::Y { element_id, dy } => {
                    out.scrollbar_thumb_drag_requests
                        .push(dispatch_outcome::ScrollbarThumbDragReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::Y,
                            delta: dispatch_outcome::milli(dy),
                        });
                }
            }
        }

        for request in shadow.scrollbar_hover_requests(event) {
            match request {
                ScrollbarHoverRequest::X {
                    element_id,
                    hovered,
                } => {
                    out.scrollbar_hover_requests
                        .push(dispatch_outcome::ScrollbarHoverReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::X,
                            hovered,
                        });
                }
                ScrollbarHoverRequest::Y {
                    element_id,
                    hovered,
                } => {
                    out.scrollbar_hover_requests
                        .push(dispatch_outcome::ScrollbarHoverReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::Y,
                            hovered,
                        });
                }
            }
        }

        for request in shadow.mouse_over_requests(event) {
            match request {
                MouseOverRequest::SetMouseOverActive { element_id, active } => {
                    out.style_runtime_requests
                        .push(dispatch_outcome::StyleRuntimeReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            kind: dispatch_outcome::StyleRuntimeKind::MouseOver,
                            active,
                        });
                }
            }
        }

        for request in shadow.mouse_down_requests(event) {
            match request {
                MouseDownRequest::SetMouseDownActive { element_id, active } => {
                    out.style_runtime_requests
                        .push(dispatch_outcome::StyleRuntimeReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            kind: dispatch_outcome::StyleRuntimeKind::MouseDown,
                            active,
                        });
                }
            }
        }

        if let Some((element_id, request)) = self.text_input_command_request(event) {
            out.text_command_requests
                .push(dispatch_outcome::TextCommandReqOut {
                    target: dispatch_outcome::node_key(&element_id),
                    request,
                });
        }

        if let Some((element_id, request)) = self.text_input_edit_request(event) {
            out.text_edit_requests.push(dispatch_outcome::TextEditReqOut {
                target: dispatch_outcome::node_key(&element_id),
                request: request.clone(),
            });

            if let Some(descriptor) = self.text_input_descriptor(&element_id)
                && let Some(next_content) =
                    Self::preview_next_content_for_edit(descriptor, &request)
            {
                out.element_events.push(dispatch_outcome::ElementEventOut {
                    target: dispatch_outcome::node_key(&element_id),
                    kind: dispatch_outcome::ElementEventKind::Change,
                    payload: Some(next_content),
                });
            }
        }

        if let Some((element_id, request)) = self.text_input_preedit_request(event) {
            out.text_preedit_requests
                .push(dispatch_outcome::TextPreeditReqOut {
                    target: dispatch_outcome::node_key(&element_id),
                    request,
                });
        }

        let has_output = out.focus_change.is_some()
            || !out.element_events.is_empty()
            || !out.scroll_requests.is_empty()
            || !out.text_command_requests.is_empty()
            || !out.text_edit_requests.is_empty()
            || !out.text_preedit_requests.is_empty()
            || !out.scrollbar_thumb_drag_requests.is_empty()
            || !out.scrollbar_hover_requests.is_empty()
            || !out.style_runtime_requests.is_empty();

        if has_output { Some(out) } else { None }
    }

    #[cfg(test)]
    pub(crate) fn registry_v2(&self) -> &EventRegistryV2 {
        &self.registry_v2
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
        let Some(next_hover) = self.next_scrollbar_thumb_hover(event) else {
            return Vec::new();
        };

        if self.hovered_scrollbar_thumb == next_hover {
            return Vec::new();
        }

        let previous = self.hovered_scrollbar_thumb.clone();
        self.hovered_scrollbar_thumb = next_hover.clone();

        let mut requests = Vec::with_capacity(2);
        if let Some(prev) = previous {
            requests.push(Self::hover_request(prev, false));
        }
        if let Some(next) = next_hover {
            requests.push(Self::hover_request(next, true));
        }
        requests
    }

    pub fn mouse_over_requests(&mut self, event: &InputEvent) -> Vec<MouseOverRequest> {
        let next_active = match event {
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
        };

        let Some(next_active) = next_active else {
            return Vec::new();
        };

        if self.mouse_over_active_id == next_active {
            return Vec::new();
        }

        let previous = self.mouse_over_active_id.clone();
        self.mouse_over_active_id = next_active.clone();

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

    pub fn mouse_down_requests(&mut self, event: &InputEvent) -> Vec<MouseDownRequest> {
        let next_active = match event {
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
        };

        let Some(next_active) = next_active else {
            return Vec::new();
        };

        if self.mouse_down_active_id == next_active {
            return Vec::new();
        }

        let previous = self.mouse_down_active_id.clone();
        self.mouse_down_active_id = next_active.clone();

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
        let InputEvent::CursorButton {
            button,
            action,
            x,
            y,
            ..
        } = event
        else {
            return Vec::new();
        };

        if button != "left" {
            return Vec::new();
        }

        if *action == crate::input::ACTION_RELEASE {
            self.scrollbar_interaction.clear();
            return Vec::new();
        }

        if *action != crate::input::ACTION_PRESS {
            return Vec::new();
        }

        let Some(hit) = hit_test_scrollbar(&self.registry, *x, *y) else {
            return Vec::new();
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

        if hit.area == ScrollbarHitArea::Track
            && let Some(request) = Self::scrollbar_drag_request(
                &hit.id,
                hit.axis,
                hit.node.scroll_offset,
                target_scroll,
            )
        {
            return vec![request];
        }

        Vec::new()
    }

    fn handle_scrollbar_drag_requests(
        &mut self,
        event: &InputEvent,
    ) -> Vec<ScrollbarThumbDragRequest> {
        let InputEvent::CursorPos { x, y } = event else {
            return Vec::new();
        };

        let Some(state) = self.scrollbar_interaction.dragging() else {
            return Vec::new();
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
            return Vec::new();
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
            return Vec::new();
        }

        if let Some(state) = self.scrollbar_interaction.dragging_mut() {
            state.current_scroll = target_scroll;
        }

        Self::scrollbar_drag_request(&id, axis, current_scroll, target_scroll)
            .map(|request| vec![request])
            .unwrap_or_default()
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
        if self.scrollbar_interaction.blocks_content_drag() {
            return Vec::new();
        }

        let InputEvent::CursorPos { x, y } = event else {
            return Vec::new();
        };

        let Some((start_x, start_y)) = self.drag_start else {
            return Vec::new();
        };

        let last_pos = self.drag_last_pos.unwrap_or((start_x, start_y));
        let dx = x - last_pos.0;
        let dy = y - last_pos.1;

        if !self.drag_active {
            let total_dx = x - start_x;
            let total_dy = y - start_y;
            let distance = (total_dx * total_dx + total_dy * total_dy).sqrt();
            if distance < DRAG_DEADZONE {
                return Vec::new();
            }
            self.drag_active = true;
            self.drag_consumed = true;
        }

        self.drag_last_pos = Some((*x, *y));

        if dx == 0.0 && dy == 0.0 {
            return Vec::new();
        }

        let mut requests = Vec::new();

        if dx != 0.0 {
            let flag = if dx > 0.0 {
                EVENT_SCROLL_X_NEG
            } else {
                EVENT_SCROLL_X_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                requests.push((id, dx, 0.0));
            }
        }

        if dy != 0.0 {
            let flag = if dy > 0.0 {
                EVENT_SCROLL_Y_NEG
            } else {
                EVENT_SCROLL_Y_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                requests.push((id, 0.0, dy));
            }
        }

        requests
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
    use super::registry_v2::{
        DispatchCtx, DispatchJob, DispatchRuleAction, EventRegistryV2, ScrollDirection, TriggerId,
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
    fn test_registry_v2_focus_order_matches_current_registry_order() {
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

        let registry_v2 = EventRegistryV2::from_event_nodes(&registry);
        assert_eq!(registry_v2.focus_order_ids(), expected);
    }

    #[test]
    fn test_registry_v2_first_visible_scrollable_matches_existing_logic() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![
            make_scroll_target_node(1, false, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
            make_scroll_target_node(3, true, EVENT_SCROLL_Y_POS),
        ];

        let old = processor.first_visible_scroll_target(KeyScrollDirection::Down);

        let registry_v2 = EventRegistryV2::from_event_nodes(&processor.registry);
        let v2 = registry_v2.first_visible_scrollable_id(ScrollDirection::Down);

        assert_eq!(v2, old);
    }

    #[test]
    fn test_registry_v2_pointer_candidates_keep_topmost_first_order() {
        let registry = vec![
            make_scroll_target_node(1, true, EVENT_CLICK),
            make_scroll_target_node(2, true, EVENT_CLICK),
            make_scroll_target_node(3, true, EVENT_CLICK),
        ];

        let top_hit = hit_test_with_flag(&registry, 10.0, 10.0, EVENT_CLICK).unwrap();
        assert_eq!(top_hit, ElementId::from_term_bytes(vec![3]));

        let registry_v2 = EventRegistryV2::from_event_nodes(&registry);
        let candidate_ids: Vec<ElementId> = registry_v2
            .pointer_candidates(TriggerId::CursorButtonLeftPress)
            .iter()
            .filter_map(|idx| registry_v2.node_id(*idx).cloned())
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
    fn test_rebuild_registry_populates_v2_indexes() {
        let mut processor = EventProcessor::new();
        let registry = vec![
            make_pressable_node(1, 0.0, 0.0, 20.0, 20.0),
            make_scroll_target_node(2, true, EVENT_SCROLL_Y_POS),
        ];

        processor.rebuild_registry(registry);

        let next_focus = processor
            .registry_v2()
            .focus_order_next(None, false)
            .and_then(|idx| processor.registry_v2().node_id(idx).cloned());
        assert_eq!(next_focus, Some(ElementId::from_term_bytes(vec![1])));

        let first_scroll = processor
            .registry_v2()
            .first_visible_scrollable_id(ScrollDirection::Down);
        assert_eq!(first_scroll, Some(ElementId::from_term_bytes(vec![2])));
    }

    fn first_scroll_action(
        registry_v2: &EventRegistryV2,
        actions: &[DispatchRuleAction],
    ) -> Option<(ElementId, f32, f32)> {
        actions.iter().find_map(|action| match action {
            DispatchRuleAction::ScrollRequest { element, dx, dy } => registry_v2
                .node_id(*element)
                .cloned()
                .map(|id| (id, *dx, *dy)),
            _ => None,
        })
    }

    fn first_focus_change_action(
        registry_v2: &EventRegistryV2,
        actions: &[DispatchRuleAction],
    ) -> Option<ElementId> {
        actions.iter().find_map(|action| match action {
            DispatchRuleAction::FocusChange { next: Some(next) } => {
                registry_v2.node_id(*next).cloned()
            }
            _ => None,
        })
    }

    #[test]
    fn test_registry_v2_arrow_down_parity_without_focus() {
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
        let old = processor.handle_key_scroll_requests(&down);

        let v2_actions = processor
            .registry_v2()
            .resolve_actions_for_job(&DispatchJob::Untargeted {
                trigger: TriggerId::KeyDownPress,
                ctx: DispatchCtx::default(),
            })
            .expect("v2 should resolve a no-focus arrow rule");
        let v2 = first_scroll_action(processor.registry_v2(), v2_actions)
            .expect("v2 should emit a scroll action");

        assert_eq!(old.len(), 1);
        assert_eq!(old[0].0, v2.0);
        assert!((old[0].1 - v2.1).abs() < f32::EPSILON);
        assert!((old[0].2 - v2.2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_registry_v2_arrow_uses_focused_directional_matcher_before_fallback() {
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
        let old = processor.handle_key_scroll_requests(&down);

        let focused_idx = processor
            .registry_v2()
            .node_idx(&focused_id)
            .expect("focused node should exist in v2 registry");
        let v2_actions = processor
            .registry_v2()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyDownPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("v2 should resolve focused directional rule");
        let v2 = first_scroll_action(processor.registry_v2(), v2_actions)
            .expect("v2 should emit focused scroll action");

        assert_eq!(old.len(), 1);
        assert_eq!(old[0].0, focused_scroll_id);
        assert_eq!(old[0].0, v2.0);
        assert_ne!(v2.0, fallback_scroll_id);
    }

    #[test]
    fn test_registry_v2_arrow_falls_back_when_focused_has_no_directional_matcher() {
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
        let old = processor.handle_key_scroll_requests(&down);

        let focused_idx = processor
            .registry_v2()
            .node_idx(&focused_id)
            .expect("focused node should exist in v2 registry");
        let v2_actions = processor
            .registry_v2()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyDownPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("v2 should resolve fallback arrow rule");
        let v2 = first_scroll_action(processor.registry_v2(), v2_actions)
            .expect("v2 should emit fallback scroll action");

        assert_eq!(old.len(), 1);
        assert_eq!(old[0].0, fallback_scroll_id);
        assert_eq!(old[0].0, v2.0);
    }

    #[test]
    fn test_registry_v2_tab_focus_change_parity() {
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
        let old_next = processor.cycle_focus(false);

        let focused_idx = processor
            .registry_v2()
            .node_idx(&id2)
            .expect("focused node should exist in v2 registry");
        let v2_next_actions = processor
            .registry_v2()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyTabPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("v2 should resolve tab forward rule");
        let v2_next = first_focus_change_action(processor.registry_v2(), v2_next_actions)
            .expect("focus action");

        assert_eq!(old_next, Some(id3.clone()));
        assert_eq!(Some(v2_next), old_next);

        let old_prev = processor.cycle_focus(true);
        let v2_prev_actions = processor
            .registry_v2()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyTabPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    mods: MOD_SHIFT,
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("v2 should resolve tab reverse rule");
        let v2_prev = first_focus_change_action(processor.registry_v2(), v2_prev_actions)
            .expect("focus action");

        assert_eq!(old_prev, Some(id1.clone()));
        assert_eq!(Some(v2_prev), old_prev);
    }

    #[test]
    fn test_registry_v2_enter_press_parity_for_focused_pressable() {
        let focused_id = ElementId::from_term_bytes(vec![40]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(40, 0.0, 0.0, 80.0, 30.0)]);
        processor.focused_id = Some(focused_id.clone());

        let enter = InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };
        let old = processor.detect_press(&enter);

        let focused_idx = processor
            .registry_v2()
            .node_idx(&focused_id)
            .expect("focused node should exist in v2 registry");
        let v2_actions = processor
            .registry_v2()
            .resolve_actions_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyEnterPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            })
            .expect("v2 should resolve enter press rule");

        let emitted_id = v2_actions.iter().find_map(|action| match action {
            DispatchRuleAction::EmitElementEvent { element } => {
                processor.registry_v2().node_id(*element).cloned()
            }
            _ => None,
        });

        assert_eq!(old, Some(focused_id.clone()));
        assert_eq!(emitted_id, Some(focused_id));

        let ctrl_enter = InputEvent::Key {
            key: "enter".to_string(),
            action: ACTION_PRESS,
            mods: MOD_CTRL,
        };
        let old_blocked = processor.detect_press(&ctrl_enter);
        let v2_blocked = processor
            .registry_v2()
            .resolve_winner_for_job(&DispatchJob::Targeted {
                trigger: TriggerId::KeyEnterPress,
                target: focused_idx,
                ctx: DispatchCtx {
                    mods: MOD_CTRL,
                    focused: Some(focused_idx),
                    ..DispatchCtx::default()
                },
            });

        assert_eq!(old_blocked, None);
        assert_eq!(v2_blocked, None);
    }

    #[test]
    fn test_registry_v2_keyboard_bucket_population_scaffold() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![
            make_pressable_node(50, 0.0, 0.0, 80.0, 30.0),
            make_scroll_target_node(51, true, EVENT_SCROLL_Y_POS),
        ]);

        let (targeted_enter, ordered_enter) = processor
            .registry_v2()
            .debug_bucket_sizes(TriggerId::KeyEnterPress);
        assert!(targeted_enter >= 1);
        assert_eq!(ordered_enter, 0);

        let (targeted_down, ordered_down) = processor
            .registry_v2()
            .debug_bucket_sizes(TriggerId::KeyDownPress);
        assert_eq!(targeted_down, 0);
        assert!(ordered_down >= 1);
    }

    #[test]
    fn test_v2_preview_text_command_request_parity() {
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

        let old = processor.text_input_command_request(&event).unwrap();
        let predicted = processor
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce command request outcome");

        assert_eq!(predicted.text_command_requests.len(), 1);
        assert_eq!(
            predicted.text_command_requests[0].target,
            dispatch_outcome::node_key(&old.0)
        );
        assert_eq!(predicted.text_command_requests[0].request, old.1);
    }

    #[test]
    fn test_v2_preview_text_edit_request_parity() {
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

        let old = processor.text_input_edit_request(&event).unwrap();
        let predicted = processor
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce edit request outcome");

        assert_eq!(predicted.text_edit_requests.len(), 1);
        assert_eq!(
            predicted.text_edit_requests[0].target,
            dispatch_outcome::node_key(&old.0)
        );
        assert_eq!(predicted.text_edit_requests[0].request, old.1);
    }

    #[test]
    fn test_v2_preview_text_commit_emits_change_event_payload() {
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
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce text commit change event");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].target, dispatch_outcome::node_key(&focused_id));
        assert_eq!(change_events[0].payload.as_deref(), Some("abx"));
    }

    #[test]
    fn test_v2_preview_backspace_emits_change_event_payload() {
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
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce backspace change event");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].target, dispatch_outcome::node_key(&focused_id));
        assert_eq!(change_events[0].payload.as_deref(), Some("abc"));
    }

    #[test]
    fn test_v2_preview_backspace_at_start_emits_no_change_event() {
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
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce backspace outcome");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert!(change_events.is_empty());
    }

    #[test]
    fn test_v2_preview_backspace_with_selection_emits_change_event_payload() {
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
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce selection delete change event");

        let change_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Change)
            .collect();

        assert_eq!(change_events.len(), 1);
        assert_eq!(change_events[0].target, dispatch_outcome::node_key(&focused_id));
        assert_eq!(change_events[0].payload.as_deref(), Some("ad"));
    }

    #[test]
    fn test_v2_preview_text_preedit_request_parity() {
        let focused_id = ElementId::from_term_bytes(vec![63]);
        let node = make_text_input_node(63, 0.0, 0.0, 160.0, 30.0);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![node]);
        processor.focused_id = Some(focused_id);

        let event = InputEvent::TextPreedit {
            text: "kana".to_string(),
            cursor: Some((1, 3)),
        };

        let old = processor.text_input_preedit_request(&event).unwrap();
        let predicted = processor
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce preedit request outcome");

        assert_eq!(predicted.text_preedit_requests.len(), 1);
        assert_eq!(
            predicted.text_preedit_requests[0].target,
            dispatch_outcome::node_key(&old.0)
        );
        assert_eq!(predicted.text_preedit_requests[0].request, old.1);
    }

    #[test]
    fn test_v2_preview_enter_press_does_not_duplicate_press_event() {
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
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce enter press outcome");

        let press_events: Vec<&dispatch_outcome::ElementEventOut> = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == dispatch_outcome::ElementEventKind::Press)
            .collect();

        assert_eq!(press_events.len(), 1);
        assert_eq!(press_events[0].target, dispatch_outcome::node_key(&focused_id));
    }

    #[test]
    fn test_v2_preview_tab_emits_focus_transition_events() {
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
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce tab focus transition outcome");

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
    fn test_v2_preview_window_focus_lost_emits_blur_event() {
        let focused_id = ElementId::from_term_bytes(vec![67]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_pressable_node(67, 0.0, 0.0, 100.0, 30.0)]);
        processor.focused_id = Some(focused_id.clone());

        let event = InputEvent::Focused { focused: false };

        let predicted = processor
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce blur outcome on window focus loss");

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

    fn v1_reference_outcome_for_event(
        processor: &mut EventProcessor,
        event: &InputEvent,
    ) -> dispatch_outcome::DispatchOutcome {
        let mut out = dispatch_outcome::DispatchOutcome::default();

        if let Some(clicked_id) = processor.detect_click(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&clicked_id),
                kind: dispatch_outcome::ElementEventKind::Click,
                payload: None,
            });
        }

        if let Some(pressed_id) = processor.detect_press(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&pressed_id),
                kind: dispatch_outcome::ElementEventKind::Press,
                payload: None,
            });
        }

        if let Some((mouse_id, kind)) = processor.detect_mouse_button_event(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&mouse_id),
                kind,
                payload: None,
            });
        }

        for (hover_id, kind) in processor.handle_hover_event(event) {
            out.element_events.push(dispatch_outcome::ElementEventOut {
                target: dispatch_outcome::node_key(&hover_id),
                kind,
                payload: None,
            });
        }

        if let Some(next_focus) = processor.text_input_focus_request(event) {
            out.focus_change = Some(next_focus.as_ref().map(dispatch_outcome::node_key));

            if let Some(focused_id) = next_focus.as_ref() {
                for (id, dx, dy) in processor.focus_reveal_scroll_requests(focused_id) {
                    out.scroll_requests.push(dispatch_outcome::ScrollRequestOut {
                        target: dispatch_outcome::node_key(&id),
                        dx: dispatch_outcome::milli(dx),
                        dy: dispatch_outcome::milli(dy),
                    });
                }
            }
        }

        for (id, dx, dy) in processor.scroll_requests(event) {
            out.scroll_requests.push(dispatch_outcome::ScrollRequestOut {
                target: dispatch_outcome::node_key(&id),
                dx: dispatch_outcome::milli(dx),
                dy: dispatch_outcome::milli(dy),
            });
        }

        for request in processor.scrollbar_thumb_drag_requests(event) {
            match request {
                ScrollbarThumbDragRequest::X { element_id, dx } => {
                    out.scrollbar_thumb_drag_requests
                        .push(dispatch_outcome::ScrollbarThumbDragReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::X,
                            delta: dispatch_outcome::milli(dx),
                        });
                }
                ScrollbarThumbDragRequest::Y { element_id, dy } => {
                    out.scrollbar_thumb_drag_requests
                        .push(dispatch_outcome::ScrollbarThumbDragReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::Y,
                            delta: dispatch_outcome::milli(dy),
                        });
                }
            }
        }

        for request in processor.scrollbar_hover_requests(event) {
            match request {
                ScrollbarHoverRequest::X {
                    element_id,
                    hovered,
                } => {
                    out.scrollbar_hover_requests
                        .push(dispatch_outcome::ScrollbarHoverReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::X,
                            hovered,
                        });
                }
                ScrollbarHoverRequest::Y {
                    element_id,
                    hovered,
                } => {
                    out.scrollbar_hover_requests
                        .push(dispatch_outcome::ScrollbarHoverReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            axis: dispatch_outcome::ScrollbarAxisOut::Y,
                            hovered,
                        });
                }
            }
        }

        for request in processor.mouse_over_requests(event) {
            match request {
                MouseOverRequest::SetMouseOverActive { element_id, active } => {
                    out.style_runtime_requests
                        .push(dispatch_outcome::StyleRuntimeReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            kind: dispatch_outcome::StyleRuntimeKind::MouseOver,
                            active,
                        });
                }
            }
        }

        for request in processor.mouse_down_requests(event) {
            match request {
                MouseDownRequest::SetMouseDownActive { element_id, active } => {
                    out.style_runtime_requests
                        .push(dispatch_outcome::StyleRuntimeReqOut {
                            target: dispatch_outcome::node_key(&element_id),
                            kind: dispatch_outcome::StyleRuntimeKind::MouseDown,
                            active,
                        });
                }
            }
        }

        if let Some((element_id, request)) = processor.text_input_command_request(event) {
            out.text_command_requests
                .push(dispatch_outcome::TextCommandReqOut {
                    target: dispatch_outcome::node_key(&element_id),
                    request,
                });
        }

        if let Some((element_id, request)) = processor.text_input_edit_request(event) {
            out.text_edit_requests.push(dispatch_outcome::TextEditReqOut {
                target: dispatch_outcome::node_key(&element_id),
                request,
            });
        }

        if let Some((element_id, request)) = processor.text_input_preedit_request(event) {
            out.text_preedit_requests
                .push(dispatch_outcome::TextPreeditReqOut {
                    target: dispatch_outcome::node_key(&element_id),
                    request,
                });
        }

        out
    }

    #[test]
    fn test_v2_preview_scrollbar_thumb_drag_parity_sequence() {
        let mut old_processor = EventProcessor::new();
        old_processor.rebuild_registry(vec![make_scrollbar_test_node(66)]);

        let mut preview_processor = old_processor.clone();

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
            let expected = v1_reference_outcome_for_event(&mut old_processor, &event);
            let predicted = preview_processor
                .preview_v2_keyboard_focus_outcome(&event, None)
                .unwrap_or_default();

            assert_eq!(
                predicted.scrollbar_thumb_drag_requests,
                expected.scrollbar_thumb_drag_requests
            );

            let _ = v1_reference_outcome_for_event(&mut preview_processor, &event);
        }
    }

    #[test]
    fn test_v2_preview_scrollbar_hover_request_parity() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_scrollbar_test_node(64)]);

        let event = InputEvent::CursorPos { x: 97.0, y: 25.0 };

        let mut old_processor = processor.clone();
        let old = old_processor.scrollbar_hover_requests(&event);

        let predicted = processor
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce scrollbar hover request outcome");

        let expected: Vec<dispatch_outcome::ScrollbarHoverReqOut> = old
            .into_iter()
            .map(|request| match request {
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
            })
            .collect();

        assert_eq!(predicted.scrollbar_hover_requests, expected);
    }

    #[test]
    fn test_v2_preview_style_runtime_mouse_over_request_parity() {
        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_mouse_over_node(65, 0.0, 0.0, 100.0, 100.0)]);

        let event = InputEvent::CursorPos { x: 10.0, y: 10.0 };

        let mut old_processor = processor.clone();
        let old = old_processor.mouse_over_requests(&event);

        let predicted = processor
            .preview_v2_keyboard_focus_outcome(&event, processor.focused_id.as_ref())
            .expect("v2 preview should produce style runtime request outcome");

        let expected: Vec<dispatch_outcome::StyleRuntimeReqOut> = old
            .into_iter()
            .map(|request| match request {
                MouseOverRequest::SetMouseOverActive { element_id, active } => {
                    dispatch_outcome::StyleRuntimeReqOut {
                        target: dispatch_outcome::node_key(&element_id),
                        kind: dispatch_outcome::StyleRuntimeKind::MouseOver,
                        active,
                    }
                }
            })
            .collect();

        assert_eq!(predicted.style_runtime_requests, expected);
    }

    #[test]
    fn test_v2_preview_style_runtime_mouse_down_parity_sequence() {
        let mut old_processor = EventProcessor::new();
        old_processor
            .rebuild_registry(vec![make_mouse_down_style_node(67, 0.0, 0.0, 100.0, 100.0)]);

        let mut preview_processor = old_processor.clone();

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
            let expected = v1_reference_outcome_for_event(&mut old_processor, &event);
            let predicted = preview_processor
                .preview_v2_keyboard_focus_outcome(&event, None)
                .unwrap_or_default();

            assert_eq!(
                predicted.style_runtime_requests,
                expected.style_runtime_requests
            );

            let _ = v1_reference_outcome_for_event(&mut preview_processor, &event);
        }
    }

    #[test]
    fn test_v2_preview_pointer_element_events_parity_sequence() {
        let mut old_processor = EventProcessor::new();
        old_processor.rebuild_registry(vec![make_pointer_events_node(68, 0.0, 0.0, 100.0, 100.0)]);

        let mut preview_processor = old_processor.clone();

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
            let expected = v1_reference_outcome_for_event(&mut old_processor, &event);
            let predicted = preview_processor
                .preview_v2_keyboard_focus_outcome(&event, None)
                .unwrap_or_default();

            assert_eq!(predicted.element_events, expected.element_events);

            let _ = v1_reference_outcome_for_event(&mut preview_processor, &event);
        }
    }

    #[test]
    fn test_v2_preview_mouse_button_payload_is_none() {
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
            .preview_v2_keyboard_focus_outcome(&event, None)
            .expect("v2 preview should produce mouse down outcome");

        let mouse_down_event = predicted
            .element_events
            .iter()
            .find(|event| event.kind == dispatch_outcome::ElementEventKind::MouseDown)
            .expect("mouse down event should be predicted");

        assert_eq!(mouse_down_event.payload, None);
    }

    #[test]
    fn test_v2_preview_key_scroll_requests_are_deduped() {
        let focused_id = ElementId::from_term_bytes(vec![73]);
        let scroll_id = ElementId::from_term_bytes(vec![74]);

        let mut focused = make_pressable_node(73, 0.0, 0.0, 100.0, 30.0);
        focused.key_scroll_targets.down = Some(scroll_id.clone());

        let mut old_processor = EventProcessor::new();
        old_processor.rebuild_registry(vec![
            focused,
            make_scroll_target_node(74, true, EVENT_SCROLL_Y_POS),
        ]);
        old_processor.focused_id = Some(focused_id.clone());

        let preview_processor = old_processor.clone();

        let event = InputEvent::Key {
            key: "down".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let expected = v1_reference_outcome_for_event(&mut old_processor, &event);
        let predicted = preview_processor
            .preview_v2_keyboard_focus_outcome(&event, preview_processor.focused_id.as_ref())
            .expect("v2 preview should produce key scroll outcome");

        assert_eq!(predicted.scroll_requests, expected.scroll_requests);
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
    fn test_v2_preview_tab_focus_reveal_scroll_is_deduped() {
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

        let mut old_processor = EventProcessor::new();
        old_processor.rebuild_registry(vec![
            first,
            second,
            make_scroll_target_node(77, true, EVENT_SCROLL_Y_POS),
        ]);
        old_processor.focused_id = Some(first_id.clone());

        let preview_processor = old_processor.clone();

        let event = InputEvent::Key {
            key: "tab".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        let expected = v1_reference_outcome_for_event(&mut old_processor, &event);
        let predicted = preview_processor
            .preview_v2_keyboard_focus_outcome(&event, preview_processor.focused_id.as_ref())
            .expect("v2 preview should produce tab focus-reveal outcome");

        assert_eq!(
            predicted.focus_change,
            Some(Some(dispatch_outcome::node_key(&second_id)))
        );
        assert_eq!(predicted.scroll_requests, expected.scroll_requests);
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

        let registry = build_event_registry(&tree);
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

        let registry = build_event_registry(&tree);
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
        assert!(processor.detect_click(&middle_press).is_none());

        let left_press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 4.0,
            y: 4.0,
        };
        assert!(processor.detect_click(&left_press).is_none());

        let left_release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_RELEASE,
            mods: 0,
            x: 4.0,
            y: 4.0,
        };
        assert_eq!(processor.detect_click(&left_release), Some(id));
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

        let registry = build_event_registry(&tree);
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

        let registry = build_event_registry(&tree);
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

        let registry = build_event_registry(&tree);
        assert_eq!(registry.len(), 1);
        assert!(registry[0].flags & EVENT_SCROLL_Y_NEG == 0);
        assert!(registry[0].flags & EVENT_SCROLL_Y_POS != 0);
        assert!(registry[0].scrollbar_y.is_some());
    }

    #[test]
    fn test_drag_deadzone_suppresses_click() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![EventNode {
            id: ElementId::from_term_bytes(vec![1]),
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            visible: true,
            flags: EVENT_CLICK,
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
        }];

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

        assert_eq!(processor.detect_click(&press), None);
        assert!(processor.scroll_requests(&move_event).is_empty());
        assert_eq!(processor.detect_click(&release), None);
    }

    #[test]
    fn test_scrollbar_thumb_drag_emits_requests_and_suppresses_click() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_scrollbar_test_node(1)];

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

        assert_eq!(processor.detect_click(&press), None);
        assert!(processor.detect_mouse_button_event(&press).is_none());
        assert!(processor.scrollbar_thumb_drag_requests(&press).is_empty());

        let requests = processor.scrollbar_thumb_drag_requests(&drag);
        assert_eq!(requests.len(), 1);
        match &requests[0] {
            ScrollbarThumbDragRequest::Y { element_id, dy } => {
                assert_eq!(*element_id, ElementId::from_term_bytes(vec![1]));
                assert!((*dy + 60.0).abs() < 0.01);
            }
            _ => panic!("expected vertical thumb drag request"),
        }

        assert_eq!(processor.detect_click(&release), None);
        assert!(processor.detect_mouse_button_event(&release).is_none());
        assert!(processor.scrollbar_thumb_drag_requests(&release).is_empty());
    }

    #[test]
    fn test_scrollbar_track_press_snaps_to_cursor_then_drags() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_scrollbar_test_node(1)];

        let press_track = InputEvent::CursorButton {
            button: "left".to_string(),
            action: crate::input::ACTION_PRESS,
            mods: 0,
            x: 95.0,
            y: 80.0,
        };
        let drag_after_press = InputEvent::CursorPos { x: 95.0, y: 70.0 };

        assert_eq!(processor.detect_click(&press_track), None);
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

        let registry = build_event_registry(&tree);
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

        let registry = build_event_registry(&tree);
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
        processor.registry = vec![make_pressable_node(4, 0.0, 0.0, 100.0, 40.0)];

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

        assert_eq!(processor.detect_click(&press), None);
        assert_eq!(processor.detect_press(&press), None);

        assert_eq!(processor.detect_click(&release), None);
        assert_eq!(
            processor.detect_press(&release),
            Some(ElementId::from_term_bytes(vec![4]))
        );
    }

    #[test]
    fn test_press_requests_activate_on_enter_for_focused_pressable_nodes() {
        let mut processor = EventProcessor::new();
        processor.registry = vec![make_pressable_node(5, 0.0, 0.0, 100.0, 40.0)];

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

        assert_eq!(
            processor.detect_press(&enter),
            Some(ElementId::from_term_bytes(vec![5]))
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

        let registry = build_event_registry(&tree);
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
