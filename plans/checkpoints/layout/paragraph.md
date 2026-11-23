# Paragraph: declaration to submitted lines

**Complete for `Emerge.UI.paragraph/2`.** Pinned `main`:
`aef5e42fc789c396505f85c34810b2ca9c008586`. Before: retained `fe0ada6`
refused `renderer_upload failed: unsupported element 7 in first slice`.
Each reference used its own producer and mapped NIF. No change to the Elixir
producer, EMRG v10, public NIFs or submission protocol.

## The six sections and their exit witnesses

| Section | Result |
| --- | --- |
| 1. Reference and admission | Froze the refusal and eight complete 144×156 public RGBA frames. Registered existing kind 7. Validate multiline Text at the **final** direct/wrapped inline site, including closed moves; tag 8, malformed styles and invalid resource domains still refuse atomically. |
| 2. Measure, offer and reflow | [Paragraph](../../../native/emerge_skia/src/tree/shapes/paragraph.rs) measures an unwrapped demand. [Relative word fragments](../../../native/emerge_skia/src/tree/paragraph.rs) use the final offered width, source font, Unicode whitespace, span-boundary spaces, zero-width anchors, padding, border and line spacing. Empty, oversized, narrow and following-peer cases match `main`. |
| 3. Per-line alignment and local floats | Explicit paragraph Font → paragraph horizontal helper → inherited Font → leading, including the last line. Left/right floats finish an open line, stack on their own side and narrow only the band they occupy. A float receives an **available**, not assigned, width. An extra oracle caught a 104px floor lost at a 76px offer: changing the offer restored both the gradient and the clipped box. The 12 left/right float controls include px, fill, content, cap and floor. |
| 4. Styled runs and inline boxes | The [painter](../../../native/emerge_skia/src/tree/paint/decoration.rs) paints a complete El box per occupied line, with internal spaces, per-edge/corner borders, gradients, shadows and glow. Bundled styled glyphs, underline and strike use the source run. Outer shadows escape ordinary host clips; the host border follows its children. Dashed joins, outer shadow scope and fractional clipped edges each produced a pixel difference before correction. |
| 5. Installation and sparse changes | [Public regressions](../../../test/emerge_skia/paragraph_flow_test.exs) cover gaps, held presses, callback-only edits, reparenting, scale, float floors, rounded clipping and fresh vs. patched frames. [Native controls](../../../native/emerge_skia/src/tree/tests/prose.rs) cover failed handoff, B sealed/C accepted, equal-metric words, display-scale changes and a kept wrapper switching inline ↔ float at the same origin. Old/new fragment and material storage is reserved before acceptance. The allocation interposer saw `[0,0,0]` for first settle **after reservation** and fitting patch+settle, including prepared draw/hit inputs; initial reservation and forced rejection are separate. |
| 6. Qualification | All **80 cold frames** (8 original, 60 expanded at authored scales 1/1.5, 12 float-width controls) matched pinned `main` byte-for-byte. Both 59-state forward/undo histories matched the corresponding fresh reference at every submission: **2 mounts and 116 patches**. Real window and suite results below. |

## Same pixels, different link target

Private pixman Sway, scale 1, raster/CPU window, 160×144. Six complete
compositor crops per implementation matched the same six headless references.
Both viewports reported the raw left press/release at each point, then the
following application callbacks; both closed normally via `xdg_toplevel.close`.
The window includes a changing background marker so waiting for a crop proves
which state was submitted. The separate headless callback-only edit produced
identical pixels.

| Submitted state / surface point | `main` target (local) | Retained target (local) |
| --- | --- | --- |
| Second continuation `(53,41)` | paragraph `(48,36)` | link `(44,32)` |
| Wider while pressed; release `(53,13)` | paragraph `(48,8)` | link `(44,4)` |
| Renamed route `(53,13)` | paragraph `(48,8)` | renamed link `(44,4)` |
| Float `(12,12)` | float `(3,3)` | same |
| Rounded-off corner `(7,7)` | background `(7,7)` | same |
| Restored gap `(53,13)` | paragraph `(48,8)` | same |

This is an intentional input change, not pixel drift. The [decision](../../../guides/internals/retained-decisions.md#d14-break-paragraph-lines-once-and-submit-their-hit-regions-with-the-ink) describes the hit region and the coordinate origin.

## Qualification record

Under the shared performance lock, release `cargo test` and strict Clippy
passed on all six profiles: headless **175**, Wayland **178**, Wayland+GL
**179**, Wayland+Vulkan/deps **203**, Wayland-all **203**, matched default +
bench-diagnostics **179**. All eleven extended seeded checks passed. Mix
compile with warnings as errors, strict Credo and the **89/89** focused suite
passed. Full Mix: **798/869 passed** (15/15 doctests, 783/854 tests), **71
failed, 21 excluded**. Relative to `fe0ada6`: **22 formerly failing tests now
pass, four new public tests pass, no new named failure and identical named
exclusions**. Repairs: ParagraphBorder 8, ParagraphAlignment 3, Gradient 4,
EmrgRoundtrip 4, FixedChildLayout 2, BundledFonts 1. One remaining gradient
test includes SVG; it is not a paragraph-only fixture.

Evidence: `/workspace/emerge-investigation-artifacts/paragraph-8Hd6du/`.
`qualification/sources` SHA-256 `30112a6d8aedf58859ac0f96cd02bfa69e79f3965285e91e7edb315c648aa266`;
reference/candidate mapped libraries `0dfe2f58020827b0965c19259012069c1fbcddb0c21ab44cef4fdba2c7a82735` /
`f0932fc1144b99dc4d5f60be98c160332668528d6e3b30f65f89ab97fe0f17fb`.
`qualification/{failures,repaired,new.failures,exclusions}` contains the named
comparison. `matrix-{ref,final}`, `float-{ref,final}` and `window-{main,candidate}`
contain the captured results. Retain this checkpoint, not a selected work item.

**Stopping point:** direct Text and El→Text prose with local floats of admitted
kinds. No TextColumn/cross-paragraph floats, arbitrary embedded widgets, editor,
custom fonts, letter/word spacing, scrolling, animation or additional listener.
The GPU path consumes the same prepared positions; these fixtures qualify
raster, not new hardware GPU pixels, physical scanout or throughput. See the
[alignment roadmap](../../alignment.md) and [API inventory](../../api-parity.md).
