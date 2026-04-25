use super::super::*;
use crate::tree::attrs::Attrs;
use crate::tree::element::Element;
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub(super) struct MockTextMeasurer;
impl TextMeasurer for MockTextMeasurer {
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

#[derive(Default)]
pub(super) struct CountingTextMeasurer {
    measure_calls: Cell<usize>,
    metric_calls: Cell<usize>,
}

impl CountingTextMeasurer {
    pub(super) fn total_calls(&self) -> usize {
        self.measure_calls.get() + self.metric_calls.get()
    }
}

impl TextMeasurer for CountingTextMeasurer {
    fn measure_with_font(
        &self,
        text: &str,
        font_size: f32,
        _family: &str,
        _weight: u16,
        _italic: bool,
    ) -> (f32, f32) {
        self.measure_calls.set(self.measure_calls.get() + 1);
        (text.len() as f32 * 8.0, font_size)
    }

    fn font_metrics(
        &self,
        font_size: f32,
        _family: &str,
        _weight: u16,
        _italic: bool,
    ) -> (f32, f32) {
        self.metric_calls.set(self.metric_calls.get() + 1);
        (font_size * 0.75, font_size * 0.25)
    }
}

pub(super) fn make_element(id: &str, kind: ElementKind, attrs: Attrs) -> Element {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);

    Element::with_attrs(NodeId::from_u64(hasher.finish()), kind, vec![], attrs)
}

pub(super) fn text_attrs(content: &str) -> Attrs {
    let mut a = Attrs::default();
    a.content = Some(content.to_string());
    a.font_size = Some(16.0);
    a
}
