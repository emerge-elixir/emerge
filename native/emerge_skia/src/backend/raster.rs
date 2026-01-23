//! Raster (offscreen CPU) backend.
//!
//! This backend renders to a CPU-backed surface without any windowing.
//! Useful for testing, headless rendering, and generating images.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use skia_safe::{surfaces, ColorType, ImageInfo};

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
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
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

    /// Resize the raster surface.
    #[allow(dead_code)]
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );

        let surface = surfaces::raster(&info, None, None)
            .ok_or_else(|| "Failed to create raster surface".to_string())?;

        self.renderer = Renderer::from_surface(surface);
        self.width = width;
        self.height = height;

        Ok(())
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
        surface.read_pixels(
            &info,
            &mut data,
            (self.width * 4) as usize,
            (0, 0),
        );

        RasterFrame {
            width: self.width,
            height: self.height,
            data,
        }
    }

    /// Get current dimensions.
    #[allow(dead_code)]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

// ============================================================================
// Polling-based Backend (for background thread use)
// ============================================================================

/// Run a polling-based raster backend in a loop.
///
/// This function blocks and periodically checks for render requests.
/// Use `dirty_flag` to signal when a new render is needed.
/// The rendered frame is placed in `frame_slot`.
#[allow(dead_code)]
pub fn run(
    config: RasterConfig,
    render_state: Arc<Mutex<RenderState>>,
    running_flag: Arc<AtomicBool>,
    dirty_flag: Arc<AtomicBool>,
    frame_slot: Arc<Mutex<Option<RasterFrame>>>,
) {
    let mut backend = match RasterBackend::new(&config) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("Failed to initialize raster backend: {err}");
            running_flag.store(false, Ordering::Relaxed);
            return;
        }
    };

    while running_flag.load(Ordering::Relaxed) {
        if dirty_flag.swap(false, Ordering::Relaxed)
            && let Ok(state) = render_state.lock()
        {
            let frame = backend.render(&state);
            if let Ok(mut slot) = frame_slot.lock() {
                *slot = Some(frame);
            }
        }

        // Sleep to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
