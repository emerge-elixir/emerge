# Pixel bounds on content size

Implemented after [nested fill bounds](nested-main-axis-fill.md) (`0236e1b`):
one `content()` (also spelled `shrink()`) and pixel leaves. Allow either operand order and nested
`min`/`max` wherever width or height can already be content-sized. Keep mixed
content/fill and multiple variable leaves out of this slice. No wire or API
change.

- Fold the same pixel interval at admission but retain which basis it bounds.
  Content is the node's own padded demand, never a viewport offer or a share.
  Scale the authored pixels once at the node. Reuse existing final-site proof,
  64-level nesting limit, byte preservation and resource admission.
- Width uses intrinsic demand. Height must bound the actual reflowed demand
  after a width change, not freeze the pre-wrap height. A content-sized Row
  or Column still offers its fill children their intrinsic bases. A definite
  parent's split charges the bounded non-fill outer size first.
- In a 240px Row with no padding or gap, A `max(px(140), shrink())` and B
  `fill()` must split **140/100** for 80px of A content, then **180/60** for
  180px. Click at 160 must switch from B local 20 to A local 160 on successful
  submission, not on acceptance. Repeat on Column heights; exercise caps,
  crossed bounds, wrapping, scale, fitting allocation and failed admission.

## Results

- Before this change the three new [viewport tests](../test/emerge_skia/bounded_content_test.exs)
  failed on the mapped NIF with `renderer_upload failed: unsupported length 4`
  or `5`. All three now pass. They compare whole headless frames, including
  cold versus patch, and check raw buttons and application callbacks across
  submission. In a private Wayland CPU window all **10 compositor crops and
  seat clicks** confirmed the 240px example and cap/restoration on both axes.
- All **38** detached geometry cases met the expected origins and extents;
  **36** also matched pinned `main` (`5c71a593`). In the other two the second
  line is laid out at `y=24` but `main` still gives its host 20px, even with a
  60px cap on 44px of wrapped demand. The plain-content control and equal
  chip content/outer heights distinguish this from the old line-gap issue.
  See the [verified changelog candidate](retained-changelog-candidates.md#capped-wrapped-rows-use-the-height-of-all-lines).
- [Native regressions](../native/emerge_skia/src/tree/tests/content_bounds.rs)
  cover equal-output propagation, a refused handoff, original bytes,
  content-sized hosts offering intrinsic bases, closed moves under an El,
  promotion to root, and masked pixels that overflow only at the new scale.
  First settle after reservation, fitting edits and first settle after refused
  growth: **0 alloc / 0 realloc / 0 free** through paint and hit preparation.
- Six release Cargo/strict Clippy profiles passed: **75/78/79/103/103/79**
  (headless/shm/GL/Vulkan/both/matched). Focused Mix: **35 passed, 7 excluded**.
  Full Mix: **665/758 passed**, identical **93 failing names / 21 exclusions**.
  No GPU hardware, timing or scanout claim.

Evidence: `/workspace/emerge-investigation-artifacts/content-bounds-1dXxdx`
(`logs/{red-live,main-oracle,main-wrap-controls,final-oracle,final-window,heap,final-mix}`,
`logs/final-{test-*,clippy-*,mapped}`, `new.failures`, `repaired`).
