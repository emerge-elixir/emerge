# DRM Vulkan display unplug/replug

The presenter currently promotes an ordinary connector disconnect to a process-terminal
ownership fault. The viewport lifecycle correctly refuses to restart a quarantined renderer,
so a reconnect cannot recover it. Do not clear quarantine or reboot the robot as a workaround.

## Implementation

- Keep the existing presenter, Vulkan device, leased connector/mode, framebuffer inventory,
  scene state, video endpoints and input actors alive during HDMI loss.
- Before recycling any scanout slot: prove GPU idle, acknowledge a **blocking** atomic disable
  of the primary plane/CRTC/connector, then drain old page-flip events. Only then reset slot
  ownership. If any proof fails, retain the existing terminal quarantine.
- Poll the same connector and originally selected mode while paused, indefinitely after
  initial startup. Force-refresh connector EDID/modes during recovery so cached disconnected
  metadata does not prevent reconnect. Do not force-probe the normal active polling path.
  Do not spin, render/commit into a missing output, or publish false presents.
- Resume with a fresh full modeset and the latest scene even when no new UI event arrives;
  invalidate the visible-frame fingerprint before reusing a formerly stale scanout slot.
- Handle disconnect races at atomic commit, and repair missing flips using the same ownership
  barriers (including a quick unplug/replug entirely between connector polls).
- Reap already-completed video imports on suspension, retaining the current video image and
  stable stream identity. Do not recreate capture or its DMA-BUF producer domain.
- Stopping while detached must not try to restore a disconnected connector's old mode.
- Same connector/mode recovery only: a replacement monitor lacking the selected mode remains
  paused. Device removal, GPU loss and unsuccessful ownership barriers remain terminal.

Kernel contract: https://docs.kernel.org/gpu/drm-kms.html, `atomic_commit`: synchronous
commits must wait for preceding updates; they must not return EBUSY for a pending predecessor.
Use ALLOW_MODESET without NONBLOCK/PAGE_FLIP_EVENT for the disable barrier.

## Validation

Validation (no real HDMI/GPU hotplug or firmware deployment):

- `cargo test --no-default-features --features drm-vulkan`: 1,519 passed, 1 ignored,
  including ordered barrier failures, partial-render/terminal refusal, 100 reconnect cycles,
  current/in-flight/prepared retirement, and long-absence/fast-replug policy.
- `cargo clippy --no-default-features --features drm-all -- -D warnings`: passed.
- `mix test`: 586 passed, 10 excluded (including hardware tests).
- Goat: 47 targeted camera/renderer/lifecycle/target/build-selection tests passed with
  temporary source compilation against cached dependencies.

Goat retains its default Hex dependency. `GOAT_EMERGE_PATH=/workspace/emerge` explicitly
selects this source checkout and forces its native source build for pre-release validation;
no new Hex release or lockfile upgrade has been published.

Hardware acceptance still required: repeated unplug/replug with static UI and live camera,
long absence, quick reconnect with a pending flip, shutdown while unplugged, and unsupported
replacement mode. Verify current scene redraws, preview resumes, endpoint/PIDs stay stable,
producer holders/FDs remain bounded, and robot control is unaffected.
