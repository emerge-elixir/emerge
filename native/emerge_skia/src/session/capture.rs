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
type Rect = (u32, u32, u32, u32);
#[derive(Clone, Copy)]
struct Region(Option<Rect>);
impl Region {
    fn bounds(self, (width, height): (u32, u32)) -> Result<Rect, String> {
        let (x, y, w, h) = self.0.unwrap_or((0, 0, width, height));
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|v| v > width)
            || y.checked_add(h).is_none_or(|v| v > height)
        {
            Err("invalid screenshot region".into())
        } else {
            Ok((x, y, w, h))
        }
    }
}
type Pixels = (u32, u32, Vec<u8>);
impl Capture<'_> {
    fn region(&self) -> Result<Region, String> {
        if self.scale != 1.
            || self.background != "transparent"
            || self.png_compression != "default"
            || !matches!(self.pixel_format, "rgba8888" | "rgb888")
        {
            return Err("unsupported screenshot options".into());
        }
        match (
            self.region_x,
            self.region_y,
            self.region_width,
            self.region_height,
        ) {
            (None, None, None, None) => Ok(Region(None)),
            (Some(x), Some(y), Some(w), Some(h)) => Ok(Region(Some((x, y, w, h)))),
            _ => Err("invalid screenshot region".into()),
        }
    }
    fn encode(&self, (w, h, mut rgba): Pixels, png: bool) -> Result<Vec<u8>, String> {
        if png {
            crate::services::png(w, h, &mut rgba)
        } else if self.pixel_format == "rgb888" {
            let mut rgb = vec![0; w as usize * h as usize * 3];
            Channels::Rgb.convert(&rgba, &mut rgb);
            Ok(rgb)
        } else {
            Ok(rgba)
        }
    }
}
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
pub(super) struct Readback {
    deadline: Instant,
    region: Region,
    reply: Sender<Result<Pixels, String>>,
}
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
impl Readback {
    pub(super) fn read(self, front: Option<&mut skia_safe::Surface>) {
        if Instant::now() > self.deadline {
            return;
        }
        let pixels = (|| {
            let surface = front.ok_or("no presented frame is available yet")?;
            let (x, y, w, h) = self
                .region
                .bounds((surface.width() as u32, surface.height() as u32))?;
            let info = skia_safe::ImageInfo::new(
                (w as i32, h as i32),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Premul,
                None,
            );
            let stride = w as usize * 4;
            let mut pixels = vec![0; stride * h as usize];
            if !surface.read_pixels(&info, &mut pixels, stride, (x as i32, y as i32)) {
                return Err("GPU readback failed".into());
            }
            Ok((w, h, pixels))
        })();
        let _ = self.reply.try_send(pixels);
    }
}
fn capture(r: &Session, opts: Capture, png: bool) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + Duration::from_millis(opts.timeout_ms.into());
    let region = opts.region()?;
    #[cfg(all(
        target_os = "linux",
        feature = "wayland",
        any(feature = "opengl", feature = "vulkan")
    ))]
    if let Some(requests) = r.owner.readbacks.get() {
        let (tx, rx) = bounded(1);
        requests
            .send_deadline(
                Readback {
                    deadline,
                    region,
                    reply: tx,
                },
                deadline,
            )
            .map_err(|_| "GPU capture is unavailable or timed out")?;
        r.wake.notify();
        let pixels = rx
            .recv_deadline(deadline)
            .map_err(|_| "GPU capture is unavailable or timed out")??;
        return opts.encode(pixels, png);
    }
    loop {
        match r.owner.live.try_lock() {
            Ok(guard) => {
                let live = guard
                    .as_ref()
                    .filter(|_| r.running())
                    .ok_or("renderer is stopped")?;
                let pixels = live.raster.pixels();
                if pixels.is_empty() {
                    return Err("no presented frame is available yet".into());
                }
                let (x, y, w, h) = region.bounds((live.width, live.height))?;
                let mut rgba = vec![0; w as usize * h as usize * 4];
                for (dy, row) in rgba.chunks_exact_mut(w as usize * 4).enumerate() {
                    let off = ((dy + y as usize) * live.width as usize + x as usize) * 4;
                    row.copy_from_slice(&pixels[off..off + row.len()]);
                }
                drop(guard);
                return opts.encode((w, h, rgba), png);
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err("renderer is unavailable".into());
            }
            Err(_) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                if wait.is_zero() {
                    return Err("screenshot timed out".into());
                }
                std::thread::park_timeout(wait.min(Duration::from_millis(1)));
            }
        }
    }
}
#[rustler::nif(schedule = "DirtyIo")]
fn renderer_capture_pixels<'a>(
    env: Env<'a>,
    r: ResourceArc<Session>,
    opts: Capture,
) -> Result<Binary<'a>, String> {
    Ok(binary(env, &capture(&r, opts, false)?))
}
#[rustler::nif(schedule = "DirtyIo")]
fn renderer_capture_png<'a>(
    env: Env<'a>,
    r: ResourceArc<Session>,
    opts: Capture,
) -> Result<Binary<'a>, String> {
    Ok(binary(env, &capture(&r, opts, true)?))
}
