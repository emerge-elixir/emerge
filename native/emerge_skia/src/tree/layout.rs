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
        let previous = element.attrs.clone();
        element.attrs = scale_attrs(&element.base_attrs, scale);
        preserve_runtime_attrs(&previous, &mut element.attrs);
    }
}

fn preserve_runtime_attrs(existing: &Attrs, incoming: &mut Attrs) {
    if incoming.scroll_x.is_none() {
        incoming.scroll_x = existing.scroll_x;
    }
    if incoming.scroll_y.is_none() {
        incoming.scroll_y = existing.scroll_y;
    }
    if incoming.scroll_x_max.is_none() {
        incoming.scroll_x_max = existing.scroll_x_max;
    }
    if incoming.scroll_y_max.is_none() {
        incoming.scroll_y_max = existing.scroll_y_max;
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
        spacing_x: attrs.spacing_x.map(|s| s * scale_f64),
        spacing_y: attrs.spacing_y.map(|s| s * scale_f64),
        align_x: attrs.align_x,
        align_y: attrs.align_y,
        scrollbar_y: attrs.scrollbar_y,
        scrollbar_x: attrs.scrollbar_x,
        scroll_x: attrs.scroll_x.map(|v| v * scale_f64),
        scroll_y: attrs.scroll_y.map(|v| v * scale_f64),
        scroll_x_max: attrs.scroll_x_max.map(|v| v * scale_f64),
        scroll_y_max: attrs.scroll_y_max.map(|v| v * scale_f64),
        on_click: attrs.on_click,
        clip: attrs.clip,
        clip_y: attrs.clip_y,
        clip_x: attrs.clip_x,
        background: attrs.background.clone(),
        border_radius: attrs.border_radius.as_ref().map(|r| scale_border_radius(r, scale_f64)),
        border_width: attrs.border_width.map(|w| w * scale_f64),
        border_color: attrs.border_color.clone(),
        font_size: attrs.font_size.map(|s| s * scale_f64),
        font_color: attrs.font_color.clone(),
        font: attrs.font.clone(),
        font_weight: attrs.font_weight.clone(),
        font_style: attrs.font_style.clone(),
        text_align: attrs.text_align,
        content: attrs.content.clone(),
        above: attrs.above.clone(),
        below: attrs.below.clone(),
        on_left: attrs.on_left.clone(),
        on_right: attrs.on_right.clone(),
        in_front: attrs.in_front.clone(),
        behind: attrs.behind.clone(),
        snap_layout: attrs.snap_layout,
        snap_text_metrics: attrs.snap_text_metrics,
        move_x: attrs.move_x.map(|v| v * scale_f64),
        move_y: attrs.move_y.map(|v| v * scale_f64),
        rotate: attrs.rotate,
        scale: attrs.scale,
        alpha: attrs.alpha,
        space_evenly: attrs.space_evenly,
    }
}

fn scale_border_radius(radius: &super::attrs::BorderRadius, scale: f64) -> super::attrs::BorderRadius {
    use super::attrs::BorderRadius;

    match radius {
        BorderRadius::Uniform(value) => BorderRadius::Uniform(*value * scale),
        BorderRadius::Corners { tl, tr, br, bl } => BorderRadius::Corners {
            tl: *tl * scale,
            tr: *tr * scale,
            br: *br * scale,
            bl: *bl * scale,
        },
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
    let spacing_x = spacing_x(attrs);
    let spacing_y = spacing_y(attrs);

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
                spacing_x * (child_sizes.len() - 1) as f32
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
                spacing_y * (child_sizes.len() - 1) as f32
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
    let attrs = element.attrs.clone();
    let kind = element.kind;
    let child_ids = element.children.clone();
    let intrinsic = element.frame.map(|f| IntrinsicSize { width: f.width, height: f.height })
        .unwrap_or_default();

    let padding = get_padding(attrs.padding.as_ref());
    let spacing_x = spacing_x(&attrs);
    let spacing_y = spacing_y(&attrs);
    let align_x = attrs.align_x.unwrap_or_default();
    let align_y = attrs.align_y.unwrap_or_default();

    // Check if this element is scrollable (scrollbars only)
    let is_scrollable = attrs.scrollbar_x.unwrap_or(false) || attrs.scrollbar_y.unwrap_or(false);

    // Resolve final dimensions
    // Use intrinsic size as default for content-based constraints
    let available_width = if is_content_length(attrs.width.as_ref()) {
        match attrs.width.as_ref() {
            Some(Length::Minimum(_, inner)) if is_content_length(Some(inner)) => {
                AvailableSpace::MinContent
            }
            _ => AvailableSpace::MaxContent,
        }
    } else {
        constraint.width
    };
    let available_height = if is_content_length(attrs.height.as_ref()) {
        match attrs.height.as_ref() {
            Some(Length::Minimum(_, inner)) if is_content_length(Some(inner)) => {
                AvailableSpace::MinContent
            }
            _ => AvailableSpace::MaxContent,
        }
    } else {
        constraint.height
    };

    let effective_constraint = Constraint::with_space(available_width, available_height);
    let max_width = effective_constraint.max_width(intrinsic.width);
    let max_height = effective_constraint.max_height(intrinsic.height);
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
                let (actual_cw, actual_ch) = resolve_el_children(
                    tree, &child_ids, content_x, content_y,
                    content_width, content_height,
                    align_x, align_y,  // Pass parent alignment for child positioning
                );
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
                let allow_fill_width = available_width.is_definite();
                let space_evenly = attrs.space_evenly.unwrap_or(false) && allow_fill_width;
                let (actual_cw, actual_ch) = resolve_row_children(
                    tree,
                    &child_ids,
                    content_x,
                    content_y,
                    content_width,
                    content_height,
                    spacing_x,
                    allow_fill_width,
                    space_evenly,
                );
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
            let actual_content_height = resolve_wrapped_row_children(
                tree,
                &child_ids,
                content_x,
                content_y,
                content_width,
                content_height,
                spacing_x,
                spacing_y,
            );
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
            let allow_fill_height = available_height.is_definite();
            let space_evenly = attrs.space_evenly.unwrap_or(false) && allow_fill_height;
            let actual_content_height = resolve_column_children(
                tree,
                &child_ids,
                content_x,
                content_y,
                content_width,
                content_height,
                spacing_y,
                allow_fill_height,
                space_evenly,
            );
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

    if is_scrollable {
        if let Some(element) = tree.get_mut(id)
            && let Some(ref mut frame) = element.frame
        {
            let max_x = (frame.content_width - frame.width).max(0.0);
            let max_y = (frame.content_height - frame.height).max(0.0);
            element.attrs.scroll_x_max = Some(max_x as f64);
            element.attrs.scroll_y_max = Some(max_y as f64);
            if element.attrs.scroll_x.is_none() {
                element.attrs.scroll_x = Some(0.0);
            }
            if element.attrs.scroll_y.is_none() {
                element.attrs.scroll_y = Some(0.0);
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

fn is_content_length(length: Option<&Length>) -> bool {
    match length {
        None | Some(Length::Content) => true,
        Some(Length::Minimum(_, inner)) | Some(Length::Maximum(_, inner)) => {
            is_content_length(Some(inner))
        }
        _ => false,
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
///
/// Alignment follows elm-ui semantics:
/// - Parent's alignment (e.g., `el([centerX()], child)`) sets default for children
/// - Child can override with its own alignment attribute
#[allow(clippy::too_many_arguments)]
fn resolve_el_children(
    tree: &mut ElementTree,
    child_ids: &[ElementId],
    content_x: f32,
    content_y: f32,
    content_width: f32,
    content_height: f32,
    parent_align_x: AlignX,
    parent_align_y: AlignY,
) -> (f32, f32) {
    let mut max_child_width = 0.0_f32;
    let mut max_child_height = 0.0_f32;

    for child_id in child_ids {
        let (align_x, align_y) = {
            let Some(child) = tree.get(child_id) else { continue };
            // Child can override parent alignment, otherwise use parent's
            let ax = child.attrs.align_x.unwrap_or(parent_align_x);
            let ay = child.attrs.align_y.unwrap_or(parent_align_y);
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

/// Resolve children for Row with fill distribution and self-alignment.
/// Children with align_x position themselves within the row:
/// - Left (default): laid out left-to-right from start
/// - Right: positioned at right edge
/// - Center: centered in remaining space
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
    allow_fill_width: bool,
    space_evenly: bool,
) -> (f32, f32) {
    if child_ids.is_empty() {
        return (0.0, 0.0);
    }

    // First pass: calculate fill_portion distribution
    let mut total_portions = 0.0_f32;
    let mut fixed_width = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.width).unwrap_or(0.0);
        let portion = if allow_fill_width {
            get_fill_portion(child.attrs.width.as_ref())
        } else {
            0.0
        };
        if portion > 0.0 {
            total_portions += portion;
        } else {
            fixed_width += resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic);
        }
    }

    // Calculate width per portion
    let effective_spacing = if space_evenly { 0.0 } else { spacing };
    let total_spacing = effective_spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_width - fixed_width - total_spacing).max(0.0);
    let width_per_portion = if total_portions > 0.0 { remaining / total_portions } else { 0.0 };

    // Partition children by horizontal alignment and calculate widths
    let mut left_children: Vec<ElementId> = Vec::new();
    let mut center_children: Vec<ElementId> = Vec::new();
    let mut right_children: Vec<ElementId> = Vec::new();

    let mut child_widths: std::collections::HashMap<ElementId, f32> = std::collections::HashMap::new();
    let mut total_left_width = 0.0_f32;
    let mut total_center_width = 0.0_f32;
    let mut total_right_width = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.width).unwrap_or(0.0);
        let portion = if allow_fill_width {
            get_fill_portion(child.attrs.width.as_ref())
        } else {
            0.0
        };
        let base_width = if portion > 0.0 {
            width_per_portion * portion
        } else {
            resolve_intrinsic_length(child.attrs.width.as_ref(), intrinsic)
        };
        // Apply min/max constraints
        let width = resolve_length(child.attrs.width.as_ref(), intrinsic, base_width);
        child_widths.insert(child_id.clone(), width);

        match child.attrs.align_x.unwrap_or_default() {
            AlignX::Left => {
                left_children.push(child_id.clone());
                total_left_width += width;
            }
            AlignX::Center => {
                center_children.push(child_id.clone());
                total_center_width += width;
            }
            AlignX::Right => {
                right_children.push(child_id.clone());
                total_right_width += width;
            }
        }
    }

    if space_evenly {
        let mut max_child_height = 0.0_f32;
        let mut current_x = content_x;
        let gap_count = child_ids.len().saturating_sub(1) as f32;
        let total_child_width: f32 = child_widths.values().sum();
        let gap = if gap_count > 0.0 {
            (content_width - total_child_width).max(0.0) / gap_count
        } else {
            0.0
        };

        for child_id in child_ids {
            let child_width = *child_widths.get(child_id).unwrap_or(&0.0);
            let align_y = tree
                .get(child_id)
                .map(|c| c.attrs.align_y.unwrap_or_default())
                .unwrap_or_default();

            let child_constraint = Constraint::new(child_width, content_height);
            resolve_element(tree, child_id, child_constraint, current_x, content_y);

            if let Some(child) = tree.get(child_id)
                && let Some(frame) = &child.frame
            {
                max_child_height = max_child_height.max(frame.content_height);
                apply_vertical_alignment(tree, child_id, content_y, content_height, align_y);
            }

            current_x += child_width + gap;
        }

        let actual_content_width = if gap_count > 0.0 {
            total_child_width + gap * gap_count
        } else {
            total_child_width
        };

        return (actual_content_width, max_child_height);
    }

    // Add spacing within each group
    let left_spacing = if left_children.len() > 1 {
        spacing * (left_children.len() - 1) as f32
    } else {
        0.0
    };
    let center_spacing = if center_children.len() > 1 {
        spacing * (center_children.len() - 1) as f32
    } else {
        0.0
    };
    let right_spacing = if right_children.len() > 1 {
        spacing * (right_children.len() - 1) as f32
    } else {
        0.0
    };

    total_left_width += left_spacing;
    total_center_width += center_spacing;
    total_right_width += right_spacing;

    // Position left-aligned children from left edge
    let mut current_x = content_x;
    let mut max_child_height = 0.0_f32;

    for child_id in &left_children {
        let child_width = *child_widths.get(child_id).unwrap_or(&0.0);
        let align_y = tree.get(child_id).map(|c| c.attrs.align_y.unwrap_or_default()).unwrap_or_default();

        let child_constraint = Constraint::new(child_width, content_height);
        resolve_element(tree, child_id, child_constraint, current_x, content_y);

        if let Some(child) = tree.get(child_id)
            && let Some(frame) = &child.frame
        {
            max_child_height = max_child_height.max(frame.content_height);
            apply_vertical_alignment(tree, child_id, content_y, content_height, align_y);
        }

        current_x += child_width + spacing;
    }

    // Position right-aligned children from right edge
    let mut right_x = content_x + content_width;
    for child_id in right_children.iter().rev() {
        let child_width = *child_widths.get(child_id).unwrap_or(&0.0);
        let align_y = tree.get(child_id).map(|c| c.attrs.align_y.unwrap_or_default()).unwrap_or_default();

        right_x -= child_width;
        let child_constraint = Constraint::new(child_width, content_height);
        resolve_element(tree, child_id, child_constraint, right_x, content_y);

        if let Some(child) = tree.get(child_id)
            && let Some(frame) = &child.frame
        {
            max_child_height = max_child_height.max(frame.content_height);
            apply_vertical_alignment(tree, child_id, content_y, content_height, align_y);
        }

        right_x -= spacing;
    }

    // Position center-aligned children in the middle of remaining space
    if !center_children.is_empty() {
        let left_end = content_x + total_left_width;
        let right_start = content_x + content_width - total_right_width;
        let available_center = (right_start - left_end).max(0.0);
        let center_start = left_end + (available_center - total_center_width) / 2.0;

        let mut center_x = center_start.max(left_end);
        for child_id in &center_children {
            let child_width = *child_widths.get(child_id).unwrap_or(&0.0);
            let align_y = tree.get(child_id).map(|c| c.attrs.align_y.unwrap_or_default()).unwrap_or_default();

            let child_constraint = Constraint::new(child_width, content_height);
            resolve_element(tree, child_id, child_constraint, center_x, content_y);

            if let Some(child) = tree.get(child_id)
                && let Some(frame) = &child.frame
            {
                max_child_height = max_child_height.max(frame.content_height);
                apply_vertical_alignment(tree, child_id, content_y, content_height, align_y);
            }

            center_x += child_width + spacing;
        }
    }

    // Calculate actual content width used by all children
    let total_child_width: f32 = child_widths.values().sum();
    let total_spacing_used = if child_ids.len() > 1 {
        spacing * (child_ids.len() - 1) as f32
    } else {
        0.0
    };
    let actual_content_width = total_child_width + total_spacing_used;

    (actual_content_width, max_child_height)
}

/// Apply vertical alignment to a child element.
fn apply_vertical_alignment(
    tree: &mut ElementTree,
    child_id: &ElementId,
    content_y: f32,
    content_height: f32,
    align_y: AlignY,
) {
    if let Some(child) = tree.get(child_id)
        && let Some(frame) = &child.frame
    {
        let aligned_y = match align_y {
            AlignY::Top => content_y,
            AlignY::Center => content_y + (content_height - frame.height) / 2.0,
            AlignY::Bottom => content_y + content_height - frame.height,
        };
        let dy = aligned_y - frame.y;
        if dy != 0.0 {
            shift_subtree(tree, child_id, 0.0, dy);
        }
    }
}

/// Resolve children for Column with fill distribution and vertical self-alignment.
/// Children are partitioned by align_y into top/center/bottom zones.
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
    allow_fill_height: bool,
    space_evenly: bool,
) -> f32 {
    if child_ids.is_empty() {
        return 0.0;
    }

    // First pass: calculate fill_portion distribution
    let mut total_portions = 0.0_f32;
    let mut fixed_height = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.height).unwrap_or(0.0);
        let portion = if allow_fill_height {
            get_fill_portion(child.attrs.height.as_ref())
        } else {
            0.0
        };
        if portion > 0.0 {
            total_portions += portion;
        } else {
            fixed_height += resolve_intrinsic_length(child.attrs.height.as_ref(), intrinsic);
        }
    }

    // Calculate height per portion
    let effective_spacing = if space_evenly { 0.0 } else { spacing };
    let total_spacing = effective_spacing * (child_ids.len().saturating_sub(1)) as f32;
    let remaining = (content_height - fixed_height - total_spacing).max(0.0);
    let height_per_portion = if total_portions > 0.0 { remaining / total_portions } else { 0.0 };

    // Partition children by vertical alignment and calculate heights
    let mut top_children: Vec<ElementId> = Vec::new();
    let mut center_children: Vec<ElementId> = Vec::new();
    let mut bottom_children: Vec<ElementId> = Vec::new();

    let mut child_heights: std::collections::HashMap<ElementId, f32> = std::collections::HashMap::new();
    let mut total_center_height = 0.0_f32;

    for child_id in child_ids {
        let Some(child) = tree.get(child_id) else { continue };
        let intrinsic = child.frame.map(|f| f.height).unwrap_or(0.0);
        let portion = if allow_fill_height {
            get_fill_portion(child.attrs.height.as_ref())
        } else {
            0.0
        };
        let base_height = if portion > 0.0 {
            height_per_portion * portion
        } else {
            resolve_intrinsic_length(child.attrs.height.as_ref(), intrinsic)
        };
        // Apply min/max constraints
        let height = resolve_length(child.attrs.height.as_ref(), intrinsic, base_height);
        child_heights.insert(child_id.clone(), height);

        match child.attrs.align_y.unwrap_or_default() {
            AlignY::Top => top_children.push(child_id.clone()),
            AlignY::Center => {
                center_children.push(child_id.clone());
                total_center_height += height;
            }
            AlignY::Bottom => bottom_children.push(child_id.clone()),
        }
    }

    if space_evenly {
        let mut current_y = content_y;
        let gap_count = child_ids.len().saturating_sub(1) as f32;
        let total_child_height: f32 = child_heights.values().sum();
        let gap = if gap_count > 0.0 {
            (content_height - total_child_height).max(0.0) / gap_count
        } else {
            0.0
        };
        let mut max_child_width = 0.0_f32;
        let mut total_height = 0.0_f32;

        for child_id in child_ids {
            let child_height = *child_heights.get(child_id).unwrap_or(&0.0);
            let align_x = tree
                .get(child_id)
                .map(|c| c.attrs.align_x.unwrap_or_default())
                .unwrap_or_default();

            let child_constraint = Constraint::new(content_width, child_height);
            resolve_element(tree, child_id, child_constraint, content_x, current_y);

            let (actual_height, frame_content_width) = tree
                .get(child_id)
                .and_then(|child| child.frame.as_ref())
                .map(|frame| (frame.height, frame.content_width))
                .unwrap_or((child_height, 0.0));

            max_child_width = max_child_width.max(frame_content_width);
            apply_horizontal_alignment(tree, child_id, content_x, content_width, align_x);

            total_height += actual_height;
            current_y += actual_height + gap;
        }

        if gap_count > 0.0 {
            total_height += gap * gap_count;
        }

        return total_height.max(0.0);
    }

    // Add spacing within each group
    let top_spacing = if top_children.len() > 1 { spacing * (top_children.len() - 1) as f32 } else { 0.0 };
    let center_spacing = if center_children.len() > 1 { spacing * (center_children.len() - 1) as f32 } else { 0.0 };
    let bottom_spacing = if bottom_children.len() > 1 { spacing * (bottom_children.len() - 1) as f32 } else { 0.0 };

    total_center_height += center_spacing;

    // Position top-aligned children from top edge
    // Resolve each child and use actual height for positioning subsequent children
    let mut current_y = content_y;
    let mut max_child_width = 0.0_f32;
    let mut actual_top_height = 0.0_f32;

    for child_id in &top_children {
        let child_height = *child_heights.get(child_id).unwrap_or(&0.0);
        let align_x = tree.get(child_id).map(|c| c.attrs.align_x.unwrap_or_default()).unwrap_or_default();

        let child_constraint = Constraint::new(content_width, child_height);
        resolve_element(tree, child_id, child_constraint, content_x, current_y);

        // Get actual frame height (may differ from constraint for WrappedRow etc.)
        let (actual_height, frame_content_width) = tree.get(child_id)
            .and_then(|child| child.frame.as_ref())
            .map(|frame| (frame.height, frame.content_width))
            .unwrap_or((child_height, 0.0));

        max_child_width = max_child_width.max(frame_content_width);
        apply_horizontal_alignment(tree, child_id, content_x, content_width, align_x);

        actual_top_height += actual_height;
        current_y += actual_height + spacing;
    }
    if !top_children.is_empty() {
        actual_top_height += top_spacing;
    }

    // Position bottom-aligned children from bottom edge
    let mut bottom_y = content_y + content_height;
    let mut actual_bottom_height = 0.0_f32;
    for child_id in bottom_children.iter().rev() {
        let child_height = *child_heights.get(child_id).unwrap_or(&0.0);
        let align_x = tree.get(child_id).map(|c| c.attrs.align_x.unwrap_or_default()).unwrap_or_default();

        bottom_y -= child_height;
        let child_constraint = Constraint::new(content_width, child_height);
        resolve_element(tree, child_id, child_constraint, content_x, bottom_y);

        // Get actual frame height
        let (actual_height, frame_content_width) = tree.get(child_id)
            .and_then(|child| child.frame.as_ref())
            .map(|frame| (frame.height, frame.content_width))
            .unwrap_or((child_height, 0.0));

        max_child_width = max_child_width.max(frame_content_width);

        // Adjust position if actual height differs from constraint
        let height_diff = actual_height - child_height;
        if height_diff != 0.0 {
            bottom_y -= height_diff;
            shift_subtree(tree, child_id, 0.0, -height_diff);
        }

        apply_horizontal_alignment(tree, child_id, content_x, content_width, align_x);

        actual_bottom_height += actual_height;
        bottom_y -= spacing;
    }
    if !bottom_children.is_empty() {
        actual_bottom_height += bottom_spacing;
    }

    // Position center-aligned children in the middle of remaining space
    let mut actual_center_height = 0.0_f32;
    if !center_children.is_empty() {
        let top_end = content_y + actual_top_height;
        let bottom_start = content_y + content_height - actual_bottom_height;
        let available_center = (bottom_start - top_end).max(0.0);
        let center_start = top_end + (available_center - total_center_height) / 2.0;

        let mut center_y = center_start.max(top_end);
        for child_id in &center_children {
            let child_height = *child_heights.get(child_id).unwrap_or(&0.0);
            let align_x = tree.get(child_id).map(|c| c.attrs.align_x.unwrap_or_default()).unwrap_or_default();

            let child_constraint = Constraint::new(content_width, child_height);
            resolve_element(tree, child_id, child_constraint, content_x, center_y);

            let (actual_height, frame_content_width) = tree.get(child_id)
                .and_then(|child| child.frame.as_ref())
                .map(|frame| (frame.height, frame.content_width))
                .unwrap_or((child_height, 0.0));

            max_child_width = max_child_width.max(frame_content_width);
            apply_horizontal_alignment(tree, child_id, content_x, content_width, align_x);

            actual_center_height += actual_height;
            center_y += actual_height + spacing;
        }
        if !center_children.is_empty() {
            actual_center_height += center_spacing;
        }
    }

    // Calculate actual content height used by all children (use actual heights)
    actual_top_height + actual_center_height + actual_bottom_height
}

/// Apply horizontal alignment to a child element.
fn apply_horizontal_alignment(
    tree: &mut ElementTree,
    child_id: &ElementId,
    content_x: f32,
    content_width: f32,
    align_x: AlignX,
) {
    if let Some(child) = tree.get(child_id)
        && let Some(frame) = &child.frame
    {
        let aligned_x = match align_x {
            AlignX::Left => content_x,
            AlignX::Center => content_x + (content_width - frame.width) / 2.0,
            AlignX::Right => content_x + content_width - frame.width,
        };
        let dx = aligned_x - frame.x;
        if dx != 0.0 {
            shift_subtree(tree, child_id, dx, 0.0);
        }
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
    spacing_x: f32,
    spacing_y: f32,
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
            && current_line_width + spacing_x + child_width > content_width;

        if would_exceed {
            lines.push(std::mem::take(&mut current_line));
            current_line_width = 0.0;
        }

        if !current_line.is_empty() {
            current_line_width += spacing_x;
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
            current_x += child_width + spacing_x;
        }

        current_y += line_height + spacing_y;
    }

    // Return total content height (subtract trailing spacing)
    let total_height = current_y - content_y;
    if num_lines > 0 {
        total_height - spacing_y // Remove trailing spacing
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

fn spacing_x(attrs: &Attrs) -> f32 {
    attrs
        .spacing_x
        .or(attrs.spacing)
        .unwrap_or(0.0) as f32
}

fn spacing_y(attrs: &Attrs) -> f32 {
    attrs
        .spacing_y
        .or(attrs.spacing)
        .unwrap_or(0.0) as f32
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
    fn test_layout_el_shrink_to_content() {
        let mut tree = ElementTree::new();

        let mut parent_attrs = Attrs::default();
        parent_attrs.width = Some(Length::Content);
        parent_attrs.height = Some(Length::Content);
        parent_attrs.padding = Some(Padding::Uniform(10.0));

        let mut child_attrs = Attrs::default();
        child_attrs.content = Some("Hi".to_string());
        child_attrs.font_size = Some(10.0);

        let mut parent = make_element("root", ElementKind::El, parent_attrs);
        let child = make_element("child", ElementKind::Text, child_attrs);
        let root_id = parent.id.clone();
        let child_id = child.id.clone();

        parent.children = vec![child_id.clone()];
        tree.root = Some(root_id.clone());
        tree.insert(parent);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(300.0, 200.0), 1.0, &MockTextMeasurer);

        let frame = tree.get(&root_id).unwrap().frame.unwrap();
        assert_eq!(frame.width, 36.0); // 2 chars * 8px + 20 padding
        assert_eq!(frame.height, 30.0); // font_size 10 + 20 padding
    }

    #[test]
    fn test_layout_row_fill_portion_with_content_parent() {
        let mut tree = ElementTree::new();

        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Content);
        row_attrs.height = Some(Length::Px(30.0));

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::Text, {
            let mut a = Attrs::default();
            a.content = Some("AAAA".to_string());
            a.font_size = Some(10.0);
            a.width = Some(Length::FillPortion(2.0));
            a
        });
        let child2 = make_element("c2", ElementKind::Text, {
            let mut a = Attrs::default();
            a.content = Some("BB".to_string());
            a.font_size = Some(10.0);
            a.width = Some(Length::FillPortion(1.0));
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

        layout_tree(&mut tree, Constraint::new(300.0, 200.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.width, 32.0); // 4 chars * 8px
        assert_eq!(c2_frame.width, 16.0); // 2 chars * 8px
    }

    #[test]
    fn test_layout_column_fill_portion_with_content_parent() {
        let mut tree = ElementTree::new();

        let mut col_attrs = Attrs::default();
        col_attrs.width = Some(Length::Px(120.0));
        col_attrs.height = Some(Length::Content);

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let child1 = make_element("c1", ElementKind::Text, {
            let mut a = Attrs::default();
            a.content = Some("Hi".to_string());
            a.font_size = Some(12.0);
            a.height = Some(Length::FillPortion(2.0));
            a
        });
        let child2 = make_element("c2", ElementKind::Text, {
            let mut a = Attrs::default();
            a.content = Some("Yo".to_string());
            a.font_size = Some(14.0);
            a.height = Some(Length::FillPortion(1.0));
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

        layout_tree(&mut tree, Constraint::new(300.0, 200.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.height, 12.0);
        assert_eq!(c2_frame.height, 14.0);
    }

    #[test]
    fn test_layout_row_spacing_xy_uses_horizontal() {
        let mut tree = ElementTree::new();

        let mut row_attrs = Attrs::default();
        row_attrs.spacing_x = Some(12.0);
        row_attrs.spacing_y = Some(30.0);

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(10.0));
            a.height = Some(Length::Px(10.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(10.0));
            a.height = Some(Length::Px(10.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone()];
        tree.root = Some(row_id);
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);

        layout_tree(&mut tree, Constraint::new(200.0, 100.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.x, 0.0);
        assert_eq!(c2_frame.x, 22.0); // 10 + spacing_x 12
    }

    #[test]
    fn test_layout_column_spacing_xy_uses_vertical() {
        let mut tree = ElementTree::new();

        let mut col_attrs = Attrs::default();
        col_attrs.spacing_x = Some(5.0);
        col_attrs.spacing_y = Some(14.0);

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(10.0));
            a.height = Some(Length::Px(10.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(10.0));
            a.height = Some(Length::Px(10.0));
            a
        });

        let col_id = col.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();

        col.children = vec![c1_id.clone(), c2_id.clone()];
        tree.root = Some(col_id);
        tree.insert(col);
        tree.insert(child1);
        tree.insert(child2);

        layout_tree(&mut tree, Constraint::new(200.0, 100.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 24.0); // 10 + spacing_y 14
    }

    #[test]
    fn test_layout_wrapped_row_spacing_xy_uses_vertical_between_lines() {
        let mut tree = ElementTree::new();

        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(50.0));
        row_attrs.spacing_x = Some(5.0);
        row_attrs.spacing_y = Some(7.0);

        let mut row = make_element("row", ElementKind::WrappedRow, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(40.0));
            a.height = Some(Length::Px(10.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(40.0));
            a.height = Some(Length::Px(10.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone()];
        tree.root = Some(row_id);
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);

        layout_tree(&mut tree, Constraint::new(200.0, 100.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 17.0); // 10 + spacing_y 7
    }

    #[test]
    fn test_layout_row_space_evenly_distribution() {
        let mut tree = ElementTree::new();

        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(200.0));
        row_attrs.height = Some(Length::Px(20.0));
        row_attrs.space_evenly = Some(true);

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a.height = Some(Length::Px(20.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a.height = Some(Length::Px(20.0));
            a
        });
        let child3 = make_element("c3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a.height = Some(Length::Px(20.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();
        let c3_id = child3.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
        tree.root = Some(row_id);
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);
        tree.insert(child3);

        layout_tree(&mut tree, Constraint::new(300.0, 100.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
        let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.x, 0.0);
        assert_eq!(c2_frame.x, 90.0);
        assert_eq!(c3_frame.x, 180.0);
    }

    #[test]
    fn test_layout_column_space_evenly_distribution() {
        let mut tree = ElementTree::new();

        let mut col_attrs = Attrs::default();
        col_attrs.width = Some(Length::Px(50.0));
        col_attrs.height = Some(Length::Px(200.0));
        col_attrs.space_evenly = Some(true);

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(20.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(20.0));
            a
        });
        let child3 = make_element("c3", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(20.0));
            a
        });

        let col_id = col.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();
        let c3_id = child3.id.clone();

        col.children = vec![c1_id.clone(), c2_id.clone(), c3_id.clone()];
        tree.root = Some(col_id);
        tree.insert(col);
        tree.insert(child1);
        tree.insert(child2);
        tree.insert(child3);

        layout_tree(&mut tree, Constraint::new(300.0, 300.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();
        let c3_frame = tree.get(&c3_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.y, 0.0);
        assert_eq!(c2_frame.y, 90.0);
        assert_eq!(c3_frame.y, 180.0);
    }

    #[test]
    fn test_layout_row_space_evenly_ignored_for_content_parent() {
        let mut tree = ElementTree::new();

        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Content);
        row_attrs.space_evenly = Some(true);

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let child1 = make_element("c1", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a.height = Some(Length::Px(10.0));
            a
        });
        let child2 = make_element("c2", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(20.0));
            a.height = Some(Length::Px(10.0));
            a
        });

        let row_id = row.id.clone();
        let c1_id = child1.id.clone();
        let c2_id = child2.id.clone();

        row.children = vec![c1_id.clone(), c2_id.clone()];
        tree.root = Some(row_id);
        tree.insert(row);
        tree.insert(child1);
        tree.insert(child2);

        layout_tree(&mut tree, Constraint::new(300.0, 100.0), 1.0, &MockTextMeasurer);

        let c1_frame = tree.get(&c1_id).unwrap().frame.unwrap();
        let c2_frame = tree.get(&c2_id).unwrap().frame.unwrap();

        assert_eq!(c1_frame.x, 0.0);
        assert_eq!(c2_frame.x, 20.0);
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
    fn test_layout_preserves_scroll_offsets() {
        let mut tree = ElementTree::new();

        let mut attrs = Attrs::default();
        attrs.scrollbar_y = Some(true);
        attrs.scroll_y = Some(40.0);

        let mut root = make_element("root", ElementKind::Column, attrs);
        let root_id = root.id.clone();
        root.frame = Some(Frame {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            content_width: 100.0,
            content_height: 200.0,
        });

        tree.root = Some(root_id.clone());
        tree.insert(root);

        layout_tree(&mut tree, Constraint::new(100.0, 100.0), 1.0, &MockTextMeasurer);
        let first = tree.get(&root_id).unwrap().attrs.scroll_y;
        assert_eq!(first, Some(40.0));

        layout_tree(&mut tree, Constraint::new(100.0, 100.0), 1.0, &MockTextMeasurer);
        let second = tree.get(&root_id).unwrap().attrs.scroll_y;
        assert_eq!(second, Some(40.0));
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
        col_attrs.scrollbar_y = Some(true); // Makes it scrollable

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

        let col_attrs = &tree.get(&col_id).unwrap().attrs;
        assert_eq!(col_attrs.scroll_y, Some(0.0));
        assert_eq!(col_attrs.scroll_y_max, Some(140.0));
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

    #[test]
    fn test_el_center_x_aligns_child() {
        let mut tree = ElementTree::new();

        // El with center_x alignment and a smaller child
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(50.0));
        el_attrs.align_x = Some(AlignX::Center);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Child should be centered horizontally: (200 - 80) / 2 = 60
        assert_eq!(child_frame.x, 60.0);
        // Child should be at top (default align_y is Top)
        assert_eq!(child_frame.y, 0.0);
    }

    #[test]
    fn test_el_center_y_aligns_child() {
        let mut tree = ElementTree::new();

        // El with center_y alignment and a smaller child
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(100.0));
        el_attrs.align_y = Some(AlignY::Center);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(40.0));
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Child should be at left (default align_x is Left)
        assert_eq!(child_frame.x, 0.0);
        // Child should be centered vertically: (100 - 40) / 2 = 30
        assert_eq!(child_frame.y, 30.0);
    }

    #[test]
    fn test_el_center_both_axes() {
        let mut tree = ElementTree::new();

        // El with both center_x and center_y
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(100.0));
        el_attrs.align_x = Some(AlignX::Center);
        el_attrs.align_y = Some(AlignY::Center);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(40.0));
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Child should be centered: (200 - 80) / 2 = 60, (100 - 40) / 2 = 30
        assert_eq!(child_frame.x, 60.0);
        assert_eq!(child_frame.y, 30.0);
    }

    #[test]
    fn test_el_align_right() {
        let mut tree = ElementTree::new();

        // El with align_right
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(50.0));
        el_attrs.align_x = Some(AlignX::Right);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(30.0));
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Child should be right-aligned: 200 - 80 = 120
        assert_eq!(child_frame.x, 120.0);
    }

    #[test]
    fn test_el_align_bottom() {
        let mut tree = ElementTree::new();

        // El with align_bottom
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(100.0));
        el_attrs.align_y = Some(AlignY::Bottom);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(40.0));
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Child should be bottom-aligned: 100 - 40 = 60
        assert_eq!(child_frame.y, 60.0);
    }

    #[test]
    fn test_child_alignment_overrides_parent() {
        let mut tree = ElementTree::new();

        // Parent has center_x, child has align_right - child should win
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(50.0));
        el_attrs.align_x = Some(AlignX::Center);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(80.0));
            a.height = Some(Length::Px(30.0));
            a.align_x = Some(AlignX::Right);  // Child overrides parent
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Child should be right-aligned (override): 200 - 80 = 120
        assert_eq!(child_frame.x, 120.0);
    }

    #[test]
    fn test_el_with_padding_and_center() {
        let mut tree = ElementTree::new();

        // El with padding and center alignment
        let mut el_attrs = Attrs::default();
        el_attrs.width = Some(Length::Px(200.0));
        el_attrs.height = Some(Length::Px(100.0));
        el_attrs.padding = Some(Padding::Uniform(20.0));
        el_attrs.align_x = Some(AlignX::Center);
        el_attrs.align_y = Some(AlignY::Center);

        let mut el = make_element("el", ElementKind::El, el_attrs);

        let child = make_element("child", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(60.0));
            a.height = Some(Length::Px(20.0));
            a
        });

        let el_id = el.id.clone();
        let child_id = child.id.clone();
        el.children = vec![child_id.clone()];

        tree.root = Some(el_id.clone());
        tree.insert(el);
        tree.insert(child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let child_frame = tree.get(&child_id).unwrap().frame.unwrap();

        // Content area: 200 - 40 = 160 width, 100 - 40 = 60 height
        // Child centered in content area:
        // x = 20 (padding) + (160 - 60) / 2 = 20 + 50 = 70
        // y = 20 (padding) + (60 - 20) / 2 = 20 + 20 = 40
        assert_eq!(child_frame.x, 70.0);
        assert_eq!(child_frame.y, 40.0);
    }

    #[test]
    fn test_row_self_alignment_zones() {
        let mut tree = ElementTree::new();

        // Row with 300px width, 3 children:
        // - left-aligned child (50px)
        // - center-aligned child (50px)
        // - right-aligned child (50px)
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(300.0));
        row_attrs.height = Some(Length::Px(50.0));

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        let left_child = make_element("left", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a.align_x = Some(AlignX::Left);
            a
        });

        let center_child = make_element("center", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a.align_x = Some(AlignX::Center);
            a
        });

        let right_child = make_element("right", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(30.0));
            a.align_x = Some(AlignX::Right);
            a
        });

        let row_id = row.id.clone();
        let left_id = left_child.id.clone();
        let center_id = center_child.id.clone();
        let right_id = right_child.id.clone();

        row.children = vec![left_id.clone(), center_id.clone(), right_id.clone()];

        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(left_child);
        tree.insert(center_child);
        tree.insert(right_child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let left_frame = tree.get(&left_id).unwrap().frame.unwrap();
        let center_frame = tree.get(&center_id).unwrap().frame.unwrap();
        let right_frame = tree.get(&right_id).unwrap().frame.unwrap();

        // Left child at x=0
        assert_eq!(left_frame.x, 0.0);

        // Right child at far right: 300 - 50 = 250
        assert_eq!(right_frame.x, 250.0);

        // Center child in the middle of remaining space
        // Remaining space: 0+50 to 250 = 200px gap
        // Center of gap: 50 + (200 - 50) / 2 = 50 + 75 = 125
        assert_eq!(center_frame.x, 125.0);
    }

    #[test]
    fn test_column_self_alignment_zones() {
        let mut tree = ElementTree::new();

        // Column with 300px height, 3 children:
        // - top-aligned child (50px)
        // - center-aligned child (50px)
        // - bottom-aligned child (50px)
        let mut col_attrs = Attrs::default();
        col_attrs.width = Some(Length::Px(100.0));
        col_attrs.height = Some(Length::Px(300.0));

        let mut col = make_element("col", ElementKind::Column, col_attrs);

        let top_child = make_element("top", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(50.0));
            a.align_y = Some(AlignY::Top);
            a
        });

        let center_child = make_element("center", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(50.0));
            a.align_y = Some(AlignY::Center);
            a
        });

        let bottom_child = make_element("bottom", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(50.0));
            a.height = Some(Length::Px(50.0));
            a.align_y = Some(AlignY::Bottom);
            a
        });

        let col_id = col.id.clone();
        let top_id = top_child.id.clone();
        let center_id = center_child.id.clone();
        let bottom_id = bottom_child.id.clone();

        col.children = vec![top_id.clone(), center_id.clone(), bottom_id.clone()];

        tree.root = Some(col_id.clone());
        tree.insert(col);
        tree.insert(top_child);
        tree.insert(center_child);
        tree.insert(bottom_child);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let top_frame = tree.get(&top_id).unwrap().frame.unwrap();
        let center_frame = tree.get(&center_id).unwrap().frame.unwrap();
        let bottom_frame = tree.get(&bottom_id).unwrap().frame.unwrap();

        // Top child at y=0
        assert_eq!(top_frame.y, 0.0);

        // Bottom child at far bottom: 300 - 50 = 250
        assert_eq!(bottom_frame.y, 250.0);

        // Center child in the middle of remaining space
        // Remaining space: 0+50 to 250 = 200px gap
        // Center of gap: 50 + (200 - 50) / 2 = 50 + 75 = 125
        assert_eq!(center_frame.y, 125.0);
    }

    #[test]
    fn test_row_with_mixed_alignments_and_vertical() {
        let mut tree = ElementTree::new();

        // Row with children at different horizontal and vertical alignments
        let mut row_attrs = Attrs::default();
        row_attrs.width = Some(Length::Px(200.0));
        row_attrs.height = Some(Length::Px(100.0));

        let mut row = make_element("row", ElementKind::Row, row_attrs);

        // Left-aligned, top-aligned
        let left_top = make_element("lt", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(40.0));
            a.height = Some(Length::Px(30.0));
            a.align_x = Some(AlignX::Left);
            a.align_y = Some(AlignY::Top);
            a
        });

        // Right-aligned, bottom-aligned
        let right_bottom = make_element("rb", ElementKind::El, {
            let mut a = Attrs::default();
            a.width = Some(Length::Px(40.0));
            a.height = Some(Length::Px(30.0));
            a.align_x = Some(AlignX::Right);
            a.align_y = Some(AlignY::Bottom);
            a
        });

        let row_id = row.id.clone();
        let lt_id = left_top.id.clone();
        let rb_id = right_bottom.id.clone();

        row.children = vec![lt_id.clone(), rb_id.clone()];

        tree.root = Some(row_id.clone());
        tree.insert(row);
        tree.insert(left_top);
        tree.insert(right_bottom);

        layout_tree(&mut tree, Constraint::new(800.0, 600.0), 1.0, &MockTextMeasurer);

        let lt_frame = tree.get(&lt_id).unwrap().frame.unwrap();
        let rb_frame = tree.get(&rb_id).unwrap().frame.unwrap();

        // Left-top: x=0, y=0
        assert_eq!(lt_frame.x, 0.0);
        assert_eq!(lt_frame.y, 0.0);

        // Right-bottom: x=160 (200-40), y=70 (100-30)
        assert_eq!(rb_frame.x, 160.0);
        assert_eq!(rb_frame.y, 70.0);
    }
}
