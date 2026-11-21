defmodule Emerge.UI.Align do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(~w(ui-align-el ui-align-row ui-align-column))

  @moduledoc """
  Alignment helpers for positioning inside layout parents.

  ## Basics

  Alignment happens inside the parent's content area. Padding and border are
  applied before alignment, so these helpers work within the remaining space.

  Alignment is visible when the parent and child have different extents on
  that axis. For example, a right-aligned child wider than its El overflows
  to the left.

  Alignment helpers position the element they are attached to inside its
  parent. `el` also aligns its child content, and `paragraph` also aligns its
  wrapped text lines.

  The root has no layout parent. `center_x/0` and `align_bottom/0` on a root
  do **not** center or bottom-align its box in the viewport: it starts at
  `(0, 0)`. A root `el` still uses those helpers to position its child. A root
  `row` with the same helpers leaves its children's alignment unchanged.

  ## `el`

  An `el` positions its child within the padded area. On each axis the
  child's own helper wins, then the El's helper, then left or top. An
  explicit `align_left/0` or `align_top/0` overrides just that axis. These
  defaults apply to the immediate child, not to all of its descendants.

  This example centers a pill inside a larger container:

  #{Examples.code_block!("ui-align-el")}

  #{Examples.image_tag!("ui-align-el", "Rendered el alignment example")}

  Here the 20×20 child starts at `(76, 22)`: its right edge sits four pixels
  inside the El, and it is vertically centered.

  ```elixir
  el([width(px(100)), height(px(64)), padding(4)],
    el([width(px(20)), height(px(20)), align_right(), center_y()], text("A"))
  )
  ```

  ## `paragraph`

  On a paragraph, `center_x/0`, `align_right/0`, and `align_left/0` also align
  each wrapped text line, including the last line. Alignment uses the content
  width remaining after padding, borders, and floating children.

  ```elixir
  paragraph([width(fill()), center_x()], [
    text("Each line of this paragraph is centered.")
  ])
  ```

  An explicit `Font.center/0`, `Font.align_right/0`, or `Font.align_left/0` on
  the paragraph overrides its line alignment without changing its placement.
  For example, `center_x()` with `Font.align_left()` centers the paragraph box
  but leaves its lines left-aligned. Inherited font alignment is used only when
  the paragraph has neither an explicit font alignment nor a horizontal helper.

  ## `row`

  In `row`, children do not inherit alignment from the row. Put alignment
  helpers on the children you want to position inside the row.

  `align_left/0`, `center_x/0`, and `align_right/0` split children into left,
  center, and right groups. The right group ends at the padded right edge. The
  center group is centered **between the two outer groups**, not in the whole
  row. With too little space it starts where the left group ends. Children
  keep their declaration order and spacing within each group. If boxes overlap,
  later siblings paint on top and the later eligible listener receives the click.

  `align_top/0`, `center_y/0`, and `align_bottom/0` position each child within
  the row height.

  If you put an alignment helper on the `row` itself, it positions that `row`
  inside its parent. It is not inherited by the row's children.

  This row shows left, center, and right grouping across the main axis.

  #{Examples.code_block!("ui-align-row")}

  #{Examples.image_tag!("ui-align-row", "Rendered row alignment example")}

  ## `wrapped_row`

  In `wrapped_row`, horizontal alignment is evaluated per wrapped line.

  Each line uses the padded width of the row, but only the children on that
  line form its left, center, and right groups. Alignment does not change line
  breaks or fill widths. A lone 20-pixel card on a 60-pixel line starts at 20
  with `center_x/0` and at 40 with `align_right/0` (no padding).

  `align_top/0`, `center_y/0`, and `align_bottom/0` still position each child
  within the height of its own wrapped line.

  ## `wrapped_column`

  Put `align_left/0`, `center_x/0`, or `align_right/0` on a child to position
  it within **its own column**, not the full container. For example, a
  16-pixel child in a 32-pixel column starts 0, 8 or 16 pixels from that
  column's left edge. `align_bottom/0` anchors the bottom group at the padded
  bottom edge; `center_y/0` centers its group between the top and bottom
  groups **in that column**. Neither helper changes which column a child fits in.

  ## `column`

  In `column`, children do not inherit alignment from the column. Put alignment
  helpers on the children you want to position inside the column.

  `align_top/0`, `center_y/0`, and `align_bottom/0` split children into top,
  center, and bottom groups down the column. The center group uses the space
  between the top and bottom groups, just as in a row.

  `align_left/0`, `center_x/0`, and `align_right/0` position each child within
  the column width.

  If you put an alignment helper on the `column` itself, it positions that
  `column` inside its parent. It is not inherited by the column's children.

  This column shows top, center, and bottom grouping down the main axis.

  #{Examples.code_block!("ui-align-column")}

  #{Examples.image_tag!("ui-align-column", "Rendered column alignment example")}

  In scrollable columns, `align_bottom/0` stays part of the column's content
  flow instead of pinning to the visible bottom edge.
  """

  @type horizontal_alignment :: :left | :center | :right
  @type vertical_alignment :: :top | :center | :bottom
  @type x_attr :: {:align_x, horizontal_alignment()}
  @type y_attr :: {:align_y, vertical_alignment()}
  @type t :: x_attr() | y_attr()

  @doc "Center horizontally within the current layout parent; on a paragraph, also center its text lines."
  @spec center_x() :: x_attr()
  def center_x, do: {:align_x, :center}

  @doc "Center vertically within the current layout parent."
  @spec center_y() :: y_attr()
  def center_y, do: {:align_y, :center}

  @doc "Align to the left within the current layout parent; on a paragraph, also left-align its text lines."
  @spec align_left() :: x_attr()
  def align_left, do: {:align_x, :left}

  @doc "Align to the right within the current layout parent; on a paragraph, also right-align its text lines."
  @spec align_right() :: x_attr()
  def align_right, do: {:align_x, :right}

  @doc "Align to the top within the current layout parent."
  @spec align_top() :: y_attr()
  def align_top, do: {:align_y, :top}

  @doc "Align to the bottom within the current layout parent."
  @spec align_bottom() :: y_attr()
  def align_bottom, do: {:align_y, :bottom}
end
