# Active Plan: Headless Layer-Aware Dithering

Created: 2026-05-28
Status: active design plan

## Goals

Add built-in dithering to Emerge headless raster output for e-ink-style targets
such as Trellis/name_badge, without adding a separate dithering NIF and without
blindly dithering the whole final frame.

Primary target:

```elixir
EmergeSkia.start(
  otp_app: :name_badge,
  backend: :headless,
  rendering_api: :raster,
  width: 400,
  height: 300,
  headless: [
    target: self(),
    mode: :binary,
    pixel_format: :bw1,
    bw1_polarity: :one_is_white,
    dither: [algorithm: :jarvis, scope: :auto]
  ]
)
```

Desired behavior:

- Keep solid color backgrounds, borders, and text crisp.
- Dither image-like/continuous-tone content such as raster photos, gradients,
  shadows, and selected paint layers.
- Keep SVG support available on Trellis; do not remove the `resvg/usvg` path for
  the minimal headless raster build.
- Output final packed binary formats directly from Emerge (`:bw1`, `:gray2`,
  `:gray4`, etc.), avoiding `Emerge -> PNG/gray -> Dither -> pack` pipelines.

## Non-goals for V1

- Do not add a dependency on `protolux-electronics/dither`, `image`, or another
  NIF for Emerge dithering.
- Do not implement color-palette dithering in V1. Start with grayscale output
  depths used by current headless binary formats.
- Do not apply dithering to Wayland/DRM/macOS presentation paths.
- Do not dither PRIME output in V1.
- Do not make SVG support optional for the Trellis profile. Optional SVG can be a
  later size-only feature, but this target keeps SVG.

## References

- `../name_badge/lib/name_badge/display.ex`
  - Current badge pipeline: Typst PNG -> `Dither` -> grayscale/raw -> bit-pack
    -> `EInk.draw/3`.
  - Current bit polarity packs white as `1`, so Emerge should use
    `bw1_polarity: :one_is_white` for direct output.
- `../dither/native/dither_nif/src/dither.rs`
  - Algorithms currently exposed by Protolux `Dither`:
    `:floyd_steinberg`, `:atkinson`, `:stucki`, `:burkes`, `:jarvis`, `:sierra`.
- `native/emerge_skia/src/render_scene.rs`
  - `RenderNode::PaintLayer(RenderPaintLayer)` and `DrawPrimitive` are the
    right semantic source for automatic dither/protect classification.
- `native/emerge_skia/src/backend/headless/mod.rs`
  - Current packed pixel conversion lives in `convert_frame/4`; this should move
    to a dedicated output conversion/dither module.

## Public API shape

Extend nested headless options:

```elixir
headless: [
  pixel_format: :rgba8888 | :rgb888 | :gray8 | :gray4 | :gray2 | :bw1,
  bw1_polarity: :one_is_black | :one_is_white,
  dither: :none | :threshold | :atkinson | :jarvis | keyword()
]
```

Keyword form:

```elixir
dither: [
  algorithm: :none | :threshold | :atkinson | :jarvis | :floyd_steinberg | :stucki | :burkes | :sierra,
  scope: :auto | :all | :images | :none,
  protect: [:text, :solid],
  serpentine: false
]
```

V1 defaults:

- No behavior change unless `headless.dither` is configured.
- Existing threshold/quantize output remains the default.
- If `dither: :jarvis` or another algorithm atom is supplied, use
  `scope: :auto`.
- `:threshold` means no error diffusion, but use the configured quantization
  threshold/polarity.

Supported V1 algorithms:

- `:threshold` / `:none` (current behavior)
- `:atkinson`
- `:jarvis`

Add the remaining algorithms after the core mask/error-diffusion path is stable:

- `:floyd_steinberg`
- `:stucki`
- `:burkes`
- `:sierra`

## Semantic dither model

Dithering should be scene-aware. Build a per-output-pixel policy mask from the
render scene rather than applying one algorithm to the entire final frame.

Initial policy classes:

```rust
enum DitherPolicy {
    Protected, // quantize directly; do not accept or propagate diffusion error
    Dither,    // apply selected error diffusion algorithm
}
```

Classification rules for `scope: :auto`:

| Content | V1 policy | Notes |
| --- | --- | --- |
| Solid rect / rounded rect | Protected | Avoid noisy backgrounds/panels. |
| Solid border / border edges | Protected | Preserve crisp edges. |
| Text | Protected | Preserve readable glyphs. |
| Raster image | Dither | Main photo/content use case. |
| Gradient | Dither | Avoid banding on 1-bit/low-bit output. |
| Shadow / inset shadow | Dither | Soft continuous-tone content. |
| Image loading / failed placeholder | Protected | Simple generated UI. |
| SVG/vector image | Protected by default in V1 | Keep icons/logos crisp. Later allow complexity/override. |
| SVG with explicit tint/template color | Protected | Usually icon-like. |
| Video | Dither | If present in headless binary output. |

`scope: :all` ignores semantic protection except for transparent/out-of-bounds
pixels and dithers the whole frame.

`scope: :images` dithers only image/video/gradient/shadow regions.

`scope: :none` is equivalent to no error diffusion.

## Mask construction

Build the mask from `RenderState.scene.nodes` after rendering the RGBA frame and
before binary conversion.

Implementation direction:

- Add a new module, e.g. `native/emerge_skia/src/backend/headless/dither.rs`.
- Move packed pixel conversion out of `backend/headless/mod.rs` into that module.
- Pass dimensions and `&RenderScene`/root nodes into conversion:

```rust
convert_frame(
    rgba,
    width,
    height,
    &state.scene,
    &headless.pixel_format,
    &headless.bw1_polarity,
    &headless.dither,
)
```

Mask walk:

- Traverse `RenderNode`s in paint order.
- Apply clips/transforms enough for axis-aligned output masks in V1.
- Mark dither/protected rectangles for primitive bounds.
- Later tighten masks for rounded rects and glyph exact coverage if needed.

Important details:

- Painter's order matters: later protected primitives should overwrite earlier
  dither regions (e.g. black text on top of a photo).
- Start with a `Protected` mask so plain solid UI remains non-dithered.
- For `RenderPaintLayer`, classify from the layer content. If the layer is
  ditherable, mark its visual/bounds region ditherable; then recurse/overwrite
  protected children where needed.
- Use existing paint-layer metrics/content as a cache-friendly source, but do
  not require paint-layer caching to be enabled.

SVG/raster distinction:

- `DrawPrimitive::Image` currently carries `image_id` and `svg_tint` but not a
  direct asset kind.
- Add lightweight asset metadata lookup so mask construction can distinguish:
  - raster asset -> ditherable
  - vector asset -> protected by default
- Later: classify SVG complexity (`solid`, `gradient`, `embedded image`) and/or
  expose per-element dither hints.

## Error diffusion design

Implement grayscale error diffusion directly over the final RGBA frame plus the
policy mask.

For each output pixel:

1. Compute luma from RGBA using the existing integer approximation.
2. If `Protected`:
   - quantize directly to the target level.
   - ignore accumulated diffusion error.
   - do not propagate new error.
3. If `Dither`:
   - add accumulated error.
   - quantize to the target levels for `:bw1`, `:gray2`, or `:gray4`.
   - distribute error only to future pixels whose policy is `Dither`.

This prevents photo dithering error from corrupting text and solid UI.

V1 kernel definitions:

Jarvis-Judice-Ninke (`/ 48`):

```text
          X   7   5
  3   5   7   5   3
  1   3   5   3   1
```

Atkinson (`/ 8`):

```text
      X   1   1
  1   1   1
      1
```

Use fixed-point integer error buffers if practical. Floating point is acceptable
for first correctness implementation at 400x300, but fixed-point avoids target
FPU surprises and simplifies deterministic fixtures.

## Paint-layer-aware future optimization

V1 can build a mask and run error diffusion over the final frame. A later phase
can dither opaque paint-layer payloads once and reuse them via
`content_generation`, but only when safe:

- Layer is opaque or has a known backdrop.
- Layer content is image-like/continuous-tone.
- Dithered payload is consumed only by compatible low-bit output modes.

Do not pre-dither transparent layers before compositing in V1; dithering before
alpha compositing can produce incorrect results.

## Implementation phases

### Phase 1: Options and config plumbing

- Add `headless.dither` normalization in `lib/emerge_skia/options.ex`.
- Add native config structs in `native/emerge_skia/src/lib.rs`.
- Keep default behavior unchanged.
- Add option tests for atom and keyword forms.

### Phase 2: Conversion module extraction

- Move current `convert_frame`, `pack_gray`, and luma helpers from
  `backend/headless/mod.rs` to a dedicated headless output module.
- Preserve existing pixel-format tests.
- Add explicit `height` validation so mask and buffer sizes are checked.

### Phase 3: Error diffusion kernels

- Implement `:threshold`, `:atkinson`, and `:jarvis` for grayscale target levels.
- Support `:bw1`, `:gray2`, and `:gray4` first.
- Leave `:gray8` as either no-op luma or optional diffusion to 256 levels (no
  visible benefit); prefer no diffusion for V1.
- Add small deterministic unit fixtures.

### Phase 4: Scene-aware mask V1

- Build a policy mask from `RenderScene`/`RenderNode`s.
- Classify primitives using the table above.
- Add asset-kind metadata lookup so raster images and SVG/vector images can be
  treated differently.
- Add tests proving protected solid/text regions are not affected by nearby
  dithered regions.

### Phase 5: Headless render-loop integration

- Pass scene/dither config to binary conversion in the headless render loop.
- Keep latest-frame screenshot publishing as RGBA; dither applies only to
  delivered binary frame data.
- Ensure conversion errors are logged but do not crash the render thread.

### Phase 6: Trellis/name_badge example and docs

- Document Trellis recommended settings:

```elixir
headless: [
  target: display_pid,
  mode: :binary,
  pixel_format: :bw1,
  bw1_polarity: :one_is_white,
  dither: [algorithm: :jarvis, scope: :auto]
]
```

- Add a small GenServer example that receives `{:emerge_skia_frame, frame}` and
  calls `EInk.draw/3` with `frame["data"]`.
- Note that SVG support remains enabled for the Trellis profile.

## Tests

Unit tests:

- Dither option normalization.
- Current no-dither output remains unchanged.
- Jarvis/Atkinson produce deterministic output on small grayscale buffers.
- Error does not cross protected-mask pixels.
- `bw1_polarity` still works after dithering.
- Packed stride/byte sizes remain correct for odd widths.

Render/headless tests:

- Solid background remains solid under `dither: :jarvis`.
- Black text over white background remains thresholded/crisp.
- Raster image/gradient region uses diffusion.
- SVG/vector image defaults to protected.

Hardware/manual tests:

- Trellis/name_badge `400x300` full refresh with `:bw1`, `:one_is_white`,
  `:jarvis`.
- Compare text-heavy screens against no-dither threshold output.
- Compare photo/gallery screens against current `Dither.dither!(algorithm:
  :jarvis)` output.

## Minimal Trellis build compatibility

This plan assumes a separate or parallel cleanup to support a true
headless-raster-only build:

- `config :emerge, compiled_backends: []`
- no Wayland/DRM/GL/GBM/PRIME compiled for Trellis
- SVG support retained
- size profile can disable PDF/JPEG if the app does not use them

The dithering implementation must not pull in large image-processing crates or a
second Rust NIF, so it remains compatible with that minimal build.

## Open questions

- Should default `dither: :jarvis` ever be enabled automatically for `:bw1`, or
  should it always be opt-in? Prefer opt-in for compatibility.
- How should per-element dither hints be exposed in Emerge UI attrs?
- Should SVG default to protected forever, or should Emerge classify SVGs with
  gradients/embedded raster images as ditherable?
- Should text protection use approximate primitive bounds in V1 or exact glyph
  coverage from a separate mask render pass?
- Should error diffusion use serpentine scan in later versions for fewer
  directional artifacts?

## Validation before completion

- `cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check`
- `mix format --check-formatted`
- `cargo test --manifest-path native/emerge_skia/Cargo.toml --lib`
- `mix test`
- Trellis/name_badge manual smoke where available
- `git diff --check`
