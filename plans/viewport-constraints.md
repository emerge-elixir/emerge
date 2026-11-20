# The viewport supplies an ordinary layout constraint

Start at `22488de`. A viewport offers an independent width and height in
**device pixels** at `(0,0)`, just as a zero-padding El of that size would.
All six supported kinds may be on that edge. Explicit fill can read the
offer; px, content and auto keep their normal sizing behavior. A Row's
already distributed width or a Column's assigned height is not a new offer
to resolve. A WrappedRow's *children* gain no new distribution behavior.
No synthetic mount, new pass, wire change, or renderer API.

- First compare cold and sparse width-only, height-only and combined cases
  on retained and pinned `5c71a593`. Keep recorded behavior distinct from
  the contract. On a 120×80 viewport, no gap, a 42×28 El with padding 6
  and a fixed 30×16 child fits exactly. After explicit fill on both axes
  it is 120×80; the child stays at `(6,6)` with 78/52 unused. Check the
  whole delivered frame and the raw click at `(76,40)` before/after submit.
- Feed the viewport into the ordinary incoming-edge storage. Shapes consume
  constraints, not their root status. Preserve the natural Column's pre-wrap
  fallback and main-axis sharing. Check all six kinds against literal sizes
  and against the same subtree in an El. Include content limits, wrapping,
  viewport-only changes, zero/restore, scale and promotion at equal viewport
  dimensions. A refused edit or handoff must not change installed input.
- Replace obsolete root refusals with positive equivalence/promotion tests;
  preserve actual negative cases on WrappedRow edges. Run the seeded and
  allocation controls, all six Cargo/Clippy profiles, focused and full Mix
  against the 93 named failures / 21 exclusions, and a CPU Wayland resize
  with compositor crops and seat input. No GPU or timing claim.

## Results

**One incoming edge.** The viewport supplies available width and height at
`(0,0)` through the same [edge writers](../native/emerge_skia/src/tree/layers.rs)
used by containers. An axis can be natural, available, or already assigned.
Zero is a size, not absence. Width flow resolves available width; reflow
reports the element's fit at that width; placement resolves available height.
No shape tests whether it is the root. A Row still assigns distributed widths,
a Column still assigns heights, and a natural Column still charges its fill
children's measured pre-wrap bases. There is no extra layout pass, node, or
invalidation during acceptance.

- **Before:** on `22488de`, 14 of 36 detached inputs were refused, all at the
  viewport. The zero-padding El controls already worked. The initial four
  public tests failed on admission. Replaying the final seven on that build
  passed only the existing Column basis-switch control. On pinned `5c71a593`
  the simple viewport cases were *accepted*, cold and patched. Both revisions
  used their own producer and verified mapped library.
- **After:** all 36 literal geometry expectations passed, 34 matching pinned.
  In a 120×80 viewport, El padding 6, no gap, switching from fixed 42×28 to
  width fill only, height fill only, or both gives **120×28, 42×80, 120×80**.
  The fixed 30×16 child stays at `(6,6)`, leaving **78/0, 0/52, 78/52**
  unused to its right/below. While the combined edit was pending, `(76,40)`
  did not reach the El; after delivery it reached the El at `(76,40)`.
- **Wrapping:** a viewport-width WrappedRow, padding 2, gap 4, two 36×12
  content-height chips, `height(min(content(), fill(3)))`. Resize **80×80 →
  79×80 → 80×80** without editing the tree: height **16 → 32 → 16**,
  unused below **64 → 48 → 64**. The second origin switches between `(42,2)`
  and `(2,18)`. Constraining height to 10 at width 79 gives height 10,
  clipping the remaining 22px. In the narrow case pinned got 32 with plain
  content but 16 with the cap, both at the viewport and inside an El. These
  are the two differences; the nested case was already correct on retained.
  A separate fixed-height/empty-chip probe also hit the known pinned
  [line-gap issue](retained-changelog-candidates.md#wrapped-rows-preserve-the-vertical-gap-between-lines).
  Those runs are saved separately, not counted in the 36.
- **Sparse edits:** a keyed panel was promoted from a scaled Row and demoted
  without resizing the viewport. A held press followed the occurrence and
  used the submitted site's local coordinates. Masked pixel overflow on a
  closed promotion leaves the accepted edit intact. A refused handoff leaves
  the old pixels and hit target installed; retry installs the new origin and
  extent. [Native controls](../native/emerge_skia/src/tree/tests/viewport.rs)
  · [seven ordinary viewport tests](../test/emerge_skia/viewport_constraints_test.exs).
- **Two intermediate regressions, not pinned fixes.** First the old Column
  assignment could survive a switch to a nonfill height (seed 6, ColumnHeight
  step 2: 12.75 instead of 9). Dropping an assignment now schedules placement
  even if the new policy ignores an available height. Then a 76×80 viewport,
  content-capped Column, no padding/gap, two 38×12 chips in a WrappedRow with
  gap 4 kept the Column at 12 after child `fill()` → `content()`. The width,
  measured height (12) and post-wrap fit (28) had not changed; which value
  the Column reads **had**. Native and delivered frames now give **12 →
  28 → 12**, leaving 68 → 52 → 68 below. In the middle frame `(5,20)` reaches
  the Column. Illegal WrappedRow child edges remain illegal; obsolete root
  refusals were replaced, not excluded.

### Qualification

- All 4,320 width × height/site cases and 11 dimension tests passed. Seeds
  0–9,999: **180,000** unary states including root policy and viewport-only
  edits plus **520,000** Row/Column states, checked against literal math and
  exact fresh geometry. These are not submitted-frame counts.
- Six release Cargo / strict Clippy profiles: **101/104/105/129/129/105**
  (headless, Wayland, GL, Vulkan, both, matched). First compute, fitting
  viewport size/patch changes through prepared paint and hit inputs, and the
  existing post-refusal compute control used **0 alloc/realloc/free**. This
  excludes reservation, rasterization and delivery.
- **115 focused Mix passed** / 7 selection exclusions. Full **702/795**
  including 15 doctests; the same **93 failing names / 21 exclusions**.
  Private CPU Wayland: **16 seat clicks**, whole compositor crops before and
  after each, including real resize-only changes. No timing, GPU hardware or
  physical scanout qualification.
- Evidence: `/workspace/emerge-investigation-artifacts/viewport-constraints-0DWDbg/`.
  `logs/{red-sites,before-seven,first-matched,red-switch,red-switch-live}`
  preserve failures; `logs/{geometry-expectations,qualification-native,qualification-app,final-window}`,
  `pinned-geometry.diff`, the failure lists and source manifests record the
  final checks. Mapped NIF SHA-256:
  `e17dfe6807dce2ac579f2e265a9c0af1f7e151d3215593eab7ac6d9b2ba57292`.
