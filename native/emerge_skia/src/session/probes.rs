//! Opt-in qualification seams, absent from release artifacts. Blocking points
//! run on native workers, never on the event dispatcher or a BEAM scheduler.
use super::*;
use std::sync::{OnceLock, atomic::AtomicBool};
rustler::atoms! {emerge_stage,held}
#[derive(Clone, Copy, rustler::NifUnitEnum)]
pub(super) enum Stage {
    Commands,
    Layout,
    Computed,
    Drawing,
    Ready,
    Opportunity,
    Claimed,
    Installed,
    Pinned,
    Writer,
    Applied,
    Reject,
    Feed,
    Sliced,
    ShortLease,
    RetainTargets,
    Handed,
    Uncertain,
}
const STAGES: usize = Stage::Uncertain as usize + 1;
#[derive(Default)]
pub(super) struct Probes {
    pid: OnceLock<LocalPid>,
    gates: [AtomicBool; STAGES],
    wake: [Signal; STAGES],
}
impl Probes {
    // Scheduler-side gates return to BEAM. Only the qualification caller waits.
    pub fn hold(&self, env: Env, s: Stage, id: u64, revision: u64) -> bool {
        if self.open(s) {
            return false;
        }
        self.pid
            .get()
            .is_some_and(|pid| env.send(pid, (emerge_stage(), s, id, revision)).is_ok())
    }
    pub fn reject(&self, id: presentation::Id, revision: u64) -> bool {
        let reject = self.gates[Stage::Reject as usize].swap(false, Ordering::AcqRel);
        if reject {
            self.emit(Stage::Reject, id.serial, revision);
        }
        reject
    }
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub fn confirm(&self, id: presentation::Id, revision: u64) -> Result<(), String> {
        if !self.open(Stage::Uncertain) {
            self.emit(Stage::Uncertain, id.serial, revision);
            return Err("uncertain handoff (probe)".into());
        }
        Ok(())
    }
    pub fn open(&self, s: Stage) -> bool {
        !self.gates[s as usize].load(Ordering::Acquire)
    }
    pub fn emit(&self, s: Stage, serial: u64, revision: u64) {
        if let Some(pid) = self.pid.get() {
            let _ = OwnedEnv::new().send_and_clear(pid, |_| (emerge_stage(), s, serial, revision));
        }
    }
    pub fn pause(&self, s: Stage, serial: u64, revision: u64, owner: &Owner) {
        self.emit(s, serial, revision);
        while !self.open(s) && !owner.lifecycle.stop.load(Ordering::Acquire) {
            self.wake[s as usize].wait(Some(Instant::now() + Duration::from_millis(50)));
        }
    }
}
#[rustler::nif]
fn renderer_stage_nif(env: Env, r: ResourceArc<Session>, s: Stage, held: bool) -> NifResult<Atom> {
    let p = &r.owner.probes;
    let pid = env.pid();
    if p.pid.get_or_init(|| pid) != &pid {
        return Err(rustler::Error::BadArg);
    }
    p.gates[s as usize].store(held, Ordering::Release);
    p.wake[s as usize].notify();
    r.owner.actor.notify();
    r.owner.render.notify();
    r.wake.notify();
    Ok(atoms::ok())
}
