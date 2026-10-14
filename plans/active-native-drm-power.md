# DRM power qualification

## Remaining device acceptance

The user reported CPU raster working on FP3. Explicit OpenGL is restored for the
next user-owned firmware build/test; corrected startup/device operation is still
unverified. Rebuild/reboot the already-stuck presenter rather than extending
cleanup timeouts or releasing a lease still in use.

- Verify delayed first tree and initially-off startup stay dark, with the first
  application frame presented before managed illumination. Use visual/optical
  observation as well as commit/write traces; logs alone cannot prove no flashes.
- Exercise repeated/rapid off/on, brightness zero and changes while off, application
  state changes, recovery, unplug/reconnect and independent sibling outputs.
- Check an animating/video scene during extended off periods: bounded frame
  retirement, responsive config, no routine layout/render/presentation work and
  current-state rendering on wake. Measure CPU/wakeups, off-to-on latency and
  battery/current against active/static baselines; record unavoidable periodic work.
- Verify supported panel DCS/backlight behavior, no placeholder flashes and
  failure-safe leave-dark cleanup. Do not claim physical monitor/device power-off.

The existing stop/recreate and ownership contracts are maintained in
[DRM lifecycle](../guides/internals/drm-outputs-and-lifecycle.md) and
[config reconciliation](../guides/internals/config-reconciliation.md).
Additional backend controls are tracked in
[viewport configuration follow-up](active-declarative-viewport-settings.md).
[Vulkan](active-vulkan-drm-power-brightness.md) and
[raster GPU-upload](active-raster-gpu-upload-drm-power-brightness.md) now reuse the
shared DRM power owner. Authorized integrated-GPU tests passed; the panel/device
acceptance above remains open. This work is independent of the Wayland Vulkan
synchronization fix.

## Fairphone handoff

Follow the consumer README and fixed FP3 OpenGL configuration. Keep app-side
power policy outside Emerge and preserve unrelated app/system/freedreno work.
The user owns deployment/device testing. No assistant device access, key generation
or authorization changes; use the user's existing target build setup.
