//! Read-only inherited scanout inspection and the first application transaction.
//! No inherited framebuffer is owned, copied, restored or destroyed here.
use super::{
    core::{Card, is_cursor_plane, mode_id, prop_handle},
    power::PresenterControl,
};
use crate::native_log::NativeLogRelay;
use drm::control::{
    self, AtomicCommitFlags, Device as _, ResourceHandle, atomic, connector, crtc, plane, property,
};
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

type Assignment = (control::RawResourceHandle, property::Handle, u64);

pub(super) struct Handoff {
    modeset: bool,
    connector: connector::Handle,
    crtc: crtc::Handle,
    routing: property::Handle,
    mode: property::Handle,
    active: property::Handle,
    cursors: Vec<Assignment>,
}

fn properties<T: ResourceHandle + Copy>(
    card: &Card,
    object: T,
) -> Result<
    (
        std::collections::HashMap<String, property::Info>,
        control::PropertyValueSet,
    ),
    String,
> {
    let values = card.get_properties(object).map_err(|e| e.to_string())?;
    let infos = values.as_hashmap(card).map_err(|e| e.to_string())?;
    Ok((infos, values))
}

fn compatible(
    active: u64,
    routing: u64,
    crtc: u32,
    current: Option<&control::Mode>,
    requested: &control::Mode,
) -> bool {
    active == 1
        && routing == u64::from(crtc)
        && current.is_some_and(|mode| mode_id(mode) == mode_id(requested))
}

impl Handoff {
    /// The caller has already leased this output; never query a sibling's planes.
    pub(super) fn read(
        card: &Card,
        connector: connector::Handle,
        crtc: crtc::Handle,
        primary: plane::Handle,
        requested: &control::Mode,
    ) -> Result<Self, String> {
        let (connector_props, connector_values) = properties(card, connector)?;
        let (crtc_props, crtc_values) = properties(card, crtc)?;
        let routing = prop_handle(&connector_props, "CRTC_ID")?;
        let mode = prop_handle(&crtc_props, "MODE_ID")?;
        let active = prop_handle(&crtc_props, "ACTIVE")?;
        let raw = |values: &control::PropertyValueSet, handle| {
            values
                .iter()
                .find_map(|(&key, &value)| (key == handle).then_some(value))
                .ok_or_else(|| "inherited DRM property has no value".to_string())
        };
        let current = card.get_crtc(crtc).map_err(|e| e.to_string())?;
        let primary_state = card.get_plane(primary).map_err(|e| e.to_string())?;
        if primary_state.crtc().is_some_and(|bound| bound != crtc) {
            return Err("selected DRM primary plane belongs to another output".into());
        }
        let cursors = cursor_assignments(card, crtc)?;
        Ok(Self {
            modeset: !compatible(
                raw(&crtc_values, active)?,
                raw(&connector_values, routing)?,
                crtc.into(),
                current.mode().as_ref(),
                requested,
            ),
            connector,
            crtc,
            routing,
            mode,
            active,
            cursors,
        })
    }

    fn assignments(&self, mode_blob: property::Value<'_>) -> Vec<Assignment> {
        self.modeset
            .then_some([
                (
                    self.connector.into(),
                    self.routing,
                    u64::from(u32::from(self.crtc)),
                ),
                (self.crtc.into(), self.mode, mode_blob.into()),
                (self.crtc.into(), self.active, 1),
            ])
            .into_iter()
            .flatten()
            .chain(self.cursors.iter().copied())
            .collect()
    }

    pub(super) fn append(
        &self,
        request: &mut atomic::AtomicModeReq,
        mode_blob: property::Value<'_>,
    ) {
        for (object, property, value) in self.assignments(mode_blob) {
            request.add_raw_property(object, property, value);
        }
    }

    /// Call only with an Emerge-owned, rendered application frame. The initial
    /// real commit is blocking; event-driven presenters still wait for their flip.
    /// `Ok(false)` means cancellation before the real ioctl, not uncertain failure.
    pub(super) fn commit(
        &self,
        card: &Card,
        request: atomic::AtomicModeReq,
        completion: AtomicCommitFlags,
        stop: &AtomicBool,
        native_log: &NativeLogRelay,
        presenter: Option<&PresenterControl>,
    ) -> io::Result<bool> {
        self.commit_with(request, completion, |flags, req| card.atomic_commit(flags, req), || {
            if stop.load(Ordering::Acquire) {
                return Ok(false);
            }
            if self.modeset {
                native_log.info("drm_handoff", "replacement application frame ready; modeset required (not a seamless handoff)");
                if let Some(presenter) = presenter
                    && presenter.before_modeset(stop).is_err()
                {
                    return Ok(false);
                }
            } else {
                native_log.info("drm_handoff", "replacing inherited scanout without modeset");
            }
            Ok(!stop.load(Ordering::Acquire))
        })
    }

    fn commit_with(
        &self,
        request: atomic::AtomicModeReq,
        completion: AtomicCommitFlags,
        mut commit: impl FnMut(AtomicCommitFlags, atomic::AtomicModeReq) -> io::Result<()>,
        before_commit: impl FnOnce() -> io::Result<bool>,
    ) -> io::Result<bool> {
        let flags = if self.modeset {
            AtomicCommitFlags::ALLOW_MODESET
        } else {
            AtomicCommitFlags::empty()
        };
        // No early detach or modeset fallback when a compatible test is rejected.
        commit(flags | AtomicCommitFlags::TEST_ONLY, request.clone())?;
        if !before_commit()? {
            return Ok(false);
        }
        commit(flags | completion, request)?;
        Ok(true)
    }
}

fn cursor_assignments(card: &Card, crtc: crtc::Handle) -> Result<Vec<Assignment>, String> {
    card.plane_handles()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|plane| -> Result<Vec<Assignment>, String> {
            if !is_cursor_plane(card, plane)?
                || card.get_plane(plane).map_err(|e| e.to_string())?.crtc() != Some(crtc)
            {
                return Ok(Vec::new());
            }
            let (props, _) = properties(card, plane)?;
            ["FB_ID", "CRTC_ID"]
                .into_iter()
                .map(|name| prop_handle(&props, name).map(|property| (plane.into(), property, 0)))
                .collect()
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|planes| planes.into_iter().flatten().collect())
}

/// Explicit off/stop before the first upload must retire inherited cursors too,
/// even when no application cursor framebuffer has ever been created.
pub(super) fn retire_cursors(
    card: &Card,
    crtc: crtc::Handle,
    request: &mut atomic::AtomicModeReq,
) -> Result<(), String> {
    for (object, property, value) in cursor_assignments(card, crtc)? {
        request.add_raw_property(object, property, value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn handle<T: From<control::RawResourceHandle>>(value: u32) -> T {
        control::RawResourceHandle::new(value).unwrap().into()
    }

    fn fixture(modeset: bool) -> Handoff {
        Handoff {
            modeset,
            connector: handle(1),
            crtc: handle(2),
            routing: handle(3),
            mode: handle(4),
            active: handle(5),
            cursors: vec![(handle(6), handle(7), 0), (handle(6), handle(8), 0)],
        }
    }

    fn mode(clock: u32) -> control::Mode {
        drm_ffi::drm_mode_modeinfo {
            clock,
            hdisplay: 800,
            vdisplay: 600,
            ..Default::default()
        }
        .into()
    }

    #[test]
    fn compatibility_requires_active_routing_and_exact_timings_not_just_size() {
        let requested = mode(40000);
        assert!(compatible(1, 2, 2, Some(&requested), &requested));
        for (active, route, current) in [
            (0, 2, Some(requested)),
            (1, 0, Some(requested)),
            (1, 9, Some(requested)),
            (1, 2, None),
            (1, 2, Some(mode(39999))),
        ] {
            assert!(!compatible(active, route, 2, current.as_ref(), &requested));
        }
    }

    #[test]
    fn compatible_transaction_only_retires_cursor_and_never_rewrites_routing_or_mode() {
        let handoff = fixture(false);
        assert_eq!(
            handoff.assignments(property::Value::Blob(42)),
            handoff.cursors
        );
        assert_eq!(
            fixture(true).assignments(property::Value::Blob(42)).len(),
            5
        );
    }

    #[test]
    fn all_presenters_validate_then_transition_then_blocking_commit_with_their_completion_proof() {
        for modeset in [false, true] {
            for completion in [
                AtomicCommitFlags::empty(),
                AtomicCommitFlags::PAGE_FLIP_EVENT,
            ] {
                let trace = RefCell::new(Vec::new());
                fixture(modeset)
                    .commit_with(
                        atomic::AtomicModeReq::new(),
                        completion,
                        |flags, _| {
                            trace.borrow_mut().push(flags);
                            Ok(())
                        },
                        || {
                            assert_eq!(trace.borrow().len(), 1);
                            Ok(true)
                        },
                    )
                    .unwrap();
                let allowed = if modeset {
                    AtomicCommitFlags::ALLOW_MODESET
                } else {
                    AtomicCommitFlags::empty()
                };
                assert_eq!(
                    *trace.borrow(),
                    [allowed | AtomicCommitFlags::TEST_ONLY, allowed | completion]
                );
                assert!(
                    !trace
                        .borrow()
                        .iter()
                        .any(|flags| flags.contains(AtomicCommitFlags::NONBLOCK))
                );
            }
        }
    }

    #[test]
    fn rejected_test_or_cancelled_transition_never_commits_or_falls_back_to_blanking() {
        for fail_test in [false, true] {
            let calls = RefCell::new(0);
            let result = fixture(false).commit_with(
                atomic::AtomicModeReq::new(),
                AtomicCommitFlags::empty(),
                |_, _| {
                    *calls.borrow_mut() += 1;
                    if fail_test {
                        Err(io::Error::from_raw_os_error(libc::EINVAL))
                    } else {
                        Ok(())
                    }
                },
                || Ok(false),
            );
            if fail_test {
                assert!(result.is_err());
            } else {
                assert!(!result.unwrap());
            }
            assert_eq!(*calls.borrow(), 1);
        }
    }
}
