# Width fill inside a WrappedRow

**Implemented:** plain and numeric width fills now share each completed line
by weight, with elm-ui's two-card wrap thresholds. This intentionally changes
existing plain-fill layouts. [After results](#results) · [chosen rule](#selected-behavior-elm-ui-wrapping-widths).

**Before:** two `width(fill())` children took **one whole line each** on
retained `44c0dc2` and pinned `main` `5c71a593`. Changing only the second
width to `fill(3)` did not turn the WrappedRow into a 1:3 Row on either
revision. Pinned accepted the change; retained rejected it. These are
observations of those revisions, not the selected behavior. The width logic
was also unchanged in the subsequent local `main` source at `aef5e42`;
the executed geometry probe used the pinned revision.

## Scene and results

100×40 host at scale 1, no padding, gap 4 on both axes. Clickable cards A
and B each have 20×12 of content and a content-sized height. Only B's width
changes during the weight edit. A, B and the host have distinct solid colors.

| Case | Outer card boxes `(x, y, width, height)` | Click at `(50,6)` |
| --- | --- | --- |
| WrappedRow, both `fill()`, both revisions | A `(0,0,100,12)`, B `(0,16,100,12)`; 12px below | A |
| WrappedRow, B `fill(3)`, pinned | Same two boxes, cold and patched | A |
| WrappedRow, B `fill(3)`, retained | Cold upload and patch refused. After the patch the **previous two boxes** remain installed. | A on the prior frame |
| Single-line Row, B `fill(3)`, both revisions | A `(0,0,24,12)`, B `(28,0,72,12)`; 28px below | B (local `(22,6)` on retained) |

Pinned also accepted `fill(1)` at B with the same detached geometry;
retained refused it both cold and patched. The two `content()` controls
fit on one line at x=0 and x=24, with 56px to the right and 28px below.
The cards' intrinsic widths did not force the two-line result.

**Check against stale pixels:** pinned's plain → weighted patch returned
`:ok`, but equal pixels do not prove a new buffer was submitted. So we also
started B at `px(20)` on line two, then changed **only B** to `fill(3)`.
On pinned, B visibly widened from 20 to 100 and `(50,22)` switched from
host to B. On retained, the patch failed with `weighted or compound fill
width is not supported on a WrappedRow child`; the same point still hit the
host. A subsequent valid change to plain `fill()` widened B and the click
reached B at local `(50,6)`. Pinned sends the click name, not local coordinates.

## Original recommendation (not selected)

Keep plain fill behavior. Admit a numeric `fill(n)` width on this edge by
offering each child the WrappedRow's full inner width **before** deciding
where to break the line. Per-line sharing would change even two existing
plain fills from stacked 100px cards to adjacent 48px cards. Use a Row when
that split is wanted. Probe content/pixel bounds separately before admitting
compound widths; this experiment did not test those or child fill heights.

## Selected behavior: elm-ui wrapping widths

Unlike the original recommendation, **form lines before sharing space**.
On a 100×40 host, gap 4, no padding, two 20×12 cards give 48/48 with plain
fills and 24/72 at 1:3. Both leave 28px below. At widths 44 and 43 the
weighted pair gives 20/20, then two 43px lines. These widths were measured
in Chromium with `mdgriffith/elm-ui` 1.1.8. In Emerge keep leading vertical
placement and its existing 4px inter-line gap, not CSS line stretching.

- Scope: plain and numeric fill on direct WrappedRow children. Break on
  outer nonfill widths and automatic fill minima; allocate each completed
  line from zero bases with the existing weighted solver and those floors.
  Keep a lone oversized item on its line. No change to Row or Column splits,
  viewport constraints, compound fill admission, or child height policies.
- Keep minimum and preferred widths distinct (a nested WrappedRow can
  break), in device pixels. Use one retained child order, no new pass or
  temporary vector of lines. Reflow needs the breaks **chosen by Flow**, not
  a second test against rounded final widths.
- The comparison is with elm-ui 1.1.8's [wrappedRow](https://github.com/mdgriffith/elm-ui/blob/1.1.8/src/Element.elm#L617-L640)
  and [row CSS](https://github.com/mdgriffith/elm-ui/blob/1.1.8/src/Internal/Style.elm#L1250-L1282),
  measured with fixed 20×12 content. It does not assert CSS cross-axis or
  general text-layout parity.

## Before-probe evidence

Each revision used its own producer BEAMs and NIF (mapped path, inode and
SHA-256 checked). The detached cold/patch comparison was separate from
whole delivered 100×40 binary frames and ordinary viewport callbacks. On
private Sway, full compositor crops bracketed **7 pinned and 8 retained
seat clicks** on CPU shm. No timing, GPU or physical scanout claim.

Scripts, literal pixel expectations, logs, sparse bytes and crops:
`/workspace/emerge-investigation-artifacts/wrapped-row-width-probe-5GGW1a/`.
`logs/*-window-1` stopped on a probe-side renderer-info field error before
input. Only the corrected runs count. The before probe changed no renderer.

## Results

- **Publication:** the three new [headless tests](../test/emerge_skia/wrapped_row_fill_test.exs)
  failed on the old build. On the new build a plain → 1:3 edit changes
  `(50,6)` from the host gap to B at local `(22,6)` only after the new frame.
  CPU Wayland: nine real seat clicks with a whole crop before and after each,
  including resize-only 100 → 44 → 43 → 44 → 100 and fixed20 + fill → 20/76.
- **Retained geometry:** [nested minimum and auto Column tests](../native/emerge_skia/src/tree/tests/wrapped_width.rs)
  distinguish a minimum from preferred width. Editing nested chips from
  20+20 to 10+30 keeps their single-line demand at 44 but moves a peer to a
  new line in a 50px host. Sparse matches fresh, including scale 1.5 and
  zero width. The original numeric-width negative was replaced with a real
  unsupported compound width; the other rollback and height tests remain.
- **Qualification:** six release Cargo/strict Clippy profiles passed
  (103/106/107/131/131/107). The 4,320 site cases and prior 10,000-seed
  Row/Column/unary campaigns passed; these are not a randomized multiline
  oracle. First, fitting and post-refusal wrapped computations through
  prepared paint/hit inputs: 0 alloc/realloc/free. Focused Mix: 118 passed,
  7 selection exclusions. Full: **705/798 passed, the same 93 failing names
  and 21 exclusions**. No new GPU, timing or scanout claim.
- Evidence: `/workspace/emerge-investigation-artifacts/wrapped-line-fill-zWaJ0D/`.
  `logs/red-{sites,live}`, `logs/first-matched`, `logs/qualification-{native,app}`,
  `logs/final-{window,heap,extended}`, `shots/`, source manifests and named
  failure lists. The two launcher permission errors executed no tests. Final
  mapped NIF: `204b3fc1fc71f48250c357f33373cf786c91ab7b0595ce74cb448f078f5a9c1d`.
