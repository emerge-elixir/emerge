//! DRM input uses a statically bundled libinput path context. No udev daemon.
use crate::{
    actors::EventMsg,
    backend::wake::BackendWakeHandle,
    cursor::{CursorState, SharedCursorState},
    input::{
        ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
        TouchPhase,
    },
    keys::CanonicalKey,
    linux_wait::{EventFd, poll_fds},
};
use crossbeam_channel::{Receiver, SendTimeoutError, Sender, TrySendError};
use emerge_input::{Context, Event};
use evdev::KeyCode as Key;
use std::{
    collections::HashMap,
    fs,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default)]
struct Modifiers {
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrintableText {
    Letter(char),
    ShiftPair { base: char, shifted: char },
    Literal(char),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyKind {
    Printable {
        key: CanonicalKey,
        text: PrintableText,
    },
    Key(CanonicalKey),
}

#[derive(Default)]
struct Contacts {
    next_id: u64,
    active: HashMap<(u64, i32), (u64, (f32, f32))>,
}
impl Contacts {
    fn event(&mut self, event: Event) -> Vec<InputEvent> {
        let slot = (event.device, event.slot);
        let position = (event.x as f32, event.y as f32);
        let touch = |id, phase, (x, y)| InputEvent::Touch { id, phase, x, y };
        match event.kind {
            8 => {
                self.next_id = self.next_id.wrapping_add(1).max(1);
                let id = self.next_id;
                self.active
                    .insert(slot, (id, position))
                    .map(|(old, pos)| touch(old, TouchPhase::Cancel, pos))
                    .into_iter()
                    .chain(Some(touch(id, TouchPhase::Down, position)))
                    .collect()
            }
            9 => self
                .active
                .get_mut(&slot)
                .map(|(id, pos)| {
                    *pos = position;
                    touch(*id, TouchPhase::Move, position)
                })
                .into_iter()
                .collect(),
            10 | 11 => self
                .active
                .remove(&slot)
                .map(|(id, pos)| {
                    touch(
                        id,
                        if event.kind == 10 {
                            TouchPhase::Up
                        } else {
                            TouchPhase::Cancel
                        },
                        pos,
                    )
                })
                .into_iter()
                .collect(),
            1 => {
                let mut removed: Vec<_> = self
                    .active
                    .iter()
                    .filter(|((device, _), _)| *device == event.device)
                    .map(|(slot, _)| *slot)
                    .collect();
                removed.sort_unstable();
                removed
                    .into_iter()
                    .filter_map(|slot| {
                        self.active
                            .remove(&slot)
                            .map(|(id, pos)| touch(id, TouchPhase::Cancel, pos))
                    })
                    .collect()
            }
            _ => vec![],
        }
    }
}

struct KeyboardRepeat {
    device: u64,
    key: Key,
    deadline: Instant,
}

pub struct DrmInput {
    context: Option<Context>,
    devices: HashMap<PathBuf, (u64, u64)>,
    discovery: Option<OwnedFd>,
    contacts: Contacts,
    cursor_pos: (f32, f32),
    modifiers: Modifiers,
    caps_lock: bool,
    repeat_key: Option<KeyboardRepeat>,
    pressed_keys: std::collections::HashSet<(u64, u16)>,
    screen_size: (u32, u32),
    screen_rx: Receiver<(u32, u32)>,
    event_tx: Sender<EventMsg>,
    cursor_state: Arc<SharedCursorState>,
    stop: Arc<AtomicBool>,
    backend_wake: BackendWakeHandle,
    input_wake: EventFd,
    pending_cursor_pos: Option<(f32, f32)>,
    pending_pointer_buttons: Vec<(String, u8, u8)>,
    next_rescan_at: Instant,
    rescan_interval: Duration,
    log_enabled: bool,
}
impl DrmInput {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        screen_size: (u32, u32),
        screen_rx: Receiver<(u32, u32)>,
        event_tx: Sender<EventMsg>,
        cursor_state: Arc<SharedCursorState>,
        stop: Arc<AtomicBool>,
        backend_wake: BackendWakeHandle,
        input_wake: EventFd,
        log_enabled: bool,
    ) -> Self {
        let context = Context::new()
            .map_err(|err| eprintln!("drm_input: cannot initialize bundled libinput: {err}"))
            .ok();
        let mut input = Self {
            context,
            devices: HashMap::new(),
            discovery: None,
            contacts: Contacts::default(),
            cursor_pos: (0.0, 0.0),
            modifiers: Modifiers::default(),
            caps_lock: false,
            repeat_key: None,
            pressed_keys: Default::default(),
            screen_size,
            screen_rx,
            event_tx,
            cursor_state,
            stop,
            backend_wake,
            input_wake,
            pending_cursor_pos: None,
            pending_pointer_buttons: vec![],
            next_rescan_at: Instant::now(),
            rescan_interval: Duration::from_millis(500),
            log_enabled,
        };
        if let Some(ctx) = input.context.as_mut() {
            ctx.set_logging(log_enabled);
        }
        input.rescan_devices();
        input
    }
    pub fn run(&mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            let mut fds: Vec<_> = self
                .context
                .iter()
                .map(AsRawFd::as_raw_fd)
                .chain(self.discovery.iter().map(AsRawFd::as_raw_fd))
                .chain(Some(self.input_wake.as_raw_fd()))
                .map(|fd| libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let deadline = self.repeat_key.as_ref().map_or(self.next_rescan_at, |key| {
                key.deadline.min(self.next_rescan_at)
            });
            let timeout = deadline.saturating_duration_since(Instant::now());
            if let Err(err) = poll_fds(&mut fds, Some(timeout))
                && self.log_enabled
            {
                eprintln!("drm_input poll: {err}");
            }
            if fds.last().is_some_and(|fd| fd.revents & libc::POLLIN != 0) {
                let _ = self.input_wake.drain();
            }
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            if self.discovery.as_ref().is_some_and(drain_discovery) {
                self.rescan_devices();
            }
            self.poll();
        }
    }
    pub fn poll(&mut self) {
        while let Ok(size) = self.screen_rx.try_recv() {
            self.screen_size = size;
        }
        if Instant::now() >= self.next_rescan_at {
            self.rescan_devices();
        }
        if !self.backend_wake.input_enabled() {
            self.modifiers = Modifiers::default();
            self.pressed_keys.clear();
            self.contacts.active.clear();
            self.pending_cursor_pos = None;
            self.pending_pointer_buttons.clear();
        }
        if let Some(ctx) = self.context.as_mut()
            && let Err(err) = ctx.dispatch()
            && self.log_enabled
        {
            eprintln!("drm_input dispatch: {err}");
        }
        while !self.stop.load(Ordering::Relaxed) {
            let Some(event) = self
                .context
                .as_mut()
                .and_then(|ctx| ctx.next_event(self.screen_size))
            else {
                break;
            };
            if !self.backend_wake.input_enabled() {
                if event.kind == 1 {
                    self.handle_event(event);
                }
                self.modifiers = Modifiers::default();
                self.pressed_keys.clear();
                self.contacts.active.clear();
                self.pending_cursor_pos = None;
                self.pending_pointer_buttons.clear();
                continue;
            }
            self.handle_event(event);
        }
        self.repeat_keyboard(Instant::now());
        self.flush_pending_cursor_pos_nonblocking();
    }
    fn repeat_keyboard(&mut self, now: Instant) {
        if !self.backend_wake.input_enabled() {
            self.repeat_key = None;
            return;
        }
        if let Some(repeat) = self.repeat_key.as_mut()
            && repeat.deadline <= now
        {
            repeat.deadline = now + Duration::from_millis(40);
            let key = repeat.key;
            self.handle_key_event(key, 2);
        }
    }
    fn rescan_devices(&mut self) {
        self.next_rescan_at = Instant::now() + self.rescan_interval;
        if self.context.is_none() {
            self.context = Context::new().ok();
            if let Some(ctx) = self.context.as_mut() {
                ctx.set_logging(self.log_enabled);
            }
        }
        let paths = list_event_device_paths();
        let removed: Vec<_> = self
            .devices
            .iter()
            .filter(|(path, identity)| device_identity(path) != Some(**identity))
            .map(|(path, _)| path.clone())
            .collect();
        if let Some(ctx) = self.context.as_mut() {
            for path in removed {
                ctx.remove(&path);
                self.devices.remove(&path);
            }
            // Removal events must precede new devices at reused paths/addresses.
            let _ = ctx.dispatch();
        }
        while let Some(event) = self
            .context
            .as_mut()
            .and_then(|ctx| ctx.next_event(self.screen_size))
        {
            if self.backend_wake.input_enabled() || event.kind == 1 {
                self.handle_event(event);
            }
        }
        if let Some(ctx) = self.context.as_mut() {
            for path in paths {
                if !self.devices.contains_key(&path)
                    && let Some(identity) = device_identity(&path)
                    && ctx.add(&path)
                {
                    self.devices.insert(path, identity);
                }
            }
        }
        // Recreate the watch as well: /dev/input can appear late or be replaced.
        self.discovery = discovery_watch();
    }
    fn handle_event(&mut self, mut event: Event) {
        // Never pass non-finite floats to BEAM encoders. Malformed touch
        // coordinates cancel only that contact rather than committing a tap.
        if !(event.x as f32).is_finite() || !(event.y as f32).is_finite() {
            if matches!(event.kind, 8 | 9) {
                event.kind = 11;
                event.x = 0.0;
                event.y = 0.0;
            } else {
                return;
            }
        }
        match event.kind {
            1 => {
                if self
                    .repeat_key
                    .as_ref()
                    .is_some_and(|key| key.device == event.device)
                {
                    self.repeat_key = None;
                }
                if let Some(ctx) = self.context.as_mut() {
                    for path in ctx.removed_device(event.device) {
                        self.devices.remove(&path);
                    }
                }
                self.pressed_keys
                    .retain(|(device, _)| *device != event.device);
                self.recompute_modifiers();
                for contact in self.contacts.event(event) {
                    self.push_input_blocking(contact);
                }
            }
            2 => {
                let key = Key::new(event.code as u16);
                if event.state != 0 && repeatable_key(key) {
                    self.repeat_key = Some(KeyboardRepeat {
                        device: event.device,
                        key,
                        deadline: Instant::now() + Duration::from_millis(400),
                    });
                } else if event.state == 0
                    && self
                        .repeat_key
                        .as_ref()
                        .is_some_and(|repeat| repeat.device == event.device && repeat.key == key)
                {
                    self.repeat_key = None;
                }
                if event.state != 0 {
                    self.pressed_keys.insert((event.device, event.code as u16));
                } else {
                    self.pressed_keys.remove(&(event.device, event.code as u16));
                }
                self.recompute_modifiers();
                self.handle_key_event(Key::new(event.code as u16), event.state as i32);
            }
            3 => self.handle_abs_relative(event.x as f32, event.y as f32, self.screen_size),
            4 => self.handle_abs_position(event.x as f32, event.y as f32, true),
            5 => {
                self.handle_key_event(Key::new(event.code as u16), event.state as i32);
                self.flush_pending_pointer_buttons_blocking();
            }
            6 | 7 => {
                self.set_cursor_visible(true);
                self.flush_pending_cursor_pos_blocking();
                let (x, y) = self.cursor_pos;
                let (dx, dy) = (-event.x as f32, -event.y as f32);
                self.push_input_blocking(if event.kind == 6 {
                    InputEvent::CursorScrollLines { dx, dy, x, y }
                } else {
                    InputEvent::CursorScroll { dx, dy, x, y }
                });
            }
            8..=11 => {
                event.x = event.x.clamp(0.0, self.screen_size.0.max(1) as f64 - 1.0);
                event.y = event.y.clamp(0.0, self.screen_size.1.max(1) as f64 - 1.0);
                self.set_cursor_visible(false);
                self.flush_pending_cursor_pos_blocking();
                for contact in self.contacts.event(event) {
                    self.push_input_blocking(contact);
                }
            }
            _ => {}
        }
    }
    fn recompute_modifiers(&mut self) {
        let held = |left: Key, right: Key| {
            self.pressed_keys
                .iter()
                .any(|(_, code)| *code == left.code() || *code == right.code())
        };
        self.modifiers = Modifiers {
            shift: held(Key::KEY_LEFTSHIFT, Key::KEY_RIGHTSHIFT),
            ctrl: held(Key::KEY_LEFTCTRL, Key::KEY_RIGHTCTRL),
            alt: held(Key::KEY_LEFTALT, Key::KEY_RIGHTALT),
            meta: held(Key::KEY_LEFTMETA, Key::KEY_RIGHTMETA),
        };
    }
    fn handle_abs_position(&mut self, x: f32, y: f32, visible: bool) {
        let x = x.clamp(0.0, self.screen_size.0.saturating_sub(1) as f32);
        let y = y.clamp(0.0, self.screen_size.1.saturating_sub(1) as f32);
        self.set_cursor_state(x, y, visible);
        self.pending_cursor_pos = Some((x, y));
    }
    fn handle_abs_relative(&mut self, dx: f32, dy: f32, _size: (u32, u32)) {
        self.handle_abs_position(self.cursor_pos.0 + dx, self.cursor_pos.1 + dy, true);
    }
    fn set_cursor_state(&mut self, x: f32, y: f32, visible: bool) {
        let previous = self.cursor_state.snapshot();
        self.cursor_pos = (x, y);
        let snapshot = self.cursor_state.update(CursorState {
            pos: (x, y),
            visible,
        });
        if snapshot.version != previous.version {
            self.backend_wake.request_redraw();
        }
    }
    fn set_cursor_visible(&mut self, visible: bool) {
        self.set_cursor_state(self.cursor_pos.0, self.cursor_pos.1, visible);
    }
    fn flush_pending_cursor_pos_nonblocking(&mut self) {
        let Some((x, y)) = self.pending_cursor_pos else {
            return;
        };
        match self.try_push_input(InputEvent::CursorPos { x, y }) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => self.pending_cursor_pos = None,
            Err(TrySendError::Full(_)) => {}
        }
    }
    fn flush_pending_cursor_pos_blocking(&mut self) {
        if let Some((x, y)) = self.pending_cursor_pos.take() {
            self.push_input_blocking(InputEvent::CursorPos { x, y });
        }
    }
    fn flush_pending_pointer_buttons_blocking(&mut self) {
        self.flush_pending_cursor_pos_blocking();
        let (x, y) = self.cursor_pos;
        for (button, action, mods) in std::mem::take(&mut self.pending_pointer_buttons) {
            self.push_input_blocking(InputEvent::CursorButton {
                button,
                action,
                mods,
                x,
                y,
            });
        }
    }
    fn handle_key_event(&mut self, key: Key, value: i32) {
        let pressed = value != 0;
        if key == Key::KEY_CAPSLOCK && pressed {
            self.caps_lock = !self.caps_lock;
        }

        if let Some(button) = evdev_key_to_button(key) {
            self.set_cursor_visible(true);
            let action = if pressed {
                ACTION_PRESS
            } else {
                ACTION_RELEASE
            };
            let mods = modifiers_to_mask(self.modifiers);
            self.pending_pointer_buttons
                .push((button.to_string(), action, mods));
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
        self.push_input_blocking(InputEvent::Key {
            key: key_kind_to_canonical_key(key_kind),
            action,
            mods,
        });

        if pressed
            && let Some(codepoint) = key_to_codepoint(key_kind, self.modifiers, self.caps_lock)
        {
            self.push_input_blocking(InputEvent::TextCommit {
                text: codepoint.to_string(),
                mods,
            });
        }
    }

    fn push_input_blocking(&self, event: InputEvent) {
        if !self.backend_wake.input_enabled() {
            return;
        }
        let mut msg = EventMsg::InputEvent(event);
        loop {
            match self.event_tx.try_send(msg) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(returned_msg)) => {
                    msg = returned_msg;
                }
            }

            if self.stop.load(Ordering::Relaxed) || !self.backend_wake.input_enabled() {
                break;
            }

            if self.log_enabled {
                eprintln!("event channel full, waiting to send");
            }

            match self.event_tx.send_timeout(msg, Duration::from_millis(10)) {
                Ok(()) => break,
                Err(SendTimeoutError::Timeout(returned_msg)) => {
                    msg = returned_msg;
                }
                Err(SendTimeoutError::Disconnected(_)) => break,
            }
        }
    }

    #[allow(clippy::result_large_err)]
    fn try_push_input(&self, event: InputEvent) -> Result<(), TrySendError<EventMsg>> {
        if !self.backend_wake.input_enabled() {
            return Ok(());
        }
        if self.log_enabled
            && let InputEvent::CursorPos { x, y } = &event
        {
            eprintln!("drm_input enqueue cursor_pos x={x:.2} y={y:.2}");
        }

        let msg = EventMsg::InputEvent(event);
        self.event_tx.try_send(msg)
    }
}

fn device_identity(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    use std::os::unix::fs::FileTypeExt;
    metadata
        .file_type()
        .is_char_device()
        .then_some((metadata.ino(), metadata.rdev()))
}
fn list_event_device_paths() -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir("/dev/input")
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("event"))
                .is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                })
        })
        .collect();
    paths.sort();
    paths
}
fn discovery_watch() -> Option<OwnedFd> {
    // SAFETY: owned nonblocking FD; path is a static terminated string.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mask = libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_ATTRIB
        | libc::IN_DELETE_SELF
        | libc::IN_MOVE_SELF;
    if unsafe { libc::inotify_add_watch(fd.as_raw_fd(), c"/dev/input".as_ptr(), mask) } < 0 {
        return None;
    }
    Some(fd)
}
fn drain_discovery(fd: &OwnedFd) -> bool {
    let mut bytes = [0_u8; 4096];
    let mut changed = false;
    // SAFETY: writable buffer covers the supplied size; read is nonblocking.
    while unsafe { libc::read(fd.as_raw_fd(), bytes.as_mut_ptr().cast(), bytes.len()) } > 0 {
        changed = true;
    }
    changed
}
fn repeatable_key(key: Key) -> bool {
    evdev_key_to_kind(key).is_some_and(|kind| {
        !matches!(
            key_kind_to_canonical_key(kind),
            CanonicalKey::Shift
                | CanonicalKey::Control
                | CanonicalKey::Alt
                | CanonicalKey::AltGraph
                | CanonicalKey::Super
                | CanonicalKey::CapsLock
                | CanonicalKey::NumLock
                | CanonicalKey::ScrollLock
                | CanonicalKey::PrintScreen
                | CanonicalKey::Pause
        )
    })
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

fn printable_letter(key: CanonicalKey, ch: char) -> KeyKind {
    KeyKind::Printable {
        key,
        text: PrintableText::Letter(ch),
    }
}

fn printable_shift_pair(key: CanonicalKey, base: char, shifted: char) -> KeyKind {
    KeyKind::Printable {
        key,
        text: PrintableText::ShiftPair { base, shifted },
    }
}

fn printable_literal(key: CanonicalKey, ch: char) -> KeyKind {
    KeyKind::Printable {
        key,
        text: PrintableText::Literal(ch),
    }
}

fn evdev_key_to_kind(key: Key) -> Option<KeyKind> {
    let kind = match key {
        Key::KEY_A => printable_letter(CanonicalKey::A, 'a'),
        Key::KEY_B => printable_letter(CanonicalKey::B, 'b'),
        Key::KEY_C => printable_letter(CanonicalKey::C, 'c'),
        Key::KEY_D => printable_letter(CanonicalKey::D, 'd'),
        Key::KEY_E => printable_letter(CanonicalKey::E, 'e'),
        Key::KEY_F => printable_letter(CanonicalKey::F, 'f'),
        Key::KEY_G => printable_letter(CanonicalKey::G, 'g'),
        Key::KEY_H => printable_letter(CanonicalKey::H, 'h'),
        Key::KEY_I => printable_letter(CanonicalKey::I, 'i'),
        Key::KEY_J => printable_letter(CanonicalKey::J, 'j'),
        Key::KEY_K => printable_letter(CanonicalKey::K, 'k'),
        Key::KEY_L => printable_letter(CanonicalKey::L, 'l'),
        Key::KEY_M => printable_letter(CanonicalKey::M, 'm'),
        Key::KEY_N => printable_letter(CanonicalKey::N, 'n'),
        Key::KEY_O => printable_letter(CanonicalKey::O, 'o'),
        Key::KEY_P => printable_letter(CanonicalKey::P, 'p'),
        Key::KEY_Q => printable_letter(CanonicalKey::Q, 'q'),
        Key::KEY_R => printable_letter(CanonicalKey::R, 'r'),
        Key::KEY_S => printable_letter(CanonicalKey::S, 's'),
        Key::KEY_T => printable_letter(CanonicalKey::T, 't'),
        Key::KEY_U => printable_letter(CanonicalKey::U, 'u'),
        Key::KEY_V => printable_letter(CanonicalKey::V, 'v'),
        Key::KEY_W => printable_letter(CanonicalKey::W, 'w'),
        Key::KEY_X => printable_letter(CanonicalKey::X, 'x'),
        Key::KEY_Y => printable_letter(CanonicalKey::Y, 'y'),
        Key::KEY_Z => printable_letter(CanonicalKey::Z, 'z'),
        Key::KEY_0 => printable_shift_pair(CanonicalKey::Digit0, '0', ')'),
        Key::KEY_1 => printable_shift_pair(CanonicalKey::Digit1, '1', '!'),
        Key::KEY_2 => printable_shift_pair(CanonicalKey::Digit2, '2', '@'),
        Key::KEY_3 => printable_shift_pair(CanonicalKey::Digit3, '3', '#'),
        Key::KEY_4 => printable_shift_pair(CanonicalKey::Digit4, '4', '$'),
        Key::KEY_5 => printable_shift_pair(CanonicalKey::Digit5, '5', '%'),
        Key::KEY_6 => printable_shift_pair(CanonicalKey::Digit6, '6', '^'),
        Key::KEY_7 => printable_shift_pair(CanonicalKey::Digit7, '7', '&'),
        Key::KEY_8 => printable_shift_pair(CanonicalKey::Digit8, '8', '*'),
        Key::KEY_9 => printable_shift_pair(CanonicalKey::Digit9, '9', '('),
        Key::KEY_SPACE => printable_literal(CanonicalKey::Space, ' '),
        Key::KEY_ENTER => KeyKind::Key(CanonicalKey::Enter),
        Key::KEY_TAB => KeyKind::Key(CanonicalKey::Tab),
        Key::KEY_ESC => KeyKind::Key(CanonicalKey::Escape),
        Key::KEY_BACKSPACE => KeyKind::Key(CanonicalKey::Backspace),
        Key::KEY_INSERT => KeyKind::Key(CanonicalKey::Insert),
        Key::KEY_DELETE => KeyKind::Key(CanonicalKey::Delete),
        Key::KEY_LEFT => KeyKind::Key(CanonicalKey::ArrowLeft),
        Key::KEY_RIGHT => KeyKind::Key(CanonicalKey::ArrowRight),
        Key::KEY_UP => KeyKind::Key(CanonicalKey::ArrowUp),
        Key::KEY_DOWN => KeyKind::Key(CanonicalKey::ArrowDown),
        Key::KEY_PAGEUP => KeyKind::Key(CanonicalKey::PageUp),
        Key::KEY_PAGEDOWN => KeyKind::Key(CanonicalKey::PageDown),
        Key::KEY_HOME => KeyKind::Key(CanonicalKey::Home),
        Key::KEY_END => KeyKind::Key(CanonicalKey::End),
        Key::KEY_CAPSLOCK => KeyKind::Key(CanonicalKey::CapsLock),
        Key::KEY_SCROLLLOCK => KeyKind::Key(CanonicalKey::ScrollLock),
        Key::KEY_NUMLOCK => KeyKind::Key(CanonicalKey::NumLock),
        Key::KEY_SYSRQ => KeyKind::Key(CanonicalKey::PrintScreen),
        Key::KEY_PAUSE => KeyKind::Key(CanonicalKey::Pause),
        Key::KEY_MENU => KeyKind::Key(CanonicalKey::ContextMenu),
        Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => KeyKind::Key(CanonicalKey::Shift),
        Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => KeyKind::Key(CanonicalKey::Control),
        Key::KEY_LEFTALT => KeyKind::Key(CanonicalKey::Alt),
        Key::KEY_RIGHTALT => KeyKind::Key(CanonicalKey::AltGraph),
        Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => KeyKind::Key(CanonicalKey::Super),
        Key::KEY_F1 => KeyKind::Key(CanonicalKey::F1),
        Key::KEY_F2 => KeyKind::Key(CanonicalKey::F2),
        Key::KEY_F3 => KeyKind::Key(CanonicalKey::F3),
        Key::KEY_F4 => KeyKind::Key(CanonicalKey::F4),
        Key::KEY_F5 => KeyKind::Key(CanonicalKey::F5),
        Key::KEY_F6 => KeyKind::Key(CanonicalKey::F6),
        Key::KEY_F7 => KeyKind::Key(CanonicalKey::F7),
        Key::KEY_F8 => KeyKind::Key(CanonicalKey::F8),
        Key::KEY_F9 => KeyKind::Key(CanonicalKey::F9),
        Key::KEY_F10 => KeyKind::Key(CanonicalKey::F10),
        Key::KEY_F11 => KeyKind::Key(CanonicalKey::F11),
        Key::KEY_F12 => KeyKind::Key(CanonicalKey::F12),
        Key::KEY_F13 => KeyKind::Key(CanonicalKey::F13),
        Key::KEY_F14 => KeyKind::Key(CanonicalKey::F14),
        Key::KEY_F15 => KeyKind::Key(CanonicalKey::F15),
        Key::KEY_F16 => KeyKind::Key(CanonicalKey::F16),
        Key::KEY_F17 => KeyKind::Key(CanonicalKey::F17),
        Key::KEY_F18 => KeyKind::Key(CanonicalKey::F18),
        Key::KEY_F19 => KeyKind::Key(CanonicalKey::F19),
        Key::KEY_F20 => KeyKind::Key(CanonicalKey::F20),
        Key::KEY_F21 => KeyKind::Key(CanonicalKey::F21),
        Key::KEY_F22 => KeyKind::Key(CanonicalKey::F22),
        Key::KEY_F23 => KeyKind::Key(CanonicalKey::F23),
        Key::KEY_F24 => KeyKind::Key(CanonicalKey::F24),
        Key::KEY_MINUS => printable_shift_pair(CanonicalKey::Minus, '-', '_'),
        Key::KEY_EQUAL => printable_shift_pair(CanonicalKey::Equal, '=', '+'),
        Key::KEY_LEFTBRACE => printable_shift_pair(CanonicalKey::LeftBracket, '[', '{'),
        Key::KEY_RIGHTBRACE => printable_shift_pair(CanonicalKey::RightBracket, ']', '}'),
        Key::KEY_BACKSLASH => printable_shift_pair(CanonicalKey::Backslash, '\\', '|'),
        Key::KEY_SEMICOLON => printable_shift_pair(CanonicalKey::Semicolon, ';', ':'),
        Key::KEY_APOSTROPHE => printable_shift_pair(CanonicalKey::Apostrophe, '\'', '"'),
        Key::KEY_GRAVE => printable_shift_pair(CanonicalKey::Grave, '`', '~'),
        Key::KEY_COMMA => printable_shift_pair(CanonicalKey::Comma, ',', '<'),
        Key::KEY_DOT => printable_shift_pair(CanonicalKey::Period, '.', '>'),
        Key::KEY_SLASH => printable_shift_pair(CanonicalKey::Slash, '/', '?'),
        Key::KEY_KP0 => printable_literal(CanonicalKey::Digit0, '0'),
        Key::KEY_KP1 => printable_literal(CanonicalKey::Digit1, '1'),
        Key::KEY_KP2 => printable_literal(CanonicalKey::Digit2, '2'),
        Key::KEY_KP3 => printable_literal(CanonicalKey::Digit3, '3'),
        Key::KEY_KP4 => printable_literal(CanonicalKey::Digit4, '4'),
        Key::KEY_KP5 => printable_literal(CanonicalKey::Digit5, '5'),
        Key::KEY_KP6 => printable_literal(CanonicalKey::Digit6, '6'),
        Key::KEY_KP7 => printable_literal(CanonicalKey::Digit7, '7'),
        Key::KEY_KP8 => printable_literal(CanonicalKey::Digit8, '8'),
        Key::KEY_KP9 => printable_literal(CanonicalKey::Digit9, '9'),
        Key::KEY_KPDOT => printable_literal(CanonicalKey::Period, '.'),
        Key::KEY_KPSLASH => printable_literal(CanonicalKey::Slash, '/'),
        Key::KEY_KPASTERISK => printable_literal(CanonicalKey::Asterisk, '*'),
        Key::KEY_KPMINUS => printable_literal(CanonicalKey::Minus, '-'),
        Key::KEY_KPPLUS => printable_literal(CanonicalKey::Plus, '+'),
        Key::KEY_KPEQUAL => printable_literal(CanonicalKey::Equal, '='),
        Key::KEY_KPENTER => KeyKind::Key(CanonicalKey::Enter),
        _ => return None,
    };

    Some(kind)
}

fn key_kind_to_canonical_key(key: KeyKind) -> CanonicalKey {
    match key {
        KeyKind::Printable { key, .. } => key,
        KeyKind::Key(key) => key,
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
    match key {
        KeyKind::Printable { text, .. } => printable_text_to_codepoint(text, mods, caps_lock),
        KeyKind::Key(_) => None,
    }
}

fn printable_text_to_codepoint(
    text: PrintableText,
    mods: Modifiers,
    caps_lock: bool,
) -> Option<char> {
    let shift = mods.shift;
    let uppercase = shift ^ caps_lock;

    Some(match text {
        PrintableText::Letter(ch) => {
            if uppercase {
                ch.to_ascii_uppercase()
            } else {
                ch
            }
        }
        PrintableText::ShiftPair { base, shifted } => {
            if shift {
                shifted
            } else {
                base
            }
        }
        PrintableText::Literal(ch) => ch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;
    fn test_input(capacity: usize) -> (DrmInput, Receiver<EventMsg>, Arc<SharedCursorState>) {
        let (_screen_tx, screen_rx) = bounded(1);
        let (event_tx, event_rx) = bounded(capacity);
        let cursor = Arc::new(SharedCursorState::new(CursorState {
            pos: (0.0, 0.0),
            visible: false,
        }));
        let input = DrmInput {
            context: Some(Context::new().unwrap()),
            devices: HashMap::new(),
            discovery: None,
            contacts: Contacts::default(),
            cursor_pos: (0.0, 0.0),
            modifiers: Modifiers::default(),
            caps_lock: false,
            repeat_key: None,
            pressed_keys: Default::default(),
            screen_size: (640, 480),
            screen_rx,
            event_tx,
            cursor_state: cursor.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            backend_wake: BackendWakeHandle::noop(),
            input_wake: EventFd::new().unwrap(),
            pending_cursor_pos: None,
            pending_pointer_buttons: vec![],
            next_rescan_at: Instant::now() + Duration::from_secs(1),
            rescan_interval: Duration::from_millis(500),
            log_enabled: false,
        };
        (input, event_rx, cursor)
    }
    #[test]
    fn touch_edges_are_clamped_and_invalid_coordinates_cancel_without_activation() {
        let (mut input, rx, _) = test_input(8);
        input.handle_event(Event {
            kind: 8,
            device: 1,
            slot: 0,
            x: 640.0,
            y: 480.0,
            ..Default::default()
        });
        assert!(
            matches!(rx.try_recv().unwrap(), EventMsg::InputEvent(InputEvent::Touch { id: 1, phase: crate::input::TouchPhase::Down, x, y }) if x == 639.0 && y == 479.0)
        );
        input.handle_event(Event {
            kind: 9,
            device: 1,
            slot: 0,
            x: f64::NAN,
            ..Default::default()
        });
        assert!(
            matches!(rx.try_recv().unwrap(), EventMsg::InputEvent(InputEvent::Touch { id: 1, phase: crate::input::TouchPhase::Cancel, x, y }) if x == 639.0 && y == 479.0)
        );
        input.handle_event(Event {
            kind: 10,
            device: 1,
            slot: 0,
            ..Default::default()
        });
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn keyboard_repeat_preserves_press_actions_and_stops_on_release_or_removal() {
        let (mut input, rx, _) = test_input(16);
        for termination in [0, 1] {
            input.handle_event(Event {
                kind: 2,
                device: 1,
                code: Key::KEY_A.code().into(),
                state: 1,
                ..Default::default()
            });
            assert_eq!(rx.try_iter().count(), 2);
            let deadline = input.repeat_key.as_ref().unwrap().deadline;
            input.repeat_keyboard(deadline);
            assert!(matches!(
                rx.try_recv().unwrap(),
                EventMsg::InputEvent(InputEvent::Key {
                    action: ACTION_PRESS,
                    ..
                })
            ));
            assert!(
                matches!(rx.try_recv().unwrap(), EventMsg::InputEvent(InputEvent::TextCommit { text, .. }) if text == "a")
            );
            input.handle_event(Event {
                kind: if termination == 0 { 2 } else { 1 },
                device: 1,
                code: Key::KEY_A.code().into(),
                state: 0,
                ..Default::default()
            });
            rx.try_iter().for_each(drop);
            assert!(input.repeat_key.is_none());
            input.repeat_keyboard(deadline + Duration::from_secs(1));
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn ordered_input_backpressure_does_not_prevent_owner_shutdown() {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (mut input, _rx, _) = test_input(1);
            input.stop = worker_stop;
            ready_tx.send(()).unwrap();
            // The key fills the channel; its ordered text commit must wait.
            input.handle_event(Event {
                kind: 2,
                device: 1,
                code: Key::KEY_A.code().into(),
                state: 1,
                ..Default::default()
            });
            done_tx.send(()).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        stop.store(true, Ordering::Relaxed);
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("input owner did not stop under backpressure");
        worker.join().unwrap();
    }

    #[test]
    fn poll_flushes_only_latest_pending_cursor_position() {
        let (mut input, event_rx, cursor_state) = test_input(8);

        input.handle_abs_position(10.0, 12.0, true);
        input.handle_abs_position(24.0, 36.0, true);
        input.poll();

        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorPos { x, y }))
                if (x - 24.0).abs() < f32::EPSILON && (y - 36.0).abs() < f32::EPSILON
        ));
        assert!(event_rx.try_recv().is_err());

        let snapshot = cursor_state.snapshot();
        assert_eq!(snapshot.state.pos, (24.0, 36.0));
        assert!(snapshot.state.visible);
    }

    #[test]
    fn cursor_starts_hidden_until_pointer_activity() {
        let (_, _, cursor_state) = test_input(8);

        let snapshot = cursor_state.snapshot();
        assert_eq!(snapshot.state.pos, (0.0, 0.0));
        assert!(!snapshot.state.visible);
    }

    #[test]
    fn cursor_motion_flushes_before_button_event() {
        let (mut input, event_rx, _) = test_input(8);

        input.handle_abs_position(14.0, 18.0, true);
        input.handle_key_event(Key::BTN_LEFT, 1);
        input.flush_pending_pointer_buttons_blocking();

        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorPos { x, y }))
                if (x - 14.0).abs() < f32::EPSILON && (y - 18.0).abs() < f32::EPSILON
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorButton { button, action, x, y, .. }))
                if button == "left"
                    && action == ACTION_PRESS
                    && (x - 14.0).abs() < f32::EPSILON
                    && (y - 18.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn button_press_makes_hidden_cursor_visible() {
        let (mut input, event_rx, cursor_state) = test_input(8);

        input.handle_key_event(Key::BTN_LEFT, 1);
        input.flush_pending_pointer_buttons_blocking();

        let snapshot = cursor_state.snapshot();
        assert_eq!(snapshot.state.pos, (0.0, 0.0));
        assert!(snapshot.state.visible);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorButton { button, action, x, y, .. }))
                if button == "left"
                    && action == ACTION_PRESS
                    && x.abs() < f32::EPSILON
                    && y.abs() < f32::EPSILON
        ));
    }

    #[test]
    fn cursor_motion_flushes_before_scroll_event() {
        let (mut input, event_rx, _) = test_input(8);

        input.handle_abs_position(30.0, 40.0, true);
        input.handle_event(Event {
            kind: 6,
            y: -2.0,
            ..Default::default()
        });

        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorPos { x, y }))
                if (x - 30.0).abs() < f32::EPSILON && (y - 40.0).abs() < f32::EPSILON
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorScrollLines { dx, dy, x, y }))
                if dx.abs() < f32::EPSILON
                    && (dy - 2.0).abs() < f32::EPSILON
                    && (x - 30.0).abs() < f32::EPSILON
                    && (y - 40.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn wheel_scroll_makes_hidden_cursor_visible() {
        let (mut input, event_rx, cursor_state) = test_input(8);

        input.handle_event(Event {
            kind: 6,
            y: -2.0,
            ..Default::default()
        });

        let snapshot = cursor_state.snapshot();
        assert_eq!(snapshot.state.pos, (0.0, 0.0));
        assert!(snapshot.state.visible);
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorScrollLines { dx, dy, x, y }))
                if dx.abs() < f32::EPSILON
                    && (dy - 2.0).abs() < f32::EPSILON
                    && x.abs() < f32::EPSILON
                    && y.abs() < f32::EPSILON
        ));
    }

    #[test]
    fn pointer_button_report_uses_final_cursor_position_when_motion_arrives_later() {
        let (mut input, event_rx, _) = test_input(8);

        input.handle_abs_position(10.0, 12.0, true);
        input.flush_pending_cursor_pos_blocking();
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorPos { x, y }))
                if (x - 10.0).abs() < f32::EPSILON && (y - 12.0).abs() < f32::EPSILON
        ));

        input.handle_key_event(Key::BTN_LEFT, 0);
        input.handle_abs_position(24.0, 36.0, true);
        input.flush_pending_pointer_buttons_blocking();

        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorPos { x, y }))
                if (x - 24.0).abs() < f32::EPSILON && (y - 36.0).abs() < f32::EPSILON
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorButton { button, action, x, y, .. }))
                if button == "left"
                    && action == ACTION_RELEASE
                    && (x - 24.0).abs() < f32::EPSILON
                    && (y - 36.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn direct_touch_position_hides_cursor() {
        let (mut input, _, cursor_state) = test_input(8);

        input.handle_abs_position(12.0, 18.0, true);
        input.handle_abs_position(24.0, 36.0, false);

        let snapshot = cursor_state.snapshot();
        assert_eq!(snapshot.state.pos, (24.0, 36.0));
        assert!(!snapshot.state.visible);
    }

    #[test]
    fn nonblocking_cursor_flush_keeps_latest_pending_position_when_queue_is_full() {
        let (mut input, event_rx, _) = test_input(1);

        input
            .event_tx
            .send(EventMsg::InputEvent(InputEvent::Key {
                key: CanonicalKey::A,
                action: ACTION_PRESS,
                mods: 0,
            }))
            .unwrap();

        input.handle_abs_position(50.0, 60.0, true);
        input.flush_pending_cursor_pos_nonblocking();

        assert_eq!(input.pending_cursor_pos, Some((50.0, 60.0)));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::Key { key, .. })) if key == CanonicalKey::A
        ));

        input.flush_pending_cursor_pos_nonblocking();

        assert!(input.pending_cursor_pos.is_none());
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorPos { x, y }))
                if (x - 50.0).abs() < f32::EPSILON && (y - 60.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn evdev_key_to_kind_exposes_canonical_keys_for_symbols_and_keypad_operators() {
        assert_eq!(
            key_kind_to_canonical_key(evdev_key_to_kind(Key::KEY_EQUAL).expect("equal key")),
            CanonicalKey::Equal
        );
        assert_eq!(
            key_kind_to_canonical_key(evdev_key_to_kind(Key::KEY_8).expect("digit 8 key")),
            CanonicalKey::Digit8
        );
        assert_eq!(
            key_kind_to_canonical_key(evdev_key_to_kind(Key::KEY_KPPLUS).expect("kp plus key")),
            CanonicalKey::Plus
        );
        assert_eq!(
            key_kind_to_canonical_key(
                evdev_key_to_kind(Key::KEY_KPASTERISK).expect("kp asterisk key")
            ),
            CanonicalKey::Asterisk
        );
        assert_eq!(
            key_kind_to_canonical_key(evdev_key_to_kind(Key::KEY_KPDOT).expect("kp dot key")),
            CanonicalKey::Period
        );
    }

    #[test]
    fn key_to_codepoint_preserves_shifted_and_keypad_text_output() {
        let shift_mods = Modifiers {
            shift: true,
            ..Modifiers::default()
        };

        assert_eq!(
            key_to_codepoint(
                evdev_key_to_kind(Key::KEY_EQUAL).expect("equal key"),
                shift_mods,
                false
            ),
            Some('+')
        );
        assert_eq!(
            key_to_codepoint(
                evdev_key_to_kind(Key::KEY_8).expect("digit 8 key"),
                shift_mods,
                false
            ),
            Some('*')
        );
        assert_eq!(
            key_to_codepoint(
                evdev_key_to_kind(Key::KEY_LEFTBRACE).expect("left brace key"),
                shift_mods,
                false
            ),
            Some('{')
        );
        assert_eq!(
            key_to_codepoint(
                evdev_key_to_kind(Key::KEY_KPPLUS).expect("kp plus key"),
                Modifiers::default(),
                false
            ),
            Some('+')
        );
        assert_eq!(
            key_to_codepoint(
                evdev_key_to_kind(Key::KEY_KPASTERISK).expect("kp asterisk key"),
                shift_mods,
                false
            ),
            Some('*')
        );
    }

    #[test]
    fn handle_key_event_emits_canonical_key_and_shifted_text_commit() {
        let (mut input, event_rx, _) = test_input(8);
        input.modifiers.shift = true;

        input.handle_key_event(Key::KEY_EQUAL, 1);

        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::Key { key, action, mods }))
                if key == CanonicalKey::Equal && action == ACTION_PRESS && mods == MOD_SHIFT
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::TextCommit { text, mods }))
                if text == "+" && mods == MOD_SHIFT
        ));
    }

    #[test]
    fn handle_key_event_emits_keypad_plus_text_without_shift() {
        let (mut input, event_rx, _) = test_input(8);

        input.handle_key_event(Key::KEY_KPPLUS, 1);

        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::Key { key, action, mods }))
                if key == CanonicalKey::Plus && action == ACTION_PRESS && mods == 0
        ));
        assert!(matches!(
            event_rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::TextCommit { text, mods }))
                if text == "+" && mods == 0
        ));
    }

    fn contact(device: u64, slot: i32, kind: u32, x: f64, y: f64) -> Event {
        Event {
            device,
            slot,
            kind,
            x,
            y,
            ..Default::default()
        }
    }
    #[test]
    fn overlapping_contact_positions_and_releases_never_share_a_cursor() {
        for reverse in [false, true] {
            let mut contacts = Contacts::default();
            let a = contacts.event(contact(1, 0, 8, 10.0, 20.0));
            let b = contacts.event(contact(1, 1, 8, 80.0, 90.0));
            let InputEvent::Touch { id: a, .. } = a[0] else {
                panic!()
            };
            let InputEvent::Touch { id: b, .. } = b[0] else {
                panic!()
            };
            assert_ne!(a, b);
            assert_eq!(
                contacts.event(contact(1, 1, 9, 85.0, 90.0)),
                [InputEvent::Touch {
                    id: b,
                    phase: TouchPhase::Move,
                    x: 85.0,
                    y: 90.0
                }]
            );
            for slot in if reverse { [1, 0] } else { [0, 1] } {
                let expected = if slot == 0 {
                    InputEvent::Touch {
                        id: a,
                        phase: TouchPhase::Up,
                        x: 10.0,
                        y: 20.0,
                    }
                } else {
                    InputEvent::Touch {
                        id: b,
                        phase: TouchPhase::Up,
                        x: 85.0,
                        y: 90.0,
                    }
                };
                assert_eq!(contacts.event(contact(1, slot, 10, 0.0, 0.0)), [expected]);
            }
            assert!(contacts.active.is_empty());
        }
    }
    #[test]
    fn slot_reuse_cancels_old_identity_and_removal_cancels_only_its_device() {
        let mut contacts = Contacts::default();
        contacts.event(contact(1, 0, 8, 10.0, 20.0));
        contacts.event(contact(2, 0, 8, 30.0, 40.0));
        let reused = contacts.event(contact(1, 0, 8, 50.0, 60.0));
        assert!(
            matches!(reused.as_slice(), [InputEvent::Touch { id: old, phase: TouchPhase::Cancel, .. }, InputEvent::Touch { id: new, phase: TouchPhase::Down, .. }] if old != new)
        );
        assert_eq!(contacts.event(contact(1, 0, 1, 0.0, 0.0)).len(), 1);
        assert_eq!(contacts.active.len(), 1);
        assert!(contacts.event(contact(1, 0, 10, 0.0, 0.0)).is_empty());
        assert!(matches!(
            contacts.event(contact(2, 0, 11, 0.0, 0.0))[0],
            InputEvent::Touch {
                phase: TouchPhase::Cancel,
                x: 30.0,
                y: 40.0,
                ..
            }
        ));
    }
    #[test]
    fn precise_scroll_does_not_apply_wheel_line_scaling() {
        let (mut input, rx, _) = test_input(4);
        input.handle_event(Event {
            kind: 7,
            x: 1.5,
            y: -2.25,
            ..Default::default()
        });
        assert!(matches!(
            rx.try_recv(),
            Ok(EventMsg::InputEvent(InputEvent::CursorScroll {
                dx: -1.5,
                dy: 2.25,
                ..
            }))
        ));
    }
    #[test]
    fn modifier_release_and_removal_preserve_other_devices_and_sides() {
        let (mut input, _, _) = test_input(32);
        for (device, code) in [(1, Key::KEY_LEFTSHIFT), (2, Key::KEY_RIGHTSHIFT)] {
            input.handle_event(Event {
                kind: 2,
                device,
                code: u32::from(code.code()),
                state: 1,
                ..Default::default()
            });
        }
        input.handle_event(Event {
            kind: 1,
            device: 1,
            ..Default::default()
        });
        assert!(input.modifiers.shift);
        input.handle_event(Event {
            kind: 2,
            device: 2,
            code: u32::from(Key::KEY_RIGHTSHIFT.code()),
            state: 0,
            ..Default::default()
        });
        assert!(!input.modifiers.shift);
    }
}
