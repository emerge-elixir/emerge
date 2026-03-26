use evdev::{
    AbsoluteAxisCode as AbsoluteAxisType, Device, EventSummary, KeyCode as Key, PropType,
    RelativeAxisCode as RelativeAxisType, SynchronizationCode as Synchronization,
};
use libc::input_absinfo;
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, SendTimeoutError, Sender, TrySendError};

use crate::actors::EventMsg;
use crate::backend::wake::BackendWakeHandle;
use crate::cursor::{CursorState, SharedCursorState};
use crate::input::{
    ACTION_PRESS, ACTION_RELEASE, InputEvent, MOD_ALT, MOD_CTRL, MOD_META, MOD_SHIFT,
};
use crate::keys::CanonicalKey;
use crate::linux_wait::{EventFd, poll_fds};

struct InputDevice {
    path: PathBuf,
    device: Device,
    abs_x: Option<AbsAxisState>,
    abs_y: Option<AbsAxisState>,
    abs_x_dirty: bool,
    abs_y_dirty: bool,
    abs_mode: AbsMode,
    last_abs_scaled: Option<(f32, f32)>,
    touch_active: bool,
    touch_tracking: bool,
    pending_direct_touch_button: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
struct AbsAxisState {
    value: i32,
    min: i32,
    max: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AbsMode {
    AbsolutePointer,
    RelativeFromAbs,
    DirectTouch,
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
    Key(CanonicalKey),
}

pub struct DrmInput {
    devices: Vec<InputDevice>,
    cursor_pos: (f32, f32),
    modifiers: Modifiers,
    caps_lock: bool,
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
        let devices = enumerate_devices(log_enabled);
        let rescan_interval = Duration::from_millis(500);
        Self {
            devices,
            cursor_pos: (0.0, 0.0),
            modifiers: Modifiers::default(),
            caps_lock: false,
            screen_size,
            screen_rx,
            event_tx,
            cursor_state,
            stop,
            backend_wake,
            input_wake,
            pending_cursor_pos: None,
            pending_pointer_buttons: Vec::new(),
            next_rescan_at: Instant::now() + rescan_interval,
            rescan_interval,
            log_enabled,
        }
    }

    pub fn run(&mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            let mut poll_fds_buf = self.build_pollfds();
            let timeout = Some(self.next_wait_timeout());

            if let Err(err) = poll_fds(&mut poll_fds_buf, timeout) {
                if self.log_enabled {
                    eprintln!("drm_input poll error: {err}");
                }
                self.poll();
                continue;
            }

            if poll_fds_buf
                .last()
                .map(|pollfd| (pollfd.revents & libc::POLLIN) != 0)
                .unwrap_or(false)
            {
                let _ = self.input_wake.drain();
            }

            if self.stop.load(Ordering::Relaxed) {
                break;
            }

            self.poll();
        }
    }

    pub fn poll(&mut self) {
        while let Ok(size) = self.screen_rx.try_recv() {
            self.screen_size = size;
        }

        let now = Instant::now();
        if now >= self.next_rescan_at {
            self.rescan_devices();
            self.next_rescan_at = now + self.rescan_interval;
        }

        let screen_size = self.screen_size;
        let mut idx = 0;
        while idx < self.devices.len() {
            let events = match {
                let device = &mut self.devices[idx];
                match device.device.fetch_events() {
                    Ok(events) => Ok(events.collect::<Vec<_>>()),
                    Err(err) if should_remove_device_on_fetch_error(&err) => Err(err),
                    Err(_) => Ok(Vec::new()),
                }
            } {
                Ok(events) => events,
                Err(err) => {
                    self.remove_device(idx, Some(err));
                    continue;
                }
            };

            for event in events {
                match event.destructure() {
                    EventSummary::Key(_, key, value) => {
                        self.handle_key_event_with_device(idx, key, value);
                    }
                    EventSummary::RelativeAxis(_, axis, value) => {
                        self.handle_rel_event(axis, value, screen_size);
                    }
                    EventSummary::AbsoluteAxis(_, axis, value) => {
                        let device = &mut self.devices[idx];
                        update_abs_state(device, axis, value, screen_size);
                    }
                    EventSummary::Synchronization(_, Synchronization::SYN_REPORT, _) => {
                        let (abs_mode, action, pending_direct_touch_button) = {
                            let device = &mut self.devices[idx];
                            (
                                device.abs_mode,
                                consume_abs_action(device, screen_size),
                                device.pending_direct_touch_button.take(),
                            )
                        };

                        let direct_touch_had_position = matches!(action, AbsAction::Absolute(_, _));

                        match (abs_mode, action) {
                            (AbsMode::AbsolutePointer, AbsAction::Absolute(x, y)) => {
                                self.handle_abs_position(x, y, true)
                            }
                            (AbsMode::RelativeFromAbs, AbsAction::Relative(dx, dy)) => {
                                self.handle_abs_relative(dx, dy, screen_size)
                            }
                            (AbsMode::DirectTouch, AbsAction::Absolute(x, y)) => {
                                self.handle_abs_position(x, y, false)
                            }
                            _ => {}
                        }

                        if abs_mode == AbsMode::DirectTouch {
                            if pending_direct_touch_button.is_some() && !direct_touch_had_position {
                                self.set_cursor_visible(false);
                            }

                            if let Some(action) = pending_direct_touch_button {
                                self.push_left_button(action);
                            }
                        }

                        self.flush_pending_pointer_buttons_blocking();
                    }
                    _ => {}
                }
            }

            idx += 1;
        }

        self.flush_pending_cursor_pos_nonblocking();
    }

    fn rescan_devices(&mut self) {
        let discovered_paths = list_event_device_paths();
        let existing_paths = self
            .devices
            .iter()
            .map(|device| device.path.clone())
            .collect::<Vec<_>>();
        let (removed_paths, added_paths) =
            reconcile_device_paths(&existing_paths, &discovered_paths);
        let removed_set = removed_paths.into_iter().collect::<HashSet<_>>();

        self.devices.retain(|device| {
            let remove = removed_set.contains(&device.path);
            if remove && self.log_enabled {
                eprintln!("drm_input remove device={:?}", device.path);
            }
            !remove
        });

        for path in added_paths {
            if let Some(device) = open_input_device(&path, self.log_enabled, Some("add")) {
                self.devices.push(device);
            }
        }
    }

    fn remove_device(&mut self, idx: usize, error: Option<std::io::Error>) {
        if idx >= self.devices.len() {
            return;
        }

        let device = self.devices.remove(idx);
        if self.log_enabled {
            if let Some(err) = error {
                eprintln!("drm_input remove device={:?} error={err}", device.path);
            } else {
                eprintln!("drm_input remove device={:?}", device.path);
            }
        }
    }

    fn next_wait_timeout(&self) -> Duration {
        self.next_rescan_at
            .saturating_duration_since(Instant::now())
    }

    fn build_pollfds(&self) -> Vec<libc::pollfd> {
        let mut poll_fds_buf = self
            .devices
            .iter()
            .map(|device| libc::pollfd {
                fd: device.device.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect::<Vec<_>>();
        poll_fds_buf.push(libc::pollfd {
            fd: self.input_wake.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        });
        poll_fds_buf
    }

    fn handle_key_event(&mut self, key: Key, value: i32) {
        let pressed = value != 0;
        self.update_modifiers(key, pressed);
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
                self.set_cursor_visible(true);
                let (cx, cy) = self.cursor_pos;
                self.flush_pending_cursor_pos_blocking();
                self.push_input_blocking(InputEvent::CursorScrollLines {
                    dx: 0.0,
                    dy: value as f32,
                    x: cx,
                    y: cy,
                });
                return;
            }
            RelativeAxisType::REL_HWHEEL => {
                self.set_cursor_visible(true);
                let (cx, cy) = self.cursor_pos;
                self.flush_pending_cursor_pos_blocking();
                self.push_input_blocking(InputEvent::CursorScrollLines {
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
        self.set_cursor_state(x, y, true);
        self.queue_cursor_pos(x, y);
    }

    fn handle_abs_position(&mut self, x: f32, y: f32, visible: bool) {
        self.set_cursor_state(x, y, visible);
        self.queue_cursor_pos(x, y);
    }

    fn handle_abs_relative(&mut self, dx: f32, dy: f32, screen_size: (u32, u32)) {
        let (mut x, mut y) = self.cursor_pos;
        x += dx;
        y += dy;
        let (width, height) = screen_size;
        x = x.clamp(0.0, width.saturating_sub(1) as f32);
        y = y.clamp(0.0, height.saturating_sub(1) as f32);
        self.set_cursor_state(x, y, true);
        self.queue_cursor_pos(x, y);
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
        let previous = self.cursor_state.snapshot();
        let snapshot = self.cursor_state.update(CursorState {
            pos: self.cursor_pos,
            visible,
        });
        if snapshot.version != previous.version {
            self.backend_wake.request_redraw();
        }
    }

    fn push_left_button(&mut self, action: u8) {
        self.flush_pending_cursor_pos_blocking();
        let (x, y) = self.cursor_pos;
        self.push_input_blocking(InputEvent::CursorButton {
            button: "left".to_string(),
            action,
            mods: modifiers_to_mask(self.modifiers),
            x,
            y,
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

    fn queue_cursor_pos(&mut self, x: f32, y: f32) {
        self.pending_cursor_pos = Some((x, y));
    }

    fn flush_pending_cursor_pos_nonblocking(&mut self) {
        let Some((x, y)) = self.pending_cursor_pos else {
            return;
        };

        match self.try_push_input(InputEvent::CursorPos { x, y }) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {
                self.pending_cursor_pos = None;
            }
            Err(TrySendError::Full(_)) => {}
        }
    }

    fn flush_pending_cursor_pos_blocking(&mut self) {
        let Some((x, y)) = self.pending_cursor_pos.take() else {
            return;
        };

        self.push_input_blocking(InputEvent::CursorPos { x, y });
    }

    fn flush_pending_pointer_buttons_blocking(&mut self) {
        if self.pending_pointer_buttons.is_empty() {
            return;
        }

        self.flush_pending_cursor_pos_blocking();
        let (x, y) = self.cursor_pos;
        let pending_buttons = std::mem::take(&mut self.pending_pointer_buttons);

        for (button, action, mods) in pending_buttons {
            self.push_input_blocking(InputEvent::CursorButton {
                button,
                action,
                mods,
                x,
                y,
            });
        }
    }

    fn push_input_blocking(&self, event: InputEvent) {
        let mut msg = EventMsg::InputEvent(event);
        loop {
            match self.event_tx.try_send(msg) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(returned_msg)) => {
                    msg = returned_msg;
                }
            }

            if self.stop.load(Ordering::Relaxed) {
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

    fn try_push_input(&self, event: InputEvent) -> Result<(), TrySendError<EventMsg>> {
        if self.log_enabled
            && let InputEvent::CursorPos { x, y } = &event
        {
            eprintln!("drm_input enqueue cursor_pos x={x:.2} y={y:.2}");
        }

        let msg = EventMsg::InputEvent(event);
        self.event_tx.try_send(msg)
    }
}

fn list_event_device_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let entries = match fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(_) => return paths,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if is_event_device(&path) {
            paths.push(path);
        }
    }

    paths.sort();
    paths
}

fn enumerate_devices(log_enabled: bool) -> Vec<InputDevice> {
    list_event_device_paths()
        .into_iter()
        .filter_map(|path| open_input_device(&path, log_enabled, None))
        .collect()
}

fn open_input_device(path: &Path, log_enabled: bool, action: Option<&str>) -> Option<InputDevice> {
    let device = match Device::open(path) {
        Ok(device) => device,
        Err(_) => return None,
    };
    set_non_blocking(device.as_raw_fd());
    let (abs_mode, info) = detect_abs_mode(&device);
    let (abs_x, abs_y) = init_abs_axes(&device);
    if log_enabled {
        let name = device.name().unwrap_or("unknown");
        if let Some(action) = action {
            eprintln!(
                "drm_input {action} device={:?} name=\"{}\" abs_mode={:?} {}",
                path, name, abs_mode, info
            );
        } else {
            eprintln!(
                "drm_input device={:?} name=\"{}\" abs_mode={:?} {}",
                path, name, abs_mode, info
            );
        }
    }

    Some(InputDevice {
        path: path.to_path_buf(),
        device,
        abs_x,
        abs_y,
        abs_x_dirty: false,
        abs_y_dirty: false,
        abs_mode,
        last_abs_scaled: None,
        touch_active: false,
        touch_tracking: false,
        pending_direct_touch_button: None,
    })
}

fn is_event_device(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("event"))
        .unwrap_or(false)
}

fn reconcile_device_paths(
    existing_paths: &[PathBuf],
    discovered_paths: &[PathBuf],
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let existing = existing_paths.iter().cloned().collect::<HashSet<_>>();
    let discovered = discovered_paths.iter().cloned().collect::<HashSet<_>>();

    let removed = existing_paths
        .iter()
        .filter(|path| !discovered.contains(*path))
        .cloned()
        .collect();
    let added = discovered_paths
        .iter()
        .filter(|path| !existing.contains(*path))
        .cloned()
        .collect();

    (removed, added)
}

fn should_remove_device_on_fetch_error(err: &std::io::Error) -> bool {
    !matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
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
        AbsoluteAxisType::ABS_X | AbsoluteAxisType::ABS_MT_POSITION_X => {
            device.abs_x = Some(update_axis_state(device.abs_x, value, fallback.0));
            device.abs_x_dirty = true;
        }
        AbsoluteAxisType::ABS_Y | AbsoluteAxisType::ABS_MT_POSITION_Y => {
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

    let abs_x =
        axis_state_from_abs(abs_state.get(AbsoluteAxisType::ABS_X.0 as usize)).or_else(|| {
            axis_state_from_abs(abs_state.get(AbsoluteAxisType::ABS_MT_POSITION_X.0 as usize))
        });
    let abs_y =
        axis_state_from_abs(abs_state.get(AbsoluteAxisType::ABS_Y.0 as usize)).or_else(|| {
            axis_state_from_abs(abs_state.get(AbsoluteAxisType::ABS_MT_POSITION_Y.0 as usize))
        });
    (abs_x, abs_y)
}

fn axis_state_from_abs(info: Option<&input_absinfo>) -> Option<AbsAxisState> {
    info.map(|info| AbsAxisState {
        value: info.value,
        min: info.minimum,
        max: info.maximum,
    })
}

fn classify_abs_mode(
    direct_prop: bool,
    pointer_prop: bool,
    buttonpad_prop: bool,
    topbuttonpad_prop: bool,
    semi_mt_prop: bool,
    touchpad_key_hint: bool,
    touch_key_hint: bool,
    mt_position_hint: bool,
    name_touchpad_hint: bool,
    name_touch_hint: bool,
) -> AbsMode {
    let touchpad = !direct_prop
        && ((pointer_prop
            && (buttonpad_prop || topbuttonpad_prop || semi_mt_prop || touchpad_key_hint))
            || name_touchpad_hint);

    let direct_touch =
        direct_prop || name_touch_hint || (!touchpad && (mt_position_hint || touch_key_hint));

    if direct_touch {
        AbsMode::DirectTouch
    } else if touchpad {
        AbsMode::RelativeFromAbs
    } else {
        AbsMode::AbsolutePointer
    }
}

fn detect_abs_mode(device: &Device) -> (AbsMode, String) {
    let (has_abs_xy, has_mt_position) = device
        .supported_absolute_axes()
        .map(|axes| {
            (
                axes.contains(AbsoluteAxisType::ABS_X) && axes.contains(AbsoluteAxisType::ABS_Y),
                axes.contains(AbsoluteAxisType::ABS_MT_POSITION_X)
                    && axes.contains(AbsoluteAxisType::ABS_MT_POSITION_Y),
            )
        })
        .unwrap_or((false, false));
    let has_abs = has_abs_xy || has_mt_position;
    if !has_abs {
        return (AbsMode::AbsolutePointer, "abs_axes=none".to_string());
    }

    let props = device.properties();
    let direct_prop = props.contains(PropType::DIRECT);
    let buttonpad_prop = props.contains(PropType::BUTTONPAD);
    let topbuttonpad_prop = props.contains(PropType::TOPBUTTONPAD);
    let semi_mt_prop = props.contains(PropType::SEMI_MT);
    let pointer_prop = props.contains(PropType::POINTER);

    let touchpad_key_hint = device.supported_keys().is_some_and(|keys| {
        keys.contains(Key::BTN_TOOL_FINGER)
            || keys.contains(Key::BTN_TOOL_DOUBLETAP)
            || keys.contains(Key::BTN_TOOL_TRIPLETAP)
            || keys.contains(Key::BTN_TOOL_QUADTAP)
            || keys.contains(Key::BTN_TOOL_QUINTTAP)
    });
    let touch_key_hint = device.supported_keys().is_some_and(|keys| {
        keys.contains(Key::BTN_TOUCH)
            || keys.contains(Key::BTN_TOOL_FINGER)
            || keys.contains(Key::BTN_TOOL_DOUBLETAP)
            || keys.contains(Key::BTN_TOOL_TRIPLETAP)
            || keys.contains(Key::BTN_TOOL_QUADTAP)
            || keys.contains(Key::BTN_TOOL_QUINTTAP)
    });

    let name = device
        .name()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let name_touchpad_hint = name.contains("touchpad");
    let name_touch_hint = !name_touchpad_hint
        && (name.contains("touchscreen")
            || name.contains("touch screen")
            || name.contains("touch")
            || name.contains("waveshare"));

    let info = format!(
        "abs_xy={} mt_xy={} direct={} pointer={} buttonpad={} topbuttonpad={} semi_mt={} touchpad_key={} touch_key={} name_touchpad={} name_touch={}",
        has_abs_xy,
        has_mt_position,
        direct_prop,
        pointer_prop,
        buttonpad_prop,
        topbuttonpad_prop,
        semi_mt_prop,
        touchpad_key_hint,
        touch_key_hint,
        name_touchpad_hint,
        name_touch_hint
    );

    (
        classify_abs_mode(
            direct_prop,
            pointer_prop,
            buttonpad_prop,
            topbuttonpad_prop,
            semi_mt_prop,
            touchpad_key_hint,
            touch_key_hint,
            has_mt_position,
            name_touchpad_hint,
            name_touch_hint,
        ),
        info,
    )
}

fn direct_touch_button_action(abs_mode: AbsMode, key: Key, pressed: bool) -> Option<u8> {
    (abs_mode == AbsMode::DirectTouch && key == Key::BTN_TOUCH).then_some(if pressed {
        ACTION_PRESS
    } else {
        ACTION_RELEASE
    })
}

impl DrmInput {
    fn handle_key_event_with_device(&mut self, idx: usize, key: Key, value: i32) {
        let pressed = value != 0;
        if let Some(device) = self.devices.get_mut(idx) {
            if let Some(action) = direct_touch_button_action(device.abs_mode, key, pressed) {
                device.pending_direct_touch_button = Some(action);
                return;
            }

            if device.abs_mode == AbsMode::RelativeFromAbs && is_touch_tracking_key(key) {
                device.touch_tracking = true;
                device.touch_active = pressed;
                if pressed {
                    device.last_abs_scaled = None;
                }
            }
        }

        self.handle_key_event(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::SharedCursorState;
    use crossbeam_channel::bounded;

    fn test_input(event_capacity: usize) -> (DrmInput, Receiver<EventMsg>, Arc<SharedCursorState>) {
        let (_screen_tx, screen_rx) = bounded(1);
        let (event_tx, event_rx) = bounded(event_capacity);
        let cursor_state = Arc::new(SharedCursorState::new(CursorState {
            pos: (0.0, 0.0),
            visible: false,
        }));

        let input = DrmInput {
            devices: Vec::new(),
            cursor_pos: (0.0, 0.0),
            modifiers: Modifiers::default(),
            caps_lock: false,
            screen_size: (640, 480),
            screen_rx,
            event_tx,
            cursor_state: Arc::clone(&cursor_state),
            stop: Arc::new(AtomicBool::new(false)),
            backend_wake: BackendWakeHandle::noop(),
            input_wake: EventFd::new().expect("eventfd available for tests"),
            pending_cursor_pos: None,
            pending_pointer_buttons: Vec::new(),
            next_rescan_at: Instant::now() + Duration::from_millis(500),
            rescan_interval: Duration::from_millis(500),
            log_enabled: false,
        };

        (input, event_rx, cursor_state)
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
        input.handle_rel_event(RelativeAxisType::REL_WHEEL, 2, (640, 480));

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

        input.handle_rel_event(RelativeAxisType::REL_WHEEL, 2, (640, 480));

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
    fn classify_abs_mode_keeps_touchpad_devices_relative_from_abs() {
        assert_eq!(
            classify_abs_mode(
                false, true, true, false, false, true, true, true, false, false
            ),
            AbsMode::RelativeFromAbs
        );
        assert_eq!(
            classify_abs_mode(
                false, false, false, false, false, false, false, false, true, false
            ),
            AbsMode::RelativeFromAbs
        );
    }

    #[test]
    fn classify_abs_mode_marks_direct_touch_devices_as_direct_touch() {
        assert_eq!(
            classify_abs_mode(
                true, true, false, false, false, false, true, true, false, false
            ),
            AbsMode::DirectTouch
        );
        assert_eq!(
            classify_abs_mode(
                false, true, false, false, false, false, true, true, false, false
            ),
            AbsMode::DirectTouch
        );
        assert_eq!(
            classify_abs_mode(
                false, true, false, false, false, false, true, false, false, true
            ),
            AbsMode::DirectTouch
        );
    }

    #[test]
    fn direct_touch_button_action_only_synthesizes_for_direct_touch_btn_touch() {
        assert_eq!(
            direct_touch_button_action(AbsMode::DirectTouch, Key::BTN_TOUCH, true),
            Some(ACTION_PRESS)
        );
        assert_eq!(
            direct_touch_button_action(AbsMode::DirectTouch, Key::BTN_TOUCH, false),
            Some(ACTION_RELEASE)
        );
        assert_eq!(
            direct_touch_button_action(AbsMode::RelativeFromAbs, Key::BTN_TOUCH, true),
            None
        );
        assert_eq!(
            direct_touch_button_action(AbsMode::DirectTouch, Key::BTN_TOOL_FINGER, true),
            None
        );
    }

    #[test]
    fn reconcile_device_paths_reports_removed_and_added_paths() {
        let existing = vec![
            PathBuf::from("/dev/input/event0"),
            PathBuf::from("/dev/input/event1"),
        ];
        let discovered = vec![
            PathBuf::from("/dev/input/event1"),
            PathBuf::from("/dev/input/event2"),
        ];

        let (removed, added) = reconcile_device_paths(&existing, &discovered);

        assert_eq!(removed, vec![PathBuf::from("/dev/input/event0")]);
        assert_eq!(added, vec![PathBuf::from("/dev/input/event2")]);
    }

    #[test]
    fn reconcile_device_paths_preserves_existing_devices_without_duplicates() {
        let existing = vec![PathBuf::from("/dev/input/event0")];
        let discovered = vec![PathBuf::from("/dev/input/event0")];

        let (removed, added) = reconcile_device_paths(&existing, &discovered);

        assert!(removed.is_empty());
        assert!(added.is_empty());
    }

    #[test]
    fn should_remove_device_on_fetch_error_ignores_would_block() {
        let would_block = std::io::Error::from(ErrorKind::WouldBlock);
        let interrupted = std::io::Error::from(ErrorKind::Interrupted);
        let other = std::io::Error::from(ErrorKind::BrokenPipe);

        assert!(!should_remove_device_on_fetch_error(&would_block));
        assert!(!should_remove_device_on_fetch_error(&interrupted));
        assert!(should_remove_device_on_fetch_error(&other));
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
        Key::KEY_KPENTER => KeyKind::Key(CanonicalKey::Enter),
        _ => return None,
    };

    Some(kind)
}

fn key_kind_to_canonical_key(key: KeyKind) -> CanonicalKey {
    match key {
        KeyKind::Character(ch) => CanonicalKey::from_printable_char(ch).unwrap_or(CanonicalKey::Unknown),
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
        KeyKind::Key(_) => None,
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
