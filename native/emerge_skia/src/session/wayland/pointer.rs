use super::*;
use crate::input::{self, InputEvent::*};
use sctk::seat::{
    Capability, SeatHandler,
    keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
    pointer::{CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec},
};
use wayland_client::protocol::wl_pointer::WlPointer;
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
                    self.pointer = Some((seat, pointer));
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
            let (x, y) = self.installed.unwrap_or(self.geometry).point(e.position);
            match e.kind {
                PointerEventKind::Enter { .. } => {
                    if let Some((_, cursor)) = &self.pointer {
                        let _ = cursor.set_cursor(conn, CursorIcon::Default);
                    }
                    self.input(CursorEntered { entered: true });
                    self.input(CursorPos { x, y });
                }
                PointerEventKind::Leave { .. } => self.input(CursorEntered { entered: false }),
                PointerEventKind::Motion { .. } => self.input(CursorPos { x, y }),
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
