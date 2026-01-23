# Tree Encoding and Patching

This document summarizes how Emerge assigns ids, builds patches, and serializes trees.

## Identity Model
- Users build pure `Emerge.Element` trees (`row/column/el/text`).
- Identity is derived from `parent_id + kind + local_identity`.
- `local_identity` is `{:k, key}` when a key/id is provided, otherwise `{:i, index}`.
- Ids are generated via a fast hash of `{parent_id, kind, local_identity}`.
- Keys (via `key/1` or `id:`) must be unique across the whole tree; duplicates raise.

### Practical Rules
- Use keys for dynamic lists and reorderable children.
- Unkeyed children are matched by position; reordering will reassign ids.

## Reconciliation
`Emerge.Reconcile` compares the new tree to the previous vdom:
- Matches children by key when present, otherwise by index + kind.
- Reuses ids for matched nodes, assigns new ids for inserts.
- Computes minimal patch operations:
  - `:set_attrs` for changed attributes.
  - `:set_children` when surviving children reorder.
  - `:insert_subtree` for new nodes.
  - `:remove` for deleted nodes.

### `:set_children` optimization
- If changes are only inserts/removes and the remaining children keep their relative order, `:set_children` is skipped.
- If surviving children reorder, `:set_children` is emitted.

## Patch Encoding
Patches are encoded into a compact binary stream:
- Each patch is tagged (`set_attrs`, `set_children`, `insert_subtree`, `remove`).
- Ids are stored as `term_to_binary` with a length prefix.
- `insert_subtree` embeds a full subtree serialization.
- Runtime-only attributes (e.g., `scroll_x`, `scroll_y`) are stripped from `:set_attrs`.
- Attribute values use the typed encodings described in `EMRG_FORMAT.md`.

## Full Tree Serialization
`Emerge.Serialization.encode_tree/1` produces:
- Header: `"EMRG"` + version + node count.
- Per node:
  - id (length + term binary)
  - type tag
  - attrs (length + term binary)
  - child ids (count + length-prefixed term binaries)

Decoding rebuilds the tree by id references.

## Typical Flow
1. Build an element tree with `row/column/el`.
2. Call `Emerge.diff_state_update/2` to:
   - reconcile ids
   - emit binary patches
3. Send patches to the Rust side for incremental updates.
4. Use `Emerge.encode_full/2` for the initial upload.
