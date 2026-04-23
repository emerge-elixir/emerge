defmodule Emerge.UI do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(~w(ui-root-tree ui-root-layouts))

  @moduledoc """
  Declarative UI tree API.

  `Emerge.UI` is the root DSL for building the element tree returned from
  `render/0` or `render/1`.

  These examples assume `use Emerge.UI`.

  ## Tree Model

  UI is expressed as a tree of elements.

  Each element has:

  - a constructor such as `el/2`, `row/2`, `column/2`, or `text/1`
  - attrs that configure layout, styling, behavior, or media
  - zero or more child elements

  `el/2` accepts exactly one child. Layout containers such as `row/2`,
  `wrapped_row/2`, `column/2`, `text_column/2`, and `paragraph/2` accept child
  lists. Leaf elements such as `text/1`, `image/2`, `svg/2`, `video/2`, and
  `none/0` have no children.

  ## Core Constructors

  The root module provides the core element constructors:

  - `el/2` for a single framed or aligned child
  - `row/2` for a horizontal line of children
  - `wrapped_row/2` for horizontal flow that wraps onto new lines
  - `column/2` for vertical stacks
  - `text_column/2` and `paragraph/2` for wrapped text flows
  - `text/1`, `image/2`, `svg/2`, `video/2`, and `none/0` for content leaves

  This small tree uses `column/2` as the root, `el/2` for framed content, and
  `row/2` for a horizontal action area.

  #{Examples.code_block!("ui-root-tree")}

  #{Examples.image_tag!("ui-root-tree", "Rendered basic Emerge.UI tree example")}

  ## Choosing A Layout

  Choose the container that matches the child flow:

  - `el/2` when there is exactly one child
  - `row/2` when children stay on one horizontal line
  - `wrapped_row/2` when horizontal content should wrap onto additional lines
  - `column/2` when children stack vertically
  - `text_column/2` and `paragraph/2` when the content is prose rather than
    generic layout blocks

  This comparison shows the difference between `row/2`, `wrapped_row/2`, and
  `column/2`.

  #{Examples.code_block!("ui-root-layouts")}

  #{Examples.image_tag!("ui-root-layouts", "Rendered Emerge.UI layout comparison")}

  `wrapped_row/2` wraps against the width it resolves from its parent,
  including nested fill-driven layouts. Its height grows to match the tallest
  child on each wrapped line, so multiline or reflowed children push following
  content down instead of clipping. Horizontal child alignment also stays
  line-local after wrapping, which means `center_x/0` and `align_right/0`
  position children within the remaining width of their wrapped line rather
  than across the full container width.

  ## use Emerge.UI

  `use Emerge.UI` imports:

  - `Emerge.UI`
  - `Emerge.UI.Color`
  - `Emerge.UI.Size`
  - `Emerge.UI.Space`
  - `Emerge.UI.Scroll`
  - `Emerge.UI.Align`

  It also aliases the grouped helper modules:

  - `Animation`
  - `Background`
  - `Border`
  - `Event`
  - `Font`
  - `Input`
  - `Interactive`
  - `Nearby`
  - `Svg`
  - `Transform`

  Using `use Emerge` for a viewport also calls `use Emerge.UI`.

  ## Top-Level Attrs

  The root module also defines a small set of attrs that are not grouped into
  submodules:

  - `key/1` for stable sibling identity
  - `focus_on_mount/0` to focus an element on first mount
  - `clip_nearby/0` to clip nearby escapes under the host
  - `image_fit/1` for `image/2` and `video/2`

  ```elixir
  Input.text([key(:search), focus_on_mount()], state.query)

  image([width(px(160)), height(px(96)), image_fit(:cover)], "images/hero.jpg")
  ```

  ## Submodules

  The rest of the API is organized by concern:

  - `Emerge.UI.Color` for named and explicit colors
  - `Emerge.UI.Size` for width, height, and length helpers
  - `Emerge.UI.Space` for padding and spacing
  - `Emerge.UI.Scroll` for scroll-related attrs
  - `Emerge.UI.Align` for alignment helpers
  - `Emerge.UI.Background`, `Emerge.UI.Border`, and `Emerge.UI.Font` for
    decorative styling
  - `Emerge.UI.Input`, `Emerge.UI.Event`, and `Emerge.UI.Interactive` for
    inputs, event handlers, and interaction styling
  - `Emerge.UI.Transform` and `Emerge.UI.Animation` for paint-time movement and
    animation
  - `Emerge.UI.Nearby` for overlays and attached nearby elements
  - `Emerge.UI.Svg` for SVG-specific styling helpers

  As trees grow, extract regular Elixir functions that return `Emerge.UI.t()`.
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

  @typedoc "A globally unique semantic key for identifying a UI element in the tree."
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
  @type clip_nearby_attr :: {:clip_nearby, true}
  @type image_fit_attr :: {:image_fit, image_fit_mode()}

  @doc """
  Import the root element DSL and the most common UI helper modules.

  This imports the core constructors from `Emerge.UI` and the frequently used
  helpers from `Color`, `Size`, `Space`, `Scroll`, and `Align`. It also aliases
  the grouped styling and behavior modules such as `Background`, `Border`,
  `Font`, `Input`, `Event`, and `Nearby`.
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
  Build a single-child container.

  `el/2` is the fundamental framing element. Use it when you need one child and
  want to apply styling, alignment, nearby elements, or sizing around that
  child.

  Font attrs applied to an `el/2` are inherited by text descendants.

  ## Example

  ```elixir
  el(
    [
      padding(12),
      Background.color(color(:slate, 900)),
      Border.rounded(12),
      Font.color(color(:slate, 50))
    ],
    text("Hello")
  )
  ```
  """
  @spec el(attrs(), child()) :: t()
  def el(attrs, child) do
    {attrs, nearby, child} = Builder.prepare_single_child!("el/2", attrs, child)
    Builder.build_element(attrs, nearby, :el, [child])
  end

  @doc """
  Lay out children horizontally in one line.

  Use `row/2` when the children should stay on the same horizontal track. Add
  `spacing/1` or `space_evenly/0` from `Emerge.UI.Space` to control the gaps.

  ## Example

  ```elixir
  row([spacing(20)], [
    el([], text("A")),
    el([], text("B")),
    el([], text("C"))
  ])
  ```
  """
  @spec row(attrs(), children()) :: t()
  def row(attrs, children) do
    {attrs, nearby, children} = Builder.prepare_children!("row/2", attrs, children)
    Builder.build_element(attrs, nearby, :row, children)
  end

  @doc """
  Lay out children horizontally and wrap onto new lines as needed.

  Use `wrapped_row/2` for chips, tags, and other horizontal content that should
  continue onto additional lines when it no longer fits the available width.

  ## Example

  ```elixir
  wrapped_row([spacing_xy(12, 12)], [
    el([], text("One")),
    el([], text("Two")),
    el([], text("Three"))
  ])
  ```
  """
  @spec wrapped_row(attrs(), children()) :: t()
  def wrapped_row(attrs, children) do
    {attrs, nearby, children} = Builder.prepare_children!("wrapped_row/2", attrs, children)
    Builder.build_element(attrs, nearby, :wrapped_row, children)
  end

  @doc """
  Lay out children vertically.

  Use `column/2` for stacks of content where each child sits below the previous
  one.

  ## Example

  ```elixir
  column([spacing(10)], [
    text("Line 1"),
    text("Line 2")
  ])
  ```
  """
  @spec column(attrs(), children()) :: t()
  def column(attrs, children) do
    {attrs, nearby, children} = Builder.prepare_children!("column/2", attrs, children)
    Builder.build_element(attrs, nearby, :column, children)
  end

  @doc """
  A text column lays out paragraph-oriented content vertically.

  It behaves like a column but comes with document-friendly defaults:

  - `width(fill())`
  - `height(content())`

  You can override these by passing explicit width/height attributes.

  ## Example

  ```elixir
  text_column([spacing(12)], [
    paragraph([], [text("First paragraph of copy.")]),
    paragraph([], [text("Second paragraph of copy.")])
  ])
  ```
  """
  @spec text_column(attrs(), children()) :: t()
  def text_column(attrs, children) do
    {attrs, nearby, children} = Builder.prepare_children!("text_column/2", attrs, children)

    attrs
    |> Map.put_new(:width, Size.fill())
    |> Map.put_new(:height, Size.content())
    |> Builder.build_element(nearby, :text_column, children)
  end

  @doc """
  A paragraph lays out inline text children with word wrapping.

  Children should be `text/1` elements or `el/2`-wrapped text elements.
  Words flow left-to-right and wrap at the container width.

  ## Example

  ```elixir
  paragraph([width(px(220))], [
    text("A paragraph wraps "),
    el([Font.semi_bold()], text("inline")),
    text(" text children.")
  ])
  ```
  """
  @spec paragraph(attrs(), children()) :: t()
  def paragraph(attrs, children) do
    {attrs, nearby, children} = Builder.prepare_children!("paragraph/2", attrs, children)
    Builder.build_element(attrs, nearby, :paragraph, children)
  end

  @doc """
  Build a text leaf element.

  It can live on its own as a content leaf, but it does not wrap by default.

  Use `paragraph/2` or `text_column/2` for wrapped text flows.

  ## Example

  ```elixir
  text("Status: ready")
  ```
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

  Use `image_fit/1` to choose between `:contain` and `:cover`.

  ## Example

  ```elixir
  image(
    [width(px(160)), height(px(96)), image_fit(:cover)],
    "images/hero.jpg"
  )
  ```
  """
  @spec image(attrs(), image_source()) :: t()
  def image(attrs, source) do
    {attrs, nearby} = Builder.prepare_attrs!("image/2", attrs)
    source = Validation.validate_image_source!("image/2", source)

    attrs
    |> Map.put(:image_src, source)
    |> Builder.build_element(nearby, :image, [])
  end

  @doc """
  An SVG element.

  Preserves the SVG's original colors by default. Use `Svg.color/1` to apply
  template tinting to all visible pixels.

  ## Example

  ```elixir
  svg([width(px(24)), height(px(24))], "icons/check.svg")
  ```
  """
  @spec svg(attrs(), image_source()) :: t()
  def svg(attrs, source) do
    {attrs, nearby} =
      Builder.prepare_attrs!("svg/2", attrs, extra_public_attr_keys: [:svg_color])

    source = Validation.validate_image_source!("svg/2", source)

    attrs
    |> Map.put(:image_src, source)
    |> Map.put(:svg_expected, true)
    |> Builder.build_element(nearby, :image, [])
  end

  @doc """
  A video element backed by a renderer-owned video target.

  `video/2` behaves like an image element whose pixels are provided by an owned
  target instead of a file source.
  """
  @spec video(attrs(), video_target()) :: t()
  def video(attrs, target) do
    {attrs, nearby} = Builder.prepare_attrs!("video/2", attrs)
    target = Validation.validate_video_target!("video/2", target)

    attrs
    |> Map.put_new(:image_fit, :contain)
    |> Map.put(:video_target, target.id)
    |> Map.put(:image_size, {target.width, target.height})
    |> Builder.build_element(nearby, :video, [])
  end

  @doc """
  Build an empty element that takes up no space.

  Use `none/0` for conditional branches that should render nothing.
  """
  @spec none() :: t()
  def none do
    %Element{type: :none, attrs: %{}, children: [], nearby: []}
  end

  @doc """
  Provide a stable semantic key for identity in the tree.

  Keys must be globally unique across the full UI tree, including nearby mounts.

  Use `key/1` when an element should retain its semantic identity across updates
  or when it may need to be addressed by future semantic APIs such as focus or
  drag-and-drop targeting.
  """
  @spec key(key()) :: key_attr()
  def key(value), do: {:key, value}

  @doc """
  Focus this element once when it is first mounted into the tree.

  The focus request is tied to first mount, not to every rerender.
  """
  @spec focus_on_mount() :: focus_on_mount_attr()
  def focus_on_mount, do: {:focus_on_mount, true}

  @doc """
  Clip nearby escape overlays attached under this host.

  Use this on hosts or scroll containers when nearby content should be clipped
  to the host bounds instead of escaping freely.
  """
  @spec clip_nearby() :: clip_nearby_attr()
  def clip_nearby, do: {:clip_nearby, true}

  @doc """
  Set image fit mode for `image/2` and `video/2`.

  - `:contain` keeps the full source visible inside the element bounds
  - `:cover` fills the bounds and crops if necessary
  """
  @spec image_fit(image_fit_mode()) :: image_fit_attr()
  def image_fit(mode) when mode in [:contain, :cover], do: {:image_fit, mode}
end
