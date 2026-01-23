//! Render an ElementTree into DrawCmds.
//!
//! Reads from pre-scaled attrs (scaling is applied in the layout pass).

use super::attrs::{Background, Color, Padding};
use super::element::{ElementId, ElementKind, ElementTree};
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
        "gray" => 0x808080FF,
        "grey" => 0x808080FF,
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
