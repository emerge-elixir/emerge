# Width and height: regression plan and results

Test the public [size language](../lib/emerge/ui/size.ex) as geometry, pixels
and input, not just as encodable terms. Scope: static sizes on the six active
shapes. The original campaign kept the fill sites of `e411961`; the
[Column-width](column-cross-axis-width.md#results) and [Row-height](row-cross-axis-height.md#results)
follow-ups add independent cross-axis offers. The [El-child campaign](el-child-fill.md#results)
checks a single child with **simultaneous** width and height policies. The
[viewport campaign](viewport-constraints.md#results) applies the same offers
to any outermost kind. [WrappedRow width](wrapped-row-child-width.md) adds per-line plain and numeric fill. Animation lengths are separate.

## Starting point

- Green at `e411961`: omitted/automatic size, fixed px, content/shrink, plain
  width fill, [Column height fill](column-height-fill.md), [weights](weighted-fill.md),
  [redistribution](bounded-main-axis-fill.md#redistribution-correction),
  [nested bounds on one fill](nested-main-axis-fill.md), [bounds on one content leaf](bounded-content.md).
  The existing [known-unit solver tests](../native/emerge_skia/src/tree/shapes/fill/tests.rs)
  include two-ended clamps, exhausted floors and 10,000 siblings. They are
  not randomized declaration or edit tests.
- Red before the fix: the pixel-cap control passed, but both the cold and
  sparse [mixed-content viewport cases](../test/emerge_skia/mixed_content_fill_test.exs)
  failed with `unsupported length 4`. The two are ordinary, unexcluded tests.
  The pre-fix full suite had **95 failing names**, two more than committed
  `e411961`; both had 21 exclusions.
- [Producer](../lib/emerge/engine/attr_codec.ex) tags: 0 fill, 1 content,
  2 px, 3 weighted fill, 4 min, 5 max. Omission has **no length tag**;
  `shrink()` is exactly `content()`. DSL acceptance alone says nothing about
  supported sites or sibling allocation. No new dependency or wire format.

## Coverage matrix

| What to vary | What must hold | Regression |
| --- | --- | --- |
| Leaves, both ops, both operand orders | Test all six samples (px 0 and 50, content, plain fill, weights 1 and 3); both axes, plus omission as a whole policy. Include pairs of constants, repeated content and multiple fills. Preserve the producer's operand order on export. | [Paired declarations](../native/emerge_skia/src/tree/tests/dimensions.rs), [codec and alias](../test/emerge/attr_codec_test.exs), [wire byte controls](../native/emerge_skia/src/tree/tests/nested.rs) |
| Width **and** height on one node | Cross 12 representative policies per axis at each of six roots and below each legal **parent shape** (Row, WrappedRow, Column, El) × six child shapes. Check actual extents for admitted cases and unchanged accepted bytes on site refusals. Text uses its content-only metric as a control, not a hardcoded font advance. | [4,320 width × height cases](../native/emerge_skia/src/tree/tests/dimensions/sites.rs) |
| Root, direct, nested and moved sites | The viewport offers **both** axes to every supported kind. A direct Row/Column/El parent also supports both axes, splitting only a Row's child widths and a Column's child heights. A WrappedRow shares each line among plain and weighted width fills; compound fill width and fill height remain site refusals. Moves and promotions check the final edge and scale, even on a closed subtree. | [Sites and pending/refused candidates](../native/emerge_skia/src/tree/tests/dimensions/sites.rs), [scaled masked operands](../native/emerge_skia/src/tree/tests/nested.rs) |
| Definite vs natural hosts | Subtract host padding, adjacent gaps and non-fill **outer** extents. A content Row offers intrinsic widths; a content Column uses fill children's pre-wrap bases. Auto/content-height Rows fit the tallest unallotted, post-wrap child. An allocated nested host subdivides even a zero offer. Auto is a separate case: an auto-width Column with a width-fill child can consume an offer. | [Row](../native/emerge_skia/src/tree/tests/row.rs), [Column](../native/emerge_skia/src/tree/tests/column.rs), [bounded content](../native/emerge_skia/src/tree/tests/content_bounds.rs) |
| Width changes the height | Test just below/at/above a wrap threshold. Check second-line origin **and** host height and following sibling. A mixed content limit in a definite Column reads the actual post-wrap padded extent; without a slot a fill uses its pre-wrap basis. Exercise content caps, floors and weight changes separately. | [Wrap matrix](../native/emerge_skia/src/tree/tests/dimensions/sites.rs), [delivered frames and clicks](../test/emerge_skia/dimension_regression_test.exs) |
| Nested min/max, numeric boundaries | Test crossed/redundant limits, both ends binding, many limited siblings, all caps/slack, impossible floors/overflow, zero room and padding/gaps exceeding the host. Scale authored px once at the declaring site, not an already-device offer. Preserve ordinary f32 split rounding; exercise max finite and minimum subnormal f64 weights, including a tiny surviving slope. | [Expression laws and extremes](../native/emerge_skia/src/tree/tests/dimensions/laws.rs), [scale](../test/emerge_skia/layout_scale_test.exs), [fractional hit edges](../test/emerge_skia/weighted_fill_test.exs) |
| Rejection vs successful edits | Unknown/truncated operand, negative or nonfinite px, zero/nonfinite weight, depth 64/65, masked pixel overflow and resource growth refusal. No partial tree, lost pending edit, new installed pixels, hit target or press. A valid mixed length at the wrong site is a **site error**, not a malformed expression. | [Negative generator](../native/emerge_skia/src/tree/tests/dimensions/laws.rs), [admission](../native/emerge_skia/src/tree/tests/bounded.rs), [heap and growth controls](../native/emerge_skia/src/tree/tests/allocations.rs) |

## Oracle and property-based campaign

1. **Specify the math independently.** For an authored tree `E`, evaluate
   each `px` at its site scale, each `content` as padded demand, each
   `fill(w)` as `w × u`, and each `min`/`max` literally. With a definite
   main-axis host, choose a nonnegative shared unit `u` that consumes the
   remaining room. If the sum at zero already exceeds the room, use the
   floors. If even the sum at infinity is smaller, leave the slack. Never
   assert unconditional conservation. In a root expression all fill leaves
   instead get the same viewport extent. On the cross axis each Column child
   reads the same inner width, each Row child the same *placed* inner height,
   without summing siblings. An El offers its single child the inner width
   and the *placed* inner height. On both Row and El edges, content in the
   height expression reads the child's post-wrap demand.
   In an intrinsic measurement all variable leaves get the same natural extent.
2. **Generate and check valid edits.** No property framework was in the
   project manifests. Use a test-only fixed integer PRNG (no clock or new
   dependency), bounded expression depth, and small keyed trees so replays
   are cheap. Generate 2–5 children: min/max up to depth 3 (4 on edit),
   dyadic weights ¼–4, fixed-content descendants, child padding and zooms.
   Each seed checks Row width, Column height, direct Column child width and
   direct Row child height through 13 settled states per site: cold mount,
   demand/policy edits, resize with padding/gap, child scale, reorder,
   display scale, zero room and restoration, insertion/removal and an edit
   undone before settle. On the dyadic small-number domain the test brackets
   and bisects the literal sum in f64. It does not call the native fold or
   splitter. Compare **every origin and extent** with a rounding allowance
   of `8 × f32::EPSILON × max(1, |expected|)`; also compare the sparse result
   **exactly** with a fresh mount of the same declaration. Agreement between
   two renderer runs alone cannot catch a shared mathematical mistake.
   The [unary generator](../native/emerge_skia/src/tree/tests/dimensions/unary.rs)
   evaluates independent policies on both the viewport's element and its
   single El child. Its 18 states include a natural wrapper, scale edits,
   replacing/removing/restoring the child, undo before refresh, a root policy
   edit and viewport-only fractional, zero and restored offers.
3. **Check more than randomness.** Hand-computable/extreme cases assert exact
   splits. Commuting the operands or repeating a term under min/max must
   preserve sizes at the *same* legal site. Transposition applies to a flat
   main-axis line, not to text metrics, wrapping or the asymmetric fill
   admission rules. No promise that reordering siblings preserves origins,
   or that scaling a rendered pixel image yields the next scale's image.
   A separate rejection corpus mutates just one operand and tests rollback;
   invalid examples do not count as successful layouts.
4. **Replay and minimize.** Normal Cargo tests run seeds 0–63. Print seed,
   site, first failing step, actual/expected and the complete case. On cold
   failures try removing siblings and replacing an operator with a child
   while retaining the failure class. For a sparse-only failure retain the
   ordered edit prefix (do not “shrink” it to an unrelated cold case);
   remove edits only if the mismatch still reproduces. Give each distinct
   minimized failure a named permanent test. For a user-visible bug on main,
   add the before/expected scene, pinned revision and evidence to the
   [tracker](retained-changelog-candidates.md); promote only after verifying
   the fix on a submitted frame. Matching main is not a gate.

To replay the generator from the repo root (use a private target directory):

```sh
export CARGO_TARGET_DIR="$(mktemp -d)" CARGO_NET_OFFLINE=true RAYON_NUM_THREADS=1
EMERGE_DIM_SEED=0 EMERGE_DIM_CASES=1 ./scripts/performance-lock.sh shared \
  cargo test --manifest-path native/emerge_skia/Cargo.toml --release --locked \
  --features bench-diagnostics seeded_
```

Set `EMERGE_DIM_CASES=10000` for the extended run. The dedicated math, site,
wrap, rollback and byte tests run with the `tree::tests::dimensions` filter;
`cargo test` also includes the existing tree/allocator controls. Use the
normal mapped, source-built NIF for Mix. Do not add a production oracle, a
second child order, direct callback dispatch or a test-only native API.

## Named acceptance scenes

- **Content cap.** 120×12 Row, no padding/gap: red A `min(content(),fill())`
  and blue B `fill()`. At demand 20: **20/100**. At demand 90: **60/60**.
  Neither leaves pixels unused. While the edit is pending, `x=35` still
  reaches B at local 15; after submission it reaches A at 35. Pinned main
  gave 20/60 with **40 unused** at the smaller demand. Preserve the existing
  pixel-cap control and the distinct cold-mount and sparse mixed test names.
- **Changing slope.** No padding/gap. A `max(min(fill(3),px(30)),fill())`
  alongside B `fill()` in hosts 20, 50 and 120 gives **15/5**, **30/20**,
  **60/60**. Zero unused/overflow in each. No single weight + pixel clamp
  represents this function. Test both axes; compare whole headless bands.
- **Width → height.** 100×120 Column, no padding, gap 4. A WrappedRow has
  padding 2, gap 4 and two 46×20 chips; B is height-fill. At width 100,
  A `min(content(),fill())` gets **24**, B **92**. At width 80 they get
  **48/68**; B starts at 52. Changing A to `max(content(),fill(3))` at 80
  gives **87/29**. No vertical slack/overflow. Assert the entire frame and
  the click at `(5,42)`, not just a detached layout query.

## Execution and evidence

- On `e411961`, **80 of 144** ordered min/max cases were red: 16 px/px,
  4 content/content, 24 content/fill and 36 fill/fill (counts include both
  axes and operand orders). Seed 0 failed on the initial mount. All three
  initial native campaign tests were red; the mapped headless control was
  green and the two mixed cases were red. These were admission failures,
  not evidence that the old splitter had computed those geometries.
- Fix: ingress folds constants and single-variable intervals. For a genuinely
  mixed formula it keeps offsets into the owner's validated bytes, not a
  heap AST. At a candidate unit each active term is a fixed charge or a
  weight. The same Row/Column allocator solves on the interval where those
  comparisons hold, including when the surviving weight changes. Intrinsic
  cache keys stay weight-independent. A definite splitter observes changes
  in mixed content demand even if the host frame is unchanged.
- Three older native tests assumed valid pairs must be malformed. The first
  full native run identified all three by name (`logs/first-matched`); they
  now check real invalid operands or a wrong site. The formerly negative
  grammar classes have positive geometry tests. No test excluded or renamed.
- Evidence directory: `/workspace/emerge-investigation-artifacts/dimension-regressions-ARsmvZ`.
  See `logs/red-{properties,live}`, `logs/final-{test-*,clippy-*,seed0,extended,heap}`,
  and `logs/{final-focused,final-mix,final-window,post-suite-mapped}`. The
  pinned-main comparison is in the earlier `mixed-content-fill-red-V88ePA`
  seal, not a new main run.

### Results

The following results are for the original two main axes at `2c802f6`;
see the [three-site](column-cross-axis-width.md#results) and subsequent
[four-site qualification](row-cross-axis-height.md#results), then the
[unary two-axis qualification](el-child-fill.md#results) and the
[viewport constraint results](viewport-constraints.md#results) for the extensions.

- **144/144 ordered expression cases** now meet the independent geometry
  expectations. The width × height site matrix and all 10 native campaign
  tests passed. Exact seed-0 replay and extended seeds **0–9,999** passed:
  **260,000 settled states** (10,000 × two axes × 13) compared with both
  public length math and a fresh tree. These are geometry checks, not
  260,000 submitted frames or an exhaustive proof over all declarations.
- The original mixed-cap viewport cases now pass unchanged. **84 focused
  Mix tests passed**, with 7 other tests excluded by selection. The full
  suite passed **671/764** (including 15 doctests), with **93 failures and
  21 exclusions**. The 93 failing names are exactly the committed sizing
  baseline's: `repaired` contains just the cold and sparse mixed-cap cases;
  `new.failures` is empty. Tests involving unported alignment, animation,
  images, Nearby and other features were neither weakened nor excluded.
- Private Wayland shm: **10 scene/click checks**, on both axes, with a full
  compositor crop before and after each seat click. The 120px content cap,
  content floor and changing-weight policy all matched their expected colors,
  target and local point. The first attempt used a stale Sway title rule,
  got a tiled 720×500 window, and did not count; the corrected run confirmed
  the 120×120 content rect and used its actual origin. `logs/window-wrong-rule`
  preserves the failed setup. No hardware GPU, timing or scanout claim.
- Six release Cargo/strict Clippy profiles passed: **85/88/89/113/113/89**
  (headless, shm, GL, Vulkan, both, matched). First native compute after
  reservation, fitting mixed edits and pending compute after refused growth
  each had **0 alloc / 0 realloc / 0 free** through paint/hit preparation;
  positive Rust/Skia and growth controls fired. A private Mix run with stale
  `external_resources` was discarded; force-compiled BEAMs and verified the
  mapped NIF inode/bytes before and after the suite. Final NIF SHA-256:
  `e8d7fda22894713001b489519460805f5208fed9f20cf6c1a2c71054934e3193`.
