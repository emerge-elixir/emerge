//! Long control-plane waits belong on neither BEAM scheduler pool. Use
//! bounded native jobs and correlated replies; stop admission is independent
//! of capture and startup. No caller environment crosses this boundary.
use super::*;
use std::sync::atomic::AtomicUsize;
rustler::atoms! {emerge_skia_reply,busy,complete}
static STARTS: AtomicUsize = AtomicUsize::new(0);
static STOPS: AtomicUsize = AtomicUsize::new(0);
pub(super) static ASSETS: AtomicUsize = AtomicUsize::new(0);
pub(super) static CAPTURES: AtomicUsize = AtomicUsize::new(0);
pub(super) fn job(
    count: &'static AtomicUsize,
    name: &str,
    work: impl FnOnce() + Send + 'static,
) -> NifResult<Atom> {
    struct Slot(&'static AtomicUsize);
    impl Drop for Slot {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::Release);
        }
    }
    count
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 32).then_some(n + 1)
        })
        .map_err(|_| rustler::Error::Term(Box::new(busy())))?;
    let slot = Slot(count);
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _slot = slot;
            work()
        })
        .map_err(|e| rustler::Error::Term(Box::new(e.to_string())))?;
    Ok(atoms::ok())
}
#[rustler::nif(schedule = "DirtyIo")]
fn renderer_start_nif(env: Env, opts: Start, observer: LocalPid, id: u64) -> NifResult<Atom> {
    let caller = env.pid();
    job(&STARTS, "emerge-start", move || {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| start(observer, opts)))
                .unwrap_or_else(|_| Err("renderer startup failed".into()));
        let _ = OwnedEnv::new().send_and_clear(&caller, |_| (emerge_skia_reply(), id, result));
    })
}
#[rustler::nif(schedule = "DirtyIo")]
fn renderer_stop_nif(env: Env, r: ResourceArc<Session>, ms: u32, id: u64) -> NifResult<Atom> {
    r.request_stop();
    if r.owner.lifecycle.status().is_ok_and(|s| s.cleanup_complete) {
        return Ok(complete());
    }
    let caller = env.pid();
    let owner = r.owner.clone();
    job(&STOPS, "emerge-stop", move || {
        let result = owner
            .lifecycle
            .wait(Duration::from_millis(ms.into()))
            .map(|()| atoms::ok());
        let _ = OwnedEnv::new().send_and_clear(&caller, |_| (emerge_skia_reply(), id, result));
    })
}
