//! Raster (offscreen CPU) backend.
//!
//! This backend renders to a CPU-backed surface without any windowing.
//! Useful for testing, headless rendering, and generating images.

use skia_safe::{ColorType, ImageInfo, surfaces};

use crate::renderer::{RenderFrame, RenderState, SceneRenderer};

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
        let info = ImageInfo::new(
            (config.width as i32, config.height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );

        let surface = surfaces::raster(&info, None, None)
            .ok_or_else(|| "Failed to create raster surface".to_string())?;

        Ok(Self {
            renderer: SceneRenderer::new(),
            surface,
            width: config.width,
            height: config.height,
        })
    }

    /// Render the current state and return the frame.
    pub fn render(&mut self, state: &RenderState) -> RasterFrame {
        let mut frame = RenderFrame::new(&mut self.surface, None);
        self.renderer.render(&mut frame, state);

        // Read pixels from the surface
        let mut data = vec![0u8; (self.width * self.height * 4) as usize];

        let info = ImageInfo::new(
            (self.width as i32, self.height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );

        self.surface
            .read_pixels(&info, &mut data, (self.width * 4) as usize, (0, 0));

        RasterFrame { data }
    }
}
