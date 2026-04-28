use std::time::Duration;

use skia_safe::{AlphaType, ColorType, Data, EncodedImageFormat, ImageInfo, images};

use crate::{
    assets::{self, AssetConfig},
    backend::raster::{RasterBackend, RasterConfig},
    renderer::{RenderState, load_font, make_font_with_style},
    tree::{self, layout::layout_and_refresh_default},
};

pub struct OffscreenRenderOptions {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub asset_mode: String,
    pub asset_timeout_ms: u64,
    pub asset_config: AssetConfig,
}

struct OffscreenRasterOutput {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

enum OffscreenAssetMode {
    Await,
    Snapshot,
}

impl OffscreenAssetMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "await" => Ok(Self::Await),
            "snapshot" => Ok(Self::Snapshot),
            other => Err(format!("unsupported asset_mode: {other}")),
        }
    }
}

pub fn measure_text(text: &str, font_size: f32) -> (f32, f32, f32, f32) {
    let font = make_font_with_style("default", 400, false, font_size);

    let (width, _bounds) = font.measure_str(text, None);
    let (_, metrics) = font.metrics();

    let ascent = metrics.ascent.abs();
    let descent = metrics.descent;
    let line_height = ascent + descent;

    (width, line_height, ascent, descent)
}

pub fn load_font_bytes(family: &str, weight: u16, italic: bool, data: &[u8]) -> Result<(), String> {
    load_font(family, weight, italic, data)
}

pub fn configure_assets(asset_config: AssetConfig) {
    assets::configure(asset_config);
}

pub fn render_tree_to_pixels(data: &[u8], opts: OffscreenRenderOptions) -> Result<Vec<u8>, String> {
    let output = render_tree_offscreen(data, opts)?;
    Ok(output.pixels)
}

pub fn render_tree_to_png(data: &[u8], opts: OffscreenRenderOptions) -> Result<Vec<u8>, String> {
    let output = render_tree_offscreen(data, opts)?;
    encode_png(&output)
}

fn render_tree_offscreen(
    data: &[u8],
    opts: OffscreenRenderOptions,
) -> Result<OffscreenRasterOutput, String> {
    let mode = OffscreenAssetMode::parse(&opts.asset_mode)?;
    let mut tree = tree::deserialize::decode_tree(data).map_err(|e| e.to_string())?;

    assets::configure(opts.asset_config);

    match mode {
        OffscreenAssetMode::Await => assets::resolve_tree_sources_sync(
            &tree,
            Some(Duration::from_millis(opts.asset_timeout_ms)),
        )?,
        OffscreenAssetMode::Snapshot => assets::snapshot_tree_sources_for_offscreen(&tree),
    }

    let constraint = tree::layout::Constraint::new(opts.width as f32, opts.height as f32);
    let output = layout_and_refresh_default(&mut tree, constraint, opts.scale);

    let config = RasterConfig {
        width: opts.width,
        height: opts.height,
    };
    let mut backend = RasterBackend::new(&config)?;

    let state = RenderState::new(output.scene, Default::default(), 0, false);

    let frame = backend.render(&state);

    Ok(OffscreenRasterOutput {
        width: opts.width,
        height: opts.height,
        pixels: frame.data,
    })
}

fn encode_png(output: &OffscreenRasterOutput) -> Result<Vec<u8>, String> {
    let info = ImageInfo::new(
        (output.width as i32, output.height as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let data = Data::new_copy(&output.pixels);
    let image = images::raster_from_data(&info, data, (output.width * 4) as usize)
        .ok_or_else(|| "Failed to create raster image from RGBA pixels".to_string())?;
    let encoded = image
        .encode(
            None::<&mut skia_safe::gpu::DirectContext>,
            EncodedImageFormat::PNG,
            100,
        )
        .ok_or_else(|| "Failed to encode raster output as PNG".to_string())?;

    Ok(encoded.as_bytes().to_vec())
}
