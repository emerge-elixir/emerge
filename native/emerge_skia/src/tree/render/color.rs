use crate::tree::attrs::Color;

pub(crate) const DEFAULT_TEXT_COLOR: u32 = 0x000000FF;

pub(super) fn color_to_u32(color: &Color) -> u32 {
    match color {
        Color::Rgb { r, g, b } => {
            ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | 0xFF
        }
        Color::Rgba { r, g, b, a } => {
            ((*r as u32) << 24) | ((*g as u32) << 16) | ((*b as u32) << 8) | (*a as u32)
        }
        Color::Named(name) => named_color(name),
    }
}

pub(super) fn named_color(name: &str) -> u32 {
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
