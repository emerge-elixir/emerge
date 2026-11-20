# Changelog candidates: retained renderer vs main

This is a draft for release review, **not** the [product changelog](../../CHANGELOG.md).
Use **Added**, **Changed** or **Removed** for intentional public differences;
reserve `Unreleased → Fixed` for a reproduced `main` bug with an expected result
and a retained regression. A different implementation with the same result gets
a [decision](retained-decisions.md), not a release note. A limitation of this
branch stays in [API parity](../../plans/api-parity.md).

## Intentional changes

| Kind | Candidate release note | Decision / evidence |
| --- | --- | --- |
| Changed | `wrapped_row` now uses minimum widths to choose line breaks, then divides each line's available width by weight, respecting those minima. Two `fill()` children that used to occupy separate lines can now sit side by side. A pixel cap or `min(content(), fill())` leaves room for a peer on the same line. A content-capped card can wrap its own contents rather than force that peer to the next line; an explicit pixel floor can still move the peer. This is intentional, **not a bug fix**. | [D7](retained-decisions.md#d7-share-fill-widths-per-wrapped-line) · [48/48, 24/72 and the wrap threshold](../../test/emerge_skia/wrapped_row_fill_test.exs) |
| Changed | A globally keyed element of the same kind keeps its occurrence across parent moves or promotion to root; removing it from an accepted declaration ends that lifetime. A press can survive a move, not a removal/reinsert. | [D2](retained-decisions.md#d2-identify-occurrences-rather-than-matching-subtrees) · [promotion test](../../test/emerge_skia/viewport_constraints_test.exs) |
| Changed | Low-level EMRG clients must use v10 declarations/patch terms; v9 bytes and opcode lists are not accepted. Call `upload_tree/2` on a vacant renderer, then use the returned state with `patch_tree/3` even for a new root. Pass `nil` to clear; `none()` mounts an element. | [D3](retained-decisions.md#d3-replace-one-target-instead-of-sending-a-list-of-mutations) · [wire contract](emrg-format.md) |
| Changed | After a tree or callback edit, clicks still use the last successfully submitted pixels, listener geometry and route until the new frame is installed. Local coordinates themselves are **not** new to current `main`. | [D5](retained-decisions.md#d5-publish-input-with-the-successful-submission) · [pending edit and click](../../test/emerge_skia/retained_tree_slice_test.exs) |
| Added | Binary headless sessions accept raw pointer, key, focus and text-commit input via `EmergeSkia.send_input/2`; subscribers and element listeners use the normal dispatch path. | [D5](retained-decisions.md#d5-publish-input-with-the-successful-submission) |
| Added | `Config.Renderer.tree_headroom_percent` controls startup reservation for tree updates (default 80, range 0–1000). Oversized/deep declarations can be rejected without replacing the accepted tree. | [D4: bounds and scope](retained-decisions.md#d4-reserve-before-committing) |
| Removed | The low-level `EmergeSkia.Native.tree_node_count/1`, `tree_is_empty/1` and `tree_clear/1` bindings were removed. Use a stateful patch to clear. The separate test harness was not an application API. | [D5](retained-decisions.md#d5-publish-input-with-the-successful-submission) |
| Changed | Explicit Wayland GPU starts may fall back once to CPU shm on proven hardware unavailability, with a warning and an accurate requested/selected API report. This is not a silent switch after startup or a workaround for missing build features. | [D8](retained-decisions.md#d8-submit-the-same-retained-drawing-to-explicit-wayland-presenters) |
| Changed | On supported sessions, accepted title, input-mask and frame-cap revisions can be reported applied immediately. Configuration completion still does not acknowledge a new frame. | [D9](retained-decisions.md#d9-apply-supported-live-controls-directly) |

## Verified fix candidates

**Which `main`?** The before measurements below ran on pinned Emerge
`5c71a593acfcbb2675c3bd282c680633439e7745`. The API reference is current local
`main` at `aef5e42fc789c396505f85c34810b2ca9c008586`: source comparison of
`tree/layout.rs` and `tree/layout/` shows only the added `on_drag` field copy,
not a change to these sizing rules. This is **not** a new run of that revision.
Pinned clicks are not an oracle for local payloads; those controls used the
pre-cut `5b389e7f` **with its overlay**. The linked regressions run against the
retained renderer. Keep or refresh the before evidence if `main` moves.

| Candidate fix | Reproduction and observed reference → expected and delivered result |
| --- | --- |
| Scale default-size text with its layout. | At display scale 1 and local scale 1.5, implicit text stayed at 16 device pixels. It now matches an explicit logical 16 at that text site: **24 device pixels**. At scale 1 the frame is unchanged. [Regression](../../test/emerge_skia/layout_scale_test.exs) |
| Preserve the gap between wrapped lines of fixed-height children. | Chips of height 32 with padding 5, `none()` content and gap 8: the second line started at **y=18**, using content height. It should start at **y=40** (outer 32 + gap 8). Ordinary wrapping/pixels/hits: [regression](../../test/emerge_skia/retained_tree_slice_test.exs). |
| Give usable space to other fill siblings after a pixel limit binds. | In a 120px Row with no gap, `min(px(20), fill())` beside `fill()` gave **20/60** and wasted 40px; now **20/100**. At width 30 both get 15. A nested 20–50 bound at widths 30/120 gave 20/15 and 50/60; now **20/10** and **50/70**, also on the Column axis. [Bounds and hits](../../test/emerge_skia/bounded_fill_test.exs) |
| Redistribute after a *content* cap binds, too. | Same 120px Row, `min(content(), fill())` beside `fill()`, first child needs 20px: **20/60 → 20/100**. At 90px demand the nonbinding control stays 60/60. On a submitted edit, x=35 moves from B to A. [Mixed-length regression](../../test/emerge_skia/mixed_content_fill_test.exs) |
| Use all wrapped lines for a pixel-capped content height. | In an 80×120 Column, two 48×20 chips wrap with gap 4. `min(px(60), content())` kept their row at 20 although plain content got 44. It now gets **44** and the following fill starts at **y=48**, not 24. [Regression](../../test/emerge_skia/bounded_content_test.exs) |
| Honor a content minimum on the independent axis of a Column or Row child. | In a 120px-wide Column or 120px-tall Row, padding 6, inner offer 108, demand 150: `max(content(), fill(3))` stopped at **108**, though the `px(108)` floor control got 150. Both now give **150** (42 overflow); the peer still gets 108 and the host clip stays put. [Column width](../../test/emerge_skia/column_cross_width_test.exs) · [Row height](../../test/emerge_skia/row_cross_height_test.exs) |
| Honor the same floor on both axes of an El child. | A 120×80 El with padding 6 offers 108×68. At demand 150×90, `max(content(), fill(3))` returned **108×68** while pixel floors gave 150×90. Both now give **150×90** (42/22 overflow); the El does not clip the child. [Delivered frames and clicks](../../test/emerge_skia/el_child_fill_test.exs) |
| Recompute a Row child's content/fill cap when it wraps. | A 100×80 Row narrows to 99×80 (padding 2, gap 4, other child 12px wide). Its WrappedRow child narrows 80→79 and wraps two 36×12 chips. `min(content(), fill(3))` stayed at **16**; it now follows plain content **16→32→16**, including when widening again. [Regression](../../test/emerge_skia/row_cross_height_test.exs) |
| Recompute an El child's cap after wrapping, including in a content-sized parent. | An 84px-wide El narrows to 83, padding 2. Its 80→79px WrappedRow child wrapped but kept the cap at **16**. The cap now goes **16→32→16**; a content-sized enclosing El grows from **20 to 36**. [Regression](../../test/emerge_skia/el_child_fill_test.exs) |
| Recompute a viewport-sized WrappedRow's content/fill cap on resize. | 80×80 viewport, padding 2, gap 4, two 36×12 chips: at width 79 the second line moved to y=18 but the root stayed **16px** high. Its capped height now tracks **16→32→16** at widths 80→79→80. [Resize, pixels and input](../../test/emerge_skia/viewport_constraints_test.exs) |

A retained refusal before a port, or a clipping correction that restores
`main`'s pixels, is not evidence for a `main` fix. No candidate above has been
promoted by this documentation change.
