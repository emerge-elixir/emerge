use evdev::{
    AbsoluteAxisType, Device, InputEventKind, Key, PropType, RelativeAxisType, Synchronization,
};
use libc::input_absinfo;
use std::fs;
use std::os::fd::AsRawFd;
use std::path::Path;

use crossbeam_channel::{Receiver, Sender, TrySendError};

use crate::actors::{EventMsg, RenderMsg};
use crate::input::{
    ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
};

struct InputDevice {
    device: Device,
    abs_x: Option<AbsAxisState>,
    abs_y: Option<AbsAxisState>,
    abs_x_dirty: bool,
    abs_y_dirty: bool,
    abs_mode: AbsMode,
    last_abs_scaled: Option<(f32, f32)>,
    touch_active: bool,
    touch_tracking: bool,
}

#[derive(Clone, Copy, Debug)]
struct AbsAxisState {
    value: i32,
    min: i32,
    max: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AbsMode {
    Absolute,
    RelativeFromAbs,
}

#[derive(Clone, Copy, Debug, Default)]
struct Modifiers {
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
}

#[derive(Clone, Copy, Debug)]
enum KeyKind {
    Character(char),
    Named(&'static str),
}

pub struct DrmInput {
    devices: Vec<InputDevice>,
    cursor_pos: (f32, f32),
    modifiers: Modifiers,
    caps_lock: bool,
    screen_size: (u32, u32),
    screen_rx: Receiver<(u32, u32)>,
    event_tx: Sender<EventMsg>,
    cursor_tx: Sender<RenderMsg>,
    log_enabled: bool,
}

impl DrmInput {
    pub fn new(
        screen_size: (u32, u32),
        screen_rx: Receiver<(u32, u32)>,
        event_tx: Sender<EventMsg>,
        cursor_tx: Sender<RenderMsg>,
        log_enabled: bool,
    ) -> Self {
        let devices = enumerate_devices(log_enabled);
        Self {
            devices,
            cursor_pos: (0.0, 0.0),
            modifiers: Modifiers::default(),
            caps_lock: false,
            screen_size,
            screen_rx,
            event_tx,
            cursor_tx,
            log_enabled,
        }
    }

    pub fn poll(&mut self) {
        while let Ok(size) = self.screen_rx.try_recv() {
            self.screen_size = size;
        }
        let screen_size = self.screen_size;
        for idx in 0..self.devices.len() {
            let events = {
                let device = &mut self.devices[idx];
                match device.device.fetch_events() {
                    Ok(events) => events.collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                }
            };

            for event in events {
                match event.kind() {
                    InputEventKind::Key(key) => {
                        self.handle_key_event_with_device(idx, key, event.value());
                    }
                    InputEventKind::RelAxis(axis) => {
                        self.handle_rel_event(axis, event.value(), screen_size);
                    }
                    InputEventKind::AbsAxis(axis) => {
                        let device = &mut self.devices[idx];
                        update_abs_state(device, axis, event.value(), screen_size);
                    }
                    InputEventKind::Synchronization(sync) => {
                        if sync == Synchronization::SYN_REPORT {
                            let action = {
                                let device = &mut self.devices[idx];
                                consume_abs_action(device, screen_size)
                            };
                            match action {
                                AbsAction::Absolute(x, y) => {
                                    self.handle_abs_position(x, y, screen_size)
                                }
                                AbsAction::Relative(dx, dy) => {
                                    self.handle_abs_relative(dx, dy, screen_size)
                                }
                                AbsAction::None => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_key_event(&mut self, key: Key, value: i32) {
        let pressed = value != 0;
        self.update_modifiers(key, pressed);
        if key == Key::KEY_CAPSLOCK && pressed {
            self.caps_lock = !self.caps_lock;
        }

        if let Some(button) = evdev_key_to_button(key) {
            let (x, y) = self.cursor_pos;
            let action = if pressed {
                ACTION_PRESS
            } else {
                ACTION_RELEASE
            };
            let mods = modifiers_to_mask(self.modifiers);
            self.push_input(InputEvent::CursorButton {
                button: button.to_string(),
                action,
                mods,
                x,
                y,
            });
            return;
        }

        let Some(key_kind) = evdev_key_to_kind(key) else {
            return;
        };

        let mods = modifiers_to_mask(self.modifiers);
        let action = if pressed {
            ACTION_PRESS
        } else {
            ACTION_RELEASE
        };
        self.push_input(InputEvent::Key {
            key: key_kind_to_name(key_kind),
            action,
            mods,
        });

        if pressed
            && let Some(codepoint) = key_to_codepoint(key_kind, self.modifiers, self.caps_lock)
        {
            self.push_input(InputEvent::TextCommit {
                text: codepoint.to_string(),
                mods,
            });
        }
    }

    fn handle_rel_event(&mut self, axis: RelativeAxisType, value: i32, screen_size: (u32, u32)) {
        let (mut x, mut y) = self.cursor_pos;
        match axis {
            RelativeAxisType::REL_X => {
                x += value as f32;
            }
            RelativeAxisType::REL_Y => {
                y += value as f32;
            }
            RelativeAxisType::REL_WHEEL => {
                let (cx, cy) = self.cursor_pos;
                self.push_input(InputEvent::CursorScrollLines {
                    dx: 0.0,
                    dy: value as f32,
                    x: cx,
                    y: cy,
                });
                return;
            }
            RelativeAxisType::REL_HWHEEL => {
                let (cx, cy) = self.cursor_pos;
                self.push_input(InputEvent::CursorScrollLines {
                    dx: value as f32,
                    dy: 0.0,
                    x: cx,
                    y: cy,
                });
                return;
            }
            _ => return,
        }

        let (width, height) = screen_size;
        x = x.clamp(0.0, width.saturating_sub(1) as f32);
        y = y.clamp(0.0, height.saturating_sub(1) as f32);
        self.set_cursor_pos(x, y);
        self.push_input(InputEvent::CursorPos { x, y });
    }

    fn handle_abs_position(&mut self, x: f32, y: f32, _screen_size: (u32, u32)) {
        self.set_cursor_pos(x, y);
        self.push_input(InputEvent::CursorPos { x, y });
    }

    fn handle_abs_relative(&mut self, dx: f32, dy: f32, screen_size: (u32, u32)) {
        let (mut x, mut y) = self.cursor_pos;
        x += dx;
        y += dy;
        let (width, height) = screen_size;
        x = x.clamp(0.0, width.saturating_sub(1) as f32);
        y = y.clamp(0.0, height.saturating_sub(1) as f32);
        self.set_cursor_pos(x, y);
        self.push_input(InputEvent::CursorPos { x, y });
    }

    fn set_cursor_pos(&mut self, x: f32, y: f32) {
        self.cursor_pos = (x, y);
        let _ = self.cursor_tx.try_send(RenderMsg::CursorUpdate {
            pos: (x, y),
            visible: true,
        });
    }

    fn update_modifiers(&mut self, key: Key, pressed: bool) {
        match key {
            Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => self.modifiers.shift = pressed,
            Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => self.modifiers.ctrl = pressed,
            Key::KEY_LEFTALT | Key::KEY_RIGHTALT => self.modifiers.alt = pressed,
            Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => self.modifiers.meta = pressed,
            _ => {}
        }
    }

    fn push_input(&self, event: InputEvent) {
        if self.log_enabled
            && let InputEvent::CursorPos { x, y } = &event
        {
            eprintln!("drm_input enqueue cursor_pos x={x:.2} y={y:.2}");
        }

        let msg = EventMsg::InputEvent(event);
        match self.event_tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(msg)) => {
                if self.log_enabled {
                    eprintln!("event channel full, blocking send");
                }
                let _ = self.event_tx.send(msg);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

fn enumerate_devices(log_enabled: bool) -> Vec<InputDevice> {
    let mut devices = Vec::new();
    let entries = match fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(_) => return devices,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_event_device(&path) {
            continue;
        }
        let device = match Device::open(&path) {
            Ok(device) => device,
            Err(_) => continue,
        };
        set_non_blocking(device.as_raw_fd());
        let (abs_mode, info) = detect_abs_mode(&device);
        let (abs_x, abs_y) = init_abs_axes(&device);
        if log_enabled {
            let name = device.name().unwrap_or("unknown");
            eprintln!(
                "drm_input device={:?} name=\"{}\" abs_mode={:?} {}",
                path, name, abs_mode, info
            );
        }
        devices.push(InputDevice {
            device,
            abs_x,
            abs_y,
            abs_x_dirty: false,
            abs_y_dirty: false,
            abs_mode,
            last_abs_scaled: None,
            touch_active: false,
            touch_tracking: false,
        });
    }

    devices
}

fn is_event_device(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("event"))
        .unwrap_or(false)
}

enum AbsAction {
    None,
    Absolute(f32, f32),
    Relative(f32, f32),
}

fn update_abs_state(
    device: &mut InputDevice,
    axis: AbsoluteAxisType,
    value: i32,
    screen_size: (u32, u32),
) {
    let fallback = (
        screen_size.0.saturating_sub(1) as i32,
        screen_size.1.saturating_sub(1) as i32,
    );
    match axis {
        AbsoluteAxisType::ABS_X => {
            device.abs_x = Some(update_axis_state(device.abs_x, value, fallback.0));
            device.abs_x_dirty = true;
        }
        AbsoluteAxisType::ABS_Y => {
            device.abs_y = Some(update_axis_state(device.abs_y, value, fallback.1));
            device.abs_y_dirty = true;
        }
        _ => {}
    }
}

fn consume_abs_action(device: &mut InputDevice, screen_size: (u32, u32)) -> AbsAction {
    if !(device.abs_x_dirty || device.abs_y_dirty) {
        return AbsAction::None;
    }

    let (abs_x, abs_y) = match (device.abs_x, device.abs_y) {
        (Some(abs_x), Some(abs_y)) => (abs_x, abs_y),
        _ => return AbsAction::None,
    };

    let (scaled_x, scaled_y) = (
        scale_abs_value(abs_x, screen_size.0),
        scale_abs_value(abs_y, screen_size.1),
    );
    let last = device.last_abs_scaled.unwrap_or((scaled_x, scaled_y));
    let scaled = (
        if device.abs_x_dirty { scaled_x } else { last.0 },
        if device.abs_y_dirty { scaled_y } else { last.1 },
    );
    device.abs_x_dirty = false;
    device.abs_y_dirty = false;

    if device.abs_mode == AbsMode::RelativeFromAbs {
        if device.touch_tracking && !device.touch_active {
            device.last_abs_scaled = Some(scaled);
            return AbsAction::None;
        }
        let (dx, dy) = match device.last_abs_scaled {
            Some((last_x, last_y)) => (scaled.0 - last_x, scaled.1 - last_y),
            None => (0.0, 0.0),
        };
        device.last_abs_scaled = Some(scaled);
        AbsAction::Relative(dx, dy)
    } else {
        AbsAction::Absolute(scaled.0, scaled.1)
    }
}

fn update_axis_state(current: Option<AbsAxisState>, value: i32, fallback_max: i32) -> AbsAxisState {
    match current {
        Some(mut state) => {
            state.value = value;
            state
        }
        None => AbsAxisState {
            value,
            min: 0,
            max: fallback_max,
        },
    }
}

fn scale_abs_value(state: AbsAxisState, screen_max: u32) -> f32 {
    let screen_max = screen_max.saturating_sub(1) as f32;
    if screen_max <= 0.0 {
        return 0.0;
    }
    let min = state.min as f32;
    let max = state.max as f32;
    if max <= min {
        return (state.value as f32).clamp(0.0, screen_max);
    }
    let norm = ((state.value as f32 - min) / (max - min)).clamp(0.0, 1.0);
    norm * screen_max
}

fn init_abs_axes(device: &Device) -> (Option<AbsAxisState>, Option<AbsAxisState>) {
    let Ok(abs_state) = device.get_abs_state() else {
        return (None, None);
    };

    let abs_x = axis_state_from_abs(abs_state.get(AbsoluteAxisType::ABS_X.0 as usize));
    let abs_y = axis_state_from_abs(abs_state.get(AbsoluteAxisType::ABS_Y.0 as usize));
    (abs_x, abs_y)
}

fn axis_state_from_abs(info: Option<&input_absinfo>) -> Option<AbsAxisState> {
    info.map(|info| AbsAxisState {
        value: info.value,
        min: info.minimum,
        max: info.maximum,
    })
}

fn detect_abs_mode(device: &Device) -> (AbsMode, String) {
    let has_abs = device.supported_absolute_axes().is_some_and(|axes| {
        axes.contains(AbsoluteAxisType::ABS_X) && axes.contains(AbsoluteAxisType::ABS_Y)
    });
    if !has_abs {
        return (AbsMode::Absolute, "abs_axes=none".to_string());
    }

    let props = device.properties();
    let direct_prop = props.contains(PropType::DIRECT);
    let buttonpad_prop = props.contains(PropType::BUTTONPAD);
    let topbuttonpad_prop = props.contains(PropType::TOPBUTTONPAD);
    let semi_mt_prop = props.contains(PropType::SEMI_MT);
    let pointer_prop = props.contains(PropType::POINTER);

    let key_hint = device.supported_keys().is_some_and(|keys| {
        keys.contains(Key::BTN_TOOL_FINGER)
            || keys.contains(Key::BTN_TOUCH)
            || keys.contains(Key::BTN_TOOL_DOUBLETAP)
            || keys.contains(Key::BTN_TOOL_TRIPLETAP)
            || keys.contains(Key::BTN_TOOL_QUADTAP)
            || keys.contains(Key::BTN_TOOL_QUINTTAP)
    });

    let name_hint = device
        .name()
        .map(|name| name.to_ascii_lowercase().contains("touchpad"))
        .unwrap_or(false);

    let touchpad = !direct_prop
        && ((pointer_prop && (buttonpad_prop || topbuttonpad_prop || semi_mt_prop || key_hint))
            || name_hint);

    let info = format!(
        "abs_axes=xy direct={} pointer={} buttonpad={} topbuttonpad={} semi_mt={} key_hint={} name_hint={}",
        direct_prop,
        pointer_prop,
        buttonpad_prop,
        topbuttonpad_prop,
        semi_mt_prop,
        key_hint,
        name_hint
    );

    if touchpad {
        (AbsMode::RelativeFromAbs, info)
    } else {
        (AbsMode::Absolute, info)
    }
}

impl DrmInput {
    fn handle_key_event_with_device(&mut self, idx: usize, key: Key, value: i32) {
        let pressed = value != 0;
        if let Some(device) = self.devices.get_mut(idx)
            && device.abs_mode == AbsMode::RelativeFromAbs
            && is_touch_tracking_key(key)
        {
            device.touch_tracking = true;
            device.touch_active = pressed;
            if pressed {
                device.last_abs_scaled = None;
            }
        }

        self.handle_key_event(key, value);
    }
}

fn is_touch_tracking_key(key: Key) -> bool {
    matches!(
        key,
        Key::BTN_TOUCH
            | Key::BTN_TOOL_FINGER
            | Key::BTN_TOOL_DOUBLETAP
            | Key::BTN_TOOL_TRIPLETAP
            | Key::BTN_TOOL_QUADTAP
            | Key::BTN_TOOL_QUINTTAP
    )
}

fn set_non_blocking(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

fn modifiers_to_mask(mods: Modifiers) -> u8 {
    let mut mask = 0;
    if mods.shift {
        mask |= MOD_SHIFT;
    }
    if mods.ctrl {
        mask |= MOD_CTRL;
    }
    if mods.alt {
        mask |= MOD_ALT;
    }
    if mods.meta {
        mask |= MOD_META;
    }
    mask
}

fn evdev_key_to_kind(key: Key) -> Option<KeyKind> {
    let kind = match key {
        Key::KEY_A => KeyKind::Character('a'),
        Key::KEY_B => KeyKind::Character('b'),
        Key::KEY_C => KeyKind::Character('c'),
        Key::KEY_D => KeyKind::Character('d'),
        Key::KEY_E => KeyKind::Character('e'),
        Key::KEY_F => KeyKind::Character('f'),
        Key::KEY_G => KeyKind::Character('g'),
        Key::KEY_H => KeyKind::Character('h'),
        Key::KEY_I => KeyKind::Character('i'),
        Key::KEY_J => KeyKind::Character('j'),
        Key::KEY_K => KeyKind::Character('k'),
        Key::KEY_L => KeyKind::Character('l'),
        Key::KEY_M => KeyKind::Character('m'),
        Key::KEY_N => KeyKind::Character('n'),
        Key::KEY_O => KeyKind::Character('o'),
        Key::KEY_P => KeyKind::Character('p'),
        Key::KEY_Q => KeyKind::Character('q'),
        Key::KEY_R => KeyKind::Character('r'),
        Key::KEY_S => KeyKind::Character('s'),
        Key::KEY_T => KeyKind::Character('t'),
        Key::KEY_U => KeyKind::Character('u'),
        Key::KEY_V => KeyKind::Character('v'),
        Key::KEY_W => KeyKind::Character('w'),
        Key::KEY_X => KeyKind::Character('x'),
        Key::KEY_Y => KeyKind::Character('y'),
        Key::KEY_Z => KeyKind::Character('z'),
        Key::KEY_0 => KeyKind::Character('0'),
        Key::KEY_1 => KeyKind::Character('1'),
        Key::KEY_2 => KeyKind::Character('2'),
        Key::KEY_3 => KeyKind::Character('3'),
        Key::KEY_4 => KeyKind::Character('4'),
        Key::KEY_5 => KeyKind::Character('5'),
        Key::KEY_6 => KeyKind::Character('6'),
        Key::KEY_7 => KeyKind::Character('7'),
        Key::KEY_8 => KeyKind::Character('8'),
        Key::KEY_9 => KeyKind::Character('9'),
        Key::KEY_SPACE => KeyKind::Character(' '),
        Key::KEY_ENTER => KeyKind::Named("enter"),
        Key::KEY_TAB => KeyKind::Named("tab"),
        Key::KEY_ESC => KeyKind::Named("escape"),
        Key::KEY_BACKSPACE => KeyKind::Named("backspace"),
        Key::KEY_INSERT => KeyKind::Named("insert"),
        Key::KEY_DELETE => KeyKind::Named("delete"),
        Key::KEY_LEFT => KeyKind::Named("left"),
        Key::KEY_RIGHT => KeyKind::Named("right"),
        Key::KEY_UP => KeyKind::Named("up"),
        Key::KEY_DOWN => KeyKind::Named("down"),
        Key::KEY_PAGEUP => KeyKind::Named("page_up"),
        Key::KEY_PAGEDOWN => KeyKind::Named("page_down"),
        Key::KEY_HOME => KeyKind::Named("home"),
        Key::KEY_END => KeyKind::Named("end"),
        Key::KEY_CAPSLOCK => KeyKind::Named("caps_lock"),
        Key::KEY_SCROLLLOCK => KeyKind::Named("scroll_lock"),
        Key::KEY_NUMLOCK => KeyKind::Named("num_lock"),
        Key::KEY_SYSRQ => KeyKind::Named("print_screen"),
        Key::KEY_PAUSE => KeyKind::Named("pause"),
        Key::KEY_MENU => KeyKind::Named("context_menu"),
        Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => KeyKind::Named("shift"),
        Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => KeyKind::Named("control"),
        Key::KEY_LEFTALT => KeyKind::Named("alt"),
        Key::KEY_RIGHTALT => KeyKind::Named("altgraph"),
        Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => KeyKind::Named("super"),
        Key::KEY_F1 => KeyKind::Named("f1"),
        Key::KEY_F2 => KeyKind::Named("f2"),
        Key::KEY_F3 => KeyKind::Named("f3"),
        Key::KEY_F4 => KeyKind::Named("f4"),
        Key::KEY_F5 => KeyKind::Named("f5"),
        Key::KEY_F6 => KeyKind::Named("f6"),
        Key::KEY_F7 => KeyKind::Named("f7"),
        Key::KEY_F8 => KeyKind::Named("f8"),
        Key::KEY_F9 => KeyKind::Named("f9"),
        Key::KEY_F10 => KeyKind::Named("f10"),
        Key::KEY_F11 => KeyKind::Named("f11"),
        Key::KEY_F12 => KeyKind::Named("f12"),
        Key::KEY_F13 => KeyKind::Named("f13"),
        Key::KEY_F14 => KeyKind::Named("f14"),
        Key::KEY_F15 => KeyKind::Named("f15"),
        Key::KEY_F16 => KeyKind::Named("f16"),
        Key::KEY_F17 => KeyKind::Named("f17"),
        Key::KEY_F18 => KeyKind::Named("f18"),
        Key::KEY_F19 => KeyKind::Named("f19"),
        Key::KEY_F20 => KeyKind::Named("f20"),
        Key::KEY_F21 => KeyKind::Named("f21"),
        Key::KEY_F22 => KeyKind::Named("f22"),
        Key::KEY_F23 => KeyKind::Named("f23"),
        Key::KEY_F24 => KeyKind::Named("f24"),
        Key::KEY_MINUS => KeyKind::Character('-'),
        Key::KEY_EQUAL => KeyKind::Character('='),
        Key::KEY_LEFTBRACE => KeyKind::Character('['),
        Key::KEY_RIGHTBRACE => KeyKind::Character(']'),
        Key::KEY_BACKSLASH => KeyKind::Character('\\'),
        Key::KEY_SEMICOLON => KeyKind::Character(';'),
        Key::KEY_APOSTROPHE => KeyKind::Character('\''),
        Key::KEY_GRAVE => KeyKind::Character('`'),
        Key::KEY_COMMA => KeyKind::Character(','),
        Key::KEY_DOT => KeyKind::Character('.'),
        Key::KEY_SLASH => KeyKind::Character('/'),
        Key::KEY_KP0 => KeyKind::Character('0'),
        Key::KEY_KP1 => KeyKind::Character('1'),
        Key::KEY_KP2 => KeyKind::Character('2'),
        Key::KEY_KP3 => KeyKind::Character('3'),
        Key::KEY_KP4 => KeyKind::Character('4'),
        Key::KEY_KP5 => KeyKind::Character('5'),
        Key::KEY_KP6 => KeyKind::Character('6'),
        Key::KEY_KP7 => KeyKind::Character('7'),
        Key::KEY_KP8 => KeyKind::Character('8'),
        Key::KEY_KP9 => KeyKind::Character('9'),
        Key::KEY_KPDOT => KeyKind::Character('.'),
        Key::KEY_KPSLASH => KeyKind::Character('/'),
        Key::KEY_KPASTERISK => KeyKind::Character('*'),
        Key::KEY_KPMINUS => KeyKind::Character('-'),
        Key::KEY_KPPLUS => KeyKind::Character('+'),
        Key::KEY_KPEQUAL => KeyKind::Character('='),
        Key::KEY_KPENTER => KeyKind::Named("enter"),
        _ => return None,
    };

    Some(kind)
}

fn key_kind_to_name(key: KeyKind) -> String {
    match key {
        KeyKind::Character(' ') => "space".to_string(),
        KeyKind::Character(ch) => ch.to_string(),
        KeyKind::Named(name) => name.to_string(),
    }
}

fn evdev_key_to_button(key: Key) -> Option<&'static str> {
    match key {
        Key::BTN_LEFT => Some("left"),
        Key::BTN_RIGHT => Some("right"),
        Key::BTN_MIDDLE => Some("middle"),
        Key::BTN_BACK => Some("back"),
        Key::BTN_FORWARD => Some("forward"),
        _ => None,
    }
}

fn key_to_codepoint(key: KeyKind, mods: Modifiers, caps_lock: bool) -> Option<char> {
    let shift = mods.shift;
    let uppercase = shift ^ caps_lock;
    match key {
        KeyKind::Character(ch) => Some(match ch {
            'a'..='z' => {
                if uppercase {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                }
            }
            '0'..='9' => shift_digit(ch, shift)?,
            '-' => {
                if shift {
                    '_'
                } else {
                    '-'
                }
            }
            '=' => {
                if shift {
                    '+'
                } else {
                    '='
                }
            }
            '[' => {
                if shift {
                    '{'
                } else {
                    '['
                }
            }
            ']' => {
                if shift {
                    '}'
                } else {
                    ']'
                }
            }
            '\\' => {
                if shift {
                    '|'
                } else {
                    '\\'
                }
            }
            ';' => {
                if shift {
                    ':'
                } else {
                    ';'
                }
            }
            '\'' => {
                if shift {
                    '"'
                } else {
                    '\''
                }
            }
            '`' => {
                if shift {
                    '~'
                } else {
                    '`'
                }
            }
            ',' => {
                if shift {
                    '<'
                } else {
                    ','
                }
            }
            '.' => {
                if shift {
                    '>'
                } else {
                    '.'
                }
            }
            '/' => {
                if shift {
                    '?'
                } else {
                    '/'
                }
            }
            ' ' => ' ',
            _ => return None,
        }),
        KeyKind::Named(_) => None,
    }
}

fn shift_digit(ch: char, shift: bool) -> Option<char> {
    if !shift {
        return Some(ch);
    }
    Some(match ch {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        _ => return None,
    })
}
