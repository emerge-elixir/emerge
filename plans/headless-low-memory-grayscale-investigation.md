# Headless Low-Memory Grayscale Rendering Investigation

Date: 2026-08-29
Status: research complete; implementation feeds
`active-headless-grayscale-output.md`

## Question

Can the headless CPU renderer avoid a full RGBA8888 framebuffer on low-memory,
low-power e-ink targets while retaining pictures, transparency, gradients,
shadows, and useful 1-bit/2-bit dithering?

## Conclusion

Yes. The pinned Skia supports CPU raster drawing directly into an 8-bit
`Gray8` surface. That is the lowest practical Skia destination for the complete
Emerge scene. Skia does not provide 1-bit or 2-bit canvas surfaces, and `Alpha8`
is a coverage mask rather than grayscale color.

Recommended pipeline:

```text
Emerge RenderScene
    -> Skia Gray8 opaque raster surface, cleared white
    -> scene-aware grayscale dithering/quantization
    -> row-packed bw1 or gray2 BEAM binary
```

This reduces the persistent framebuffer from four bytes per pixel to one while
letting Skia perform image sampling and alpha compositing. The final 1-bit or
2-bit conversion remains a small native CPU pass.

## Exact pinned-Skia findings

The project uses rust-skia commit
`0d2261c63941f4b534522246cc1ace13ca4242d8` (`skia-safe` 0.99.0).

- `ColorType::Gray8` is one byte per pixel and is always opaque.
- Raster surfaces accept `Gray8` with `AlphaType::Opaque`.
- The raster pipeline can load, blend, and store `Gray8` destinations.
- Color is converted to Gray8 with Skia's BT.709 luminance/luma stage.
- Source alpha still participates in normal blending against the current opaque
  Gray8 destination. A translucent image or fill therefore works when the
  destination is cleared to white first.
- `ColorType::Alpha8` stores alpha only. RGB is discarded, so it cannot render a
  general grayscale scene without replacing normal paint/image semantics.
- There is no Skia 1-bit, 2-bit, or 4-bit general canvas color type.
- Skia's built-in paint dithering uses a Gray8 rate of 1/255. It does not solve
  final 1-bit or 2-bit quantization and is not error diffusion.
- Save layers and image filters can still allocate temporary N32/RGBA surfaces.
  Skia explicitly upgrades Gray8 layers to N32, especially for image filters.
  The main framebuffer saving is real, but pathological full-frame alpha/filter
  layers can temporarily consume RGBA-sized memory.

Relevant pinned source:

- `skia-safe/src/core/color_type.rs`
- `skia-safe/src/core/surface.rs`
- `skia/src/core/SkRasterPipeline.cpp`
- `skia/src/core/SkRasterPipelineBlitter.cpp`
- `skia/src/core/SkCanvas.cpp`
- `skia/src/core/SkBitmapDevice.cpp`

## Local qualification probe

A temporary, uncommitted release-mode probe rendered identical scenes to
RGBA8888 and Gray8 with renderer payload caching disabled.

Coverage:

- 324-node Showcase Borders scene: rects, rounded rects, borders, shadows, and
  93 text draws.
- Manual color gradients.
- A 45%-alpha gradient group, forcing alpha-layer compositing.
- A decoded 640x420 JPEG drawn into a 320x240 viewport.

Pixel comparison used the same white clear and BT.709 integer approximation.

| Scene | Maximum Gray8 difference | Mean absolute difference |
| --- | ---: | ---: |
| Showcase Borders | 2/255 | 0.227 |
| Gradient + alpha group | 1/255 | 0.465 |
| JPEG picture | 1/255 | 0.474 |

Indicative workstation render timings:

| Scene | RGBA8888 | Gray8 |
| --- | ---: | ---: |
| 960x800 Showcase Borders | 2.0-2.3 ms | 1.26-1.28 ms |
| 320x160 gradient + alpha | 0.137 ms | 0.138-0.140 ms |
| 320x240 JPEG | 0.440 ms | 0.435 ms |

These are qualification probes, not committed benchmarks. They establish that
Gray8 is viable and does not introduce an obvious CPU penalty. The text-heavy
scene was faster because less destination memory was cleared and written.

## Frame-memory model

For a 400x300 display:

| Buffer | Size |
| --- | ---: |
| RGBA8888 surface/frame | 480,000 bytes |
| Gray8 surface/frame | 120,000 bytes |
| Gray2 packed frame | 30,000 bytes |
| BW1 packed frame | 15,000 bytes |
| One-bit semantic dither mask | 15,000 bytes |
| Three i16 error rows at width 400 | about 2,400 bytes |

The current steady-state path keeps or creates multiple RGBA copies: the Skia
surface, readback frame, latest-frame snapshot, and transient replacement of the
previous snapshot. It then allocates packed output and copies that output into
an `OwnedBinary`. Depending on frame history, framebuffer-related peak memory is
roughly 1.5-2.0 MB at 400x300, before assets, glyphs, scenes, and Skia internals.

A Gray8-native path can keep the main working set near:

- 120 KB surface
- 120 KB native latest-frame snapshot
- 15-30 KB output binary
- 15 KB packed policy mask
- a few KB of rolling diffusion error

Allowing for the previous snapshot during replacement and temporary output, the
frame pipeline should stay around 0.3-0.5 MB. This is a 4x-6x reduction in the
frame-related peak, not total VM/RSS.

## Required renderer changes

### Select Gray8 for grayscale binary output

For `backend: :headless`, `rendering_api: :raster`, and output formats
`:gray8`, `:gray4`, `:gray2`, or `:bw1`:

- Create the main raster surface as `ColorType::Gray8` / `AlphaType::Opaque`.
- Use unknown/no pixel geometry instead of RGB horizontal LCD geometry.
- Clear the surface to opaque white, since the destination cannot preserve
  transparency and e-ink output is opaque.
- Keep RGBA8888 surfaces for `:rgba8888` and `:rgb888`.
- Keep renderer payload caching disabled for the low-memory raster profile until
  Gray8 payloads are explicitly implemented and qualified.

### Preserve native frame format

`RasterFrame` and `LatestFrameSnapshot` currently imply RGBA.

- Add an internal pixel-storage enum carrying format and row stride.
- Read/borrow Gray8 pixels as Gray8; do not expand to RGBA every frame.
- Pack/dither directly from Gray8.
- Store Gray8 in `LatestFrameStore`.
- Expand Gray8 to RGBA only when a screenshot/PNG is explicitly requested.
- Fill the final `OwnedBinary` directly where practical instead of building a
  `Vec<u8>` and copying it into another allocation.

### Keep fallback and parity paths

- Retain an RGBA render-and-convert path behind tests or a diagnostic option
  until Gray8 scene parity is established.
- Pixel-test text, images, gradients, shadows, clips, transforms, and alpha
  groups against RGBA composited over white.
- Detect unsupported Gray8 surface creation at startup and return a precise
  error; do not silently return malformed low-bit output.

## Dithering design for low memory

Dither from the final composited Gray8 frame, not from individual transparent
layers. This preserves correct alpha/background interaction.

Use scene semantics only to build a policy mask:

- Opaque solid fills, borders, and text: protected/direct quantization.
- Raster pictures and video: dither.
- Gradients and shadows: dither.
- Translucent backgrounds/fills: dither so alpha-generated gray levels survive
  1-bit output.
- Later protected content overwrites earlier dither policy in painter order,
  keeping text crisp over pictures or translucent panels.

Memory-conscious implementation:

- One packed bit per output pixel for the protected/dither policy.
- Fixed-point rolling error rows, not a full-frame error buffer.
- Write packed output row-by-row.
- Never diffuse error through protected pixels.
- Keep dithering deterministic across frames to avoid unnecessary e-ink changes.

Algorithm trade-off:

- Ordered Bayer dithering is cheapest, bounded, stable under partial updates, and
  particularly suitable for flat translucent backgrounds.
- Atkinson diffusion is a reasonable low-cost photo default with local error.
- Jarvis improves some photographs but does more writes and spreads changes over
  a wider neighborhood.

The consolidated V1 plan uses deterministic Atkinson behind a boolean public
option and requires target comparison against the current external Jarvis
pipeline. Selecting different algorithms for pictures and alpha fills later
would require either a second mask bit or separate ordered regions.

## Remaining dominant memory risks

A Gray8 framebuffer does not make the whole renderer low-memory by itself.

### Raster pictures

`insert_raster_asset` currently decodes and retains a raster Skia image. An
opaque 640x420 N32 image is roughly 1.1 MB, larger than the proposed entire
frame pipeline.

Follow-up options:

- Decode opaque JPEG/PNG assets directly to Gray8 with `SkCodec` for the
  grayscale raster profile.
- Use codec-supported downsampling to cap decoded dimensions to the output or
  configured asset maximum.
- Keep RGBA fallback for images that genuinely require alpha.
- Consider an explicit small asset-cache budget and eviction policy.

### SVG variants and caches

Rendered SVG/vector variants are currently budgeted up to 16 MB and accounted as
RGBA. For the low-memory profile:

- Disable the rendered-vector variant cache or give it a small configurable
  budget.
- Later cache Gray8 variants where semantic parity is proven.
- Keep direct vector rendering available; do not remove SVG support.

### Temporary layers and filters

Skia upgrades Gray8 save layers/image-filter work to N32. Avoid unbounded
full-frame alpha groups and filters on low-memory screens. Emerge already draws
simple alpha rects, rounded rects, and text directly without a save layer; extend
that direct-alpha coverage only with pixel tests.

### Delivery backlog

Binary headless delivery has no acknowledgement/backpressure. A slow e-ink
consumer can accumulate frame binaries in its mailbox.

A low-memory profile should eventually add:

- at most one outstanding binary frame or explicit consumer acknowledgement
- duplicate packed-frame suppression
- deterministic output suitable for frame differencing
- optional changed-region metadata for partial e-ink updates

## Recommended implementation order

1. Fix packed-row correctness and define exact BW1/Gray2 output.
2. Add Gray8 as the raster surface for grayscale headless formats.
3. Store latest frames in native Gray8 and expand only on screenshot capture.
4. Fill packed BEAM binaries without an intermediate copy.
5. Add packed scene-policy masks and target-benchmarked Bayer/Atkinson dithering.
6. Add low-memory asset decode/cache limits; prioritize opaque pictures.
7. Add binary-output backpressure and duplicate-frame suppression.

The first four steps provide most framebuffer savings without coupling the
renderer to one dither algorithm.
