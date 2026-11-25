# Public API parity with main

**Latest Linux ports:** [Image-leaf paint transforms, early height-driven width, BMP/static GIF and file backgrounds](images-and-svg.md). [Qualified on binary headless and Wayland shm/GL/Vulkan](images-and-svg.md#linux-qualification); the ordinary suite is still red (54 named failures, four repaired, no additions). No next item selected. Defer native macOS testing until **full Linux parity**.

**Reference:** `main` at `aef5e42fc789c396505f85c34810b2ca9c008586`.
See the [overview](README.md) for what works and the [decision record](../guides/internals/retained-decisions.md)
for intentional differences. **Partial** means the stated subset works;
**missing** means no retained implementation; **verify** means the behavior
has not yet been demonstrated on the target.

## UI declarations, layout and events

Scope: `use Emerge.UI` and its 16 helper modules (140 public function/macro
arities including defaults); 13 wire kinds and 79 attribute tags on `main`.
Aliases, constructors and tags are not independent units of implementation.
The [DSL](../lib/emerge/ui.ex), [attribute codec](../lib/emerge/engine/attr_codec.ex),
[retained decoder](../native/emerge_skia/src/tree/wire.rs) and
[shape dispatch](../native/emerge_skia/src/tree/shapes.rs) distinguish what can
be expressed from what can be drawn or receive input. **9 of 13** `main` kinds
are ported. `svg/2` uses the existing Image kind; `wrapped_column/2` is a
retained-only addition. The [alignment roadmap](alignment.md) covers sites that
still need Nearby, scrolling or editors.

### Supported on Linux: static SVG image elements

`image/2` draws [self-contained SVG](../guides/tutorials/use_assets.md#tint-an-svg)
in its original colors. `svg/2` accepts SVG only and can apply `Svg.color/1`
as a solid or content-box gradient tint using the artwork's alpha. Both accept
`path/1` or inline XML via `binary/1`, with `:contain` or `:cover` fit.
An authorized file and identical inline bytes [share one resident vector picture](../native/emerge_skia/src/tree/tests/svg_pictures.rs)
across sizes, fits and tints. Reuse does not bypass file authorization.

[Pixel and input tests](../test/emerge_skia/svg_pictures_test.exs) cover tint,
file/inline equivalence and remounts without stale hits. See the [current Linux image results](images-and-svg.md#linux-qualification) for binary headless and Wayland shm, OpenGL and Vulkan, and the [earlier tree-owner checkpoint](checkpoints/performance/tree-owner-isolation.md#qualification-and-reproduction) for the cutover itself.

### Remaining UI gaps

| Surface | Remaining behavior |
| --- | --- |
| Image sources and sizing — partial | [PNG](checkpoints/layout/raster-images.md), [EXIF-oriented JPEG](checkpoints/layout/jpeg.md), 24-bit BI_RGB BMP, static GIF frame zero and static SVG. Automatic width can use fill height at the viewport root or directly under a pixel-height El/Row; unresolved height shares are refused. [Sizing, codecs and limits](images-and-svg.md#image-sources-and-sizing). No positive WebP result on the pinned reference. Renderer-local ID lookup is [intentionally removed](../guides/internals/retained-decisions.md#d21-declare-artwork-by-source-not-cache-id). |
| `Input.text/2`, `multiline/2`, `slider/2`, `Input.Slider.config/0,1` — missing | Editable single/multiline text, echo, caret, selection, IME, multiline wrap/scroll; controlled slider range/value/step, track, filled track, thumb and change delivery. A button is already an El, but its keyboard/press behavior is not there. |
| `video/2` — missing | Layout, fit and paint the latest frame of a renderer-local named target; distinct from sending headless frames *out*. See video submission below. |
| `Size`, `Space`, `Align` — partial | Static lengths, padding and fixed gaps work. Direct children of all four linear kinds support [start/center/end groups and cross-axis alignment](../test/emerge_skia/linear_zones_test.exs). [WrappedColumn](../test/emerge_skia/wrapped_column_test.exs) can reject content-dependent widths needed before a height share or child reflow is known. An [El child](../test/emerge_skia/el_child_alignment_test.exs) can override either axis. An [aligned root](../test/emerge_skia/root_alignment_test.exs) keeps its attributes without moving its box. [Even spacing](../test/emerge_skia/linear_even_spacing_test.exs) sizes children before distributing gaps on both axes, independently per wrapped line or column. [El, Text and None retain the flag without using it](../test/emerge_skia/inactive_even_spacing_test.exs). Still separate: placement on unported kinds and layout/text snapping. [Sizing sites](../native/emerge_skia/src/tree/tests/dimensions/sites.rs) |
| `UI.rotate/1`, `Transform` — partial | Paragraph/ancestor move X/Y works on the [submitted paint path](../test/emerge_skia/paragraph_border_test.exs). This does not establish all transform sites. Static alpha on Image or an enclosing El preserves submitted hits. [Image-only paint rotation and uniform scale](images-and-svg.md#rotation-clips-and-installed-input) turn artwork and installed hits without moving siblings. Layout rotation, paint rotation/scale on other sites, animated opacity and other alpha sites remain. Layout `UI.scale/1` and screenshot scale are different contracts. |
| `Nearby`, `clip_nearby/0` — missing | All six mount positions (`behind_content`, `above`, `on_right`, `below`, `on_left`, `in_front`), stacking, escaping/clipped hits and identity on moves. The producer understands the edges; native rejects them. When scrolling lands, test [descendant vs scroller-owned overlays](https://github.com/emerge-elixir/emerge/pull/124) for paint, culling and input; ordinary linear-container clipping is not overlay support. |
| `Color`, `Background`, `Border` — partial | [Inline boxes](../test/emerge_skia/paragraph_border_test.exs) paint solid/gradient backgrounds, per-edge widths, per-corner radii, solid/dashed/dotted borders, outer/inner shadows and glow. Shared ordinary-box painting and [gradient stops/alpha](../test/emerge_skia/gradient_test.exs) work on admitted kinds. [File backgrounds](images-and-svg.md#file-backgrounds-and-tiling) paint on block El, Row, Column and wrapped variants, fitted or tiled on either/both axes. No inline paragraph backgrounds from files or second artwork on an Image. |
| `Font` — partial | Bundled default/sans-serif and mono (including JetBrains Mono aliases); regular/bold/italic/bold-italic and synthetic weight fallback, solid/gradient ink, underline/strike. [Single-line Text](../test/emerge_skia/text_alignment_test.exs) and [paragraph lines](../test/emerge_skia/paragraph_alignment_test.exs) have distinct alignment rules. Letter/word spacing, custom font loading and editor layout remain. |
| `Scroll` — missing | Scrollbars and interaction on both axes; controlled `scroll_x/1` and `scroll_y/1` fractions/key targets, resize/echo rules and `on_scroll` position fractions. Resolving a key in Elixir does not move native content. |
| `Event`, `focus_on_mount/0` — partial | Beyond `on_click`: mouse down/up/enter/leave/move, press, drag/capture, swipe in four directions, key down/up/press, focus/blur, change, scroll and `virtual_key`. Deliver the proper payload and callback (including custom `wrap_payload/3` and external PIDs) on the installed geometry. [Raw-to-local controls](../test/emerge/pointer_drag_test.exs) |
| `Interactive`, `Animation` — missing | Hover, mouse-down and focused styles; `animate`, enter, exit and `change` policies, endpoints, length reflow, hit-testing of moving elements, frame pacing and becoming idle. A decoded transition is not a presented animation. |

## Viewports, configuration and renderer services

Scope: [Emerge](../lib/emerge.ex), [Emerge.Input](../lib/emerge/input.ex),
`Runtime.Viewport`, `Runtime.CodeReloader`, `Assets.Path`/`Ref`, all 19
[Config records](../lib/emerge/config) including `%Emerge.Config{}`, and the
24 documented function arities of [EmergeSkia](../lib/emerge_skia.ex) on `main`.

- **Already usable:** `use Emerge` supplies `start_link/0,1` and `child_spec/1`;
  `mount/1`, `render/0,1`, `config/1,2`, `renderer/1`, `rerender/1`,
  `handle_input/2`, `handle_info/2` and `handle_close/2` have ordinary viewport
  paths. The default/custom payload wrapper and code reload (`notify_source_reloaded/0,1`,
  `CodeReloader.start_link/1`, watcher/compiler options) are BEAM contracts;
  [adapter/lifecycle](../test/emerge/viewport_lifecycle_test.exs) and
  [reloader tests](../test/emerge/code_reloader_test.exs) are not proof of an
  unported listener or platform. `Config.Runtime` health checks, custom adapter
  and `adapter_opts` are distinct from DRM startup retries.
- **Live binary headless and Wayland:** native edit success means applied to the
  preceding accepted revision, not displayed. A busy edit is not queued. The
  viewport retries from its accepted state on credit. [Stage tests](../test/emerge_skia/stage_isolation_test.exs)
  check both selections: ready B can install while C is still drawing, and a
  completed C can supersede an unclaimed B. Only the installed frame supplies
  capture, hit geometry and callback routes. Detached trees and one-shot
  exports do not use this pipeline.
- **Already usable on the supported outputs:** `start/1`, `running?/1`,
  `stop/1,2`, `renderer_status/1`, `renderer_info/1`, `config_status/1`,
  `set_input_target/2`, `set_log_target/2`, `upload_tree/2`, `patch_tree/3`.
  Public `render_to_pixels/1,2` and `render_to_png/1,2` capture an installed
  frame (region, timeout, unit scale, transparent background); no tree overload
  on `main` either. `measure_text/2` supplies default-font metrics; `rgb/3` and
  `rgba/4` pack colors in Elixir. [Delivered-frame tests](../test/emerge_skia/retained_tree_slice_test.exs)
  and [control/status tests](../test/emerge_skia/config_control_test.exs). `renderer_info/1`
  is not field-for-field parity: e.g. by source inspection headless raster reports
  `capabilities.raster_present: [:cpu]`, versus `[]` on `main`.

| Surface | Remaining behavior |
| --- | --- |
| Raw input (`Config.Input.events`, `all_events/0`, `scroll_line_pixels`) — partial | Wayland emits move, enter/leave, left button with modifiers, focus and resize. These cover only part of the ten subscription categories. Other buttons, wheel/precise scroll, keys/repeat/text and platform IME preedit/clear/delete need backend delivery; touch and display-wake require the appropriate backend. Binary headless `send_input/2` can inject pointer (including scroll), key, focus and text-commit, not seat events or an element target. [Wayland handlers](../native/emerge_skia/src/session/wayland/pointer.rs) |
| Live configuration and recovery — partial | Nested records/keyword normalization, input masks, Wayland title and target FPS, session revisions and monitored cleanup work on the supported path. Retained applies these controls immediately; do not infer `main`'s pending/failed device revisions, retry/replacement or safe DRM reuse from an acknowledgment. [Config contract](../lib/emerge/config.ex) |
| Assets and fonts (`Config.Assets`, `RuntimePaths`, `AssetCache`, `Font`; `load_font_file/5`) — partial | Image elements accept logical names, `~m`, `path/1` and `binary/1`; absolute files require an enabled runtime-path policy and an allowlist. [Fitted decode](checkpoints/layout/sized-rasters.md) applies to supported bitmaps including BMP/static GIF, not SVG. The [SVG cache limits](../lib/emerge/config/asset_cache.ex) control idle pictures. One-shot exports support `:snapshot` and `:await`. Remaining: other asset consumers, live asset-root changes and font registration before first paint. [Asset contracts](../lib/emerge/config/assets.ex) |
| `Emerge.submit_video_frame/3`, `EmergeSkia.submit_video_frame/3` — missing renderer | A visible target retains the latest frame; a hidden target consumes/drops it. Honor `VideoInterop.Frame` storage/sync, lease release/abandonment, and the distinction between viewport consumption and direct caller-owned/transferred errors. [Ownership controls](../test/emerge_skia/video_interop_session_test.exs) |
| Diagnostics and caching — partial | `stats/1,2` has v25 peek/take/reset, successful submissions, submit duration and available display interval. Remaining: applicable timing families/counters, draining GPU samples, periodic stats and input/render/close/animation logs, DRM `force_gpu_finish`. Implement `RendererCache`/`PaintLayerCache` limits and reporting; `renderer_info` currently advertises no renderer cache or PRIME video. Tree/glyph reuse is not the public paint cache. [Collectors](../native/emerge_skia/src/session/stats.rs) |

## Outputs, platforms and distributions

Use `main`'s [documented output/API combinations](../guides/reference/native-renderer-builds.md),
not every permutation of config values. A compiled feature or an archive
name proves neither device selection nor visible output.

| Surface | Remaining behavior |
| --- | --- |
| `Headless`, `BinaryFrames`, `PrimeFrames` — partial | Raster binary target/message and three color formats work. Remaining: Gray8, packed Gray2/BW1, polarity, dithering and corresponding capture; OpenGL binary and `:auto` trying GL before raster on eligible builds; OpenGL/Vulkan PRIME export, device selection, bounded in-flight leases, drop-new backpressure and safe shutdown. Headless Vulkan on `main` is PRIME-only. |
| `Window`, `Renderer` on Wayland — partial | Explicit raster/CPU, GL and Vulkan work. Missing `renderer.api: :auto` selection and raster `:gpu_upload`. Preserve `main`'s backend/feature errors and distinguish compiled capabilities from the selected API in `renderer_info/1`. Wayland Vulkan uses compositor feedback, not the explicit DRM display device setting. |
| `DRM`, `Backlight`, `Cursor`, `CursorIcon`, `Runtime` retries — missing | `drm_outputs/0,1` discovery **without** a running renderer; card/connector/mode/size selection, CPU/GL/Vulkan, explicit Vulkan render device independent of the KMS card, handoff of the boot image, hotplug, display power, wake gesture, backlight, cursor hotspots and input, leases/cleanup and restart. Test actual output and device ownership, not only config validation. |
| Linux builds and distribution — partial | `compiled_backends`, feature unions, legacy option normalization, `mix emerge.build_info` and `scripts/build-info.exs` describe the build. Remaining: verify source and precompiled variants (including Nerves/embedded), checksums, fallback and availability errors. [Artifact smoke test](../test/raster_artifact_smoke_test.exs). The [full package check](../scripts/check-package.sh) last stopped at the `counter-basic` screenshot's unsupported `Event.on_press/1`; rerun it after that listener lands. |
| Hardware and performance evidence — verify, not new APIs | An isolated seat exercised 101×75 at 1.0 → 152×113 at 1.5, including the same pointer position across installation, on shm and AMD hardware GL/Vulkan. The opt-in [Vulkan sync tests](../test/emerge_skia/wayland_vulkan_sync_test.exs) ran there. Broader fractional-scale, real acquire starvation, device migration/loss and physical scanout are not established. Use those gates only for the corresponding claim. |

### Deferred: macOS

After Linux reaches full parity, test native Darwin builds and loading on Apple
hardware, including headless raster and SVG in the raster and `macos` profiles.
The retained AppKit window host is still missing: Metal/auto and window raster,
title/floating preference, input/IME, video and close. Headless Metal is not a
`main` parity target.

## Engine and low-level clients

The documented `Engine`, `Element`, `VNode`, `Tree`, `AttrCodec`, `DiffState`,
`Reconcile`, `Patch` and `Serialization` cover declaration building, ID assignment,
lookup/dispatch and binary round trips. The public diff/encode and renderer
submission calls exist, but **the bytes, patch terms and occurrence semantics
changed deliberately**; see [decisions](../guides/internals/retained-decisions.md).
`EmergeSkia.Native` also documents low-level calls: detached `tree_new`,
upload/patch/layout and round trips use the same supported kinds as the live
renderer. What remains is native coverage for the missing declarations above,
offscreen assets beyond the supported image leaves and block backgrounds, custom fonts, and detached tree stats. A round trip
or a synthetic dispatch to a registry is not a rendered frame or real input.
The three removed tree inspection/reset functions and the private test/transport
boundaries are accounted for in the decision record, not silently marked ported.
Some standing tests assert superseded v9 opcodes; check their behavioral intent
against sparse and fresh trees, not by restoring those opcodes.

## Closing one item

1. Identify the public call, variant, backend/site and baseline revision. If
   selecting a change rather than parity, agree on it and record why.
2. Observe it through its real consumer. For UI: cold mount and sparse edit,
   check delivered pixels and raw input through the application's callbacks;
   for a window, check compositor pixels and `wl_seat` where applicable. For a
   BEAM-only or build API, check its own return value, message or artifact.
3. Retire only the proven variant. Update the overview and the two durable
   records when appropriate. Leave unverified behavior explicit and clear the
   selected slot before taking another item.
