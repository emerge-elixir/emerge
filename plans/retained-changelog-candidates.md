# Retained renderer: bug fixes for the changelog

Track user-visible bugs found on `main` during the port. For each one, record
what reproduces it, what `main` does, what should happen, and the regression
that proves the fix. Only copy **verified fixes** to `Unreleased → Fixed` in
[CHANGELOG.md](../CHANGELOG.md). A port that merely matches `main` is not a fix.

## Verified fixes — ready for the changelog

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

### Default-size text scales with its layout

- **Release note:** Fixed text without an explicit font size staying the same
  size when its layout was scaled.
- At `scale(1.5)` with display scale 1, `main` kept implicit text at 16 device
  pixels. Retained renders it at **24**, matching an explicit `Font.size(16)`
  at that site. At scale 1 the output is unchanged.
- [Delivered-frame regression](../test/emerge_skia/layout_scale_test.exs) ·
  [before/after results](scale-results.md#default-font-size).
