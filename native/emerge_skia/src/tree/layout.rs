//! Layout engine for Emerge element trees.
//!
//! Three-pass algorithm:
//! 0. Scale: Apply scale factor to all attributes
//! 1. Measurement (bottom-up): Compute intrinsic sizes
//! 2. Resolution (top-down): Assign frames with constraints

use super::attrs::{AlignX, AlignY, Attrs, Length, Padding};
use super::element::{ElementId, ElementKind, ElementTree, Frame};

// =============================================================================
// Layout Types
// =============================================================================

/// Available space for layout, following elm-ui semantics.
/// More expressive than a simple f32 constraint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AvailableSpace {
    /// Definite constraint - a fixed amount of available space (px).
    Definite(f32),
    /// Minimize to content - use minimum size needed to fit content.
    /// Equivalent to elm-ui's `shrink` or `content` when space is tight.
    MinContent,
    /// Maximize to content - expand to fit all content without constraint.
    /// Equivalent to elm-ui's `content` when space is plentiful.
    MaxContent,
}

impl AvailableSpace {
    /// Convert to a definite f32 value, using the provided default for content modes.
    pub fn resolve(&self, default: f32) -> f32 {
        match self {
            AvailableSpace::Definite(px) => *px,
            AvailableSpace::MinContent => default,
            AvailableSpace::MaxContent => default,
        }
    }

    /// Check if this is a definite constraint.
    pub fn is_definite(&self) -> bool {
        matches!(self, AvailableSpace::Definite(_))
    }
}

impl From<f32> for AvailableSpace {
    fn from(value: f32) -> Self {
        AvailableSpace::Definite(value)
    }
}

/// Constraint passed down during layout resolution.
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub width: AvailableSpace,
    pub height: AvailableSpace,
}

impl Constraint {
    /// Create a constraint with definite values (most common case).
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            width: AvailableSpace::Definite(max_width),
            height: AvailableSpace::Definite(max_height),
        }
    }

    /// Create a constraint with custom available space.
    pub fn with_space(width: AvailableSpace, height: AvailableSpace) -> Self {
        Self { width, height }
    }

    /// Get max_width, resolving content modes to the provided default.
    pub fn max_width(&self, default: f32) -> f32 {
        self.width.resolve(default)
    }

    /// Get max_height, resolving content modes to the provided default.
    pub fn max_height(&self, default: f32) -> f32 {
        self.height.resolve(default)
    }
}

/// Intrinsic (natural) size computed during measurement pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct IntrinsicSize {
    pub width: f32,
    pub height: f32,
}

// MeasuredElement reserved for future layout caching.

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

/// Main layout function: scale, measure, and resolve the tree.
pub fn layout_tree<M: TextMeasurer>(
    tree: &mut ElementTree,
    constraint: Constraint,
    scale: f32,
    measurer: &M,
) {
    let Some(root_id) = tree.root.clone() else {
        return;
    };

    // Pass 0: Scale all attributes (base_attrs -> attrs with scale applied)
    apply_scale_to_tree(tree, scale);

    // Pass 1: Measure (bottom-up) - uses pre-scaled attrs
    measure_element(tree, &root_id, measurer);

    // Pass 2: Resolve (top-down) - uses pre-scaled attrs
    resolve_element(tree, &root_id, constraint, 0.0, 0.0);
}

/// Layout with default Skia text measurer.
pub fn layout_tree_default(tree: &mut ElementTree, constraint: Constraint, scale: f32) {
    layout_tree(tree, constraint, scale, &SkiaTextMeasurer);
}

// =============================================================================
// Pass 0: Scale Attributes
// =============================================================================

/// Apply scale factor to all elements, copying base_attrs to attrs with scaling.
fn apply_scale_to_tree(tree: &mut ElementTree, scale: f32) {
    for element in tree.nodes.values_mut() {
        element.attrs = scale_attrs(&element.base_attrs, scale);
    }
}

/// Scale all pixel-based attributes in an Attrs struct.
fn scale_attrs(attrs: &Attrs, scale: f32) -> Attrs {
    let scale_f64 = scale as f64;
    Attrs {
        width: attrs.width.as_ref().map(|l| scale_length(l, scale)),
        height: attrs.height.as_ref().map(|l| scale_length(l, scale)),
        padding: attrs.padding.as_ref().map(|p| scale_padding(p, scale)),
        spacing: attrs.spacing.map(|s| s * scale_f64),
        align_x: attrs.align_x,
        align_y: attrs.align_y,
        scrollbar_y: attrs.scrollbar_y,
        scrollbar_x: attrs.scrollbar_x,
        clip: attrs.clip,
        clip_y: attrs.clip_y,
        clip_x: attrs.clip_x,
        background: attrs.background.clone(),
        border_radius: attrs.border_radius.map(|r| r * scale_f64),
        border_width: attrs.border_width.map(|w| w * scale_f64),
        border_color: attrs.border_color.clone(),
        font_size: attrs.font_size.map(|s| s * scale_f64),
        font_color: attrs.font_color.clone(),
        font: attrs.font.clone(),
        font_weight: attrs.font_weight.clone(),
        font_style: attrs.font_style.clone(),
        content: attrs.content.clone(),
        above: attrs.above.clone(),
        below: attrs.below.clone(),
        on_left: attrs.on_left.clone(),
        on_right: attrs.on_right.clone(),
        in_front: attrs.in_front.clone(),
        behind: attrs.behind.clone(),
        snap_layout: attrs.snap_layout,
        snap_text_metrics: attrs.snap_text_metrics,
    }
}

/// Scale pixel values within a Length, recursively handling Minimum/Maximum.
fn scale_length(length: &Length, scale: f32) -> Length {
    let scale_f64 = scale as f64;
    match length {
        Length::Px(val) => Length::Px(*val * scale_f64),
        Length::Minimum(min_px, inner) => {
            Length::Minimum(*min_px * scale_f64, Box::new(scale_length(inner, scale)))
        }
        Length::Maximum(max_px, inner) => {
            Length::Maximum(*max_px * scale_f64, Box::new(scale_length(inner, scale)))
        }
        Length::Fill => Length::Fill,
        Length::Content => Length::Content,
        Length::FillPortion(p) => Length::FillPortion(*p),
    }
}

/// Scale padding values.
fn scale_padding(padding: &Padding, scale: f32) -> Padding {
    let scale_f64 = scale as f64;
    match padding {
        Padding::Uniform(val) => Padding::Uniform(*val * scale_f64),
        Padding::Sides { top, right, bottom, left } => Padding::Sides {
            top: *top * scale_f64,
            right: *right * scale_f64,
            bottom: *bottom * scale_f64,
            left: *left * scale_f64,
        },
    }
}

// =============================================================================
// Pass 1: Measurement (Bottom-Up)
// =============================================================================

/// Measure an element and its children, computing intrinsic sizes.
/// Reads from pre-scaled attrs.
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

    // Read from pre-scaled attrs
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
            content_width: intrinsic.width,
            content_height: intrinsic.height,
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
        Some(Length::Minimum(min_px, inner)) => {
            let inner_size = resolve_intrinsic_length(Some(inner), intrinsic);
            inner_size.max(*min_px as f32)
        }
        Some(Length::Maximum(max_px, inner)) => {
            let inner_size = resolve_intrinsic_length(Some(inner), intrinsic);
            inner_size.min(*max_px as f32)
        }
    }
}

// =============================================================================
// Pass 2: Resolution (Top-Down)
// =============================================================================

/// Resolve an element's frame given constraints and position.
/// Reads from pre-scaled attrs.
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

    // Read from pre-scaled attrs
    let attrs = &element.attrs;
    let kind = element.kind;
    let child_ids = element.children.clone();
    let intrinsic = element.frame.map(|f| IntrinsicSize { width: f.width, height: f.height })
        .unwrap_or_default();

    let padding = get_padding(attrs.padding.as_ref());
    let spacing = attrs.spacing.unwrap_or(0.0) as f32;

    // Check if this element is scrollable (clips content)
    let is_scrollable = attrs.clip.unwrap_or(false)
        || attrs.clip_x.unwrap_or(false)
        || attrs.clip_y.unwrap_or(false)
        || attrs.scrollbar_x.unwrap_or(false)
        || attrs.scrollbar_y.unwrap_or(false);

    // Resolve final dimensions
    // Use intrinsic size as default for content-based constraints
    let max_width = constraint.max_width(intrinsic.width);
    let max_height = constraint.max_height(intrinsic.height);
    let width = resolve_length(attrs.width.as_ref(), intrinsic.width, max_width);
    let height = resolve_length(attrs.height.as_ref(), intrinsic.height, max_height);

    // Update frame (content size will be updated after children are resolved)
    if let Some(element) = tree.get_mut(id) {
        element.frame = Some(Frame {
            x,
            y,
            width,
            height,
            content_width: width,
            content_height: height,
        });
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
            if !child_ids.is_empty() {
                let (actual_cw, actual_ch) = resolve_el_children(tree, &child_ids, content_x, content_y, content_width, content_height);
                // Update content dimensions when there are children
                if let Some(element) = tree.get_mut(id)
                    && let Some(ref mut frame) = element.frame
                {
                    frame.content_width = actual_cw + padding.left + padding.right;
                    frame.content_height = actual_ch + padding.top + padding.bottom;
                }
            }
            // For El without children, content_width/content_height stay equal to width/height
        }

        ElementKind::Row => {
            if !child_ids.is_empty() {
                let (actual_cw, actual_ch) = resolve_row_children(tree, &child_ids, content_x, content_y, content_width, content_height, spacing);
                // Update content dimensions when there are children
                if let Some(element) = tree.get_mut(id)
                    && let Some(ref mut frame) = element.frame
                {
                    frame.content_width = actual_cw + padding.left + padding.right;
                    frame.content_height = actual_ch + padding.top + padding.bottom;
                }
            }
            // For Row without children, content_width/content_height stay equal to width/height
        }

        ElementKind::WrappedRow => {
            let actual_content_height = resolve_wrapped_row_children(tree, &child_ids, content_x, content_y, content_width, content_height, spacing);
            // Update frame height if content height exceeds initial estimate (due to wrapping)
            // For non-scrollable wrapped rows, expand the frame
            if actual_content_height > content_height && !is_scrollable {
                let new_height = actual_content_height + padding.top + padding.bottom;
                if let Some(element) = tree.get_mut(id)
                    && let Some(ref mut frame) = element.frame
                {
                    frame.height = new_height;
                    frame.content_height = new_height;
                }
            } else if let Some(element) = tree.get_mut(id)
                && let Some(ref mut frame) = element.frame
            {
                // Always track actual content height
                frame.content_height = actual_content_height + padding.top + padding.bottom;
            }
        }

        ElementKind::Column => {
            let actual_content_height = resolve_column_children(tree, &child_ids, content_x, content_y, content_width, content_height, spacing);
            // Update frame height if content height exceeds initial estimate (e.g., due to wrapped_row children)
            // For non-scrollable columns, expand the frame
            if actual_content_height > content_height && !is_scrollable {
                let new_height = actual_content_height + padding.top + padding.bottom;
                if let Some(element) = tree.get_mut(id)
                    && let Some(ref mut frame) = element.frame
                {
                    frame.height = new_height;
                    frame.content_height = new_height;
                }
            } else if let Some(element) = tree.get_mut(id)
                && let Some(ref mut frame) = element.frame
            {
                // Always track actual content height
                frame.content_height = actual_content_height + padding.top + padding.bottom;
            }
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
        Some(Length::Minimum(min_px, inner)) => {
            let inner_size = resolve_length(Some(inner), intrinsic, constraint);
            inner_size.max(*min_px as f32)
        }
        Some(Length::Maximum(max_px, inner)) => {
            let inner_size = resolve_length(Some(inner), intrinsic, constraint);
            inner_size.min(*max_px as f32)
        }
    }
}

/// Get the portion value for a fill-based length.
/// Returns 1.0 for Fill, the portion value for FillPortion, or 0.0 for non-fill.
fn get_fill_portion(length: Option<&Length>) -> f32 {
    match length {
        Some(Length::Fill) => 1.0,
        Some(Length::FillPortion(portion)) => *portion as f32,
        Some(Length::Minimum(_, inner)) | Some(Length::Maximum(_, inner)) => {
            get_fill_portion(Some(inner))
        }
        _ => 0.0,
    }
}

// =============================================================================
// Child Resolution by Element Type
// =============================================================================

/// Resolve children for El (single child container with alignment).
/// Reads from pre-scaled attrs.
/// Returns (actual_content_width, actual_content_height).
fn resolve_el_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
) -> (f32, f32) {
    let mut max_child_width = 0.0_f32;
    let mut max_child_height = 0.0_f32;

    for child_id in child_ids {
        let (align_x, align_y) = {
            let Some(child) = tree.get(child_id) else { continue };
            let ax = child.attrs.align_x.unwrap_or_default();
            let ay = child.attrs.align_y.unwrap_or_default();
            (ax, ay)
        };

        let child_constraint = Constraint::new(content_width, content_height);

        // Resolve child first to get final size
        resolve_element(tree, child_id, child_constraint, 0.0, 0.0);

        let Some(child) = tree.get(child_id) else { continue };
        let Some(frame) = &child.frame else { continue };

        // Track max child dimensions for content size
        max_child_width = max_child_width.max(frame.content_width);
        max_child_height = max_child_height.max(frame.content_height);

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

        let dx = child_x - frame.x;
        let dy = child_y - frame.y;
        shift_subtree(tree, child_id, dx, dy);
    }

    (max_child_width, max_child_height)
}

/// Resolve children for Row with fill distribution.
/// Reads from pre-scaled attrs.
/// Returns (actual_content_width, actual_content_height).
#[allow(clippy::too_many_arguments)]
fn resolve_row_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
    spacing: f32,
) -> (f32, f32) {
    if child_ids.is_empty() {
        return (0.0, 0.0);
    }

    // Categorize children: track total fill portions and fixed width
    let mut total_portions = 0.0;
    let mut fixed_width = 0.0;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.width).unwrap_or(0.0);

        let portion = get_fill_portion(child.attrs.width.as_ref());
        if portion > 0.0 {
            total_portions += portion;
        } else {
            fixed_width += resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic);
        }
    }

    // Calculate width per portion unit
    let total_spacing = spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_width - fixed_width - total_spacing).max(0.0);
    let width_per_portion = if total_portions > 0.0 { remaining / total_portions } else { 0.0 };

    // Position children
    let mut current_x = content_x;
    let mut max_child_height = 0.0_f32;

    for child_id in child_ids {
        let (child_width, align_y) = {
            let Some(child) = tree.get(child_id) else { continue };
            let intrinsic = child.frame.map(|f| f.width).unwrap_or(0.0);
            let portion = get_fill_portion(child.attrs.width.as_ref());
            let base_width = if portion > 0.0 {
                width_per_portion * portion
            } else {
                resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic)
            };
            // Apply min/max constraints on top of base width
            let w = resolve_length(child.attrs.width.as_ref(), intrinsic, base_width);
            (w, child.attrs.align_y.unwrap_or_default())
        };

        let child_constraint = Constraint::new(child_width, content_height);
        resolve_element(tree, child_id, child_constraint, current_x, content_y);

        // Apply vertical alignment and track max height
        if let Some(child) = tree.get(child_id)
            && let Some(frame) = &child.frame
        {
            max_child_height = max_child_height.max(frame.content_height);

            let aligned_y = match align_y {
                AlignY::Top => content_y,
                AlignY::Center => content_y + (content_height - frame.height) / 2.0,
                AlignY::Bottom => content_y + content_height - frame.height,
            };
            let dy = aligned_y - frame.y;
            shift_subtree(tree, child_id, 0.0, dy);
        }

        current_x += child_width + spacing;
    }

    // Total content width (subtract trailing spacing)
    let total_width = current_x - content_x;
    let actual_width = if !child_ids.is_empty() {
        total_width - spacing
    } else {
        0.0
    };

    (actual_width, max_child_height)
}

/// Resolve children for Column with fill distribution.
/// Reads from pre-scaled attrs.
/// Returns the actual content height after resolution.
#[allow(clippy::too_many_arguments)]
fn resolve_column_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
    spacing: f32,
) -> f32 {
    if child_ids.is_empty() {
        return 0.0;
    }

    // Categorize children: track total fill portions and fixed height
    let mut total_portions = 0.0;
    let mut fixed_height = 0.0;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.height).unwrap_or(0.0);

        let portion = get_fill_portion(child.attrs.height.as_ref());
        if portion > 0.0 {
            total_portions += portion;
        } else {
            fixed_height += resolve_intrinsic_length(child.attrs.height.as_ref(), intrinsic);
        }
    }

    // Calculate height per portion unit
    let total_spacing = spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_height - fixed_height - total_spacing).max(0.0);
    let height_per_portion = if total_portions > 0.0 { remaining / total_portions } else { 0.0 };

    // Position children
    let mut current_y = content_y;

    for child_id in child_ids {
        let (child_height, align_x) = {
            let Some(child) = tree.get(child_id) else { continue };
            let intrinsic = child.frame.map(|f| f.height).unwrap_or(0.0);
            let portion = get_fill_portion(child.attrs.height.as_ref());
            let base_height = if portion > 0.0 {
                height_per_portion * portion
            } else {
                resolve_intrinsic_length(child.attrs.height.as_ref(), intrinsic)
            };
            // Apply min/max constraints on top of base height
            let h = resolve_length(child.attrs.height.as_ref(), intrinsic, base_height);
            (h, child.attrs.align_x.unwrap_or_default())
        };

        let child_constraint = Constraint::new(content_width, child_height);
        resolve_element(tree, child_id, child_constraint, content_x, current_y);

        // Get frame info for alignment and actual height (may differ from child_height for wrapped_row)
        let (dx, actual_height) = {
            let Some(child) = tree.get(child_id) else {
                current_y += child_height + spacing;
                continue;
            };
            let Some(frame) = &child.frame else {
                current_y += child_height + spacing;
                continue;
            };
            let aligned_x = match align_x {
                AlignX::Left => content_x,
                AlignX::Center => content_x + (content_width - frame.width) / 2.0,
                AlignX::Right => content_x + content_width - frame.width,
            };
            (aligned_x - frame.x, frame.height)
        };

        // Apply horizontal alignment
        shift_subtree(tree, child_id, dx, 0.0);

        // Use actual height after resolution (important for wrapped_row)
        current_y += actual_height + spacing;
    }

    // Return total content height (subtract trailing spacing)
    let total_height = current_y - content_y;
    if !child_ids.is_empty() {
        total_height - spacing // Remove trailing spacing
    } else {
        0.0
    }
}

/// Resolve children for WrappedRow.
/// Reads from pre-scaled attrs.
/// Returns the actual content height after wrapping.
fn resolve_wrapped_row_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    _content_height: f32,
    spacing: f32,
) -> f32 {
    if child_ids.is_empty() {
        return 0.0;
    }

    // Build lines by wrapping (attrs are pre-scaled)
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

    // Layout each line and track total height
    let mut current_y = content_y;
    let num_lines = lines.len();

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

    // Return total content height (subtract trailing spacing)
    let total_height = current_y - content_y;
    if num_lines > 0 {
        total_height - spacing // Remove trailing spacing
    } else {
        0.0
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

fn shift_subtree(tree: &mut ElementTree, id: &ElementId, dx: f32, dy: f32) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }

    let child_ids = {
        let Some(element) = tree.get_mut(id) else { return };
        if let Some(frame) = &mut element.frame {
            frame.x += dx;
            frame.y += dy;
        }
        element.children.clone()
    };

    for child_id in child_ids {
        shift_subtree(tree, &child_id, dx, dy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::attrs::Attrs;
    use crate::tree::element::Element;

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

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

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

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

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

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

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

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        // Both children should split the 100px height equally
        assert_eq!(c1_frame.height, 50.0);
        assert_eq!(c2_frame.height, 50.0);
        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 50.0);
    }

    #[test]
    fn test_layout_minimum_constraint() {
        let mut tree = ElementTree::new();

        // Element with width = minimum(200, fill())
        // When constraint is 800px, fill() = 800px, but minimum clamps to at least 200px
        // Result should be 800px (fill wins since 800 > 200)
        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Minimum(200.0, Box::new(Length::Fill)));
        attrs.height = Some(Length::Px(50.0));

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        assert_eq!(frame.width, 800.0); // fill() = 800, 800 >= 200, so 800
    }

    #[test]
    fn test_layout_minimum_constraint_enforced() {
        let mut tree = ElementTree::new();

        // Element with width = minimum(200, content)
        // When content is small, minimum should enforce 200px
        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Minimum(200.0, Box::new(Length::Content)));
        attrs.height = Some(Length::Px(50.0));

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        assert_eq!(frame.width, 200.0); // content = 0, minimum enforces 200
    }

    #[test]
    fn test_layout_maximum_constraint() {
        let mut tree = ElementTree::new();

        // Element with width = maximum(300, fill())
        // When constraint is 800px, fill() = 800px, but maximum clamps to 300px
        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Maximum(300.0, Box::new(Length::Fill)));
        attrs.height = Some(Length::Px(50.0));

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        assert_eq!(frame.width, 300.0); // fill() = 800, clamped to max 300
    }

    #[test]
    fn test_layout_row_with_max_width_child() {
        let mut tree = ElementTree::new();

        // Row with two children: one fill, one max(100, fill)
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Fill); // Row needs explicit fill to expand
        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Fill);
            a.height = Some(Length::Px(30.0));
            a
        });

        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Maximum(100.0, Box::new(Length::Fill)));
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

        layout_tree(&mut tree, Constraint::new(400.0, 600.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        // Both children are fill, so they split 400px = 200px each
        // But c2 has max(100), so it gets clamped to 100px
        assert_eq!(c1_frame.width, 200.0);
        assert_eq!(c2_frame.width, 100.0);
    }

    #[test]
    fn test_layout_with_scale() {
        let mut tree = ElementTree::new();

        // Element with width=100px, height=50px, padding=10px, font_size=16
        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Px(100.0));
        attrs.height = Some(Length::Px(50.0));
        attrs.padding = Some(Padding::Uniform(10.0));
        attrs.font_size = Some(16.0);

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        // With scale=2.0, frame pixel values should double
        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 2.0, &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        // width: 100 * 2 = 200
        // height: 50 * 2 = 100
        assert_eq!(frame.width, 200.0);
        assert_eq!(frame.height, 100.0);

        // base_attrs should remain unchanged (original unscaled values)
        assert_eq!(root.base_attrs.padding, Some(Padding::Uniform(10.0)));
        assert_eq!(root.base_attrs.font_size, Some(16.0));

        // attrs should be scaled (for render to read)
        assert_eq!(root.attrs.padding, Some(Padding::Uniform(20.0)));
        assert_eq!(root.attrs.font_size, Some(32.0));
    }

    #[test]
    fn test_layout_scale_minimum_maximum() {
        let mut tree = ElementTree::new();

        // Element with width=minimum(100, fill), height=maximum(200, fill)
        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Minimum(100.0, Box::new(Length::Fill)));
        attrs.height = Some(Length::Maximum(200.0, Box::new(Length::Fill)));

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        // With scale=2.0:
        // width: minimum(200, fill) -> fill=800, clamped to min 200 -> 800
        // height: maximum(400, fill) -> fill=600, clamped to max 400 -> 400
        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 2.0, &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();
        assert_eq!(frame.width, 800.0); // fill = 800, min 200 doesn't apply
        assert_eq!(frame.height, 400.0); // fill = 600, clamped to max 400
    }

    #[test]
    fn test_wrapped_row_height_with_wrapping() {
        let mut tree = ElementTree::new();

        // Create a wrapped row with 3 children, each 50px wide
        // Container is 100px wide, so items should wrap:
        // Line 1: child1, child2 (50 + 10 spacing + 50 = 110 > 100, so child2 wraps)
        // Actually with 100px width: child1 (50) fits, child2 (50+10=60) would make 110, wraps
        // Line 1: child1 (50px)
        // Line 2: child2 (50px)
        // Line 3: child3 (50px)
        // Total height = 3 * 30 + 2 * 10 spacing = 110px

        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(100.0));
        row_attrs.spacing = Some(10.0);

        let mut row = make_element("row", ElementKind::WrappedRow, row_attrs);

        // Children 50px wide, 30px tall each
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
        let child3 = make_element("c3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();
        let c3_id = child3.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);
        tree.insert(child3);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        // Check wrapped row height
        let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
        // With 100px width, children wrap: each on its own line
        // 3 lines * 30px height + 2 * 10px spacing = 110px
        assert_eq!(row_frame.height, 110.0);

        // Check child positions
        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
        let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

        // All children should be at x=0 (each on its own line)
        assert_eq!(c1_frame.x, 0.0);
        assert_eq!(c2_frame.x, 0.0);
        assert_eq!(c3_frame.x, 0.0);

        // Y positions: 0, 40 (30+10), 80 (30+10+30+10)
        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 40.0);
        assert_eq!(c3_frame.y, 80.0);
    }

    #[test]
    fn test_wrapped_row_two_items_per_line() {
        let mut tree = ElementTree::new();

        // Container 120px wide with 10px spacing
        // Children 50px wide each
        // Two children fit per line: 50 + 10 + 50 = 110 < 120
        // With 4 children: 2 lines
        // Total height = 2 * 30 + 1 * 10 spacing = 70px

        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(120.0));
        row_attrs.spacing = Some(10.0);

        let mut row = make_element("row", ElementKind::WrappedRow, row_attrs);

        let children: Vec<_> = (0..4).map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(50.0));
                a.height = Some(Length::Px(30.0));
                a
            })
        }).collect();

        let child_ids: Vec<_> = children.iter().map(|c| c.id.clone()).collect();
        let row_id = row.id.clone();
        row.children = child_ids.clone();

        tree.root = Some(row_id.clone());
        tree.insert(row);
        for child in children {
            tree.insert(child);
        }

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        // Check wrapped row height: 2 lines * 30px + 1 * 10px spacing = 70px
        let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
        assert_eq!(row_frame.height, 70.0);

        // Check child positions
        // Line 1: c0 at x=0, c1 at x=60
        // Line 2: c2 at x=0, c3 at x=60
        let c0_frame = tree.get(&child_ids[0]).unwrap().frame.unwrap();
        let c1_frame = tree.get(&child_ids[1]).unwrap().frame.unwrap();
        let c2_frame = tree.get(&child_ids[2]).unwrap().frame.unwrap();
        let c3_frame = tree.get(&child_ids[3]).unwrap().frame.unwrap();

        assert_eq!(c0_frame.x, 0.0);
        assert_eq!(c0_frame.y, 0.0);
        assert_eq!(c1_frame.x, 60.0);
        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.x, 0.0);
        assert_eq!(c2_frame.y, 40.0);
        assert_eq!(c3_frame.x, 60.0);
        assert_eq!(c3_frame.y, 40.0);
    }

    #[test]
    fn test_column_with_wrapped_row_pushes_siblings() {
        let mut tree = ElementTree::new();

        // Column containing:
        // 1. A wrapped_row (100px wide, 3 children 50px each -> wraps to 3 lines = 110px tall)
        // 2. An element (40px tall)
        //
        // The element should be pushed down by the wrapped_row's actual height (110px),
        // not its initial intrinsic height (30px).

        let mut col_attrs = Attrs::default();
        col_attrs.width = Some(Length::Px(100.0));
        col_attrs.spacing = Some(10.0);

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        // Wrapped row with 100px width constraint from parent
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Fill);
        row_attrs.spacing = Some(10.0);

        let mut wrapped_row = make_element("wrapped_row", ElementKind::WrappedRow, row_attrs);

        // Three children that will each wrap to their own line
        let chip1 = make_element("chip1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let chip2 = make_element("chip2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let chip3 = make_element("chip3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        // Element below the wrapped row
        let below_el = make_element("below", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Fill);
            a.height = Some(Length::Px(40.0));
            a
        });

        let col_id = col.id.clone();
        let row_id = wrapped_row.id.clone();
        let chip1_id = chip1.id.clone();
        let chip2_id = chip2.id.clone();
        let chip3_id = chip3.id.clone();
        let below_id = below_el.id.clone();

        wrapped_row.children = vec![chip1_id.clone(), chip2_id.clone(), chip3_id.clone()];
        col.children = vec![row_id.clone(), below_id.clone()];

        tree.root = Some(col_id.clone());
        tree.insert(col);
        tree.insert(wrapped_row);
        tree.insert(chip1);
        tree.insert(chip2);
        tree.insert(chip3);
        tree.insert(below_el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        // Check wrapped_row height (3 lines * 30px + 2 * 10px spacing = 110px)
        let row_frame = tree.get(&row_id).unwrap().frame.unwrap();
        assert_eq!(row_frame.height, 110.0);
        assert_eq!(row_frame.y, 0.0);

        // Check that the element below is positioned after the wrapped_row
        // y = wrapped_row.height (110) + spacing (10) = 120
        let below_frame = tree.get(&below_id).unwrap().frame.unwrap();
        assert_eq!(below_frame.y, 120.0);
        assert_eq!(below_frame.height, 40.0);

        // Column should encompass both children
        let col_frame = tree.get(&col_id).unwrap().frame.unwrap();
        // Total: 110 (wrapped_row) + 10 (spacing) + 40 (below) = 160
        assert_eq!(col_frame.height, 160.0);
    }

    #[test]
    fn test_row_fill_portion_distribution() {
        let mut tree = ElementTree::new();

        // Row with 300px width, containing:
        // - child1: fillPortion(1) -> 1/6 of 300 = 50px
        // - child2: fillPortion(2) -> 2/6 of 300 = 100px
        // - child3: fillPortion(3) -> 3/6 of 300 = 150px
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(300.0));

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::FillPortion(1.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::FillPortion(2.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let child3 = make_element("c3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::FillPortion(3.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();
        let c3_id = child3.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);
        tree.insert(child3);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
        let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

        // Total portions = 1 + 2 + 3 = 6
        // c1: 300 * 1/6 = 50
        // c2: 300 * 2/6 = 100
        // c3: 300 * 3/6 = 150
        assert_eq!(c1_frame.width, 50.0);
        assert_eq!(c2_frame.width, 100.0);
        assert_eq!(c3_frame.width, 150.0);

        // Check positions
        assert_eq!(c1_frame.x, 0.0);
        assert_eq!(c2_frame.x, 50.0);
        assert_eq!(c3_frame.x, 150.0);
    }

    #[test]
    fn test_row_fill_portion_with_fixed() {
        let mut tree = ElementTree::new();

        // Row with 400px width, containing:
        // - child1: 100px fixed
        // - child2: fillPortion(1) -> 1/3 of remaining 300 = 100px
        // - child3: fillPortion(2) -> 2/3 of remaining 300 = 200px
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(400.0));

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(100.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::FillPortion(1.0));
            a.height = Some(Length::Px(30.0));
            a
        });
        let child3 = make_element("c3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::FillPortion(2.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();
        let c3_id = child3.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);
        tree.insert(child3);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
        let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

        // Remaining = 400 - 100 = 300
        // c1: 100px fixed
        // c2: 300 * 1/3 = 100
        // c3: 300 * 2/3 = 200
        assert_eq!(c1_frame.width, 100.0);
        assert_eq!(c2_frame.width, 100.0);
        assert_eq!(c3_frame.width, 200.0);
    }

    #[test]
    fn test_column_fill_portion_distribution() {
        let mut tree = ElementTree::new();

        // Column with 300px height, containing:
        // - child1: fillPortion(1) -> 1/6 of 300 = 50px
        // - child2: fillPortion(2) -> 2/6 of 300 = 100px
        // - child3: fillPortion(3) -> 3/6 of 300 = 150px
        let mut col_attrs = Attrs::default();
        col_attrs.height = Some(Length::Px(300.0));

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::FillPortion(1.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::FillPortion(2.0));
            a
        });
        let child3 = make_element("c3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::FillPortion(3.0));
            a
        });

        let col_id = col.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();
        let c3_id = child3.id.clone();

        col.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
        tree.root = Some(col_id.clone());
        tree.insert(col);
        tree.insert(child1);
        tree.insert(child2);
        tree.insert(child3);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
        let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

        // Total portions = 1 + 2 + 3 = 6
        // c1: 300 * 1/6 = 50
        // c2: 300 * 2/6 = 100
        // c3: 300 * 3/6 = 150
        assert_eq!(c1_frame.height, 50.0);
        assert_eq!(c2_frame.height, 100.0);
        assert_eq!(c3_frame.height, 150.0);

        // Check positions
        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 50.0);
        assert_eq!(c3_frame.y, 150.0);
    }

    #[test]
    fn test_fill_and_fill_portion_mixed() {
        let mut tree = ElementTree::new();

        // Row with 400px, containing:
        // - child1: fill (= fillPortion(1))
        // - child2: fillPortion(3)
        // Total portions = 1 + 3 = 4
        // c1: 400 * 1/4 = 100
        // c2: 400 * 3/4 = 300
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(400.0));

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Fill);  // Equivalent to FillPortion(1)
            a.height = Some(Length::Px(30.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::FillPortion(3.0));
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

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.width, 100.0);
        assert_eq!(c2_frame.width, 300.0);
    }

    #[test]
    fn test_content_size_basic_element() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.width = Some(Length::Px(100.0));
        attrs.height = Some(Length::Px(50.0));

        let el = make_element("root", ElementKind::El, attrs);
        let root_id = el.id.clone();
        tree.root = Some(root_id.clone());
        tree.insert(el);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let root = tree.get(&root_id).unwrap();
        let frame = root.frame.unwrap();

        // For a basic element without children, content size equals frame size
        assert_eq!(frame.content_width, 100.0);
        assert_eq!(frame.content_height, 50.0);
    }

    #[test]
    fn test_content_size_row_with_children() {
        let mut tree = ElementTree::new();

        // Row with 300px width, 3 children of 80px each + 10px spacing
        // Children: 80 + 10 + 80 + 10 + 80 = 260px total content width
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(300.0));
        row_attrs.height = Some(Length::Px(50.0));
        row_attrs.spacing = Some(10.0);

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let children: Vec<_> = (0..3).map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(80.0));
                a.height = Some(Length::Px(30.0));
                a
            })
        }).collect();

        let child_ids: Vec<_> = children.iter().map(|c| c.id.clone()).collect();
        let row_id = row.id.clone();
        row.children = child_ids;

        tree.root = Some(row_id.clone());
        tree.insert(row);
        for child in children {
            tree.insert(child);
        }

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let row_frame = tree.get(&row_id).unwrap().frame.unwrap();

        // Frame size is the specified size
        assert_eq!(row_frame.width, 300.0);
        assert_eq!(row_frame.height, 50.0);

        // Content size reflects actual children layout
        // 80 + 10 + 80 + 10 + 80 = 260px content width
        // Max child height = 30px content height
        assert_eq!(row_frame.content_width, 260.0);
        assert_eq!(row_frame.content_height, 30.0);
    }

    #[test]
    fn test_content_size_column_with_children() {
        let mut tree = ElementTree::new();

        // Column with 3 children of 30px each + 10px spacing
        // Children: 30 + 10 + 30 + 10 + 30 = 110px total content height
        let mut col_attrs = Attrs::default();
        col_attrs.width = Some(Length::Px(100.0));
        col_attrs.height = Some(Length::Px(200.0));
        col_attrs.spacing = Some(10.0);

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let children: Vec<_> = (0..3).map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(80.0));
                a.height = Some(Length::Px(30.0));
                a
            })
        }).collect();

        let child_ids: Vec<_> = children.iter().map(|c| c.id.clone()).collect();
        let col_id = col.id.clone();
        col.children = child_ids;

        tree.root = Some(col_id.clone());
        tree.insert(col);
        for child in children {
            tree.insert(child);
        }

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let col_frame = tree.get(&col_id).unwrap().frame.unwrap();

        // Frame size is the specified size
        assert_eq!(col_frame.width, 100.0);
        assert_eq!(col_frame.height, 200.0);

        // Content height: 30 + 10 + 30 + 10 + 30 = 110px
        assert_eq!(col_frame.content_height, 110.0);
    }

    #[test]
    fn test_content_size_scrollable_column() {
        let mut tree = ElementTree::new();

        // Scrollable column with content that would overflow
        // 5 children of 50px each + 10px spacing = 250 + 40 = 290px
        // But frame is constrained to 150px height
        let mut col_attrs = Attrs::default();
        col_attrs.width = Some(Length::Px(100.0));
        col_attrs.height = Some(Length::Px(150.0));
        col_attrs.spacing = Some(10.0);
        col_attrs.clip_y = Some(true); // Makes it scrollable

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let children: Vec<_> = (0..5).map(|i| {
            make_element(&format!("c{}", i), ElementKind::El, {
                let mut a = Attrs::default();
                a.width = Some(Length::Px(80.0));
                a.height = Some(Length::Px(50.0));
                a
            })
        }).collect();

        let child_ids: Vec<_> = children.iter().map(|c| c.id.clone()).collect();
        let col_id = col.id.clone();
        col.children = child_ids;

        tree.root = Some(col_id.clone());
        tree.insert(col);
        for child in children {
            tree.insert(child);
        }

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let col_frame = tree.get(&col_id).unwrap().frame.unwrap();

        // Frame stays at specified size (clipped/scrollable)
        assert_eq!(col_frame.width, 100.0);
        assert_eq!(col_frame.height, 150.0);

        // Content height reflects actual content: 5 * 50 + 4 * 10 = 290px
        assert_eq!(col_frame.content_height, 290.0);
    }

    #[test]
    fn test_content_size_el_with_child() {
        let mut tree = ElementTree::new();

        // El container with a child smaller than the container
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(150.0));

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(60.0));
            a
        });

        let child_id = child.id.clone();
        let el_id = el.id.clone();
        el.children = vec![child_id];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let el_frame = tree.get(&el_id).unwrap().frame.unwrap();

        // Frame is the specified size
        assert_eq!(el_frame.width, 200.0);
        assert_eq!(el_frame.height, 150.0);

        // Content size reflects the child's dimensions
        assert_eq!(el_frame.content_width, 80.0);
        assert_eq!(el_frame.content_height, 60.0);
    }

    #[test]
    fn test_available_space_resolve() {
        // Definite resolves to its value
        let definite = AvailableSpace::Definite(100.0);
        assert_eq!(definite.resolve(50.0), 100.0);

        // MinContent resolves to default
        let min_content = AvailableSpace::MinContent;
        assert_eq!(min_content.resolve(50.0), 50.0);

        // MaxContent resolves to default
        let max_content = AvailableSpace::MaxContent;
        assert_eq!(max_content.resolve(50.0), 50.0);
    }

    #[test]
    fn test_available_space_is_definite() {
        assert!(AvailableSpace::Definite(100.0).is_definite());
        assert!(!AvailableSpace::MinContent.is_definite());
        assert!(!AvailableSpace::MaxContent.is_definite());
    }

    #[test]
    fn test_constraint_max_methods() {
        let constraint = Constraint::new(800.0, 600.0);
        assert_eq!(constraint.max_width(100.0), 800.0);
        assert_eq!(constraint.max_height(100.0), 600.0);

        // With content-based constraints
        let content_constraint = Constraint::with_space(
            AvailableSpace::MaxContent,
            AvailableSpace::MinContent,
        );
        // Should resolve to the default values
        assert_eq!(content_constraint.max_width(150.0), 150.0);
        assert_eq!(content_constraint.max_height(200.0), 200.0);
    }

    #[test]
    fn test_available_space_from_f32() {
        let space: AvailableSpace = 100.0.into();
        assert_eq!(space, AvailableSpace::Definite(100.0));
    }
}
