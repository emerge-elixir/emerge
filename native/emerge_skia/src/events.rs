use rustler::{Atom, Encoder, LocalPid, OwnedBinary, OwnedEnv};

use crate::input::{
    ACTION_PRESS, EVENT_CLICK, EVENT_FOCUSABLE, EVENT_MOUSE_DOWN, EVENT_MOUSE_DOWN_STYLE,
    EVENT_MOUSE_ENTER, EVENT_MOUSE_LEAVE, EVENT_MOUSE_MOVE, EVENT_MOUSE_OVER_STYLE, EVENT_MOUSE_UP,
    EVENT_PRESS, EVENT_SCROLL_X_NEG, EVENT_SCROLL_X_POS, EVENT_SCROLL_Y_NEG, EVENT_SCROLL_Y_POS,
    EVENT_TEXT_INPUT, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
};
use crate::tree::attrs::{BorderWidth, Font, Padding, TextAlign};
use crate::tree::element::{ElementId, ElementKind, ElementTree};
use crate::tree::scrollbar::{self as tree_scrollbar, ScrollbarAxis};

mod runtime;
mod scrollbar;
pub(crate) use runtime::spawn_event_actor;
use scrollbar::{
    ScrollbarDragState, ScrollbarHitArea, ScrollbarInteraction, ScrollbarThumbHover, axis_coord,
    hit_test_scrollbar, scroll_from_pointer, scrollbar_node_from_metrics, thumb_hover_from_hit,
};
pub use scrollbar::{ScrollbarHoverRequest, ScrollbarNode, ScrollbarThumbDragRequest};

const DRAG_DEADZONE: f32 = 10.0;

pub struct EventProcessor {
    registry: Vec<EventNode>,
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
    pub flags: u16,
    pub self_rect: Rect,
    pub self_radii: Option<CornerRadii>,
    pub clip_rect: Option<Rect>,
    pub clip_radii: Option<CornerRadii>,
    pub scrollbar_x: Option<ScrollbarNode>,
    pub scrollbar_y: Option<ScrollbarNode>,
    pub text_input: Option<TextInputDescriptor>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextInputDescriptor {
    pub content: String,
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
    collect_event_nodes(tree, root, &mut registry, 0.0, 0.0, None);
    registry
}

fn collect_event_nodes(
    tree: &ElementTree,
    id: &ElementId,
    registry: &mut Vec<EventNode>,
    offset_x: f32,
    offset_y: f32,
    clip_rect: Option<ClipContext>,
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

        if flags != 0 && visible_rect.width > 0.0 && visible_rect.height > 0.0 {
            registry.push(EventNode {
                id: element.id.clone(),
                hit_rect: visible_rect,
                flags,
                self_rect: adjusted_rect,
                self_radii,
                clip_rect: active_clip_rect,
                clip_radii,
                scrollbar_x,
                scrollbar_y,
                text_input,
            });
        }

        let clip_enabled = element.attrs.clip_x.unwrap_or(false)
            || element.attrs.clip_y.unwrap_or(false)
            || element.attrs.scrollbar_x.unwrap_or(false)
            || element.attrs.scrollbar_y.unwrap_or(false);

        if clip_enabled {
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

    let scroll_x = if element.attrs.scrollbar_x.unwrap_or(false) {
        element.attrs.scroll_x.unwrap_or(0.0) as f32
    } else {
        0.0
    };
    let scroll_y = if element.attrs.scrollbar_y.unwrap_or(false) {
        element.attrs.scroll_y.unwrap_or(0.0) as f32
    } else {
        0.0
    };

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
        );
    }
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
    let (inset_left, inset_right) = text_content_insets(&element.attrs);
    let (font_family, font_weight, font_italic) = font_info_from_attrs(&element.attrs);

    TextInputDescriptor {
        content,
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
    ) -> Option<(ElementId, Atom)> {
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

        let (flag, event_atom) = match *action {
            crate::input::ACTION_PRESS => (EVENT_MOUSE_DOWN, mouse_down()),
            crate::input::ACTION_RELEASE => (EVENT_MOUSE_UP, mouse_up()),
            _ => return None,
        };

        let hit = hit_test_with_flag(&self.registry, *x, *y, flag)?;
        Some((hit, event_atom))
    }

    pub(crate) fn handle_hover_event(&mut self, event: &InputEvent) -> Vec<(ElementId, Atom)> {
        let mut emitted = Vec::new();

        match event {
            InputEvent::CursorPos { x, y } => {
                let hover_mask = EVENT_MOUSE_ENTER | EVENT_MOUSE_LEAVE | EVENT_MOUSE_MOVE;
                let hit = hit_test_with_flag(&self.registry, *x, *y, hover_mask);

                if hit != self.hovered_id {
                    if let Some(previous) = self.hovered_id.take()
                        && self.node_has_flag(&previous, EVENT_MOUSE_LEAVE)
                    {
                        emitted.push((previous, mouse_leave()));
                    }

                    if let Some(new_id) = hit.clone()
                        && self.node_has_flag(&new_id, EVENT_MOUSE_ENTER)
                    {
                        emitted.push((new_id.clone(), mouse_enter()));
                    }

                    self.hovered_id = hit;
                }

                if let Some(current) = self.hovered_id.as_ref()
                    && self.node_has_flag(current, EVENT_MOUSE_MOVE)
                {
                    emitted.push((current.clone(), mouse_move()));
                }
            }
            InputEvent::CursorEntered { entered } => {
                if !*entered
                    && let Some(previous) = self.hovered_id.take()
                    && self.node_has_flag(&previous, EVENT_MOUSE_LEAVE)
                {
                    emitted.push((previous, mouse_leave()));
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
                    self.cycle_visible_focus(reverse).map(Some)
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

        match event {
            InputEvent::Key { key, action, mods } if *action == ACTION_PRESS => {
                let extend_selection = *mods & MOD_SHIFT != 0;
                let request = match key.as_str() {
                    "left" => Some(TextInputEditRequest::MoveLeft { extend_selection }),
                    "right" => Some(TextInputEditRequest::MoveRight { extend_selection }),
                    "home" => Some(TextInputEditRequest::MoveHome { extend_selection }),
                    "end" => Some(TextInputEditRequest::MoveEnd { extend_selection }),
                    "backspace" => Some(TextInputEditRequest::Backspace),
                    "delete" => Some(TextInputEditRequest::Delete),
                    _ => None,
                }?;

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

    fn cycle_visible_focus(&self, reverse: bool) -> Option<ElementId> {
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
        requests
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

#[cfg(test)]
mod tests {
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
            text_input: Some(TextInputDescriptor {
                content: String::new(),
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
            text_input: None,
        }
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
        processor.registry = vec![make_text_input_node(5, 0.0, 0.0, 100.0, 20.0)];

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
