# Public API parity with main

Emerge has one layout engine. It sizes and places elements before any renderer executes the shared Skia draw commands. Layout support is tracked by element and rule, not by renderer or OS.

**Latest layout change:** [Vertical alignment of inline children](inline-layout-rotation.md#vertical-alignment-on-one-line) uses the height of each child's own line. A static `UI.rotate/1` can make that line taller; `Transform.rotate/1` turns the phrase and its clicks without enlarging the slot.

**Linux test results:** For [inline rotation and vertical alignment](inline-layout-rotation.md#linux-verification), **287 native** and **41 focused Mix** tests passed. Ordinary Mix passed **887/942** with the same 55 named failures and 21 exclusions. The standalone **Text turns** demo verified installed pixels, compositor crops and local clicks on a private scale-1 Wayland raster seat. The [six-profile, stage-probe and three-presenter results](size.md#linux-qualification) are from the earlier Size change.

**Selected work:** None.

**Upstream PRs:** Rotation was new to retained. Two results deliberately correct pinned `main`: a turned bitmap no longer paints into the exposed corners ([#122](https://github.com/emerge-elixir/emerge/pull/122)), and a rounded mask borrowed from the image's host turns with the art instead of cutting off its tips ([#123](https://github.com/emerge-elixir/emerge/pull/123)). Independent ancestor clips stay fixed; clicks follow the installed shape. These were [checked against `main`](images-and-svg.md#rotation-clips-and-installed-input), not cherry-picked. [#124](https://github.com/emerge-elixir/emerge/pull/124) is **deferred**: it concerns overlays escaping a scroller, not image clipping. The separate `../wayland_cards` demo exposes the image changes in **Image pose**, **Row height** and **Backdrop**.

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

### Image sources and sizing

`image/2` accepts [PNG](checkpoints/layout/raster-images.md), [EXIF-oriented JPEG](checkpoints/layout/jpeg.md), Windows `BI_RGB` BMP (1/4/8/24 bpp), GIF frame zero, still WebP and self-contained SVG. Leaf sources can be logical `priv/` names (strings, `~m` or `path/1`), authorized absolute files via `path/1`, or encoded bytes via `binary/1`. Renderer-local `{:id, id}` lookup was [removed by design](../guides/internals/retained-decisions.md#d21-declare-artwork-by-source-not-cache-id), not left to port. `svg/2` requires SVG and supports [solid or gradient tint](../guides/tutorials/use_assets.md#tint-an-svg). An authorized file and identical inline bytes [share a resident picture](../native/emerge_skia/src/tree/tests/svg_pictures.rs) across sizes, fits, tints and poses; reuse does not bypass authorization.

A literal pixel axis and automatic opposite axis use display aspect inside the insets. `min` and `max` are lengths, not literal pixels: 200×100 art at `width(px(40))` is 40×20, but at `width(min(px(40), px(100)))` it is 40×100 with automatic height. A fill allocation can drive the opposite content axis, including its bounds. `height(fill())` uses the offering site: an available height, an assigned group share, unbounded space for a document block or measured demand for a float. A content Column retains the Image's intrinsic width minimum without forcing its placed Image to that width. A pending Image supplies no ratio. [Aspect examples](image-sizing-and-codecs.md#2-automatic-width-from-an-allocated-height) · [nested and post-wrap shares](size.md#s4-column-shares-after-independent-sibling-reflow).

The [reference comparisons](image-sizing-and-codecs.md#red-controls-before-implementation) are one-shot raster exports on Linux. The three indexed BMPs matched PNG on pinned `main`; VP8, VP8L and VP8X+alpha instead matched missing art. The [WebP regression](../test/emerge_skia/still_images_test.exs) uses independently decoded reference PNGs. Installed capture, compositor pixels and routed local clicks were [checked separately](image-sizing-and-codecs.md#linux-qualification).

### UI coverage and remaining gaps

#### Implemented

- **Elements:** `el`, `text`, `none`, `row`, `column`, `wrapped_row`, `paragraph`, `text_column` and `image` (including `svg/2`). `wrapped_column/2` is a retained addition.
- **Static sizing (`Size`):** Document blocks and floats resolve [both axes](../native/emerge_skia/src/tree/tests/dimensions/sites.rs). Rows and columns divide space per group; paragraphs [wrap once at their final width](size.md#s5-row-charges-and-wrappedrow-membership).
  [Inline widths](size.md#s2-document-and-paragraph-floats) can affect paragraph size without setting word height. The engine rejects [circular packs](size.md#s7-real-feedback-and-the-closing-audit) without changing the accepted tree or installed hits. The nine shared kinds accepted all 51 Size reference declarations; [48 matched the sampled raster measurements](size.md#reference-differences). `wrapped_column/2` has explicit expected sizes, not a `main` oracle.
- **Spacing and alignment (`Space`, `Align`):** Padding, fixed or [even gaps](../test/emerge_skia/linear_even_spacing_test.exs), and [main- and cross-axis alignment](../test/emerge_skia/linear_zones_test.exs) work per line or column.
  A [paragraph span](inline-layout-rotation.md#vertical-alignment-on-one-line) aligns to the top, center or bottom of its own line, including beside a layout-turned phrase. An [El child](../test/emerge_skia/el_child_alignment_test.exs) can override either axis; an [aligned root](../test/emerge_skia/root_alignment_test.exs) stays at the origin. [Inactive even spacing](../test/emerge_skia/inactive_even_spacing_test.exs) and snap flags do not add geometry or font rounding.
- **Static layout rotation (`UI.rotate/1`):** Blocks and floats occupy their [turned border box](static-layout-transforms.md#layout-controls). A [Row or WrappedRow](size.md#s5-row-charges-and-wrappedrow-membership) uses the independently measured wrapped height when dividing space between siblings. On [inline Text or El→Text](inline-layout-rotation.md#reference-cases), the rotated phrase contributes its width and height to the line; its decoration, ink and hit region turn together.
- **Static `Transform`:** Translation, rotation and uniform scale are **transforms that preserve sibling layout**. Ink can overlap a sibling; [clicks follow the installed shape and clips](../test/emerge_skia/static_transforms_test.exs), including on paragraph spans. Negative scale reverses both axes; zero scale removes ink and hits. `alpha/1` changes opacity, not hits.
- **Images:** Bitmap and SVG leaves support [intrinsic size, aspect ratio and fill](#image-sources-and-sizing). BMP support is limited to the listed `BI_RGB` formats, GIF to frame zero and WebP to still images.
- **Decoration (`Color`, `Background`, `Border`):** Boxes and [inline spans](../test/emerge_skia/paragraph_border_test.exs) support solid and [gradient fills](../test/emerge_skia/gradient_test.exs), per-edge widths, per-corner radii, solid/dashed/dotted borders, inner and outer shadows, and glow.
  [File backgrounds](images-and-svg.md#file-backgrounds-and-tiling) fit or tile on El, Row, Column and wrapped containers. They are not inline backgrounds or a second art layer on an Image.
- **Fonts:** Bundled sans-serif and mono faces (including JetBrains Mono aliases) support regular, bold, italic and bold-italic, with synthetic weight fallback. Text supports solid or gradient ink, underline and strike. [Single-line Text](../test/emerge_skia/text_alignment_test.exs) and [paragraph lines](../test/emerge_skia/paragraph_alignment_test.exs) have distinct alignment rules.
- **Clicks:** `on_click/1` routes a left-button click to the callback for the installed element, using local coordinates. `Input.button/2` builds an El; it is not a separate element type.

#### Remaining

- **Text inputs (`Input.text/2`, `Input.multiline/2`):** Controlled editing and value echo, caret, selection, IME, and multiline wrapping and scrolling.
- **Sliders (`Input.slider/2`, `Input.Slider.config/0,1`):** Controlled range, value and step; track, filled track and thumb; delivery of value changes.
- **Video (`video/2`):** Size and fit a named video target and paint its latest frame. Sending headless frames *out* does not implement video input.
- **Nearby overlays (`Nearby`, `clip_nearby/0`):** Elixir encodes all six positions (`behind_content`, `above`, `on_right`, `below`, `on_left`, `in_front`), but native layout rejects them.
  Overlays need host-local anchoring, stacking, clipping, hit testing and stable identity on moves. In the `main` raster check, `below` turns with its host without moving the Row peer. [Scroller-owned and descendant overlays](https://github.com/emerge-elixir/emerge/pull/124) need separate paint, culling and input checks.
- **Scrolling (`Scroll`):** Scrollbars and interaction on both axes; `scroll_x/1` and `scroll_y/1` by fraction or key; resize and value echo; `on_scroll` position fractions. Elixir can resolve a key, but native content does not yet move.
- **Fonts:** Letter spacing, word spacing, custom font loading and text layout inside editors.
- **Element events and focus (`Event`, `focus_on_mount/0`):** Mouse down/up/enter/leave/move, press, drag/capture, directional swipes, key down/up/press, focus/blur, change, scroll and `virtual_key`. Button press and keyboard activation are also missing.
  Deliver each event to the installed element's callback, with [local pointer coordinates](../test/emerge/pointer_drag_test.exs) where applicable, custom `wrap_payload/3` and external PID targets.
- **Interactive styles and animation (`Interactive`, `Animation`):** Hover, mouse-down and focused styles; `animate`, enter, exit and `change` policies. Animated lengths need reflow. Hit tests must follow moving elements, and frames must be paced until the animation reaches its endpoint and becomes idle.

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
  exports do not use this pipeline. [Tree-owner cutover checks](checkpoints/performance/tree-owner-isolation.md#qualification-and-reproduction).
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
| Assets and fonts (`Config.Assets`, `RuntimePaths`, `AssetCache`, `Font`; `load_font_file/5`) — partial | Image elements accept logical names, `~m`, `path/1` and `binary/1`; absolute files require an enabled runtime-path policy and an allowlist. [Fitted decode](checkpoints/layout/sized-rasters.md) applies to supported bitmaps including indexed BMP, GIF frame zero and still WebP, not SVG. The [SVG cache limits](../lib/emerge/config/asset_cache.ex) control idle pictures. One-shot exports support `:snapshot` and `:await`. Remaining: other asset consumers, live asset-root changes and font registration before first paint. [Asset contracts](../lib/emerge/config/assets.ex) |
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

1. Identify the public call, variant and baseline revision. For layout, name
   the element and where it is placed. If selecting a change rather than parity,
   agree on it and record why.
2. For a layout rule, compare geometry on a cold mount and after a sparse edit;
   one working backend suffices. For presentation or input, test the backend
   being claimed: check installed pixels and routed callbacks, and for Wayland
   check compositor pixels and seat events where applicable. For a BEAM-only
   or build API, check its return value, message or artifact.
3. Retire only the proven variant. Update the overview and the two durable
   records when appropriate. Leave unverified behavior explicit and clear the
   selected slot before taking another item.
