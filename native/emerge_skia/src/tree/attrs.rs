//! Attribute types and decoding for EMRG v2 format.
//!
//! Attributes are encoded as a compact binary block:
//! - attr_count (u16)
//! - attr_records... (tag u8 + value)

use super::deserialize::DecodeError;

// =============================================================================
// Attribute Types
// =============================================================================

/// Length specification for width/height.
#[derive(Clone, Debug, PartialEq)]
pub enum Length {
    Fill,
    Content,
    Px(f64),
    FillPortion(f64),
    /// Minimum constraint: the resolved length must be at least this many pixels.
    Minimum(f64, Box<Length>),
    /// Maximum constraint: the resolved length must be at most this many pixels.
    Maximum(f64, Box<Length>),
}

/// Padding specification.
#[derive(Clone, Debug, PartialEq)]
pub enum Padding {
    Uniform(f64),
    Sides {
        top: f64,
        right: f64,
        bottom: f64,
        left: f64,
    },
}

impl Default for Padding {
    fn default() -> Self {
        Padding::Uniform(0.0)
    }
}

/// Horizontal alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlignX {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlignY {
    #[default]
    Top,
    Center,
    Bottom,
}

/// Text alignment within element bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Color value.
#[derive(Clone, Debug, PartialEq)]
pub enum Color {
    Rgb { r: u8, g: u8, b: u8 },
    Rgba { r: u8, g: u8, b: u8, a: u8 },
    Named(String),
}

/// Background specification.
#[derive(Clone, Debug, PartialEq)]
pub enum Background {
    Color(Color),
    Gradient { from: Color, to: Color, angle: f64 },
}

/// Border radius specification.
#[derive(Clone, Debug, PartialEq)]
pub enum BorderRadius {
    Uniform(f64),
    Corners { tl: f64, tr: f64, br: f64, bl: f64 },
}

/// Font specification.
#[derive(Clone, Debug, PartialEq)]
pub enum Font {
    Atom(String),
    String(String),
}

/// Font weight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontWeight(pub String);

/// Font style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontStyle(pub String);

/// All decoded attributes for an element.
#[derive(Clone, Debug, Default)]
pub struct Attrs {
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub padding: Option<Padding>,
    pub spacing: Option<f64>,
    pub spacing_x: Option<f64>,
    pub spacing_y: Option<f64>,
    pub align_x: Option<AlignX>,
    pub align_y: Option<AlignY>,
    pub scrollbar_y: Option<bool>,
    pub scrollbar_x: Option<bool>,
    pub scroll_x: Option<f64>,
    pub scroll_y: Option<f64>,
    pub scroll_x_max: Option<f64>,
    pub scroll_y_max: Option<f64>,
    pub on_click: Option<bool>,
    pub on_mouse_down: Option<bool>,
    pub on_mouse_up: Option<bool>,
    pub on_mouse_enter: Option<bool>,
    pub on_mouse_leave: Option<bool>,
    pub on_mouse_move: Option<bool>,
    pub clip_y: Option<bool>,
    pub clip_x: Option<bool>,
    pub background: Option<Background>,
    pub border_radius: Option<BorderRadius>,
    pub border_width: Option<f64>,
    pub border_color: Option<Color>,
    pub font_size: Option<f64>,
    pub font_color: Option<Color>,
    pub font: Option<Font>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub text_align: Option<TextAlign>,
    pub content: Option<String>,
    pub snap_layout: Option<bool>,
    pub snap_text_metrics: Option<bool>,
    pub move_x: Option<f64>,
    pub move_y: Option<f64>,
    pub rotate: Option<f64>,
    pub scale: Option<f64>,
    pub alpha: Option<f64>,
    pub space_evenly: Option<bool>,
    // Nearby elements stored as raw EMRG bytes (decoded lazily)
    pub above: Option<Vec<u8>>,
    pub below: Option<Vec<u8>>,
    pub on_left: Option<Vec<u8>>,
    pub on_right: Option<Vec<u8>>,
    pub in_front: Option<Vec<u8>>,
    pub behind: Option<Vec<u8>>,
}

// =============================================================================
// Attribute Tags
// =============================================================================

const TAG_WIDTH: u8 = 1;
const TAG_HEIGHT: u8 = 2;
const TAG_PADDING: u8 = 3;
const TAG_SPACING: u8 = 4;
const TAG_ALIGN_X: u8 = 5;
const TAG_ALIGN_Y: u8 = 6;
const TAG_SCROLLBAR_Y: u8 = 7;
const TAG_SCROLLBAR_X: u8 = 8;
const TAG_CLIP_Y: u8 = 10;
const TAG_CLIP_X: u8 = 11;
const TAG_BACKGROUND: u8 = 12;
const TAG_BORDER_RADIUS: u8 = 13;
const TAG_BORDER_WIDTH: u8 = 14;
const TAG_BORDER_COLOR: u8 = 15;
const TAG_FONT_SIZE: u8 = 16;
const TAG_FONT_COLOR: u8 = 17;
const TAG_FONT: u8 = 18;
const TAG_FONT_WEIGHT: u8 = 19;
const TAG_FONT_STYLE: u8 = 20;
const TAG_CONTENT: u8 = 21;
const TAG_ABOVE: u8 = 22;
const TAG_BELOW: u8 = 23;
const TAG_ON_LEFT: u8 = 24;
const TAG_ON_RIGHT: u8 = 25;
const TAG_IN_FRONT: u8 = 26;
const TAG_BEHIND: u8 = 27;
const TAG_SNAP_LAYOUT: u8 = 28;
const TAG_SNAP_TEXT_METRICS: u8 = 29;
const TAG_TEXT_ALIGN: u8 = 30;
const TAG_MOVE_X: u8 = 31;
const TAG_MOVE_Y: u8 = 32;
const TAG_ROTATE: u8 = 33;
const TAG_SCALE: u8 = 34;
const TAG_ALPHA: u8 = 35;
const TAG_SPACING_XY: u8 = 36;
const TAG_SPACE_EVENLY: u8 = 37;
const TAG_SCROLL_X: u8 = 38;
const TAG_SCROLL_Y: u8 = 39;
const TAG_ON_CLICK: u8 = 40;
const TAG_ON_MOUSE_DOWN: u8 = 41;
const TAG_ON_MOUSE_UP: u8 = 42;
const TAG_ON_MOUSE_ENTER: u8 = 43;
const TAG_ON_MOUSE_LEAVE: u8 = 44;
const TAG_ON_MOUSE_MOVE: u8 = 45;

// =============================================================================
// Decoder
// =============================================================================

/// Cursor for reading attribute binary data.
struct AttrCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> AttrCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + len > self.data.len() {
            return Err(DecodeError::UnexpectedEof);
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let bytes = self.read_bytes(1)?;
        Ok(bytes[0])
    }

    fn read_u16_be(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32_be(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_f64(&mut self) -> Result<f64, DecodeError> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_bool(&mut self) -> Result<bool, DecodeError> {
        Ok(self.read_u8()? != 0)
    }

    fn read_string_u16(&mut self) -> Result<String, DecodeError> {
        let len = self.read_u16_be()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| DecodeError::InvalidStructure("invalid UTF-8 in string".to_string()))
    }

    fn read_bytes_u32(&mut self) -> Result<Vec<u8>, DecodeError> {
        let len = self.read_u32_be()? as usize;
        let bytes = self.read_bytes(len)?;
        Ok(bytes.to_vec())
    }
}

/// Decode an attribute block into Attrs struct.
pub fn decode_attrs(data: &[u8]) -> Result<Attrs, DecodeError> {
    let mut cursor = AttrCursor::new(data);
    let mut attrs = Attrs::default();

    // Empty block is valid
    if cursor.remaining() == 0 {
        return Ok(attrs);
    }

    let attr_count = cursor.read_u16_be()? as usize;

    for _ in 0..attr_count {
        let tag = cursor.read_u8()?;
        decode_attr(&mut cursor, tag, &mut attrs)?;
    }

    Ok(attrs)
}

fn decode_attr(cursor: &mut AttrCursor, tag: u8, attrs: &mut Attrs) -> Result<(), DecodeError> {
    match tag {
        TAG_WIDTH => attrs.width = Some(decode_length(cursor)?),
        TAG_HEIGHT => attrs.height = Some(decode_length(cursor)?),
        TAG_PADDING => attrs.padding = Some(decode_padding(cursor)?),
        TAG_SPACING => attrs.spacing = Some(cursor.read_f64()?),
        TAG_ALIGN_X => attrs.align_x = Some(decode_align_x(cursor)?),
        TAG_ALIGN_Y => attrs.align_y = Some(decode_align_y(cursor)?),
        TAG_SCROLLBAR_Y => attrs.scrollbar_y = Some(cursor.read_bool()?),
        TAG_SCROLLBAR_X => attrs.scrollbar_x = Some(cursor.read_bool()?),
        TAG_CLIP_Y => attrs.clip_y = Some(cursor.read_bool()?),
        TAG_CLIP_X => attrs.clip_x = Some(cursor.read_bool()?),
        TAG_BACKGROUND => attrs.background = Some(decode_background(cursor)?),
        TAG_BORDER_RADIUS => attrs.border_radius = Some(decode_radius(cursor)?),
        TAG_BORDER_WIDTH => attrs.border_width = Some(cursor.read_f64()?),
        TAG_BORDER_COLOR => attrs.border_color = Some(decode_color(cursor)?),
        TAG_FONT_SIZE => attrs.font_size = Some(cursor.read_f64()?),
        TAG_FONT_COLOR => attrs.font_color = Some(decode_color(cursor)?),
        TAG_FONT => attrs.font = Some(decode_font(cursor)?),
        TAG_FONT_WEIGHT => attrs.font_weight = Some(decode_font_weight(cursor)?),
        TAG_FONT_STYLE => attrs.font_style = Some(decode_font_style(cursor)?),
        TAG_CONTENT => attrs.content = Some(cursor.read_string_u16()?),
        TAG_ABOVE => attrs.above = Some(cursor.read_bytes_u32()?),
        TAG_BELOW => attrs.below = Some(cursor.read_bytes_u32()?),
        TAG_ON_LEFT => attrs.on_left = Some(cursor.read_bytes_u32()?),
        TAG_ON_RIGHT => attrs.on_right = Some(cursor.read_bytes_u32()?),
        TAG_IN_FRONT => attrs.in_front = Some(cursor.read_bytes_u32()?),
        TAG_BEHIND => attrs.behind = Some(cursor.read_bytes_u32()?),
        TAG_SNAP_LAYOUT => attrs.snap_layout = Some(cursor.read_bool()?),
        TAG_SNAP_TEXT_METRICS => attrs.snap_text_metrics = Some(cursor.read_bool()?),
        TAG_TEXT_ALIGN => attrs.text_align = Some(decode_text_align(cursor)?),
        TAG_MOVE_X => attrs.move_x = Some(cursor.read_f64()?),
        TAG_MOVE_Y => attrs.move_y = Some(cursor.read_f64()?),
        TAG_ROTATE => attrs.rotate = Some(cursor.read_f64()?),
        TAG_SCALE => attrs.scale = Some(cursor.read_f64()?),
        TAG_ALPHA => attrs.alpha = Some(cursor.read_f64()?),
        TAG_SPACING_XY => {
            attrs.spacing_x = Some(cursor.read_f64()?);
            attrs.spacing_y = Some(cursor.read_f64()?);
        }
        TAG_SPACE_EVENLY => attrs.space_evenly = Some(cursor.read_bool()?),
        TAG_SCROLL_X => attrs.scroll_x = Some(cursor.read_f64()?),
        TAG_SCROLL_Y => attrs.scroll_y = Some(cursor.read_f64()?),
        TAG_ON_CLICK => attrs.on_click = Some(cursor.read_bool()?),
        TAG_ON_MOUSE_DOWN => attrs.on_mouse_down = Some(cursor.read_bool()?),
        TAG_ON_MOUSE_UP => attrs.on_mouse_up = Some(cursor.read_bool()?),
        TAG_ON_MOUSE_ENTER => attrs.on_mouse_enter = Some(cursor.read_bool()?),
        TAG_ON_MOUSE_LEAVE => attrs.on_mouse_leave = Some(cursor.read_bool()?),
        TAG_ON_MOUSE_MOVE => attrs.on_mouse_move = Some(cursor.read_bool()?),
        _ => {
            return Err(DecodeError::InvalidStructure(format!(
                "unknown attribute tag: {}",
                tag
            )));
        }
    }
    Ok(())
}

fn decode_length(cursor: &mut AttrCursor) -> Result<Length, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(Length::Fill),
        1 => Ok(Length::Content),
        2 => Ok(Length::Px(cursor.read_f64()?)),
        3 => Ok(Length::FillPortion(cursor.read_f64()?)),
        4 => {
            // Minimum: min_px (f64) + inner length
            let min_px = cursor.read_f64()?;
            let inner = decode_length(cursor)?;
            Ok(Length::Minimum(min_px, Box::new(inner)))
        }
        5 => {
            // Maximum: max_px (f64) + inner length
            let max_px = cursor.read_f64()?;
            let inner = decode_length(cursor)?;
            Ok(Length::Maximum(max_px, Box::new(inner)))
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown length variant: {}",
            variant
        ))),
    }
}

fn decode_padding(cursor: &mut AttrCursor) -> Result<Padding, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(Padding::Uniform(cursor.read_f64()?)),
        1 | 2 => {
            // Both tuple and map forms encode 4 f64s
            let top = cursor.read_f64()?;
            let right = cursor.read_f64()?;
            let bottom = cursor.read_f64()?;
            let left = cursor.read_f64()?;
            Ok(Padding::Sides {
                top,
                right,
                bottom,
                left,
            })
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown padding variant: {}",
            variant
        ))),
    }
}

fn decode_radius(cursor: &mut AttrCursor) -> Result<BorderRadius, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(BorderRadius::Uniform(cursor.read_f64()?)),
        1 => {
            let tl = cursor.read_f64()?;
            let tr = cursor.read_f64()?;
            let br = cursor.read_f64()?;
            let bl = cursor.read_f64()?;
            Ok(BorderRadius::Corners { tl, tr, br, bl })
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown border_radius variant: {}",
            variant
        ))),
    }
}

fn decode_align_x(cursor: &mut AttrCursor) -> Result<AlignX, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(AlignX::Left),
        1 => Ok(AlignX::Center),
        2 => Ok(AlignX::Right),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown align_x variant: {}",
            variant
        ))),
    }
}

fn decode_align_y(cursor: &mut AttrCursor) -> Result<AlignY, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(AlignY::Top),
        1 => Ok(AlignY::Center),
        2 => Ok(AlignY::Bottom),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown align_y variant: {}",
            variant
        ))),
    }
}

fn decode_text_align(cursor: &mut AttrCursor) -> Result<TextAlign, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(TextAlign::Left),
        1 => Ok(TextAlign::Center),
        2 => Ok(TextAlign::Right),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown text_align variant: {}",
            variant
        ))),
    }
}

fn decode_color(cursor: &mut AttrCursor) -> Result<Color, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => {
            let r = cursor.read_u8()?;
            let g = cursor.read_u8()?;
            let b = cursor.read_u8()?;
            Ok(Color::Rgb { r, g, b })
        }
        1 => {
            let r = cursor.read_u8()?;
            let g = cursor.read_u8()?;
            let b = cursor.read_u8()?;
            let a = cursor.read_u8()?;
            Ok(Color::Rgba { r, g, b, a })
        }
        2 => {
            let name = cursor.read_string_u16()?;
            Ok(Color::Named(name))
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown color variant: {}",
            variant
        ))),
    }
}

fn decode_background(cursor: &mut AttrCursor) -> Result<Background, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(Background::Color(decode_color(cursor)?)),
        1 => {
            let from = decode_color(cursor)?;
            let to = decode_color(cursor)?;
            let angle = cursor.read_f64()?;
            Ok(Background::Gradient { from, to, angle })
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown background variant: {}",
            variant
        ))),
    }
}

fn decode_font(cursor: &mut AttrCursor) -> Result<Font, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(Font::Atom(cursor.read_string_u16()?)),
        1 => Ok(Font::String(cursor.read_string_u16()?)),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown font variant: {}",
            variant
        ))),
    }
}

fn decode_font_weight(cursor: &mut AttrCursor) -> Result<FontWeight, DecodeError> {
    Ok(FontWeight(cursor.read_string_u16()?))
}

fn decode_font_style(cursor: &mut AttrCursor) -> Result<FontStyle, DecodeError> {
    Ok(FontStyle(cursor.read_string_u16()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_empty_attrs() {
        let attrs = decode_attrs(&[]).unwrap();
        assert!(attrs.width.is_none());
        assert!(attrs.height.is_none());
    }

    #[test]
    fn test_decode_length_fill() {
        // 1 attr, tag=1 (width), variant=0 (fill)
        let data = [0, 1, 1, 0];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.width, Some(Length::Fill));
    }

    #[test]
    fn test_decode_length_px() {
        // 1 attr, tag=1 (width), variant=2 (px), f64=100.0
        let mut data = vec![0, 1, 1, 2];
        data.extend_from_slice(&100.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.width, Some(Length::Px(100.0)));
    }

    #[test]
    fn test_decode_padding_uniform() {
        // 1 attr, tag=3 (padding), variant=0 (uniform), f64=10.0
        let mut data = vec![0, 1, 3, 0];
        data.extend_from_slice(&10.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.padding, Some(Padding::Uniform(10.0)));
    }

    #[test]
    fn test_decode_border_radius_corners() {
        // 1 attr, tag=13 (border_radius), variant=1 (corners)
        let mut data = vec![0, 1, 13, 1];
        data.extend_from_slice(&4.0_f64.to_be_bytes());
        data.extend_from_slice(&6.0_f64.to_be_bytes());
        data.extend_from_slice(&8.0_f64.to_be_bytes());
        data.extend_from_slice(&10.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(
            attrs.border_radius,
            Some(BorderRadius::Corners {
                tl: 4.0,
                tr: 6.0,
                br: 8.0,
                bl: 10.0,
            })
        );
    }

    #[test]
    fn test_decode_align() {
        // 2 attrs: align_x=center, align_y=bottom
        let data = [0, 2, 5, 1, 6, 2];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.align_x, Some(AlignX::Center));
        assert_eq!(attrs.align_y, Some(AlignY::Bottom));
    }

    #[test]
    fn test_decode_color_rgb() {
        // 1 attr, tag=17 (font_color), variant=0 (rgb), r=255, g=128, b=64
        let data = [0, 1, 17, 0, 255, 128, 64];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(
            attrs.font_color,
            Some(Color::Rgb {
                r: 255,
                g: 128,
                b: 64
            })
        );
    }

    #[test]
    fn test_decode_color_rgba() {
        // 1 attr, tag=15 (border_color), variant=1 (rgba), r=255, g=128, b=64, a=200
        let data = [0, 1, 15, 1, 255, 128, 64, 200];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(
            attrs.border_color,
            Some(Color::Rgba {
                r: 255,
                g: 128,
                b: 64,
                a: 200
            })
        );
    }

    #[test]
    fn test_decode_content() {
        // 1 attr, tag=21 (content), len=5, "hello"
        let data = [0, 1, 21, 0, 5, b'h', b'e', b'l', b'l', b'o'];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.content, Some("hello".to_string()));
    }

    #[test]
    fn test_decode_bool() {
        // 1 attr: scrollbar_y=false
        let data = [0, 1, 7, 0];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.scrollbar_y, Some(false));
    }

    #[test]
    fn test_decode_spacing_xy() {
        // 1 attr, tag=36 (spacing_xy), x=10.0, y=20.0
        let mut data = vec![0, 1, 36];
        data.extend_from_slice(&10.0_f64.to_be_bytes());
        data.extend_from_slice(&20.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.spacing_x, Some(10.0));
        assert_eq!(attrs.spacing_y, Some(20.0));
    }

    #[test]
    fn test_decode_space_evenly() {
        // 1 attr, tag=37 (space_evenly), value=true
        let data = [0, 1, 37, 1];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.space_evenly, Some(true));
    }

    #[test]
    fn test_decode_scroll_offsets() {
        // 2 attrs: scroll_x=12.0, scroll_y=34.0
        let mut data = vec![0, 2, 38];
        data.extend_from_slice(&12.0_f64.to_be_bytes());
        data.push(39);
        data.extend_from_slice(&34.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.scroll_x, Some(12.0));
        assert_eq!(attrs.scroll_y, Some(34.0));
    }

    #[test]
    fn test_decode_on_click() {
        // 1 attr, tag=40 (on_click), value=true
        let data = [0, 1, 40, 1];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.on_click, Some(true));
    }

    #[test]
    fn test_decode_mouse_events() {
        let data = [0, 5, 41, 1, 42, 1, 43, 1, 44, 1, 45, 1];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.on_mouse_down, Some(true));
        assert_eq!(attrs.on_mouse_up, Some(true));
        assert_eq!(attrs.on_mouse_enter, Some(true));
        assert_eq!(attrs.on_mouse_leave, Some(true));
        assert_eq!(attrs.on_mouse_move, Some(true));
    }

    #[test]
    fn test_decode_move_x() {
        // 1 attr, tag=31 (move_x), f64=12.5
        let mut data = vec![0, 1, 31];
        data.extend_from_slice(&12.5_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.move_x, Some(12.5));
    }

    #[test]
    fn test_decode_move_y() {
        // 1 attr, tag=32 (move_y), f64=-8.0
        let mut data = vec![0, 1, 32];
        data.extend_from_slice(&(-8.0_f64).to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.move_y, Some(-8.0));
    }

    #[test]
    fn test_decode_rotate() {
        // 1 attr, tag=33 (rotate), f64=45.0
        let mut data = vec![0, 1, 33];
        data.extend_from_slice(&45.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.rotate, Some(45.0));
    }

    #[test]
    fn test_decode_scale() {
        // 1 attr, tag=34 (scale), f64=1.25
        let mut data = vec![0, 1, 34];
        data.extend_from_slice(&1.25_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.scale, Some(1.25));
    }

    #[test]
    fn test_decode_alpha() {
        // 1 attr, tag=35 (alpha), f64=0.5
        let mut data = vec![0, 1, 35];
        data.extend_from_slice(&0.5_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.alpha, Some(0.5));
    }
}
