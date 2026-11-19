# Retained renderer: bug fixes for the changelog

Track user-visible bugs found on `main` during the port. For each one, record
what reproduces it, what `main` does, what should happen, and the regression
that proves the fix. Only copy **verified fixes** to `Unreleased → Fixed` in
[CHANGELOG.md](../CHANGELOG.md). A port that merely matches `main` is not a fix.

## Verified fixes — ready for the changelog

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
