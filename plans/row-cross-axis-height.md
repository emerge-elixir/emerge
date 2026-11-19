# Next slice: fill heights on direct children of a Row

**Proposed after `2bf82a9`; not implemented.** The Column-width slice is
complete. The next missing case is a single-line Row with two children that
both need its full inner height. Unlike width, the content height may not be
known until after wrapping. Do not divide it between horizontal siblings.

## Contract and first red test

Use a **40×120 Row**, padding 6 on all sides, horizontal gap 4. Both children
are 12px wide; A starts at `(6,6)`, B at `(22,6)`. No horizontal space is
unused. Every height-fill leaf gets the same 108px inner height, regardless
of weight. A content leaf reads the child's padded, post-wrap demand; px
leaves scale at their own site. B uses `height(fill())`: 108px, no slack.

| A's height policy | A's demand | A's expected height | Unused height on A’s side |
| --- | ---: | ---: | ---: |
| `fill()`, `fill(1)` or `fill(3)` | 30 | 108 | 0 |
| `min(content(), fill(3))` | 30 → 90 → 150 | 30 → 90 → 108 | 78 → 18 → 0 |
| `max(content(), fill(3))` | 150 | **150** | 42 overflow |

**Start red on ordinary headless output.** Retained rejects even plain fill
at this edge: `height(fill()) requires a root Column or a child of a Column`.
A detached sparse edit from A/B `px(30)/px(108)` to a mixed A was also
refused; geometry stayed 30/108. Use that valid starting scene for the patch
test and a separate cold two-fill mount. Give the Row, A and B distinct
colors/listeners; leave the demand child unpainted and non-listening. At the
30 → 90 edit, `(10,76)` must change from Row local `(10,76)` to A `(4,70)`
**only after submission**; `(26,76)` stays on B `(4,70)`.

## Two traps in the reference

- **Content floor.** Pinned `5c71a593` matches the plain/weighted and cap
  cases, but gives A **108** for the floor, even with B fixed at 108. Both
  builds give **150** for `content()` and `max(content(), px(108))`. Require
  the same 150 for the fill-bearing floor. Embed the Row in a taller view:
  `(10,117)` should hit A in the bottom padding; `(10,121)` must hit only
  the enclosing view. B and the host clip do not grow.
- **Wrap before bounding height.** Resize a **100×80 Row to 99×80**, padding
  2, gap 4. A is a width-fill WrappedRow (padding 2, gap 4, two 36×12
  content-height chips); B is 12×76. A's width goes 80 → 79; nothing is
  unused horizontally. A `min(content(), fill(3))` should be **16 → 32**
  high, leaving **60 → 44** of its 76px offer unused. B remains full-height.
  Pinned main moves chip two from y=4 to y=20 but leaves A at **16**. With
  plain content A reaches 32; retained's `min(content(), px(76))` also
  reaches 32. Check A's own pixels and listener, not just the chip position.

These are **detached before measurements**, not verified fixes. [Open
findings](retained-changelog-candidates.md#open--row-child-heights).
Probe, verified producer/NIF identities and geometry: `/workspace/emerge-investigation-artifacts/row-cross-height-plan-u2l1S6/{probe.exs,retained-final.log,pinned-final.log}`.
The first probe mixed keyed and unkeyed siblings and never completed; only
the corrected runs count.

## Implementation order and gates

1. Extend [final-edge admission](../native/emerge_skia/src/tree/wire/batch.rs)
   to **direct Row children** for plain, weighted and compound fill heights.
   Do not admit height fill on a root Row, or under El or WrappedRow. Keep
   root Column heights, Column height sharing and all width rules unchanged.
2. Compute demand **up**, place **down**. With auto or `content()` height,
   a Row sizes to the tallest unallotted, reflowed child plus padding, not
   the viewport or a child's *installed* height. In the padded scene with a
   30px demand fill child and a fixed 60px peer, both host forms yield a
   40×72 Row and two 12×60 children, no unused inner height. A Row allocated
   by a Column instead uses its actual placed height, including zero. Don't
   change the pre-wrap fallback of a fill child on a natural Column's main axis.
3. In [Row layout](../native/emerge_skia/src/tree/shapes/row.rs), keep width
   sharing in Flow. In [Reflow and Place](../native/emerge_skia/src/tree/layers.rs),
   `ReflowCx::fixed_basis` currently gives plain fill its pre-wrap measurement;
   a Row must also see its child's height after wrapping. Keep that demand
   separate from the final offer. Move Row x-offset writing to `place_children`
   so one writer sets the offset **and** optional
   outer height in the reserved `Placement` entry. Evaluate each fill child's
   height once with the settled Row inner height; don't mark Row as a Column
   height splitter or feed the allotted value back up. No second child order,
   recursive retry, new pass, wire tag or public NIF.
4. Invalidate the actual readers. At demand 30 change only A's cap to a
   floor: its measured size is unchanged, but the offered height must go
   30 → 108 without reshaping B. Width/wrap, padding, scale, reordering
   and a new incoming edge can also matter at equal Row bounds. Check
   zero → restore, nested Rows/Columns,
   keyed moves between independent and shared height offers, and press across
   submit. Wrong-site moves, masked overflow, failed reserve or refused
   handoff must preserve prior pending work and installed pixels/hits.
5. Add a fourth site to the [seeded oracle and site matrix](dimension-regressions.md):
   predict each Row child's height independently and compare sparse results
   exactly to fresh. Keep negative cases at still-illegal sites. Repeat
   first/fitting zero-heap controls, six release Cargo/strict Clippy profiles,
   focused/full Mix against **93 named failures / 21 exclusions**, and real
   CPU Wayland crops/seat clicks before updating coverage or promoting a fix.
