# Active Plan: Picture Asset Memory Validation

Status: issues #71/#72 implementation is being integrated; Trellis picture-scene acceptance pending

## Goal

Validate bounded, target-sized raster decoding on the 400x300 Trellis display
with several packaged photographs while reporting enough native data to explain
asset memory use.

## Scope

- Port the bounded raster LRU and target-sized decode changes from
  `../emerge-issues-71-72` onto the current headless branch.
- Replace the motion and opacity showcase scenes with one picture grid.
- Keep `:visual`, `:typography`, and `:pictures` scene selection in Solve.
- Log one record whenever a raster variant is decoded, including source path/id,
  encoded file bytes, source and decoded dimensions, decoded/source pixel ratio,
  decoded bytes, decoded/encoded byte ratio, retention status, and total retained
  raster-cache entries/bytes.
- Enable the memory log only for the name-badge picture application; keep the
  library default disabled.

## Acceptance

- Multiple photographs render correctly in BW1 and Gray2.
- Text/SVG protection and raster dithering remain unchanged.
- Each image is decoded near its fitted device-space size rather than source
  resolution.
- Cache totals stay within configured entry/byte limits.
- Repeated renders hit retained variants and do not emit repeated decode records.
- Rust, Elixir, EInk, and name-badge tests pass; the ARM release builds.
