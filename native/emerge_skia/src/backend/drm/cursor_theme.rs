use std::fs;

use resvg::usvg;
use skia_safe::{
    AlphaType, Color, ColorType, Data, FilterMode, Image, ImageInfo, MipmapMode, Paint, Rect,
    SamplingOptions, images, surfaces,
};

use crate::DrmCursorOverrideConfig;
use crate::assets::{AssetConfig, resolve_configured_path};
use crate::events::CursorIcon;

pub(crate) const CURSOR_PLANE_SIZE: (u32, u32) = (64, 64);

const BUILTIN_DEFAULT_SVG: &str = include_str!("cursors/mocu_black_right/default.svg");
const BUILTIN_POINTER_SVG: &str = include_str!("cursors/mocu_black_right/pointer.svg");
const BUILTIN_TEXT_SVG: &str = include_str!("cursors/mocu_black_right/text.svg");

pub(crate) struct CursorVisual {
    image: Image,
    size: (u32, u32),
    hotspot: (f32, f32),
    plane_bgra: Vec<u8>,
}

impl CursorVisual {
    pub(crate) fn image(&self) -> &Image {
        &self.image
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(crate) fn hotspot(&self) -> (f32, f32) {
        self.hotspot
    }

    pub(crate) fn plane_bgra(&self) -> &[u8] {
        &self.plane_bgra
    }
}

pub(crate) struct DrmCursorTheme {
    default: CursorVisual,
    pointer: CursorVisual,
    text: CursorVisual,
}

impl DrmCursorTheme {
    pub(crate) fn load(
        asset_config: &AssetConfig,
        overrides: &[DrmCursorOverrideConfig],
    ) -> Result<Self, String> {
        let mut theme = Self::builtin()?;

        for entry in overrides {
            let visual = load_override_visual(asset_config, entry)?;
            theme.set(entry.icon, visual);
        }

        Ok(theme)
    }

    pub(crate) fn cursor(&self, icon: CursorIcon) -> &CursorVisual {
        match icon {
            CursorIcon::Default => &self.default,
            CursorIcon::Pointer => &self.pointer,
            CursorIcon::Text => &self.text,
        }
    }

    fn builtin() -> Result<Self, String> {
        Ok(Self {
            default: load_builtin_svg(BUILTIN_DEFAULT_SVG, (1.0, 1.0))?,
            pointer: load_builtin_svg(BUILTIN_POINTER_SVG, (7.0, 2.0))?,
            text: load_builtin_svg(BUILTIN_TEXT_SVG, (11.5, 11.5))?,
        })
    }

    fn set(&mut self, icon: CursorIcon, visual: CursorVisual) {
        match icon {
            CursorIcon::Default => self.default = visual,
            CursorIcon::Pointer => self.pointer = visual,
            CursorIcon::Text => self.text = visual,
        }
    }
}

fn load_builtin_svg(svg: &str, hotspot: (f32, f32)) -> Result<CursorVisual, String> {
    let tree = parse_svg(svg.as_bytes())?;
    build_visual_from_svg(&tree, hotspot)
}

fn load_override_visual(
    asset_config: &AssetConfig,
    entry: &DrmCursorOverrideConfig,
) -> Result<CursorVisual, String> {
    let resolved = resolve_configured_path(&entry.source, asset_config)?;
    let bytes = fs::read(&resolved).map_err(|err| {
        format!(
            "failed to read DRM cursor override {}: {err}",
            resolved.display()
        )
    })?;
    let extension = resolved
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or_else(|| format!("unsupported DRM cursor override: {}", resolved.display()))?;

    match extension.as_str() {
        "svg" => {
            let tree = parse_svg(&bytes)?;
            build_visual_from_svg(&tree, entry.hotspot)
        }
        "png" => build_visual_from_png(&bytes, entry.hotspot),
        other => Err(format!(
            "unsupported DRM cursor override format .{other}: {}",
            resolved.display()
        )),
    }
}

fn parse_svg(bytes: &[u8]) -> Result<usvg::Tree, String> {
    let options = usvg::Options::default();
    usvg::Tree::from_data_nested(bytes, &options)
        .map_err(|err| format!("failed to parse DRM cursor SVG: {err}"))
}

fn build_visual_from_svg(tree: &usvg::Tree, hotspot: (f32, f32)) -> Result<CursorVisual, String> {
    let source_size = (tree.size().width(), tree.size().height());
    validate_hotspot(hotspot, source_size)?;
    let (target_width, target_height, scale) = fit_size(source_size)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(target_width, target_height)
        .ok_or_else(|| "failed to allocate DRM cursor SVG pixmap".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    let mut pixmap_mut = pixmap.as_mut();
    resvg::render(tree, transform, &mut pixmap_mut);

    build_visual_from_rgba(
        target_width,
        target_height,
        pixmap.data(),
        (hotspot.0 * scale, hotspot.1 * scale),
    )
}

fn build_visual_from_png(bytes: &[u8], hotspot: (f32, f32)) -> Result<CursorVisual, String> {
    let image = Image::from_encoded(Data::new_copy(bytes))
        .ok_or_else(|| "failed to decode DRM cursor PNG".to_string())?;
    let source_size = (image.width().max(0) as f32, image.height().max(0) as f32);
    validate_hotspot(hotspot, source_size)?;
    let (target_width, target_height, scale) = fit_size(source_size)?;
    let rgba = rasterize_image_to_rgba(&image, target_width, target_height)?;
    build_visual_from_rgba(
        target_width,
        target_height,
        &rgba,
        (hotspot.0 * scale, hotspot.1 * scale),
    )
}

fn fit_size(source_size: (f32, f32)) -> Result<(u32, u32, f32), String> {
    let (source_width, source_height) = source_size;
    if source_width <= 0.0 || source_height <= 0.0 {
        return Err("DRM cursor source must have positive dimensions".to_string());
    }

    let scale = (CURSOR_PLANE_SIZE.0 as f32 / source_width)
        .min(CURSOR_PLANE_SIZE.1 as f32 / source_height)
        .min(1.0);
    let width = ((source_width * scale).round().max(1.0) as u32).min(CURSOR_PLANE_SIZE.0);
    let height = ((source_height * scale).round().max(1.0) as u32).min(CURSOR_PLANE_SIZE.1);
    Ok((width, height, scale))
}

fn validate_hotspot(hotspot: (f32, f32), source_size: (f32, f32)) -> Result<(), String> {
    let (x, y) = hotspot;
    let (width, height) = source_size;
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return Err(format!(
            "DRM cursor hotspot must be finite and non-negative: ({x}, {y})"
        ));
    }

    if x >= width || y >= height {
        return Err(format!(
            "DRM cursor hotspot ({x}, {y}) lies outside source bounds {width}x{height}"
        ));
    }

    Ok(())
}

fn build_visual_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    hotspot: (f32, f32),
) -> Result<CursorVisual, String> {
    let image = rgba_image(width, height, rgba)?;
    let plane_bgra = pad_rgba_for_cursor_plane(width, height, rgba)?;
    Ok(CursorVisual {
        image,
        size: (width, height),
        hotspot,
        plane_bgra,
    })
}

fn rgba_image(width: u32, height: u32, rgba: &[u8]) -> Result<Image, String> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let data = Data::new_copy(rgba);
    images::raster_from_data(&info, data, (width * 4) as usize)
        .ok_or_else(|| "failed to build DRM cursor image".to_string())
}

fn rasterize_image_to_rgba(image: &Image, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, None)
        .ok_or_else(|| "failed to create DRM cursor raster surface".to_string())?;
    {
        let canvas = surface.canvas();
        canvas.clear(Color::TRANSPARENT);
        let paint = Paint::default();
        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        let dst = Rect::from_xywh(0.0, 0.0, width as f32, height as f32);
        canvas.draw_image_rect_with_sampling_options(image, None, dst, sampling, &paint);
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    surface.read_pixels(&info, pixels.as_mut_slice(), (width * 4) as usize, (0, 0));
    Ok(pixels)
}

fn pad_rgba_for_cursor_plane(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected_len = (width * height * 4) as usize;
    if rgba.len() != expected_len {
        return Err(format!(
            "DRM cursor pixel buffer has wrong length: {} != {}",
            rgba.len(),
            expected_len
        ));
    }

    let mut plane = vec![0u8; (CURSOR_PLANE_SIZE.0 * CURSOR_PLANE_SIZE.1 * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let src = ((y * width + x) * 4) as usize;
            let dst = ((y * CURSOR_PLANE_SIZE.0 + x) * 4) as usize;
            plane[dst] = rgba[src + 2];
            plane[dst + 1] = rgba[src + 1];
            plane[dst + 2] = rgba[src];
            plane[dst + 3] = rgba[src + 3];
        }
    }
    Ok(plane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("emerge_cursor_theme_{nonce}_{name}"))
    }

    #[test]
    fn builtin_theme_uses_expected_mocu_hotspots() {
        let theme = DrmCursorTheme::load(&AssetConfig::default(), &[]).expect("builtin theme");

        assert_eq!(theme.cursor(CursorIcon::Default).size(), (24, 24));
        assert_eq!(theme.cursor(CursorIcon::Default).hotspot(), (1.0, 1.0));
        assert_eq!(theme.cursor(CursorIcon::Pointer).hotspot(), (7.0, 2.0));
        assert_eq!(theme.cursor(CursorIcon::Text).hotspot(), (11.5, 11.5));
        assert_eq!(
            theme.cursor(CursorIcon::Default).plane_bgra().len(),
            (CURSOR_PLANE_SIZE.0 * CURSOR_PLANE_SIZE.1 * 4) as usize
        );
    }

    #[test]
    fn svg_override_scales_image_and_hotspot_into_plane() {
        let dir = unique_temp_path("svg");
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("pointer.svg");
        fs::write(
            &path,
            r##"<svg width="128" height="64" xmlns="http://www.w3.org/2000/svg"><rect width="128" height="64" fill="#000"/></svg>"##,
        )
        .expect("write svg override");

        let asset_config = AssetConfig {
            sources: vec![dir.to_string_lossy().into_owned()],
            runtime_enabled: true,
            runtime_allowlist: vec![dir.to_string_lossy().into_owned()],
            runtime_follow_symlinks: false,
            runtime_max_file_size: 1_000_000,
            runtime_extensions: vec![".svg".to_string(), ".png".to_string()],
        };
        let theme = DrmCursorTheme::load(
            &asset_config,
            &[DrmCursorOverrideConfig {
                icon: CursorIcon::Pointer,
                source: path.to_string_lossy().into_owned(),
                hotspot: (64.0, 32.0),
            }],
        )
        .expect("override theme");

        let pointer = theme.cursor(CursorIcon::Pointer);
        assert_eq!(pointer.size(), (64, 32));
        assert_eq!(pointer.hotspot(), (32.0, 16.0));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
    }
}
