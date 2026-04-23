# Node Identity And Rust Tree Structure Plan

## Goal

Define a simpler and faster identity model for Emerge that:

- preserves meaningful shared semantics between Elixir and Rust
- keeps explicit user keys globally unique across the full tree for future semantic APIs
- makes a clean cut immediately between semantic `key` and runtime `id`
- avoids unnecessary identity layers and translations
- unlocks efficient native tree storage, dirty propagation, and layout caching
- preserves identity across reorder within the same parent
- preserves nearby identity across reorder and slot changes within the same host
- does not preserve identity across reparenting for now

This plan is intentionally Emerge-specific. The EMRG protocol is private and is
not intended to support unrelated external consumers, so simplicity and runtime
efficiency are more important than general-purpose decoupling.

## Guiding Principles

1. Keep shared semantics explicit.
2. Keep hot-path native storage compact.
3. Avoid extra protocol layers unless they produce clear wins.
4. Let reconciliation semantics decide identity reuse.
5. Do not encode semantics into the numeric id value itself.

## Decision Summary

### Chosen model

- One shared `NodeId` across Elixir, EMRG, events, and Rust.
- One native-only `NodeIx` inside Rust.
- No separate `WireId` layer.
- `NodeId` is allocator-assigned, not hash-derived.
- Explicit user `key`s stay globally unique across the full tree.
- Elixir tree nodes use explicit `key` and `id` fields instead of overloading the old pre-clean-cut meaning of `id`.
- Reconciliation rules decide whether an existing `NodeId` is reused.

### Explicitly rejected for now

- Sharing `NodeIx` across Elixir and Rust.
- Introducing a separate `WireId` transport identity layer.
- Continuing to derive ids from semantic hashes such as `phash2(...)`.
- Keeping `Element.id` overloaded as both key and runtime identity during migration.
- Preserving identity across reparenting.

## Why This Model Fits Emerge

### Why not share `NodeIx`

`NodeIx` is a storage handle, not a semantic identity.

It is good for:

- dense arrays
- parent links
- bitsets
- cache tables
- compact traversal

It is bad for:

- Elixir reconciliation semantics
- event routing across the BEAM/native boundary
- stable identity across uploads and patch application
- representing runtime-only native nodes such as ghosts

So `NodeIx` should remain purely native.

### Why not introduce `WireId`

The main value of a separate `WireId` would be further decoupling between:

- semantic identity
- protocol identity
- native storage identity

That abstraction is not pulling its weight for Emerge because:

- Emerge's protocol is private
- Emerge's tree semantics intentionally stay close between Elixir and Rust
- tree sizes and patch sizes are modest
- another identity layer would add more complexity than benefit

So the best practical split is simply:

- shared `NodeId`
- native `NodeIx`

### Why not keep semantic-derived ids

The current Elixir reconciler computes ids from structural semantics using:

- parent context
- kind
- key or positional index
- `:erlang.phash2(...)`

That approach has several downsides:

- it bakes semantic matching rules into the id value itself
- it is opaque
- it is harder to evolve later
- it introduces collision risk

The semantics should determine whether a node keeps identity, but the id value
itself should just be a stable assigned token.

## Shared Identity Model

## NodeId

`NodeId` is the only identity shared across:

- Elixir assigned trees
- VDOM reconciliation state
- EMRG full-tree and patch payloads
- native Rust tree storage boundary
- event routing back to Elixir

Recommended representation:

- Elixir: positive integer
- wire: `u64`
- Rust: `type NodeId = u64`

Rules:

1. `NodeId` is monotonic.
2. `NodeId` is assigned by the Elixir diff state.
3. `NodeId` is never reused within a `DiffState` lifetime.
4. `NodeId` does not encode parent, kind, slot, or position.

## User keys

Explicit user keys are a separate concept from `NodeId`.

Recommended meaning:

- the input `Element.key` is the user key when present
- keys are globally unique across the full tree
- nearby mounts participate in the same global uniqueness domain
- keys are semantic handles for future APIs such as focus targeting, drag and
  drop semantics, or other logical element references

Important rule:

- `key` is global and semantic
- `NodeId` is assigned and runtime-oriented
- key lookup may find the same logical element globally, but `NodeId` reuse is
  still scoped and does not survive reparenting for now

## Semantic matching rules

Identity reuse is controlled by matching rules, not id derivation.

### Keyed children

Preserve identity when all of the following hold:

- same globally unique key resolves to an old node
- same parent scope
- same kind

This allows reorder within the same parent while preserving `NodeId`.

### Unkeyed children

Preserve identity when all of the following hold:

- same parent scope
- same index
- same kind

This keeps unkeyed reconciliation positional.

### Nearby mounts

Preserve identity when all of the following hold:

- same globally unique key if keyed, or same index if unkeyed
- same nearby host scope
- same kind

Important: nearby slot changes on the same host do **not** change identity.
The slot is treated as layout/mount data, not identity.

### Reparenting

Do not preserve identity across:

- parent changes
- child-to-nearby changes
- nearby-to-child changes
- nearby host changes

Those cases are treated as remove + insert.

### Kind changes

Kind changes always allocate a new `NodeId`.

## Matching scopes

Identity matching is scoped. Suggested scopes:

- `:root`
- `{:children, parent_node_id}`
- `{:nearby, host_node_id}`

Matching only happens within one scope.

This means:

- keys are global across the full tree
- child identity is local to one parent
- nearby identity is local to one host
- child and nearby are different scopes
- keyed lookup can be global, but `NodeId` reuse still requires the matched old
  node to be in the same scope

## Elixir Reconciliation Plan

## Elixir list-building note

Reconciliation code should treat Elixir lists as linked lists and build them the
cheap way:

- prepend with `[item | acc]`
- reverse once at the end with `Enum.reverse/1`
- when stitching a forward list onto a reversed accumulator, prefer
  `Enum.reverse(list, tail)` rather than repeated `++`

Important rule of thumb:

- avoid repeated `++` in reconciliation hot paths

Any pseudocode below should be implemented in this prepend-and-reverse style,
even when the high-level intent is just "append these patches in order".

## DiffState changes

Add a monotonic allocator to the diff state.

Suggested shape:

```elixir
defmodule Emerge.Engine.DiffState do
  defstruct tree: nil,
            vdom: nil,
            next_id: 1,
            event_registry: %{}
end
```

Allocator:

```elixir
defp alloc_id(%DiffState{next_id: id} = state) do
  {id, %{state | next_id: id + 1}}
end
```

Important rule: do not reuse ids during the lifetime of one `DiffState`.

That avoids ABA-style stale event bugs and keeps identity reasoning simple.

## VNode changes

The reconciled vnode should store assigned runtime identity directly.

Suggested conceptual shape:

```elixir
defmodule Emerge.Engine.VNode do
  defstruct [:id, :kind, :key, :attrs, children: [], nearby: []]
end
```

`key` stays the semantic reconciliation hint.
`id` is the assigned shared runtime identity.

## Naming model

Use this naming consistently:

- Elixir: `key` + `id`
- Rust: `NodeId` type + `id` field + `NodeIx` internal index

That means:

- `key` is the semantic/user identity
- `id` is the shared runtime identity
- `NodeIx` is native-only storage/traversal identity

## Element shape

Make the clean cut immediately on the Elixir side.

Suggested conceptual shape:

```elixir
defmodule Emerge.Engine.Element do
  defstruct [
    :type,
    :key,
    :id,
    attrs: %{},
    children: [],
    nearby: [],
    frame: nil
  ]
end
```

Rules:

- key: semantic matching hint
- id: assigned runtime identity
- public UI trees carry `key` and start with `id: nil`
- assigned trees carry both `key` and `id`
- no code path should continue to overload one field with both meanings

This makes the migration larger up front, but it keeps the model clear and
avoids carrying compatibility ambiguity through the rest of the work.

In the final cross-language naming model:

- Elixir uses `key` + `id`
- Rust uses `NodeId` as the shared-id type and `id` as the field name
- `NodeIx` remains native-only

## Clean-cut Elixir file changes

The initial Elixir-facing cut should change these files together:

- `lib/emerge/engine/element.ex`
  add explicit `:key` and `:id` fields
- `lib/emerge/ui/internal/builder.ex`
  move public `:key` attr into `Element.key`, not runtime `Element.id`
- `lib/emerge/ui.ex`
  keep `key/1` as the public API, but document it as filling `Element.key`
- `lib/emerge/engine/vnode.ex`
  keep runtime identity on `id`
- any Elixir code that currently reads `element.id` must be classified as either
  semantic-key usage or runtime-id usage and updated accordingly

## Global key index

Reconciliation should build one keyed index for the old tree before diffing.

Suggested shape:

```elixir
%{
  key => %{scope: scope(), vnode: vnode}
}
```

Because keys are globally unique, there is at most one old keyed candidate for
each explicit key.

This lets the reconciler:

- detect reorder within the same scope and preserve `NodeId`
- detect reparenting explicitly when the same key is found in a different scope
- keep future semantic APIs aligned with the same global-key model

## Key uniqueness rules

Keep key validation globally unique across the full tree.

Rules:

- explicit keys must be unique across all children and nearby mounts in the full tree
- any duplicate explicit key should fail reconciliation immediately

This should be validated once per tree before reconciliation proceeds.

## Fresh tree assignment

When there is no previous tree, assign ids in preorder.

Conceptual flow:

```elixir
def assign_ids(%Element{} = root, %DiffState{} = state) do
  validate_global_keys!(root)
  {vnode, assigned, state} = build_fresh_subtree(root, :root, state)
  {vnode, assigned, state}
end
```

Fresh subtree builder:

```elixir
defp build_fresh_subtree(%Element{} = element, scope, state) do
  {id, state} = alloc_id(state)
  key = element.key

  {child_vnodes, child_elements, state} =
    build_fresh_children(element.children, {:children, id}, state)

  {nearby_vnodes, nearby_elements, state} =
    build_fresh_nearby(element.nearby, {:nearby, id}, state)

  vnode = %VNode{
    id: id,
    kind: element.type,
    key: key,
    attrs: strip_runtime_attrs(element.attrs),
    children: child_vnodes,
    nearby: nearby_vnodes
  }

  assigned = %{
    element
    | id: id,
      children: child_elements,
      nearby: nearby_elements
  }

  {vnode, assigned, state}
end
```

## Root reconciliation

Suggested rule:

- preserve root identity if the root kind still matches
- otherwise replace the whole root subtree

Conceptual flow:

```elixir
def reconcile(nil, %Element{} = root, %DiffState{} = state) do
  validate_global_keys!(root)
  {vnode, assigned, state} = build_fresh_subtree(root, :root, state)
  {vnode, [], assigned, state}
end

def reconcile(%VNode{} = old_root, %Element{} = new_root, %DiffState{} = state) do
  validate_global_keys!(new_root)
  old_key_index = build_old_key_index(old_root)

  if old_root.kind == new_root.type do
    reconcile_matched_node(old_root, new_root, :root, old_key_index, state)
  else
    {new_vnode, new_assigned, state} = build_fresh_subtree(new_root, :root, state)

    patches = [
      {:remove, old_root.id},
      {:insert_subtree, nil, 0, new_assigned}
    ]

    {new_vnode, patches, new_assigned, state}
  end
end
```

## Matched node reconciliation

This is the core rule: once a node is matched semantically, it keeps the old
`NodeId`.

Conceptual flow:

```elixir
defp reconcile_matched_node(%VNode{} = old, %Element{} = element, scope, old_key_index, state) do
  id = old.id
  key = element.key

  {child_vnodes, child_elements, child_patches, state} =
    reconcile_children(old.children, element.children, {:children, id}, old_key_index, state)

  {nearby_vnodes, nearby_elements, nearby_patches, state} =
    reconcile_nearby(old.nearby, element.nearby, {:nearby, id}, old_key_index, state)

  new_attrs = strip_runtime_attrs(element.attrs)

  child_ids = Enum.map(child_vnodes, & &1.id)
  old_child_ids = Enum.map(old.children, & &1.id)

  nearby_refs = Enum.map(nearby_vnodes, fn {slot, vnode} -> {slot, vnode.id} end)
  old_nearby_refs = Enum.map(old.nearby, fn {slot, vnode} -> {slot, vnode.id} end)

  patches_rev =
    []
    |> maybe_prepend(old_nearby_refs != nearby_refs, {:set_nearby_mounts, id, nearby_refs})
    |> prepend_many(Enum.reverse(nearby_patches))
    |> maybe_prepend(old_child_ids != child_ids, {:set_children, id, child_ids})
    |> prepend_many(Enum.reverse(child_patches))
    |> maybe_prepend(old.attrs != new_attrs, {:set_attrs, id, new_attrs})

  patches = Enum.reverse(patches_rev)

  vnode = %VNode{
    id: id,
    kind: element.type,
    key: key,
    attrs: new_attrs,
    children: child_vnodes,
    nearby: nearby_vnodes
  }

  assigned = %{
    element
    | id: id,
      children: child_elements,
      nearby: nearby_elements
  }

  {vnode, patches, assigned, state}
end
```

This removes the need for `make_id/3` entirely.

## Children reconciliation

### Keyed children

Algorithm:

1. Build one global `old_key_index` for the full old tree.
2. For each new keyed child in order:
   - look up the old keyed node globally by key
   - if the candidate is in the same parent scope and has the same kind, reuse `old.id`
   - if the candidate exists in a different scope, treat it as reparenting and allocate a fresh `NodeId`
   - otherwise allocate a fresh `NodeId` and emit `insert_subtree`
3. Any old keyed child not used becomes `remove`
4. Parent order is expressed by final `set_children`

Conceptual flow:

```elixir
defp reconcile_children_keyed(old_children, new_children, {:children, parent_id}, old_key_index, state) do

  {vnodes_rev, elements_rev, patches_rev, used_ids, state} =
    Enum.with_index(new_children)
    |> Enum.reduce({[], [], [], MapSet.new(), state}, fn {child, index}, acc ->
      key = child.key
      scope = {:children, parent_id}

      case old_key_index[key] do
        %{scope: ^scope, vnode: %VNode{kind: kind} = old_child} when kind == child.type ->
          ...

        _ ->
          ...
      end
    end)

  patches_rev =
    old_children
    |> Enum.reject(&MapSet.member?(used_ids, &1.id))
    |> Enum.reduce(patches_rev, fn child, acc -> [{:remove, child.id} | acc] end)

  {
    Enum.reverse(vnodes_rev),
    Enum.reverse(elements_rev),
    Enum.reverse(patches_rev),
    state
  }
end
```

Key semantic property: reorder within the same parent preserves identity.
Global keys let us detect reparenting explicitly, but reparenting still does
not preserve `NodeId`.

### Unkeyed children

Algorithm:

1. Match by a single forward walk over old and new siblings within the same parent scope.
2. Require same kind.
3. If matched, reuse `NodeId`.
4. Otherwise remove old and insert fresh.

Key semantic property: unkeyed siblings stay positional, but implementation
should be list-native and avoid `Enum.at/2`-style indexed access.

## Nearby reconciliation

Nearby follows the same structure as children, but with scope `{:nearby, host_id}`
and the same global key index.

Important semantic rule:

- nearby slot changes on the same host do **not** break identity

So a keyed nearby tooltip can move from `:above` to `:below` on the same host
and keep the same `NodeId`.

Identity depends on:

- same host scope
- same globally unique key if keyed, or same index if unkeyed
- same kind

Identity does **not** depend on slot.

## Patch generation policy

Simplify patch generation compared with the current heuristics.

For a matched parent:

1. emit `set_attrs` if attrs changed
2. emit descendant inserts/removes/attr updates
3. emit `set_children` if ordered child ids changed
4. emit `set_nearby_mounts` if ordered nearby refs changed

This is simpler and more explicit than trying to avoid some order patches by
deriving ids from structure.

When implementing this in Elixir, patch accumulation should still follow the
prepend-and-reverse rule described above rather than using repeated `++`.

## Example behaviors

### Keyed reorder within same parent

Old:

```text
[a(id=11), b(id=12), c(id=13)]
```

New:

```text
[c, a, b]
```

Result:

- `c` keeps `13`
- `a` keeps `11`
- `b` keeps `12`
- emit `set_children(parent, [13, 11, 12])`

### Unkeyed reorder

Old:

```text
[text("A", id=11), text("B", id=12)]
```

New:

```text
[text("B"), text("A")]
```

Result:

- index `0` keeps `11`
- index `1` keeps `12`
- attrs/content update in place
- no identity swap

### Nearby slot change on same host

Old:

```text
[{:above, tooltip(key=:tip, id=21)}]
```

New:

```text
[{:below, tooltip(key=:tip)}]
```

Result:

- tooltip keeps `21`
- emit `set_nearby_mounts(host, [{:below, 21}])`

### Reparenting

Old:

```text
parent_a children: [item(key=:x, id=31)]
parent_b children: []
```

New:

```text
parent_a children: []
parent_b children: [item(key=:x)]
```

Result:

- `31` is removed from `parent_a`
- the same global key is found, but in a different scope
- fresh id allocated under `parent_b`
- no identity preservation

## EMRG Protocol Plan

`key` is Elixir-side semantic data. EMRG should carry `id`, not `key`, for
runtime patching and native lookup.

## Fixed-width ids

Replace Erlang term-encoded ids with fixed-width numeric ids.

Recommended wire format:

- `NodeId` encoded as unsigned big-endian `u64`

This applies to:

- full-tree serialization
- patch streams
- event ids coming back to Elixir
- event registry keys on the Elixir side

It does not imply sending semantic keys over the runtime protocol unless a later
feature proves that necessary.

This should simplify both sides materially:

- less encoding overhead
- less allocation
- simpler Rust decode path
- simpler Elixir event registry keys

## Rust Tree Structure Plan

## Immediate next step

The immediate next native step after phases 1-3 is a narrower phase 4 than this
document originally described.

Phase 4 should be:

- `NodeIx` storage
- `id_to_ix`
- parent links
- ix-based child and nearby topology
- patch/layout/render traversal rewritten around `NodeIx`

Phase 4 should **not** yet include:

- dirty flags
- layout caches
- `NodeSpec` / `NodeRuntime` / `NodeLayoutState` split
- moving `paint_children` out of the node

Those remain useful longer-term ideas, but they should be deferred until after
the storage/topology rewrite is complete.

See `plans/phase-4-nodeix-storage-plan.md` for the concrete scope.

## Identity split

Rust should use:

```rust
type NodeId = u64;
type NodeIx = u32;
```

Rules:

- `NodeId` crosses the boundary and is used for patches/events
- `NodeIx` is internal only
- all hot-path tree traversal and caches use `NodeIx`

## Core tree shape

Suggested direction:

```rust
struct ElementTree {
    root: Option<NodeIx>,
    nodes: Vec<NodeRecord>,
    id_to_ix: HashMap<NodeId, NodeIx>,
    free_list: Vec<NodeIx>,
    revision: u64,
    current_scale: f32,
    next_ghost_seq: u64,
}
```

This gives:

- dense node storage
- fast `NodeId -> NodeIx` lookup at patch boundaries
- compact traversal and cache indexing

## NodeRecord split

Longer-term native target after phase 4+:

Suggested node record split:

```rust
struct NodeRecord {
    id: Option<NodeId>,
    spec: NodeSpec,
    links: NodeLinks,
    runtime: NodeRuntime,
    layout: NodeLayoutState,
    dirty: DirtyFlags,
    versions: NodeVersions,
    residency: NodeResidency,
}
```

### `id: Option<NodeId>`

- `Some(id)` for live shared nodes
- `None` for native-only ghost/runtime nodes if we want to keep them fully
  outside the shared identity space

This keeps ghost mechanics from polluting the shared identity model.

## NodeSpec

This is a later-phase target, not part of the narrowed phase 4 scope.

Immutable or patch-driven declarative data.

Suggested contents:

```rust
struct NodeSpec {
    kind: ElementKind,
    attrs_raw: Vec<u8>,
    declared: DeclaredAttrs,
}
```

Purpose:

- stable basis for cache keys
- clear separation from runtime-only mutation
- avoids rebuilding one giant mixed `Attrs` object every frame

## NodeLinks

Canonical topology and attachment information.

Suggested contents:

```rust
struct NodeLinks {
    parent: Option<NodeIx>,
    attachment: AttachmentKind,
    children: Vec<NodeIx>,
    nearby: Vec<NearbyEdge>,
}

enum AttachmentKind {
    Root,
    Child,
    Nearby { slot: NearbySlot },
}

struct NearbyEdge {
    slot: NearbySlot,
    node: NodeIx,
}
```

This removes the need for full-tree parent scans.

Key properties:

- parent lookup becomes O(1)
- ancestor invalidation becomes O(depth)
- child and nearby topology become explicit

## NodeRuntime

This is a later-phase target, not part of the narrowed phase 4 scope.

Runtime-only mutable state that should not poison layout cache keys.

Suggested contents:

```rust
struct NodeRuntime {
    scroll_x: f32,
    scroll_y: f32,
    scroll_x_max: f32,
    scroll_y_max: f32,
    scrollbar_hover_axis: Option<ScrollbarHoverAxis>,
    mouse_over_active: bool,
    mouse_down_active: bool,
    focused_active: bool,
    text_input: Option<TextInputRuntime>,
    animation_overlay_rev: u64,
    ghost: Option<GhostRuntime>,
}
```

This is where current mixed-in runtime fields from `Attrs` should move.

## NodeLayoutState

This is a later-phase target, not part of the narrowed phase 4 scope.

Separate layout artifacts from declared and runtime attrs.

Suggested contents:

```rust
struct NodeLayoutState {
    scaled_style: Option<ScaledStyleCache>,
    intrinsic: Option<IntrinsicCacheEntry>,
    resolved: Option<ResolvedLayoutEntry>,
    paragraph_fragments: Option<Vec<TextFragment>>,
    paint_order: Option<Vec<u16>>,
    subtree_layout_changed: bool,
}
```

This replaces the current overloading of:

- `frame`
- `measured_frame`
- runtime `paragraph_fragments` inside `Attrs`

### Intrinsic cache entry

Suggested idea:

```rust
struct IntrinsicCacheEntry {
    key: IntrinsicKey,
    size: IntrinsicSize,
}
```

### Resolved layout entry

Suggested idea:

```rust
struct ResolvedLayoutEntry {
    key: ResolvedLayoutKey,
    frame: Frame,
    content_size: (f32, f32),
    scroll_max: (f32, f32),
}
```

The exact key contents can evolve, but this gives caches an explicit home.

## DirtyFlags

This is a later-phase target, not part of the narrowed phase 4 scope.

Replace one coarse tree-wide dirty bit with per-node flags.

Suggested shape:

```rust
struct DirtyFlags {
    structure: bool,
    measure: bool,
    resolve: bool,
    paint: bool,
    registry: bool,
}
```

Semantics:

- `structure`: child/nearby topology changed
- `measure`: intrinsic measurement invalid
- `resolve`: geometry invalid but intrinsic may still be valid
- `paint`: scene rebuild needed but layout may not be
- `registry`: event registry rebuild needed

This is the minimum structure needed for proper caching and selective refresh.

## NodeVersions

This is a later-phase target, not part of the narrowed phase 4 scope.

Per-node versions make cache validation and ancestor dependency checks much
cheaper.

Suggested shape:

```rust
struct NodeVersions {
    spec_rev: u64,
    runtime_rev: u64,
    measure_rev: u64,
    resolve_rev: u64,
    subtree_rev: u64,
}
```

Purpose:

- validate cached results cheaply
- propagate dependency changes upward
- decide whether parent layout caches remain valid

## Parent links and ancestor propagation

One of the biggest structural wins of `NodeIx` + `parent` links is that dirty
propagation becomes straightforward:

- mark node dirty
- walk `parent` links upward
- update only ancestors that actually depend on the change

This is much better than current repeated tree scans for parent discovery.

## Child ordering

Keep one canonical child list in `links.children` as the longer-term target.

For the narrower phase 4, keep `paint_children` on the node and simply convert
it from `Vec<NodeId>` to `Vec<NodeIx>` so the storage rewrite can preserve
behavior without also taking on a layout-state split.

Later, if a container needs paint reordering, store that as derived layout state:

```rust
paint_order: Option<Vec<u16>>
```

This avoids duplicating structural topology and reduces per-frame churn.

## Patch application in Rust

Patch application in the narrowed phase 4 should become:

1. translate incoming `NodeId` to `NodeIx`
2. update topology and node payload through `NodeIx`
3. update parent links immediately
4. use parent links for detach, remove, and ghost attachment logic

This should be much cheaper and much easier to reason about than the current
`HashMap<NodeId, Element>` plus repeated scans.

Precise dirty flags and invalidation propagation belong to the following phase.

## Event registry impact

The Elixir event registry should use `element.id`, not `element.key`.

`key` remains available for future semantic APIs such as:

- focus targeting by key
- drag-and-drop semantics
- logical element lookup

But the runtime event-routing contract should stay on `NodeId`.

## Why this structure supports caching well

This design directly unlocks the caching ideas from the earlier plans:

- per-node intrinsic measurement cache
- per-node resolved layout cache
- ancestor-only invalidation
- layout vs paint dirty separation
- subtree refresh skipping using `subtree_layout_changed`
- stable cache identity across keyed reorder within the same parent or host

## Migration Plan

### Phase 1. Clean-cut Elixir model change

1. Replace `Element.id` with explicit `Element.key` and `Element.id`.
2. Update builders and UI docs so public `key/1` fills `Element.key`.
3. Update `VNode` to store assigned `id` explicitly.
4. Keep explicit keys globally unique across the full tree.
5. Build and use a global keyed index during reconciliation.
6. Replace `phash2(...)`-derived ids with allocator-assigned `NodeId`.
7. Preserve nearby identity across slot changes on the same host.

### Phase 2. Reconciliation rewrite

1. Rework keyed reconciliation around the global key index plus scope checks.
2. Rework unkeyed reconciliation as linear list-native walks instead of indexed access.
3. Rewrite patch accumulation using prepend-and-reverse only.
4. Update event registry code to use `id` explicitly.

### Phase 3. EMRG id format change

1. Replace term-encoded ids with fixed-width `u64` ids.
2. Update tree serialization.
3. Update patch encoding.
4. Update event id handling.

### Phase 4. Rust tree storage rewrite

1. Introduce `NodeIx` as the native-only storage handle.
2. Replace `HashMap<NodeId, Element>` with dense node storage plus `id_to_ix`.
3. Add an internal ix-based topology layer with parent links.
4. Keep id-based child and nearby fields as compatibility mirrors for now.
5. Rewrite patch/layout/render traversal around `NodeIx`.
6. Keep behavior unchanged and defer caches/dirty flags/state split.

### Phase 5. Native NodeId / NodeIx cleanup

1. Rename Rust shared runtime identity from `ElementId` to `NodeId`.
2. Rename native node fields to use `id` terminology.
3. Remove compatibility-mirror id topology from native nodes.
4. Make native topology fully `NodeIx`-authoritative.
5. Keep behavior unchanged and keep boundary/runtime state keyed by `NodeId`.

See `plans/phase-5-nodeid-cleanup-plan.md` for the concrete scope.

### Phase 6. Native state split and dirty propagation

1. Split native node state into smaller responsibility-focused pieces as needed.
2. Replace coarse tree-wide dirty handling with per-node flags.
3. Propagate invalidation upward through parent links.

### Phase 7. Layout caches

1. Add intrinsic measurement cache.
2. Add resolved layout cache.
3. Add refresh traversal skipping.

## Success Criteria

This plan is successful if it gives us all of the following:

- simpler identity logic than the current semantic hash approach
- no id collision risk
- globally unique explicit keys suitable for future semantic APIs
- reorder-preserving identity within the same parent and host
- nearby identity preserved across slot changes on the same host
- no reparenting preservation for now
- faster patch apply on the Rust side
- a tree structure that makes layout caching natural instead of awkward

## Bottom Line

The right simplification for Emerge is:

- one shared `NodeId`
- one native `NodeIx`
- no `WireId`
- no semantic hash ids
- globally unique semantic keys
- reconciliation decides id reuse
- Rust uses dense indexed storage and explicit parent links

This keeps semantics shared where they matter, keeps the runtime fast where it
matters, and creates the right foundation for real layout caching.
