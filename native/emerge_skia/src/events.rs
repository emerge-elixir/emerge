use rustler::{Atom, Encoder, LocalPid, OwnedBinary, OwnedEnv};

use crate::input::{
    EVENT_CLICK, EVENT_MOUSE_DOWN, EVENT_MOUSE_ENTER, EVENT_MOUSE_LEAVE, EVENT_MOUSE_MOVE,
    EVENT_MOUSE_UP, EVENT_SCROLL_X_NEG, EVENT_SCROLL_X_POS, EVENT_SCROLL_Y_NEG, EVENT_SCROLL_Y_POS,
    InputEvent,
};
use crate::tree::element::{ElementId, ElementTree};
use crate::tree::scrollbar::{self, ScrollbarAxis, ScrollbarMetrics};
const DRAG_DEADZONE: f32 = 10.0;

pub struct EventProcessor {
    registry: Vec<EventNode>,
    pressed_id: Option<ElementId>,
    hovered_id: Option<ElementId>,
    hovered_scrollbar_thumb: Option<ScrollbarThumbHover>,
    drag_start: Option<(f32, f32)>,
    drag_last_pos: Option<(f32, f32)>,
    drag_active: bool,
    drag_consumed: bool,
    scrollbar_drag: Option<ScrollbarDragState>,
    button_captured_by_scrollbar: bool,
    suppress_mouse_button_event: bool,
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
}

#[derive(Clone, Copy, Debug)]
pub struct ScrollbarNode {
    pub track_rect: Rect,
    pub thumb_rect: Rect,
    pub track_start: f32,
    pub track_len: f32,
    pub thumb_start: f32,
    pub thumb_len: f32,
    pub scroll_offset: f32,
    pub scroll_range: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollbarHitArea {
    Thumb,
    Track,
}

#[derive(Clone, Debug)]
struct ScrollbarHit {
    id: ElementId,
    axis: ScrollbarAxis,
    area: ScrollbarHitArea,
    node: ScrollbarNode,
}

#[derive(Clone, Debug)]
struct ScrollbarDragState {
    id: ElementId,
    axis: ScrollbarAxis,
    track_start: f32,
    track_len: f32,
    thumb_len: f32,
    pointer_offset: f32,
    scroll_range: f32,
    current_scroll: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScrollbarThumbHover {
    id: ElementId,
    axis: ScrollbarAxis,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollbarThumbDragRequest {
    X { element_id: ElementId, dx: f32 },
    Y { element_id: ElementId, dy: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScrollbarHoverRequest {
    X { element_id: ElementId, hovered: bool },
    Y { element_id: ElementId, hovered: bool },
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
        let scrollbar_x = scrollbar::horizontal_metrics(frame, &element.attrs)
            .map(|metrics| scrollbar_node_from_metrics(metrics, offset_x, offset_y));
        let scrollbar_y = scrollbar::vertical_metrics(frame, &element.attrs)
            .map(|metrics| scrollbar_node_from_metrics(metrics, offset_x, offset_y));

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

fn scrollbar_node_from_metrics(
    metrics: ScrollbarMetrics,
    offset_x: f32,
    offset_y: f32,
) -> ScrollbarNode {
    ScrollbarNode {
        track_rect: Rect {
            x: metrics.track_x - offset_x,
            y: metrics.track_y - offset_y,
            width: metrics.track_width,
            height: metrics.track_height,
        },
        thumb_rect: Rect {
            x: metrics.thumb_x - offset_x,
            y: metrics.thumb_y - offset_y,
            width: metrics.thumb_width,
            height: metrics.thumb_height,
        },
        track_start: metrics.track_start
            - match metrics.axis {
                ScrollbarAxis::X => offset_x,
                ScrollbarAxis::Y => offset_y,
            },
        track_len: metrics.track_len,
        thumb_start: metrics.thumb_start
            - match metrics.axis {
                ScrollbarAxis::X => offset_x,
                ScrollbarAxis::Y => offset_y,
            },
        thumb_len: metrics.thumb_len,
        scroll_offset: metrics.scroll_offset,
        scroll_range: metrics.scroll_range,
    }
}

fn hit_test_scrollbar(registry: &[EventNode], x: f32, y: f32) -> Option<ScrollbarHit> {
    for node in registry.iter().rev() {
        if !point_hits_node(node, x, y) {
            continue;
        }

        if let Some(scrollbar) = node.scrollbar_y
            && scrollbar.thumb_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::Y,
                area: ScrollbarHitArea::Thumb,
                node: scrollbar,
            });
        }

        if let Some(scrollbar) = node.scrollbar_x
            && scrollbar.thumb_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::X,
                area: ScrollbarHitArea::Thumb,
                node: scrollbar,
            });
        }

        if let Some(scrollbar) = node.scrollbar_y
            && scrollbar.track_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::Y,
                area: ScrollbarHitArea::Track,
                node: scrollbar,
            });
        }

        if let Some(scrollbar) = node.scrollbar_x
            && scrollbar.track_rect.contains(x, y)
        {
            return Some(ScrollbarHit {
                id: node.id.clone(),
                axis: ScrollbarAxis::X,
                area: ScrollbarHitArea::Track,
                node: scrollbar,
            });
        }
    }

    None
}

fn hit_test_scrollbar_thumb(registry: &[EventNode], x: f32, y: f32) -> Option<ScrollbarThumbHover> {
    for node in registry.iter().rev() {
        if !point_hits_node(node, x, y) {
            continue;
        }

        if let Some(scrollbar) = node.scrollbar_y
            && scrollbar.thumb_rect.contains(x, y)
        {
            return Some(ScrollbarThumbHover {
                id: node.id.clone(),
                axis: ScrollbarAxis::Y,
            });
        }

        if let Some(scrollbar) = node.scrollbar_x
            && scrollbar.thumb_rect.contains(x, y)
        {
            return Some(ScrollbarThumbHover {
                id: node.id.clone(),
                axis: ScrollbarAxis::X,
            });
        }
    }

    None
}

fn axis_coord(axis: ScrollbarAxis, x: f32, y: f32) -> f32 {
    match axis {
        ScrollbarAxis::X => x,
        ScrollbarAxis::Y => y,
    }
}

fn scroll_from_pointer(
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

impl EventProcessor {
    pub fn new() -> Self {
        Self {
            registry: Vec::new(),
            pressed_id: None,
            hovered_id: None,
            hovered_scrollbar_thumb: None,
            drag_start: None,
            drag_last_pos: None,
            drag_active: false,
            drag_consumed: false,
            scrollbar_drag: None,
            button_captured_by_scrollbar: false,
            suppress_mouse_button_event: false,
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

        let Some((drag_id, drag_axis)) = self
            .scrollbar_drag
            .as_ref()
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
                if let Some(drag) = self.scrollbar_drag.as_mut() {
                    drag.track_start = scrollbar.track_start;
                    drag.track_len = scrollbar.track_len;
                    drag.thumb_len = scrollbar.thumb_len;
                    drag.scroll_range = scrollbar.scroll_range;
                    drag.current_scroll = scrollbar.scroll_offset;
                    drag.pointer_offset = drag.pointer_offset.clamp(0.0, drag.thumb_len);
                }
            }
            None => {
                self.scrollbar_drag = None;
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
                self.button_captured_by_scrollbar = true;
                self.suppress_mouse_button_event = true;
                self.pressed_id = None;
                self.drag_start = None;
                self.drag_last_pos = None;
                self.drag_active = false;
                self.drag_consumed = true;
                return None;
            }

            self.button_captured_by_scrollbar = false;
            self.suppress_mouse_button_event = false;
            let hit = hit_test_with_flag(&self.registry, *x, *y, EVENT_CLICK);
            self.pressed_id = hit;
            self.drag_start = Some((*x, *y));
            self.drag_last_pos = Some((*x, *y));
            self.drag_active = false;
            self.drag_consumed = false;
            return None;
        }

        if *action == crate::input::ACTION_RELEASE {
            let consumed_by_scrollbar = self.button_captured_by_scrollbar;
            self.button_captured_by_scrollbar = false;
            self.suppress_mouse_button_event = consumed_by_scrollbar;

            let hit = hit_test_with_flag(&self.registry, *x, *y, EVENT_CLICK);
            let pressed = self.pressed_id.take();
            self.drag_start = None;
            self.drag_last_pos = None;
            let was_dragged = self.drag_consumed;
            self.drag_active = false;
            self.drag_consumed = false;
            if consumed_by_scrollbar || was_dragged {
                return None;
            }
            if let (Some(pressed_id), Some(hit_id)) = (pressed, hit)
                && pressed_id == hit_id
            {
                return Some(pressed_id);
            }
        }

        None
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

        if self.suppress_mouse_button_event {
            self.suppress_mouse_button_event = false;
            return None;
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

    fn node_has_flag(&self, id: &ElementId, flag: u16) -> bool {
        self.registry
            .iter()
            .find(|node| node.id == *id)
            .is_some_and(|node| node.flags & flag != 0)
    }

    pub fn scrollbar_thumb_drag_requests(&mut self, event: &InputEvent) -> Vec<ScrollbarThumbDragRequest> {
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

    fn next_scrollbar_thumb_hover(&self, event: &InputEvent) -> Option<Option<ScrollbarThumbHover>> {
        match event {
            InputEvent::CursorPos { x, y } => Some(self.current_scrollbar_thumb_hover(*x, *y)),
            InputEvent::CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == "left"
                && (*action == crate::input::ACTION_PRESS || *action == crate::input::ACTION_RELEASE) =>
            {
                Some(self.current_scrollbar_thumb_hover(*x, *y))
            }
            InputEvent::CursorEntered { entered } if !*entered => Some(None),
            _ => None,
        }
    }

    fn current_scrollbar_thumb_hover(&self, x: f32, y: f32) -> Option<ScrollbarThumbHover> {
        if let Some(drag) = self.scrollbar_drag.as_ref() {
            return Some(ScrollbarThumbHover {
                id: drag.id.clone(),
                axis: drag.axis,
            });
        }
        hit_test_scrollbar_thumb(&self.registry, x, y)
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
            self.scrollbar_drag = None;
            return Vec::new();
        }

        if *action != crate::input::ACTION_PRESS {
            return Vec::new();
        }

        let Some(hit) = hit_test_scrollbar(&self.registry, *x, *y) else {
            return Vec::new();
        };

        self.button_captured_by_scrollbar = true;
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

        self.scrollbar_drag = Some(ScrollbarDragState {
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

        let Some(state) = self.scrollbar_drag.as_ref() else {
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
            self.scrollbar_drag = None;
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

        if let Some(state) = self.scrollbar_drag.as_mut() {
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
        if self.scrollbar_drag.is_some() || self.button_captured_by_scrollbar {
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

pub(crate) fn send_input_event(pid: LocalPid, event: &InputEvent) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (emerge_skia_event(), event).encode(inner_env)
    });
}

rustler::atoms! {
    emerge_skia_event,
    click,
    mouse_down,
    mouse_up,
    mouse_enter,
    mouse_leave,
    mouse_move,
}

pub(crate) fn click_atom() -> Atom {
    click()
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
            flags: EVENT_SCROLL_X_NEG | EVENT_SCROLL_X_POS | EVENT_SCROLL_Y_NEG | EVENT_SCROLL_Y_POS,
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
}
