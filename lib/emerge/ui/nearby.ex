defmodule Emerge.UI.Nearby do
  @moduledoc """
  Nearby positioning helpers.

  For creating dropdowns, tooltips, confirmation dialogs, modals, badges, and
  other attached UI that should stay anchored to an element.

  Use:

  - `above/1`, `below/1`, `on_left/1`, and `on_right/1` for nearby content that
    escapes the normal layout while staying anchored to a host element
  - `in_front/1` for overlays painted over the host slot
  - `behind_content/1` for placeholders, highlights, and decorative layers
    behind the host content

  ## Escaping The Layout

  `Nearby.above/1`, `Nearby.below/1`, `Nearby.on_left/1`, `Nearby.on_right/1`,
  and `Nearby.in_front/1` all let content escape the normal layout while staying
  anchored to a host element.

  Rows and columns still place the host normally. Nearby content does not take
  up layout space in that row or column. That makes nearby useful for things
  like dropdown menus and tooltips that should stay attached to a control
  without pushing later siblings around.

  ## Complex background elements 

  `Nearby.behind_content/1` is a little different: it lives between background
  and the element content. Use it to create placeholders, highlights, or any UI
  tree as decorative layers behind the host content.

  ## Alignment

  Nearby alignment comes from the nearby root element itself.

  - `above/1` and `below/1` use horizontal alignment from the nearby root,
    for example `align_left()`, `center_x()`, or `align_right()`
  - `on_left/1` and `on_right/1` use vertical alignment from the nearby root,
    for example `align_top()`, `center_y()`, or `align_bottom()`
  - `in_front/1` and `behind_content/1` use both axes

  ## Clipping

  Nearby escape content is not clipped by ancestor hosts by default. Add
  `Emerge.UI.clip_nearby/0` on a host or scroll container when you want that
  host to clip nearby escapes too.

  ## Paint Order

  Same-host nearby content follows definition order, so later nearby attrs on
  the same host paint above earlier nearby attrs when they overlap.

  Between different hosts, nearby follows the same paint order used by normal
  content. Nearby on later sibling branches paints above nearby on earlier
  sibling branches, and nearby attached to an ancestor host paints above nearby
  attached deeper in that host subtree.

  ## Example

  See the dropdown example in
  [`guides/tutorials/describe_ui.md`](guides/tutorials/describe_ui.md#escaping-the-layout-with-nearby-element)
  for a fuller nearby composition walkthrough.
  """

  @type overlay_attr ::
          {:above, Emerge.UI.element()}
          | {:below, Emerge.UI.element()}
          | {:on_left, Emerge.UI.element()}
          | {:on_right, Emerge.UI.element()}
          | {:in_front, Emerge.UI.element()}
          | {:behind, Emerge.UI.element()}

  @type t :: overlay_attr()

  @doc """
  Place an element above the current one without affecting layout flow.

  Horizontal alignment comes from the nearby root, so `align_left()`,
  `center_x()`, and `align_right()` control how the nearby element lines up with
  the host.
  """
  @spec above(Emerge.UI.element()) :: {:above, Emerge.UI.element()}
  def above(element), do: {:above, element}

  @doc """
  Place an element below the current one without affecting layout flow.

  Horizontal alignment comes from the nearby root, so `align_left()`,
  `center_x()`, and `align_right()` control how the nearby element lines up with
  the host.
  """
  @spec below(Emerge.UI.element()) :: {:below, Emerge.UI.element()}
  def below(element), do: {:below, element}

  @doc """
  Place an element on the left of the current one without affecting layout flow.

  Vertical alignment comes from the nearby root, so `align_top()`, `center_y()`,
  and `align_bottom()` control how the nearby element lines up with the host.
  """
  @spec on_left(Emerge.UI.element()) :: {:on_left, Emerge.UI.element()}
  def on_left(element), do: {:on_left, element}

  @doc """
  Place an element on the right of the current one without affecting layout flow.

  Vertical alignment comes from the nearby root, so `align_top()`, `center_y()`,
  and `align_bottom()` control how the nearby element lines up with the host.
  """
  @spec on_right(Emerge.UI.element()) :: {:on_right, Emerge.UI.element()}
  def on_right(element), do: {:on_right, element}

  @doc """
  Render an element in front of the current one.

  `in_front/1` paints over the host slot. `width(fill())` and `height(fill())`
  fill the host slot, while explicit sizes can overflow it.
  """
  @spec in_front(Emerge.UI.element()) :: {:in_front, Emerge.UI.element()}
  def in_front(element), do: {:in_front, element}

  @doc """
  Render an element behind the current one.

  `behind_content/1` paints between the host background and the host content. It
  is useful for placeholders, highlights, and decorative backing layers.
  """
  @spec behind_content(Emerge.UI.element()) :: {:behind, Emerge.UI.element()}
  def behind_content(element), do: {:behind, element}
end
