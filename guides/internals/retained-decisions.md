# Retained renderer decisions

These are standing decisions, not a list of ports. The comparison is with Emerge
`main` at `aef5e42f`; the [parity tracker](../../plans/api-parity.md) owns missing
behavior. When a port takes a different route or changes a result, record the
`main` contract, the choice, the reason, and a witness here. For a user-visible
difference, also add or update a [release-note candidate](retained-changelog-candidates.md).
An implementation difference alone is not a release note.

### D1. One producer, one invalidation

`main` feeds tree, event and render actors with layout/scene and registry
outputs. Retained accepts all edits first, invalidates **once against the final
tree**, then computes context ↓, measure ↑, flow ↓, reflow ↑, place ↓, paint
context ↓ and compose ↑. Shapes implement the work they own. One child order
serves layout, draw and hit testing. An unchanged value stops only its readers;
a sibling or a moved occurrence can still need work. This avoids both early
invalidation and a second renderer for the supported subset. On live binary
headless and Wayland the NIF applies each edit to the preceding **accepted**
declaration. The tree actor reads those shared records; it does not reapply
wire or borrow a mutable document from the renderer. A stalled consumer can
exhaust credit, but busy means neither accepted nor queued. [Layout flow](layout-refresh-render-flow.md) · [work queue](../../native/emerge_skia/src/tree/queue.rs).

### D2. Identify occurrences rather than matching subtrees

`main` requires global keys but its reconciler looks for a child among the
old children of that parent. Retained matches an explicit key **and kind**
anywhere in the preceding accepted declaration, including on promotion to
root. Unkeyed children match by kind at their ordinal under a matched parent.
Accepted absence ends an occurrence; reusing the key later does not revive a
press. Equal-looking siblings may share measured values or glyphs, never an
ID, incoming constraint, hit or callback. This makes moves continuous without
using geometry or hashes as identity. [Identity tests](../../test/emerge/stable_identity_test.exs) ·
[press across promotion](../../test/emerge_skia/viewport_constraints_test.exs).

The [snapshot reconciler](../../lib/emerge/engine/reconcile.ex) still visits the
new declaration. A matched key is not evidence that a row is unchanged. Reuse
its VNode only after forward targets settle and its resolved attrs (including
callbacks), attachment, inherited inputs and ordered children agree. The public
assigned tree still takes non-wire fields such as `frame` from the new declaration.

### D3. Replace one target instead of sending a list of mutations

`main` uses EMRG v9 full trees and separate patch opcodes. Retained uses zero
or one v10 `patch(target, subtree)`. A bare live ID keeps a subtree; a node
can keep its attributes or children independently. No move opcode, unchanged
subtree resend or v9 decoder. Native validates the whole replacement before
changing the tree, so a bad edit cannot partly remove an installed target.
`upload_tree/2` mounts into a vacant renderer; keep its returned state and use
`patch_tree/3` for later declarations, even to replace the root. `nil` clears;
`none()` is a live element; empty patch bytes mean no tree change. This is a
wire and low-level API break, not just compression: `Patch.diff/2` now yields
replacement terms (`Patch.patch/2`, `Patch.full/1`), and `DiffState`/`VNode`
carry occurrence indexes. Regenerate saved binaries with the matching producer.
[Wire format](emrg-format.md) ·
[decoder tests](../../native/emerge_skia/src/tree/tests/topology.rs).

### D4. Reserve before committing

`main` has no public tree-workspace setting. Rather than grow stores during
layout or paint-input preparation, check peak
old/candidate overlap, detached residents, glyphs and scratch before accepting
the edit. Startup-only `Config.Renderer.tree_headroom_percent` defaults to 80,
range 0–1000; fitting updates spend the reserve, not top it up. Input limits
include 64 MiB per wire body, one million nodes, root depth 0 through 512,
64 declared font sizes and 64 levels of length operators. A refused edit leaves
prior state usable. Zero heap calls were checked for first compute **after**
reservation and fitting edits through prepared draw/hit inputs, not for
provisioning, drawing, BEAM messages or display. [Configuration test](../../test/emerge_skia/config_control_test.exs) ·
[allocation boundary and controls](../../native/emerge_skia/src/tree/tests/allocations.rs).

The live [storage ledger](../../native/emerge_skia/src/tree/storage.rs) caps
declaration charges at 512 MiB, workspace at 1 GiB and inline media at 256 MiB.
These are separate ceilings, not estimates of process memory. A draft's cumulative
reservation must fit half the corresponding account. A fitting transaction
blocked by pinned readers returns busy; one that exceeds its limit is refused
permanently. Neither advances the accepted revision.

### D5. Publish input with the successful submission

`main` rebuilds event state through its actor pipeline. Retained prepares draw,
hits, local coordinates and callback changes together but installs them only
after binary delivery, a successful window handoff, or reuse of still-valid
submitted pixels. If A is visible, B is sealed and C is accepted, B's success
publishes B, not C; B's failure leaves A. A callback-only edit may change the
route without changing any pixel. Element-local click payloads already exist
on current `main`; the decision is about **which frame** they describe.
[Frame and click controls](../../test/emerge_skia/retained_tree_slice_test.exs) ·
[sealed route ordering](../../test/emerge/viewport_submit_routes_test.exs).

Select twice: skip superseded commands **before drawing**, then choose the
newest eligible **completed image** at handoff. If C is still drawing, submit
ready B; if both completed before the claim, submit C. The dispatcher installs
the claimed bundle's mapping and epoch, and retires its predecessor's callbacks
after their last event. Capture pins installed pixels, not the declaration.
[Stall witnesses and measured trade-offs](../../plans/checkpoints/performance/tree-owner-isolation.md).

Exercise that boundary through `EmergeSkia.send_input/2` on binary headless
or real seat input on Wayland. Unlike `main`'s test harness this cannot inject
a resolved element event, set an animation clock or drain the queue. The
old `Native.test_harness_*` bindings and PRIME test-only close helper are not
port targets. Neither are `Native.tree_node_count/1`, `tree_is_empty/1` and
`tree_clear/1`: inspect/layout or patch the same tree instead. Keep detached
codec/layout clients, not a second frame/hit API. The viewport event handler
and PRIME shutdown-result helper are private; positional native startup is an
internal ABI, not the application's `start/1` contract.

### D6. Resolve constraints at the site that offers the space

Instead of special-casing the root in layout, the viewport supplies ordinary
available width and height at a zero offset. Unlike a layout parent it never
reads root alignment for that offset. Keep the attributes: a root El can use
them for its child, and a future parent may use them to place a [demoted
box](../../test/emerge_skia/root_alignment_test.exs).

An El offers each axis to its one child; a Row splits width, a Column splits
height. A parent-assigned share is not evaluated
as available space again. For static `min`/`max`, fold simple expressions on
admission; evaluate irreducible formulas from validated bytes, not a heap AST.
Redistribute actual charges after a cap/floor binds, and use the child's own
post-wrap content where the policy reads it. Otherwise a cap can leave space
beside a sibling that could use it, or cut a second line off. An offer is not
a clip: El doesn't clip; the four linear containers clip paint and hits to the
host, without changing the child's size. Like `main`, an El resolves each
child alignment from the child first, then the immediate El, then start;
this is not an inherited text style. Place uses the child's actual outer box,
including late packed width, not its natural fit. [El alignment and moves](../../test/emerge_skia/el_child_alignment_test.exs) · [Flow and examples](layout-refresh-render-flow.md) ·
[main-axis regression](../../test/emerge_skia/mixed_content_fill_test.exs).

Subtree and display scale compose. Explicit font size resolves where declared;
implicit text is **16 logical pixels at the text site**, not the 16 device
pixels observed on the reference. Use actual bundled Skia metrics, not an
approximate width. Fractional window input uses the installed buffer/logical
extent ratio, not the requested nominal scale. These choices keep geometry,
glyphs and local coordinates together. [Scale regressions](../../test/emerge_skia/layout_scale_test.exs).

### D7. Share fill widths per wrapped line

**Intentional behavior change, not a fix to `main`.** In a 100px WrappedRow,
gap 4 and two 20px-content cards, `main` offered each fill child 100px; both
plain and 1:3 fills occupied separate lines. We chose the widths measured in
`mdgriffith/elm-ui` 1.1.8: form lines from minimum outer widths first, then
share each line by weight from zero bases, respecting those minima. Reflow uses
the same breaks. Equal fills take **48/48**; 1:3 takes **24/72**, not 34/62;
at 44px they take 20/20; at 43px each takes its own line. Keep Emerge's
leading vertical placement and line gap, not CSS cross-axis stretching.
Existing plain-fill layouts can change.

For a pixel bound on a fill, a cap lowers the automatic content minimum for
breaking; an explicit floor replaces that minimum, even at zero.
Unused width goes to uncapped siblings **on that line**. On `main` at `aef5e42f`,
a 30px cap gave A 30px on line one and B 100px on line two. With the elm-ui
rule the same unpadded cards take **30/66** on one line. A 10px cap on 20px
content fits beside the peer at 34px, not 33px; a 70px floor fits at 94px,
not 93px. Keep Emerge's outer-box units: with 6px left/right padding a 10px
cap is still 10px (as on `main`), not elm-ui's 12px padding minimum.

For `min(content(), fill())`, break at the child's minimum outer width and cap
its share at its **preferred** outer width (including padding, before wrapping).
On `main` at `aef5e42f`, two 20px chips with gap 4 made a 44px-wide card; a
fixed 20px peer fit beside it at 68px but wrapped at 67px. Retained keeps the
peer on that line at 67px: A gets 43px and wraps its chips instead. At 44px
both still fit at 20px each; at 43px the peer wraps. A content edit can change
the cap without changing the minimum, so the parent reads both measurements.

For `max(content(), fill())`, preferred outer width is instead a floor. In a
67px host with gap 4, A's two chips of 20px and 19px need 43px together. A
20px fill peer fits exactly. Grow the 19px chip to 20px: A now needs 44px,
so the peer wraps and both get 67px on their own lines. At `aef5e42f`, `main`
gave both cards 67px before **and** after the edit, with the peer at y=4.

Nested bounds compose around a single fill. For `min(px(50), max(content(), fill()))`
with a 64px host, gap 4 and a 20px-minimum peer, a 40px content floor fits:
A/B take **40/20**. Grow the content to 41px: B wraps to y=16 and A stops at
its 50px cap. `main` at `aef5e42f` drew A at 50px and B at `(0,4)` with width
64px in **both** states. `min(max(content(), px(10)), fill())` still breaks
at the capped automatic minimum, not at 10px: the fill can undercut the bound.

With any number of fill leaves, only a floor **guaranteed by the whole width**
replaces the automatic minimum. Pixels and preferred content guarantee a floor;
fill alone does not. `min` needs a guarantee from both operands, `max` from
at least one. Thus `max(fill(), fill(3))` keeps its automatic minimum, while
`max(px(0), max(fill(), fill(3)))` has an explicit zero. Every fill leaf in a
child's width uses the same line unit; the resulting width counts **once**.
In an 84px host with gap 4, A `max(min(fill(3), px(cap)), fill())` next to B
`fill()` takes **40/40** at cap 30 and **50/30** at cap 50 (20px content each).
`main` at `aef5e42f` put both on separate 84px lines at both caps.

Keep line formation separate from sharing: it reads child measurements, host
inner width and gaps, not lengths, weights or trial shares. Row, each wrapped
line and Column's height group use the same allocator on the authored lengths.
[Submitted frames and clicks](../../test/emerge_skia/wrapped_row_fill_test.exs) ·
[minimum on a moved child](../../native/emerge_skia/src/tree/tests/wrapped_width.rs).

Height is independent of those width shares. Like `main`, a definite host
offers its whole inner height to each fill child; a content host offers the
natural height of one row before packing the stack. Use the children's
post-wrap fits for that basis. Advance each line by its tallest **outer** box,
not just the content inside it. On `aef5e42f`, raising a child's height cap
from 16px to 32px left the next line at y=18, overlapping the taller box.
With 2px top padding and gap 4 it should move from **y=22 to y=38**. This
overlap correction is distinct from the intentional width-sharing change.
[Pixels and local clicks](../../test/emerge_skia/wrapped_row_height_test.exs).

### D8. Submit the same retained drawing to explicit Wayland presenters

Do not import `main`'s scene renderer or upload CPU-drawn glyph masks as a GPU
frame. Shm submits a CPU buffer. GL/Vulkan draw outlines as triangles on a
multisampled GPU target; keep the last successful target for capture on demand.
A configure, acquire or frame callback does not alone install output. `main`'s
window startup reserves raster fallback for `:auto`. Here, on proven hardware
unavailability **at startup**, even an explicit GPU request may use CPU shm
for the whole session and warn once. Report requested and selected APIs; a
missing build feature, ambiguous device or runtime failure is not that fallback.
This keeps output ownership explicit without calling software rendering a GPU
pass. Draw into independently owned targets; on handoff GL samples the
resolved share-group texture and Vulkan copies the finished image on a
separate transfer/present queue. Capture is not part of ordinary handoff.
The private AMD seat exercised integer scale and a 1.0→1.5 configure; neither
handoff nor a frame callback establishes physical scanout. [Presenter](../../native/emerge_skia/src/session/wayland/mod.rs) ·
[capture](../../native/emerge_skia/src/session/capture.rs).

### D9. Apply supported live controls directly

`main` queues native desired revisions and can supersede or fail device work.
For the retained title, input mask and frame cap, the dispatcher applies a
bounded control message, then acknowledges its session/revision. `config_status/1` can
therefore already say applied on return. That token is not proof of a new
frame. Do not generalize this shortcut to asynchronous device, backlight or
power operations. [Control path](../../native/emerge_skia/src/session/mailbox.rs) ·
[status tests](../../test/emerge_skia/config_control_test.exs).

### D10. Wrap columns at the resolved height

`wrapped_column/2` is **new**, not one of the 13 `main` kinds. Append v10 tag
14; use the same declaration order, group selection, allocator and placement
arithmetic as the other linear containers. Vertical membership needs the
post-width child heights and the actual height limit. Natural width fits the
occupied columns, not the widest single child. Cross-axis alignment is relative
to each group; it neither sizes the child nor changes where it breaks.

Share the arithmetic, not the schedule. Both wrap directions resolve an
explicit minimum at the child's scale; otherwise they keep the minimum from
that pass. Both compute the cross extent as the sum of group maxima with a
gap **between** groups. WrappedRow supplies the lines saved by Flow;
WrappedColumn supplies groups chosen at its height limit. The shared fold
never chooses breaks. [Geometry and rounding controls](../../native/emerge_skia/src/tree/tests/layout_rules.rs).

Only use a packed width in an earlier width split if its inputs can be proved
before that split. A 60px Column with a 16→17px header can independently give
its remaining **44→43px** to two 12×20 cards: they need **12→30px** of width,
and a sibling can rewrap at its new width. Contrast a content-width Column
with a width-sharing, wrapping header and two 30×21 cards (no gaps). At 30px
the header is 20px tall: the remaining 40px forces two columns needing 60px.
At 60px the header is 10px tall: the 50px left fits one 30px column. **Neither
width satisfies both constraints.** This is a retained example; `main` has no
WrappedColumn. Reject cycles and unproved early dependencies atomically. No
previous-frame estimate, one-column fallback or retry from Place to Flow.

A width edit can add or remove the only reader of a Column's height shares
without changing the child's intrinsic clamp. Compare the **authored** reader
before and after the edit, against the final parent. For example, in a 16px
Column, `min(px(80), fill())` takes the offered 16px. After switching to
`min(px(80), max(content(), fill()))`, two packed columns can require 30px;
right alignment then starts at −14px on both a sparse edit and a fresh upload.
[Independent and circular cases](../../native/emerge_skia/src/tree/tests/wrapped_column.rs) ·
[submitted results](../../test/emerge_skia/wrapped_column_test.exs).

### D11. Place alignment groups without changing stacking

Like `main`, anchor the end group at the padded edge and center the whole
middle group between the occupied ends. In a 100px row with padding and gap
4, widths 36/12/20 at start/center/end give origins **4/52/76**, not 4/44/76.
Use resolved outer extents after sharing. Reset the cursors on each line of a
WrappedRow or column of a WrappedColumn; alignment must not change the break.

Unlike `main`, alignment never changes stacking order. In a 30px Row, declare
an end-aligned red 20px card first and a start-aligned blue 20px card second.
At `(15,6)` `aef5e42f` painted **red** using its derived child order. Retained
paints **blue** and routes the click to blue at local `(15,6)`: later eligible
siblings win in the one declaration order, even on overflow. The comparison
with `main` is of pixels, not its seat routing. [Public frames and clicks](../../test/emerge_skia/linear_zones_test.exs) ·
[transposed geometry oracle](../../native/emerge_skia/src/tree/tests/dimensions/zones.rs).

### D12. Distribute even gaps after sizing each linear group

`space_evenly()` is a container policy, not a fourth child alignment. It
activates on a pixel or fill-bearing **main axis**. Auto, content and bounded
content keep their authored gaps and alignment zones, even at the same size:
`px(100)` and `max(px(100), content())` are not interchangeable.

All four kinds use [one `Linear` implementation](../../native/emerge_skia/src/tree/shapes/linear.rs).
Keep authored gaps in intrinsic demand and wrap membership. A content parent
can assign a fill Row or Column its natural **76px**, including padding 4 and
two 4px gaps around three 20px cards. In a wrapped group, two fills with 20px
minima and a 4px gap still need **44px** to fit together. Once they fit, active
even spacing charges **zero** fixed gap to the allocator: they receive 22px each,
not 20px each. Editing the authored gap to 5 makes them wrap at the same 44px.

After width allocation in Flow or height allocation in Place, distribute the
nonnegative remainder across `n − 1` gaps between **resolved outer boxes**.
Use the same forward cursor in declaration order, with no edge gaps; a
singleton leads. Cross alignment, spacing *between* groups, clipping and
stacking do not change. In particular, `main` ignores the flag on WrappedRow:
in a 100px padded row, x=`4/28/52` becomes **`4/40/76`** here, by design.
There is no WrappedColumn on `main`.

El, Text and None retain the flag but have no main axis to distribute. A
flag-only edit there schedules no layout or paint and leaves hit geometry unchanged.
In particular, an El doesn't pass its flag to a nested Row. [Submitted
no-op and kind replacement](../../test/emerge_skia/inactive_even_spacing_test.exs).

A Column's early height share must use the same charge as final placement.
Activating even spacing with a 1px authored gap can change the assigned
height **43→44**, its child's packed
width **30→12**, and a peer's width **34→52** before that peer wraps. This
uses the proven height group, not a previous-frame estimate or a layout retry.
[Row frames](../../test/emerge_skia/row_even_spacing_test.exs) ·
[Column and wrapped frames, kept moves and local clicks](../../test/emerge_skia/linear_even_spacing_test.exs) ·
[breaks, equal-size policy edits, rounding and failed handoff](../../native/emerge_skia/src/tree/tests/linear_even.rs).

### D13. Align single-line ink inside its settled Text box

Resolve local `Font` alignment → inherited alignment → left in Context, after
accepting edits. Alignment is not a strike resource or a glyph-cache key.
Only a Text with **omitted width** and non-left alignment takes the available
width instead of its natural run. This isn't a fill request: horizontal rows
still plan the measured slot. If a WrappedColumn needs an early packed width,
Measure uses the same child width rule as Flow, including when packed content
caps a fill share rather than setting the final width in Place. In a 120px Row,
a centered label can raise that cap from 49.765625px to above 60px: the two
children then take 60px each. Switching center ↔ right doesn't change the share.
[Native reader and publication controls](../../native/emerge_skia/src/tree/tests/text_align.rs).

Like `main`, use the signed space between the scaled insets, the run's visual
width and its left overhang. Keep the baseline, clip ink at the Text frame (not
at the padding), and hit-test the submitted box. In a 100×40 El with padding 4,
white `Hi` starts at x **6 / 44 / 82** for left / center / right; the fully
white bounds stay 8×12 at y=8. Explicit left restores the natural-width box.
The existing [default-size scale rule](retained-changelog-candidates.md#verified-fix-candidates)
still applies. [Public frames and local clicks](../../test/emerge_skia/text_alignment_test.exs).

### D14. Break paragraph lines once and submit their hit regions with the ink

Measure the preferred unwrapped width; in Reflow use the final width and the
vertical extent of each local float. Its width constraint is an **offer**, not
an assigned share: a `max(px(104), fill())` float can overflow a 76px band.
Align each finished line separately, including the last. An explicit paragraph
Font alignment overrides its horizontal box helper, then the inherited Font
applies. Neither rule positions the paragraph in its parent.

Each [fragment](../../native/emerge_skia/src/tree/paragraph.rs) names a range
in its Text run and its direct inline owner. Adjacent words of the same El on
one line share a decorated box; each continuation gets its own insets and
border. Place and both painters consume the relative result. A color-only edit
need not rewrap; a different word with equal metrics must not reuse stale
ink. A kept wrapper that becomes a float must relinquish its inline geometry
even at an unchanged origin. Admission reserves old and new fragments, scratch,
strikes, commands, clips and material bytes before committing. A prepared
drawing owns variable paint bytes once per contributing element, not per word.

**Intentional input difference:** use the submitted continuation as a hit region
and the listener's outer origin across all its lines for local coordinates;
the space between continuations is not a link. In matching compositor crops,
a press/release at `(53,41)` reached the paragraph at `(48,36)` on pinned
`main`; retained delivered the wrapped link at `(44,32)`. After widening the
paragraph during a press, release at `(53,13)` delivered that same link at
`(44,4)`. The float, rounded corner and gap controls agreed. [Public edits and
clicks](../../test/emerge_skia/paragraph_flow_test.exs) · [failed submission and
resource controls](../../native/emerge_skia/src/tree/tests/prose.rs) ·
[qualification and scope](../../plans/checkpoints/layout/paragraph.md).

### D15. Resolve shared float bands in document order

A TextColumn measures the widest block and the sum of block heights, authored
vertical gaps and insets. It offers widths before reflow, but does not allot
Column height shares. An explicit left/right child floats; each side stacks
independently. Floats don't advance the block cursor. A non-floating paragraph
can start alongside earlier floats; any other ordinary block clears below them.

[Document reflow](../../native/emerge_skia/src/tree/layers/document.rs) positions
the paragraph **before** running the same [line breaker](../../native/emerge_skia/src/tree/paragraph.rs)
against shared exclusions. Fragment positions remain paragraph-relative; the
float rectangles are document-relative. Search for a float's band with its
measured width and height, then exclude its final outer box. This matters for
a floating paragraph: content can grow after wrapping, but a pixel cap or a
fill-height float does not take all that wrapped height. A clean paragraph with no incoming
exclusions can reuse its relative result below a changed block. Do not solve a
provisional paragraph, sort children by resulting geometry, or use a previous
frame to decide where to break. Document child rectangles, including late packed
widths, are read directly in Place. Prepared drawing and hits still install together.

[D11](#d11-place-alignment-groups-without-changing-stacking) applies even where
a stacked float overlaps a later paragraph. At `(15,90)` on the private Wayland
seat, `main` painted and routed the blue float `(8,5)`; retained painted and
routed the later paragraph `(8,55.16)` (rounded here). For an **uncapped
fill-height block**, both implementations offer `f32::MAX`, not the viewport's
height. Retained clips solid backgrounds to their visible bounds before raster;
it does not make them disappear because the full rectangle is too large for
Skia. [Whole frames, seated observations and scope](../../plans/checkpoints/layout/text-column.md) ·
[public regressions](../../test/emerge_skia/text_column_test.exs).

### D16. Apply image opacity at the compositing boundary

`main` can request `save_layer_alpha(None, …)` for image opacity. `None` means no explicit content bounds, not a demonstrated full-screen allocation. Retained seals the sampled bitmap and fit with the drawing. A single ready source-over bitmap multiplies its paint alpha and draws once. A decorated image or overlapping subtree is not one bitmap: keep original opacities inside one bounded group and apply the group's opacity on restore. A precomposited snapshot qualifies as a bitmap; a list of cached commands does not. No retained paint-layer cache is implied.

The byte alpha is `round(clamp(alpha, 0, 1) * 255)`. Zero skips painting, not input; successful submission still installs hits. Bounds are hints: union paint extents including ink, translated children, shadows and sampling outsets, intersect the device clip and surface, round outward, map back to local space. The nearest media clip may bleed; an outer clip does not become relaxed. In the raster comparison, avoiding the intermediate surface changed 57 AA-edge pixels across eight half-opacity frames by **one level per channel at most**. Forcing isolation reproduced all 228 reference frames. Prefer the direct bitmap draw to reproducing that intermediate rounding.

Resource completion enters [D1](#d1-one-producer-one-invalidation) as a change to an immutable source. Keep the previously bound dimensions and pixels together while loading a replacement. Reject stale occurrence tickets; dimensions have layout readers, while equal-sized new pixels have only paint readers. An accepted edit, a decode and a successful presentation are three different events. [Resource and submission tests](../../native/emerge_skia/src/tree/tests/images.rs) · [layer counters and bounds](../../native/emerge_skia/src/tree/paint/opacity/tests.rs) · [frames and real input](../../plans/checkpoints/layout/raster-images.md).

### D17. Bind JPEG pixels in display orientation

Pinned `main` uses the encoded dimensions for layout and requests that size from an orientation-aware decode. A non-square 16×8 JPEG tagged for a quarter turn produced the failure indicator instead of a portrait. Retained decodes the encoded dimensions first, then applies EXIF orientation once on the asset worker. In full-resolution mode the immutable display-oriented bitmap supplies intrinsic width, height and texels to the tree. This keeps measurement, fit and hit geometry on the same coordinate system across source replacements; the tree does not need an EXIF transform. Reserve both pixel buffers for a transform, release the raw staging pixels before publishing, and charge cached or sealed display pixels until their last owner drops them. An upright JPEG takes the ordinary single-bitmap path. [All eight origins and budget tests](../../native/emerge_skia/src/raster_assets/tests.rs) · [full-frame comparisons](../../plans/checkpoints/layout/jpeg.md).

### D18. Fit decoded memory to the prepared drawing

A thumbnail's decoded dimensions are **not** its intrinsic size. In sized mode retain one immutable compressed record per live source identity. Use its display dimensions for layout and crop math. During preparation collect the largest fitted device-space demand of that source in the command stream; contain uses the drawn rectangle, cover the whole scaled image before cropping. Exclude sampling bleed and cap each axis at the display size. Scale the source crop as origin and extent, not by subtracting rounded rectangle edges. For an EXIF quarter turn, transpose the target into codec coordinates and turn the decoded pixels once.

Use an existing live or cached bitmap only if **both** axes suffice. Otherwise the asset worker decodes at a sufficient codec size and resamples to the exact target. Budget compressed data separately from resident pixels; count staging, old drawings and the new candidate while they coexist. The LRU holds at most one eligible bitmap per name, not a pyramid. With caching disabled an installed or sealed drawing still owns the pixels it uses. Neither a header nor a larger decode installs hits. Reject obsolete work; a successful in-flight decode may satisfy a new preparation that explicitly wants the same immutable record and target.

`main` can decode a smaller visible use and then a larger one in replay order. Retained binds the largest demand once **before** replay, so a small first use can look like `main`'s warmed draw rather than its cold first draw. Do not keep an extra size or perform codec work in replay to recreate that intermediate state. [Resource and crop tests](../../native/emerge_skia/src/raster_assets/tests.rs) · [memory, complete frames and real input](../../plans/checkpoints/layout/sized-rasters.md).

### D20. Share one Skia picture per SVG source on every renderer

We use AnyRender to record each resident SVG as a Skia picture so CPU and GPU renderers can reuse the same vector artwork across sizes and tints without storing a bitmap for each size.
