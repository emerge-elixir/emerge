# Taffy Layout Caching Insights

## Goal

Capture the layout-caching ideas from `../taffy` that transfer well to Emerge,
without copying flexbox-specific machinery that does not fit our engine.

This is a temporary planning document, not an implementation commitment.

## Why This Is Worth Doing

Recent renderer stats make the optimization target clear:

- steady-state `layout_ms_avg` is about `2.5ms - 2.7ms`
- steady-state `render_ms_avg` is about `1.2ms - 1.5ms`
- `present_submit_ms_avg` and `event_resolve_ms_avg` are small

So layout is the largest repeated CPU cost in the frame pipeline.

The current Emerge pipeline also still recomputes too coarsely:

- the tree actor largely reduces changes to a single `tree_changed` boolean
- `RefreshDecision::Recompute` then runs full layout from the root
- `refresh(tree)` already exists as a cheaper downstream rebuild step, but it is
  not backed by a proper layout cache

Relevant code:

- `native/emerge_skia/src/runtime/tree_actor.rs`
- `native/emerge_skia/src/tree/layout.rs`

## Current Emerge Shape

The layout pipeline already has a strong internal split:

1. prepare attrs for the frame
2. measure intrinsic sizes bottom-up
3. resolve final geometry top-down
4. rebuild render scene and event registry

That split is useful because it means we do not need a single giant cache. We
can cache the expensive parts at the level where they naturally belong.

Relevant entry points:

- `prepare_attrs_for_frame(...)`
- `measure_element(...)`
- `resolve_element(...)`
- `refresh(tree)`

All of these live in `native/emerge_skia/src/tree/layout.rs`.

## The Most Useful Ideas From Taffy

Taffy is worth studying for cache architecture, invalidation strategy, and the
boundary between recursive layout and cache ownership.

The most relevant files are:

- `../taffy/src/tree/cache.rs`
- `../taffy/src/compute/mod.rs`
- `../taffy/src/tree/taffy_tree.rs`
- `../taffy/src/tree/traits.rs`

### 1. Cache per node

Taffy stores cache state directly on each node.

Why this matters for Emerge:

- invalidation stays local
- ancestor propagation is simple
- cache lifetime matches tree lifetime
- we avoid a large global memo table with awkward ownership

This should map well to Emerge because `ElementTree` already owns the retained
tree, and each `Element` already carries runtime state such as frames and
interaction flags.

### 2. Cache by inputs, not by node identity alone

Taffy does not treat layout as `node -> result`.
It treats layout as `node + layout inputs -> result`.

That is the right mental model for Emerge too.

A cached result is only valid if the inputs that affect layout are unchanged.
For Emerge, that means the key must include more than `NodeId`.

Likely Emerge layout inputs:

- incoming constraint width and height
- scale
- node kind
- inherited font context or an inherited text-style hash
- layout-affecting attrs hash
- child structure version
- nearby mount structure version
- run mode

### 3. Separate measurement cache from final layout cache

This is the most important Taffy insight for Emerge.

Taffy keeps separate cache domains for preliminary size computation and final
layout. That matches Emerge extremely well because our engine already splits
into `measure_element(...)` and `resolve_element(...)`.

For Emerge, a good direction is:

- intrinsic measurement cache
- resolved layout cache

This is better than a single cache because:

- many nodes are measured repeatedly under similar constraints
- some changes invalidate measurement and resolve
- some changes invalidate only resolve
- render/event rebuild should remain downstream from cached layout state

### 4. Dirty propagation should move upward through ancestors

Taffy clears the changed node and then propagates dirtiness to ancestors.

That is the correct general invalidation direction for Emerge.

If a child's intrinsic size changes, only the path from that child to the root
needs layout invalidation. Unrelated sibling subtrees should not be forced to
recompute.

This is a large conceptual upgrade over one coarse `tree_changed` flag.

### 5. Centralize cache lookup and store in one wrapper

Taffy uses `compute_cached_layout(...)` as the uniform wrapper around recursive
layout entry points.

That pattern is worth copying.

Emerge should not spread caching decisions across every row, column, paragraph,
and leaf branch. Instead, it should have one cache-aware entry point for:

- intrinsic measurement
- resolved layout

That keeps correctness easier to reason about.

### 6. Treat run modes as separate cache domains

Taffy differentiates cache behavior by run mode.

Emerge should do the same.

At minimum, the engine should distinguish:

- intrinsic measurement
- full resolved layout
- refresh-only rebuild

This matters because `refresh(tree)` is not the same operation as layout, and
we should not accidentally entangle render/event rebuild semantics with geometry
reuse semantics.

### 7. Use bounded cache entries instead of an unbounded memo map

Taffy normalizes inputs into a small number of cache slots rather than storing
an unbounded set of arbitrary entries.

We should not copy Taffy's exact slot table, because it is tuned for CSS sizing
modes. But the underlying lesson is useful:

- normalize the constraint shapes we actually care about
- keep cache storage bounded
- prefer a few high-value entries over unlimited growth

## Emerge-Specific Interpretation

The transferable Taffy ideas suggest a more Emerge-native design.

### Per-node cache ownership

Each element should own cache state for the work that can be reused.

Likely per-node cached data:

- intrinsic size result
- width-constrained paragraph or multiline layout result
- resolved frame and content size
- dependency versions that explain why the entry is valid

This could live either:

- directly on `Element`
- in a side table keyed by `NodeId`

Directly on `Element` is simpler to reason about and matches Taffy's model.

### Two invalidation levels are not enough

Emerge should likely distinguish at least three invalidation classes.

#### Paint-only invalidation

No layout recomputation needed.

Examples:

- background color
- border color
- font color
- SVG color
- alpha

#### Resolve invalidation

Intrinsic measurement is still valid, but geometry must be recomputed.

Examples:

- some alignment changes
- scroll position if it affects placement only
- container distribution changes when child intrinsic sizes stay valid

#### Measure invalidation

Intrinsic measurement must be recomputed, which also forces resolve.

Examples:

- text content changes
- font family, size, or spacing changes
- width, height, padding, spacing, or border-width changes
- image or video intrinsic dimension changes
- child list or nearby list changes

This classification matters more than any specific cache container.
Without it, caches either become too conservative to help or too optimistic to
be correct.

### Refresh should stay separate from layout caching

This is one place where Emerge already has a valuable separation.

`refresh(tree)` rebuilds:

- render scene
- event registry
- IME-related output

That should stay downstream from layout caching.

The engine should cache geometry and measurement, then call `refresh(tree)` when
visual or event output needs rebuilding from already-valid layout state.

In other words:

- cache layout state
- do not treat render/event output as the same cache problem

## High-Value Cache Targets

### 1. Text measurement

This is the clearest first target.

Text, text input, multiline, and paragraph nodes all perform work that is both
repeated and expensive enough to justify caching.

Measurement cache key should include:

- content
- effective font family
- weight
- italic
- font size
- letter spacing
- word spacing
- width constraint class when wrapping matters
- scale

### 2. Paragraph and multiline width-dependent layout

Paragraph and multiline layout are not just scalar intrinsic-size operations.
They often produce structured intermediate data such as wrapped fragments.

That means a useful cache may need to store more than `(width, height)`.

For those nodes, the cached value may include:

- measured width/height
- wrapped lines or paragraph fragments
- height under a specific available width

### 3. Container measurement

Rows, columns, wrapped rows, and text columns should be able to reuse measured
child results when:

- the relevant children are clean
- spacing/insets are unchanged
- the container's own layout-affecting attrs are unchanged

This is where upward dirty propagation becomes important, because parent cache
validity depends on child dependency versions rather than just the parent's own
attrs.

### 4. Resolved geometry

Once intrinsic sizes are valid, many nodes should be able to reuse resolved
geometry if the incoming parent constraint and relevant dependency versions are
unchanged.

That includes:

- frame width/height
- content box width/height
- positioned child frames
- scroll max values

## Recommended Implementation Order

The order matters. Proper caching is mostly an invalidation problem.

### Step 1. Introduce layout-affecting dirty classification

Before adding real caches, define which changes affect:

- paint only
- resolve only
- measurement and resolve

This should be driven from the mutation points in:

- upload
- patching
- interaction state updates
- text input runtime updates
- scroll updates
- animation overlays if they remain part of layout

### Step 2. Add per-node intrinsic measurement cache

Start with leaf-like nodes and text-heavy nodes.

This gives the fastest feedback loop and avoids immediately coupling cache
correctness to the full resolve pass.

### Step 3. Add upward dirty propagation

When a node becomes measurement-dirty or resolve-dirty, propagate the necessary
invalidations to ancestors only.

This is where the current full-root recompute model should begin to shrink.

### Step 4. Add resolved layout cache

Only after measurement caching and invalidation rules are trustworthy.

Resolved layout cache should reuse geometry when:

- the parent constraint matches
- the node's own layout signature matches
- the relevant child dependency versions match

### Step 5. Keep `refresh(tree)` as the final rebuild step

Once layout state is valid or reused, regenerate render scene and event rebuild
output from the retained tree.

This preserves a clean boundary between layout caching and downstream outputs.

## What Not To Copy From Taffy

### Do not copy Taffy's exact cache slot scheme

It is tuned for CSS sizing semantics. The useful lesson is bounded cache shape,
not the literal slot mapping.

### Do not copy the trait layering wholesale

Taffy's abstraction boundaries are helpful to study, but Emerge should prefer
the smallest abstraction surface that keeps cache lookup/store centralized.

### Do not merge layout caching with render caching

Caching render scenes together with layout would blur ownership and make
invalidation harder, not easier.

## Practical Success Criteria

This work is worth it only if it changes the steady-state profile in a clear
way.

Success should look like:

- fewer full layout recomputes
- lower `layout_ms_avg`
- lower variance in layout cost for steady-state frames
- better reuse for text-heavy scenes
- no correctness regressions in scroll, nearby, paragraph flow, or interaction
  state styling

## Bottom Line

The most meaningful lessons from Taffy are architectural, not algorithmic:

- cache per node
- key cache entries by layout inputs
- separate measurement from final resolved layout
- propagate invalidation upward through ancestors
- keep cache logic centralized
- keep layout caching separate from `refresh(tree)` output generation

That combination fits Emerge well even though the actual layout engine is not
flexbox.
