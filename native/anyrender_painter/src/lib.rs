//! Painter-only build of anyrender_skia. Emerge owns the recording canvas and all presenters.
mod cache;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod ct_collection;
mod scene;
pub use scene::{SkiaSceneCache, SkiaScenePainter};

const FREETYPE_TEXT: bool = !cfg!(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
));
