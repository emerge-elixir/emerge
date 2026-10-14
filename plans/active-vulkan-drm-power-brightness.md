# Vulkan DRM power and brightness

Status: implemented; local gates and authorized integrated-GPU evidence below.
Physical brightness/optical, video and destructive-fault qualification remain open.

- Branch: `plan/vulkan-drm-power-brightness`
- Worktree: `/workspace/emerge-vulkan-drm-power-brightness`
- Base: `ad91b14` on `release/0.5.0`

## Scope

CPU raster and OpenGL power/brightness are already implemented. **Only Vulkan DRM
is being added here.** Reuse their shared power/backlight owner and existing
stop/recreate contract; do not reimplement those backends or their power policy.
Shared code changes are limited to proven requirements of Vulkan integration.

Keep the existing API: explicit `renderer.api: :vulkan`, independently selected
`renderer.device`, `output.display` and `output.backlight.brightness`. Preserve
the public renderer, callback PID/state, output lease, backlight reservation and
original requested brightness baseline across internal presenter recreation.

Out of scope: raster GPU-upload, `api: :auto` power admission, GPU-state restoration,
live connector/mode changes, frame caps, Wayland power, system suspend, new NIFs,
config/frame proof protocols, and changes to the FP3 OpenGL consumer setup.

## Existing code to reuse

- `native/emerge_skia/src/backend/drm/power.rs`: persistent output/backlight owner,
  child stop/recreate, presentation-gated illumination, bounded config lane and
  joined input/output completion. No second Vulkan power owner.
- `native/emerge_skia/src/backend/backlight/`: device association/reservation,
  finite `0..1` quantization, requested baseline, suppression and failed-enable
  cleanup. Brightness zero is not display-off; updates while off retain intent.
- `native/emerge_skia/src/backend/drm/vulkan.rs`: already duplicates the parent's
  lease, requests a fresh application frame, gates rendering before first upload,
  notifies `PresenterControl::presented()` on page flip and participates in the
  suppression handshake. Vulkan controls now use this same owner.
- `ScanoutLifecycle::detach_output`: GPU idle → blocking KMS disable → old-event
  drain before current/in-flight/prepared slots become reusable. Preserve the
  three-slot inventory and explicit `SYNC_FD` / `IN_FENCE_FD` ownership.
- Existing viewport reconciliation, renderer recovery and config revision tokens.
  Off defers callback rendering/serialization/upload; recovery retains desired
  intent and only replaces a renderer after proven cleanup.

## Addressed Vulkan-specific gaps

1. Managed retirement now disables KMS explicitly, without restoring inherited
   scanout. Standalone functional probes retain their restoration policy.
2. Stop/unplug, early returns and unwind cleanup use common retirement;
   suppression precedes GPU idle, blocking disable and old-event drain.
3. Explicit Vulkan admission is consistent across Elixir, native startup and
   `DrmBackendWake`. Automatic API controls stay guarded; raster GPU-upload was
   separately admitted in the subsequently requested follow-up.
4. Tests now cover fresh application-frame gating, teardown ordering/failure
   barriers, shared suppression acknowledgements, retained stream capabilities,
   config/callback preservation and opt-in native rapid power cycling.

## Implementation

### 1. Make managed Vulkan retirement safe

- [x] For a healthy managed presenter, reuse the existing detach barriers before
  destroying Vulkan/GBM/framebuffer owners. Leave the final KMS state disabled;
  skip restore after successful managed detach. Preserve standalone probe restore.
- [x] Move suppression acknowledgement ahead of every managed disable, including
  stop racing unplug and safe missing-flip recovery. Acknowledgement means an
  attempt, not proof of success when sysfs reports an error.
- [x] Do not publish illumination/capture/timing/animation progress from drained
  or shutdown-only events. Atomic acceptance or GPU idle alone cannot retire
  KMS-owned buffers.
- [x] Preserve process-terminal quarantine for uncertain ownership/device loss.
  Audit post-allocation early returns and presenter panics so they cannot bypass
  cleanup/quarantine or strand the parent handshake. Never extend timeouts to
  disguise a blocked driver or use off/on to recover poisoned Vulkan state.
- [x] Disable/revalidate PRIME capabilities with child retirement/recreation and
  retire old imports/source leases safely. Off must not accumulate frame queues,
  producer leases or descriptors; wake must use the fresh presenter's capabilities.

### 2. Exercise Vulkan through the existing owner

- [x] Initially off still acquires the output/backlight. Its temporary capability
  presenter must retire without a placeholder commit or application upload.
  Stop before first upload must safely release a healthy owner.
- [x] Off gates UI input and retires the child; on creates a fresh child and uses
  the existing monotonic `RequestFrame` barrier. Illuminate only after its first
  application page flip, not resource readiness, submission or incidental wakes.
- [x] Test off before/after the first flip and on while the old child retires.
  Per-child `PresenterControl` plus current desired-on/live-child checks must
  prevent old presentation notifications from relighting a retired output.
- [x] Brightness-only changes use the existing owner with no draw, modeset or
  presenter replacement. While dark retain the latest level; `nil` restores the
  original requested baseline, including after renderer recovery.
- [x] Keep session/revision completion fencing and joined input/output acks.
  Off completion follows safe retirement; on follows presentation-gated enable.
  Superseded requests cannot report applied. No GPU/KMS wait or child join holds
  the revision lock; all hardware I/O remains on native owner threads.

### 3. Enable explicit Vulkan admission and document it

- [x] Add Vulkan to Elixir `supports_drm_power?/1`, native startup admission and
  selected-runner `DrmBackendWake` capability. Preserve compiled-feature,
  exact-device/import/synchronization and terminal-quarantine admission checks.
- [x] Keep `:auto`, connector/mode/device changes, title and DRM frame caps guarded.
  Raster GPU-upload remained guarded during this Vulkan phase and is now handled
  by the separately requested follow-up. No silent OpenGL fallback or alternate output selection.
- [x] Update `test/emerge_skia/drm_power_config_test.exs` and
  `test/emerge/config_test.exs`: explicit Vulkan accepts startup/live off/on and
  brightness; unfinished paths remain rejected. Verify rejected native requests
  do not advance revisions.
- [x] Update DRM/Backlight module docs, viewport configuration reference, DRM
  lifecycle and config reconciliation to include Vulkan only after implementation.
  No strict config/frame provenance, physical power-off or optical no-flash promise.

## Validation gates

### Deterministic tests

Extend existing Vulkan lifecycle and backlight fake-attribute tests. Extract only
small pure helpers/injectable operations needed for Vulkan coverage, not a new
presenter framework. CPU/OpenGL checks are regressions, not implementation work.

- [x] Current + in-flight + prepared ownership at off; `EBUSY`, lost/late flip,
  startup failure, Stop before upload, each barrier failure and repeated cycles.
  Require suppression → GPU idle → blocking disable → event drain → destruction.
- [x] Application-frame matching, stale queued scenes, valid empty application
  frames and cursor/capture/video wakes before upload. Wake requires application
  flip before brightness/unblank; failed enable resuppresses and reports failure.
- [x] Superseded/rapid requests, child exit and quarantine keep ownership bounded
  and completions at most once. Failed cleanup pins output/backlight reservations.
- [x] Reuse `test/emerge/config_callback_test.exs` / lifecycle coverage for off
  rendering deferral, latest-state resume, callback identity and desired-config
  recovery. Fake adapters do not prove Vulkan or physical backlight behavior.

After implementation, run `cargo test` and `mix test`, preferably
`EMERGE_SKIA_BUILD=1 ./ci-tests.sh`. Default Cargo features are Wayland, so also run:

```sh
cargo test --release --manifest-path native/emerge_skia/Cargo.toml \
  --no-default-features --features drm-vulkan
cargo clippy --manifest-path native/emerge_skia/Cargo.toml \
  --no-default-features --features drm-vulkan -- -D warnings
```

Repeat feature tests/lints for `drm-all`; regression-check `drm` and `drm-raster`.
Source-build/test matching Vulkan-only and combined Mix profiles. No new EGL/GL
requirement in Vulkan-only, GPU dependency in CPU DRM or per-frame idle wait.

### Authorized Vulkan hardware

- [x] Add an opt-in single-output test with explicit card, connector/mode, Vulkan
  node and optional backlight selection. Never automatically modeset the desktop
  GPU or write an arbitrary backlight; no permissions/key/deployment changes.
- [ ] Cover delayed upload, initial-off, repeated/rapid off/on, `nil/0/0.5/1`,
  brightness while off, latest-state wake, managed/unmanaged backlights and Stop
  in each phase. Observe the panel optically as well as commit/sysfs traces.
- [ ] Check panel DCS suppression-before-unprepare, unplug/reconnect, retained
  handles after callback death, clean reacquisition and sibling presentation.
  Quarantine fault injection requires a separately authorized disposable process.
- [ ] Include static UI, animation, CPU video and supported PRIME video. Record
  cleanup status, frame/FD/GPU/source-lease high-water marks and capture behavior
  while off. Require no growth across clean cycles or default-content wake flashes.
- [x] Enable `EMERGE_VULKAN_SYNC_VALIDATION=1` where supported and retain complete
  native output. No synchronization/lifetime errors or unexpected quarantine.
- [ ] Measure active-static versus extended-off CPU/wakeups, frame production,
  off/on latency and panel current where available. Off has no live presenter or
  routine rendering; document the existing owner polling/external producer work.

The rapid native on/off hardware test exposed an intentionally canceled startup
being treated as an owner failure. The shared owner now ignores readiness errors
only after that child has been stopped; retirement/join and global quarantine
still gate completion. Pure regression tests preserve genuine startup failures
and initial-off capability metadata. No CPU/OpenGL power policy was reimplemented.

Completion requires deterministic gates and recorded Vulkan target evidence
(device/kernel/Mesa, build profile, selected paths, optical observations, timings,
cleanup). Without suitable authorized hardware, leave qualification open. Do not
reopen CPU/OpenGL implementation or acceptance as part of this Vulkan task.

## Recorded validation

- `EMERGE_SKIA_BUILD=1 ./ci-tests.sh`: passed (quality, full-sweep Mix, default
  Cargo tests, Dialyzer). Mix: 677 passed, 12 hardware tests excluded.
- Source Vulkan-only and combined DRM OpenGL/Vulkan Mix profiles: 677 passed each.
- Release Cargo tests and strict all-target Clippy: `drm-vulkan`, `drm-all`, `drm`,
  `drm-raster` all passed, including the canceled-startup regression.
- Authorized target: Radeon 610M / Ryzen 9 7950X (`RADV RAPHAEL_MENDOCINO`),
  PCI `0000:6c:00.0`, kernel `7.2.7-1-cachyos`, Mesa `26.2.4-arch1.1`.
  KMS `/dev/dri/by-path/pci-0000:6c:00.0-card`, `DP-4`, 1920×1080; Vulkan
  `/dev/dri/by-path/pci-0000:6c:00.0-render` (226:129), software=false.
- Vulkan-only and combined hardware profiles: 3 tests passed each. Ten matched
  instance startup/final reports per run, synchronization validation enabled with
  Khronos layer `1.4.363`, zero errors/warnings. Full native output:
  [Vulkan-only](validation/vulkan-drm-power-brightness/vulkan-only.txt),
  [combined](validation/vulkan-drm-power-brightness/combined.txt).
- Verified delayed initial upload, initial off, three full off/on cycles,
  twenty rapid on/off pairs, latest-content RGBA captures, one-second extended
  off with no frames, Stop before upload, cleanup_complete and reacquisition.
  Off-state descriptors stayed 26→26. KMS returned to disabled; desktop `card1`
  outputs `DP-1`/`DP-2` remained enabled.
- No sysfs backlight exists on this target. Brightness levels, DCS, optical
  observations, animation/video, unplug, sibling rendering, GPU/source-lease
  high-water measurements, electrical power and destructive faults remain open.

The user subsequently requested [raster GPU-upload power](active-raster-gpu-upload-drm-power-brightness.md);
that follow-up does not change this Vulkan ownership contract.

## References

- [DRM lifecycle](../guides/internals/drm-outputs-and-lifecycle.md),
  [config reconciliation](../guides/internals/config-reconciliation.md),
  [native builds](../guides/reference/native-renderer-builds.md).
- [Existing CPU/OpenGL qualification](active-native-drm-power.md) stays separate.
  [Viewport settings](active-declarative-viewport-settings.md) retains GPU-upload
  and other unrelated controls; [Wayland Vulkan](active-vulkan-acquire-synchronization.md)
  synchronization is not being redesigned here.
- Linux [backlight sysfs ABI](https://www.kernel.org/doc/Documentation/ABI/stable/sysfs-class-backlight)
  and [DRM/KMS completion contracts](https://docs.kernel.org/gpu/drm-kms.html).
