# Active Plan: Headless Gray4 and Gray8 Output

Status: BW1 and Gray2 implemented and accepted on Trellis; Gray4 and Gray8 remain

## Goal

Extend the accepted low-memory raster pipeline from BW1/Gray2 to exact packed
Gray4 and byte-per-pixel Gray8 output without regressing row packing, alpha
composition, protected text/SVG coverage, or deterministic dithering.

## Accepted foundation

- Skia renders packed-output scenes directly into opaque Gray8 CPU storage.
- Alpha is composited over white before quantization.
- Packed rows restart independently and clear unused tail bits.
- BW1 and Gray2 are MSB-first and accepted end-to-end on the UC8276 Trellis
  target.
- Atkinson diffusion is deterministic and excludes protected text/SVG coverage
  from both receiving and propagating error.
- Registered fonts and CPU-rendered SVG remain available in `embedded-cpu`
  builds.
- BW1 output uses full-frame partial refresh at approximately 1.71 FPS; Gray2
  full refresh runs at approximately 1 FPS.

## Remaining work

### 1. Gray4

- Define two pixels per byte, high nibble first, levels `0..15` from black to
  white.
- Restart packing per row and clear an unused low nibble for odd widths.
- Composite over white before nearest-level quantization.
- Extend the accepted boolean Atkinson option while preserving protected-pixel
  behavior.
- Add exact polarity, row-tail, alpha, quantization, determinism, text, and SVG
  tests.
- Verify Elixir metadata, stride, and malformed-buffer checks.

### 2. Gray8

- Deliver one opaque grayscale byte per pixel with row stride equal to width.
- Avoid a redundant quantization or packing copy when the rendered Gray8 storage
  can be delivered directly.
- Keep dithering unsupported because Gray8 is already the source precision.
- Add alpha, stride, ownership/lifetime, text, SVG, and deterministic-output
  tests.

### 3. Integration

- Extend `EmergeSkia` option validation and documentation only where the existing
  public format declarations are incomplete.
- Keep RGB/RGBA and PRIME paths unchanged.
- Measure peak memory and per-frame CPU cost against the accepted Gray2 path.
- Add panel-specific transport support only when hardware exists; renderer
  contracts must not encode controller plane formats or waveforms.

## Acceptance

- `cargo test`, warnings-as-errors Clippy, and `mix test` pass.
- Exact expected bytes cover even/odd widths and multiple rows.
- No full-frame RGBA allocation is introduced for raster Gray4/Gray8 output.
- Protected text/SVG coverage remains directly quantized and does not exchange
  diffusion error with surrounding raster content.
- Documentation describes canonical renderer output independently of any panel
  controller.
