# Unweighted height fill in columns

**Implemented.** A fixed header takes the space it needs and the fill panels
below share what is left. The unchanged [column fill test](../test/tree_test.exs)
now gives both children of a 100px column 50px. Before this slice (`31cc269`)
it failed at upload: `fill height is not in the first slice`. The DSL, wire
format, NIF API, presenters and demo are unchanged.

## Contract for this cut

- Admit unweighted height fill on a **root Column** (offer = viewport height)
  and on **ordinary children of a Column**, including nested columns. The
  parent column itself need not be the root. Continue to reject height fill
  in other positions, weighted `fill(n)` (even `fill(1)`) and `min`/`max`.
  Cross-axis stretch, alignment, scrolling and `space_evenly` are separate.
- A column with a definite outer height (fixed or allocated) subtracts its
  vertical padding, the **reflowed outer heights** of its non-fill children,
  and one gap per adjacent pair. Divide `max(0, remaining)` equally among
  fill children, in device pixels. A share is an **outer** height: don't add
  the child's padding or multiply it by scale again. No division with no fills.
- A column with omitted or `content()` height does not borrow spare viewport
  space. Offer each fill child its own measured intrinsic height; don't divide
  the siblings' total equally. A nested fill column can then subdivide its
  assigned extent. Zero is a real offer, not a missing one.

## Reference cases

These are **detached geometry observations**, not window or input evidence.
Pinned main `5c71a593` and retained `31cc269` used their own producers and
verified mapped NIFs. Retained rejected every fill declaration below.

| Declaration / edit | Expected device geometry at 1× |
| --- | --- |
| H=120, padding 5, gap 5; header `20 → 60 → 20`, then fill B and C | B `(y,h)` = `(30,40) → (70,20) → (30,40)`; C = `(75,40) → (95,20) → (75,40)`. |
| H=100, column gap 4; non-fill `wrapped_row` of two 48×20 chips with XY gaps 4, then one fill. Inner width `100 → 80` | Wrapped height `20 → 44`; fill `(24,76) → (48,52)`. Use content-height chip wrappers around 20px boxes so the two producers agree on outer vs content height. |
| Viewport H=100, gap 4; two fill children with natural heights 15 and 0 | Auto/content column: outer 19; children 15 and 0. Root fill column: outer 100; children 48 and 48. |

Keep one distinguishing edge case: if the **wrapped child itself requests
fill** under a content-height column, main offers its pre-wrap intrinsic 20,
not 44. That's distinct from charging a **non-fill** wrapping sibling 44 in
a definite column. Don't change `WrappedRow` line advancement to get either
case to pass. See main's `build_column_layout_plan` and `resolve_planned_length`.

## Implementation order

1. **Admission.** Decode the existing height tag 2, variant 0, then validate
   the allowed site in the v10 batch proof using final parentage and effective
   attrs (`:same` included). Check moved bare references, promotions and
   subtree-target replacements, not just fresh definitions. Interior edges of
   an unchanged kept branch need no rewalk. A rejected edit leaves the last
   accepted declaration, pending work, installed hits and routes intact.
2. **Resolve demand up, allot space down.** In `tree/layers.rs` keep width
   flow first. Bottom-up `Reflow` supplies width-dependent non-fill heights;
   the pre-width `Measure` cannot supply those. Use shallow-first `Place` to
   resolve the final outer height and have `shapes/column.rs` distribute child
   offers and offsets together, before placing those children. Keep one writer
   for column Y offsets. Don't write a share back into intrinsic measurement,
   recursively lay out a child, or iterate layout until stable.
3. **Invalidate actual readers.** A height-policy edit can change the split
   even with equal measured heights. Revisit distribution on gap/padding,
   scale, non-fill reflow, fill membership, order/new edge, keyed reparenting
   and root viewport-height changes. Compare each affected offer and offset
   even if the parent's box is unchanged. Use the existing ordered children,
   reserved keyed storage and directional worksets. Apply edits first; seed
   work once against the final tree, never during mutation. Reserve any new
   per-key height storage before commit, including detached residents and
   old/candidate overlap; an injected reserve failure must be atomic.

## Exit checks

- Keep the original two-child test. On one ordinary headless viewport, give B
  and C different colors/listeners. Check full frames against cold mounts.
  A raw click at `(10,80)` in the padded example must reach **C `(5,5)` →
  B `(5,10)` → C `(5,5)`** after the respective submissions. Check a real gap,
  a keyed swap/move, and that a pending edit or refused handoff keeps the old hit.
- Exercise the wrap case in both directions, nested fill, height-only viewport
  resize, exhausted space and a fractional split with a scaled child. Check
  inclusive hit edges separately from pixel rounding. Preserve Column clipping.
- Check traces: a height-only split doesn't reshape unchanged text or recompute
  widths of unaffected siblings; a pre-refresh round trip to the same declaration
  does no layout. First compute after reservation and fitting edits through
  prepared paint/hit inputs must retain the measured `[0,0,0]` heap target.
- Run release Cargo tests/strict Clippy and focused plus full Mix tests from a
  private build under the shared lock; verify the mapped NIF. Compare failure
  **names** to the 94 failures / 21 exclusions baseline; require the named fill
  test green, no new failures or exclusions. Mixed-feature tests may reach a
  different unported feature. For a Wayland claim, also replay the resize and
  click on private shm with compositor pixels and `wl_seat` events. No hardware
  or timing claim. The stale `retained_topology_reads` CI invocation is separate.
- Only then update the [port tracker](retained-ui-port-tracker.md) for the
  positions actually supported; leave weighted lengths and other hosts open.

## Results

- The v10 proof checks the final incoming edge and accepted attributes of each
  visited occurrence, including closed moves and promotions. Placement stores
  the optional outer offer with the offset in the **existing reserved map**;
  no per-column vector, second child order or extra layout pass. A height-only
  viewport resize visits no measure, width flow or reflow in the trace. The
  pre-refresh header/order round trip visits no pass.
- All 11 detached reference cases above and in the planning probe matched
  pinned main's keyed Y positions and heights. The [viewport tests](../test/emerge_skia/column_height_fill_test.exs)
  compare whole delivered frames to independent cold mounts, then send raw
  buttons through `handle_input/2` and check `handle_info/2`. At `(10,80)` the
  route was C `(5,5)` → B `(5,10)` → C `(5,5)`; the gap reached the column
  and a press followed C through a keyed swap. The native [regressions](../native/emerge_skia/src/tree/tests/column.rs)
  cover nested distribution, zero shares, scaled fractional clipping,
  last-accepted policy on move/promotion and a refused output handoff.
- Linux/glibc allocation interposer: **0 allocations, 0 reallocations, 0
  frees** on the first settle after reservation, and from fitting wire edits
  through prepared paint/hit inputs. Fill insert/detach/reattach at 180/180
  resident slots also fit. Injected growth failure kept the pending header
  and installed target. Adding slot 181 needed growth at admission; its first
  compute was zero again. Rust and Skia positive controls: `[1,1,1]` and
  `[1,0,1]`. Not a zero-allocation claim for reservation, rasterizing changed
  pixels, frame delivery or the BEAM.
- Separate private CPU Wayland check: full compositor RGB crops before and
  after six `wl_seat` clicks, including height-only resize `120 → 200 → 120`.
  With the 20px header, `(10,80)` went C `(5,5)` → B `(5,50)` → C `(5,5)`
  on resize. Used the compositor-reported origin after Sway moved the window;
  the initial fixed-origin capture timed out and is not counted. Normal close.
  No GPU device, physical scanout or timing claim.
- Release Cargo and strict Clippy passed all six profiles (headless **56**,
  shm **59**, GL **60**, Vulkan **84**, both **84**, matched **60** Rust tests).
  Full Mix **651/744 passed**, **93 known failures**, **21 excluded**. The
  original column-fill case was the only failure name removed; none added.
  A broader focused run still fails the pre-existing frame-rate animation
  case on attribute 66. No change to that test or the stale CI target.

Evidence: `/workspace/emerge-investigation-artifacts/column-height-fill-vZfPXo`
(`logs/final-{mapped,oracle,window,test-*,lint-*,focused,mix}`, `oracle.delta`,
`new.failures`, `repaired`). Verified the mapped NIF's inode and bytes against
this build, not a library already present in the application.
