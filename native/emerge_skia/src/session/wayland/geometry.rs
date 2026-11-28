//! Nominal layout scale and post-rounding pointer ratios are different inputs.
//! Geometry is copyable so a submission retains exactly its own surface mapping.
use super::*;
use wayland_client::{Dispatch, delegate_noop, globals::GlobalList};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{Event, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scale {
    Integer(u32),
    Fractional(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Geometry {
    pub logical: (u32, u32),
    pub scale: Scale,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mapping {
    pub geometry: Geometry,
    pub buffer: (u32, u32),
}
impl Geometry {
    pub fn nominal(self) -> f32 {
        match self.scale {
            Scale::Integer(i) => i as f32,
            Scale::Fractional(ticks) => ticks as f32 / 120.,
        }
    }
    pub fn buffer(self) -> Result<(u32, u32), String> {
        let dimension = |n| match self.scale {
            Scale::Integer(i) => u32::checked_mul(n, i),
            Scale::Fractional(_) => {
                let n = (f64::from(n) * f64::from(self.nominal())).round();
                (n <= f64::from(u32::MAX)).then_some(n as u32)
            }
        };
        dimension(self.logical.0)
            .zip(dimension(self.logical.1))
            .filter(|&size| super::super::valid_size(size))
            .ok_or_else(|| "invalid Wayland buffer dimensions".into())
    }
    pub fn mapping(self) -> Result<Mapping, String> {
        Ok(Mapping {
            geometry: self,
            buffer: self.buffer()?,
        })
    }
}
impl Mapping {
    pub fn point(self, (x, y): (f64, f64)) -> (f32, f32) {
        let [sx, sy] = self.ratios();
        ((x * sx) as f32, (y * sy) as f32)
    }
    pub(super) fn ratios(self) -> [f64; 2] {
        let (w, h) = self.buffer;
        let (lw, lh) = self.geometry.logical;
        [f64::from(w) / f64::from(lw), f64::from(h) / f64::from(lh)]
    }
}
#[derive(Default)]
pub(super) struct Protocols {
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
}
impl Protocols {
    pub fn bind(
        globals: &GlobalList,
        qh: &QueueHandle<WindowApp>,
        surface: &wl_surface::WlSurface,
    ) -> Self {
        // Do not request a fractional preference we cannot map onto the surface.
        let Ok(v) = globals.bind::<WpViewporter, _, _>(qh, 1..=1, ()) else {
            return Self::default();
        };
        let Ok(f) = globals.bind::<WpFractionalScaleManagerV1, _, _>(qh, 1..=1, ()) else {
            v.destroy();
            return Self::default();
        };
        let viewport = v.get_viewport(surface, qh, ());
        let fractional = f.get_fractional_scale(surface, qh, ());
        v.destroy();
        f.destroy();
        Self {
            viewport: Some(viewport),
            fractional: Some(fractional),
        }
    }
    pub fn apply(&self, surface: &wl_surface::WlSurface, g: Geometry) {
        let (buffer_scale, dst) = match g.scale {
            Scale::Integer(i) => (i as i32, (-1, -1)),
            Scale::Fractional(_) => (1, (g.logical.0 as i32, g.logical.1 as i32)),
        };
        if surface.version() >= 3 {
            surface.set_buffer_scale(buffer_scale);
        }
        if let Some(v) = &self.viewport {
            v.set_destination(dst.0, dst.1);
        }
    }
}
impl Drop for Protocols {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}
delegate_noop!(WindowApp: ignore WpViewport);
delegate_noop!(WindowApp: ignore WpViewporter);
delegate_noop!(WindowApp: ignore WpFractionalScaleManagerV1);
impl Dispatch<WpFractionalScaleV1, ()> for WindowApp {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Event::PreferredScale { scale } = event {
            state.set_scale(Scale::Fractional(scale.max(120)));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_points_follow_each_rounded_dimension_not_the_nominal_scale() {
        let g = Geometry {
            logical: (101, 100),
            scale: Scale::Fractional(180),
        };
        assert_eq!(g.nominal(), 1.5);
        assert_eq!(g.buffer().unwrap(), (152, 150));
        assert_eq!(g.mapping().unwrap().point((50.5, 25.)), (76., 37.5));
        let tiny = Geometry {
            logical: (1, 1),
            scale: Scale::Fractional(150),
        };
        let integer = Geometry {
            scale: Scale::Integer(1),
            ..tiny
        };
        assert_eq!(tiny.buffer(), integer.buffer());
        assert_ne!(tiny, integer);
        assert!(
            Geometry {
                logical: (u32::MAX, 2),
                ..g
            }
            .buffer()
            .is_err()
        );
    }
}
