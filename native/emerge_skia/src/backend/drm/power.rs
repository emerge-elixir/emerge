//! DRM power owner. The lease and backlight outlive internal presenter sessions.
//! Off stops and destroys the presenter; on creates a fresh one from the latest tree.
//! No GPU/KMS wait holds the revision lock.
use super::{
    DrmBackendStartupInfo, DrmRunConfig, DrmRunContext,
    core::{Card, prop_handle},
};
use crate::{
    actors::{EventMsg, TreeMsg},
    backend::backlight::{self, Backlight, Baseline, Reservations, Selection, SysfsAttributes},
    linux_wait::{EventFd, poll_fds},
    runtime::config::ConfigRequest,
};
use crossbeam_channel::TrySendError;
use drm::control::{AtomicCommitFlags, Device as _, atomic, property};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct PresenterControl {
    pub(super) card: Arc<Card>,
    sequence: Arc<AtomicU64>,
    presented: Arc<AtomicBool>,
    retiring: Arc<AtomicBool>,
    dark_ack: Arc<AtomicBool>,
    wake: EventFd,
}

impl PresenterControl {
    pub fn request_frame(
        &self,
        tree: &crossbeam_channel::Sender<TreeMsg>,
        dimensions: (u32, u32),
        stop: &AtomicBool,
    ) -> Result<u64, String> {
        let request = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let mut message = TreeMsg::Batch(vec![
            TreeMsg::Resize {
                width: dimensions.0 as f32,
                height: dimensions.1 as f32,
                scale: 1.0,
            },
            TreeMsg::RequestFrame { request },
        ]);
        loop {
            if stop.load(Ordering::Acquire) {
                return Err("DRM presenter stopped".into());
            }
            match tree.send_timeout(message, Duration::from_millis(10)) {
                Ok(()) => return Ok(request),
                Err(crossbeam_channel::SendTimeoutError::Timeout(returned)) => message = returned,
                Err(_) => return Err("DRM tree owner stopped".into()),
            }
        }
    }

    pub fn presented(&self) {
        self.presented.store(true, Ordering::Release);
        let _ = self.wake.signal();
    }

    pub fn before_detach(&self) {
        self.retiring.store(true, Ordering::Release);
        let _ = self.wake.signal();
        // DCS backlight commands must precede panel unprepare. The parent owns
        // the backlight; acknowledgement means it attempted suppression.
        while !self.dark_ack.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

struct Child {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
    wake: EventFd,
    control: PresenterControl,
    ready: std::sync::mpsc::Receiver<Result<DrmBackendStartupInfo, String>>,
}

fn spawn_presenter(
    context: &DrmRunContext,
    config: &DrmRunConfig,
    card: Arc<Card>,
    sequence: Arc<AtomicU64>,
) -> Result<Child, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let wake = EventFd::new().map_err(|e| e.to_string())?;
    let (tx, ready) = std::sync::mpsc::channel();
    let control = PresenterControl {
        card,
        sequence,
        presented: Arc::new(AtomicBool::new(false)),
        retiring: Arc::new(AtomicBool::new(false)),
        dark_ack: Arc::new(AtomicBool::new(false)),
        wake: context.presenter_wake.clone(),
    };
    let child_context = DrmRunContext {
        stop: stop.clone(),
        running_flag: Arc::new(AtomicBool::new(true)),
        presenter_wake: wake.clone(),
        startup_tx: tx,
        presenter: Some(control.clone()),
        ..context.clone()
    };
    let child_config = config.clone();
    let assets = crate::assets::current_context();
    let owner_wake = context.presenter_wake.clone();
    let handle = thread::spawn(move || {
        let _assets = assets.enter();
        super::run_presenter(child_context, child_config);
        let _ = owner_wake.signal();
    });
    Ok(Child {
        handle,
        stop,
        wake,
        control,
        ready,
    })
}

fn stop_child(child: &Child) {
    child.stop.store(true, Ordering::Release);
    let _ = child.wake.signal();
}

/// Read the master device's identity, not a mutable /dev symlink or anonymous
/// lease fd's stat information. Connector IDs are scoped to that card.
fn connector_inventory(card: &Card) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let number = match &card.1 {
        Some(lease) => lease.device_number(),
        None => card.0.metadata().map(|m| m.rdev()),
    }
    .map_err(|e| e.to_string())?;
    let card_path = PathBuf::from(format!(
        "/sys/dev/char/{}:{}",
        libc::major(number),
        libc::minor(number)
    ))
    .canonicalize()
    .map_err(|e| e.to_string())?;
    let connector = card.1.as_ref().ok_or("DRM output has no lease")?.connector;
    let connectors = std::fs::read_dir("/sys/class/drm")
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|e| e.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| path.join("connector_id").is_file())
        .map(|path| path.canonicalize().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let selected = connectors
        .iter()
        .find(|path| {
            path.parent() == Some(card_path.as_path())
                && std::fs::read_to_string(path.join("connector_id"))
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    == Some(connector.into())
        })
        .cloned()
        .ok_or("selected DRM connector is absent from sysfs")?;
    Ok((selected, connectors))
}

fn bind(card: &Card, config: &DrmRunConfig) -> Result<Option<Backlight<SysfsAttributes>>, String> {
    let selection = match config.backlight.as_deref() {
        None => Selection::Unmanaged,
        Some("auto") => Selection::Auto,
        Some(path) => Selection::Explicit(path.into()),
    };
    if matches!(selection, Selection::Unmanaged) {
        if config.power.brightness.is_some() {
            return Err("brightness requires a managed backlight".into());
        }
        return Ok(None);
    }
    let (connector, connectors) = connector_inventory(card)?;
    let inventory = backlight::discover(Path::new("/sys/class/backlight"), &connectors)?;
    let Some(candidate) = backlight::select(&inventory, &connector, &selection)? else {
        if config.power.brightness.is_some() {
            return Err(
                "no proven backlight for this connector; select its device explicitly".into(),
            );
        }
        return Ok(None);
    };
    let reservations = Reservations::process_wide();
    let mut light = Backlight::open(&candidate, &reservations, None)?;
    let baseline = light.baseline();
    let (maximum, requested) = config.backlight_state.remember(
        (baseline.identity.device, baseline.identity.inode),
        baseline.maximum,
        baseline.requested,
    )?;
    light.use_baseline(Baseline {
        maximum,
        requested,
        ..baseline
    })?;
    light.set_brightness(config.power.brightness)?;
    Ok(Some(light))
}

fn darken(light: &mut Option<Backlight<SysfsAttributes>>) -> Result<(), String> {
    if let Some(light) = light {
        light.darken_if_needed()?;
    }
    Ok(())
}

/// The descriptor is a kernel output lease, so its plane inventory contains
/// only this owner's planes. Never inspect or detach a sibling's CRTC.
fn detach(card: &Card) -> Result<(), String> {
    let lease = card.1.as_ref().ok_or("DRM output has no lease")?;
    let mut request = atomic::AtomicModeReq::new();
    for plane in card.plane_handles().map_err(|e| e.to_string())? {
        let props = card
            .get_properties(plane)
            .and_then(|p| p.as_hashmap(card))
            .map_err(|e| e.to_string())?;
        request.add_property(
            plane,
            prop_handle(&props, "FB_ID")?,
            property::Value::Framebuffer(None),
        );
        request.add_property(
            plane,
            prop_handle(&props, "CRTC_ID")?,
            property::Value::CRTC(None),
        );
    }
    let props = card
        .get_properties(lease.connector)
        .and_then(|p| p.as_hashmap(card))
        .map_err(|e| e.to_string())?;
    request.add_property(
        lease.connector,
        prop_handle(&props, "CRTC_ID")?,
        property::Value::CRTC(None),
    );
    let props = card
        .get_properties(lease.crtc)
        .and_then(|p| p.as_hashmap(card))
        .map_err(|e| e.to_string())?;
    request.add_property(
        lease.crtc,
        prop_handle(&props, "ACTIVE")?,
        property::Value::Boolean(false),
    );
    request.add_property(
        lease.crtc,
        prop_handle(&props, "MODE_ID")?,
        property::Value::Blob(0),
    );
    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, request)
        .map_err(|e| format!("DRM power detach failed: {e}"))
}

pub(super) fn run(context: DrmRunContext, config: DrmRunConfig) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_inner(&context, &config)
    }))
    .unwrap_or_else(|_| {
        context
            .lifecycle
            .quarantine_output("DRM power owner panicked");
        Err("DRM power owner panicked".into())
    });

    if let Err(error) = result {
        let _ = context.startup_tx.send(Err(error.clone()));
        context.native_log.error("drm_power", error);
    }
    context.input_gate.store(false, Ordering::Release);
    context.stop.store(true, Ordering::Release);
    context.running_flag.store(false, Ordering::Release);
    let _ = context.input_wake.signal();
    for request in context.control_rx.try_iter() {
        request.finish(Err("DRM power owner stopped".into()));
    }
}

fn run_inner(context: &DrmRunContext, config: &DrmRunConfig) -> Result<(), String> {
    config.power.validate()?;
    if context.stop.load(Ordering::Acquire) {
        return Err("DRM startup aborted".into());
    }
    let card = Arc::new(super::session::open_output(
        config.card_path.as_deref(),
        config.output.as_deref(),
        config.mode.as_deref(),
        config.requested_size,
        config.hw_cursor,
        Some(context.lifecycle.clone()),
    )?);
    let mut light = bind(&card, config)?;
    // Detach inherited scanout even when suppression fails. No presenter exists.
    let suppressed = darken(&mut light);
    if let Err(error) = detach(&card) {
        context.lifecycle.quarantine_output(error.clone());
        if let Some(light) = &mut light {
            light.retain();
        }
        std::mem::forget(card);
        return Err(error);
    }
    suppressed?;
    let result = run_owner(context, config, &card, &mut light);
    let suppressed = darken(&mut light);
    if let Err(error) = detach(&card) {
        context.lifecycle.quarantine_output(error.clone());
        if let Some(light) = &mut light {
            light.retain();
        }
        std::mem::forget(card);
        return Err(error);
    }
    if context
        .lifecycle
        .status()
        .map_or(true, |status| status.state == "quarantined")
    {
        if let Some(light) = &mut light {
            light.retain();
        }
        std::mem::forget(card);
        return Err("DRM presenter ownership is quarantined".into());
    }
    suppressed?;
    if let Some(light) = light.take() {
        light.release_after_detach()?;
    }
    result
}

fn run_owner(
    context: &DrmRunContext,
    config: &DrmRunConfig,
    card: &Arc<Card>,
    light: &mut Option<Backlight<SysfsAttributes>>,
) -> Result<(), String> {
    let sequence = Arc::new(AtomicU64::new(0));
    // Prepare once for capabilities even with initial-off; no tree exists yet.
    let mut child = Some(spawn_presenter(
        context,
        config,
        card.clone(),
        sequence.clone(),
    )?);
    let mut pending = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner_loop(
            context,
            config,
            card,
            light,
            sequence,
            &mut child,
            &mut pending,
        )
    }))
    .unwrap_or_else(|_| Err("DRM power owner panicked".into()));
    if let Some(pending) = pending {
        pending.finish(Err("DRM power owner stopped".into()));
    }
    context.input_gate.store(false, Ordering::Release);
    if let Some(child) = child {
        let suppressed = darken(light);
        stop_child(&child);
        child.control.dark_ack.store(true, Ordering::Release);
        child.handle.join().map_err(|_| "DRM presenter panicked")?;
        suppressed?;
    }
    result
}

fn owner_loop(
    context: &DrmRunContext,
    config: &DrmRunConfig,
    card: &Arc<Card>,
    light: &mut Option<Backlight<SysfsAttributes>>,
    sequence: Arc<AtomicU64>,
    child: &mut Option<Child>,
    pending: &mut Option<ConfigRequest>,
) -> Result<(), String> {
    let mut desired = config.power.clone();
    let mut input_pending = None;
    let mut visible = false;
    let mut startup = None;
    let mut ready_sent = false;
    loop {
        let _ = context.presenter_wake.drain();
        if let Some(child) = child.as_ref() {
            let _ = child.wake.signal();
        }
        let incoming =
            context
                .control_rx
                .try_iter()
                .take(64)
                .fold(None::<ConfigRequest>, |old, next| {
                    if let Some(old) = old {
                        old.finish(Err("config superseded".into()));
                    }
                    Some(next)
                });
        if let Some(next) = incoming {
            let applied = next.apply(|| {
                if let Some(power) = &next.config.drm_power {
                    desired = power.clone();
                }
                if !desired.display_on {
                    // Ignore input and pending presenter work as soon as off starts.
                    context.input_gate.store(false, Ordering::Release);
                    visible = false;
                    if let Some(child) = child.as_ref() {
                        stop_child(child);
                    }
                    darken(light)?;
                }
                match light {
                    Some(light) => light.set_brightness(desired.brightness),
                    None if desired.brightness.is_some() => {
                        Err("brightness requires a managed backlight".into())
                    }
                    None => Ok(()),
                }
            });
            match applied {
                Err(error) => {
                    next.finish(Err(error.clone()));
                    if error != "config superseded" {
                        return Err(error);
                    }
                }
                Ok(()) => {
                    if let Some(old) = pending.replace(next.clone()) {
                        old.finish(Err("config superseded".into()));
                    }
                    input_pending = Some(next.clone());
                    if visible {
                        next.finish_output(Ok(()));
                    }
                }
            }
        }
        if let Some(request) = input_pending.take() {
            match context
                .event_tx
                .try_send(EventMsg::ConfigureInput(request.clone()))
            {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => input_pending = Some(request),
                Err(TrySendError::Disconnected(_)) => {
                    return Err("renderer input owner stopped".into());
                }
            }
        }
        let stopping = context.stop.load(Ordering::Acquire);
        if let Some(active) = child.as_ref() {
            if let Ok(info) = active.ready.try_recv() {
                let info = info?;
                if !ready_sent {
                    startup = Some(info);
                }
                if !desired.display_on {
                    stop_child(active);
                }
            }
            if stopping || active.control.retiring.load(Ordering::Acquire) {
                context.input_gate.store(false, Ordering::Release);
                visible = false;
                let unexpected =
                    desired.display_on && !stopping && !active.stop.load(Ordering::Acquire);
                let suppressed = darken(light);
                stop_child(active);
                active.control.dark_ack.store(true, Ordering::Release);
                suppressed?;
                if unexpected {
                    return Err("DRM presenter stopped".into());
                }
            }
            if desired.display_on
                && !stopping
                && !visible
                && !active.stop.load(Ordering::Acquire)
                && active.control.presented.load(Ordering::Acquire)
                && !active.handle.is_finished()
            {
                if let Some(light) = light {
                    light.illuminate_after_present()?;
                }
                visible = true;
                context.input_gate.store(true, Ordering::Release);
                let _ = context.input_wake.signal();
                if let Some(pending) = pending.as_ref() {
                    pending.finish_output(Ok(()));
                }
            }
        }
        if child
            .as_ref()
            .is_some_and(|child| child.handle.is_finished())
        {
            let old = child.take().ok_or("missing DRM presenter")?;
            let stopped = old.stop.load(Ordering::Acquire);
            old.handle.join().map_err(|_| "DRM presenter panicked")?;
            visible = false;
            if !stopped {
                return Err("DRM presenter stopped".into());
            }
        }
        if !ready_sent && startup.is_some() && (desired.display_on || child.is_none()) {
            context
                .startup_tx
                .send(Ok(startup.take().ok_or("missing DRM startup metadata")?))
                .map_err(|_| "DRM startup receiver stopped")?;
            ready_sent = true;
        }
        if child.is_none() {
            if stopping {
                return Ok(());
            }
            if desired.display_on {
                *child = Some(spawn_presenter(
                    context,
                    config,
                    card.clone(),
                    sequence.clone(),
                )?);
            } else if let Some(pending) = pending.as_ref() {
                pending.finish_output(Ok(()));
            }
        }
        poll_fds(
            &mut [libc::pollfd {
                fd: context.presenter_wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            }],
            Some(Duration::from_millis(if input_pending.is_some() {
                10
            } else {
                250
            })),
        )
        .map_err(|error| error.to_string())?;
    }
}
