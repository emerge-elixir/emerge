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
        let mut next = content.to_string();
        let start_byte = char_to_byte_index(&next, start);
        let end_byte = char_to_byte_index(&next, end);
        next.replace_range(start_byte..end_byte, "");
        return Some((next, start));
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
        let mut next = content.to_string();
        let start_byte = char_to_byte_index(&next, start);
        let end_byte = char_to_byte_index(&next, end);
        next.replace_range(start_byte..end_byte, "");
        return Some((next, start));
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
        TextInputEditRequest::DeleteSurrounding {
            before_length,
            after_length,
        } => apply_delete_surrounding(content, cursor, *before_length, *after_length),
        TextInputEditRequest::MoveLeft { .. }
        | TextInputEditRequest::MoveRight { .. }
        | TextInputEditRequest::MoveUp { .. }
        | TextInputEditRequest::MoveDown { .. }
        | TextInputEditRequest::MoveHome { .. }
        | TextInputEditRequest::MoveEnd { .. } => None,
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
