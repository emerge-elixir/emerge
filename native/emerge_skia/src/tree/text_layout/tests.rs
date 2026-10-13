use super::*;

fn unit_layout(text: &str, width: f32, spacing: (f32, f32)) -> TextLayout {
    layout_text_lines(
        text,
        Some(width),
        (1.0, 0.0),
        TextLayoutStyle {
            font_size: 1.0,
            letter_spacing: spacing.0,
            word_spacing: spacing.1,
        },
        |_| 1.0,
    )
}

fn assert_consistent(text: &str, width: f32, layout: &TextLayout) {
    let chars: Vec<_> = text.chars().collect();
    let reconstructed: String = layout
        .lines
        .iter()
        .flat_map(|line| chars[line.start..line.next_start].iter())
        .collect();
    assert_eq!(reconstructed, text);
    assert_eq!(layout.lines.first().unwrap().start, 0);
    assert_eq!(layout.lines.last().unwrap().next_start, chars.len());
    for pair in layout.lines.windows(2) {
        assert_eq!(pair[0].next_start, pair[1].start);
    }
    for line in &layout.lines {
        assert_eq!(line.positions.len(), line.visual_end - line.start + 1);
        assert_eq!(
            line.text,
            chars[line.start..line.visual_end]
                .iter()
                .collect::<String>()
        );
        assert!(
            (line.width - line.positions.last().unwrap()).abs() < 0.001,
            "{text:?}: {:?} records {} but advances {:?}",
            line.text,
            line.width,
            line.positions.last()
        );
        assert!(line.width <= width + 0.001, "{text:?}: {line:?}");
    }
    assert_eq!(
        layout.max_width,
        layout
            .lines
            .iter()
            .map(|line| line.width)
            .fold(0.0, f32::max)
    );
    assert_eq!(layout.total_height, layout.lines.len() as f32);
}

#[test]
fn word_wrap_remeasures_the_moved_word_prefix() {
    let layout = unit_layout("a abc de", 4.0, (0.0, 0.0));
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["a ", "abc ", "de"]
    );
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| (line.start, line.visual_end, line.next_start))
            .collect::<Vec<_>>(),
        [(0, 2, 2), (2, 6, 6), (6, 8, 8)]
    );
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| line.width)
            .collect::<Vec<_>>(),
        [2.0, 4.0, 2.0]
    );
    assert_consistent("a abc de", 4.0, &layout);
}

#[test]
fn word_wrap_stays_consistent_on_each_edit_with_spacing_and_newlines() {
    for text in [
        "a abc de",
        "a  abc de",
        "a abc\nde\n",
        "é é中🙂 de",
        "a e\u{301}c de",
        "\n\n",
    ] {
        let chars: Vec<_> = text.chars().collect();
        for end in 0..=chars.len() {
            let prefix: String = chars[..end].iter().collect();
            for (width, spacing) in [
                (3.5, (0.0, 0.0)),
                (4.0, (0.0, 0.0)),
                (4.5, (0.0, 0.0)),
                (5.0, (0.25, 0.5)),
            ] {
                assert_consistent(&prefix, width, &unit_layout(&prefix, width, spacing));
            }
        }
    }
}

#[test]
fn word_wrap_cursor_and_hit_positions_use_the_remeasured_lines() {
    let layout = unit_layout("a abc de", 4.0, (0.0, 0.0));
    for (cursor, line_index, x) in [
        (0, 0, 0.0),
        (1, 0, 1.0),
        (2, 1, 0.0),
        (5, 1, 3.0),
        (6, 2, 0.0),
        (8, 2, 2.0),
    ] {
        assert_eq!(layout.line_index_for_cursor(cursor), line_index);
        assert_eq!(layout.lines[line_index].offset_for_cursor(cursor), x);
        assert_eq!(layout.lines[line_index].nearest_cursor_for_x(x), cursor);
        assert_eq!(layout.line_index_for_y(line_index as f32 + 0.5), line_index);
    }
}
