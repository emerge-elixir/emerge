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
use crate::renderer::make_font_with_style;
use crate::tree::attrs::{BorderWidth, Font, Padding, TextAlign};
#[cfg(test)]
use crate::tree::element::ElementKind;
#[cfg(test)]
use crate::tree::element::ElementTree;
use crate::tree::element::{Element, ElementId};
use crate::tree::geometry::Rect;
#[cfg(test)]
use crate::tree::render::render_tree;
use crate::tree::scrollbar::ScrollbarAxis;
use crate::tree::transform::{Affine2, Point};

pub mod registry_builder;
mod runtime;
pub mod scrollbar;
#[cfg(test)]
pub mod test_support;
pub mod text_ops;

pub(crate) use runtime::spawn_event_actor;
use scrollbar::ScrollbarNode;

/// Element-level events that Rust can emit back to Elixir after input matching.
///
/// These are semantic events derived from listener actions, not raw backend
/// input packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementEventKind {
    Click,
    Press,
    MouseDown,
    MouseUp,
    MouseEnter,
    MouseLeave,
    MouseMove,
    Focus,
    Blur,
    Change,
}

/// Unified text-input state used by both rebuild output and runtime editing.
///
/// `TextInputState` combines:
///
/// - live editing state (`content`, `cursor`, `selection_anchor`, `preedit`)
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
    pub content_len: u32,
    pub cursor: u32,
    pub selection_anchor: Option<u32>,
    pub preedit: Option<String>,
    pub preedit_cursor: Option<(u32, u32)>,
    pub focused: bool,
    pub emit_change: bool,
    pub frame_x: f32,
    pub frame_width: f32,
    pub inset_left: f32,
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
        self.emit_change = other.emit_change;
        self.frame_x = other.frame_x;
        self.frame_width = other.frame_width;
        self.inset_left = other.inset_left;
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
        if !focused {
            next_anchor = None;
        } else if next_anchor == Some(next_cursor) {
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
        let local_x = self
            .screen_to_local
            .map(|transform| transform.map_point(Point { x, y }).x)
            .unwrap_or(x);
        self.cursor_from_local_x(local_x)
    }

    fn cursor_from_local_x(&self, x: f32) -> u32 {
        let text_width = self.measure_text_width(&self.content);
        let content_width = (self.frame_width - self.inset_left - self.inset_right).max(0.0);

        let text_start_x = match self.text_align {
            TextAlign::Left => self.frame_x + self.inset_left,
            TextAlign::Center => {
                self.frame_x + self.inset_left + (content_width - text_width) / 2.0
            }
            TextAlign::Right => self.frame_x + self.frame_width - self.inset_right - text_width,
        };

        let click_x = (x - text_start_x).clamp(0.0, text_width.max(0.0));
        self.nearest_char_index_for_offset(&self.content, click_x)
    }

    fn measure_text_width(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }

        let font = make_font_with_style(
            &self.font_family,
            self.font_weight,
            self.font_italic,
            self.font_size,
        );

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

        total
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEditRequest {
    MoveLeft { extend_selection: bool },
    MoveRight { extend_selection: bool },
    MoveHome { extend_selection: bool },
    MoveEnd { extend_selection: bool },
    Backspace,
    Delete,
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
pub struct RegistryRebuildPayload {
    pub base_registry: registry_builder::Registry,
    pub text_inputs: HashMap<ElementId, TextInputState>,
    pub scrollbars: HashMap<(ElementId, ScrollbarAxis), ScrollbarNode>,
    pub focused_id: Option<ElementId>,
}

impl Default for RegistryRebuildPayload {
    fn default() -> Self {
        Self {
            base_registry: registry_builder::Registry::default(),
            text_inputs: HashMap::new(),
            scrollbars: HashMap::new(),
            focused_id: None,
        }
    }
}

fn text_input_state(element: &Element, adjusted_rect: Rect, screen_to_local: Option<Affine2>) -> TextInputState {
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
    let (inset_left, inset_right) = text_content_insets(&element.attrs);
    let (font_family, font_weight, font_italic) = font_info_from_attrs(&element.attrs);

    TextInputState {
        content,
        content_len,
        cursor,
        selection_anchor,
        preedit: element.attrs.text_input_preedit.clone(),
        preedit_cursor: element.attrs.text_input_preedit_cursor,
        focused: element.attrs.text_input_focused.unwrap_or(false),
        emit_change: element.attrs.on_change.unwrap_or(false),
        frame_x: adjusted_rect.x,
        frame_width: adjusted_rect.width,
        inset_left,
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

fn text_content_insets(attrs: &crate::tree::attrs::Attrs) -> (f32, f32) {
    let (pad_left, pad_right) = match attrs.padding.as_ref() {
        Some(Padding::Uniform(v)) => (*v as f32, *v as f32),
        Some(Padding::Sides { left, right, .. }) => (*left as f32, *right as f32),
        None => (0.0, 0.0),
    };

    let (border_left, border_right) = match attrs.border_width.as_ref() {
        Some(BorderWidth::Uniform(v)) => (*v as f32, *v as f32),
        Some(BorderWidth::Sides { left, right, .. }) => (*left as f32, *right as f32),
        None => (0.0, 0.0),
    };

    (pad_left + border_left, pad_right + border_right)
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

rustler::atoms! {
    emerge_skia_event,
    click,
    press,
    change,
    focus,
    blur,
    mouse_down,
    mouse_up,
    mouse_enter,
    mouse_leave,
    mouse_move,
}

pub(crate) fn click_atom() -> Atom {
    click()
}

pub(crate) fn press_atom() -> Atom {
    press()
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

#[cfg(test)]
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
            content_len: 2,
            cursor: 0,
            selection_anchor: None,
            preedit: None,
            preedit_cursor: None,
            focused: true,
            emit_change: false,
            frame_x: 0.0,
            frame_width: 100.0,
            inset_left: 0.0,
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
}
