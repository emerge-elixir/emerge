//! The presentation context/queue has one owner. It only resolves selected
//! images or reads a pinned installation; it never replays a drawing.
use super::*;
use crate::session::{
    Selected,
    capture::{Readback, Rect},
};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::thread::{self, JoinHandle};
rustler::atoms! {warning}
#[derive(Debug)]
pub(super) enum ProbeError {
    Unavailable(String),
    Fatal(String),
}
#[cfg(feature = "vulkan")]
impl ProbeError {
    pub(super) fn reason(self) -> String {
        match self {
            Self::Fatal(e) | Self::Unavailable(e) => e,
        }
    }
}
impl From<String> for ProbeError {
    fn from(s: String) -> Self {
        Self::Fatal(s)
    }
}

enum Device {
    #[cfg(feature = "opengl")]
    Gl(gl::Gl),
    #[cfg(feature = "vulkan")]
    Vk(vulkan::VkPresenter),
}
impl Device {
    fn handoff(&mut self, c: &Claim, owner: &Owner) -> Result<Outcome, String> {
        // Order the start with reconfiguration without holding the exchange
        // lock over a driver call. An already started successful handoff
        // installs its own mapping, never the subsequent configure's.
        if lock(&owner.pipe)?
            .view
            .is_none_or(|v| v.generation != c.frame.id.generation)
        {
            return Ok(Outcome::SupersededView);
        }
        #[cfg(feature = "stage-probes")]
        if owner
            .probes
            .reject(c.frame.id, c.frame.value.bundle.revision)
        {
            return Ok(Outcome::Unsubmitted { outdated: false });
        }
        let Target::Gpu(image) = &c.frame.value.target else {
            return Err("not a device image".into());
        };
        self.submit(image)
    }
    fn submit(&mut self, image: &Arc<Lease>) -> Result<Outcome, String> {
        match (self, &image.image) {
            #[cfg(feature = "opengl")]
            (Self::Gl(g), textures::Image::Gl(t)) => {
                g.submit(t, image.size)?;
                Ok(Outcome::Submitted)
            }
            #[cfg(feature = "vulkan")]
            (Self::Vk(v), textures::Image::Vk(_)) => {
                if !v.acquire(image.size)? {
                    return Ok(Outcome::Unsubmitted {
                        outdated: v.recreate,
                    });
                }
                Ok(if v.present(image)? {
                    Outcome::Submitted
                } else {
                    Outcome::Unsubmitted { outdated: true }
                })
            }
            #[cfg(all(feature = "opengl", feature = "vulkan"))]
            _ => Err("foreign device image".into()),
        }
    }
    fn read(&mut self, image: &Arc<Lease>, rect: Rect) -> Result<Vec<u8>, String> {
        match (self, &image.image) {
            #[cfg(feature = "opengl")]
            (Self::Gl(g), textures::Image::Gl(t)) => g.read(t, rect),
            #[cfg(feature = "vulkan")]
            (Self::Vk(v), textures::Image::Vk(_)) => v.read(image, rect),
            #[cfg(all(feature = "opengl", feature = "vulkan"))]
            _ => Err("foreign capture image".into()),
        }
    }
}
pub(super) enum Outcome {
    Submitted,
    SupersededView,
    #[cfg(any(feature = "vulkan", feature = "stage-probes"))]
    Unsubmitted {
        outdated: bool,
    },
}
struct Claim {
    frame: Frame<Completed>,
    mapping: Mapping,
}
// One preallocated channel slot. Boxing a claim to shrink the capture variant
// would add a heap allocation to every Wayland GPU handoff.
#[allow(clippy::large_enum_variant)]
enum Job {
    Present(Claim),
    Capture(Readback),
}
// A capture replies directly to its caller; the dispatcher only needs its completion.
type Report = Option<(Claim, Outcome, std::ops::Range<Instant>)>;
pub(super) struct Output {
    jobs: Option<Sender<Job>>,
    reports: Receiver<Result<Report, String>>,
    thread: Option<JoinHandle<()>>,
    captures: Receiver<Readback>,
    busy: bool,
    retry: Option<Instant>,
}
impl Drop for Output {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
impl Output {
    fn send(&mut self, job: Job) -> Result<(), String> {
        self.jobs
            .as_ref()
            .ok_or("device stopped")?
            .try_send(job)
            .map_err(|_| "device queue unavailable")?;
        self.busy = true;
        Ok(())
    }
}
impl WindowApp {
    pub(super) fn init_device(
        &mut self,
        conn: &Connection,
        request: Requested,
        ping: &Ping,
    ) -> Result<(), String> {
        let size = self.geometry.buffer()?;
        let win = self.window.clone();
        let conn = conn.clone();
        let owner = self.owner.clone();
        let ping = ping.clone();
        #[cfg(feature = "vulkan")]
        let node = self.feedback.node;
        let (jobs, inbox) = bounded(1);
        let (tx, reports) = bounded(1);
        let (started, rx) = bounded(1);
        let (requests, captures) = bounded(1);
        let t = thread::Builder::new()
            .name("emerge-present".into())
            .spawn(move || {
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<(), String> {
                        let probe = match request {
                            Requested::Raster => return Err("raster cannot start a GPU".into()),
                            #[cfg(feature = "opengl")]
                            Requested::Gl => gl::Gl::new(&conn, &win, size, &owner, &ping)
                                .map(|(g, p)| (Device::Gl(g), p, Selected::Gl)),
                            #[cfg(feature = "vulkan")]
                            Requested::Vk => {
                                vulkan::VkPresenter::new(&conn, &win, size, node, &owner, &ping)
                                    .map(|(v, p, d)| (Device::Vk(v), p, Selected::Vk(d)))
                            }
                        };
                        let (device, producer, kind) = match probe {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = started.send(Err(e));
                                return Ok(());
                            }
                        };
                        // The WSI owner retires all GPU reads before Producer drops its
                        // bank. Leases held by the selector after shutdown are inert.
                        let _producer = producer;
                        let mut device = device;
                        started.send(Ok(kind)).map_err(|_| "startup cancelled")?;
                        while let Ok(job) = inbox.recv() {
                            if owner.lifecycle.stop.load(Ordering::Acquire) {
                                break;
                            }
                            let report = match job {
                                Job::Present(c) => {
                                    #[cfg(feature = "stage-probes")]
                                    owner.probes.pause(
                                        crate::session::probes::Stage::Claimed,
                                        c.frame.id.serial,
                                        c.frame.value.bundle.revision,
                                        &owner,
                                    );
                                    let t = Instant::now();
                                    let handed = device.handoff(&c, &owner).and_then(|outcome| {
                                        conn.flush().map_err(err)?;
                                        let span = t..Instant::now();
                                        #[cfg(feature = "stage-probes")]
                                        if matches!(outcome, Outcome::Submitted) {
                                            let revision = c.frame.value.bundle.revision;
                                            owner.probes.confirm(c.frame.id, revision)?;
                                            owner.probes.pause(
                                                crate::session::probes::Stage::Handed,
                                                c.frame.id.serial,
                                                revision,
                                                &owner,
                                            );
                                        }
                                        Ok((outcome, span))
                                    });
                                    // Tell dispatch and the draw owner *before* retiring the
                                    // device. An uncertain handoff is never retried or installed.
                                    let (outcome, span) = handed.inspect_err(|e| {
                                        owner.lifecycle.fail(e.clone());
                                        owner.actor.notify();
                                        owner.render.notify();
                                        ping.ping();
                                    })?;
                                    Some((c, outcome, span))
                                }
                                Job::Capture(read) => {
                                    read.read(|image, rect| device.read(image, rect));
                                    None
                                }
                            };
                            if tx.try_send(Ok(report)).is_err() {
                                break;
                            }
                            ping.ping();
                        }
                        Ok(())
                    },
                ))
                .unwrap_or_else(|_| Err("presentation worker panicked".into()));
                if let Err(e) = run {
                    let _ = tx.try_send(Err(e));
                    ping.ping();
                }
            })
            .map_err(err)?;
        let service = Output {
            jobs: Some(jobs),
            reports,
            thread: Some(t),
            captures,
            busy: false,
            retry: None,
        };
        let selected = match rx.recv().map_err(err)? {
            Ok(kind) => {
                self.owner
                    .readbacks
                    .set(requests)
                    .map_err(|_| "capture already initialized")?;
                self.gpu = Some(service);
                kind
            }
            Err(ProbeError::Fatal(e)) => return Err(e),
            Err(ProbeError::Unavailable(e)) => {
                let log = *lock(&self.owner.log)?;
                if let Some(pid) = log {
                    let msg = format!(
                        "Wayland: {} requested; {e}; using CPU raster (wl_shm)",
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
            .map_err(|_| "renderer already selected".into())
    }
    pub(super) fn device_report(&mut self) -> Result<(), String> {
        let Some(gpu) = &mut self.gpu else {
            return Ok(());
        };
        let result = match gpu.reports.try_recv() {
            Ok(r) => r?,
            Err(crossbeam_channel::TryRecvError::Empty) => return Ok(()),
            Err(_) => return Err("presentation device stopped".into()),
        };
        gpu.busy = false;
        match result {
            Some((Claim { frame, mapping }, Outcome::Submitted, t)) => {
                let Target::Gpu(target) = frame.value.target else {
                    return Err("missing submitted texture".into());
                };
                #[cfg(feature = "stage-probes")]
                self.pin_target(&frame.value.bundle, target.clone());
                self.install_mapping(mapping);
                self.frame_pending = self.callback_armed;
                self.display.install(
                    &self.owner,
                    &mut self.env,
                    frame.value.bundle,
                    Pixels::Gpu(target),
                    Some(t),
                )?;
            }
            #[cfg(any(feature = "vulkan", feature = "stage-probes"))]
            Some((Claim { frame, .. }, Outcome::Unsubmitted { outdated }, _)) => {
                // The frame request may survive even if we retry the same serial.
                // It is no longer attributable to an individual handoff.
                self.display.callback = None;
                if outdated
                    && lock(&self.owner.pipe)?
                        .view
                        .is_some_and(|v| v.generation == frame.id.generation)
                {
                    self.attempted = None;
                    self.owner.invalidate_view()?;
                } else {
                    let retired = lock(&self.owner.pipe)?.completed.retry(frame);
                    drop(retired);
                }
                gpu.retry = Some(Instant::now() + Duration::from_millis(4));
            }
            Some((_, Outcome::SupersededView, _)) => self.display.callback = None,
            None => (),
        }
        self.owner.actor.notify();
        Ok(())
    }
    pub(super) fn present_gpu(
        &mut self,
        qh: &QueueHandle<Self>,
        mapping: Mapping,
    ) -> Result<Option<Duration>, String> {
        let gpu = self.gpu.as_mut().expect("selected GPU");
        let owner = &self.owner;
        let changed = self.installed != Some(mapping);
        gpu.retry = gpu.retry.filter(|t| *t > Instant::now());
        let open =
            owner.opportunity(!gpu.busy && gpu.retry.is_none() && (!self.frame_pending || changed));
        let frame = {
            let mut p = lock(&owner.pipe)?;
            p.submit_open = open;
            let f = open.then(|| p.completed.take()).flatten();
            if f.is_some() {
                p.submit_open = false;
            }
            f
        };
        owner.render.notify();
        if let Some(f) = frame {
            let Target::Gpu(image) = &f.value.target else {
                return Err("expected GPU target".into());
            };
            if !changed
                && self.display.installed.as_ref().is_some_and(|i| {
                    i.bundle.view.generation == f.id.generation
                        && matches!(&i.pixels,Pixels::Gpu(t) if Arc::ptr_eq(t,image))
                })
            {
                self.display.install(
                    owner,
                    &mut self.env,
                    f.value.bundle,
                    Pixels::Gpu(image.clone()),
                    None,
                )?;
            } else {
                self.protocols
                    .apply(self.window.wl_surface(), mapping.geometry);
                if !self.callback_armed {
                    self.window
                        .wl_surface()
                        .frame(qh, self.window.wl_surface().clone());
                    self.display.callback = crate::session::stats::Callback::arm(&f.value.bundle);
                    self.callback_armed = true;
                }
                gpu.send(Job::Present(Claim { frame: f, mapping }))?;
            }
        }
        if !gpu.busy
            && let Ok(read) = gpu.captures.try_recv()
        {
            gpu.send(Job::Capture(read))?;
        }
        Ok(gpu
            .retry
            .map(|t| t.saturating_duration_since(Instant::now())))
    }
}
