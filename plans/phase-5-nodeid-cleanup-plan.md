# Phase 5: NodeId Naming And Topology Cleanup

## Goal

Finish the native cleanup started by phase 4.

Phase 4 introduced `NodeIx` as the internal storage/traversal index, but kept a
compatibility layer around the old native naming and around id-based topology
fields on `Element`.

Phase 5 should:

- unify shared runtime identity naming around `NodeId`
- remove compatibility-mirror id topology from native tree nodes
- make the internal tree topology fully `NodeIx`-authoritative
- keep behavior unchanged

This is still a cleanup phase, not a caching phase.

## Why This Phase Exists

After phase 4, many shared-runtime-id references are still present for four different
reasons:

1. **Essential shared-runtime-id usage**
   - wire decode/encode
   - patch decode
   - event payloads to Elixir
   - runtime maps that must survive tree mutations

2. **Phase 4 compatibility mirrors**
   - `Element.id`
   - `Element.children: Vec<NodeId>`
   - `Element.paint_children: Vec<NodeId>`
   - `NearbyMount.id`

3. **Internals that still walk by shared id instead of `NodeIx`**
   - some layout traversal
   - some patch bookkeeping
   - some registry rebuild traversal

4. **Pure rename debt**
   - tests, helper constructors, comments, and docs still using mixed or stale
     terminology

Phase 5 addresses categories 2, 3, and 4 while preserving category 1 under the
final naming model.

## In Scope

- rename the Rust shared runtime-id type from `ElementId` to `NodeId`
- keep the Rust field name as `id`
- keep the Elixir field name as `id`
- removing mirrored id-based topology fields from native nodes
- making `TreeTopology` authoritative for child / paint-child / nearby topology
- converting remaining internal tree traversal and bookkeeping to `NodeIx`
- updating tests, comments, docs, and plan files to the new naming

## Out Of Scope

- Elixir changes
- wire/protocol changes
- event-runtime behavior changes
- dirty flags
- layout caches
- `NodeSpec` / `NodeRuntime` / `NodeLayoutState` split

## Core Decision

After this phase, the naming model should be:

- Elixir: `key` + `id`
- Rust: `NodeId` type + `id` field + `NodeIx` internal index

That gives one clean distinction:

- `key`: semantic/user identity
- `id`: shared runtime identity
- `NodeIx`: native-only storage/traversal index

Important:

- phase 5 is not renaming the Rust field away from `id`
- phase 5 is renaming the Rust type to `NodeId`
- Elixir already uses `key` + `id` as the target naming model

## Target Native Shape

```rust
type NodeId = u64;
type NodeIx = usize;

struct ElementTree {
    revision: u64,
    next_ghost_seq: u64,
    current_scale: f32,
    root: Option<NodeIx>,
    nodes: Vec<Option<ElementNode>>,
    id_to_ix: HashMap<NodeId, NodeIx>,
    free_list: Vec<NodeIx>,
    topology: TreeTopology,
}
```

```rust
struct TreeTopology {
    parents: Vec<Option<ParentLink>>,
    children: Vec<Vec<NodeIx>>,
    paint_children: Vec<Vec<NodeIx>>,
    nearby: Vec<Vec<NearbyMountIx>>,
}
```

```rust
struct ElementNode {
    id: NodeId,
    kind: ElementKind,
    attrs_raw: Vec<u8>,
    base_attrs: Attrs,
    attrs: Attrs,
    text_input_content_origin: TextInputContentOrigin,
    patch_content: Option<String>,
    frame: Option<Frame>,
    measured_frame: Option<Frame>,
    mounted_at_revision: u64,
    residency: NodeResidency,
    ghost_attachment: Option<GhostAttachmentIx>,
    ghost_capture_scale: Option<f32>,
    ghost_exit_animation: Option<AnimationSpec>,
}
```

```rust
struct NearbyMountIx {
    slot: NearbySlot,
    ix: NodeIx,
}

enum ParentLink {
    Child { parent: NodeIx },
    Nearby { host: NodeIx, slot: NearbySlot },
}

enum GhostAttachmentIx {
    Child { parent: NodeIx, live_index: usize, seq: u64 },
    Nearby { host: NodeIx, mount_index: usize, slot: NearbySlot, seq: u64 },
}
```

## Expected Cleanup Outcomes

After phase 5:

- production Rust code no longer uses `ElementId`
- production Rust node structs use `id` consistently for shared runtime identity
- native node structs no longer store mirrored child / paint-child / nearby
  topology by shared id
- `TreeTopology` is the authoritative native topology
- remaining shared-id usage exists only at boundaries and in runtime state where
  stable identity is semantically required

## File-By-File Plan

### 1. `native/emerge_skia/src/tree/element.rs`

This is the main cleanup file.

Changes:

- rename `ElementId` to `NodeId`
- keep `Element.id` as `id`
- keep `NearbyMount.id` as `id`
- remove mirrored topology fields from `Element`
  - `children`
  - `paint_children`
  - `nearby`
- move remaining ghost attachment metadata to `GhostAttachmentIx`
- make `TreeTopology` authoritative rather than lazily rebuilt from mirrored
  topology
- keep ix/id bridge helpers:
  - `ix_of(&NodeId)`
  - `id_of(NodeIx)`
  - `get(&NodeId)`
  - `get_ix(NodeIx)`

### 2. `native/emerge_skia/src/tree/deserialize.rs`

Build authoritative topology directly during decode.

Changes:

- rename to `NodeId`
- stop decoding into temporary mirrored id lists on nodes
- build `TreeTopology.children`, `paint_children`, `nearby`, and `parents`
  directly after allocating `NodeIx`
- set `root` directly to `Option<NodeIx>`

### 3. `native/emerge_skia/src/tree/serialize.rs`

Serialize from authoritative `NodeIx` topology.

Changes:

- rename to `NodeId`
- traverse from `root: NodeIx`
- emit child and nearby refs by mapping `NodeIx -> NodeId`

### 4. `native/emerge_skia/src/tree/patch.rs`

Convert patch internals from mirrored id topology mutation to topology mutation.

Changes:

- rename to `NodeId`
- patch structs still stay boundary-id based
- after resolving ids, mutate topology through `NodeIx`
- remove id-based child / nearby mutation from nodes
- convert ghost attachment bookkeeping to `GhostAttachmentIx`
- convert descendant collection to ix-based traversal

### 5. `native/emerge_skia/src/tree/layout.rs`

Finish ix-native layout traversal.

Changes:

- rename to `NodeId`
- stop reading `element.children` / `element.nearby`
- read authoritative topology from `TreeTopology`
- keep emitted layout state keyed by `NodeId` only where external identity is needed

### 6. `native/emerge_skia/src/tree/render.rs`

Remove remaining bridge lookups.

Changes:

- rename to `NodeId`
- stop reading compatibility mount/child ids from nodes
- consume retained traversal data directly from authoritative topology
- recover `NodeId` only at scene/event emission boundaries

### 7. `native/emerge_skia/src/events/registry_builder.rs`

Finish ix-native subtree traversal.

Changes:

- rename to `NodeId`
- registry rebuild traversal should walk `NodeIx`
- final listener/runtime state remains keyed by `NodeId`

### 8. `native/emerge_skia/src/tree/animation.rs`

Keep runtime maps keyed by shared identity, but rename terminology.

Changes:

- rename to `NodeId`
- keep animation runtime keyed by stable `NodeId`

### 9. `native/emerge_skia/src/events.rs`
### 10. `native/emerge_skia/src/events/runtime.rs`
### 11. `native/emerge_skia/src/actors.rs`
### 12. `native/emerge_skia/src/lib.rs`

These are mostly rename fallout and helper adaptation.

### 13. Tests, helpers, docs, and plans

Update:

- native tests using `NodeId::from_term_bytes(...)`
- docs/comments that still describe `NodeId`
- plan files and guides so they consistently describe `NodeId`

## Implementation Passes

Phase 5 should be implemented in four passes. Each pass should compile and test
before moving on.

### Pass 1. Naming Cleanup

Goal:

- remove the `ElementId` / `NodeId` terminology split in Rust
- avoid changing behavior yet

Do:

- rename `ElementId` to `NodeId`
- keep `Element.id` as `id`
- keep `NearbyMount.id` as `id`
- rename helper APIs such as `node_id_of(...)` to `id_of(...)`
- rename local variables and comments accordingly

Do not do yet:

- remove mirrored topology fields
- change traversal strategy

Primary files:

- `native/emerge_skia/src/tree/element.rs`
- `native/emerge_skia/src/tree/deserialize.rs`
- `native/emerge_skia/src/tree/serialize.rs`
- `native/emerge_skia/src/tree/patch.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/render.rs`
- `native/emerge_skia/src/tree/animation.rs`
- `native/emerge_skia/src/events.rs`
- `native/emerge_skia/src/events/runtime.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
- `native/emerge_skia/src/actors.rs`
- `native/emerge_skia/src/lib.rs`

Checkpoint:

- `cargo check`
- `cargo test`

### Pass 2. Make Topology Authoritative

Goal:

- stop rebuilding ix topology from mirrored id lists
- move topology ownership fully into `TreeTopology`

Main file:

- `native/emerge_skia/src/tree/element.rs`

Do:

- change `ElementTree.root` to `Option<NodeIx>`
- remove mirrored topology fields from `ElementNode`
  - `children`
  - `paint_children`
  - `nearby`
- remove lazy rebuild state
  - `RefCell<TreeTopology>`
  - `Cell<bool>`
  - `ensure_topology()`
  - `mark_topology_dirty()`
- keep bridge helpers:
  - `ix_of(&NodeId)`
  - `id_of(NodeIx)`
  - `get(&NodeId)`
  - `get_ix(NodeIx)`
  - `get_ix_mut(NodeIx)`
- convert ghost attachment storage to `GhostAttachmentIx`
- update retained traversal helpers:
  - `for_each_retained_child`
  - `for_each_retained_local_branch`

Checkpoint:

- `cargo check`

### Pass 3. Rebuild Native Algorithms Around Authoritative Topology

Goal:

- remove the last internal dependencies on mirrored id-based topology

#### 3.1 `native/emerge_skia/src/tree/deserialize.rs`

Do:

- decode raw nodes by `NodeId`
- allocate `NodeIx`
- fill `id_to_ix`
- build `topology.children`, `topology.paint_children`, `topology.nearby`, and
  `topology.parents` directly
- set `root: Option<NodeIx>` directly

#### 3.2 `native/emerge_skia/src/tree/serialize.rs`

Do:

- traverse authoritative topology starting from `root: NodeIx`
- emit child and nearby references by mapping `NodeIx -> NodeId`

#### 3.3 `native/emerge_skia/src/tree/patch.rs`

This is the highest-risk file in phase 5.

Target functions:

- `apply_patches`
- `locate_attachment`
- `attach_ghost_root`
- `clone_as_ghost`
- `remap_nearby_mounts`
- `remove_subtree`
- `collect_descendants`

Do:

- keep patch structs boundary-id based
- after resolving ids, mutate authoritative topology through `NodeIx`
- stop mutating child / nearby topology on nodes
- convert descendant collection to ix-based traversal
- convert ghost bookkeeping to `GhostAttachmentIx`

Checkpoint:

- `cargo check`
- focused native tests for patch/tree behavior if useful
- `cargo test`

### Pass 4. Finish Internal Traversal Cleanup

Goal:

- ensure layout, render, and registry traversal read topology only from
  authoritative `TreeTopology`

#### 4.1 `native/emerge_skia/src/tree/layout.rs`

Target functions:

- `measure_element`
- `resolve_element_sizing`
- `resolve_element`
- `refresh`

Do:

- stop reading node-owned `children` / `nearby`
- walk `NodeIx` topology directly

#### 4.2 `native/emerge_skia/src/tree/render.rs`

Target functions:

- `build_element_subtree`
- `build_host_content_subtree`
- `build_nearby_mount_subtree`

Do:

- remove remaining bridge lookups from `NodeId` back to `NodeIx`
- consume authoritative topology directly
- recover `NodeId` only at scene/event emission boundaries

#### 4.3 `native/emerge_skia/src/events/registry_builder.rs`

Target function:

- `accumulate_subtree_rebuild_local`

Do:

- recurse by `NodeIx`
- keep final listener/runtime payloads keyed by `NodeId`

#### 4.4 Supporting files

- `native/emerge_skia/src/tree/animation.rs`
- `native/emerge_skia/src/events.rs`
- `native/emerge_skia/src/events/runtime.rs`
- `native/emerge_skia/src/actors.rs`
- `native/emerge_skia/src/lib.rs`

These should mostly be rename fallout and helper adaptation.

Checkpoint:

- `cargo test`
- `mix test`

## Function-Level Hotspots

The most important functions to touch during implementation are:

- `ElementTree::with_attrs`
- `ElementTree::get`
- `ElementTree::get_mut`
- `ElementTree::ix_of`
- `ElementTree::id_of`
- `ElementTree::live_child_ids`
- `ElementTree::merge_live_children_with_ghosts`
- `ElementTree::merge_live_nearby_with_ghosts`
- `decode_tree`
- `encode_tree`
- `apply_patches`
- `locate_attachment`
- `attach_ghost_root`
- `remove_subtree`
- `collect_descendants`
- `measure_element`
- `resolve_element`
- `build_element_subtree`
- `accumulate_subtree_rebuild_local`

## Testing Strategy

Run tests after every pass.

Use:

- `cargo check`
- `cargo test`
- `mix test`

Native tests most likely to need updates:

- `native/emerge_skia/src/tree/patch.rs` tests
- `native/emerge_skia/src/tree/render/tests/*`
- `native/emerge_skia/src/tree/layout/tests/*`
- `native/emerge_skia/src/events/registry_builder.rs` tests

## Practical Rule

Do not try to eliminate every `NodeId` from transient helper structs.

It is fine for some transient traversal structs to carry both:

- `ix`
- `id`

The real cleanup target is:

- no mirrored topology on nodes
- no topology rebuild from ids
- no dual naming convention

## Recommended Execution Order

1. rename the shared-id type and fields
2. make topology authoritative in `tree/element.rs`
3. update `deserialize.rs`
4. update `serialize.rs`
5. update `patch.rs`
6. update `layout.rs`
7. update `render.rs`
8. update `registry_builder.rs`
9. update runtime/event/helper files
10. update tests and docs

## Acceptance Criteria

Phase 5 is done when:

1. production Rust code no longer uses `ElementId`
2. production Rust node structs use `id` consistently for shared runtime identity
3. native nodes no longer store mirrored child / paint-child / nearby topology by id
4. `TreeTopology` is authoritative
5. `root` is `Option<NodeIx>`
6. internal traversal uses `NodeIx`
7. boundary/wire/event/runtime state still uses `NodeId`
8. behavior is unchanged
9. `cargo test` passes
10. `mix test` passes

## Non-Goals

Do not mix into phase 5:

- dirty flags
- invalidation propagation
- cache entries
- layout cache keys
- Elixir-side changes

Those belong to the following phases.

## Bottom Line

Phase 4 introduced `NodeIx`.

Phase 5 should finish the cleanup by:

- making naming consistent (`NodeId` + `NodeIx`)
- removing compatibility-mirror topology from native nodes
- making native topology fully ix-authoritative

It is the phase that makes the Rust side match the intended cross-language model:

- Elixir: `key` + `id`
- Rust: `NodeId` + `id` + `NodeIx`

That gives a much cleaner base for the actual performance phases that follow.
