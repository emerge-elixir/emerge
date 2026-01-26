use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rustler::{Atom, Encoder, LocalPid, OwnedBinary, OwnedEnv};

use crate::input::{
    InputEvent, EVENT_CLICK, EVENT_MOUSE_DOWN, EVENT_MOUSE_ENTER, EVENT_MOUSE_LEAVE, EVENT_MOUSE_MOVE,
    EVENT_MOUSE_UP, EVENT_SCROLL_X_NEG, EVENT_SCROLL_X_POS, EVENT_SCROLL_Y_NEG, EVENT_SCROLL_Y_POS,
};
use crate::renderer::RenderState;
use crate::tree::element::{ElementId, ElementTree};
use crate::tree::layout::refresh;

const EVENT_POLL_SLEEP: Duration = Duration::from_millis(2);
const DRAG_DEADZONE: f32 = 10.0;

pub struct EventProcessor {
    queue: VecDeque<InputEvent>,
    registry: Vec<EventNode>,
    pressed_id: Option<ElementId>,
    hovered_id: Option<ElementId>,
    drag_start: Option<(f32, f32)>,
    drag_last_pos: Option<(f32, f32)>,
    drag_active: bool,
    drag_consumed: bool,
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

        if flags != 0 && visible_rect.width > 0.0 && visible_rect.height > 0.0 {
            registry.push(EventNode {
                id: element.id.clone(),
                hit_rect: visible_rect,
                flags,
                self_rect: adjusted_rect,
                self_radii,
                clip_rect: active_clip_rect,
                clip_radii,
            });
        }

        let clip_enabled = element.attrs.clip.unwrap_or(false)
            || element.attrs.clip_x.unwrap_or(false)
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
                Some(active_clip) => content_rect
                    .intersect(active_clip.rect)
                    .unwrap_or(Rect {
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
        collect_event_nodes(tree, child_id, registry, child_offset_x, child_offset_y, next_clip);
    }
}

pub fn hit_test_with_flag(registry: &[EventNode], x: f32, y: f32, flag: u16) -> Option<ElementId> {
    for node in registry.iter().rev() {
        if node.flags & flag == 0 {
            continue;
        }
        if !node.hit_rect.contains(x, y) {
            continue;
        }
        if let (Some(rect), Some(radii)) = (node.clip_rect, node.clip_radii) {
            if !point_in_rounded_rect(rect, radii, x, y) {
                continue;
            }
        }
        if let Some(radii) = node.self_radii {
            if !point_in_rounded_rect(node.self_rect, radii, x, y) {
                continue;
            }
        }
        return Some(node.id.clone());
    }
    None
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
        return check_corner(rect.x + rect.width - radii.tr, rect.y + radii.tr, radii.tr, x, y);
    }
    if radii.br > 0.0
        && x > rect.x + rect.width - radii.br
        && y > rect.y + rect.height - radii.br
    {
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
            queue: VecDeque::new(),
            registry: Vec::new(),
            pressed_id: None,
            hovered_id: None,
            drag_start: None,
            drag_last_pos: None,
            drag_active: false,
            drag_consumed: false,
        }
    }

    pub fn enqueue(&mut self, event: InputEvent) {
        self.queue.push_back(event);
    }

    pub fn rebuild_registry(&mut self, registry: Vec<EventNode>) {
        self.registry = registry;
    }

    pub fn start_loop(
        processor: Arc<Mutex<EventProcessor>>,
        tree: Arc<Mutex<ElementTree>>,
        render_state: Arc<Mutex<RenderState>>,
        target: Arc<Mutex<Option<LocalPid>>>,
        redraw: Arc<dyn Fn() + Send + Sync>,
    ) {
        thread::spawn(move || loop {
            let mut drained = Vec::new();
            if let Ok(mut guard) = processor.lock() {
                while let Some(event) = guard.queue.pop_front() {
                    drained.push(event);
                }
            }

            if drained.is_empty() {
                thread::sleep(EVENT_POLL_SLEEP);
                continue;
            }

            let Some(mut tree_guard) = tree.lock().ok() else {
                thread::sleep(EVENT_POLL_SLEEP);
                continue;
            };

            let mut needs_redraw = false;
            for event in drained {
                if let Ok(mut guard) = processor.lock() {
                    let pid = target.lock().ok().and_then(|t| *t);
                    if let Some(pid) = pid {
                        send_input_event(pid, &event);

                        if let Some(clicked_id) = guard.detect_click(&event) {
                            send_element_event(pid, &clicked_id, click());
                        }

                        if let Some((mouse_id, mouse_event)) = guard.detect_mouse_button_event(&event)
                        {
                            send_element_event(pid, &mouse_id, mouse_event);
                        }

                        for (hover_id, hover_event) in guard.handle_hover_event(&event) {
                            send_element_event(pid, &hover_id, hover_event);
                        }
                    }

                    if let Some(changed) = guard.handle_drag_scroll(&event, &mut tree_guard) {
                        needs_redraw |= changed;
                    }

                    if let Some(changed) = guard.handle_scroll(&event, &mut tree_guard) {
                        needs_redraw |= changed;
                    }
                }
            }

            if needs_redraw {
                // Refresh produces both render commands AND event registry
                let output = refresh(&tree_guard);
                if let Ok(mut state) = render_state.lock() {
                    state.commands = output.commands;
                }
                // Rebuild event registry so clicks match new scroll positions
                if let Ok(mut guard) = processor.lock() {
                    guard.rebuild_registry(output.event_registry);
                }
                redraw();
            }
        });
    }

    fn detect_click(&mut self, event: &InputEvent) -> Option<ElementId> {
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

        let hit = hit_test_with_flag(&self.registry, *x, *y, EVENT_CLICK);
        if *action == crate::input::ACTION_PRESS {
            self.pressed_id = hit;
            self.drag_start = Some((*x, *y));
            self.drag_last_pos = Some((*x, *y));
            self.drag_active = false;
            self.drag_consumed = false;
            return None;
        }

        if *action == crate::input::ACTION_RELEASE {
            let pressed = self.pressed_id.take();
            self.drag_start = None;
            self.drag_last_pos = None;
            let was_dragged = self.drag_consumed;
            self.drag_active = false;
            self.drag_consumed = false;
            if was_dragged {
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

    fn detect_mouse_button_event(&self, event: &InputEvent) -> Option<(ElementId, Atom)> {
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

        let (flag, event_atom) = match *action {
            crate::input::ACTION_PRESS => (EVENT_MOUSE_DOWN, mouse_down()),
            crate::input::ACTION_RELEASE => (EVENT_MOUSE_UP, mouse_up()),
            _ => return None,
        };

        let hit = hit_test_with_flag(&self.registry, *x, *y, flag)?;
        Some((hit, event_atom))
    }

    fn handle_hover_event(&mut self, event: &InputEvent) -> Vec<(ElementId, Atom)> {
        let mut emitted = Vec::new();

        match event {
            InputEvent::CursorPos { x, y } => {
                let hover_mask = EVENT_MOUSE_ENTER | EVENT_MOUSE_LEAVE | EVENT_MOUSE_MOVE;
                let hit = hit_test_with_flag(&self.registry, *x, *y, hover_mask);

                if hit != self.hovered_id {
                    if let Some(previous) = self.hovered_id.take() {
                        if self.node_has_flag(&previous, EVENT_MOUSE_LEAVE) {
                            emitted.push((previous, mouse_leave()));
                        }
                    }

                    if let Some(new_id) = hit.clone() {
                        if self.node_has_flag(&new_id, EVENT_MOUSE_ENTER) {
                            emitted.push((new_id.clone(), mouse_enter()));
                        }
                    }

                    self.hovered_id = hit;
                }

                if let Some(current) = self.hovered_id.as_ref() {
                    if self.node_has_flag(current, EVENT_MOUSE_MOVE) {
                        emitted.push((current.clone(), mouse_move()));
                    }
                }
            }
            InputEvent::CursorEntered { entered } => {
                if !*entered {
                    if let Some(previous) = self.hovered_id.take() {
                        if self.node_has_flag(&previous, EVENT_MOUSE_LEAVE) {
                            emitted.push((previous, mouse_leave()));
                        }
                    }
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

    fn handle_drag_scroll(
        &mut self,
        event: &InputEvent,
        tree: &mut ElementTree,
    ) -> Option<bool> {
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
                return Some(false);
            }
            self.drag_active = true;
            self.drag_consumed = true;
        }

        self.drag_last_pos = Some((*x, *y));

        if dx == 0.0 && dy == 0.0 {
            return Some(false);
        }

        let mut changed = false;

        if dx != 0.0 {
            let flag = if dx > 0.0 {
                EVENT_SCROLL_X_NEG
            } else {
                EVENT_SCROLL_X_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                changed |= tree.apply_scroll(&id, dx, 0.0);
            }
        }

        if dy != 0.0 {
            let flag = if dy > 0.0 {
                EVENT_SCROLL_Y_NEG
            } else {
                EVENT_SCROLL_Y_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                changed |= tree.apply_scroll(&id, 0.0, dy);
            }
        }

        Some(changed)
    }

    fn handle_scroll(&mut self, event: &InputEvent, tree: &mut ElementTree) -> Option<bool> {
        let InputEvent::CursorScroll { dx, dy, x, y } = event else {
            return None;
        };

        let mut changed = false;

        if *dx != 0.0 {
            let flag = if *dx > 0.0 {
                EVENT_SCROLL_X_NEG
            } else {
                EVENT_SCROLL_X_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                changed |= tree.apply_scroll(&id, *dx, 0.0);
            }
        }

        if *dy != 0.0 {
            let flag = if *dy > 0.0 {
                EVENT_SCROLL_Y_NEG
            } else {
                EVENT_SCROLL_Y_POS
            };
            if let Some(id) = hit_test_with_flag(&self.registry, *x, *y, flag) {
                changed |= tree.apply_scroll(&id, 0.0, *dy);
            }
        }

        Some(changed)
    }
}

fn send_element_event(pid: LocalPid, element_id: &ElementId, event: Atom) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        let mut bin = OwnedBinary::new(element_id.0.len()).unwrap();
        bin.as_mut_slice().copy_from_slice(&element_id.0);
        let id_bin = bin.release(inner_env);
        (emerge_skia_event(), (id_bin, event)).encode(inner_env)
    });
}

fn send_input_event(pid: LocalPid, event: &InputEvent) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{Element, ElementKind, ElementTree};

    fn make_element(id: u8, attrs: Attrs, frame: crate::tree::element::Frame, children: Vec<ElementId>) -> Element {
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
    }

    #[test]
    fn test_drag_deadzone_suppresses_click() {
        let mut processor = EventProcessor {
            queue: VecDeque::new(),
            registry: vec![EventNode {
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
            }],
            pressed_id: None,
            hovered_id: None,
            drag_start: None,
            drag_last_pos: None,
            drag_active: false,
            drag_consumed: false,
        };

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
        assert_eq!(processor.handle_drag_scroll(&move_event, &mut ElementTree::new()), Some(false));
        assert_eq!(processor.detect_click(&release), None);
    }
}
