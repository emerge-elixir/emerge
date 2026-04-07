defmodule Emerge.UI.Scroll do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(~w(ui-scroll-vertical ui-scroll-horizontal ui-scroll-both))

  @moduledoc """
  Overflow helpers for scrollable layouts.

  Use `scrollbar_y/0` and `scrollbar_x/0` on a bounded container when its child
  content may exceed the available space.

  These helpers do not create layout on their own. They simply enable scrolling
  on the container you attach them to.

  ## Behavior

  - `scrollbar_y/0` enables vertical scrolling when content is taller than the container
  - `scrollbar_x/0` enables horizontal scrolling when content is wider than the container
  - you can enable both axes on the same element
  - scrollbars only appear when content actually overflows

  In practice, scrolling is most useful on containers that already have a size
  constraint, such as a fixed `height(px(...))`, a fixed `width(px(...))`, or a
  parent that gives the element bounded `fill()` space.

  Style the container normally with helpers such as `Background.color/1`,
  `Border.rounded/1`, `padding/1`, and `spacing/1`.

  ## Examples

  A vertical scroll panel:

  The panel itself is fixed at `180px` tall, so once the column grows past that
  height the user can scroll through the list.

  #{Examples.code_block!("ui-scroll-vertical")}

  #{Examples.image_tag!("ui-scroll-vertical", "Rendered vertical scroll container")}

  A horizontal chip row:

  This keeps the chip row on one line and lets the user scroll sideways once the
  row becomes wider than the container.

  #{Examples.code_block!("ui-scroll-horizontal")}

  #{Examples.image_tag!("ui-scroll-horizontal", "Rendered horizontal scroll container")}

  A panel that can scroll in both directions:

  This is useful for oversized content such as previews, canvases, or debug
  surfaces that may exceed the container in both axes.

  #{Examples.code_block!("ui-scroll-both")}

  #{Examples.image_tag!("ui-scroll-both", "Rendered two-axis scroll container")}
  """

  @typedoc "Scroll attribute returned by this module."
  @type scrollbar_y_attr :: {:scrollbar_y, true}
  @type scrollbar_x_attr :: {:scrollbar_x, true}
  @type t :: scrollbar_y_attr() | scrollbar_x_attr()

  @doc """
  Enable vertical scrolling when the child content is taller than the container.

  This is commonly used on fixed-height or fill-height panels whose child is a
  long `column/2`.

  ## Example

  Here the panel height is bounded, so the log can grow while remaining usable
  inside a fixed area.

  ```elixir
  el(
    [
      height(px(220)),
      padding(12),
      scrollbar_y(),
      Background.color(color(:slate, 900)),
      Border.rounded(12)
    ],
    column([spacing(8)], [
      text("Log line 1"),
      text("Log line 2"),
      text("Log line 3")
    ])
  )
  ```
  """
  @spec scrollbar_y() :: scrollbar_y_attr()
  def scrollbar_y, do: {:scrollbar_y, true}

  @doc """
  Enable horizontal scrolling when the child content is wider than the container.

  This is commonly used on fixed-height containers whose child is a wide
  `row/2`.

  ## Example

  Here the container is short and the row can keep growing horizontally without
  wrapping, so the user scrolls left and right to see the rest.

  ```elixir
  el(
    [
      height(px(84)),
      padding(10),
      scrollbar_x(),
      Background.color(color(:slate, 100)),
      Border.rounded(10)
    ],
    row([spacing(10)], [
      text("Alpha"),
      text("Beta"),
      text("Gamma"),
      text("Delta")
    ])
  )
  ```
  """
  @spec scrollbar_x() :: scrollbar_x_attr()
  def scrollbar_x, do: {:scrollbar_x, true}
end
