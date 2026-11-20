# Fill on the single child of an El

**Implemented; [qualification below](#results).** Both axes on this one
incoming edge receive independent offers. The child does not compete with
siblings, and its resolved width can change its content height. Root rules,
WrappedRow child distribution, alignment and clipping are unchanged. The
plan and before measurements were recorded at `5e8162a` before the change.

## Contract and before evidence

Use a **120×80 El**, padding 6, no gap. Its only child A starts at `(6,6)`
with an offer of **108×68**. Each fill leaf reads the inner extent on its own
axis, independent of weight. Content reads A's padded demand (after wrapping
for height); authored px scale at A, not a second time on the device offer.

| A's width and height policies | Demand | Expected A | Unused right / below |
| --- | --- | --- | --- |
| `fill(3)` / `fill(0.5)` | 30×16 | 108×68 | 0 / 0 |
| Both `min(content(), fill(3))` | 30×16 → 90×44 → 150×90 | 30×16 → 90×44 → 108×68 | 78/52 → 18/24 → 0/0 |
| Both `max(content(), fill(3))` | 150×90 | **150×90** | overflow 42 / 22 |

Before the port (`7229d0a`), `width(fill())` with fixed height gave **108×16**.
Isolated weighted-width and plain-height uploads were refused. From a valid
30×16 child, sparse width-only, height-only and two-axis cap edits all failed
site proof and left it 30×16. Pinned `main` (`5c71a593`) accepts those edits
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

Ordinary fixed-size controls were not proof for rejected fill policies. The
before measurements for the [two findings](retained-changelog-candidates.md#content-floors-on-the-child-of-an-el)
were detached. There were 50 cold/patch observations per
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

## Results

- **Before:** on the mapped `7229d0a` native library the fixed/plain-width
  viewport control passed; the six original fill tests failed **admission**
  (1/7). The width × height site matrix also failed. The final ten viewport
  regressions replayed on that same baseline passed only the control:
  width-only, height-only and combined sparse cap edits each failed on patch.
- **After:** Flow resolves the El child's width against the full offer (a
  Row's distributed width is already resolved). Reflow reports unallotted
  post-wrap height; Place writes the single padded offset and optional
  height from the El's *placed* inner height. It doesn't split or clip.
  No new pass, child order, wire field or public NIF.
- All **50 detached expectations** passed: **42 pinned matches / 8 intentional
  differences**. Three differences isolate the floor on width, height or
  both. Five are 83px wrap cases: fixed-wrapper pixel cap (already correct
  on retained), fill cap with plain/weighted width, and natural-wrapper
  plain/capped fill. Both omitted and content wrappers fit 30×16 demand at
  **42×28**, no inner slack. The allocated El offers **48 → 0 → 48px**; the
  fixed-host scaled pixel limits give **30×75** as specified above.
- [Ten headless viewport tests](../test/emerge_skia/el_child_fill_test.exs)
  check whole delivered frames and raw press/release through application
  callbacks. The two-axis cap gives **30×16 → 90×44 → 108×68**. The click
  at `(76,40)` stays on the El until submission, then reaches A `(70,34)`.
  A **150×90** floor paints and hits beyond the El but not the enclosing
  Column. The width edit gives **16 → 32 → 16** for A's own painted and
  clickable height; a natural wrapper follows **20 → 36 → 20**. A press
  follows a keyed Row ↔ scaled El move with the local coordinates of the
  submitted frame.
- [Native edge controls](../native/emerge_skia/src/tree/tests/el_child.rs)
  check a cap → floor at unchanged demand without Measure, equal-size
  padding edits, extreme plain weights with no work, allocation and scale.
  Closed moves between Column and El compare the first policy **and** site
  to the last in either edit order. Masked scale overflow, refused growth
  and refused handoff preserve the appropriate accepted or installed state.
  Three obsolete native site fixtures and both cross-axis viewport moves now
  use genuinely illegal WrappedRow edges; no exclusions added.
- The **4,320** site-matrix cases and all 11 dimension tests passed. Exact
  seed-0 replay and seeds **0–9,999** checked 14 unary states per seed against
  literal two-axis math and exact fresh geometry. That's **140,000** unary
  states plus the previous **520,000** Row/Column states, not delivered frames.
- Six release Cargo / strict Clippy profiles passed: **97/100/101/125/125/101**
  (headless, Wayland, GL, Vulkan, both, matched). First compute after
  reservation, fitting policy/demand edits through prepared paint/hit inputs
  and compute after a refused reservation each used **0 alloc/realloc/free**.
  This excludes provisioning, drawing and delivery.
- **108 focused Mix passed** (7 selection exclusions); full Mix **695/788**,
  exactly the standing **93 failing names / 21 exclusions**. A private CPU
  Wayland window passed **18 seat clicks**, each bracketed by whole compositor
  crops, including the floor/ancestor clip and both sides of the wrap. No
  GPU hardware, timing or physical scanout claim.
- Evidence: `/workspace/emerge-investigation-artifacts/el-child-fill-nLzQQQ/`,
  `logs/{red-live,red-sites,red-isolated,first-matched,geometry-expectations,final-extended,final-heap,qualification-native,qualification-app,final-window,post-suite-mapped}`,
  `pinned-geometry.diff`, failure lists and `changed.sha256`. A copied Mix
  build with stale compile-time paths (`logs/red-copy-startup-not-a-test`)
  did not run tests; `red-isolated` force-compiled and verified the baseline.
  Final mapped NIF `c48206ee6de63af65f8c332e5ab24a498b61a77d44a6ebf943b8f55f68f54cd8`.
