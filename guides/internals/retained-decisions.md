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
invalidation and a second renderer for the supported subset. Route change only;
no speed claim. [Layout flow](layout-refresh-render-flow.md) · [work queue](../../native/emerge_skia/src/tree/queue.rs).

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
available width and height. An El offers each axis to its one child; a Row
splits width, a Column splits height. A parent-assigned share is not evaluated
as available space again. For static `min`/`max`, fold simple expressions on
admission; evaluate irreducible formulas from validated bytes, not a heap AST.
Redistribute actual charges after a cap/floor binds, and use the child's own
post-wrap content where the policy reads it. Otherwise a cap can leave space
beside a sibling that could use it, or cut a second line off. An offer is not
a clip: El doesn't clip; the four linear containers clip paint and hits to the
host, without changing the child's size. [Flow and examples](layout-refresh-render-flow.md) ·
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
pass. Integer-scale hardware frames/seat were exercised; see the tracker for
what that does not prove. [Presenter](../../native/emerge_skia/src/session/wayland/mod.rs) ·
[capture](../../native/emerge_skia/src/session/capture.rs).

### D9. Apply supported live controls directly

`main` queues native desired revisions and can supersede or fail device work.
For the retained title, input mask and frame cap, apply the snapshot under the
session lock, then acknowledge its session/revision. `config_status/1` can
therefore already say applied on return. That token is not proof of a new
frame. Do not generalize this shortcut to asynchronous device, backlight or
power operations. [Control path](../../native/emerge_skia/src/session/mod.rs) ·
[status tests](../../test/emerge_skia/config_control_test.exs).

### D10. Wrap columns at the resolved height

`wrapped_column/2` is **new**, not one of the 13 `main` kinds. Append v10 tag
14; use the same declaration order, group selection, allocator and placement
arithmetic as the other linear containers. Vertical membership needs the
post-width child heights and the actual height limit. Natural width fits the
occupied columns, not the widest single child. Cross-axis alignment is relative
to each group; it neither sizes the child nor changes where it breaks.

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
[Independent and circular cases](../../native/emerge_skia/src/tree/tests/wrapped_column.rs) ·
[submitted results](../../test/emerge_skia/wrapped_column_test.exs).
