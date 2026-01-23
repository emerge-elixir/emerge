//! Render an ElementTree into DrawCmds.
//!
//! Reads from pre-scaled attrs (scaling is applied in the layout pass).

use super::attrs::{Background, Color, Padding};
use super::deserialize::decode_tree;
use super::element::{ElementId, ElementKind, ElementTree, Frame};
use super::layout::{layout_tree, Constraint, SkiaTextMeasurer};
use crate::renderer::DrawCmd;

/// Render the tree to draw commands.
/// Reads from pre-scaled attrs (layout pass must run first).
pub fn render_tree(tree: &ElementTree) -> Vec<DrawCmd> {
    let Some(root) = tree.root.as_ref() else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    render_element(tree, root, &mut commands);
    commands
}

fn render_element(tree: &ElementTree, id: &ElementId, commands: &mut Vec<DrawCmd>) {
    let Some(element) = tree.get(id) else {
        return;
    };

    let frame = match element.frame {
        Some(frame) => frame,
        None => return,
    };

    // Read from pre-scaled attrs
    let attrs = &element.attrs;
    let radius = attrs.border_radius.unwrap_or(0.0) as f32;

    // Render "behind" elements first (underlay)
    if let Some(behind_bytes) = &attrs.behind {
        render_nearby_element(behind_bytes, frame, NearbyPosition::Behind, commands);
    }

    if let Some(background) = &attrs.background {
        match background {
            Background::Color(color) => {
                let fill = color_to_u32(color);
                if radius > 0.0 {
                    commands.push(DrawCmd::RoundedRect(
                        frame.x,
                        frame.y,
                        frame.width,
                        frame.height,
                        radius,
                        fill,
                    ));
                } else {
                    commands.push(DrawCmd::Rect(
                        frame.x,
                        frame.y,
                        frame.width,
                        frame.height,
                        fill,
                    ));
                }
            }
            Background::Gradient { from, to, angle } => {
                commands.push(DrawCmd::Gradient(
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    color_to_u32(from),
                    color_to_u32(to),
                    *angle as f32,
                ));
            }
        }
    }

    if let (Some(border_width), Some(border_color)) = (attrs.border_width, &attrs.border_color)
        && border_width > 0.0
    {
        commands.push(DrawCmd::Border(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            radius,
            border_width as f32,
            color_to_u32(border_color),
        ));
    }

    let clip = attrs.clip.unwrap_or(false) || attrs.clip_x.unwrap_or(false) || attrs.clip_y.unwrap_or(false);
    if clip {
        commands.push(DrawCmd::PushClip(frame.x, frame.y, frame.width, frame.height));
    }

    if element.kind == ElementKind::Text
        && let Some(content) = attrs.content.as_deref()
    {
        let font_size = attrs.font_size.unwrap_or(16.0) as f32;
        let color = attrs.font_color.as_ref().map(color_to_u32).unwrap_or(0xFFFFFFFF);
        let (padding_left, padding_top) = text_padding(attrs.padding.as_ref());
        let (ascent, _) = text_metrics(font_size);
        let text_x = frame.x + padding_left;
        let baseline_y = frame.y + padding_top + ascent;
        commands.push(DrawCmd::Text(text_x, baseline_y, content.to_string(), font_size, color));
    }

    for child_id in &element.children {
        render_element(tree, child_id, commands);
    }

    if clip {
        commands.push(DrawCmd::PopClip);
    }

    // Render nearby positioned elements (above, below, on_left, on_right)
    if let Some(above_bytes) = &attrs.above {
        render_nearby_element(above_bytes, frame, NearbyPosition::Above, commands);
    }
    if let Some(below_bytes) = &attrs.below {
        render_nearby_element(below_bytes, frame, NearbyPosition::Below, commands);
    }
    if let Some(on_left_bytes) = &attrs.on_left {
        render_nearby_element(on_left_bytes, frame, NearbyPosition::OnLeft, commands);
    }
    if let Some(on_right_bytes) = &attrs.on_right {
        render_nearby_element(on_right_bytes, frame, NearbyPosition::OnRight, commands);
    }

    // Render "in_front" elements last (overlay)
    if let Some(in_front_bytes) = &attrs.in_front {
        render_nearby_element(in_front_bytes, frame, NearbyPosition::InFront, commands);
    }
}

fn color_to_u32(color: &Color) -> u32 {
    match color {
        Color::Rgb { r, g, b } => ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | 0xFF,
        Color::Rgba { r, g, b, a } => ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | (*a as u32),
        Color::Named(name) => named_color(name),
    }
}

fn named_color(name: &str) -> u32 {
    match name {
        "white" => 0xFFFFFFFF,
        "black" => 0x000000FF,
        "red" => 0xFF0000FF,
        "green" => 0x00FF00FF,
        "blue" => 0x0000FFFF,
        "cyan" => 0x00FFFFFF,
        "magenta" => 0xFF00FFFF,
        "yellow" => 0xFFFF00FF,
        "orange" => 0xFFA500FF,
        "purple" => 0x800080FF,
        "pink" => 0xFFC0CBFF,
        "gray" => 0x808080FF,
        "grey" => 0x808080FF,
        "navy" => 0x000080FF,
        "teal" => 0x008080FF,
        _ => 0xFFFFFFFF,
    }
}

/// Get text padding from pre-scaled attrs.
fn text_padding(padding: Option<&Padding>) -> (f32, f32) {
    match padding {
        Some(Padding::Uniform(value)) => (*value as f32, *value as f32),
        Some(Padding::Sides { top, left, .. }) => (*left as f32, *top as f32),
        None => (0.0, 0.0),
    }
}

fn text_metrics(font_size: f32) -> (f32, f32) {
    use crate::renderer::get_default_typeface;
    use skia_safe::Font;

    let typeface = get_default_typeface();
    let font = Font::new(typeface, font_size);
    let (_, metrics) = font.metrics();
    (metrics.ascent.abs(), metrics.descent)
}

// =============================================================================
// Nearby Element Rendering
// =============================================================================

/// Position for nearby elements relative to the parent.
#[derive(Clone, Copy, Debug)]
enum NearbyPosition {
    Above,
    Below,
    OnLeft,
    OnRight,
    InFront,
    Behind,
}

/// Decode, layout, and render a nearby element.
fn render_nearby_element(
    data: &[u8],
    parent_frame: Frame,
    position: NearbyPosition,
    commands: &mut Vec<DrawCmd>,
) {
    // Decode the nearby element tree from EMRG bytes
    let mut nearby_tree = match decode_tree(data) {
        Ok(tree) => tree,
        Err(_) => return,
    };

    if nearby_tree.is_empty() {
        return;
    }

    // Layout the nearby element with a large constraint (it will size to content)
    let constraint = Constraint::new(10000.0, 10000.0);
    layout_tree(&mut nearby_tree, constraint, 1.0, &SkiaTextMeasurer);

    // Get the nearby element's computed size
    let Some(root_id) = nearby_tree.root.clone() else {
        return;
    };
    let nearby_frame = {
        let Some(root) = nearby_tree.get(&root_id) else {
            return;
        };
        let Some(frame) = root.frame else {
            return;
        };
        frame
    };

    // Calculate the offset based on position
    let (offset_x, offset_y) = match position {
        NearbyPosition::Above => {
            // Centered horizontally, positioned above
            let x = parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0;
            let y = parent_frame.y - nearby_frame.height;
            (x, y)
        }
        NearbyPosition::Below => {
            // Centered horizontally, positioned below
            let x = parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0;
            let y = parent_frame.y + parent_frame.height;
            (x, y)
        }
        NearbyPosition::OnLeft => {
            // Positioned to the left, centered vertically
            let x = parent_frame.x - nearby_frame.width;
            let y = parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0;
            (x, y)
        }
        NearbyPosition::OnRight => {
            // Positioned to the right, centered vertically
            let x = parent_frame.x + parent_frame.width;
            let y = parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0;
            (x, y)
        }
        NearbyPosition::InFront | NearbyPosition::Behind => {
            // Centered on the parent
            let x = parent_frame.x + (parent_frame.width - nearby_frame.width) / 2.0;
            let y = parent_frame.y + (parent_frame.height - nearby_frame.height) / 2.0;
            (x, y)
        }
    };

    // Shift the entire nearby tree to the calculated position
    shift_nearby_tree(&mut nearby_tree, offset_x, offset_y);

    // Render the nearby tree
    render_tree_recursive(&nearby_tree, &root_id, commands);
}

/// Shift all frames in a nearby tree by the given offset.
fn shift_nearby_tree(tree: &mut ElementTree, offset_x: f32, offset_y: f32) {
    for element in tree.nodes.values_mut() {
        if let Some(ref mut frame) = element.frame {
            frame.x += offset_x;
            frame.y += offset_y;
        }
    }
}

/// Recursively render a tree (used for nearby elements).
fn render_tree_recursive(tree: &ElementTree, id: &ElementId, commands: &mut Vec<DrawCmd>) {
    let Some(element) = tree.get(id) else {
        return;
    };

    let frame = match element.frame {
        Some(frame) => frame,
        None => return,
    };

    let attrs = &element.attrs;
    let radius = attrs.border_radius.unwrap_or(0.0) as f32;

    // Render behind first
    if let Some(behind_bytes) = &attrs.behind {
        render_nearby_element(behind_bytes, frame, NearbyPosition::Behind, commands);
    }

    if let Some(background) = &attrs.background {
        match background {
            Background::Color(color) => {
                let fill = color_to_u32(color);
                if radius > 0.0 {
                    commands.push(DrawCmd::RoundedRect(
                        frame.x,
                        frame.y,
                        frame.width,
                        frame.height,
                        radius,
                        fill,
                    ));
                } else {
                    commands.push(DrawCmd::Rect(
                        frame.x,
                        frame.y,
                        frame.width,
                        frame.height,
                        fill,
                    ));
                }
            }
            Background::Gradient { from, to, angle } => {
                commands.push(DrawCmd::Gradient(
                    frame.x,
                    frame.y,
                    frame.width,
                    frame.height,
                    color_to_u32(from),
                    color_to_u32(to),
                    *angle as f32,
                ));
            }
        }
    }

    if let (Some(border_width), Some(border_color)) = (attrs.border_width, &attrs.border_color)
        && border_width > 0.0
    {
        commands.push(DrawCmd::Border(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            radius,
            border_width as f32,
            color_to_u32(border_color),
        ));
    }

    let clip = attrs.clip.unwrap_or(false)
        || attrs.clip_x.unwrap_or(false)
        || attrs.clip_y.unwrap_or(false);
    if clip {
        commands.push(DrawCmd::PushClip(
            frame.x,
            frame.y,
            frame.width,
            frame.height,
        ));
    }

    if element.kind == ElementKind::Text
        && let Some(content) = attrs.content.as_deref()
    {
        let font_size = attrs.font_size.unwrap_or(16.0) as f32;
        let color = attrs
            .font_color
            .as_ref()
            .map(color_to_u32)
            .unwrap_or(0xFFFFFFFF);
        let (padding_left, padding_top) = text_padding(attrs.padding.as_ref());
        let (ascent, _) = text_metrics(font_size);
        let text_x = frame.x + padding_left;
        let baseline_y = frame.y + padding_top + ascent;
        commands.push(DrawCmd::Text(
            text_x,
            baseline_y,
            content.to_string(),
            font_size,
            color,
        ));
    }

    for child_id in &element.children {
        render_tree_recursive(tree, child_id, commands);
    }

    if clip {
        commands.push(DrawCmd::PopClip);
    }

    // Render nearby positioned elements
    if let Some(above_bytes) = &attrs.above {
        render_nearby_element(above_bytes, frame, NearbyPosition::Above, commands);
    }
    if let Some(below_bytes) = &attrs.below {
        render_nearby_element(below_bytes, frame, NearbyPosition::Below, commands);
    }
    if let Some(on_left_bytes) = &attrs.on_left {
        render_nearby_element(on_left_bytes, frame, NearbyPosition::OnLeft, commands);
    }
    if let Some(on_right_bytes) = &attrs.on_right {
        render_nearby_element(on_right_bytes, frame, NearbyPosition::OnRight, commands);
    }

    // Render in_front last
    if let Some(in_front_bytes) = &attrs.in_front {
        render_nearby_element(in_front_bytes, frame, NearbyPosition::InFront, commands);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_named_colors() {
        // Test basic colors
        assert_eq!(named_color("white"), 0xFFFFFFFF);
        assert_eq!(named_color("black"), 0x000000FF);
        assert_eq!(named_color("red"), 0xFF0000FF);
        assert_eq!(named_color("green"), 0x00FF00FF);
        assert_eq!(named_color("blue"), 0x0000FFFF);

        // Test newly added colors
        assert_eq!(named_color("cyan"), 0x00FFFFFF);
        assert_eq!(named_color("magenta"), 0xFF00FFFF);
        assert_eq!(named_color("yellow"), 0xFFFF00FF);
        assert_eq!(named_color("orange"), 0xFFA500FF);
        assert_eq!(named_color("purple"), 0x800080FF);
        assert_eq!(named_color("pink"), 0xFFC0CBFF);
        assert_eq!(named_color("navy"), 0x000080FF);
        assert_eq!(named_color("teal"), 0x008080FF);

        // Test both spellings of gray
        assert_eq!(named_color("gray"), 0x808080FF);
        assert_eq!(named_color("grey"), 0x808080FF);

        // Test unknown color defaults to white
        assert_eq!(named_color("unknown"), 0xFFFFFFFF);
    }

    #[test]
    fn test_color_to_u32() {
        // Test RGB color
        let rgb = Color::Rgb { r: 255, g: 128, b: 64 };
        assert_eq!(color_to_u32(&rgb), 0xFF8040FF);

        // Test RGBA color
        let rgba = Color::Rgba { r: 255, g: 128, b: 64, a: 200 };
        assert_eq!(color_to_u32(&rgba), 0xFF8040C8);

        // Test named color
        let named = Color::Named("cyan".to_string());
        assert_eq!(color_to_u32(&named), 0x00FFFFFF);
    }

    #[test]
    fn test_nearby_position_calculations() {
        // Test Above: should be centered horizontally, positioned above
        let parent = Frame { x: 100.0, y: 100.0, width: 200.0, height: 50.0, content_width: 200.0, content_height: 50.0 };
        let nearby = Frame { x: 0.0, y: 0.0, width: 50.0, height: 20.0, content_width: 50.0, content_height: 20.0 };

        // Above: x = 100 + (200 - 50) / 2 = 175, y = 100 - 20 = 80
        let (x, y) = match NearbyPosition::Above {
            NearbyPosition::Above => {
                let x = parent.x + (parent.width - nearby.width) / 2.0;
                let y = parent.y - nearby.height;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 175.0);
        assert_eq!(y, 80.0);

        // Below: x = 175, y = 100 + 50 = 150
        let (x, y) = match NearbyPosition::Below {
            NearbyPosition::Below => {
                let x = parent.x + (parent.width - nearby.width) / 2.0;
                let y = parent.y + parent.height;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 175.0);
        assert_eq!(y, 150.0);

        // OnLeft: x = 100 - 50 = 50, y = 100 + (50 - 20) / 2 = 115
        let (x, y) = match NearbyPosition::OnLeft {
            NearbyPosition::OnLeft => {
                let x = parent.x - nearby.width;
                let y = parent.y + (parent.height - nearby.height) / 2.0;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 50.0);
        assert_eq!(y, 115.0);

        // OnRight: x = 100 + 200 = 300, y = 115
        let (x, y) = match NearbyPosition::OnRight {
            NearbyPosition::OnRight => {
                let x = parent.x + parent.width;
                let y = parent.y + (parent.height - nearby.height) / 2.0;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 300.0);
        assert_eq!(y, 115.0);

        // InFront: centered on parent x = 175, y = 115
        let (x, y) = match NearbyPosition::InFront {
            NearbyPosition::InFront => {
                let x = parent.x + (parent.width - nearby.width) / 2.0;
                let y = parent.y + (parent.height - nearby.height) / 2.0;
                (x, y)
            }
            _ => unreachable!(),
        };
        assert_eq!(x, 175.0);
        assert_eq!(y, 115.0);
    }
}
