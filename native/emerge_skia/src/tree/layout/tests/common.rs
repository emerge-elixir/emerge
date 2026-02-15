use super::super::*;
use crate::tree::attrs::Attrs;
use crate::tree::element::Element;

pub(super) struct MockTextMeasurer;
impl TextMeasurer for MockTextMeasurer {
    fn measure(&self, text: &str, font_size: f32) -> (f32, f32) {
        self.measure_with_font(text, font_size, "default", 400, false)
    }

    fn measure_with_font(
        &self,
        text: &str,
        font_size: f32,
        _family: &str,
        _weight: u16,
        _italic: bool,
    ) -> (f32, f32) {
        // Simple mock: 8px per char, height = font_size
        (text.len() as f32 * 8.0, font_size)
    }

    fn font_metrics(
        &self,
        font_size: f32,
        _family: &str,
        _weight: u16,
        _italic: bool,
    ) -> (f32, f32) {
        // Mock: ascent = 75% of font_size, descent = 25%
        (font_size * 0.75, font_size * 0.25)
    }
}

pub(super) fn make_element(id: &str, kind: ElementKind, attrs: Attrs) -> Element {
    Element::with_attrs(
        ElementId::from_term_bytes(id.as_bytes().to_vec()),
        kind,
        vec![],
        attrs,
    )
}

pub(super) fn text_attrs(content: &str) -> Attrs {
    let mut a = Attrs::default();
    a.content = Some(content.to_string());
    a.font_size = Some(16.0);
    a
}
