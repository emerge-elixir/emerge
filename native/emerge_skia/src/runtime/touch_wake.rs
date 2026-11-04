//! Bounded, per-renderer off-state recognition control. No display I/O here.
#![cfg_attr(not(all(feature = "drm", target_os = "linux")), allow(dead_code))]
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, rustler::NifUnitEnum)]
pub(crate) enum WakeGesture {
    SingleTap,
    DoubleTap,
    LongPress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Armed {
    pub revision: u64,
    pub gesture: WakeGesture,
    pub since_usec: u64,
}

#[derive(Default)]
struct State {
    armed: Option<Armed>,
    request: Option<Armed>,
}

#[derive(Default)]
pub(crate) struct TouchWakeControl(Mutex<State>);

impl TouchWakeControl {
    pub fn arm(&self, revision: u64, gesture: WakeGesture) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.request.is_none()
            && !state
                .armed
                .is_some_and(|a| a.revision == revision && a.gesture == gesture)
        {
            state.armed = Some(Armed {
                revision,
                gesture,
                since_usec: monotonic_usec(),
            });
            true
        } else {
            false
        }
    }

    pub fn disarm(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = State::default();
    }

    pub fn snapshot(&self) -> Option<Armed> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).armed
    }

    pub fn request(&self, armed: Armed) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.armed != Some(armed) || state.request.is_some() {
            return false;
        }
        state.request = state.armed.take();
        true
    }

    pub fn take_request(&self) -> Option<Armed> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .request
            .take()
    }
}

/// libinput touch timestamps use CLOCK_MONOTONIC, not Rust Instant's private epoch.
pub(crate) fn monotonic_usec() -> u64 {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: valid writable timespec; initialized only after successful clock_gettime.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, time.as_mut_ptr()) } != 0 {
        return 0;
    }
    let time = unsafe { time.assume_init() };
    (time.tv_sec.max(0) as u64)
        .saturating_mul(1_000_000)
        .saturating_add(time.tv_nsec.max(0) as u64 / 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_request_per_arm_and_disarming_cancels_pending_work() {
        let control = TouchWakeControl::default();
        assert_eq!(control.snapshot(), None);
        control.arm(3, WakeGesture::SingleTap);
        let old = control.snapshot().unwrap();
        control.arm(3, WakeGesture::SingleTap);
        assert_eq!(control.snapshot(), Some(old));
        assert!(control.request(old));
        assert!(!control.request(old));
        control.arm(3, WakeGesture::SingleTap);
        assert_eq!(control.snapshot(), None);
        control.disarm();
        assert_eq!(control.take_request(), None);
        control.arm(4, WakeGesture::DoubleTap);
        assert!(!control.request(old));
        assert!(monotonic_usec() > 0);
    }
}
