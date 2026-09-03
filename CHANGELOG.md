# Changelog

## [0.4.0] - Unreleased

### Added

- Added packed BW1 and Gray2 output for headless raster sessions. Rows are packed independently and MSB-first with zero tail bits; BW1 polarity is configurable and Gray2 uses levels `0..3` from black to white.
- Added deterministic Atkinson dithering for BW1 and Gray2 after compositing over white, with text, SVG/vector coverage, borders, and other crisp UI protected from error diffusion.
- Added bounded raster asset caching with configurable 256-entry and 256 MiB defaults, target-sized image decoding, and asset source/decode/cache memory reporting in `renderer_stats_log`.
- Added unified `rendering_api` selection and capability checks across macOS, Wayland, DRM, and headless sessions, including Wayland raster presentation and DRM raster GPU upload.
- Added `EmergeSkia.renderer_info/1`, renderer-aware statistics, and periodic renderer diagnostics for the selected backend, rendering API, cache, pipeline, and video paths.
- Added retained raster and OpenGL headless binary output plus bounded PRIME output. Both modes deliver `%VideoInterop.Frame{}` values directly to a configured target process.
- Added viewport-local atom video targets and `Emerge.submit_video_frame/3`, with owned binary and leased DMA-BUF storage, immediate hidden-target drops, latest-visible-frame replacement, explicit synchronization, retirement, and drained shutdown.
- Added experimental Vulkan rendering for Wayland, DRM, and headless sessions, with deterministic UI/video composition. Vulkan and its video import paths remain experimental pending hardware qualification.
- Added strict immutable stream colorimetry forwarding and a shared Vulkan NV12 DMA-BUF importer for exact one-object/two-plane layouts, with BT.709 YCbCr conversion, sync-file ownership transfers, bounded terminal quarantine, and dedicated Vulkan video fault counters.
- Added strict XRGB8888 Camera admission with persistent direct import where supported and bounded linear-texel-buffer-to-optimal-BGRA staging otherwise, preserving ordinary Skia composition at arbitrary paint-layer z-order.
- Added an `auto` Vulkan NV12 path that copies non-linear image planes or exact linear producer-buffer planes into bounded, ordinary optimal Y/UV images before Skia composition; forced `planar` remains the compute-plane rollback. Linear transfer imports separate the final copied byte from the complete allocation so producer-owned V3DV read-ahead tails remain outside every copy region.

### Changed

- Made SVG/vector rendering available unconditionally, including embedded and headless builds.
- Updated the development Erlang/OTP 29 pin to 29.0.5.
- `render_to_pixels/2` and `render_to_png/2` now capture the latest retained renderer frame instead of rendering a supplied tree. They return `{:ok, binary}` or `{:error, reason}` and accept region, pixel-format, and timeout options; scale, background, and PNG compression currently support `1.0`, `:transparent`, and `:default` respectively.
- Replaced `macos_backend` with `rendering_api` and removed `dispatch_mode`. The `backend_renderer` option and `:gl` rendering value remain accepted as deprecated aliases.
- Replaced the old PRIME lease shape with authority-verified per-holder abandonment guards. This is a breaking video interoperability protocol change.
- Replaced raw PRIME submission, renderer-owned video target handles, consumer-session ceremony, and direct viewport connections with `video(attrs, atom)` and `Emerge.submit_video_frame/3`.
- Simplified mailbox-free video submission to register only the EmergeSkia renderer handle, call `EmergeSkia.submit_video_frame/3` directly, and document its low-level ownership receipts publicly.
- Made producer and renderer release dispatchers lifecycle-owned and explicitly drained/joined outside BEAM resource destructors. `stop/1` can now return `{:error, reason}` when an ownership-safe PRIME shutdown cannot complete.
- Replaced damage-inferred paint layers with deterministic semantic Root, Nearby, ScrollContent, Animation, SliderValue, and DirectMedia topology backed by broadly coalesced own runs with exact scoped interleaving.
- Renamed the public DRM timer metric to `gpu_render_elapsed`; the native stats schema is now version 25 after adding renderer, cache, asset, Vulkan video synchronization, release-fence, saturation, quarantine-terminal, and device-loss diagnostics.

### Fixed

- Reused retained decoded rasters immediately when an inactive image source returns to the tree, avoiding a transient loading-placeholder frame while its active encoded source payload is restored.
- Recentered horizontally centered text after content patches change its measured width.
- Published the complete fd-backed allocation size for headless Vulkan PRIME frames instead of the smaller Vulkan image requirement, while keeping plane spans and importer validation strict.
- Made inertial touch scrolling preserve chronological final motion before release, accumulate high-frequency velocity samples, preserve sub-pixel fling motion, avoid stale-deadline watchdog spins, and clamp high-velocity flings exactly at either boundary.
- Kept high-volume event-runtime traces at debug level while native diagnostic logging remains disabled by default.
- Fixed Nerves Skia cross-compilation with newer host toolchains by isolating host Python, sanitizing Clang/sysroot flags, and packaging the embedded link support used by source builds.
- Removed unconditional full-frame GPU screenshot readback from DRM and Wayland presentation; screenshots now trigger one bounded on-demand capture without serializing every rendered frame.
- Made direct video submission consume and release inactive-target frames successfully so transient scene visibility no longer terminates an ownership-safe sink.
- Declared concrete headless PRIME synchronization and modifier contracts from the selected producer path, allowing both Vulkan and explicit-sync OpenGL producers with linear DMA-BUFs to open Vulkan consumer streams without weakening synchronization or layout policy.
- Allowed OpenGL consumers to admit explicit non-linear DMA-BUF modifier contracts and pass their exact per-plane modifiers to EGL, while retaining immutable stream/frame matching and failing import when the EGL modifier extension or driver support is unavailable.
- Removed a headless redraw feedback loop, kept PRIME backpressure recovery on the configured target cadence, and excluded Wayland late-replacement swaps from displayed-frame FPS counters.
- Fixed Wayland video-only redraw starvation.
- Kept unchanged ordered paint runs reusable when another run in the same semantic layer changes, isolated correctness-bearing scopes from adjacent paint, deferred rapidly changing GPU payload replacements until stable, staggered related replacements across frames, and retained only the latest version per run.
- Sized isolated text payloads from measured font visual bounds, including glyph overhang and vertical extents, instead of an approximate character-width estimate.
- Admitted opaque `XR24` stream formats at the direct DMA-BUF submission boundary so supported XRGB8888 frames reach native Vulkan validation and staging.
- Serialized VideoInterop queue submissions through the Vulkan render-thread authority, resolved exact NV12 candidates per stream contract, bound synchronization lanes to unique imports, and preserved explicit recovery when Ganesh rejects a wait semaphore.

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
