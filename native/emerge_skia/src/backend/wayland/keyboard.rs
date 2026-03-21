use smithay_client_toolkit::seat::keyboard::{Keysym, Modifiers};
use wayland_client::protocol::wl_keyboard;

use crate::input::{MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT};

pub(super) struct KeyboardInputState {
    pub(super) keyboard: Option<wl_keyboard::WlKeyboard>,
    pub(super) focused: bool,
    pub(super) current_mods: u8,
}

impl KeyboardInputState {
    pub(super) fn new() -> Self {
        Self {
            keyboard: None,
            focused: false,
            current_mods: 0,
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

pub(super) fn key_name_from_keysym(keysym: Keysym) -> String {
    match keysym {
        Keysym::Escape => "escape".to_string(),
        Keysym::BackSpace => "backspace".to_string(),
        Keysym::Tab
        | Keysym::ISO_Left_Tab
        | Keysym::KP_Tab
        | Keysym::BackTab
        | Keysym::KP_BackTab => "tab".to_string(),
        Keysym::Return | Keysym::KP_Enter => "enter".to_string(),
        Keysym::space | Keysym::KP_Space => "space".to_string(),
        Keysym::Delete | Keysym::KP_Delete => "delete".to_string(),
        Keysym::Insert | Keysym::KP_Insert => "insert".to_string(),
        Keysym::Home | Keysym::KP_Home => "home".to_string(),
        Keysym::End | Keysym::KP_End => "end".to_string(),
        Keysym::Page_Up | Keysym::KP_Page_Up => "page_up".to_string(),
        Keysym::Page_Down | Keysym::KP_Page_Down => "page_down".to_string(),
        Keysym::Up | Keysym::KP_Up => "up".to_string(),
        Keysym::Down | Keysym::KP_Down => "down".to_string(),
        Keysym::Left | Keysym::KP_Left => "left".to_string(),
        Keysym::Right | Keysym::KP_Right => "right".to_string(),
        Keysym::F1 => "f1".to_string(),
        Keysym::F2 => "f2".to_string(),
        Keysym::F3 => "f3".to_string(),
        Keysym::F4 => "f4".to_string(),
        Keysym::F5 => "f5".to_string(),
        Keysym::F6 => "f6".to_string(),
        Keysym::F7 => "f7".to_string(),
        Keysym::F8 => "f8".to_string(),
        Keysym::F9 => "f9".to_string(),
        Keysym::F10 => "f10".to_string(),
        Keysym::F11 => "f11".to_string(),
        Keysym::F12 => "f12".to_string(),
        Keysym::Shift_L | Keysym::Shift_R => "shift".to_string(),
        Keysym::Control_L | Keysym::Control_R => "control".to_string(),
        Keysym::Alt_L | Keysym::Alt_R => "alt".to_string(),
        Keysym::Super_L
        | Keysym::Super_R
        | Keysym::Meta_L
        | Keysym::Meta_R
        | Keysym::Hyper_L
        | Keysym::Hyper_R => "super".to_string(),
        Keysym::Caps_Lock => "caps_lock".to_string(),
        Keysym::Num_Lock => "num_lock".to_string(),
        Keysym::Scroll_Lock => "scroll_lock".to_string(),
        Keysym::Print => "print_screen".to_string(),
        Keysym::Pause => "pause".to_string(),
        _ => keysym
            .key_char()
            .filter(|ch| !ch.is_control())
            .map(|ch| ch.to_string())
            .or_else(|| keysym.name().map(|name| name.to_ascii_lowercase()))
            .unwrap_or_else(|| "unknown".to_string()),
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
    fn key_name_from_keysym_matches_expected_named_keys() {
        assert_eq!(key_name_from_keysym(Keysym::Escape), "escape");
        assert_eq!(key_name_from_keysym(Keysym::BackSpace), "backspace");
        assert_eq!(key_name_from_keysym(Keysym::Page_Down), "page_down");
        assert_eq!(key_name_from_keysym(Keysym::Super_L), "super");
        assert_eq!(key_name_from_keysym(Keysym::Print), "print_screen");
    }

    #[test]
    fn key_name_from_keysym_falls_back_to_printable_character() {
        assert_eq!(key_name_from_keysym(Keysym::a), "a");
        assert_eq!(key_name_from_keysym(Keysym::A), "A");
    }

    #[test]
    fn normalize_commit_text_filters_control_characters() {
        assert_eq!(normalize_commit_text("ab\ncd"), Some("abcd".to_string()));
        assert_eq!(normalize_commit_text("\u{7f}"), None);
    }
}
