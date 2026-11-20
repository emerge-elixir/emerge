# Retained renderer: bug fixes for the changelog

Track user-visible bugs found on `main` during the port. For each one, record
what reproduces it, what `main` does, what should happen, and the regression
that proves the fix. Only copy **verified fixes** to `Unreleased → Fixed` in
[CHANGELOG.md](../CHANGELOG.md). A port that merely matches `main` is not a fix.

## Verified fixes — ready for the changelog

### Content floors on the child of an El

- **Release note:** An El child with a content minimum combined with fill
  can now be wider or taller than its parent's inner size.
- **Reproduce:** a 120×80 El, padding 6, no gap, offers 108×68. For 150×90
  demand, pinned `main` (`5c71a593`) returns 108×68 for `max(content(), fill(3))`
  on both axes, but 150×90 for floors using `px(108)` and `px(68)`.
- **Fixed:** both spellings give **150×90**, with 42/22 overflow. The El
  doesn't clip the child: `(123,86)` reaches A at `(117,80)`. Outside its
  enclosing 132×92 Column, `(133,86)` reaches only the outer view.
  [Delivered frames and raw clicks](../test/emerge_skia/el_child_fill_test.exs)
  · [pinned controls and Wayland seat](el-child-fill.md#results).

### El child height after wrapping

- **Release note:** An El child's content-limited fill height now accounts
  for all wrapped lines. A content-sized El grows with it.
- **Reproduce:** narrow an 84×80 El to 83×80, padding 2, no host gap.
  The width-fill WrappedRow has padding 2, gap 4 and two 36×12 chips. At
  79px child width main moves chip two to `y=20`, but keeps the content/fill
  cap at **16px**; plain content reaches **32px**.
- **Fixed:** the child's own height is **16 → 32 → 16**, leaving 60 → 44 → 60
  of the 76px offer unused. At `(10,24)` the submitted click follows El → A
  (local `(8,22)`) → El. A natural wrapper grows **20 → 36**. [Headless](../test/emerge_skia/el_child_fill_test.exs)
  · [CPU Wayland crops and seat input](el-child-fill.md#results). The older
  retained pixel-cap and fixed-overflow controls did not prove either El fix.

### Content floor on the height of a Row child

- **Release note:** A Row child's height now respects a content minimum
  combined with fill, even when the content is taller than the Row's offer.
- **Reproduce:** 40×120 Row, padding 6, horizontal gap 4, two 12px-wide
  children. At 150px demand, pinned `main` (`5c71a593`) gives A **108px**
  for `max(content(), fill(3))`, but **150px** for `max(content(), px(108))`.
  Fixing B at 108 instead of using fill does not change that discrepancy.
- **Fixed:** both floors give **150px**, 42 beyond the inner offer; B still
  takes 108px with no slack. A paints and receives the click at `(10,117)`
  in the bottom padding; at `(10,121)` only the enclosing view does. The
  [headless regressions](../test/emerge_skia/row_cross_height_test.exs) and
  [Wayland crops and seat input](row-cross-axis-height.md#results) check both.

### Row child height cap after wrapping

- **Release note:** A Row child whose height is capped by both content and
  fill now grows to fit a second line when its width makes its children wrap.
- **Reproduce:** 100×80 → 99×80 Row, padding 2, gap 4. A is a width-fill
  WrappedRow with padding 2, gap 4 and two 36×12 chips; B stays 12×76.
  A gets 80 → 79px wide. Main (`5c71a593`) moves chip two to `y=20` but
  keeps `min(content(), fill(3))` at **16px**. Plain content reaches 32px.
- **Fixed:** A grows **16 → 32px**, leaving 60 → 44 unused below it; widening
  restores the first line. Its own pixels and click at `(10,24)` move from
  the Row to A (local `(8,22)`) and back on submitted frames. [Headless](../test/emerge_skia/row_cross_height_test.exs)
  · [main controls and Wayland seat](row-cross-axis-height.md#results).
  Retained `2bf82a9` rejected both fill-bearing Row cases above. Its already
  working [pixel-cap control](#capped-wrapped-rows-use-the-height-of-all-lines)
  was not evidence for the fill-height fix.

### Content floor on the width of a Column child

- **Release note:** A Column child's width now respects a content minimum
  combined with fill, even when its content is wider than the Column's offer.

- In a 120×40 Column with 6px padding and a 4px vertical gap, two 12px-high
  children each have 108px of available width. B uses `fill()` and takes all
  108px. A has 150px of content. Pinned `main` (`5c71a593`) gives A 150px
  with `max(content(), px(108))`, but only 108px with `max(content(), fill(3))`.
- Retained `2c802f6` rejected the fill-bearing policy on this edge. Both
  spellings now give A **150px**, 42px past the inner edge. B still takes
  108px with nothing unused, and the host clip stays at x=120. The [headless
  regression](../test/emerge_skia/column_cross_width_test.exs) and [Wayland
  seat checks](column-cross-axis-width.md#results) see A at `(117,10)` in
  the right padding and only the enclosing view at `(121,10)` outside the
  host. This is not the main-axis redistribution fix below.

### A content-limited fill leaves room unused

- **Release note:** Fixed unused space beside a fill whose size is limited
  by its own content. Other fill children receive the remaining space.

- In a **120px-wide Row**, no padding or gap, A is `min(content(), fill())`
  and B is `fill()`. With 20px of content in A, pinned `main` (`5c71a593`)
  gives A/B **20/60**, leaving 40px unused; the expected split is **20/100**.
  With 90px of content it gives the expected **60/60**, with nothing unused.
- Before the fix, retained (`e411961`) rejected this expression on both
  upload and patch with `unsupported length 4`; only the pixel-cap control
  passed. Now all three [viewport regressions](../test/emerge_skia/mixed_content_fill_test.exs)
  pass: A/B **20/100 → 60/60**, nothing unused, and `x=35` moves from B at
  local `15` to A at local `35` only after the larger frame is submitted.
  [Both axes, reflow, generated layouts and real Wayland seat checks](dimension-regressions.md#results).
  This is a content-derived limit, distinct from the fixed pixel cap below.

### Capped fill leaves room unused

- **Release note:** Row widths and Column heights now redistribute space among
  fill siblings when a child reaches a pixel cap or floor.
- **Reproduce:** a **120px-wide Row**, no padding or gap, A with
  `width(min(px(20), fill()))` and B with `width(fill())`.
- **Observed:** `main` assigns 60px to each child, caps A at 20px, then stops.
  A is 20px, B is 60px, and the last 40px are empty. Measured on `5c71a593`;
  the Row allocation code is unchanged on local `main` at `aef5e42`.
- **Fixed on retained:** A is 20px and B is **100px**. Cold mount and an edit
  from `width(px(20))` agree; `x=90` reaches B at local `x=70`. At a Row width
  of 30px, the cap no longer binds and the labels share 15/15.
- The same bug affects [nested bounds](nested-main-axis-fill.md#results): with
  A bounded to 20–50px, B plain fill, no padding or gap, pinned `main` gives
  **20/15** in a 30px host (5px overflow) and **50/60** in a 120px host
  (10px unused). Retained gives **20/10** and **50/70** on both axes.
- [Delivered-frame and raw-click regressions](../test/emerge_skia/bounded_fill_test.exs) ·
  [allocation and Wayland checks](bounded-main-axis-fill.md#redistribution-correction).

### Wrapped rows preserve the vertical gap between lines

- **Release note:** Fixed `wrapped_row` using the height of a child's content
  instead of its outer box to position the next line. Fixed-height children
  no longer reduce the requested gap or overlap the next line.
- **Example at 1×:** with 32px-tall chips, 5px padding, `none()` inside, and
  an 8px gap, `main` (`5c71a593`) starts the second line at `y=18`. Retained
  starts it at **`y=40`**: 32px for the chip plus the full 8px gap.
- [Wrapping regression](../test/emerge_skia/retained_tree_slice_test.exs) ·
  [scale comparisons](scale-results.md#evidence-and-limits). This is the gap
  **between wrapped lines**. The separate [Column clipping fix](scale-results.md#column-clipping-follow-up)
  restored `main`'s behavior in the horizontal gap between card lanes.

### Capped wrapped rows use the height of all lines

- **Release note:** Fixed a pixel cap on `wrapped_row` content height leaving
  the row at its one-line height after its children wrapped.
- **Reproduce:** an 80×120px Column contains a full-width `wrapped_row` with
  two 48×20px children, followed by a `height(fill())` sibling. No padding;
  both gaps are 4px. Each child's content and outer height are both 20px.
  Both renderers start the second line at `y=24`: this is not the line-gap bug.
- **Observed on `5c71a593`:** `height(min(px(60), content()))` leaves the
  row at 20px. The sibling starts at `y=24` with 96px instead of following
  the second line. With plain `content()`, the same row correctly gets 44px.
- **Fixed on retained:** the 60px cap allows 44px (`20 + 4 + 20`); the sibling
  starts at `y=48` with 72px. A tighter 36px cap gives 36px and 80px with the
  same 4px gap. Nothing is unused. [Delivered frames and clicks](../test/emerge_skia/bounded_content_test.exs)
  · [comparison and controls](bounded-content.md#results).

### Default-size text scales with its layout

- **Release note:** Fixed text without an explicit font size staying the same
  size when its layout was scaled.
- At `scale(1.5)` with display scale 1, `main` kept implicit text at 16 device
  pixels. Retained renders it at **24**, matching an explicit `Font.size(16)`
  at that site. At scale 1 the output is unchanged.
- [Delivered-frame regression](../test/emerge_skia/layout_scale_test.exs) ·
  [before/after results](scale-results.md#default-font-size).
