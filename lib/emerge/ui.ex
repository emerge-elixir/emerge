defmodule Emerge.UI do
  @moduledoc """
  Helpers for declaring UI trees.

  The root `Emerge.UI` module contains the core element constructors:

  - `el/2`
  - `row/2`
  - `wrapped_row/2`
  - `column/2`
  - `text_column/2`
  - `paragraph/2`
  - `text/1`
  - `image/2`
  - `svg/2`
  - `video/2`
  - `none/0`
  - `key/1`
  - `image_fit/1`

  UI is expressed as a tree of elements.

  Each element contains:

  - a type such as `:el`, `:row`, `:column`, or `:text`
  - attributes stored in a map
  - children, which are themselves elements

  Container constructors such as `row/2` and `column/2` build parent elements
  with child elements underneath them. Leaf elements such as `text/1` have no
  children.

  `use Emerge.UI` brings the most common helpers into scope:

  - imports `Emerge.UI`
  - imports `Emerge.UI.Color`
  - imports `Emerge.UI.Size`
  - imports `Emerge.UI.Space`
  - imports `Emerge.UI.Scroll`
  - imports `Emerge.UI.Align`

  It also aliases the grouped helper modules:

  - `Background`
  - `Border`
  - `Font`
  - `Input`
  - `Svg`
  - `Event`
  - `Interactive`
  - `Transform`
  - `Animation`
  - `Nearby`

  The rest of the API is organized by concern:

  - `Emerge.UI.Color` for named and explicit colors
  - `Emerge.UI.Size` for width, height, and length helpers
  - `Emerge.UI.Space` for padding and spacing
  - `Emerge.UI.Scroll` for scroll-related attributes
  - `Emerge.UI.Align` for alignment helpers
  - `Emerge.UI.Event` for event handlers
  - `Emerge.UI.Interactive` for interaction-driven background, border, font, SVG, and transform styling
  - `Emerge.UI.Transform` for movement, rotation, scale, and alpha
  - `Emerge.UI.Animation` for declarative animations
  - `Emerge.UI.Nearby` for overlays and nearby positioning
  - `Emerge.UI.Background`, `Emerge.UI.Border`, `Emerge.UI.Font`, `Emerge.UI.Input`, and `Emerge.UI.Svg` for styling helpers and element helpers

  As rendering grows, it is natural to extract parts into smaller regular Elixir
  functions. Those functions only need to return `Emerge.tree()`.
  """

  alias Emerge.Engine.Element
  alias Emerge.UI.Internal.Builder
  alias Emerge.UI.Internal.Validation
  alias Emerge.UI.Size

  @typedoc "An Emerge UI element."
  @type element :: Element.t()

  @typedoc "A tree of UI returned by DSL helpers."
  @type t :: element()

  @typedoc "A single public UI attribute tuple."
  @type attr :: {atom(), term()}

  @typedoc "A list of public UI attribute tuples."
  @type attrs :: [attr()]

  @typedoc "A single child element."
  @type child :: element()

  @typedoc "A list of child elements."
  @type children :: [child()]

  @typedoc "A stable key used to retain identity among siblings."
  @type key :: term()

  @typedoc "Image fit modes accepted by `image_fit/1`."
  @type image_fit_mode :: :contain | :cover

  @typedoc "An image source accepted by `image/2` and `svg/2`."
  @type image_source ::
          binary() | atom() | {:id, binary()} | {:path, binary()} | Emerge.Assets.Ref.t()

  @typedoc "A video target accepted by `video/2`."
  @type video_target :: EmergeSkia.VideoTarget.t()

  @type key_attr :: {:key, key()}
  @type focus_on_mount_attr :: {:focus_on_mount, true}
  @type image_fit_attr :: {:image_fit, image_fit_mode()}

  @doc """
  Imports the root element DSL and the most common UI helper modules.
  """
  @spec __using__(term()) :: Macro.t()
  defmacro __using__(_opts) do
    quote do
      import Kernel, except: [min: 2, max: 2]

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
  @spec el(attrs(), child()) :: t()
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
  @spec row(attrs(), children()) :: t()
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
  @spec wrapped_row(attrs(), children()) :: t()
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
  @spec column(attrs(), children()) :: t()
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
  @spec text_column(attrs(), children()) :: t()
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
  @spec paragraph(attrs(), children()) :: t()
  def paragraph(attrs, children) do
    {attrs, children} = Builder.prepare_children!("paragraph/2", attrs, children)
    Builder.build_element(attrs, :paragraph, children)
  end

  @doc """
  A text element.

  It can live on its own as a content leaf, but it does not wrap by default.

  Use `paragraph/2` or `text_column/2` for wrapped text flows.
  """
  @spec text(String.t()) :: t()
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
  @spec image(attrs(), image_source()) :: t()
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
  @spec svg(attrs(), image_source()) :: t()
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
  @spec video(attrs(), video_target()) :: t()
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
  @spec none() :: t()
  def none do
    %Element{type: :none, attrs: %{}, children: []}
  end

  @doc "Provide a stable key for identity in lists (all siblings must have keys)."
  @spec key(key()) :: key_attr()
  def key(value), do: {:key, value}

  @doc "Focus this element once when it is first mounted into the tree"
  @spec focus_on_mount() :: focus_on_mount_attr()
  def focus_on_mount, do: {:focus_on_mount, true}

  @doc "Set image fit mode (`:contain` or `:cover`)"
  @spec image_fit(image_fit_mode()) :: image_fit_attr()
  def image_fit(mode) when mode in [:contain, :cover], do: {:image_fit, mode}
end
