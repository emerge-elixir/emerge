# Scale parity: subtree layout and Wayland display scale

**Implemented in the retained tree and all three Wayland presenters.** See
[results and remaining differences](scale-results.md). The design below keeps
layout scale and display scale independent: `scale(1.25)` works on a scale-1
output without a compositor protocol, and HiDPI needs no scale attribute.

## What main does

Use local `main` (`5c71a593acfcbb2675c3bd282c680633439e7745`) for layout and
Wayland behavior. The references are `tree/layout.rs` (`scale_attrs`),
`tree/layout/tests/layout_transform.rs`, and `backend/wayland/{geometry,protocols}.rs`
under `native/emerge_skia/src` at that revision. For callback payloads use
`5b389e7f` **with the pre-cut overlay** (`PointerCoordinates::position`): main
predates element-local position payloads. Do not port either old renderer.

| Input | Meaning |
| --- | --- |
| `scale(s)` (`:layout_scale`, tag **78**, big-endian f64) | Positive, unitless multiplier on this element **and its descendants**. For ordinary values, node scale = parent's effective scale × `s`; the root starts at output scale, absence means `1`. Apply before measurement, not around the painted center. |
| Output scale | Device pixels per surface logical pixel. It sizes the backing buffer and supplies the tree's initial scale. A local scale edit does **not** resize the window or multiply the viewport constraint. |
| `Transform.scale(s)` (`:scale`, tag **34**) | Paint-only, about the center; siblings keep their slots. It is **not** the implementation of tag 78. Keep the existing same-element conflict check. Capture's `scale:` option is a separate API again. |

For example, main's root `scale(2)` and child `scale(1.5)` turn a `20×10px`
child into a `60×30` box at display scale 1. A following sibling moves. `fill()`
still consumes its parent's remaining device-space offer; don't scale the offer
or measured child extents a second time. Scale padding and gaps at their owner.
One non-obvious rule: main scales an explicit `Font.size` **where declared**.
Children inherit the resolved size; a deeper layout scale does not multiply it
again. Without any declaration, main uses `16.0` device pixels. The retained
renderer now intentionally differs on that last point: its default is 16
**logical** pixels at the text site, just like `Font.size(16)` there.

## 1. Port the layout attribute once, in the shared tree

1. **Pin red and reference cases first.** Feed the same DSL scenes through each
   revision's own producer. The retained Elixir helper and codec already emit
   tag 78; before this port `tree/wire.rs::attrs` fell through to `unsupported
   attribute 78 in first slice`. Keep EMRG v10, the existing public API and
   authored attr bytes. Validate a finite positive factor in the shared
   upload/patch proof. Pin fractional rounding at wrap boundaries and main's
   root-only vs nested `f32::EPSILON` behavior. Reject unrepresentable products
   atomically, not by passing infinity to Skia or silently substituting `1`.
2. **Carry the dependency, not a scaled tree.** Store the authored factor in
   `Attrs`; resolve it from the parent's settled value in `layers.rs::context`.
   Add `:layout_scale` to `Reconcile`'s inheritable site inputs. A parent edit
   changes descendant ancestry, not occurrence IDs or unchanged branch values.
   Send one sparse attrs replacement with closed children; `attrs: :same`
   preserves a pending scale and clearing the attr restores the identity factor.
3. **Admit resources before committing.** The old `text::Sizes::devices` knew
   only declared sizes × one display factor. One declared size at several
   scaled sites can now need several real strikes. Keep the 64 **declared-size**
   limit distinct from effective strike demand. In `wire/patch.rs::admit`, use
   final candidate parent links/attrs and reusable scratch to determine changed
   strike requirements, including closed branches moved under a different scale.
   Stage the delta to counted `(family, device_size)` requirements in `Batch`
   scratch against the preceding **accepted** state, not the last draw. Publish
   counts only at commit. Stop at unchanged context with no independent edit;
   a color edit elsewhere must not enumerate every text or every scale/size pair.
   Use the same checked preparation on a display-scale change in `Tree::refresh`.
   Charge strike tables, old/new glyph overlap, detached residents, indexes and
   scratch; reserve or reject before changing declarations/installed output.
   No unbounded cache of historical scales, or strike creation during measure.
4. **Invalidate once against the final tree.** In `changes.rs`, a changed scale
   seeds `Context` at its owner on refresh, not at patch time. Use the settled
   per-node scale in measure, row offers/gaps, reflow/box height, text origin
   and hit mapping; these last paths currently read `Layers::scale` globally.
   Keep layout in device units. Compare context down, intrinsic/fit up, offers
   and placement down; equal results stop only that dependency. A scaled chip
   can wrap its row and push the *next* sibling without reshaping that sibling.
   A keyed move/promotion rechecks the new site without remounting the listener.
5. **Publish drawing and hits together at the existing handoff.** CPU, GL and
   Vulkan consume the same resolved rectangles and glyph positions. No canvas
   zoom for layout, CPU bitmap upscaling, or host glyph coverage on GPU. For an
   untransformed listener, local = `(buffer_point − unclipped_outer_origin) /
   effective_node_scale`. The hit record needs that divisor even when its fill
   and clip are unchanged. Until successful delivery/submit (or proven reuse on
   the same target), input uses the old divisor, attachment and callback route.

## 2. Match Wayland output scaling across shm, OpenGL and Vulkan

- Factor `Geometry` out of `session/wayland/shm.rs` for all three presenters.
  Keep logical size, nominal factor, actual buffer extent and surface mapping
  together. Bind `wp_fractional_scale_v1` only with `wp_viewporter` support.
  On a preference use `max(1, preferred / 120)` like main; use buffer scale 1
  and a viewport destination in logical units. Otherwise retain integer buffer
  scale and unset the destination. The display floor of 1 is **not** a floor
  on `scale/1`. No new viewport config, renderer default or IEx flag.
- Round each logical dimension × nominal factor to its nearest device pixel,
  checking existing size/stride limits. Pass the nominal factor to layout;
  map seat positions using the **installed actual per-axis ratios**, not the
  nominal factor or the node scale. Example: `101×100` at `1.5` → `152×150`;
  surface `(50.5,25)` → buffer `(76,37.5)`, not `(75.75,37.5)`. Preserve the
  backend raw-input and resize payload units; only element callbacks undo the
  listener's layout scale.
- Apply surface state with the matching attach/swap/present, not on receipt of
  a preference alone. Track requested, sealed and installed geometry separately.
  Even if extents round to the same size, a changed mapping needs a handoff.
  A busy shm slot, blocked acquire, failed swap, or out-of-date swapchain must
  not publish new hits/routes. No extra render loop, readback or CPU fallback
  after GPU selection.

## Gates, in order

- **Main parity:** fixed root local `2` vs global `2` at the same physical
  constraint; `2 × 1.5` nested; shrink below 1; explicit, inherited and
  default text; row `[20px, fill]` in 120px goes `[20,100] → [40,80]` when
  only the fixed child doubles. Compare cold vs patched layout and whole CPU
  frames for two-axis gaps, fractional clips, wrap and a following sibling.
- **Ordinary viewport:** real delivered frames, raw move/press/release and
  `handle_info/2`. Check old/new hit and miss sites, child precedence, and a
  listener whose fill box stays fixed: a 10-device-pixel offset reports `10`
  at scale 1 and `5` at scale 2. Cover capped pending edits, callback-only
  updates, failed submit, and a press spanning a still-attached keyed move.
  Keep detached geometry/codec and native failure controls separate.
- **Retained cost and rollback:** trace `1 → 2 → 1` before refresh (no net
  compute), a closed subtree moved between unlike scales, and a 10,000-child
  root-only scale edit (no descendant IDs in the wire, but affected consumers
  legitimately do work). Try malformed tag-78 values (zero, negative, NaN,
  infinity), cumulative overflow, and an injected reserve failure after a
  valid edit; pending state and installed output must survive. Exercise more
  than 64 effective sizes from one declared size without conflating the limits.
  Measure fitting warmed
  recompute through draw inputs `[0,0,0]` separately from cold strike creation
  and one-over growth; changing scales repeatedly must not leak strikes.
- **Window:** run the sibling `../wayland_cards` with `iex -S mix`. In a
  private copy put `scale(1.25)` and an explicit font size on the root, then
  on just one lane; use ordinary app rerenders and real seat clicks. Check
  compositor pixels, local payloads, wrapping and close. Exercise `1 → 1.5 → 2
  → 1` including odd logical dimensions, and integer-only protocol fallback.
  Check shm and, with hardware consent, each selected GPU API separately;
  software fallback is not hardware evidence. No screenshot NIF or synthetic
  window input as proof. The opt-in Vulkan synchronization gate stays separate.
- Use private builds under `scripts/performance-lock.sh shared`; verify the
  mapped NIF. Run release Cargo tests/strict Clippy for headless, shm, GL,
  Vulkan without GL, both and matched default, plus focused and full Mix tests
  and the sibling's test. Compare the **94 named failures / 21 exclusions**,
  not just totals. Do not relax the mixed scrolling/rotation mouse test or
  infer hardware sync, physical scanout or speed from this slice.

**Separate follow-up if paint zoom is wanted:** port tag 34 in paint context,
compose center-based subtree transforms and inverse-map hit/clip coordinates.
Prove a visually doubled card does *not* move its sibling; cover nested,
negative and singular factors against main. That is not a prerequisite for
layout/display scale. Animation, rotation, Nearby, scrolling, richer paint and
non-Wayland window ports retain their existing boundaries.
