# Weighted main-axis fill

**Implemented on top of `3a7eccc`** (unweighted Column height fill). Uses the
existing EMRG length variant 3 and `fill(n)` DSL, without a new pass, per-child
buffers or presenter changes.

- Admit finite, positive weights for ordinary children of a Row (width) or
  Column (height). A root Row width or Column height gets the whole viewport
  offer, regardless of weight. Leave other weighted sites, cross-axis layout,
  WrappedRow distribution and `min`/`max` out. Prove the final site, including
  kept attrs and moved closed branches; don't restrict ordinary `width(fill())`.
- A definite host distributes its nonnegative remainder in proportion to the
  sum of the weights; `fill()` counts as one. Subtract non-fill outer extents
  and gaps first, including reflowed heights for Column. Offers are already
  device pixels; weights are dimensionless. A content host still uses each
  child's own intrinsic basis, not a proportional split of intrinsic space.
- A weight belongs to the parent's distribution, not the child's intrinsic
  value. Compare effective weights during final-tree invalidation and individual
  offers in width flow / height placement. A numerically unchanged `fill(1)`
  must not remeasure text or disturb an attachment.
- Start with a 90px host, 45/45 → 30/60 → 45/45. Compare cold and edited whole
  frames. At coordinate 35 the click goes A(35) → B(5) → A(35), only after
  submission. Check scaled, nested, fixed/gapped, fractional, zero-room and
  content cases, rejection/rollback and extreme weights. Measure the first
  settle after reservation and fitting edits through paint/hit inputs for
  zero heap calls. Run release Cargo, strict Clippy and the full Mix suite;
  compare failure names to the 93/21 baseline.

## Results

- Ordinary [viewport tests](../test/emerge_skia/weighted_fill_test.exs) were red
  before the port (`renderer_upload failed: unsupported length 3`). Both axes
  now deliver whole 45/45 → 30/60 → 45/45 frames. At coordinate 35 the old
  listener still answers after patch acceptance, until the new frame arrives.
  A scaled child at a fractional split receives the inclusive 22.5 hit but
  not 22.75 in the gap. A separate private CPU Wayland window varied both
  axes independently; five full compositor crops and real seat clicks at the
  same point visited all four colored cells and returned to the first.
- 20/20 detached cases match pinned main `5c71a593` for main-axis origins and
  sizes, including decimal and fractional weights, fixed/padded/gapped,
  exhausted, content, root and scaled cases. An initial all-f64 calculation
  differed by one f32 ULP on a decimal edge; the ordinary path now uses
  device sum → divide → multiply. When narrowing, summation or the unit would
  lose the usable range, the same split uses ratios relative to the largest
  weight. Two maximum f64s or two minimum subnormals still split evenly.
- [Native regressions](../native/emerge_skia/src/tree/tests/weighted.rs) check
  nested distribution after wrap, weight-only invalidation, net-zero edits,
  final-site validation and a refused handoff. A continuous keyed move keeps
  its attachment and receives its new parent's share. Malformed weights and
  unsupported sites preserve pending and submitted state.
- Linux/glibc allocation counter: first settle after reservation, fitting
  weight edits (including extreme weights), insert/remove, and first settle
  after injected growth failure: **0 allocations / 0 reallocations / 0 frees**
  through prepared paint/hit inputs. Controls: Rust `[1,1,1]`, Skia `[1,0,1]`.
  Not a claim about new reservations, rasterization, BEAM or presentation.
- Release Cargo and strict Clippy passed all six profiles: headless 60, shm
  63, GL 64, Vulkan 88, both 88, matched 64 Rust tests. Full Mix: **654/747
  passed**, identical **93 failing names / 21 exclusions**. No backend timing
  or GPU hardware claim.

Evidence: `/workspace/emerge-investigation-artifacts/weighted-fill-PWVW0u`
(`logs/{red-live,ref-decimal,first-decimal,final-oracle,final-window,final-mix}`,
`logs/final-{mapped,test-*,clippy-*}`, `oracle.delta`, `new.failures`, `repaired`).
