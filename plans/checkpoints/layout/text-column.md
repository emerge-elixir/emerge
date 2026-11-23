# TextColumn: document blocks and shared floats

**Complete for `Emerge.UI.text_column/2` on admitted kinds.** Reference: `aef5e42fc789c396505f85c34810b2ca9c008586`; before: `4c3f57d` refused `renderer_upload failed: unsupported element 8 in first slice`. Each producer was paired with its own verified mapped NIF. No new API, wire encoding, producer behavior or submission protocol.

## Plan and implementation

1. **Freeze the reference.** Capture a left float, two paragraphs and a following block; remove the float and undo. Freeze 27 declarations at authored scales 1 and 1.5, including empty/float-only, both sides, stacked, centered, nested, bounded, inset, locally floated and clipped documents.
2. **T1: block flow.** The [document shape](../../../native/emerge_skia/src/tree/shapes/text_column.rs) reads existing tag 8. Keep the DSL defaults `width(fill())` and `height(content())`, honor explicit sizes, and measure the maximum child width and summed heights plus gaps and insets. Center a block in the available width; don't distribute height shares, vertical zones or even gaps. The intrinsic height includes the floats even if the paragraphs fit alongside them.
3. **T2: shared exclusions.** In [Reflow](../../../native/emerge_skia/src/tree/layers/document.rs), float at the first fitting band, stacking on the same side. A float's measured demand selects a band; its resolved box excludes space. Charge the authored gap between children, not the float's height to the ordinary cursor. Position a non-floating paragraph before its only line solve; use the [same float queries](../../../native/emerge_skia/src/tree/paragraph/floats.rs) as a standalone paragraph. Clear ordinary blocks below both sides. Use relative rectangles in Place, draw and hit testing; no second child order or provisional layout. A fixed-width document can center a late-packed ordinary WrappedColumn, but an unproved early width for a float still refuses.
4. **Retention and publication.** Reparenting a kept paragraph at equal width can change its exclusions. Measure, fitted size and final incoming site have different readers. Color and callback-only changes need no document reflow. [Native regressions](../../../native/emerge_skia/src/tree/tests/document.rs) cover net-zero edits, measured versus fitted float sizes, moved/closed branches, rejected edits, failed handoff, sealed B/accepted C, packed widths, and display scales 1/1.5/2/1. [Public tests](../../../test/emerge_skia/text_column_test.exs) check full frames, held presses, route-only edits, local coordinates, literal block placement and undo.
5. **Qualify** with whole-buffer, seated, allocation and named-suite checks below. No scrolling, editor, arbitrary inline widget, new listener, hardware GPU or timing claim.

## Frame comparisons

- **52/54** complete 176×196 document frames matched pinned `main`. The two exceptions were the same stacked-float overlap at scales 1 and 1.5. At scale 1 only a 20×17 area differed: `main` sorted the float over a later paragraph by Y; retained painted in declaration order. Each arm's two 53-state forward/undo histories matched its own cold frames at every submission: **2 mounts and 104 patches per arm**. The detached ordinary boxes are not claimed bit-identical: relative vs absolute f32 arithmetic differed by up to 0.000031px in these fixtures. `main` also omits some inline frames where retained supplies a continuation union.
- **68/100** additional height-policy frames matched pinned `main` exactly. All 100 agreed exactly on boxes reported by both implementations. The 32 differences were confined to unbounded fill heights on ordinary blocks (El, Row, Paragraph or nested TextColumn). Both gave the block `f32::MAX`; Skia's direct draw of such a rectangle dropped its background, whereas retained clipped the solid to the visible bounds first. Each of those 32 retained frames matched its **400px finite-height control** on both implementations. The [decision](../../../guides/internals/retained-decisions.md#d15-resolve-shared-float-bands-in-document-order) keeps visible backgrounds rather than emulating a dropped draw. No special paint branch was added.
- An additional **18** empty, inset and nested-float cases caught two errors: insets alone enlarged an explicitly short empty paragraph, and a paragraph-local float searched with its wrapped height instead of its measured demand. After repair, all **140** comparable rectangles matched exactly. **12/18** buffers matched; in the other six, every different pixel was the blue float on `main`. Making only that earlier float transparent produced **6/6** matches. A native edit/undo regression changes `min(fill(), content())` to `max(fill(), content())` at the same intrinsic offer: the float stays at y=4, but grows from 14.52px to 159.71px after wrapping.
- All **80** prior standalone-paragraph frames still matched their frozen references. The original alignment probe now renders 28/28 and matches 24 exactly, including `even_text_col`. The four older differences are `even_wrapped`, `overlap`, `wrap_center` and `wrap_line`.

## Submitted input

Private pixman Sway, 176×196 raster/CPU window at display scale 1. Six complete compositor crops on each arm matched the corresponding headless reference (the two implementations differ in the stacked state). Both recorded 12 actual `wl_pointer.button` events, raw press/releases in `handle_input/2`, routed messages in `handle_info/2` and a normal compositor close.

| State and surface point | Pinned `main` target (local) | Retained target (local) |
| --- | --- | --- |
| Beside the left float `(45,14)` | paragraph `(38,5)` | link `(3,3)` |
| Remove float during press; release `(12,12)` | paragraph `(5,5)` | same keyed link `(3,3)` |
| Stacked float overlapping later paragraph `(15,90)` | float `(8,5)` | paragraph `(8,55.16)` (rounded) |
| Clearing block `(52,89)` | block `(3,4)` | same |
| Right float `(83,8)` | float `(4,1)` | same |
| Restored left float `(10,8)` | float `(3,1)` | same |

## Qualification and replay

Release Cargo tests and strict Clippy passed: matched defaults + diagnostics **186**, headless **182**, Wayland **185**, Wayland+GL **186**, Wayland+Vulkan/deps **210**, Wayland-all **210**. All eleven extended seeded tests passed. The heap interposer measured `[0,0,0]` alloc/realloc/free for first settle **after reservation** and fitting edits through prepared draw/hit inputs, including reversal of 2,048 same-side floats. Initial reservation, one-over growth and forced rejection were separate controls, not claims about raster or BEAM allocation.

Mix warnings-as-errors compile, strict Credo, formatting and focused **93/93** passed. Full Mix: **803/873 passed** (15/15 doctests, 788/858 tests), **70 failed, 21 excluded**. Against `4c3f57d`: the TextColumn EMRG roundtrip was repaired and four new public tests pass; **no new named failure or exclusion**. Full Mix still returns 2; the named comparison passes.

Evidence: `/workspace/emerge-investigation-artifacts/text-column-v6Tlt4/`. See `PLAN.md`, `HOWTO.md`, `qualification/`, `cold-{ref,final}`, `sizing-{ref,final}`, `tall-{ref,final}` and `window-{main,candidate}`. The locked candidate source manifest has SHA-256 `7c83428d6c3f368e2167db6fd23b516121c70356d89e0f18dd99b78d22a1d361`; reference and installed candidate NIFs: `0dfe2f58020827b0965c19259012069c1fbcddb0c21ab44cef4fdba2c7a82735` and `a186b698171fa0535f217957f7bdaf9f87154bbc370d0a9dc2e0166c614c1b79`.
