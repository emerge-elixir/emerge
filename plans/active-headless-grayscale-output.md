# Headless grayscale follow-up

## Gray4

Extend the accepted BW1/Gray2 pipeline without regressing protected text/SVG,
white-background alpha composition, deterministic diffusion or row packing:

- Define two pixels per byte, high nibble first, levels `0..15` black to white.
- Restart each row; clear the unused low nibble for odd widths.
- Composite over white before nearest-level quantization; extend the boolean
  Atkinson option without receiving/propagating error through protected pixels.
- Test exact polarity, tails, alpha, quantization, determinism, text and SVG;
  verify Elixir metadata, stride and malformed-buffer checks.

## Gray8

- Deliver one opaque grayscale byte per pixel with stride equal to width.
- Avoid a redundant quantization/packing copy when rendered storage can be
  delivered directly; preserve ownership/lifetime safety.
- Keep dithering unsupported at source precision.
- Test alpha, stride, lifetime, text/SVG and deterministic output.

## Integration and acceptance

- Update format validation/docs only where declarations are incomplete; keep
  RGB/RGBA and PRIME unchanged.
- Compare peak memory and frame CPU cost against Gray2. No full-frame RGBA
  allocation in the raster Gray4/Gray8 output path.
- Exact bytes must cover even/odd widths and multiple rows; protected text/SVG
  remains directly quantized and isolated from diffusion error.
- Keep canonical output independent of controller plane formats/waveforms;
  panel-specific transport needs available hardware and explicit consent.
- Run `cargo test`, warnings-as-errors Clippy and `mix test` after implementation.

## Later low-memory work

Prioritize only measured dominant costs; the Gray8 framebuffer does not bound
asset, temporary-layer or consumer-mailbox memory.

- Evaluate direct Gray8 decoding of opaque JPEG/PNG assets for grayscale raster,
  with codec downsampling, output-sized limits and RGBA fallback for real alpha.
  Qualify existing asset budgets/eviction under a constrained-memory picture scene.
- Measure SVG variant-cache memory; use existing configurable limits/direct
  drawing first. Consider Gray8 variants only with semantic/pixel parity.
- Bound N32 temporary save-layer/filter allocations. Extend direct-alpha drawing
  only with exact pixel tests; retain useful transparency, shadows and SVG support.
- Design bounded binary-frame acknowledgement/admission and duplicate packed-frame
  suppression for slow consumers. Preserve deterministic output; consider changed
  regions separately without coupling renderer contracts to a panel waveform.

Current asset ownership and copy measurements are maintained in
[asset internals](../guides/internals/assets-images.md) and
[headless frame copies](../bench/headless_frame_copy.md).
