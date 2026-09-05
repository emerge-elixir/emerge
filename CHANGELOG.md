# Changelog

## [0.4.0] - 2026-09-03

See the [0.4 migration guide](guides/migrations/0.4.md) for upgrade examples.

### Added

- Added a unified `rendering_api` option across macOS, Wayland, DRM, and headless renderers. Wayland and DRM now support raster presentation, and Vulkan is available for supported Wayland, DRM, and headless builds.
- Added headless binary and Linux DMA-BUF PRIME output. Frames are delivered directly to the configured process as `%VideoInterop.Frame{}` values.
- Added packed BW1 and Gray2 headless output with configurable BW1 polarity and deterministic Atkinson dithering that preserves crisp text, borders, and SVG content.
- Added viewport-local atom video targets through `video(attrs, target)` and `Emerge.submit_video_frame/3`, supporting owned binary frames and leased DMA-BUF frames. Hidden targets consume and drop frames; visible targets retain only the latest frame.
- Added Vulkan video composition for supported NV12 and XRGB8888 DMA-BUF streams, including explicit synchronization and linear or non-linear NV12 layouts.
- Added bounded decoded-image caching, configurable target-sized raster decoding, and asset memory diagnostics. Defaults are 256 entries and 256 MiB per renderer.
- Added `EmergeSkia.renderer_info/1` and expanded renderer statistics for rendering, caches, assets, and video.
- Added precompiled minimal raster NIFs for x86_64, AArch64, and ARMv7 hard-float Linux, OpenGL for ARMv7 hard-float Linux, and combined or Vulkan-only builds for 64-bit Linux. `compiled_backends` now accepts a per-backend GPU API matrix such as `[drm: [:vulkan]]` or `[drm: :all]`.

### Changed

- **Breaking:** `render_to_pixels/2` and `render_to_png/2` now capture a renderer's latest retained frame. They no longer accept a tree and now return `{:ok, binary}` or `{:error, reason}`.
- **Breaking:** the macOS `macos_backend` option was replaced by the cross-platform `rendering_api` option.
- **Breaking:** video submission now uses atom targets and `Emerge.submit_video_frame/3`. Raw PRIME submission, direct connections, and renderer-owned target handles were removed; VideoInterop now defines frame ownership, leases, and synchronization.
- **Breaking:** runtime font loading is now renderer-local and requires `EmergeSkia.load_font_file/5`, with the renderer as its first argument.
- Asset workers, source policies, registered fonts, decoded caches, and diagnostics are now isolated per renderer. Starting, reconfiguring, or stopping one renderer does not affect another.
- Renderer statistics use schema version 25, and the DRM `gpu_queue_completion` timing field was replaced by `gpu_render_elapsed`.

### Fixed

- Fixed centered text positioning after content changes alter the measured width.
- Fixed touch-scroll ordering, sub-pixel fling movement, velocity sampling, and exact boundary clamping.
- Fixed Wayland redraw starvation for scenes containing only video.
- OpenGL video import now accepts supported non-linear DMA-BUF modifiers and fails safely when the required EGL extension or modifier support is unavailable.
- Improved retained rendering-cache correctness for changing text, scrolling, animation, and interleaved static content.
- Fixed native builds for newer Nerves toolchains.

### Known limitations

- Gray8 headless output remains outside the stable 0.4 output contract. Gray4 output is unsupported.

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
