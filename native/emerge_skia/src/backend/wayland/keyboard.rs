use wayland_client::protocol::wl_keyboard;

pub(super) use crate::keys::wayland::{key_from_keysym, mods_from_sctk};

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
