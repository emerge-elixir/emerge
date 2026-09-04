//! Input event handling for emerge_skia.
//!
//! This module provides:
//! - `InputEvent` enum representing mouse/keyboard events
//! - `InputHandler` for filtering and sending events to Elixir
//! - Encoder impl for sending events to Elixir
//! - Input mask constants for filtering events

// Event processing lives in events.rs
use rustler::{Atom, Encoder, Env, Term};

use crate::keys::CanonicalKey;

// ============================================================================
// Input Event
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
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

    /// Mouse scroll wheel (line delta, normalized later)
    CursorScrollLines { dx: f32, dy: f32, x: f32, y: f32 },

    /// Keyboard key pressed/released
    Key {
        key: CanonicalKey,
        action: u8,
        mods: u8,
    },

    /// Text input commit (may contain multiple chars)
    TextCommit { text: String, mods: u8 },

    /// IME preedit text update for focused text input
    #[cfg_attr(
        not(any(all(feature = "wayland-core", target_os = "linux"), feature = "macos")),
        allow(dead_code)
    )]
    TextPreedit {
        text: String,
        cursor: Option<(u32, u32)>,
    },

    /// IME preedit text cleared
    #[cfg_attr(
        not(any(all(feature = "wayland-core", target_os = "linux"), feature = "macos")),
        allow(dead_code)
    )]
    TextPreeditClear,

    /// IME requests deletion of surrounding text in UTF-8 byte lengths
    #[cfg_attr(
        not(any(all(feature = "wayland-core", target_os = "linux"), feature = "macos")),
        allow(dead_code)
    )]
    DeleteSurrounding {
        before_length: u32,
        after_length: u32,
    },

    /// Cursor entered/exited window
    #[cfg_attr(
        not(any(all(feature = "wayland-core", target_os = "linux"), feature = "macos")),
        allow(dead_code)
    )]
    CursorEntered { entered: bool },

    /// Window resized
    Resized {
        width: u32,
        height: u32,
        scale_factor: f32,
    },

    /// Window focused/unfocused
    #[cfg_attr(
        not(any(all(feature = "wayland-core", target_os = "linux"), feature = "macos")),
        allow(dead_code)
    )]
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

pub const SCROLL_LINE_PIXELS: f32 = 30.0;

// ============================================================================
// Shared Input Normalization
// ============================================================================

pub mod pointer {
    use super::{ACTION_PRESS, ACTION_RELEASE, InputEvent};

    pub const BUTTON_LEFT: &str = "left";
    pub const BUTTON_RIGHT: &str = "right";
    pub const BUTTON_MIDDLE: &str = "middle";
    pub const BUTTON_BACK: &str = "back";
    pub const BUTTON_FORWARD: &str = "forward";
    pub const BUTTON_OTHER: &str = "other";

    pub fn canonical_button_label(label: &str) -> &'static str {
        match label {
            BUTTON_LEFT => BUTTON_LEFT,
            BUTTON_RIGHT => BUTTON_RIGHT,
            BUTTON_MIDDLE => BUTTON_MIDDLE,
            BUTTON_BACK => BUTTON_BACK,
            BUTTON_FORWARD => BUTTON_FORWARD,
            _ => BUTTON_OTHER,
        }
    }

    pub fn cursor_button_event(
        button: &str,
        pressed: bool,
        mods: u8,
        position: (f32, f32),
    ) -> InputEvent {
        InputEvent::CursorButton {
            button: canonical_button_label(button).to_string(),
            action: if pressed {
                ACTION_PRESS
            } else {
                ACTION_RELEASE
            },
            mods,
            x: position.0,
            y: position.1,
        }
    }

    pub fn precise_scroll_deltas(dx: f32, dy: f32, scale_factor: f32) -> (f32, f32) {
        let scale = scale_factor.max(1.0);
        (dx * scale, dy * scale)
    }

    pub fn line_scroll_deltas(dx: f32, dy: f32, scroll_line_pixels: f32) -> (f32, f32) {
        (dx * scroll_line_pixels, dy * scroll_line_pixels)
    }
}

pub mod keyboard {
    use crate::keys::CanonicalKey;

    use super::{MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT};

    pub fn modifier_bits(shift: bool, ctrl: bool, alt: bool, meta: bool) -> u8 {
        let mut mods = 0;

        if shift {
            mods |= MOD_SHIFT;
        }
        if ctrl {
            mods |= MOD_CTRL;
        }
        if alt {
            mods |= MOD_ALT;
        }
        if meta {
            mods |= MOD_META;
        }

        mods
    }

    pub fn normalize_commit_text(text: &str) -> Option<String> {
        let filtered: String = text
            .chars()
            .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
            .collect();

        if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        }
    }

    pub fn text_commit_from_key_and_text(
        key: CanonicalKey,
        mods: u8,
        text: &str,
    ) -> Option<String> {
        if mods & (MOD_CTRL | MOD_META) != 0 || suppress_text_commit_for_key(key) {
            return None;
        }

        normalize_commit_text(text)
    }

    pub fn suppress_text_commit_for_key(key: CanonicalKey) -> bool {
        matches!(
            key,
            CanonicalKey::Escape
                | CanonicalKey::Backspace
                | CanonicalKey::Delete
                | CanonicalKey::Insert
                | CanonicalKey::Home
                | CanonicalKey::End
                | CanonicalKey::PageUp
                | CanonicalKey::PageDown
                | CanonicalKey::ArrowLeft
                | CanonicalKey::ArrowRight
                | CanonicalKey::ArrowUp
                | CanonicalKey::ArrowDown
                | CanonicalKey::Shift
                | CanonicalKey::Control
                | CanonicalKey::Alt
                | CanonicalKey::AltGraph
                | CanonicalKey::Super
                | CanonicalKey::CapsLock
                | CanonicalKey::NumLock
                | CanonicalKey::ScrollLock
                | CanonicalKey::PrintScreen
                | CanonicalKey::Pause
                | CanonicalKey::ContextMenu
                | CanonicalKey::F1
                | CanonicalKey::F2
                | CanonicalKey::F3
                | CanonicalKey::F4
                | CanonicalKey::F5
                | CanonicalKey::F6
                | CanonicalKey::F7
                | CanonicalKey::F8
                | CanonicalKey::F9
                | CanonicalKey::F10
                | CanonicalKey::F11
                | CanonicalKey::F12
                | CanonicalKey::F13
                | CanonicalKey::F14
                | CanonicalKey::F15
                | CanonicalKey::F16
                | CanonicalKey::F17
                | CanonicalKey::F18
                | CanonicalKey::F19
                | CanonicalKey::F20
                | CanonicalKey::F21
                | CanonicalKey::F22
                | CanonicalKey::F23
                | CanonicalKey::F24
        )
    }
}

// ============================================================================
// Atoms
// ============================================================================

rustler::atoms! {
    emerge_skia_event,
    key,
    text_commit,
    text_preedit,
    text_preedit_clear,
    delete_surrounding,
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
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            mask: INPUT_MASK_ALL,
        }
    }

    /// Set the input mask for filtering events
    pub fn set_mask(&mut self, mask: u32) {
        self.mask = mask;
    }

    /// Set the target pid for input events
    pub fn accepts(&self, event: &InputEvent) -> bool {
        let event_mask = match &event {
            InputEvent::Key { .. } => INPUT_MASK_KEY,
            InputEvent::TextCommit { .. }
            | InputEvent::TextPreedit { .. }
            | InputEvent::TextPreeditClear
            | InputEvent::DeleteSurrounding { .. } => INPUT_MASK_CODEPOINT,
            InputEvent::CursorPos { .. } => INPUT_MASK_CURSOR_POS,
            InputEvent::CursorButton { .. } => INPUT_MASK_CURSOR_BUTTON,
            InputEvent::CursorScroll { .. } | InputEvent::CursorScrollLines { .. } => {
                INPUT_MASK_CURSOR_SCROLL
            }
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

// ============================================================================
// Encoder Implementation
// ============================================================================

impl InputEvent {
    pub fn normalize_scroll(self) -> InputEvent {
        self.normalize_scroll_with_line_pixels(SCROLL_LINE_PIXELS)
    }

    pub fn normalize_scroll_with_line_pixels(self, scroll_line_pixels: f32) -> InputEvent {
        match self {
            InputEvent::CursorScrollLines { dx, dy, x, y } => InputEvent::CursorScroll {
                dx: dx * scroll_line_pixels,
                dy: dy * scroll_line_pixels,
                x,
                y,
            },
            other => other,
        }
    }

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

            InputEvent::CursorScrollLines { dx, dy, x, y } => {
                let dx = *dx * SCROLL_LINE_PIXELS;
                let dy = *dy * SCROLL_LINE_PIXELS;
                (cursor_scroll(), ((dx, dy), (*x, *y))).encode(env)
            }

            InputEvent::Key {
                key: key_name,
                action,
                mods,
            } => {
                let key_atom = Atom::from_str(env, key_name.atom_name())
                    .unwrap_or_else(|_| Atom::from_str(env, "unknown").expect("unknown"));
                let mods = InputEvent::mods_to_terms(env, *mods);
                (key(), (key_atom, *action, mods)).encode(env)
            }

            InputEvent::TextCommit { text, mods } => {
                let mods = InputEvent::mods_to_terms(env, *mods);
                (text_commit(), (text.clone(), mods)).encode(env)
            }

            InputEvent::TextPreedit { text, cursor } => {
                (text_preedit(), (text.clone(), *cursor)).encode(env)
            }

            InputEvent::TextPreeditClear => text_preedit_clear().encode(env),

            InputEvent::DeleteSurrounding {
                before_length,
                after_length,
            } => (delete_surrounding(), (*before_length, *after_length)).encode(env),

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
    use crate::keys::CanonicalKey;

    use super::{ACTION_PRESS, InputEvent, MOD_CTRL, MOD_META, keyboard, pointer};

    #[test]
    fn normalize_scroll_with_line_pixels_scales_discrete_wheel_steps() {
        let event = InputEvent::CursorScrollLines {
            dx: 2.0,
            dy: -3.0,
            x: 10.0,
            y: 20.0,
        }
        .normalize_scroll_with_line_pixels(45.0);

        assert!(matches!(
            event,
            InputEvent::CursorScroll { dx, dy, x, y }
                if (dx - 90.0).abs() < f32::EPSILON
                    && (dy + 135.0).abs() < f32::EPSILON
                    && (x - 10.0).abs() < f32::EPSILON
                    && (y - 20.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn normalize_scroll_with_line_pixels_leaves_absolute_scroll_unchanged() {
        let event = InputEvent::CursorScroll {
            dx: 4.5,
            dy: -7.25,
            x: 3.0,
            y: 5.0,
        }
        .normalize_scroll_with_line_pixels(45.0);

        assert!(matches!(
            event,
            InputEvent::CursorScroll { dx, dy, x, y }
                if (dx - 4.5).abs() < f32::EPSILON
                    && (dy + 7.25).abs() < f32::EPSILON
                    && (x - 3.0).abs() < f32::EPSILON
                    && (y - 5.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn pointer_button_helper_normalizes_labels_and_actions() {
        assert_eq!(pointer::canonical_button_label("left"), "left");
        assert_eq!(pointer::canonical_button_label("back"), "back");
        assert_eq!(pointer::canonical_button_label("unknown"), "other");

        assert_eq!(
            pointer::cursor_button_event("forward", true, MOD_META, (12.0, 18.0)),
            InputEvent::CursorButton {
                button: "forward".to_string(),
                action: ACTION_PRESS,
                mods: MOD_META,
                x: 12.0,
                y: 18.0,
            }
        );
    }

    #[test]
    fn pointer_scroll_delta_helpers_match_backend_units() {
        assert_eq!(pointer::precise_scroll_deltas(3.0, -4.0, 2.0), (6.0, -8.0));
        assert_eq!(pointer::precise_scroll_deltas(3.0, -4.0, 1.0), (3.0, -4.0));
        assert_eq!(pointer::line_scroll_deltas(1.0, -2.0, 45.0), (45.0, -90.0));
    }

    #[test]
    fn keyboard_helpers_pack_modifiers_and_filter_text() {
        assert_eq!(
            keyboard::modifier_bits(true, true, false, true),
            super::MOD_SHIFT | MOD_CTRL | MOD_META
        );
        assert_eq!(
            keyboard::normalize_commit_text("a\u{7f}\nb"),
            Some("a\nb".to_string())
        );
        assert_eq!(keyboard::normalize_commit_text("\u{7f}"), None);
    }

    #[test]
    fn keyboard_text_commit_suppression_matches_shortcut_and_named_key_rules() {
        assert_eq!(
            keyboard::text_commit_from_key_and_text(CanonicalKey::A, 0, "a"),
            Some("a".to_string())
        );
        assert_eq!(
            keyboard::text_commit_from_key_and_text(CanonicalKey::A, MOD_CTRL, "a"),
            None
        );
        assert_eq!(
            keyboard::text_commit_from_key_and_text(CanonicalKey::ArrowLeft, 0, "\u{f702}"),
            None
        );
        assert!(keyboard::suppress_text_commit_for_key(CanonicalKey::F24));
    }
}
