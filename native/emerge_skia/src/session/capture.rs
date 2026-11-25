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
pub(super) type Rect = (u32, u32, u32, u32);
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
type Rgba = (u32, u32, Vec<u8>);
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
}
/// Capture pins only pixels and their dimensions. Keeping a whole installed
/// bundle here would consume an unrelated command/hit slot and route epoch
/// until slow PNG encoding or device readback finishes.
pub(super) struct Snapshot {
    pixels: Pixels,
    size: (u32, u32),
    #[cfg(feature = "stage-probes")]
    stamp: (u64, u64),
}
impl From<&Installed> for Snapshot {
    fn from(i: &Installed) -> Self {
        Self {
            pixels: i.pixels.clone(),
            size: i.bundle.view.size,
            #[cfg(feature = "stage-probes")]
            stamp: (i.bundle.id.serial, i.bundle.revision),
        }
    }
}
struct Request {
    region: Region,
    deadline: Instant,
    rgb: bool,
    png: bool,
}
impl Request {
    fn encode(&self, (w, h, mut rgba): Rgba) -> Result<Vec<u8>, String> {
        if self.png {
            crate::services::png(w, h, &mut rgba)
        } else if self.rgb {
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
    target: Arc<wayland::Lease>,
    deadline: Instant,
    region: Region,
    reply: Sender<Result<Rgba, String>>,
}
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(feature = "opengl", feature = "vulkan")
))]
impl Readback {
    pub(super) fn read(
        self,
        read: impl FnOnce(&Arc<wayland::Lease>, Rect) -> Result<Vec<u8>, String>,
    ) {
        if Instant::now() > self.deadline {
            return;
        }
        let pixels = (|| {
            let rect = self.region.bounds(self.target.size)?;
            Ok((rect.2, rect.3, read(&self.target, rect)?))
        })();
        let _ = self.reply.try_send(pixels);
    }
}
fn capture(
    r: &Session,
    opts: Request,
    pinned: Receiver<Result<Snapshot, String>>,
) -> Result<Vec<u8>, String> {
    let Request {
        deadline, region, ..
    } = opts;
    let installed = pinned
        .recv_deadline(deadline)
        .map_err(|_| "screenshot timed out")??;
    #[cfg(feature = "stage-probes")]
    r.owner.probes.pause(
        probes::Stage::Pinned,
        installed.stamp.0,
        installed.stamp.1,
        &r.owner,
    );
    if !r.running() {
        return Err("renderer is stopped".into());
    }
    if Instant::now() > deadline {
        return Err("screenshot timed out".into());
    }
    #[cfg(all(
        target_os = "linux",
        feature = "wayland",
        any(feature = "opengl", feature = "vulkan")
    ))]
    if let Some(requests) = r.owner.readbacks.get() {
        let Pixels::Gpu(target) = installed.pixels else {
            return Err("no installed GPU target".into());
        };
        let (tx, rx) = bounded(1);
        requests
            .try_send(Readback {
                target,
                deadline,
                region,
                reply: tx,
            })
            .map_err(|_| "GPU capture is unavailable or timed out")?;
        r.wake.notify();
        let pixels = rx
            .recv_deadline(deadline)
            .map_err(|_| "GPU capture is unavailable or timed out")??;
        return opts.encode(pixels);
    }
    let pixels = installed.pixels.cpu().ok_or("no installed CPU pixels")?;
    let size = installed.size;
    let (x, y, w, h) = region.bounds(size)?;
    let mut rgba = vec![0; w as usize * h as usize * 4];
    for (dy, row) in rgba.chunks_exact_mut(w as usize * 4).enumerate() {
        let off = ((dy + y as usize) * size.0 as usize + x as usize) * 4;
        row.copy_from_slice(&pixels[off..off + row.len()]);
    }
    opts.encode((w, h, rgba))
}
#[rustler::nif(schedule = "DirtyIo")]
fn renderer_capture_nif(
    env: Env,
    r: ResourceArc<Session>,
    opts: Capture,
    png: bool,
    id: u64,
) -> NifResult<Atom> {
    let region = opts
        .region()
        .map_err(|e| rustler::Error::Term(Box::new(e)))?;
    let req = Request {
        region,
        deadline: Instant::now() + Duration::from_millis(opts.timeout_ms.into()),
        rgb: opts.pixel_format == "rgb888",
        png,
    };
    if r.owner.capturing.swap(true, Ordering::AcqRel) {
        return Err(rustler::Error::Term(Box::new(control::busy())));
    }
    struct Lease(ResourceArc<Session>);
    impl Drop for Lease {
        fn drop(&mut self) {
            self.0.owner.capturing.store(false, Ordering::Release);
        }
    }
    let lease = Lease(r.clone());
    let pid = env.pid();
    let (pin, pinned) = bounded(1);
    control::job(&control::CAPTURES, "emerge-capture", move || {
        let result = capture(&lease.0, req, pinned);
        drop(lease);
        let _ = OwnedEnv::new().send_and_clear(&pid, |env| {
            (
                control::emerge_skia_reply(),
                id,
                result.map(|rgba| binary(env, &rgba)),
            )
        });
    })?;
    r.enqueue(mailbox::Envelope::pin(pin))
}
