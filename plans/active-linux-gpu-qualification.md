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
