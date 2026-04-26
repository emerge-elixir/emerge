# Native Tree Implementation Insights

Last updated: 2026-04-26.

This document merges the useful lessons from the old node-identity, phase 4,
and phase 5 plans. Those migrations are complete enough that separate phase
plans were more confusing than helpful. This file keeps the implementation
insights that should guide future layout-caching and invalidation work.

## Final identity model

Emerge uses three distinct identity concepts:

- `key` - semantic/user identity on the Elixir side
- `id` / `NodeId` - shared runtime identity across Elixir, EMRG, Rust, events,
  and patches
- `NodeIx` - native-only dense storage/traversal index

Important rule:

```text
key is semantic
NodeId is the shared runtime handle
NodeIx is a Rust storage handle
```

Do not collapse these concepts.

## Why `NodeId` and `NodeIx` are separate

`NodeId` is stable across the boundary and must survive tree mutations. It is
used for:

- EMRG full-tree decode/encode
- patch payloads
- event payloads back to Elixir
- runtime maps such as animations and input state
- public/debug-facing native APIs

`NodeIx` is dense and native-only. It is used for:

- arena indexing
- topology storage
- parent/host links
- cache ownership and dirty propagation
- avoiding repeated hash lookups when a path uses ix-native traversal

A `NodeIx` is not a semantic identity and must not cross the BEAM/native
boundary.

Current caveat: production topology is ix-authoritative, but some layout and
patch helpers still expose id-facing compatibility APIs such as `child_ids(...)`,
`nearby_mounts_for(...)`, and `get(&NodeId)`. That is correct today, but future
hot-path cleanup can make measure/resolve traversal more directly ix-native.

## Why no separate `WireId`

A separate wire identity layer was considered and rejected. Emerge's protocol is
private, and the extra abstraction would add complexity without much value.

The useful split is simply:

```text
NodeId crosses the boundary
NodeIx stays native
```

## Runtime IDs should not encode semantics

Earlier designs derived ids from structural/semantic hashes. That was rejected
because it:

- encoded reconciliation rules into the id value
- made future identity rules harder to change
- carried collision risk
- made debugging harder

The current direction is allocator-assigned runtime ids. Reconciliation decides
whether identity is reused; the numeric id itself does not explain why.

## Keyed and unkeyed identity rules

These rules are still useful when reasoning about cache preservation.

### Keyed children

Preserve identity when:

- the same explicit key appears
- the node remains in the same parent/host scope
- the kind is compatible

This preserves cache state across reorder within the same parent or nearby host.

### Unkeyed children

Unkeyed identity is positional. This is simple and predictable.

### Nearby mounts

Nearby slot changes on the same host should not break identity. The slot is
layout/mount data, not semantic identity.

### Reparenting

Do not preserve identity across reparenting for now. Treat it as remove+insert.
That keeps event/runtime/cache semantics simple and avoids ambiguous ownership.

## Native tree shape

The production tree is dense and index-backed:

```rust
struct ElementTree {
    revision: u64,
    next_ghost_seq: u64,
    current_scale: f32,
    root: Option<NodeIx>,
    nodes: Vec<Option<NodeRecord>>,
    id_to_ix: HashMap<NodeId, NodeIx>,
    free_list: Vec<NodeIx>,
    topology: TreeTopology,
}
```

Important properties:

- `id_to_ix` is the boundary lookup table
- topology is stored by `NodeIx`
- render, serialize, registry, and parts of patching/layout use ix-aware helpers
- some measure/resolve code still converts topology back to `NodeId` lists for compatibility
- parent/host links make upward propagation cheap
- free slots allow storage reuse without changing `NodeId` semantics

## Production topology is authoritative

Production topology is `NodeIx`-based:

- child links use `NodeIx`
- paint-child links use `NodeIx`
- nearby mount links use `NodeIx`
- parent links point to parent/host `NodeIx`

Some `#[cfg(test)]` id mirrors remain for tests. Treat them as test
compatibility helpers, not production design.

If future work updates topology, update the authoritative topology first. Test
mirrors should never be the source of truth.

## Node state split

Native nodes are split by responsibility.

### `NodeSpec`

Declarative patch/upload data:

- element kind
- raw attrs
- declared attrs before scale/runtime overlays

This is the stable basis for many cache keys.

### `NodeRuntime`

Runtime-only mutable state:

- text-input focus/cursor/selection/preedit
- active interaction state
- scrollbar hover state
- pending text-input patch content

This state should not automatically poison all layout caches. Classify the
actual effect before dirtying.

### `NodeLayoutState`

Layout-derived state:

- effective scaled attrs
- frame
- measured frame
- scroll extents
- paragraph fragments
- intrinsic measurement cache
- subtree measurement cache
- resolve cache
- measure/resolve dirty bits

This is the current home for layout cache state.

### `NodeLifecycle`

Lifecycle/residency data:

- mounted revision
- live vs ghost residency
- ghost attachment metadata
- ghost capture scale
- ghost exit animation

Ghosts are runtime/lifecycle concerns. Avoid letting ghost mechanics leak into
shared identity semantics.

## Dirty propagation insight

The storage rewrite made this possible:

```text
changed node -> parent link -> ancestor -> ...
```

That is much better than whole-tree scans or global dirty flags.

Current behavior:

- measure dirty implies resolve dirty
- resolve dirty propagates upward
- structure changes are classified separately

Future improvement:

- stop propagation at relayout/dependency boundaries when a parent does not
  depend on the changed child layout

## Cache ownership insight

Per-node cache ownership works because identity and storage are stable.
Current cache fields are on `NodeLayoutState`:

```rust
intrinsic_measure_cache: Option<IntrinsicMeasureCache>,
subtree_measure_cache: Option<SubtreeMeasureCache>,
measure_dirty: bool,
resolve_cache: Option<ResolveCache>,
resolve_dirty: bool,
```

This design keeps cache lifetime tied to retained node lifetime and lets patches
preserve cache state when identity is reused.

## Text-flow resolve-cache insight

Text-flow containers can use the same coordinate-invariant resolve cache when a
cache hit can restore all retained layout state by shifting the subtree.

Implemented shape:

- `Multiline`, `WrappedRow`, `TextColumn`, and `Paragraph` are resolve-cache
  eligible
- wrapped rows and text columns pass resolve-cache usage through to children
  where child layout is independent and cacheable
- paragraph inline text is parent-owned fragment layout, so inline children do
  not need independent resolve cache entries before a paragraph can store
- text columns may own paragraph child flow layout, so a text-column cache entry
  can restore that retained child state even when the paragraph child does not
  have a standalone resolve cache for the text-column flow context
- paragraph fragment positions are shifted by `shift_subtree(...)` alongside
  frames on resolve-cache hits

Future key/version work should preserve this distinction between independently
cacheable child layout and parent-owned derived flow layout.

## Boundary APIs can stay id-based

Not every function must expose `NodeIx`. It is fine, and often clearer, for
boundary/helper APIs to take `NodeId` when they are logically boundary-facing.

Good target rule:

1. accept/emit `NodeId` at the boundary
2. resolve to `NodeIx` once
3. do internal traversal/mutation by `NodeIx` where it is hot or topology-heavy
4. convert back to `NodeId` only for external output or stable runtime maps

The current codebase follows this in render/serialize and registry traversal,
but layout still has id-facing compatibility paths. Treat those as future
performance cleanup, not as a semantic problem.

Do not remove stable `NodeId` from runtime maps just to make everything
index-based. Runtime maps need identity that survives arena slot reuse and tree
mutation.

## Test compatibility insight

The implementation kept some test-only mirrors and helpers because they made a
large migration easier to validate.

This is acceptable as long as:

- production code does not depend on id mirrors for topology
- tests assert authoritative topology where correctness matters
- future production simplifications are not blocked by test-only fields

If tests start hiding topology bugs, replace mirrors with assertion helpers that
read authoritative `NodeIx` topology and convert to `NodeId` for comparison.

## Ghost/lifecycle insight

Ghost roots need stable runtime behavior but should not redefine identity rules.
Useful constraints:

- live shared nodes use `NodeId`
- ghost ids are native/runtime-generated and distinguishable
- ghost attachment should be represented as lifecycle/topology metadata
- ghost rendering/layout should not require preserving semantic identity across
  reparenting

## Implementation lessons for future work

### Keep phases small

The storage and cache work was easiest to review when committed as completed
slices:

- identity/topology foundation
- benchmark harnesses
- intrinsic cache
- subtree cache
- resolve cache
- stats/observability

Continue this pattern for future performance layers.

### Add observability before deeper optimization

The unified stats path is valuable because it avoids guessing. Before changing
cache behavior, add or refine counters that tell whether the change helped.

Current example: layout-cache stats should stay focused on hit/miss/store
outcomes. Animation and dirty propagation should be tracked through invalidation
or versioning when needed, not through a growing cache-bypass taxonomy.

### Prefer conservative cache correctness first

The current cache keys are conservative. That is good for correctness. Replace
conservative keys with cheaper version keys only when the version captures the
same dependency.

### Do not make every optimization public API

The stats path is intentionally unified and debug/benchmark-facing. Avoid adding
one NIF per cache family or optimization knob.

### Choose work from invalidation, not update source

Paint-only work should do the same amount of computation whether it originated
from a patch, animation sample, scroll runtime state, hover/focus state, or any
other source. The tree actor now does this through a frame update plan:

```text
external invalidation + sampled/effective dynamic invalidation -> work decision
```

`AnimationPulse` requests dynamic sampling instead of forcing measure/layout.
The sampled attrs produce ordinary `TreeInvalidation`, which is joined with
patch/scroll/runtime invalidation. The refresh decision then depends on that
combined invalidation and cached output availability, not on broad active-
animation state.

### Turn layout-affecting animation into dirty paths

The retained tree should treat sampled layout-affecting animation values like
other effective layout changes: mark the affected node and dependent ancestors
dirty, then let normal cache lookup produce hit/miss/store outcomes. This is now
the layout path behavior. `AnimationOverlayResult` records per-node animation
effects, and layout preparation converts measure/resolve effects into ordinary
dirty propagation before measure/resolve runs.

This preserves paint-only animation as refresh-only work and lets unrelated
clean sibling subtrees keep using measurement/resolve caches during width,
font-size, or alignment animations elsewhere.

### Keep render/event refresh separate

Layout cache entries should not own render scene or event registry output.
Those are downstream refresh concerns and need their own invalidation/skip
logic. Paint-only updates now use this separation: they update sampled effective
attrs and refresh render output without running measure/resolve layout when the
combined invalidation is paint-only.

## Future work enabled by this implementation

The native tree now supports the next layout-caching stages:

1. relayout/dependency boundaries
2. versioned cache keys plus more ix-native layout traversal where useful
3. refresh subtree skipping
4. viewport/repeater-aware cache preservation

See `layout-caching-roadmap.md` for the active implementation order.
