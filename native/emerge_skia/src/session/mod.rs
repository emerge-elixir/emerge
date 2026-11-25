//! Independent tree, drawing and publication wakes. An accepted edit is not an
//! installation: input and capture only see the bundle the backend handed off.
mod admission;
mod presentation;
#[cfg(feature = "stage-probes")]
mod probes;
mod timing;
mod tree_actor;
use tree_actor::{Bundle, Completed, Cpu, Pipeline, Signal, Target};
mod capture;
mod control;
mod dispatch;
mod frame;
mod images;
mod mailbox;
mod stats;
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod wayland;
use crate::{
    atoms, binary, input,
    lifecycle::{Lifecycle, Status},
    lock, submitted,
    tree::Tree,
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
rustler::atoms! {emerge_skia_config,emerge_skia_log,emerge_skia_route_changes,emerge_skia_retire_routes,emerge_skia_routed,error}

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
#[derive(Clone)]
enum Pixels {
    Cpu(Arc<Vec<u8>>),
    #[cfg(all(
        target_os = "linux",
        feature = "wayland",
        any(feature = "vulkan", feature = "opengl")
    ))]
    Gpu(Arc<wayland::Lease>),
}
impl Pixels {
    fn cpu(&self) -> Option<&Arc<Vec<u8>>> {
        match self {
            Self::Cpu(p) => Some(p),
            #[cfg(all(
                target_os = "linux",
                feature = "wayland",
                any(feature = "vulkan", feature = "opengl")
            ))]
            Self::Gpu(_) => None,
        }
    }
}
#[derive(Clone)]
struct Installed {
    bundle: Bundle,
    pixels: Pixels,
}
struct Display {
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    toplevel: Option<wayland::Toplevel>,
    installed: Option<Installed>,
    input: Dispatcher,
    stats: Option<stats::Samples>,
}
impl Display {
    fn title(&self, _title: String) -> Result<(), String> {
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        if let Some(top) = &self.toplevel {
            top.set_title(_title);
            return Ok(());
        }
        Err("invalid headless control".into())
    }
    fn install(
        &mut self,
        owner: &Owner,
        bundle: Bundle,
        pixels: Pixels,
        elapsed: Option<Duration>,
    ) -> Result<(), String> {
        debug_assert!(self.installed.as_ref().is_none_or(
            |i| i.bundle.revision <= bundle.revision && i.bundle.id.serial < bundle.id.serial
        ));
        bundle.route.installed_after(
            self.installed.as_ref().map(|i| i.bundle.route.as_ref()),
            owner.lifecycle.id,
        );
        self.input.after_draw(&bundle.scene);
        if let (Some(stats), Some(t)) = (&mut self.stats, elapsed) {
            stats.submitted(t);
        }
        let id = bundle.id;
        let revision = bundle.revision;
        let now = Instant::now();
        bundle.endpoints.observe(now);
        let cost = now.saturating_duration_since(bundle.started);
        let installed = Installed { bundle, pixels };
        let old = lock(&owner.front)?.replace(installed.clone());
        self.installed = Some(installed);
        drop(old);
        owner.submitted(id, revision, now, cost)
    }
    fn dispatch(&mut self, env: Env, event: input::InputEvent) {
        self.input
            .dispatch(env, self.installed.as_ref().map(|i| &i.bundle), event);
    }
}
struct BinarySink {
    output: LocalPid,
    message: String,
    channels: Channels,
}
struct Owner {
    #[cfg(feature = "stage-probes")]
    probes: probes::Probes,
    lifecycle: Lifecycle,
    admission: admission::Admission,
    loader: (
        Sender<crate::raster_assets::Loader>,
        Receiver<crate::raster_assets::Loader>,
    ),
    configured_assets: std::sync::atomic::AtomicBool,
    capturing: std::sync::atomic::AtomicBool,
    pipe: Mutex<Pipeline>,
    actor: Signal,
    render: Signal,
    front: Mutex<Option<Installed>>,
    log: Mutex<Option<LocalPid>>,
    controls: (
        Mutex<Option<Sender<mailbox::Envelope>>>,
        Receiver<mailbox::Envelope>,
    ),
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
impl Owner {
    fn opportunity(&self, open: bool) -> bool {
        #[cfg(feature = "stage-probes")]
        {
            open && self.probes.open(probes::Stage::Opportunity)
        }
        #[cfg(not(feature = "stage-probes"))]
        {
            open
        }
    }
    fn frame_error(&self, env: &mut OwnedEnv, wake: &Wake, why: String) {
        let log = self.log.lock().ok().and_then(|l| *l);
        if let Some(pid) = log {
            let backend = match wake {
                Wake::Binary(_) => "headless",
                #[cfg(all(target_os = "linux", feature = "wayland"))]
                Wake::Window(_) => "wayland",
            };
            let _ = env.send_and_clear(&pid, |_| (emerge_skia_log(), error(), backend, why));
        }
    }
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
    owner: Arc<Owner>,
    wake: Wake,
}
impl Session {
    fn running(&self) -> bool {
        self.owner.lifecycle.running.load(Ordering::Acquire)
    }
    fn request_stop(&self) {
        self.owner.admission.close();
        if self.owner.lifecycle.begin_cleanup() {
            self.owner.actor.notify();
            self.owner.render.notify();
            self.wake.notify();
        }
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
        // A frame-rate ceiling rounds up. Even u32::MAX has a nonzero period.
        _ => Ok(fps.map(|value| Duration::from_nanos(1_000_000_000u64.div_ceil(value.into())))),
    }
}
fn run_binary(
    owner: &Owner,
    rx: Receiver<()>,
    sink: BinarySink,
    mut display: Display,
) -> Result<(), String> {
    let mut env = OwnedEnv::new();
    let mut claim = None;
    while !owner.lifecycle.stop.load(Ordering::Acquire) {
        display.controls(owner, &mut env);
        {
            let mut p = lock(&owner.pipe)?;
            p.submit_open = owner.opportunity(claim.is_none());
            if p.submit_open
                && let Some(f) = p.completed.take()
            {
                #[cfg(feature = "stage-probes")]
                owner
                    .probes
                    .emit(probes::Stage::Claimed, f.id.serial, f.value.bundle.revision);
                p.submit_open = false;
                claim = Some(f);
            }
        }
        owner.render.notify();
        #[cfg(feature = "stage-probes")]
        if !owner.probes.open(probes::Stage::Claimed) {
            if rx.recv().is_err() {
                break;
            }
            continue;
        }
        if let Some(frame) = claim.take() {
            #[cfg(feature = "stage-probes")]
            if owner.probes.reject(frame.id, frame.value.bundle.revision) {
                let loser = lock(&owner.pipe)?.completed.retry(frame);
                drop(loser);
                continue;
            }
            let Completed { bundle, target } = frame.value;
            let (data, rgba) = match target {
                Target::Binary(b, r) => (b, r),
                #[cfg(all(target_os = "linux", feature = "wayland"))]
                _ => return Err("unexpected headless target".into()),
            };
            let (w, h) = bundle.view.size;
            // The binary has already been converted. Only the final small message
            // and installation are ordered against the receiver's next click.
            let started = Instant::now();
            env.send_and_clear(&sink.output, |e| {
                sink.channels.message(e, &sink.message, w, h, data)
            })
            .map_err(|_| "headless output target is gone")?;
            display.install(owner, bundle, Pixels::Cpu(rgba), Some(started.elapsed()))?;
        } else if rx.recv().is_err() {
            break;
        }
    }
    Ok(())
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
fn start(caller: LocalPid, opts: Start) -> Result<ResourceArc<Session>, String> {
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
        #[cfg(feature = "stage-probes")]
        probes: Default::default(),
        lifecycle: Lifecycle::default(),
        admission: admission::Admission::new(opts.tree_headroom_percent),
        loader: bounded(1),
        configured_assets: Default::default(),
        capturing: Default::default(),
        pipe: Mutex::new(Pipeline::new(interval(opts.target_fps)?)),
        actor: Signal::default(),
        render: Signal::default(),
        revision: AtomicU64::new(0),
        api: opts.rendering_api.kind,
        selected: Default::default(),
        #[cfg(all(
            target_os = "linux",
            feature = "wayland",
            any(feature = "opengl", feature = "vulkan")
        ))]
        readbacks: Default::default(),
        controls: {
            let (tx, rx) = bounded(32);
            (Mutex::new(Some(tx)), rx)
        },
        front: Default::default(),
        log: Mutex::new(Some(caller)),
    });
    let display = Display {
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        toplevel: None,
        installed: None,
        input: Default::default(),
        stats: (opts.stats_enabled || opts.renderer_stats_log).then(stats::Samples::new),
    };
    if matches!(driver, Driver::Binary(_)) {
        lock(&owner.pipe)?
            .clock
            .cadence(interval(opts.target_fps)?.unwrap_or(timing::HEADLESS_PERIOD));
        owner.geometry((opts.width, opts.height), 1.)?;
    }
    let (ready, rx) = bounded(1);
    let worker = owner.clone();
    std::thread::Builder::new()
        .name(format!("emerge-{}", opts.backend))
        .spawn(move || {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                driver.run(&worker, &ready, display)
            }))
            .unwrap_or_else(|_| Err("renderer worker panicked".into()));
            if let Err(e) = run {
                let _ = ready.try_send(Err(e.clone()));
                worker.lifecycle.fail(e);
            }
            worker.lifecycle.request_stop();
            worker
                .front
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            worker
                .controls
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            let mut env = OwnedEnv::new();
            for c in worker.controls.1.try_iter() {
                c.stopped(&mut env);
            }
            let mut p = worker.pipe.lock().unwrap_or_else(|p| p.into_inner());
            p.commands.reset(0);
            p.completed.reset(0);
            drop(p);
            let _ = worker.loader.1.try_recv();
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
    let session = ResourceArc::new(Session { owner, wake });
    if let Some(pid) = opts.owner
        && session.monitor(None, &pid).is_none()
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
    fn run(self, owner: &Arc<Owner>, ready: &Ready, display: Display) -> Result<(), String> {
        match self {
            Self::Binary(sink) => {
                let (tx, rx) = bounded(1);
                let wake = Wake::Binary(tx);
                tree_actor::run(owner, &wake, Some(Cpu::Binary(sink.channels)), || {
                    ready
                        .send(Ok(wake.clone()))
                        .map_err(|_| "startup cancelled")?;
                    run_binary(owner, rx, sink, display)
                })
            }
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            Self::Wayland {
                conn,
                title,
                size,
                api,
            } => wayland::run(owner, ready, conn, title, size, api, display),
        }
    }
}
#[rustler::nif(schedule = "DirtyIo")]
fn renderer_assets_nif(
    env: Env,
    r: ResourceArc<Session>,
    opts: crate::raster_assets::Settings,
    id: u64,
) -> NifResult<Atom> {
    if !r.running() {
        return submitted(Err("renderer is stopping".into()));
    }
    if r.owner.configured_assets.swap(true, Ordering::AcqRel) {
        return submitted(Err("changing the asset root requires a new renderer".into()));
    }
    let owner = r.owner.clone();
    let pid = env.pid();
    let job = control::job(&control::ASSETS, "emerge-asset-setup", move || {
        let result = crate::raster_assets::Loader::new(opts).and_then(|loader| {
            if owner.lifecycle.stop.load(Ordering::Acquire) {
                return Err("renderer is stopping".into());
            }
            owner
                .loader
                .0
                .try_send(loader)
                .map_err(|_| "asset setup unavailable")?;
            owner.actor.notify();
            Ok(atoms::ok())
        });
        if result.is_err() {
            owner.configured_assets.store(false, Ordering::Release);
        }
        let _ =
            OwnedEnv::new().send_and_clear(&pid, |_| (control::emerge_skia_reply(), id, result));
    });
    if job.is_err() {
        r.owner.configured_assets.store(false, Ordering::Release);
    }
    job
}
#[rustler::nif]
fn load_font_nif(
    _r: ResourceArc<Session>,
    _name: Term,
    _weight: u32,
    _italic: bool,
    _data: Term,
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
#[derive(rustler::NifMap)]
struct Control {
    title: Option<String>,
    input_mask: u32,
    target_fps: Option<u32>,
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
