# Shared animation implementation

Worktree: `/workspace/emerge-animation`, branch `plan/shared-animation-core`.

Implement the feature directly on this branch. No feature flag, test-only runtime,
or separate public-enablement phase. The historical design and detailed numeric
oracles are in [design history](artifacts/shared-animation-design-history.md).

## Feature

```elixir
Animation.animate([[width(px(40))], [width(fill())]], 1000, :linear)
Animation.change([width(fill()), height(content())], 1000, :linear)
```

- Both axes: pixels, content, fill and weighted fill. Min/max expressions remain
  static-only; they are not animation endpoints or change sources.
- One native animation core for regular, enter, exit and change animations.
- Automatic direct-child groups: joint resolved destinations, individual clocks,
  complete allocation-footprint holds, and joint release. Static weighting is unchanged.
- `Animation.change/3` takes a list of attributes sharing duration/curve, with
  independent per-field policies and last-write ordering. Empty lists are no-ops.
- First change-policy mount is immediate. Identical resolved targets and timing-only
  changes do not restart. Interruptions use the published source; plain attributes
  clear their own policy.
- Detect resolved content width/height changes even when the declared
  length remains `content()`, including changes originating in descendants.
- Real layout/reflow, including descendants, images, scroll, clipping and input.
  No paint scaling substitute, frozen subtree or second layout engine.
- Preserve successful patch prefixes and published presentation on failed attempts.
  Keep sources and candidate metadata bounded; acknowledge only successful output.
- Use one renderer-local query workspace. Queries do not hydrate assets or mutate
  published layout, scroll, clocks, scenes or event registries.
- Same input and schedule must replay consistently. Do not hide release jumps by
  weakening footprint validation or restoring the old external context.

## Implementation checklist

- [x] Shared timing, field selection, automatic groups and full-footprint queries.
- [x] Bounded admission deltas, regular field handoffs and source lifetime controls.
- [x] Exclusive prepare/layout/output commit; stale-attempt rejection and ghost
  retirement intents. Publication eligibility excludes cached unpublished mounts.
- [x] Remove test-only runtime gates and restore the public API/codec implementation.
- [x] Public validation, policy codec, macOS protocol 14 and initial native/actor integration tests.
- [x] List-based `Animation.change/3`: shared timing, independent field policies,
  ordered overrides, validation and unchanged policy wire format.
- [x] Detect changes in resolved content width (and symmetric content height), not
  just differences in declared length expressions. Track affected policy-bearing
  ancestors when text/children or intrinsic font/image facts change, even without
  an attribute patch on the owner. An index maintained by existing admission traversal
  and native dirty paths select candidates; there is no per-frame BEAM geometry traffic.
  Native queries still use the existing workspace and can evaluate/copy the full model.
- [x] Resolve the new native destination without the owner's current animation
  sample feeding back into it. Compare against the last admitted resolved target;
  unchanged destinations and animation's own reflow must not restart the clock.
  Animate the complete published footprint over the policy duration; interrupt from
  the currently published pose on a new destination. Preserve first-mount, retry,
  successful-output acknowledgement and joint hold/release semantics. Tests cover text
  growth/shrink, both axes/scales, descendant replacement, metric/image changes,
  independent nested content clocks, identical destinations, timing-only changes,
  interruption/reversal, query/output failures and actor hit geometry. Pending input
  timestamps survive query failures. Cold/idle watchers do not force layout queries;
  inactive workspaces are released when their policies are removed.
  Broader mixed-context/scope transport remains in the unchecked items below.
- [x] Reject min/max animation endpoints in Elixir, native decoding and runtime admission.
- [x] Freeze font facts through native layout and deferred painting; notify idle trees after font loads.
- [x] Deadline input replay uses frozen font/image facts and old interaction/scroll
  seeds, without restoring live inputs. Seed membership must match; opaque changed
  metric epochs without replayable snapshots are rejected. Hover/font/image changes,
  opposite-axis image reflow, initially empty scroll containers and query retries are covered.
- [x] Rendered input checks cover same-size font and image changes, retained/fresh
  raster agreement, and combined font/model/viewport/scale completion changes.
  Failed native witnesses preserve the old raster and registry; old font snapshots
  replay unchanged after a successful new-font output.
- [ ] Finish broader runtime/media/scroll provenance and combined-cause coverage.
- [x] Viewport/scale changes at completion: validate the combined release against
  native layout in the previous viewport, then publish the new native geometry.
  Bad charges/policies still fail; rejected queries retry without reverting the viewport.
  Scale changes promote active/dirty preparation to full preparation for static siblings.
- [x] Declared-container completion edits: sparse first-write declaration preimages
  in the existing workspace, with published model/topology identity and full combined
  release checks. Both axes, all preparation modes, scales .5/1/2, absent properties,
  query-order isolation, corruption and failed-query retry are covered.
- [x] Active/dirty retries include unacknowledged ancestor edits and local-scale
  subtrees; failed publication no longer loses the caller's ancestor preparation.
- [x] Continuous regular pixel-sized parent contexts preserve original curve anchors
  and established held intervals. Native prior-target witnesses retain corruption
  checks; both axes, four curves, 30/60/120Hz and failed-query retries are covered.
- [x] Later pixel segments in mixed regular specifications preserve dependent curve
  anchors. Retained full samples must match native pixel footprints and dependent
  targets on both sides; no published sample is replaced by a pixel declaration.
  Both axes, full/active/dirty, four curves, 30/60/120Hz, global/local scales .5/1/2,
  scoped charges, policy corruption and query/output failure retries are covered.
  Thousand-cycle skips reset segment clocks without replay; these fixtures use at
  most four queries per attempt, including native witnesses in the same workspace.
  Eligibility is conservative: canonical unrotated/unimposed pixel footprints.
  One model copy per unchanged-model run is asserted; query evaluation can still
  cover the whole model. This does not qualify general mixed-foreign continuation.
- [x] Resolved mixed ancestor clocks preserve dependent anchors using native foreign
  destination and full-sample witnesses, not pixel lowering. Both axes, four curves,
  30/60/120Hz, scales .5/1/2, corruption and failed-output retries are covered.
  Mixed-clock ancestry eligibility is checked per consumer, outside shared proof caches.
- [x] Pixel loop wrappers can drive both ancestor intrinsic size and descendant
  allocations in the same finite group. Joint release, intrinsic corruption and
  query/output retries are covered in all preparation modes, within three queries.
- [x] Ordinary scalar pixel parents can cross segment/cycle boundaries at finite
  dependent release. Native prior-target checks permit the new clock input without
  rewinding it. Both axes/modes/scales, loops/finite repeats, thousand-cycle skips,
  corruption and query/output retries are covered within two queries. The exception
  is release-only: moving anchors still retarget across resets. Retained mixed
  samples use the separate full-footprint boundary witnesses below.
- [x] Mixed ancestor segment/repeat boundaries resolve the current full foreign
  sample before dependent release, retaining ordinary frozen owner endpoints.
  Native old/new foreign destinations and dependent targets remain checked; no
  pixel lowering or nonrelease anchor preservation across resets. Both axes,
  simultaneous axes, four curves, scales/modes, late/thousand-cycle skips, content,
  weighted parent-pool charges and retained pixel intervals are covered. Query and
  output retries preserve publication. A 72-case raster/hit matrix covers regular,
  enter, change and exit consumers, old-scene replay and fresh renderer agreement.
  One changed ancestor uses at most 8 queries (12 for both axes) and one unchanged-
  model copy in these fixtures. Selection scans retained ancestry; this is not a
  claim of group-local query evaluation or general feedback/topology transport.
- [x] Independent mixed panels preserve finite motion through native original/hybrid
  target checks, consumer-relative cohort batching and current combined-release
  validation. Both axes/modes/scales/curves, query-order controls, corruption and
  retries pass; 64 owners share at most 8 queries in the fixture. The rendered
  boundary matrix now has 144 cases, including 72 independent-panel variants.
  Shared-pool/upward deadline support is recorded separately below.
- [x] Covered shared-pool/upward mixed-loop deadlines release with native forecast,
  retained-driver and boundary witnesses. Query clock inputs are leaf-only, not a
  retained history; actual feedback samples and joint native release are checked.
  Both axes/modes/scales/curves, resets/large skips, multiple weighted drivers,
  owner/raster/hit cases and corrupt-source/query/output retries pass. Single-driver
  fixtures use at most 12 queries; two-driver fixtures at most 20, one model copy.
  General continuous feedback, self-axis and combined input causes remain open.
- [ ] Finish broader continuous-context retargeting and other deadline changes:
  mixed foreign dimensions, combined causes, segment/repeat resets and runtime/media
  inputs. The pixel-clock case does not complete nested/context-dependent behavior.
  Topology changes deliberately cannot use the declaration-only release witness.
- [x] Cancellation preserves an admitted hold interval; arriving siblings do not
  extend unchanged hold work. Full/active/dirty and failed-query retries are covered.
- [x] Animated content/fill/weighted-fill parents share intrinsic destinations with
  finite dependent children, including pixel and other layout fields. Links cross
  static content/fill wrappers, stop at fixed-axis and Nearby host boundaries, and
  account for cross-axis layout. Independent deadlines and failed-release retries
  are covered. Loops remain outside finite barriers. Upward links are bounded,
  path-compressed, and actual metadata visits are counted.
- [x] Exit captures remap sampled descendant allocation scopes and bake captured
  pixel units, including parent-local charges. Dimension exits preserve the published
  mixed footprint; latest declarations supply exit policies. Validate endpoints before
  capturing pixels. Both axes/scales and pixel/paint-only zero-workspace paths are covered.
- [ ] Complete skipped segments/repeats, broader cancellation/arrival and dependent
  scope phase changes, terminal field sources, full ghost baseline qualification and
  removal/reparent/wrap/float/Nearby transport.
- [x] Persistent automatic pulse failures back off from an immediate retry to a
  250ms cap, retaining no error history and never advancing samples during skipped
  attempts. Both decode policies cover 1,000 pulses, preserved sources/registry,
  recovery via external input or explicit retry, ghost cleanup and final idle Skip.
- [ ] Finish remaining actor/direct/headless and final-damage qualification.
  Ghost terminal/cleanup deliberately use separate outputs.
- [x] Raster/hit-region matrix covers all 16 supported length pairs, four owners,
  both axes, three preparation modes and scales .5/1/2: 1,152 cases, five clock
  samples each, plus retained-renderer replay and fresh-renderer comparisons.
  Scroll viewport tests check actual clipping, offsets/ranges and descendant hits.
  Broader scope/lifecycle cases remain above; this is not platform qualification.
- [x] Cache registry-subtree eligibility across native geometry pulses, preserving
  ordinary mutation/attempt invalidation and rebuilding after external/runtime/
  topology edits. Actual cold visits remain 5k/20k through warm frames; handler and
  Nearby changes plus the raster/hit matrix guard correctness. Empty-registry trees
  avoid empty geometry-snapshot scans. Remaining full-model costs are not hidden.
- [x] Add a reproducible release publication/RSS probe for 5k/20k nodes and 1/64
  owners, with paint/pixel baselines and static/moving native targets. Actual retained
  projections include workspace Arcs; query counters expose full model copies.
  [Raw results and method](artifacts/shared-animation-probe/README.md) show bounded
  records but costly full-model memory and 20k-node release work above 12ms.
- [x] Preserve normal runtime-retirement query/destruction statistics without
  retaining the workspace, and count continuation ancestry lookups. A locked
  72-process matrix includes mixed boundaries and independent panels, immutable
  source/build identities, raw warm samples and subsequent loop cancellation.
  [Evidence and remaining gaps](artifacts/shared-animation-remaining/coverage.md)
  keep full-model memory, 20k release and ~31–33ms cancellation/settle costs open.
- [ ] Complete performance optimization and available platform checks; macOS and
  constrained-device qualification remain unavailable here. User docs/changelog and
  the covered Linux test/CI checks are updated, not a claim of overall completion.

## Current execution

Follow the [detailed remaining implementation plan](shared-animation-remaining-implementation.md).
It maps every unchecked item to P1–P9, with dependencies, implementation tasks,
native proof requirements, test matrices, performance accounting and closure gates.
P1's ledger, ordinary retirement receipt and first locked matrix are recorded;
P2.1's independent-context proof is implemented for the covered matrix. Next close
P1's remaining phase/lifecycle evidence gaps and P2's broader continuous/coupled
and composed-input cases, then P3 provenance before P4 transport and P5 lifecycle. Integration
fixtures and profiling can begin early; final performance/platform qualification
follows those correctness gates. The registry eligibility optimization does not
resolve the remaining full-model memory/release costs.
Native independent-input witnesses supplement the ancestor clock proof; coupled finite motion/holds/releases additionally use leaf forecast evidence. Self-axis,
combined model/clock provenance and arbitrary reparenting remain unfinished. Pixel clocks have native substitution witnesses when needed.
No broad feature-completion or unavailable-device claim follows from green CI.

## Essential regression cases

- Resolved-content behavior: update
  `el([Animation.change([width(content())], 1000, :linear)], text("S"))`
  to the same retained element containing `text("Something")`. Width must animate
  for 1s despite the unchanged `content()` declaration. With native measured widths
  `w0` and `w1`, expect `w0 + (w1 - w0) * t` at t=0/.25/.5/.75/1 under a fixed
  non-wrapping context, with real layout/clip/input updates and no release jump.
  Also cover shrinking, mid-flight text replacement, same measured size/no restart,
  timing-only edits, unchanged first mount, descendant-only edits, content height,
  both-axis policies, and failed-query retries in full/active/dirty/actor paths.

- Static allocation control in a 600px row: `min(px(50), fill)` draws 50 but charges 300; weighted-fill(3)
  charges 450. A fixed-pixel substitution must not give the peer the wrong pool.
- Two 40→fill siblings lasting 1s/2s: 300/170 at 1s, 300/235 at 1.5s,
  300/300 at 2s. Do not release the first child early to 430.
- Parent `[fill, px(400), px(800)]` and child `[px(40), px(40), fill]`, both
  2s/linear: parent/child 400/40 at 1s, 600/170 at 1.5s, 800/400 at 2s.
  The old 600/120 midpoint chases an obsolete target and is incorrect.
- Weight 1→3 beside weight 1: 300→450 with a linear midpoint of 375.
- Min/max animate/enter/exit/change endpoints are rejected; static min/max layout remains supported.
- Deadline resize 600→800 must finish at native 600/200, while bad-charge corruption still fails.
- A 200→content parent with a 40→fill child must not stall at 120/120 and fail
  release. Joint native destinations are 0/0; the 1s/2s case holds the parent at 0
  while the child passes 20 and 10 before joint release.
- A weight-1→3 node removed at its 375px midpoint exits from 375, not its old
  literal keyframe. A 1s exit to 0 passes 187.5 with its sibling at 412.5.
- Failed B followed by A restores A's committed clock/source; retrying B does not
  reset its admitted clock. No failed-attempt history chain.
- Enter paint can hand off while geometry stays held; delayed regular/change fields
  keep independent admitted timing without duplicating specifications per field.

## Validation

After code changes run `cargo test` and `mix test`; use `./ci-tests.sh all` for full
local coverage. Also check Rust formatting, Clippy and benchmark compilation.
Current checks: 1324 Rust unit tests plus 14 integration tests; 494 standalone
Elixir tests/doctests; full CI 499 tests/doctests and zero Dialyzer errors.
Rust formatting, Clippy with warnings denied, benchmark compilation and diff checks
pass. Rust tests passed standalone and through the full CI runner.
Latest checks: `/tmp/shared-final-{cargo,mix,ci,clippy,bench}.log`.
Focused raster checks: `/tmp/rendered-{matrix,scroll,font,image}.log`;
wrapper/continuation checks: `/tmp/pixel-wrapper.log`,
`/tmp/continuation-owner-cache.log`, `/tmp/continuation-cache-direction.log`,
`/tmp/pixel-boundary.log` and `/tmp/pixel-boundary-scales.log`;
registry-cache checks: `/tmp/registry-eligibility-focused.log`.
The first full CI attempt exposed the existing closed-FD-number reuse race in a
headless test. Its assertion now polls the still-owned pipe writer instead; the
rerun passes without changing production fence handling.
These results do not mark the remaining feature work complete.

Mixed boundary checks: `/tmp/mixed-boundary-{expanded,curves,rendered,weighted,both}.log`.
The initial failure is retained in `/tmp/mixed-boundary-repro.log`.

Remaining-plan first-slice evidence: [coverage](artifacts/shared-animation-remaining/coverage.md),
[decisions](artifacts/shared-animation-remaining/decisions.md),
[locked measurements](artifacts/shared-animation-remaining/performance/README.md).
Raw full CI logs and an immutable source archive survive outside `/tmp`. Platform
inventory confirms an available Wayland socket and AMD Vulkan devices; it does not
qualify animation presentation. macOS and constrained targets remain untested.

Coupled-slice logs: `plans/artifacts/shared-animation-remaining/validation/coupled/`.
[96-process coupled baseline](artifacts/shared-animation-remaining/performance/README-02.md)
adds actual forecast/projection retention and exposes the 64-loop upward cost
(~107ms warm / ~86ms release / ~398MiB RSS at 20k), not a passed performance gate.


Ongoing-slice logs: `plans/artifacts/shared-animation-remaining/validation/continuous/`.
[96-process baseline 03](artifacts/shared-animation-remaining/performance/README-03.md)
counts 194 steady upward native queries versus 257 previously, but ~76ms warm /
~87ms release / ~377MiB RSS at 20k/64 remains a failure. D7 records combined-model
cancellation and first change/ghost admission gaps, not completion of P3.


Latest D8 slice: native historical goal receipts fix D7 first change/ghost and
model-plus-cancellation failures. Field-composed self-axis/numeric/mixed witnesses
have wrapping/rotation/imposed-size tests; original-query context replay covers
joint model/viewport/scale/reset and seed membership. Real actor/direct ghost
lifecycle and capacity-one publication tests pass. Logs/functional source identity:
`plans/artifacts/shared-animation-remaining/validation/provenance-fields/`.
General topology/role/unit transport and complete P2/P3/P5/P6 matrices remain open.
Baseline 03 predates receipt storage; its timings are not current qualification.

### D9 checkpoint — directed native structural transport

Native source transport now covers directed reparent/role/unit/root/remount cases,
including numeric fast-path promotion, both-axis and coupled motion, first-write
retries, hold migration, incoming change ownership and ghost cleanup. Added 270
role/unit/curve cases and 24 joint font/image/runtime/scroll/scale traces. See
[coverage](artifacts/shared-animation-remaining/coverage.md) and decisions D9.
Broad P2–P5 qualification and performance remain open; same-ID image primitives
still have mutable render-time bindings. No package is closed by this checkpoint.

### D10 checkpoint — retained image bindings

The D9 same-ID image replay gap is fixed for captured native scenes, including
cached-only assets, replacement before first paint, source/cache reset, raster/SVG
fits, grayscale policy and renderer-local generation collisions. Loader stale
completion is now exercised with real tree-update animation publication under
both decode policies. See decisions D10 and `validation/image-bindings/`.
Atomic layout/scene asset provenance, broader lifecycle/input matrices, retention
budgets and performance/platform qualification remain open.


### D11 checkpoint — atomic frames and bounded actor output

Prepared image inputs now bind dimensions and paint atomically across controlled
replacement/epoch races. Broader orphan/root, noncanonical self-/cross-axis and
complex ghost/hold traces exposed and fixed root source loss and Slider intrinsic
width leakage. A full event channel no longer prevents actor Stop; latest registry/
scene coalescing and native input/raster/damage parity have directed tests. See D11
in decisions/coverage and `validation/atomic-publication/`. Full cross-product,
performance/memory and platform gates remain open; no package closure.

### Post-D11 rebase checkpoint

Rebased onto `headless-backend` `39997f0`; the rebase checkpoint's planning HEAD
was `84906f7`. Implementation subsequently landed in the
[commit sequence](animation-commit-sequence.md). See [collision review](animation-headless-rebase.md)
and `validation/headless-rebase/`: 1392 Rust units + 14 integration, full CI 526
Elixir tests/doctests, Dialyzer 0. Paragraph decoration/animation integration has
18 new directed traces. Old benchmark identities remain historical; cache-budget
and node-storage changes require fresh measurement. No P1–P9 closure.
