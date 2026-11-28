# Nearby: layers and depth within a layer

**Implemented for block anchors on the retained shapes.** See [verification and limits](#verification-and-limits). Keep one ownership tree. Make a layer a **run of paint visits**, not a layout pass, a numeric z-index or a Skia surface. Within a run, depth is the existing painter's order: local phases, children in declaration order, mounts on one host in attribute order. A later eligible hit wins in the reverse of that order. Neither alignment nor a slot tag breaks ties.

## Observable order

Suppose the root has `in_front(scrim)`. Its ordinary children A and B, declared in that order, each own a menu; A's menu itself owns a tooltip. All four overlays can overlap. Back to front:

| Layer | Depth within the layer (back → front) |
| --- | --- |
| Base | Root, A's ordinary content, B's ordinary content, in their normal paint phases |
| Branch menus | A.menu, **B.menu** |
| Menus' own Nearby | **A.menu.tip** (then B.menu.tip if present) |
| Root's Nearby | **scrim** |

The tip is above **both** menus, not just A's, but cannot cover the scrim. Swapping A and B swaps the menu winner, not the layers. A Nearby on the scrim can paint above the scrim. `behind_content` stays **local**, after the host backdrop and before its content; it is neither a global background nor an escape. The four directional slots and `in_front` are all escapes; a later `below` on the same host can cover an earlier `in_front`.

This makes one choice explicit: **layer first, branch order on ties**. Keeping an earlier branch's entire submenu below every later sibling menu would give `A.menu, A.tip, B.menu`; that is a different contract. The [public regression](../test/emerge_skia/nearby_layers_test.exs) reveals each of those layers. The [internal contract](../guides/internals/nearby-semantics.md) supersedes the old v4, host-scoped-ID and fixed-slot description. By source inspection, pinned `main` (`aef5e42f`, `RenderSubtree::extend_local` / `extend_escape`) appends a child's escapes before the next child's escapes. Its separate ancestor, sibling and submenu tests are useful controls, not an oracle for this combined case.

## Representation and traversal

**Declared edges.** Each [Order](../native/emerge_skia/src/tree/declaration.rs) holds one sequence of keys: ordinary prefix, mounted suffix, with one slot byte per suffix key. `children(k)` remains the borrowed ordinary slice; `nearby(k)` zips the suffix with its tags; `owned(k)` sees both. Cache the reverse `(parent, role)` for edge readers. Preserve the same order in detached pools and share the immutable order on a live `:same` edit. A slot is not identity: the same key may change host or slot. Two distinct mounts may use the same slot.

**Layer rule.** Only a host with a declared escape opens a boundary, even when that escape has no ink or listener. A plain wrapper doesn't change stacking. For a test oracle, write `0` when passing such a host into its local content and `1` when following its escape. Pad on the right with zeros and compare shallowest choice first, then local paint ordinal. The example's layer addresses are `000…`, `010…`, `011…`, `100…`. Production need not store addresses or ordinals.

**Packing scratch.** [Bands](../native/emerge_skia/src/tree/paint/bands.rs) in `paint::Output` uses one flat reserved job array (root, escape coordinates, opacity ancestry, next link), FIFO head/tail indices per boundary depth and a bitset of occupied depths. The depth of a host here is the number of **enclosing escape-bearing hosts**, not its DOM depth. Keep the local visit stack for ordinary traversal. Do not use the [invalidation WorkSet](../native/emerge_skia/src/tree/queue.rs) as the paint queue: its same-depth order is intentionally LIFO.

```text
emit_local(host, d):
  append only its relevant escape roots to FIFO[d], in authored order
  emit host's local phases, visiting behinds and children at d + has_declared_escape

emit_local(root, 0)
while any FIFO is nonempty:
  d = deepest occupied depth
  run = detach the whole FIFO[d]           # one layer; do not preempt it
  for job in run, first to last:
    emit_local(job.root, d + 1) in job.escape_scope
```

On detaching depth `d`, all other pending depths are shallower. Every escape found in that run goes to a depth **greater** than `d`. Finishing the run first keeps peers together; choosing deepest next inserts their new layer before any waiting ancestor layer. A bucket is reusable workspace, **not** a persistent layer ID. Inserting a submenu doesn't renumber the tree. Each emitted escape is queued and drained once; there is no sort or tree scan per layer.

Draw and hit packing call the **same visit policy** with their own sinks and scope handles. An unchanged root port skips that side as today. A changed flat stream can still cost its output traversal; this design adds no separate discovery walk or work on a clean frame.

## Implementation contract

1. **Admit and round-trip edges.** [v10](../guides/internals/emrg-format.md) already carries ordered `(slot, id)` refs. Both the [detached batch](../native/emerge_skia/src/tree/wire/batch.rs) and [live stream](../native/emerge_skia/src/tree/wire/stream.rs) now prove those refs, including repeated slots, kept/removed edges, limits and reservation. `Tree::encode` preserves the tags and declaration order. Count all edges for ownership, depth, resources, byte limits and cycle/duplicate-ID proof; only ordinary children for arity, layout demand, shares, floats and prose. No new API, wire field or NIF.
2. **Anchor independently of flow.** The six slots use the [slot geometry examples](alignment.md#text-alignment-and-dependent-sites): front/behind use the host's local border box on both axes; above/below use its width, left/right its height. The other axis is natural; the mount's alignment positions it. Do not inset the slot twice or charge it to the host. Send width offers in Flow and origins/assigned heights in Place. Reject a width reader on an anchor whose width first finalizes in Place, and a height-dependent width on a host without a proved pixel height. Shared-flow document paragraphs can grow beyond that height. Inline continuations have no single anchor. No second paragraph solve or Place → Flow loop.
3. **Invalidate once against the final declaration.** Extend [changes](../native/emerge_skia/src/tree/changes.rs) and the existing seven [passes](../native/emerge_skia/src/tree/layers.rs), not their schedule. Compare the first replaced *tagged* order with the final order:
   - Reorder only mounted siblings: Compose on the host for the affected draw/hit ports; **no layout**. Reorder the ordinary hosts themselves: keep the existing ordinary layout invalidation too.
   - Change a slot or anchor: update the offer/origin readers that actually change. Ordinary ↔ mounted also changes the old/new host's ordinary layout inputs. A mount resize never grows its host.
   - Compare role, nonempty ordinal and revision per side in Compose. A locally empty branch can export visible escapes. Adding the first or removing the last escape can change **descendant stacking** even if the mount itself draws nothing. Propagate this boundary change to the affected root port; derive depths during packing. No descendant rank rewrite, wire-time enqueue or global dependency scan.
4. **Preserve scopes and installation.** A deferred scope carries pose, escape clips and group ancestry, not a saved canvas. Keep viewport and explicit `clip_nearby` (tag 77) clips in their establishing coordinate spaces; don't inherit ordinary host clipping or discard the host transform. Cull local and escaped contributions separately. Close opacity groups before unrelated content; reopen the shared chain for contiguous contributions in a new band, not per primitive. Test overlapping translucent mounts; zero alpha alone does not disable hits.

   Put a front-root **blocker** below that root's listeners and children in the hit stream. A reverse lookup meeting it stops; it mustn't fall through to the host. Use the same clip and inverse pose for blockers and targets. Only successful installation changes routed hits and capture. For scrolling, the overlay follows its host but not the host's own content translation; the [scrollport controls](scrolling.md#reference-controls) exercise opt-in clipping of drawing and input.
5. **Prove capacity before acceptance.** For `M` declared escapes the scheduling bound is `M` jobs, `min(M, MAX_DEPTH + 1)` FIFO headers and the active words. Drain used buckets; no jobs for a Nearby-free tree. Charge tagged suffixes, local visits, blockers, planes/clips and output buffers too. For split opacity a conservative extra bound is one group pair per *(escape edge, enclosing opacity owner)*: fold subtree escape counts bottom-up to reserve it without a dependency walk. Use checked sizes in both [patch demand](../native/emerge_skia/src/tree/wire/patch.rs) and [live workspace credits](../native/emerge_skia/src/tree/wire/publisher.rs), including saved orders, growth overlap and pinned readers. Sealed frames never point into builder scratch.

## Acceptance gates

- Compare the FIFO trace to the independent path/ordinal oracle. Include unequal ordinary wrapper depths, multiple mounts of one slot, a local behind with its own escape, reorder/undo, and empty → first escape → empty. Reveal each layer in turn: with all overlays present the scrim alone can hide a wrong order underneath.
- Compare sparse edits with a fresh final tree: frames, pixels, topmost local click **or blocker**, plus a point outside the overlay that still reaches the host. Cover clips on/off, rotation, host moves, kept IDs and remove/reinsert, two accepted edits before one settle, and last-successful-install routing. Check stage traces for the pure reorder and for a real slot-size change separately.
- Extend the [allocation interposer](../native/emerge_skia/src/tree/tests/allocations.rs): first settle **after reserve**, fitting wide/deep reorder, insert/remove, role change and nested escapes through prepared draw/hits must show **0 alloc / 0 realloc / 0 free**. Include positive controls and injected reserve failure with unchanged accepted tree and installed output. This is not an allocation claim for live draft creation, BEAM delivery or Skia rasterization.
- Run source-matched `cargo test --locked` in `native/emerge_skia/` and `mix test`, comparing named failures/exclusions with the [current baseline](api-parity.md). Use delivered headless pixels and raw press/release for the first public gate; verify compositor pixels and seat input separately for a window claim. Existing `nearby_reorder` / `nearby_slot_change` fixtures are workloads, not stacking or allocation oracles.

## Verification and limits

- [Native tests](../native/emerge_skia/src/tree/tests/nearby.rs) check tagged upload/patch and byte-at-a-time live ingress, late-site refusal, target replacement, ordinary ↔ mounted moves and reorder/undo. They compare sparse and fresh frames, RGBA and local hits. An independent address sort checks the native hit sequence on 98 generated trees, including escapes inside local behinds. A guarded draw-only or hit-only reorder reuses the opposite sealed stream. Interleaving behinds and escapes without changing order within either phase changes the encoded tree but schedules no pass. Resizing a mounted paragraph solves once per changed width, not on a height-only edit.
- [Paint controls](../native/emerge_skia/src/tree/tests/nearby/scopes.rs) cover behinds under real glyphs and PNG samples, and a locally clipped-out anchor whose image and outer shadow escape unless an explicit square/rounded clip intervenes. Separate controls check a rotated ancestor, listenerless blockers, zero alpha and shared opacity. Failed submission leaves the prior pixels and local hit intact.
- The Linux/glibc [allocator interposer](../native/emerge_skia/src/tree/tests/allocations.rs) checks first settle after reservation and fitting edits through prepared output and installed inverse hits: 512 sibling mounts, reversal, a 512-edge chain, role changes and remove/reinsert. The three counters are `[0, 0, 0]` (allocate, reallocate, free). A forced growth failure preserves declaration and installation; Rust and Skia positive controls exercise the counter. This does not measure live draft construction, BEAM delivery, rasterization or elapsed time.
- [Public tests](../test/emerge_skia/nearby_layers_test.exs) distinguish fresh one-shot output, delivered headless RGBA, installed capture and raw press/release routing. They check all six slots after host translations, reveal both sibling orders under a tooltip and scrim, and move a pressed keyed panel to another scaled anchor and then into ordinary content. Removing and reinserting the panel cancels the held click.

**Linux checks.** The previous NIF rejected the public overlap tree before delivering any pixels. On the source-matched release build, all **302** default-feature Cargo tests and **93** focused Mix tests passed. The ordinary seed-17 run passed **898/945** (15 doctests and 883 tests), with **47 failures and 21 exclusions**. Compared by name to the [inline checkpoint](inline-layout-rotation.md#linux-verification), eight failures cleared and none were added: six patch tests, the EMRG Nearby round trip and the centered SVG with `in_front` tint. No exclusions changed. Strict Clippy, formatting, Credo, warning-clean compile, Dialyzer and ExDoc links (67 pages) passed. Rust 1.91 also passed 302 native tests. The `stage-probes` build passed 302 native and 17 headless isolation tests; Wayland/Vulkan passed 326 native. The three public Nearby tests passed separately on each build.

**Window evidence.** On isolated Linux Wayland raster, OpenGL and Vulkan windows, removing layers revealed the white top, black scrim, green tooltip and blue menu; reversing siblings revealed the red menu, then its tooltip. At scale 1 the compositor's RGB crop matched the entire installed RGBA frame after dropping alpha. At scale 1.5 the overlap and outside samples matched separately in capture and crop. Each presenter routed 24 seat clicks to the expected IDs and local coordinates. This qualifies this overlap on those windows, not timing, physical scanout or other platforms. Local source manifests, per-gate logs and replay notes: `/workspace/.emerge-nearby-bands-SPBuHU/` (`qualified`, `checked-probes`, `checked-vk`, `seat-*-checked`).

**Remaining:** no anchor for inline continuations or backward size dependencies. See the subsequent [scrolling checkpoint](scrolling.md) for scroller-owned versus descendant overlays. Focus, editors and other unported event types remain separate. No new NIF, public helper, wire tag, presenter, input path or animation clock.
