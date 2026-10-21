# Plans

Keep only concrete unfinished implementation, acceptance gates and research that
supports them. Completed designs, validation logs and checklists belong in Git
history, tests, `CHANGELOG.md` and maintained guides—not an archive directory.

## Open work

- [Native build rollout](active-build-capabilities.md): remote cross/macOS artifact
  qualification, next-release archives/checksums and opted-in board acceptance.
- [0.5 publication](active-0.5-release.md): exact-integrated-commit CI, artifacts,
  date confirmation and publication.
- [Viewport configuration](active-declarative-viewport-settings.md): remaining
  backend controls, macOS config revisions, input transport coverage and demo migration.
- [DRM power](active-native-drm-power.md): user-owned FP3 OpenGL/no-flash/power
  acceptance; Fairphone deployment handoff.
- [Bundled DRM input](active-drm-libinput.md): full FP3 firmware, physical
  overlapping-key input and minimal-system/daemonless hotplug acceptance.
- [Multiline word wrapping](active-multiline-word-wrap.md): FP3 Notes acceptance.
- [Frame production](active-frame-production-pipeline.md): macOS worker/native
  opportunities and target-specific cadence/resource measurements.
- [Linux platform](active-linux-gpu-qualification.md): device-loss/presenter-panic
  cleanup, Nexus 7 CPU-DRM cadence/faults and Vulkan-only dependency closure.
- [Wayland Vulkan](active-vulkan-acquire-synchronization.md): interactive
  resize/occlusion/recreation, other drivers and submit-cost/idle measurements.
- [macOS runtime](active-macos-runtime-qualification.md): independent asset/font
  sessions, gradient coverage and packaged Intel/Apple Silicon raster NIFs.
- [Embedded artifacts](active-embedded-artifact-qualification.md): musl/RISC-V
  consumer packages and physical targets.
- [Headless grayscale](active-headless-grayscale-output.md): Gray4/Gray8 output,
  asset/layer memory follow-up and binary-output backlog controls.
- [Low-resource animations](active-low-resource-animation-smoothness.md):
  constrained-device profiling, registry geometry, safe coalescing and cadence.
- [RPi5 Camera](active-rpi5-camera-60fps.md): production NV12 correctness/ownership
  and sustained active-scene 60 FPS/GPU headroom.
- [Gradients](active-universal-gradient-colors.md): constrained-device performance
  and macOS coverage.

## Research and roadmaps

- [Screenshot cache benchmark](screenshot-cache-benchmark-investigation.md):
  expected direct-shadow coverage and separate cached/direct pixel differences.
- [Layout caching](layout-caching-roadmap.md): broader dependency boundaries,
  cheaper keys/traversal, compatible measurements and viewport/repeater reuse.
- [Parallel paint recording](skia-ddl-paint-layer-note.md): profiling-led
  off-thread recording/DDL experiments.

## Closed qualification and maintained references

Maintainer-confirmed Trellis operation/pictures, general Linux GPU/PRIME, Vulkan
unplug/replug, FP3 renderer and macOS Metal/raster playback acceptance are closed.
Do not recreate those gates from missing historical logs. New input/power/pacing
checks above are specific follow-ups, not repeat general qualification.

- [Events/input](../guides/internals/events.md)
- [Configuration](../lib/emerge/config.ex)
- [DRM lifecycle](../guides/internals/drm-outputs-and-lifecycle.md)
- [Runtime architecture](../guides/internals/architecture.md)
- [Frame production/synchronization](../guides/internals/frame-pacing.md)
- [Native builds/release gates](../guides/reference/native-renderer-builds.md)
- [Video ownership](../guides/internals/video-interop-architecture.md)
- [Assets](../guides/internals/assets-images.md)
- [Headless frame copies](../bench/headless_frame_copy.md)
- [Borders benchmark](../bench/README.md#rich-borders-cache-workload)
- [Release history](../CHANGELOG.md) and [0.5 migration](../guides/migrations/0.5.md)

## Maintenance

- Use `active-*.md` only for unfinished work or acceptance.
- Remove completed sections; retain only measurements informing the next decision.
- Consolidate overlapping gates and link maintained contracts instead of copying them.
- Update this index and repair links when removing a plan.
- Run `cargo test` and `mix test` after changes; prefer `./ci-tests.sh` for local CI.
- Device access/deployment and performance runs require their explicit opt-in/lock;
  ordinary checks must not operate host devices.
