//! Touch wake recognition and contact-lifetime swallowing, independent of UI input.
use crate::runtime::touch_wake::{Armed, WakeGesture};
use emerge_input::Event;
use std::collections::HashMap;

type Contact = (u64, i32);
const TAP_USEC: u64 = 250_000;
const DOUBLE_USEC: u64 = 300_000;
const HOLD_USEC: u64 = 600_000;
const SLOP_SQUARED: f64 = 12.0 * 12.0;
const DOUBLE_SQUARED: f64 = 24.0 * 24.0;

#[derive(Clone, Copy)]
struct Press {
    contact: Contact,
    start: (f64, f64),
    at: u64,
    second: bool,
}
#[derive(Default)]
pub(super) struct TouchWake {
    armed: Option<Armed>,
    // true means this complete contact lifetime must never reach UI processing.
    contacts: HashMap<Contact, bool>,
    press: Option<Press>,
    first: Option<(u64, (f64, f64), u64)>,
    last_time: u64,
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

impl TouchWake {
    pub fn configure(&mut self, armed: Option<Armed>, input_enabled: bool) {
        if self.armed != armed {
            if self.armed.is_some() || armed.is_some() {
                self.reset();
            }
            self.armed = armed;
            self.last_time = 0;
        }
        if !input_enabled {
            self.contacts
                .values_mut()
                .for_each(|swallow| *swallow = true);
        }
    }

    pub fn reset(&mut self) {
        self.press = None;
        self.first = None;
        self.contacts
            .values_mut()
            .for_each(|swallow| *swallow = true);
    }

    pub fn deadline(&self) -> Option<u64> {
        self.armed
            .filter(|a| a.gesture == WakeGesture::LongPress)
            .and(self.press)
            .map(|p| p.at.saturating_add(HOLD_USEC))
    }

    pub fn tick(&mut self, now: u64) -> Option<Armed> {
        let armed = self.armed?;
        if self.deadline().is_some_and(|at| at <= now) && self.contacts.len() == 1 {
            self.press = None;
            self.first = None;
            return Some(armed);
        }
        None
    }

    /// Returns (consume UI event, recognized wake). now and source time share
    /// CLOCK_MONOTONIC. Contact up/cancel never relies on its absent coordinates.
    pub fn event(&mut self, event: Event, input_enabled: bool, now: u64) -> (bool, Option<Armed>) {
        if event.kind == 1 {
            self.contacts
                .retain(|(device, _), _| *device != event.device);
            self.press = None;
            self.first = None;
            return (false, None);
        }
        if !(8..=11).contains(&event.kind) {
            return (false, None);
        }
        let contact = (event.device, event.slot);
        let pos = (event.x, event.y);
        let at = event.time_usec;
        let valid = at != 0
            && at >= self.last_time
            && at <= now
            && (!(8..=9).contains(&event.kind) || (pos.0.is_finite() && pos.1.is_finite()));
        self.last_time = self.last_time.max(at.min(now));
        if !valid {
            self.press = None;
            self.first = None;
        }
        match event.kind {
            8 => {
                let swallow = !input_enabled || self.contacts.values().any(|s| *s);
                let replaced = self.contacts.insert(contact, swallow).is_some();
                if valid
                    && !replaced
                    && self.contacts.len() == 1
                    && let Some(armed) = self.armed
                    && at >= armed.since_usec
                {
                    let second = self.first.is_some_and(|(device, start, up)| {
                        device == event.device
                            && at >= up
                            && at - up <= DOUBLE_USEC
                            && distance(start, pos) <= DOUBLE_SQUARED
                    });
                    if !second {
                        self.first = None;
                    }
                    self.press = Some(Press {
                        contact,
                        start: pos,
                        at,
                        second,
                    });
                } else {
                    self.press = None;
                    self.first = None;
                }
                (swallow, None)
            }
            9 => {
                // A contact held before device discovery may have no observed
                // down. Drain it too; it cannot establish a fresh wake sequence.
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    self.contacts.entry(contact)
                {
                    entry.insert(true);
                    self.press = None;
                    self.first = None;
                }
                if self
                    .press
                    .is_some_and(|p| p.contact == contact && distance(p.start, pos) > SLOP_SQUARED)
                {
                    self.press = None;
                    self.first = None;
                }
                (*self.contacts.get(&contact).unwrap_or(&true), None)
            }
            10 => {
                let swallow = self.contacts.remove(&contact).unwrap_or(true);
                let press = self.press.take().filter(|p| p.contact == contact);
                let wake = if valid
                    && self.contacts.is_empty()
                    && let (Some(armed), Some(press)) = (self.armed, press)
                    && at >= press.at
                {
                    match armed.gesture {
                        WakeGesture::SingleTap if at - press.at <= TAP_USEC => Some(armed),
                        WakeGesture::DoubleTap if at - press.at <= TAP_USEC => {
                            if press.second {
                                self.first = None;
                                Some(armed)
                            } else {
                                self.first = Some((event.device, press.start, at));
                                None
                            }
                        }
                        // A queued up after a valid hold still counts, even if
                        // input dispatch was delayed past the timer deadline.
                        WakeGesture::LongPress if at - press.at >= HOLD_USEC => Some(armed),
                        _ => {
                            self.first = None;
                            None
                        }
                    }
                } else {
                    self.first = None;
                    None
                };
                (swallow, wake)
            }
            _ => {
                let swallow = if event.slot < 0 {
                    let swallowed = self
                        .contacts
                        .iter()
                        .any(|((device, _), s)| *device == event.device && *s);
                    self.contacts
                        .retain(|(device, _), _| *device != event.device);
                    swallowed
                } else {
                    self.contacts.remove(&contact).unwrap_or(true)
                };
                self.press = None;
                self.first = None;
                (swallow, None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arm(gesture: WakeGesture) -> Armed {
        Armed {
            revision: 1,
            gesture,
            since_usec: 1_000_000,
        }
    }
    fn event(kind: u32, ms: u64, x: f64) -> Event {
        Event {
            device: 1,
            slot: 0,
            kind,
            time_usec: 1_000_000 + ms * 1000,
            x,
            y: 20.0,
            ..Event::default()
        }
    }
    fn feed(w: &mut TouchWake, kind: u32, ms: u64, x: f64) -> Option<Armed> {
        let e = event(kind, ms, x);
        let (swallow, wake) = w.event(e, false, 10_000_000);
        assert!(swallow);
        wake
    }
    #[test]
    fn nil_and_all_gestures_with_source_time_boundaries() {
        let mut disabled = TouchWake::default();
        feed(&mut disabled, 8, 1, 10.0);
        assert_eq!(feed(&mut disabled, 10, 100, 0.0), None);
        for gesture in [
            WakeGesture::SingleTap,
            WakeGesture::DoubleTap,
            WakeGesture::LongPress,
        ] {
            let mut w = TouchWake::default();
            w.configure(Some(arm(gesture)), false);
            assert_eq!(feed(&mut w, 8, 1, 10.0), None);
            match gesture {
                WakeGesture::SingleTap => {
                    assert_eq!(feed(&mut w, 10, 251, 0.0), Some(arm(gesture)))
                }
                WakeGesture::DoubleTap => {
                    assert_eq!(feed(&mut w, 10, 101, 0.0), None);
                    feed(&mut w, 8, 401, 34.0);
                    assert_eq!(feed(&mut w, 10, 501, 0.0), Some(arm(gesture)));
                }
                WakeGesture::LongPress => {
                    assert_eq!(w.tick(1_600_999), None);
                    assert_eq!(w.tick(1_601_000), Some(arm(gesture)));
                    assert_eq!(w.tick(2_000_000), None);
                    w.configure(None, true);
                    assert!(w.event(event(9, 700, 11.0), true, 10_000_000).0);
                    assert!(w.event(event(10, 701, 0.0), true, 10_000_000).0);
                    assert!(!w.event(event(8, 702, 11.0), true, 10_000_000).0);
                }
            }
        }
    }
    #[test]
    fn moving_out_and_back_multitouch_cancel_and_device_removal_do_not_wake() {
        for cancel in [1, 9, 11, 8] {
            let mut w = TouchWake::default();
            w.configure(Some(arm(WakeGesture::SingleTap)), false);
            feed(&mut w, 8, 1, 10.0);
            let mut e = event(cancel, 50, 23.0);
            if cancel == 8 {
                e.slot = 1;
            }
            w.event(e, false, 10_000_000);
            assert_eq!(feed(&mut w, 9, 60, 10.0), None);
            assert_eq!(feed(&mut w, 10, 100, 0.0), None);
        }
    }
    #[test]
    fn stale_or_malformed_events_and_policy_changes_cannot_activate() {
        for bad in [
            event(8, 0, f64::NAN),
            Event {
                time_usec: 999_999,
                ..event(8, 0, 10.0)
            },
        ] {
            let mut w = TouchWake::default();
            w.configure(Some(arm(WakeGesture::SingleTap)), false);
            w.event(bad, false, 10_000_000);
            assert_eq!(feed(&mut w, 10, 100, 0.0), None);
        }
        let mut w = TouchWake::default();
        // A normal UI contact held when off begins cannot become a wake tap.
        w.event(event(8, 1, 10.0), true, 10_000_000);
        w.configure(Some(arm(WakeGesture::SingleTap)), false);
        assert_eq!(feed(&mut w, 10, 100, 0.0), None);
        feed(&mut w, 8, 200, 10.0);
        w.configure(None, false);
        assert_eq!(feed(&mut w, 10, 250, 0.0), None);
    }
    #[test]
    fn missing_down_and_cancelled_hold_do_not_wake_and_require_an_all_up_boundary() {
        let mut w = TouchWake::default();
        w.configure(Some(arm(WakeGesture::LongPress)), false);
        feed(&mut w, 9, 1, 10.0);
        assert_eq!(w.tick(9_000_000), None);
        let mut other = event(8, 2, 10.0);
        other.slot = 1;
        w.event(other, false, 10_000_000);
        assert_eq!(w.tick(9_000_000), None);
        feed(&mut w, 11, 3, 0.0);
        other.kind = 11;
        other.time_usec += 2_000;
        w.event(other, false, 10_000_000);
        feed(&mut w, 8, 10, 10.0);
        assert_eq!(w.deadline(), Some(1_610_000));
        // Process the queued cancellation before the expired timer.
        feed(&mut w, 11, 700, 0.0);
        assert_eq!(w.tick(9_000_000), None);
    }

    #[test]
    fn rejects_long_taps_distant_or_late_second_taps_and_backwards_time() {
        let mut w = TouchWake::default();
        w.configure(Some(arm(WakeGesture::SingleTap)), false);
        feed(&mut w, 8, 1, 10.0);
        assert_eq!(feed(&mut w, 10, 252, 0.0), None);
        feed(&mut w, 8, 400, 10.0);
        assert_eq!(feed(&mut w, 10, 399, 0.0), None);
        for (ms, x) in [(402, 10.0), (400, 35.0)] {
            let mut w = TouchWake::default();
            w.configure(Some(arm(WakeGesture::DoubleTap)), false);
            feed(&mut w, 8, 1, 10.0);
            feed(&mut w, 10, 101, 0.0);
            feed(&mut w, 8, ms, x);
            assert_eq!(feed(&mut w, 10, ms + 100, 0.0), None);
        }
    }
}
