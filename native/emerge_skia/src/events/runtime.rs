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
    EventNode, EventProcessor, TextInputCommandRequest, TextInputDescriptor, TextInputEditRequest,
    TextInputPreeditRequest, blur_atom, change_atom, click_atom, focus_atom, mouse_down_atom,
    mouse_enter_atom, mouse_leave_atom, mouse_move_atom, mouse_up_atom, press_atom,
    registry::TriggerId, send_element_event, send_element_event_with_string_payload,
    send_input_event, text_ops,
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
struct NoPredictionStats {
    total: u64,
    unclassified: u64,
    by_trigger: Vec<u64>,
}

impl NoPredictionStats {
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

#[derive(Clone, Copy, Debug, Default)]
struct PredictionMeta {
    had_jobs: bool,
    trigger_for_stats: Option<TriggerId>,
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

fn send_element_event_if_target(
    target: Option<LocalPid>,
    element_id: &ElementId,
    event: rustler::Atom,
) {
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
        | InputEvent::Focused { .. }
        | InputEvent::Resized { .. } => true,
    }
}

fn maybe_dump_no_prediction_stats(stats: &NoPredictionStats, reason: &str) {
    eprintln!(
        "no-prediction reason={reason} total={} unclassified={}",
        stats.total, stats.unclassified
    );

    for (index, count) in stats.by_trigger.iter().copied().enumerate() {
        if count == 0 {
            continue;
        }

        eprintln!("no-prediction trigger_index={} count={}", index, count);
    }
}

fn text_char_len(content: &str) -> u32 {
    text_ops::text_char_len(content)
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
    text_ops::apply_insert(
        &session.content,
        session.cursor,
        session.selection_anchor,
        insert_text,
    )
    .map(|(next_content, next_cursor)| {
        apply_content_change(session, next_content.clone(), next_cursor);
        next_content
    })
}

fn delete_backward(session: &mut TextInputSession) -> Option<String> {
    text_ops::apply_backspace(&session.content, session.cursor, session.selection_anchor).map(
        |(next_content, next_cursor)| {
            apply_content_change(session, next_content.clone(), next_cursor);
            next_content
        },
    )
}

fn delete_forward(session: &mut TextInputSession) -> Option<String> {
    text_ops::apply_delete(&session.content, session.cursor, session.selection_anchor).map(
        |(next_content, next_cursor)| {
            apply_content_change(session, next_content.clone(), next_cursor);
            next_content
        },
    )
}

fn cut_selection(session: &mut TextInputSession) -> Option<(String, String)> {
    text_ops::cut_selection_content(&session.content, session.cursor, session.selection_anchor).map(
        |(next_content, next_cursor, selected)| {
            apply_content_change(session, next_content.clone(), next_cursor);
            (next_content, selected)
        },
    )
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

fn emit_change_event(target: &Option<LocalPid>, element_id: &ElementId, value: &str) {
    send_element_event_with_string_payload_if_target(
        target.as_ref().copied(),
        element_id,
        change_atom(),
        value,
    );
}

fn emit_content_change_outputs(
    target: &Option<LocalPid>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    element_id: &ElementId,
    session: &TextInputSession,
    emit_change: bool,
    next_content: String,
) {
    send_content_update(tree_tx, log_render, element_id, next_content.clone());
    send_runtime_update(tree_tx, log_render, element_id, session);
    if emit_change {
        emit_change_event(target, element_id, &next_content);
    }
}

#[cfg(test)]
fn change_payload_for_target(
    predicted: &DispatchOutcome,
    element_id: Option<&ElementId>,
) -> Option<String> {
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
    allow_change_events: bool,
    sessions: &HashMap<ElementId, TextInputSession>,
    clipboard: &mut ClipboardManager,
) {
    let command_requests = predicted.text_command_requests.clone();
    for request in command_requests {
        let element_id = element_id_from_node_key(&request.target);
        let Some(session) = sessions.get(&element_id) else {
            remove_predicted_change_event(predicted, &element_id);
            continue;
        };
        if !allow_change_events || !session.descriptor.emit_change {
            remove_predicted_change_event(predicted, &element_id);
            continue;
        }

        let mut simulated_session = session.clone();
        let next_content = match request.request {
            TextInputCommandRequest::Cut => {
                cut_selection(&mut simulated_session).map(|(next_content, _selected)| next_content)
            }
            TextInputCommandRequest::Paste => clipboard
                .get_text(ClipboardTarget::Clipboard)
                .and_then(|pasted| {
                    let sanitized = sanitize_single_line_text(&pasted);
                    replace_selection_or_insert(&mut simulated_session, &sanitized)
                }),
            TextInputCommandRequest::PastePrimary => clipboard
                .get_text(ClipboardTarget::Primary)
                .and_then(|pasted| {
                    let sanitized = sanitize_single_line_text(&pasted);
                    replace_selection_or_insert(&mut simulated_session, &sanitized)
                }),
            TextInputCommandRequest::SelectAll | TextInputCommandRequest::Copy => None,
        };

        if let Some(next_content) = next_content {
            upsert_predicted_change_event(predicted, &element_id, next_content);
        } else {
            remove_predicted_change_event(predicted, &element_id);
        }
    }
}

fn enrich_predicted_edit_change_events(
    predicted: &mut super::dispatch_outcome::DispatchOutcome,
    sessions: &HashMap<ElementId, TextInputSession>,
    allow_change_events: bool,
) {
    let edit_requests = predicted.text_edit_requests.clone();
    for request in edit_requests {
        let element_id = element_id_from_node_key(&request.target);

        if !allow_change_events {
            remove_predicted_change_event(predicted, &element_id);
            continue;
        }

        let Some(session) = sessions.get(&element_id) else {
            remove_predicted_change_event(predicted, &element_id);
            continue;
        };
        if !session.descriptor.emit_change {
            remove_predicted_change_event(predicted, &element_id);
            continue;
        }

        let mut simulated_session = session.clone();
        let next_content = match request.request {
            TextInputEditRequest::MoveLeft { .. }
            | TextInputEditRequest::MoveRight { .. }
            | TextInputEditRequest::MoveHome { .. }
            | TextInputEditRequest::MoveEnd { .. } => None,
            TextInputEditRequest::Backspace => delete_backward(&mut simulated_session),
            TextInputEditRequest::Delete => delete_forward(&mut simulated_session),
            TextInputEditRequest::Insert(text) => {
                replace_selection_or_insert(&mut simulated_session, &text)
            }
        };

        if let Some(next_content) = next_content {
            upsert_predicted_change_event(predicted, &element_id, next_content);
        } else {
            remove_predicted_change_event(predicted, &element_id);
        }
    }
}

fn element_id_from_node_key(key: &super::dispatch_outcome::NodeKey) -> ElementId {
    ElementId(key.0.clone())
}

fn float_from_milli(value: super::dispatch_outcome::Milli) -> f32 {
    value.0 as f32 / 1000.0
}

fn apply_dispatch_outcome(
    outcome: DispatchOutcome,
    processor: &mut EventProcessor,
    target: &Option<LocalPid>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    sessions: &mut HashMap<ElementId, TextInputSession>,
    focused: &mut Option<ElementId>,
    clipboard: &mut ClipboardManager,
) {
    for request in &outcome.window_resize_requests {
        send_tree(
            tree_tx,
            TreeMsg::Resize {
                width: request.width as f32,
                height: request.height as f32,
                scale: float_from_milli(request.scale),
            },
            log_render,
        );
    }

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

    for request in outcome.text_cursor_requests {
        let element_id = element_id_from_node_key(&request.target);
        if let Some(session) = sessions.get_mut(&element_id) {
            let next_cursor = cursor_from_click_x(session, float_from_milli(request.x));
            if move_cursor(session, next_cursor, request.extend_selection) {
                send_runtime_update(tree_tx, log_render, &element_id, session);
                if request.extend_selection {
                    sync_primary_selection(session, clipboard);
                }
            }
        }
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
                    let emit_change = session.descriptor.emit_change;
                    if let Some((next_content, selected)) = cut_selection(session) {
                        clipboard.set_text(ClipboardTarget::Clipboard, &selected);
                        clipboard.set_text(ClipboardTarget::Primary, &selected);
                        emit_content_change_outputs(
                            target,
                            tree_tx,
                            log_render,
                            &element_id,
                            session,
                            emit_change,
                            next_content,
                        );
                    }
                }
                TextInputCommandRequest::Paste => {
                    let emit_change = session.descriptor.emit_change;
                    if let Some(pasted) = clipboard.get_text(ClipboardTarget::Clipboard) {
                        let pasted = sanitize_single_line_text(&pasted);
                        if let Some(next_content) = replace_selection_or_insert(session, &pasted) {
                            emit_content_change_outputs(
                                target,
                                tree_tx,
                                log_render,
                                &element_id,
                                session,
                                emit_change,
                                next_content,
                            );
                        }
                    }
                }
                TextInputCommandRequest::PastePrimary => {
                    let emit_change = session.descriptor.emit_change;
                    if let Some(pasted) = clipboard.get_text(ClipboardTarget::Primary) {
                        let pasted = sanitize_single_line_text(&pasted);
                        if let Some(next_content) = replace_selection_or_insert(session, &pasted) {
                            emit_content_change_outputs(
                                target,
                                tree_tx,
                                log_render,
                                &element_id,
                                session,
                                emit_change,
                                next_content,
                            );
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
                    let emit_change = session.descriptor.emit_change;
                    if let Some(next_content) = delete_backward(session) {
                        emit_content_change_outputs(
                            target,
                            tree_tx,
                            log_render,
                            &element_id,
                            session,
                            emit_change,
                            next_content,
                        );
                    }
                }
                TextInputEditRequest::Delete => {
                    let emit_change = session.descriptor.emit_change;
                    if let Some(next_content) = delete_forward(session) {
                        emit_content_change_outputs(
                            target,
                            tree_tx,
                            log_render,
                            &element_id,
                            session,
                            emit_change,
                            next_content,
                        );
                    }
                }
                TextInputEditRequest::Insert(text) => {
                    let emit_change = session.descriptor.emit_change;
                    if let Some(next_content) = replace_selection_or_insert(session, &text) {
                        emit_content_change_outputs(
                            target,
                            tree_tx,
                            log_render,
                            &element_id,
                            session,
                            emit_change,
                            next_content,
                        );
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

fn advance_processor_state_after_event(processor: &mut EventProcessor, event: &InputEvent) {
    processor.advance_runtime_state_after_event(event);
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

fn coalesce_input_events(events: &mut Vec<InputEvent>) -> Vec<InputEvent> {
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

    coalesced
}

fn preview_and_enrich_dispatch_outcome(
    event: &InputEvent,
    processor: &EventProcessor,
    focused: Option<&ElementId>,
    target: &Option<LocalPid>,
    sessions: &HashMap<ElementId, TextInputSession>,
    clipboard: &mut ClipboardManager,
) -> (Option<DispatchOutcome>, PredictionMeta) {
    let super::DispatchPreview {
        outcome: mut predicted_outcome,
        had_jobs,
        trigger_for_stats,
    } = processor.preview_dispatch_outcome(event, focused);

    if let Some(mut predicted) = predicted_outcome.take() {
        enrich_predicted_command_change_events(
            &mut predicted,
            target.is_some(),
            sessions,
            clipboard,
        );

        enrich_predicted_edit_change_events(&mut predicted, sessions, target.is_some());

        predicted_outcome = Some(predicted);
    }

    (
        predicted_outcome,
        PredictionMeta {
            had_jobs,
            trigger_for_stats,
        },
    )
}

fn forward_observer_input(
    event: &InputEvent,
    input_handler: &InputHandler,
    target: &Option<LocalPid>,
) {
    let forward_to_target = input_handler.accepts(event);
    if let Some(pid) = target.as_ref()
        && forward_to_target
    {
        send_input_event(*pid, event);
    }
}

fn finalize_dispatch_for_event(
    event: &InputEvent,
    predicted_outcome: Option<DispatchOutcome>,
    prediction_meta: PredictionMeta,
    processor: &mut EventProcessor,
    target: &Option<LocalPid>,
    tree_tx: &Sender<TreeMsg>,
    log_render: bool,
    sessions: &mut HashMap<ElementId, TextInputSession>,
    focused: &mut Option<ElementId>,
    clipboard: &mut ClipboardManager,
    no_prediction_stats: &mut NoPredictionStats,
    no_prediction_verbose: bool,
) {
    if let Some(outcome) = predicted_outcome {
        apply_dispatch_outcome(
            outcome, processor, target, tree_tx, log_render, sessions, focused, clipboard,
        );
        advance_processor_state_after_event(processor, event);
    } else if event_is_dispatch_candidate(event) {
        advance_processor_state_after_event(processor, event);
        if prediction_meta.had_jobs {
            no_prediction_stats.record(prediction_meta.trigger_for_stats);
        }
        if no_prediction_verbose && prediction_meta.had_jobs {
            eprintln!(
                "no-prediction trigger={:?} event={:?}",
                prediction_meta.trigger_for_stats, event
            );
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
    no_prediction_stats: &mut NoPredictionStats,
    no_prediction_verbose: bool,
) {
    if events.is_empty() {
        return;
    }

    let coalesced = coalesce_input_events(events);

    for event in coalesced {
        let (predicted_outcome, prediction_meta) = preview_and_enrich_dispatch_outcome(
            &event,
            processor,
            focused.as_ref(),
            target,
            sessions,
            clipboard,
        );

        forward_observer_input(&event, input_handler, target);

        finalize_dispatch_for_event(
            &event,
            predicted_outcome,
            prediction_meta,
            processor,
            target,
            tree_tx,
            log_render,
            sessions,
            focused,
            clipboard,
            no_prediction_stats,
            no_prediction_verbose,
        );
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

        let mut no_prediction_stats = NoPredictionStats::new();
        let no_prediction_verbose = env_flag_enabled("EMERGE_SKIA_NO_PRED_VERBOSE");

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
                            &mut no_prediction_stats,
                            no_prediction_verbose,
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
                            &mut no_prediction_stats,
                            no_prediction_verbose,
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
                            &mut no_prediction_stats,
                            no_prediction_verbose,
                        );
                        target = pid;
                    }
                    EventMsg::Stop => {
                        maybe_dump_no_prediction_stats(&no_prediction_stats, "stop");
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
                &mut no_prediction_stats,
                no_prediction_verbose,
            );
        }

        maybe_dump_no_prediction_stats(&no_prediction_stats, "channel_closed");
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crossbeam_channel::{Receiver, bounded};

    use super::{
        DispatchOutcome, ElementEventKind, NoPredictionStats, TextInputSession,
        change_payload_for_target, coalesce_input_events, enrich_predicted_command_change_events,
        enrich_predicted_edit_change_events, process_input_events, reconcile_text_input_sessions,
    };
    use crate::{
        actors::TreeMsg,
        clipboard::ClipboardManager,
        events::{
            EventNode, EventProcessor, KeyScrollTargets, Rect, TextInputCommandRequest,
            TextInputDescriptor, TextInputEditRequest,
        },
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
            emit_change: true,
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

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.content = "abcd".to_string();
        session.cursor = 4;
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        predicted
            .text_edit_requests
            .push(super::super::dispatch_outcome::TextEditReqOut {
                target: super::super::dispatch_outcome::node_key(&id),
                request: TextInputEditRequest::Insert("e".to_string()),
            });
        push_change_event(&mut predicted, &id, "abce");

        enrich_predicted_edit_change_events(&mut predicted, &sessions, true);

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
    fn enrich_predicted_command_change_events_uses_outcome_requests() {
        let id = ElementId::from_term_bytes(vec![209]);
        let descriptor = make_text_input_descriptor("abc", 3, None);

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        predicted
            .text_command_requests
            .push(super::super::dispatch_outcome::TextCommandReqOut {
                target: super::super::dispatch_outcome::node_key(&id),
                request: TextInputCommandRequest::Paste,
            });
        push_change_event(&mut predicted, &id, "stale");

        let mut clipboard = ClipboardManager::new(false);
        clipboard.set_text(crate::clipboard::ClipboardTarget::Clipboard, "z");

        enrich_predicted_command_change_events(&mut predicted, true, &sessions, &mut clipboard);

        assert_eq!(
            change_payload_for_target(&predicted, Some(&id)).as_deref(),
            Some("abcz")
        );
    }

    #[test]
    fn enrich_predicted_edit_change_events_removes_noop_backspace_change() {
        let id = ElementId::from_term_bytes(vec![204]);
        let descriptor = make_text_input_descriptor("abc", 0, None);

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.cursor = 0;
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        predicted
            .text_edit_requests
            .push(super::super::dispatch_outcome::TextEditReqOut {
                target: super::super::dispatch_outcome::node_key(&id),
                request: TextInputEditRequest::Backspace,
            });
        push_change_event(&mut predicted, &id, "stale");

        enrich_predicted_edit_change_events(&mut predicted, &sessions, true);

        assert_eq!(change_payload_for_target(&predicted, Some(&id)), None);
    }

    #[test]
    fn enrich_predicted_edit_change_events_removes_change_when_not_allowed() {
        let id = ElementId::from_term_bytes(vec![205]);
        let descriptor = make_text_input_descriptor("abc", 3, None);

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        predicted
            .text_edit_requests
            .push(super::super::dispatch_outcome::TextEditReqOut {
                target: super::super::dispatch_outcome::node_key(&id),
                request: TextInputEditRequest::Insert("x".to_string()),
            });
        push_change_event(&mut predicted, &id, "stale");

        enrich_predicted_edit_change_events(&mut predicted, &sessions, false);

        assert_eq!(change_payload_for_target(&predicted, Some(&id)), None);
    }

    #[test]
    fn enrich_predicted_edit_change_events_removes_change_when_on_change_disabled() {
        let id = ElementId::from_term_bytes(vec![210]);
        let mut descriptor = make_text_input_descriptor("abc", 3, None);
        descriptor.emit_change = false;

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        predicted
            .text_edit_requests
            .push(super::super::dispatch_outcome::TextEditReqOut {
                target: super::super::dispatch_outcome::node_key(&id),
                request: TextInputEditRequest::Insert("x".to_string()),
            });
        push_change_event(&mut predicted, &id, "stale");

        enrich_predicted_edit_change_events(&mut predicted, &sessions, true);

        assert_eq!(change_payload_for_target(&predicted, Some(&id)), None);
    }

    #[test]
    fn enrich_predicted_command_change_events_removes_change_when_on_change_disabled() {
        let id = ElementId::from_term_bytes(vec![211]);
        let mut descriptor = make_text_input_descriptor("abc", 3, None);
        descriptor.emit_change = false;

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut predicted = DispatchOutcome::default();
        predicted
            .text_command_requests
            .push(super::super::dispatch_outcome::TextCommandReqOut {
                target: super::super::dispatch_outcome::node_key(&id),
                request: TextInputCommandRequest::Paste,
            });
        push_change_event(&mut predicted, &id, "stale");

        let mut clipboard = ClipboardManager::new(false);
        clipboard.set_text(crate::clipboard::ClipboardTarget::Clipboard, "z");

        enrich_predicted_command_change_events(&mut predicted, true, &sessions, &mut clipboard);

        assert_eq!(change_payload_for_target(&predicted, Some(&id)), None);
    }

    #[test]
    fn coalesce_input_events_merges_scroll_and_keeps_last_cursor() {
        let mut events = vec![
            InputEvent::Key {
                key: "a".to_string(),
                action: ACTION_PRESS,
                mods: 0,
            },
            InputEvent::CursorPos { x: 1.0, y: 2.0 },
            InputEvent::CursorScroll {
                dx: 1.0,
                dy: 2.0,
                x: 3.0,
                y: 4.0,
            },
            InputEvent::Focused { focused: false },
            InputEvent::CursorPos { x: 5.0, y: 6.0 },
            InputEvent::CursorScroll {
                dx: 0.5,
                dy: -1.5,
                x: 7.0,
                y: 8.0,
            },
        ];

        let coalesced = coalesce_input_events(&mut events);

        assert!(events.is_empty());
        assert_eq!(coalesced.len(), 4);
        assert!(matches!(
            &coalesced[0],
            InputEvent::Key { key, action, mods } if key == "a" && *action == ACTION_PRESS && *mods == 0
        ));
        assert!(matches!(
            &coalesced[1],
            InputEvent::Focused { focused: false }
        ));

        match &coalesced[2] {
            InputEvent::CursorScroll { dx, dy, x, y } => {
                assert_eq!(*dx, 1.5);
                assert_eq!(*dy, 0.5);
                assert_eq!(*x, 7.0);
                assert_eq!(*y, 8.0);
            }
            _ => panic!("expected merged cursor scroll event"),
        }

        match &coalesced[3] {
            InputEvent::CursorPos { x, y } => {
                assert_eq!(*x, 5.0);
                assert_eq!(*y, 6.0);
            }
            _ => panic!("expected final cursor position event"),
        }
    }

    #[test]
    fn process_input_events_resize_is_dispatch_driven_and_no_prediction_neutral() {
        let mut processor = EventProcessor::new();
        let mut input_handler = InputHandler::new();
        let target = None;
        let (tree_tx, tree_rx) = bounded(32);
        let mut sessions = HashMap::new();
        let mut focused = None;
        let mut clipboard = ClipboardManager::new(false);
        let mut no_prediction_stats = NoPredictionStats::new();

        let mut events = vec![InputEvent::Resized {
            width: 800,
            height: 600,
            scale_factor: 2.0,
        }];

        process_input_events(
            &mut events,
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

        assert_eq!(no_prediction_stats.total, 0);

        let messages = drain_msgs(&tree_rx);
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            &messages[0],
            TreeMsg::Resize {
                width,
                height,
                scale,
            } if *width == 800.0 && *height == 600.0 && *scale == 2.0
        ));
    }

    #[test]
    fn process_input_events_applies_dispatch_text_cursor_requests() {
        let id = ElementId::from_term_bytes(vec![207]);
        let descriptor = make_text_input_descriptor("abcd", 0, None);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(id.clone(), descriptor.clone())]);

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.cursor = 0;
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut input_handler = InputHandler::new();
        let target = None;
        let (tree_tx, _tree_rx) = bounded(32);
        let mut focused = Some(id.clone());
        let mut clipboard = ClipboardManager::new(false);
        let mut no_prediction_stats = NoPredictionStats::new();

        let mut events = vec![InputEvent::CursorButton {
            button: "left".to_string(),
            action: ACTION_PRESS,
            mods: 0,
            x: 0.0,
            y: 10.0,
        }];

        process_input_events(
            &mut events,
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

        let session = sessions
            .get(&id)
            .expect("text session should exist after cursor request");
        assert_eq!(session.cursor, 0);
        assert_eq!(no_prediction_stats.total, 0);
    }

    #[test]
    fn matched_dispatch_job_with_empty_outcome_does_not_increment_no_prediction() {
        let id = ElementId::from_term_bytes(vec![208]);
        let descriptor = make_text_input_descriptor("abc", 3, None);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_text_input_node(id.clone(), descriptor.clone())]);
        processor.focused_id = Some(id.clone());

        let mut sessions = HashMap::new();
        let mut session = TextInputSession::from_descriptor(descriptor);
        session.focused = true;
        sessions.insert(id.clone(), session);

        let mut input_handler = InputHandler::new();
        let target = None;
        let (tree_tx, _tree_rx) = bounded(32);
        let mut focused = Some(id);
        let mut clipboard = ClipboardManager::new(false);
        let mut no_prediction_stats = NoPredictionStats::new();

        let mut events = vec![InputEvent::Key {
            key: "a".to_string(),
            action: ACTION_PRESS,
            mods: 0,
        }];

        process_input_events(
            &mut events,
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

        assert_eq!(no_prediction_stats.total, 0);
        assert_eq!(no_prediction_stats.unclassified, 0);
    }

    #[test]
    fn no_prediction_still_advances_hover_state_for_leave_only_target() {
        let id = ElementId::from_term_bytes(vec![206]);

        let mut processor = EventProcessor::new();
        processor.rebuild_registry(vec![make_mouse_leave_only_node(id)]);

        let mut input_handler = InputHandler::new();
        let target = None;
        let (tree_tx, _tree_rx) = bounded(32);
        let mut sessions = HashMap::new();
        let mut focused = None;
        let mut clipboard = ClipboardManager::new(false);
        let mut no_prediction_stats = NoPredictionStats::new();

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
