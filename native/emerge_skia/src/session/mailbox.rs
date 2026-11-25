//! The output dispatcher alone applies controls, interprets headless input and
//! installs handoffs. No NIF borrows mutable display state or waits on it.
use super::*;
use rustler::Encoder;

#[derive(Clone, Copy, rustler::NifUnitEnum)]
enum Op {
    Input,
    Mask,
    InputTarget,
    LogTarget,
    FrameRate,
    Configure,
    Stats,
}
enum Command {
    Input(input::InputEvent),
    Mask(u32),
    InputTarget(Option<LocalPid>),
    LogTarget(Option<LocalPid>),
    FrameRate(Option<Duration>),
    Configure(Control, LocalPid),
    Stats(stats::StatsCommandNif),
    Pin(Sender<Result<capture::Snapshot, String>>),
}
pub(super) struct Envelope {
    reply: Option<(LocalPid, u64)>,
    cmd: Command,
}
impl Envelope {
    pub fn pin(to: Sender<Result<capture::Snapshot, String>>) -> Self {
        Self {
            reply: None,
            cmd: Command::Pin(to),
        }
    }
    fn apply(self, s: &mut Display, owner: &Owner, env: &mut OwnedEnv) {
        let Some((pid, id)) = self.reply else {
            if let Command::Pin(to) = self.cmd {
                let _ = to.try_send(
                    s.installed
                        .as_ref()
                        .map(capture::Snapshot::from)
                        .ok_or_else(|| "no presented frame is available yet".into()),
                );
            }
            return;
        };
        let _ = env.send_and_clear(&pid, |e| {
            let result = match self.cmd {
                Command::Input(event) => {
                    s.dispatch(e, event);
                    atoms::ok().encode(e)
                }
                Command::Mask(mask) => {
                    s.input.observer.set_mask(mask);
                    atoms::ok().encode(e)
                }
                Command::InputTarget(target) => {
                    s.input.set_target(target);
                    atoms::ok().encode(e)
                }
                Command::LogTarget(target) => {
                    *owner.log.lock().unwrap_or_else(|p| p.into_inner()) = target;
                    atoms::ok().encode(e)
                }
                Command::FrameRate(limit) => s
                    .frame_rate(owner, limit)
                    .map(|_| atoms::ok())
                    .map_or_else(|why| (error(), why).encode(e), |ok| ok.encode(e)),
                Command::Configure(c, observer) => s.configure(owner, c, e, observer).encode(e),
                Command::Stats(cmd) => stats::snapshot(s, owner, cmd).encode(e),
                Command::Pin(_) => unreachable!(),
            };
            (control::emerge_skia_reply(), id, result)
        });
    }
    pub fn stopped(self, env: &mut OwnedEnv) {
        if let Some((pid, id)) = self.reply {
            let _ = env.send_and_clear(&pid, |_| {
                (
                    control::emerge_skia_reply(),
                    id,
                    (error(), "renderer is stopped"),
                )
            });
        }
    }
}
impl Display {
    pub fn controls(&mut self, owner: &Owner, env: &mut OwnedEnv) {
        for command in owner.controls.1.try_iter().take(32) {
            command.apply(self, owner, env);
        }
    }
    fn frame_rate(&self, owner: &Owner, limit: Option<Duration>) -> Result<(), String> {
        let mut p = lock(&owner.pipe)?;
        p.cadence.set(limit);
        // Headless predicts from its own cadence. A window retains the actual
        // output refresh period; the clock also considers the next capped draw.
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        let headless = self.toplevel.is_none();
        #[cfg(not(all(target_os = "linux", feature = "wayland")))]
        let headless = true;
        if headless {
            p.clock
                .cadence(limit.unwrap_or(super::timing::HEADLESS_PERIOD));
        }
        drop(p);
        owner.actor.notify();
        owner.render.notify();
        Ok(())
    }
    fn configure(
        &mut self,
        owner: &Owner,
        c: Control,
        e: Env,
        observer: LocalPid,
    ) -> Result<(u64, u64), String> {
        let limit = interval(c.target_fps)?;
        if let Some(title) = c.title {
            self.title(title)?;
        }
        self.frame_rate(owner, limit)?;
        self.input.observer.set_mask(c.input_mask);
        let token = (
            owner.lifecycle.id,
            owner.revision.fetch_add(1, Ordering::AcqRel) + 1,
        );
        let _ = e.send(&observer, (emerge_skia_config(), token, atoms::ok()));
        Ok(token)
    }
}
impl Session {
    pub fn enqueue(&self, msg: Envelope) -> NifResult<Atom> {
        if !self.running() {
            return Err(rustler::Error::Term(Box::new("renderer is stopping")));
        }
        self.owner
            .controls
            .0
            .try_lock()
            .map_err(|_| rustler::Error::Term(Box::new(control::busy())))?
            .as_ref()
            .ok_or_else(|| rustler::Error::Term(Box::new("renderer is stopped")))?
            .try_send(msg)
            .map_err(|_| rustler::Error::Term(Box::new(control::busy())))?;
        self.wake.notify();
        Ok(atoms::ok())
    }
}
#[rustler::nif(schedule = "DirtyCpu")]
fn renderer_control_nif(
    env: Env,
    r: ResourceArc<Session>,
    op: Op,
    payload: Term,
    observer: LocalPid,
    id: u64,
) -> NifResult<Atom> {
    if r.owner.controls.1.is_full() {
        return Err(rustler::Error::Term(Box::new(control::busy())));
    }
    let mask = |n: u32| {
        if n > input::INPUT_MASK_ALL {
            Err(rustler::Error::Term(Box::new("invalid input mask")))
        } else {
            Ok(n)
        }
    };
    let cmd =
        match op {
            Op::Input => {
                #[cfg(all(target_os = "linux", feature = "wayland"))]
                if !matches!(r.wake, Wake::Binary(_)) {
                    return Err(rustler::Error::Term(Box::new(
                        "send_input is only available for binary headless output",
                    )));
                }
                Command::Input(dispatch::decode(payload).map_err(|_| {
                    rustler::Error::Term(Box::new("invalid or unsupported raw input"))
                })?)
            }
            Op::Mask => Command::Mask(mask(payload.decode()?)?),
            Op::InputTarget => Command::InputTarget(payload.decode()?),
            Op::LogTarget => Command::LogTarget(payload.decode()?),
            Op::FrameRate => Command::FrameRate(
                interval(payload.decode()?).map_err(|e| rustler::Error::Term(Box::new(e)))?,
            ),
            Op::Configure => {
                let c: Control = payload.decode()?;
                mask(c.input_mask)?;
                Command::Configure(c, observer)
            }
            Op::Stats => Command::Stats(payload.decode()?),
        };
    r.enqueue(Envelope {
        reply: Some((env.pid(), id)),
        cmd,
    })
}
