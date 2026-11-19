# Next slice: fill widths on direct children of a Column

**Proposed after `2c802f6`; not implemented.** Start here because a Column
already offers its inner width to a child with `width(fill())`, yet the same
child with `width(fill(1))` is rejected. This is one new site for existing
lengths, not another distribution algorithm: children stacked vertically do
not compete for horizontal space.

## Contract and first regression

Use a 120×40 Column, 6px padding on all sides, 4px vertical gap, two 12px-high
children. Inner width is 108. A starts at `(6,6)`, B at `(6,22)`; there is no
unused height. B always uses `width(fill())` and is 108px wide. For A, each
fill leaf should read the same 108px offer regardless of weight; each content
leaf should read A's padded natural width. Evaluate `min`/`max` literally.

| A's width | A's demand | A's expected width | Unused width on A's line |
| --- | ---: | ---: | ---: |
| `fill(3)` | 30 | 108 | 0 |
| `min(content(), fill(3))` | 30 → 90 → 150 | 30 → 90 → 108 | 78 → 18 → 0 |
| `max(content(), fill(3))` | 150 | **150 proposed** | 42 overflow |

B has no unused width in any case. A floor may overflow; it must not enlarge
B or the host clip. Do **not** widen admission for root Column widths, El or
WrappedRow parents, or Row child heights. Keep existing plain-width behavior,
Row width sharing and Column height sharing unchanged.

**Measured before evidence (detached geometry only).** On `2c802f6` both
plain-fill children are 108 wide. A weighted or compound fill fails upload
with `weighted fill width requires a root Row or a child of a Row` or the
corresponding `bounded fill width` error. On pinned `5c71a593`, A gets 108
for `fill(3)` and 30/90/108 for the content cap. One decision needs a named
regression before porting: pinned main gives **108** for the content floor,
but **150** for both `content()` and `max(content(), px(108))` in the very
same scene. Retained also gives 150 for those two controls. Recommend 150:
replacing the known 108px offer with `px(108)` should not change the result.
[Open finding, not a verified fix](retained-changelog-candidates.md#content-floor-on-the-width-of-a-column-child).
Probe and logs: `/workspace/emerge-investigation-artifacts/column-cross-width-plan-wuhUgE/{probe.exs,pinned-controls.log,retained-controls.log}`.
Each run used its own producer and verified mapped NIF inode and bytes.

## Implementation order and exit criteria

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
