# Stable IDs and sparse `patch(target, subtree)`

**Implemented for the binary CPU headless path.** Given the last *accepted*
declaration and the next Elixir tree, return a candidate `DiffState`, **zero or
one tree patch**, and independent callback-route edits. Walk the new declaration
once to match occurrences; fold up which branches can be kept and where to start
the patch. No layout, old-tree reindex, or search for similar subtrees. The
[requirements](stable-node-ids.md) remain the contract.

## 1. Assign the occurrence on entry; stamp the value on exit

Keep viewport-local `key → live ID` and `ID → node` indexes. A node stores
its kind, owner, ordered edges, normalized native attributes/payload, and two
stamps. Compare against the immutable old indexes throughout the walk; write
into a candidate. Use list cursors for old/new siblings, prepend and reverse
once.

- **Enter:** for an explicit key, look up the exact old key and require the same
  kind, regardless of parent. For an unkeyed node, use the old unkeyed node of
  the same kind at this ordinal in the matched parent's corresponding edge list
  (the viewport supplies the root position). Otherwise allocate a fresh, checked
  positive `u64`. Claim once; reject duplicate explicit keys and mixed-key
  sibling lists. For a moved parent, its old child list follows its ID.
- **Stamp:** pack `<<branch::32, ancestry::32>>` separately from that occurrence
  ID. Use two exact descriptor maps, not truncated hashes:

  ```text
  on entry: ancestry = ROOT_SITE, or intern(parent.ancestry, parent.id,
                           parent.local_inheritable_attrs, edge_tag, ordinal)
  on exit:  branch   = intern(kind, native_attrs_and_payload,
                           ordered [(edge_tag, child.branch)])
  ```

  Interning returns the same token for equal descriptors, using exact descriptor
  equality even on a hash collision. Reuse an interned own-attributes token;
  never recursively compare child trees. A `branch` change means a value changed
  here or below; an `ancestry` change means the declared site changed. Neither
  requires layout to detect. Keys, own ID and callback addresses are not native
  values; text, listener presence and resolved native bindings are. Drop
  descriptors once neither accepted/candidate state nor a cache holds them;
  reject counter exhaustion rather than wrap. Stamps stay in Elixir.
- **Forward key references:** for a declaration such as `scroll_y(:target)`,
  finalize the numeric target from the new key index (missing → `0`); retain
  the declaration token if the requested key is unchanged. Defer the exit fold
  for that owner and any waiting ancestors in postorder until IDs are known. No
  second tree search. Diff callback addresses by ID separately. After all
  claims, retire displaced old branches, stopping at claimed survivors; never
  delete a route because its old parent was visited first.

## 2. Fold up both the keep decision and the patch target

For each new node `n` let `old` mean the record for **this occurrence ID**, not
a record found by stamp. Fold the following while closing its child list:

```text
own_equal   = old exists and its kind and native attributes/payload equal n's
order_equal = old exists and its ordered (edge tag, child ID) list equals n's
list_same   = order_equal and every child is keepable
keepable    = own_equal and list_same

cover(n) = none                 if keepable
         = cover(changed_child) if own_equal and order_equal
                                   and exactly one child is not keepable
         = n                    otherwise
```

Neither incoming ancestry nor callback address participates in `keepable`. A
move can keep the subtree's declaration. Conversely, equal branch stamps do
**not** hide a reorder of equal-valued *keyed* children: their edge IDs differ.
Identical *unkeyed* twins keep their two positional IDs, so swapping them
yields no patch.

If the root ID is unchanged, target `cover(root)`; `none` means **no tree
bytes**. If the root changes, target the old root and emit the new root as the
body. On first mount the target is the vacant position `0`; clearing it uses an
empty body. These are address choices, not different mutations. The fold
descends past a parent only when its own declaration and order are unchanged
and every other branch stays put. Thus a move between two owners stops at their
lowest common ancestor, including when one owner is inserted or removed.
Choosing one enclosing patch avoids coordinating temporary removals or ordering
disjoint edits.

## 3. Emit only the opened paths

```text
emit(n):
  if keepable: return n.id
  attrs    = if own_equal then :same else n.full_native_attrs_and_payload
  children = if list_same then :same else [(edge, emit(child)), ...]
  return node(n.id, n.kind, attrs, children)
submit patch(target, emit(selected_subtree))
```

A bare ID keeps that exact live subtree. In an opened record, `attrs: :same`
keeps the occurrence's last accepted local attributes/payload and sends no
attribute bytes beyond the marker. An explicit map replaces the whole set;
`attrs: %{}` clears it. Independently, `children: :same` keeps the old ordered
children/mounts and their subtrees; `children: []` removes them all. A fresh
node must supply both fields explicitly. Use distinct keep/replace tags so an
empty map is never decoded as `:same`. Encode the versioned EMRG body as
iodata; framing is not a third patch argument. Publish candidate state and
route edits only upon successful admission. An unchanged tree with
changed callbacks sends just the route edits through the existing draw-ordered
route path.

**Cases and exact expected results:**

- **Only `#2`'s color changes; it has 10,000 children:** Target `#2`; one
  shallow node with `children: :same`; zero child IDs.
- **Attr edits at `#2`, `#4`, `#7` on path `2→3→4→5→6→7`:**
  Nodes `#2`, `#4`, `#7` send full replacement attributes. Connecting nodes
  `#3`, `#5`, `#6` send `attrs: :same` with explicit children. Side branches
  are bare IDs; `#7` sends `children: :same`.
- **Move intact keyed `#17` from left to right:** Open the common ancestor and
  changed owners' paths. Old parent omits `17`, new parent references `17`; no
  record for the card.
- **Promote the same card to replace root `#1`:** `patch(1, 17)`. No descendant
  records and no cache reset.

## 4. Apply declarations, then invalidate and recompute

Under the session owner, validate in reserved scratch against the state
immediately before **each** patch, including accepted undrawn edits, not the
installed frame. Index definitions by ID; from the body root follow explicit
edges, checking kind/arity, uniqueness and reachability. A bare ID must be live
inside the old target; either `:same` field requires a compatible live
occurrence. Only a bare ID or `children: :same` closes a subtree boundary;
`attrs: :same` does not close it. For each independently named live ID, follow
old parent links: reject an ID outside the target or strictly below another
closed boundary. This rejects keeping `#4` while also naming its old grandchild
elsewhere, without walking `#4`. Reject cycles and duplicate definitions/owners.
Maintain heights/counts on edited ancestor paths; a moved kept branch adds its
retained height to its new base depth. Check byte/node/depth limits and reserve
resident/candidate overlap, pools and scratch before commit. Failure preserves
earlier pending edits and drawn output.

Commit changed owner lists and declarations. For edited edges, displaced minus
retained detaches; untouched interiors of kept branches stay live. Detaching
does not require freeing storage. Preserve continuous attachments and the first
replaced inputs. Keep compute worksets empty until all accepted patches for the
cycle are applied. **Invalidate once** against final membership, including an
owner losing its last child. Get scheduling depths from final parent links,
memoized only for the current topology. After promotion the viewport supplies
the card's inheritance, width offer, origin and clip; a kept declaration does
not freeze these values.

For a scheduled pass, look up an immutable result by the inputs that determine
it: e.g. glyphs by text and effective strike; intrinsic box size by kind,
sizing inputs, scale and settled child measures. On miss, compute and store
once; on hit, bind the existing value to this mount. Compare outputs to this
mount's prior results and propagate only changed inputs; service new child
edges even on a value hit. Never share a live hit target or press. A different
offer can change reflow without changing intrinsic measurement. Install hits
and callback routes only on successful draw, including actual pixel reuse.

## Cost and qualification gates

One enter/exit per new node, plus processing immediate edges/attribute data,
displaced removals and any deferred references. Encoding visits opened records
and their explicit edge lists only; a wide *opened* list can still cost one ID
per child. Native checks may follow ancestor paths; actual detachments and
changed dependencies cost work. This is not a claim of constant-time
reconciliation.

Qualification order: **(1)** pure Elixir matcher/fold and replay (including
forced hash collisions, nested moves and root changes); **(2)** cut over writer
and reader together, with rejection and fitting-allocation controls; **(3)**
verify shared measurement vs changed context, and drawn pixels/raw clicks on an
ordinary capped viewport. Check exact record/byte counts above, zero old-subtree
candidate searches, and incremental replay against a cold final declaration.
With padding already set on `#3`, a child edit with `attrs: :same` must keep
that padding without sending it; `attrs: %{}` must clear it. Reject either
`:same` field on a fresh node without changing earlier accepted edits.
Compare named suite failures to the
[96-failure checkpoint](keyed-insert-remove.md#results); no new exclusions or
performance claim. See the [qualification results](stable-node-ids-results.md).
