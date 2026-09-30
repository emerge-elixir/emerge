# Changelog

## [0.4.1] - 2026-09-30

### Fixed

- GNU DRM precompiled artifacts embed FreeType and disable fontconfig. DRM
  OpenGL+Vulkan configurations select a dedicated `drm_all` archive instead of
  the desktop Vulkan bundle, avoiding xkbcommon/Wayland dependencies on Nerves.
- Release builds check embedded ELF dependencies and load AArch64 DRM archives
  against a checksum-pinned stock RPi5 rootfs before publishing.
- Release and Hex workflows reuse successful CI for the exact source commit
  instead of rerunning the standard test matrix. Native builds wait for that
  gate; the full-sweep CI step runs only the additional tagged tests.

## [0.4.0] - 2026-09-29

Changes below are relative to stable 0.3.4 and include the features introduced in
0.4.0-beta.1.

See the [0.4 migration guide](guides/migrations/0.4.md) for upgrade examples.

### Breaking changes

- `render_to_pixels/2` and `render_to_png/2` now capture a running renderer's latest
  retained frame instead of rendering a supplied tree. They return `{:ok, binary}`
  or `{:error, reason}` rather than a binary.
- Replace `macos_backend` with `rendering_api`. The `dispatch_mode` option was
  removed. `backend_renderer` and `:gl` remain deprecated aliases, not removals.
- Video submission now uses viewport-local atom targets and
  `Emerge.submit_video_frame/3`. Raw PRIME submission, direct renderer connections,
  and renderer-owned target handles were removed. Every normal viewport submission
  return consumes the frame.
- Runtime font loading is renderer-local: `EmergeSkia.load_font_file/5` now takes
  the owning renderer as its first argument.
- `EmergeSkia.stop/1` can return `{:error, reason}` when ownership-safe shutdown
  cannot finish. Do not treat an error as successful cleanup or reuse uncertain resources.
- `Background.gradient/2,3` and raw `{:gradient, from, to, angle}` attributes were
  removed. Use `Background.color(gradient([from, to], angle))` with
  `Emerge.UI.Color.gradient/1,2` instead.
- Animated width and height no longer accept `min`/`max` expressions, including
  as sources of change transitions. These expressions remain available for static layout.

### Added

- Unified `rendering_api` selection across macOS, Wayland, DRM, and headless
  renderers. Wayland and DRM gained raster presentation; Vulkan is available when
  compiled for Wayland, DRM, or headless PRIME output.
- Headless binary and Linux DMA-BUF PRIME output, delivered directly to a configured
  process as `%VideoInterop.Frame{}` values. Packed BW1 and Gray2 output supports
  configurable BW1 polarity and deterministic Atkinson dithering that protects
  crisp text, borders, and SVG content.
- Viewport-local `video(attrs, target)` elements for owned binary and leased
  DMA-BUF frames. Hidden targets consume and drop frames; visible targets retain
  only the latest frame. Vulkan composition supports NV12 and XRGB8888 DMA-BUF
  streams with explicit synchronization and supported linear/non-linear layouts.
- Owned RGBA8888 binary video frames on macOS. DMA-BUF/PRIME input remains unsupported.
- DRM display discovery with `EmergeSkia.drm_outputs/1` and explicit display/mode
  selection with `drm_output` and `drm_mode`. Multiple viewports can use separate
  displays on the same GPU.
- Native renderer status and shutdown timeouts through `EmergeSkia.renderer_status/1`
  and `EmergeSkia.stop/2`. Viewports can recover from an unexpectedly stopped native
  renderer after safe cleanup without losing their callback PID or state.
- `Emerge.UI.Animation.change/3` animates retained attribute changes from the
  current presentation, for example
  `Animation.change([width(fill()), height(px(60))], 300, :ease_out)`.
  Explicit and change animations support pixel, content, fill, and weighted-fill
  dimensions. Content-size transitions also respond to text, child, and asset
  metric changes without changing the `content()` declaration.
- `Emerge.UI.Color.gradient/1,2` creates evenly spaced multi-color gradients for
  backgrounds, text, borders, shadows, and SVG tint. Animated solid endpoints can
  transition to gradients; gradient endpoints must have matching stop counts.
- Configurable image/SVG caching, target-sized image decoding, and asset-memory
  diagnostics. The default pixel cache is 256 entries / 256 MiB per renderer;
  parsed SVG caching has separate limits. SVG support is also included in embedded builds.
- Bundled JetBrains Mono NL v2.304 regular, bold, italic, and bold italic fonts
  under SIL OFL 1.1. Select `"monospace"`, `"JetBrains Mono NL"`, or `"JetBrains Mono"`
  with `Font.family/1`; Inter remains the default proportional font.
- `EmergeSkia.renderer_info/1` and expanded renderer, cache, asset, and video statistics.
- A per-backend `compiled_backends` GPU API matrix, such as `[drm: [:vulkan]]`
  or `[drm: :all]`. Expanded release-build profiles include minimal raster,
  combined OpenGL/Vulkan, and Vulkan-only variants for x86_64/AArch64 GNU Linux;
  ARMv7 hard-float GNU, x86_64 musl, and RISC-V64 GNU have raster and DRM/headless
  OpenGL profiles.

### Changed

- Renderers can use independent asset roots, fonts, and cache limits without
  affecting each other when started, reconfigured, or stopped.
- **Visual change:** `Border.shadow` and `Border.glow` paint the full box-shaped
  shadow behind backgrounds and content. Transparent interiors reveal the shadow;
  opaque backgrounds cover it. This also applies to inline paragraph wrappers;
  inset shadows are unchanged.
- Pending images reserve their layout slot without a loading indicator for the
  first 100 ms. Slower loads show a small centered three-dot indicator instead
  of a full-slot shimmer; ready images and errors appear immediately.
- Raised the default renderer cache creation budget from 16 to 64 payloads per
  frame. Total/per-entry byte limits and cache admission policy are unchanged.
- Renderer statistics use schema version 25. DRM timing field
  `gpu_queue_completion` was replaced by `gpu_render_elapsed`.
- Updated the VideoInterop dependency minimum to 0.1.2. Source builds require
  Rust 1.91 or newer.

### Fixed

- Fixed stuck text selections and slider drags, delayed or misdirected input during
  UI updates, and touch-scrolling glitches.
- Improved renderer responsiveness and shutdown during heavy UI updates, and
  recovery from temporary rendering failures that could leave headless output frozen.
- Fixed animation jumps, unexpected restarts, and completion issues when content
  or layout changes, including interrupted and exit animations.
- Fixed stale images and rendered content after asset replacement, text changes,
  scrolling, animation, or cache resets.
- Images and SVGs preserve aspect ratio when one dimension is fixed or fill-based
  and the other is automatic, and align correctly with `in_front` overlays.
- Fixed content-sized containers wrapping fixed-size children and sizing errors
  after slider updates.
- Fixed centered text after content changes, alignment of wrapped paragraph lines,
  and excessive spacing in highlighted code containing zero-width anchors.
- Fixed paragraph backgrounds, borders, shadows, and glow across wrapped lines,
  including rounded corners and border/padding spacing.
- Fixed the bundled Inter Bold Italic font and SVG gradient transparency.
- Fixed video-only Wayland scenes not refreshing and expanded OpenGL video
  compatibility with supported non-linear DMA-BUF layouts.
- Fixed a DRM buffer-reuse bug that could display stale frames and retain memory.
- Fixed source builds with newer Nerves toolchains and musl, and added automatic
  MangoPi RISC-V64 target detection.

### Known limitations

- Gray8 headless output remains outside the stable 0.4 output contract. Gray4
  output is unsupported.
- macOS does not support retained-frame capture, `renderer_status/1`, or `stop/2`.
- Automatic renderer recovery does not cover GPU hangs or all device failures;
  some failures require restarting the BEAM process. Separate DRM displays do not
  have independent input-device routing.
- Hardware compatibility and constrained-device performance testing remain ongoing;
  a precompiled artifact does not guarantee a board's GPU compatibility.
- Image file-size and cache limits do not cap temporary memory use during decoding.

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
