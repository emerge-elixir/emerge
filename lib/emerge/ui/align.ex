defmodule Emerge.UI.Align do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(~w(ui-align-el ui-align-row ui-align-column))

  @moduledoc """
  Alignment helpers for positioning inside layout parents.

  ## Basics

  Alignment happens inside the parent's content area. Padding and border are
  applied before alignment, so these helpers work within the remaining space.

  Alignment is most visible when the parent has extra space on that axis.

  Alignment helpers usually position the element they are attached to inside its
  parent. `el` is the main exception: child content inherits alignment from the
  `el`.

  ## `el`

  In `el`, child content inherits alignment from the container.

  Putting `center_x/0`, `align_right/0`, `center_y/0`, or `align_bottom/0` on
  the `el` controls how its child content is positioned inside that `el`.

  This example centers a pill inside a larger container:

  #{Examples.code_block!("ui-align-el")}

  #{Examples.image_tag!("ui-align-el", "Rendered el alignment example")}

  A child can override inherited alignment with its own alignment helper:

  ```elixir
  el(
    [width(px(200)), height(px(100)), center_x()],
    el([align_right()], text("Child"))
  )
  ```

  ## `row`

  In `row`, children do not inherit alignment from the row. Put alignment
  helpers on the children you want to position inside the row.

  `align_left/0`, `center_x/0`, and `align_right/0` split children into left,
  center, and right groups across the row.

  `align_top/0`, `center_y/0`, and `align_bottom/0` position each child within
  the row height.

  If you put an alignment helper on the `row` itself, it positions that `row`
  inside its parent. It is not inherited by the row's children.

  This row shows left, center, and right grouping across the main axis.

  #{Examples.code_block!("ui-align-row")}

  #{Examples.image_tag!("ui-align-row", "Rendered row alignment example")}

  ## `column`

  In `column`, children do not inherit alignment from the column. Put alignment
  helpers on the children you want to position inside the column.

  `align_top/0`, `center_y/0`, and `align_bottom/0` split children into top,
  center, and bottom groups down the column.

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

  @doc "Center horizontally within the current layout parent."
  @spec center_x() :: x_attr()
  def center_x, do: {:align_x, :center}

  @doc "Center vertically within the current layout parent."
  @spec center_y() :: y_attr()
  def center_y, do: {:align_y, :center}

  @doc "Align to the left within the current layout parent."
  @spec align_left() :: x_attr()
  def align_left, do: {:align_x, :left}

  @doc "Align to the right within the current layout parent."
  @spec align_right() :: x_attr()
  def align_right, do: {:align_x, :right}

  @doc "Align to the top within the current layout parent."
  @spec align_top() :: y_attr()
  def align_top, do: {:align_y, :top}

  @doc "Align to the bottom within the current layout parent."
  @spec align_bottom() :: y_attr()
  def align_bottom, do: {:align_y, :bottom}
end
