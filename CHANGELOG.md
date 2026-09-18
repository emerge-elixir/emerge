# Changelog

## [Unreleased]

### Added

- Bundle JetBrains Mono NL v2.304 regular, bold, italic, and bold italic fonts (826 KiB total, SIL OFL 1.1). Select with `Font.family("monospace")`, `"JetBrains Mono NL"`, or `"JetBrains Mono"`; Inter remains the default proportional font.

- Add release builds for x86_64 musl and RISC-V64 GNU (raster and DRM/OpenGL), Nerves compiler detection for MangoPi, and dynamic-CRT flags for musl NIF source builds. New artifacts require publication with matching package checksums.
- Added `Emerge.UI.Animation.change/3` for native retained-value transitions, with per-field timing, current-presentation interruption, and automatic sibling geometry holds. Explicit and change animations support pixel/content/fill/weighted-fill width and height transitions.

- Added `Emerge.UI.Color.gradient/1,2` for evenly spaced gradients with two or more colors, usable in all UI color slots: backgrounds, inherited/paragraph/input text, borders, shadows and SVG template tint. Animated solid endpoints are lifted to the gradient endpoint; gradient endpoints require matching stop counts.

### Changed

- Split Mix configuration into build-only native, packaging and documentation helpers, sharing compiler target data while preserving SDK setup and precompiled selection.

- **Visual change:** `Border.shadow` and `Border.glow` now paint the full box-shaped shadow behind backgrounds and content, including text. Transparent interiors reveal the shadow instead of cutting it out; opaque backgrounds still cover it. Applies to ordinary elements and inline paragraph wrappers; inset shadows are unchanged.

- Add native allocation-source transport for same-mount reparent, role and scale
  changes, preserving published geometry and existing animation clocks; remounts
  do not inherit old sources. Structural, coupled-input, hold and ghost retries
  have native regression coverage. Large-tree performance optimization and physical
  platform/constrained-device qualification remain separate follow-up work.

- `Emerge.UI.Animation.change/3` now takes an attribute list, for example `Animation.change([width(fill()), height(px(60))], 1000, :linear)`, instead of a single attribute. Each field receives the shared timing; ordered overrides and per-field policy removal are preserved.

- Animated width/height no longer accept `min`/`max` expressions. These remain available for static layout. Change sources using these expressions are rejected as well.
- Added EMRG attribute tag 84 for change policies; macOS host compatibility is now version 14. Upgrade native artifacts with the Elixir code.

- **Breaking:** removed `Background.gradient/2,3` and the old raw gradient tuple. Use `Background.color(gradient([from, to], angle))`; see the migration guide. EMRG is now v9 and the macOS host handshake is v14; upgrade native artifacts and re-encode stored trees/patches together.

- Raised the default renderer cache creation budget from 16 to 64 payloads per frame. Total/per-entry byte limits and cache admission policy are unchanged; explicit count-budget overrides remain supported.
- Rasterized SVG size/fit variants now share the configured asset pixel-cache budget and LRU with raster images, instead of separate fixed SVG limits.
- SVG font discovery and bounded parsed trees are reused across scenes and sizes. Added `assets.cache.svg_tree_max_entries` and `svg_tree_max_bytes`, plus parsed/pixel cache diagnostics.
- Parsed SVG cache configuration and universal multi-color gradients are included in the v14 macOS host protocol.

### Fixed

- Paragraph layout now treats U+200B anchors as zero-width instead of measuring/rendering font glyphs for them. This fixes inflated indentation and spaces in Makeup-highlighted code, while retaining blank-line height.

- Replace the invalid bundled Inter Bold Italic HTML download with the genuine v4.0 TTF, matching the other embedded Inter faces. Bold italic text now uses its real face instead of synthetic styling.

- A full tree channel no longer blocks the event actor from receiving Stop. Unsent
  native operation packets retain FIFO order and mount/receipt authority; host
  drains remain bounded. Renderer shutdown signals actors independently and wakes
  rendering before waiting on full actor channels. This does not impose a memory cap.

- Native input replay and fresh cursor draining now yield after 64 top-level inputs.
  The macOS host feedback loop no longer drains and loses the next request at its
  eight-round budget boundary; queued work remains available for the next call.

- Stalled native input no longer rescans/reallocates its full history on every
  enqueue or reinserts untouched tails after each replayed edit. FIFO recovery
  preserves adjacent coalescing and event order; full drain releases queue storage.
  Noncoalescible input already admitted to that FIFO is retained without a new
  memory cap; this does not fix Wayland full-ingress-channel drops.

- Failed binary headless draws/readbacks no longer strand terminal frames. The
  newest failed scene retries with bounded backoff; replacement and Stop remain
  responsive between attempts, without restarting native animation clocks.

- In-flight native input commands no longer edit, focus, scroll or restyle a
  replacement widget reusing the same ID. Mount-scoped event packets preserve
  grouped effects and response controls; deferred writes also recheck their mount.

- Buffered pointer releases and drag anchors no longer transfer to replacement
  widgets reusing the same ID. Native mount checks also reset old hover and pending
  edit state; delayed hover/drag and actor/headless damage coverage is expanded.

- Older queued animation registries no longer release newer buffered input.
  Native response fences preserve keyboard/IME replay through delayed installation;
  event-side coalescing retains only still-eligible mount focus and drains bounded batches.

- Use libc's platform-specific ioctl request type for DMA-BUF CPU synchronization, fixing DRM/OpenGL compilation against musl.

- Inline paragraph wrappers now paint explicit solid/gradient backgrounds, all border styles, outer/inner shadows and glow per wrapped-line segment. Opaque backgrounds hide interior shadows; absent backgrounds emit no background draw. Border/padding insets participate in wrapping; shadows remain paint-only. Per-edge borders and shadows now preserve independent corner radii on ordinary boxes too.

- Paragraph `center_x()` and `align_right()` now align each wrapped text line, including the last line. Explicit `Font` text alignment takes precedence; inherited font-alignment changes invalidate retained paragraph positions correctly.
- Prepared native frames freeze image layout facts and scene bindings together across
  replacement, stale completions and configuration resets. Decorative image edits
  retain the paint-only path.
- Tree actors coalesce blocked registry/scene output while continuing to receive
  input and Stop; pending mount focus is rebound to current native geometry.
- Slider-imposed widths no longer leak into later intrinsic queries. Clearing or
  reattaching orphan roots preserves published animation sources and clocks.
  Expanded noncanonical coupling, complex ghost, input and retained-damage tests.

- Native retained scenes keep their raster/SVG image bindings when the same asset
  ID is replaced or cache state is reset. Image cache identity now distinguishes
  renderer-local generation collisions; stale loader completion and animation
  replay coverage is expanded.

- Native dimension-clock evidence now composes per axis, including self-node feedback and numeric/mixed drivers. Historical native goal receipts preserve first change/exit motion and coupled cancellation across model changes; combined context releases use original clock inputs rather than mixing old environment with new loop phase.

- Ongoing finite shared-pool and Content-parent animations preserve their curve against mixed looping peers/children using native clock evidence. Feedback loops keep their clocks and frozen presentations; exact forecast destinations and combined historical model/clock inputs remain validated.

- Finite shared-pool and Content-parent dimension animations can release against mixed looping peers/children, including loop boundaries. Native forecast/destination checks preserve query provenance, retry safety and independent clocks without retaining a history chain.

- Unrelated mixed-length animation inputs no longer restart dependent easing or block finite completion when native layout proves independence. Original targets, joint allocation and actual release-frame samples remain validated.

- Finite dimension animations finish correctly when a mixed-length parent crosses a segment or repeat boundary, including late/skipped frames. Native full-allocation checks and retry/publication safety remain enforced.

- Unchanged native animation frames reuse registry-subtree eligibility instead of repeatedly walking the entire tree. External/runtime/topology edits still invalidate it, and empty-registry layouts skip empty geometry-snapshot scans.

- Persistent native animation failures now back off automatic pulse retries while preserving the last published layout, input registry and admitted clocks; external updates and explicit retries remain immediate.
- Mixed ancestor dimensions use native clock witnesses for dependent motion. Finite intrinsic dependencies cross static fill/content wrappers, while pixel loop wrappers can update ancestor and descendant members without delaying finite release.

- Finite dimension releases handle ordinary pixel-parent segment/repeat boundaries, including skipped cycles, without rewinding the parent's clock or bypassing native footprint checks.

- Pixel-sized parent animation segments now preserve dependent child motion after earlier content/fill segments, with native footprint validation across segment transitions.

- `Animation.change([width(content())], duration, curve)` now animates resolved size changes from text/descendant and intrinsic metric updates without changing the `content()` declaration. Content height works symmetrically, with published-pose interruption, retry-safe clocks and joint holds for nested content policies.

- Exit animations use the latest exit policy and the published layout footprint, including interrupted fill animations. Captured descendants preserve allocation scope and scale without retaining event handlers.
- Content-parent/fill-child animation scopes share native targets rather than sustaining each other's intermediate sizes. Admitted hold intervals survive sibling cancellation and arrival.

- SVG color attributes now validate their values before serialization, and gradient tint preserves source alpha in rendering and grayscale policy.

- Centered responsive images now align with matching `in_front` overlays: content-sized `el` hosts finalize both growth and shrinkage before alignment, and columns center using resolved child heights.

- Images and SVGs with one fill-based dimension now grow or shrink the automatic opposite dimension proportionally, honoring explicit limits on either axis. Content-width parents and row allocation account for height-driven image widths.

- SVG and raster images now preserve their intrinsic aspect ratio when one dimension is pixel-sized and the other is omitted or content-sized (#74). Asset dimension changes also invalidate retained image and ancestor measurements.

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
