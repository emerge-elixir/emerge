# Static Image sizing and decoding on Linux

**Implemented and qualified on Linux.** Bounded Image lengths, sizing from an allocated height and indexed BMP close gaps observed against `main` at `aef5e42f`. Still WebP is an **addition**. This follows the [image transforms and file backgrounds checkpoint](images-and-svg.md) at `6f12563`. No new NIF, setting, source form, cache ID or wire version. Native macOS testing waits for full Linux parity.

## Red controls before implementation

One-shot Linux raster `:await`, `decode_at_size: false`, with matched BEAMs and mapped NIFs. A colored sibling marks the layout edge without counting sampling bleed as layout. These comparisons do not observe input.

| Case | Pinned `main` | Retained `6f12563` | Current result |
| --- | --- | --- | --- |
| 200×100 art, `height(fill())`, 8px sibling | 40px Row: **x=80**; 48px Column: **y=40**; fill-height Row on 80px viewport: **x=160**; 40px WrappedRow: **x=80** | Only the ordinary Row drew | All four match `main` |
| Image width `min(px(60), fill())`, `min(px(60), content())`, `max(px(60), content())`, `min(content(), fill())`; fixed height 24 | Sibling x=**60/60/200/200**; constant `min(px(90), px(60))`: **60** | Only the constant control drew | All five match |
| `width(px(40))` vs `width(min(px(40), px(100)))`, content height | Sibling y=**20/100** | **20/20** | **20/100** |
| `width(min(px(80), fill()))` with content height; `width(fill())` with `height(min(content(), px(60)))` | Sibling y=**40/60** | Both refused | **40/60** |
| Same 16×8 swatch as 1/4/8/24-bit Windows BMP | All four equal PNG | Only 24-bit equaled PNG | All four equal PNG |
| VP8L, VP8 and VP8X+alpha | All three equal missing art, not their decoded PNGs | Same | Draw actual pixels; see the [independent references](../priv/test_assets/SOURCES.md#reproduce-the-still-fixtures) |

The frozen red replay runs 14 original and seven additional checks per build:

```sh
p=/workspace/.emerge-image-gaps-plan-QuOJnP
(cd "$p" && sha256sum --quiet -c PLAN.sha256)
"$p/check" "$(mktemp -d /workspace/.emerge-image-plan-replay-XXXXXX)/fresh"
```

## 1. Bounded lengths on both image axes

The [bounded wire walk](../native/emerge_skia/src/tree/wire/length.rs) preserves the authored Image axes alongside the folded allocation policy. A bound which folds to pixels is not a literal pixel axis. Only literal pixels paired with omitted/bare content project the intrinsic aspect ratio before allocation. An allotted fill can drive a content-bearing, non-fill opposite axis: subtract insets, clamp at zero, project the display ratio, add the opposite insets, then apply its bounds. Two fills do not infer from each other. Pending art supplies a 64px fallback, not a ratio. No second AST or wire field.

The [live edit tests](../test/emerge_skia/raster_images_test.exs) check literal → bound → literal, fresh pixels and local hits. [Native controls](../native/emerge_skia/src/tree/tests/images/sizing.rs) transpose the axes with insets at scale 1.5, including a quarter-turned JPEG and zero inner space.

## 2. Automatic width from an allocated height

| When width is needed | Where height comes from |
| --- | --- |
| Before a parent measures or splits its children | Pass the viewport or host's definite inner height down the El/Row reader branch. A Column uses the same weights, gap charge, bounds and device rounding as placement when its height and slot charges are independent of width reflow. A content cap on an Image slot reads actual intrinsic demand. An available height still evaluates the policy; an assigned share already includes it. Zero is known. |
| Before a WrappedRow chooses breaks | Project from the definite host's inner height. Do not divide by the number of lines. Two 2:1 fill-height Images in 450×150 wrap into 300×150 boxes; widening to 600 puts them on one line. |
| Only to size and align the placed leaf | A pixel-width El or Column can use the actual placement height; other slots need not supply an early width. |

This checkpoint checked the final parent, siblings and kept branches before publication and refused early shares that depended on reflow. It did not cover a fill-height Image directly under a WrappedColumn or TextColumn or nested chains of early shares. The later [Size qualification](size.md#implementation-order) covers those sites and distinguishes independently measured heights from circular packs. No previous-frame guess, document copy or Place-to-Flow retry. [Tests](../native/emerge_skia/src/tree/tests/images/sizing.rs) change the height of a Column sibling **8→16→8** (image widths **80→64→80**), change sources and weights, resize through zero, compare moved subtrees with a fresh tree and reject a cycle without advancing the accepted declaration. The wrapper minimum and wrap breaks have separate [pinned reference cases](https://github.com/emerge-elixir/emerge/blob/aef5e42fc789c396505f85c34810b2ca9c008586/native/emerge_skia/src/tree/layout/tests/measurement.rs#L1157-L1301).

## 3. Indexed Windows BMP

[`complete_rgb_bmp`](../native/emerge_skia/src/raster_assets/format.rs) accepts Windows 40-byte `BITMAPINFOHEADER`, one plane, `BI_RGB`, 1/4/8 bpp or the previous 24-bit BGR. Before Skia expands indices, check signed dimensions, table length/offset, declared file and padded row extents, and visible indices for a short palette. Unused bits, row padding and reserved palette bytes do not become pixels or alpha. [Odd-width top-down rows, high indices and rejection controls](../native/emerge_skia/src/raster_assets/tests.rs). No RLE, bitfields or general BMP claim.

## 4. Still WebP (new behavior)

All six artifacts request `webp-decode` on the existing registry Skia 0.157.0. Its archives include the bundled decoder/demux; there is no second graphics library, WebP encoder feature, system WebP dependency or conversion executable. [Build requirements](../guides/reference/native-renderer-builds.md#baseline-and-optional-features).

The [worker preflight](../native/emerge_skia/src/raster_assets/format/webp.rs) accepts one VP8 or VP8L image, optionally in VP8X with consistent canvas, alpha and supported metadata. It bounds the RIFF/chunk extents, padding, dimensions and combined metadata (1 MiB). Animation flags/`ANIM`/`ANMF`, unsupported chunks and inconsistent declarations are failed art. Even a valid 16×8 header can fail full or fitted pixel decoding. Use EXIF display orientation exactly once, not a tree transform.

[Pixel tests](../test/emerge_skia/still_images_test.exs) compare lossless opaque, alpha and EXIF-turned images exactly against FFmpeg-decoded PNGs over black and white. For lossy RGB, interior probes allow 1 level on the swatch and 20 on the alpha grid, based on **independent decoders before renderer testing**; alpha agrees exactly. All 16 [fixture checksums](../priv/test_assets/stills/SHA256SUMS) reproduced. No FFmpeg or Netpbm dependency at build, replay or test time.

Both codecs use the existing authorization and fitted worker: logical names, authorized absolute files and inline leaf bytes; file-only block backgrounds (contain, cover and repeat); full/fitted, `:snapshot`/`:await`, denied and wrong-format files after inline warming. `svg/2` still refuses rasters. [Stale-fit replacement and shrink](../native/emerge_skia/src/tree/tests/images.rs) leave the installed output intact until the replacement is submitted. A missing/undecodable image is failed art, not an invalid EMRG declaration. No change to decoded residency, staging, sparse definitions or the picture cache.

## Linux qualification

Each slice had a separate native and focused Mix run. Bounded lengths and allocated height each reached **862/916** in ordinary seed 17. The first indexed-BMP run had one extra 100 ms raw-press timeout in `EmergeSkia.BoundedContentTest`; the unchanged module passed three isolated repeats, then the full suite gave **863/917**. No timeout was raised; cause unknown. The integrated still-WebP run gave **867/921**. Named sets, not just counts, were compared.

| Final Linux x86-64 release artifact | Native tests passed | Focused Mix passed |
| --- | ---: | ---: |
| Desktop GL | 246 | 202 |
| GL + `stage-probes` | 246 | 202 |
| Wayland Vulkan | 270 | 202 |
| CPU-only | 242 | 202 |
| Embedded CPU (host build) | 242 | 202 |
| Wayland shm-only | 245 | 202 |

- Locked offline release tests, strict all-target Clippy and warning-clean Mix compile passed for each. The loaded BEAMs, mapped library bytes/inode and compiled backend matrix were verified. Each has one Skia; CPU profiles do not pull a GPU renderer. Embedded `ldd` has no desktop font, display, GPU or WebP library. Default Rust **1.91.0** also passed **246** tests with the WebP-enabled archives.
- Final ordinary Mix, seed 17: **867/921 passed** (15/15 doctests, 852/906 tests). The **54 failures and 21 exclusions match by name** with `6f12563`. Seven new tests pass in the ordinary suite; the opt-in stage tests are separate. This is not all-green CI.
- All **15** opt-in headless [stage tests](../test/emerge_skia/stage_isolation_test.exs) passed. Across a held and definitely rejected height-share handoff, `(30,36)` stayed blue and clicked the sibling at `(30,4)`; only the installed retry was red and used the new image callback at `(30,36)`. Callback epochs and detach/remount remain tied to installation. The [heap interposer](../native/emerge_skia/src/tree/tests/allocations.rs) covers a fitting bounded Image/Column edit through prepared drawing and hits, **not** codec, BEAM or GPU allocation.
- On three isolated Wayland seats (shm, OpenGL, Vulkan), each artifact reported the requested API. At scales **1 and 1.5**, each run checked **34** installed captures and compositor crops, and **44** routed clicks with local coordinates. Scenes changed inherited Row height, a Column sibling, a WrappedRow break, literal vs folded SVG width, and indexed/WebP file backgrounds. Vulkan reported `MESA_RADV`, Radeon RX 7900 XTX, `software: false`. These observations are not one-shot export, physical scanout, latency or device-loss tests.

Formatting, strict Credo, Dialyzer, warning-clean ExDoc with the existing screenshots, local links in 67 generated pages and the SVG screenshot check passed. The Hex source package compiled, but `scripts/check-package.sh` still stops in the screenshot phase: the `counter-basic` check refuses `Event.on_press/1` (attribute 61) before paint. CI also requests the absent `retained_topology_reads` integration target. Neither gate is green.

Private replay: `/workspace/.emerge-static-cLqsQV/HOWTO.md` (immutable input manifests, builds, reference comparisons, named reports and isolated-seat controls). The [API tracker](api-parity.md) keeps remaining site and platform limits visible. No animation/video, general codec, Darwin/Metal, ARM/Nerves or retained DRM/KMS qualification.
