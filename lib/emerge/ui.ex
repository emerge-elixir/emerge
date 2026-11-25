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
  `wrapped_row/2`, `column/2`, `wrapped_column/2`, `text_column/2`, and
  `paragraph/2` accept child lists. Leaf elements such as `text/1`, `image/2`, `svg/2`, `video/2`, and
  `none/0` have no children.

  ## Core Constructors

  The root module provides the core element constructors:

  - `el/2` for a single framed or aligned child
  - `row/2` for a horizontal line of children
  - `wrapped_row/2` for horizontal flow that wraps onto new lines
  - `column/2` for vertical stacks
  - `wrapped_column/2` for vertical flow that wraps into columns to the right
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
  content down instead of clipping. Each line has the padded width of the row.
  Its right-aligned group ends at the right edge, and its centered group uses
  the space left between that line's left and right groups.

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
  - `scale/1` for layout-aware subtree scaling
  - `rotate/1` for layout-aware element rotation
  - `focus_on_mount/0` to focus an element on first mount
  - `clip_nearby/0` to clip nearby escapes under the host
  - `image_fit/1` for `image/2` and `video/2`

  ```elixir
  Input.text([key(:search), focus_on_mount()], state.query)

  image([width(px(160)), height(px(96)), image_fit(:cover)], "images/hero.jpg")
  ```

  ## Submodules

  The rest of the API is organized by concern:

  - `Emerge.UI.Color` for named colors, explicit colors, and background gradients
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

  @typedoc "A logical asset name, verified `~m` reference, or runtime file path."
  @type file_source :: binary() | atom() | {:path, binary()} | Emerge.Assets.Ref.t()

  @typedoc "A file source or explicitly supplied encoded bytes for `image/2` and `svg/2`."
  @type image_source :: file_source() | {:bytes, binary()}

  @typedoc "A viewport-local atom target accepted by `video/2`."
  @type video_target :: atom()

  @type key_attr :: {:key, key()}
  @type layout_scale_attr :: {:layout_scale, number()}
  @type layout_rotate_attr :: {:layout_rotate, number()}
  @type focus_on_mount_attr :: {:focus_on_mount, true}
  @type clip_nearby_attr :: {:clip_nearby, true}
  @type image_fit_attr :: {:image_fit, image_fit_mode()}

  @doc """
  Import the root element DSL and the most common UI helper modules.

  This imports the core constructors from `Emerge.UI` and the frequently used
  helpers from `Color`, `Size`, `Space`, `Scroll`, and `Align`. It also aliases
  the grouped styling and behavior modules such as `Background`, `Border`,
  `Font`, `Input`, `Slider`, `Event`, and `Nearby`.
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
        Input.Slider,
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
  Lay out children from top to bottom, starting a column to the right when the
  next child no longer fits. `spacing_xy(x, y)` sets the gap between columns
  and between items in a column, respectively. With no height limit the
  children form one column. Height fills share space within each column.
  `width(content())` fits the occupied columns and the gaps between them;
  `center_x/0` on a child centers it across its own column. `center_y/0` and
  `align_bottom/0` place groups down each column after its members are known.

  If another element needs that packed width to decide its own width, the
  renderer must be able to determine the column breaks before width reflow.
  For example, a fixed-height container with pixel-height cards works. A
  content-width column whose remaining height depends on a sibling wrapping
  at that same width can be refused. Give the container a pixel width to
  remove that dependency.

      wrapped_column([width(px(120)), height(px(80)), spacing_xy(8, 4)], cards)
  """
  @spec wrapped_column(attrs(), children()) :: t()
  def wrapped_column(attrs, children) do
    {attrs, nearby, children} = Builder.prepare_children!("wrapped_column/2", attrs, children)
    Builder.build_element(attrs, nearby, :wrapped_column, children)
  end

  @doc """
  A text column lays out paragraph-oriented content vertically.

  Its default size is `width(fill())` and `height(content())`; explicit widths
  and heights override these defaults. Blocks follow each other with the
  authored spacing. Unlike `column/2`, a text column does not divide the
  remaining height into fill shares or distribute extra space between blocks.

  Put `align_left()` or `align_right()` on a direct child to float it. Paragraph
  lines use the remaining width beside the float, even in the next paragraph.
  A non-floating, non-paragraph child clears the floats above it. Use
  `center_x()` to center a block without floating it.

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
  Words flow left-to-right and wrap at the container width. `center_x/0`,
  `align_right/0`, and `align_left/0` on the paragraph also align each wrapped
  line within its content area, including the last line. Padding, borders, and
  floating children reduce the available line width.

  Explicit `Font.center/0`, `Font.align_right/0`, or `Font.align_left/0` on the
  paragraph takes precedence over its horizontal alignment helper. Without either,
  inherited font text alignment applies; otherwise lines are left-aligned.
  The paragraph's width and placement in its parent still follow normal layout.

  Inline `el` wrappers support `Background.color/1`, every `Border` variant,
  outer/inner shadows and glow. No implicit background is painted. A wrapped
  phrase gets one complete decoration box per line, including
  spaces between its words, rather than a border around each word. Padding and
  border widths participate in wrapping; shadows/glow do not affect layout.
  See `Emerge.UI.Border` for continuation and gradient behavior.

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

  Give `image/2` a PNG, JPEG, uncompressed 24-bit BMP, GIF (first frame), or
  self-contained SVG. Use `path/1` for a file or `binary/1` for encoded bytes. A relative path, bare string or verified
  `~m"..."` names a file under the configured application's `priv/`. Absolute
  paths require an enabled `Emerge.Config.RuntimePaths` allowlist.

  JPEG sizing and fit use the EXIF display orientation. SVGs draw in their
  original colors; use `svg/2` if you want a template tint.

  When one dimension is pixel-sized and the other is omitted or `content()`,
  the content-sized dimension follows the source's intrinsic aspect ratio.
  With `width(fill())`, an omitted or `content()` height follows the resolved
  width. Conversely, `height(fill())` derives an automatic width at the viewport
  root or in a directly enclosing `el/2` or `row/2` with a pixel height. Sizes
  include padding and borders; the ratio applies to the inner content.

  Use `image_fit/1` to choose between `:contain` and `:cover`. Setting both
  dimensions fixes the frame; the fit mode controls how the image fills it.
  `Transform.rotate/1` and `Transform.scale/1` turn and scale the artwork and
  its click target around the frame center, without changing sibling layout.

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

  @doc "Declares a logical `priv/` file, or an absolute file subject to the runtime path policy. Does not read the file."
  @spec path(String.t()) :: String.t() | {:path, String.t()}
  def path(name) when is_binary(name) and byte_size(name) in 1..4096 do
    if not String.valid?(name) or String.contains?(name, <<0>>),
      do: raise(ArgumentError, "path/1 expects a UTF-8 filename")

    if Path.type(name) == :absolute, do: {:path, name}, else: name
  end

  def path(_), do: raise(ArgumentError, "path/1 expects a filename of 1 to 4096 bytes")

  @doc "Declares encoded image bytes (see `image/2`) or UTF-8 SVG XML, never a filename or raw pixel plane."
  @spec binary(binary()) :: {:bytes, binary()}
  def binary(bytes) when is_binary(bytes) and byte_size(bytes) in 1..67_108_864,
    do: {:bytes, bytes}

  def binary(_), do: raise(ArgumentError, "binary/1 expects 1 byte to 64 MiB of encoded media")

  @doc """
  An SVG element.

  Pass a path or `binary/1` containing up to 4 MiB of self-contained UTF-8
  SVG XML. SVGs preserve their original colors; `Svg.color/1` replaces the
  color on the final source alpha. A renderer records the vector commands once
  while the source is resident and replays them at each drawn size. See
  [Use assets](use_assets.html) for supported resources and cache limits.

  Like `image/2`, a pixel-sized dimension determines an omitted or `content()`
  dimension from the source's intrinsic aspect ratio. For example, a 200×200 SVG
  with only `height(px(68))` occupies 68×68 pixels without padding or borders.
  `width(fill())` derives an automatic height from the allocated width, including
  enlargement beyond the source dimensions. `height(fill())` derives an
  automatic width at the same definite-height sites as `image/2`. Setting
  both dimensions fixes the frame, with `image_fit/1` controlling the fit.

  ## Example

  ```elixir
  svg([height(px(24))], "icons/check.svg")
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
  A video element addressed by a viewport-local atom target.

  Submit frames with `Emerge.submit_video_frame/3`. The element has no intrinsic
  size; normal layout attrs determine its bounds and the latest frame is fitted
  within them.
  """
  @spec video(attrs(), video_target()) :: t()
  def video(attrs, target) do
    {attrs, nearby} = Builder.prepare_attrs!("video/2", attrs)
    target = Validation.validate_video_target!("video/2", target)

    attrs
    |> Map.put_new(:image_fit, :contain)
    |> Map.put(:video_target, Atom.to_string(target))
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

  Keys must be unique across the viewport's entire declaration, including
  nearby mounts. A keyed element of the same kind keeps its identity when it
  changes parent or becomes the root. Once absent from an accepted declaration,
  that occurrence ends; reusing its key later starts a new occurrence.
  """
  @spec key(key()) :: key_attr()
  def key(value), do: {:key, value}

  @doc """
  Scale this element and its subtree during layout.

  The multiplier is positive and composes with parent and display scale.
  It changes layout, not the viewport size: scaling a `20px` child by `1.5`
  gives it `30px` at display scale `1`, leaving less room for a fill sibling.
  Padding, spacing and explicit font sizes scale where they are declared.
  Descendants inherit the resolved font size unless they declare their own.
  Text with no size declaration uses 16 logical pixels at the text's scale.

  Use `Transform.scale/1` when the element should only be painted larger or
  smaller without affecting layout.
  """
  @spec scale(number()) :: layout_scale_attr()
  def scale(value) when is_number(value) and value > 0, do: {:layout_scale, value}

  @doc """
  Rotate this element during layout.

  `width/1` and `height/1` still describe the authored unrotated box. The parent
  reserves the rotated axis-aligned bounds, while rendering and hit testing use
  the rotated shape.

  Use `Transform.rotate/1` when rotation should be paint-only and should not
  affect sibling placement or scroll extents.
  """
  @spec rotate(number()) :: layout_rotate_attr()
  def rotate(value) when is_number(value), do: {:layout_rotate, value}

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
