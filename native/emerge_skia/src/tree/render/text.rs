use super::box_model::content_insets;
use super::color::{DEFAULT_TEXT_COLOR, color_to_u32};
use super::scope::RenderItem;
use crate::renderer::{DrawCmd, make_font_with_style};
use crate::tree::attrs::{Attrs, TextAlign};
use crate::tree::element::Frame;
use crate::tree::layout::{FontContext, font_info_with_inheritance};

pub(super) const TEXT_SELECTION_COLOR: u32 = 0x4A90E266;

pub(super) fn render_text_items(
    frame: Frame,
    attrs: &Attrs,
    inherited: &FontContext,
) -> Vec<RenderItem> {
    let Some(content) = attrs.content.as_deref() else {
        return Vec::new();
    };

    // Use inherited font context for missing values
    let font_size = attrs
        .font_size
        .map(|s| s as f32)
        .or(inherited.font_size)
        .unwrap_or(16.0);
    let color = attrs
        .font_color
        .as_ref()
        .map(color_to_u32)
        .or(inherited.font_color)
        .unwrap_or(DEFAULT_TEXT_COLOR);
    let underline = attrs
        .font_underline
        .or(inherited.font_underline)
        .unwrap_or(false);
    let strike = attrs.font_strike.or(inherited.font_strike).unwrap_or(false);
    let letter_spacing = attrs
        .font_letter_spacing
        .map(|v| v as f32)
        .or(inherited.font_letter_spacing)
        .unwrap_or(0.0);
    let word_spacing = attrs
        .font_word_spacing
        .map(|v| v as f32)
        .or(inherited.font_word_spacing)
        .unwrap_or(0.0);
    let (family, weight, italic) = font_info_with_inheritance(attrs, inherited);
    let insets = content_insets(attrs);
    let inset_left = insets.left;
    let inset_top = insets.top;
    let inset_right = insets.right;
    let (ascent, _) = text_metrics_with_font(font_size, &family, weight, italic);
    let text_width = measure_text_width_with_font(
        content,
        font_size,
        &family,
        weight,
        italic,
        letter_spacing,
        word_spacing,
    );
    let content_width = frame.width - inset_left - inset_right;
    let text_align = attrs
        .text_align
        .or(inherited.text_align)
        .unwrap_or_default();
    let text_x = match text_align {
        TextAlign::Left => frame.x + inset_left,
        TextAlign::Center => frame.x + inset_left + (content_width - text_width) / 2.0,
        TextAlign::Right => frame.x + frame.width - inset_right - text_width,
    };
    let baseline_y = frame.y + inset_top + ascent;

    let mut items = text_run_items(
        text_x,
        baseline_y,
        content,
        font_size,
        color,
        &family,
        weight,
        italic,
        letter_spacing,
        word_spacing,
    );
    items.extend(text_decoration_items(TextDecorationSpec {
        x: text_x,
        baseline_y,
        width: text_width,
        font_size,
        color,
        underline,
        strike,
    }));
    items
}

pub(super) fn render_text_input_items(
    items: &mut Vec<RenderItem>,
    frame: Frame,
    attrs: &Attrs,
    inherited: &FontContext,
) -> Option<(f32, f32, f32, f32)> {
    let content = attrs.content.as_deref().unwrap_or("");
    let preedit = attrs
        .text_input_preedit
        .as_deref()
        .filter(|value| !value.is_empty());

    let font_size = attrs
        .font_size
        .map(|s| s as f32)
        .or(inherited.font_size)
        .unwrap_or(16.0);
    let color = attrs
        .font_color
        .as_ref()
        .map(color_to_u32)
        .or(inherited.font_color)
        .unwrap_or(DEFAULT_TEXT_COLOR);
    let underline = attrs
        .font_underline
        .or(inherited.font_underline)
        .unwrap_or(false);
    let strike = attrs.font_strike.or(inherited.font_strike).unwrap_or(false);
    let letter_spacing = attrs
        .font_letter_spacing
        .map(|v| v as f32)
        .or(inherited.font_letter_spacing)
        .unwrap_or(0.0);
    let word_spacing = attrs
        .font_word_spacing
        .map(|v| v as f32)
        .or(inherited.font_word_spacing)
        .unwrap_or(0.0);
    let (family, weight, italic) = font_info_with_inheritance(attrs, inherited);
    let insets = content_insets(attrs);
    let inset_left = insets.left;
    let inset_top = insets.top;
    let inset_right = insets.right;
    let (ascent, descent) = text_metrics_with_font(font_size, &family, weight, italic);
    let content_char_count = content.chars().count() as u32;
    let base_cursor = attrs
        .text_input_cursor
        .unwrap_or(content_char_count)
        .min(content_char_count);
    let prefix: String = content.chars().take(base_cursor as usize).collect();
    let suffix: String = content.chars().skip(base_cursor as usize).collect();
    let displayed_text = match preedit {
        Some(preedit_text) => {
            let mut value = String::with_capacity(content.len() + preedit_text.len());
            value.push_str(&prefix);
            value.push_str(preedit_text);
            value.push_str(&suffix);
            value
        }
        None => content.to_string(),
    };

    let text_width = measure_text_width_with_font(
        &displayed_text,
        font_size,
        &family,
        weight,
        italic,
        letter_spacing,
        word_spacing,
    );
    let content_width = frame.width - inset_left - inset_right;
    let text_align = attrs
        .text_align
        .or(inherited.text_align)
        .unwrap_or_default();
    let text_x = match text_align {
        TextAlign::Left => frame.x + inset_left,
        TextAlign::Center => frame.x + inset_left + (content_width - text_width) / 2.0,
        TextAlign::Right => frame.x + frame.width - inset_right - text_width,
    };
    let baseline_y = frame.y + inset_top + ascent;

    if let Some(anchor) = attrs.text_input_selection_anchor {
        let anchor = anchor.min(content_char_count);
        if anchor != base_cursor {
            let preedit_len = preedit
                .map(|value| value.chars().count() as u32)
                .unwrap_or(0);
            let map_committed_to_displayed = |index: u32| {
                if preedit_len > 0 && index > base_cursor {
                    index + preedit_len
                } else {
                    index
                }
            };

            let sel_start = anchor.min(base_cursor);
            let sel_end = anchor.max(base_cursor);
            let displayed_start = map_committed_to_displayed(sel_start);
            let displayed_end = map_committed_to_displayed(sel_end);

            let start_offset = text_offset_for_char_index(
                &displayed_text,
                displayed_start as usize,
                font_size,
                &family,
                weight,
                italic,
                letter_spacing,
                word_spacing,
            );
            let end_offset = text_offset_for_char_index(
                &displayed_text,
                displayed_end as usize,
                font_size,
                &family,
                weight,
                italic,
                letter_spacing,
                word_spacing,
            );

            let selection_width = (end_offset - start_offset).max(0.0);
            if selection_width > 0.0 {
                let selection_top = baseline_y - ascent;
                let selection_height = (ascent + descent).max(font_size * 0.9);
                items.push(RenderItem::Draw(DrawCmd::Rect(
                    text_x + start_offset,
                    selection_top,
                    selection_width,
                    selection_height,
                    TEXT_SELECTION_COLOR,
                )));
            }
        }
    }

    items.extend(text_run_items(
        text_x,
        baseline_y,
        &displayed_text,
        font_size,
        color,
        &family,
        weight,
        italic,
        letter_spacing,
        word_spacing,
    ));

    items.extend(text_decoration_items(TextDecorationSpec {
        x: text_x,
        baseline_y,
        width: text_width,
        font_size,
        color,
        underline,
        strike,
    }));

    if let Some(preedit_text) = preedit {
        let preedit_start_offset = text_offset_for_char_index(
            &displayed_text,
            base_cursor as usize,
            font_size,
            &family,
            weight,
            italic,
            letter_spacing,
            word_spacing,
        );
        let preedit_width = measure_text_width_with_font(
            preedit_text,
            font_size,
            &family,
            weight,
            italic,
            letter_spacing,
            word_spacing,
        );

        items.extend(text_decoration_items(TextDecorationSpec {
            x: text_x + preedit_start_offset,
            baseline_y,
            width: preedit_width,
            font_size,
            color,
            underline: true,
            strike: false,
        }));
    }

    if attrs.text_input_focused.unwrap_or(false) {
        let displayed_char_count = displayed_text.chars().count() as u32;
        let caret_char_index = if let Some(preedit_text) = preedit {
            let preedit_len = preedit_text.chars().count() as u32;
            let preedit_cursor_end = attrs
                .text_input_preedit_cursor
                .map(|(_start, end)| end.min(preedit_len))
                .unwrap_or(preedit_len);
            (base_cursor + preedit_cursor_end).min(displayed_char_count)
        } else {
            base_cursor.min(displayed_char_count)
        };

        let caret_offset = text_offset_for_char_index(
            &displayed_text,
            caret_char_index as usize,
            font_size,
            &family,
            weight,
            italic,
            letter_spacing,
            word_spacing,
        );
        let caret_x = text_x + caret_offset;
        let caret_top = baseline_y - ascent;
        let caret_height = (ascent + descent).max(font_size * 0.9);
        let caret_width = (font_size * 0.08).max(1.0);

        items.push(RenderItem::Draw(DrawCmd::Rect(
            caret_x,
            caret_top,
            caret_width,
            caret_height,
            color,
        )));

        return Some((caret_x, caret_top, caret_width, caret_height));
    }

    None
}

pub(super) fn text_run_items(
    x: f32,
    baseline_y: f32,
    text: &str,
    font_size: f32,
    color: u32,
    family: &str,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
) -> Vec<RenderItem> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();

    if letter_spacing == 0.0 && word_spacing == 0.0 {
        items.push(RenderItem::Draw(DrawCmd::TextWithFont(
            x,
            baseline_y,
            text.to_string(),
            font_size,
            color,
            family.to_string(),
            weight,
            italic,
        )));
        return items;
    }

    let measure_font = make_font_with_style(family, weight, italic, font_size);
    let mut cursor_x = x;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let glyph = ch.to_string();
        items.push(RenderItem::Draw(DrawCmd::TextWithFont(
            cursor_x,
            baseline_y,
            glyph.clone(),
            font_size,
            color,
            family.to_string(),
            weight,
            italic,
        )));

        let (glyph_width, _bounds) = measure_font.measure_str(&glyph, None);
        cursor_x += glyph_width;

        if chars.peek().is_some() {
            cursor_x += letter_spacing;
            if ch.is_whitespace() {
                cursor_x += word_spacing;
            }
        }
    }

    items
}

pub(super) fn text_decoration_items(spec: TextDecorationSpec) -> Vec<RenderItem> {
    let TextDecorationSpec {
        x,
        baseline_y,
        width,
        font_size,
        color,
        underline,
        strike,
    } = spec;

    if width <= 0.0 || (!underline && !strike) {
        return Vec::new();
    }

    let thickness = (font_size * 0.06).max(1.0);
    let mut items = Vec::new();

    if underline {
        let y = baseline_y + font_size * 0.08 - thickness / 2.0;
        items.push(RenderItem::Draw(DrawCmd::Rect(
            x, y, width, thickness, color,
        )));
    }
    if strike {
        let y = baseline_y - font_size * 0.3 - thickness / 2.0;
        items.push(RenderItem::Draw(DrawCmd::Rect(
            x, y, width, thickness, color,
        )));
    }

    items
}

pub(super) fn text_metrics_with_font(
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
) -> (f32, f32) {
    let font = make_font_with_style(family, weight, italic, font_size);
    let (_, metrics) = font.metrics();
    (metrics.ascent.abs(), metrics.descent)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TextDecorationSpec {
    pub(super) x: f32,
    pub(super) baseline_y: f32,
    pub(super) width: f32,
    pub(super) font_size: f32,
    pub(super) color: u32,
    pub(super) underline: bool,
    pub(super) strike: bool,
}

pub(super) fn measure_text_width_with_font(
    text: &str,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
) -> f32 {
    let font = make_font_with_style(family, weight, italic, font_size);

    if text.is_empty() {
        return 0.0;
    }

    if letter_spacing == 0.0 && word_spacing == 0.0 {
        let (width, _bounds) = font.measure_str(text, None);
        return width;
    }

    let mut total = 0.0;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
        total += glyph_width;

        if chars.peek().is_some() {
            total += letter_spacing;
            if ch.is_whitespace() {
                total += word_spacing;
            }
        }
    }

    total
}

pub(super) fn text_offset_for_char_index(
    text: &str,
    char_index: usize,
    font_size: f32,
    family: &str,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    word_spacing: f32,
) -> f32 {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return 0.0;
    }

    let clamped_index = char_index.min(chars.len());
    if clamped_index == 0 {
        return 0.0;
    }

    let font = make_font_with_style(family, weight, italic, font_size);
    let mut total = 0.0;

    for (idx, ch) in chars.iter().enumerate() {
        if idx >= clamped_index {
            break;
        }

        let glyph = ch.to_string();
        let (glyph_width, _bounds) = font.measure_str(&glyph, None);
        total += glyph_width;

        if idx + 1 < chars.len() {
            total += letter_spacing;
            if ch.is_whitespace() {
                total += word_spacing;
            }
        }
    }

    total
}
