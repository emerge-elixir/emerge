# Images and SVG on the retained Linux renderer

**Checkpoint at `6f12563`.** For subsequent sizing, indexed BMP and still WebP results see the [Linux follow-up](image-sizing-and-codecs.md). The counts and boundaries below describe this earlier build.

**Implemented:** static Image-leaf rotation and uniform paint scale, automatic width from an early fill height, uncompressed 24-bit BMP, GIF frame zero, and file-backed block backgrounds (including tiling). No new NIF, EMRG version or SVG renderer. These are scoped ports, not general transform, codec or layout parity. See the [API tracker](api-parity.md). Native macOS testing waits for full Linux parity.

## Rotation, clips and installed input

The [maintainer's note](https://github.com/emerge-elixir/emerge/pull/122#issuecomment-7155679314) explains why the upstream patches cannot be applied directly. Reviewed against upstream `18078280`: [#122](https://github.com/emerge-elixir/emerge/pull/122) (`823208be`) corrects device-space bitmap bleed; [#123](https://github.com/emerge-elixir/emerge/pull/123) (`a7530a0b`, includes #122) identifies the misplaced borrowed media clip. The controls below used pinned `main` `aef5e42f`, not either PR branch. Before the port, `7f14a0c` refused tag 33.

Center a 100×100 red `:cover` Image in a white 180×180 El, either directly or inside a 100×100 El with radius 8. Sample `(90,8)` outside the turned square, `(90,30)` at its tip, `(44,44)` in the exposed corner:

| Case | Pinned `main` | Retained |
| --- | --- | --- |
| Unturned, both hosts and sources | white / white / red | white / white / red |
| 45°, roomy PNG | **red** / red / **red** | white / red / white |
| 45°, roomy SVG | white / red / white | white / red / white |
| 45°, rounded PNG | white / **white** / **red** | white / red / white |
| 45°, rounded SVG | white / **white** / white | white / red / white |

The unturned rounded frames already differed across renderers (196 PNG pixels, 68 SVG pixels); `main` is not an antialiasing golden. Retained [raster](../test/emerge_skia/raster_images_test.exs) and [SVG](../test/emerge_skia/svg_pictures_test.exs) tests check sparse turns, undo, fresh frames, the analytical silhouette away from its AA edge, rounded hits, tint, border, opacity and EXIF crop. At radius 8 the tip hits at local `(7.574,7.574)`; `(44,44)` misses. At radius 24 `(90,25)` misses too.

- [Tag 33](../native/emerge_skia/src/tree/shapes/image.rs) reduces finite degrees before narrowing. Tag 34 scales around the same frame center: negative reverses both axes; zero has neither paint nor hit. Neither moves siblings. Layout rotation, transformed subtrees, video and animated poses are separate.
- The [prepared pose](../native/emerge_skia/src/tree/paint/pose.rs) keeps viewport/independent ancestor clips stationary and turns the leaf's own or borrowed media shape once. The [sampler](../native/emerge_skia/src/tree/paint/image.rs) divides one device pixel by the transformed unit-axis length; eligible bitmap masks relax by half that. A 45° turn gives pads `(1,1)`, not the extent of an enclosed diamond. SVG gets no bitmap bleed. A bounding rectangle limits work; it never defines the shape or target.
- Installed hits use the inverse pose, not sampled alpha or the newest accepted tree. The [held-handoff control](../test/emerge_skia/stage_isolation_test.exs) checks capture, local coordinates and callbacks. A pure turn neither grows the fitted bitmap nor records another SVG. Genuine magnification can grow raster demand (native control: 80×40 → 160×80); shrinking can reuse it. SVG rechecks its effect budget.
- Pinned scale control: center an 18×16 BMP in 40×40. At `(14,20)`, scale `1 / −1 / 0 / 2` gave red / blue / white / red. The reference input harness routed art `(3,8)` / art `(15,8)` / host `(14,20)` / art `(6,8)`. These are static observations, not playback or timing.

## Image sources and sizing

[PNG](checkpoints/layout/raster-images.md), [EXIF-oriented JPEG](checkpoints/layout/jpeg.md), the BMP/GIF profiles below and static SVG accept logical `priv/` names (including `~m`), authorized absolute paths or encoded leaf bytes. `svg/2` requires SVG; it can tint composited alpha. Identical resident SVG content can share one recording across uses, fits, tints and poses. [Renderer-local ID lookup was intentionally removed](../guides/internals/retained-decisions.md#d21-declare-artwork-by-source-not-cache-id); it is not a remaining port.

| Declaration | Size input |
| --- | --- |
| Neither axis specified | Intrinsic **display** dimensions, independent of cached bitmap size. |
| One pixel axis; other omitted or `content()` | Apply the known aspect ratio inside padding and border. |
| `width(fill())`; automatic height | Use the settled inner width. A 2:1 image at width 80 puts the next Column child at y=40. |
| `height(fill())`; automatic width | Use the viewport height at the root, or a directly enclosing El/Row's pixel height less its insets. Subtract the leaf's vertical insets, clamp to zero, apply the display ratio, then add horizontal insets. Pending art has the 64px fallback, **not** a known 1:1 ratio. |
| Both axes fixed | `:contain` centers the artwork; `:cover` crops it. Neither changes the box. |

The Image foundation rejected unresolved Column shares or wrap-dependent heights before publication. The [current Size rules](size.md) also allow independent post-wrap charges and nested height offers; neither implementation consults the last frame as a size estimate. The [Row regression](../test/emerge_skia/raster_images_test.exs) and [native controls](../native/emerge_skia/src/tree/tests/images.rs) cover changing the parent's height, viewport resize, invalid moves, pending → ready, explicit content width and an oriented JPEG with insets. With 200×100 art and Row height **40 → 60 → 40**, the next sibling starts at **x=80 → 120 → 80** on both pinned `main` and retained. At height 40 `(84,3)` hits the sibling at `(4,3)`; at height 60 it hits the image. Before the port the fill-height declarations were refused.

## Static raster profiles

The [extension allowlist](../lib/emerge/config/runtime_paths.ex) grants permission; it does not select a decoder. For the original 16×8 swatch, BMP/GIF logical files matched PNG on pinned `main`; the tested lossless WebP matched **missing**. Before the port all three settled to failed art on retained, file and inline. Additional pinned controls used these [9×8 fixtures](../priv/test_assets/SOURCES.md).

| Format | Worker product and controls |
| --- | --- |
| BMP | Windows `BITMAPINFOHEADER`, `BI_RGB`, 24-bit BGR. Signed height, padded row stride, dimensions and file extent are checked before decode. Bottom-up and top-down encodings produce the expected swatch with an opaque black marker. Other BMP encodings are not claimed. |
| GIF | Bounded GIF87a/GIF89a logical screen and sub-block stream, including frame rectangles and trailer. Skia paints frame **zero**: the interlaced transparent fixture has a clear marker. A second frame with a different palette does not change it, also on pinned `main`. No playback or frame cache. |
| WebP | **Not ported.** Require a positive reference on an identified supported build or a separate scope decision before adding a decoder. No inference about all lossless, lossy or alpha profiles. |

Both new formats use the [existing worker and fitted pipeline](../native/emerge_skia/src/raster_assets/format.rs), not render-thread decoding. [Tests](../test/emerge_skia/raster_images_test.exs) exercise logical/file/inline, full/fitted, contain/cover, denied absolute path, format mismatch, truncation, oversized dimensions and `svg/2` refusing raster art. A valid declaration for a bad file settles to the failure indicator. The encoded-file, per-bitmap and resident budgets still apply; they are not RSS or GPU limits.

## File backgrounds and tiling

Tag 12 variant 2 on a block El, Row, Column, WrappedRow or WrappedColumn declares a **file** background. It paints the rounded border box over outer shadows, below inset shadows, children and border. It adds no intrinsic size or listener. It is not a second artwork source on an Image, an inline paragraph decoration, `binary/1` or `Svg.color/1`. Absolute files are authorized before warm reuse.

`:contain`/`:cover` use the box, not the padded child area. `:repeat`, `:repeat_x` and `:repeat_y` use one shader over a bitmap or the resident picture, anchored at the border-box origin at intrinsic device size. A non-repeated axis is transparent outside the first tile. No per-tile nodes, source opens or SVG recordings. A picture tile exceeding 16,777,216 texels draws the failed indicator; that doesn't prevent a small non-tiled draw of the same source.

[Pixel and click controls](../test/emerge_skia/image_backgrounds_test.exs) check sparse JPEG ↔ SVG ↔ color, shadow order, all five block sites, authorization, source removal and the three repeat modes. Pinned `main` and retained each matched the intrinsic tile grid for the 2×2 SVG and 9×8 BMP across all three modes at scales 1 and 1.5. [Native publication](../native/emerge_skia/src/tree/tests/publication.rs) checks folded removal/undo and stale open/fit completions against a pinned frame; [picture counters](../native/emerge_skia/src/tree/tests/svg_pictures.rs) check a file backdrop and inline leaf share one recording across fit edits.

## Linux qualification

- Locked release `cargo test`: desktop GL **236**, GL with stage probes **236**, Vulkan **260**, CPU-only **232**, embedded CPU **232**, Wayland shm-only **235**. Strict all-target Clippy passed on all six. The Rust 1.91 default build also passed **236** tests. Embedded `ldd` has no desktop font, display or GPU library. These are Linux host builds, not ARM board tests.
- The five focused UI, wire, raster, SVG and background files passed **180** tests on the matching headless artifacts. On the Vulkan-feature build, one earlier headless run missed a raw press at `(4,43)` within the existing 100 ms deadline in the PNG resize test. Six unchanged isolated repeats and two complete repeats passed; cause not established. No timeout was raised.
- All **14** opt-in headless stage tests passed. A ready then definitely rejected image turn left the old pixels and host callback installed; its retry installed the new inverse and callback. A held fill-height edit likewise left the sibling at its old position until installation. The native heap interposer checked fitting pose edits through prepared samples and inverse hits, not codec or GPU allocation.
- Full ordinary Mix, seed 17: **860/914 passed**, 54 failures, 21 exclusions (15/15 doctests, 845/899 tests). Against the pre-port **58 failures / 21 exclusions**, four named failures were repaired, none added: fill-axis sibling movement, logical SVG repeat pixels, and both background image wire round trips. Exclusions match by name. This is **not** a green release suite.
- Isolated Wayland shm, OpenGL and Vulkan: selected API, compositor crops and real virtual-seat click routes checked at **1 and 1.5**. Controls covered PNG/SVG poses and height edits, BMP/GIF scale `1 / −1 / 0 / 2` (with a combined quarter turn), and file-background/foreground order. Installed captures also matched all **827** SVG tile-strip probes per mode/scale step (`repeat → repeat_x → repeat_y → repeat`); no physical scanout or smoothness measurement.

- Format, warning-clean Mix compile, strict Credo, Dialyzer and ExDoc with the existing screenshots passed (67 pages checked for local links). The SVG documentation screenshot passed separately. The `counter-basic` check still fails before paint on `Event.on_press/1` (tag 61); the whole package script was not rerun. Separately, CI names a `bench-diagnostics` integration target, `retained_topology_reads`, which is absent on this retained branch. No all-green CI, package, hardware or timing claim.

Private replay: `/workspace/.emerge-image-ports-jAD1Vt/release/HOWTO.md` (frozen inputs, named reports, mapped builds and checksum manifest). Pre-port witnesses: `/workspace/.emerge-image-plan-eLzZ2P/` (rotation and PR review) and `/workspace/.emerge-image-inventory-fPDzGW/` (sizing and codecs).

## Deferred with scrolling and Nearby

[#124](https://github.com/emerge-elixir/emerge/pull/124) (`77b42e14`, independent of #122/#123) changes the [documented Nearby escape rule](../lib/emerge/ui/nearby.ex). The [scrolling checkpoint](scrolling.md#reference-controls) kept the documented opt-in rule; the [scroll visibility implementation](scroll-paint-visibility.md) changes it for scrolling descendants. A row's `in_front` knob scrolls with the row but stops at the port. The navigation outside receives clicks; the port's own overlay can still overflow unless the port sets `clip_nearby()`. This is not an image-decoder change.
