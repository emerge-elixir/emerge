//! Input event handling for emerge_skia.
//!
//! This module provides:
//! - `InputEvent` enum representing mouse/keyboard events
//! - `InputHandler` for filtering and sending events to Elixir
//! - Encoder impl for sending events to Elixir
//! - Input mask constants for filtering events

use rustler::{Atom, Encoder, Env, LocalPid, OwnedBinary, OwnedEnv, Term};

use crate::tree::element::{ElementId, ElementTree, Frame};

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
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            target: None,
            mask: INPUT_MASK_ALL,
            cursor_pos: (0.0, 0.0),
            event_registry: Vec::new(),
            pressed_id: None,
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

    pub fn set_event_registry(&mut self, registry: Vec<EventNode>) {
        self.event_registry = registry;
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

        if let Some(clicked_id) = self.detect_click(&event) {
            send_element_event(pid, &clicked_id, click());
        }

        // Send event to Elixir process
        send_input_event(pid, event);
        true
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

        let hit = hit_test(&self.event_registry, *x, *y);
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
    pub frame: Frame,
}

pub fn build_click_registry(tree: &ElementTree) -> Vec<EventNode> {
    let Some(root) = tree.root.as_ref() else {
        return Vec::new();
    };

    let mut registry = Vec::new();
    collect_click_nodes(tree, root, &mut registry);
    registry
}

fn collect_click_nodes(tree: &ElementTree, id: &ElementId, registry: &mut Vec<EventNode>) {
    let Some(element) = tree.get(id) else {
        return;
    };

    if element.attrs.on_click.unwrap_or(false) {
        if let Some(frame) = element.frame {
            registry.push(EventNode {
                id: element.id.clone(),
                frame,
            });
        }
    }

    for child_id in &element.children {
        collect_click_nodes(tree, child_id, registry);
    }
}

fn hit_test(registry: &[EventNode], x: f32, y: f32) -> Option<ElementId> {
    for node in registry.iter().rev() {
        let frame = node.frame;
        let within_x = x >= frame.x && x <= frame.x + frame.width;
        let within_y = y >= frame.y && y <= frame.y + frame.height;
        if within_x && within_y {
            return Some(node.id.clone());
        }
    }
    None
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
    fn test_build_click_registry_order() {
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

        let registry = build_click_registry(&tree);
        assert_eq!(registry.len(), 2);
        assert_eq!(registry[0].id, ElementId::from_term_bytes(vec![1]));
        assert_eq!(registry[1].id, ElementId::from_term_bytes(vec![2]));

        let hit = hit_test(&registry, 10.0, 10.0).unwrap();
        assert_eq!(hit, ElementId::from_term_bytes(vec![2]));
    }

    #[test]
    fn test_detect_click_press_release() {
        let mut handler = InputHandler::new();
        let registry = vec![EventNode {
            id: ElementId::from_term_bytes(vec![1]),
            frame: Frame {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
                content_width: 100.0,
                content_height: 100.0,
            },
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
