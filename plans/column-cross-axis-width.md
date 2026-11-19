# Fill widths on direct children of a Column

**Implemented and checked.** At `2c802f6` a Column already offered its inner
width to a child with `width(fill())`, yet the same child with `width(fill(1))`
was rejected. This is one new site for existing lengths, not another
allocation algorithm: children stacked vertically do not compete for width.

## Contract and first regression

Use a 120×40 Column, 6px padding on all sides, 4px vertical gap, two 12px-high
children. Inner width is 108. A starts at `(6,6)`, B at `(6,22)`; there is no
unused height. B always uses `width(fill())` and is 108px wide. For A, each
fill leaf reads the same 108px offer regardless of weight; each content
leaf reads A's padded natural width. Evaluate `min`/`max` literally.

| A's width | A's demand | A's expected width | Unused width on A's line |
| --- | ---: | ---: | ---: |
| `fill(3)` | 30 | 108 | 0 |
| `min(content(), fill(3))` | 30 → 90 → 150 | 30 → 90 → 108 | 78 → 18 → 0 |
| `max(content(), fill(3))` | 150 | **150** | 42 overflow |

B has no unused width in any case. A floor may overflow; it must not enlarge
B or the host clip. Do **not** widen admission for root Column widths, El or
WrappedRow parents, or Row child heights. Keep existing plain-width behavior,
Row width sharing and Column height sharing unchanged.

**Before the change (detached geometry).** On `2c802f6` both plain-fill
children were 108 wide. A weighted or compound fill failed upload
with `weighted fill width requires a root Row or a child of a Row` or the
corresponding `bounded fill width` error. On pinned `5c71a593`, A gets 108
for `fill(3)` and 30/90/108 for the content cap. Pinned main gives **108**
for the content floor, but **150** for both `content()` and
`max(content(), px(108))` in the same scene. Retained also gave 150 for
those two controls. The [floor regression](retained-changelog-candidates.md#content-floor-on-the-width-of-a-column-child)
requires 150: substituting `px(108)` for a known 108px offer must not
change the result.
Probe and logs: `/workspace/emerge-investigation-artifacts/column-cross-width-plan-wuhUgE/{probe.exs,pinned-controls.log,retained-controls.log}`.
Each run used its own producer and verified mapped NIF inode and bytes.

## Regression and implementation gates

1. **Start red on an ordinary headless viewport.** Give A a red background,
   B blue, the Column white; make all three clickable and A's demand child
   unpainted and non-listening. Check whole frames on cold mount and sparse
   edit. When A's demand changes 30 → 90 under the cap, `(76,10)` must go
   from the Column at local `(76,10)` to A at `(70,4)` **only on submission**.
   `(76,26)` must stay on B at `(70,4)`. Preserve the plain-fill control;
   give the oversize floor its own test, with the host inside a larger view
   so a host clip can be distinguished from the viewport edge.
2. **Prove the site, then resolve the offer once.** In [final-edge admission](../native/emerge_skia/src/tree/wire/batch.rs)
   distinguish a direct Column parent from the root's own kind. Do not mark
   the Column as a Row-style width distributor. In [width flow](../native/emerge_skia/src/tree/layers.rs)
   keep the Column's incoming child offer in device pixels; resolve the
   child's policy against that offer and its measured demand. Row children
   still consume the Row's already-resolved share. A content-sized Column
   still measures its natural width first; an auto-width Column retains its
   existing fill-child rule. No new pass, child order, NIF or wire field.
3. **Test which edits reach which readers.** Change only A's demand while
   the Column stays 120px wide. Then keep demand at 30px and switch A from
   `min(content(), fill(3))` to `max(content(), fill(3))`: its intrinsic
   width is still 30, but its offered width must change from 30 to 108.
   Check flow on that edge; don't remeasure unrelated text. Exercise
   padding, zero offers, scale, width-driven wrap
   and the following child's y. Keyed Row ↔ Column moves must switch offer
   semantics without remounting; an illegal move or masked pixel overflow
   must preserve pending state and installed pixels, hits and attachments.
4. **Extend the [site matrix and seeded tests](dimension-regressions.md).**
   Predict each Column child's width independently using the same inner width
   for every fill leaf, not a sibling sum. Compare cold and sparse geometry;
   retain wrong-site tests outside this one new edge. Run the reserved/fitting
   zero-heap controls, release Cargo/strict Clippy profiles, focused and full
   Mix against the **93 named failures / 21 exclusions** baseline. Confirm
   representative pixels and clicks on a private CPU Wayland window before
   claiming window behavior. Only then update port status or promote a fix.

## Results

- **Before implementation:** the plain-fill viewport control passed. The
  weighted mount, cold cap, sparse cap and oversize floor tests failed with
  the admission errors above. Four extended native campaign tests were red;
  seed 0 failed at `ColumnWidth` step 0. These were refusals, not incorrect
  geometry from a successful layout.
- **After:** all 11 detached controls from the planning probe met the stated
  geometry; only the 150px content floor differs from pinned main. The
  [viewport tests](../test/emerge_skia/column_cross_width_test.exs) check
  whole delivered frames and raw input: `(76,10)` stays on the Column until
  the 90px cap is submitted, then reaches A at `(70,4)`. B does not move.
  A press also follows the same keyed card from a Row to a Column and back,
  including a rejected sparse move while the legal move is pending.
- Width flow distinguishes a Row's resolved offer from available space on
  the final incoming edge. A policy-only cap → floor at equal 30px demand
  reruns child flow, not the Column's flow or intrinsic measurement. An
  auto-width Column observes whether a child requests fill, not changes to
  its weight. [Native tests](../native/emerge_skia/src/tree/tests/column_width.rs)
  cover natural/auto hosts, zero width, width-driven wrap and following y,
  masked overflow on a scaled move, refused handoff and attachment identity.
  No new pass, wire field, renderer API or heap-backed expression.
- Extended the 4,320 width × height site cases without admitting these
  widths at root Columns or El/WrappedRow parent sites. **10,000 seeds ×
  3 sites × 13 = 390,000 settled states**, including 130,000 independent
  Column-width states, matched literal test math and exact fresh replay.
  Not a frame count. Two old tests (`tests/{bounded,weighted}.rs`) assumed
  a move under a Column was illegal. Their width fixtures now use a WrappedRow
  as the illegal destination and still verify the original rollback and hits.
- Six release Cargo/strict Clippy profiles passed: **89/92/93/117/117/93**
  (headless, Wayland, GL, Vulkan, both, matched). First reserved native
  compute, fitting cross-axis policy/demand edits through prepared paint/hit
  inputs, and pending compute after a refused growth: **0 alloc / 0 realloc /
  0 free**. This excludes reservation, rasterization and delivery.
- **90 focused Mix tests passed** (7 excluded by selection). Full Mix:
  **677/770 passed**, **93 failures / 21 exclusions**. The 93 failing names
  are identical to `2c802f6`. No new exclusions. The private CPU Wayland
  window passed **12 seat clicks**, each bracketed by full compositor crops.
  This includes the 150px floor in a 160px-wide view: A paints/hits in the
  right padding at x=117, but neither paints nor receives input at x=121.
  No new GPU, timing or physical scanout claim.
- Evidence: `/workspace/emerge-investigation-artifacts/column-cross-width-ke9QpY`.
  See `logs/red-{live,properties}`, `logs/first-matched`, `logs/final-geometry`,
  `logs/{qualification-native,qualification-app,final-window,post-suite-mapped}`,
  failure-name lists and `changed.sha256`. The mapped NIF inode and bytes
  were checked before/after the full suite and in the Wayland client. Matched
  NIF SHA-256: `a3105d9006df8e92e615a3c5138b9431d4c7cbe00923220fd716085336932453`.
