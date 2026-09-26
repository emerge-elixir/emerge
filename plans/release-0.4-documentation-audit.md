# Emerge 0.4 Documentation Audit

## Status

Complete. The feature audit, release notes, guides, API reference, package
contents, and documentation validation were implemented. This file remains as a
durable audit of the 0.4 documentation scope.

## Scope

This audit covers all 45 commits from `v0.3.4` through `9311412` on
`release/0.4.0-integration`.

Eight commits are patch-equivalent to changes already released in 0.3.3 or
0.3.4 and are not new 0.4 changelog items: `f42e3ef`, `c9a44d9`, `9c3364f`,
`68af5c7`, `86950f2`, `c2adb7b`, `a64e058`, and `7f8f4f0`.

Documentation and planning-only commits were also reviewed: `97147c0`,
`57b1a9f`, `6aece1f`, `b3375fc`, `e036792`, `991a06c`, `15e79e5`, `6e2be28`,
`9f497a2`, `3beb31f`, and `1dd6d28`. Commit `20cfa16` reconciled ancestry rather
than adding another runtime feature.

## Feature audit

The gaps below record the pre-implementation audit. The completed coverage
matrix follows the feature inventory.

### Renderer selection and presentation

Commits: `9c63668`, `191e05c`, `dd95693`, `a867f6a`, `9959e08`, `b614c5c`,
`ac0c7e0`, `cea0cf4`, `f08d80b`, `9311412`.

Added or changed:

- `rendering_api` selects auto, OpenGL, raster, Metal, or Vulkan under backend-
  specific compatibility rules.
- `backend_renderer` and `:gl` remain deprecated aliases; `macos_backend` and
  `dispatch_mode` were removed.
- Wayland and DRM gained raster presentation routes; headless gained retained
  raster and offscreen OpenGL binary output.
- `compiled_backends` maps each backend to `:all` or an exact GPU API list.
  DRM Vulkan requires a `vulkan_drm_node` separate from `drm_card`.
- Wayland, DRM, and headless Vulkan are supported when compiled and available.

Documentation gap: `EmergeSkia.start/1` contains most details, but the setup
guide and README lack a compatibility/fallback matrix, Vulkan build config, and
exact DRM node rules. The README also calls raster a backend rather than a
rendering API used with headless output.

### Renderer information, capture, and diagnostics

Commits: `dd95693`, `33440f1`, `97c896f`, `dfb067a`.

Added or changed:

- `renderer_info/1` reports selected backend/API, capabilities, and Vulkan
  device/DRM-node identity.
- Stats and periodic logs gained backend, pipeline, cache, video, and asset
  detail.
- `render_to_pixels/2` and `render_to_png/2` changed from one-shot tree rendering
  returning a binary to retained-frame capture returning a result tuple.
- Capture gained region, scale, pixel-format, timeout, background, and PNG
  compression options; macOS capture is currently unsupported.

Documentation gap: add before/after capture examples and one diagnostics guide
covering `renderer_info/1`, `stats/2`, `:peek`, `:take`, DRM draining retries,
log routing, and backend capability differences.

### Headless binary and packed grayscale output

Commits: `ac0c7e0`, `cea0cf4`, `088f644`.

Added or changed:

- Retained headless sessions send `%VideoInterop.Frame{}` values directly to a configured process.
- BW1 and Gray2 rows are packed independently, MSB-first, with zero tail bits.
- BW1 polarity is configurable; Gray2 uses levels `0..3` from black to white.
- Raster BW1/Gray2 can use deterministic Atkinson dithering after compositing
  over white. Text, SVG/vector coverage, borders, and crisp generated UI are
  protected from error diffusion.

Documentation result: the primary `EmergeSkia` API reference defines the frame
schema and packed-byte contract. Gray4 was removed rather than shipping its
known odd-width multi-row defect; Emerge Gray8 output remains outside the stable
0.4 output contract.

### Headless PRIME and video transport

Commits: `c085596`, `a75f1a3`, plus the direct-frame submission redesign.

Added or changed:

- Headless binary and PRIME output produce `%VideoInterop.Frame{}` values.
- PRIME frames use bounded in-flight backpressure and published allocation sizes.
- A required target PID receives frames directly. Emerge-to-Emerge delivery uses
  `membrane_video_interop`, not renderer connections or application forwarding.

Documentation result: the internal architecture guide now documents the
Membrane source/sink route, supervision, frame ownership, and shutdown.

### VideoInterop formats, synchronization, and ownership

Commits: `f08d80b`, `c68a383`, `ca5f81b`, `4ff095a`, `b06c0c7`, `22e7f8e`,
`a75f1a3`.

Added or changed:

- `video(attrs, atom)` describes a viewport-local target and
  `Emerge.submit_video_frame/3` consumes owned binary or leased DMA-BUF frames.
- Hidden targets drop immediately and visible targets retain only the latest frame.
- Vulkan imports XRGB8888 and NV12 with staging where required, explicit
  synchronization, strict allocation handling, and render-thread retirement.
- Shutdown drains or quarantines ownership rather than reporting a false clean
  stop. `stop/1` can return an error when safe shutdown cannot finish.

Documentation gap: add a short ownership example, the “do not release after
consume” rule, cold-restart upgrade guidance, and a concise
backend/format/synchronization table.

### Render layers and payload caching

Commit: `f08d80b`.

Added or changed: deterministic semantic paint layers preserve scoped order
while coalescing unchanged own-paint runs. Raster disables renderer payload
caching by default unless explicitly enabled; stats and benchmarks expose cache
behavior.

Documentation gap: explain when to tune renderer cache limits and which counters
show useful or wasteful retention.

### Raster assets and vector support

Commits: `811a53c`, `d88d897`.

Added or changed:

- Decoded raster retention is bounded by entry and decoded-byte limits.
- `assets.decode_at_size` decodes/resamples at device-space draw size. A larger
  retained raster can satisfy a smaller target; a larger target replaces it.
- Encoded source metadata is tracked separately from decoded pixels.
- `renderer_stats_log` reports source/decode/cache memory and retained
  dimensions.
- Retained rasters render while evicted source state is restored asynchronously.
- SVG/vector rendering is unconditional, including embedded builds.

Documentation gap: `use_assets.md` and `assets-images.md` still describe the old
flow. Add configuration examples, zero-limit behavior, sizing/reuse rules,
encoded-versus-decoded accounting, and diagnostics interpretation.

### Correctness and lifecycle

Commits: `dfb067a`, `fe4ecac`, plus lifecycle work in `22e7f8e` and `c085596`.

Fixed centered-text relayout, touch-scroll sample ordering, Wayland video-only
redraws, inactive-target frame release, and ownership-safe shutdown. These need
changelog and regression coverage rather than standalone guides, except for the
public `stop/1` return behavior.

### Build and distribution

Commits: `e6bb0c8`, `a75f1a3`, `20cfa16`.

Added or changed: Nerves Skia cross-builds now isolate host Python, normalize
Clang/sysroot flags, configure embedded fonts, and package link support. The OTP
29 development pin moved to 29.0.5, and the release branch now descends from
`v0.3.4`.

Documentation gap: document source/Nerves builds only after declaring and
testing the Rust floor. Also resolve the mismatch where ExDoc includes internal
guides by default but `package_files/0` omits `guides/internals`.

## Completed coverage

### 1. Release notes and migration — release blocking

- [x] Add missing BW1/Gray2, SVG, screenshot, renderer option, video API,
  shutdown, Nerves build, and centered-text changelog entries.
- [x] Correct `backend_renderer`: deprecated, not removed.
- [x] Remove duplicated 0.3.3/0.3.4 entries from the 0.4 section.
- [x] Document Vulkan rendering/video as supported and build-time selectable.
- [x] Consolidate all 0.4 candidate entries under one unreleased heading; set
  the date only when tagging.
- [x] Add `guides/migrations/0.4.md` covering:
  - renderer option aliases/removals;
  - one-shot tree rendering to retained capture and result tuples;
  - renderer-owned video targets to atom targets and direct frame submission;
  - possible `stop/1` errors and native-video cold restart requirements.

### 2. Backend selection — release blocking

- [x] Document backend/API compatibility, presentation, build flags, fallback,
  capture, video, and support status in `EmergeSkia.start/1`.
- [x] Document the `compiled_backends` backend/API matrix and `drm_card`
  versus `vulkan_drm_node`.
- [x] Keep `set_up_viewport.md` focused on ordinary startup and link to the
  matrix.
- [x] Correct the README backend list and link headless and Vulkan guidance.

### 3. Headless output — release blocking

- [x] Document headless as a viewport mode in `Emerge` and
  `Emerge.Runtime.Viewport`, including a supervised direct frame sink.
- [x] Document frame keys, stride/length formulas, row tails, BW1 polarity,
  Gray2 levels, alpha-over-white, dithering, protected coverage, and packed-
  binary duplicate comparison.
- [x] Include an odd-width validation example.
- [x] Remove Gray4 from the 0.4 public options and keep Emerge Gray8 output
  outside the stable 0.4 output contract.

### 4. Video output — release blocking for video claims

- [x] Document source/target setup, notifications, reconnect, disconnect, and
  shutdown in the `Emerge` and `EmergeSkia` API documentation.
- [x] Add one lower-level `VideoInterop.Consumer` example with explicit
  ownership rules.
- [x] Add the backend/format/synchronization table.
- [x] Keep the long internal architecture guide as an implementation reference;
  replace workspace-specific public instructions with repository links.

### 5. Assets and memory — release blocking for embedded claims

- [x] Update `use_assets.md` with cache limits and `decode_at_size` examples.
- [x] Explain target sizing, reuse/replacement, encoded/decoded accounting, and
  zero limits.
- [x] Update `assets-images.md` and `architecture.md` for bounded retention and
  current ownership.
- [x] Explain one asset-memory log sample and state that SVG needs no optional
  embedded feature.

### 6. Capture and diagnostics

- [x] Document renderer info, stats, logs, capture, backend support, and DRM
  draining retries in the relevant `EmergeSkia` function documentation.
- [x] Add practical cache-tuning guidance based on counters rather than fixed
  recommendations.

### 7. API reference

- [x] Keep exact renderer and output contracts in `EmergeSkia.start/1`, while
  keeping viewport usage and configuration in `Emerge`/`Emerge.Runtime.Viewport`.
- [x] Add examples or guide links for direct video connection, renderer info,
  stats, capture, target info, and managed frame submission.
- [x] Cross-link deprecated and removed options to the migration guide.
- [x] Remove renderer-owned target and consumer-session APIs from Emerge.

### 8. Build, package, and validation

- [x] Declare and test the Rust floor before documenting it.
- [x] Add source/Nerves build troubleshooting under `guides/reference/`.
- [x] Keep maintainer internals out of the published package and align packaged
  files, ExDoc extras, and README links.
- [x] Register new guides in ExDoc, package files, and the README index.
- [x] Run `mix docs`, screenshot/link checks, and formatting.
- [x] Build and unpack the Hex archive, then run `mix docs` from the unpacked
  source so missing guides/assets fail before tagging.

## Acceptance criteria

- Every public 0.4 start option and compatibility constraint has one
  authoritative table linked from `EmergeSkia.start/1`.
- Every new public function has an example or links to one.
- Breaking changes are visible in the changelog and migration guide.
- BW1/Gray2 contracts are exact; Gray4 is unavailable and Emerge Gray8 output is
  not stable in 0.4.
- Vulkan rendering/video is consistently documented as supported.
- Asset limits and diagnostics cover constrained devices.
- All linked guides/assets exist in the unpacked Hex package.
- `mix docs` passes from a clean checkout and the unpacked package.
