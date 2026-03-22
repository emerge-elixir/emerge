defmodule Emerge.UI do
  @moduledoc """
  Elm-UI inspired layout primitives for Emerge.

  ## Example

      defmodule MyApp.Components do
        use Emerge.UI

        def hero do
          column([spacing(10), center_x()], [
            el(
              [width(fill()), height(px(100)), padding(20), Background.color(color(:sky, 700))],
              text("Hello World")
            ),
            row([spacing(20), padding(10)], [
              el([width(fill())], text("Left")),
              el([width(fill())], text("Right"))
            ])
          ])
        end
      end
  """

  alias Emerge.Engine.Element
  alias Emerge.UI.Internal.Builder
  alias Emerge.UI.Internal.Validation
  alias Emerge.UI.Size

  @doc """
  Imports the root element DSL and the most common UI helper modules.

  `Emerge.UI` keeps element constructors on the root module while grouping
  attribute helpers into focused submodules.
  """
  defmacro __using__(_opts) do
    quote do
      import Emerge.UI
      import Emerge.UI.Color
      import Emerge.UI.Size
      import Emerge.UI.Space
      import Emerge.UI.Scroll
      import Emerge.UI.Align

      alias Emerge.UI.{
        Animation,
        Background,
        Border,
        Event,
        Font,
        Input,
        Interactive,
        Nearby,
        Svg,
        Transform
      }
    end
  end

  @doc """
  A container element. The fundamental building block.

  Font styles (size, color) are passed down to text children.

  ## Example

      el([padding(10), Font.size(20), Font.color(:white)], text("Hello"))
  """
  def el(attrs, child) do
    {attrs, child} = Builder.prepare_single_child!("el/2", attrs, child)
    Builder.build_element(attrs, :el, [child])
  end

  @doc """
  A row lays out children horizontally.

  ## Example

      row([spacing(20)], [
        el([], text("A")),
        el([], text("B")),
        el([], text("C"))
      ])
  """
  def row(attrs, children) do
    {attrs, children} = Builder.prepare_children!("row/2", attrs, children)
    Builder.build_element(attrs, :row, children)
  end

  @doc """
  A wrapped row lays out children horizontally and wraps onto new lines.

  ## Example

      wrapped_row([spacing(12)], [
        el([], text("One")),
        el([], text("Two")),
        el([], text("Three"))
      ])
  """
  def wrapped_row(attrs, children) do
    {attrs, children} = Builder.prepare_children!("wrapped_row/2", attrs, children)
    Builder.build_element(attrs, :wrapped_row, children)
  end

  @doc """
  A column lays out children vertically.

  ## Example

      column([spacing(10)], [
        text("Line 1"),
        text("Line 2")
      ])
  """
  def column(attrs, children) do
    {attrs, children} = Builder.prepare_children!("column/2", attrs, children)
    Builder.build_element(attrs, :column, children)
  end

  @doc """
  A text column lays out paragraph-oriented content vertically.

  It behaves like a column but comes with document-friendly defaults:

  - `width(fill())`
  - `height(content())`

  You can override these by passing explicit width/height attributes.
  """
  def text_column(attrs, children) do
    {attrs, children} = Builder.prepare_children!("text_column/2", attrs, children)

    attrs
    |> Map.put_new(:width, Size.fill())
    |> Map.put_new(:height, Size.content())
    |> Builder.build_element(:text_column, children)
  end

  @doc """
  A paragraph lays out inline text children with word wrapping.

  Children should be `text/1` elements or `el/2`-wrapped text elements.
  Words flow left-to-right and wrap at the container width.
  """
  def paragraph(attrs, children) do
    {attrs, children} = Builder.prepare_children!("paragraph/2", attrs, children)
    Builder.build_element(attrs, :paragraph, children)
  end

  @doc """
  A text element.

  It can live on its own as a content leaf, but it does not wrap by default.

  Use `paragraph/2` or `text_column/2` for wrapped text flows.
  """
  def text(content) when is_binary(content) do
    Builder.build_element(%{content: content}, :text, [])
  end

  def text(other) do
    raise ArgumentError, "text/1 expects a binary string, got: #{inspect(other)}"
  end

  @doc """
  An image element.

  `source` can be a verified `~m"..."` reference, logical asset path,
  runtime file path, or `{:id, image_id}`.
  """
  def image(attrs, source) do
    attrs = Builder.prepare_attrs!("image/2", attrs)
    source = Validation.validate_image_source!("image/2", source)

    attrs
    |> Map.put(:image_src, source)
    |> Builder.build_element(:image, [])
  end

  @doc """
  An SVG element.

  Preserves the SVG's original colors by default. Use `Svg.color/1` to apply
  template tinting to all visible pixels.
  """
  def svg(attrs, source) do
    attrs = Builder.prepare_attrs!("svg/2", attrs, extra_public_attr_keys: [:svg_color])
    source = Validation.validate_image_source!("svg/2", source)

    attrs
    |> Map.put(:image_src, source)
    |> Map.put(:svg_expected, true)
    |> Builder.build_element(:image, [])
  end

  @doc """
  A video element backed by a renderer-owned video target.
  """
  def video(attrs, target) do
    attrs = Builder.prepare_attrs!("video/2", attrs)
    target = Validation.validate_video_target!("video/2", target)

    attrs
    |> Map.put_new(:image_fit, :contain)
    |> Map.put(:video_target, target.id)
    |> Map.put(:image_size, {target.width, target.height})
    |> Builder.build_element(:video, [])
  end

  @doc """
  An empty element that takes up no space.
  """
  def none do
    %Element{type: :none, attrs: %{}, children: []}
  end

  @doc "Provide a stable key for identity in lists (all siblings must have keys)."
  def key(value), do: {:key, value}

  @doc "Set image fit mode (`:contain` or `:cover`)"
  def image_fit(mode) when mode in [:contain, :cover], do: {:image_fit, mode}
end
