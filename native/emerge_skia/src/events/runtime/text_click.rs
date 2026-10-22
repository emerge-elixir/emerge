//! Shared, deterministic multi-click classification. Times are captured before queueing.
use std::time::{Duration, Instant};

use crate::{
    input::{ACTION_PRESS, ACTION_RELEASE, InputEvent},
    tree::element::NodeId,
};

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_DISTANCE: f32 = 5.0;

struct Click {
    element_id: NodeId,
    position: (f32, f32),
    at: Instant,
    released: bool,
}

#[derive(Default)]
pub(super) struct TextClicks {
    previous: Option<Click>,
    pending_press: bool,
    word_drag: Option<(NodeId, (f32, f32))>,
}

impl TextClicks {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn retain_mount(&mut self, same_mount: impl FnOnce(&NodeId) -> bool) {
        if self
            .previous
            .as_ref()
            .is_some_and(|click| !same_mount(&click.element_id))
        {
            self.reset();
        }
    }

    pub(super) fn begin_input(&mut self, event: &InputEvent) {
        match event {
            InputEvent::CursorButton {
                button,
                action,
                mods: 0,
                x,
                y,
            } if button == "left" => {
                if *action == ACTION_PRESS {
                    self.pending_press = true;
                } else if *action == ACTION_RELEASE {
                    self.word_drag = None;
                    if let Some(previous) = self.previous.as_mut() {
                        previous.released = close(previous.position, (*x, *y));
                        if !previous.released {
                            self.reset();
                        }
                    }
                }
            }
            InputEvent::CursorPos { x, y } => {
                if self
                    .previous
                    .as_ref()
                    .is_some_and(|click| !close(click.position, (*x, *y)))
                {
                    self.reset();
                }
            }
            InputEvent::CursorEntered { entered: true } | InputEvent::Focused { focused: true } => {
            }
            _ => self.reset(),
        }
    }

    pub(super) fn end_input(&mut self) {
        // A press on a different kind of element must break a text-click chain.
        if self.pending_press {
            self.reset();
        }
    }

    pub(super) fn press(&mut self, element_id: NodeId, position: (f32, f32), at: Instant) -> bool {
        if !self.pending_press {
            return false;
        }
        self.pending_press = false;
        let repeated = self.previous.as_ref().is_some_and(|click| {
            click.element_id == element_id
                && click.released
                && close(click.position, position)
                && at
                    .checked_duration_since(click.at)
                    .is_some_and(|elapsed| elapsed <= DOUBLE_CLICK_INTERVAL)
        });
        self.previous = Some(Click {
            element_id,
            position,
            at,
            released: false,
        });
        self.word_drag = repeated.then_some((element_id, position));
        repeated
    }

    pub(super) fn preserve_word(&self, element_id: NodeId, position: (f32, f32)) -> bool {
        self.word_drag
            .is_some_and(|(id, origin)| id == element_id && close(origin, position))
    }
}

fn close(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).hypot(a.1 - b.1) <= DOUBLE_CLICK_DISTANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(action: u8, x: f32) -> InputEvent {
        InputEvent::CursorButton {
            button: "left".into(),
            action,
            mods: 0,
            x,
            y: 0.0,
        }
    }

    fn click(clicks: &mut TextClicks, id: u64, x: f32, at: Instant) -> bool {
        clicks.begin_input(&button(ACTION_PRESS, x));
        let repeated = clicks.press(NodeId(id), (x, 0.0), at);
        clicks.end_input();
        clicks.begin_input(&button(ACTION_RELEASE, x));
        repeated
    }

    #[test]
    fn interval_distance_release_and_identity_boundaries_are_deterministic() {
        let now = Instant::now();
        for (delay, distance, id, expected) in [
            (500, 5.0, 1, true),
            (501, 0.0, 1, false),
            (100, 5.1, 1, false),
            (100, 0.0, 2, false),
        ] {
            let mut clicks = TextClicks::default();
            assert!(!click(&mut clicks, 1, 0.0, now));
            assert_eq!(
                click(
                    &mut clicks,
                    id,
                    distance,
                    now + Duration::from_millis(delay)
                ),
                expected
            );
        }
        let mut clicks = TextClicks::default();
        clicks.begin_input(&button(ACTION_PRESS, 0.0));
        assert!(!clicks.press(NodeId(1), (0.0, 0.0), now));
        clicks.end_input();
        assert!(!click(&mut clicks, 1, 0.0, now + Duration::from_millis(10)));
    }

    #[test]
    fn movement_intervening_input_and_remount_cancel_the_chain() {
        let now = Instant::now();
        for event in [
            InputEvent::CursorPos { x: 6.0, y: 0.0 },
            InputEvent::Focused { focused: false },
            InputEvent::CursorEntered { entered: false },
            InputEvent::TextCommit {
                text: "x".into(),
                mods: 0,
            },
            InputEvent::CursorButton {
                button: "left".into(),
                action: ACTION_PRESS,
                mods: 1,
                x: 0.0,
                y: 0.0,
            },
        ] {
            let mut clicks = TextClicks::default();
            click(&mut clicks, 1, 0.0, now);
            clicks.begin_input(&event);
            clicks.end_input();
            assert!(!click(&mut clicks, 1, 0.0, now + Duration::from_millis(10)));
        }
        let mut clicks = TextClicks::default();
        click(&mut clicks, 1, 0.0, now);
        clicks.retain_mount(|_| false);
        assert!(!click(&mut clicks, 1, 0.0, now));
        clicks.begin_input(&button(ACTION_PRESS, 0.0));
        clicks.end_input(); // no editable input matched
        assert!(!click(&mut clicks, 1, 0.0, now));
    }
}
