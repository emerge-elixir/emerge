use super::*;
use crate::input::{self, InputEvent::*};
use sctk::seat::{
    Capability, SeatHandler,
    keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
    pointer::{AxisScroll, CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec},
};
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
pub(super) enum Pointer {
    Theme(ThemedPointer),
    Server(WlPointer, Option<WpCursorShapeDeviceV1>),
}
impl Pointer {
    fn pointer(&self) -> &WlPointer {
        match self {
            Self::Theme(p) => p.pointer(),
            Self::Server(p, _) => p,
        }
    }
    fn enter(&self, conn: &Connection, serial: u32) {
        match self {
            Self::Theme(p) => {
                let _ = p.set_cursor(conn, CursorIcon::Default);
            }
            Self::Server(_, Some(d)) => d.set_shape(serial, Shape::Default),
            Self::Server(_, None) => {}
        }
    }
}
impl Drop for Pointer {
    fn drop(&mut self) {
        if let Self::Server(p, d) = self {
            if let Some(d) = d.take() {
                d.destroy();
            }
            if p.version() >= 3 {
                p.release();
            }
        }
    }
}
impl SeatHandler for WindowApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        match cap {
            Capability::Pointer if self.pointer.is_none() => {
                if !self.allow_cursor_theme {
                    if let Ok(p) = self.seats.get_pointer(qh, &seat) {
                        let shape = self
                            .cursor_shapes
                            .as_ref()
                            .map(|m| m.get_shape_device(&p, qh));
                        self.pointer = Some((seat, Pointer::Server(p, shape)));
                    }
                    return;
                }
                let cursor = self.compositor.create_surface(qh);
                if let Ok(pointer) = self
                    .seats
                    .get_pointer_with_theme::<Self, sctk::compositor::SurfaceData>(
                        qh,
                        &seat,
                        self.shm.wl_shm(),
                        cursor,
                        ThemeSpec::default(),
                    )
                {
                    self.pointer = Some((seat, Pointer::Theme(pointer)));
                }
            }
            Capability::Keyboard if self.keyboard.is_none() => {
                if let Ok(k) = self.seats.get_keyboard(qh, &seat, None) {
                    self.keyboard = Some((seat, k));
                }
            }
            _ => (),
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer && self.pointer.as_ref().is_some_and(|(s, _)| s == &seat) {
            self.pointer.take();
            self.input(CursorEntered { entered: false });
        }
        if cap == Capability::Keyboard && self.keyboard.as_ref().is_some_and(|(s, _)| s == &seat) {
            if let Some((_, k)) = self.keyboard.take()
                && k.version() >= 3
            {
                k.release();
            }
            self.mods = 0;
            self.input(Focused { focused: false });
        }
    }
    fn remove_seat(&mut self, c: &Connection, q: &QueueHandle<Self>, s: wl_seat::WlSeat) {
        self.remove_capability(c, q, s.clone(), Capability::Pointer);
        self.remove_capability(c, q, s, Capability::Keyboard);
    }
}
impl KeyboardHandler for WindowApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        s: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if s == self.window.wl_surface() {
            self.input(Focused { focused: true });
        }
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        s: &wl_surface::WlSurface,
        _: u32,
    ) {
        if s == self.window.wl_surface() {
            self.mods = 0;
            self.input(Focused { focused: false });
        }
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = (u8::from(m.shift) * input::MOD_SHIFT)
            | (u8::from(m.ctrl) * input::MOD_CTRL)
            | (u8::from(m.alt) * input::MOD_ALT)
            | (u8::from(m.logo) * input::MOD_META);
    }
}
// SCTK can report the same wheel step in three forms. Prefer value120,
// then discrete steps; only a precise surface delta uses buffer mapping.
fn axis(a: AxisScroll, line: f32, precise: f32) -> f32 {
    if a.value120 != 0 {
        -(a.value120 as f32 / 120.) * line
    } else if a.discrete != 0 {
        -(a.discrete as f32) * line
    } else {
        -precise
    }
}
impl PointerHandler for WindowApp {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        p: &WlPointer,
        events: &[PointerEvent],
    ) {
        if self
            .pointer
            .as_ref()
            .is_none_or(|(_, current)| current.pointer() != p)
        {
            return;
        }
        for e in events {
            if &e.surface != self.window.wl_surface() {
                continue;
            }
            // A bufferless surface has no seat-coordinate mapping. Never
            // interpret a click using a requested but not yet submitted size.
            let Some(mapping) = self.installed else {
                continue;
            };
            let (x, y) = mapping.point(e.position);
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some((_, cursor)) = &self.pointer {
                        cursor.enter(conn, serial);
                    }
                    self.input(CursorEntered { entered: true });
                    self.input(CursorPos { x, y });
                }
                PointerEventKind::Leave { .. } => self.input(CursorEntered { entered: false }),
                PointerEventKind::Motion { .. } => self.input(CursorPos { x, y }),
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    let (px, py) = mapping.point((horizontal.absolute, vertical.absolute));
                    let dx = axis(horizontal, self.display.line_pixels, px);
                    let dy = axis(vertical, self.display.line_pixels, py);
                    if dx.is_finite() && dy.is_finite() && (dx != 0. || dy != 0.) {
                        self.input(CursorScroll { dx, dy, x, y });
                    }
                }
                PointerEventKind::Press { button: 0x110, .. }
                | PointerEventKind::Release { button: 0x110, .. } => self.input(CursorButton {
                    button: input::pointer::BUTTON_LEFT.into(),
                    action: if matches!(e.kind, PointerEventKind::Press { .. }) {
                        input::ACTION_PRESS
                    } else {
                        input::ACTION_RELEASE
                    },
                    mods: self.mods,
                    x,
                    y,
                }),
                _ => (),
            }
        }
    }
}
sctk::delegate_seat!(WindowApp);
sctk::delegate_pointer!(WindowApp);
sctk::delegate_keyboard!(WindowApp);

#[test]
fn wheel_units_choose_one_source_per_axis_and_precise_uses_the_installed_ratio() {
    let mut sample = AxisScroll {
        value120: 60,
        discrete: 1,
        absolute: 3.5,
        ..Default::default()
    };
    let mapping = super::geometry::Geometry {
        logical: (101, 75),
        scale: super::geometry::Scale::Fractional(180),
    }
    .mapping()
    .unwrap();
    let precise = mapping.point((0., sample.absolute)).1;
    assert_eq!(axis(sample, 45., precise), -22.5);
    sample.value120 = 0;
    assert_eq!(axis(sample, 45., precise), -45.);
    sample.discrete = 0;
    assert_eq!(axis(sample, 45., precise), -(3.5 * 113. / 75.) as f32);
}
