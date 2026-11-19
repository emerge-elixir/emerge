# Fill heights on direct children of a Row

**Implemented and qualified** after `2bf82a9`. A single-line Row offers
its full inner height to each direct child independently. Unlike width,
content height may not be known until after wrapping. [Results](#results).

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

**Start red on ordinary headless output.** Retained `2bf82a9` rejected even plain fill
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

These are **detached before measurements**. The [submitted evidence](#results)
verifies the two [changelog candidates](retained-changelog-candidates.md#verified-fixes--ready-for-the-changelog) separately.
Probe, verified producer/NIF identities and geometry: `/workspace/emerge-investigation-artifacts/row-cross-height-plan-u2l1S6/{probe.exs,retained-final.log,pinned-final.log}`.
The first probe mixed keyed and unkeyed siblings and never completed; only
the corrected runs count.

## Implementation order and gates (original plan)

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

## Results

- **Before:** the fixed-height headless control passed. Cold admission, the
  sparse fixed-to-fill patch, the oversize floor and the wrap case all failed:
  **1/5 passed**. Four native campaign tests failed admission; seed 0,
  `RowHeight` step 0 reduced to one `fill(0.25)` child. No bad geometry was
  returned for the rejected trees.
- **After:** Row width sharing stays in Flow. Reflow reports unallotted
  demand up; Place writes x/y and the optional resolved height together on
  the incoming edge. Each fill leaf reads the *placed* inner height, even
  zero; a child can overflow without enlarging the host clip. No new pass,
  second child order, wire field, public API or heap-backed expression.
- All **39 detached expectations** passed. Compared with the pinned planning
  run, 32 matched. The seven differences are the 150px floor (fixed/fill B)
  and 99px wrap controls: pixel/fill caps with a fixed host, and plain/capped
  fills in a natural one. At 99px the natural Row is 36px tall and its
  width-fill WrappedRow gets 32px, not 16px. The fixed pixel cap was already
  correct on retained. A Row allocated 100px below a 20px header offers
  88px after its padding; the natural and auto-height controls offer 60px.
- [Eight ordinary viewport tests](../test/emerge_skia/row_cross_height_test.exs)
  check entire delivered frames and raw input. At the 30 → 90 cap edit,
  `(10,76)` stays on the Row until submission, then reaches A at `(4,70)`;
  B is unchanged. The floor paints/hits in the bottom padding, not outside
  the Row. At 100 → 99 → 100 wide the wrapped child's **own** height is
  16 → 32 → 16. A press follows a keyed Column ↔ scaled Row move across
  submission despite a rejected sparse move to an El.
- [Native regressions](../native/emerge_skia/src/tree/tests/row_height.rs)
  cover a policy-only cap → floor (Place, no Measure/Flow/Reflow), extreme
  weight-only edits with no work, nested natural/allocated hosts, zero and
  restore, a closed move between pre- and post-wrap bases at unchanged
  width, scaled masked-pixel overflow, and refused handoff. The
  natural Column still uses a fill child's pre-wrap basis. Three existing
  site tests now use genuinely illegal destinations; the Row work trace
  requires Place, not Reflow, for changed widths at unchanged child height.
  The initial failing traces are retained in `logs/first-matched`.
- **Additional sparse-only failure during implementation:** a keyed move and
  a cap edit can arrive in either order before refresh. In a 76×40 view, no
  host padding or gap, two 38×12 chips with gap 4 wrap inside a 76px child.
  Moving from a content-height Column to a content-height Row while changing
  `min(px(90), fill())` to `min(px(90), min(content(), fill()))` leaves the
  same intrinsic bounds. The first candidate painted the card only 12px
  tall; a fresh tree painted the expected 28px (12px unused below it).
  Compare the
  **first policy and parent together** against the final pair to select the
  pre-/post-wrap basis. Both edit orders and the return move now agree with
  fresh. The submitted click at `(5,20)` follows host → card → host. See
  `logs/red-batched-move{,-live}` and `before-batched-move-fix/`. This was
  a retained implementation bug, not a third pinned-main changelog finding.
- The 4,320 width × height site matrix still rejects a root Row height fill
  and El/WrappedRow parent sites. All ten dimension tests passed. Seed 0
  replay and seeds **0–9,999 × 4 sites × 13 states = 520,000** matched both
  literal math and exact fresh geometry (130,000 independent Row heights).
  These are geometry checks, not 520,000 submitted frames.
- Six release Cargo/strict Clippy profiles passed: **93/96/97/121/121/97**
  (headless, Wayland, GL, Vulkan, both, matched). First native compute after
  reservation, fitting policy/demand edits through prepared paint/hit inputs
  and the pending compute after refused growth used **0 alloc/realloc/free**.
  This does not include provisioning, rasterization or frame delivery.
- **98 focused Mix tests passed** (7 selection exclusions). Full Mix:
  **685/778 passed**, exactly the same **93 failing names / 21 exclusions**
  as `2bf82a9`. A private CPU Wayland window passed **19 seat clicks**, each
  bracketed by a full compositor crop, including the clipped floor, both
  sides of the wrap threshold and the combined move/policy edit. No GPU
  hardware, timing or scanout claim.
- Evidence: `/workspace/emerge-investigation-artifacts/row-cross-height-KzwbUD`;
  see `logs/red-{live,properties}`, `pinned-geometry.diff`,
  `logs/{geometry-expectations,qualification-native,qualification-app,final-window,post-suite-mapped}`,
  named failure lists and `changed.sha256`. An intermediate Mix attempt with
  a stale compile-time file list (`logs/first-move`) did not run tests; the
  final run synced sources, force-compiled and checked the mapped NIF before
  and after. Final SHA-256: `507e7c2aadeceae928da97bb967fe0f2017b383a6401f1fe01e70eceb0af732e`.
