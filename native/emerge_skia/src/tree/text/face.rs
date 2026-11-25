//! Bundled faces, including the reference renderer's synthetic fallback for
//! weights without an embedded face. Metrics and both painters use one key.
use skia_safe::{Data, Font, FontHinting, FontMgr, Typeface, font::Edging};
use skrifa::{FontRef, MetadataProvider, charmap::Charmap};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, Hash, Eq, PartialEq)]
pub enum Family {
    #[default]
    Sans,
    Mono,
}
#[derive(Clone, Copy, Debug, Default, Hash, Eq, PartialEq)]
pub(in crate::tree) enum Weight {
    #[default]
    Regular,
    Bold,
    OtherLight,
    OtherHeavy,
}
#[derive(Clone, Copy, Debug, Default, Hash, Eq, PartialEq)]
pub(in crate::tree) struct Face {
    pub family: Family,
    pub weight: Weight,
    pub italic: bool,
}
impl From<Family> for Face {
    fn from(family: Family) -> Self {
        Self {
            family,
            ..Self::default()
        }
    }
}
pub(crate) const BUNDLED_FONTS: [&[u8]; 8] = [
    include_bytes!("../../fonts/inter/Inter-Regular.ttf"),
    include_bytes!("../../fonts/inter/Inter-Italic.ttf"),
    include_bytes!("../../fonts/inter/Inter-Bold.ttf"),
    include_bytes!("../../fonts/inter/Inter-BoldItalic.ttf"),
    include_bytes!("../../fonts/jetbrains-mono/JetBrainsMonoNL-Regular.ttf"),
    include_bytes!("../../fonts/jetbrains-mono/JetBrainsMonoNL-Italic.ttf"),
    include_bytes!("../../fonts/jetbrains-mono/JetBrainsMonoNL-Bold.ttf"),
    include_bytes!("../../fonts/jetbrains-mono/JetBrainsMonoNL-BoldItalic.ttf"),
];
impl Face {
    pub fn synthetic(self) -> bool {
        matches!(self.weight, Weight::OtherHeavy)
            || (self.weight == Weight::OtherLight && self.italic)
    }
    fn index(self) -> usize {
        let real = matches!(self.weight, Weight::Regular | Weight::Bold);
        usize::from(self.family == Family::Mono) * 4
            + usize::from(self.weight == Weight::Bold) * 2
            + usize::from(real && self.italic)
    }
    pub fn bytes(self) -> &'static [u8] {
        BUNDLED_FONTS[self.index()]
    }
    pub(super) fn cmap(self) -> &'static Charmap<'static> {
        static MAPS: [OnceLock<Charmap>; 8] = [const { OnceLock::new() }; 8];
        MAPS[self.index()]
            .get_or_init(|| FontRef::new(self.bytes()).expect("bundled font").charmap())
    }
    pub fn font(self, size: f32) -> Font {
        static FACES: [OnceLock<Typeface>; 8] = [const { OnceLock::new() }; 8];
        let face = FACES[self.index()].get_or_init(|| {
            FontMgr::new()
                .new_from_data(Data::new_with_owner(self.bytes()), 0)
                .expect("bundled typeface")
        });
        let mut f = Font::from_typeface(face.clone(), size);
        f.set_subpixel(true);
        f.set_linear_metrics(true);
        f.set_baseline_snap(true);
        f.set_edging(Edging::SubpixelAntiAlias);
        f.set_hinting(FontHinting::Normal);
        if self.synthetic() {
            f.set_embolden(self.weight == Weight::OtherHeavy);
            if self.italic {
                f.set_skew_x(-0.25);
            }
        }
        f
    }
}
