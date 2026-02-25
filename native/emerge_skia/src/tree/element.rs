//! Element types for Emerge UI trees.

#[cfg(test)]
use super::attrs::MouseOverAttrs;
use super::attrs::{Attrs, BorderWidth, Font, Padding, ScrollbarHoverAxis, TextAlign};
use crate::renderer::make_font_with_style;
use std::collections::HashMap;

/// Unique identifier for an element, derived from Erlang term.
/// Stored as the raw bytes of the serialized Erlang term for exact matching.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ElementId(pub Vec<u8>);

impl ElementId {
    pub fn from_term_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// The type/kind of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementKind {
    Row,
    WrappedRow,
    Column,
    TextColumn,
    El,
    Text,
    TextInput,
    Image,
    None,
    Paragraph,
}

impl ElementKind {
    /// Decode from the type tag byte used in serialization.
    pub fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Row),
            2 => Some(Self::WrappedRow),
            3 => Some(Self::Column),
            4 => Some(Self::El),
            5 => Some(Self::Text),
            6 => Some(Self::None),
            7 => Some(Self::Paragraph),
            8 => Some(Self::TextColumn),
            9 => Some(Self::Image),
            10 => Some(Self::TextInput),
            _ => None,
        }
    }
}

/// Frame representing the computed layout bounds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// X position relative to parent.
    pub x: f32,
    /// Y position relative to parent.
    pub y: f32,
    /// Visible width (may be smaller than content for scrollable areas).
    pub width: f32,
    /// Visible height (may be smaller than content for scrollable areas).
    pub height: f32,
    /// Actual content width (for scroll extent calculation).
    pub content_width: f32,
    /// Actual content height (for scroll extent calculation).
    pub content_height: f32,
}

/// A single element in the UI tree.
#[derive(Clone, Debug)]
pub struct Element {
    /// Unique identifier for this element.
    pub id: ElementId,

    /// The type of element (row, column, el, text, etc).
    pub kind: ElementKind,

    /// Raw attributes as binary (EMRG format).
    pub attrs_raw: Vec<u8>,

    /// Original unscaled attributes (as received from Elixir).
    pub base_attrs: Attrs,

    /// Scaled attributes (populated by layout pass, used by render).
    pub attrs: Attrs,

    /// Child element IDs (order matters).
    pub children: Vec<ElementId>,

    /// Computed layout frame (populated after layout pass).
    pub frame: Option<Frame>,
}

impl Element {
    /// Create an element with decoded attributes.
    /// The attrs are stored as base_attrs (original) and cloned to attrs (for scaling).
    pub fn with_attrs(id: ElementId, kind: ElementKind, attrs_raw: Vec<u8>, attrs: Attrs) -> Self {
        Self {
            id,
            kind,
            attrs_raw,
            base_attrs: attrs.clone(),
            attrs,
            children: Vec::new(),
            frame: None,
        }
    }
}

/// The complete element tree with indexed access.
#[derive(Clone, Debug, Default)]
pub struct ElementTree {
    /// Root element ID (if tree is non-empty).
    pub root: Option<ElementId>,

    /// All elements indexed by ID for O(1) lookup.
    pub nodes: HashMap<ElementId, Element>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollAxis {
    X,
    Y,
}

impl ElementTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get an element by ID.
    pub fn get(&self, id: &ElementId) -> Option<&Element> {
        self.nodes.get(id)
    }

    /// Get a mutable element by ID.
    pub fn get_mut(&mut self, id: &ElementId) -> Option<&mut Element> {
        self.nodes.get_mut(id)
    }

    /// Insert or update an element.
    pub fn insert(&mut self, element: Element) {
        self.nodes.insert(element.id.clone(), element);
    }

    /// Check if tree is empty.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Get the number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Clear the tree.
    pub fn clear(&mut self) {
        self.root = None;
        self.nodes.clear();
    }

    /// Apply scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll(&mut self, id: &ElementId, dx: f32, dy: f32) -> bool {
        let mut changed = false;
        if dx != 0.0 {
            changed |= self.apply_scroll_x(id, dx);
        }
        if dy != 0.0 {
            changed |= self.apply_scroll_y(id, dy);
        }
        changed
    }

    /// Apply horizontal scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll_x(&mut self, id: &ElementId, dx: f32) -> bool {
        self.apply_scroll_axis(id, dx, ScrollAxis::X)
    }

    /// Apply vertical scroll delta to an element. Returns true if scroll changed.
    pub fn apply_scroll_y(&mut self, id: &ElementId, dy: f32) -> bool {
        self.apply_scroll_axis(id, dy, ScrollAxis::Y)
    }

    /// Set horizontal scrollbar thumb hover state. Returns true when state changes.
    pub fn set_scrollbar_x_hover(&mut self, id: &ElementId, hovered: bool) -> bool {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::X, hovered)
    }

    /// Set vertical scrollbar thumb hover state. Returns true when state changes.
    pub fn set_scrollbar_y_hover(&mut self, id: &ElementId, hovered: bool) -> bool {
        self.set_scrollbar_hover_axis(id, ScrollbarHoverAxis::Y, hovered)
    }

    /// Set mouse_over active state. Returns true when state changes.
    pub fn set_mouse_over_active(&mut self, id: &ElementId, active: bool) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.attrs.mouse_over.is_none() {
            if element.attrs.mouse_over_active.take().is_some() {
                return true;
            }
            return false;
        }

        let current = element.attrs.mouse_over_active.unwrap_or(false);
        if current == active {
            return false;
        }

        element.attrs.mouse_over_active = Some(active);
        true
    }

    /// Set focused text input. Returns true when any runtime focus/cursor state changes.
    pub fn set_text_input_focus(&mut self, id: Option<&ElementId>) -> bool {
        let mut changed = false;

        for element in self.nodes.values_mut() {
            if element.kind != ElementKind::TextInput {
                continue;
            }

            let should_focus = id.is_some_and(|focused| focused == &element.id);
            let current_focus = element.attrs.text_input_focused.unwrap_or(false);
            if current_focus != should_focus {
                element.attrs.text_input_focused = Some(should_focus);
                changed = true;
            }

            if !should_focus {
                let had_preedit = element.attrs.text_input_preedit.take().is_some();
                let had_preedit_cursor = element.attrs.text_input_preedit_cursor.take().is_some();
                if had_preedit || had_preedit_cursor {
                    changed = true;
                }

                if element.attrs.text_input_selection_anchor.take().is_some() {
                    changed = true;
                }
            }

            let content = element.base_attrs.content.as_deref().unwrap_or("");
            let content_len = text_char_len(content);

            match element.attrs.text_input_cursor {
                Some(cursor) => {
                    let clamped = cursor.min(content_len);
                    if clamped != cursor {
                        element.attrs.text_input_cursor = Some(clamped);
                        changed = true;
                    }
                }
                None if should_focus => {
                    element.attrs.text_input_cursor = Some(content_len);
                    changed = true;
                }
                None => {}
            }

            if let Some(anchor) = element.attrs.text_input_selection_anchor {
                let clamped = anchor.min(content_len);
                let cursor = element.attrs.text_input_cursor.unwrap_or(content_len);
                let next = if clamped == cursor {
                    None
                } else {
                    Some(clamped)
                };
                if next != element.attrs.text_input_selection_anchor {
                    element.attrs.text_input_selection_anchor = next;
                    changed = true;
                }
            }
        }

        changed
    }

    pub fn set_text_input_preedit(
        &mut self,
        id: &ElementId,
        text: String,
        cursor: Option<(u32, u32)>,
    ) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        if !element.attrs.text_input_focused.unwrap_or(false) {
            return false;
        }

        let next_text = if text.is_empty() { None } else { Some(text) };
        let next_cursor = normalize_preedit_cursor(next_text.as_deref(), cursor);
        let changed = element.attrs.text_input_preedit != next_text
            || element.attrs.text_input_preedit_cursor != next_cursor;

        if !changed {
            return false;
        }

        element.attrs.text_input_preedit = next_text;
        element.attrs.text_input_preedit_cursor = next_cursor;
        true
    }

    pub fn clear_text_input_preedit(&mut self, id: &ElementId) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        let had_preedit = element.attrs.text_input_preedit.take().is_some();
        let had_cursor = element.attrs.text_input_preedit_cursor.take().is_some();
        had_preedit || had_cursor
    }

    pub fn text_input_move_left(&mut self, id: &ElementId, extend_selection: bool) -> bool {
        self.move_text_input_cursor(id, extend_selection, |cursor, content, selection| {
            if !extend_selection {
                if let Some((start, _end)) = selection {
                    return start;
                }
                return cursor.saturating_sub(1);
            }

            cursor.saturating_sub(1).min(text_char_len(content))
        })
    }

    pub fn text_input_move_right(&mut self, id: &ElementId, extend_selection: bool) -> bool {
        self.move_text_input_cursor(id, extend_selection, |cursor, content, selection| {
            if !extend_selection {
                if let Some((_start, end)) = selection {
                    return end;
                }
                return (cursor + 1).min(text_char_len(content));
            }

            (cursor + 1).min(text_char_len(content))
        })
    }

    pub fn text_input_move_home(&mut self, id: &ElementId, extend_selection: bool) -> bool {
        self.move_text_input_cursor(id, extend_selection, |_cursor, _content, _selection| 0)
    }

    pub fn text_input_move_end(&mut self, id: &ElementId, extend_selection: bool) -> bool {
        self.move_text_input_cursor(id, extend_selection, |_cursor, content, _selection| {
            text_char_len(content)
        })
    }

    pub fn text_input_backspace(&mut self, id: &ElementId) -> Option<String> {
        let element = self.get_mut(id)?;
        if element.kind != ElementKind::TextInput {
            return None;
        }

        let content = element.base_attrs.content.clone().unwrap_or_default();
        let cursor = clamp_cursor(element.attrs.text_input_cursor, &content);

        if let Some((start, end)) = selected_range(&element.attrs, &content) {
            let mut next = content.clone();
            let start_byte = char_to_byte_index(&next, start);
            let end_byte = char_to_byte_index(&next, end);
            next.replace_range(start_byte..end_byte, "");
            apply_text_input_content_change(element, next.clone(), start);
            return Some(next);
        }

        if cursor == 0 {
            return None;
        }

        let start = char_to_byte_index(&content, cursor - 1);
        let end = char_to_byte_index(&content, cursor);
        let mut next = content;
        next.replace_range(start..end, "");
        apply_text_input_content_change(element, next.clone(), cursor - 1);
        Some(next)
    }

    pub fn text_input_delete(&mut self, id: &ElementId) -> Option<String> {
        let element = self.get_mut(id)?;
        if element.kind != ElementKind::TextInput {
            return None;
        }

        let content = element.base_attrs.content.clone().unwrap_or_default();
        let cursor = clamp_cursor(element.attrs.text_input_cursor, &content);

        if let Some((start, end)) = selected_range(&element.attrs, &content) {
            let mut next = content.clone();
            let start_byte = char_to_byte_index(&next, start);
            let end_byte = char_to_byte_index(&next, end);
            next.replace_range(start_byte..end_byte, "");
            apply_text_input_content_change(element, next.clone(), start);
            return Some(next);
        }

        let content_len = text_char_len(&content);
        if cursor >= content_len {
            return None;
        }

        let start = char_to_byte_index(&content, cursor);
        let end = char_to_byte_index(&content, cursor + 1);
        let mut next = content;
        next.replace_range(start..end, "");
        apply_text_input_content_change(element, next.clone(), cursor);
        Some(next)
    }

    pub fn text_input_insert(&mut self, id: &ElementId, text: &str) -> Option<String> {
        if text.is_empty() {
            return None;
        }

        let element = self.get_mut(id)?;
        if element.kind != ElementKind::TextInput {
            return None;
        }

        let content = element.base_attrs.content.clone().unwrap_or_default();
        let cursor = clamp_cursor(element.attrs.text_input_cursor, &content);
        let (replace_start, replace_end) =
            selected_range(&element.attrs, &content).unwrap_or((cursor, cursor));

        let mut next = content;
        let start_byte = char_to_byte_index(&next, replace_start);
        let end_byte = char_to_byte_index(&next, replace_end);
        next.replace_range(start_byte..end_byte, text);

        let next_cursor = replace_start.saturating_add(text_char_len(text));
        apply_text_input_content_change(element, next.clone(), next_cursor);
        Some(next)
    }

    pub fn set_text_input_cursor_from_point(
        &mut self,
        id: &ElementId,
        x: f32,
        extend_selection: bool,
    ) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        let Some(frame) = element.frame else {
            return false;
        };

        let content = element.base_attrs.content.as_deref().unwrap_or("");
        let chars: Vec<char> = content.chars().collect();
        let font_size = element.attrs.font_size.unwrap_or(16.0) as f32;
        let (font_family, font_weight, font_italic) = font_info_from_attrs(&element.attrs);
        let letter_spacing = element.attrs.font_letter_spacing.unwrap_or(0.0) as f32;
        let word_spacing = element.attrs.font_word_spacing.unwrap_or(0.0) as f32;

        let insets = content_insets(&element.attrs);
        let content_width = (frame.width - insets.left - insets.right).max(0.0);
        let text_width = measure_text_width(
            content,
            font_size,
            &font_family,
            font_weight,
            font_italic,
            letter_spacing,
            word_spacing,
        );

        let text_start_x = match element.attrs.text_align.unwrap_or_default() {
            TextAlign::Left => frame.x + insets.left,
            TextAlign::Center => frame.x + insets.left + (content_width - text_width) / 2.0,
            TextAlign::Right => frame.x + frame.width - insets.right - text_width,
        };

        let local_x = (x - text_start_x).clamp(0.0, text_width.max(0.0));
        let cursor = nearest_char_index_for_offset(
            &chars,
            local_x,
            font_size,
            &font_family,
            font_weight,
            font_italic,
            letter_spacing,
            word_spacing,
        ) as u32;

        let current = clamp_cursor(element.attrs.text_input_cursor, content);
        let had_preedit = element.attrs.text_input_preedit.take().is_some()
            || element.attrs.text_input_preedit_cursor.take().is_some();

        let mut changed = false;

        if extend_selection {
            let current_anchor = element.attrs.text_input_selection_anchor.unwrap_or(current);
            let next_anchor = if current_anchor == cursor {
                None
            } else {
                Some(current_anchor)
            };

            if element.attrs.text_input_selection_anchor != next_anchor {
                element.attrs.text_input_selection_anchor = next_anchor;
                changed = true;
            }
        } else if element.attrs.text_input_selection_anchor.take().is_some() {
            changed = true;
        }

        if current == cursor && !had_preedit && !changed {
            return false;
        }

        element.attrs.text_input_cursor = Some(cursor);
        true
    }

    pub fn text_input_select_all(&mut self, id: &ElementId) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        let content = element.base_attrs.content.as_deref().unwrap_or("");
        let len = text_char_len(content);

        if len == 0 {
            let had_selection = element.attrs.text_input_selection_anchor.take().is_some();
            return had_selection;
        }

        let changed_cursor = element.attrs.text_input_cursor != Some(len);
        let changed_anchor = element.attrs.text_input_selection_anchor != Some(0);

        if !changed_cursor && !changed_anchor {
            return false;
        }

        element.attrs.text_input_cursor = Some(len);
        element.attrs.text_input_selection_anchor = Some(0);
        element.attrs.text_input_preedit = None;
        element.attrs.text_input_preedit_cursor = None;
        true
    }

    pub fn text_input_copy_selection(&self, id: &ElementId) -> Option<String> {
        let element = self.get(id)?;
        if element.kind != ElementKind::TextInput {
            return None;
        }

        let content = element.base_attrs.content.as_deref().unwrap_or("");
        let (start, end) = selected_range(&element.attrs, content)?;
        Some(
            content
                .chars()
                .skip(start as usize)
                .take((end - start) as usize)
                .collect(),
        )
    }

    pub fn text_input_cut_selection(&mut self, id: &ElementId) -> Option<(String, String)> {
        let element = self.get_mut(id)?;
        if element.kind != ElementKind::TextInput {
            return None;
        }

        let content = element.base_attrs.content.clone().unwrap_or_default();
        let (start, end) = selected_range(&element.attrs, &content)?;
        let selected: String = content
            .chars()
            .skip(start as usize)
            .take((end - start) as usize)
            .collect();
        if selected.is_empty() {
            return None;
        }

        let mut next = content;
        let start_byte = char_to_byte_index(&next, start);
        let end_byte = char_to_byte_index(&next, end);
        next.replace_range(start_byte..end_byte, "");
        apply_text_input_content_change(element, next.clone(), start);
        Some((next, selected))
    }

    pub fn text_input_paste_text(&mut self, id: &ElementId, text: &str) -> Option<String> {
        let pasted = sanitize_single_line_text(text);
        if pasted.is_empty() {
            return None;
        }

        self.text_input_insert(id, &pasted)
    }

    fn move_text_input_cursor<F>(
        &mut self,
        id: &ElementId,
        extend_selection: bool,
        update: F,
    ) -> bool
    where
        F: FnOnce(u32, &str, Option<(u32, u32)>) -> u32,
    {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        let content = element.base_attrs.content.as_deref().unwrap_or("");
        let current = clamp_cursor(element.attrs.text_input_cursor, content);
        let selection = selected_range(&element.attrs, content);
        let next = update(current, content, selection).min(text_char_len(content));

        let mut changed = false;

        if extend_selection {
            let current_anchor = element.attrs.text_input_selection_anchor.unwrap_or(current);
            let next_anchor = if current_anchor == next {
                None
            } else {
                Some(current_anchor)
            };
            if element.attrs.text_input_selection_anchor != next_anchor {
                element.attrs.text_input_selection_anchor = next_anchor;
                changed = true;
            }
        } else if element.attrs.text_input_selection_anchor.take().is_some() {
            changed = true;
        }

        if current == next && !changed {
            return false;
        }

        element.attrs.text_input_cursor = Some(next);
        element.attrs.text_input_preedit = None;
        element.attrs.text_input_preedit_cursor = None;
        true
    }

    fn apply_scroll_axis(&mut self, id: &ElementId, delta: f32, axis: ScrollAxis) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };
        let Some(frame) = element.frame else {
            return false;
        };

        let (current, max) = match axis {
            ScrollAxis::X => (
                element.attrs.scroll_x.unwrap_or(0.0) as f32,
                (frame.content_width - frame.width).max(0.0),
            ),
            ScrollAxis::Y => (
                element.attrs.scroll_y.unwrap_or(0.0) as f32,
                (frame.content_height - frame.height).max(0.0),
            ),
        };
        let next = (current - delta).clamp(0.0, max);

        if (next - current).abs() < f32::EPSILON {
            return false;
        }

        match axis {
            ScrollAxis::X => element.attrs.scroll_x = Some(next as f64),
            ScrollAxis::Y => element.attrs.scroll_y = Some(next as f64),
        }
        true
    }

    fn set_scrollbar_hover_axis(
        &mut self,
        id: &ElementId,
        axis: ScrollbarHoverAxis,
        hovered: bool,
    ) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        let current = element.attrs.scrollbar_hover_axis;
        let axis_enabled = match axis {
            ScrollbarHoverAxis::X => element.attrs.scrollbar_x.unwrap_or(false),
            ScrollbarHoverAxis::Y => element.attrs.scrollbar_y.unwrap_or(false),
        };

        if hovered {
            if !axis_enabled || current == Some(axis) {
                return false;
            }
            element.attrs.scrollbar_hover_axis = Some(axis);
            return true;
        }

        if current == Some(axis) {
            element.attrs.scrollbar_hover_axis = None;
            return true;
        }

        false
    }
}

fn text_char_len(content: &str) -> u32 {
    content.chars().count() as u32
}

fn clamp_cursor(cursor: Option<u32>, content: &str) -> u32 {
    cursor
        .unwrap_or_else(|| text_char_len(content))
        .min(text_char_len(content))
}

fn char_to_byte_index(content: &str, char_index: u32) -> usize {
    let idx = char_index as usize;
    content
        .char_indices()
        .nth(idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(content.len())
}

fn normalize_preedit_cursor(text: Option<&str>, cursor: Option<(u32, u32)>) -> Option<(u32, u32)> {
    let text_len = text.map(text_char_len).unwrap_or(0);
    let (mut start, mut end) = cursor?;
    start = start.min(text_len);
    end = end.min(text_len);
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    Some((start, end))
}

fn selected_range(attrs: &Attrs, content: &str) -> Option<(u32, u32)> {
    let content_len = text_char_len(content);
    let cursor = clamp_cursor(attrs.text_input_cursor, content);
    let anchor = attrs.text_input_selection_anchor?.min(content_len);
    if anchor == cursor {
        return None;
    }
    Some((anchor.min(cursor), anchor.max(cursor)))
}

fn apply_text_input_content_change(element: &mut Element, next_content: String, next_cursor: u32) {
    let clamped_cursor = next_cursor.min(text_char_len(&next_content));
    element.base_attrs.content = Some(next_content.clone());
    element.attrs.content = Some(next_content);
    element.attrs.text_input_cursor = Some(clamped_cursor);
    element.attrs.text_input_selection_anchor = None;
    element.attrs.text_input_preedit = None;
    element.attrs.text_input_preedit_cursor = None;
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

#[derive(Clone, Copy, Debug, Default)]
struct ResolvedInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

fn resolved_padding(padding: Option<&Padding>) -> ResolvedInsets {
    match padding {
        Some(Padding::Uniform(value)) => {
            let value = *value as f32;
            ResolvedInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedInsets::default(),
    }
}

fn resolved_border_width(border_width: Option<&BorderWidth>) -> ResolvedInsets {
    match border_width {
        Some(BorderWidth::Uniform(value)) => {
            let value = *value as f32;
            ResolvedInsets {
                top: value,
                right: value,
                bottom: value,
                left: value,
            }
        }
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => ResolvedInsets {
            top: *top as f32,
            right: *right as f32,
            bottom: *bottom as f32,
            left: *left as f32,
        },
        None => ResolvedInsets::default(),
    }
}

fn content_insets(attrs: &Attrs) -> ResolvedInsets {
    let padding = resolved_padding(attrs.padding.as_ref());
    let border = resolved_border_width(attrs.border_width.as_ref());
    ResolvedInsets {
        top: padding.top + border.top,
        right: padding.right + border.right,
        bottom: padding.bottom + border.bottom,
        left: padding.left + border.left,
    }
}

fn font_info_from_attrs(attrs: &Attrs) -> (String, u16, bool) {
    let family = attrs
        .font
        .as_ref()
        .map(|font| match font {
            Font::Atom(name) | Font::String(name) => name.clone(),
        })
        .unwrap_or_else(|| "default".to_string());

    let weight = attrs
        .font_weight
        .as_ref()
        .map(|value| parse_font_weight(&value.0))
        .unwrap_or(400);

    let italic = attrs
        .font_style
        .as_ref()
        .map(|style| style.0 == "italic")
        .unwrap_or(false);

    (family, weight, italic)
}

fn parse_font_weight(value: &str) -> u16 {
    match value {
        "bold" => 700,
        "normal" => 400,
        "light" => 300,
        "thin" => 100,
        "medium" => 500,
        "semibold" | "semi_bold" => 600,
        "extrabold" | "extra_bold" => 800,
        "black" => 900,
        _ => value.parse().unwrap_or(400),
    }
}

fn measure_text_width(
    text: &str,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }

    let font = make_font_with_style(family, weight, italic, font_size);
    let mut total = 0.0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
        total += glyph_width;

        if chars.peek().is_some() {
            total += letter_spacing;
            if ch.is_whitespace() {
                total += word_spacing;
            }
        }
    }

    total
}

fn nearest_char_index_for_offset(
    chars: &[char],
    offset_x: f32,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
) -> usize {
    if chars.is_empty() {
        return 0;
    }

    let font = make_font_with_style(family, weight, italic, font_size);
    let mut caret_positions = Vec::with_capacity(chars.len() + 1);
    caret_positions.push(0.0);

    let mut advance = 0.0;
    for (idx, ch) in chars.iter().enumerate() {
        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
        advance += glyph_width;

        if idx + 1 < chars.len() {
            advance += letter_spacing;
            if ch.is_whitespace() {
                advance += word_spacing;
            }
        }

        caret_positions.push(advance);
    }

    for idx in 0..chars.len() {
        let left = caret_positions[idx];
        let right = caret_positions[idx + 1];
        let midpoint = left + (right - left) / 2.0;
        if offset_x <= midpoint {
            return idx;
        }
    }

    chars.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_kind_from_tag() {
        assert_eq!(ElementKind::from_tag(1), Some(ElementKind::Row));
        assert_eq!(ElementKind::from_tag(2), Some(ElementKind::WrappedRow));
        assert_eq!(ElementKind::from_tag(3), Some(ElementKind::Column));
        assert_eq!(ElementKind::from_tag(4), Some(ElementKind::El));
        assert_eq!(ElementKind::from_tag(5), Some(ElementKind::Text));
        assert_eq!(ElementKind::from_tag(6), Some(ElementKind::None));
        assert_eq!(ElementKind::from_tag(7), Some(ElementKind::Paragraph));
        assert_eq!(ElementKind::from_tag(8), Some(ElementKind::TextColumn));
        assert_eq!(ElementKind::from_tag(9), Some(ElementKind::Image));
        assert_eq!(ElementKind::from_tag(10), Some(ElementKind::TextInput));
    }

    #[test]
    fn test_scrollbar_hover_axis_is_tri_state() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_scrollbar_x_hover(&id, true));
        assert_eq!(
            tree.get(&id).unwrap().attrs.scrollbar_hover_axis,
            Some(ScrollbarHoverAxis::X)
        );

        assert!(tree.set_scrollbar_y_hover(&id, true));
        assert_eq!(
            tree.get(&id).unwrap().attrs.scrollbar_hover_axis,
            Some(ScrollbarHoverAxis::Y)
        );

        assert!(!tree.set_scrollbar_x_hover(&id, false));
        assert!(tree.set_scrollbar_y_hover(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_apply_scroll_axis_helpers() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 200.0,
            content_height: 200.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.apply_scroll_x(&id, -30.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_x, Some(30.0));

        assert!(tree.apply_scroll_y(&id, -25.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_y, Some(25.0));

        assert!(!tree.apply_scroll_x(&id, 0.0));
        assert!(!tree.apply_scroll_y(&id, 0.0));
    }

    #[test]
    fn test_apply_scroll_axis_helpers_clamp_to_bounds() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.scrollbar_x = Some(true);
        attrs.scrollbar_y = Some(true);
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 180.0,
            content_height: 170.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.apply_scroll_x(&id, -500.0));
        assert!(tree.apply_scroll_y(&id, -500.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_x, Some(80.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_y, Some(70.0));

        assert!(tree.apply_scroll_x(&id, 500.0));
        assert!(tree.apply_scroll_y(&id, 500.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_x, Some(0.0));
        assert_eq!(tree.get(&id).unwrap().attrs.scroll_y, Some(0.0));
    }

    #[test]
    fn test_set_scrollbar_hover_axis_noop_when_axis_disabled() {
        let id = ElementId::from_term_bytes(vec![1]);
        let attrs = Attrs::default();
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_scrollbar_x_hover(&id, true));
        assert!(!tree.set_scrollbar_y_hover(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_set_mouse_over_active_requires_mouse_over_attrs() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_mouse_over_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, None);
    }

    #[test]
    fn test_set_mouse_over_active_toggles_state() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.mouse_over = Some(MouseOverAttrs {
            alpha: Some(0.6),
            ..Default::default()
        });
        let mut element = Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 100.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_mouse_over_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, Some(true));

        assert!(!tree.set_mouse_over_active(&id, true));

        assert!(tree.set_mouse_over_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_over_active, Some(false));
    }

    #[test]
    fn test_text_input_focus_and_cursor_defaults() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
            content_width: 100.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_focus(Some(&id)));
        let focused = tree.get(&id).unwrap();
        assert_eq!(focused.attrs.text_input_focused, Some(true));
        assert_eq!(focused.attrs.text_input_cursor, Some(5));

        assert!(tree.set_text_input_focus(None));
        let blurred = tree.get(&id).unwrap();
        assert_eq!(blurred.attrs.text_input_focused, Some(false));
    }

    #[test]
    fn test_text_input_edit_operations() {
        let id = ElementId::from_term_bytes(vec![1]);
        let mut attrs = Attrs::default();
        attrs.content = Some("abc".to_string());
        attrs.text_input_cursor = Some(3);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
            content_width: 100.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.text_input_move_left(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(2));

        let edited = tree.text_input_backspace(&id).unwrap();
        assert_eq!(edited, "ac");
        assert_eq!(tree.get(&id).unwrap().attrs.content.as_deref(), Some("ac"));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(1));

        let inserted = tree.text_input_insert(&id, "Z").unwrap();
        assert_eq!(inserted, "aZc");
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(2));

        assert!(tree.text_input_move_home(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(0));

        let deleted = tree.text_input_delete(&id).unwrap();
        assert_eq!(deleted, "Zc");
        assert_eq!(tree.get(&id).unwrap().attrs.content.as_deref(), Some("Zc"));
    }

    #[test]
    fn test_set_text_input_cursor_from_point_maps_click_to_nearest_caret() {
        let id = ElementId::from_term_bytes(vec![2]);
        let mut attrs = Attrs::default();
        attrs.content = Some("abcd".to_string());
        attrs.font_size = Some(16.0);
        attrs.text_input_cursor = Some(3);

        let mut element = Element::with_attrs(
            id.clone(),
            ElementKind::TextInput,
            Vec::new(),
            attrs.clone(),
        );
        element.frame = Some(Frame {
            x: 10.0,
            y: 5.0,
            width: 220.0,
            height: 32.0,
            content_width: 220.0,
            content_height: 32.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_cursor_from_point(&id, 0.0, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(0));

        let node = tree.get(&id).unwrap();
        let content = node.base_attrs.content.as_deref().unwrap();
        let (family, weight, italic) = font_info_from_attrs(&node.attrs);
        let text_width = measure_text_width(content, 16.0, &family, weight, italic, 0.0, 0.0);
        let text_start = node.frame.unwrap().x;
        assert!(tree.set_text_input_cursor_from_point(&id, text_start + text_width + 20.0, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(4));

        let prefix_1 = measure_text_width("a", 16.0, &family, weight, italic, 0.0, 0.0);
        let prefix_2 = measure_text_width("ab", 16.0, &family, weight, italic, 0.0, 0.0);
        let click_x = text_start + (prefix_1 + prefix_2) / 2.0 + 0.5;

        assert!(tree.set_text_input_cursor_from_point(&id, click_x, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(2));
    }

    #[test]
    fn test_set_text_input_cursor_from_point_respects_center_alignment() {
        let id = ElementId::from_term_bytes(vec![6]);
        let mut attrs = Attrs::default();
        attrs.content = Some("center".to_string());
        attrs.font_size = Some(16.0);
        attrs.text_align = Some(TextAlign::Center);

        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 32.0,
            content_width: 300.0,
            content_height: 32.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_cursor_from_point(&id, 0.0, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(0));

        assert!(tree.set_text_input_cursor_from_point(&id, 300.0, false));
        assert_eq!(tree.get(&id).unwrap().attrs.text_input_cursor, Some(6));
    }

    #[test]
    fn test_text_input_preedit_set_clear_and_blur() {
        let id = ElementId::from_term_bytes(vec![3]);
        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
            content_width: 100.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_text_input_focus(Some(&id)));
        assert!(tree.set_text_input_preedit(&id, "ka".to_string(), Some((2, 2))));
        let with_preedit = tree.get(&id).unwrap();
        assert_eq!(with_preedit.attrs.text_input_preedit.as_deref(), Some("ka"));
        assert_eq!(with_preedit.attrs.text_input_preedit_cursor, Some((2, 2)));

        assert!(tree.clear_text_input_preedit(&id));
        let cleared = tree.get(&id).unwrap();
        assert_eq!(cleared.attrs.text_input_preedit, None);
        assert_eq!(cleared.attrs.text_input_preedit_cursor, None);

        assert!(tree.set_text_input_preedit(&id, "nih".to_string(), Some((1, 1))));
        assert!(tree.set_text_input_focus(None));
        let blurred = tree.get(&id).unwrap();
        assert_eq!(blurred.attrs.text_input_preedit, None);
        assert_eq!(blurred.attrs.text_input_preedit_cursor, None);
    }

    #[test]
    fn test_text_input_preedit_requires_focus() {
        let id = ElementId::from_term_bytes(vec![4]);
        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
            content_width: 100.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_text_input_preedit(&id, "ka".to_string(), Some((2, 2))));

        let node = tree.get(&id).unwrap();
        assert_eq!(node.attrs.text_input_preedit, None);
        assert_eq!(node.attrs.text_input_preedit_cursor, None);
    }

    #[test]
    fn test_text_input_selection_and_clipboard_ops() {
        let id = ElementId::from_term_bytes(vec![7]);
        let mut attrs = Attrs::default();
        attrs.content = Some("abcd".to_string());
        attrs.text_input_cursor = Some(4);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.text_input_move_left(&id, true));
        let node = tree.get(&id).unwrap();
        assert_eq!(node.attrs.text_input_cursor, Some(3));
        assert_eq!(node.attrs.text_input_selection_anchor, Some(4));

        assert_eq!(tree.text_input_copy_selection(&id).as_deref(), Some("d"));

        let (cut_value, cut_text) = tree.text_input_cut_selection(&id).unwrap();
        assert_eq!(cut_text, "d");
        assert_eq!(cut_value, "abc");

        let node = tree.get(&id).unwrap();
        assert_eq!(node.attrs.content.as_deref(), Some("abc"));
        assert_eq!(node.attrs.text_input_cursor, Some(3));
        assert_eq!(node.attrs.text_input_selection_anchor, None);

        let pasted = tree.text_input_paste_text(&id, "X\nY").unwrap();
        assert_eq!(pasted, "abcX Y");
    }

    #[test]
    fn test_text_input_select_all_replaces_on_insert() {
        let id = ElementId::from_term_bytes(vec![8]);
        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        attrs.text_input_cursor = Some(5);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::TextInput, Vec::new(), attrs);
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 24.0,
            content_width: 120.0,
            content_height: 24.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.text_input_select_all(&id));
        let node = tree.get(&id).unwrap();
        assert_eq!(node.attrs.text_input_selection_anchor, Some(0));
        assert_eq!(node.attrs.text_input_cursor, Some(5));

        let inserted = tree.text_input_insert(&id, "z").unwrap();
        assert_eq!(inserted, "z");
        let node = tree.get(&id).unwrap();
        assert_eq!(node.attrs.text_input_cursor, Some(1));
        assert_eq!(node.attrs.text_input_selection_anchor, None);
    }
}
