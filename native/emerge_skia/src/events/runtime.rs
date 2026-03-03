use std::{
    collections::{HashMap, HashSet},
    env, thread,
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use rustler::LocalPid;

use crate::{
    actors::{EventMsg, TreeMsg},
    clipboard::{ClipboardManager, ClipboardTarget},
    input::{InputEvent, InputHandler},
    renderer::make_font_with_style,
    tree::{attrs::TextAlign, element::ElementId},
};

use super::dispatch_outcome::{DispatchOutcome, ElementEventKind};
use super::{
    EventNode, EventProcessor, TextInputCommandRequest, TextInputCursorRequest,
    TextInputDescriptor, TextInputEditRequest, TextInputPreeditRequest, blur_atom, change_atom,
    click_atom, focus_atom, mouse_down_atom, mouse_enter_atom, mouse_leave_atom, mouse_move_atom,
    mouse_up_atom, press_atom, registry_v2::TriggerId, send_element_event,
    send_element_event_with_string_payload, send_input_event,
};

#[derive(Clone, Debug)]
struct TextInputSession {
    descriptor: TextInputDescriptor,
    content: String,
    cursor: u32,
    selection_anchor: Option<u32>,
    preedit: Option<String>,
    preedit_cursor: Option<(u32, u32)>,
    focused: bool,
}

impl TextInputSession {
    fn from_descriptor(descriptor: TextInputDescriptor) -> Self {
        let len = text_char_len(&descriptor.content);
        Self {
            content: descriptor.content.clone(),
            descriptor,
            cursor: len,
            selection_anchor: None,
            preedit: None,
            preedit_cursor: None,
            focused: false,
        }
    }
}

#[derive(Clone, Debug)]
struct V2OnlyNoPredictionStats {
    total: u64,
    unclassified: u64,
    by_trigger: Vec<u64>,
}

impl V2OnlyNoPredictionStats {
    fn new() -> Self {
        Self {
            total: 0,
            unclassified: 0,
            by_trigger: vec![0; TriggerId::COUNT],
        }
    }

    fn record(&mut self, trigger: Option<TriggerId>) {
        self.total += 1;
        if let Some(trigger) = trigger {
            self.by_trigger[trigger.index()] += 1;
        } else {
            self.unclassified += 1;
        }
    }
}

fn send_tree(tree_tx: &Sender<TreeMsg>, msg: TreeMsg, log_render: bool) {
    match tree_tx.try_send(msg) {
        Ok(()) => {}
        Err(TrySendError::Full(msg)) => {
            if log_render {
                eprintln!("tree channel full, blocking send");
            }
            let _ = tree_tx.send(msg);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

fn send_element_event_if_target(target: Option<LocalPid>, element_id: &ElementId, event: rustler::Atom) {
    if let Some(pid) = target {
        send_element_event(pid, element_id, event);
    }
}

fn send_element_event_with_string_payload_if_target(
    target: Option<LocalPid>,
    element_id: &ElementId,
    event: rustler::Atom,
    value: &str,
) {
    if let Some(pid) = target {
        send_element_event_with_string_payload(pid, element_id, event, value);
    }
}

fn event_kind_to_atom(kind: ElementEventKind) -> rustler::Atom {
    match kind {
        ElementEventKind::Click => click_atom(),
        ElementEventKind::Press => press_atom(),
        ElementEventKind::MouseDown => mouse_down_atom(),
        ElementEventKind::MouseUp => mouse_up_atom(),
        ElementEventKind::MouseEnter => mouse_enter_atom(),
        ElementEventKind::MouseLeave => mouse_leave_atom(),
        ElementEventKind::MouseMove => mouse_move_atom(),
        ElementEventKind::Focus => focus_atom(),
        ElementEventKind::Blur => blur_atom(),
        ElementEventKind::Change => change_atom(),
    }
}

fn env_flag_enabled(name: &str) -> bool {
    let Ok(value) = env::var(name) else {
        return false;
    };

    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn event_is_dispatch_candidate(event: &InputEvent) -> bool {
    match event {
        InputEvent::CursorPos { .. }
        | InputEvent::CursorButton { .. }
        | InputEvent::CursorScroll { .. }
        | InputEvent::CursorScrollLines { .. }
        | InputEvent::Key { .. }
        | InputEvent::TextCommit { .. }
        | InputEvent::TextPreedit { .. }
        | InputEvent::TextPreeditClear
        | InputEvent::CursorEntered { .. }
        | InputEvent::Focused { .. } => true,
        InputEvent::Resized { .. } => false,
    }
}

fn maybe_dump_v2_no_prediction_stats(
    stats: &V2OnlyNoPredictionStats,
    reason: &str,
) {
    eprintln!(
        "v2-only no-prediction reason={reason} total={} unclassified={}",
        stats.total, stats.unclassified
    );

    for (index, count) in stats.by_trigger.iter().copied().enumerate() {
        if count == 0 {
            continue;
        }

        eprintln!(
            "v2-only no-prediction trigger_index={} count={}",
            index, count
        );
    }
}

fn text_char_len(content: &str) -> u32 {
    content.chars().count() as u32
}

fn char_to_byte_index(content: &str, char_index: u32) -> usize {
    content
        .char_indices()
        .nth(char_index as usize)
        .map(|(idx, _)| idx)
        .unwrap_or(content.len())
}

fn selected_range(cursor: u32, selection_anchor: Option<u32>) -> Option<(u32, u32)> {
    let anchor = selection_anchor?;
    if anchor == cursor {
        return None;
    }
    Some((anchor.min(cursor), anchor.max(cursor)))
}

fn normalize_preedit_cursor(text: Option<&str>, cursor: Option<(u32, u32)>) -> Option<(u32, u32)> {
    let text_len = text.map(text_char_len)?;
    let (mut start, mut end) = cursor?;
    start = start.min(text_len);
    end = end.min(text_len);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    Some((start, end))
}

fn normalize_session_runtime(session: &mut TextInputSession) -> bool {
    let mut changed = false;
    let len = text_char_len(&session.content);

    if session.cursor > len {
        session.cursor = len;
        changed = true;
    }

    if let Some(anchor) = session.selection_anchor {
        let anchor = anchor.min(len);
        let next_anchor = if anchor == session.cursor {
            None
        } else {
            Some(anchor)
        };
        if session.selection_anchor != next_anchor {
            session.selection_anchor = next_anchor;
            changed = true;
        }
    }

    if !session.focused {
        if session.selection_anchor.take().is_some() {
            changed = true;
        }
        if session.preedit.take().is_some() {
            changed = true;
        }
        if session.preedit_cursor.take().is_some() {
            changed = true;
        }
    } else {
        let normalized =
            normalize_preedit_cursor(session.preedit.as_deref(), session.preedit_cursor);
        if session.preedit_cursor != normalized {
            session.preedit_cursor = normalized;
            changed = true;
        }
    }

    changed
}

fn clear_preedit(session: &mut TextInputSession) -> bool {
    let had_preedit = session.preedit.take().is_some();
    let had_cursor = session.preedit_cursor.take().is_some();
    had_preedit || had_cursor
}

fn selection_text(session: &TextInputSession) -> Option<String> {
    let (start, end) = selected_range(session.cursor, session.selection_anchor)?;
    Some(
        session
            .content
            .chars()
            .skip(start as usize)
            .take((end - start) as usize)
            .collect(),
    )
}

fn apply_content_change(session: &mut TextInputSession, next_content: String, next_cursor: u32) {
    session.content = next_content;
    session.cursor = next_cursor.min(text_char_len(&session.content));
    session.selection_anchor = None;
    session.preedit = None;
    session.preedit_cursor = None;
}

fn replace_selection_or_insert(
    session: &mut TextInputSession,
    insert_text: &str,
) -> Option<String> {
    if insert_text.is_empty() {
        return None;
    }

    let (start, end) = selected_range(session.cursor, session.selection_anchor)
        .unwrap_or((session.cursor, session.cursor));

    let mut next = session.content.clone();
    let start_byte = char_to_byte_index(&next, start);
    let end_byte = char_to_byte_index(&next, end);
    next.replace_range(start_byte..end_byte, insert_text);

    if next == session.content {
        return None;
    }

    let next_cursor = start + text_char_len(insert_text);
    apply_content_change(session, next.clone(), next_cursor);
    Some(next)
}

fn delete_backward(session: &mut TextInputSession) -> Option<String> {
    let (start, end) = selected_range(session.cursor, session.selection_anchor)
        .unwrap_or((session.cursor, session.cursor));

    if start != end {
        let mut next = session.content.clone();
        let start_byte = char_to_byte_index(&next, start);
        let end_byte = char_to_byte_index(&next, end);
        next.replace_range(start_byte..end_byte, "");
        apply_content_change(session, next.clone(), start);
        return Some(next);
    }

    if session.cursor == 0 {
        return None;
    }

    let mut next = session.content.clone();
    let start_byte = char_to_byte_index(&next, session.cursor - 1);
    let end_byte = char_to_byte_index(&next, session.cursor);
    next.replace_range(start_byte..end_byte, "");
    apply_content_change(session, next.clone(), session.cursor - 1);
    Some(next)
}

fn delete_forward(session: &mut TextInputSession) -> Option<String> {
    let (start, end) = selected_range(session.cursor, session.selection_anchor)
        .unwrap_or((session.cursor, session.cursor));

    if start != end {
        let mut next = session.content.clone();
        let start_byte = char_to_byte_index(&next, start);
        let end_byte = char_to_byte_index(&next, end);
        next.replace_range(start_byte..end_byte, "");
        apply_content_change(session, next.clone(), start);
        return Some(next);
    }

    let len = text_char_len(&session.content);
    if session.cursor >= len {
        return None;
    }

    let mut next = session.content.clone();
    let start_byte = char_to_byte_index(&next, session.cursor);
    let end_byte = char_to_byte_index(&next, session.cursor + 1);
    next.replace_range(start_byte..end_byte, "");
    apply_content_change(session, next.clone(), session.cursor);
    Some(next)
}

fn cut_selection(session: &mut TextInputSession) -> Option<(String, String)> {
    let (start, end) = selected_range(session.cursor, session.selection_anchor)?;
    let selected = selection_text(session)?;

    if selected.is_empty() {
        return None;
    }

    let mut next = session.content.clone();
    let start_byte = char_to_byte_index(&next, start);
    let end_byte = char_to_byte_index(&next, end);
    next.replace_range(start_byte..end_byte, "");
    apply_content_change(session, next.clone(), start);
    Some((next, selected))
}

fn sanitize_single_line_text(text: &str) -> String {
    text.chars()
        .filter_map(|ch| {
            if ch == '\n' || ch == '\r' || ch == '\t' {
                Some(' ')
            } else if ch.is_control() {
                None
            } else {
                Some(ch)
            }
        })
        .collect()
}

fn move_cursor(session: &mut TextInputSession, next_cursor: u32, extend_selection: bool) -> bool {
    let len = text_char_len(&session.content);
    let next_cursor = next_cursor.min(len);
    let mut changed = false;

    if extend_selection {
        let anchor = session.selection_anchor.unwrap_or(session.cursor);
        let next_anchor = if anchor == next_cursor {
            None
        } else {
            Some(anchor)
        };
        if session.selection_anchor != next_anchor {
            session.selection_anchor = next_anchor;
            changed = true;
        }
    } else if session.selection_anchor.take().is_some() {
        changed = true;
    }

    if session.cursor != next_cursor {
        session.cursor = next_cursor;
        changed = true;
    }

    if clear_preedit(session) {
        changed = true;
    }

    changed
}

fn measure_text_width(text: &str, descriptor: &TextInputDescriptor) -> f32 {
    if text.is_empty() {
        return 0.0;
    }

    let font = make_font_with_style(
        &descriptor.font_family,
        descriptor.font_weight,
        descriptor.font_italic,
        descriptor.font_size,
    );

    let mut total = 0.0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
        total += glyph_width;

        if chars.peek().is_some() {
            total += descriptor.letter_spacing;
            if ch.is_whitespace() {
                total += descriptor.word_spacing;
            }
        }
    }

    total
}

fn nearest_char_index_for_offset(
    text: &str,
    descriptor: &TextInputDescriptor,
    offset_x: f32,
) -> u32 {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return 0;
    }

    let font = make_font_with_style(
        &descriptor.font_family,
        descriptor.font_weight,
        descriptor.font_italic,
        descriptor.font_size,
    );

    let mut positions = Vec::with_capacity(chars.len() + 1);
    positions.push(0.0);

    let mut advance = 0.0;
    for (idx, ch) in chars.iter().enumerate() {
        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
        advance += glyph_width;
        if idx + 1 < chars.len() {
            advance += descriptor.letter_spacing;
            if ch.is_whitespace() {
                advance += descriptor.word_spacing;
            }
        }
        positions.push(advance);
    }

    for idx in 0..chars.len() {
        let midpoint = positions[idx] + (positions[idx + 1] - positions[idx]) / 2.0;
        if offset_x <= midpoint {
            return idx as u32;
        }
    }

    chars.len() as u32
}

fn cursor_from_click_x(session: &TextInputSession, x: f32) -> u32 {
    let descriptor = &session.descriptor;
    let text_width = measure_text_width(&session.content, descriptor);
    let content_width =
        (descriptor.frame_width - descriptor.inset_left - descriptor.inset_right).max(0.0);

    let text_start_x = match descriptor.text_align {
        TextAlign::Left => descriptor.frame_x + descriptor.inset_left,
        TextAlign::Center => {
            descriptor.frame_x + descriptor.inset_left + (content_width - text_width) / 2.0
        }
        TextAlign::Right => {
            descriptor.frame_x + descriptor.frame_width - descriptor.inset_right - text_width
        }
    };

    let local_x = (x - text_start_x).clamp(0.0, text_width.max(0.0));
    nearest_char_index_for_offset(&session.content, descriptor, local_x)
}

fn send_runtime_update(
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    element_id: &ElementId,
    session: &TextInputSession,
) {
    send_tree(
        tree_tx,
        TreeMsg::SetTextInputRuntime {
            element_id: element_id.clone(),
            focused: session.focused,
            cursor: Some(session.cursor),
            selection_anchor: session.selection_anchor,
            preedit: session.preedit.clone(),
            preedit_cursor: session.preedit_cursor,
        },
        log_render,
    );
}

fn send_content_update(
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    element_id: &ElementId,
    content: String,
) {
    send_tree(
        tree_tx,
        TreeMsg::SetTextInputContent {
            element_id: element_id.clone(),
            content,
        },
        log_render,
    );
}

fn emit_change_event(
    target: &Option<LocalPid>,
    element_id: &ElementId,
    value: &str,
) {
    send_element_event_with_string_payload_if_target(
        target.as_ref().copied(),
        element_id,
        change_atom(),
        value,
    );
}

#[cfg(test)]
fn change_payload_for_target(predicted: &DispatchOutcome, element_id: Option<&ElementId>) -> Option<String> {
    let target = super::dispatch_outcome::node_key(element_id?);
    predicted
        .element_events
        .iter()
        .find(|event| event.kind == ElementEventKind::Change && event.target == target)
        .and_then(|event| event.payload.clone())
}

fn upsert_predicted_change_event(
    predicted: &mut super::dispatch_outcome::DispatchOutcome,
    element_id: &ElementId,
    next_content: String,
) {
    let target = super::dispatch_outcome::node_key(element_id);

    if let Some(existing) = predicted
        .element_events
        .iter_mut()
        .find(|event| event.kind == ElementEventKind::Change && event.target == target)
    {
        existing.payload = Some(next_content);
    } else {
        predicted
            .element_events
            .push(super::dispatch_outcome::ElementEventOut {
                target,
                kind: ElementEventKind::Change,
                payload: Some(next_content),
            });
    }
}

fn remove_predicted_change_event(
    predicted: &mut super::dispatch_outcome::DispatchOutcome,
    element_id: &ElementId,
) {
    let target = super::dispatch_outcome::node_key(element_id);
    predicted
        .element_events
        .retain(|event| !(event.kind == ElementEventKind::Change && event.target == target));
}

fn enrich_predicted_command_change_events(
    predicted: &mut super::dispatch_outcome::DispatchOutcome,
    event: &InputEvent,
    processor: &EventProcessor,
    target: &Option<LocalPid>,
    sessions: &HashMap<ElementId, TextInputSession>,
    clipboard: &mut ClipboardManager,
) {
    if target.is_none() {
        return;
    }

    let Some((element_id, request)) = processor.text_input_command_request(event) else {
        return;
    };
    let Some(session) = sessions.get(&element_id) else {
        return;
    };

    let mut shadow_session = session.clone();
    let next_content =
        match request {
            TextInputCommandRequest::Cut => {
                cut_selection(&mut shadow_session).map(|(next_content, _selected)| next_content)
            }
            TextInputCommandRequest::Paste => clipboard
                .get_text(ClipboardTarget::Clipboard)
                .and_then(|pasted| {
                    let sanitized = sanitize_single_line_text(&pasted);
                    replace_selection_or_insert(&mut shadow_session, &sanitized)
                }),
            TextInputCommandRequest::PastePrimary => clipboard
                .get_text(ClipboardTarget::Primary)
                .and_then(|pasted| {
                    let sanitized = sanitize_single_line_text(&pasted);
                    replace_selection_or_insert(&mut shadow_session, &sanitized)
                }),
            TextInputCommandRequest::SelectAll | TextInputCommandRequest::Copy => None,
        };

    if let Some(next_content) = next_content {
        upsert_predicted_change_event(predicted, &element_id, next_content);
    } else {
        remove_predicted_change_event(predicted, &element_id);
    }
}

fn enrich_predicted_edit_change_events(
    predicted: &mut super::dispatch_outcome::DispatchOutcome,
    event: &InputEvent,
    processor: &EventProcessor,
    sessions: &HashMap<ElementId, TextInputSession>,
    allow_change_events: bool,
) {
    let Some((element_id, request)) = processor.text_input_edit_request(event) else {
        return;
    };

    if !allow_change_events {
        remove_predicted_change_event(predicted, &element_id);
        return;
    }

    let Some(session) = sessions.get(&element_id) else {
        remove_predicted_change_event(predicted, &element_id);
        return;
    };

    let mut shadow_session = session.clone();
    let next_content = match request {
        TextInputEditRequest::MoveLeft { .. }
        | TextInputEditRequest::MoveRight { .. }
        | TextInputEditRequest::MoveHome { .. }
        | TextInputEditRequest::MoveEnd { .. } => None,
        TextInputEditRequest::Backspace => delete_backward(&mut shadow_session),
        TextInputEditRequest::Delete => delete_forward(&mut shadow_session),
        TextInputEditRequest::Insert(text) => {
            replace_selection_or_insert(&mut shadow_session, &text)
        }
    };

    if let Some(next_content) = next_content {
        upsert_predicted_change_event(predicted, &element_id, next_content);
    } else {
        remove_predicted_change_event(predicted, &element_id);
    }
}

fn element_id_from_node_key(key: &super::dispatch_outcome::NodeKey) -> ElementId {
    ElementId(key.0.clone())
}

fn float_from_milli(value: super::dispatch_outcome::Milli) -> f32 {
    value.0 as f32 / 1000.0
}

fn apply_v2_dispatch_outcome(
    outcome: DispatchOutcome,
    processor: &mut EventProcessor,
    target: &Option<LocalPid>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    sessions: &mut HashMap<ElementId, TextInputSession>,
    focused: &mut Option<ElementId>,
    clipboard: &mut ClipboardManager,
) {
    if let Some(next_focus) = outcome.focus_change {
        let next_focus = next_focus.as_ref().map(element_id_from_node_key);
        apply_focus_change(
            next_focus.clone(),
            focused,
            target,
            sessions,
            tree_tx,
            log_render,
        );
        processor.set_focused_id_for_runtime(next_focus);
    }

    for request in outcome.text_command_requests {
        let element_id = element_id_from_node_key(&request.target);
        if let Some(session) = sessions.get_mut(&element_id) {
            match request.request {
                TextInputCommandRequest::SelectAll => {
                    let len = text_char_len(&session.content);
                    let mut changed = if len == 0 {
                        session.selection_anchor.take().is_some()
                    } else {
                        let changed_cursor = session.cursor != len;
                        let changed_anchor = session.selection_anchor != Some(0);
                        session.cursor = len;
                        session.selection_anchor = Some(0);
                        changed_cursor || changed_anchor
                    };

                    if clear_preedit(session) {
                        changed = true;
                    }

                    if changed {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        sync_primary_selection(session, clipboard);
                    }
                }
                TextInputCommandRequest::Copy => {
                    if let Some(selection) = selection_text(session)
                        && !selection.is_empty()
                    {
                        clipboard.set_text(ClipboardTarget::Clipboard, &selection);
                        clipboard.set_text(ClipboardTarget::Primary, &selection);
                    }
                }
                TextInputCommandRequest::Cut => {
                    if let Some((next_content, selected)) = cut_selection(session) {
                        clipboard.set_text(ClipboardTarget::Clipboard, &selected);
                        clipboard.set_text(ClipboardTarget::Primary, &selected);
                        send_content_update(tree_tx, log_render, &element_id, next_content.clone());
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        emit_change_event(target, &element_id, &next_content);
                    }
                }
                TextInputCommandRequest::Paste => {
                    if let Some(pasted) = clipboard.get_text(ClipboardTarget::Clipboard) {
                        let pasted = sanitize_single_line_text(&pasted);
                        if let Some(next_content) = replace_selection_or_insert(session, &pasted) {
                            send_content_update(
                                tree_tx,
                                log_render,
                                &element_id,
                                next_content.clone(),
                            );
                            send_runtime_update(tree_tx, log_render, &element_id, session);
                            emit_change_event(target, &element_id, &next_content);
                        }
                    }
                }
                TextInputCommandRequest::PastePrimary => {
                    if let Some(pasted) = clipboard.get_text(ClipboardTarget::Primary) {
                        let pasted = sanitize_single_line_text(&pasted);
                        if let Some(next_content) = replace_selection_or_insert(session, &pasted) {
                            send_content_update(
                                tree_tx,
                                log_render,
                                &element_id,
                                next_content.clone(),
                            );
                            send_runtime_update(tree_tx, log_render, &element_id, session);
                            emit_change_event(target, &element_id, &next_content);
                        }
                    }
                }
            }
        }
    }

    for request in outcome.text_edit_requests {
        let element_id = element_id_from_node_key(&request.target);
        if let Some(session) = sessions.get_mut(&element_id) {
            match request.request {
                TextInputEditRequest::MoveLeft { extend_selection } => {
                    let next_cursor = if !extend_selection {
                        if let Some((start, _end)) =
                            selected_range(session.cursor, session.selection_anchor)
                        {
                            start
                        } else {
                            session.cursor.saturating_sub(1)
                        }
                    } else {
                        session.cursor.saturating_sub(1)
                    };

                    if move_cursor(session, next_cursor, extend_selection) {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        if extend_selection {
                            sync_primary_selection(session, clipboard);
                        }
                    }
                }
                TextInputEditRequest::MoveRight { extend_selection } => {
                    let len = text_char_len(&session.content);
                    let next_cursor = if !extend_selection {
                        if let Some((_start, end)) =
                            selected_range(session.cursor, session.selection_anchor)
                        {
                            end
                        } else {
                            (session.cursor + 1).min(len)
                        }
                    } else {
                        (session.cursor + 1).min(len)
                    };

                    if move_cursor(session, next_cursor, extend_selection) {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        if extend_selection {
                            sync_primary_selection(session, clipboard);
                        }
                    }
                }
                TextInputEditRequest::MoveHome { extend_selection } => {
                    if move_cursor(session, 0, extend_selection) {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        if extend_selection {
                            sync_primary_selection(session, clipboard);
                        }
                    }
                }
                TextInputEditRequest::MoveEnd { extend_selection } => {
                    let len = text_char_len(&session.content);
                    if move_cursor(session, len, extend_selection) {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        if extend_selection {
                            sync_primary_selection(session, clipboard);
                        }
                    }
                }
                TextInputEditRequest::Backspace => {
                    if let Some(next_content) = delete_backward(session) {
                        send_content_update(tree_tx, log_render, &element_id, next_content.clone());
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        emit_change_event(target, &element_id, &next_content);
                    }
                }
                TextInputEditRequest::Delete => {
                    if let Some(next_content) = delete_forward(session) {
                        send_content_update(tree_tx, log_render, &element_id, next_content.clone());
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        emit_change_event(target, &element_id, &next_content);
                    }
                }
                TextInputEditRequest::Insert(text) => {
                    if let Some(next_content) = replace_selection_or_insert(session, &text) {
                        send_content_update(tree_tx, log_render, &element_id, next_content.clone());
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                        emit_change_event(target, &element_id, &next_content);
                    }
                }
            }
        }
    }

    for request in outcome.text_preedit_requests {
        let element_id = element_id_from_node_key(&request.target);
        if let Some(session) = sessions.get_mut(&element_id) {
            match request.request {
                TextInputPreeditRequest::Set { text, cursor } => {
                    let next_preedit = if text.is_empty() { None } else { Some(text) };
                    let next_cursor = normalize_preedit_cursor(next_preedit.as_deref(), cursor);

                    let mut changed = false;
                    if session.preedit != next_preedit {
                        session.preedit = next_preedit;
                        changed = true;
                    }
                    if session.preedit_cursor != next_cursor {
                        session.preedit_cursor = next_cursor;
                        changed = true;
                    }

                    if changed {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                    }
                }
                TextInputPreeditRequest::Clear => {
                    if clear_preedit(session) {
                        send_runtime_update(tree_tx, log_render, &element_id, session);
                    }
                }
            }
        }
    }

    for request in outcome.scrollbar_thumb_drag_requests {
        let element_id = element_id_from_node_key(&request.target);
        match request.axis {
            super::dispatch_outcome::ScrollbarAxisOut::X => send_tree(
                tree_tx,
                TreeMsg::ScrollbarThumbDragX {
                    element_id,
                    dx: float_from_milli(request.delta),
                },
                log_render,
            ),
            super::dispatch_outcome::ScrollbarAxisOut::Y => send_tree(
                tree_tx,
                TreeMsg::ScrollbarThumbDragY {
                    element_id,
                    dy: float_from_milli(request.delta),
                },
                log_render,
            ),
        }
    }

    for request in outcome.scroll_requests {
        send_tree(
            tree_tx,
            TreeMsg::ScrollRequest {
                element_id: element_id_from_node_key(&request.target),
                dx: float_from_milli(request.dx),
                dy: float_from_milli(request.dy),
            },
            log_render,
        );
    }

    for request in outcome.scrollbar_hover_requests {
        let element_id = element_id_from_node_key(&request.target);
        match request.axis {
            super::dispatch_outcome::ScrollbarAxisOut::X => send_tree(
                tree_tx,
                TreeMsg::SetScrollbarXHover {
                    element_id,
                    hovered: request.hovered,
                },
                log_render,
            ),
            super::dispatch_outcome::ScrollbarAxisOut::Y => send_tree(
                tree_tx,
                TreeMsg::SetScrollbarYHover {
                    element_id,
                    hovered: request.hovered,
                },
                log_render,
            ),
        }
    }

    for request in outcome.style_runtime_requests {
        let element_id = element_id_from_node_key(&request.target);
        match request.kind {
            super::dispatch_outcome::StyleRuntimeKind::MouseOver => send_tree(
                tree_tx,
                TreeMsg::SetMouseOverActive {
                    element_id,
                    active: request.active,
                },
                log_render,
            ),
            super::dispatch_outcome::StyleRuntimeKind::MouseDown => send_tree(
                tree_tx,
                TreeMsg::SetMouseDownActive {
                    element_id,
                    active: request.active,
                },
                log_render,
            ),
        }
    }

    if let Some(pid) = target.as_ref().copied() {
        for event in outcome.element_events {
            let element_id = element_id_from_node_key(&event.target);
            match event.kind {
                ElementEventKind::Focus | ElementEventKind::Blur | ElementEventKind::Change => {}
                _ => {
                    let atom = event_kind_to_atom(event.kind);
                    if let Some(payload) = event.payload {
                        send_element_event_with_string_payload(pid, &element_id, atom, &payload);
                    } else {
                        send_element_event(pid, &element_id, atom);
                    }
                }
            }
        }
    }
}

fn advance_processor_state_after_v2_event(processor: &mut EventProcessor, event: &InputEvent) {
    processor.detect_click(event);
    processor.detect_press(event);
    processor.detect_mouse_button_event(event);
    processor.handle_hover_event(event);
    processor.scrollbar_thumb_drag_requests(event);
    processor.scrollbar_hover_requests(event);
    processor.mouse_over_requests(event);
    processor.mouse_down_requests(event);
}

fn sync_primary_selection(session: &TextInputSession, clipboard: &mut ClipboardManager) {
    if let Some(text) = selection_text(session)
        && !text.is_empty()
    {
        clipboard.set_text(ClipboardTarget::Primary, &text);
    }
}

fn reconcile_text_input_sessions(
    registry: &[EventNode],
    sessions: &mut HashMap<ElementId, TextInputSession>,
    focused: &mut Option<ElementId>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
) {
    let mut seen = HashSet::new();

    for node in registry {
        let Some(descriptor) = node.text_input.clone() else {
            continue;
        };

        let id = node.id.clone();
        seen.insert(id.clone());
        let should_focus = focused.as_ref().is_some_and(|focused_id| focused_id == &id);

        let session = sessions
            .entry(id.clone())
            .or_insert_with(|| TextInputSession::from_descriptor(descriptor.clone()));

        let mut changed = false;
        if session.descriptor != descriptor {
            session.descriptor = descriptor.clone();
        }

        if session.content != descriptor.content {
            if should_focus {
                send_content_update(tree_tx, log_render, &id, session.content.clone());
            } else {
                session.content = descriptor.content;
                session.selection_anchor = None;
                session.preedit = None;
                session.preedit_cursor = None;
                changed = true;
            }
        }

        if session.focused != should_focus {
            session.focused = should_focus;
            changed = true;
        }

        if normalize_session_runtime(session) {
            changed = true;
        }

        if should_focus {
            let content_len = text_char_len(&session.content);
            session.descriptor.content = session.content.clone();
            session.descriptor.content_len = content_len;
            session.descriptor.cursor = session.cursor;
            session.descriptor.selection_anchor = session.selection_anchor;
        }

        if changed {
            send_runtime_update(tree_tx, log_render, &id, session);
        }
    }

    sessions.retain(|id, _| seen.contains(id));
}

fn apply_focus_change(
    next_focus: Option<ElementId>,
    focused: &mut Option<ElementId>,
    target: &Option<LocalPid>,
    sessions: &mut HashMap<ElementId, TextInputSession>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
) {
    let previous_focus = focused.clone();
    *focused = next_focus.clone();

    if previous_focus == next_focus {
        return;
    }

    if let Some(prev_id) = previous_focus {
        send_element_event_if_target(target.as_ref().copied(), &prev_id, blur_atom());

        send_tree(
            tree_tx,
            TreeMsg::SetFocusedActive {
                element_id: prev_id.clone(),
                active: false,
            },
            log_render,
        );

        if let Some(session) = sessions.get_mut(&prev_id) {
            session.focused = false;
            session.selection_anchor = None;
            clear_preedit(session);
            normalize_session_runtime(session);
            send_runtime_update(tree_tx, log_render, &prev_id, session);
        }
    }

    if let Some(next_id) = next_focus {
        send_element_event_if_target(target.as_ref().copied(), &next_id, focus_atom());

        send_tree(
            tree_tx,
            TreeMsg::SetFocusedActive {
                element_id: next_id.clone(),
                active: true,
            },
            log_render,
        );

        if let Some(session) = sessions.get_mut(&next_id) {
            session.focused = true;
            normalize_session_runtime(session);
            send_runtime_update(tree_tx, log_render, &next_id, session);
        }
    }
}

fn process_input_events(
    events: &mut Vec<InputEvent>,
    processor: &mut EventProcessor,
    input_handler: &mut InputHandler,
    target: &Option<LocalPid>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    sessions: &mut HashMap<ElementId, TextInputSession>,
    focused: &mut Option<ElementId>,
    clipboard: &mut ClipboardManager,
    v2_no_prediction_stats: &mut V2OnlyNoPredictionStats,
    v2_no_prediction_verbose: bool,
) {
    if events.is_empty() {
        return;
    }

    let mut coalesced = Vec::new();
    let mut last_cursor: Option<InputEvent> = None;
    let mut scroll_acc: Option<(f32, f32, f32, f32)> = None;

    for event in events.drain(..) {
        let event = event.normalize_scroll();
        match event {
            InputEvent::CursorPos { .. } => {
                last_cursor = Some(event);
            }
            InputEvent::CursorScroll { dx, dy, x, y } => {
                scroll_acc = Some(match scroll_acc {
                    Some((acc_dx, acc_dy, _, _)) => (acc_dx + dx, acc_dy + dy, x, y),
                    None => (dx, dy, x, y),
                });
            }
            other => coalesced.push(other),
        }
    }

    if let Some((dx, dy, x, y)) = scroll_acc {
        coalesced.push(InputEvent::CursorScroll { dx, dy, x, y });
    }
    if let Some(cursor) = last_cursor {
        coalesced.push(cursor);
    }

    for event in coalesced {
        let trigger = processor.v2_trigger_for_input_event(&event);
        let mut predicted_outcome: Option<DispatchOutcome> = None;

        if let Some(mut predicted) =
            processor.preview_v2_keyboard_focus_outcome(&event, focused.as_ref())
        {
            enrich_predicted_command_change_events(
                &mut predicted,
                &event,
                processor,
                target,
                sessions,
                clipboard,
            );

            enrich_predicted_edit_change_events(
                &mut predicted,
                &event,
                processor,
                sessions,
                target.is_some(),
            );

            predicted_outcome = Some(predicted);
        }

        if let InputEvent::Resized {
            width,
            height,
            scale_factor,
        } = &event
        {
            send_tree(
                tree_tx,
                TreeMsg::Resize {
                    width: *width as f32,
                    height: *height as f32,
                    scale: *scale_factor,
                },
                log_render,
            );
        }

        if let InputEvent::CursorPos { x, y } = &event {
            input_handler.set_cursor_pos(*x, *y);
        }

        let forward_to_target = input_handler.accepts(&event);

        if let Some(pid) = target.as_ref()
            && forward_to_target
        {
            send_input_event(*pid, &event);
        }

        for request in processor.text_input_cursor_requests(&event) {
            match request {
                TextInputCursorRequest::Set {
                    element_id,
                    x,
                    extend_selection,
                } => {
                    if let Some(session) = sessions.get_mut(&element_id) {
                        let next_cursor = cursor_from_click_x(session, x);
                        if move_cursor(session, next_cursor, extend_selection) {
                            send_runtime_update(tree_tx, log_render, &element_id, session);
                            if extend_selection {
                                sync_primary_selection(session, clipboard);
                            }
                        }
                    }
                }
            }
        }

        if let Some(outcome) = predicted_outcome {
            apply_v2_dispatch_outcome(
                outcome,
                processor,
                target,
                tree_tx,
                log_render,
                sessions,
                focused,
                clipboard,
            );
            advance_processor_state_after_v2_event(processor, &event);
        } else if event_is_dispatch_candidate(&event) {
            advance_processor_state_after_v2_event(processor, &event);
            v2_no_prediction_stats.record(trigger);
            if v2_no_prediction_verbose {
                eprintln!(
                    "v2-only no-prediction trigger={:?} event={:?}",
                    trigger, event
                );
            }
        }

    }
}

pub(crate) fn spawn_event_actor(
    event_rx: Receiver<EventMsg>,
    tree_tx: Sender<TreeMsg>,
    log_render: bool,
    system_clipboard: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut processor = EventProcessor::new();
        let mut input_handler = InputHandler::new();
        let mut target: Option<LocalPid> = None;
        let mut sessions: HashMap<ElementId, TextInputSession> = HashMap::new();
        let mut focused: Option<ElementId> = None;
        let mut clipboard = ClipboardManager::new(system_clipboard);

        let mut v2_no_prediction_stats = V2OnlyNoPredictionStats::new();
        let v2_no_prediction_verbose = env_flag_enabled("EMERGE_SKIA_V2_NO_PRED_VERBOSE");

        while let Ok(msg) = event_rx.recv() {
            let mut messages = vec![msg];
            while let Ok(next) = event_rx.try_recv() {
                messages.push(next);
            }

            let mut pending_inputs = Vec::new();

            for message in messages {
                match message {
                    EventMsg::InputEvent(event) => pending_inputs.push(event),
                    EventMsg::RegistryUpdate { registry } => {
                        process_input_events(
                            &mut pending_inputs,
                            &mut processor,
                            &mut input_handler,
                            &target,
                            &tree_tx,
                            log_render,
                            &mut sessions,
                            &mut focused,
                            &mut clipboard,
                            &mut v2_no_prediction_stats,
                            v2_no_prediction_verbose,
                        );

                        processor.rebuild_registry(registry.clone());
                        let next_focus = processor.focused_id();
                        apply_focus_change(
                            next_focus,
                            &mut focused,
                            &target,
                            &mut sessions,
                            &tree_tx,
                            log_render,
                        );
                        reconcile_text_input_sessions(
                            &registry,
                            &mut sessions,
                            &mut focused,
                            &tree_tx,
                            log_render,
                        );
                    }
                    EventMsg::SetInputMask(mask) => {
                        process_input_events(
                            &mut pending_inputs,
                            &mut processor,
                            &mut input_handler,
                            &target,
                            &tree_tx,
                            log_render,
                            &mut sessions,
                            &mut focused,
                            &mut clipboard,
                            &mut v2_no_prediction_stats,
                            v2_no_prediction_verbose,
                        );
                        input_handler.set_mask(mask);
                    }
                    EventMsg::SetInputTarget(pid) => {
                        process_input_events(
                            &mut pending_inputs,
                            &mut processor,
                            &mut input_handler,
                            &target,
                            &tree_tx,
                            log_render,
                            &mut sessions,
                            &mut focused,
                            &mut clipboard,
                            &mut v2_no_prediction_stats,
                            v2_no_prediction_verbose,
                        );
                        target = pid;
                    }
                    EventMsg::Stop => {
                        maybe_dump_v2_no_prediction_stats(&v2_no_prediction_stats, "stop");
                        return;
                    }
                }
            }

            process_input_events(
                &mut pending_inputs,
                &mut processor,
                &mut input_handler,
                &target,
                &tree_tx,
                log_render,
                &mut sessions,
                &mut focused,
                &mut clipboard,
                &mut v2_no_prediction_stats,
                v2_no_prediction_verbose,
            );
        }

        maybe_dump_v2_no_prediction_stats(&v2_no_prediction_stats, "channel_closed");
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crossbeam_channel::{Receiver, bounded};

    use super::{
        DispatchOutcome, ElementEventKind, TextInputSession, V2OnlyNoPredictionStats,
        change_payload_for_target, enrich_predicted_edit_change_events, process_input_events,
        reconcile_text_input_sessions,
    };
    use crate::{
        actors::TreeMsg,
        clipboard::ClipboardManager,
        events::{EventNode, EventProcessor, KeyScrollTargets, Rect, TextInputDescriptor},
        input::{ACTION_PRESS, EVENT_MOUSE_LEAVE, EVENT_TEXT_INPUT, InputEvent, InputHandler},
        tree::{attrs::TextAlign, element::ElementId},
    };

    fn make_text_input_descriptor(
        content: &str,
        cursor: u32,
        selection_anchor: Option<u32>,
    ) -> TextInputDescriptor {
        TextInputDescriptor {
            content: content.to_string(),
            content_len: content.chars().count() as u32,
            cursor,
            selection_anchor,
            frame_x: 0.0,
            frame_width: 300.0,
            inset_left: 0.0,
            inset_right: 0.0,
            text_align: TextAlign::Left,
            font_family: "Inter".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        }
    }

    fn make_text_input_node(id: ElementId, descriptor: TextInputDescriptor) -> EventNode {
        EventNode {
            id,
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 40.0,
            },
            visible: true,
            flags: EVENT_TEXT_INPUT,
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 40.0,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: Some(descriptor),
        }
    }

    fn make_mouse_leave_only_node(id: ElementId) -> EventNode {
        EventNode {
            id,
            hit_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 60.0,
            },
            visible: true,
            flags: EVENT_MOUSE_LEAVE,
            self_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 60.0,
            },
            self_radii: None,
            clip_rect: None,
            clip_radii: None,
            scrollbar_x: None,
            scrollbar_y: None,
            key_scroll_targets: KeyScrollTargets::default(),
            focus_reveal_scrolls: Vec::new(),
            text_input: None,
        }
    }

    fn drain_msgs(rx: &Receiver<TreeMsg>) -> Vec<TreeMsg> {
        let mut out = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            out.push(msg);
        }
        out
    }

    fn push_change_event(predicted: &mut DispatchOutcome, id: &ElementId, payload: &str) {
        predicted
            .element_events
            .push(super::super::dispatch_outcome::ElementEventOut {
                target: super::super::dispatch_outcome::node_key(id),
                kind: ElementEventKind::Change,
                payload: Some(payload.to_string()),
            });
    }

    #[test]
    fn reconcile_keeps_focused_session_when_descriptor_is_stale() {
        let id = ElementId::from_term_bytes(vec![201]);
        let stale_descriptor = make_text_input_descriptor("abc", 3, None);
        let registry = vec![make_text_input_node(id.clone(), stale_descriptor.clone())];

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(stale_descriptor);
        session.content = "abcd".to_string();
        session.cursor = 4;
        session.selection_anchor = Some(2);
        session.preedit = Some("x".to_string());
        session.preedit_cursor = Some((0, 1));
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut focused = Some(id.clone());
        let (tx, rx) = bounded(16);

        reconcile_text_input_sessions(&registry, &mut sessions, &mut focused, &tx, false);

        let session = sessions
            .get(&id)
            .expect("focused session should still be present");
        assert_eq!(session.content, "abcd");
        assert_eq!(session.cursor, 4);
        assert_eq!(session.selection_anchor, Some(2));
        assert_eq!(session.preedit.as_deref(), Some("x"));
        assert_eq!(session.preedit_cursor, Some((0, 1)));
        assert_eq!(session.descriptor.content, "abcd");
        assert_eq!(session.descriptor.cursor, 4);
        assert_eq!(session.descriptor.selection_anchor, Some(2));

        let msgs = drain_msgs(&rx);
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, content }
                if *element_id == id && content == "abcd"
        )));
    }

    #[test]
    fn reconcile_applies_descriptor_content_when_unfocused() {
        let id = ElementId::from_term_bytes(vec![202]);
        let descriptor = make_text_input_descriptor("server", 6, None);
        let registry = vec![make_text_input_node(id.clone(), descriptor.clone())];

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.content = "local".to_string();
        session.cursor = 5;
        session.selection_anchor = Some(0);
        session.preedit = Some("x".to_string());
        session.preedit_cursor = Some((0, 1));
        session.focused = false;
        sessions.insert(id.clone(), session);

        let mut focused = None;
        let (tx, rx) = bounded(16);

        reconcile_text_input_sessions(&registry, &mut sessions, &mut focused, &tx, false);

        let session = sessions
            .get(&id)
            .expect("unfocused session should still be present");
        assert_eq!(session.content, "server");
        assert_eq!(session.selection_anchor, None);
        assert_eq!(session.preedit, None);
        assert_eq!(session.preedit_cursor, None);

        let msgs = drain_msgs(&rx);
        assert!(!msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputContent { element_id, .. } if *element_id == id
        )));
        assert!(msgs.iter().any(|msg| matches!(
            msg,
            TreeMsg::SetTextInputRuntime { element_id, .. } if *element_id == id
        )));
    }

    #[test]
    fn enrich_predicted_edit_change_events_uses_session_state_for_text_commit() {
        let id = ElementId::from_term_bytes(vec![203]);
        let descriptor = make_text_input_descriptor("abc", 3, None);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(id.clone(), descriptor.clone())]);
        processor.focused_id = Some(id.clone());

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.content = "abcd".to_string();
        session.cursor = 4;
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        push_change_event(&mut predicted, &id, "abce");

        let event = InputEvent::TextCommit {
            text: "e".to_string(),
            mods: 0,
        };

        enrich_predicted_edit_change_events(&mut predicted, &event, &processor, &sessions, true);

        assert_eq!(
            change_payload_for_target(&predicted, Some(&id)).as_deref(),
            Some("abcde")
        );

        let target = super::super::dispatch_outcome::node_key(&id);
        let count = predicted
            .element_events
            .iter()
            .filter(|event| event.kind == ElementEventKind::Change && event.target == target)
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn enrich_predicted_edit_change_events_removes_noop_backspace_change() {
        let id = ElementId::from_term_bytes(vec![204]);
        let descriptor = make_text_input_descriptor("abc", 0, None);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(id.clone(), descriptor.clone())]);
        processor.focused_id = Some(id.clone());

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.cursor = 0;
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        push_change_event(&mut predicted, &id, "stale");

        let event = InputEvent::Key {
            key: "backspace".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        };

        enrich_predicted_edit_change_events(&mut predicted, &event, &processor, &sessions, true);

        assert_eq!(change_payload_for_target(&predicted, Some(&id)), None);
    }

    #[test]
    fn enrich_predicted_edit_change_events_removes_change_when_not_allowed() {
        let id = ElementId::from_term_bytes(vec![205]);
        let descriptor = make_text_input_descriptor("abc", 3, None);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(id.clone(), descriptor.clone())]);
        processor.focused_id = Some(id.clone());

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        push_change_event(&mut predicted, &id, "stale");

        let event = InputEvent::TextCommit {
            text: "x".to_string(),
            mods: 0,
        };

        enrich_predicted_edit_change_events(&mut predicted, &event, &processor, &sessions, false);

        assert_eq!(change_payload_for_target(&predicted, Some(&id)), None);
    }

    #[test]
    fn v2_only_no_prediction_still_advances_hover_state_for_leave_only_target() {
        let id = ElementId::from_term_bytes(vec![206]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_mouse_leave_only_node(id)]);

        let mut input_handler = InputHandler::new();
        let target = None;
        let (tree_tx, _tree_rx) = bounded(32);
        let mut sessions = HashMap::new();
        let mut focused = None;
        let mut clipboard = ClipboardManager::new(false);
        let mut no_prediction_stats = V2OnlyNoPredictionStats::new();

        let mut first = vec![InputEvent::CursorPos { x: 10.0, y: 10.0 }];
        process_input_events(
            &mut first,
            &mut processor,
            &mut input_handler,
            &target,
            &tree_tx,
            false,
            &mut sessions,
            &mut focused,
            &mut clipboard,
            &mut no_prediction_stats,
            false,
        );
        assert_eq!(no_prediction_stats.total, 1);

        let mut second = vec![InputEvent::CursorEntered { entered: false }];
        process_input_events(
            &mut second,
            &mut processor,
            &mut input_handler,
            &target,
            &tree_tx,
            false,
            &mut sessions,
            &mut focused,
            &mut clipboard,
            &mut no_prediction_stats,
            false,
        );

        assert_eq!(
            no_prediction_stats.total, 1,
            "leave-only hover target should emit prediction on window-leave after state advancement"
        );
    }
}
