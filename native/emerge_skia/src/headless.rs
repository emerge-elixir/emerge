//! Binary CPU headless session. The session lock orders edits, successful draws
//! and raw input. A wake carries no tree or hit data; a pending edit is not a draw.
mod capture;
mod dispatch;
mod frame;
use crate::{
    atoms, binary, input,
    lifecycle::{Lifecycle, Status},
    lock, submitted,
    tree::{Raster, Tree},
};
use crossbeam_channel::{Receiver, Sender, bounded};
use dispatch::Dispatcher;
use frame::Channels;
use rustler::{
    Atom, Binary, Env, LocalPid, Monitor, NifResult, OwnedEnv, Resource, ResourceArc, Term,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
rustler::atoms! {emerge_skia_config,emerge_skia_log,error}

#[derive(rustler::NifMap)]
struct Output {
    mode: String,
    target: Option<LocalPid>,
    frame_message: String,
    pixel_format: String,
    dither: bool,
}
#[derive(rustler::NifMap)]
struct Api {
    kind: String,
    raster_present: String,
}
#[derive(rustler::NifMap)]
struct Start {
    backend: String,
    rendering_api: Api,
    headless: Output,
    width: u32,
    height: u32,
    owner: Option<LocalPid>,
    target_fps: Option<u32>,
    tree_headroom_percent: u16,
}
struct Live {
    raster: Raster,
    input: Dispatcher,
    log: Option<LocalPid>,
    output: LocalPid,
    message: String,
    channels: Channels,
    width: u32,
    height: u32,
    request: bool,
    last: Option<Instant>,
    interval: Option<Duration>,
}
struct Owner {
    lifecycle: Lifecycle,
    live: Mutex<Option<Live>>,
    revision: AtomicU64,
    api: String,
}
struct Session {
    owner: Arc<Owner>,
    wake: Sender<()>,
}
impl Session {
    fn live<T>(&self, f: impl FnOnce(&mut Live) -> Result<T, String>) -> Result<T, String> {
        let mut live = lock(&self.owner.live)?;
        if !self.running() {
            return Err("renderer is stopping".into());
        }
        f(live.as_mut().ok_or("renderer is stopped")?)
    }
    fn running(&self) -> bool {
        self.owner.lifecycle.running.load(Ordering::Acquire)
    }
    fn request_stop(&self) {
        if self.owner.lifecycle.begin_cleanup() {
            let _ = self.wake.try_send(());
        }
    }
    fn stop(&self, ms: u32) -> Result<(), String> {
        self.request_stop();
        self.owner.lifecycle.wait(Duration::from_millis(ms.into()))
    }
    fn edit(
        &self,
        bytes: &[u8],
        edit: fn(&mut Tree, &[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        self.live(|live| {
            edit(&mut live.raster.tree, bytes)?;
            live.request = true;
            Ok(())
        })?;
        let _ = self.wake.try_send(());
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.request_stop()
    }
}
#[rustler::resource_impl]
impl Resource for Session {
    fn down(&self, _: Env, _: LocalPid, _: Monitor) {
        self.request_stop()
    }
}
fn interval(fps: Option<u32>) -> Result<Option<Duration>, String> {
    match fps {
        Some(0) => Err("target_fps must be a positive integer or nil".into()),
        _ => Ok(fps.map(|value| Duration::from_secs_f64(1. / f64::from(value)))),
    }
}
fn run(owner: &Owner, rx: Receiver<()>) -> Result<(), String> {
    let mut env = OwnedEnv::new();
    loop {
        let deadline = {
            let mut guard = lock(&owner.live)?;
            if owner.lifecycle.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            let live = guard.as_mut().ok_or("renderer is stopped")?;
            let deadline = live.last.zip(live.interval).map(|(t, d)| t + d);
            if !live.request {
                None
            } else if deadline.is_some_and(|d| d > Instant::now()) {
                deadline
            } else {
                live.request = false;
                let Live {
                    raster,
                    width,
                    height,
                    output,
                    message,
                    channels,
                    ..
                } = live;
                match raster.draw(*width as f32, *height as f32, 1.) {
                    Ok(pixels) => {
                        env.send_and_clear(output, |e| {
                            channels.message(e, message, *width, *height, pixels)
                        })
                        .map_err(|_| "headless output target is gone")?;
                        live.input.after_draw(raster);
                        live.last = Some(Instant::now());
                    }
                    Err(why) => {
                        if let Some(log) = live.log {
                            let _ = env.send_and_clear(&log, |_| {
                                (emerge_skia_log(), error(), "headless", why)
                            });
                        }
                    }
                }
                None
            }
        };
        match deadline {
            Some(t) => {
                let _ = rx.recv_deadline(t);
            }
            None => {
                if rx.recv().is_err() {
                    return Ok(());
                }
            }
        }
    }
}
fn start(env: Env, opts: Start) -> Result<ResourceArc<Session>, String> {
    if opts.backend != "headless"
        || opts.headless.mode != "binary"
        || !matches!(opts.rendering_api.kind.as_str(), "auto" | "raster")
        || !matches!(opts.rendering_api.raster_present.as_str(), "auto" | "cpu")
    {
        return Err("only binary headless raster is supported in this build".into());
    }
    if opts.tree_headroom_percent > 1000 {
        return Err("tree_headroom_percent exceeds 1000".into());
    }
    if opts.headless.dither {
        return Err("dithered output is not in the first slice".into());
    }
    if opts.width == 0
        || opts.height == 0
        || u64::from(opts.width) * u64::from(opts.height) > 64 * 1024 * 1024
    {
        return Err("invalid headless output size".into());
    }
    let owner = Arc::new(Owner {
        lifecycle: Lifecycle::default(),
        revision: AtomicU64::new(0),
        api: opts.rendering_api.kind,
        live: Mutex::new(Some(Live {
            raster: Raster::with_tree(Tree::with_headroom(opts.tree_headroom_percent)),
            input: Dispatcher::default(),
            log: Some(env.pid()),
            output: opts.headless.target.ok_or("missing headless target")?,
            message: opts.headless.frame_message,
            channels: Channels::parse(&opts.headless.pixel_format)?,
            width: opts.width,
            height: opts.height,
            request: false,
            last: None,
            interval: interval(opts.target_fps)?,
        })),
    });
    let (wake, rx) = bounded(1);
    let session = ResourceArc::new(Session {
        owner: owner.clone(),
        wake,
    });
    if let Some(pid) = opts.owner
        && session.monitor(Some(env), &pid).is_none()
    {
        return Err("renderer owner is not alive".into());
    }
    std::thread::Builder::new()
        .name("emerge-headless".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&owner, rx)));
            match result {
                Ok(Err(e)) => owner.lifecycle.fail(e),
                Err(_) => owner.lifecycle.fail("headless renderer panicked"),
                Ok(Ok(())) => (),
            }
            owner.lifecycle.request_stop();
            owner.live.lock().unwrap_or_else(|p| p.into_inner()).take();
            owner.lifecycle.complete(Ok(()));
        })
        .map_err(|e| e.to_string())?;
    Ok(session)
}
#[rustler::nif(schedule = "DirtyIo")]
fn start_opts(env: Env, opts: Start) -> NifResult<ResourceArc<Session>> {
    start(env, opts).map_err(|e| rustler::Error::Term(Box::new(e)))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_upload(r: ResourceArc<Session>, data: Binary) -> NifResult<Atom> {
    submitted(r.edit(data.as_slice(), Tree::upload))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_patch(r: ResourceArc<Session>, data: Binary) -> NifResult<Atom> {
    if data.is_empty() {
        submitted(r.live(|_| Ok(())))
    } else {
        submitted(r.edit(data.as_slice(), Tree::patch))
    }
}
#[rustler::nif(schedule = "DirtyCpu")]
fn send_input(env: Env, r: ResourceArc<Session>, raw: Term) -> NifResult<Atom> {
    let event = dispatch::decode(raw)
        .map_err(|_| rustler::Error::Term(Box::new("invalid or unsupported raw input")))?;
    submitted(r.live(|live| {
        live.input.dispatch(env, &live.raster, event);
        Ok(())
    }))
}
#[rustler::nif(schedule = "DirtyIo")]
fn set_input_target(r: ResourceArc<Session>, pid: Option<LocalPid>) -> NifResult<Atom> {
    submitted(r.live(|s| {
        s.input.set_target(pid);
        Ok(())
    }))
}
#[rustler::nif(schedule = "DirtyIo")]
fn set_input_mask(r: ResourceArc<Session>, mask: u32) -> NifResult<Atom> {
    submitted(r.live(|s| {
        if mask > input::INPUT_MASK_ALL {
            return Err("invalid input mask".into());
        }
        s.input.observer.set_mask(mask);
        Ok(())
    }))
}
#[rustler::nif(schedule = "DirtyIo")]
fn set_log_target(r: ResourceArc<Session>, pid: Option<LocalPid>) -> NifResult<Atom> {
    submitted(r.live(|s| {
        s.log = pid;
        Ok(())
    }))
}
#[rustler::nif(schedule = "DirtyIo")]
fn configure_assets_nif(r: ResourceArc<Session>, _opts: Term) -> NifResult<Atom> {
    // No supported declaration resolves an external asset.
    submitted(r.live(|_| Ok(())))
}
#[rustler::nif]
fn load_font_nif(
    _r: ResourceArc<Session>,
    _name: &str,
    _weight: u32,
    _italic: bool,
    _data: Binary,
) -> NifResult<Atom> {
    submitted(Err("custom fonts are not in the first slice".into()))
}
#[rustler::nif]
fn is_running(r: ResourceArc<Session>) -> bool {
    r.running()
}
#[rustler::nif]
fn renderer_status(r: ResourceArc<Session>) -> Result<Status, String> {
    r.owner.lifecycle.status()
}
#[rustler::nif(schedule = "DirtyIo")]
fn stop(r: ResourceArc<Session>) -> Result<Atom, String> {
    r.stop(5000)?;
    Ok(atoms::ok())
}
#[rustler::nif(schedule = "DirtyIo")]
fn stop_timeout(r: ResourceArc<Session>, ms: u32) -> NifResult<Atom> {
    submitted(r.stop(ms))
}
#[rustler::nif(schedule = "DirtyIo")]
fn set_frame_rate(r: ResourceArc<Session>, fps: Option<u32>) -> NifResult<Atom> {
    submitted(r.live(|s| {
        s.interval = interval(fps)?;
        let _ = r.wake.try_send(());
        Ok(())
    }))
}
#[derive(rustler::NifMap)]
struct Control {
    title: Option<String>,
    input_mask: u32,
    target_fps: Option<u32>,
}
#[rustler::nif(schedule = "DirtyIo")]
fn configure_nif(env: Env, r: ResourceArc<Session>, config: Control) -> Result<(u64, u64), String> {
    r.live(|s| {
        if config.title.is_some() || config.input_mask > input::INPUT_MASK_ALL {
            return Err("invalid headless control".into());
        }
        s.interval = interval(config.target_fps)?;
        s.input.observer.set_mask(config.input_mask);
        let token = (
            r.owner.lifecycle.id,
            r.owner.revision.fetch_add(1, Ordering::AcqRel) + 1,
        );
        let _ = env.send(&env.pid(), (emerge_skia_config(), token, atoms::ok()));
        let _ = r.wake.try_send(());
        Ok(token)
    })
}
#[derive(rustler::NifMap)]
struct ConfigStatus {
    session_id: u64,
    desired_revision: u64,
    pending_revision: Option<u64>,
    applied_revision: Option<u64>,
    failed_revision: Option<u64>,
    error: Option<String>,
}
#[rustler::nif]
fn config_status(r: ResourceArc<Session>) -> ConfigStatus {
    let rev = r.owner.revision.load(Ordering::Acquire);
    ConfigStatus {
        session_id: r.owner.lifecycle.id,
        desired_revision: rev,
        pending_revision: None,
        applied_revision: (rev != 0).then_some(rev),
        failed_revision: None,
        error: None,
    }
}
#[derive(rustler::NifMap)]
struct RenderingApi {
    requested: String,
    selected: String,
}
#[derive(rustler::NifMap)]
struct Capabilities {
    gpu: bool,
    renderer_cache: bool,
    screenshot: bool,
    raster_present: Vec<String>,
    prime_video: bool,
    prime_video_formats: Vec<String>,
}
#[derive(rustler::NifMap)]
struct RendererInfo {
    backend: String,
    rendering_api: RenderingApi,
    capabilities: Capabilities,
    vulkan_device: Option<Atom>,
}
#[rustler::nif]
fn renderer_info(r: ResourceArc<Session>) -> Result<RendererInfo, String> {
    Ok(RendererInfo {
        backend: "headless".into(),
        rendering_api: RenderingApi {
            requested: r.owner.api.clone(),
            selected: "raster".into(),
        },
        capabilities: Capabilities {
            gpu: false,
            renderer_cache: false,
            screenshot: true,
            raster_present: vec!["cpu".into()],
            prime_video: false,
            prime_video_formats: vec![],
        },
        vulkan_device: None,
    })
}
