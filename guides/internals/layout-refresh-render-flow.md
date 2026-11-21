# Layout, Refresh, Render, and Cache Flow

This guide documents the runtime path from an Elixir tree update to rendered
pixels. It focuses on the Rust-side tree traversals, invalidation decisions, and
cache boundaries used by layout, refresh, render-scene construction, and the
renderer paint-layer cache.

## Current retained path: binary headless and Wayland

The ordinary session admits one [sparse EMRG v10 replacement](emrg-format.md)
against the preceding accepted declaration. It applies all accepted edits before
invalidating once and recomputing in dependency order. Keyed reparenting and
promotion preserve occurrence identity. Context, offers, placement, clip and
hits are checked against the new site. The raster separates preparation from
installation. Headless installs after binary frame delivery. Wayland installs
after attaching and committing a `wl_shm` buffer and flushing the connection,
or reusing actual pixels already submitted to that surface at that geometry.

Explicit hardware Wayland sessions use the same ordered fill/glyph commands.
Glyph outlines become triangles; color and coverage are produced on a Ganesh
multisampled device target. Before swap/present the display worker seals the
drawing, hit list and callback route changes. Only a successful handoff (or
reuse of a still-valid submission) publishes that candidate. A later edit
cannot change its addresses. Capture reads the retained submitted target on
demand, not the swapchain. Configure, a blocked acquire, and a frame callback
are not installations. See the [presenter decision](retained-decisions.md#d8-submit-the-same-retained-drawing-to-explicit-wayland-presenters)
and the [remaining hardware checks](../../plans/api-parity.md#outputs-platforms-and-distributions).

`Emerge.UI.scale/1` changes layout before measurement. Admission proves each
changed site's effective scale and provisions the font sizes used there. At
refresh a scale change propagates down the context pass; intrinsic sizes and
fit propagate back up. Fill offers remain in device pixels. A successful
submission installs the effective scale with the listener's origin and clip,
so a click is local to the box that was drawn, even across a keyed move.
Rows, columns and their wrapped forms clip ordinary descendants' paint and hits to
intersecting host frames. Clipping does not shrink the child or change the
wrapped-line height: spacing is measured between the laid-out child boxes.

The viewport offers the same width and height in device pixels as a
zero-padding El of that size, but it does not align the root. Its origin
stays `(0,0)`, even when a fixed-size root is smaller than the viewport. The element receiving the offer
doesn't inspect whether it is the root: on either axis, `fill()` and
`fill(3)` read the same available extent. A Row can instead **assign** a
child its share of the width, and a Column its share of the height. An
assignment is used as-is, not passed through the child's policy again.
Resizing the viewport or promoting a subtree replaces its incoming
constraints, not its child-placement rules. For example, a 100×64 root El
with padding 4, `center_x()` and `align_bottom()` puts a 20×20 child at
`(40,40)`. The same helpers on a root Row do not align its children. A
parent can use the root's preserved attributes again after demotion.
See [viewport sizing](../../test/emerge_skia/viewport_constraints_test.exs) and
[aligned root moves, frames and clicks](../../test/emerge_skia/root_alignment_test.exs).

Any Column can host ordinary fill-height children. Width flow and bottom-up reflow settle the
heights of non-fill siblings first, including wrapped lines. Placement splits
a fixed or allocated height after subtracting those outer heights, vertical
padding and gaps. A content-height column instead uses each fill child's own
intrinsic height. The outer offer and offset share one reserved entry for the
incoming edge; a nested column distributes its offer without scaling it again.

Direct children of a Column do **not** divide its width. In each child's
width expression every fill leaf, including `fill(3)`, reads the same inner
width of the Column. Content reads the child's own padded demand; pixel
leaves use the child's scale. A content floor can make a child wider than
its parent without changing a sibling's width or extending the parent clip.
The child resolves the expression in width flow, never a second time on an
already resolved offer from a Row. See the [Column child width regression](../../test/emerge_skia/column_cross_width_test.exs).

Direct children of a single-line Row likewise do **not** divide its height.
Width flow settles first. Each child reports its padded height after wrapping;
with auto or content height the Row fits the tallest such child plus its own
padding. Placement then uses the Row's actual inner height for every height-fill
leaf, not the intrinsic estimate or a previously installed height. For example,
with a 40×120 Row, padding 6, gap 4 and two 12px-wide children, both get a
108px offer. If A caps fill at 30px of content, its height is 30px (78px unused)
and B can still take all 108px. See the [wrap and overflow regressions](../../test/emerge_skia/row_cross_height_test.exs).

The single child of an `el` does not divide **either** axis. Width flow
resolves the child's width first. The child reports its height after wrapping;
placement then offers the El's actual inner height, including zero. For a
120×80 El with padding 6 and no gap, `fill(3)` / `fill(0.5)` gives the child
108×68 at `(6,6)`, with nothing unused. With 150×90 of content and
`max(content(), fill(3))` on both axes, the child is 150×90: it overflows
by 42×22. An offer is not a clip. The El keeps its inherited paint and hit
clip; an enclosing Column still clips at its own edge. See the [submitted
examples](../../test/emerge_skia/el_child_fill_test.exs).

El positions that resolved child independently on each axis: explicit child
alignment, otherwise the immediate El's explicit alignment, otherwise start.
Use the signed space remaining after padding and the child's outer extent.
Even a **horizontal** offset can read a new **height**: in a 64×43 El,
switching a WrappedColumn from `max(content(), fill(3))` to
`min(content(), fill(3))` can change its height from 44 to 43 and its packed
width from 12 to 30. A right-aligned box must move from x=52 to
x=34 even if its natural fit stays the same. [Frames, kept moves and local
clicks](../../test/emerge_skia/el_child_alignment_test.exs).

A Row divides spare width, and a Column divides spare height, in proportion
to the children's weights. `fill()` and `fill(1)` both take one share. A
weight-only edit invalidates the distributing parent, not the child's
intrinsic measurement. An unchanged offer stops propagation to that child.
A capped or floored child consumes its actual outer size. The fill siblings
that can still grow or shrink divide the remainder by weight. In a 120px Row,
`min(px(20), fill())` and `fill()` take **20px and 100px**; in a 30px Row they
take **15px each**. All caps can leave unused space, and unavoidable floors
can overflow. Neither case changes the host clip.

A plain Row with active [`space_evenly()`](retained-decisions.md#d12-resolve-row-widths-before-distributing-even-gaps)
charges no fixed gaps during that split. Place uses the **resolved widths** to
distribute only the remaining space between consecutive children. With a 100px
Row, padding 4 and children `20, fill(), 20`, switching from spacing 4 to even
moves the fill from x=28 to x=24 **and widens it from 44 to 52**. Its contents
may rewrap. Changing spacing mode doesn't change its intrinsic measurement or
publish its new origin before submission.

A WrappedRow chooses a line before distributing any of its space. Subtract
left and right host padding once. The next child fits when the measured outer
widths (including child padding) and the gaps **between** them fit that inner
width. An oversized first child still gets a line. Weights aren't an input to
this test. Row sends one group, WrappedRow sends a group per line, and Column
sends a height group to the same allocator. For each group it evaluates the
original child lengths, honoring any minimum supplied by the site.

In a 100×40 host with no padding and a 4px gap, two cards with 20×12
content take 48/48 at equal weights or 24/72 at 1:3, leaving 28px
below. At 44px the 1:3 pair takes 20/20; at 43px each gets its own 43px
line, leaving 12px below. A pixel cap limits the automatic minimum; an
explicit pixel floor replaces it. Both resolve at the child's scale. The
line split honors the cap as well as the floor: capping A at 30px in the
100px equal-weight case leaves 66px for B. With `min(content(), fill())`
on A, its preferred outer width caps the share; its minimum still decides the
break. Growing A's content from 20px to 40px changes **20/76 to 40/56** when
B can use the remainder. With `max(content(), fill())`, A's preferred width
is a floor: in a 67px row, **43 + 4 + 20** fits exactly, but growing A's
content by 1px puts B on its own line.

A width may contain several fills. While decoding, fold its guaranteed floor:
`px` and `content()` supply one, fill does not. For `min` both operands must
supply one; for `max` either suffices. Measurement applies that floor as a
clamp of preferred outer width after the intrinsic cache lookup. No guarantee
means keep the capped automatic minimum; a guaranteed **zero** replaces it.
Only after the line is chosen does the allocator evaluate the original width
at the common fill unit. For `max(min(fill(3), px(50)), fill())`, that means
`max(min(3u, 50), u)` at scale 1, not two sibling shares. Reflow uses the
chosen breaks and actual child heights.
See [why this differs from main](retained-decisions.md#d7-share-fill-widths-per-wrapped-line)
and the [submitted-frame regression](../../test/emerge_skia/wrapped_row_fill_test.exs).

A WrappedRow shares **width**, not height. In a fixed or allocated 64px-tall
host with 2px padding, every child's height-fill leaf reads 60px, on every
line, regardless of weight. Content reads that child's outer height **after**
width reflow. A content-sized host instead takes its cross-axis offer from the
tallest unallotted child, applying its own bounds and padding as for one Row.
It then fits the stack using the actual outer heights and gaps, without feeding
the packed height back as a second offer. Each new line starts below the tallest
resolved box on the preceding line; a host clip does not move that line up.
[Submitted heights, line origins and hits](../../test/emerge_skia/wrapped_row_height_test.exs).

The four linear containers use [one layout implementation](../../native/emerge_skia/src/tree/shapes/linear.rs).
It takes an axis and either the whole child slice or its next fitting prefix.
Horizontal breaks use width measurements in Flow. Vertical breaks use the
children's heights **after width reflow** and the actual inner height in Place.
Both advance by resolved outer boxes. On the cross axis, center/end alignment
uses the padded host for an unwrapped group, the tallest/widest child for a
wrapped group. It changes placement, not shares or membership. A natural
WrappedRow retains its single-row height offer for Place, not its grown stack.

On the main axis, start and end groups pack against the padded edges. Center
members and their internal gaps form one group, centered between the occupied
edges or starting immediately after the start group if space runs out. No gap
is charged twice at a zone boundary. Place uses the already resolved sizes
and does not repartition or sort the children; overlaps draw and hit in
[declaration order](retained-decisions.md#d11-place-alignment-groups-without-changing-stacking).

A WrappedColumn fits the **sum** of its occupied column widths plus gaps.
In a content-width host with two 12×20 cards and `spacing_xy(6, 4)`, a height
of 44px takes 12px of width; 43px takes 30px. Child width offers in a natural
host come from one column; packing does not feed that larger width back into
its children. If the packed width is needed in width Flow, Measure can use
pixel-height children or intrinsically measured leaves and an independently
known height. For a pixel-height Column whose slots are pixels or fills that
do not read content, Context computes the shared unit once from the **final**
declarations, and Place reuses it. A content-width child of a fixed-width
Column can instead pack late, using its actual assigned height. Unproved
early dependencies are refused before commit, never resolved from an old frame.
[Dependency and cycle tests](../../native/emerge_skia/src/tree/tests/wrapped_column.rs) ·
[delivered frames and local clicks](../../test/emerge_skia/wrapped_column_test.exs).

Nested `min`/`max` can put both a floor and a cap on one fill. With no padding
or gap, `min(px(50), max(px(20), fill()))` next to `fill()` takes 20/10 in a
30px host, 40/40 in an 80px host, and 50/70 in a 120px host. Nothing is left
unused in these examples. Nesting order matters: `min(20, max(80, share))`
always gives 20, while `max(80, min(20, share))` always gives 80. The renderer
folds these single-variable bounds when accepting the declaration; layout
uses the resulting interval.

With `content()` (also `shrink()`) instead of a fill, these bounds apply to
**the node’s own outer size, including padding**. In a 240px Row with no gap
or padding, A `max(px(140), shrink())` beside B `fill()` takes 140/100 when
A needs 80px, and 180/60 when A needs 180px. A Column uses the actual height
after wrapping, then gives the fill sibling the remaining space. A content
bound is not a fill weight or an offer from the viewport.

A limit can itself depend on content. In a 120px Row with no padding or gap,
`min(content(), fill())` next to `fill()` gives **20/100** when the first
child needs 20px and **60/60** when it needs 90px. Neither leaves unused
space. In a definite Column the analogous content limit uses the height
**after wrapping**. When an expression cannot be folded to a single weight
and interval, the allocator evaluates its validated length bytes at a shared
fill unit; each active term contributes either a fixed extent or a weight.
See the [width/height regression cases](../../test/emerge_skia/dimension_regression_test.exs).

Wayland uses the preferred fractional scale with a logical viewport destination
when both protocols are available, or integer buffer scaling otherwise. Layout
uses the nominal factor; seat input uses the submitted buffer's actual width
and height ratios after rounding. A local scale edit does not resize the window.

## Reference actor path

The following sections describe the pre-cut actor/presenter reference, not a
second producer running alongside the retained renderer. In that path:

- `EmergeSkia.upload_tree/2` sends a complete EMRG tree.
- `EmergeSkia.patch_tree/3` sends incremental EMRG patch operations.
- Input, scroll, resize, focus, text edit, slider, image, and animation events
  also become `TreeMsg` values.

The high-level runtime flow is:

```text
Elixir tree upload or patch
  -> TreeMsg batch
  -> TreeUpdateEngine::process_messages
  -> optional frame attr preparation
  -> refresh decision
     -> Skip
     -> RegistryUpdate
     -> RefreshOnly
     -> Recompute layout + refresh
  -> LayoutOutput
  -> publish_layout_output
  -> RenderMsg::Scene
  -> backend render loop
  -> Skia surface flush/present
```

The tree actor owns mutation of `ElementTree`. The renderer never mutates the
tree. The renderer receives a `RenderScene`, renders it, and owns the GPU/CPU
payload cache for paint layers.

## Update Batching

`spawn_tree_actor_with_initial_tree` receives one `TreeMsg`, drains any queued
messages with `try_recv`, and sends the whole batch to
`TreeUpdateEngine::process_messages`.

`process_messages` first flattens nested `TreeMsg::Batch` values. An empty batch
returns `TreeUpdateEffect::Skip`.

The update engine then folds the batch into a single update plan:

- Tree uploads replace the whole tree after EMRG decode.
- Tree patches decode patch operations and apply them to the existing
  `ElementTree`.
- Resize updates width, height, and scale.
- Scroll, drag, hover, focus, text input, slider, and asset-state messages are
  collected and applied once per batch.
- Linux presenters grant one successor sample after successful normal submission,
  using a separate bounded request lane. `SampleAnimation.sample_at` predicts a
  frame time without asserting presentation. Per-transition observation tokens
  gate enter/exit bootstrap; replies also acknowledge inactive/unchanged/error
  outcomes. See [frame pacing](frame-pacing.md).

Each message contributes a `TreeInvalidation`. The invalidation enum is ordered
from cheapest to most expensive:

| Invalidation | Meaning | Minimum work |
|--------------|---------|--------------|
| `None` | No tree, layout, registry, or paint output changed. | Skip. |
| `Registry` | Event registry or hit-test metadata changed. | Registry refresh or cached registry reply. |
| `Paint` | Geometry stayed valid, but render output changed. | Render-scene refresh. |
| `Resolve` | Intrinsic sizes are valid, but placement changed. | Layout resolve + refresh. |
| `Measure` | Intrinsic or subtree size may have changed. | Measure + resolve + refresh. |
| `Structure` | Topology changed. | Rebuild topology-sensitive caches, measure + resolve + refresh. |

The final invalidation is a join of every message and every sampled animation
effect.

## Refresh Decision

After mutations and animation sampling, the engine calls `decide_refresh_action`.

The decision inputs are:

- final invalidation
- whether the event actor requested a registry rebuild
- whether `TreeUpdateEngine.cached_rebuild` exists
- whether the tree already has a root layout frame

The decision is intentionally small:

```text
requires recompute                -> Recompute
Paint or Registry with root frame -> RefreshOnly
Paint or Registry without frame   -> Recompute
registry requested with cache     -> UseCachedRebuild
otherwise                         -> Skip
```

`UseCachedRebuild` sends only `TreeUpdateEffect::RegistryUpdate`. It does not
publish a render scene.

`RefreshOnly` builds a new `RenderScene` and either reuses the cached event
registry or rebuilds it if registry damage exists.

`Recompute` runs layout passes, then does the same refresh work.

## Shared Animation Contract

Regular, enter, exit and retained-value changes use one native animation core:

```elixir
Animation.animate([[width(px(40))], [width(fill())]], 1000, :linear)
Animation.change([width(content()), height(content())], 1000, :linear)
```

Width/height support pixels, content, fill and weighted fill on both axes. Static
`min`/`max` expressions still work, but are not animation endpoints or change
sources. A change call takes a list with shared timing; fields keep independent
policies and ordered overrides. Empty lists validate timing/curve then do nothing.
A later plain attribute clears only that field's policy.

First change-policy mount is immediate. A descendant text/child or intrinsic
font/image change can animate a retained `content()` dimension without changing
its declaration. Equal resolved targets and timing-only edits do not restart.
Interruptions start from the last successfully published presentation, not an
unpublished attempted frame.

Related finite layout owners resolve joint destinations but retain individual
clocks. An early finisher holds its complete allocation until joint release;
loops and paint-only fields do not become finite waiters. Allocation includes
intrinsic size, pool charges and parent obligations, not just the visible box.
Text wrapping, descendants, clipping, scroll ranges and hits use real layout.

Same-mount reparenting transports the published source with its original clock,
converts scale units and derives destination-role allocation obligations. Remounts
cannot inherit the removed mount's clocks or captures. This does not introduce
implicit position/FLIP animation. Exit ghosts capture published geometry, lose
handlers immediately, and have distinct terminal and structural-cleanup outputs.
An output may be superseded in transport; native publication is not proof that
every intermediate frame reached a display.

Continuous context changes and segment/repeat resets use native clock/footprint
witnesses. Identical input and sampling schedules replay consistently regardless
of query order; different context-dependent frame schedules need not converge to
the same intermediate trajectory. Failed attempts preserve published output and
first-write sources, including successful patch prefixes. Automatic pulse-only
retries back off to 250ms; external updates are not throttled.

## Frame Attribute Preparation

Preparation returns an exclusive `FrameAttrsPreparation`; it does not apply
candidate attrs to the published presentation. All three selection modes enter
the same transaction:

```text
prepare_frame_attrs_for_update
or prepare_animation_frame_attrs_for_update
or prepare_dirty_frame_attrs_for_update
  -> capture immutable fonts/assets and prepare native samples
  -> FrameAttrsPreparation.publish
     -> validate tree/runtime/attempt/context and preflight commit
     -> apply geometry and full/active/dirty effective attrs
     -> native layout/refresh: scene plus event registry
     -> commit clocks/sources/releases and ghost follow-up activity
```

Queries share one renderer-local workspace and do not hydrate assets or publish
output. Captured image layout facts and scene bindings agree; accepted scenes pin
their font/image inputs through deferred paint. Asset/font locks are not held
across layout or painting. Recoverable validation failures occur before candidate
presentation writes; native scene publication remains distinct from renderer
installation or compositor acknowledgment.

After preflight, the normal application traversal copies declared frame attributes
from `element.spec.declared`, overlays samples, scales them into
`layout.effective`, and normalizes extracted runtime state.

The incremental animation preparation path is used for steady animation pulses
when the tree already has frames and no transient animation entries require a
full pass. It prepares only active animation nodes unless a layout scale root
forces subtree preparation.

Refresh-only runtime and `SetAttrs` updates can use the same incremental
preparation machinery. The update engine tracks touched frame-attr ids for
mouse-over/down/focus, text-input runtime/content, slider runtime values, and
`SetAttrs` patches. If the final invalidation is still `Paint` or `Registry`
and the patch list contains only `SetAttrs`, preparation touches only those ids
plus active animation ids. Scroll-only refresh with no active animations skips
frame-attr preparation entirely because scroll state already lives in layout
state; scroll refresh with active animations prepares the active animation ids
without walking unrelated frame attrs. Structure, nearby insertion/removal,
measure, resolve, and transient animation cases keep the full preparation path.

Preparation marks refresh damage separately from layout damage:

- Animation effects that only change paint mark render refresh dirty.
- Effects that alter layout mark layout dirty and also mark refresh dirty.
- Registry refresh is only marked when the changed subtree can affect the event
  registry.

### Performance and Platform Limits

Paint/pixel fast paths avoid the dimension-query workspace where native geometry
does not require it. Resolved-length queries can still copy and evaluate the full
private model; sparse animation records do not imply sparse layout work or low
memory use. Large coupled/upward workloads have measured high costs. Functional
animation support is not a low-resource frame-time or memory-budget guarantee.

Linux native/raster integration tests do not qualify physical Wayland/DRM cadence,
GPU fault recovery, macOS execution or constrained devices. Native/Elixir artifacts
must agree on EMRG9 and the macOS14 handshake; incompatible hosts are rejected.
Input throughput/overflow policy and asynchronous local-edit redesign are separate
runtime work, not part of this animation contract.

## Layout Recompute

`run_layout_passes` is the layout recompute entry point. It performs the layout
tree traversals and refresh-affect bookkeeping:

```text
run_layout_passes
  -> mark_animation_layout_effects_dirty
  -> refresh_registry_subtree_affects_cache
  -> measure_element(root)
  -> resolve_element(root)
```

### Registry-Affects Prepass

`refresh_registry_subtree_affects_cache` computes whether each subtree can
affect the event registry. This lets registry refresh skip large neutral
subtrees later.

A subtree affects the registry when it contains interactive/event-visible state,
for example text inputs, sliders, focus/hover/down state, scrollbars, key
handlers, mouse handlers, virtual keys, or nearby mounts that can participate in
hit testing.

### Measure Traversal

The measure pass is bottom-up. It computes intrinsic sizes and measured frames
from pre-scaled effective attrs.

Important caches:

| Cache | Owner | Purpose |
|-------|-------|---------|
| `IntrinsicMeasureCache` | Node layout state | Reuse leaf/text/media intrinsic measurement when the intrinsic key and inherited font match. |
| `SubtreeMeasureCache` | Node layout state | Reuse measured subtree output when attrs, topology, constraints, and inherited font match. |

The measure pass uses dirty flags and cache keys so stable descendants do not
need full remeasurement when an unrelated sibling animates. For text, paragraph
fragments and measured text data are reused when the text/font key is unchanged.

When a cache hits, the pass restores the measured frames and associated
measurement outputs for the clean subtree. When a cache misses, it measures
children, computes content/fill sizing, records measured frames, and stores a
new cache entry.

### Resolve Traversal

The resolve pass is top-down. It takes measured sizes and resolves final
positions, scroll extents, render frames, nearby slots, and transformed geometry.

Important cache:

| Cache | Owner | Purpose |
|-------|-------|---------|
| `ResolveCache` | Node layout state | Reuse resolved frames, render frames, scroll maxima, and nearby placement when constraints, attrs, topology, and inherited font match. |

Resolve can reuse a clean parent while still visiting dirty descendants. When a
child changes size or placement, sibling and descendant frames can be shifted
instead of recomputing their full measurement.

The resolve pass also updates geometry that render and events need:

- `layout.frame`
- `layout.render_frame`
- `layout.scroll_x_max` and `layout.scroll_y_max`
- paragraph fragments
- nearby slot geometry
- transformed hit geometry

## Refresh

Refresh produces `LayoutOutput` from an already-laid-out tree:

```text
refresh or refresh_reusing_clean_registry
  -> build_scene: update affected retained paint/composition records
  -> registry_snapshot: update affected event records and ordering
  -> compare compiled registry identity with last admitted output
  -> derive IME from compiled input ownership
  -> record publication metadata and clear the consumed output journal
  -> LayoutOutput
```

`LayoutOutput` contains:

- `scene`: the render scene sent to the renderer
- `event_rebuild`: event registry payload when rebuilt
- `event_rebuild_changed`: whether to publish the registry payload
- IME state and cursor area
- `animations_active`

`refresh_reusing_clean_registry` compares actual compiled identity with the
last admitted `cached_rebuild`, rather than selecting a rebuild mode from dirty
severity. It borrows unchanged compiled metadata without cloning or scanning its
input maps. IME ownership is selected when the registry is materialized.

`layout_effect` records successfully admitted registry output. Explicit consumer
requests still publish a complete current payload with current mount/fence
metadata. A compiled-but-unadmitted candidate does not suppress retry publication.
Transaction validation, animation admission and final samples remain authoritative.

## Event Registry Maintenance

`events/registry_builder/retained.rs` maintains one event record per owner: inputs,
local contributions, child contexts, and bounded persistent normal/overlay order.
Touched paths update owners and actual context dependents. Unchanged inputs are
compared without cloning attrs or resolving unrelated slider geometry. Hover
ancestry compares compiled actions and geometry rather than encoded-attr hashes.
Context-only owners retain a geometry projection and routing records, without
unused runtime/content payloads or listener candidates. Interactive inputs remain
conservative; front Nearby roots still compile implicit blockers. Contribution
ranges preserve logical slots but omit references to neutral records; tree-owned
records and the existing affected-path journal still route first contributions,
including simultaneous context changes. Empty order/link storage is allocation-free. Resolved host state is retained only for Nearby edges, and
ordinary child context only where children exist; acquiring either edge kind
invalidates that context selection.

A changed contribution version materializes a complete flat registry, including
scroll, focus, slider and text-input metadata. This changed-output work is O(R)
in registry size; dispatch still uses the flat registry directly. Ordinary and
Nearby ordering, including deferred overlay draining, is preserved. Equal
contribution versions do not imply equal record ownership: neutral record
replacement must still release removed descendants.

The old registry subtree chunks, keys, budget and cached runtime-input scan were
deleted. An uncached traversal remains only as a test/benchmark semantic oracle.
Registry eligibility is maintained by per-owner contributions and ancestor counts;
structural changes can rebuild it. Geometry changes are recorded at their owners,
not rediscovered by a layout-wide before/after geometry-map comparison.

## Render-Scene Construction

Scene production updates retained owner output from laid-out elements:

```text
build_scene
  -> build_element_subtree(root)
  -> reuse, path-copy affected ports, or recompile an owner's changed inputs
  -> complete shared scene snapshot, including the retained Root boundary
```

`build_element_subtree` reads effective layout, inherited fonts, scene/clip context,
scroll state, and runtime visuals. It preserves culling, paint order and semantic
raster boundaries. Shared composition groups are transparent, not new raster
layers. Dirty flags never create or remove a semantic paint layer.

The result is a `RenderScene`; the renderer never traverses the `ElementTree`.

## Paint-Layer Boundaries

The stable semantic roles are:

| Reason | Boundary | Policy |
|--------|----------|--------|
| `Root` | Whole-scene composition shell, including top-level live Video. | `DirectOnly` |
| `Nearby` | One mounted Nearby root. | `Cacheable` |
| `ScrollContent` | Canonical scroll content; offset is a composition transform. | `Cacheable` |
| `Animation` | Declared transform/alpha-only animation. | `Cacheable` |
| `SliderValue` | Filled track and thumb of a valid three-slot Slider. | `Cacheable` |
| `DirectMedia` | Live Video below a cacheable semantic owner. | `DirectOnly` |

Ordinary rows, columns, elements, text, focus decoration, and custom subtrees
remain in-flow paint. Recursive media/focus inspection and damage-created
boundaries are not used.

Placement remains separate from payload contents. `Fixed` content uses scene
placement; `ScrollMoving` content keeps canonical payload coordinates and is
translated during composition.

### Ordered Scoped Content

A layer contains an ordered tree of:

- own paint runs;
- semantic child layers;
- clip, relaxed-clip, transform, alpha, and shadow-pass scopes;
- immutable shared composition groups, with fanout-eight ordering storage.

Own runs and child layers may interleave. A shared scope surrounds both exactly
once, preserving cases such as parent paint, a child layer, and later parent
paint under one group alpha. Each own run has a deterministic slot and its own
payload bounds. Child contents never enter the parent's payload. All adjacent own
paint between semantic child-layer boundaries coalesces into one run, including
ordinary Clip, RelaxedClip, Transform, Alpha, and Shadow scopes.

The scene retains the complete inherited host-clip list required for shadow
escapes, semantic boundaries, transforms, and payload correctness. Semantic child
layers and composition scopes containing child layers preserve their structural
boundaries. This avoids multiplying GPU payloads merely because ordinary own
content uses nested clip, transform, alpha, or shadow scopes.

## Retained Owner Output

Ordinary and Nearby owners use `tree/render/retained.rs::NodeOutput`; the old
Nearby fragment cache, keys and context hashes have been removed. Immutable
shared records allow complete snapshots to survive coalescing and replacement
without deltas or renderer acknowledgements. Paint never supplies IME side effects.

Owner-local command chunks and child ordering are retained independently. A
non-inherited paint edit reuses the child sequence and its port index. Missing-frame
and culled children keep a consuming slot in their parent; leaves/culled owners
need no child-order record of their own. Sparse replacements are grouped by path
before copying records and folding summaries, so a batch shares ancestor work.
Leading/trailing own spans remain outside the layer-containing middle: storage
balancing must not create extra raster runs. Structural or raster-boundary changes
still repartition explicitly. Unchanged descendant paint preserves scene identity.
Recursive construction returns shared owner-output handles, not singleton command
Vecs. Read-only child/paint-child/Nearby topology is borrowed. Local paint parts
are optional Shared handles; their bookkeeping lives in the owner record and is
borrowed on reuse. Port paths use ranges into one owner-local buffer, with linked
multiple uses per key; they retain arbitrary scope depth and local/escape uses.
Leaves cannot have escaping descendants and roots have already collected escapes,
so neither needs an escape carrier. Other owners retain that port even when its paint is empty; visibility
does not remove consuming slots, while topology changes can repartition them.

After fresh viewport culling, an already-culled ordinary non-root owner can reuse
its physically empty local/escape handles when identity, topology and non-spatial
consumed inputs still match. Missing frames are not evidence of culling. Pending
full or descendant output work blocks this shortcut: inline-child decorations can
change culling, and acknowledging a hidden mutation must not leave an older
context's proof reusable on an A→B→A transition. Reuse neither retags old context
proofs nor acknowledges output; first visible-to-culled retirement stays ordinary.

Resolved properties are retained with the owner. Descendant edits and decorative
paint changes with unchanged geometry do not rebuild clip contexts. Consumed paint
attributes and caret/scrollbar runtime inputs are projected rather than copying
unrelated handlers, interaction styles and source metadata. Command-input equality
is separate from placement, rotation, scale and opacity; those properties reuse
local commands when
visible geometry/dependencies permit. Paint contexts retain coordinate/culling
facts but do not build the event compiler's interaction-clip chains. They retain
the nearest self clip, complete normal clip shapes, and only scroll boundaries
for escaping shadows; Nearby clip inheritance remains separate. Leaf owners do
not construct unused child contexts. Paragraph
inline decorations affect culling even when the paragraph's own inputs match;
their journal still triggers property resolution and scope-dependent paint.

Run metrics/bounds/hash/shadow facts are derived together from local commands when
a run is prepared. An all-own sequence moves its command buffer into the run;
semantic own/child/own partitions remain unchanged. Content metadata folds those
facts and immediate Shared summaries without allocating own-node
views, recounting child layers or re-reading descendant primitives. Semantic
child-layer contents do not change parent own-payload facts. Folded transformed
bounds may be conservative; raw-command pixel controls cover clipping, reflections,
alpha and shadow escape. Generation fields supply payload keys; they are not
proof of executable-input equality. Slot assignment does not repeat metric
derivation. Clipping, paragraph shadow scoping, semantic partitioning and retained
port replacement preserve each prepared program together with its supplied image
arguments. A shared call must not reopen callee defaults when moved into a new
composition. Old snapshots and unchanged command buffers remain shared.

Necessary coordinate conversion, changed composition, resource acquisition and
retirement remain paid work. Passing functional tests does not establish a net
pipeline benefit or a cold-path win; measure both cold and incremental updates.

## Renderer Paint-Layer Cache

`RendererCacheManager` stores cacheable own runs as GPU images on GPU backends
and CPU images on raster paths. It traverses ordered content directly: a hit
composes the cached run at its exact position, a miss renders that run, and
semantic children recurse independently. `DirectOnly` runs always draw direct
but still compose cacheable children.

The payload key includes:

- typed semantic identity `(node_id, role)`;
- own-run slot and content generation;
- payload pixel size and scale/subpixel phase;
- font/image resource generation.

Video, loading placeholders, and failed media are ineligible payload resources.

The cache records:

- candidates and visible candidates
- low-value bypasses
- hits, misses, stores, and evictions
- rejected stores: ineligible, oversized, payload budget, fractional placement,
  unsupported transform
- payload residency: entries, bytes, GPU/CPU payload counts
- composition pixels: cached image draws, payload pixels, visible pixels, and
  waste ratio
- prepare time and cached-image hit draw time

### Admission and Bypass

Paint-layer scene construction says what may be cached. Renderer admission says
whether a concrete payload should be stored.

The renderer can bypass low-value payloads such as tiny cheap layers or large
simple layers where texture composition is likely more expensive than direct
drawing. It also rejects payloads that exceed entry or total byte budgets.

A first GPU payload for a semantic run may store immediately. A changed version
of an already-known run must remain exact and visible for a probation period
before another render-target snapshot is created. Rapidly changing runs draw
direct during probation, preventing interaction from creating a stream of
short-lived GPU surfaces. Stable replacements are admitted at most one per
frame, so related text, focus, and value runs that settle together cannot create
a burst of offscreen render passes. Once stored, a replacement evicts the
previous version for the same `(PaintLayerId, run slot)`; unrelated runs remain
resident.
CPU raster payloads do not need this GPU-specific probation.

Bypassing or deferring a payload does not change scene correctness. It only
means the layer's current own nodes are drawn directly for that frame while
semantic child layers still compose independently.

### GPU and Raster Behavior

The cache model is backend-neutral:

- GPU-backed backends store GPU render-target payloads and compose them as Skia
  images.
- Raster backend stores CPU raster payloads.
- Backends without cache tracking render the scene directly.

Correctness must not depend on Wayland-specific behavior. Wayland, DRM, macOS,
and raster paths should see the same scene semantics.

### Visible Execution Comparison

`can_skip_unchanged_visible_frame` compares actual installed order, commands,
text geometry and image arguments. It saves the current ready inputs **before
DRAW**, including when comparison returns false: A → B → B can return false,
false, true without drawing B. Readiness is still checked after a difference.
Unready input discards the saved comparison. Backend failure, missing drawable,
resize and context/resource invalidation call
`invalidate_visible_execution_inputs`; this is not a last-successful-frame API.

Current scenes and in-flight acquisitions own resources. Saved image selections
own only nonpixel selectors and weak headers. Lookup distinguishes no active scope
from a scoped miss and a scoped value; a scoped miss cannot select a later live
source. Captured vector variants win over matching global variants. Later global
variants retain their own epoch/revision/pixel validation and in-flight pins.

## Dirty Flags and Cache Invalidations

`NodeRefreshState` tracks render and registry damage:

```text
render_dirty
render_descendant_dirty
registry_dirty
registry_descendant_dirty
rendered
events
registry_subtree_affects
registry_own_affects
registry_contributions
paint_generation
```

Marking render dirty:

- increments `paint_generation` once for the dirty cycle
- records the origin/ancestor edges in the touched-output journal
- sets `render_dirty` on the origin
- bubbles `render_descendant_dirty` to ancestors

Marking registry dirty:

- records the same output path for independent event maintenance
- sets `registry_dirty` on the origin
- bubbles `registry_descendant_dirty` to ancestors

Owner records remain available for value comparison and structural sharing;
dirty marking does not select a separate cache engine. `clear_refresh_dirty`
updates source bindings before consuming the journal after both output compilers.
Paint-only diagnostic construction clears render flags but leaves the journal
for event compilation, even when no coarse registry damage flag was set.

### Output and Scratch Lifecycles

1. Request: mutations enqueue owners/edges in `ElementTree::output_changes`.
2. Prepare: exclusive attempt scratch selects work; failed/abandoned preparation
   may return its capacity without clearing pending output.
3. Produce: paint and event compilers maintain independent retained candidates.
   Render port/context leases and event geometry scratch have their own borrowing
   lifetimes; they are not a generic pool or additional dirty authorities.
4. Acknowledge tree work: `clear_refresh_dirty` follows both output compilers.
   `OutputJournal::mark_work` records routing bits, not output completion; `reset`
   clears only that instance's membership. Tree clear also resets its journal.
5. Publish/admit: registry publication compares with the last admitted version.
   Failed/deferred delivery and coalescing do not turn compiled output into an
   acknowledgement; metadata-only publication still participates in this protocol.
6. Retire: immutable old scenes/registries survive independently. Dropping any
   scratch lease releases references and returns capacity, never acknowledges
   output or changes publication state.

## Common Update Scenarios

### Registry-Only Request

When the event actor requests a registry and layout/paint remain valid,
`UseCachedRebuild` sends `TreeUpdateEffect::RegistryUpdate` using the current
retained candidate. This plan requires neither layout nor scene construction;
its historical action name does not bypass changed input metadata.

### Paint-Only Hover or Focus

Hover/focus state can change attrs. Preparation applies interaction styles and
marks paint and possibly registry damage. If geometry is unchanged, the engine
uses `RefreshOnly`.

Expected traversals:

- frame attr preparation for affected state
- render-scene construction
- retained event checks; affected-owner compilation and changed-version materialization
- render-node traversal in the renderer

Expected cache behavior:

- eligible unchanged paint payloads may hit
- changed paint payloads miss or bypass
- child paint layers survive parent redraws

### Layout Animation in a Scroll Container

For a layout-affecting animated row inside a scroll container, the row's size
and follower placement may change. Stable sibling sections and stable rows can
remain cached paint layers. Their contents do not need redraw just because their
position changes.

Expected traversals:

- incremental animation preparation when possible
- measure/resolve for dirty layout region
- render-scene construction with stable moving layer reuse
- renderer composition using cached payloads for stable sections

### Scroll

Scroll changes update scroll offsets and visible scene context. Layout frames
are usually still valid, so scroll can refresh without measure/resolve.

Expected cache behavior:

- stable scroll-moving layers keep their payloads
- layers that leave the viewport can remain resident within cache budgets
- layers that re-enter the viewport can hit if their payload key still matches

### Tree Patch

A patch can change attrs, text, structure, or nearby topology. Patch application
marks the weakest invalidation that preserves correctness.

Expected behavior:

- paint-only attr patch refreshes without layout
- geometry attr patch recomputes resolve or measure as needed
- structural patch invalidates topology-sensitive caches
- event-affecting patches update owning event records and dependent ordering/context

## Traversal Inventory

| Traversal | Entry point | Direction | Runs when | Primary cache |
|-----------|-------------|-----------|-----------|---------------|
| Message flattening | `push_tree_message_flat` | Message tree | Every batch | None |
| Patch application | `apply_patches` | Patch list plus affected nodes | Patch messages | Tree topology and dirty flags |
| Frame attr preparation | `prepare_frame_attrs_for_update` | Flat tree or active nodes | Dirty frame or animation sample | Animation samples |
| Registry eligibility | `refresh_registry_subtree_affects_cache` | Affected owners/ancestors; tree on structure changes | Layout recompute | Contribution counts |
| Measure | `measure_element` | Bottom-up | Recompute requiring measure | Intrinsic and subtree measure caches |
| Resolve | `resolve_element` | Top-down | Recompute requiring resolve | Resolve cache |
| Render scene | `build_element_subtree` | Affected owner paths and dependencies | Output production | Immutable owner records and ordering ports |
| Event registry | `registry_snapshot` | Affected owners/contexts and deferred Nearby order | Output production or explicit request | Contributions, persistent order and compiled flat version |
| Renderer scene draw | `render_nodes_with_cache_tracking` | Render-node tree | Every rendered scene | Renderer paint-layer payload cache |
| Visible hash/resource generation | `hash_visible_render_nodes` and resource-generation helpers | Render-node tree | Cache admission/keying | Font/image generation |
| Dirty cleanup | `clear_refresh_dirty` | Touched owners | After both output compilers | Shared output journal |

## Retained State and Cache Inventory

| State/cache | Stored on | Key inputs | Invalidated by | Reuses |
|-------|-----------|------------|----------------|--------|
| Intrinsic measure | Element layout state | Kind, text/media/font/input attrs | Measure/structure damage or key change | Leaf intrinsic size |
| Subtree measure | Element layout state | Attr subset, inherited font, constraints, topology | Measure/structure damage or key change | Measured subtree frames |
| Resolve | Element layout state | Resolve attrs, available space, constraints, topology | Resolve/measure/structure damage or key change | Final frames and scroll/nearby geometry |
| Event records/order | Element refresh state | Actual event inputs and contexts; topology | Changed inputs/dependencies | Immutable contributions and persistent order |
| Compiled flat registry | Element tree | Contribution version | Changed version | Complete flat output and compiled IME owner |
| Last admitted registry | `TreeUpdateEngine` | Last successful output | Successful replacement | Publication identity and explicit replies |
| Paint/composition records | Element refresh state | Owner inputs, topology, fonts, scene/clip context | Changed inputs/dependencies | Ordinary and Nearby immutable scene records |
| Declared-source index | Element tree | Touched declarations and structure epoch | Source-binding or structure changes | Immutable source lists |
| Renderer paint payload | Renderer cache manager | Semantic id, run slot/content hash, own generation, pixel size, scale, resource generation | Store budget, stale frames, resource/content change | GPU/CPU own-run image payload |

## Correctness Invariants

- Layout and render read `layout.effective`, not the unscaled declared attrs.
- Patches write unscaled declared attrs; preparation applies scale each frame.
- Paint-layer topology depends on semantic structure, never dirty state.
- Placement-only scroll, transform, and alpha changes do not alter own-run payloads.
- Own runs and child layers retain exact order under shared scopes.
- A child layer content change does not invalidate unrelated parent own runs.
- No cacheable own run contains Video.
- Event reuse depends on compiled values and context, not dirty severity alone.
- Reusing a compiled candidate must not skip publication after a failed transaction.
- Event dispatch reads only the flat registry, never retained scene/tree records.
- Renderer cache hits are an optimization only. Bypass or rejection must fall
  back to direct drawing without changing output.
- Cached layer bounds must include the visual content that belongs to the layer,
  but should not include unrelated parent clips except as composition clips.

## Source Map

Primary files:

- `native/emerge_skia/src/runtime/tree_actor.rs`: tree actor loop and output
  publication.
- `native/emerge_skia/src/runtime/tree_update.rs`: message batching,
  invalidation, refresh decision, and animation sync.
- `native/emerge_skia/src/tree/invalidation.rs`: invalidation ordering and
  refresh decision.
- `native/emerge_skia/src/tree/element.rs`: tree state, dirty flags, layout
  caches, output journal and retained owner records.
- `native/emerge_skia/src/tree/patch.rs`: patch application and patch
  invalidation.
- `native/emerge_skia/src/tree/layout.rs`: frame attr preparation, measure,
  resolve, refresh, and benchmark profiling entry points.
- `native/emerge_skia/src/events/registry_builder.rs` and `registry_builder/retained.rs`:
  event compilation, retained contributions/order and flat output; uncached test oracle.
- `native/emerge_skia/src/tree/render.rs` and `render/retained.rs`: owner scene
  production, path-copy updates and semantic paint-layer boundary construction.
- `native/emerge_skia/src/render_scene.rs`: render scene nodes, ordered scoped
  paint-layer content, own runs, semantic child layers, and metrics.
- `native/emerge_skia/src/renderer.rs`: render-node traversal, paint-layer
  payload cache, cache stats, and Skia drawing.
