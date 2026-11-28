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

  A menu on a scrolling item moves with the item. As long as any part of the
  item's border box is inside the visible scroll area, the menu can draw and
  receive clicks outside that area. When the item scrolls fully out, the whole
  menu disappears and no longer blocks clicks, even if the menu itself would
  overlap the scroll area. Scrolling the item back into view restores it.

  Mount a panel directly on the scroll container to keep it stationary.
  Use `Emerge.UI.clip_nearby/0` on the item, the container, or another ancestor
  to clip escaping panels to that element's inner edge.

  ## Paint Order

  On one host, later escaping overlays paint above earlier ones where they
  overlap, regardless of slot. `behind_content/1` stays below the host's
  content; multiple behinds also paint in definition order.

  Menus on sibling hosts share a layer: where they overlap, the later sibling's
  menu wins. A tooltip attached to either menu sits above both menus. An
  `in_front/1` overlay on their common ancestor sits above the tooltips. Adding
  an ordinary wrapper around a menu host does not raise or lower its menu.

  ## Pointer Hit Testing

  Front nearby overlays such as `in_front/1` form their own hit-test scope and
  can block the host even when they have no event handlers. Use
  `behind_content/1` for decorative backgrounds, and ordinary noninteractive
  children for markers that must not cover a control with a front nearby scope.
  Keep front nearby content for actual overlays such as menus and popups.

  An event's coordinates belong to its listener element, not the nearby host.
  See `Emerge.UI.Event` for coordinate and captured-drag rules.

  ## Example

  See the dropdown example in
  [Describe your UI](guides/tutorials/describe_ui.md#attach-a-dropdown-with-nearby-elements)
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
