# Nested pixel bounds around one fill

Implemented after the [redistributing main-axis split](bounded-main-axis-fill.md)
(`48b37bd`), without changing its sites, producer, EMRG tags or output boundary.
Row widths and Column heights, at the root or on a direct child, accept nested
`min`/`max` with exactly one `fill()` or `fill(w)` leaf and otherwise `px`
leaves, on either side. `content()` operands, multiple fills, and a whole
expression with no fill are separate features.

- Fold the prefix expression once during admission, with bounded depth and no
  heap AST. A pixel-only branch is a constant. The fill branch is its weight
  and an ordered interval: applying a constant to both ends preserves authored
  nesting even for conflicting bounds. Keep the original bytes. Validate the
  largest authored pixel, including discarded operands, at the final site.
- Resolve both ends at the child's scale. Use both thresholds in the existing
  allocator, not two independent splits. Content hosts still bound each
  intrinsic basis; a bounded root uses the viewport as its basis.
- Regress `min(px(50), max(px(20), fill()))` beside `fill()` in both axes.
  With zero padding and gap, containers of 30, 80 and 120px must give **20/10,
  40/40, 50/70** with nothing unused. Prove cold and sparse edits with whole
  frames and raw clicks; cover crossing bounds, weights, scaling, equivalent
  rewrites, malformed/deep input, refused edits and fitting heap counts.

## Results

- The [decoder](../native/emerge_skia/src/tree/wire/length.rs) reads each
  operand once. It accepts 64 nested operator levels in either direction;
  level 65 rejects before publication. Pixel-only branches may appear inside
  a fill expression. Crossing bounds retain their order: capping a floor of
  80 at 20 always gives 20; flooring a cap of 20 at 80 always gives 80.
- All 24 independent geometry expectations passed. Pinned `main` (`5c71a593`,
  separate verified NIF and producer) matches six; the other 18 show the
  known non-redistributing split. In the 120px example `main` leaves 10px
  unused (50/60). In 30px it overflows by 5px (20/15).
- Both new [headless regressions](../test/emerge_skia/bounded_fill_test.exs)
  failed on the prior mapped NIF with `unsupported length 5`. They now verify
  complete cold and edited frames and which child gets a click before and
  after submission. A private CPU Wayland window matched full compositor
  crops and **12 seat clicks**. No new GPU or timing claim.
- [Native regressions](../native/emerge_skia/src/tree/tests/nested.rs) cover
  equivalent rewrites and original bytes, refused handoff, root/content and
  nested allocating hosts, child scale, truncated/deep expressions, and a
  masked oversized pixel on a closed move or display change. Cold settle
  after reservation, fitting edits (including 64-deep rewrites), and the first
  settle after refused growth used **0 allocations / 0 reallocations / 0 frees**
  through paint and hit preparation.
- Release Cargo tests and strict Clippy passed in six profiles: **72/75/76/
  100/100/76** (headless/shm/GL/Vulkan/both/matched). Focused Mix: **32 passed,
  7 excluded**. Full Mix: **662/755 passed**, same **93 failing names / 21
  exclusions**. One initial byte test assumed full records arrived in depth-first
  order; it now compares the original attribute block and both trees' exports.
  An experimental Rust match guard was replaced with stable error handling.

Evidence: `/workspace/emerge-investigation-artifacts/nested-fill-TqBopY`
(`logs/{red-live,main-oracle,final-oracle,final-window,heap,final-mix}`,
`logs/final-{test-*,clippy-*,mapped}`, `new.failures`, `repaired`).
