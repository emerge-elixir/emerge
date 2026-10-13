# Plans

Keep open implementation/qualification work and research that supports it here.
Completed feature designs, validation logs, and superseded release checklists
belong in Git history, tests, `CHANGELOG.md`, and the maintained guides—not in an
archive subdirectory or a permanently active plan.

## Open work

- [Multiline word wrapping](active-multiline-word-wrap.md): scanner fix and
  shared-consumer regressions validated; FP3 hardware acceptance pending.
- [Embedded artifact follow-up](active-embedded-artifact-qualification.md):
  musl/RISC-V package and target checks; Trellis acceptance is complete.
- [Headless grayscale output](active-headless-grayscale-output.md): Gray4/Gray8
  expansion of the accepted BW1/Gray2 pipeline.
- [Linux platform follow-up](active-linux-gpu-qualification.md): specific
  device-loss/presenter-panic checks, Nexus 7 CPU-DRM cadence, and Vulkan-only
  artifact dependency closure—not repeat GPU/PRIME, reconnect, or FP3 testing.
- [Low-resource animation smoothness](active-low-resource-animation-smoothness.md):
  constrained-device profiling, registry geometry work, safe patch coalescing,
  and final cadence acceptance.
- [macOS-specific follow-up](active-macos-runtime-qualification.md): independent
  asset/font sessions, gradient coverage, and packaged raster-NIF checks;
  Metal/raster playback acceptance is complete.
- [RPi5 Camera 60 FPS](active-rpi5-camera-60fps.md): production NV12 correctness,
  ownership, and sustained rendering/headroom targets.
- [Universal gradient qualification](active-universal-gradient-colors.md):
  constrained-device gradient performance and the outstanding screenshot-cache
  benchmark issue.

- [Declarative viewport settings](active-declarative-viewport-settings.md):
  breaking 0.5.0 typed config/input APIs, lifecycle replay, and remaining native
  controls. Schema/migration reference: [viewport settings](viewport-settings-schema.md).
  New power-cycle acceptance: [native DRM lifecycle](active-native-drm-power.md)
  and [Fairphone display controls](active-fairphone-display-controls.md); this does
  not reopen the already-closed general FP3 renderer qualification.

- [Vulkan acquire synchronization](active-vulkan-acquire-synchronization.md):
  deferred acquired-image layout-transition fix discovered in Wayland frame-cap
  validation; separate from config implementation.

## Research and roadmaps

- [Screenshot cache investigation](screenshot-cache-benchmark-investigation.md):
  unresolved coverage accounting and cached/direct text/shadow pixel differences.
- [Low-memory grayscale research](headless-low-memory-grayscale-investigation.md):
  pinned-Skia evidence and memory constraints for the remaining output work.
- [Layout caching roadmap](layout-caching-roadmap.md) and
  [cross-engine insights](layout-caching-engine-insights.md): retained-layout
  direction beyond the completed foundation.
- [Platform runtime differences](platform-runtime-architecture-differences.md):
  Linux actor versus macOS host orchestration.
- [Skia recording/DDL](skia-ddl-paint-layer-note.md): possible future paint-layer
  recording work.

## Completed work: authoritative references

Maintainer-confirmed testing closes Trellis operation/pictures, Linux GPU/PRIME,
Vulkan unplug/replug, FP3, and macOS Metal/raster playback. Do not reopen those
qualification reminders just because an old plan lacks the test log.

- [Events and controlled-input reconciliation](../guides/internals/events.md)
- [Tree wire format](../guides/internals/emrg-format.md)
- [DRM output selection and viewport lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)
- [Native builds, artifacts, and release gates](../guides/reference/native-renderer-builds.md)
- [Video submission and ownership](../guides/internals/video-interop-architecture.md)
- [Renderer-local asset ownership](../guides/internals/assets-images.md)
- [Headless frame-copy measurements](../bench/headless_frame_copy.md)
- [Corrected borders-cache benchmark](../bench/README.md#rich-borders-cache-workload)
- [Release history](../CHANGELOG.md) and [0.4 migration](../guides/migrations/0.4.md)

## Maintenance

- Use `active-*.md` only for concrete unfinished work or acceptance gates.
- Remove completed implementation logs; preserve evidence in Git history.
- Consolidate overlapping follow-up gates instead of retaining whole completed
  designs for a pending hardware smoke test.
- Keep measurements only when they inform remaining work.
- Prefer maintained guides and authoritative cross-repository plans over copies.
- Update this index and repair links whenever a plan is removed.
