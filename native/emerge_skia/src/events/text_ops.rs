use super::TextInputEditRequest;

pub(super) fn text_char_len(content: &str) -> u32 {
    content.chars().count() as u32
}

fn char_to_byte_index(content: &str, char_index: u32) -> usize {
    content
        .char_indices()
        .nth(char_index as usize)
        .map(|(idx, _)| idx)
        .unwrap_or(content.len())
}

fn byte_to_char_index(content: &str, byte_index: usize) -> u32 {
    content[..byte_index.min(content.len())].chars().count() as u32
}

fn clamp_byte_start(content: &str, mut byte_index: usize) -> usize {
    byte_index = byte_index.min(content.len());
    while byte_index > 0 && !content.is_char_boundary(byte_index) {
        byte_index -= 1;
    }
    byte_index
}

fn clamp_byte_end(content: &str, mut byte_index: usize) -> usize {
    byte_index = byte_index.min(content.len());
    while byte_index < content.len() && !content.is_char_boundary(byte_index) {
        byte_index += 1;
    }
    byte_index.min(content.len())
}

pub(super) fn selected_range(
    cursor: u32,
    selection_anchor: Option<u32>,
    content_len: u32,
) -> Option<(u32, u32)> {
    let cursor = cursor.min(content_len);
    let anchor = selection_anchor?.min(content_len);
    if anchor == cursor {
        return None;
    }

    Some((anchor.min(cursor), anchor.max(cursor)))
}

fn char_at(content: &str, char_index: usize) -> Option<char> {
    content.chars().nth(char_index)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_whitespace_char(ch: char) -> bool {
    ch.is_whitespace()
}

pub(super) fn move_word_left_target(content: &str, cursor: u32) -> u32 {
    let mut index = cursor.min(text_char_len(content)) as usize;

    while index > 0 && char_at(content, index - 1).is_some_and(is_whitespace_char) {
        index -= 1;
    }

    if index == 0 {
        return 0;
    }

    let previous = char_at(content, index - 1);
    if previous.is_some_and(is_word_char) {
        while index > 0 && char_at(content, index - 1).is_some_and(is_word_char) {
            index -= 1;
        }
    } else {
        while index > 0
            && char_at(content, index - 1)
                .is_some_and(|ch| !is_whitespace_char(ch) && !is_word_char(ch))
        {
            index -= 1;
        }
    }

    index as u32
}

pub(super) fn move_word_right_target(content: &str, cursor: u32) -> u32 {
    let len = text_char_len(content) as usize;
    let mut index = cursor.min(len as u32) as usize;

    while index < len && char_at(content, index).is_some_and(is_whitespace_char) {
        index += 1;
    }

    if index >= len {
        return len as u32;
    }

    let current = char_at(content, index);
    if current.is_some_and(is_word_char) {
        while index < len && char_at(content, index).is_some_and(is_word_char) {
            index += 1;
        }
    } else {
        while index < len
            && char_at(content, index)
                .is_some_and(|ch| !is_whitespace_char(ch) && !is_word_char(ch))
        {
            index += 1;
        }
    }

    index as u32
}

pub(super) fn move_paragraph_start_target(content: &str, cursor: u32) -> u32 {
    let cursor = cursor.min(text_char_len(content)) as usize;
    let chars: Vec<char> = content.chars().collect();
    let mut index = cursor;

    while index > 0 {
        if chars[index - 1] == '\n' {
            break;
        }
        index -= 1;
    }

    index as u32
}

pub(super) fn move_paragraph_end_target(content: &str, cursor: u32) -> u32 {
    let chars: Vec<char> = content.chars().collect();
    let mut index = cursor.min(chars.len() as u32) as usize;

    while index < chars.len() {
        if chars[index] == '\n' {
            break;
        }
        index += 1;
    }

    index as u32
}

fn apply_delete_range(content: &str, start: u32, end: u32) -> Option<(String, u32)> {
    if start == end {
        return None;
    }

    let mut next = content.to_string();
    let start_byte = char_to_byte_index(&next, start);
    let end_byte = char_to_byte_index(&next, end);
    next.replace_range(start_byte..end_byte, "");
    Some((next, start))
}

pub(super) fn apply_insert(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
    insert_text: &str,
) -> Option<(String, u32)> {
    if insert_text.is_empty() {
        return None;
    }

    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let (start, end) =
        selected_range(cursor, selection_anchor, content_len).unwrap_or((cursor, cursor));

    let mut next = content.to_string();
    let start_byte = char_to_byte_index(&next, start);
    let end_byte = char_to_byte_index(&next, end);
    next.replace_range(start_byte..end_byte, insert_text);

    if next == content {
        return None;
    }

    let next_cursor = start + text_char_len(insert_text);
    Some((next, next_cursor))
}

pub(super) fn apply_backspace(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
) -> Option<(String, u32)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let (start, end) =
        selected_range(cursor, selection_anchor, content_len).unwrap_or((cursor, cursor));

    if start != end {
        return apply_delete_range(content, start, end);
    }

    if cursor == 0 {
        return None;
    }

    let mut next = content.to_string();
    let start_byte = char_to_byte_index(&next, cursor - 1);
    let end_byte = char_to_byte_index(&next, cursor);
    next.replace_range(start_byte..end_byte, "");
    Some((next, cursor - 1))
}

pub(super) fn apply_delete(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
) -> Option<(String, u32)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let (start, end) =
        selected_range(cursor, selection_anchor, content_len).unwrap_or((cursor, cursor));

    if start != end {
        return apply_delete_range(content, start, end);
    }

    if cursor >= content_len {
        return None;
    }

    let mut next = content.to_string();
    let start_byte = char_to_byte_index(&next, cursor);
    let end_byte = char_to_byte_index(&next, cursor + 1);
    next.replace_range(start_byte..end_byte, "");
    Some((next, cursor))
}

pub(super) fn apply_delete_word_backward(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
) -> Option<(String, u32)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let (start, end) =
        selected_range(cursor, selection_anchor, content_len).unwrap_or((cursor, cursor));

    if start != end {
        return apply_delete_range(content, start, end);
    }

    let target = move_word_left_target(content, cursor);
    apply_delete_range(content, target, cursor)
}

pub(super) fn apply_delete_word_forward(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
) -> Option<(String, u32)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let (start, end) =
        selected_range(cursor, selection_anchor, content_len).unwrap_or((cursor, cursor));

    if start != end {
        return apply_delete_range(content, start, end);
    }

    let target = move_word_right_target(content, cursor);
    apply_delete_range(content, cursor, target)
}

pub(super) fn apply_delete_to_target(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
    target: u32,
) -> Option<(String, u32)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let target = target.min(content_len);
    let (start, end) =
        selected_range(cursor, selection_anchor, content_len).unwrap_or((cursor, cursor));

    if start != end {
        return apply_delete_range(content, start, end);
    }

    apply_delete_range(content, cursor.min(target), cursor.max(target))
}

pub(super) fn apply_delete_surrounding(
    content: &str,
    cursor: u32,
    before_length: u32,
    after_length: u32,
) -> Option<(String, u32)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let cursor_byte = char_to_byte_index(content, cursor);
    let start_byte = clamp_byte_start(content, cursor_byte.saturating_sub(before_length as usize));
    let end_byte = clamp_byte_end(content, cursor_byte.saturating_add(after_length as usize));

    if start_byte == end_byte {
        return None;
    }

    let mut next = content.to_string();
    next.replace_range(start_byte..end_byte, "");
    Some((next, byte_to_char_index(content, start_byte)))
}

pub(super) fn apply_edit_request(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
    request: &TextInputEditRequest,
) -> Option<(String, u32)> {
    match request {
        TextInputEditRequest::Insert(text) => apply_insert(content, cursor, selection_anchor, text),
        TextInputEditRequest::Backspace => apply_backspace(content, cursor, selection_anchor),
        TextInputEditRequest::Delete => apply_delete(content, cursor, selection_anchor),
        TextInputEditRequest::DeleteWordBackward => {
            apply_delete_word_backward(content, cursor, selection_anchor)
        }
        TextInputEditRequest::DeleteWordForward => {
            apply_delete_word_forward(content, cursor, selection_anchor)
        }
        TextInputEditRequest::DeleteToHome => {
            apply_delete_to_target(content, cursor, selection_anchor, 0)
        }
        TextInputEditRequest::DeleteToEnd => {
            apply_delete_to_target(content, cursor, selection_anchor, text_char_len(content))
        }
        TextInputEditRequest::DeleteToParagraphStart => apply_delete_to_target(
            content,
            cursor,
            selection_anchor,
            move_paragraph_start_target(content, cursor),
        ),
        TextInputEditRequest::DeleteToParagraphEnd => apply_delete_to_target(
            content,
            cursor,
            selection_anchor,
            move_paragraph_end_target(content, cursor),
        ),
        TextInputEditRequest::DeleteSurrounding {
            before_length,
            after_length,
        } => apply_delete_surrounding(content, cursor, *before_length, *after_length),
        TextInputEditRequest::MoveLeft { .. }
        | TextInputEditRequest::MoveRight { .. }
        | TextInputEditRequest::MoveWordLeft { .. }
        | TextInputEditRequest::MoveWordRight { .. }
        | TextInputEditRequest::MoveUp { .. }
        | TextInputEditRequest::MoveDown { .. }
        | TextInputEditRequest::MoveHome { .. }
        | TextInputEditRequest::MoveEnd { .. }
        | TextInputEditRequest::MoveParagraphStart { .. }
        | TextInputEditRequest::MoveParagraphEnd { .. }
        | TextInputEditRequest::MoveDocumentStart { .. }
        | TextInputEditRequest::MoveDocumentEnd { .. } => None,
    }
}

pub(super) fn cut_selection_content(
    content: &str,
    cursor: u32,
    selection_anchor: Option<u32>,
) -> Option<(String, u32, String)> {
    let content_len = text_char_len(content);
    let cursor = cursor.min(content_len);
    let (start, end) = selected_range(cursor, selection_anchor, content_len)?;

    let selected: String = content
        .chars()
        .skip(start as usize)
        .take((end - start) as usize)
        .collect();

    if selected.is_empty() {
        return None;
    }

    let mut next = content.to_string();
    let start_byte = char_to_byte_index(&next, start);
    let end_byte = char_to_byte_index(&next, end);
    next.replace_range(start_byte..end_byte, "");
    Some((next, start, selected))
}
