# Active Plan: Linux GPU and PRIME Qualification

Created: 2026-08-29
Status: implementation complete; remaining work is hardware qualification, packaging, and fixes exposed by those gates

## Purpose

Consolidate the remaining validation from the completed backend-unification,
headless PRIME, explicit-sync, allocation-size, DRM GLES2, and Vulkan
implementation plans. Those implementations now live in code and architecture
docs; do not reopen their design phases here.

## Implemented baseline

- Independent backend and `rendering_api` selection.
- Wayland OpenGL/raster/Vulkan presentation.
- DRM GLES2 baseline, raster presentation, and no-WSI Vulkan/KMS presentation.
- Headless raster/OpenGL binary output and OpenGL/Vulkan PRIME output.
- Canonical `%VideoInterop.Frame{}` transport, bounded leases/backpressure, and
  render-thread retirement.
- OpenGL native-fence export with safe `glFinish` fallback.
- Vulkan sync-file import/export and external queue-family ownership.
- Truthful DMA-BUF allocation sizes from the fd-backed allocation.
- Shared ABGR8888 and NV12 Vulkan import paths.
- Candidate four-route PRIME smoke on RADV with exact solid pixels, hide/show,
  reconnect, restart, and bounded resources.
- Pinned RPi5 DRM/Vulkan UI smoke with exact page flips and bounded shutdown.

## Remaining gates

### 1. Full Wayland PRIME matrix

Run each route in a compositor session that keeps the validation window active:

1. OpenGL producer -> OpenGL consumer
2. Vulkan producer -> OpenGL consumer
3. OpenGL producer -> Vulkan consumer
4. Vulkan producer -> Vulkan consumer

For every route require:

- five minutes / 9,000 source frames at the configured 30 FPS;
- byte-exact submitted and displayed pixels;
- live resize, hide/show, reconnect, and a second renderer lifetime;
- bounded FD, RSS, export slots, import cache, and leases;
- delayed/rejected fence, export failure, and consumer disappearance handling;
- clean stop with no stranded holders or uncertain resource reuse.

Repeat Vulkan routes with synchronization validation enabled where available.
The earlier full control run is not evidence because the compositor throttled an
unfocused surface (`866/9000` displayed frames).

### 2. OpenGL PRIME explicit-sync qualification

On hardware advertising native fence support require:

- `SyncFile` publication and consumer server waits;
- zero producer `GPU finish fallback` samples in the normal run;
- 150+ exports/imports/releases with bounded FDs;
- fence lifetime through retained fan-out and closure after final release;
- no tearing, partial frame, corruption, or orientation regression.

Repeat with forced implicit synchronization. It must remain correct and use the
conservative producer wait. Re-run the hardware-gated headless PRIME ExUnit test
and top-left orientation fixture.

### 3. Vulkan producer metadata and lifecycle

The direct producer probe already proves declared fd size equals the live
allocation for OpenGL and Vulkan. Complete end-to-end consumer proof:

- both Vulkan-producer matrix routes accept page-rounded allocation tails;
- visible plane span remains separately bounded;
- exact validation is not weakened;
- subsequent frame, release/reuse, and shutdown remain correct.

### 4. Wayland Vulkan lifecycle

Qualify explicit Wayland Vulkan for:

- resize/out-of-date recreation;
- screenshots;
- delayed acquire/present and injected device loss;
- validation-layer-clean startup, rendering, restart, and teardown;
- exact multi-GPU device matching from compositor feedback.

Keep `:auto` OpenGL-first. Explicit Vulkan fails closed and never chooses a
software ICD, OpenGL, or raster fallback.

### 5. DRM/Vulkan target qualification

On the pinned RPi5/Nerves image:

- run `vulkan_probe --functional --allocation-direction gbm-import --validation`
  with explicit VC5 KMS and V3D Vulkan nodes;
- prove modifier intersection, GBM allocation, Vulkan import/bind, Ganesh draw,
  sync-file handoff, atomic `IN_FENCE_FD`, exact readback, page flip, and KMS
  restoration;
- run repeated resize/restart/fault/teardown cycles with no validation callback,
  V3D/MMU fault, `EBUSY` leak, quarantine, or FD/RSS growth;
- preserve the existing OpenGL DRM rollback and GLES2-only base smoke.

Camera/NV12 correctness and performance are owned by
`active-rpi5-camera-60fps.md`.

### 6. Build and publication cleanup

- Remove accidental EGL/OpenGL linkage from Vulkan-only Wayland artifacts if the
  release feature still pulls that compatibility closure.
- Publish canonical `video-interop` / `video_interop` artifacts in dependency
  order only after registry-clean host gates pass.
- Remove temporary sibling path overrides and rebuild release/Nerves artifacts
  with recorded checksums.
- Keep detailed generic ownership and rollout authority in the
  `../video_interop` plans rather than duplicating it here.

## Acceptance

- All four PRIME routes pass the complete matrix, not only candidate smokes.
- Explicit and implicit OpenGL sync modes are both safe and bounded.
- Wayland and DRM explicit Vulkan pass lifecycle/fault validation.
- Descriptor sizes, plane spans, sync ownership, leases, and slot reuse remain
  exact under failure.
- Vulkan-only builds contain no unnecessary OpenGL/EGL runtime dependency.
- Canonical packages build without sibling paths.

## Validation

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo clippy --manifest-path native/emerge_skia/Cargo.toml --all-targets --features headless-all -- -D warnings
mix format --check-formatted
mix test
./ci-tests.sh all
(cd ../emerge_demo && mix format --check-formatted && mix test)
git diff --check
```

Hardware commands remain in the repository scripts and hardware-gated tests;
do not duplicate alternate one-off harnesses in this plan.
