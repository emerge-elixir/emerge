//! Layout publishes only owned draw and hit data. The presentation gate never
//! prevents a coherent layout turn; an unclaimed drawing may be replaced.
use super::{presentation::*, *};
use crate::tree::Scene;
use std::thread;

#[derive(Clone)]
pub(super) struct Signal(Sender<()>, Receiver<()>);
impl Default for Signal {
    fn default() -> Self {
        let (tx, rx) = bounded(1);
        Self(tx, rx)
    }
}
impl Signal {
    pub fn notify(&self) {
        let _ = self.0.try_send(());
    }
    pub(super) fn wait(&self, deadline: Option<Instant>) {
        match deadline {
            Some(at) => {
                let _ = self.1.recv_deadline(at);
            }
            None => {
                let _ = self.1.recv();
            }
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct View {
    pub size: (u32, u32),
    pub scale: f32,
    pub generation: u64,
}
#[derive(Clone)]
pub(super) struct Bundle {
    pub scene: Scene,
    pub id: Id,
    pub revision: u64,
    pub started: Instant,
    // Stats-only provenance of this sealed scene, never consulted by the selector.
    pub accepted: Option<Instant>,
    pub queued: Option<Instant>,
    pub sample: Option<timing::Tick>,
    pub endpoints: crate::tree::Endpoints,
    pub route: Arc<admission::Route>,
    pub view: View,
}
pub(super) struct Completed {
    pub bundle: Bundle,
    pub target: Target,
}
pub(super) enum Target {
    Binary(rustler::OwnedBinary, Arc<Vec<u8>>),
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    Shm(Arc<smithay_client_toolkit::shm::slot::Buffer>, Arc<Vec<u8>>),
    #[cfg(all(
        target_os = "linux",
        feature = "wayland",
        any(feature = "vulkan", feature = "opengl")
    ))]
    Gpu(Arc<wayland::Lease>),
}
pub(super) struct Pipeline {
    pub view: Option<View>,
    generation: u64,
    pub commands: Latest<Bundle>,
    pub completed: Latest<Completed>,
    pub cadence: Cadence,
    pub clock: timing::Clock,
    pub submit_open: bool,
    last_drawn: u64,
}
impl Pipeline {
    pub fn new(interval: Option<Duration>) -> Self {
        Self {
            view: None,
            generation: 0,
            commands: Latest::new(),
            completed: Latest::new(),
            cadence: Cadence::new(interval),
            clock: Default::default(),
            submit_open: true,
            last_drawn: 0,
        }
    }
    fn next_draw(&mut self, now: Instant) -> (Option<Frame<Bundle>>, Option<Instant>) {
        if self.submit_open && self.completed.pending() {
            return (None, None);
        }
        if self.commands.pending()
            && let Some(t) = self.cadence.due(now)
        {
            if t > now {
                return (None, Some(t));
            }
            self.cadence.start(now);
            let frame = self.commands.take().unwrap();
            self.last_drawn = frame.id.serial;
            return (Some(frame), None);
        }
        (None, None)
    }
}
pub(super) struct Compute {
    pub tree: Tree,
    pub assets: Option<images::Assets>,
    pub request: bool,
    pub route: Arc<admission::Route>,
    pub revision: u64,
}
impl Compute {
    pub fn new(signal: &Signal) -> Self {
        Self {
            tree: Tree::for_session(0),
            assets: None,
            request: false,
            route: admission::Route::initial(signal),
            revision: 0,
        }
    }
    fn drive_images(&mut self, signal: &Signal) -> Result<bool, String> {
        self.assets
            .as_mut()
            .map_or(Ok(false), |a| a.drive(&mut self.tree, signal))
    }
}
impl Owner {
    pub(super) fn draw(&self, p: &mut Pipeline) -> (Option<Frame<Bundle>>, Option<Instant>) {
        #[cfg(feature = "stage-probes")]
        if !self.probes.open(probes::Stage::Commands) {
            return (None, None);
        }
        p.next_draw(Instant::now())
    }
    pub(super) fn completed(&self, frame: Frame<Completed>) -> Result<(), String> {
        #[cfg(feature = "stage-probes")]
        let stamp = (frame.id.serial, frame.value.bundle.revision);
        let old = lock(&self.pipe)?.completed.offer(frame);
        drop(old);
        #[cfg(feature = "stage-probes")]
        self.probes.emit(probes::Stage::Ready, stamp.0, stamp.1);
        self.actor.notify();
        Ok(())
    }
    pub(super) fn geometry(&self, size: (u32, u32), scale: f32) -> Result<(), String> {
        self.set_view(Some((size, scale)))
    }
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub(super) fn invalidate_view(&self) -> Result<(), String> {
        self.set_view(None)
    }
    fn set_view(&self, view: Option<((u32, u32), f32)>) -> Result<(), String> {
        let mut pipe = lock(&self.pipe)?;
        pipe.generation += 1;
        let generation = pipe.generation;
        pipe.view = view.map(|(size, scale)| {
            self.admission.scale(scale);
            View {
                size,
                scale,
                generation,
            }
        });
        let old = (
            pipe.commands.reset(generation),
            pipe.completed.reset(generation),
        );
        pipe.clock.reset(generation);
        pipe.commands.advance(2);
        pipe.cadence.advance(false);
        drop(pipe);
        drop(old);
        self.actor.notify();
        self.render.notify();
        Ok(())
    }
    pub(super) fn submitted(
        &self,
        id: Id,
        _revision: u64,
        at: Instant,
        cost: Duration,
    ) -> Result<(), String> {
        let retired = {
            let mut p = lock(&self.pipe)?;
            if p.generation == id.generation {
                p.clock.progress(at, Some(cost));
                let opportunity = p.clock.opportunity();
                (
                    p.commands.advance(opportunity + 1),
                    p.completed.advance(opportunity),
                )
            } else {
                (None, None)
            }
        };
        drop(retired);
        #[cfg(feature = "stage-probes")]
        self.probes
            .emit(probes::Stage::Installed, id.serial, _revision);
        self.return_draw_credit(id)
    }
    fn return_draw_credit(&self, id: Id) -> Result<(), String> {
        {
            let mut p = lock(&self.pipe)?;
            if p.generation == id.generation {
                let successor = p.last_drawn > id.serial;
                p.cadence.advance(successor);
            }
        }
        self.actor.notify();
        self.render.notify();
        Ok(())
    }
}
fn run_tree(owner: &Owner, wake: &Wake) -> Result<(), String> {
    let mut env = OwnedEnv::new();
    let mut generation = 0;
    let mut serial = 0;
    let mut c = Compute::new(&owner.admission.event);
    let mut motion = crate::tree::Motion::default();
    let mut sample = None;
    let mut deadline = None;
    let mut needs_pack = false;
    let mut accepted = None;
    let mut scroll = Default::default();
    while !owner.lifecycle.stop.load(Ordering::Acquire) {
        owner.actor.wait(deadline.take());
        if owner.lifecycle.stop.load(Ordering::Acquire) {
            break;
        }
        let turn = owner.stats.now();
        let before = c.revision;
        for edit in owner.admission.prefix() {
            env.run(|env| owner.admission.credit(env, owner.lifecycle.id));
            if let Some(at) = edit.at {
                owner.stats.edit(at, edit.times);
                accepted.get_or_insert(at);
            }
            owner.stats.measure(stats::Metric::Patch, || {
                edit.declaration.install(&mut c.tree)
            });
            c.route = edit.route;
            c.revision = edit.revision;
            c.request = true;
        }
        if c.revision != before {
            owner.admission.collected();
        }
        // A controller echo can only be accepted after dispatch posted the
        // corresponding input. Take the inbox after the accepted prefix.
        c.request |= owner.scroll.collect(&mut c.tree, &mut scroll);
        if let Ok(mut loader) = owner.loader.1.try_recv() {
            owner.stats.assets(&mut loader);
            c.assets = Some(images::Assets::new(loader));
            c.request |= c.tree.needs_images();
        }
        let started = Instant::now();
        let (view, tick, wake_at) = {
            let mut p = lock(&owner.pipe)?;
            let due = p.cadence.due(started);
            let view = p.view;
            let (tick, wake_at) = p
                .clock
                .poll(view.is_some() && motion.active(), started, due);
            (view, tick, wake_at)
        };
        deadline = wake_at;
        if let Some(t) = tick {
            sample = Some(t);
            c.request = true;
        }
        if let Some(s) = sample {
            c.request |= motion.sample(&mut c.tree, s.sample_at);
        }
        c.request |= c.drive_images(&owner.actor)?;
        let Some(view) = view else { continue };
        if generation != view.generation {
            generation = view.generation;
            // Binary headless has no drawable until the first accepted edit.
            c.request |= !matches!(wake, Wake::Binary(_)) || serial != 0;
        }
        c.tree.reclaim_frames();
        if std::mem::take(&mut c.request) {
            #[cfg(feature = "stage-probes")]
            owner
                .probes
                .pause(probes::Stage::Layout, serial + 1, c.revision, owner);
            if let Err(e) = owner.stats.measure(stats::Metric::Layout, || {
                c.tree
                    .layout(view.size.0 as f32, view.size.1 as f32, view.scale)
            }) {
                needs_pack = false;
                owner.frame_error(&mut env, wake, e);
                continue;
            }
            needs_pack = true;
        }
        if !needs_pack
            || !owner
                .stats
                .measure(stats::Metric::Refresh, || c.tree.collect_if_available())
        {
            continue;
        }
        if c.tree.waiting_for_pixels() {
            c.drive_images(&owner.actor)?;
            // A result may have arrived between the two calls to drive_images.
            if !c.tree.waiting_for_pixels() {
                c.request = true;
                owner.actor.notify();
            }
            continue;
        }
        let Some(scene) = c.tree.seal() else {
            continue;
        };
        needs_pack = false;
        serial += 1;
        let id = Id { generation, serial };
        let sample = sample.filter(|s| s.output_generation == generation);
        let queued = owner.stats.now();
        if let Some((start, end)) = turn.zip(queued) {
            owner.stats.constructed(&c.tree, &scene, start, end);
        }
        let bundle = Bundle {
            accepted: accepted.take(),
            queued,
            id,
            scene,
            revision: c.revision,
            route: c.route.clone(),
            view,
            started,
            sample,
            endpoints: motion.endpoints(),
        };
        let frame = Frame {
            id,
            earliest: bundle.sample.map_or(0, |s| s.opportunity_id),
            value: bundle,
        };
        let retired = lock(&owner.pipe)?.commands.offer(frame);
        if retired.as_ref().is_some_and(|old| old.id != id) {
            owner.stats.overwrite();
        }
        drop(retired);
        #[cfg(feature = "stage-probes")]
        owner
            .probes
            .emit(probes::Stage::Computed, serial, c.revision);
        owner.render.notify();
        wake.notify();
    }
    if let Some(assets) = c.assets.take() {
        assets.stop();
    }
    Ok(())
}
pub(super) enum Cpu {
    Binary(Channels),
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    Shm(smithay_client_toolkit::shm::Shm, Box<wayland::Slots>),
}
impl Cpu {
    fn available(&self, _size: (u32, u32)) -> bool {
        match self {
            Self::Binary(_) => true,
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            Self::Shm(_, slots) => slots.available(_size).is_some(),
        }
    }
    fn finish(&mut self, _size: (u32, u32), rgba: Arc<Vec<u8>>) -> Result<Target, String> {
        match self {
            Self::Binary(channels) => channels
                .prepare(&rgba)
                .map(|data| Target::Binary(data, rgba)),
            #[cfg(all(target_os = "linux", feature = "wayland"))]
            Self::Shm(shm, slots) => {
                let i = slots.available(_size).ok_or("writable shm slot lost")?;
                slots
                    .write(i, shm, _size, &rgba)
                    .map(|buf| Target::Shm(buf, rgba))
            }
        }
    }
}
fn run_cpu(owner: &Owner, wake: &Wake, mut cpu: Cpu) -> Result<(), String> {
    let mut env = OwnedEnv::new();
    loop {
        if owner.lifecycle.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let (next, deadline) = {
            let mut p = lock(&owner.pipe)?;
            if p.view.is_some_and(|v| cpu.available(v.size)) {
                owner.draw(&mut p)
            } else {
                (None, None)
            }
        };
        let Some(frame) = next else {
            owner.render.wait(deadline);
            continue;
        };
        let mut times = owner.stats.draw_times();
        let installed = lock(&owner.front)?.clone();
        let same = installed.as_ref().filter(|i| {
            i.bundle.view.generation == frame.id.generation
                && i.bundle.scene.same_drawing(&frame.value.scene)
        });
        let reused = same.and_then(|i| i.pixels.cpu()).cloned();
        let reuse = reused.is_some();
        let rgba = match reused {
            Some(p) => p,
            None => match times.time(|t| &mut t.draw, || frame.value.scene.rasterize()) {
                Ok(v) => Arc::new(v),
                Err(e) => {
                    owner.frame_error(&mut env, wake, e);
                    owner.return_draw_credit(frame.id)?;
                    continue;
                }
            },
        };
        drop(installed);
        #[cfg(feature = "stage-probes")]
        owner.probes.pause(
            probes::Stage::Drawing,
            frame.id.serial,
            frame.value.revision,
            owner,
        );
        let target = cpu.finish(frame.value.view.size, rgba)?;
        owner.drawn(&frame.value, times, reuse);
        let image = frame.map(|bundle| Completed { bundle, target });
        owner.completed(image)?;
        wake.notify();
    }
}
// Device and native threads finish before lifecycle reports cleanup complete.
// The guard also wakes parked workers during unwinding or partial startup.
struct Shutdown<'a>(&'a Owner);
impl Drop for Shutdown<'_> {
    fn drop(&mut self) {
        self.0.admission.close();
        self.0.lifecycle.request_stop();
        self.0.actor.notify();
        self.0.render.notify();
    }
}
pub(super) fn run(
    owner: &Owner,
    wake: &Wake,
    cpu: Option<Cpu>,
    body: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let service = |run: &mut dyn FnMut() -> Result<(), String>| {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
            .unwrap_or_else(|_| Err("frame worker panicked".into()));
        if let Err(e) = result {
            owner.lifecycle.fail(e);
            owner.actor.notify();
            owner.render.notify();
            wake.notify();
        }
    };
    thread::scope(|scope| {
        let _shutdown = Shutdown(owner);
        thread::Builder::new()
            .name("emerge-provision".into())
            .spawn_scoped(scope, || service(&mut || owner.admission.serve(owner)))
            .map_err(|e| e.to_string())?;
        thread::Builder::new()
            .name("emerge-tree".into())
            .spawn_scoped(scope, || service(&mut || run_tree(owner, wake)))
            .map_err(|e| e.to_string())?;
        if let Some(cpu) = cpu {
            thread::Builder::new()
                .name("emerge-draw".into())
                .spawn_scoped(scope, move || {
                    let mut cpu = Some(cpu);
                    service(&mut || run_cpu(owner, wake, cpu.take().unwrap()))
                })
                .map_err(|e| e.to_string())?;
        }
        body()
    })
}
