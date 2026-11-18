//! Headless ingress constructs the same InputEvent as a backend. Observation is
//! optional; click tracking is not. No wire IDs are accepted from the caller.
use crate::{
    binary,
    input::{self, InputEvent, InputHandler},
    keys::CanonicalKey,
    tree::{Attachment, Raster},
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
    while !list.is_empty_list() {
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
    Ok(bits)
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
        let (text, m): (String, Term) = payload.decode()?;
        InputEvent::TextCommit {
            text,
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
}
#[derive(rustler::NifMap)]
struct Position {
    x: f64,
    y: f64,
}
impl Dispatcher {
    pub fn after_draw(&mut self, raster: &Raster) {
        if self.pressed.is_some_and(|key| !raster.has_target(key)) {
            self.pressed = None
        }
    }
    pub fn set_target(&mut self, target: Option<LocalPid>) {
        self.pressed = None;
        self.target = target;
    }
    pub fn dispatch(&mut self, env: Env, raster: &Raster, event: InputEvent) {
        let Some(pid) = self.target else { return };
        if self.observer.accepts(&event) {
            let _ = env.send(&pid, (input::emerge_skia_event(), &event));
        }
        match event {
            InputEvent::CursorButton {
                ref button,
                action,
                x,
                y,
                ..
            } if button == input::pointer::BUTTON_LEFT => {
                let hit = raster.target_at(x, y);
                if action == input::ACTION_PRESS {
                    self.pressed = hit.map(|h| h.0);
                } else if let (Some(pressed), Some((key, id, x, y))) = (self.pressed.take(), hit)
                    && key == pressed
                {
                    let _ = env.send(
                        &pid,
                        (
                            input::emerge_skia_event(),
                            (
                                binary(env, &id.to_be_bytes()),
                                input::click(),
                                Position { x, y },
                            ),
                        ),
                    );
                }
            }
            InputEvent::Focused { focused: false }
            | InputEvent::CursorEntered { entered: false } => self.pressed = None,
            _ => (),
        }
    }
}
