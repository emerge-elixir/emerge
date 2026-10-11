//! Viewport-lifetime baseline storage. No file descriptors or I/O in this resource.
//! Persist before suppression so a failed startup cannot recapture our own zero.
#[cfg(any(all(feature = "drm-core", target_os = "linux"), test))]
use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
pub(crate) struct BacklightState {
    #[cfg(any(all(feature = "drm-core", target_os = "linux"), test))]
    baselines: Mutex<HashMap<(u64, u64), (u32, u32)>>,
}

#[rustler::resource_impl]
impl rustler::Resource for BacklightState {}

#[cfg(any(all(feature = "drm-core", target_os = "linux"), test))]
impl BacklightState {
    pub(crate) fn remember(
        &self,
        identity: (u64, u64),
        maximum: u32,
        requested: u32,
    ) -> Result<(u32, u32), String> {
        if maximum == 0 || requested > maximum {
            return Err("invalid initial backlight baseline".into());
        }
        let mut entries = self
            .baselines
            .lock()
            .map_err(|_| "backlight baseline storage poisoned")?;
        let baseline = *entries.entry(identity).or_insert((maximum, requested));
        if baseline.0 != maximum || baseline.1 > maximum {
            return Err("backlight baseline no longer matches device limits".into());
        }
        Ok(baseline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_startup_and_dark_recovery_do_not_replace_requested_baseline() {
        let state = BacklightState::default();
        assert_eq!(state.remember((1, 2), 100, 73).unwrap(), (100, 73));
        assert_eq!(state.remember((1, 2), 100, 0).unwrap(), (100, 73));
        assert_eq!(state.remember((1, 3), 100, 42).unwrap(), (100, 42));
        assert!(state.remember((1, 2), 200, 0).is_err());
    }

    #[test]
    fn invalid_first_capture_does_not_poison_later_admission() {
        let state = BacklightState::default();
        assert!(state.remember((1, 2), 0, 0).is_err());
        assert!(state.remember((1, 2), 100, 101).is_err());
        assert_eq!(state.remember((1, 2), 100, 37).unwrap(), (100, 37));
    }
}
