use smithay_client_toolkit::shell::{WaylandSurface, xdg::window::Window};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;

use crate::backend::wayland_config::WaylandConfig;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SurfaceGeometry {
    pub(super) logical_size: (u32, u32),
    pub(super) buffer_size: (u32, u32),
    integer_scale_factor: u32,
    preferred_fractional_scale: Option<f32>,
    scale_factor: f32,
}

impl SurfaceGeometry {
    pub(super) fn new(config: &WaylandConfig) -> Self {
        let logical_size = (config.width.max(1), config.height.max(1));

        Self {
            logical_size,
            buffer_size: logical_size,
            integer_scale_factor: 1,
            preferred_fractional_scale: None,
            scale_factor: 1.0,
        }
    }

    pub(super) fn set_logical_size(&mut self, width: u32, height: u32) -> bool {
        let next_size = (width.max(1), height.max(1));

        if self.logical_size == next_size {
            return false;
        }

        self.logical_size = next_size;
        true
    }

    pub(super) fn set_integer_scale_factor(&mut self, scale_factor: i32) {
        self.integer_scale_factor = scale_factor.max(1) as u32;
    }

    pub(super) fn set_preferred_fractional_scale(&mut self, scale_factor: Option<f32>) {
        self.preferred_fractional_scale = scale_factor;
    }

    pub(super) fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub(super) fn surface_to_buffer_position(&self, position: (f64, f64)) -> (f32, f32) {
        let logical_width = self.logical_size.0.max(1) as f64;
        let logical_height = self.logical_size.1.max(1) as f64;
        let buffer_width = self.buffer_size.0.max(1) as f64;
        let buffer_height = self.buffer_size.1.max(1) as f64;

        (
            (position.0 * buffer_width / logical_width) as f32,
            (position.1 * buffer_height / logical_height) as f32,
        )
    }

    pub(super) fn apply_to_surface(&mut self, window: &Window, viewport: Option<&WpViewport>) {
        let scale_factor = self.current_scale_factor();
        let fractional_active = self.preferred_fractional_scale.is_some() && viewport.is_some();

        if fractional_active {
            let _ = window.set_buffer_scale(1);

            if let Some(viewport) = viewport {
                viewport.set_destination(self.logical_size.0 as i32, self.logical_size.1 as i32);
            }
        } else {
            let _ = window.set_buffer_scale(self.integer_scale_factor.max(1));

            if let Some(viewport) = viewport {
                viewport.set_destination(-1, -1);
            }
        }

        self.scale_factor = scale_factor;
        self.buffer_size = Self::buffer_dimensions(self.logical_size, scale_factor);
    }

    fn current_scale_factor(&self) -> f32 {
        self.preferred_fractional_scale
            .unwrap_or(self.integer_scale_factor.max(1) as f32)
            .max(1.0)
    }

    fn buffer_dimensions(logical_size: (u32, u32), scale_factor: f32) -> (u32, u32) {
        let width = ((logical_size.0.max(1) as f64) * scale_factor as f64).round() as u32;
        let height = ((logical_size.1.max(1) as f64) * scale_factor as f64).round() as u32;

        (width.max(1), height.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceGeometry;
    use crate::backend::wayland_config::WaylandConfig;

    #[test]
    fn surface_to_buffer_position_scales_integer_factor() {
        let mut geometry = SurfaceGeometry::new(&WaylandConfig {
            title: "test".to_string(),
            width: 100,
            height: 80,
        });

        geometry.buffer_size = (200, 160);

        let (x, y) = geometry.surface_to_buffer_position((12.5, 30.0));

        assert!((x - 25.0).abs() < f32::EPSILON);
        assert!((y - 60.0).abs() < f32::EPSILON);
    }

    #[test]
    fn surface_to_buffer_position_scales_fractional_factor() {
        let mut geometry = SurfaceGeometry::new(&WaylandConfig {
            title: "test".to_string(),
            width: 100,
            height: 100,
        });

        geometry.buffer_size = (150, 150);

        let (x, y) = geometry.surface_to_buffer_position((20.0, 40.0));

        assert!((x - 30.0).abs() < f32::EPSILON);
        assert!((y - 60.0).abs() < f32::EPSILON);
    }

    #[test]
    fn surface_to_buffer_position_uses_actual_buffer_ratio_for_rounded_fractional_size() {
        let mut geometry = SurfaceGeometry::new(&WaylandConfig {
            title: "test".to_string(),
            width: 101,
            height: 100,
        });

        geometry.buffer_size = (152, 150);

        let (x, y) = geometry.surface_to_buffer_position((50.5, 25.0));

        assert!((x - (50.5_f32 * 152.0 / 101.0)).abs() < 0.0001);
        assert!((y - 37.5).abs() < 0.0001);
    }
}
