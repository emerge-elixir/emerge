# Next: pixel caps and floors on main-axis fill

**Plan only, after [weighted fill](weighted-fill.md) (`bd40808`).** Support one
`min` (cap) or `max` (floor) with exactly one `px(n)` and one `fill()` or
`fill(w)`, in either operand order. Start where a share already exists: width
on a root Row or a Row's direct child; height on a root Column or a Column's
direct child. Keep existing plain fill working everywhere it works today.
Defer recursive length expressions, `content()` as an operand, cross-axis
stretch and WrappedRow distribution. No DSL, wire, NIF or presenter change.

## The first regression: same box, different sibling

Give a 120px host a green background and click listener, a red child A and a
blue `fill()` child B. Pinned main (`5c71a593`) gives these extents on **both**
axes. B starts at the end of A; its share is not recomputed after a bound.

| A's declaration | A | B | Visible result in the host |
| --- | ---: | ---: | --- |
| `px(20)` | 20 | 100 | No green |
| `min(px(20), fill())` | 20 | 60 | Last 40px green |
| `min(px(20), fill(3))` | 20 | 30 | Last 70px green |
| `max(px(80), fill())` | 80 | 60 | B extends 20px past the host; clip it, don't shrink it |

On the current renderer the first row delivers the expected whole frame. At
coordinate 90 a raw click reaches B at local 70. Editing A to the second row
logs `renderer_patch failed: unsupported length 4`; the old pixels and click
still work. Make that edit succeed: while pending, B must still answer; **only
after submission** should coordinate 90 reach the green host at local 90.
Then change just the weight: coordinate 65 must switch from B at local 45 to
the host at 65, even though A stays 20. Reverse both edits. Compare each
submitted frame with a cold mount and the expected color bands.

## Implementation order

1. In `tree/wire.rs`, decode the two simple operands of EMRG tags 4/5 into a
   small copyable policy (cap/floor, authored px, weight), not a boxed tree.
   Preserve the original wire bytes; reject nested, malformed or unsupported
   operands before commit. Validate the final site in `wire/batch.rs`, even
   for a kept branch moving or becoming the root. Check the px bound at the
   child's effective scale during admission, including a move or scale edit.
2. Keep the **share** distinct from the **outer extent**. In `FillShares`,
   keep the existing weight denominator and fixed/gap charges (reflowed
   non-fill heights for Column). Apply the bound to the resulting share;
   never redistribute slack or overflow. Use the existing Row width offers
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
plain/fixed controls match, the 24 bounded cases reject on retained with
`unsupported length 4` or `5`. Each used its own producer and verified mapped
NIF. The two live rejection probes were ordinary **binary headless** viewports,
not tests of successful bounded rendering. Scripts and logs:
`/workspace/emerge-investigation-artifacts/emerge-bounded-fill-plan-Nk0pqa`.
