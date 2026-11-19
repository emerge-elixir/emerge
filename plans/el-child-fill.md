# Next slice: fill on the single child of an El

**Proposed after `7229d0a`; not implemented.** A padded `el` can already
host a plain width fill, but not `width(fill(1))` or even `height(fill())`.
Port both axes on this **one incoming edge**: the child does not compete
with a sibling, and its resolved width can change its content height. Leave
root rules, WrappedRow child distribution, alignment and clipping unchanged.

## Contract and before evidence

Use a **120×80 El**, padding 6, no gap. Its only child A starts at `(6,6)`
with an offer of **108×68**. Each fill leaf reads the inner extent on its own
axis, independent of weight. Content reads A's padded demand (after wrapping
for height); authored px scale at A, not a second time on the device offer.

| A's width and height policies | Demand | Proposed A | Unused right / below |
| --- | --- | --- | --- |
| `fill(3)` / `fill(0.5)` | 30×16 | 108×68 | 0 / 0 |
| Both `min(content(), fill(3))` | 30×16 → 90×44 → 150×90 | 30×16 → 90×44 → 108×68 | 78/52 → 18/24 → 0/0 |
| Both `max(content(), fill(3))` | 150×90 | **150×90** | overflow 42 / 22 |

On retained, `width(fill())` with fixed height is **108×16**. Isolated
weighted-width and plain-height uploads are refused. From a valid 30×16
child, sparse width-only, height-only and two-axis cap edits all fail site
proof and leave it 30×16. Pinned `main` (`5c71a593`) accepts those edits
and gives **90×16, 30×44, 90×44**. It matches the weights and caps above,
but limits the two content floors to **108×68**. Both engines give **150×90**
for plain content and for floors using `px(108)` / `px(68)` in place of fill.

**Start implementation red on ordinary headless output.** Keep the fixed
and plain-width controls. Test each axis on its own before combining them
so a width refusal cannot hide a height error. Check cold mount and a sparse
edit from fixed sizes. Make A red and the El white, both clickable; leave A's
demand child unpainted and non-listening. On the small → grown cap edit,
`(76,40)` changes from El local `(76,40)` to A `(70,34)` only when the
new frame is submitted. Check the whole frame, raw press/release in
`handle_input/2` and the resulting application message in `handle_info/2`.

## Two boundaries not to guess

- **Overflow:** El does not impose a Row/Column clip. Put the fixed El in
  a 132×92 Column with no padding/gap in a 160×120 view. A **fixed** 150×90
  child paints/hits beyond the El at `(123,86)` (A local `(117,80)`), but
  not beyond the Column at `(133,86)` (outer view). A fixed 108×68 child
  leaves `(123,86)` to the Column. These are observed *submitted* controls
  on `7229d0a`; require the same behavior for the new cap/floor policies.
- **Wrapping:** narrow an 84×80 El to 83×80, padding 2, no host gap. A is a
  width-fill WrappedRow with padding 2, gap 4 and two 36×12 chips. A uses
  all 80 → 79px of inner width. Its `min(content(), fill(3))` height should
  be **16 → 32**, leaving **60 → 44** of the 76px offer unused. Pinned main
  moves chip two to `y=20` but keeps A at 16. With `content()` it reaches
  32; retained's pixel-cap control also reaches 32. Test A's own paint/hit
  at `(10,24)` and the reverse resize. With a natural-height El, expect the
  wrapper to grow **20 → 36**, no unused inner height; pinned main stays at
  20 for the plain fill child in a content El and the capped fill in an auto El.

Ordinary fixed-size controls are not proof for rejected fill policies. The
[open findings](retained-changelog-candidates.md#open--fill-on-an-el-child)
are detached before measurements. There were 50 cold/patch observations per
revision; 33 were site refusals on retained. Probe, mapped NIF/producer and logs: `/workspace/emerge-investigation-artifacts/el-child-fill-plan-KbhSjr/`
(`probe.exs`, `retained.log`, `pinned.log`, `clip-control.exs`, `retained-clip.log`).

## Implementation and qualification order

1. Extend [final-edge admission](../native/emerge_skia/src/tree/wire/batch.rs)
   for direct El children only. A root El is not its own child. Keep the
   zero-or-one arity and the difference between a removed child and `none()`.
2. In [El layout](../native/emerge_skia/src/tree/shapes/el.rs), use existing
   width Flow and `Length::at_offer`. Fit the wrapper from its child's
   unallotted post-wrap demand up; in Place write the padded offset and
   optional outer height together from the wrapper's *placed* height. Don't
   leave `cx.offset` as a second writer. Separate two facts currently served
   by `in_row`: a Row has already resolved the child's **width**; a Row or
   El child needs post-wrap **height**. Keep natural Column main-axis bases,
   Row width sharing and `BoxPaint` unchanged. No retry, new split, pass,
   child list, wire field or public NIF.
3. Test auto and `content()` wrappers separately: with the 30×16 demand and
   6px padding expect **42×28**, not viewport size. In a 120×80 Column with
   no padding/gap and a 20px header, the El allocated 60px offers **48px**
   after padding; header 80 → real zero → restore. Check child scale 1.5:
   `min(px(20),fill(3))` / `max(px(50),fill(3))` in the fixed El gives
   30×75 (78 unused / 7 overflow).
4. At unchanged 30×16 demand, a cap → floor edit should give **108×68**
   without remeasuring unrelated text. Check padding changes at equal host
   bounds. Compare first and final **policy plus parent** on sparse moves:
   a closed move at equal width but different height basis, and a move plus
   policy edit in both orders before refresh. Keep the press on the same
   occurrence; use the submitted site's scale for the local click. Replace obsolete El-as-illegal
   fixtures (including both cross-axis viewport move tests) with a genuinely
   illegal WrappedRow edge; preserve rollback, masked overflow, failed
   reservation, refused handoff and installed hit/route checks.
5. Extend the [site matrix and seeded oracle](dimension-regressions.md).
   Model one child on **both** axes; don't run the existing 2–5 sibling
   generator under a unary El. Compare literal geometry and exact fresh
   replay for demand, policy, host size, scale and one-child replacement/
   removal/restore; retain the four existing sites. Run extended seed replay,
   first/fitting zero-heap checks, six release Cargo/strict Clippy profiles,
   focused and full Mix (baseline **93 named failures / 21 exclusions**),
   and CPU Wayland compositor crops with seat input before promoting a fix.
