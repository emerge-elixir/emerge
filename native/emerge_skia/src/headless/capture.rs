use super::*;
#[derive(rustler::NifMap)]
struct Capture<'a> {
    pixel_format: &'a str,
    scale: f64,
    region_x: Option<u32>,
    region_y: Option<u32>,
    region_width: Option<u32>,
    region_height: Option<u32>,
    background: &'a str,
    png_compression: &'a str,
    timeout_ms: u32,
}
impl Capture<'_> {
    fn read(&self, live: &Live, png: bool) -> Result<Vec<u8>, String> {
        if self.scale != 1.
            || self.background != "transparent"
            || self.png_compression != "default"
            || !matches!(self.pixel_format, "rgba8888" | "rgb888")
        {
            return Err("unsupported screenshot options".into());
        }
        let pixels = live.raster.pixels();
        if pixels.is_empty() {
            return Err("no presented frame".into());
        }
        let (x, y, w, h) = match (
            self.region_x,
            self.region_y,
            self.region_width,
            self.region_height,
        ) {
            (None, None, None, None) => (0, 0, live.width, live.height),
            (Some(x), Some(y), Some(w), Some(h))
                if w > 0
                    && h > 0
                    && x.checked_add(w).is_some_and(|v| v <= live.width)
                    && y.checked_add(h).is_some_and(|v| v <= live.height) =>
            {
                (x, y, w, h)
            }
            _ => return Err("invalid screenshot region".into()),
        };
        let rgb = !png && self.pixel_format == "rgb888";
        let bpp = if rgb { 3 } else { 4 };
        let mut dst = vec![0; w as usize * h as usize * bpp];
        for (dy, out) in dst.chunks_exact_mut(w as usize * bpp).enumerate() {
            let offset = ((dy + y as usize) * live.width as usize + x as usize) * 4;
            let rgba = &pixels[offset..offset + w as usize * 4];
            if rgb {
                Channels::Rgb.convert(rgba, out)
            } else {
                out.copy_from_slice(rgba)
            }
        }
        if png {
            crate::services::png(w, h, &mut dst)
        } else {
            Ok(dst)
        }
    }
}
fn capture(r: &Session, opts: Capture, png: bool) -> Result<Vec<u8>, String> {
    let start = Instant::now();
    let max = Duration::from_millis(opts.timeout_ms.into());
    // A screenshot never requests a draw. Bound contention with an in-flight draw.
    loop {
        match r.owner.live.try_lock() {
            Ok(live) => {
                return opts.read(
                    live.as_ref()
                        .filter(|_| r.running())
                        .ok_or("renderer is stopped")?,
                    png,
                );
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err("renderer is unavailable".into());
            }
            Err(_) => {
                let remaining = max.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    return Err("screenshot timed out".into());
                }
                std::thread::park_timeout(remaining.min(Duration::from_millis(1)));
            }
        }
    }
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_capture_pixels<'a>(
    env: Env<'a>,
    r: ResourceArc<Session>,
    opts: Capture,
) -> Result<Binary<'a>, String> {
    Ok(binary(env, &capture(&r, opts, false)?))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_capture_png<'a>(
    env: Env<'a>,
    r: ResourceArc<Session>,
    opts: Capture,
) -> Result<Binary<'a>, String> {
    Ok(binary(env, &capture(&r, opts, true)?))
}
