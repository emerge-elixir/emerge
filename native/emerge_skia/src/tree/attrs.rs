//! Attribute types and decoding for EMRG v3 format.
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
    Image { source: ImageSource, fit: ImageFit },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageSource {
    Id(String),
    Logical(String),
    RuntimePath(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
    Repeat,
    RepeatX,
    RepeatY,
}

/// Border radius specification.
#[derive(Clone, Debug, PartialEq)]
pub enum BorderRadius {
    Uniform(f64),
    Corners { tl: f64, tr: f64, br: f64, bl: f64 },
}

/// Border width specification.
#[derive(Clone, Debug, PartialEq)]
pub enum BorderWidth {
    Uniform(f64),
    Sides {
        top: f64,
        right: f64,
        bottom: f64,
        left: f64,
    },
}

/// Border style specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

/// Box shadow specification.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
    pub size: f64,
    pub color: Color,
    pub inset: bool,
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

/// Runtime hover axis for scrollbar thumb styling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarHoverAxis {
    X,
    Y,
}

/// Decorative attributes to apply while mouse is over an element.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MouseOverAttrs {
    pub background: Option<Background>,
    pub border_color: Option<Color>,
    pub font_color: Option<Color>,
    pub font_size: Option<f64>,
    pub font_underline: Option<bool>,
    pub font_strike: Option<bool>,
    pub font_letter_spacing: Option<f64>,
    pub font_word_spacing: Option<f64>,
    pub move_x: Option<f64>,
    pub move_y: Option<f64>,
    pub rotate: Option<f64>,
    pub scale: Option<f64>,
    pub alpha: Option<f64>,
}

/// A positioned text fragment within a paragraph, computed during layout.
#[derive(Clone, Debug)]
pub struct TextFragment {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub font_size: f32,
    pub color: u32,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub ascent: f32,
}

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
    pub scrollbar_hover_axis: Option<ScrollbarHoverAxis>,
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
    pub on_change: Option<bool>,
    pub on_focus: Option<bool>,
    pub on_blur: Option<bool>,
    pub mouse_over: Option<MouseOverAttrs>,
    pub focused: Option<MouseOverAttrs>,
    pub mouse_down: Option<MouseOverAttrs>,
    pub mouse_over_active: Option<bool>,
    pub mouse_down_active: Option<bool>,
    pub text_input_focused: Option<bool>,
    pub text_input_cursor: Option<u32>,
    pub text_input_selection_anchor: Option<u32>,
    pub text_input_preedit: Option<String>,
    pub text_input_preedit_cursor: Option<(u32, u32)>,
    pub clip_y: Option<bool>,
    pub clip_x: Option<bool>,
    pub background: Option<Background>,
    pub border_radius: Option<BorderRadius>,
    pub border_width: Option<BorderWidth>,
    pub border_style: Option<BorderStyle>,
    pub border_color: Option<Color>,
    pub box_shadows: Option<Vec<BoxShadow>>,
    pub font_size: Option<f64>,
    pub font_color: Option<Color>,
    pub font: Option<Font>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_underline: Option<bool>,
    pub font_strike: Option<bool>,
    pub font_letter_spacing: Option<f64>,
    pub font_word_spacing: Option<f64>,
    pub image_src: Option<ImageSource>,
    pub image_fit: Option<ImageFit>,
    pub image_size: Option<(f64, f64)>,
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
    /// Runtime-only: computed paragraph text fragments (not decoded from binary).
    pub paragraph_fragments: Option<Vec<TextFragment>>,
    // Nearby elements stored as raw EMRG bytes (decoded lazily)
    pub above: Option<Vec<u8>>,
    pub below: Option<Vec<u8>>,
    pub on_left: Option<Vec<u8>>,
    pub on_right: Option<Vec<u8>>,
    pub in_front: Option<Vec<u8>>,
    pub behind: Option<Vec<u8>>,
}

/// Preserve runtime-only fields across attr replacement.
pub fn preserve_runtime_scroll_attrs(existing: &Attrs, incoming: &mut Attrs) {
    let content_changed = incoming.content != existing.content;

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
    if incoming.scrollbar_hover_axis.is_none() {
        incoming.scrollbar_hover_axis = existing.scrollbar_hover_axis;
    }
    if incoming.mouse_over_active.is_none() {
        incoming.mouse_over_active = existing.mouse_over_active;
    }
    if incoming.mouse_down_active.is_none() {
        incoming.mouse_down_active = existing.mouse_down_active;
    }
    if incoming.text_input_focused.is_none() {
        incoming.text_input_focused = existing.text_input_focused;
    }
    if incoming.text_input_cursor.is_none() {
        incoming.text_input_cursor = existing.text_input_cursor;
    }
    if incoming.text_input_selection_anchor.is_none() {
        incoming.text_input_selection_anchor = existing.text_input_selection_anchor;
    }

    if content_changed {
        incoming.text_input_selection_anchor = None;
        incoming.text_input_preedit = None;
        incoming.text_input_preedit_cursor = None;
    } else {
        if incoming.text_input_preedit.is_none() {
            incoming.text_input_preedit = existing.text_input_preedit.clone();
        }
        if incoming.text_input_preedit_cursor.is_none() {
            incoming.text_input_preedit_cursor = existing.text_input_preedit_cursor;
        }
    }

    if incoming.mouse_over.is_none() {
        incoming.mouse_over_active = None;
    }

    if incoming.mouse_down.is_none() {
        incoming.mouse_down_active = None;
    }

    normalize_scrollbar_hover_axis(incoming);
    normalize_text_input_runtime(incoming);
}

fn normalize_scrollbar_hover_axis(attrs: &mut Attrs) {
    match attrs.scrollbar_hover_axis {
        Some(ScrollbarHoverAxis::X) if !attrs.scrollbar_x.unwrap_or(false) => {
            attrs.scrollbar_hover_axis = None;
        }
        Some(ScrollbarHoverAxis::Y) if !attrs.scrollbar_y.unwrap_or(false) => {
            attrs.scrollbar_hover_axis = None;
        }
        _ => {}
    }
}

fn normalize_text_input_runtime(attrs: &mut Attrs) {
    let content_len = attrs
        .content
        .as_ref()
        .map(|content| content.chars().count() as u32)
        .unwrap_or(0);

    if let Some(cursor) = attrs.text_input_cursor {
        attrs.text_input_cursor = Some(cursor.min(content_len));
    } else if attrs.text_input_focused.unwrap_or(false) {
        attrs.text_input_cursor = Some(content_len);
    }

    if let Some(anchor) = attrs.text_input_selection_anchor {
        let clamped_anchor = anchor.min(content_len);
        let cursor = attrs.text_input_cursor.unwrap_or(content_len);
        if clamped_anchor == cursor {
            attrs.text_input_selection_anchor = None;
        } else {
            attrs.text_input_selection_anchor = Some(clamped_anchor);
        }
    }

    if attrs.text_input_preedit.is_none() {
        attrs.text_input_preedit_cursor = None;
    } else if let Some((start, end)) = attrs.text_input_preedit_cursor {
        let preedit_len = attrs
            .text_input_preedit
            .as_ref()
            .map(|value| value.chars().count() as u32)
            .unwrap_or(0);
        let mut start = start.min(preedit_len);
        let mut end = end.min(preedit_len);
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        attrs.text_input_preedit_cursor = Some((start, end));
    }
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
const TAG_MOUSE_OVER: u8 = 46;
const TAG_FONT_UNDERLINE: u8 = 47;
const TAG_FONT_STRIKE: u8 = 48;
const TAG_FONT_LETTER_SPACING: u8 = 49;
const TAG_FONT_WORD_SPACING: u8 = 50;
const TAG_BORDER_STYLE: u8 = 51;
const TAG_BOX_SHADOW: u8 = 52;
const TAG_IMAGE_SRC: u8 = 53;
const TAG_IMAGE_FIT: u8 = 54;
const TAG_IMAGE_SIZE: u8 = 55;
const TAG_ON_CHANGE: u8 = 56;
const TAG_ON_FOCUS: u8 = 57;
const TAG_ON_BLUR: u8 = 58;
const TAG_FOCUSED: u8 = 59;
const TAG_MOUSE_DOWN_STYLE: u8 = 60;

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
        TAG_BORDER_WIDTH => attrs.border_width = Some(decode_border_width(cursor)?),
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
        TAG_ON_CHANGE => attrs.on_change = Some(cursor.read_bool()?),
        TAG_ON_FOCUS => attrs.on_focus = Some(cursor.read_bool()?),
        TAG_ON_BLUR => attrs.on_blur = Some(cursor.read_bool()?),
        TAG_MOUSE_OVER => {
            attrs.mouse_over = Some(decode_decorative_style_attrs(cursor, "mouse_over")?)
        }
        TAG_FOCUSED => attrs.focused = Some(decode_decorative_style_attrs(cursor, "focused")?),
        TAG_MOUSE_DOWN_STYLE => {
            attrs.mouse_down = Some(decode_decorative_style_attrs(cursor, "mouse_down")?)
        }
        TAG_FONT_UNDERLINE => attrs.font_underline = Some(cursor.read_bool()?),
        TAG_FONT_STRIKE => attrs.font_strike = Some(cursor.read_bool()?),
        TAG_FONT_LETTER_SPACING => attrs.font_letter_spacing = Some(cursor.read_f64()?),
        TAG_FONT_WORD_SPACING => attrs.font_word_spacing = Some(cursor.read_f64()?),
        TAG_BORDER_STYLE => attrs.border_style = Some(decode_border_style(cursor)?),
        TAG_BOX_SHADOW => attrs.box_shadows = Some(decode_box_shadows(cursor)?),
        TAG_IMAGE_SRC => attrs.image_src = Some(decode_image_source(cursor)?),
        TAG_IMAGE_FIT => attrs.image_fit = Some(decode_image_fit(cursor)?),
        TAG_IMAGE_SIZE => {
            let width = cursor.read_f64()?;
            let height = cursor.read_f64()?;
            attrs.image_size = Some((width, height));
        }
        _ => {
            return Err(DecodeError::InvalidStructure(format!(
                "unknown attribute tag: {}",
                tag
            )));
        }
    }
    Ok(())
}

fn decode_decorative_style_attrs(
    cursor: &mut AttrCursor,
    style_name: &str,
) -> Result<MouseOverAttrs, DecodeError> {
    let data = cursor.read_bytes_u32()?;
    let mut nested = AttrCursor::new(&data);
    let mut out = MouseOverAttrs::default();

    if nested.remaining() == 0 {
        return Ok(out);
    }

    let attr_count = nested.read_u16_be()? as usize;

    for _ in 0..attr_count {
        let tag = nested.read_u8()?;
        match tag {
            TAG_BACKGROUND => out.background = Some(decode_background(&mut nested)?),
            TAG_BORDER_COLOR => out.border_color = Some(decode_color(&mut nested)?),
            TAG_FONT_COLOR => out.font_color = Some(decode_color(&mut nested)?),
            TAG_FONT_SIZE => out.font_size = Some(nested.read_f64()?),
            TAG_FONT_UNDERLINE => out.font_underline = Some(nested.read_bool()?),
            TAG_FONT_STRIKE => out.font_strike = Some(nested.read_bool()?),
            TAG_FONT_LETTER_SPACING => out.font_letter_spacing = Some(nested.read_f64()?),
            TAG_FONT_WORD_SPACING => out.font_word_spacing = Some(nested.read_f64()?),
            TAG_MOVE_X => out.move_x = Some(nested.read_f64()?),
            TAG_MOVE_Y => out.move_y = Some(nested.read_f64()?),
            TAG_ROTATE => out.rotate = Some(nested.read_f64()?),
            TAG_SCALE => out.scale = Some(nested.read_f64()?),
            TAG_ALPHA => out.alpha = Some(nested.read_f64()?),
            _ => {
                return Err(DecodeError::InvalidStructure(format!(
                    "{} supports decorative attrs only, got tag: {}",
                    style_name, tag
                )));
            }
        }
    }

    if nested.remaining() != 0 {
        return Err(DecodeError::InvalidStructure(format!(
            "{} has {} trailing bytes",
            style_name,
            nested.remaining(),
        )));
    }

    Ok(out)
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

fn decode_border_width(cursor: &mut AttrCursor) -> Result<BorderWidth, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(BorderWidth::Uniform(cursor.read_f64()?)),
        1 => {
            let top = cursor.read_f64()?;
            let right = cursor.read_f64()?;
            let bottom = cursor.read_f64()?;
            let left = cursor.read_f64()?;
            Ok(BorderWidth::Sides {
                top,
                right,
                bottom,
                left,
            })
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown border_width variant: {}",
            variant
        ))),
    }
}

fn decode_border_style(cursor: &mut AttrCursor) -> Result<BorderStyle, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(BorderStyle::Solid),
        1 => Ok(BorderStyle::Dashed),
        2 => Ok(BorderStyle::Dotted),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown border_style variant: {}",
            variant
        ))),
    }
}

fn decode_box_shadows(cursor: &mut AttrCursor) -> Result<Vec<BoxShadow>, DecodeError> {
    let count = cursor.read_u8()? as usize;
    let mut shadows = Vec::with_capacity(count);
    for _ in 0..count {
        let offset_x = cursor.read_f64()?;
        let offset_y = cursor.read_f64()?;
        let blur = cursor.read_f64()?;
        let size = cursor.read_f64()?;
        let color = decode_color(cursor)?;
        let inset = cursor.read_bool()?;
        shadows.push(BoxShadow {
            offset_x,
            offset_y,
            blur,
            size,
            color,
            inset,
        });
    }
    Ok(shadows)
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
        2 => {
            let source = decode_image_source(cursor)?;
            let fit = decode_image_fit(cursor)?;
            Ok(Background::Image { source, fit })
        }
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown background variant: {}",
            variant
        ))),
    }
}

fn decode_image_source(cursor: &mut AttrCursor) -> Result<ImageSource, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(ImageSource::Id(cursor.read_string_u16()?)),
        1 => Ok(ImageSource::Logical(cursor.read_string_u16()?)),
        2 => Ok(ImageSource::RuntimePath(cursor.read_string_u16()?)),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown image source variant: {}",
            variant
        ))),
    }
}

fn decode_image_fit(cursor: &mut AttrCursor) -> Result<ImageFit, DecodeError> {
    let variant = cursor.read_u8()?;
    match variant {
        0 => Ok(ImageFit::Contain),
        1 => Ok(ImageFit::Cover),
        2 => Ok(ImageFit::Repeat),
        3 => Ok(ImageFit::RepeatX),
        4 => Ok(ImageFit::RepeatY),
        _ => Err(DecodeError::InvalidStructure(format!(
            "unknown image_fit variant: {}",
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
    fn test_decode_font_decoration_and_spacing() {
        // 4 attrs: underline=true, strike=true, letter_spacing=1.5, word_spacing=3.0
        let mut data = vec![0, 4, 47, 1, 48, 1, 49];
        data.extend_from_slice(&1.5_f64.to_be_bytes());
        data.push(50);
        data.extend_from_slice(&3.0_f64.to_be_bytes());

        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.font_underline, Some(true));
        assert_eq!(attrs.font_strike, Some(true));
        assert_eq!(attrs.font_letter_spacing, Some(1.5));
        assert_eq!(attrs.font_word_spacing, Some(3.0));
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

    #[test]
    fn test_decode_mouse_over_attrs() {
        // nested: attr_count=2, font_color=rgb(1,2,3), alpha=0.5
        let nested = vec![0, 2, 17, 0, 1, 2, 3, 35, 0x3F, 0xE0, 0, 0, 0, 0, 0, 0];
        let mut data = vec![0, 1, 46];
        data.extend_from_slice(&(nested.len() as u32).to_be_bytes());
        data.extend_from_slice(&nested);

        let attrs = decode_attrs(&data).unwrap();
        let mouse_over = attrs.mouse_over.unwrap();
        assert_eq!(mouse_over.alpha, Some(0.5));
        assert_eq!(mouse_over.font_color, Some(Color::Rgb { r: 1, g: 2, b: 3 }));
    }

    #[test]
    fn test_decode_mouse_over_font_decoration_and_spacing() {
        // nested: underline=true, strike=true, letter_spacing=2.0, word_spacing=4.0
        let mut nested = vec![0, 4, 47, 1, 48, 1, 49];
        nested.extend_from_slice(&2.0_f64.to_be_bytes());
        nested.push(50);
        nested.extend_from_slice(&4.0_f64.to_be_bytes());

        let mut data = vec![0, 1, 46];
        data.extend_from_slice(&(nested.len() as u32).to_be_bytes());
        data.extend_from_slice(&nested);

        let attrs = decode_attrs(&data).unwrap();
        let mouse_over = attrs.mouse_over.unwrap();
        assert_eq!(mouse_over.font_underline, Some(true));
        assert_eq!(mouse_over.font_strike, Some(true));
        assert_eq!(mouse_over.font_letter_spacing, Some(2.0));
        assert_eq!(mouse_over.font_word_spacing, Some(4.0));
    }

    #[test]
    fn test_decode_mouse_over_rejects_non_decorative_tag() {
        // nested: attr_count=1, width=fill (tag 1) -> invalid in mouse_over
        let nested = vec![0, 1, 1, 0];
        let mut data = vec![0, 1, 46];
        data.extend_from_slice(&(nested.len() as u32).to_be_bytes());
        data.extend_from_slice(&nested);

        let err = decode_attrs(&data).unwrap_err();
        assert!(
            err.to_string()
                .contains("mouse_over supports decorative attrs only")
        );
    }

    #[test]
    fn test_decode_focused_and_mouse_down_styles() {
        let mut focused_nested = vec![0, 1, 35];
        focused_nested.extend_from_slice(&0.25_f64.to_be_bytes());

        let mut mouse_down_nested = vec![0, 1, 31];
        mouse_down_nested.extend_from_slice(&4.0_f64.to_be_bytes());

        let mut data = vec![0, 2, 59];
        data.extend_from_slice(&(focused_nested.len() as u32).to_be_bytes());
        data.extend_from_slice(&focused_nested);
        data.push(60);
        data.extend_from_slice(&(mouse_down_nested.len() as u32).to_be_bytes());
        data.extend_from_slice(&mouse_down_nested);

        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(
            attrs.focused.as_ref().and_then(|style| style.alpha),
            Some(0.25)
        );
        assert_eq!(
            attrs.mouse_down.as_ref().and_then(|style| style.move_x),
            Some(4.0)
        );
    }

    #[test]
    fn test_decode_focused_rejects_non_decorative_tag() {
        let nested = vec![0, 1, 1, 0];
        let mut data = vec![0, 1, 59];
        data.extend_from_slice(&(nested.len() as u32).to_be_bytes());
        data.extend_from_slice(&nested);

        let err = decode_attrs(&data).unwrap_err();
        assert!(
            err.to_string()
                .contains("focused supports decorative attrs only")
        );
    }

    #[test]
    fn test_decode_border_width_uniform() {
        // 1 attr, tag=14 (border_width), variant=0 (uniform), f64=3.0
        let mut data = vec![0, 1, 14, 0];
        data.extend_from_slice(&3.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.border_width, Some(BorderWidth::Uniform(3.0)));
    }

    #[test]
    fn test_decode_on_change() {
        let data = [0, 1, 56, 1];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.on_change, Some(true));
    }

    #[test]
    fn test_decode_focus_events() {
        let data = [0, 2, 57, 1, 58, 1];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.on_focus, Some(true));
        assert_eq!(attrs.on_blur, Some(true));
    }

    #[test]
    fn test_decode_border_width_sides() {
        // 1 attr, tag=14 (border_width), variant=1 (sides)
        let mut data = vec![0, 1, 14, 1];
        data.extend_from_slice(&1.0_f64.to_be_bytes());
        data.extend_from_slice(&2.0_f64.to_be_bytes());
        data.extend_from_slice(&3.0_f64.to_be_bytes());
        data.extend_from_slice(&4.0_f64.to_be_bytes());
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(
            attrs.border_width,
            Some(BorderWidth::Sides {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            })
        );
    }

    #[test]
    fn test_decode_border_style_variants() {
        // Solid: tag=51, variant=0
        let data = [0, 1, 51, 0];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.border_style, Some(BorderStyle::Solid));

        // Dashed: tag=51, variant=1
        let data = [0, 1, 51, 1];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.border_style, Some(BorderStyle::Dashed));

        // Dotted: tag=51, variant=2
        let data = [0, 1, 51, 2];
        let attrs = decode_attrs(&data).unwrap();
        assert_eq!(attrs.border_style, Some(BorderStyle::Dotted));
    }

    #[test]
    fn test_decode_box_shadows() {
        // 1 attr, tag=52, count=1, shadow fields
        let mut data = vec![0, 1, 52, 1]; // 1 attr, tag=52(box_shadow), count=1
        data.extend_from_slice(&2.0_f64.to_be_bytes()); // offset_x
        data.extend_from_slice(&3.0_f64.to_be_bytes()); // offset_y
        data.extend_from_slice(&8.0_f64.to_be_bytes()); // blur
        data.extend_from_slice(&4.0_f64.to_be_bytes()); // size
        // color: named "red" -> variant=2, len=3, "red"
        data.extend_from_slice(&[2, 0, 3, b'r', b'e', b'd']);
        data.push(0); // inset=false

        let attrs = decode_attrs(&data).unwrap();
        let shadows = attrs.box_shadows.unwrap();
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].offset_x, 2.0);
        assert_eq!(shadows[0].offset_y, 3.0);
        assert_eq!(shadows[0].blur, 8.0);
        assert_eq!(shadows[0].size, 4.0);
        assert_eq!(shadows[0].color, Color::Named("red".to_string()));
        assert!(!shadows[0].inset);
    }

    #[test]
    fn test_decode_box_shadows_multiple() {
        let mut data = vec![0, 1, 52, 2]; // 1 attr, tag=52, count=2

        // Shadow 1: outer shadow
        data.extend_from_slice(&1.0_f64.to_be_bytes());
        data.extend_from_slice(&1.0_f64.to_be_bytes());
        data.extend_from_slice(&4.0_f64.to_be_bytes());
        data.extend_from_slice(&0.0_f64.to_be_bytes());
        data.extend_from_slice(&[0, 0, 0, 0]); // rgb black
        data.push(0); // inset=false

        // Shadow 2: inset shadow
        data.extend_from_slice(&0.0_f64.to_be_bytes());
        data.extend_from_slice(&0.0_f64.to_be_bytes());
        data.extend_from_slice(&10.0_f64.to_be_bytes());
        data.extend_from_slice(&5.0_f64.to_be_bytes());
        data.extend_from_slice(&[0, 0, 0, 255]); // rgb blue
        data.push(1); // inset=true

        let attrs = decode_attrs(&data).unwrap();
        let shadows = attrs.box_shadows.unwrap();
        assert_eq!(shadows.len(), 2);
        assert!(!shadows[0].inset);
        assert!(shadows[1].inset);
        assert_eq!(shadows[1].size, 5.0);
    }

    #[test]
    fn test_decode_box_shadow_inset() {
        let mut data = vec![0, 1, 52, 1]; // 1 attr, tag=52, count=1
        data.extend_from_slice(&0.0_f64.to_be_bytes());
        data.extend_from_slice(&0.0_f64.to_be_bytes());
        data.extend_from_slice(&10.0_f64.to_be_bytes());
        data.extend_from_slice(&0.0_f64.to_be_bytes());
        data.extend_from_slice(&[2, 0, 5, b'b', b'l', b'a', b'c', b'k']); // named "black"
        data.push(1); // inset=true

        let attrs = decode_attrs(&data).unwrap();
        let shadows = attrs.box_shadows.unwrap();
        assert!(shadows[0].inset);
    }

    #[test]
    fn test_decode_image_source_variants() {
        let mut image_src = vec![0, 1, 53, 2, 0, 7];
        image_src.extend_from_slice(b"a/b.png");

        let attrs = decode_attrs(&image_src).unwrap();
        assert_eq!(
            attrs.image_src,
            Some(ImageSource::RuntimePath("a/b.png".to_string()))
        );

        let mut background = vec![0, 1, 12, 2, 0, 0, 13];
        background.extend_from_slice(b"img_preloaded");
        background.push(1);

        let attrs = decode_attrs(&background).unwrap();
        assert_eq!(
            attrs.background,
            Some(Background::Image {
                source: ImageSource::Id("img_preloaded".to_string()),
                fit: ImageFit::Cover,
            })
        );

        for (fit_variant, expected_fit) in [
            (2_u8, ImageFit::Repeat),
            (3_u8, ImageFit::RepeatX),
            (4_u8, ImageFit::RepeatY),
        ] {
            let mut tiled_bg = vec![0, 1, 12, 2, 0, 0, 13];
            tiled_bg.extend_from_slice(b"img_preloaded");
            tiled_bg.push(fit_variant);

            let attrs = decode_attrs(&tiled_bg).unwrap();
            assert_eq!(
                attrs.background,
                Some(Background::Image {
                    source: ImageSource::Id("img_preloaded".to_string()),
                    fit: expected_fit,
                })
            );
        }
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_copies_missing_values() {
        let mut existing = Attrs::default();
        existing.scroll_x = Some(11.0);
        existing.scroll_y = Some(22.0);
        existing.scroll_x_max = Some(110.0);
        existing.scroll_y_max = Some(220.0);
        existing.scrollbar_hover_axis = Some(ScrollbarHoverAxis::Y);

        let mut incoming = Attrs::default();
        incoming.scrollbar_x = Some(true);
        incoming.scrollbar_y = Some(true);

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.scroll_x, Some(11.0));
        assert_eq!(incoming.scroll_y, Some(22.0));
        assert_eq!(incoming.scroll_x_max, Some(110.0));
        assert_eq!(incoming.scroll_y_max, Some(220.0));
        assert_eq!(incoming.scrollbar_hover_axis, Some(ScrollbarHoverAxis::Y));
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_keeps_incoming_values() {
        let mut existing = Attrs::default();
        existing.scroll_x = Some(11.0);
        existing.scroll_y = Some(22.0);
        existing.scroll_x_max = Some(110.0);
        existing.scroll_y_max = Some(220.0);
        existing.scrollbar_hover_axis = Some(ScrollbarHoverAxis::X);

        let mut incoming = Attrs::default();
        incoming.scrollbar_x = Some(true);
        incoming.scrollbar_y = Some(true);
        incoming.scroll_x = Some(1.0);
        incoming.scroll_y = Some(2.0);
        incoming.scroll_x_max = Some(3.0);
        incoming.scroll_y_max = Some(4.0);
        incoming.scrollbar_hover_axis = Some(ScrollbarHoverAxis::Y);

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.scroll_x, Some(1.0));
        assert_eq!(incoming.scroll_y, Some(2.0));
        assert_eq!(incoming.scroll_x_max, Some(3.0));
        assert_eq!(incoming.scroll_y_max, Some(4.0));
        assert_eq!(incoming.scrollbar_hover_axis, Some(ScrollbarHoverAxis::Y));
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_clears_invalid_hover_axis() {
        let mut existing = Attrs::default();
        existing.scrollbar_hover_axis = Some(ScrollbarHoverAxis::X);

        let mut incoming = Attrs::default();
        incoming.scrollbar_x = Some(false);
        incoming.scrollbar_y = Some(true);

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.scrollbar_hover_axis, None);
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_is_idempotent() {
        let mut existing = Attrs::default();
        existing.scroll_x = Some(7.0);
        existing.scroll_y = Some(9.0);
        existing.scroll_x_max = Some(30.0);
        existing.scroll_y_max = Some(40.0);
        existing.scrollbar_hover_axis = Some(ScrollbarHoverAxis::Y);

        let mut incoming = Attrs::default();
        incoming.scrollbar_x = Some(true);
        incoming.scrollbar_y = Some(true);

        preserve_runtime_scroll_attrs(&existing, &mut incoming);
        let after_first = (
            incoming.scroll_x,
            incoming.scroll_y,
            incoming.scroll_x_max,
            incoming.scroll_y_max,
            incoming.scrollbar_hover_axis,
        );

        preserve_runtime_scroll_attrs(&existing, &mut incoming);
        let after_second = (
            incoming.scroll_x,
            incoming.scroll_y,
            incoming.scroll_x_max,
            incoming.scroll_y_max,
            incoming.scrollbar_hover_axis,
        );

        assert_eq!(after_first, after_second);
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_preserves_mouse_over_active() {
        let mut existing = Attrs::default();
        existing.mouse_over = Some(MouseOverAttrs {
            alpha: Some(0.5),
            ..Default::default()
        });
        existing.mouse_over_active = Some(true);

        let mut incoming = Attrs::default();
        incoming.mouse_over = Some(MouseOverAttrs {
            alpha: Some(0.5),
            ..Default::default()
        });

        preserve_runtime_scroll_attrs(&existing, &mut incoming);
        assert_eq!(incoming.mouse_over_active, Some(true));
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_clears_mouse_over_active_without_mouse_over() {
        let mut existing = Attrs::default();
        existing.mouse_over = Some(MouseOverAttrs {
            alpha: Some(0.5),
            ..Default::default()
        });
        existing.mouse_over_active = Some(true);

        let mut incoming = Attrs::default();
        preserve_runtime_scroll_attrs(&existing, &mut incoming);
        assert_eq!(incoming.mouse_over_active, None);
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_preserves_mouse_down_active() {
        let mut existing = Attrs::default();
        existing.mouse_down = Some(MouseOverAttrs {
            alpha: Some(0.5),
            ..Default::default()
        });
        existing.mouse_down_active = Some(true);

        let mut incoming = Attrs::default();
        incoming.mouse_down = Some(MouseOverAttrs {
            alpha: Some(0.5),
            ..Default::default()
        });

        preserve_runtime_scroll_attrs(&existing, &mut incoming);
        assert_eq!(incoming.mouse_down_active, Some(true));
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_clears_mouse_down_active_without_mouse_down() {
        let mut existing = Attrs::default();
        existing.mouse_down = Some(MouseOverAttrs {
            alpha: Some(0.5),
            ..Default::default()
        });
        existing.mouse_down_active = Some(true);

        let mut incoming = Attrs::default();
        preserve_runtime_scroll_attrs(&existing, &mut incoming);
        assert_eq!(incoming.mouse_down_active, None);
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_preserves_text_input_runtime_state() {
        let mut existing = Attrs::default();
        existing.content = Some("hello".to_string());
        existing.text_input_focused = Some(true);
        existing.text_input_cursor = Some(4);
        existing.text_input_selection_anchor = Some(1);

        let mut incoming = Attrs::default();
        incoming.content = Some("hello".to_string());

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.text_input_focused, Some(true));
        assert_eq!(incoming.text_input_cursor, Some(4));
        assert_eq!(incoming.text_input_selection_anchor, Some(1));
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_clears_preedit_when_content_changes() {
        let mut existing = Attrs::default();
        existing.content = Some("hello".to_string());
        existing.text_input_selection_anchor = Some(1);
        existing.text_input_preedit = Some("ka".to_string());
        existing.text_input_preedit_cursor = Some((2, 2));

        let mut incoming = Attrs::default();
        incoming.content = Some("world".to_string());

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.text_input_selection_anchor, None);
        assert_eq!(incoming.text_input_preedit, None);
        assert_eq!(incoming.text_input_preedit_cursor, None);
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_clamps_and_orders_preedit_cursor() {
        let mut existing = Attrs::default();
        existing.content = Some("hello".to_string());
        existing.text_input_preedit = Some("abc".to_string());
        existing.text_input_preedit_cursor = Some((5, 1));

        let mut incoming = Attrs::default();
        incoming.content = Some("hello".to_string());

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.text_input_preedit.as_deref(), Some("abc"));
        assert_eq!(incoming.text_input_preedit_cursor, Some((1, 3)));
    }

    #[test]
    fn test_preserve_runtime_scroll_attrs_clears_selection_when_anchor_equals_cursor() {
        let mut existing = Attrs::default();
        existing.content = Some("abc".to_string());
        existing.text_input_cursor = Some(2);
        existing.text_input_selection_anchor = Some(2);

        let mut incoming = Attrs::default();
        incoming.content = Some("abc".to_string());

        preserve_runtime_scroll_attrs(&existing, &mut incoming);

        assert_eq!(incoming.text_input_cursor, Some(2));
        assert_eq!(incoming.text_input_selection_anchor, None);
    }
}
