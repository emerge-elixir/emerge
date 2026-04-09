#[derive(Clone, Copy, Debug)]
pub(crate) struct TextLayoutStyle {
    pub font_size: f32,
    pub letter_spacing: f32,
    pub word_spacing: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutLine {
    pub start: usize,
    pub visual_end: usize,
    pub next_start: usize,
    pub width: f32,
    pub positions: Vec<f32>,
    pub text: String,
}

impl TextLayoutLine {
    pub fn offset_for_cursor(&self, cursor: usize) -> f32 {
        let clamped = cursor.clamp(self.start, self.visual_end);
        self.positions[clamped - self.start]
    }

    pub fn nearest_cursor_for_x(&self, x: f32) -> usize {
        if self.positions.len() <= 1 {
            return self.start;
        }

        for idx in 0..(self.positions.len() - 1) {
            let midpoint =
                self.positions[idx] + (self.positions[idx + 1] - self.positions[idx]) / 2.0;
            if x <= midpoint {
                return self.start + idx;
            }
        }

        self.visual_end
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextLayout {
    pub lines: Vec<TextLayoutLine>,
    pub ascent: f32,
    pub line_height: f32,
    pub max_width: f32,
    pub total_height: f32,
}

impl TextLayout {
    pub fn line_index_for_cursor(&self, cursor: usize) -> usize {
        let Some(last_index) = self.lines.len().checked_sub(1) else {
            return 0;
        };

        self.lines
            .iter()
            .enumerate()
            .find_map(|(index, line)| {
                if index == last_index {
                    (cursor <= line.visual_end).then_some(index)
                } else {
                    (cursor >= line.start && cursor < line.next_start).then_some(index)
                }
            })
            .unwrap_or(last_index)
    }

    pub fn line_index_for_y(&self, y: f32) -> usize {
        if self.lines.is_empty() {
            return 0;
        }

        ((y / self.line_height.max(1.0)).floor() as isize).clamp(0, self.lines.len() as isize - 1)
            as usize
    }
}

pub(crate) fn layout_text_lines(
    text: &str,
    wrap_width: Option<f32>,
    metrics: (f32, f32),
    style: TextLayoutStyle,
    mut glyph_width: impl FnMut(char) -> f32,
) -> TextLayout {
    let (ascent, descent) = metrics;
    let line_height = (ascent + descent).max(style.font_size.max(1.0));
    let wrap_width = wrap_width.filter(|width| *width > 0.0);
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut line_width = 0.0f32;
    let mut last_char: Option<char> = None;
    let mut last_break: Option<(usize, f32)> = None;
    let mut idx = 0usize;

    while idx < chars.len() {
        let ch = chars[idx];
        if ch == '\n' {
            lines.push(build_line(
                &chars,
                line_start,
                idx,
                idx + 1,
                line_width,
                style,
                &mut glyph_width,
            ));
            line_start = idx + 1;
            line_width = 0.0;
            last_char = None;
            last_break = None;
            idx += 1;
            continue;
        }

        let spacing_before = last_char.map_or(0.0, |prev| {
            style.letter_spacing
                + if prev.is_whitespace() {
                    style.word_spacing
                } else {
                    0.0
                }
        });
        let next_width = line_width + spacing_before + glyph_width(ch);

        if wrap_width.is_some_and(|max_width| line_start < idx && next_width > max_width + 0.001) {
            if let Some((break_idx, break_width)) = last_break
                && break_idx > line_start
            {
                lines.push(build_line(
                    &chars,
                    line_start,
                    break_idx,
                    break_idx,
                    break_width,
                    style,
                    &mut glyph_width,
                ));
                line_start = break_idx;
            } else {
                lines.push(build_line(
                    &chars,
                    line_start,
                    idx,
                    idx,
                    line_width,
                    style,
                    &mut glyph_width,
                ));
                line_start = idx;
            }

            line_width = 0.0;
            last_char = None;
            last_break = None;
            continue;
        }

        line_width = next_width;
        last_char = Some(ch);
        if ch.is_whitespace() {
            last_break = Some((idx + 1, line_width));
        }
        idx += 1;
    }

    if lines.is_empty() || line_start <= chars.len() {
        lines.push(build_line(
            &chars,
            line_start,
            chars.len(),
            chars.len(),
            line_width,
            style,
            &mut glyph_width,
        ));
    }

    let max_width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    let total_height = line_height * lines.len() as f32;

    TextLayout {
        lines,
        ascent,
        line_height,
        max_width,
        total_height,
    }
}

fn build_line(
    chars: &[char],
    start: usize,
    visual_end: usize,
    next_start: usize,
    width: f32,
    style: TextLayoutStyle,
    glyph_width: &mut impl FnMut(char) -> f32,
) -> TextLayoutLine {
    let positions = build_positions(chars, start, visual_end, style, glyph_width);
    let text: String = chars[start.min(chars.len())..visual_end.min(chars.len())]
        .iter()
        .collect();

    TextLayoutLine {
        start,
        visual_end,
        next_start,
        width,
        positions,
        text,
    }
}

fn build_positions(
    chars: &[char],
    start: usize,
    end: usize,
    style: TextLayoutStyle,
    glyph_width: &mut impl FnMut(char) -> f32,
) -> Vec<f32> {
    let mut positions = Vec::with_capacity(end.saturating_sub(start) + 1);
    positions.push(0.0);

    let mut total = 0.0f32;
    let mut last_char: Option<char> = None;
    for &ch in chars[start.min(chars.len())..end.min(chars.len())].iter() {
        if let Some(prev) = last_char {
            total += style.letter_spacing;
            if prev.is_whitespace() {
                total += style.word_spacing;
            }
        }

        total += glyph_width(ch);
        positions.push(total);
        last_char = Some(ch);
    }

    positions
}
