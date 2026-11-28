//! wl_touch.up has no position, cancel has no id, and compositor slot ids
//! can be reused. Retain only contacts begun on this installed window.
use super::*;
use crate::input::TouchPhase;
use sctk::seat::touch::TouchHandler;
use std::collections::HashMap;
use wayland_client::protocol::wl_touch::WlTouch;

#[derive(Clone, Copy)]
struct Contact {
    id: u64,
    surface: (f64, f64),
    down_ms: u32,
    down_at: Instant,
}
impl Contact {
    fn event(self, phase: TouchPhase, mapping: Mapping) -> InputEvent {
        let (x, y) = mapping.point(self.surface);
        InputEvent::Touch {
            id: self.id,
            phase,
            x,
            y,
        }
    }
    fn at(self, ms: u32) -> Instant {
        self.down_at + Duration::from_millis(u64::from(ms.wrapping_sub(self.down_ms)))
    }
}
#[derive(Default)]
pub(super) struct Contacts {
    pub device: Option<(wl_seat::WlSeat, WlTouch)>,
    active: HashMap<i32, Contact>,
    serial: u64,
}
impl Contacts {
    pub(super) fn position(&self, id: u64, mapping: Mapping) -> Option<[f32; 2]> {
        self.active
            .values()
            .find(|c| c.id == id)
            .map(|c| mapping.point(c.surface).into())
    }
    fn owns(&self, t: &WlTouch) -> bool {
        self.device.as_ref().is_some_and(|(_, d)| d == t)
    }
}
impl WindowApp {
    fn cancel_touches(&mut self) {
        self.display.input.interrupt_momentum();
        let contacts = std::mem::take(&mut self.touch.active);
        if let Some(m) = self.installed {
            for (_, c) in contacts {
                self.input(c.event(TouchPhase::Cancel, m));
            }
        }
    }
    pub(super) fn remove_touch(&mut self) {
        self.cancel_touches();
        if let Some((_, t)) = self.touch.device.take()
            && t.version() >= 3
        {
            t.release();
        }
    }
}
impl TouchHandler for WindowApp {
    fn down(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        t: &WlTouch,
        _: u32,
        time: u32,
        s: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        if !self.touch.owns(t)
            || &s != self.window.wl_surface()
            || self.touch.active.contains_key(&id)
        {
            return;
        }
        let Some(mapping) = self.installed else {
            return;
        };
        self.touch.serial += 1;
        let contact = Contact {
            id: self.touch.serial,
            surface: position,
            down_ms: time,
            down_at: Instant::now(),
        };
        self.touch.active.insert(id, contact);
        self.input_at(contact.event(TouchPhase::Down, mapping), contact.down_at);
    }
    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        t: &WlTouch,
        time: u32,
        id: i32,
        position: (f64, f64),
    ) {
        if !self.touch.owns(t) {
            return;
        }
        if let Some((c, m)) = self.touch.active.get_mut(&id).zip(self.installed) {
            c.surface = position;
            let c = *c;
            self.input_at(c.event(TouchPhase::Move, m), c.at(time));
        }
    }
    fn up(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        t: &WlTouch,
        _: u32,
        time: u32,
        id: i32,
    ) {
        if self.touch.owns(t)
            && let Some((c, m)) = self.touch.active.remove(&id).zip(self.installed)
        {
            self.input_at(c.event(TouchPhase::Up, m), c.at(time));
        }
    }
    fn cancel(&mut self, _: &Connection, _: &QueueHandle<Self>, t: &WlTouch) {
        if self.touch.owns(t) {
            self.cancel_touches();
        }
    }
    fn shape(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlTouch,
        _: i32,
        _: f64,
        _: f64,
    ) {
    }
    fn orientation(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlTouch, _: i32, _: f64) {}
}
sctk::delegate_touch!(WindowApp);
