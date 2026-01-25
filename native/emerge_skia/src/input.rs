//! Input event handling for emerge_skia.
//!
//! This module provides:
//! - `InputEvent` enum representing mouse/keyboard events
//! - `InputHandler` for filtering and sending events to Elixir
//! - Encoder impl for sending events to Elixir
//! - Input mask constants for filtering events

// Event processing lives in events.rs
use rustler::{Atom, Encoder, Env, Term};

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
pub const EVENT_SCROLL_X_NEG: u8 = 0x02;
pub const EVENT_SCROLL_X_POS: u8 = 0x04;
pub const EVENT_SCROLL_Y_NEG: u8 = 0x08;
pub const EVENT_SCROLL_Y_POS: u8 = 0x10;

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
    mask: u32,
    cursor_pos: (f32, f32),
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            mask: INPUT_MASK_ALL,
            cursor_pos: (0.0, 0.0),
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
    pub fn accepts(&self, event: &InputEvent) -> bool {
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

        self.mask & event_mask != 0
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
