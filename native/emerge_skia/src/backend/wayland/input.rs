use smithay_client_toolkit::{
    reexports::client::globals::GlobalList,
    seat::{
        SeatState,
        pointer::{
            AxisScroll, BTN_BACK, BTN_EXTRA, BTN_FORWARD, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT,
            BTN_SIDE, ThemedPointer,
        },
    },
};
use wayland_client::QueueHandle;

use crate::input::{ACTION_PRESS, ACTION_RELEASE, InputEvent};

use super::runtime::WaylandApp;

pub(super) struct PointerInputState {
    pub(super) seat_state: SeatState,
    pub(super) pointer: Option<ThemedPointer>,
    pub(super) cursor_pos: (f32, f32),
    pub(super) current_mods: u8,
    pub(super) entered: bool,
}

impl PointerInputState {
    pub(super) fn new(globals: &GlobalList, qh: &QueueHandle<WaylandApp>) -> Self {
        Self {
            seat_state: SeatState::new(globals, qh),
            pointer: None,
            cursor_pos: (0.0, 0.0),
            current_mods: 0,
            entered: false,
        }
    }

    pub(super) fn set_cursor_pos(&mut self, x: f32, y: f32) {
        self.cursor_pos = (x, y);
    }
}

pub(super) fn pointer_button_name(button: u32) -> &'static str {
    match button {
        BTN_LEFT => "left",
        BTN_RIGHT => "right",
        BTN_MIDDLE => "middle",
        BTN_SIDE | BTN_BACK => "back",
        BTN_EXTRA | BTN_FORWARD => "forward",
        _ => "other",
    }
}

pub(super) fn pointer_button_event(
    button: u32,
    pressed: bool,
    mods: u8,
    position: (f32, f32),
) -> InputEvent {
    InputEvent::CursorButton {
        button: pointer_button_name(button).to_string(),
        action: if pressed {
            ACTION_PRESS
        } else {
            ACTION_RELEASE
        },
        mods,
        x: position.0,
        y: position.1,
    }
}

pub(super) fn pointer_scroll_event(
    horizontal: AxisScroll,
    vertical: AxisScroll,
    position: (f32, f32),
) -> Option<InputEvent> {
    let (x, y) = position;

    if horizontal.discrete != 0 || vertical.discrete != 0 {
        return Some(InputEvent::CursorScrollLines {
            dx: -(horizontal.discrete as f32),
            dy: -(vertical.discrete as f32),
            x,
            y,
        });
    }

    if horizontal.absolute != 0.0 || vertical.absolute != 0.0 {
        return Some(InputEvent::CursorScroll {
            dx: -(horizontal.absolute as f32),
            dy: -(vertical.absolute as f32),
            x,
            y,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use smithay_client_toolkit::seat::pointer::BTN_TASK;

    use super::*;

    #[test]
    fn pointer_button_name_matches_legacy_labels() {
        assert_eq!(pointer_button_name(BTN_LEFT), "left");
        assert_eq!(pointer_button_name(BTN_RIGHT), "right");
        assert_eq!(pointer_button_name(BTN_MIDDLE), "middle");
        assert_eq!(pointer_button_name(BTN_SIDE), "back");
        assert_eq!(pointer_button_name(BTN_BACK), "back");
        assert_eq!(pointer_button_name(BTN_EXTRA), "forward");
        assert_eq!(pointer_button_name(BTN_FORWARD), "forward");
        assert_eq!(pointer_button_name(BTN_TASK), "other");
    }

    #[test]
    fn pointer_scroll_event_prefers_discrete_steps() {
        let event = pointer_scroll_event(
            AxisScroll {
                absolute: 6.0,
                discrete: 2,
                value120: 0,
                relative_direction: None,
                stop: false,
            },
            AxisScroll {
                absolute: -9.0,
                discrete: -3,
                value120: 0,
                relative_direction: None,
                stop: false,
            },
            (12.0, 18.0),
        )
        .unwrap();

        assert!(matches!(
            event,
            InputEvent::CursorScrollLines { dx, dy, x, y }
                if (dx + 2.0).abs() < f32::EPSILON
                    && (dy - 3.0).abs() < f32::EPSILON
                    && (x - 12.0).abs() < f32::EPSILON
                    && (y - 18.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn pointer_scroll_event_uses_absolute_deltas_when_discrete_missing() {
        let event = pointer_scroll_event(
            AxisScroll {
                absolute: 4.5,
                discrete: 0,
                value120: 0,
                relative_direction: None,
                stop: false,
            },
            AxisScroll {
                absolute: -7.25,
                discrete: 0,
                value120: 0,
                relative_direction: None,
                stop: false,
            },
            (3.0, 5.0),
        )
        .unwrap();

        assert!(matches!(
            event,
            InputEvent::CursorScroll { dx, dy, x, y }
                if (dx + 4.5).abs() < f32::EPSILON
                    && (dy - 7.25).abs() < f32::EPSILON
                    && (x - 3.0).abs() < f32::EPSILON
                    && (y - 5.0).abs() < f32::EPSILON
        ));
    }
}
