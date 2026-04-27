# Cross-Engine Layout Caching Insights

Last updated: 2026-04-26.

This document preserves the cross-engine investigation that informed Emerge's
layout-caching work. It is research/reference material, not the active
implementation roadmap. For the current roadmap, see
`layout-caching-roadmap.md`.

The original investigation looked at Taffy, Yoga, Flutter, Slint, Iced, and
Servo. The useful outcome was not "copy one engine". The useful outcome was a
set of recurring patterns that fit Emerge's retained tree and split layout
pipeline.

## Emerge context

Emerge's native layout pipeline is naturally split into phases:

1. prepare effective attrs for the frame
2. measure intrinsic sizes bottom-up
3. resolve geometry top-down
4. refresh render scene and event registry

Important code paths:

- `native/emerge_skia/src/tree/layout.rs`
  - `prepare_attrs_for_frame(...)`
  - `measure_element(...)`
  - `resolve_element(...)`
  - `refresh(...)`
- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/invalidation.rs`
- `native/emerge_skia/src/runtime/tree_actor.rs`

The main lesson across engines is that this split should stay explicit. Emerge
should not have one giant layout cache. It should cache the work at the phase
where the work naturally belongs.

## Current Emerge status compared with the investigation

Already implemented:

- per-node cache ownership
- typed invalidation classes
- separate intrinsic/subtree/resolve cache layers
- upward measure/resolve dirty propagation
- `NodeId` / `NodeIx` split
- dense native tree storage and parent links
- gated layout-cache stats
- origin-agnostic invalidation/work scheduling for paint-only refresh vs layout
- targeted dirty propagation for layout-affecting animation samples
- broader resolve-cache reuse for text-flow-heavy layouts
- first relayout/dependency boundary for fixed-size `El`/`None` parents
- compact child/nearby topology dependency versions in layout cache keys
- detached nearby layout-cache reuse scoped to matching attachment context
- refresh-specific render/registry damage tracking with cached full-registry
  reuse when registry damage is clean
- retained render subtree caching/skipping for clean render subtrees
- render-cache regression guards comparing cached and uncached refresh paths,
  including cold upload/switch paths, with dirty/full rebuild cache-store
  deferral, damaged-no-cache fallback to the uncached renderer, and scroll-offset
  cache bypass to avoid immediately stale retained scenes

Still open:

- broader relayout/dependency boundaries
- further version/key work for attrs or measured/resolve dependency generations if profiles justify it
- broader registry chunk seeding/skipping if profiles justify it
- replacing remaining broad debug-hash render key fields with typed dependency
  versions if profiles justify it
- viewport/repeater-aware cache preservation

## Engine findings

## Taffy

Relevant files from the investigation:

- `/workspace/tmp-layout-engines/taffy/src/tree/cache.rs`
- `/workspace/tmp-layout-engines/taffy/src/compute/mod.rs`
- `/workspace/tmp-layout-engines/taffy/src/tree/taffy_tree.rs`
- `/workspace/tmp-layout-engines/taffy/src/tree/traits.rs`

### Useful ideas

#### Cache per node

Taffy stores cache state on each node instead of in one global memo table.

Why it transferred well to Emerge:

- cache lifetime matches retained node lifetime
- invalidation is local
- upward propagation is straightforward
- storage is bounded
- cache state survives retained patch/layout cycles when identity is preserved

Emerge status: implemented through `NodeLayoutState` cache fields.

#### Cache by layout inputs, not identity alone

Taffy treats cache validity as:

```text
node + layout inputs -> result
```

not simply:

```text
node -> result
```

Inputs that matter for Emerge include:

- element kind
- effective layout attrs
- inherited font/text context
- incoming constraint
- scale
- measured frame where resolve depends on it
- child/nearby topology or dependency versions
- active animation/cache mode

Emerge status: implemented conservatively with explicit cache keys. Future work
should make keys cheaper with versions.

#### Separate measurement cache from final layout cache

Taffy separates preliminary sizing from final layout results. This maps directly
to Emerge's bottom-up measure pass and top-down resolve pass.

Emerge status: implemented as intrinsic measurement, subtree measurement, and
resolved layout caches.

#### Upward dirty propagation

Taffy dirties ancestors rather than globally dirtying the whole tree.

Emerge status: implemented for measure/resolve through parent links. The first
fixed-size `El`/`None` and nearby overlay boundaries are implemented; future
work should add more dependency boundaries so upward propagation can stop
earlier in row/column, scrollable, and text-flow cases.

#### Centralize cache lookup/store

Taffy keeps cache lookup/store around recursive layout entry points. This is
important for correctness because caching behavior stays easy to audit.

Emerge should keep this property. Cache logic should remain concentrated in:

- `measure_element(...)`
- `try_reuse_intrinsic_measure_cache(...)`
- `try_reuse_subtree_measure_cache(...)`
- `resolve_element(...)`
- `try_reuse_resolve_cache(...)`

Kind-specific caches are acceptable only when they cache kind-specific derived
state, such as paragraph fragments.

#### Bounded cache shape

Taffy normalizes inputs into a bounded number of cache slots rather than growing
an unbounded memo map.

Emerge status: each node currently stores one high-value entry per cache family.
This is simple and low-risk. If multiple slots are added later, they should be
bounded and justified by benchmark counters.

### What not to copy

Do not copy Taffy's exact cache slot scheme. It is designed for CSS/flexbox
sizing modes. Emerge should keep its own constraint model.

Do not copy Taffy's trait layering wholesale. The useful part is cache ownership
and cache-domain separation, not the abstraction surface.

## Yoga

Relevant files from the investigation:

- `/workspace/tmp-layout-engines/yoga/yoga/node/LayoutResults.h`
- `/workspace/tmp-layout-engines/yoga/yoga/algorithm/CalculateLayout.cpp`
- `/workspace/tmp-layout-engines/yoga/yoga/algorithm/Cache.cpp`
- `/workspace/tmp-layout-engines/yoga/yoga/node/Node.cpp`

### Useful ideas

#### Value-sensitive dirtying

Yoga avoids dirtying when a set operation does not actually change the value.
Dirtying is idempotent and stops when an already-dirty ancestor is reached.

Emerge takeaway:

- classify attr changes before marking dirty
- avoid turning no-op patches into cache invalidation
- keep dirty marking idempotent

Emerge status: attr-change classification exists; further precision may still be
useful around runtime state and interaction styling.

#### Invalidation depends on inherited environment

Yoga's cache validity is not only local. Inherited environment inputs affect
measurement and layout.

Emerge takeaway:

- inherited font context must be part of text/layout cache keys
- scale and effective text styling must participate in measurement validity
- parent constraints must participate in resolve validity

Emerge status: inherited font keys are included in measurement/resolve cache
keys.

#### Compatibility-based measurement reuse

Yoga can reuse measurements when constraints are compatible, not only exactly
equal.

Emerge takeaway:

Text/paragraph caching should eventually be smarter than exact-key matching.
Examples:

- reuse height-for-width if the used width is unchanged
- reuse measurement when stricter constraints resolve to the same used width
- skip text measurement when declared dimensions determine the result

Emerge status: not implemented yet. This remains important for paragraph and
multiline performance.

#### Explicit changed-layout state

Yoga tracks whether a node has new layout so downstream consumers can avoid
unnecessary work.

Emerge takeaway:

- add subtree changed flags after layout reuse improves
- let `refresh(tree)` skip clean subtrees
- track whether scene/event data actually needs rebuilding

Emerge status: not implemented yet.

### What not to copy

Do not copy Yoga's exact leaf measurement semantics. Emerge should preserve its
own element kinds and text/paragraph behavior.

## Flutter

Relevant files from the investigation:

- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/object.dart`
- `/workspace/tmp-layout-engines/flutter/packages/flutter/lib/src/rendering/shifted_box.dart`
- `/workspace/tmp-layout-engines/flutter/packages/flutter/test/rendering/relayout_boundary_test.dart`

### Useful ideas

#### Relayout boundaries are dependency boundaries

Flutter does not treat relayout boundaries as just static node types. A boundary
is about whether layout dependencies cross that edge.

Emerge takeaway:

- track whether parent layout depends on child size
- stop upward invalidation when the parent does not consume the changed layout
- make relayout boundaries an explicit result of dependency shape

Emerge status: upward propagation exists, and the first fixed-size `El`/`None`
plus nearby overlay boundaries are implemented. Broader dependency boundaries
remain future work.

#### `parentUsesSize`

Flutter's `parentUsesSize` captures whether a parent depends on a child's size.
This is one of the most directly useful ideas for Emerge.

Emerge examples:

- a row/column usually uses child size
- an absolutely positioned/contained child may not affect parent intrinsic size
- nearby overlays may affect their own placement but not host intrinsic size
- scroll offset may require refresh/resolve without parent measurement

Future Emerge work should add explicit dependency metadata rather than guessing
from element kind alone.

#### Child owns the fast-path bailout

Flutter parents may call child layout unconditionally, while the child decides
whether constraints/dirty state allow it to return early.

Emerge takeaway:

- keep cache checks at child entry points
- do not make every parent branch duplicate child cache rules
- pass enough context for the child to validate its own cache

Emerge status: this is mostly how `measure_element(...)` and
`resolve_element(...)` now work.

#### Separate layout and paint invalidation

Flutter keeps layout invalidation separate from paint invalidation.

Emerge status: implemented at the `TreeInvalidation` level. Further work should
carry this separation into refresh skipping.

### What not to copy

Do not copy Flutter's render-object hierarchy. The dependency ideas are useful;
the object model is too large for Emerge.

## Slint

Relevant files from the investigation:

- `/workspace/tmp-layout-engines/slint/internal/core/partial_renderer.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/properties.rs`
- `/workspace/tmp-layout-engines/slint/internal/core/model/repeater.rs`
- `/workspace/tmp-layout-engines/slint/internal/interpreter/eval_layout.rs`

### Useful ideas

#### Separate geometry cache from render-property dirtiness

Slint tracks geometry and render properties separately.

Emerge takeaway:

- keep layout caches separate from scene/event refresh
- paint-only changes should not force measurement/geometry recompute
- later refresh skipping should still honor paint/registry dirtiness

Emerge status: layout-vs-paint invalidation exists; refresh skipping is still
future work.

#### Lazy dependency-based properties

Slint's property system tracks dependencies lazily.

Emerge takeaway:

Emerge does not need to adopt Slint's property engine, but it can still borrow
the principle that invalidation should be based on actual dependencies rather
than broad categories when possible.

This matters for future relayout boundaries and versioned cache keys.

#### Repeaters preserve incremental state

Slint repeaters keep state across inserts/removes and viewport movement.

Emerge takeaway:

- large dynamic lists need cache identity preservation
- keyed child identity should survive reorder
- viewport materialization should not destroy layout cache state unnecessarily

Emerge status: keyed identity preservation exists at the retained tree level;
viewport/repeater-specific caching remains future work.

#### Multi-phase layout for height-for-width content

Slint uses multiple phases to avoid dependency cycles in width/height-dependent
content.

Emerge takeaway:

Paragraph and multiline layout may need dedicated constrained-layout cache data,
not just scalar intrinsic size and frame extent.

Emerge status: this is likely needed for future paragraph/text-flow caching.

### What not to copy

Do not copy Slint's full property engine. Use dependency/version concepts where
they fit Emerge's simpler retained tree.

## Iced

Relevant files from the investigation:

- `/workspace/tmp-layout-engines/iced/core/src/widget/tree.rs`
- `/workspace/tmp-layout-engines/iced/core/src/shell.rs`
- `/workspace/tmp-layout-engines/iced/runtime/src/user_interface.rs`
- `/workspace/tmp-layout-engines/iced/widget/src/keyed/column.rs`
- `/workspace/tmp-layout-engines/iced/widget/src/lazy.rs`

### Useful ideas

#### Preserve subtree state aggressively

Iced's widget tree preserves state across diffs. This matters because caches are
only valuable if identity survives common updates.

Emerge takeaway:

- preserve `NodeId` across keyed reorder within a parent/host
- keep cache state attached to retained nodes
- avoid remove+insert churn when a patch can update in place

Emerge status: implemented as part of the identity and retained tree work.

#### Separate layout invalidity from widget/state invalidity

Iced distinguishes widget-tree invalidation from layout invalidation.

Emerge takeaway:

- structure, registry, paint, measure, and resolve changes should remain
  separate
- event registry rebuilds should not imply layout recompute unless geometry or
  hit-test structure changed

Emerge status: partially implemented through `TreeInvalidation` and refresh
routing.

#### Lazy subtrees

Iced's lazy widgets avoid rebuilding when inputs are unchanged.

Emerge takeaway:

Future refresh skipping can use a similar idea: if a retained subtree has no
layout/paint/registry changes, skip rebuilding its scene/event outputs.

### What not to copy

Iced is less useful for engine-level geometry caching than Taffy/Yoga/Flutter,
but it is useful for identity and state preservation.

## Servo

Relevant files from the investigation:

- `/workspace/tmp-layout-engines/servo/components/shared/layout/layout_damage.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/traversal.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/layout_box_base.rs`
- `/workspace/tmp-layout-engines/servo/components/layout/layout_impl.rs`

### Useful ideas

#### Damage bits instead of one dirty flag

Servo uses separate damage categories for style/layout/overflow/display-list
work.

Emerge takeaway:

One dirty bit is not enough. At minimum Emerge needs separate classes for:

- registry/event changes
- paint-only changes
- resolve/geometry changes
- measure/intrinsic changes
- structure changes

Emerge status: implemented through `TreeInvalidation`.

#### Stop relayout at incremental boundaries

Servo avoids relayout propagation across boundaries where it is not needed.

Emerge takeaway:

This reinforces the relayout-boundary work suggested by Flutter.

Emerge status: future work.

#### Preserve structure and repair state in place

Servo tries to preserve layout structure and repair style/cache state rather
than rebuilding from scratch.

Emerge takeaway:

- patch in place where identity is stable
- keep per-node cache state
- invalidate the smallest affected region

Emerge status: mostly implemented at tree/cache ownership level. More precise
boundaries remain future work.

#### Separate style, layout, overflow, display-list phases

Servo keeps downstream display-list work separate from layout.

Emerge takeaway:

`refresh(tree)` should remain downstream from layout caching. Do not merge scene
or event-registry rebuilds into geometry cache entries.

Emerge status: architecture already separates layout and refresh. Refresh
skipping remains future work.

### What not to copy

Do not copy Servo's browser-specific fragmentation, formatting contexts, or web
layout model. The damage model and phase separation are the useful pieces.

## Common conclusions across engines

### 1. One dirty bit is not enough

Every engine with strong incremental behavior separates invalidation kinds.

Emerge status: implemented.

### 2. Measurement and final layout must be cached separately

This was reinforced by Taffy, Yoga, Servo, and Emerge's own pipeline.

Emerge status: implemented.

### 3. Cache keys must be based on layout inputs

Useful Emerge inputs include:

- incoming constraint
- scale/effective attrs
- node kind
- inherited font context
- layout-affecting attrs
- child/nearby dependency versions
- run/cache mode

Emerge status: implemented conservatively. Child/nearby topology dependencies
now use compact version keys; additional attr or dependency generations remain
future work if profiles justify them.

### 4. Dirtiness should propagate upward, not globally

A child's layout-affecting change should dirty ancestors that depend on it, not
unrelated subtrees.

Emerge status: upward propagation exists. Initial dependency boundaries exist
for fixed-size `El`/`None` and nearby overlays; broader boundaries remain future
work.

### 5. Relayout boundaries matter

Flutter and Servo strongly suggest that upward propagation needs explicit stop
points.

Emerge status: initial fixed-size `El`/`None` and nearby overlay boundaries are
implemented. Row/column, scrollable, and text-flow boundaries remain future
work.

### 6. State identity must survive list and subtree edits

Slint and Iced reinforce that caching depends on stable identity.

Emerge status: retained `NodeId` identity is in place; viewport/repeater work is
future.

### 7. Paint invalidation must stay separate from layout invalidation

Flutter, Slint, and Servo all reinforce this.

Emerge status: implemented at invalidation classification and scheduling level.
Paint-only updates now reach refresh without measure/resolve layout through the
same combined-invalidation path whether the paint change came from animation,
scroll, patching, hover/focus runtime state, or another source.

### 8. Work scheduling should be origin-agnostic

Other engines generally convert external changes and dynamic state into dirty /
damage / invalidation state, then choose work from that state. They do not need a
separate scheduler rule for every source of the change.

Emerge status: implemented in the tree actor. Each batch builds a frame update
plan, samples dynamic animation state when needed, joins sampled invalidation
with external patch/scroll/runtime invalidation, and chooses work from the
combined `TreeInvalidation` plus cached output availability. Broad active-
animation state no longer forces the refresh decision.

### 9. Layout-affecting dynamic state should become ordinary dirty state

Taffy/Yoga/Flutter/Servo all point toward invalidating the affected dependency
paths, then letting normal cache lookup decide hit/miss/store.

Emerge status: layout-affecting animation samples now record per-node effects.
Before layout, those effects mark ordinary measure/resolve dirty paths. This
removes the previous whole-tree animation cache-disable mode while preserving
paint-only animation refresh skipping.

### 10. Refresh/render/event output is downstream work

Do not conflate geometry caching with scene or event-registry caching.

Emerge status: architecture separates these phases. Refresh-specific
render/registry damage, cached full-registry reuse, retained render subtree
reuse, and conservative registry chunk caching are implemented; broader
registry chunk seeding remains future work if profiles justify it.

## How these insights shape the current roadmap

The current roadmap is not "add caches" anymore. Those are implemented. The
next work should use the insights above to make reuse broader, safer, and more
precise:

1. broaden relayout/dependency boundaries beyond fixed-size `El`/`None` and
   nearby overlays
2. make hot layout traversal more ix-native where profiles show id-facing
   compatibility helpers are costly
3. revisit registry chunk seeding/skipping only if profiles justify it
4. preserve cache identity through viewport/repeater movement

See `layout-caching-roadmap.md` for the implementation order.
