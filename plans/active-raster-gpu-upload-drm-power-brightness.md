# Raster GPU-upload DRM power and brightness

Status: implemented; final validation recorded below. Same worktree/branch.

## Scope

Enable existing `output.display` and `output.backlight.brightness` with explicit
`renderer: [api: :raster, raster_present: :gpu_upload]`. Reuse the shared DRM owner
and OpenGL/GBM presenter: CPU rasterization plus GPU upload already uses that
presenter's fresh application-frame, suppression and teardown paths. No second
power policy or GPU-state restoration.

Automatic API selection, live device/connector/mode changes, DRM frame caps,
Wayland power and physical suspend remain excluded. CPU-only and Vulkan-only
builds must still reject GPU-upload presentation before hardware initialization.

## Work

- [x] Audit the shared GL path for initially-off capability startup, cancellation,
  first application presentation, CPU surface/upload retirement and quarantine.
- [x] Admit explicit raster GPU-upload consistently in Elixir, native startup and
  live wake validation; retain compiled OpenGL presenter checks.
- [x] Add config, callback and native admission regressions without changing CPU,
  OpenGL or Vulkan lifecycle policy.
- [x] Reuse hardware power test scenarios for GPU-upload: initial off, delayed
  upload, retained latest content, rapid/repeated off/on, stable owner, captures,
  bounded descriptors, Stop without upload and clean reacquisition.
- [x] Update configuration/lifecycle docs and the viewport umbrella plan.
- [x] Run Cargo tests/strict Clippy for `drm`, `drm-all`, `drm-raster`, `drm-vulkan`,
  source-profile Mix tests and `EMERGE_SKIA_BUILD=1 ./ci-tests.sh`.

## Implementation notes

The GPU-upload route already uses `gl::run` with `RenderingApi::Raster`, a CPU
`RasterBackend`, and a GL upload surface. It already duplicates the parent's
lease, returns capability readiness before upload, uses the fresh application
frame request, and signals presentation only after the initial blocking modeset.
Retirement acknowledges suppression before disabling KMS, completes pending GL
work, and destroys CPU/upload/GBM/EGL owners. Uncertain retirement parks the live
owners and quarantines the output. No GL teardown changes were necessary.

Admission now depends on the explicit drawing API, not raster presentation kind.
Existing presenter feature checks still run before hardware initialization;
Vulkan-only and CPU-only tests require the OpenGL-unavailable error for GPU
upload, not CPU/Vulkan fallback. Raster remains the selected drawing API and
PRIME video remains unsupported for this route. Automatic API power, immutable
settings and joined/revision-fenced completions are unchanged.

The opt-in scenarios are now shared in
`test/emerge_skia/drm_gpu_power_hardware_test.exs`. Choose
`EMERGE_DRM_TEST_POWER_RENDERER=gpu_upload` or `vulkan` (default). GPU upload also
requires a hardware GL report, excluding llvmpipe/softpipe.

## Authorized target

The user authorized display testing on the integrated GPU. Use only
`/dev/dri/by-path/pci-0000:6c:00.0-card`, connector `DP-4` (1920×1080), on the idle
Radeon 610M; keep the desktop on `card1` untouched. There is no sysfs backlight,
so physical brightness/DCS/optical qualification remains open. No permissions,
package, deployment, or fault-injection changes are authorized by this test.

## Recorded validation

- Release Cargo tests and strict all-target Clippy passed for `drm`, `drm-all`,
  `drm-raster` and `drm-vulkan`.
- OpenGL-only and combined OpenGL/Vulkan source profiles: 678 Mix tests passed
  each. GPU-upload hardware tests: 3 passed each on hardware AMD/radeonsi,
  OpenGL ES 3.2, Mesa `26.2.4-arch1.1`, kernel `7.2.7-1-cachyos`.
- Both hardware runs exercised initial off, delayed upload, three full off/on
  cycles, twenty rapid pairs, latest-content CPU-raster captures, extended off
  with no frames, Stop without upload and clean reacquisition. Off-state FDs
  stayed 26→26; `DP-4` finished disabled. Hardware commands targeted only `card0`;
  desktop `card1` was enabled at the earlier post-test check. A much later check
  after software validation found desktop outputs disabled; no commands were
  issued to those outputs and no desktop state restoration was attempted.
- Full native output: [OpenGL-only](validation/raster-gpu-upload-drm-power-brightness/opengl-only.txt),
  [combined](validation/raster-gpu-upload-drm-power-brightness/combined.txt).
  Snapshot identity: `worktree-sha256-b36846be91cbd89ccfa9233668288035e3644a644e2851a94799fb9eb5d99847`.
  Existing implicit-GBM/DRM synchronization warnings remain expected. The combined
  run also recorded one render-channel-close message during Stop before upload;
  cleanup/reacquisition succeeded, with no quarantine or GL retirement failure.
- [Vulkan regression](validation/raster-gpu-upload-drm-power-brightness/vulkan-regression.txt):
  3 passed, ten complete synchronization-validation lifetimes, zero warnings/errors.
- CPU-only profile tests were corrected to respect intentionally missing PRIME
  dispatcher/storage support. This changes fixtures, not CPU runtime behavior:
  skip the dispatcher-only case, verify caller ownership on rejected DMA-BUFs,
  and explicitly release that caller's lease.

- Final Vulkan-only Mix suite: 679 passed. CPU-only DRM: 678 passed, one
  dispatcher-only test skipped. Both passed with `--warnings-as-errors`.
- Default source build restored; `EMERGE_SKIA_BUILD=1 ./ci-tests.sh` passed,
  including formatting, Credo, Rust checks, full Mix suite and Dialyzer (zero errors).
  Authoritative software logs use `/tmp/emerge-raster-gpu-upload-power-` prefixes:
  `mix-{vulkan,raster}-verified`, `ci-verified`, and
  `{drm,drm-all,drm-raster,drm-vulkan}-{test,clippy}` (`.exit` files all zero).
  Earlier CPU-only fixture failures and type-warning runs are superseded.

Physical brightness/DCS/optical and destructive-fault qualification remain open.
[Vulkan implementation/evidence](active-vulkan-drm-power-brightness.md) remains
separate; its safe retirement and rapid-startup-cancellation fix are preserved.
