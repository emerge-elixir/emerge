use std::{
    collections::{HashMap, HashSet},
    thread,
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

use super::{
    EventNode, EventProcessor, MouseDownRequest, MouseOverRequest, ScrollbarHoverRequest,
    ScrollbarThumbDragRequest, TextInputCommandRequest, TextInputCursorRequest,
    TextInputDescriptor, TextInputEditRequest, TextInputPreeditRequest, blur_atom, change_atom,
    focus_atom, press_atom, send_element_event, send_element_event_with_string_payload,
    send_input_event,
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

fn emit_change_event(target: &Option<LocalPid>, element_id: &ElementId, value: &str) {
    if let Some(pid) = target.as_ref() {
        send_element_event_with_string_payload(*pid, element_id, change_atom(), value);
    }
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

        let session = sessions
            .entry(id.clone())
            .or_insert_with(|| TextInputSession::from_descriptor(descriptor.clone()));

        let mut changed = false;
        if session.descriptor != descriptor {
            session.descriptor = descriptor.clone();
        }

        if session.content != descriptor.content {
            session.content = descriptor.content;
            session.selection_anchor = None;
            session.preedit = None;
            session.preedit_cursor = None;
            changed = true;
        }

        let should_focus = focused.as_ref().is_some_and(|focused_id| focused_id == &id);
        if session.focused != should_focus {
            session.focused = should_focus;
            changed = true;
        }

        if normalize_session_runtime(session) {
            changed = true;
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
        if let Some(pid) = target.as_ref() {
            send_element_event(*pid, &prev_id, blur_atom());
        }

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
        if let Some(pid) = target.as_ref() {
            send_element_event(*pid, &next_id, focus_atom());
        }

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

        if let Some(pid) = target.as_ref() {
            let pid = *pid;
            if forward_to_target {
                send_input_event(pid, &event);
            }

            if let Some(clicked_id) = processor.detect_click(&event) {
                send_element_event(pid, &clicked_id, super::click_atom());
            }

            if let Some(pressed_id) = processor.detect_press(&event) {
                send_element_event(pid, &pressed_id, press_atom());
            }

            if let Some((mouse_id, mouse_event)) = processor.detect_mouse_button_event(&event) {
                send_element_event(pid, &mouse_id, mouse_event);
            }

            for (hover_id, hover_event) in processor.handle_hover_event(&event) {
                send_element_event(pid, &hover_id, hover_event);
            }
        } else {
            processor.detect_click(&event);
            processor.detect_press(&event);
            processor.detect_mouse_button_event(&event);
            processor.handle_hover_event(&event);
        }

        if let Some(next_focus) = processor.text_input_focus_request(&event) {
            apply_focus_change(next_focus, focused, target, sessions, tree_tx, log_render);
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

        if let Some((element_id, request)) = processor.text_input_command_request(&event)
            && let Some(session) = sessions.get_mut(&element_id)
        {
            match request {
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

        if let Some((element_id, request)) = processor.text_input_edit_request(&event)
            && let Some(session) = sessions.get_mut(&element_id)
        {
            match request {
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

        if let Some((element_id, request)) = processor.text_input_preedit_request(&event)
            && let Some(session) = sessions.get_mut(&element_id)
        {
            match request {
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

        for request in processor.scrollbar_thumb_drag_requests(&event) {
            match request {
                ScrollbarThumbDragRequest::X { element_id, dx } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::ScrollbarThumbDragX { element_id, dx },
                        log_render,
                    );
                }
                ScrollbarThumbDragRequest::Y { element_id, dy } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::ScrollbarThumbDragY { element_id, dy },
                        log_render,
                    );
                }
            }
        }

        for (id, dx, dy) in processor.scroll_requests(&event) {
            send_tree(
                tree_tx,
                TreeMsg::ScrollRequest {
                    element_id: id,
                    dx,
                    dy,
                },
                log_render,
            );
        }

        for request in processor.scrollbar_hover_requests(&event) {
            match request {
                ScrollbarHoverRequest::X {
                    element_id,
                    hovered,
                } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetScrollbarXHover {
                            element_id,
                            hovered,
                        },
                        log_render,
                    );
                }
                ScrollbarHoverRequest::Y {
                    element_id,
                    hovered,
                } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetScrollbarYHover {
                            element_id,
                            hovered,
                        },
                        log_render,
                    );
                }
            }
        }

        for request in processor.mouse_over_requests(&event) {
            match request {
                MouseOverRequest::SetMouseOverActive { element_id, active } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetMouseOverActive { element_id, active },
                        log_render,
                    );
                }
            }
        }

        for request in processor.mouse_down_requests(&event) {
            match request {
                MouseDownRequest::SetMouseDownActive { element_id, active } => {
                    send_tree(
                        tree_tx,
                        TreeMsg::SetMouseDownActive { element_id, active },
                        log_render,
                    );
                }
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
                        );
                        target = pid;
                    }
                    EventMsg::Stop => return,
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
            );
        }
    })
}
