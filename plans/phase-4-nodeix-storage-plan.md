# Phase 4: Rust NodeIx Storage Rewrite

## Goal

Rewrite the native Rust tree storage to use an internal dense index (`NodeIx`)
while keeping shared runtime-id semantics unchanged.

This phase is intentionally structural only. It prepares the tree for later
dirty propagation and layout caching work, but does not implement them yet.

## In Scope

- internal `NodeIx` storage in Rust
- `id_to_ix` lookup map
- `root: Option<NodeIx>`
- child and nearby topology stored as `NodeIx`
- explicit parent/host links
- patch application rewritten around `NodeIx`
- layout/render traversal rewritten around `NodeIx`

## Out Of Scope

- dirty flags
- layout caches
- `Element` split into `spec/runtime/layout`
- moving `paint_children` out of the node
- event/runtime protocol changes
- Elixir-side changes
- behavior changes

## Why Phase 4 Exists

Current native costs are still dominated by structural issues:

- `ElementTree` is still `HashMap<NodeId, Element>`
- topology is stored as `Vec<NodeId>`
- layout/render recurse by id lookup
- parent discovery still scans the whole tree
- patch/remove/ghost logic rebuilds structure by repeated lookup/filter passes

This phase removes those costs first.

## Core Decision

Use:

- shared runtime id: `NodeId`
- native-only storage index: `NodeIx`

Keep external/native APIs id-based where useful for now, but make tree internals
index-based.

## Implemented Compatibility Shape

To keep behavior stable and avoid unnecessary churn in tests and helper code,
phase 4 uses a compatibility-friendly internal topology layer:

- `ElementTree` owns dense node storage plus `id_to_ix`
- `NodeIx` and parent links live in an internal topology structure
- id-based child and nearby fields remain on `Element` as compatibility mirrors
- layout, render, and patch internals can use the ix-based topology
- boundary and helper code can still reason about `NodeId`

This keeps the structural win while deferring the final removal of mirrored
id-based topology fields.

## Target Data Model

```rust
type NodeIx = usize;

struct ElementTree {
    revision: u64,
    next_ghost_seq: u64,
    current_scale: f32,
    root: Option<NodeId>,
    nodes: Vec<Option<ElementNode>>,
    id_to_ix: HashMap<NodeId, NodeIx>,
    free_list: Vec<NodeIx>,
    topology: RefCell<TreeTopology>,
    topology_dirty: Cell<bool>,
}
```

```rust
struct TreeTopology {
    root: Option<NodeIx>,
    parents: Vec<Option<ParentLink>>,
    children: Vec<Vec<NodeIx>>,
    paint_children: Vec<Vec<NodeIx>>,
    nearby: Vec<Vec<NearbyMountIx>>,
}
```

```rust
struct ElementNode {
    id: NodeId,
    parent: Option<ParentLink>,

    kind: ElementKind,
    attrs_raw: Vec<u8>,
    base_attrs: Attrs,
    attrs: Attrs,

    text_input_content_origin: TextInputContentOrigin,
    patch_content: Option<String>,

    children: Vec<NodeId>,
    paint_children: Vec<NodeId>,
    nearby: NearbyMounts,

    frame: Option<Frame>,
    measured_frame: Option<Frame>,

    mounted_at_revision: u64,
    residency: NodeResidency,
    ghost_attachment: Option<GhostAttachment>,
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

## Important Constraints

- Keep `paint_children` as a field on the node in phase 4.
- Keep existing runtime behavior unchanged.
- Keep boundary/event/runtime ids as `NodeId`.
- Use `NodeIx` only internally.
- Do not introduce cache state in this phase.
- Keep id-based topology fields on `Element` as compatibility mirrors for now.

## Tree API Direction

Keep these id-based accessors for compatibility:

- `get(&NodeId) -> Option<&ElementNode>`
- `get_mut(&NodeId) -> Option<&mut ElementNode>`
- `ix_of(&NodeId) -> Option<NodeIx>`

Add ix-native helpers:

- `get_ix(NodeIx) -> Option<&ElementNode>`
- `get_ix_mut(NodeIx) -> Option<&mut ElementNode>`
- `id_of(NodeIx) -> NodeId`
- `root_ix() -> Option<NodeIx>`
- `parent_link_of(NodeIx) -> Option<ParentLink>`
- `child_ixs(NodeIx) -> Vec<NodeIx>`
- `paint_child_ixs(NodeIx) -> Vec<NodeIx>`
- `nearby_ixs(NodeIx) -> Vec<NearbyMountIx>`
- `ensure_topology()`

This lets layout/render/patch internals move to `NodeIx` without forcing every
external call site to change immediately.

The later phase 5 cleanup keeps this shape but converges the naming model to:

- Elixir: `key` + `id`
- Rust: `NodeId` + `id` + `NodeIx`

## Structural Wins Expected

This rewrite should eliminate:

- whole-tree scans for parent lookup
- whole-tree scans for attachment lookup
- repeated child-id cloning during layout
- repeated hash lookups during retained traversal
- subtree removal that requires global detach scans

## File-by-File Plan

### 1. `native/emerge_skia/src/tree/element.rs`

This is the foundational rewrite.

Changes:

- introduce `NodeIx`
- change `ElementTree.root` to `Option<NodeIx>`
- replace `nodes: HashMap<NodeId, Element>` with arena storage plus `id_to_ix`
- convert `children`, `paint_children`, `nearby` to index-based topology
- add `parent: Option<ParentLink>`
- convert ghost attachment anchors to ix-based links
- rework:
  - `insert`
  - `get`
  - `get_mut`
  - `clear`
  - `replace_with_uploaded`
  - `live_child_ids`
  - `merge_live_children_with_ghosts`
  - `merge_live_nearby_with_ghosts`

### 2. `native/emerge_skia/src/tree/deserialize.rs`

Build the tree in two passes:

1. decode all raw nodes by `NodeId`
2. allocate `NodeIx` for each node and build `id_to_ix`
3. resolve child/nearby references into `NodeIx`
4. stamp parent links
5. set `root` to the root `NodeIx`

### 3. `native/emerge_skia/src/tree/serialize.rs`

Serialize by traversing `NodeIx`, but emit each node’s `NodeId`.

Changes:

- traverse from `root: NodeIx`
- map child/nearby `NodeIx` back to `NodeId`
- preserve exact wire behavior from phase 3

### 4. `native/emerge_skia/src/tree/patch.rs`

This is the second major rewrite.

Replace id-scan logic with ix-native logic.

Delete or rewrite away:

- `locate_attachment`
- `find_parent_id`

Rework:

- `SetAttrs`
- `SetChildren`
- `SetNearbyMounts`
- `InsertSubtree`
- `InsertNearbySubtree`
- `Remove`
- ghost capture
- ghost attach
- subtree removal
- descendant collection

All of these should operate on `NodeIx` after resolving boundary ids through
`id_to_ix`.

### 5. `native/emerge_skia/src/tree/layout.rs`

Convert layout traversal to `NodeIx`:

- measure pass
- resolve pass
- nearby traversal
- paint-order construction

Keep behavior identical.

The main win is eliminating child-id cloning plus repeated `tree.get(...)`.

### 6. `native/emerge_skia/src/tree/render.rs`

Convert retained traversal to `NodeIx`.

Still emit scene/event output using each node’s `NodeId`.

### 7. `native/emerge_skia/src/tree/scene.rs`

Update any helper signatures that assume id-based traversal.

This should be smaller than `layout.rs` and `render.rs`.

### 8. `native/emerge_skia/src/runtime/tree_actor.rs`

Keep incoming runtime messages id-based.

Minimal goal:

- resolve `NodeId -> NodeIx` through tree helpers
- let tree internals do ix-native work
- avoid broad actor API churn in this phase

## Recommended Execution Order

1. `tree/element.rs`
2. `tree/deserialize.rs`
3. `tree/serialize.rs`
4. `tree/patch.rs`
5. `tree/layout.rs`
6. `tree/render.rs`
7. `tree/scene.rs`
8. `runtime/tree_actor.rs`

## Testing Plan

### Tree Structure Tests

Add or strengthen tests for:

- `id_to_ix` is populated for every live node
- root is stored as `NodeIx`
- child nodes have correct `ParentLink::Child`
- nearby nodes have correct `ParentLink::Nearby`

### Patch Tests

Add or strengthen tests for:

- `insert_subtree` stamps parent links correctly
- `insert_nearby_subtree` stamps nearby host links correctly
- `set_children` updates parent links and leaves no stale links
- `set_nearby_mounts` updates nearby host links and leaves no stale links
- `remove` clears `id_to_ix` for removed live nodes
- ghost capture/attach preserves existing behavior

### Layout/Render Regression Coverage

Existing tests should remain green, especially:

- row and wrapped-row paint order
- paragraph float behavior
- nearby overlay ordering
- transform/clip retained traversal behavior

## Acceptance Criteria

Phase 4 is done when:

1. `ElementTree` is internally index-backed.
2. `root` is `Option<NodeIx>`.
3. `id_to_ix` exists and is authoritative.
4. child and nearby topology is stored as `NodeIx`.
5. mounted nodes have explicit parent/host links.
6. whole-tree parent/attachment scans are gone.
7. layout and render traverse by `NodeIx`.
8. patch/remove/ghost logic uses `NodeIx` plus parent links.
9. behavior is unchanged.
10. `mix test` passes.
11. `cargo test` passes.

## Explicit Non-Goals For This Phase

Do not add in phase 4:

- dirty flags
- intrinsic or resolved layout caches
- cache invalidation versions
- `NodeSpec` / `NodeRuntime` / `NodeLayoutState` split
- protocol changes
- Elixir reconciliation changes

Those belong to later phases.

## Bottom Line

Phase 4 should be a pure native storage/topology rewrite:

- shared `NodeId` stays as-is
- native internals move to `NodeIx`
- explicit parent links replace global scans
- layout/render/patching become index-driven
- no behavioral or caching work is mixed in yet

The cleanup that removes compatibility-mirror topology and unifies Rust naming
around `NodeId` belongs to phase 5 and is described in
`plans/phase-5-nodeid-cleanup-plan.md`.
