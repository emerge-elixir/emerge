# Tree Encoding and Patching

This document summarizes how Emerge assigns ids, builds patches, and serializes
trees.

## Identity Model

- Users build pure `Emerge.Element` trees.
- Normal child identity is derived from `{parent_id, kind, local_identity}`.
- `local_identity` is `{:k, key}` when a key is provided, otherwise `{:i, index}`.
- Nearby root identity is derived from `{host_id, slot, local_identity}`.
- Keys must be unique across the whole tree; duplicates raise.
- If any siblings in a normal child list are keyed, all siblings in that list
  must be keyed.

## Reconciliation

`Emerge.Reconcile` keeps public nearby attrs but normalizes them internally into
host-owned mount slots.

- normal `children` reconcile by keyed-or-indexed sibling matching
- nearby mounts reconcile per fixed slot
- nearby changes do not flow through `:set_attrs`
- nearby root replacement is structural (`:remove` + insert)

## Patch Operations

Current patch operations are:

- `:set_attrs`
- `:set_children`
- `:insert_subtree`
- `:insert_nearby_subtree`
- `:remove`

### Notes

- `:set_children` only applies to normal child order.
- Nearby uses fixed slots, so it does not need a `:set_nearby` reorder/update op.
- Replacing a nearby root uses `:remove` for the old root and
  `:insert_nearby_subtree` for the new root.
- Runtime-only attrs are stripped from `:set_attrs` payloads.
- Nearby attrs are also stripped from `:set_attrs`; they travel through
  structural patch ops only.

## Full Tree Serialization

`Emerge.Serialization.encode_tree/1` now emits EMRG v4.

- Header: `"EMRG" + version + node_count`
- Per node:
  - id
  - type tag
  - attrs block (ordinary attrs only)
  - child ids
  - nearby mount refs

Nearby is serialized as retained node edges, not as nested subtree blobs inside
attr values.

## Typical Flow

1. Build an element tree with public attrs, including nearby attrs when needed.
2. Reconcile it against the previous vdom.
3. Emit nearby-aware structural patches and attr patches.
4. Send patches to Rust for incremental retained-tree updates.
5. Use full-tree EMRG upload for initial tree state.
