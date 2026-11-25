//! The session lock orders accepted edits, successful output and raw input.
//! A wake carries no tree or hit data; a pending edit is not a draw.
mod capture;
mod dispatch;
mod frame;
mod images;
mod stats;
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod wayland;
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
rustler::atoms! {emerge_skia_config,emerge_skia_log,emerge_skia_route_changes,emerge_skia_install_routes,error}

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
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    title: String,
    rendering_api: Api,
    headless: Output,
    width: u32,
    height: u32,
    owner: Option<LocalPid>,
    target_fps: Option<u32>,
    stats_enabled: bool,
    renderer_stats_log: bool,
    tree_headroom_percent: u16,
}
struct Live {
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    toplevel: Option<wayland::Toplevel>,
    raster: Raster,
    assets: Option<images::Assets>,
    input: Dispatcher,
    route_target: Option<LocalPid>,
    log: Option<LocalPid>,
    stats: Option<stats::Samples>,
    width: u32,
    height: u32,
    request: bool,
    last: Option<Instant>,
    interval: Option<Duration>,
}
impl Live {
    fn title(&self, _title: String) -> Result<(), String> {
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        if let Some(top) = &self.toplevel {
            top.set_title(_title);
            return Ok(());
        }
        Err("invalid headless control".into())
    }
    fn settle(&mut self, size: (u32, u32), scale: f32) -> Result<Option<bool>, String> {
        let changed = self.raster.settle(size.0 as f32, size.1 as f32, scale)?;
        if self.raster.tree.waiting_for_pixels() {
            if let Some(assets) = &self.assets {
                assets.notify_fits();
            }
            return Ok(None);
        }
        Ok(Some(changed))
    }
    fn present(
        &mut self,
        env: &mut OwnedEnv,
        size: (u32, u32),
        scale: f32,
        backend: &'static str,
        submit: impl FnOnce(&mut OwnedEnv, &[u8], bool) -> Result<bool, String>,
    ) -> Result<bool, String> {
        self.request = false;
        let prepared = match self.settle(size, scale) {
            Ok(Some(c)) => self.raster.prepare_settled(c),
            Ok(None) => return Ok(false),
            Err(why) => Err(why),
        };
        let prepared = match prepared {
            Ok(p) => p,
            Err(why) => {
                self.log_frame_error(env, backend, why);
                return Ok(false);
            }
        };
        let started = self.stats.as_ref().map(|_| Instant::now());
        let mut actual = false;
        prepared.submit(|pixels, reuse| {
            actual = submit(env, pixels, reuse)?;
            Ok(())
        })?;
        if actual && let (Some(s), Some(t)) = (&mut self.stats, started) {
            s.submitted(t.elapsed());
        }
        self.after_present(env, size);
        Ok(true)
    }
    fn after_present(&mut self, env: &mut OwnedEnv, size: (u32, u32)) {
        (self.width, self.height) = size;
        self.input.after_draw(&self.raster);
        if let Some(target) = self.route_target.take() {
            let _ = env.send_and_clear(&target, |_| emerge_skia_install_routes());
        }
        self.last = Some(Instant::now());
    }
    fn log_frame_error(&self, env: &mut OwnedEnv, backend: &'static str, why: String) {
        if let Some(log) = self.log {
            let _ = env.send_and_clear(&log, |_| (emerge_skia_log(), error(), backend, why));
        }
    }
}
struct BinarySink {
    output: LocalPid,
    message: String,
    channels: Channels,
}
struct Owner {
    lifecycle: Lifecycle,
    live: Mutex<Option<Live>>,
    revision: AtomicU64,
    api: String,
    selected: std::sync::OnceLock<Selected>,
    #[cfg(all(
        target_os = "linux",
        feature = "wayland",
        any(feature = "opengl", feature = "vulkan")
    ))]
    readbacks: std::sync::OnceLock<Sender<capture::Readback>>,
}
#[derive(Default)]
enum Selected {
    #[default]
    Raster,
    #[cfg(all(target_os = "linux", feature = "wayland", feature = "opengl"))]
    Gl,
    #[cfg(all(target_os = "linux", feature = "wayland", feature = "vulkan"))]
    Vk(DeviceReport),
}
#[derive(Clone, rustler::NifMap)]
struct DeviceReport {
    physical_device_name: String,
    driver_name: Option<String>,
    driver_id: Option<String>,
    software: bool,
    drm_node: Option<DrmReport>,
}
#[derive(Clone, rustler::NifMap)]
struct DrmReport {
    path: String,
    match_field: String,
    major: u32,
    minor: u32,
}
impl Selected {
    fn name(&self) -> &'static str {
        match self {
            Self::Raster => "raster",
            #[cfg(all(target_os = "linux", feature = "wayland", feature = "opengl"))]
            Self::Gl => "opengl",
            #[cfg(all(target_os = "linux", feature = "wayland", feature = "vulkan"))]
            Self::Vk(_) => "vulkan",
        }
    }
    fn device(&self) -> Option<DeviceReport> {
        match self {
            #[cfg(all(target_os = "linux", feature = "wayland", feature = "vulkan"))]
            Self::Vk(d) => Some(d.clone()),
            _ => None,
        }
    }
}
#[derive(Clone)]
enum Wake {
    Binary(Sender<()>),
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    Window(wayland::Ping),
}
impl Wake {
    fn notify(&self) {
        match self {
            Self::Binary(tx) => {
                let _ = tx.try_send(());
            }
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            Self::Window(w) => w.ping(),
        }
    }
}
struct Session {
    input_budget: crate::raster_assets::InputBudget,
    owner: Arc<Owner>,
    wake: Wake,
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
            self.wake.notify();
        }
    }
    fn stop(&self, ms: u32) -> Result<(), String> {
        self.request_stop();
        self.owner.lifecycle.wait(Duration::from_millis(ms.into()))
    }
    fn edit(&self, bytes: &[u8], routes: Option<Term<'_>>) -> Result<(), String> {
        let edit = crate::tree::Staged::new(bytes, &self.input_budget)?;
        self.live(|live| {
            live.raster.tree.patch_staged(&edit)?;
            if let Some(assets) = &mut live.assets {
                assets.request(&self.owner, &self.wake, &mut live.raster.tree);
            }
            // Send only changed callback addresses down the same ordered channel
            // as clicks. No BEAM term is retained by the renderer.
            if let (Some(edits), Some(target)) = (routes, live.input.target) {
                let _ = edits
                    .get_env()
                    .send(&target, (emerge_skia_route_changes(), edits));
                live.route_target = Some(target);
            }
            live.request = true;
            Ok(())
        })?;
        self.wake.notify();
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
fn run_binary(owner: &Owner, rx: Receiver<()>, sink: BinarySink) -> Result<(), String> {
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
                let size = (live.width, live.height);
                live.present(&mut env, size, 1., "headless", |env, pixels, _| {
                    env.send_and_clear(&sink.output, |e| {
                        sink.channels
                            .message(e, &sink.message, size.0, size.1, pixels)
                    })
                    .map(|_| true)
                    .map_err(|_| "headless output target is gone".to_string())
                })?;
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
// The layout/raster boundary uses f32 device extents. Never advertise a
// buffer whose integer stride or height differs after that conversion.
fn valid_size((w, h): (u32, u32)) -> bool {
    w != 0
        && h != 0
        && u64::from(w) * u64::from(h) <= 64 * 1024 * 1024
        && w as f32 as u32 == w
        && h as f32 as u32 == h
}
fn start(env: Env, opts: Start) -> Result<ResourceArc<Session>, String> {
    if opts.tree_headroom_percent > 1000 {
        return Err("tree_headroom_percent exceeds 1000".into());
    }
    if !valid_size((opts.width, opts.height)) {
        return Err("invalid raster output size".into());
    }
    let driver = Driver::select(&opts)?;
    // Interrupt a compositor that accepts a connection but never answers the
    // registry roundtrip. A startup deadline must not strand a worker thread.
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    let interrupt = match &driver {
        Driver::Wayland { conn, .. } => Some(wayland::startup_socket(conn)?),
        _ => None,
    };
    let owner = Arc::new(Owner {
        lifecycle: Lifecycle::default(),
        revision: AtomicU64::new(0),
        api: opts.rendering_api.kind,
        selected: Default::default(),
        #[cfg(all(
            target_os = "linux",
            feature = "wayland",
            any(feature = "opengl", feature = "vulkan")
        ))]
        readbacks: Default::default(),
        live: Mutex::new(Some(Live {
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            toplevel: None,
            raster: Raster::with_tree(Tree::with_headroom(opts.tree_headroom_percent)),
            input: Dispatcher::default(),
            assets: None,
            route_target: None,
            log: Some(env.pid()),
            stats: (opts.stats_enabled || opts.renderer_stats_log).then(stats::Samples::new),
            width: opts.width,
            height: opts.height,
            request: false,
            last: None,
            interval: interval(opts.target_fps)?,
        })),
    });
    let (ready, rx) = bounded(1);
    let worker = owner.clone();
    std::thread::Builder::new()
        .name(format!("emerge-{}", opts.backend))
        .spawn(move || {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                driver.run(&worker, &ready)
            }))
            .unwrap_or_else(|_| Err("renderer worker panicked".into()));
            if let Err(e) = run {
                let _ = ready.try_send(Err(e.clone()));
                worker.lifecycle.fail(e);
            }
            worker.lifecycle.request_stop();
            let live = worker.live.lock().unwrap_or_else(|p| p.into_inner()).take();
            if let Some(images) = live.and_then(|s| s.assets) {
                images.stop();
            }
            worker.lifecycle.complete(Ok(()));
        })
        .map_err(|e| e.to_string())?;
    let wake = rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
        owner.lifecycle.request_stop();
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        if let Some(socket) = interrupt {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
        "renderer startup did not complete"
    })??;
    let session = ResourceArc::new(Session {
        owner,
        wake,
        input_budget: Default::default(),
    });
    if let Some(pid) = opts.owner
        && session.monitor(Some(env), &pid).is_none()
    {
        return Err("renderer owner is not alive".into());
    }
    Ok(session)
}
type Ready = Sender<Result<Wake, String>>;
enum Driver {
    Binary(BinarySink),
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    Wayland {
        conn: wayland_client::Connection,
        title: String,
        size: (u32, u32),
        api: wayland::Requested,
    },
}
impl Driver {
    fn select(s: &Start) -> Result<Self, String> {
        if !matches!(s.rendering_api.raster_present.as_str(), "cpu" | "auto") {
            return Err("GPU presentation is not supported in this build".into());
        }
        match s.backend.as_str() {
            "headless"
                if s.headless.mode == "binary"
                    && matches!(s.rendering_api.kind.as_str(), "auto" | "raster") =>
            {
                if s.headless.dither {
                    return Err("dithered output is not in the first slice".into());
                }
                Ok(Self::Binary(BinarySink {
                    output: s.headless.target.ok_or("missing headless target")?,
                    message: s.headless.frame_message.clone(),
                    channels: Channels::parse(&s.headless.pixel_format)?,
                }))
            }
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            "wayland" => Ok(Self::Wayland {
                api: wayland::Requested::parse(
                    &s.rendering_api.kind,
                    &s.rendering_api.raster_present,
                )?,
                conn: wayland_client::Connection::connect_to_env()
                    .map_err(|e| format!("Wayland: {e}"))?,
                title: s.title.clone(),
                size: (s.width, s.height),
            }),
            #[cfg(not(all(target_os = "linux", feature = "wayland")))]
            "wayland" => Err("Wayland requires the wayland feature".into()),
            _ => Err("unsupported output: expected binary headless raster".into()),
        }
    }
    fn run(self, owner: &Arc<Owner>, ready: &Ready) -> Result<(), String> {
        match self {
            Self::Binary(sink) => {
                let (tx, rx) = bounded(1);
                ready
                    .send(Ok(Wake::Binary(tx)))
                    .map_err(|_| "startup cancelled")?;
                run_binary(owner, rx, sink)
            }
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            Self::Wayland {
                conn,
                title,
                size,
                api,
            } => wayland::run(owner, ready, conn, title, size, api),
        }
    }
}
#[rustler::nif(schedule = "DirtyIo")]
fn start_opts(env: Env, opts: Start) -> NifResult<ResourceArc<Session>> {
    start(env, opts).map_err(|e| rustler::Error::Term(Box::new(e)))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_upload(r: ResourceArc<Session>, data: Binary) -> NifResult<Atom> {
    submitted(r.edit(data.as_slice(), None))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_patch(r: ResourceArc<Session>, submission: Term) -> NifResult<Atom> {
    let (data, routes) = if submission.is_binary() {
        (submission.decode::<Binary>()?, None)
    } else {
        let (data, routes): (Binary, Term) = submission.decode()?;
        if !routes.is_map() {
            return Err(rustler::Error::BadArg);
        }
        (data, Some(routes))
    };
    if data.is_empty() && routes.is_none() {
        submitted(r.live(|_| Ok(())))
    } else {
        submitted(r.edit(data.as_slice(), routes))
    }
}
#[rustler::nif(schedule = "DirtyCpu")]
fn send_input(env: Env, r: ResourceArc<Session>, raw: Term) -> NifResult<Atom> {
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    if !matches!(r.wake, Wake::Binary(_)) {
        return submitted(Err(
            "send_input is only available for binary headless output".into(),
        ));
    }
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
fn configure_assets_nif(
    r: ResourceArc<Session>,
    opts: crate::raster_assets::Settings,
) -> NifResult<Atom> {
    let loader = crate::raster_assets::Loader::new(opts);
    submitted(r.live(|live| {
        if live.assets.is_some() {
            return Err("changing the asset root requires a new renderer".into());
        }
        let mut assets = images::Assets::new(loader?);
        if live.raster.tree.needs_images() {
            assets.request(&r.owner, &r.wake, &mut live.raster.tree);
            live.request = true;
            r.wake.notify();
        }
        live.assets = Some(assets);
        Ok(())
    }))
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
        r.wake.notify();
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
        if config.input_mask > input::INPUT_MASK_ALL {
            return Err("invalid input mask".into());
        }
        let limit = interval(config.target_fps)?;
        if let Some(title) = config.title {
            s.title(title)?;
        }
        s.interval = limit;
        s.input.observer.set_mask(config.input_mask);
        let token = (
            r.owner.lifecycle.id,
            r.owner.revision.fetch_add(1, Ordering::AcqRel) + 1,
        );
        let _ = env.send(&env.pid(), (emerge_skia_config(), token, atoms::ok()));
        r.wake.notify();
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
#[derive(Clone, Debug, rustler::NifMap)]
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
    vulkan_device: Option<DeviceReport>,
}
#[rustler::nif]
fn renderer_info(r: ResourceArc<Session>) -> Result<RendererInfo, String> {
    let selected = r.owner.selected.get().unwrap_or(&Selected::Raster);
    let gpu = !matches!(selected, Selected::Raster);
    Ok(RendererInfo {
        backend: match r.wake {
            Wake::Binary(_) => "headless",
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            Wake::Window(_) => "wayland",
        }
        .into(),
        rendering_api: RenderingApi {
            requested: r.owner.api.clone(),
            selected: selected.name().into(),
        },
        capabilities: Capabilities {
            gpu,
            renderer_cache: false,
            screenshot: true,
            raster_present: if gpu { vec![] } else { vec!["cpu".into()] },
            prime_video: false,
            prime_video_formats: vec![],
        },
        vulkan_device: selected.device(),
    })
}
