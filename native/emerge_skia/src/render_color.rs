//! Resolved, geometry-bound paint shared by every color-bearing primitive.
use crate::tree::geometry::Rect;
use skia_safe::{
    Paint, Rect as SkRect, TileMode,
    gradient::{Colors, Gradient, Interpolation},
    shaders,
};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum RenderColor {
    Solid(u32),
    Linear {
        colors: Arc<[u32]>,
        angle: f32,
        bounds: Rect,
    },
}

impl From<u32> for RenderColor {
    fn from(value: u32) -> Self {
        Self::Solid(value)
    }
}

impl Hash for RenderColor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Solid(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            Self::Linear {
                colors,
                angle,
                bounds,
            } => {
                1u8.hash(state);
                colors.hash(state);
                (if *angle == 0.0 { 0.0 } else { *angle })
                    .to_bits()
                    .hash(state);
                for v in [bounds.x, bounds.y, bounds.width, bounds.height] {
                    (if v == 0.0 { 0.0 } else { v }).to_bits().hash(state);
                }
            }
        }
    }
}

impl RenderColor {
    pub fn linear(colors: impl Into<Arc<[u32]>>, angle: f64, bounds: Rect) -> Self {
        let colors = colors.into();
        if let Some(first) = colors
            .first()
            .filter(|_| colors.len() >= 2 && angle.is_finite())
            .filter(|first| colors.iter().all(|c| c == *first))
        {
            return Self::Solid(*first);
        }
        Self::Linear {
            colors,
            angle: (angle % 360.0) as f32,
            bounds,
        }
    }

    pub fn with_bounds(&self, bounds: Rect) -> Self {
        match self {
            Self::Solid(_) => self.clone(),
            Self::Linear { colors, angle, .. } => Self::Linear {
                colors: colors.clone(),
                angle: *angle,
                bounds,
            },
        }
    }

    pub fn solid(&self) -> Option<u32> {
        match self {
            Self::Solid(c) => Some(*c),
            _ => None,
        }
    }

    pub fn is_translucent(&self) -> bool {
        match self {
            Self::Solid(c) => c & 255 < 255,
            Self::Linear { .. } => true,
        }
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        match self {
            Self::Solid(_) => self.clone(),
            Self::Linear {
                colors,
                angle,
                bounds,
            } => Self::Linear {
                colors: colors.clone(),
                angle: *angle,
                bounds: Rect {
                    x: bounds.x + dx,
                    y: bounds.y + dy,
                    ..*bounds
                },
            },
        }
    }

    pub fn policy(&self, white: bool) -> Self {
        let recolor = |c: u32| (if white { 0xffffff00 } else { 0 }) | (c & 255);
        match self {
            Self::Solid(c) => Self::Solid(recolor(*c)),
            Self::Linear {
                colors,
                angle,
                bounds,
            } => Self::Linear {
                colors: colors.iter().copied().map(recolor).collect(),
                angle: *angle,
                bounds: *bounds,
            },
        }
    }

    pub fn paint(&self) -> Paint {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        match self {
            Self::Solid(c) => {
                paint.set_color(crate::renderer::color_from_u32(*c));
            }
            Self::Linear {
                colors,
                angle,
                bounds,
            } => {
                // Invalid internal inputs paint nothing; wire input is validated earlier.
                paint.set_color(skia_safe::Color::TRANSPARENT);
                if colors.len() < 2
                    || !angle.is_finite()
                    || bounds.width <= 0.0
                    || bounds.height <= 0.0
                    || ![bounds.x, bounds.y, bounds.width, bounds.height]
                        .iter()
                        .all(|v| v.is_finite())
                {
                    return paint;
                }
                let radians = (*angle as f64).to_radians();
                let cx = bounds.x as f64 + bounds.width as f64 / 2.0;
                let cy = bounds.y as f64 + bounds.height as f64 / 2.0;
                let half = (bounds.width as f64).hypot(bounds.height as f64) / 2.0;
                let start = (
                    (cx - radians.cos() * half) as f32,
                    (cy - radians.sin() * half) as f32,
                );
                let end = (
                    (cx + radians.cos() * half) as f32,
                    (cy + radians.sin() * half) as f32,
                );
                if ![start.0, start.1, end.0, end.1]
                    .iter()
                    .all(|v| v.is_finite())
                {
                    return paint;
                }
                let colors: Vec<_> = colors
                    .iter()
                    .map(|c| crate::renderer::color_from_u32(*c).into())
                    .collect();
                let gradient = Gradient::new(
                    Colors::new_evenly_spaced(&colors, TileMode::Clamp, None),
                    Interpolation::default(),
                );
                if let Some(shader) = shaders::linear_gradient((start, end), &gradient, None) {
                    paint.set_color(skia_safe::Color::WHITE);
                    paint.set_shader(shader);
                }
            }
        }
        paint
    }

    /// Tint only the isolated source's coverage, not the destination background.
    pub fn tint(
        &self,
        canvas: &skia_safe::Canvas,
        bounds: SkRect,
        draw: impl FnOnce(&skia_safe::Canvas),
    ) {
        canvas.save();
        canvas.clip_rect(bounds, skia_safe::ClipOp::Intersect, false);
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().bounds(&bounds));
        draw(canvas);
        let mut paint = self.paint();
        paint.set_blend_mode(skia_safe::BlendMode::SrcIn);
        canvas.draw_rect(bounds, &paint);
        canvas.restore();
        canvas.restore();
    }
}

impl Default for RenderColor {
    fn default() -> Self {
        Self::Solid(0)
    }
}

impl PartialEq<u32> for RenderColor {
    fn eq(&self, other: &u32) -> bool {
        self.solid() == Some(*other)
    }
}

impl std::fmt::Display for RenderColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Solid(color) => write!(f, "0x{color:08X}"),
            Self::Linear { colors, angle, .. } => {
                write!(f, "linear({} stops, {angle}deg)", colors.len())
            }
        }
    }
}
