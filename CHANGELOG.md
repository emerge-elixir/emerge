# Changelog

## [0.4.0] - 2026-07-31

### Added

- Added raster and OpenGL headless backends with canonical guarded PRIME output and direct video-target connections.
- Added canonical `VideoInterop` producer and consumer sessions with exact prepare/claim ownership, explicit synchronization, retirement, and drained shutdown.
- Added bounded nonblocking DRM GPU render-elapsed sampling with exact stats-window draining, page-flip correlation, explicit discarded/skipped/stale sample diagnostics, and DRM-scoped draw/presentation counters.
- Added strict immutable stream colorimetry forwarding and the shared direct Vulkan NV12 DMA-BUF importer for exact target-proven one-object/two-plane layouts, with BT.709 output-identical YCbCr conversion, sync-file ownership transfers, bounded process-wide terminal quarantine, and dedicated Vulkan video fault counters.
- Added strict XRGB8888 Camera admission with persistent direct import where supported and bounded linear-texel-buffer-to-optimal-BGRA staging otherwise, preserving ordinary Skia composition at arbitrary paint-layer z-order.

### Changed

- Replaced the old PRIME lease shape with authority-verified per-holder abandonment guards. This is a breaking video interoperability protocol change.
- Unified renderer selection under `backend` and `rendering_api`, while retaining documented compatibility aliases.
- Made producer and consumer release dispatchers lifecycle-owned and explicitly drained/joined outside BEAM resource destructors.
- Replaced damage-inferred paint layers with deterministic semantic Root, Nearby, ScrollContent, Animation, SliderValue, and DirectMedia topology backed by broadly coalesced own runs with exact scoped interleaving.
- Renamed the public DRM timer metric to `gpu_render_elapsed`; the native stats schema is now version 23 after adding dedicated Vulkan video synchronization, release-fence, saturation, quarantine-terminal, and device-loss diagnostics.

### Fixed

- Preserved DRM OpenGL ES 2 compatibility and the macOS fixes published in 0.3.3 while integrating the headless renderer line.
- Removed unconditional full-frame GPU screenshot readback from DRM and Wayland presentation; screenshots now trigger one bounded on-demand capture without serializing every rendered frame.
- Made canonical consumer sessions release inactive-target frames successfully so transient scene visibility no longer terminates an ownership-safe sink.
- Fixed Wayland video-only redraw starvation and width-dependent column fill allocation.
- Kept unchanged ordered paint runs reusable when another run in the same semantic layer changes, isolated correctness-bearing scopes from adjacent paint, deferred rapidly changing GPU payload replacements until stable, staggered related replacements across frames, and retained only the latest version per run.
- Sized isolated text payloads from measured font visual bounds, including glyph overhang and vertical extents, instead of an approximate character-width estimate.

## [0.3.4] - 2026-07-31

### Fixed

- Fixed column fill and weighted-fill height allocation after width-dependent child reflow.

## [0.3.3] - 2026-07-30

### Added

- Added detailed DRM video, GPU queue, atomic commit, and page-flip diagnostics, including the `drm_force_gpu_finish` diagnostic option.

### Changed

- Reworked the DRM PRIME/DMA-BUF video pipeline to own frames safely across page flips and improve import compatibility, synchronization, and release handling.
- Improved retained paint-layer caching for dynamic, animated, scrolling, and visibility-changing content.
- Upgraded Skia to 0.99 and Rustler to 0.38.
- Upgraded the development, CI, and release toolchain to Elixir 1.20.2 and Erlang/OTP 29.0.4.

### Fixed

- Fixed text metrics and rendering-cache invalidation after text content updates.
- Fixed inherited text decorations refreshing correctly when toggled.
- Fixed macOS local host selection, frame retries when Metal drawables are unavailable, and text rendering across raster and Metal surfaces.
- Fixed macOS compilation and compatibility with newer Clippy checks.

## [0.3.2] - 2026-06-09

### Changed

- Improved rendering cache for complex and mostly unchanged UI.
- `renderer_cache.clean_subtree` options are now `renderer_cache.paint_layer` options.
- Drag scroll works in both axes.
- Improved runtime update performance.
- Improved Wayland scaling and suspend/resume behavior.
- Improved DRM animation timing.
- Added renderer diagnostics for debugging slow updates.

## [0.3.1] - 2026-05-07

### Added

- Added first-class `Emerge.UI.Input.slider/2` with native pointer, keyboard, focus, and custom track/thumb support.

### Changed

- Converged macOS and native runtime update paths around shared tree update processing, input normalization, presentation timing, cursor state, and render timing stats.

### Fixed

- Fixed macOS `mouse_over` behavior so hover-driven state and cursor updates refresh correctly.

## [0.3.0] - 2026-05-06

### Added

- Added layout-aware `Emerge.UI.scale/1` and `Emerge.UI.rotate/1`. These top-level attrs affect layout, hit testing, scroll extents, and sibling placement, while `Emerge.UI.Transform.scale/1` and `Transform.rotate/1` remain paint-only.
- Added animation support for layout-aware scale and rotate through `Animation.animate/4`, `Animation.animate_enter/4`, and `Animation.animate_exit/4`.
- Added native performance diagnostics and benchmark coverage for layout, patching, rendering, and runtime stats.

### Changed

- Changed `Emerge.UI.Size.min/2` and `max/2` into mathematical length combinators. `min(a, b)` now resolves to the smaller length and `max(a, b)` resolves to the larger length. This does not affect normal `px/1`, `fill/0`, `fill/1`, `shrink/0`, or `content/0` usage; only code that used `min/2` or `max/2` as the previous bound wrappers needs migration.
- Changed row/column fill planning to resolve nested `fill/1`, `min/2`, and `max/2` expressions recursively against a shared fill unit. This enables layouts such as `height(min(content(), fill()))`.
- Improved retained layout, render refresh, and event-registry reuse so unchanged subtrees can skip more work across rerenders.
- Improved layout-affecting animation scheduling so sampled layout changes become ordinary dirty paths and unrelated retained subtrees can keep using caches.
- Improved Wayland frame pacing and animation timing.
- Improved render and registry refresh by culling clipped/offscreen scroll viewport subtrees, reusing clean registry payloads, and avoiding cold render-cache seeding on dirty rebuilds.
- Improved direct rendering performance for solid borders, template-image tinting, and simple alpha distribution where benchmarks proved a win.

### Fixed

- Fixed keyed reconciliation ordering for mixed insert/remove updates.
- Fixed exit-animation ghost topology so child, paint-child, and nearby trees stay attached during removal animations.
- Fixed nearby overlay layout reuse and hover/dropdown refresh behavior after subtree removal and reinsertion.
- Fixed single-line text input handling so Enter key handlers can suppress the follow-up text commit.
- Fixed macOS host element id encoding.

## [0.2.1] - 2026-04-17

### Changed

- Hardened native video target and NIF boundary handling, including the `submit_prime` path.
- Reduced CI noise and flakiness by gating heavier hover timing tests, relaxing one tail-clear tolerance, and downgrading routine macOS tree update logs to Elixir debug level.
- Updated macOS release and documentation flow so published HexDocs excludes internal guides and release asset verification reports visible release assets more clearly.

## [0.2.0] - 2026-04-17

### Added

- Added initial macOS support through the external macOS host runtime, using Metal when available and falling back to raster rendering when needed. `video_target` is not supported on macOS in this release.

### Changed

- Corrected wrapped row layout behavior after wrapping. Wrapped rows now respect `center_x` and `align_right` attributes from their children, and existing UIs may see visible layout changes.
