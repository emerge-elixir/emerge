//! The worker alone owns the device, the sealed recording, and the submitted
//! image. No GL/Vulkan call below takes the session lock. Edits may reserve
//! output storage, but only this worker can settle or install draw/hit lists.
use super::super::{Live, Selected, capture::Readback};
use super::*;
use crate::tree::GeometryPainter;
use crossbeam_channel::Receiver;
use rustler::LocalPid;
use skia_safe::{
    AlphaType, BlendMode, Color, ColorType, ImageInfo, Paint, Picture, PictureRecorder, Surface,
    gpu::{self as ganesh, DirectContext},
};
rustler::atoms! { warning, emerge_skia_seal_routes, emerge_skia_publish_routes, emerge_skia_retry_routes }
#[derive(Debug)]
pub(super) enum ProbeError {
    Unavailable(String),
    Fatal(String),
}
impl From<String> for ProbeError {
    fn from(s: String) -> Self {
        Self::Fatal(s)
    }
}
#[cfg(feature = "vulkan")]
impl ProbeError {
    pub(super) fn reason(self) -> String {
        match self {
            Self::Unavailable(s) | Self::Fatal(s) => s,
        }
    }
}
enum Device {
    #[cfg(feature = "opengl")]
    Gl(gl::Gl),
    #[cfg(feature = "vulkan")]
    Vk(vulkan::VkPresenter),
}
impl Device {
    fn acquire(&mut self, size: (u32, u32)) -> Result<bool, String> {
        match self {
            #[cfg(feature = "opengl")]
            Self::Gl(g) => {
                g.resize(size)?;
                Ok(true)
            }
            #[cfg(feature = "vulkan")]
            Self::Vk(v) => v.acquire(size),
        }
    }
    fn recreate(&self) -> bool {
        match self {
            #[cfg(feature = "opengl")]
            Self::Gl(_) => false,
            #[cfg(feature = "vulkan")]
            Self::Vk(v) => v.recreate,
        }
    }
    fn context(&mut self) -> Result<&mut DirectContext, String> {
        match self {
            #[cfg(feature = "opengl")]
            Self::Gl(g) => Ok(g.context()),
            #[cfg(feature = "vulkan")]
            Self::Vk(v) => v.context(),
        }
    }
    fn surface(&mut self) -> &mut Surface {
        match self {
            #[cfg(feature = "opengl")]
            Self::Gl(g) => g.surface(),
            #[cfg(feature = "vulkan")]
            Self::Vk(v) => v.surface(),
        }
    }
    fn submit(&mut self) -> Result<bool, String> {
        match self {
            #[cfg(feature = "opengl")]
            Self::Gl(g) => g.swap().map(|()| true),
            #[cfg(feature = "vulkan")]
            Self::Vk(v) => v.present(),
        }
    }
}
struct Sealed {
    geometry: Geometry,
    record: Picture,
    routes: Option<LocalPid>,
}
pub(super) struct Output {
    // Drop both offscreen targets before their owning device/context.
    front: Option<Surface>,
    back: Option<Surface>,
    sealed: Option<Sealed>,
    retry_at: Option<Instant>,
    painter: GeometryPainter,
    captures: Receiver<Readback>,
    device: Device,
}
fn target(ctx: &mut DirectContext, (w, h): (u32, u32)) -> Result<Surface, String> {
    let info = ImageInfo::new(
        (w as i32, h as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    ganesh::surfaces::render_target(
        ctx,
        ganesh::Budgeted::Yes,
        &info,
        4,
        ganesh::SurfaceOrigin::TopLeft,
        None,
        false,
        false,
    )
    .ok_or_else(|| "could not allocate multisampled GPU color target".into())
}
impl Output {
    pub(super) fn readback(&mut self) {
        if let Ok(req) = self.captures.try_recv() {
            req.read(self.front.as_mut());
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.sealed.is_some() || self.device.recreate()
    }
    fn retry(&mut self, live: &mut Live, env: &mut OwnedEnv) {
        if let Some(s) = self.sealed.take()
            && let Some(pid) = s.routes
        {
            live.route_target.get_or_insert(pid);
            let _ = env.send_and_clear(&pid, |_| emerge_skia_retry_routes());
        }
        self.retry_at = None;
        live.request = true;
    }
    fn render(&mut self, size: (u32, u32)) -> Result<(), String> {
        let dims = (size.0 as i32, size.1 as i32);
        if self
            .back
            .as_ref()
            .is_none_or(|s| (s.width(), s.height()) != dims)
        {
            self.back = Some(target(self.device.context()?, size)?);
        }
        let back = self.back.as_mut().expect("allocated GPU target");
        back.canvas().clear(Color::TRANSPARENT);
        back.canvas().draw_picture(
            &self.sealed.as_ref().expect("sealed drawing").record,
            None,
            None,
        );
        let image = back.image_snapshot();
        if !image.is_texture_backed() {
            return Err("expected a device-resident color target".into());
        }
        let canvas = self.device.surface().canvas();
        canvas.clear(Color::TRANSPARENT);
        let mut paint = Paint::default();
        paint.set_blend_mode(BlendMode::Src);
        canvas.draw_image(image, (0, 0), Some(&paint));
        Ok(())
    }
}
impl WindowApp {
    pub(super) fn init_device(
        &mut self,
        conn: &Connection,
        request: Requested,
    ) -> Result<(), String> {
        let size = self.geometry.buffer()?;
        let probe = match request {
            Requested::Raster => return Ok(()),
            #[cfg(feature = "opengl")]
            Requested::Gl => {
                gl::Gl::new(conn, &self.window, size).map(|g| (Device::Gl(g), Selected::Gl))
            }
            #[cfg(feature = "vulkan")]
            Requested::Vk => {
                vulkan::VkPresenter::new(conn, &self.window, size, self.feedback.node, &self.owner)
                    .map(|(g, info)| (Device::Vk(g), Selected::Vk(info)))
            }
        };
        let result = probe.and_then(|(mut device, selected)| {
            let ctx = device.context()?;
            if ctx.max_surface_sample_count_for_color_type(ColorType::RGBA8888) < 4 {
                return Err(ProbeError::Unavailable(
                    "RGBA8 4x hardware multisampling is unavailable".into(),
                ));
            }
            let back = Some(target(ctx, size)?);
            let (tx, captures) = crossbeam_channel::bounded(1);
            Ok((
                Output {
                    device,
                    back,
                    front: None,
                    sealed: None,
                    retry_at: None,
                    painter: Default::default(),
                    captures,
                },
                tx,
                selected,
            ))
        });
        if self.owner.lifecycle.stop.load(Ordering::Acquire) {
            return Err("Wayland startup stopped before renderer selection".into());
        }
        let selected = match result {
            Ok((output, tx, selected)) => {
                self.gpu = Some(output);
                self.owner
                    .readbacks
                    .set(tx)
                    .map_err(|_| "capture already initialized")?;
                selected
            }
            Err(ProbeError::Fatal(why)) => return Err(why),
            Err(ProbeError::Unavailable(why)) => {
                let guard = lock(&self.owner.live)?;
                if let Some(pid) = guard.as_ref().and_then(|l| l.log) {
                    let msg = format!(
                        "Wayland: {} requested; {why}; using CPU raster (wl_shm)",
                        self.owner.api
                    );
                    let _ = self.env.send_and_clear(&pid, |_| {
                        (super::super::emerge_skia_log(), warning(), "wayland", msg)
                    });
                }
                Selected::Raster
            }
        };
        self.owner
            .selected
            .set(selected)
            .map_err(|_| "renderer already selected".to_string())
    }
    pub(super) fn present_gpu(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        size: (u32, u32),
        changed_surface: bool,
    ) -> Result<Option<Duration>, String> {
        let gpu = self.gpu.as_mut().expect("selected GPU");
        let owner = &self.owner;
        let mut guard = lock(&owner.live)?;
        let live = guard.as_mut().ok_or("stopped")?;
        if gpu
            .sealed
            .as_ref()
            .is_some_and(|s| s.geometry != self.geometry)
        {
            gpu.retry(live, &mut self.env);
        }
        if let Some(wait) = gpu
            .retry_at
            .and_then(|d| d.checked_duration_since(Instant::now()))
        {
            return Ok(Some(wait));
        }
        if gpu.sealed.is_none() {
            live.request = false;
            let changed =
                match live
                    .raster
                    .settle(size.0 as f32, size.1 as f32, self.geometry.scale as f32)
                {
                    Ok(c) => c,
                    Err(e) => {
                        live.log_frame_error(&mut self.env, "wayland", e);
                        return Ok(None);
                    }
                };
            if !changed && !changed_surface && !gpu.device.recreate() && gpu.front.is_some() {
                live.raster.install();
                live.after_present(&mut self.env, size);
                return Ok(None);
            }
            let mut recorder = PictureRecorder::new();
            let canvas = recorder.begin_recording(
                skia_safe::Rect::from_wh(size.0 as f32, size.1 as f32),
                false,
            );
            // Geometry and commands only; the recorder has no pixel storage or GPU.
            gpu.painter.record(canvas, &live.raster)?;
            let record = recorder
                .finish_recording_as_picture(None)
                .ok_or("failed to seal GPU drawing")?;
            let routes = live.route_target.take();
            if let Some(pid) = routes {
                let _ = self.env.send_and_clear(&pid, |_| emerge_skia_seal_routes());
            }
            gpu.sealed = Some(Sealed {
                geometry: self.geometry,
                record,
                routes,
            });
        }
        drop(guard);
        let retry = Duration::from_millis(4);
        if !gpu.device.acquire(size)? {
            if gpu.device.recreate() {
                // OUT_OF_DATE (not merely NOT_READY): discard this recording
                // and seal the newest accepted edits for the new generation.
                let mut live = lock(&owner.live)?;
                gpu.retry(live.as_mut().ok_or("stopped")?, &mut self.env);
            }
            gpu.retry_at = Some(Instant::now() + retry);
            return Ok(Some(retry));
        }
        gpu.render(size)?;
        let surface = self.window.wl_surface();
        if surface.version() >= 3 {
            surface.set_buffer_scale(self.geometry.scale as i32);
        }
        // An unsuccessful present may leave this request pending on wl_surface.
        // Reuse it on retry rather than requesting a second callback.
        if !self.callback_armed {
            surface.frame(qh, surface.clone());
            self.callback_armed = true;
        }
        let started = Instant::now();
        let submitted = gpu.device.submit()?;
        conn.flush().map_err(err)?;
        let mut guard = lock(&owner.live)?;
        let live = guard.as_mut().ok_or("stopped")?;
        if submitted {
            if let Some(s) = &mut live.stats {
                s.submitted(started.elapsed());
            }
            std::mem::swap(&mut gpu.front, &mut gpu.back);
            let s = gpu.sealed.take().expect("sealed before submit");
            gpu.retry_at = None;
            live.raster.install();
            (live.width, live.height) = size;
            live.input.after_draw(&live.raster);
            live.last = Some(Instant::now());
            self.installed = Some(self.geometry);
            self.frame_pending = true;
            if let Some(pid) = s.routes {
                let _ = self
                    .env
                    .send_and_clear(&pid, |_| emerge_skia_publish_routes());
            }
            Ok(None)
        } else {
            self.frame_pending = false;
            gpu.retry(live, &mut self.env);
            gpu.retry_at = Some(Instant::now() + retry);
            Ok(Some(retry))
        }
    }
}
