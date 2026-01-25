//! Input event handling for emerge_skia.
//!
//! This module provides:
//! - `InputEvent` enum representing mouse/keyboard events
//! - `InputHandler` for filtering and sending events to Elixir
//! - Encoder impl for sending events to Elixir
//! - Input mask constants for filtering events

use rustler::{Atom, Encoder, Env, LocalPid, OwnedBinary, OwnedEnv, Term};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::renderer::RenderState;
use crate::tree::element::{ElementId, ElementTree, Frame};
use crate::tree::render::render_tree;

// ============================================================================
// Input Event
// ============================================================================

#[derive(Clone, Debug)]
pub enum InputEvent {
    /// Mouse cursor position changed
    CursorPos { x: f32, y: f32 },

    /// Mouse button pressed/released
    CursorButton {
        button: String,
        action: u8,
        mods: u8,
        x: f32,
        y: f32,
    },

    /// Mouse scroll wheel
    CursorScroll { dx: f32, dy: f32, x: f32, y: f32 },

    /// Keyboard key pressed/released
    Key { key: String, action: u8, mods: u8 },

    /// Text input (character typed)
    #[allow(dead_code)]
    Codepoint { codepoint: char, mods: u8 },

    /// Cursor entered/exited window
    CursorEntered { entered: bool },

    /// Window resized
    Resized {
        width: u32,
        height: u32,
        scale_factor: f32,
    },

    /// Window focused/unfocused
    Focused { focused: bool },
}

// ============================================================================
// Input Mask (for filtering events)
// ============================================================================

pub const INPUT_MASK_KEY: u32 = 0x01;
pub const INPUT_MASK_CODEPOINT: u32 = 0x02;
pub const INPUT_MASK_CURSOR_POS: u32 = 0x04;
pub const INPUT_MASK_CURSOR_BUTTON: u32 = 0x08;
pub const INPUT_MASK_CURSOR_SCROLL: u32 = 0x10;
pub const INPUT_MASK_CURSOR_ENTER: u32 = 0x20;
pub const INPUT_MASK_RESIZE: u32 = 0x40;
pub const INPUT_MASK_FOCUS: u32 = 0x80;

/// All input events enabled
pub const INPUT_MASK_ALL: u32 = 0xFF;

// ============================================================================
// Modifier Keys
// ============================================================================

pub const MOD_SHIFT: u8 = 0x01;
pub const MOD_CTRL: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_META: u8 = 0x08;

// ============================================================================
// Action Constants
// ============================================================================

pub const ACTION_RELEASE: u8 = 0;
pub const ACTION_PRESS: u8 = 1;

pub const EVENT_CLICK: u8 = 0x01;
pub const EVENT_SCROLL: u8 = 0x02;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Rect {
    fn from_frame(frame: Frame) -> Self {
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
struct ClipContext {
    rect: Rect,
    radii: Option<CornerRadii>,
}

// ============================================================================
// Atoms
// ============================================================================

rustler::atoms! {
    emerge_skia_event,
    key,
    codepoint,
    cursor_pos,
    cursor_button,
    cursor_scroll,
    cursor_entered,
    resized,
    focused,
    click,
    shift,
    ctrl,
    alt,
    meta,
}

// ============================================================================
// Input Handler
// ============================================================================

/// Handles input event filtering and delivery to Elixir.
pub struct InputHandler {
    target: Option<LocalPid>,
    mask: u32,
    cursor_pos: (f32, f32),
    event_registry: Vec<EventNode>,
    pressed_id: Option<ElementId>,
    scroll_state: HashMap<ElementId, (f32, f32)>,
    tree: Option<Arc<Mutex<ElementTree>>>,
    render_state: Option<Arc<Mutex<RenderState>>>,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            target: None,
            mask: INPUT_MASK_ALL,
            cursor_pos: (0.0, 0.0),
            event_registry: Vec::new(),
            pressed_id: None,
            scroll_state: HashMap::new(),
            tree: None,
            render_state: None,
        }
    }

    /// Update cursor position (used for button/scroll events)
    pub fn set_cursor_pos(&mut self, x: f32, y: f32) {
        self.cursor_pos = (x, y);
    }

    /// Get current cursor position
    pub fn cursor_pos(&self) -> (f32, f32) {
        self.cursor_pos
    }

    /// Set the input mask for filtering events
    pub fn set_mask(&mut self, mask: u32) {
        self.mask = mask;
    }

    /// Set the target pid for input events
    pub fn set_target(&mut self, target: Option<LocalPid>) {
        self.target = target;
    }

    pub fn set_render_context(
        &mut self,
        tree: Arc<Mutex<ElementTree>>,
        render_state: Arc<Mutex<RenderState>>,
    ) {
        self.tree = Some(tree);
        self.render_state = Some(render_state);
    }

    pub fn set_event_registry(&mut self, registry: Vec<EventNode>) {
        self.event_registry = registry;
    }

    pub fn apply_scroll_state(&mut self, tree: &mut ElementTree) {
        for (id, (scroll_x, scroll_y)) in &self.scroll_state {
            if let Some(element) = tree.get_mut(id)
                && let Some(frame) = element.frame
            {
                let max_x = (frame.content_width - frame.width).max(0.0);
                let max_y = (frame.content_height - frame.height).max(0.0);
                let clamped_x = scroll_x.clamp(0.0, max_x);
                let clamped_y = scroll_y.clamp(0.0, max_y);
                element.attrs.scroll_x = Some(clamped_x as f64);
                element.attrs.scroll_y = Some(clamped_y as f64);
            }
        }
    }

    /// Send an event to the target if it passes the mask filter.
    /// Returns true if the event was sent.
    pub fn send_event(&mut self, event: InputEvent) -> bool {
        // Check if we have a target
        let Some(pid) = self.target else {
            return false;
        };

        // Check mask
        let event_mask = match &event {
            InputEvent::Key { .. } => INPUT_MASK_KEY,
            InputEvent::Codepoint { .. } => INPUT_MASK_CODEPOINT,
            InputEvent::CursorPos { .. } => INPUT_MASK_CURSOR_POS,
            InputEvent::CursorButton { .. } => INPUT_MASK_CURSOR_BUTTON,
            InputEvent::CursorScroll { .. } => INPUT_MASK_CURSOR_SCROLL,
            InputEvent::CursorEntered { .. } => INPUT_MASK_CURSOR_ENTER,
            InputEvent::Resized { .. } => INPUT_MASK_RESIZE,
            InputEvent::Focused { .. } => INPUT_MASK_FOCUS,
        };

        if self.mask & event_mask == 0 {
            return false; // Event filtered out
        }

        let mut needs_redraw = false;

        if let Some(clicked_id) = self.detect_click(&event) {
            send_element_event(pid, &clicked_id, click());
        }

        if let Some(scroll_changed) = self.handle_scroll(&event) {
            needs_redraw = scroll_changed;
        }

        // Send event to Elixir process
        send_input_event(pid, event);
        needs_redraw
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

        let hit = hit_test_with_flag(&self.event_registry, *x, *y, EVENT_CLICK);
        if *action == ACTION_PRESS {
            self.pressed_id = hit;
            return None;
        }

        if *action == ACTION_RELEASE {
            let pressed = self.pressed_id.take();
            if let (Some(pressed_id), Some(hit_id)) = (pressed, hit)
                && pressed_id == hit_id
            {
                return Some(pressed_id);
            }
        }

        None
    }

    fn handle_scroll(&mut self, event: &InputEvent) -> Option<bool> {
        let InputEvent::CursorScroll { dx, dy, x, y } = event else {
            return None;
        };

        let id = hit_test_with_flag(&self.event_registry, *x, *y, EVENT_SCROLL)?;
        let Some(tree) = self.tree.as_ref() else {
            return Some(false);
        };

        let mut tree_guard = tree.lock().ok()?;
        let element = tree_guard.get_mut(&id)?;
        let frame = element.frame?;

        let max_x = (frame.content_width - frame.width).max(0.0);
        let max_y = (frame.content_height - frame.height).max(0.0);

        let current_x = element.attrs.scroll_x.unwrap_or(0.0) as f32;
        let current_y = element.attrs.scroll_y.unwrap_or(0.0) as f32;
        let next_x = (current_x - dx).clamp(0.0, max_x);
        let next_y = (current_y - dy).clamp(0.0, max_y);

        if (next_x - current_x).abs() < f32::EPSILON && (next_y - current_y).abs() < f32::EPSILON {
            return Some(false);
        }

        element.attrs.scroll_x = Some(next_x as f64);
        element.attrs.scroll_y = Some(next_y as f64);
        self.scroll_state.insert(id, (next_x, next_y));

        let commands = render_tree(&tree_guard);
        if let Some(render_state) = self.render_state.as_ref()
            && let Ok(mut state) = render_state.lock()
        {
            state.commands = commands;
        }

        Some(true)
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Send Event to Elixir
// ============================================================================

/// Send an input event to the target pid as {:emerge_skia_event, event}
fn send_input_event(pid: LocalPid, event: InputEvent) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (emerge_skia_event(), event).encode(inner_env)
    });
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

#[derive(Clone, Debug)]
pub struct EventNode {
    pub id: ElementId,
    pub hit_rect: Rect,
    pub flags: u8,
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

    let mut flags = 0u8;
    if element.attrs.on_click.unwrap_or(false) {
        flags |= EVENT_CLICK;
    }
    if element.attrs.scrollbar_x.unwrap_or(false) || element.attrs.scrollbar_y.unwrap_or(false) {
        flags |= EVENT_SCROLL;
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
                Some(crate::tree::attrs::Padding::Uniform(v)) => (*v as f32, *v as f32, *v as f32, *v as f32),
                Some(crate::tree::attrs::Padding::Sides { left, top, right, bottom }) => {
                    (*left as f32, *top as f32, *right as f32, *bottom as f32)
                }
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

fn hit_test_with_flag(registry: &[EventNode], x: f32, y: f32, flag: u8) -> Option<ElementId> {
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

// ============================================================================
// Encoder Implementation
// ============================================================================

impl InputEvent {
    fn mods_to_terms<'a>(env: Env<'a>, mods: u8) -> Vec<Term<'a>> {
        let mut terms = Vec::new();
        if mods & MOD_SHIFT != 0 {
            terms.push(shift().encode(env));
        }
        if mods & MOD_CTRL != 0 {
            terms.push(ctrl().encode(env));
        }
        if mods & MOD_ALT != 0 {
            terms.push(alt().encode(env));
        }
        if mods & MOD_META != 0 {
            terms.push(meta().encode(env));
        }
        terms
    }
}

impl Encoder for InputEvent {
    fn encode<'a>(&self, env: Env<'a>) -> Term<'a> {
        match self {
            InputEvent::CursorPos { x, y } => (cursor_pos(), (*x, *y)).encode(env),

            InputEvent::CursorButton {
                button,
                action,
                mods,
                x,
                y,
            } => {
                let button_atom = Atom::from_str(env, button)
                    .unwrap_or_else(|_| Atom::from_str(env, "unknown").expect("unknown"));
                let mods = InputEvent::mods_to_terms(env, *mods);
                (cursor_button(), (button_atom, *action, mods, (*x, *y))).encode(env)
            }

            InputEvent::CursorScroll { dx, dy, x, y } => {
                (cursor_scroll(), ((*dx, *dy), (*x, *y))).encode(env)
            }

            InputEvent::Key {
                key: key_name,
                action,
                mods,
            } => {
                let key_atom = Atom::from_str(env, key_name)
                    .unwrap_or_else(|_| Atom::from_str(env, "unknown").expect("unknown"));
                let mods = InputEvent::mods_to_terms(env, *mods);
                (key(), (key_atom, *action, mods)).encode(env)
            }

            InputEvent::Codepoint {
                codepoint: cp,
                mods,
            } => {
                let mods = InputEvent::mods_to_terms(env, *mods);
                (codepoint(), (cp.to_string(), mods)).encode(env)
            }

            InputEvent::CursorEntered { entered } => (cursor_entered(), *entered).encode(env),

            InputEvent::Resized {
                width,
                height,
                scale_factor,
            } => (resized(), (*width, *height, *scale_factor)).encode(env),

            InputEvent::Focused {
                focused: is_focused,
            } => (focused(), *is_focused).encode(env),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{Element, ElementKind, ElementTree};

    fn make_element(id: u8, attrs: Attrs, frame: Frame, children: Vec<ElementId>) -> Element {
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
            Frame {
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
            Frame {
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
            Frame {
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
            Frame {
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
            Frame {
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
            Frame {
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
    fn test_detect_click_press_release() {
        let mut handler = InputHandler::new();
        let registry = vec![EventNode {
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
        }];
        handler.set_event_registry(registry);

        let press = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };
        let release = InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_RELEASE,
            mods: 0,
            x: 10.0,
            y: 10.0,
        };

        assert_eq!(handler.detect_click(&press), None);
        assert_eq!(handler.detect_click(&release), Some(ElementId::from_term_bytes(vec![1])));
    }
}
