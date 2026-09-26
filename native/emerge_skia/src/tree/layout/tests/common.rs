use super::super::*;
use crate::renderer::{RenderFrame, RenderState, SceneRenderer};
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
    Attrs {
        content: Some(content.to_string()),
        font_size: Some(16.0),
        ..Attrs::default()
    }
}

pub(super) fn fixed_box_attrs(width: f64, height: f64) -> Attrs {
    Attrs {
        width: Some(Length::Px(width)),
        height: Some(Length::Px(height)),
        ..Attrs::default()
    }
}

pub(super) fn fixed_width_attrs(width: f64) -> Attrs {
    Attrs {
        width: Some(Length::Px(width)),
        ..Attrs::default()
    }
}

pub(super) fn fixed_height_attrs(height: f64) -> Attrs {
    Attrs {
        height: Some(Length::Px(height)),
        ..Attrs::default()
    }
}

pub(super) fn fill_width_attrs() -> Attrs {
    Attrs {
        width: Some(Length::Fill),
        ..Attrs::default()
    }
}

pub(super) fn fill_width_box_attrs(height: f64) -> Attrs {
    Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Px(height)),
        ..Attrs::default()
    }
}

pub(super) fn fill_box_attrs() -> Attrs {
    Attrs {
        width: Some(Length::Fill),
        height: Some(Length::Fill),
        ..Attrs::default()
    }
}

pub(super) fn render_scene_to_pixels(
    width: u32,
    height: u32,
    scene: crate::render_scene::RenderScene,
) -> Vec<u8> {
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .expect("raster surface should be created for render equivalence test");
    let state = RenderState::new(scene, skia_safe::Color::TRANSPARENT, 1, false);
    {
        let mut frame = RenderFrame::new(&mut surface, None);
        SceneRenderer::new().render(&mut frame, &state);
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    surface.read_pixels(&info, pixels.as_mut_slice(), (width * 4) as usize, (0, 0));
    pixels
}
