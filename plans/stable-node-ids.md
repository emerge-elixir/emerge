# Stable node IDs and one EMRG patch

**Implemented for the ordinary binary CPU headless renderer.** Elixir can tell what was declared and where it was attached without running layout or looking for a similar subtree. Rust decides which computed values still apply. The only tree mutation is `patch(target, subtree)`. Its payload is a sparse tree: an unchanged branch needs only its occurrence ID.

[Implementation algorithm and worked patch examples](stable-node-ids-algorithm.md) · [qualification and limits](stable-node-ids-results.md).

## Elixir: give each node two exact stamps

Keep the existing numeric occurrence ID (`#17`) for addressing patches and events. Alongside it, assign `<<branch::32, ancestry::32>>` in the viewport's `DiffState`. The two parts describe different facts; neither is a hint to search for some other node.

| Part | How to assign it | What equality means |
| --- | --- | --- |
| `branch` — leaves up | Intern `(kind, normalized native attributes and payload, ordered [(edge tag, child.branch)])`. Include text and listener presence; exclude occurrence IDs, DSL keys, callback addresses and runtime fields. | Same declared subtree **by value**, independently of its location or who owns it. Shape alone is insufficient: `text("Pay")` and `text("Cancel")` must differ. |
| `ancestry` — root down | The root has a reserved site. At each child intern `(parent.ancestry, parent occurrence ID, parent's locally declared inheritable attributes, edge tag, ordinal)`. An edge is an ordinary child or a Nearby slot. | Same declared ancestry and position. This is not a measured width, a screen coordinate or a clip. Don't include the parent's entire `branch`: editing one sibling must not relabel every other sibling's ancestry. |

`intern` is an exact descriptor → token map. Equal descriptors get the same `u32`; a hash collision does not merge unequal descriptors. Descriptors refer to immediate children's tokens, not to copies of their subtrees. Scope the tables to a viewport; bound live storage and never reassign a token while any retained state or cache can read it. Reject exhaustion rather than wrap.

**One walk of the new declaration, not a search of the old tree.** On entry, resolve the occurrence: direct lookup of live explicit `key + kind`, otherwise the same-kind unkeyed occurrence at this ordinal of this matched parent/edge list, otherwise fresh. Read old siblings with a list cursor, not indexed list lookups. Assign `ancestry` on entry. On return from the children, assign `branch` and compare the two parts with the old record for **this ID**. Maintain lookup indexes incrementally. No `branch → candidate nodes` index, candidate scoring, backtracking or recursive equality check. A freshly built unstamped declaration must still be read; assigning stamps is bookkeeping, not measurement. A move may change descendant ancestry stamps during that walk, not their branch stamps or necessarily any computed value.

| Comparing the same occurrence | Conclusion before native computation |
| --- | --- |
| Same `branch`, different `ancestry` | Same declared value, new context source. Promoting `#17` to root is this case; its old measurement is not invalid just because the path changed. |
| Different `branch`, same `ancestry` | A declaration changed here or below, not necessarily the layout. Keep the actual changed node as the patch target; don't send the root just because its branch stamp changed too. |
| Both different | Both facts changed; e.g. move a keyed card and edit its label. |
| Both equal | No change to those two facts. Check **bindings separately**: compare each parent's ordered child occurrence IDs/slots and each occurrence's callback routes during the same walk. Swapping equal-valued *keyed* buttons changes order; changing only the callback changes the route, not the native declaration. |

Two identical **unkeyed** siblings swapping retain IDs `[17, 23]` in place: no patch, route edit or invalidation. A new copy can have the same `branch` but never inherits the first copy's press. A keyed subtree moved across parents keeps its occurrence ID and its matched unkeyed children. Without an explicit key or an already identified occurrence, don't guess which displaced unkeyed subtree it used to be: a fresh occurrence can still reuse the same values. Keep duplicate-key and mixed-sibling validation; absence from an accepted declaration ends that occurrence even if never drawn.

## What can be stored under a part?

**Equal keys must imply equal results for everything stored there.** A lookup by `branch` returns one immutable value or rule, not a list of mounts to inspect. Store a final result by `branch` alone only if no other input can change it. For example, two equal wrapped rows can share intrinsic measurement when their effective measurement inputs agree; text with identical declarations can still inherit 16px in one parent and 24px in another. For that text, key the glyph result by text and effective strike, not by shape or path. The same branch can instead identify the measurement rule to apply to either strike.

This distinction applies even to the full pair. The same row at the same declared site can receive a different width when a sibling grows or the viewport resizes. Width-dependent reflow needs the actual offer and settled child inputs; it may be reusable across *different* parents if those inputs agree. Placement, clip, hits and live attachment state are not intrinsic measurements. Elixir need not calculate any of them. Compare or look up actual inputs in the [native layers](../native/emerge_skia/src/tree/layers.rs); a cache miss is not a reason to walk the tree looking for another occurrence.

## One sparse patch: an ID is enough to keep a branch

`patch(target, subtree)` takes two arguments: the old occurrence to replace and the sparse subtree that belongs there. The subtree's root is a node record, a bare occurrence ID to keep, or empty. A record supplies one node's kind, attributes and children. Each of the latter two fields independently says keep or replace. An ID with **no record** means **keep that exact live occurrence and everything declared beneath it**. This applies to the replacement root too. Omitting a child's *record* keeps it if referenced; omitting its *edge* from an explicit final order removes it from that parent.

- `attrs: :same` keeps the last accepted local native attributes/payload and
  sends no attribute block. A map is a **full replacement**, not a merge:
  `attrs: %{}` explicitly clears the local declarations.
- `children: :same` keeps the ordered children/mounts and everything beneath
  them; `children: []` means no children. An attribute-only edit to a parent
  with 10,000 children sends **no child IDs**.

A fresh occurrence supplies both fields explicitly. `:same` is a field marker
inside `patch`, not another operation. Keeping attributes does not prevent
edits below the node or changes to its inherited context.

For attribute edits at depths 2, 4 and 7 on one path, start at `#2`.
The maps below are the complete new attributes for those nodes:

```text
patch(#2, subtree)  // subtree starts with the record for #2 below
#2  attrs: %{padding: 12}  children: [#3, #20]
#3  attrs: :same           children: [#4, #30]
#4  attrs: %{spacing: 8}   children: [#5, #40]
#5  attrs: :same           children: [#6, #50]
#6  attrs: :same           children: [#7, #60]
#7  attrs: %{color: :blue} children: :same
```

Six shallow records, for `#2` through `#7`; none for `#20`, `#30`, `#40`, `#50`, `#60` or their descendants. Nodes 3, 5 and 6 connect the path with ordinary records. If **only** `#2` changes, send its record with `children: :same`. Cost is the opened records and any explicit child lists, not the size of the branches behind them. If one child of a very wide opened parent also needs a record, the explicit list still costs one ID per child; no list-delta language.

**Elixir decides where to stop during the stamp walk.** On return from each node, mark its child list keepable if that occurrence's ordered IDs/slots are unchanged and all its children are keepable. The node itself is keepable if it already exists and its own native attributes/payload are unchanged too. Fold this bit up with `branch`; the value stamp alone would miss a reorder of equal-valued keyed grandchildren. Keepability excludes the *incoming* edge, so an intact `#17` can move by reference. Callback-only changes travel via the route edits. Emit records on edited paths and for new nodes; stop encoding at every keepable branch. No second search for matching nodes, no layout and no `set_attrs`.

- For a move, target the lowest common ancestor enclosing both old and new owners. Expand both changed parent lists, but reference the moved subtree if intact. For root promotion, `patch(1, 17)` puts the retained subtree `#17` in place of root `#1`. Target `0` addresses the vacant viewport on first mount, where all nodes need definitions. An empty subtree clears the target; `none()` remains an ordinary element. No reset or special root operation.
- Resolve bare IDs by exact lookup against the **accepted declaration immediately before this patch**, not the last draw. A reference must be live inside the old target, not merely cached. Validate the combined topology and absolute depth, including retained branches: reject unknown/outside references, duplicate ownership, cycles (including overlaps with retained descendants), and records unreachable without crossing a keep boundary. Reuse the existing topology proof for a kept branch: check touched owners via parent links and a moved branch's depth bound via its retained height, rather than scanning untouched descendants to interpret a reference. Reserve before committing; reject a bad batch without altering earlier accepted edits.
- **Keeping a subtree means no declaration edit beneath it, not no computation.** Keeping only its attributes leaves its children open to edits. Apply all accepted patches in order → invalidate once against the final tree → recompute. An inherited font edit at `#2` can still change measurement under `#20`; promoting `#17` can change its width offer, origin and clip. Propagate changed inputs, not the fact that an ID appeared on the wire. Hits and routes still install only on successful draw.

## Checks, in order

1. **Elixir only, no renderer.** Check the four stamp cases, two copies with one branch value and distinct IDs, identical unkeyed swap, equal-valued keyed reorder, callback-only edit, changing one sibling's width, keyed root promotion, and a true absence. Force descriptor hash collisions. Count visited new records and require zero old-subtree candidate visits, including for deep chains and many identical siblings. Assigned, binary-only and routed outputs must agree on IDs. Preserve controlled-scroll resolution against the final live index.
2. **Wire cutover.** Replace the [multi-command codec](../lib/emerge/engine/patch.ex) and [clearing upload path](../native/emerge_skia/src/tree/wire/upload.rs) together, without a fallback. Assert exact targets and sparse record IDs in the example above, and replay equality with a fully declared final tree. Growing a kept branch from 1 to 1,000 nodes must not add patch bytes. An attribute-only edit on a wide parent must emit no child IDs; distinguish `:same` from explicit empty attributes and empty children. On a path node with nonempty attributes, assert no attribute block on keep, preservation on replay, and clearing on `%{}`. Test first mount, empty vs kept root, reordered equal-valued keyed grandchildren, and a keep referencing an earlier accepted but undrawn edit. Reject missing IDs, implicit double ownership and unreachable records atomically. Reverse the three [fresh-ID reparent expectations](../test/emerge/diff_state_test.exs#L371), not the two absence tests. Preserve existing behavior gates; Nearby identity in Elixir does not imply native Nearby support.
3. **Reuse and behavior, separately.** Check one shared intrinsic evaluation for two identical cards under equal effective inputs; compare both to cold output under different font sizes, width offers, origins and clips. Replaying an identical settled root schedules no compute. On one capped viewport, move a 20px card between two 60px parents: `(5,5)` reaches the old route before draw, then misses; `(65,5)` reaches the new site at local `(5,5)`. A continuous press is distinct from a remount. Verify fitting allocations, atomic rejection, mapped NIF and named suite failures against the [96-failure checkpoint](keyed-insert-remove.md#results). Track wire bytes, bookkeeping and computation separately; no timing claim.

Research basis: [AHU](https://arxiv.org/html/2401.07636v1#S3) supplies the bottom-up labeling idea (preserve UI child order). [XyDiff](https://www.cs.rutgers.edu/~amelie/papers/2002/diff.pdf) and [GumTree](https://www.labri.fr/perso/xblanc/data/papers/ASE14.pdf) search for candidate matches; that step is deliberately **not** part of this design. [Nominal Adapton](https://arxiv.org/abs/1503.07792) distinguishes a stable name from the value computed under it.
