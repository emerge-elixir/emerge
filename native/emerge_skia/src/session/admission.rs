//! A ticket reserves the only publisher and one of two journal entries before
//! inspecting a binary. Provisioning returns owned, validated records; bounded
//! NIF continuations apply them to the private directory and publish it once.
use super::*;
use crate::tree::{CHUNK, Draft, Publication, Publisher, Stream, storage};
use std::sync::atomic::{AtomicU8, AtomicU32};
rustler::atoms! {busy, stale_base, emerge_skia_credit, emerge_edit, ready, more}
const CLOSED: u64 = 1 << 63;
const CANCEL: u64 = 1;
const COMMIT: u64 = 2;
const FEED: u8 = 0;
const PROVE: u8 = 1;
const APPLY: u8 = 2;
const DONE: u8 = 3;
static NEXT: AtomicU64 = AtomicU64::new(1);

const PROSPECTIVE: u8 = 0;
const INSTALLED: u8 = 1;
const UNUSED: u8 = 2;
struct Subscriber {
    pid: LocalPid,
    usage: AtomicU8,
}
pub(super) struct Route {
    pub epoch: u64,
    wake: Signal,
    subscriber: Option<Arc<Subscriber>>,
}
impl Route {
    pub fn initial(wake: &Signal) -> Arc<Self> {
        Arc::new(Self {
            epoch: 0,
            wake: wake.clone(),
            subscriber: None,
        })
    }
    /// Only dispatch retires an installed epoch: its final click and its
    /// retirement have the same sender and relay. Frame serials and epochs
    /// never go backwards. Uninstalled epochs cannot have emitted a click.
    pub fn installed_after(&self, old: Option<&Route>, session: u64) {
        if old.is_some_and(|r| r.epoch == self.epoch) {
            return;
        }
        if let Some(s) = &self.subscriber {
            s.usage.store(INSTALLED, Ordering::Release);
        }
        if let Some(r) = old
            && let Some(s) = &r.subscriber
        {
            debug_assert!(r.epoch < self.epoch);
            let _ = OwnedEnv::new()
                .send_and_clear(&s.pid, |_| (emerge_skia_retire_routes(), session, r.epoch));
        }
    }
}
impl Drop for Route {
    fn drop(&mut self) {
        if let Some(s) = &self.subscriber {
            let _ =
                s.usage
                    .compare_exchange(PROSPECTIVE, UNUSED, Ordering::AcqRel, Ordering::Acquire);
        }
        self.wake.notify();
    }
}
pub(super) struct Accepted {
    pub declaration: Publication,
    pub revision: u64,
    pub route: Arc<Route>,
}
struct Data {
    stream: Option<Stream>,
    draft: Option<Draft>,
    route: Option<Arc<Route>>,
}
struct Pending {
    id: u64,
    pid: LocalPid,
    revision: u64,
    waiter: Option<LocalPid>,
    routes: bool,
    scale: f32,
    phase: AtomicU8,
    activity: AtomicU64,
    data: Mutex<Data>,
}
pub(super) struct Admission {
    pub accepted: AtomicU64,
    state: AtomicU64,
    scale: AtomicU32,
    percent: u16,
    storage: storage::Storage,
    jobs: (Sender<Arc<Pending>>, Receiver<Arc<Pending>>),
    pub event: Signal,
    entries: Sender<Accepted>,
    prefix: Receiver<Accepted>,
    reader: Sender<LocalPid>,
    waiting: Receiver<LocalPid>,
    credits: AtomicU64,
    collected: std::sync::atomic::AtomicBool,
}
impl Admission {
    pub fn new(percent: u16) -> Self {
        let (entries, prefix) = bounded(2);
        let (reader, waiting) = bounded(1);
        let event = Signal::default();
        let wake = event.clone();
        let storage = storage::Storage::new(move || wake.notify());
        Self {
            percent,
            storage,
            state: Default::default(),
            accepted: Default::default(),
            scale: AtomicU32::new(1f32.to_bits()),
            jobs: bounded(1),
            event,
            entries,
            prefix,
            reader,
            waiting,
            credits: Default::default(),
            collected: Default::default(),
        }
    }
    pub fn scale(&self, v: f32) {
        if self.scale.swap(v.to_bits(), Ordering::AcqRel) != v.to_bits() {
            // Linearize scale invalidation with the NIF's commit CAS. A winning
            // commit belongs to the old scale; a losing draft must be reproved.
            let _ = self
                .state
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |s| {
                    (s != 0 && s & (CLOSED | 3) == 0).then_some(s | CANCEL)
                });
            self.event.notify();
        }
    }
    pub fn close(&self) {
        self.state.fetch_or(CLOSED, Ordering::AcqRel);
        self.event.notify();
    }
    pub fn prefix(&self) -> impl Iterator<Item = Accepted> + '_ {
        self.prefix.try_iter().take(2)
    }
    pub fn collected(&self) {
        self.collected.store(true, Ordering::Release);
        self.event.notify();
    }
    pub fn credit(&self, env: Env, session: u64) {
        self.credits.fetch_add(1, Ordering::AcqRel);
        if self.state.load(Ordering::Acquire) == 0
            && !self.entries.is_full()
            && self.storage.available()
            && let Ok(pid) = self.waiting.try_recv()
        {
            let _ = env.send(&pid, (emerge_skia_credit(), session));
        }
    }
    fn busy(&self, env: Env, pid: Option<LocalPid>, seen: u64, session: u64) -> rustler::Error {
        if let Some(pid) = pid {
            let _ = self.reader.try_send(pid);
            if self.credits.load(Ordering::Acquire) != seen {
                self.credit(env, session);
            }
        }
        rustler::Error::Term(Box::new(busy()))
    }
    fn cancel(&self, id: u64) {
        if self
            .state
            .compare_exchange(id, id | CANCEL, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.event.notify();
        }
    }
    fn release(&self, env: Env, session: u64) {
        self.state.fetch_and(CLOSED, Ordering::AcqRel);
        self.credit(env, session);
    }
    pub fn serve(&self, owner: &Owner) -> Result<(), String> {
        let session = owner.lifecycle.id;
        let mut writer = Publisher::with_storage(self.percent, self.storage.clone());
        let mut env = OwnedEnv::new();
        let mut route = Route::initial(&self.event);
        let mut retired: Vec<(u64, Arc<Subscriber>)> = Vec::new();
        loop {
            if self.collected.swap(false, Ordering::AcqRel) {
                writer.collected();
            }
            writer.trim_if_pressured();
            retired.retain(
                |(epoch, subscriber)| match subscriber.usage.load(Ordering::Acquire) {
                    PROSPECTIVE => true,
                    INSTALLED => false, // The display orders retirement with the final event.
                    _ => {
                        let _ = env.send_and_clear(&subscriber.pid, |_| {
                            (emerge_skia_retire_routes(), session, *epoch)
                        });
                        false
                    }
                },
            );
            let job = match self.jobs.1.try_recv() {
                Ok(j) => j,
                Err(_) => {
                    if self.state.load(Ordering::Acquire) == CLOSED {
                        break;
                    }
                    if !self.waiting.is_empty() {
                        env.run(|e| self.credit(e, session));
                    }
                    self.event.wait(None);
                    continue;
                }
            };
            let idle = Duration::from_secs(30);
            #[cfg(feature = "stage-probes")]
            let idle = if owner.probes.open(probes::Stage::ShortLease) {
                idle
            } else {
                Duration::from_millis(200)
            };
            let mut deadline = Instant::now() + idle;
            let mut seen = job.activity.load(Ordering::Acquire);
            let mut error = None;
            // Once the session is closed, an already won commit still needs
            // its reserved descriptor and postcommit reclamation.
            loop {
                let state = self.state.load(Ordering::Acquire);
                let phase = job.phase.load(Ordering::Acquire);
                if phase == DONE || (state != job.id && state & !CLOSED != job.id | COMMIT) {
                    break;
                }
                if phase == PROVE {
                    #[cfg(feature = "stage-probes")]
                    owner
                        .probes
                        .pause(probes::Stage::Writer, job.id, job.revision, owner);
                    if self.state.load(Ordering::Acquire) != job.id {
                        continue;
                    }
                    let stream = lock(&job.data)?
                        .stream
                        .take()
                        .ok_or("edit stream already finished")?;
                    match stream
                        .finish()
                        .and_then(|rows| writer.prepare(rows, job.scale))
                    {
                        Ok(d) => {
                            lock(&job.data)?.draft = Some(d);
                            let r = if job.routes {
                                Arc::new(Route {
                                    epoch: job.revision,
                                    wake: self.event.clone(),
                                    subscriber: job.waiter.map(|pid| {
                                        Arc::new(Subscriber {
                                            pid,
                                            usage: AtomicU8::new(PROSPECTIVE),
                                        })
                                    }),
                                })
                            } else {
                                route.clone()
                            };
                            lock(&job.data)?.route = Some(r);
                            // Cold proof is service work, not time spent waiting for a caller.
                            deadline = Instant::now() + idle;
                            job.phase.store(APPLY, Ordering::Release);
                            if env
                                .send_and_clear(&job.pid, |_| (emerge_edit(), job.id, ready()))
                                .is_err()
                            {
                                self.cancel(job.id);
                            }
                        }
                        Err(e) => {
                            error = Some(e);
                            break;
                        }
                    }
                } else if state & COMMIT != 0 {
                    // Past the linearization point: never time out a winning commit.
                    self.event.wait(None);
                } else {
                    // An abandoned but still-live caller cannot retain the only ticket.
                    // No per-chunk wake/timer: check progress when the deadline fires.
                    let activity = job.activity.load(Ordering::Acquire);
                    if activity != seen {
                        seen = activity;
                        deadline = Instant::now() + idle;
                    }
                    if Instant::now() >= deadline {
                        if self
                            .state
                            .compare_exchange(
                                job.id,
                                job.id | CANCEL,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            error = Some("edit ticket expired".into());
                            break;
                        }
                    } else {
                        self.event.wait(Some(deadline));
                    }
                }
            }
            let committed = job.phase.load(Ordering::Acquire) == DONE;
            let mut data = lock(&job.data)?;
            data.stream.take();
            let next_route = data.route.take();
            if let Some(d) = data.draft.take() {
                writer.release(d, committed);
            }
            drop(data);
            if committed && job.routes {
                if let Some(s) = &route.subscriber {
                    retired.push((route.epoch, s.clone()));
                }
                route = next_route.expect("committed route");
            }
            // No not-yet-consumed bytes or abandoned layout workspace behind
            // the credit. The caller's return process may already be gone.
            if !committed
                && (error.is_none() || error.as_deref() == Some(storage::BUSY))
                && let Some(pid) = job.waiter
            {
                let _ = self.reader.try_send(pid);
            }
            env.run(|e| self.release(e, session));
            #[cfg(feature = "stage-probes")]
            if committed {
                owner
                    .probes
                    .pause(probes::Stage::Applied, job.id, job.revision, owner);
            }
            let _ = env.send_and_clear(&job.pid, |e| {
                use rustler::Encoder;
                let result = if committed {
                    (atoms::ok(), (session, job.revision)).encode(e)
                } else if error.as_deref() == Some(storage::BUSY) {
                    (super::error(), busy()).encode(e)
                } else if let Some(why) = error {
                    (super::error(), why).encode(e)
                } else if !owner.lifecycle.running.load(Ordering::Acquire) {
                    (super::error(), "renderer is stopping").encode(e)
                } else {
                    (super::error(), busy()).encode(e)
                };
                (emerge_edit(), job.id, result)
            });
        }
        while self.prefix.try_recv().is_ok() {}
        Ok(())
    }
}
/// Admission credits, not allocations, RSS or device bytes. Test artifacts only.
#[cfg(feature = "stage-probes")]
#[rustler::nif]
fn renderer_storage_nif(
    r: ResourceArc<Session>,
) -> ((usize, usize), (usize, usize), (usize, usize)) {
    let s = &r.owner.admission.storage;
    (s.document.bytes(), s.workspace.bytes(), s.media.bytes())
}
fn reason(msg: impl Into<String>) -> rustler::Error {
    let why = msg.into();
    if why == storage::BUSY {
        rustler::Error::Term(Box::new(busy()))
    } else {
        rustler::Error::Term(Box::new(why))
    }
}
impl Session {
    fn base(&self, base: Option<(u64, u64)>) -> NifResult<(u64, u64)> {
        if !self.running() || self.owner.admission.state.load(Ordering::Acquire) & CLOSED != 0 {
            return Err(reason("renderer is stopping"));
        }
        let now = (
            self.owner.lifecycle.id,
            self.owner.admission.accepted.load(Ordering::Acquire),
        );
        if base.is_some_and(|base| base != now) {
            return Err(rustler::Error::Term(Box::new(stale_base())));
        }
        Ok(now)
    }
}
struct Ticket {
    owner: Arc<Owner>,
    job: Arc<Pending>,
}
impl Ticket {
    #[cfg(feature = "stage-probes")]
    fn hold(&self, env: Env, stage: probes::Stage) -> bool {
        self.owner
            .probes
            .hold(env, stage, self.job.id, self.job.revision)
    }
    fn check(&self, env: Env, phase: u8) -> NifResult<()> {
        if env.pid() != self.job.pid || self.job.phase.load(Ordering::Acquire) != phase {
            return Err(rustler::Error::BadArg);
        }
        let state = self.owner.admission.state.load(Ordering::Acquire);
        if state & CLOSED != 0 {
            return Err(reason("renderer is stopping"));
        }
        if state != self.job.id {
            return Err(rustler::Error::Term(Box::new(busy())));
        }
        Ok(())
    }
    fn cancel(&self) {
        self.owner.admission.cancel(self.job.id);
    }
}
#[rustler::resource_impl]
impl Resource for Ticket {
    fn down(&self, _: Env, _: LocalPid, _: Monitor) {
        self.cancel();
    }
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.cancel();
    }
}
#[rustler::nif]
fn renderer_revision_nif(
    r: ResourceArc<Session>,
    base: Option<(u64, u64)>,
) -> NifResult<(Atom, (u64, u64))> {
    Ok((atoms::ok(), r.base(base)?))
}
#[rustler::nif]
fn renderer_begin_nif(
    env: Env,
    r: ResourceArc<Session>,
    base: Option<(u64, u64)>,
    size: usize,
    routes: bool,
    waiter: Option<LocalPid>,
) -> NifResult<(Atom, ResourceArc<Ticket>, u64)> {
    let a = &r.owner.admission;
    let (session, _) = r.base(base)?;
    if size > 64 * 1024 * 1024 {
        return Err(reason("edit exceeds storage limit"));
    }
    let seen = a.credits.load(Ordering::Acquire);
    let id = NEXT
        .fetch_add(1, Ordering::Relaxed)
        .checked_mul(4)
        .filter(|v| *v < CLOSED)
        .ok_or_else(|| reason("edit tickets exhausted"))?;
    if a.entries.is_full()
        || a.state
            .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        r.base(base)?;
        return Err(a.busy(env, waiter, seen, session));
    }
    let checked = r.base(base);
    if checked.is_err() || a.entries.is_full() {
        a.release(env, session);
        checked?;
        return Err(a.busy(env, waiter, seen, session));
    }
    let stream = match Stream::bounded(size, &a.storage) {
        Ok(s) => s,
        Err(why) => {
            a.release(env, session);
            return Err(if why == storage::BUSY {
                a.busy(env, waiter, seen, session)
            } else {
                reason(why)
            });
        }
    };
    let (_, previous) = checked?;
    let job = Arc::new(Pending {
        id,
        pid: env.pid(),
        revision: previous + 1,
        routes,
        waiter,
        scale: f32::from_bits(a.scale.load(Ordering::Acquire)),
        data: Mutex::new(Data {
            stream: Some(stream),
            draft: None,
            route: None,
        }),
        phase: AtomicU8::new(FEED),
        activity: AtomicU64::new(0),
    });
    let ticket = ResourceArc::new(Ticket {
        owner: r.owner.clone(),
        job: job.clone(),
    });
    a.jobs.0.try_send(job).expect("reserved writer service");
    a.event.notify();
    if ticket.monitor(Some(env), &env.pid()).is_none() {
        ticket.cancel();
        return Err(reason("edit caller is not alive"));
    }
    Ok((atoms::ok(), ticket, id))
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_chunk_nif(
    env: Env,
    t: ResourceArc<Ticket>,
    offset: usize,
    part: Term,
) -> NifResult<Atom> {
    t.check(env, FEED)?;
    // Private stub: the only BEAM call site splits at CHUNK before inspection
    // (enif_inspect_binary may copy a bit-offset binary into the call env).
    let part = part.decode::<Binary>()?;
    if part.len() > CHUNK {
        return Err(rustler::Error::BadArg);
    }
    t.job
        .data
        .try_lock()
        .map_err(|_| rustler::Error::BadArg)?
        .stream
        .as_mut()
        .ok_or(rustler::Error::BadArg)?
        .push(offset, part.as_slice())
        .map_err(reason)?;
    t.job.activity.fetch_add(1, Ordering::Release);
    #[cfg(feature = "stage-probes")]
    if t.hold(env, probes::Stage::Feed) {
        return Ok(probes::held());
    }
    Ok(atoms::ok())
}
#[rustler::nif]
fn renderer_prove_nif(env: Env, t: ResourceArc<Ticket>) -> NifResult<Atom> {
    t.check(env, FEED)?;
    t.job.phase.store(PROVE, Ordering::Release);
    t.owner.admission.event.notify();
    Ok(atoms::ok())
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_apply_nif(env: Env, t: ResourceArc<Ticket>) -> NifResult<Atom> {
    t.check(env, APPLY)?;
    let a = &t.owner.admission;
    let j = &t.job;
    let mut guard = j.data.try_lock().map_err(|_| rustler::Error::BadArg)?;
    let d = guard.draft.as_mut().ok_or(rustler::Error::BadArg)?;
    if !d.apply_slice() {
        j.activity.fetch_add(1, Ordering::Release);
        #[cfg(feature = "stage-probes")]
        if t.hold(env, probes::Stage::Sliced) {
            return Ok(probes::held());
        }
        return Ok(more());
    }
    d.check_extent().map_err(reason)?;
    if let Err(s) =
        a.state
            .compare_exchange(j.id, j.id | COMMIT, Ordering::AcqRel, Ordering::Acquire)
    {
        return Err(if s & CLOSED != 0 {
            reason("renderer is stopping")
        } else {
            rustler::Error::Term(Box::new(busy()))
        });
    }
    let declaration = d.publish();
    let entry = Accepted {
        declaration,
        revision: j.revision,
        route: guard.route.as_ref().expect("provisioned route").clone(),
    };
    a.accepted.store(j.revision, Ordering::Release);
    a.entries.try_send(entry).expect("reserved journal slot");
    drop(guard);
    j.phase.store(DONE, Ordering::Release);
    a.event.notify();
    t.owner.actor.notify();
    Ok(atoms::ok())
}
#[rustler::nif]
fn renderer_abort_nif(env: Env, t: ResourceArc<Ticket>) -> NifResult<Atom> {
    if env.pid() != t.job.pid {
        return Err(rustler::Error::BadArg);
    }
    t.cancel();
    Ok(atoms::ok())
}
