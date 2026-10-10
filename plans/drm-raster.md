# CPU-only DRM presentation

Motivation: PR #83 reports Tegra 3's available driver cannot compile texture-sampling shaders. Raster rendering must not require EGL/GBM for presentation.

## Implemented

- Accept `config :emerge, compiled_backends: [drm: [:raster]]`; map it to `drm-raster` and source-build this profile until dedicated artifacts exist.
- Remove GBM from `drm-core`; keep it explicitly in GPU feature bundles.
- Route DRM raster `:auto`/`:cpu` presentation through CPU-mapped XRGB8888 dumb buffers; preserve explicit `:gpu_upload` in OpenGL builds. DRM `rendering_api: :auto` selects raster without compiled OpenGL.
- Reuse output leasing, mode selection, input, cursor assets, actor wakeups, screenshots and lifecycle infrastructure. Existing atomic KMS + leasing requirements remain; no legacy-KMS fallback.
- Draw Skia CPU directly into the non-current mapped buffer with DRM pitch. Two buffers, blocking atomic commits, no per-frame BEAM transfer. Software cursor, CPU video, on-demand RGBA screenshots.
- Detach before releasing resources, including on unwinding. Quarantine buffers and lease on uncertain detach. Startup retry waits are wakeable; output disconnection/mode loss stops safely for viewport lifecycle recovery.
- Add mapped-surface/cursor/layout tests, runtime and build matrix coverage, a single-output opt-in hardware test, docs, and CPU-only CI dependency/link checks.
- Upgrade the now-published Elixir video_interop dependency and lock to 0.1.3.

## Validation

- `mix test --warnings-as-errors`: 591 passed, 11 excluded (including hardware tests).
- `cargo test`: 1,502 passed (default features).
- `cargo test --no-default-features --features drm-raster`: 1,476 passed; one hardware test ignored.
- DRM tests with `drm-all`: 54 passed, two hardware tests ignored.
- `./ci-tests.sh quality`: passed.
- Clippy with `drm-raster` and `drm-all`, warnings denied: passed.
- CPU-only release build succeeded. Normal Cargo dependency tree contains no GBM/GL/GLutin/Ash/Wayland packages; `ldd` shows no EGL/GL/GBM/Vulkan/Wayland libraries.
- With explicit user authorization, the single-display hardware test passed on AMD Radeon 610M (Raphael), `/dev/dri/by-path/pci-0000:6c:00.0-card`, DP-4, preferred 1920×1080 @ 60 Hz. The discrete GPU was not targeted.
- Tested a temporary `compiled_backends: [drm: [:raster]]` source build with no GPU-library linkage. Runtime `:auto`, `:raster`, and explicit `present: :cpu` each passed repeated color updates, RGBA captures, stop, and output reacquisition. The test stopped and the original NIF was restored afterward.
- Hardware testing exposed a screenshot-dispatch bug: CPU DRM was using stored snapshots instead of requesting capture. Fixed the shared dispatch and added a Rust regression covering CPU DRM, legacy GPU upload, and headless capture policies. Full Rust/Elixir suites pass after the fix.
- Hardware logs: `/tmp/emerge-drm-hardware.U92mVO/test-final.log`; initial failure and linkage evidence are in the same directory.
- A standalone dashboard with text, rounded color panels and a live progress bar ran on DP-4 for 20.46 seconds. The user confirmed it worked visually. Script, screenshot and log are in `/tmp/emerge-drm-hardware.U92mVO/visual_scene.{exs,png,log}`; the renderer stopped cleanly afterward.
- Nexus 7/ARMv7 performance, hot-unplug, and fault-injected teardown still require device testing.

Device test (after compiling the CPU DRM profile; deliberately selects and modesets a card):

```sh
EMERGE_DRM_TEST_CARD=/dev/dri/card0 \
  mix test test/emerge_skia/drm_raster_hardware_test.exs --include drm_hardware
```
