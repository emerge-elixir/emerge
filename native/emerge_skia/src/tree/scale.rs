//! Resource inputs at the most recently accepted site. No geometry, glyph run,
//! drawing, or callback is computed or installed by admission.
use super::{Attrs, Family, Length};

pub(super) fn pixels(authored: f64, factor: f32) -> f32 {
    (authored * f64::from(factor)) as f32
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Inputs {
    pub family: Family,
    pub size: f32,
    pub scale: f32,
}
impl Inputs {
    pub fn display(scale: f32) -> Self {
        Self {
            scale,
            size: 16.,
            family: Family::Sans,
        }
    }
    pub fn child(self, a: &Attrs, nested: bool) -> Result<Self, String> {
        let local = a.layout_scale.unwrap_or(1.);
        // Main's flat root path multiplies the wire f64 before narrowing. Its
        // nested path composes f32 factors and floors each edge at EPSILON.
        let scale = if nested {
            (self.scale * local as f32).max(f32::EPSILON)
        } else {
            pixels(local, self.scale)
        };
        if !scale.is_finite() || scale <= 0. {
            return Err("layout scale exceeds device range".into());
        }
        let size = a.font_size.map_or(self.size, |s| pixels(s, scale));
        if !size.is_finite() {
            return Err("font size exceeds device range".into());
        }
        let b = a.sizing;
        if ![b.width, b.height]
            .into_iter()
            .filter_map(|v| match v {
                Length::Px(x) => Some(x),
                _ => None,
            })
            .chain(b.padding)
            .chain(b.spacing)
            .all(|v| pixels(v, scale).is_finite())
        {
            return Err("scaled length exceeds device range".into());
        }
        Ok(Self {
            family: a.family.unwrap_or(self.family),
            size,
            scale,
        })
    }
}
