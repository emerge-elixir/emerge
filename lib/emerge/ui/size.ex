defmodule Emerge.UI.Size do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(
    ~w(ui-size-fixed ui-size-shrink-fill ui-size-weighted-fill ui-size-min-max)
  )

  @moduledoc """
  Length and sizing helpers for Emerge UI layouts.

  `Emerge.UI.Size` provides the common vocabulary for sizing elements in rows,
  columns, and generic containers.

  Use these helpers through `width/1` and `height/1`.

  ## Length Kinds

  The main length helpers are:

  - `px(n)` for a fixed pixel size
  - `fill()` for taking remaining space
  - `fill(n)` for weighted remaining-space distribution
  - `shrink()` for sizing to content
  - `content()` as an alias for `shrink()`

  `fill()` is most noticeable inside layouts that distribute space between
  siblings, such as `row/2` and `column/2`.

  `fill(2)` does not mean 2 pixels. It means "take 2 shares of the remaining
  space". If a sibling uses `fill(1)`, the `fill(2)` element gets twice as much
  of the leftover room.

  ## Constraints

  `min/2` and `max/2` wrap another length:

  - `min(px(140), shrink())` means "size to content, but never below 140px"
  - `max(px(180), fill())` means "fill remaining space, but never above 180px"

  ## Examples

  A fixed-width panel:

  This is the most direct sizing mode: the panel is always `220px` wide.

  #{Examples.code_block!("ui-size-fixed")}

  #{Examples.image_tag!("ui-size-fixed", "Rendered fixed width size example")}

  Shrink next to fill:

  The first child stays only as wide as its content, while the second child
  expands to take the remaining room in the row.

  #{Examples.code_block!("ui-size-shrink-fill")}

  #{Examples.image_tag!("ui-size-shrink-fill", "Rendered shrink and fill size example")}

  Weighted fill distribution:

  These three children split the leftover width in a 1:2:3 ratio.

  #{Examples.code_block!("ui-size-weighted-fill")}

  #{Examples.image_tag!("ui-size-weighted-fill", "Rendered weighted fill size example")}

  Min and max constraints:

  The first item never becomes narrower than `140px`, and the second fills the
  available space but stops growing once it reaches `180px`.

  #{Examples.code_block!("ui-size-min-max")}

  #{Examples.image_tag!("ui-size-min-max", "Rendered min and max size constraint example")}
  """

  @typedoc "Fixed pixel length, for example `px(220)`."
  @type px_length :: {:px, number()}

  @typedoc "Fill length. Use `fill()` for one share or `fill(n)` for weighted fill."
  @type fill_length :: :fill | {:fill, number()}

  @typedoc "Content-sized length. `content()` and `shrink()` return this value."
  @type content_length :: :content

  @typedoc "Base length accepted by `width/1` and `height/1`."
  @type base_length :: px_length() | fill_length() | content_length()

  @typedoc "Length with a minimum or maximum pixel constraint wrapped around another length."
  @type constrained_length :: {:minimum, number(), length()} | {:maximum, number(), length()}

  @typedoc "Public length type accepted by `width/1` and `height/1`."
  @type length :: base_length() | constrained_length()

  @typedoc "Width attribute built from a `length()`."
  @type width_attr :: {:width, length()}

  @typedoc "Height attribute built from a `length()`."
  @type height_attr :: {:height, length()}

  @typedoc "Size attribute returned by this module."
  @type t :: width_attr() | height_attr()

  @doc """
  Apply a length to the element width.

  `width/1` accepts fixed, fill, shrink/content, and constrained lengths.

  ## Example

  This puts three width strategies next to each other so the difference is easy
  to see: fixed, fill, and shrink-to-content.

  ```elixir
  column([spacing(12)], [
    el([width(px(220))], text("Fixed")),
    el([width(fill())], text("Fill")),
    el([width(shrink())], text("Shrink"))
  ])
  ```
  """
  @spec width(length()) :: width_attr()
  def width({:px, _} = val), do: {:width, val}
  def width(:fill), do: {:width, :fill}
  def width({:fill, _} = val), do: {:width, val}
  def width(:content), do: {:width, :content}
  def width({:minimum, _, _} = val), do: {:width, val}
  def width({:maximum, _, _} = val), do: {:width, val}

  @doc """
  Apply a length to the element height.

  Height commonly uses `px(...)`, `fill()`, or `shrink()` depending on whether
  the element should be fixed, expand, or follow its content.

  ## Example

  The first child stays fixed at `48px` tall, and the second child expands to
  use the rest of the available column height.

  ```elixir
  column([height(fill()), spacing(12)], [
    el([height(px(48))], text("Fixed height")),
    el([height(fill())], text("Fill remaining height"))
  ])
  ```
  """
  @spec height(length()) :: height_attr()
  def height({:px, _} = val), do: {:height, val}
  def height(:fill), do: {:height, :fill}
  def height({:fill, _} = val), do: {:height, val}
  def height(:content), do: {:height, :content}
  def height({:minimum, _, _} = val), do: {:height, val}
  def height({:maximum, _, _} = val), do: {:height, val}

  @doc """
  Create a fixed pixel length.

  Use `px(...)` when you want an exact width or height.

  ## Example

  Use this when a component should stay at an exact visual size.

  ```elixir
  el([width(px(220)), height(px(48))], text("Fixed size"))
  ```
  """
  @spec px(number()) :: px_length()
  def px(n) when is_number(n), do: {:px, n}

  @doc """
  Fill the remaining space on an axis.

  `fill()` means one share of the leftover room. Use `fill(n)` for weighted
  distribution between siblings.

  ## Examples

  In the first example both children split the leftover width evenly. In the
  second, the right child gets twice as much space as the left child.

  ```elixir
  row([width(fill()), spacing(12)], [
    el([width(fill())], text("Left")),
    el([width(fill())], text("Right"))
  ])
  ```

  ```elixir
  row([width(fill()), spacing(8)], [
    el([width(fill(1))], text("1 share")),
    el([width(fill(2))], text("2 shares"))
  ])
  ```
  """
  @spec fill() :: :fill
  def fill, do: :fill

  @spec fill(number()) :: {:fill, number()}
  def fill(weight) when is_number(weight) and weight > 0, do: {:fill, weight}

  def fill(weight) do
    raise ArgumentError, "fill/1 expects a positive number, got: #{inspect(weight)}"
  end

  @doc """
  Size to content.

  This is an alias for `shrink/0`.
  """
  @spec content() :: content_length()
  def content, do: :content

  @doc """
  Shrink to content.

  Use this when the element should be only as large as its child content.

  `shrink/0` and `content/0` return the same value. `shrink/0` is often the
  clearer name in layout code.

  ## Example

  This is useful for labels, pills, and small controls that should hug their
  content instead of stretching.

  ```elixir
  row([width(fill()), spacing(12)], [
    el([width(shrink())], text("Content sized")),
    el([width(fill())], text("Takes the rest"))
  ])
  ```
  """
  @spec shrink() :: content_length()
  def shrink, do: :content

  @doc """
  Wrap a length in a minimum pixel constraint.

  The resolved length must be at least the given `px(...)` value.

  ## Example

  This keeps a content-sized element from collapsing below a readable minimum.

  ```elixir
  el([width(min(px(140), shrink()))], text("At least 140px wide"))
  ```
  """
  @spec min(px_length(), length()) :: constrained_length()
  def min({:px, min_px}, length) when is_number(min_px) and min_px >= 0,
    do: {:minimum, min_px, length}

  def min(length_px, _length) do
    raise ArgumentError,
          "min/2 expects the first argument to be px(n) with a non-negative number, got: #{inspect(length_px)}"
  end

  @doc """
  Wrap a length in a maximum pixel constraint.

  The resolved length must be at most the given `px(...)` value.

  ## Example

  This lets a flexible element participate in fill layout while still capping
  its final size.

  ```elixir
  el([width(max(px(180), fill()))], text("Fill, but cap at 180px"))
  ```
  """
  @spec max(px_length(), length()) :: constrained_length()
  def max({:px, max_px}, length) when is_number(max_px) and max_px >= 0,
    do: {:maximum, max_px, length}

  def max(length_px, _length) do
    raise ArgumentError,
          "max/2 expects the first argument to be px(n) with a non-negative number, got: #{inspect(length_px)}"
  end
end
