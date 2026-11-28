//! Headless ingress constructs the same InputEvent as a backend. Observation is
//! optional; click tracking is not. No wire IDs are accepted from the caller.
use crate::{
    binary,
    input::{self, InputEvent, InputHandler},
    keys::CanonicalKey,
    tree::{Attachment, Scene},
};
use rustler::{Atom, Env, Error, LocalPid, NifResult, Term};

rustler::atoms! { press, release, pointer_move, pointer_button, pointer_scroll, pointer_enter, focus }
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
    pressed: Option<Attachment>,
    scroll: super::scroll::Driver,
}
#[derive(rustler::NifMap)]
struct Position {
    x: f64,
    y: f64,
}
pub(super) fn deliver(env: Env, pid: LocalPid, epoch: u64, id: u64, kind: Atom, [x, y]: [f64; 2]) {
    let event = (binary(env, &id.to_be_bytes()), kind, Position { x, y });
    if epoch == 0 {
        let _ = env.send(&pid, (input::emerge_skia_event(), event));
    } else {
        let _ = env.send(&pid, (super::emerge_skia_routed(), epoch, event));
    }
}
impl Dispatcher {
    pub fn after_draw(&mut self, owner: &super::Owner, scene: &Scene) {
        self.scroll.installed(owner, scene);
        if self.pressed.is_some_and(|key| !scene.hits.has_target(key)) {
            self.pressed = None
        }
    }
    pub fn set_target(&mut self, target: Option<LocalPid>) {
        self.pressed = None;
        self.scroll.cancel();
        self.target = target;
    }
    pub fn dispatch(
        &mut self,
        owner: &super::Owner,
        env: Env,
        frame: Option<&super::Bundle>,
        event: InputEvent,
    ) {
        if let Some(pid) = self.target
            && self.observer.accepts(&event)
        {
            let _ = env.send(&pid, (input::emerge_skia_event(), &event));
        }
        if let Some(f) = frame
            && self.scroll.dispatch(owner, env, self.target, f, &event)
        {
            self.pressed = None;
            return;
        }
        let Some(pid) = self.target else { return };
        match event {
            InputEvent::CursorButton {
                ref button,
                action,
                x,
                y,
                ..
            } if button == input::pointer::BUTTON_LEFT => {
                let hit = frame.and_then(|f| f.scene.hits.target_at(f.scene.viewport, x, y));
                if action == input::ACTION_PRESS {
                    self.pressed = hit.map(|h| h.0);
                } else if let (Some(pressed), Some((key, id, x, y))) = (self.pressed.take(), hit)
                    && key == pressed
                {
                    deliver(
                        env,
                        pid,
                        frame.map_or(0, |f| f.route.epoch),
                        id,
                        input::click(),
                        [x, y],
                    );
                }
            }
            InputEvent::Focused { focused: false }
            | InputEvent::CursorEntered { entered: false } => self.pressed = None,
            _ => (),
        }
    }
}
