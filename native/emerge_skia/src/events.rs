//! # Event Domain Types
//!
//! This module defines the shared event-side data used by the tree actor, the
//! event actor, and backend integration.
//!
//! The event system has two main phases:
//!
//! - the tree actor rebuilds listener data and retained interaction metadata
//! - the event actor dispatches input against that rebuilt listener data while
//!   managing transient runtime interaction state
//!
//! This file contains the shared types that cross that boundary:
//!
//! - emitted element event kinds
//! - focused text-input runtime state and layout metadata
//! - the tree-to-event rebuild payload installed by the event actor
//!
//! The main supporting submodules are:
//!
//! - `registry_builder`
//!   - builds listener registries from the retained tree and from transient
//!     runtime state
//! - `runtime`
//!   - runs the event actor, dispatches input, and manages runtime interaction
//!     state
//! - `scrollbar`
//!   - shared scrollbar geometry and hit area helpers
//! - `text_ops`
//!   - shared single-line text editing helpers
use rustler::{Atom, Encoder, LocalPid, OwnedBinary, OwnedEnv};
use std::collections::HashMap;

use crate::input::InputEvent;
use crate::native_log::NativeLogLevel;
use crate::renderer::{make_font_with_style, measure_text_visual_metrics_with_font};
use crate::tree::attrs::{BorderWidth, Font, Padding, TextAlign};
#[cfg(test)]
use crate::tree::element::ElementKind;
#[cfg(test)]
use crate::tree::element::ElementTree;
use crate::tree::element::{Element, ElementId, TextInputContentOrigin};
use crate::tree::geometry::Rect;
#[cfg(test)]
use crate::tree::render::render_tree;
use crate::tree::scrollbar::ScrollbarAxis;
use crate::tree::text_layout::{TextLayoutStyle, layout_text_lines};
use crate::tree::transform::{Affine2, Point};

pub mod registry_builder;
mod runtime;
pub mod scrollbar;
#[cfg(test)]
pub mod test_support;
pub mod text_ops;

pub(crate) use runtime::{SpawnEventActorConfig, spawn_event_actor};
pub use runtime::{HostEventRuntime, HostEventSink};
use scrollbar::ScrollbarNode;

/// Element-level events that Rust can emit back to Elixir after input matching.
///
/// These are semantic events derived from listener actions, not raw backend
/// input packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementEventKind {
    Click,
    Press,
    SwipeUp,
    SwipeDown,
    SwipeLeft,
    SwipeRight,
    KeyDown,
    KeyUp,
    KeyPress,
    VirtualKeyHold,
    MouseDown,
    MouseUp,
    MouseEnter,
    MouseLeave,
    MouseMove,
    Focus,
    Blur,
    Change,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorIcon {
    Default,
    Text,
    Pointer,
}

/// Unified text-input state used by both rebuild output and runtime editing.
///
/// `TextInputState` combines:
///
/// - live editing state (`content`, `cursor`, `selection_anchor`, `preedit`)
/// - focused-only pending tree patch text (`patch_content`)
/// - focus/runtime flags
/// - layout and font metadata needed to place the caret, selection, and preedit
///   correctly after rebuild
///
/// During focused editing, runtime cursor/selection/preedit state remains the
/// source of truth. Rebuilds refresh geometry and style metadata without
/// discarding active editing state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextInputState {
    pub content: String,
    pub patch_content: Option<String>,
    pub content_origin: TextInputContentOrigin,
    pub content_len: u32,
    pub cursor: u32,
    pub selection_anchor: Option<u32>,
    pub preedit: Option<String>,
    pub preedit_cursor: Option<(u32, u32)>,
    pub focused: bool,
    pub emit_change: bool,
    pub multiline: bool,
    pub frame_x: f32,
    pub frame_y: f32,
    pub frame_width: f32,
    pub frame_height: f32,
    pub inset_top: f32,
    pub inset_left: f32,
    pub inset_bottom: f32,
    pub inset_right: f32,
    pub screen_to_local: Option<Affine2>,
    pub text_align: TextAlign,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_italic: bool,
    pub letter_spacing: f32,
    pub word_spacing: f32,
}

impl TextInputState {
    pub fn copy_rebuild_metadata_from(&mut self, other: &Self) {
        self.patch_content = other.patch_content.clone();
        self.emit_change = other.emit_change;
        self.multiline = other.multiline;
        self.frame_x = other.frame_x;
        self.frame_y = other.frame_y;
        self.frame_width = other.frame_width;
        self.frame_height = other.frame_height;
        self.inset_top = other.inset_top;
        self.inset_left = other.inset_left;
        self.inset_bottom = other.inset_bottom;
        self.inset_right = other.inset_right;
        self.screen_to_local = other.screen_to_local;
        self.text_align = other.text_align;
        self.font_family = other.font_family.clone();
        self.font_size = other.font_size;
        self.font_weight = other.font_weight;
        self.font_italic = other.font_italic;
        self.letter_spacing = other.letter_spacing;
        self.word_spacing = other.word_spacing;
    }

    pub fn sync_content_metadata(&mut self) {
        let len = text_ops::text_char_len(&self.content);
        self.content_len = len;
        self.cursor = self.cursor.min(len);
        self.selection_anchor = self.selection_anchor.map(|anchor| anchor.min(len));
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
    }

    pub fn clear_preedit(&mut self) -> bool {
        let had_preedit = self.preedit.take().is_some();
        let had_cursor = self.preedit_cursor.take().is_some();
        had_preedit || had_cursor
    }

    pub fn set_selection_range(&mut self, start: u32, end: u32) -> bool {
        let len = text_ops::text_char_len(&self.content);
        let start = start.min(len);
        let end = end.min(len);
        let next_start = start.min(end);
        let next_end = start.max(end);
        let next_anchor = (next_start != next_end).then_some(next_start);
        let mut changed = false;

        if self.cursor != next_end {
            self.cursor = next_end;
            changed = true;
        }

        if self.selection_anchor != next_anchor {
            self.selection_anchor = next_anchor;
            changed = true;
        }

        if self.clear_preedit() {
            changed = true;
        }

        self.sync_content_metadata();
        changed
    }

    pub fn set_content(&mut self, content: String) -> bool {
        let mut changed = self.content != content;
        self.content = content;

        let len = text_ops::text_char_len(&self.content);
        let clamped_cursor = self.cursor.min(len);
        if clamped_cursor != self.cursor {
            self.cursor = clamped_cursor;
            changed = true;
        }

        let next_anchor = self
            .selection_anchor
            .map(|anchor| anchor.min(len))
            .filter(|anchor| *anchor != self.cursor);
        if self.selection_anchor != next_anchor {
            self.selection_anchor = next_anchor;
            changed = true;
        }

        if self.clear_preedit() {
            changed = true;
        }

        self.sync_content_metadata();
        changed
    }

    pub fn set_runtime(
        &mut self,
        focused: bool,
        cursor: Option<u32>,
        selection_anchor: Option<u32>,
        preedit: Option<String>,
        preedit_cursor: Option<(u32, u32)>,
    ) -> bool {
        let len = text_ops::text_char_len(&self.content);
        let next_cursor = cursor.unwrap_or(self.cursor).min(len);

        let mut next_anchor = selection_anchor.map(|anchor| anchor.min(len));
        if !focused || next_anchor == Some(next_cursor) {
            next_anchor = None;
        }

        let next_preedit = if focused {
            preedit.filter(|value| !value.is_empty())
        } else {
            None
        };
        let next_preedit_cursor = if focused {
            Self::normalize_preedit_cursor(next_preedit.as_deref(), preedit_cursor)
        } else {
            None
        };

        let changed = self.focused != focused
            || self.cursor != next_cursor
            || self.selection_anchor != next_anchor
            || self.preedit != next_preedit
            || self.preedit_cursor != next_preedit_cursor;

        self.focused = focused;
        self.cursor = next_cursor;
        self.selection_anchor = next_anchor;
        self.preedit = next_preedit;
        self.preedit_cursor = next_preedit_cursor;
        self.sync_content_metadata();
        changed
    }

    pub fn normalize_runtime(&mut self) -> bool {
        let mut changed = false;
        let len = text_ops::text_char_len(&self.content);

        if self.cursor > len {
            self.cursor = len;
            changed = true;
        }

        if let Some(anchor) = self.selection_anchor {
            let anchor = anchor.min(len);
            let next_anchor = if anchor == self.cursor {
                None
            } else {
                Some(anchor)
            };
            if self.selection_anchor != next_anchor {
                self.selection_anchor = next_anchor;
                changed = true;
            }
        }

        if !self.focused {
            if self.selection_anchor.take().is_some() {
                changed = true;
            }
            if self.preedit.take().is_some() {
                changed = true;
            }
            if self.preedit_cursor.take().is_some() {
                changed = true;
            }
        } else {
            let normalized =
                Self::normalize_preedit_cursor(self.preedit.as_deref(), self.preedit_cursor);
            if self.preedit_cursor != normalized {
                self.preedit_cursor = normalized;
                changed = true;
            }
        }

        self.sync_content_metadata();
        changed
    }

    pub fn selected_range(&self) -> Option<(u32, u32)> {
        text_ops::selected_range(self.cursor, self.selection_anchor, self.content_len)
    }

    pub fn selection_text(&self) -> Option<String> {
        let (start, end) = self.selected_range()?;
        Some(
            self.content
                .chars()
                .skip(start as usize)
                .take((end - start) as usize)
                .collect(),
        )
    }

    pub fn apply_content_change(&mut self, next_content: String, next_cursor: u32) {
        self.content = next_content;
        self.cursor = next_cursor.min(text_ops::text_char_len(&self.content));
        self.selection_anchor = None;
        self.preedit = None;
        self.preedit_cursor = None;
        self.sync_content_metadata();
    }

    pub fn move_cursor(&mut self, next_cursor: u32, extend_selection: bool) -> bool {
        let next_cursor = next_cursor.min(self.content_len);
        let mut changed = false;

        if extend_selection {
            let anchor = self.selection_anchor.unwrap_or(self.cursor);
            let next_anchor = if anchor == next_cursor {
                None
            } else {
                Some(anchor)
            };
            if self.selection_anchor != next_anchor {
                self.selection_anchor = next_anchor;
                changed = true;
            }
        } else if self.selection_anchor.take().is_some() {
            changed = true;
        }

        if self.cursor != next_cursor {
            self.cursor = next_cursor;
            changed = true;
        }

        if self.clear_preedit() {
            changed = true;
        }

        self.sync_content_metadata();
        changed
    }

    pub fn cursor_from_click_point(&self, x: f32, y: f32) -> u32 {
        let local = self
            .screen_to_local
            .map(|transform| transform.map_point(Point { x, y }))
            .unwrap_or(Point { x, y });

        if self.multiline {
            self.cursor_from_local_point(local.x, local.y)
        } else {
            self.cursor_from_local_x(local.x)
        }
    }

    fn cursor_from_local_x(&self, x: f32) -> u32 {
        let (text_left_overhang, text_width) = self.measure_text_visual_metrics(&self.content);
        let content_width = (self.frame_width - self.inset_left - self.inset_right).max(0.0);

        let text_start_x = match self.text_align {
            TextAlign::Left => self.frame_x + self.inset_left + text_left_overhang,
            TextAlign::Center => {
                self.frame_x
                    + self.inset_left
                    + (content_width - text_width) / 2.0
                    + text_left_overhang
            }
            TextAlign::Right => {
                self.frame_x + self.frame_width - self.inset_right - text_width + text_left_overhang
            }
        };

        let click_x = (x - text_start_x).clamp(0.0, text_width.max(0.0));
        self.nearest_char_index_for_offset(&self.content, click_x)
    }

    fn cursor_from_local_point(&self, x: f32, y: f32) -> u32 {
        let layout = self.text_layout(&self.content);
        let local_y = (y - (self.frame_y + self.inset_top)).max(0.0);
        let line_index = layout.line_index_for_y(local_y);
        let line = &layout.lines[line_index];
        let click_x = (x - self.line_x(line.width)).clamp(0.0, line.width.max(0.0));
        line.nearest_cursor_for_x(click_x) as u32
    }

    pub fn move_home_target(&self) -> u32 {
        if !self.multiline {
            return 0;
        }

        let layout = self.text_layout(&self.content);
        let line = &layout.lines[layout.line_index_for_cursor(self.cursor as usize)];
        line.start as u32
    }

    pub fn move_end_target(&self) -> u32 {
        if !self.multiline {
            return self.content_len;
        }

        let layout = self.text_layout(&self.content);
        let line = &layout.lines[layout.line_index_for_cursor(self.cursor as usize)];
        line.visual_end as u32
    }

    pub fn move_word_left_target(&self) -> u32 {
        text_ops::move_word_left_target(&self.content, self.cursor)
    }

    pub fn move_word_right_target(&self) -> u32 {
        text_ops::move_word_right_target(&self.content, self.cursor)
    }

    pub fn move_paragraph_start_target(&self) -> u32 {
        text_ops::move_paragraph_start_target(&self.content, self.cursor)
    }

    pub fn move_paragraph_end_target(&self) -> u32 {
        text_ops::move_paragraph_end_target(&self.content, self.cursor)
    }

    pub fn move_document_start_target(&self) -> u32 {
        0
    }

    pub fn move_document_end_target(&self) -> u32 {
        self.content_len
    }

    pub fn move_vertical_target(&self, direction: i32) -> u32 {
        if !self.multiline {
            return self.cursor;
        }

        let layout = self.text_layout(&self.content);
        let current_line_index = layout.line_index_for_cursor(self.cursor as usize);
        let next_line_index = (current_line_index as i32 + direction)
            .clamp(0, layout.lines.len() as i32 - 1) as usize;
        if next_line_index == current_line_index {
            return self.cursor;
        }

        let current_line = &layout.lines[current_line_index];
        let target_x = current_line.offset_for_cursor(self.cursor as usize);
        layout.lines[next_line_index].nearest_cursor_for_x(target_x) as u32
    }

    pub fn appkit_selected_range_utf16(&self) -> (usize, usize) {
        let displayed_text = self.displayed_text();

        match self.preedit_selection_char_range() {
            Some((start, end)) => {
                let location = char_index_to_utf16_offset(&displayed_text, start);
                let end = char_index_to_utf16_offset(&displayed_text, end);
                (location, end.saturating_sub(location))
            }
            None => {
                let (start, end) = self.selected_range().unwrap_or((self.cursor, self.cursor));
                let location = char_index_to_utf16_offset(&self.content, start);
                let end = char_index_to_utf16_offset(&self.content, end);
                (location, end.saturating_sub(location))
            }
        }
    }

    pub fn appkit_marked_range_utf16(&self) -> Option<(usize, usize)> {
        let displayed_text = self.displayed_text();
        let (start, end) = self.marked_char_range()?;
        let location = char_index_to_utf16_offset(&displayed_text, start);
        let end = char_index_to_utf16_offset(&displayed_text, end);
        Some((location, end.saturating_sub(location)))
    }

    pub fn appkit_displayed_text(&self) -> String {
        self.displayed_text()
    }

    pub fn appkit_substring_for_utf16_range(
        &self,
        location: usize,
        length: usize,
    ) -> Option<String> {
        let displayed_text = self.displayed_text();
        let start = utf16_offset_to_char_index(&displayed_text, location);
        let end = utf16_offset_to_char_index(&displayed_text, location.saturating_add(length));
        Some(
            displayed_text
                .chars()
                .skip(start as usize)
                .take((end.saturating_sub(start)) as usize)
                .collect(),
        )
    }

    pub fn appkit_character_index_for_point_utf16(&self, x: f32, y: f32) -> usize {
        let displayed_text = self.displayed_text();
        let local = self
            .screen_to_local
            .map(|transform| transform.map_point(Point { x, y }))
            .unwrap_or(Point { x, y });

        let char_index = if self.multiline {
            self.cursor_from_local_point_in_text(&displayed_text, local.x, local.y)
        } else {
            self.cursor_from_local_x_in_text(&displayed_text, local.x)
        };

        char_index_to_utf16_offset(&displayed_text, char_index)
    }

    pub fn appkit_first_rect_for_utf16_range(
        &self,
        location: usize,
        length: usize,
    ) -> Option<(f32, f32, f32, f32)> {
        let displayed_text = self.displayed_text();
        let start = utf16_offset_to_char_index(&displayed_text, location);
        let end = utf16_offset_to_char_index(&displayed_text, location.saturating_add(length));
        self.rect_for_char_range_in_text(&displayed_text, start, end.max(start))
    }

    pub fn appkit_replacement_char_range(
        &self,
        location: usize,
        length: usize,
    ) -> Option<(u32, u32)> {
        if location == usize::MAX {
            return None;
        }

        let displayed_text = self.displayed_text();
        let start = utf16_offset_to_char_index(&displayed_text, location);
        let end = utf16_offset_to_char_index(&displayed_text, location.saturating_add(length));

        Some((
            self.displayed_char_index_to_committed(start, false),
            self.displayed_char_index_to_committed(end, true),
        ))
    }

    fn displayed_text(&self) -> String {
        let preedit = self.preedit.as_deref().filter(|value| !value.is_empty());
        let (base_start, base_end) = self.preedit_base_range();

        match preedit {
            Some(preedit_text) => {
                let prefix: String = self.content.chars().take(base_start as usize).collect();
                let suffix: String = self.content.chars().skip(base_end as usize).collect();
                let mut displayed =
                    String::with_capacity(prefix.len() + preedit_text.len() + suffix.len());
                displayed.push_str(&prefix);
                displayed.push_str(preedit_text);
                displayed.push_str(&suffix);
                displayed
            }
            None => self.content.clone(),
        }
    }

    fn marked_char_range(&self) -> Option<(u32, u32)> {
        let preedit = self.preedit.as_deref().filter(|value| !value.is_empty())?;
        let (start, _) = self.preedit_base_range();
        let end = start + text_ops::text_char_len(preedit);
        Some((start, end))
    }

    fn preedit_selection_char_range(&self) -> Option<(u32, u32)> {
        let preedit = self.preedit.as_deref().filter(|value| !value.is_empty())?;
        let (base, _) = self.preedit_base_range();
        let preedit_len = text_ops::text_char_len(preedit);
        let (start, end) = self
            .preedit_cursor
            .map(|(start, end)| (start.min(preedit_len), end.min(preedit_len)))
            .unwrap_or((preedit_len, preedit_len));
        Some((base + start, base + end))
    }

    fn preedit_base_range(&self) -> (u32, u32) {
        let content_len = text_ops::text_char_len(&self.content);
        let cursor = self.cursor.min(content_len);

        if self.preedit.is_some() {
            self.selected_range().unwrap_or((cursor, cursor))
        } else {
            (cursor, cursor)
        }
    }

    fn displayed_char_index_to_committed(&self, index: u32, prefer_end: bool) -> u32 {
        let Some(preedit) = self.preedit.as_deref().filter(|value| !value.is_empty()) else {
            return index.min(self.content_len);
        };

        let (replace_start, replace_end) = self.preedit_base_range();
        let preedit_len = text_ops::text_char_len(preedit);
        let displayed_preedit_start = replace_start;
        let displayed_preedit_end = replace_start + preedit_len;

        if index <= displayed_preedit_start {
            index.min(self.content_len)
        } else if index >= displayed_preedit_end {
            (index - preedit_len + (replace_end - replace_start)).min(self.content_len)
        } else if prefer_end {
            replace_end
        } else {
            replace_start
        }
    }

    fn cursor_from_local_x_in_text(&self, text: &str, x: f32) -> u32 {
        let (text_left_overhang, text_width) = self.measure_text_visual_metrics(text);
        let text_start_x = self.text_start_x(text_width, text_left_overhang);
        let click_x = (x - text_start_x).clamp(0.0, text_width.max(0.0));
        self.nearest_char_index_for_offset(text, click_x)
    }

    fn cursor_from_local_point_in_text(&self, text: &str, x: f32, y: f32) -> u32 {
        let layout = self.text_layout(text);
        let local_y = (y - (self.frame_y + self.inset_top)).max(0.0);
        let line_index = layout.line_index_for_y(local_y);
        let line = &layout.lines[line_index];
        let click_x = (x - self.line_x(line.width)).clamp(0.0, line.width.max(0.0));
        line.nearest_cursor_for_x(click_x) as u32
    }

    fn rect_for_char_range_in_text(
        &self,
        text: &str,
        start: u32,
        end: u32,
    ) -> Option<(f32, f32, f32, f32)> {
        if self.multiline {
            self.multiline_rect_for_char_range(text, start, end)
        } else {
            Some(self.single_line_rect_for_char_range(text, start, end))
        }
    }

    fn single_line_rect_for_char_range(
        &self,
        text: &str,
        start: u32,
        end: u32,
    ) -> (f32, f32, f32, f32) {
        let (text_left_overhang, text_width) = self.measure_text_visual_metrics(text);
        let text_x = self.text_start_x(text_width, text_left_overhang);
        let start_offset = self.text_offset_for_char_index(text, start as usize);
        let end_offset = self.text_offset_for_char_index(text, end as usize);
        let (ascent, descent) = self.text_metrics();
        let top = self.frame_y + self.inset_top;
        let height = (ascent + descent).max(self.font_size * 0.9);
        let caret_width = (self.font_size * 0.08).max(1.0);
        let width = if start == end {
            caret_width
        } else {
            (end_offset - start_offset).abs().max(caret_width)
        };

        (text_x + start_offset, top, width, height)
    }

    fn multiline_rect_for_char_range(
        &self,
        text: &str,
        start: u32,
        end: u32,
    ) -> Option<(f32, f32, f32, f32)> {
        let layout = self.text_layout(text);
        let start_index = start.min(text.chars().count() as u32) as usize;
        let end_index = end.min(text.chars().count() as u32) as usize;
        let line_index = layout.line_index_for_cursor(start_index);
        let line = layout.lines.get(line_index)?;
        let start_x = self.line_x(line.width) + line.offset_for_cursor(start_index);
        let end_line_index = layout
            .line_index_for_cursor(end_index.saturating_sub((end_index > start_index) as usize));
        let end_x = if start_index == end_index {
            start_x + (self.font_size * 0.08).max(1.0)
        } else if end_line_index == line_index {
            self.line_x(line.width) + line.offset_for_cursor(end_index)
        } else {
            self.line_x(line.width) + line.width
        };
        let top = self.frame_y + self.inset_top + line_index as f32 * layout.line_height;
        let height = layout.line_height.max(self.font_size * 0.9);
        let width = (end_x - start_x)
            .abs()
            .max((self.font_size * 0.08).max(1.0));
        Some((start_x, top, width, height))
    }

    fn text_start_x(&self, text_width: f32, text_left_overhang: f32) -> f32 {
        let content_width = (self.frame_width - self.inset_left - self.inset_right).max(0.0);
        match self.text_align {
            TextAlign::Left => self.frame_x + self.inset_left + text_left_overhang,
            TextAlign::Center => {
                self.frame_x
                    + self.inset_left
                    + (content_width - text_width) / 2.0
                    + text_left_overhang
            }
            TextAlign::Right => {
                self.frame_x + self.frame_width - self.inset_right - text_width + text_left_overhang
            }
        }
    }

    fn text_offset_for_char_index(&self, text: &str, char_index: usize) -> f32 {
        let target = char_index.min(text.chars().count());
        let font = make_font_with_style(
            &self.font_family,
            self.font_weight,
            self.font_italic,
            self.font_size,
        );

        let chars: Vec<char> = text.chars().collect();
        let mut advance = 0.0;
        for (idx, ch) in chars.iter().take(target).enumerate() {
            let glyph = ch.to_string();
            let (glyph_width, _bounds) = font.measure_str(&glyph, None);
            advance += glyph_width;
            if idx + 1 < chars.len() {
                advance += self.letter_spacing;
                if ch.is_whitespace() {
                    advance += self.word_spacing;
                }
            }
        }
        advance
    }

    fn text_layout(&self, text: &str) -> crate::tree::text_layout::TextLayout {
        let font = make_font_with_style(
            &self.font_family,
            self.font_weight,
            self.font_italic,
            self.font_size,
        );
        let wrap_width = self
            .multiline
            .then_some((self.frame_width - self.inset_left - self.inset_right).max(0.0));
        layout_text_lines(
            text,
            wrap_width,
            self.text_metrics(),
            TextLayoutStyle {
                font_size: self.font_size,
                letter_spacing: self.letter_spacing,
                word_spacing: self.word_spacing,
            },
            |ch| font.measure_str(ch.to_string(), None).0,
        )
    }

    fn text_metrics(&self) -> (f32, f32) {
        let font = make_font_with_style(
            &self.font_family,
            self.font_weight,
            self.font_italic,
            self.font_size,
        );
        let (_, metrics) = font.metrics();
        (metrics.ascent.abs(), metrics.descent)
    }

    fn line_x(&self, line_width: f32) -> f32 {
        let content_width = (self.frame_width - self.inset_left - self.inset_right).max(0.0);
        match self.text_align {
            TextAlign::Left => self.frame_x + self.inset_left,
            TextAlign::Center => {
                self.frame_x + self.inset_left + (content_width - line_width) / 2.0
            }
            TextAlign::Right => self.frame_x + self.frame_width - self.inset_right - line_width,
        }
    }

    fn measure_text_visual_metrics(&self, text: &str) -> (f32, f32) {
        if text.is_empty() {
            return (0.0, 0.0);
        }

        let font = make_font_with_style(
            &self.font_family,
            self.font_weight,
            self.font_italic,
            self.font_size,
        );

        if self.letter_spacing == 0.0 && self.word_spacing == 0.0 {
            let metrics = measure_text_visual_metrics_with_font(&font, text);
            return (metrics.left_overhang, metrics.visual_width);
        }

        let mut total = 0.0;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            let glyph = ch.to_string();
            let (glyph_width, _bounds) = font.measure_str(&glyph, None);
            total += glyph_width;

            if chars.peek().is_some() {
                total += self.letter_spacing;
                if ch.is_whitespace() {
                    total += self.word_spacing;
                }
            }
        }

        (0.0, total)
    }

    fn nearest_char_index_for_offset(&self, text: &str, offset_x: f32) -> u32 {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return 0;
        }

        let font = make_font_with_style(
            &self.font_family,
            self.font_weight,
            self.font_italic,
            self.font_size,
        );

        let mut positions = Vec::with_capacity(chars.len() + 1);
        positions.push(0.0);

        let mut advance = 0.0;
        for (idx, ch) in chars.iter().enumerate() {
            let glyph = ch.to_string();
            let (glyph_width, _bounds) = font.measure_str(&glyph, None);
            advance += glyph_width;
            if idx + 1 < chars.len() {
                advance += self.letter_spacing;
                if ch.is_whitespace() {
                    advance += self.word_spacing;
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

    pub(crate) fn normalize_preedit_cursor(
        text: Option<&str>,
        cursor: Option<(u32, u32)>,
    ) -> Option<(u32, u32)> {
        let text_len = text.map(text_ops::text_char_len)?;
        let (mut start, mut end) = cursor?;
        start = start.min(text_len);
        end = end.min(text_len);
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        Some((start, end))
    }
}

fn char_index_to_utf16_offset(text: &str, char_index: u32) -> usize {
    text.chars()
        .take(char_index as usize)
        .map(char::len_utf16)
        .sum()
}

fn utf16_offset_to_char_index(text: &str, utf16_offset: usize) -> u32 {
    let mut utf16_count = 0;
    let mut char_count = 0;

    for ch in text.chars() {
        let next = utf16_count + ch.len_utf16();
        if next > utf16_offset {
            break;
        }

        utf16_count = next;
        char_count += 1;
    }

    char_count
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEditRequest {
    MoveLeft {
        extend_selection: bool,
    },
    MoveRight {
        extend_selection: bool,
    },
    MoveWordLeft {
        extend_selection: bool,
    },
    MoveWordRight {
        extend_selection: bool,
    },
    MoveUp {
        extend_selection: bool,
    },
    MoveDown {
        extend_selection: bool,
    },
    MoveHome {
        extend_selection: bool,
    },
    MoveEnd {
        extend_selection: bool,
    },
    MoveParagraphStart {
        extend_selection: bool,
    },
    MoveParagraphEnd {
        extend_selection: bool,
    },
    MoveDocumentStart {
        extend_selection: bool,
    },
    MoveDocumentEnd {
        extend_selection: bool,
    },
    Backspace,
    Delete,
    DeleteWordBackward,
    DeleteWordForward,
    DeleteToHome,
    DeleteToEnd,
    DeleteToParagraphStart,
    DeleteToParagraphEnd,
    DeleteSurrounding {
        before_length: u32,
        after_length: u32,
    },
    Insert(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputCommandRequest {
    SelectAll,
    Copy,
    Cut,
    Paste,
    PastePrimary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputPreeditRequest {
    Set {
        text: String,
        cursor: Option<(u32, u32)>,
    },
    Clear,
}

#[derive(Clone, Debug)]
pub struct FocusOnMountTarget {
    pub element_id: ElementId,
    pub reveal_scrolls: Vec<registry_builder::FocusRevealScroll>,
    pub mounted_at_revision: u64,
}

#[derive(Clone, Debug)]
/// Tree-to-event rebuild payload installed by the event actor.
///
/// The tree actor produces this during the fused render/rebuild walk. It
/// contains the rebuilt base listener registry plus retained metadata that the
/// event actor needs to reconcile transient runtime state:
///
/// - `base_registry` for normal listener matching
/// - `text_inputs` for focused text editing reconciliation
/// - `scrollbars` for active scrollbar drag reconciliation
/// - `focused_id` for focus reconciliation
/// - `focus_on_mount` for one-shot mount-time focus requests
#[derive(Default)]
pub struct RegistryRebuildPayload {
    pub base_registry: registry_builder::Registry,
    pub text_inputs: HashMap<ElementId, TextInputState>,
    pub scrollbars: HashMap<(ElementId, ScrollbarAxis), ScrollbarNode>,
    pub focused_id: Option<ElementId>,
    pub focus_on_mount: Option<FocusOnMountTarget>,
}

fn text_input_state(
    element: &Element,
    adjusted_rect: Rect,
    screen_to_local: Option<Affine2>,
) -> TextInputState {
    let content = element.base_attrs.content.clone().unwrap_or_default();
    let content_len = content.chars().count() as u32;
    let cursor = element
        .attrs
        .text_input_cursor
        .unwrap_or(content_len)
        .min(content_len);
    let selection_anchor = element
        .attrs
        .text_input_selection_anchor
        .map(|anchor| anchor.min(content_len))
        .filter(|anchor| *anchor != cursor);
    let (inset_top, inset_right, inset_bottom, inset_left) = text_content_insets(&element.attrs);
    let (font_family, font_weight, font_italic) = font_info_from_attrs(&element.attrs);

    TextInputState {
        content,
        patch_content: element.patch_content.clone(),
        content_origin: element.text_input_content_origin,
        content_len,
        cursor,
        selection_anchor,
        preedit: element.attrs.text_input_preedit.clone(),
        preedit_cursor: element.attrs.text_input_preedit_cursor,
        focused: element.attrs.text_input_focused.unwrap_or(false),
        emit_change: element.attrs.on_change.unwrap_or(false),
        multiline: element.kind == crate::tree::element::ElementKind::Multiline,
        frame_x: adjusted_rect.x,
        frame_y: adjusted_rect.y,
        frame_width: adjusted_rect.width,
        frame_height: adjusted_rect.height,
        inset_top,
        inset_left,
        inset_bottom,
        inset_right,
        screen_to_local,
        text_align: element.attrs.text_align.unwrap_or_default(),
        font_family,
        font_size: element.attrs.font_size.unwrap_or(16.0) as f32,
        font_weight,
        font_italic,
        letter_spacing: element.attrs.font_letter_spacing.unwrap_or(0.0) as f32,
        word_spacing: element.attrs.font_word_spacing.unwrap_or(0.0) as f32,
    }
}

fn text_content_insets(attrs: &crate::tree::attrs::Attrs) -> (f32, f32, f32, f32) {
    let (pad_top, pad_right, pad_bottom, pad_left) = match attrs.padding.as_ref() {
        Some(Padding::Uniform(v)) => (*v as f32, *v as f32, *v as f32, *v as f32),
        Some(Padding::Sides {
            top,
            right,
            bottom,
            left,
        }) => (*top as f32, *right as f32, *bottom as f32, *left as f32),
        None => (0.0, 0.0, 0.0, 0.0),
    };

    let (border_top, border_right, border_bottom, border_left) = match attrs.border_width.as_ref() {
        Some(BorderWidth::Uniform(v)) => (*v as f32, *v as f32, *v as f32, *v as f32),
        Some(BorderWidth::Sides {
            top,
            right,
            bottom,
            left,
        }) => (*top as f32, *right as f32, *bottom as f32, *left as f32),
        None => (0.0, 0.0, 0.0, 0.0),
    };

    (
        pad_top + border_top,
        pad_right + border_right,
        pad_bottom + border_bottom,
        pad_left + border_left,
    )
}

fn font_info_from_attrs(attrs: &crate::tree::attrs::Attrs) -> (String, u16, bool) {
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

pub(crate) fn send_element_event(pid: LocalPid, element_id: &ElementId, event: Atom) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        let mut bin = OwnedBinary::new(element_id.0.len()).unwrap();
        bin.as_mut_slice().copy_from_slice(&element_id.0);
        let id_bin = bin.release(inner_env);
        (emerge_skia_event(), (id_bin, event)).encode(inner_env)
    });
}

pub(crate) fn send_element_event_with_string_payload(
    pid: LocalPid,
    element_id: &ElementId,
    event: Atom,
    value: &str,
) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        let mut bin = OwnedBinary::new(element_id.0.len()).unwrap();
        bin.as_mut_slice().copy_from_slice(&element_id.0);
        let id_bin = bin.release(inner_env);
        (emerge_skia_event(), (id_bin, event, value.to_string())).encode(inner_env)
    });
}

pub(crate) fn send_input_event(pid: LocalPid, event: &InputEvent) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (emerge_skia_event(), event).encode(inner_env)
    });
}

pub(crate) fn send_running_message(pid: LocalPid) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (emerge_viewport_renderer(), heartbeat()).encode(inner_env)
    });
}

#[cfg(all(feature = "wayland", target_os = "linux"))]
pub(crate) fn send_close_message(pid: LocalPid) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (emerge_skia_close(), window_close_requested()).encode(inner_env)
    });
}

#[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
pub(crate) fn send_log_event(pid: LocalPid, level: NativeLogLevel, source: &str, message: &str) {
    let mut env = OwnedEnv::new();
    let _ = env.send_and_clear(&pid, |inner_env| {
        (
            emerge_skia_log(),
            log_level_atom(level),
            source.to_string(),
            message.to_string(),
        )
            .encode(inner_env)
    });
}

rustler::atoms! {
    emerge_skia_event,
    emerge_skia_close,
    emerge_skia_log,
    emerge_viewport_renderer,
    heartbeat,
    click,
    press,
    swipe_up,
    swipe_down,
    swipe_left,
    swipe_right,
    key_down,
    key_up,
    key_press,
    virtual_key_hold,
    change,
    focus,
    blur,
    mouse_down,
    mouse_up,
    mouse_enter,
    mouse_leave,
    mouse_move,
    window_close_requested,
    info,
    warning,
    error,
}

pub(crate) fn click_atom() -> Atom {
    click()
}

pub(crate) fn press_atom() -> Atom {
    press()
}

pub(crate) fn swipe_up_atom() -> Atom {
    swipe_up()
}

pub(crate) fn swipe_down_atom() -> Atom {
    swipe_down()
}

pub(crate) fn swipe_left_atom() -> Atom {
    swipe_left()
}

pub(crate) fn swipe_right_atom() -> Atom {
    swipe_right()
}

pub(crate) fn key_down_atom() -> Atom {
    key_down()
}

pub(crate) fn key_up_atom() -> Atom {
    key_up()
}

pub(crate) fn key_press_atom() -> Atom {
    key_press()
}

pub(crate) fn virtual_key_hold_atom() -> Atom {
    virtual_key_hold()
}

pub(crate) fn change_atom() -> Atom {
    change()
}

pub(crate) fn focus_atom() -> Atom {
    focus()
}

pub(crate) fn blur_atom() -> Atom {
    blur()
}

pub(crate) fn mouse_down_atom() -> Atom {
    mouse_down()
}

pub(crate) fn mouse_up_atom() -> Atom {
    mouse_up()
}

pub(crate) fn mouse_enter_atom() -> Atom {
    mouse_enter()
}

pub(crate) fn mouse_leave_atom() -> Atom {
    mouse_leave()
}

pub(crate) fn mouse_move_atom() -> Atom {
    mouse_move()
}

#[cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
fn log_level_atom(level: NativeLogLevel) -> Atom {
    match level {
        NativeLogLevel::Info => info(),
        NativeLogLevel::Warning => warning(),
        NativeLogLevel::Error => error(),
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::{Element, Frame};
    use crate::tree::transform::Affine2;

    fn make_element(id: u8, kind: ElementKind, attrs: Attrs) -> Element {
        Element::with_attrs(
            ElementId::from_term_bytes(vec![id]),
            kind,
            Vec::new(),
            attrs,
        )
    }

    #[test]
    fn build_registry_rebuild_collects_text_inputs_focus_and_scrollbars() {
        let mut tree = ElementTree::new();

        let mut root = make_element(1, ElementKind::Column, Attrs::default());
        root.children = vec![ElementId::from_term_bytes(vec![2])];
        root.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 120.0,
        });

        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        attrs.text_input_cursor = Some(2);
        attrs.focused_active = Some(true);
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(10.0);
        let mut child = make_element(2, ElementKind::TextInput, attrs);
        child.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 30.0,
            content_width: 100.0,
            content_height: 120.0,
        });

        tree.root = Some(root.id.clone());
        tree.insert(root);
        tree.insert(child);

        let rebuild = render_tree(&tree).event_rebuild;
        assert_eq!(
            rebuild.focused_id,
            Some(ElementId::from_term_bytes(vec![2]))
        );
        assert_eq!(rebuild.text_inputs.len(), 1);
        assert_eq!(
            rebuild
                .text_inputs
                .get(&ElementId::from_term_bytes(vec![2]))
                .expect("text input present")
                .content_origin,
            TextInputContentOrigin::TreePatch
        );
        assert_eq!(
            rebuild
                .text_inputs
                .get(&ElementId::from_term_bytes(vec![2]))
                .expect("text input present")
                .cursor,
            2
        );
        assert_eq!(rebuild.scrollbars.len(), 1);
        assert!(
            rebuild
                .scrollbars
                .contains_key(&(ElementId::from_term_bytes(vec![2]), ScrollbarAxis::Y))
        );
    }

    #[test]
    fn text_input_cursor_from_click_point_uses_screen_to_local_transform() {
        let state = TextInputState {
            content: "ab".to_string(),
            patch_content: None,
            content_origin: TextInputContentOrigin::TreePatch,
            content_len: 2,
            cursor: 0,
            selection_anchor: None,
            preedit: None,
            preedit_cursor: None,
            focused: true,
            emit_change: false,
            multiline: false,
            frame_x: 0.0,
            frame_y: 0.0,
            frame_width: 100.0,
            frame_height: 20.0,
            inset_top: 0.0,
            inset_left: 0.0,
            inset_bottom: 0.0,
            inset_right: 0.0,
            screen_to_local: Some(Affine2::translation(-40.0, -10.0)),
            text_align: TextAlign::Left,
            font_family: "default".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        };

        assert_eq!(state.cursor_from_click_point(40.0, 10.0), 0);
        assert_eq!(state.cursor_from_click_point(140.0, 10.0), 2);
    }

    #[test]
    fn appkit_ranges_account_for_marked_text_and_utf16_offsets() {
        let state = TextInputState {
            content: "a😀b".to_string(),
            patch_content: None,
            content_origin: TextInputContentOrigin::TreePatch,
            content_len: 3,
            cursor: 1,
            selection_anchor: None,
            preedit: Some("é".to_string()),
            preedit_cursor: Some((1, 1)),
            focused: true,
            emit_change: false,
            multiline: false,
            frame_x: 0.0,
            frame_y: 0.0,
            frame_width: 100.0,
            frame_height: 20.0,
            inset_top: 0.0,
            inset_left: 0.0,
            inset_bottom: 0.0,
            inset_right: 0.0,
            screen_to_local: None,
            text_align: TextAlign::Left,
            font_family: "default".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        };

        assert_eq!(state.appkit_displayed_text(), "aé😀b");
        assert_eq!(state.appkit_marked_range_utf16(), Some((1, 1)));
        assert_eq!(state.appkit_selected_range_utf16(), (2, 0));
        assert_eq!(
            state.appkit_substring_for_utf16_range(1, 3),
            Some("é😀".to_string())
        );
    }

    #[test]
    fn appkit_replacement_range_maps_marked_text_back_to_committed_selection() {
        let state = TextInputState {
            content: "abcd".to_string(),
            patch_content: None,
            content_origin: TextInputContentOrigin::TreePatch,
            content_len: 4,
            cursor: 3,
            selection_anchor: Some(1),
            preedit: Some("X".to_string()),
            preedit_cursor: Some((1, 1)),
            focused: true,
            emit_change: false,
            multiline: false,
            frame_x: 0.0,
            frame_y: 0.0,
            frame_width: 100.0,
            frame_height: 20.0,
            inset_top: 0.0,
            inset_left: 0.0,
            inset_bottom: 0.0,
            inset_right: 0.0,
            screen_to_local: None,
            text_align: TextAlign::Left,
            font_family: "default".to_string(),
            font_size: 16.0,
            font_weight: 400,
            font_italic: false,
            letter_spacing: 0.0,
            word_spacing: 0.0,
        };

        assert_eq!(state.appkit_displayed_text(), "aXd");
        assert_eq!(state.appkit_marked_range_utf16(), Some((1, 1)));
        assert_eq!(state.appkit_replacement_char_range(1, 1), Some((1, 3)));
    }
}
