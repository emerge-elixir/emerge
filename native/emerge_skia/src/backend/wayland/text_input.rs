use smithay_client_toolkit::{
    reexports::client::globals::GlobalList,
    shell::{WaylandSurface, xdg::window::Window},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_seat, wl_surface},
};
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{
        ChangeCause, ContentHint, ContentPurpose, Event as TextInputEvent, ZwpTextInputV3,
    },
};

use crate::{events::TextInputState, input::InputEvent};

use super::{geometry::SurfaceGeometry, runtime::WaylandApp};

#[derive(Clone, Debug, PartialEq)]
enum PendingTextInputOp {
    Preedit {
        text: Option<String>,
        cursor: Option<(u32, u32)>,
    },
    Commit(Option<String>),
    DeleteSurrounding {
        before_length: u32,
        after_length: u32,
    },
}

pub(super) struct TextInputProtocolState {
    manager: Option<ZwpTextInputManagerV3>,
    text_input: Option<ZwpTextInputV3>,
    entered_surface: bool,
    enabled: bool,
    commit_serial: u32,
    render_enabled: bool,
    render_cursor_area: Option<(f32, f32, f32, f32)>,
    render_text_state: Option<TextInputState>,
    pending_ops: Vec<PendingTextInputOp>,
    preedit_active: bool,
}

impl TextInputProtocolState {
    pub(super) fn new(globals: &GlobalList, qh: &QueueHandle<WaylandApp>) -> Self {
        let manager = globals.bind(qh, 1..=1, ()).ok();

        Self {
            manager,
            text_input: None,
            entered_surface: false,
            enabled: false,
            commit_serial: 0,
            render_enabled: false,
            render_cursor_area: None,
            render_text_state: None,
            pending_ops: Vec::new(),
            preedit_active: false,
        }
    }

    pub(super) fn create_for_seat(&mut self, qh: &QueueHandle<WaylandApp>, seat: &wl_seat::WlSeat) {
        if self.text_input.is_none()
            && let Some(manager) = self.manager.as_ref()
        {
            self.text_input = Some(manager.get_text_input(seat, qh, ()));
        }
    }

    pub(super) fn release(&mut self) {
        if let Some(text_input) = self.text_input.take() {
            text_input.destroy();
        }

        self.entered_surface = false;
        self.enabled = false;
        self.pending_ops.clear();
        self.preedit_active = false;
    }

    pub(super) fn protocol_text_active(&self) -> bool {
        self.text_input.is_some() && self.entered_surface && self.enabled
    }

    pub(super) fn update_render_state(
        &mut self,
        ime_enabled: bool,
        ime_cursor_area: Option<(f32, f32, f32, f32)>,
        ime_text_state: Option<TextInputState>,
    ) -> bool {
        let changed = self.render_enabled != ime_enabled
            || self.render_cursor_area != ime_cursor_area
            || self.render_text_state != ime_text_state;

        self.render_enabled = ime_enabled;
        self.render_cursor_area = ime_cursor_area;
        self.render_text_state = ime_text_state;
        changed
    }

    pub(super) fn sync(&mut self, _window: &Window, geometry: &SurfaceGeometry) {
        let Some(text_input) = self.text_input.as_ref() else {
            return;
        };

        let should_enable = self.entered_surface
            && self.render_enabled
            && self
                .render_text_state
                .as_ref()
                .is_some_and(|state| state.focused);

        if should_enable != self.enabled {
            if should_enable {
                text_input.enable();
            } else {
                text_input.disable();
            }

            self.enabled = should_enable;
        }

        if should_enable {
            if let Some(state) = self.render_text_state.as_ref() {
                let (cursor, anchor) = surrounding_text_offsets(state);
                text_input.set_surrounding_text(
                    state.content.clone(),
                    cursor as i32,
                    anchor as i32,
                );
                text_input.set_text_change_cause(ChangeCause::Other);
                text_input.set_content_type(ContentHint::None, ContentPurpose::Normal);
            }

            if let Some(rect) = self.render_cursor_area {
                let (x, y, width, height) = geometry.buffer_to_surface_rect(rect);
                text_input.set_cursor_rectangle(x, y, width, height);
            }
        }

        self.commit_serial = self.commit_serial.saturating_add(1);
        text_input.commit();
    }

    pub(super) fn handle_enter(&mut self, surface: &wl_surface::WlSurface, window: &Window) {
        self.entered_surface = surface == window.wl_surface();
    }

    pub(super) fn handle_leave(
        &mut self,
        surface: &wl_surface::WlSurface,
        window: &Window,
    ) -> bool {
        if surface != window.wl_surface() {
            return false;
        }

        self.entered_surface = false;
        self.enabled = false;
        self.pending_ops.clear();
        let had_preedit = self.preedit_active;
        self.preedit_active = false;
        had_preedit
    }

    pub(super) fn queue_preedit(
        &mut self,
        text: Option<String>,
        cursor_begin: i32,
        cursor_end: i32,
    ) {
        let cursor = text
            .as_deref()
            .and_then(|text| preedit_cursor_to_char_range(text, cursor_begin, cursor_end));
        self.pending_ops
            .push(PendingTextInputOp::Preedit { text, cursor });
    }

    pub(super) fn queue_commit(&mut self, text: Option<String>) {
        self.pending_ops.push(PendingTextInputOp::Commit(text));
    }

    pub(super) fn queue_delete_surrounding(&mut self, before_length: u32, after_length: u32) {
        self.pending_ops
            .push(PendingTextInputOp::DeleteSurrounding {
                before_length,
                after_length,
            });
    }

    pub(super) fn take_done_events(&mut self, _serial: u32) -> Vec<InputEvent> {
        let ops = std::mem::take(&mut self.pending_ops);
        if ops.is_empty() {
            return Vec::new();
        }

        let mut events = Vec::new();
        let requires_clear = self.preedit_active
            && ops.iter().any(|op| {
                matches!(
                    op,
                    PendingTextInputOp::Commit(_)
                        | PendingTextInputOp::DeleteSurrounding { .. }
                        | PendingTextInputOp::Preedit { .. }
                )
            });

        if requires_clear {
            events.push(InputEvent::TextPreeditClear);
            self.preedit_active = false;
        }

        for op in ops {
            match op {
                PendingTextInputOp::DeleteSurrounding {
                    before_length,
                    after_length,
                } => events.push(InputEvent::DeleteSurrounding {
                    before_length,
                    after_length,
                }),
                PendingTextInputOp::Commit(text) => {
                    if let Some(text) = text.and_then(|text| normalize_commit_text(&text)) {
                        events.push(InputEvent::TextCommit { text, mods: 0 });
                    }
                }
                PendingTextInputOp::Preedit { text, cursor } => {
                    if let Some(text) = text.filter(|text| !text.is_empty()) {
                        self.preedit_active = true;
                        events.push(InputEvent::TextPreedit { text, cursor });
                    } else {
                        self.preedit_active = false;
                        events.push(InputEvent::TextPreeditClear);
                    }
                }
            }
        }

        events
    }

    pub(super) fn preedit_active(&self) -> bool {
        self.preedit_active
    }
}

impl Dispatch<ZwpTextInputManagerV3, ()> for WaylandApp {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("zwp_text_input_manager_v3 has no events")
    }
}

impl Dispatch<ZwpTextInputV3, ()> for WaylandApp {
    fn event(
        state: &mut Self,
        _: &ZwpTextInputV3,
        event: <ZwpTextInputV3 as Proxy>::Event,
        _: &(),
        _conn: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            TextInputEvent::Enter { surface } => {
                state.text_input.handle_enter(&surface, &state.window);
                state.text_input.sync(&state.window, &state.geometry);
            }
            TextInputEvent::Leave { surface } => {
                if state.text_input.handle_leave(&surface, &state.window) {
                    state.keyboard.ime_preedit_active = false;
                    state.send_input_event(InputEvent::TextPreeditClear);
                }
            }
            TextInputEvent::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                state
                    .text_input
                    .queue_preedit(text, cursor_begin, cursor_end);
            }
            TextInputEvent::CommitString { text } => {
                state.text_input.queue_commit(text);
            }
            TextInputEvent::DeleteSurroundingText {
                before_length,
                after_length,
            } => {
                state
                    .text_input
                    .queue_delete_surrounding(before_length, after_length);
            }
            TextInputEvent::Done { serial } => {
                let events = state.text_input.take_done_events(serial);
                state.keyboard.ime_preedit_active = state.text_input.preedit_active();
                for event in events {
                    state.send_input_event(event);
                }
                state.text_input.sync(&state.window, &state.geometry);
            }
            _ => {}
        }
    }
}

fn surrounding_text_offsets(state: &TextInputState) -> (u32, u32) {
    (
        char_index_to_byte_index(&state.content, state.cursor) as u32,
        char_index_to_byte_index(
            &state.content,
            state.selection_anchor.unwrap_or(state.cursor),
        ) as u32,
    )
}

fn char_index_to_byte_index(text: &str, char_index: u32) -> usize {
    text.char_indices()
        .nth(char_index as usize)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn byte_index_to_char_index(text: &str, byte_index: usize) -> u32 {
    let clamped = byte_index.min(text.len());
    text.char_indices()
        .take_while(|(idx, _)| *idx < clamped)
        .count() as u32
}

fn preedit_cursor_to_char_range(
    text: &str,
    cursor_begin: i32,
    cursor_end: i32,
) -> Option<(u32, u32)> {
    if cursor_begin < 0 || cursor_end < 0 {
        return None;
    }

    let mut start = byte_index_to_char_index(text, cursor_begin as usize);
    let mut end = byte_index_to_char_index(text, cursor_end as usize);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    Some((start, end))
}

fn normalize_commit_text(text: &str) -> Option<String> {
    let filtered: String = text.chars().filter(|ch| !ch.is_control()).collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preedit_cursor_to_char_range_converts_byte_indices() {
        let cursor = preedit_cursor_to_char_range("aé中", 1, 6);
        assert_eq!(cursor, Some((1, 3)));
    }

    #[test]
    fn surrounding_text_offsets_use_utf8_byte_indices() {
        let state = TextInputState {
            content: "aé中".to_string(),
            content_len: 3,
            cursor: 2,
            selection_anchor: Some(1),
            ..Default::default()
        };

        assert_eq!(surrounding_text_offsets(&state), (3, 1));
    }
}
