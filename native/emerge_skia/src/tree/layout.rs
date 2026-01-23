//! Layout engine for Emerge element trees.
//!
//! Two-pass algorithm:
//! 1. Measurement (bottom-up): Compute intrinsic sizes
//! 2. Resolution (top-down): Assign frames with constraints

use super::attrs::{AlignX, AlignY, Length, Padding};
use super::element::{Element, ElementId, ElementKind, ElementTree, Frame};

// =============================================================================
// Layout Types
// =============================================================================

/// Constraint passed down during layout resolution.
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub max_width: f32,
    pub max_height: f32,
}

impl Constraint {
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self { max_width, max_height }
    }

    /// Create a constraint with infinite dimensions.
    pub fn unbounded() -> Self {
        Self {
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }
}

/// Intrinsic (natural) size computed during measurement pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct IntrinsicSize {
    pub width: f32,
    pub height: f32,
}

/// Measured element with intrinsic size attached.
#[derive(Clone, Debug)]
struct MeasuredElement {
    id: ElementId,
    intrinsic: IntrinsicSize,
}

// =============================================================================
// Text Measurement
// =============================================================================

/// Trait for measuring text dimensions.
pub trait TextMeasurer {
    /// Measure text and return (width, height).
    fn measure(&self, text: &str, font_size: f32) -> (f32, f32);
}

/// Default text measurer using Skia.
pub struct SkiaTextMeasurer;

impl TextMeasurer for SkiaTextMeasurer {
    fn measure(&self, text: &str, font_size: f32) -> (f32, f32) {
        use crate::renderer::get_default_typeface;
        use skia_safe::Font;

        let typeface = get_default_typeface();
        let font = Font::new(typeface, font_size);
        let (width, _bounds) = font.measure_str(text, None);
        let (_, metrics) = font.metrics();
        let height = metrics.ascent.abs() + metrics.descent;

        (width, height)
    }
}

// =============================================================================
// Layout Engine
// =============================================================================

/// Main layout function: measure and resolve the tree.
pub fn layout_tree<M: TextMeasurer>(
    tree: &mut ElementTree,
    constraint: Constraint,
    measurer: &M,
) {
    let Some(root_id) = tree.root.clone() else {
        return;
    };

    // Pass 1: Measure (bottom-up)
    measure_element(tree, &root_id, measurer);

    // Pass 2: Resolve (top-down)
    resolve_element(tree, &root_id, constraint, 0.0, 0.0);
}

/// Layout with default Skia text measurer.
pub fn layout_tree_default(tree: &mut ElementTree, constraint: Constraint) {
    layout_tree(tree, constraint, &SkiaTextMeasurer);
}

// =============================================================================
// Pass 1: Measurement (Bottom-Up)
// =============================================================================

/// Measure an element and its children, computing intrinsic sizes.
fn measure_element<M: TextMeasurer>(
    tree: &mut ElementTree,
    id: &ElementId,
    measurer: &M,
) -> IntrinsicSize {
    // First measure all children
    let child_ids: Vec<ElementId> = tree
        .get(id)
        .map(|e| e.children.clone())
        .unwrap_or_default();

    let child_sizes: Vec<IntrinsicSize> = child_ids
        .iter()
        .map(|child_id| measure_element(tree, child_id, measurer))
        .collect();

    // Now measure this element
    let Some(element) = tree.get(id) else {
        return IntrinsicSize::default();
    };

    let attrs = &element.attrs;
    let padding = get_padding(attrs.padding.as_ref());
    let spacing = attrs.spacing.unwrap_or(0.0) as f32;

    let intrinsic = match element.kind {
        ElementKind::Text => {
            let content = attrs.content.as_deref().unwrap_or("");
            let font_size = attrs.font_size.unwrap_or(16.0) as f32;
            let (text_width, text_height) = measurer.measure(content, font_size);
            IntrinsicSize {
                width: text_width + padding.left + padding.right,
                height: text_height + padding.top + padding.bottom,
            }
        }

        ElementKind::El | ElementKind::None => {
            // Single child container: intrinsic = max child size + padding
            let max_child_width = child_sizes.iter().map(|s| s.width).fold(0.0, f32::max);
            let max_child_height = child_sizes.iter().map(|s| s.height).fold(0.0, f32::max);

            IntrinsicSize {
                width: resolve_intrinsic_length(attrs.width.as_ref(), max_child_width)
                    + padding.left + padding.right,
                height: resolve_intrinsic_length(attrs.height.as_ref(), max_child_height)
                    + padding.top + padding.bottom,
            }
        }

        ElementKind::Row | ElementKind::WrappedRow => {
            // Row: sum widths + spacing + padding
            let total_spacing = if child_sizes.len() > 1 {
                spacing * (child_sizes.len() - 1) as f32
            } else {
                0.0
            };
            let sum_width: f32 = child_sizes.iter().map(|s| s.width).sum();
            let max_height = child_sizes.iter().map(|s| s.height).fold(0.0, f32::max);

            IntrinsicSize {
                width: resolve_intrinsic_length(attrs.width.as_ref(), sum_width + total_spacing)
                    + padding.left + padding.right,
                height: resolve_intrinsic_length(attrs.height.as_ref(), max_height)
                    + padding.top + padding.bottom,
            }
        }

        ElementKind::Column => {
            // Column: sum heights + spacing + padding
            let total_spacing = if child_sizes.len() > 1 {
                spacing * (child_sizes.len() - 1) as f32
            } else {
                0.0
            };
            let max_width = child_sizes.iter().map(|s| s.width).fold(0.0, f32::max);
            let sum_height: f32 = child_sizes.iter().map(|s| s.height).sum();

            IntrinsicSize {
                width: resolve_intrinsic_length(attrs.width.as_ref(), max_width)
                    + padding.left + padding.right,
                height: resolve_intrinsic_length(attrs.height.as_ref(), sum_height + total_spacing)
                    + padding.top + padding.bottom,
            }
        }
    };

    // Store intrinsic size in frame temporarily (will be replaced in resolve pass)
    if let Some(element) = tree.get_mut(id) {
        element.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: intrinsic.width,
            height: intrinsic.height,
        });
    }

    intrinsic
}

/// Resolve intrinsic length from attribute.
fn resolve_intrinsic_length(length: Option<&Length>, intrinsic: f32) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic,
        Some(Length::Fill) | Some(Length::FillPortion(_)) => intrinsic, // Will expand in resolve
    }
}

// =============================================================================
// Pass 2: Resolution (Top-Down)
// =============================================================================

/// Resolve an element's frame given constraints and position.
fn resolve_element(
    tree: &mut ElementTree,
    id: &ElementId,
    constraint: Constraint,
    x: f32,
    y: f32,
) {
    let Some(element) = tree.get(id) else {
        return;
    };

    let attrs = &element.attrs;
    let kind = element.kind;
    let child_ids = element.children.clone();
    let intrinsic = element.frame.map(|f| IntrinsicSize { width: f.width, height: f.height })
        .unwrap_or_default();

    let padding = get_padding(attrs.padding.as_ref());
    let spacing = attrs.spacing.unwrap_or(0.0) as f32;

    // Resolve final dimensions
    let width = resolve_length(attrs.width.as_ref(), intrinsic.width, constraint.max_width);
    let height = resolve_length(attrs.height.as_ref(), intrinsic.height, constraint.max_height);

    // Update frame
    if let Some(element) = tree.get_mut(id) {
        element.frame = Some(Frame { x, y, width, height });
    }

    // Content area for children
    let content_x = x + padding.left;
    let content_y = y + padding.top;
    let content_width = (width - padding.left - padding.right).max(0.0);
    let content_height = (height - padding.top - padding.bottom).max(0.0);

    // Resolve children based on element type
    match kind {
        ElementKind::Text | ElementKind::None => {
            // No children to layout
        }

        ElementKind::El => {
            resolve_el_children(tree, &child_ids, content_x, content_y, content_width, content_height);
        }

        ElementKind::Row => {
            resolve_row_children(tree, &child_ids, content_x, content_y, content_width, content_height, spacing);
        }

        ElementKind::WrappedRow => {
            resolve_wrapped_row_children(tree, &child_ids, content_x, content_y, content_width, content_height, spacing);
        }

        ElementKind::Column => {
            resolve_column_children(tree, &child_ids, content_x, content_y, content_width, content_height, spacing);
        }
    }
}

/// Resolve final length from attribute, intrinsic, and constraint.
fn resolve_length(length: Option<&Length>, intrinsic: f32, constraint: f32) -> f32 {
    match length {
        Some(Length::Px(px)) => *px as f32,
        Some(Length::Content) | None => intrinsic.min(constraint),
        Some(Length::Fill) => constraint,
        Some(Length::FillPortion(_)) => constraint, // Simplified: treat as fill
    }
}

// =============================================================================
// Child Resolution by Element Type
// =============================================================================

/// Resolve children for El (single child container with alignment).
fn resolve_el_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
) {
    for child_id in child_ids {
        let (child_intrinsic, align_x, align_y) = {
            let Some(child) = tree.get(child_id) else { continue };
            let intrinsic = child.frame.map(|f| IntrinsicSize { width: f.width, height: f.height })
                .unwrap_or_default();
            let ax = child.attrs.align_x.unwrap_or_default();
            let ay = child.attrs.align_y.unwrap_or_default();
            (intrinsic, ax, ay)
        };

        let child_constraint = Constraint::new(content_width, content_height);

        // Resolve child first to get final size
        resolve_element(tree, child_id, child_constraint, 0.0, 0.0);

        // Get resolved size and apply alignment
        let Some(child) = tree.get_mut(child_id) else { continue };
        let Some(frame) = &mut child.frame else { continue };

        let child_x = match align_x {
            AlignX::Left => content_x,
            AlignX::Center => content_x + (content_width - frame.width) / 2.0,
            AlignX::Right => content_x + content_width - frame.width,
        };

        let child_y = match align_y {
            AlignY::Top => content_y,
            AlignY::Center => content_y + (content_height - frame.height) / 2.0,
            AlignY::Bottom => content_y + content_height - frame.height,
        };

        frame.x = child_x;
        frame.y = child_y;
    }
}

/// Resolve children for Row with fill distribution.
fn resolve_row_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
    spacing: f32,
) {
    if child_ids.is_empty() {
        return;
    }

    // Categorize children as fill or fixed
    let mut fill_count = 0;
    let mut fixed_width = 0.0;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.width).unwrap_or(0.0);

        match child.attrs.width.as_ref() {
            Some(Length::Fill) | Some(Length::FillPortion(_)) => {
                fill_count += 1;
            }
            _ => {
                fixed_width += resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic);
            }
        }
    }

    // Calculate fill width per child
    let total_spacing = spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_width - fixed_width - total_spacing).max(0.0);
    let fill_width = if fill_count > 0 { remaining / fill_count as f32 } else { 0.0 };

    // Position children
    let mut current_x = content_x;

    for child_id in child_ids {
        let (child_width, align_y) = {
            let Some(child) = tree.get(child_id) else { continue };
            let intrinsic = child.frame.map(|f| f.width).unwrap_or(0.0);
            let w = match child.attrs.width.as_ref() {
                Some(Length::Fill) | Some(Length::FillPortion(_)) => fill_width,
                _ => resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic),
            };
            (w, child.attrs.align_y.unwrap_or_default())
        };

        let child_constraint = Constraint::new(child_width, content_height);
        resolve_element(tree, child_id, child_constraint, current_x, content_y);

        // Apply vertical alignment
        if let Some(child) = tree.get_mut(child_id) {
            if let Some(frame) = &mut child.frame {
                frame.y = match align_y {
                    AlignY::Top => content_y,
                    AlignY::Center => content_y + (content_height - frame.height) / 2.0,
                    AlignY::Bottom => content_y + content_height - frame.height,
                };
            }
        }

        current_x += child_width + spacing;
    }
}

/// Resolve children for Column with fill distribution.
fn resolve_column_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
    spacing: f32,
) {
    if child_ids.is_empty() {
        return;
    }

    // Categorize children as fill or fixed
    let mut fill_count = 0;
    let mut fixed_height = 0.0;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.height).unwrap_or(0.0);

        match child.attrs.height.as_ref() {
            Some(Length::Fill) | Some(Length::FillPortion(_)) => {
                fill_count += 1;
            }
            _ => {
                fixed_height += resolve_intrinsic_length(child.attrs.height.as_ref(), intrinsic);
            }
        }
    }

    // Calculate fill height per child
    let total_spacing = spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_height - fixed_height - total_spacing).max(0.0);
    let fill_height = if fill_count > 0 { remaining / fill_count as f32 } else { 0.0 };

    // Position children
    let mut current_y = content_y;

    for child_id in child_ids {
        let (child_height, align_x) = {
            let Some(child) = tree.get(child_id) else { continue };
            let intrinsic = child.frame.map(|f| f.height).unwrap_or(0.0);
            let h = match child.attrs.height.as_ref() {
                Some(Length::Fill) | Some(Length::FillPortion(_)) => fill_height,
                _ => resolve_intrinsic_length(child.attrs.height.as_ref(), intrinsic),
            };
            (h, child.attrs.align_x.unwrap_or_default())
        };

        let child_constraint = Constraint::new(content_width, child_height);
        resolve_element(tree, child_id, child_constraint, content_x, current_y);

        // Apply horizontal alignment
        if let Some(child) = tree.get_mut(child_id) {
            if let Some(frame) = &mut child.frame {
                frame.x = match align_x {
                    AlignX::Left => content_x,
                    AlignX::Center => content_x + (content_width - frame.width) / 2.0,
                    AlignX::Right => content_x + content_width - frame.width,
                };
            }
        }

        current_y += child_height + spacing;
    }
}

/// Resolve children for WrappedRow.
fn resolve_wrapped_row_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
    spacing: f32,
) {
    if child_ids.is_empty() {
        return;
    }

    // Build lines by wrapping
    let mut lines: Vec<Vec<(ElementId, f32, f32)>> = Vec::new(); // (id, width, height)
    let mut current_line: Vec<(ElementId, f32, f32)> = Vec::new();
    let mut current_line_width = 0.0;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic_width = child.frame.map(|f| f.width).unwrap_or(0.0);
        let intrinsic_height = child.frame.map(|f| f.height).unwrap_or(0.0);

        let child_width = resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic_width);

        // Check if we need to wrap
        let would_exceed = !current_line.is_empty()
            && current_line_width + spacing + child_width > content_width;

        if would_exceed {
            lines.push(std::mem::take(&mut current_line));
            current_line_width = 0.0;
        }

        if !current_line.is_empty() {
            current_line_width += spacing;
        }
        current_line_width += child_width;
        current_line.push((child_id.clone(), child_width, intrinsic_height));
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    // Layout each line
    let mut current_y = content_y;

    for line in lines {
        let line_height = line.iter().map(|(_, _, h)| *h).fold(0.0_f32, f32::max);
        let mut current_x = content_x;

        for (child_id, child_width, _) in &line {
            let child_constraint = Constraint::new(*child_width, line_height);
            resolve_element(tree, child_id, child_constraint, current_x, current_y);
            current_x += child_width + spacing;
        }

        current_y += line_height + spacing;
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Resolved padding values.
struct ResolvedPadding {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

/// Get padding as resolved values.
fn get_padding(padding: Option<&Padding>) -> ResolvedPadding {
    match padding {
        Some(Padding::Uniform(p)) => {
            let p = *p as f32;
            ResolvedPadding { top: p, right: p, bottom: p, left: p }
        }
        Some(Padding::Sides { top, right, bottom, left }) => {
            ResolvedPadding {
                top: *top as f32,
                right: *right as f32,
                bottom: *bottom as f32,
                left: *left as f32,
            }
        }
        None => ResolvedPadding { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;

    struct MockTextMeasurer;
    impl TextMeasurer for MockTextMeasurer {
        fn measure(&self, text: &str, font_size: f32) -> (f32, f32) {
            // Simple mock: 8px per char, height = font_size
            (text.len() as f32 * 8.0, font_size)
        }
    }

    fn make_element(id: &str, kind: ElementKind, attrs: Attrs) -> Element {
        Element::with_attrs(
            ElementId::from_term_bytes(id.as_bytes().to_vec()),
            kind,
            vec![],
            attrs,
        )
    }

    #[test]
    fn test_layout_single_el() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Px(100.0));
        attrs.height = Some(Length::Px(50.0));

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        assert_eq!(frame.x, 0.0);
        assert_eq!(frame.y, 0.0);
        assert_eq!(frame.width, 100.0);
        assert_eq!(frame.height, 50.0);
    }

    #[test]
    fn test_layout_text() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.content = Some("Hello".to_string());
        attrs.font_size = Some(16.0);

        let el = make_element("text", ElementKind::Text, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        assert_eq!(frame.width, 40.0); // 5 chars * 8px
        assert_eq!(frame.height, 16.0); // font_size
    }

    #[test]
    fn test_layout_row() {
        let mut tree = ElementTree::new();

        // Create row with two children
        let mut row_attrs = Attrs::default();
        row_attrs.spacing = Some(10.0);

        let mut row = make_element("row", ElementKind::Row, row_attrs);
        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone()];
        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.x, 0.0);
        assert_eq!(c2_frame.x, 60.0); // 50 + 10 spacing
    }

    #[test]
    fn test_layout_column_fill() {
        let mut tree = ElementTree::new();

        let mut col_attrs = Attrs::default();
        col_attrs.height = Some(Length::Px(100.0));

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Fill);
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Fill);
            a
        });

        let col_id = col.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();

        col.children = vec![c1_id.clone(), c2_id.clone()];
        tree.root = Some(col_id.clone());
        tree.insert(col);
        tree.insert(child1);
        tree.insert(child2);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        // Both children should split the 100px height equally
        assert_eq!(c1_frame.height, 50.0);
        assert_eq!(c2_frame.height, 50.0);
        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 50.0);
    }
}
