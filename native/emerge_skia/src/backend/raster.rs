//! Raster (offscreen CPU) backend.
//!
//! This backend renders to a CPU-backed surface without any windowing.
//! Useful for testing, headless rendering, and generating images.

use skia_safe::{ColorType, ImageInfo, surfaces};

use crate::renderer::{
    RenderFrame, RenderState, RenderTimings, RendererCacheConfig, SceneRenderer, text_surface_props,
};

// ============================================================================
// Configuration
// ============================================================================

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

// ============================================================================
// Raster Frame
// ============================================================================

/// A rendered frame with pixel data.
#[derive(Clone)]
pub struct RasterFrame {
    pub data: Vec<u8>, // RGBA bytes
}

// ============================================================================
// Raster Backend
// ============================================================================

pub struct RasterBackend {
    renderer: SceneRenderer,
    surface: skia_safe::Surface,
    width: u32,
    height: u32,
}

impl RasterBackend {
    /// Create a new raster backend with the given dimensions.
    pub fn new(config: &RasterConfig) -> Result<Self, String> {
        Self::with_cache_config(config, RendererCacheConfig::default())
    }

    pub fn with_cache_config(
        config: &RasterConfig,
        cache_config: RendererCacheConfig,
    ) -> Result<Self, String> {
        let surface = create_surface(config.width, config.height)?;

        Ok(Self {
            renderer: SceneRenderer::with_cache_config(cache_config),
            surface,
            width: config.width,
            height: config.height,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.surface = create_surface(width, height)?;
        self.renderer.invalidate_visible_frame_fingerprint();
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Render the current state and return the frame.
    pub fn render(&mut self, state: &RenderState) -> RasterFrame {
        self.render_with_timings(state).0
    }

    pub fn render_with_timings(&mut self, state: &RenderState) -> (RasterFrame, RenderTimings) {
        let mut frame = RenderFrame::new(&mut self.surface, None);
        let timings = self.renderer.render(&mut frame, state);

        let mut data = vec![0u8; (self.width * self.height * 4) as usize];

        let info = ImageInfo::new(
            (self.width as i32, self.height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );

        self.surface
            .read_pixels(&info, &mut data, (self.width * 4) as usize, (0, 0));

        (RasterFrame { data }, timings)
    }
}

fn create_surface(width: u32, height: u32) -> Result<skia_safe::Surface, String> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );

    let surface_props = text_surface_props();
    surfaces::raster(&info, None, Some(&surface_props))
        .ok_or_else(|| "Failed to create raster surface".to_string())
}
