# Retained static sizing

**S1–S7 implemented.** The reference for the nine shared kinds is `main` at `aef5e42fc789c396505f85c34810b2ca9c008586`. `wrapped_column/2` is a retained addition, with literal expectations rather than a `main` oracle. No new public attribute, wire form, NIF or renderer setting.

The [layout passes](../native/emerge_skia/src/tree/layers.rs) run **Context → Measure → Flow → Reflow → Place**. Measuring the height of a branch at an independently known local width does not require a second Flow. [Constrained measurement](../native/emerge_skia/src/tree/layers/constrained.rs) uses the same length, group and line algorithms as ordinary layout. Flow publishes the width edges and breaks; Reflow uses the saved result only for matching inputs. A fixed-height paragraph can supply a scalar without breaking any lines. A direct document paragraph uses the document's final float bands. There is no previous-frame estimate, Place-to-Flow edge or iteration to convergence.

## Implementation order

### S1. Direct TextColumn blocks

An ordinary document block gets **unbounded available height**, not the viewport or a share. `Available` evaluates the child's policy; `Assigned` already includes it. For 200×100 art, `height(min(px(24), fill()))` and `height(px(24))` give 48×24. For a 24px-wide quarter-turned block the same cap gives 24×24; raising it to 80 changes the occupied width to 80 but leaves the following marker at y=24. A bare fill occupies `f32::MAX` × 24; there is no solid red in the reference crop. That observation is not a hit-test rule or a request for a bitmap that size. [Block offer](../native/emerge_skia/src/tree/layers/document.rs) · [geometry, finite caps and installation](../native/emerge_skia/src/tree/tests/document.rs).

### S2. Document and Paragraph floats

A float searches the band's free space with its **measured demand**, then excludes its **resolved occupied box**. Left/right floats do not inherit the document block's unbounded height. The turned float is 24×24; the capped Image float excludes 48px once its ratio is known, not the pending 64px. [Regression tests](../test/emerge_skia/static_size_test.exs) distinguish the side change from a layout or hit change at an ordinary block. A [kept move between documents](../native/emerge_skia/src/tree/tests/size.rs) also releases the old band.

Inline Text and El→Text remain fragments. A span's declared width can contribute to **paragraph preferred width**, but a 90×2 span doesn't make its words two pixels tall. The padded `AA BB CC` reference gives a 79.390625×23.359375 content paragraph, or 90×23.359375 with that declaration. At a 64px paragraph width the same words occupy two lines (46.71875px). Nine fixed/content/bounded controls matched on root geometry and full one-shot raster hashes. Retained additionally reports the direct inline owner's rectangle in detached layout and uses its installed continuation for hits ([D14](../guides/internals/retained-decisions.md#d14-break-paragraph-lines-once-and-submit-their-hit-regions-with-the-ink)).

### S3. Independent height offers and nested Column shares

A natural Row's cross offer is the tallest unallotted child, including a height measured at an independent width. WrappedRow uses that **single-row** basis, not the resulting stack. With an 8×20 header and the loaded 2:1 Image, the natural Row's peer starts at x=208; fixing the height to 40 moves it to 88. A Column assigns a share after insets, gaps, bounds and weights. Shares can pass through filling wrappers. In a 144px Column with two nested headers, changing the first from 8 to 24 moves the outer peer **256 → 224**, then back on undo. Zero is a real offer; mixed bounds evaluate at the host's own site. [Native share, resize and weight-swap controls](../native/emerge_skia/src/tree/tests/size.rs).

### S4. Column shares after independent sibling reflow

In a 100px Column, a 48px-wide `AA BB` / `AA BB CC DD EE` paragraph contributes **19.359375 / 58.078125px**, leaving **80.640625 / 41.921875px** for a height fill. A sibling that needs the very content width being derived contributes intrinsic height to that calculation; its **actual** fitted height allocates the placed group. Don't confuse content width with the width of the allocated child. The loaded Image keeps a 200px intrinsic minimum for its content Column, even when the placed Image is narrower. A turned fill-height box does not add its empty allocation to the Column's intrinsic width. With the 8px fixed header, the outer peers are at x=200 (Image) and x=8 (turn). [Submitted pixels, clip and clicks](../test/emerge_skia/static_size_test.exs).

### S5. Row charges and WrappedRow membership

The [fixed-local-width Row control](row-independent-turn.md#result) now uses the ordinary forward schedule. What matters is whether the non-fill **card** has a width independent of the share, not whether its width is a literal. The 48px paragraph's long form charges 58.078125px after a quarter turn; a fill peer in a 160px Row takes the remaining 101.921875px. With no room, the empty peer in a content Row takes zero. A nested independent split puts the outer peer at x=48. In WrappedRow, width 66 puts the 8px peer at (0,48); at 67 it fits on the first line and is fully blue from x=59. Membership uses the authored gap, then each group allocates its shares.

A fixed card width is **not** proof that every inner paragraph has its final width. In a turned `min(content(), fill())` card, if a height-filled Image takes width from a paragraph beside it, the later lines cannot set the height used to project that Image. Use the intrinsic height for this cap and charge and place the same occupied slot. In the 200px card with loaded 200×100 art, the outer peer is fully blue from x=61 or 22 below a short/long header in an 80px Column; at 180px both start at x=100, even with twenty `AA` words inside. The fixed-width paragraph whose lines **are** independent still supplies its fitted height. Both the detached boxes and exported blue edge matched `main` in the eight extra controls. [Order, actual line solves and inverse hits](../native/emerge_skia/src/tree/tests/transforms.rs).

### S6. WrappedColumn packing (retained addition)

Choose vertical groups from minima and settled non-fill charges at the height limit. Allocate **within each group**, then project the Image or turned cross extent; do not offer the sum of packed widths back to the cards. At height 60 with gaps x=6/y=4, a 48px-wide short/long paragraph and 20×20 card put the peer at **48 → 74 → 48**. [Packing tests](../native/emerge_skia/src/tree/tests/size.rs) cover content-bearing fills, a projected child inside a filling El, and a fixed-width card whose **inner** fill paragraph must wait for the Image's group share. Changing the 16px header to 8px changes the art/paragraph widths from 80/120 to 96/104 at one solve per edit. The independent [44/43px height threshold](../native/emerge_skia/src/tree/tests/wrapped_column.rs) remains supported.

### S7. Real feedback and the closing audit

The [final declaration fold](../native/emerge_skia/src/tree/rotation.rs) distinguishes a height that needs *a width* from a height that needs *this group's width*. It includes kept branches and affected ancestors outside an attrs-only patch target. A fixed-width paragraph under a width-fill wrapper can supply an independent height. A two-card width-filling WrappedRow can depend on width through line membership even if neither card's own height depends on width.

- **Shared Image feedback:** a content Column with two 75×20 header cards over a loaded 2:1 Image gives the outer peer x=200, as on `main`. The fixed-120/160 controls separate that intrinsic decision from actual line advance (see below).
- **Retained-only circular pack:** at width 30, two 30×21 cards need 60px of packed width; at 60 they need only 30. Reject the circular dependency **before publication**. An attrs-only sibling edit and a move of a closed pack leave the old tree, empty queues, installed pixels and hit intact. Declaring the width breaks the cycle; removing the second card also removes the variable group membership. [Atomic rejection and noncycles](../native/emerge_skia/src/tree/tests/size.rs).

## Reference differences

Of **51 shared declarations**, all were accepted. Detached comparisons use **unloaded** Image geometry. Separate 320×180, scale-1 `:await` exports use the 200×100 PNG with `decode_at_size: false`. The solid red/blue extents and counts and the opaque count matched in **48/51** exports; this is not a claim of full-frame identity. There were five further retained-only controls.

- **Line advance ([D7](../guides/internals/retained-decisions.md#d7-share-fill-widths-per-wrapped-line)):** retained advances by the tallest *outer* box. `main` uses the children's `content_height` and can overlap empty fixed boxes or clip a peer. The fixed-120 header with two empty 75×20 cards is 40px high here, 20 on `main`; at 160 both are 20. In the natural 160px WrappedRow with a 20px header, 200×100 Image and 8px peer, retained puts the Image at y=20 and the peer at 120. A literal-dimension Image makes this visible even detached: `main` puts them at 0 and 100, with the host ending at 100; retained ends at 128. The loaded export shows the same distinction. This is **not** a different aspect ratio.
- **Fractional coverage, not sizing:** the long Column turn starts at y=58.078125 on both renderers. At x=10, row 58 is fully red on `main` and red with alpha 236 here; the retained far edge at y=82 has alpha 20 rather than 0. Interior and peer geometry agree. Keep subpixel placement rather than changing layout to match a sampled edge.
- **Detached span sites:** six Paragraph float declarations additionally report the inline `phrase` rectangle on retained. Their sampled raster summaries agree. Neither the extra rectangle nor one-shot pixels imply a route through a running viewport.

## Gates for each slice

The [length × site matrix](../native/emerge_skia/src/tree/tests/dimensions/sites.rs) covers both axes, roots and every block parent, document blocks and both prose float hosts, with asymmetric insets and scales 1 and 1.5. Separate controls cover inline fragments, loaded versus pending Image and literal versus folded pixels. Snapshots, source arrival, closed moves, weight changes and undo exercise **accept edits → invalidate once against the final tree → recompute**. Test-only counters check actual line rebuilds and final widths; pass boundaries check empty earlier queues. The [heap interposer](../native/emerge_skia/src/tree/tests/allocations.rs) covers first reserved settle and fitting edits **through prepared draw/hit inputs**, not rasterization, installation or GPU submission.

## Linux qualification

Same frozen source on x86-64 Linux, locked offline release Cargo tests, `cargo clippy --all-targets -- -D warnings`, warning-clean Mix compile and pre/post checks of the loaded BEAMs and mapped NIF. Correctness used the shared performance lock; these were not timing runs.

| Build | Native tests passed | Focused Mix passed |
| --- | ---: | ---: |
| Wayland OpenGL | 280 | 274 |
| OpenGL + `stage-probes` | 280 | 274 |
| Wayland Vulkan | 304 | 274 |
| CPU-only | 276 | 274 |
| Embedded CPU (Linux host) | 276 | 274 |
| Wayland shm-only | 279 | 274 |

- Minimum Rust **1.91.0**: 280 native tests on default GL. Ordinary Mix, seed 17: **883/938 passed** (15/15 doctests, 868/923 tests), exit 2. Its **55 failures and 21 exclusions match by name** the pre-Size baseline (875/930 after restoring `main` module docs). The existing `tree_headroom_percent` documentation assertion is among them. All eight new ordinary headless tests passed. Rust and Mix formatting, `./ci-tests.sh quality`, Dialyzer, warning-clean ExDoc and local links in 67 generated pages passed. This does not close the unrelated package-screenshot and absent topology-target gates.
- All **17** opt-in [headless stage tests](../test/emerge_skia/stage_isolation_test.exs) passed. While the long pack was ready, and after its handoff was rejected, `(57,3)` remained blue and routed to `:peer` at `(9,3)`. Only a successful retry turned it red and routed to `:long` at `(3,3)`. Rejected *handoff* is separate from a cycle rejected at declaration admission.
- **Shm, OpenGL and Vulkan**, each on a private Wayland seat: **16** installed-capture/compositor-crop pairs at specified pixels and **20** routed clicks per API across scales 1 and 1.5. The 272×160 and 280×168 logical windows (420×252 buffer for the latter) covered document caps/float, nested headers and packed paragraph edits. The harness checked the resized window's compositor position before cropping. This is not inferred from detached boxes or exports.

Private reproduction: `/workspace/.emerge-size-plan-ZxB7mR/replay` retains the pinned reference and before-declarations; use a fresh output directory. `/workspace/.emerge-size-work-7SYSO6/` holds `verified/source.inputs` (SHA-256 `b8a0ce8594af7f425678a01d8bfe4f0c8fe0002e655a8bf0f3de16dbf353b600`), the `release-*` profile logs, `minimum-release`, `dev-release`, the `final-reference/` probes (including `main-outer-long.log` and `retained-outer-long.log`), `seat-{raster,opengl,vulkan}-final/observed.log`, `HOWTO.md` and `EVIDENCE.sha256`. The manifest identifies the product and tests before these Markdown reports.

Static Size on the nine shared kinds, plus the specified retained-only pack, is the scope of this result. [Inline layout rotation](inline-layout-rotation.md) and [transforms that preserve sibling layout](static-layout-transforms.md) have separate results. Animated lengths, Nearby, scrolling, editors, slider and video were outside Size. No module docs were changed for Size. No performance, Darwin/Metal, ARM/Nerves, retained DRM/KMS or physical scanout claim.
