# Viewport configuration follow-up

## Remaining implementation

- macOS host config revisions and session/revision-tagged completion reporting.
- DRM and macOS startup/live frame caps. Cap animation, video and other redraw
  admissions consistently; preserve idle behavior, responsive input/control and
  backend readiness. Physical refresh remains separate from the render cap.
- Safe DRM connector/mode changes on the selected card: exact-selection
  preflight/reservation, size-dependent resource/layout/input updates, a correctly
  sized first frame and stale-completion rejection. Preserve the old working
  output on preflight failure; do not promise rollback after destructive teardown
  or silently move onto another viewport's output.
- [Vulkan DRM power/brightness](active-vulkan-drm-power-brightness.md) and
  [raster GPU-upload power](active-raster-gpu-upload-drm-power-brightness.md) reuse
  the implemented CPU raster/OpenGL power owner. Integrated-GPU evidence is
  recorded; panel/video qualification remains separate. Preserve stop/recreate
  and GPU/KMS ownership, and keep automatic API power guarded.
- Complete input transport contract coverage across native and macOS routes:
  actual callback payloads, category filtering and backend availability. Raw
  subscriptions must not disable element interactions or text/scroll behavior.
- Migrate the standalone demo consumer to the 0.5 config/input APIs.

Hardware acceptance for existing power controls belongs to
[native DRM power](active-native-drm-power.md). macOS worker/pacing integration is
tracked in [frame production](active-frame-production-pipeline.md), independently
of config revision reporting. Release/artifact work belongs to
[0.5 publication](active-0.5-release.md).

## Authoritative contracts

- [Viewport configuration](../lib/emerge/config.ex)
- [0.5 migration](../guides/migrations/0.5.md)
- [Input payloads/subscriptions](../guides/tutorials/handle_input.md)
- [Config reconciliation](../guides/internals/config-reconciliation.md)
- [DRM lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)

Do not restore legacy config/mask compatibility, config-to-frame proof machinery
or a competing recovery owner. Preserve latest desired intent, callback PID/state
and independent outputs. Hardware checks require explicit permission.
