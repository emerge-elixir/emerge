# Linux platform follow-up

Status: Linux GPU/PRIME, Vulkan unplug/replug, and FP3 testing confirmed by the
maintainer. Those qualification reminders are closed; do not repeat them as
release gates merely because the earlier logs lacked the evidence.

## Remaining specific checks

- Exercise GPU device-loss and presenter-panic handling with a sibling viewport
  active. Verify safe cleanup/quarantine, not isolation from a shared GPU fault.
  Failed ownership barriers remain terminal; never clear quarantine to pass.
- Measure CPU-only DRM cadence on Nexus 7/ARMv7 and exercise fault-injected
  teardown. Trellis acceptance does not establish this device's performance.
- Check Vulkan-only Wayland artifacts for accidental EGL/OpenGL dependencies;
  remove the compatibility linkage if it is still present. Packaging checks and
  runtime acceptance are distinct.

No additional PRIME matrix, Vulkan reconnect, or FP3 soak checklist remains.
Production NV12 and Camera performance work belongs to
[RPi5 Camera 60 FPS](active-rpi5-camera-60fps.md), not this plan.

## References

- [DRM ownership and lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)
- [Build profiles and artifact gates](../guides/reference/native-renderer-builds.md)

Hardware checks require explicit authorization; never modeset an in-use desktop
GPU during routine validation.

## Wayland frame-cap run: 2026-10-10 — synchronization validation failure

Tested commit `853d951f73b49b491829c229d53ae8165e3c8306` on the available
Hyprland compositor (`wayland-1`; compositor version unavailable in the container).
The source-built NIF used `compiled_backends: [wayland: [:opengl, :vulkan]]`.
`renderer_info/1` confirmed explicit Vulkan, `software: false`, and
**AMD Radeon RX 7900 XTX (RADV NAVI31)**, Mesa **26.2.4-arch1.1**.

- The opt-in `wayland_frame_rate_test.exs` passed without validation and in four
  validation-enabled runs: capped bursts, live uncap, exact captured colors,
  config-only idleness, refresh statistics and prompt stop.
- **This is not a qualification pass.** Khronos validation **1.4.363**, with core
  and synchronization checks enabled, repeatedly reported
  `SYNC-HAZARD-WRITE-AFTER-READ` after `vkAcquireNextImageKHR`.
- The run using `EMERGE_VULKAN_VALIDATION=1` ended with **4 errors, 0 warnings**.
  Earlier forced-layer runs reported 3/4/4 errors plus loader notices about
  `VK_INSTANCE_LAYERS`. The ExUnit exit status alone does not catch these errors.
- The reported image transition has `srcStageMask = TOP_OF_PIPE` and
  `dstStageMask = COLOR_ATTACHMENT_OUTPUT`; its execution dependency does not
  cover the acquisition semaphore wait. Investigate the handoff from
  `backend/wayland/vulkan.rs::acquire_next_image` to the Ganesh `Surface::wait`
  path in `backend/vulkan/frame.rs`. Preserve nonblocking acquisition and safe
  semaphore ownership when fixing it; do not suppress validation or add a
  per-frame device-idle workaround.
- This run exercised the compositor-selected discrete GPU, not both GPUs. It does
  not reopen the maintainer-confirmed general GPU/PRIME or reconnect qualification.

Logs and the temporary validation-layer installation are under
`/tmp/emerge-vulkan-qualification/` in the qualification environment; in particular
`renderer-info.log`, `smoke.log`, and `validation-5.log`. Validation settings used
`khronos_validation.validate_sync = true` in `vk_layer_settings.txt`, selected by
`VK_LAYER_SETTINGS_PATH`. Temporary project configuration was removed and the
default test NIF rebuilt afterward. No system packages were installed.

