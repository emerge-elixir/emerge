//! Rotation of the UI on a scanout whose orientation differs from the layout, such as a portrait
//! panel showing a landscape UI. The renderer draws through `transform`; input maps back through
//! `to_logical`.

use crate::tree::transform::{Affine2, Point};

/// Counter-clockwise quarter turns of the UI on the scanout, as DRM's `DRM_MODE_ROTATE_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputRotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl OutputRotation {
    pub fn from_degrees(degrees: u32) -> Option<Self> {
        match degrees {
            0 => Some(Self::Deg0),
            90 => Some(Self::Deg90),
            180 => Some(Self::Deg180),
            270 => Some(Self::Deg270),
            _ => None,
        }
    }

    /// Layout size for a scanout of `physical` size, and the reverse.
    pub fn logical_size(self, physical: (u32, u32)) -> (u32, u32) {
        if matches!(self, Self::Deg90 | Self::Deg270) {
            (physical.1, physical.0)
        } else {
            physical
        }
    }

    /// Maps layout coordinates to scanout coordinates.
    pub fn transform(self, physical: (u32, u32)) -> Affine2 {
        let (w, h) = (physical.0 as f32, physical.1 as f32);
        match self {
            Self::Deg0 => Affine2::identity(),
            Self::Deg90 => Affine2 {
                xx: 0.0,
                yx: -1.0,
                xy: 1.0,
                yy: 0.0,
                tx: 0.0,
                ty: h,
            },
            Self::Deg180 => Affine2 {
                xx: -1.0,
                yx: 0.0,
                xy: 0.0,
                yy: -1.0,
                tx: w,
                ty: h,
            },
            Self::Deg270 => Affine2 {
                xx: 0.0,
                yx: 1.0,
                xy: -1.0,
                yy: 0.0,
                tx: w,
                ty: 0.0,
            },
        }
    }

    /// Maps a layout point, such as the cursor position, to scanout coordinates.
    pub fn to_physical(self, (x, y): (f32, f32), physical: (u32, u32)) -> (f32, f32) {
        let point = self.transform(physical).map_point(Point { x, y });
        (point.x, point.y)
    }

    /// Turns a square bitmap with 4 bytes per pixel, such as a cursor plane image, the way
    /// `transform` turns the UI.
    pub fn rotate_square_pixels(self, pixels: &[u8], side: u32) -> Vec<u8> {
        let side_px = side as usize;
        (0..side_px * side_px)
            .flat_map(|index| {
                let (x, y) = (index % side_px, index / side_px);
                let (sx, sy) = self.to_logical((x as f64 + 0.5, y as f64 + 0.5), (side, side));
                let source = (sy as usize * side_px + sx as usize) * 4;
                pixels[source..source + 4].iter().copied()
            })
            .collect()
    }

    /// Maps a scanout point, such as a touch contact, to layout coordinates.
    pub fn to_logical(self, (x, y): (f64, f64), physical: (u32, u32)) -> (f64, f64) {
        let (w, h) = (physical.0 as f64, physical.1 as f64);
        match self {
            Self::Deg0 => (x, y),
            Self::Deg90 => (h - y, x),
            Self::Deg180 => (w - x, h - y),
            Self::Deg270 => (y, w - x),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::transform::Point;

    const PANEL: (u32, u32) = (800, 1280);

    #[test]
    fn parses_quarter_turns_only() {
        assert_eq!(
            OutputRotation::from_degrees(90),
            Some(OutputRotation::Deg90)
        );
        assert_eq!(OutputRotation::from_degrees(45), None);
    }

    #[test]
    fn quarter_turns_swap_the_layout_size() {
        assert_eq!(OutputRotation::Deg90.logical_size(PANEL), (1280, 800));
        assert_eq!(OutputRotation::Deg180.logical_size(PANEL), PANEL);
        assert_eq!(OutputRotation::Deg270.logical_size(PANEL), (1280, 800));
    }

    #[test]
    fn deg90_puts_the_layout_origin_at_the_bottom_left_of_the_panel() {
        let transform = OutputRotation::Deg90.transform(PANEL);
        let origin = transform.map_point(Point { x: 0.0, y: 0.0 });
        assert_eq!((origin.x, origin.y), (0.0, 1280.0));
        let right = transform.map_point(Point { x: 1280.0, y: 0.0 });
        assert_eq!((right.x, right.y), (0.0, 0.0));
    }

    #[test]
    fn rotate_square_pixels_turns_the_bitmap_like_the_ui() {
        // 2 x 2 bitmap, one byte value per pixel: a b / c d.
        let pixels: Vec<u8> = [1, 2, 3, 4].iter().flat_map(|&v| [v; 4]).collect();
        let first_bytes = |rotation: OutputRotation| {
            rotation
                .rotate_square_pixels(&pixels, 2)
                .chunks(4)
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>()
        };
        assert_eq!(first_bytes(OutputRotation::Deg0), [1, 2, 3, 4]);
        // Counter-clockwise: the top row (a b) becomes the left column, read bottom to top.
        assert_eq!(first_bytes(OutputRotation::Deg90), [2, 4, 1, 3]);
        assert_eq!(first_bytes(OutputRotation::Deg180), [4, 3, 2, 1]);
        assert_eq!(first_bytes(OutputRotation::Deg270), [3, 1, 4, 2]);
    }

    #[test]
    fn rotated_cursor_plane_puts_the_hotspot_on_the_cursor() {
        let side = 64;
        let hotspot = (7.0, 2.0);
        let cursor = (100.0, 50.0);
        for rotation in [
            OutputRotation::Deg0,
            OutputRotation::Deg90,
            OutputRotation::Deg180,
            OutputRotation::Deg270,
        ] {
            let plane_hotspot = rotation.to_physical(hotspot, (side, side));
            let device_cursor = rotation.to_physical(cursor, PANEL);
            let plane_origin = (
                device_cursor.0 - plane_hotspot.0,
                device_cursor.1 - plane_hotspot.1,
            );
            // The hotspot pixel of the turned bitmap is at the device cursor position.
            assert_eq!(
                (
                    plane_origin.0 + plane_hotspot.0,
                    plane_origin.1 + plane_hotspot.1
                ),
                device_cursor,
                "{rotation:?}"
            );
            let back = rotation.to_logical((device_cursor.0 as f64, device_cursor.1 as f64), PANEL);
            assert_eq!(back, (cursor.0 as f64, cursor.1 as f64), "{rotation:?}");
        }
    }

    #[test]
    fn to_logical_inverts_transform() {
        for rotation in [
            OutputRotation::Deg90,
            OutputRotation::Deg180,
            OutputRotation::Deg270,
        ] {
            let transform = rotation.transform(PANEL);
            for logical in [(0.0, 0.0), (100.0, 250.0), (640.0, 400.0), (799.0, 799.0)] {
                let device = transform.map_point(Point {
                    x: logical.0 as f32,
                    y: logical.1 as f32,
                });
                let back = rotation.to_logical((device.x as f64, device.y as f64), PANEL);
                assert_eq!(back, logical, "{rotation:?} {logical:?}");
            }
        }
    }
}
