//! Element types for Emerge UI trees.

#[cfg(test)]
use super::attrs::MouseOverAttrs;
use super::attrs::{Attrs, ScrollbarHoverAxis};
use super::interaction::ElementInteraction;
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

    /// Computed interaction geometry (populated by interaction pass).
    pub interaction: Option<ElementInteraction>,
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
            interaction: None,
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

    /// Set mouse_down active state. Returns true when state changes.
    pub fn set_mouse_down_active(&mut self, id: &ElementId, active: bool) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.attrs.mouse_down.is_none() {
            if element.attrs.mouse_down_active.take().is_some() {
                return true;
            }
            return false;
        }

        let current = element.attrs.mouse_down_active.unwrap_or(false);
        if current == active {
            return false;
        }

        element.attrs.mouse_down_active = Some(active);
        true
    }

    /// Set focused active state. Returns true when state changes.
    pub fn set_focused_active(&mut self, id: &ElementId, active: bool) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        let current = element.attrs.focused_active.unwrap_or(false);
        if current == active {
            return false;
        }

        element.attrs.focused_active = Some(active);
        true
    }

    pub fn set_text_input_content(&mut self, id: &ElementId, content: String) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        let prev_base = element.base_attrs.content.as_deref().unwrap_or("");
        let prev_attrs = element.attrs.content.as_deref().unwrap_or("");
        let mut changed = prev_base != content || prev_attrs != content;

        element.base_attrs.content = Some(content.clone());
        element.attrs.content = Some(content.clone());

        let len = text_char_len(&content);
        if let Some(cursor) = element.attrs.text_input_cursor {
            let clamped = cursor.min(len);
            if clamped != cursor {
                element.attrs.text_input_cursor = Some(clamped);
                changed = true;
            }
        }

        if let Some(anchor) = element.attrs.text_input_selection_anchor {
            let clamped = anchor.min(len);
            let cursor = element.attrs.text_input_cursor.unwrap_or(len);
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

        let had_preedit = element.attrs.text_input_preedit.take().is_some();
        let had_preedit_cursor = element.attrs.text_input_preedit_cursor.take().is_some();
        if had_preedit || had_preedit_cursor {
            changed = true;
        }

        changed
    }

    pub fn set_text_input_runtime(
        &mut self,
        id: &ElementId,
        focused: bool,
        cursor: Option<u32>,
        selection_anchor: Option<u32>,
        preedit: Option<String>,
        preedit_cursor: Option<(u32, u32)>,
    ) -> bool {
        let Some(element) = self.get_mut(id) else {
            return false;
        };

        if element.kind != ElementKind::TextInput {
            return false;
        }

        let content = element.base_attrs.content.as_deref().unwrap_or("");
        let len = text_char_len(content);

        let mut next_cursor = cursor.or(element.attrs.text_input_cursor);
        if focused {
            next_cursor = Some(next_cursor.unwrap_or(len).min(len));
        } else {
            next_cursor = next_cursor.map(|value| value.min(len));
        }

        let cursor_value = next_cursor.unwrap_or(len);
        let mut next_anchor = selection_anchor.map(|value| value.min(len));
        if !focused {
            next_anchor = None;
        } else if let Some(anchor) = next_anchor
            && anchor == cursor_value
        {
            next_anchor = None;
        }

        let next_preedit = if focused {
            preedit.filter(|value| !value.is_empty())
        } else {
            None
        };
        let next_preedit_cursor = if focused {
            normalize_preedit_cursor(next_preedit.as_deref(), preedit_cursor)
        } else {
            None
        };

        let mut changed = false;

        if element.attrs.text_input_focused != Some(focused) {
            element.attrs.text_input_focused = Some(focused);
            changed = true;
        }

        if element.attrs.text_input_cursor != next_cursor {
            element.attrs.text_input_cursor = next_cursor;
            changed = true;
        }

        if element.attrs.text_input_selection_anchor != next_anchor {
            element.attrs.text_input_selection_anchor = next_anchor;
            changed = true;
        }

        if element.attrs.text_input_preedit != next_preedit {
            element.attrs.text_input_preedit = next_preedit;
            changed = true;
        }

        if element.attrs.text_input_preedit_cursor != next_preedit_cursor {
            element.attrs.text_input_preedit_cursor = next_preedit_cursor;
            changed = true;
        }

        changed
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
    fn test_set_mouse_down_active_requires_mouse_down_attrs() {
        let id = ElementId::from_term_bytes(vec![11]);
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

        assert!(!tree.set_mouse_down_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_down_active, None);
    }

    #[test]
    fn test_set_mouse_down_active_toggles_state() {
        let id = ElementId::from_term_bytes(vec![12]);
        let mut attrs = Attrs::default();
        attrs.mouse_down = Some(MouseOverAttrs {
            alpha: Some(0.7),
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

        assert!(tree.set_mouse_down_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_down_active, Some(true));

        assert!(!tree.set_mouse_down_active(&id, true));

        assert!(tree.set_mouse_down_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.mouse_down_active, Some(false));
    }

    #[test]
    fn test_set_focused_active_toggles_state() {
        let id = ElementId::from_term_bytes(vec![13]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            content_width: 100.0,
            content_height: 40.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(tree.set_focused_active(&id, true));
        assert_eq!(tree.get(&id).unwrap().attrs.focused_active, Some(true));

        assert!(!tree.set_focused_active(&id, true));

        assert!(tree.set_focused_active(&id, false));
        assert_eq!(tree.get(&id).unwrap().attrs.focused_active, Some(false));
    }

    #[test]
    fn test_set_text_input_content_updates_and_clamps_runtime() {
        let id = ElementId::from_term_bytes(vec![2]);
        let mut attrs = Attrs::default();
        attrs.content = Some("hello".to_string());
        attrs.text_input_cursor = Some(10);
        attrs.text_input_selection_anchor = Some(10);
        attrs.text_input_preedit = Some("pre".to_string());
        attrs.text_input_preedit_cursor = Some((2, 2));
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

        assert!(tree.set_text_input_content(&id, "hey".to_string()));
        let node = tree.get(&id).unwrap();
        assert_eq!(node.base_attrs.content.as_deref(), Some("hey"));
        assert_eq!(node.attrs.content.as_deref(), Some("hey"));
        assert_eq!(node.attrs.text_input_cursor, Some(3));
        assert_eq!(node.attrs.text_input_selection_anchor, None);
        assert_eq!(node.attrs.text_input_preedit, None);
        assert_eq!(node.attrs.text_input_preedit_cursor, None);

        assert!(!tree.set_text_input_content(&id, "hey".to_string()));
    }

    #[test]
    fn test_set_text_input_runtime_normalizes_focus_selection_and_preedit() {
        let id = ElementId::from_term_bytes(vec![3]);
        let mut attrs = Attrs::default();
        attrs.content = Some("abcd".to_string());
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

        assert!(tree.set_text_input_runtime(
            &id,
            true,
            Some(99),
            Some(1),
            Some("ka".to_string()),
            Some((7, 2)),
        ));

        let focused = tree.get(&id).unwrap();
        assert_eq!(focused.attrs.text_input_focused, Some(true));
        assert_eq!(focused.attrs.text_input_cursor, Some(4));
        assert_eq!(focused.attrs.text_input_selection_anchor, Some(1));
        assert_eq!(focused.attrs.text_input_preedit.as_deref(), Some("ka"));
        assert_eq!(focused.attrs.text_input_preedit_cursor, Some((2, 2)));

        assert!(tree.set_text_input_runtime(
            &id,
            false,
            Some(2),
            Some(0),
            Some("ignored".to_string()),
            Some((1, 1)),
        ));

        let blurred = tree.get(&id).unwrap();
        assert_eq!(blurred.attrs.text_input_focused, Some(false));
        assert_eq!(blurred.attrs.text_input_cursor, Some(2));
        assert_eq!(blurred.attrs.text_input_selection_anchor, None);
        assert_eq!(blurred.attrs.text_input_preedit, None);
        assert_eq!(blurred.attrs.text_input_preedit_cursor, None);
    }

    #[test]
    fn test_set_text_input_runtime_ignores_non_text_input_nodes() {
        let id = ElementId::from_term_bytes(vec![4]);
        let mut element =
            Element::with_attrs(id.clone(), ElementKind::El, Vec::new(), Attrs::default());
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 20.0,
            content_width: 50.0,
            content_height: 20.0,
        });

        let mut tree = ElementTree::new();
        tree.root = Some(id.clone());
        tree.insert(element);

        assert!(!tree.set_text_input_content(&id, "nope".to_string()));
        assert!(!tree.set_text_input_runtime(
            &id,
            true,
            Some(0),
            None,
            Some("x".to_string()),
            None,
        ));
    }
}
