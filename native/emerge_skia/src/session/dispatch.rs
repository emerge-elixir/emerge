//! Headless ingress constructs the same InputEvent as a backend. Observation is
//! optional; click tracking is not. No wire IDs are accepted from the caller.
use crate::{
    binary,
    input::{self, InputEvent, InputHandler},
    keys::CanonicalKey,
    tree::{Attachment, Scene, Stop, interaction::Actions},
};
use rustler::{Atom, Encoder, Env, Error, LocalPid, NifResult, Term};
use std::time::Instant;
mod gesture;
use gesture::{Gesture, Source};

rustler::atoms! { press, release, pointer_move, pointer_button, pointer_scroll, pointer_enter, focus, blur }
fn action(a: Atom) -> NifResult<u8> {
    if a == press() {
        Ok(input::ACTION_PRESS)
    } else if a == release() {
        Ok(input::ACTION_RELEASE)
    } else {
        Err(Error::BadArg)
    }
}
fn mods(mut list: Term) -> NifResult<u8> {
    let mut bits = 0;
    for _ in 0..4 {
        if list.is_empty_list() {
            return Ok(bits);
        }
        let (t, tail) = list.list_get_cell()?;
        list = tail;
        let a: Atom = t.decode()?;
        let bit = if a == input::shift() {
            input::MOD_SHIFT
        } else if a == input::ctrl() {
            input::MOD_CTRL
        } else if a == input::alt() {
            input::MOD_ALT
        } else if a == input::meta() {
            input::MOD_META
        } else {
            return Err(Error::BadArg);
        };
        if bits & bit != 0 {
            return Err(Error::BadArg);
        }
        bits |= bit;
    }
    list.is_empty_list().then_some(bits).ok_or(Error::BadArg)
}
fn point((x, y): (f32, f32)) -> NifResult<(f32, f32)> {
    if x.is_finite() && y.is_finite() {
        Ok((x, y))
    } else {
        Err(Error::BadArg)
    }
}
pub(super) fn decode(raw: Term) -> NifResult<InputEvent> {
    let (tag, payload): (Atom, Term) = raw.decode()?;
    Ok(if tag == pointer_move() {
        let (x, y) = point(payload.decode()?)?;
        InputEvent::CursorPos { x, y }
    } else if tag == pointer_button() {
        let (button, a, m, pos): (Term, Atom, Term, (f32, f32)) = payload.decode()?;
        let (x, y) = point(pos)?;
        let button = button.atom_to_string()?;
        if button != input::pointer::canonical_button_label(&button) {
            return Err(Error::BadArg);
        }
        InputEvent::CursorButton {
            button,
            action: action(a)?,
            mods: mods(m)?,
            x,
            y,
        }
    } else if tag == input::key() {
        let (key, a, m): (Term, Atom, Term) = payload.decode()?;
        let key = CanonicalKey::from_atom_name(&key.atom_to_string()?).ok_or(Error::BadArg)?;
        InputEvent::Key {
            key,
            action: action(a)?,
            mods: mods(m)?,
        }
    } else if tag == pointer_scroll() {
        let (delta, pos) = payload.decode()?;
        let (dx, dy) = point(delta)?;
        let (x, y) = point(pos)?;
        InputEvent::CursorScroll { dx, dy, x, y }
    } else if tag == input::touch() {
        let (id, phase, pos): (u64, Atom, (f32, f32)) = payload.decode()?;
        let phase = if phase == input::down() {
            input::TouchPhase::Down
        } else if phase == input::move_touch() {
            input::TouchPhase::Move
        } else if phase == input::up() {
            input::TouchPhase::Up
        } else if phase == input::cancel() {
            input::TouchPhase::Cancel
        } else {
            return Err(Error::BadArg);
        };
        let (x, y) = point(pos)?;
        InputEvent::Touch { id, phase, x, y }
    } else if tag == pointer_enter() {
        InputEvent::CursorEntered {
            entered: payload.decode()?,
        }
    } else if tag == focus() {
        InputEvent::Focused {
            focused: payload.decode()?,
        }
    } else if tag == input::text_commit() {
        let (text, m): (rustler::Binary, Term) = payload.decode()?;
        if text.len() > 64 * 1024 {
            return Err(Error::BadArg);
        }
        InputEvent::TextCommit {
            text: std::str::from_utf8(text.as_slice())
                .map_err(|_| Error::BadArg)?
                .to_owned(),
            mods: mods(m)?,
        }
    } else {
        return Err(Error::BadArg);
    })
}

#[derive(Default)]
pub(super) struct Dispatcher {
    pub observer: InputHandler,
    pub target: Option<LocalPid>,
    gesture: Option<Gesture>,
    focus: Option<Attachment>,
    autofocus_mount: u64,
    blurred: bool,
    scroll: super::scroll::Driver,
}
#[derive(rustler::NifMap)]
pub(super) struct Position {
    x: f64,
    y: f64,
}
impl From<[f64; 2]> for Position {
    fn from([x, y]: [f64; 2]) -> Self {
        Self { x, y }
    }
}
// No env is stored in the dispatcher or the gesture.
pub(super) struct Context<'a, 'env> {
    pub owner: &'a super::Owner,
    pub pid: Option<LocalPid>,
    env: Env<'env>,
    epoch: u64,
}
impl<'a, 'env> Context<'a, 'env> {
    fn new(
        owner: &'a super::Owner,
        env: Env<'env>,
        pid: Option<LocalPid>,
        f: &super::Bundle,
    ) -> Self {
        Self {
            owner,
            env,
            pid,
            epoch: f.route.epoch,
        }
    }
    fn send(&self, event: impl Encoder) {
        if let Some(pid) = self.pid {
            if self.epoch == 0 {
                let _ = self.env.send(&pid, (input::emerge_skia_event(), event));
            } else {
                let _ = self
                    .env
                    .send(&pid, (super::emerge_skia_routed(), self.epoch, event));
            }
        }
    }
    pub fn payload(&self, id: u64, kind: Atom, value: impl Encoder) {
        if self.pid.is_some() {
            self.send((binary(self.env, &id.to_be_bytes()), kind, value));
        }
    }
    fn bare(&self, id: u64, kind: Atom) {
        if self.pid.is_some() {
            self.send((binary(self.env, &id.to_be_bytes()), kind));
        }
    }
}
impl Dispatcher {
    pub fn after_draw(
        &mut self,
        owner: &super::Owner,
        env: Env,
        f: &super::Bundle,
        old: Option<&super::Bundle>,
    ) {
        let s = &f.scene;
        let changed = old.is_none_or(|o| {
            o.revision != f.revision
                || o.id.generation != f.id.generation
                || !same_navigation(&o.scene, s)
        });
        self.scroll.installed(owner, s, changed);
        if let Some(g) = self.gesture.as_mut().filter(|g| !g.valid(s, &self.scroll)) {
            g.suppress();
            self.scroll.cancel();
        }
        let nav = s.navigation.as_ref();
        self.focus = self.focus.filter(|&k| nav.and_then(|n| n.get(k)).is_some());
        if let Some((stamp, key)) = nav.and_then(|n| n.autofocus())
            && stamp > self.autofocus_mount
        {
            self.autofocus_mount = stamp;
            if !self.blurred {
                self.focus_to(
                    s,
                    nav.and_then(|n| n.get(key)),
                    &Context::new(owner, env, self.target, f),
                );
            }
        }
    }
    fn focus_to(&mut self, s: &Scene, next: Option<Stop>, cx: &Context<'_, '_>) {
        if self.focus == next.map(|t| t.key) {
            return;
        }
        let nav = s.navigation.as_ref();
        if let Some(old) = self.focus.and_then(|k| nav?.get(k))
            && old.actions.has(Actions::BLUR)
        {
            cx.bare(old.id, blur());
        }
        self.focus = next.map(|t| t.key);
        if let Some((stop, nav)) = next.zip(nav) {
            self.scroll.interrupt();
            if stop.actions.has(Actions::FOCUS) {
                cx.bare(stop.id, focus());
            }
            self.scroll.reveal(nav, stop, cx);
        }
    }
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub fn interrupt_momentum(&mut self) {
        self.scroll.interrupt();
    }
    pub fn kinetic(&self) -> bool {
        self.scroll.kinetic()
    }
    pub fn tick(
        &mut self,
        owner: &super::Owner,
        env: Env,
        f: &super::Bundle,
        now: Instant,
    ) -> Option<Instant> {
        self.scroll
            .tick(&f.scene, &Context::new(owner, env, self.target, f), now)
    }
    pub fn set_target(&mut self, target: Option<LocalPid>) {
        self.gesture = None;
        self.focus = None;
        self.scroll.cancel();
        self.target = target;
    }
    pub fn dispatch(
        &mut self,
        owner: &super::Owner,
        env: Env,
        frame: Option<&super::Bundle>,
        event: InputEvent,
        at: Instant,
    ) {
        if let Some(pid) = self.target
            && self.observer.accepts(&event)
        {
            let _ = env.send(&pid, (input::emerge_skia_event(), &event));
        }
        if let InputEvent::Focused { focused } = event {
            self.blurred = !focused;
        }
        let Some(f) = frame else { return };
        let cx = Context::new(owner, env, self.target, f);
        let s = &f.scene;
        use input::{InputEvent::*, TouchPhase::*};
        match event {
            Touch { id, phase, x, y } => self.contact(s, Source::Touch(id), phase, [x, y], at, &cx),
            CursorButton {
                button,
                action,
                x,
                y,
                ..
            } if button == input::pointer::BUTTON_LEFT => {
                self.scroll.pointer_at([x, y]);
                self.contact(
                    s,
                    Source::Mouse,
                    if action == input::ACTION_PRESS {
                        Down
                    } else {
                        Up
                    },
                    [x, y],
                    at,
                    &cx,
                );
            }
            CursorPos { x, y } => {
                self.scroll.cursor(owner, s, Some([x, y]));
                self.contact(s, Source::Mouse, Move, [x, y], at, &cx);
            }
            CursorScroll { dx, dy, x, y } => self.scroll.wheel(s, [x, y], [dx, dy], &cx),
            CursorEntered { entered: false } => {
                self.scroll.cursor(owner, s, None);
                self.cancel_contact(s, Some(Source::Mouse), &cx);
            }
            Focused { focused: false } => {
                self.scroll.cursor(owner, s, None);
                self.cancel_contact(s, None, &cx);
                self.focus_to(s, None, &cx);
            }
            Key {
                key,
                action: input::ACTION_PRESS,
                mods,
            } if !self.blurred
                && (mods == 0 || (key == CanonicalKey::Tab && mods == input::MOD_SHIFT)) =>
            {
                let nav = s.navigation.as_ref();
                let focused = self.focus.and_then(|k| nav?.get(k));
                if key == CanonicalKey::Tab {
                    self.focus_to(s, nav.and_then(|n| n.tab(self.focus, mods != 0)), &cx);
                } else if key == CanonicalKey::Enter
                    && let Some(t) = focused.filter(|t| t.actions.has(Actions::PRESS))
                {
                    cx.bare(t.id, press());
                } else {
                    self.scroll.key(s, focused, key, owner.line_pixels, &cx);
                }
            }
            _ => (),
        }
    }
}
fn same_navigation(a: &Scene, b: &Scene) -> bool {
    match (&a.navigation, &b.navigation) {
        (Some(a), Some(b)) => a.same_receipts(b),
        (None, None) => true,
        _ => false,
    }
}
