use super::super::color::{color_to_u32, named_color};
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
    let rgb = Color::Rgb {
        r: 255,
        g: 128,
        b: 64,
    };
    assert_eq!(color_to_u32(&rgb), 0xFF8040FF);

    // Test RGBA color
    let rgba = Color::Rgba {
        r: 255,
        g: 128,
        b: 64,
        a: 200,
    };
    assert_eq!(color_to_u32(&rgba), 0xFF8040C8);

    // Test named color
    let named = Color::Named("cyan".to_string());
    assert_eq!(color_to_u32(&named), 0x00FFFFFF);
}
