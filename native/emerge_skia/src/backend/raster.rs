//! Raster (offscreen CPU) backend.
//!
//! This backend renders to a CPU-backed surface without any windowing.
//! Useful for testing, headless rendering, and generating images.

use skia_safe::{ColorType, ImageInfo, surfaces};

use crate::renderer::{RenderState, Renderer};

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
    renderer: Renderer,
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

        let renderer = Renderer::from_surface(surface);

        Ok(Self {
            renderer,
            width: config.width,
            height: config.height,
        })
    }

    /// Render the current state and return the frame.
    pub fn render(&mut self, state: &RenderState) -> RasterFrame {
        self.renderer.render(state);

        // Read pixels from the surface
        let mut data = vec![0u8; (self.width * self.height * 4) as usize];

        let info = ImageInfo::new(
            (self.width as i32, self.height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );

        let surface = self.renderer.surface_mut();
        surface.read_pixels(&info, &mut data, (self.width * 4) as usize, (0, 0));

        RasterFrame { data }
    }
}
