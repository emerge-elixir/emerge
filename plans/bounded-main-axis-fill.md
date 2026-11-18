# Pixel caps and floors on main-axis fill

**Implemented after [weighted fill](weighted-fill.md) (`bd40808`).** Supports one
`min` (cap) or `max` (floor) with exactly one `px(n)` and one `fill()` or
`fill(w)`, in either operand order. Start where a share already exists: width
on a root Row or a Row's direct child; height on a root Column or a Column's
direct child. Keep existing plain fill working everywhere it works today.
This checkpoint deferred recursive expressions; the [next slice](nested-main-axis-fill.md)
implements nested pixel bounds around one fill. `content()` as an operand,
cross-axis stretch and WrappedRow distribution remain separate. No DSL, wire,
NIF or presenter change.

## Main-parity checkpoint: same box, different sibling

**Historical comparison:** the 20/60 result below reproduces a bug on `main`.
Retained now gives the uncapped B the remaining space: [20/100, verified for
the changelog](retained-changelog-candidates.md#capped-fill-leaves-room-unused).

Give a 120px host a green background and click listener, a red child A and a
blue `fill()` child B. Pinned main (`5c71a593`) gives these extents on **both**
axes. B starts at the end of A; its share is not recomputed after a bound.

| A's declaration | A | B | Visible result in the host |
| --- | ---: | ---: | --- |
| `px(20)` | 20 | 100 | No green |
| `min(px(20), fill())` | 20 | 60 | Last 40px green |
| `min(px(20), fill(3))` | 20 | 30 | Last 70px green |
| `max(px(80), fill())` | 80 | 60 | B extends 20px past the host; clip it, don't shrink it |

Before this port, the first row delivered the expected whole frame. At
coordinate 90 a raw click reached B at local 70. Editing A to the second row
logged `renderer_patch failed: unsupported length 4`; the old pixels and click
still worked. The parity test made that edit succeed: B answered while it was
pending; after submission, coordinate 90 reached the green host. Changing just
the weight also switched coordinate 65 from B to the host. These are observations
of the old allocation rule, not acceptance criteria for the redistribution fix.

## Redistribution correction

In a definite host, first subtract padding, gaps and fixed outer extents. Split
what remains by weight **among children still free to grow or shrink**. A cap
or floor consumes its actual size, not the share it refused. Keep surplus
only if every fill caps out; overflow only when the floors cannot fit. An
unbounded/content host still uses intrinsic bases. Keep the ordinary unbounded
rounding and the existing final-tree invalidation/submission boundary.

- [The shared allocator](../native/emerge_skia/src/tree/shapes/fill.rs) keeps
  the original unbounded split when no limit binds. Between two bound/weight
  thresholds the locked extents are constant; divide the residual by the
  remaining weights. Search intervals rather than freezing both ends at once:
  a provisional floor can stop binding after a cap releases space. The search
  uses scalar scratch, no second child order, and at most 64 bisections. It
  scales free weights to their own largest weight, not to a saturated sibling.
- All **28/28** independent geometry expectations pass (12 still match pinned
  main; 16 intentionally differ). [Native regressions](../native/emerge_skia/src/tree/tests/bounded.rs)
  cover cascades, mixed bounds, zero room, both child orders and extreme weights.
  Known feasible splits are also checked with up to 10,000 participants.
- The Row and Column 20/100 regressions failed on the previous mapped NIF.
  All six [headless tests](../test/emerge_skia/bounded_fill_test.exs) now pass,
  including `text("A")`/`text("B")`, a fixed→capped edit with identical pixels,
  a narrowing host where the cap releases, and changed offers that wait for
  submission. All 14 private CPU Wayland clicks matched full compositor crops;
  coordinate 90 reached B at local 70 on both axes.
- Zero native heap calls on the first settle after reservation and fitting
  edits through paint/hit preparation. Release Cargo/strict Clippy: **68/71/
  72/96/96/72** tests in headless/shm/GL/Vulkan/both/matched profiles. Full
  Mix **660/753 passed**; same **93 failing names / 21 exclusions**. No GPU
  hardware or timing claim.

Correction evidence: `/workspace/emerge-investigation-artifacts/fill-redistribution-cbXGvD`
(`logs/{red-live,geometry-expected,final-window,final-mix}`, `logs/final-test-*`,
`new.failures`, `repaired`). The sections below record the earlier checkpoint.

## Original port: implementation order

1. In `tree/wire.rs`, decode the two simple operands of EMRG tags 4/5 into a
   small copyable policy (cap/floor, authored px, weight), not a boxed tree.
   Preserve the original wire bytes; reject nested, malformed or unsupported
   operands before commit. Validate the final site in `wire/batch.rs`, even
   for a kept branch moving or becoming the root. Check the px bound at the
   child's effective scale during admission, including a move or scale edit.
2. Keep the **share** distinct from the **outer extent**. In `FillShares`,
   keep the existing weight denominator and fixed/gap charges (reflowed
   non-fill heights for Column). Apply the bound to the resulting share;
   this checkpoint does not redistribute slack or overflow. Use the existing Row width offers
   and Column placements. Scale the px operand once at its declaration, not
   the offered pixels or weight. A content-sized host bounds each child's own
   intrinsic basis; a bounded root uses the viewport as its fill offer. A
   nested allocating host distributes its *bounded* outer extent minus padding.
3. In `changes.rs`, `layers.rs` and `intrinsic.rs`, distinguish intrinsic
   inputs from distribution inputs. A changed bound or child scale can move
   a sibling even at equal intrinsic size and weight. A weight can resize B
   at equal A. Invalidate the parent reader once against the final tree;
   compare each offer/offset, and keep glyphs and attachments when unchanged.
   Swapped operands, `fill()` vs `fill(1)`, and A→B→A edits should not schedule
   layout for a semantically unchanged final policy.

## Acceptance

- Cover cap-only edits, a floor with zero remaining space, fractional weights
  and hit edges, wrap changing a non-fill sibling, and root/content hosts.
  A 12px cap at child scale 1.5 is 18 device pixels; a nested host capped at
  20 splits 1:3 into 5/15. Keep the extreme-weight fallback. Use an enclosing
  listener to distinguish host clipping from viewport clipping. Exercise refused handoff,
  keyed moves, invalid sites and growth failure with an earlier edit pending.
- Measure the first settle after reservation and fitting sparse edits from
  wire through prepared paint/hit inputs: zero alloc/realloc/free calls, with
  positive controls. Run release Cargo/strict Clippy and focused/full Mix;
  compare exact names to the **93 failures / 21 exclusions** baseline. Only
  claim window behavior after compositor pixels and real seat input.

Planning evidence: 28 detached cases per revision (14 per axis); the four
plain/fixed controls matched, the 24 bounded cases rejected before this port with
`unsupported length 4` or `5`. Each used its own producer and verified mapped
NIF. The two live rejection probes were ordinary **binary headless** viewports,
not tests of successful bounded rendering. Scripts and logs:
`/workspace/emerge-investigation-artifacts/emerge-bounded-fill-plan-Nk0pqa`.

## Original port results (before redistribution)

- All **28/28** detached cases at that checkpoint matched pinned main for main-axis positions
  and sizes. The five new [headless tests](../test/emerge_skia/bounded_fill_test.exs)
  went from **0/5 to 5/5**. Whole delivered frames, raw input and callbacks
  distinguish the pending from the submitted split. A click just outside the
  floored host reaches the enclosing listener, even inside the child's frame.
- [Native controls](../native/emerge_skia/src/tree/tests/bounded.rs) cover
  cap-only and child-scale-only edits, unchanged intrinsic boxes, operand
  reversal with raw bytes preserved, zero remaining room after wrap, extreme
  weights, viewport resize, and closed moves. A refused submit preserves the
  old pixels and hit; invalid edits and an overflowing scale preserve the
  earlier pending declaration. A truncated-operand fixture initially included
  another attribute whose tag became the operand; the corrected fixture ends
  at the missing operand and rejects as truncated.
- A private CPU Wayland window matched whole compositor RGB crops before and
  after **14 real seat clicks** across both axes, including the exposed host
  and the clipped overflow. No GPU hardware, timing or scanout claim.
- First settle after reservation, fitting bounded edits and first settle after
  refused growth: **0 allocations / 0 reallocations / 0 frees** through paint
  and hit preparation. Release Cargo/strict Clippy passed in six profiles:
  headless 65, shm 68, GL 69, Vulkan 93, both 93, matched 69. Full Mix:
  **659/752 passed**, identical **93 failing names / 21 exclusions**.

Evidence: `/workspace/emerge-investigation-artifacts/bounded-fill-Nyf6m0`
(`logs/{red-live,final-oracle,final-focused,final-window,final-mix}`,
`logs/final-{test-*,clippy-*,mapped}`, `oracle.delta`, `new.failures`, `repaired`).
