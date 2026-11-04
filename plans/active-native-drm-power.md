# DRM power qualification

## Remaining device acceptance

The user reported CPU raster working on FP3. Explicit OpenGL is restored for the
next user-owned firmware build/test; corrected startup/device operation is still
unverified. Rebuild/reboot the already-stuck presenter rather than extending
cleanup timeouts or releasing a lease still in use.

- Verify initially-off startup stays dark until the first application frame.
  For initial-on startup, verify inherited scanout and backlight remain unchanged
  while the first upload is delayed. Use visual observation and commit/write
  traces; logs alone cannot prove no flashes.
- Exercise repeated/rapid off/on, brightness zero and changes while off, application
  state changes, recovery, unplug/reconnect and independent sibling outputs.
- Check an animating/video scene during extended off periods: bounded frame
  retirement, responsive config, no routine layout/render/presentation work and
  current-state rendering on wake. Measure CPU/wakeups, off-to-on latency and
  battery/current against active/static baselines; record unavoidable periodic work.
- Qualify Vulkan and raster GPU-upload on supported panels: initial-off,
  delayed upload, brightness `nil`/zero/0.5/1 and changes while off, DCS ordering,
  no placeholder flashes, stop at each phase and failure-safe leave-dark cleanup.
  Include managed and unmanaged backlights, retained handles, reacquisition and
  sibling presentation. Authorize destructive fault injection separately.
- Include static UI, animation, CPU video and supported PRIME video. Check frame,
  descriptor, GPU and source-lease high-water marks across clean cycles. Do not
  claim physical monitor/device power-off.

The existing stop/recreate and ownership contracts are maintained in
[DRM lifecycle](../guides/internals/drm-outputs-and-lifecycle.md) and
[config reconciliation](../guides/internals/config-reconciliation.md).
Additional backend controls are tracked in
[viewport configuration follow-up](active-declarative-viewport-settings.md).
Vulkan and raster GPU-upload reuse the shared DRM power owner. Authorized
integrated-GPU tests passed, but that target had no managed backlight; the
panel/device acceptance above remains open. See the opt-in test in the DRM
lifecycle guide. This work is independent of Wayland Vulkan synchronization.

## Fairphone handoff

Follow the consumer README and fixed FP3 OpenGL configuration. Keep app-side
power policy outside Emerge and preserve unrelated app/system/freedreno work.
The user owns deployment/device testing. No assistant device access, key generation
or authorization changes; use the user's existing target build setup.
