//! One-shot image export using the tree and CPU renderer.
use crate::tree::Raster;
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo, Pixmap};

pub(crate) fn render_tree_offscreen(
    bytes: &[u8],
    w: u32,
    h: u32,
    scale: f32,
) -> Result<Vec<u8>, String> {
    let mut raster = Raster::default();
    raster.tree.upload_scaled(bytes, scale)?;
    raster.draw(w as f32, h as f32, scale)?;
    Ok(raster.into_pixels())
}

pub(crate) fn png(width: u32, height: u32, pixels: &mut [u8]) -> Result<Vec<u8>, String> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    Pixmap::new(&info, pixels, width as usize * 4)
        .and_then(|pixmap| pixmap.encode(EncodedImageFormat::PNG, None))
        .ok_or_else(|| "could not encode raster pixels as PNG".into())
}
