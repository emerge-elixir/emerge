# Linux platform follow-up

## Remaining specific checks

- Exercise GPU device-loss and presenter-panic handling with a sibling viewport
  active. Verify safe cleanup/quarantine, not isolation from a shared GPU fault.
  Failed ownership barriers remain terminal; never clear quarantine to pass.
- Measure CPU-only DRM cadence on Nexus 7/ARMv7 and exercise fault-injected
  teardown. Trellis acceptance does not establish this device's performance.
- Check Vulkan-only Wayland artifacts for accidental EGL/OpenGL dependencies;
  remove compatibility linkage if still present. Packaging and runtime acceptance
  are distinct.

No additional general GPU/PRIME, Vulkan reconnect or FP3 soak checklist remains.
Production NV12/Camera work belongs to [RPi5 Camera](active-rpi5-camera-60fps.md).
Remaining Wayland interactive synchronization/performance checks belong to
[the synchronization follow-up](active-vulkan-acquire-synchronization.md).

## References

- [DRM ownership and lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)
- [Build profiles and artifact gates](../guides/reference/native-renderer-builds.md)

Hardware checks require explicit authorization; never modeset an in-use desktop
GPU during routine validation.
