# Emerge 0.4.0 Release Audit

## Executive summary

The candidate is a major backend/video release, not a small increment over 0.3.4. It adds headless rendering, Vulkan, canonical VideoInterop ownership, raster presentation, improved diagnostics, and substantial renderer/cache work.

It is **not ready to tag yet**. The main blockers are:

1. Unpublished `video_interop` dependencies and active sibling-path overrides.
2. Missing `video_interop` entries in both Mix and Cargo registry locks.
3. The release branch diverged before 0.3.3/0.3.4 and needs reconciliation with `main`.
4. A large dirty working tree contains unfinished grayscale/dithering work.
5. Committed packed grayscale output is incorrect for non-byte-aligned multi-row images.
6. Vulkan and hardware matrices remain incomplete.
7. Local commits in Emerge and VideoInterop are not pushed.

## Commit scope

`git log v0.3.4..HEAD` reports **40 commits**, but this needs qualification:

- `headless-backend` forked at `v0.3.2`; it is not descended from `v0.3.4`.
- Eight commits are patch-equivalent to changes already included by 0.3.4.
- There are **32 genuinely new commits**.
- Tree comparison: **137 files, +45,301 / -9,843 lines**.

The branch also does not contain the ancestry of four 0.3.3 fixes plus the 0.3.3/0.3.4 release-preparation commits. Their behavior appears to have been carried into later large commits, but this should be resolved by rebasing/merging and rerunning regressions.

### New commit groups

| Area | Commits |
|---|---|
| Renderer selection, capabilities, stats, screenshots, raster presentation | `9c63668`, `191e05c`, `dd95693`, `33440f1`, `97c896f`, `a867f6a`, `9959e08`, `b614c5c`, `ac0c7e0`, `cea0cf4` |
| Canonical headless PRIME and VideoInterop | `c085596` |
| Vulkan, semantic layers, renderer restructuring | `f08d80b` |
| XRGB/NV12/lifecycle/allocation follow-ups | `c68a383`, `ca5f81b`, `4ff095a`, `b06c0c7`, `22e7f8e`, `a75f1a3` |
| Nerves, touch scrolling, centered text fixes | `e6bb0c8`, `dfb067a`, `fe4ecac` |
| Documentation/planning/qualification records | Remaining 11 commits |

## What 0.4.0 brings

### 1. Unified backend and rendering API selection

Rendering is now selected independently:

- Backends: Wayland, DRM, macOS, headless
- APIs: OpenGL, raster, Metal, Vulkan
- `backend_renderer` and `:gl` remain deprecated compatibility aliases.
- Explicit Vulkan is fail-closed: it does not silently fall back to OpenGL, raster, or software Vulkan.
- `compiled_vulkan_backends` provides compile-time Vulkan feature selection.

New combinations include:

- Wayland raster CPU or GPU-upload presentation
- DRM raster GPU upload
- Headless raster/OpenGL binary frames
- Headless OpenGL/Vulkan PRIME output
- Explicit Wayland and DRM Vulkan rendering

`rendering_api: :auto` remains OpenGL-first on Linux.

### 2. Headless rendering

The release adds retained headless sessions with:

- Frame messages and configurable tags/FPS
- `rgba8888`, `rgb888`, `gray8`, `gray4`, `gray2`, and `bw1` declarations
- PRIME/DMA-BUF output with bounded in-flight frames and backpressure
- Renderer screenshots for binary headless output
- Direct viewport-to-video-target connections through:
  - `Emerge.connect_video_output/3`
  - `Emerge.disconnect_video_output/1`

### 3. Canonical VideoInterop

The old PRIME lease shape is replaced by the shared `VideoInterop` protocol:

- Canonical frame, format, colorimetry and DMA-BUF structures
- Per-holder abandonment guards
- Prepare/claim ownership boundary
- Explicit sync-file acquire fences
- Exact retirement and drained shutdown
- Safe lease fan-out
- Lifecycle-owned release dispatchers
- Strict allocation-size and plane-span validation

This is a **breaking interoperability protocol change**. Producers, consumers, adapters, NIFs and applications must be upgraded together and cold-restarted or fully drained.

### 4. Vulkan and camera/video paths

New Vulkan functionality includes:

- Wayland WSI rendering
- Headless Vulkan PRIME production
- DRM no-WSI/KMS presentation
- Exact DRM-device matching
- Vulkan validation diagnostics and `vulkan_probe`
- Persistent DMA-BUF imports and bounded pools
- Direct or staged XRGB8888 support
- NV12 direct, optimal multi-planar, separate Y/UV and planar rollback paths
- Exact BT.709 range/chroma metadata
- Complete fd-backed allocation-size publication
- Queue authority, sync-lane identity and device-loss quarantine

### 5. Renderer and diagnostics

- New `EmergeSkia.renderer_info/1`
- Renderer-aware stats; schema version 23
- DRM metric renamed to `gpu_render_elapsed`
- GPU timing correlated with page flips and atomic commits
- New Vulkan synchronization, saturation, quarantine and device-loss counters
- On-demand screenshot capture replaces unconditional per-frame GPU readback
- Deterministic semantic paint-layer topology and improved retained caching

### 6. Correctness and platform fixes

- Improved high-frequency touch scrolling and inertial fling behavior
- Fixed centered text after content patches
- Fixed Wayland video-only redraw starvation
- Improved text visual-bound cache sizing
- Better native log-level handling
- Nerves Skia cross-compilation fixes for newer toolchains
- macOS runtime convergence retained without adding Vulkan/video-target support

## Dependency changes since 0.3.4

### New direct dependencies

| Ecosystem | Dependency | Purpose |
|---|---|---|
| Hex | `video_interop ~> 0.1.0` | Canonical Elixir frame, format, lease and consumer contracts |
| crates.io | `video-interop = 0.1.0` | Rust schemas, FD ownership, EGL/Vulkan integration |
| crates.io | `ash 0.38` | Vulkan API |
| crates.io | `ash-window 0.13` | Wayland/window-surface Vulkan integration |

### Changed dependency sources/features

- `skia-safe` and `skia-bindings` remain version 0.99.0 but move from crates.io to pinned rust-skia commit:
  `0d2261c63941f4b534522246cc1ace13ca4242d8`
- `gl` and `glutin_egl_sys` are now optional.
- GBM `import-egl` is enabled through the OpenGL feature bundle instead of unconditionally.
- New Cargo feature families:
  - `wayland-core`, `wayland-vulkan`, `wayland-all`
  - `drm-core`, `drm-vulkan`, `drm-all`
  - `headless-opengl`, `headless-vulkan`, `headless-all`
  - `vulkan`, `vulkan-probe`

### New transitive lock entries

Mostly from `ash-window`/macOS raw-window support:

`bitflags 1.3`, `block`, `cocoa`, `cocoa-foundation`, `core-foundation`, `core-foundation-sys`, `core-graphics`, `core-graphics-types`, `foreign-types`, `foreign-types-macros`, `foreign-types-shared`, `malloc_buf`, `objc`, `raw-window-metal`, and `syn 3`.

The new `video-interop` crate requires **Rust 1.91**, which should be documented or declared by Emerge for source-build users.

## Current release blockers

### 1. Registry dependencies are not published

Registry checks currently show:

- crates.io `video-interop`: not found
- Hex `video_interop`: not found
- Hex `membrane_video_interop`: not found
- Hex `emerge`: latest remains 0.3.4

Consequences:

- `mix deps.get` currently fails without `VIDEO_INTEROP_PATH`.
- `mix.lock` has no `video_interop` entry.
- Cargo resolves `video-interop` through:

```toml
[patch.crates-io]
video-interop = { path = "../../../video_interop/rust/video-interop" }
```

That path will not exist in GitHub release builders or downstream Hex source builds.

Before release:

1. Publish the crate.
2. Publish the Hex core package.
3. Remove the Cargo patch.
4. Regenerate and commit both lock files from registry sources.

### 2. Git state is not releasable

- Emerge `headless-backend` is two commits ahead of its remote.
- VideoInterop `main` is one commit ahead of its remote.
- Emerge has extensive uncommitted source and plan changes.
- No `v0.4.0` tag exists.
- The branch must be reconciled with `v0.3.4`/`main`.

### 3. Packed grayscale correctness

Committed `pack_gray/3` packs the flattened pixel stream rather than restarting each scanline. For example, BW1 width 3 × height 2 produces one byte while metadata declares a one-byte stride and therefore requires two bytes.

Gray2 and Gray4 have similar row-boundary/padding problems.

The dirty worktree contains a partial BW1 replacement and dithering implementation, but it is unfinished and uncommitted. Before 0.4.0 either:

- finish and fully validate all advertised grayscale formats, or
- remove/defer unsupported low-bit formats from the public 0.4.0 contract.

### 4. Release documentation/hygiene

- Update the 0.4.0 changelog date.
- Remove the duplicated column-fill fix from 0.4.0; that was already the 0.3.4 change.
- Document Vulkan source-build requirements and absence of Vulkan precompiled NIFs.
- Update the setup tutorial, which still omits Vulkan/headless details.
- Fix `.gitignore`: the Hex archive is `emerge-*.tar`, not `emerge_skia-*.tar`.

## What still needs validation

### Automated host/package validation

Run from clean registry-only worktrees:

- Full `./ci-tests.sh all`
- `mix test --include full_sweep`
- `mix docs`
- Hex package build/unpack and compile from the unpacked archive
- Cargo format/test/clippy for:
  - no features
  - Wayland/OpenGL/Vulkan/all
  - DRM/OpenGL/Vulkan/all
  - headless OpenGL/Vulkan/all
  - macOS
- Build and run `vulkan_probe`
- AArch64 Nerves source build using registry dependencies
- All six Linux precompiled NIF profiles
- Both macOS host archives and checksums
- NIF loading from generated release assets

Current CI exercises primarily default features and excludes hardware tests; it does not provide the complete Vulkan profile matrix.

### Linux GPU/hardware qualification

Still open:

- Four Wayland PRIME routes:
  - GL → GL
  - Vulkan → GL
  - GL → Vulkan
  - Vulkan → Vulkan
- Five minutes / 9,000 frames per route
- Exact pixels, resize, hide/show, reconnect and second renderer lifetime
- Stable FD/RSS/cache/lease counts
- Fence/export/rejection/consumer-disappearance fault handling
- Explicit and forced-implicit OpenGL synchronization
- Wayland Vulkan resize, screenshot, validation, device-loss and multi-GPU matching
- RPi5 DRM/Vulkan functional probe, KMS restore and repeated restart/fault cycles
- DRM OpenGL/GLES2 rollback smoke

### Camera/RPi5 qualification

Still open:

- Exact NV12 Rec.709/range/chroma pixel oracles
- Validation-layer and V3D/MMU-clean runs
- Delayed/error fence and device-loss injection
- 300+ captures and 30-minute FD/RSS/cache/lease soak
- NV12 versus XRGB A/B/A decision
- Focus-active target:
  - 59.8–60.2 FPS
  - median GPU ≤10.86 ms
  - p95 ≤11.67 ms
  - p99 <16.67 ms

### Low-resource animation

Constrained-device cadence remains below target:

- Patch actor approximately 17.4 ms versus ≤12 ms target
- Re-baseline combined traversal
- Add missing sidepane exit/prune and interrupted-handoff tests
- Validate transform-only registry updates and event hit geometry

### Grayscale, if included

The current dirty BW1 work requires exact row packing, alpha-over-white, deterministic dithering, glyph protection, screenshots, restart/memory testing, and direct 400×300 EInk hardware acceptance.

## Required publication order

1. **crates.io:** `video-interop` 0.1.0
2. **Hex:** `video_interop` 0.1.0
3. **Hex:** `membrane_video_interop` 0.1.0
4. Remove path overrides and regenerate downstream locks
5. Tag `v0.4.0`; publish Emerge GitHub assets:
   - six Linux NIF archives
   - two macOS host archives plus checksums
6. **Hex:** `emerge` 0.4.0
7. Publish migrated producer packages:
   - `membrane_video_transcode`
   - `membrane_libcamera`
8. Update Demo/Camera locks and build the versioned Nerves system/firmware

Not separately published:

- `emerge_skia` is an internal NIF crate, not a crates.io release.
- `macos_host` and NIFs are GitHub release assets.
- `vulkan_probe` is a diagnostic/system artifact.
- Do not publish the legacy `membrane_dmabuf` protocol.

## Audit note

No tests were run during this audit. Findings are based on Git history, manifests, current source, release workflows, plans, and registry checks.
