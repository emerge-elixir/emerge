use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};
use wayland_client::protocol::wl_keyboard;

use crate::input::{MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT};
use crate::keys::CanonicalKey;

pub(super) struct KeyboardInputState {
    pub(super) keyboard: Option<wl_keyboard::WlKeyboard>,
    pub(super) focused: bool,
    pub(super) current_mods: u8,
    pub(super) ime_preedit_active: bool,
}

impl KeyboardInputState {
    pub(super) fn new() -> Self {
        Self {
            keyboard: None,
            focused: false,
            current_mods: 0,
            ime_preedit_active: false,
        }
    }
}

pub(super) fn mods_from_sctk(modifiers: Modifiers) -> u8 {
    let mut mods = 0;

    if modifiers.shift {
        mods |= MOD_SHIFT;
    }
    if modifiers.ctrl {
        mods |= MOD_CTRL;
    }
    if modifiers.alt {
        mods |= MOD_ALT;
    }
    if modifiers.logo {
        mods |= MOD_META;
    }

    mods
}

pub(super) fn normalize_commit_text(text: &str) -> Option<String> {
    let filtered: String = text.chars().filter(|ch| !ch.is_control()).collect();

    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

pub(super) fn key_from_keysym(keysym: Keysym) -> CanonicalKey {
    match keysym {
        Keysym::Escape => CanonicalKey::Escape,
        Keysym::BackSpace => CanonicalKey::Backspace,
        Keysym::Tab
        | Keysym::ISO_Left_Tab
        | Keysym::KP_Tab
        | Keysym::BackTab
        | Keysym::KP_BackTab => CanonicalKey::Tab,
        Keysym::Return | Keysym::KP_Enter => CanonicalKey::Enter,
        Keysym::space | Keysym::KP_Space => CanonicalKey::Space,
        Keysym::Delete | Keysym::KP_Delete => CanonicalKey::Delete,
        Keysym::Insert | Keysym::KP_Insert => CanonicalKey::Insert,
        Keysym::Home | Keysym::KP_Home => CanonicalKey::Home,
        Keysym::End | Keysym::KP_End => CanonicalKey::End,
        Keysym::Page_Up | Keysym::KP_Page_Up => CanonicalKey::PageUp,
        Keysym::Page_Down | Keysym::KP_Page_Down => CanonicalKey::PageDown,
        Keysym::Up | Keysym::KP_Up => CanonicalKey::ArrowUp,
        Keysym::Down | Keysym::KP_Down => CanonicalKey::ArrowDown,
        Keysym::Left | Keysym::KP_Left => CanonicalKey::ArrowLeft,
        Keysym::Right | Keysym::KP_Right => CanonicalKey::ArrowRight,
        Keysym::F1 => CanonicalKey::F1,
        Keysym::F2 => CanonicalKey::F2,
        Keysym::F3 => CanonicalKey::F3,
        Keysym::F4 => CanonicalKey::F4,
        Keysym::F5 => CanonicalKey::F5,
        Keysym::F6 => CanonicalKey::F6,
        Keysym::F7 => CanonicalKey::F7,
        Keysym::F8 => CanonicalKey::F8,
        Keysym::F9 => CanonicalKey::F9,
        Keysym::F10 => CanonicalKey::F10,
        Keysym::F11 => CanonicalKey::F11,
        Keysym::F12 => CanonicalKey::F12,
        Keysym::F13 => CanonicalKey::F13,
        Keysym::F14 => CanonicalKey::F14,
        Keysym::F15 => CanonicalKey::F15,
        Keysym::F16 => CanonicalKey::F16,
        Keysym::F17 => CanonicalKey::F17,
        Keysym::F18 => CanonicalKey::F18,
        Keysym::F19 => CanonicalKey::F19,
        Keysym::F20 => CanonicalKey::F20,
        Keysym::F21 => CanonicalKey::F21,
        Keysym::F22 => CanonicalKey::F22,
        Keysym::F23 => CanonicalKey::F23,
        Keysym::F24 => CanonicalKey::F24,
        Keysym::Shift_L | Keysym::Shift_R => CanonicalKey::Shift,
        Keysym::Control_L | Keysym::Control_R => CanonicalKey::Control,
        Keysym::Alt_L => CanonicalKey::Alt,
        Keysym::Alt_R => CanonicalKey::AltGraph,
        Keysym::Super_L
        | Keysym::Super_R
        | Keysym::Meta_L
        | Keysym::Meta_R
        | Keysym::Hyper_L
        | Keysym::Hyper_R => CanonicalKey::Super,
        Keysym::Caps_Lock => CanonicalKey::CapsLock,
        Keysym::Num_Lock => CanonicalKey::NumLock,
        Keysym::Scroll_Lock => CanonicalKey::ScrollLock,
        Keysym::Print => CanonicalKey::PrintScreen,
        Keysym::Pause => CanonicalKey::Pause,
        Keysym::Menu => CanonicalKey::ContextMenu,
        _ => keypad_canonical_key(keysym)
            .or_else(|| non_keypad_canonical_key(keysym))
            .or_else(|| {
                keysym
                    .name()
                    .map(|name| name.to_ascii_lowercase())
                    .and_then(|name| CanonicalKey::from_atom_name(&name))
            })
            .unwrap_or(CanonicalKey::Unknown),
    }
}

fn keypad_canonical_key(keysym: Keysym) -> Option<CanonicalKey> {
    if !keysym.is_keypad_key() {
        return None;
    }

    match keysym.key_char().filter(|ch| !ch.is_control())? {
        '0' => Some(CanonicalKey::Digit0),
        '1' => Some(CanonicalKey::Digit1),
        '2' => Some(CanonicalKey::Digit2),
        '3' => Some(CanonicalKey::Digit3),
        '4' => Some(CanonicalKey::Digit4),
        '5' => Some(CanonicalKey::Digit5),
        '6' => Some(CanonicalKey::Digit6),
        '7' => Some(CanonicalKey::Digit7),
        '8' => Some(CanonicalKey::Digit8),
        '9' => Some(CanonicalKey::Digit9),
        '.' => Some(CanonicalKey::Period),
        '/' => Some(CanonicalKey::Slash),
        '-' => Some(CanonicalKey::Minus),
        '=' => Some(CanonicalKey::Equal),
        '+' => Some(CanonicalKey::Plus),
        '*' => Some(CanonicalKey::Asterisk),
        ' ' => Some(CanonicalKey::Space),
        _ => None,
    }
}

fn non_keypad_canonical_key(keysym: Keysym) -> Option<CanonicalKey> {
    if keysym.is_keypad_key() {
        return None;
    }

    let ch = keysym.key_char().filter(|ch| !ch.is_control())?;

    match ch {
        '0' | ')' => Some(CanonicalKey::Digit0),
        '1' | '!' => Some(CanonicalKey::Digit1),
        '2' | '@' => Some(CanonicalKey::Digit2),
        '3' | '#' => Some(CanonicalKey::Digit3),
        '4' | '$' => Some(CanonicalKey::Digit4),
        '5' | '%' => Some(CanonicalKey::Digit5),
        '6' | '^' => Some(CanonicalKey::Digit6),
        '7' | '&' => Some(CanonicalKey::Digit7),
        '8' | '*' => Some(CanonicalKey::Digit8),
        '9' | '(' => Some(CanonicalKey::Digit9),
        '-' | '_' => Some(CanonicalKey::Minus),
        '=' | '+' => Some(CanonicalKey::Equal),
        '[' | '{' => Some(CanonicalKey::LeftBracket),
        ']' | '}' => Some(CanonicalKey::RightBracket),
        '\\' | '|' => Some(CanonicalKey::Backslash),
        ';' | ':' => Some(CanonicalKey::Semicolon),
        '\'' | '"' => Some(CanonicalKey::Apostrophe),
        '`' | '~' => Some(CanonicalKey::Grave),
        ',' | '<' => Some(CanonicalKey::Comma),
        '.' | '>' => Some(CanonicalKey::Period),
        '/' | '?' => Some(CanonicalKey::Slash),
        _ => CanonicalKey::from_printable_char(ch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_from_sctk_maps_supported_modifier_bits() {
        let modifiers = Modifiers {
            ctrl: true,
            alt: true,
            shift: true,
            caps_lock: false,
            logo: true,
            num_lock: false,
        };

        assert_eq!(
            mods_from_sctk(modifiers),
            MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_META
        );
    }

    #[test]
    fn key_from_keysym_matches_expected_named_keys() {
        assert_eq!(key_from_keysym(Keysym::Escape), CanonicalKey::Escape);
        assert_eq!(key_from_keysym(Keysym::BackSpace), CanonicalKey::Backspace);
        assert_eq!(key_from_keysym(Keysym::Page_Down), CanonicalKey::PageDown);
        assert_eq!(key_from_keysym(Keysym::Super_L), CanonicalKey::Super);
        assert_eq!(key_from_keysym(Keysym::Print), CanonicalKey::PrintScreen);
    }

    #[test]
    fn key_from_keysym_normalizes_printable_characters() {
        assert_eq!(key_from_keysym(Keysym::a), CanonicalKey::A);
        assert_eq!(key_from_keysym(Keysym::A), CanonicalKey::A);
    }

    #[test]
    fn key_from_keysym_normalizes_shifted_symbols_to_logical_keys() {
        assert_eq!(key_from_keysym(Keysym::from_char('+')), CanonicalKey::Equal);
        assert_eq!(
            key_from_keysym(Keysym::from_char('*')),
            CanonicalKey::Digit8
        );
        assert_eq!(
            key_from_keysym(Keysym::from_char('{')),
            CanonicalKey::LeftBracket
        );
        assert_eq!(key_from_keysym(Keysym::from_char('?')), CanonicalKey::Slash);
    }

    #[test]
    fn key_from_keysym_maps_keypad_symbols_to_canonical_keys() {
        assert_eq!(key_from_keysym(Keysym::new(0xffab)), CanonicalKey::Plus);
        assert_eq!(key_from_keysym(Keysym::new(0xffaa)), CanonicalKey::Asterisk);
        assert_eq!(key_from_keysym(Keysym::new(0xffae)), CanonicalKey::Period);
        assert_eq!(key_from_keysym(Keysym::new(0xffaf)), CanonicalKey::Slash);
        assert_eq!(key_from_keysym(Keysym::new(0xffbd)), CanonicalKey::Equal);
        assert_eq!(key_from_keysym(Keysym::new(0xffb1)), CanonicalKey::Digit1);
    }

    #[test]
    fn normalize_commit_text_filters_control_characters() {
        assert_eq!(normalize_commit_text("ab\ncd"), Some("abcd".to_string()));
        assert_eq!(normalize_commit_text("\u{7f}"), None);
    }
}
