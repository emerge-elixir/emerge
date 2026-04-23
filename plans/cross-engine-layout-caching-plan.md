# Cross-Engine Layout Caching Plan

## Goal

Capture the layout and invalidation ideas from Taffy, Yoga, Flutter, Slint,
Iced, and Servo that fit Emerge well.

This plan is intentionally Emerge-first. The point is not to copy another
engine's architecture wholesale, but to identify the patterns that best match
Emerge's retained tree, current layout pipeline, and renderer stats.

## Why This Matters

Current renderer stats show the repeated cost is layout, not event handling or
presentation:

- `layout_ms_avg` is roughly `2.5ms - 2.7ms`
- `render_ms_avg` is roughly `1.2ms - 1.5ms`
- `present_submit_ms_avg` and `event_resolve_ms_avg` are low

So the highest-value optimization work is to avoid unnecessary layout and reuse
more of the work we already did.

Today Emerge still recomputes too coarsely:

- the tree actor mainly reduces changes to a coarse `tree_changed` flag
- full recompute then starts from the root
- `refresh(tree)` exists as a downstream rebuild path, but layout caching does
  not yet feed it

Relevant Emerge files:

- `native/emerge_skia/src/runtime/tree_actor.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/attrs.rs`

## Emerge's Current Layout Shape

Emerge already has a good internal split:

1. prepare attrs for the current frame
2. measure intrinsic sizes
3. resolve geometry
4. rebuild render scene and event registry

That means Emerge does not need one giant cache. It should exploit this split
and cache the work at the correct level.

Key entry points today:

- `prepare_attrs_for_frame(...)`
- `measure_element(...)`
- `resolve_element(...)`
- `refresh(tree)`

All of these live in `native/emerge_skia/src/tree/layout.rs`.

## Engine Comparison

### Taffy

Most relevant files:

- `../taffy/src/tree/cache.rs`
- `../taffy/src/compute/mod.rs`
- `../taffy/src/tree/taffy_tree.rs`

Best ideas:

- cache per node rather than in one global memo map
- cache by layout inputs, not only by node identity
- keep measurement cache separate from final layout cache
- propagate dirtiness upward through ancestors only
- centralize cache lookup/store in one wrapper around recursive layout
- treat run modes as separate cache domains
- keep cache storage bounded rather than unbounded

What fits Emerge especially well:

- per-node measurement cache
- per-node resolved-layout cache
- ancestor-only invalidation
- centralized cache-aware measure/resolve entry points

### Yoga

Most relevant files:

- `/workspace/tmp-layout-engines/yoga/yoga/node/LayoutResults.h`
- `/workspace/tmp-layout-engines/yoga/yoga/algorithm/CalculateLayout.cpp`
- `/workspace/tmp-layout-engines/yoga/yoga/algorithm/Cache.cpp`
- `/workspace/tmp-layout-engines/yoga/yoga/node/Node.cpp`

Best ideas:

- dirtying is value-sensitive and idempotent
- dirtiness propagates upward only until an already-dirty ancestor
- layout cache invalidation depends on more than local dirtiness, including
  inherited environment inputs
- measurement cache reuse is compatibility-based, not only exact-key-based
- measure callbacks are normalized and skipped entirely when constraints already
  determine the size
- changed-subtree state is explicit through `hasNewLayout`

What fits Emerge especially well:

- idempotent dirty marking for layout-affecting changes
- width-constraint compatibility rules for text and paragraph caches
- a subtree "new layout" bit to skip downstream refresh traversal work
- measure fast paths when width or height is already fully determined

### Flutter

Most relevant files:

- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/object.dart`
- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/shifted_box.dart`
- `/workspace/tmp-layout-engines/flutter/packages/flutter/test/rendering/relayout_boundary_test.dart`

Best ideas:

- relayout boundaries are dependency boundaries, not static node types
- `parentUsesSize` explicitly decides whether child relayout must bubble upward
- parents call child layout unconditionally, while the child owns the fast-path
  bailout when constraints are unchanged and the child is clean
- layout invalidation and paint invalidation are separate concerns
- mode changes such as `sizedByParent` changing are treated as dependency-shape
  changes, not normal value changes

What fits Emerge especially well:

- a `parent_uses_child_size` concept during resolve/layout recursion
- relayout boundaries that stop upward invalidation where layout is isolated
- distinct layout vs paint dirty bits
- central child-level early-return logic on unchanged constraints

### Slint

Most relevant files:

- `/workspace/tmp-layout-engines/slint/internal/core/partial_renderer.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/properties.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/model/repeater.rs`
- `/workspace/tmp-layout-engines/slint/internal/interpreter/eval_layout.rs`

Best ideas:

- geometry caches and render-property dirtiness are tracked separately
- property tracking is lazy and dependency-based instead of relying only on
  manual broad invalidation
- repeaters keep stable incremental state across inserts, removes, and viewport
  movement
- dynamic children are materialized by layout when needed
- multi-phase layout avoids dependency cycles for height-for-width content

What fits Emerge especially well:

- repeater/list cache state should be separate from ordinary element layout
  caches
- paint-property dependency tracking can stay downstream of layout caching
- width-dependent text or paragraph layout should have a dedicated cache phase
- viewport-aware list invalidation can prevent broad recompute later

### Iced

Most relevant files:

- `/workspace/tmp-layout-engines/iced/core/src/widget/tree.rs`
- `/workspace/tmp-layout-engines/iced/core/src/shell.rs`
- `/workspace/tmp-layout-engines/iced/runtime/src/user_interface.rs`
- `/workspace/tmp-layout-engines/iced/widget/src/keyed/column.rs`
- `/workspace/tmp-layout-engines/iced/widget/src/lazy.rs`

Best ideas:

- preserve subtree state aggressively across diffs
- separate "layout invalid" from "widgets invalid"
- keyed child reconciliation preserves per-child state across edits
- lazy subtrees avoid unnecessary rebuild churn above layout

What fits Emerge especially well:

- preserve layout cache identity across patching and keyed child updates
- distinguish tree-structure invalidation from layout invalidation
- use keyed diffing ideas for repeaters and dynamic child lists

Iced is less useful for engine-level layout caching than the others, but it is
very useful for state continuity and avoiding layout churn upstream.

### Servo

Most relevant files:

- `/workspace/tmp-layout-engines/servo/components/shared/layout/layout_damage.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/traversal.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/layout_box_base.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/layout_impl.rs`

Best ideas:

- use separate damage bits instead of one dirty flag
- stop relayout propagation at real incremental boundaries
- preserve structure when possible and repair style/cache state in place
- keep intrinsic-size cache separate from subtree layout-result cache
- separate style, layout, overflow, and display-list invalidation phases

What fits Emerge especially well:

- more than one layout dirty class is required
- relayout boundaries should be explicit and enforced
- intrinsic measurement and resolved layout should be invalidated separately
- downstream refresh work should not always imply full geometry recompute

## Common Ideas Across Engines

The overlap between these systems is more important than any single engine.

### 1. One dirty bit is not enough

Every engine with strong incremental behavior separates kinds of invalidation.

At minimum Emerge should distinguish:

- paint-only invalidation
- resolve/layout invalidation
- intrinsic measurement invalidation
- tree-structure invalidation

Potential extension later:

- overflow or clip recompute invalidation
- event-registry rebuild invalidation

### 2. Measurement and final layout must be cached separately

This is reinforced by Taffy, Yoga, and Servo.

For Emerge the correct split is likely:

- intrinsic measurement cache
- resolved geometry cache
- downstream refresh/render/event rebuild output kept separate

### 3. Cache keys must be based on layout inputs

Useful inputs for Emerge cache keys:

- incoming parent constraint
- scale
- node kind
- inherited text or font context hash
- layout-affecting attrs hash
- child structure version
- nearby structure version
- run mode

### 4. Dirtiness should propagate upward, not globally

Changes should invalidate the changed node and then only the ancestors that
depend on it.

This is the core shift away from whole-tree recomputation.

### 5. There should be relayout boundaries

Borrowing from Flutter and Servo, some subtrees should own layout
independently enough that child relayout does not keep propagating upward.

### 6. State identity must survive list and subtree edits

Borrowing from Slint and Iced, layout caches are only useful if patching and
repeaters preserve enough identity for the caches to survive.

### 7. Paint invalidation must stay separate from layout invalidation

Borrowing from Flutter, Slint, and Servo, color/opacity/styling changes should
not force geometry recompute.

## Ideas That Fit Emerge Best

These are the highest-confidence additions for Emerge specifically.

### 1. Per-node cache ownership

Each `Element` should own or be associated with cache state for:

- intrinsic measurement
- width-sensitive text or paragraph measurement data
- resolved frame and content size
- child dependency versions
- subtree changed bits for downstream refresh

This could be either:

- fields directly on `Element`
- or a side table keyed by stable node identity

Direct ownership is easier to reason about, but a side table may be less
intrusive if the `Element` structure is already crowded.

### 2. Dirty classes instead of `tree_changed`

Suggested Emerge classes:

#### Paint-only

Examples:

- background color
- border color
- text color
- SVG color
- alpha

#### Resolve-only

Examples:

- alignment changes
- some position-only changes
- scroll offset if intrinsic sizes remain valid
- container distribution changes with unchanged child intrinsic sizes

#### Measure-and-resolve

Examples:

- text content changes
- font changes
- width, height, padding, spacing changes
- border width changes if it changes effective box size
- child list changes
- nearby list changes
- intrinsic image or video size changes

#### Structure

Examples:

- child insertion/removal/reorder
- repeater realization changes
- nearby tree topology changes

This classification should drive invalidation before caches are introduced.

### 3. Distinct cache layers

#### Intrinsic measurement cache

Best first targets:

- `Text`
- `TextInput`
- `Multiline`
- `Paragraph`
- `Image`
- `Video`

Likely key inputs:

- content
- font family
- weight
- italic
- font size
- letter spacing
- word spacing
- wrapping width mode
- scale

#### Resolved layout cache

Should store data such as:

- frame size and position under a parent constraint
- content box size
- child positions
- scroll maxes
- width-constrained paragraph fragments if resolve depends on them

Likely key inputs:

- parent constraint
- scale
- layout-affecting attrs hash
- inherited text context hash
- relevant child dependency versions

#### Refresh-only downstream rebuild

Keep `refresh(tree)` as a separate phase that consumes already-valid layout.

This should rebuild:

- render scene
- event registry
- IME-related outputs

without forcing measurement or geometry recompute when layout caches are still
valid.

### 4. Compatibility-based cache reuse

Borrow from Yoga rather than relying only on exact-key matches.

Examples:

- if a paragraph measurement for width `200` already fits a stricter width mode
  that still gives `200`, reuse it
- if a node was measured under a width that is unchanged and height remains
  unconstrained, reuse the height-for-width result

This is especially important for text and paragraph measurement.

### 5. Relayout boundaries and parent dependency edges

Borrow from Flutter and Servo.

Emerge should explicitly represent whether a parent depends on a child's size.

That means:

- if parent geometry depends on child size, child relayout invalidates parent
- if parent only contains or paints the child without consuming child size,
  relayout can stop lower in the tree

This should eventually produce real relayout boundaries.

### 6. Repeater and keyed-child cache preservation

Borrow from Slint and Iced.

Dynamic child lists should preserve per-child cache identity when items are:

- inserted
- removed
- reordered
- virtualized in or out of a viewport window

Without this, cache hit rates will collapse on real applications.

### 7. Subtree changed flags for downstream traversal skipping

Borrow from Yoga.

If a subtree has no new layout, downstream scene/event rebuild traversal should
be able to skip it.

This will matter more after layout caching starts reducing actual geometry work.

## Recommended Emerge Implementation Order

### Step 1. Replace coarse invalidation with dirty classes

Before adding caches, define and propagate:

- paint dirty
- resolve dirty
- measure dirty
- structure dirty

This should be wired from:

- upload
- patching
- interaction state updates
- scroll updates
- text input updates
- animations if they currently flow through attrs

### Step 2. Preserve identity through patching and dynamic lists

Before sophisticated caches, make sure patching preserves stable cache identity
for children and repeaters when logically possible.

This is the prerequisite for good cache hit rates.

### Step 3. Add intrinsic measurement cache first

Start with leaf-heavy and text-heavy nodes.

This gives the best signal with the least coupling to the rest of the solve.

### Step 4. Add upward invalidation with dependency versions

When a node becomes dirty:

- invalidate its relevant caches
- update its dependency version
- propagate only the necessary invalidation to ancestors

### Step 5. Add resolved-layout cache second

Only after measurement invalidation rules are solid.

This cache should reuse geometry when:

- parent constraint matches
- local layout signature matches
- relevant child dependency versions match

### Step 6. Introduce relayout boundaries

Once dirty propagation exists, cap it at nodes whose parent does not depend on
their layout information.

### Step 7. Add subtree changed flags to speed downstream refresh

This will let `refresh(tree)` skip clean subtrees when rebuilding scene and
event output.

### Step 8. Add viewport-aware repeater/list caching

This can be later, but it should be in scope if Emerge wants to scale to large
dynamic lists smoothly.

## What Not To Copy Literally

### Taffy's exact cache slot scheme

Useful as inspiration, but it is tuned for CSS sizing modes rather than Emerge's
constraint patterns.

### Yoga's exact leaf-measure semantics

The general cache and compatibility ideas are useful, but Emerge should keep its
own measurement API aligned with its tree and renderer.

### Flutter's full render object hierarchy

The dependency ideas are valuable. The object model itself should not be copied.

### Slint's property engine as-is

Its dependency tracking is powerful, but Emerge does not need to adopt the whole
property engine to benefit from the invalidation ideas.

### Servo's browser-specific fragmentation and formatting context rules

The damage model and cache layering matter; the web-specific layout model does
not.

## Success Criteria

This work is only worthwhile if it shifts the actual runtime profile.

Expected signs of success:

- fewer full recomputes from root
- lower `layout_ms_avg`
- lower layout variance in steady-state frames
- better reuse on text-heavy scenes
- no correctness regressions in nearby layout, scrolling, paragraph wrapping,
  event hit testing, or interaction styling

Useful future metrics to add:

- measure cache hit rate
- resolved-layout cache hit rate
- relayout-boundary stop count
- subtree refresh skip count
- dirty-kind counters by source

## Bottom Line

The strongest shared direction across these engines is:

- cache per node
- split measurement from resolved layout
- use typed invalidation rather than one dirty bit
- propagate dirtiness upward only where dependencies require it
- introduce relayout boundaries
- preserve cache identity across patching and dynamic lists
- keep paint invalidation separate from layout invalidation
- treat `refresh(tree)` as a downstream rebuild phase, not layout itself

That combination fits Emerge far better than copying any one external engine.
