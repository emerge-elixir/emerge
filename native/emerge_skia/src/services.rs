//! One-shot image export using the tree and CPU renderer.
use crate::tree::Raster;
use skia_safe::{AlphaType, ColorType, EncodedImageFormat, ImageInfo, Pixmap};

pub(crate) fn render_tree_offscreen(
    bytes: &[u8],
    w: u32,
    h: u32,
    scale: f32,
    assets: impl FnOnce() -> Result<(crate::raster_assets::Settings, bool, std::time::Duration), String>,
) -> Result<Vec<u8>, String> {
    let mut raster = Raster::default();
    raster.tree.upload_scaled(bytes, scale)?;
    if raster.tree.needs_images()
        && let (config, true, timeout) = assets()?
    {
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::Builder::new()
            .name("emerge-image-export".into())
            .spawn(move || {
                let start = std::time::Instant::now();
                let result = crate::raster_assets::Loader::new(config).and_then(|mut loader| {
                    loop {
                        while let Some((ticket, request)) = raster.tree.next_image() {
                            if start.elapsed() >= timeout {
                                return Err("image loading timed out".into());
                            }
                            loader.fulfill(request, |s| raster.tree.complete_image(&ticket, s));
                        }
                        raster.tree.refresh(w as f32, h as f32, scale)?;
                        if !raster.tree.waiting_for_pixels() {
                            break;
                        }
                    }
                    Ok(raster)
                });
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        raster = rx
            .recv_timeout(timeout)
            .map_err(|e| format!("awaiting image assets: {e}"))??;
    }
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
