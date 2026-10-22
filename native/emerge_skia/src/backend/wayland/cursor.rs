//! Prefer compositor shapes/system themes, but remain usable without installed Xcursors.
use smithay_client_toolkit::{
    compositor::SurfaceData,
    seat::pointer::{CursorIcon as ThemeIcon, PointerData, PointerThemeError, ThemedPointer},
    shm::{
        Shm,
        slot::{Buffer, SlotPool},
    },
};
use wayland_client::{
    Connection, Proxy,
    protocol::{wl_shm, wl_surface::WlSurface},
};

use crate::{backend::cursor_image::builtin_cursor, events::CursorIcon};

fn icon_index(icon: CursorIcon) -> usize {
    match icon {
        CursorIcon::Default => 0,
        CursorIcon::Text => 1,
        CursorIcon::Pointer => 2,
    }
}

fn theme_icon(icon: CursorIcon) -> ThemeIcon {
    match icon {
        CursorIcon::Default => ThemeIcon::Default,
        CursorIcon::Text => ThemeIcon::Text,
        CursorIcon::Pointer => ThemeIcon::Pointer,
    }
}

#[derive(Default)]
struct ThemeCache {
    missing: [bool; 3],
}

impl ThemeCache {
    fn resolve(
        &mut self,
        icon: CursorIcon,
        mut apply: impl FnMut(CursorIcon) -> Result<(), PointerThemeError>,
    ) -> Result<Option<CursorIcon>, PointerThemeError> {
        for candidate in [
            Some(icon),
            (icon != CursorIcon::Default).then_some(CursorIcon::Default),
        ]
        .into_iter()
        .flatten()
        {
            if self.missing[icon_index(candidate)] {
                continue;
            }
            match apply(candidate) {
                Ok(()) => return Ok(Some(candidate)),
                Err(PointerThemeError::CursorNotFound) => {
                    self.missing[icon_index(candidate)] = true
                }
                Err(err) => return Err(err),
            }
        }
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentedCursor {
    Theme(CursorIcon),
    Builtin(CursorIcon),
}

pub(super) struct WaylandCursor {
    pointer: ThemedPointer,
    theme: ThemeCache,
    scale: i32,
    // (requested, actually presented). Never mark a failed operation applied.
    applied: Option<(CursorIcon, PresentedCursor)>,
    fallback: Option<BuiltinBuffers>,
    warned: bool,
}

impl WaylandCursor {
    pub(super) fn new(pointer: ThemedPointer) -> Self {
        Self {
            pointer,
            theme: ThemeCache::default(),
            scale: 0,
            applied: None,
            fallback: None,
            warned: false,
        }
    }

    pub(super) fn surface(&self) -> &WlSurface {
        self.pointer.surface()
    }

    pub(super) fn invalidate(&mut self) {
        self.applied = None;
    }

    pub(super) fn apply(&mut self, conn: &Connection, shm: &Shm, icon: CursorIcon) {
        let surface = self.pointer.surface();
        let scale = surface
            .data::<SurfaceData>()
            .map_or(1, |data| data.scale_factor())
            .max(1);
        if self.scale != scale {
            self.scale = scale;
            self.theme = ThemeCache::default();
            self.fallback = None;
            self.applied = None;
            self.warned = false;
        }
        if self.applied.is_some_and(|(requested, _)| requested == icon) {
            return;
        }
        // Check before theme lookup, including when all candidates are cached misses.
        let Some(serial) = self
            .pointer
            .pointer()
            .data::<PointerData>()
            .and_then(|data| data.latest_enter_serial())
        else {
            return;
        };

        let result = self.theme.resolve(icon, |candidate| {
            self.pointer.set_cursor(conn, theme_icon(candidate))
        });
        let presented = match result {
            Ok(Some(actual)) => PresentedCursor::Theme(actual),
            Ok(None) => match self.apply_builtin(shm, icon, serial) {
                Ok(()) => {
                    self.warn_once("Wayland cursor theme unavailable; using bundled cursors. Check XCURSOR_THEME, XCURSOR_PATH and installed cursor themes.");
                    PresentedCursor::Builtin(icon)
                }
                Err(error) => {
                    self.warn_once(&format!("failed to apply bundled Wayland cursor: {error}"));
                    return;
                }
            },
            Err(PointerThemeError::MissingEnterSerial) => return,
            Err(error) => {
                self.warn_once(&format!("failed to apply Wayland cursor: {error}"));
                return;
            }
        };
        self.applied = Some((icon, presented));
    }

    fn warn_once(&mut self, message: &str) {
        if !self.warned {
            eprintln!("{message}");
            self.warned = true;
        }
    }

    fn apply_builtin(&mut self, shm: &Shm, icon: CursorIcon, serial: u32) -> Result<(), String> {
        if self.fallback.is_none() {
            self.fallback = Some(BuiltinBuffers {
                pool: SlotPool::new(4096, shm).map_err(|err| err.to_string())?,
                buffers: Default::default(),
            });
        }
        let fallback = self.fallback.as_mut().unwrap();
        let entry = &mut fallback.buffers[icon_index(icon)];
        if entry.is_none() {
            let image = rasterize(icon, self.scale)?;
            let (buffer, canvas) = fallback
                .pool
                .create_buffer(
                    image.size,
                    image.size,
                    image.size * 4,
                    wl_shm::Format::Argb8888,
                )
                .map_err(|err| err.to_string())?;
            canvas.copy_from_slice(&image.pixels);
            *entry = Some((buffer, image.hotspot));
        }
        let (buffer, hotspot) = entry.as_ref().unwrap();
        let surface = self.pointer.surface();
        surface.set_buffer_scale(self.scale);
        // All buffers are immutable. Reattaching an active buffer is safe, but
        // rewriting its pixels or recycling its slot before release would not be.
        if buffer.attach_to(surface).is_err() {
            surface.attach(Some(buffer.wl_buffer()), 0, 0);
        }
        // Surface coordinates also support compositors older than damage_buffer.
        surface.damage(0, 0, 24, 24);
        surface.commit();
        self.pointer
            .pointer()
            .set_cursor(serial, Some(surface), hotspot.0, hotspot.1);
        Ok(())
    }
}

struct BuiltinBuffers {
    // Buffer drop defers destruction until compositor release (SCTK SlotPool).
    buffers: [Option<(Buffer, (i32, i32))>; 3],
    pool: SlotPool,
}

struct CursorPixels {
    pixels: Vec<u8>,
    size: i32,
    // wl_pointer hotspots use surface coordinates, not buffer pixels.
    hotspot: (i32, i32),
}

fn rasterize(icon: CursorIcon, scale: i32) -> Result<CursorPixels, String> {
    if !(1..=16).contains(&scale) {
        return Err(format!("unsupported cursor scale {scale}"));
    }
    let (svg, hotspot) = builtin_cursor(icon);
    let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &Default::default())
        .map_err(|err| err.to_string())?;
    let size = 24 * scale;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size as u32, size as u32)
        .ok_or("could not allocate cursor pixels")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|rgba| {
            // tiny-skia supplies premultiplied RGBA; wl_shm requires native-endian ARGB.
            u32::from_be_bytes([rgba[3], rgba[0], rgba[1], rgba[2]]).to_ne_bytes()
        })
        .collect();
    Ok(CursorPixels {
        pixels,
        size,
        hotspot: (hotspot.0.round() as i32, hotspot.1.round() as i32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_theme_is_preferred_and_only_permanent_misses_are_cached() {
        let mut cache = ThemeCache::default();
        assert_eq!(
            cache.resolve(CursorIcon::Pointer, |_| Ok(())).unwrap(),
            Some(CursorIcon::Pointer)
        );
        assert!(
            cache
                .resolve(CursorIcon::Text, |_| Err(
                    PointerThemeError::MissingEnterSerial
                ))
                .is_err()
        );
        assert_eq!(cache.missing, [false; 3]);
        let mut attempts = Vec::new();
        assert_eq!(
            cache
                .resolve(CursorIcon::Text, |icon| {
                    attempts.push(icon);
                    if icon == CursorIcon::Default {
                        Ok(())
                    } else {
                        Err(PointerThemeError::CursorNotFound)
                    }
                })
                .unwrap(),
            Some(CursorIcon::Default)
        );
        assert_eq!(attempts, [CursorIcon::Text, CursorIcon::Default]);
        assert_eq!(
            cache
                .resolve(CursorIcon::Text, |icon| {
                    assert_eq!(icon, CursorIcon::Default);
                    Ok(())
                })
                .unwrap(),
            Some(CursorIcon::Default)
        );
    }

    #[test]
    fn absent_theme_falls_back_without_repeating_failed_lookups() {
        let mut cache = ThemeCache::default();
        for icon in [CursorIcon::Default, CursorIcon::Pointer, CursorIcon::Text] {
            assert_eq!(
                cache
                    .resolve(icon, |_| Err(PointerThemeError::CursorNotFound))
                    .unwrap(),
                None
            );
        }
        for icon in [CursorIcon::Text, CursorIcon::Default, CursorIcon::Pointer] {
            assert_eq!(
                cache
                    .resolve(icon, |_| panic!("cached theme miss retried"))
                    .unwrap(),
                None
            );
        }
        assert_eq!(
            ThemeCache::default()
                .resolve(CursorIcon::Text, |_| Ok(()))
                .unwrap(),
            Some(CursorIcon::Text)
        );
    }

    #[test]
    fn builtin_cursors_have_scaled_premultiplied_pixels_and_logical_hotspots() {
        for icon in [CursorIcon::Default, CursorIcon::Pointer, CursorIcon::Text] {
            let normal = rasterize(icon, 1).unwrap();
            let hidpi = rasterize(icon, 2).unwrap();
            assert_eq!(normal.size, 24);
            assert_eq!(hidpi.size, 48);
            assert_eq!(normal.hotspot, hidpi.hotspot);
            assert_eq!(hidpi.pixels.len(), 48 * 48 * 4);
            let pixels: Vec<_> = hidpi
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_ne_bytes(*bytes))
                .collect();
            assert!(pixels.iter().any(|pixel| pixel >> 24 != 0));
            assert!(pixels.iter().any(|pixel| pixel >> 24 == 0));
            assert!(pixels.iter().all(|pixel| {
                let alpha = pixel >> 24;
                [16, 8, 0]
                    .iter()
                    .all(|shift| (pixel >> shift) & 255 <= alpha)
            }));
        }
        assert!(rasterize(CursorIcon::Default, 0).is_err());
        assert!(rasterize(CursorIcon::Default, i32::MAX).is_err());
    }
}
