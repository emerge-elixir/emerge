//! Raster (offscreen CPU) backend.
//!
//! This backend renders to a CPU-backed surface without any windowing.
//! Useful for testing, headless rendering, and generating images.

use skia_safe::{AlphaType, ColorType, ImageInfo, surfaces};

use crate::renderer::{
    RenderFrame, RenderState, RenderTimings, RendererCacheConfig, SceneRenderer, text_surface_props,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RasterPixelFormat {
    #[default]
    Rgba8888Premul,
    Gray8Opaque,
}

impl RasterPixelFormat {
    pub fn color_type(self) -> ColorType {
        match self {
            Self::Rgba8888Premul => ColorType::RGBA8888,
            Self::Gray8Opaque => ColorType::Gray8,
        }
    }

    pub fn alpha_type(self) -> AlphaType {
        match self {
            Self::Rgba8888Premul => AlphaType::Premul,
            Self::Gray8Opaque => AlphaType::Opaque,
        }
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba8888Premul => 4,
            Self::Gray8Opaque => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RasterConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for RasterConfig {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
        }
    }
}

#[derive(Clone)]
pub struct RasterFrame {
    pub data: Vec<u8>,
    pub row_bytes: usize,
    pub format: RasterPixelFormat,
}

pub struct RasterBackend {
    renderer: SceneRenderer,
    surface: skia_safe::Surface,
    width: u32,
    height: u32,
    format: RasterPixelFormat,
}

impl RasterBackend {
    pub fn new(config: &RasterConfig) -> Result<Self, String> {
        Self::with_cache_config(config, RendererCacheConfig::default())
    }

    pub fn with_cache_config(
        config: &RasterConfig,
        cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        Self::with_cache_config_and_format(config, cache_config, RasterPixelFormat::default())
    }

    pub fn with_cache_config_and_format(
        config: &RasterConfig,
        cache_config: RendererCacheConfig,
        format: RasterPixelFormat,
    ) -> Result<Self, String> {
        let surface = create_surface(config.width, config.height, format)?;

        Ok(Self {
            renderer: SceneRenderer::with_cache_config(cache_config),
            surface,
            width: config.width,
            height: config.height,
            format,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.surface = create_surface(width, height, self.format)?;
        self.renderer.invalidate_visible_frame_fingerprint();
        self.width = width;
        self.height = height;
        Ok(())
    }

    pub fn sync_cpu_video_frames(
        &mut self,
        registry: &std::sync::Arc<crate::video::VideoRegistry>,
    ) -> Result<bool, String> {
        self.renderer.sync_cpu_video_frames(registry)
    }

    pub fn render(&mut self, state: &RenderState) -> RasterFrame {
        self.render_with_timings(state).0
    }

    pub fn render_with_timings(&mut self, state: &RenderState) -> (RasterFrame, RenderTimings) {
        let mut frame = RenderFrame::new(&mut self.surface, None);
        let timings = self.renderer.render(&mut frame, state);
        let row_bytes = usize::try_from(self.width)
            .ok()
            .and_then(|width| width.checked_mul(self.format.bytes_per_pixel()))
            .expect("validated raster surface width must fit memory");
        let byte_len = row_bytes
            .checked_mul(self.height as usize)
            .expect("validated raster surface dimensions must fit memory");
        let mut data = vec![0_u8; byte_len];
        let info = ImageInfo::new(
            (self.width as i32, self.height as i32),
            self.format.color_type(),
            self.format.alpha_type(),
            None,
        );

        let read = self
            .surface
            .read_pixels(&info, &mut data, row_bytes, (0, 0));
        debug_assert!(read, "raster surface readback must match its native format");

        (
            RasterFrame {
                data,
                row_bytes,
                format: self.format,
            },
            timings,
        )
    }

    pub fn render_grayscale_dither_policy(&self, state: &RenderState) -> Result<Vec<u8>, String> {
        self.renderer
            .render_grayscale_dither_policy(self.width, self.height, state)
    }
}

fn create_surface(
    width: u32,
    height: u32,
    format: RasterPixelFormat,
) -> Result<skia_safe::Surface, String> {
    let width_i32 = i32::try_from(width).map_err(|_| "raster width is too large".to_string())?;
    let height_i32 = i32::try_from(height).map_err(|_| "raster height is too large".to_string())?;
    let info = ImageInfo::new(
        (width_i32, height_i32),
        format.color_type(),
        format.alpha_type(),
        None,
    );

    let surface = match format {
        RasterPixelFormat::Rgba8888Premul => {
            let surface_props = text_surface_props();
            surfaces::raster(&info, None, Some(&surface_props))
        }
        RasterPixelFormat::Gray8Opaque => surfaces::raster(&info, None, None),
    };
    surface.ok_or_else(|| format!("failed to create {format:?} raster surface"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{render_scene::RenderScene, renderer::RenderState};
    use skia_safe::Color;

    #[test]
    fn gray8_surface_returns_one_byte_per_pixel() {
        let mut backend = RasterBackend::with_cache_config_and_format(
            &RasterConfig {
                width: 3,
                height: 2,
            },
            RendererCacheConfig {
                enabled: false,
                ..RendererCacheConfig::default()
            },
            RasterPixelFormat::Gray8Opaque,
        )
        .unwrap();
        let state = RenderState::new(RenderScene::default(), Color::WHITE, 1, false);
        let frame = backend.render(&state);

        assert_eq!(frame.format, RasterPixelFormat::Gray8Opaque);
        assert_eq!(frame.row_bytes, 3);
        assert_eq!(frame.data, vec![255; 6]);
    }
}
