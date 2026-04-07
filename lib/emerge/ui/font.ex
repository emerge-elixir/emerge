defmodule Emerge.UI.Font do
  require Emerge.Docs.Examples

  alias Emerge.Docs.Examples

  Examples.external_resources(~w(ui-font-overview ui-font-alignment))

  @moduledoc """
  Font styling attributes.

  Font attrs control text family, weight, style, decorations, spacing, and text
  alignment. They apply to text-bearing elements and inherit through descendants
  until overridden.

  Use:

  - `size/1`, `color/1`, and `family/1` for the core text appearance
  - `weight/1` or the named weight helpers for full font-weight coverage
  - `italic/0`, `underline/0`, and `strike/0` for emphasis and decoration
  - `letter_spacing/1` and `word_spacing/1` to loosen or tighten text rhythm
  - `align_left/0`, `align_right/0`, and `center/0` to align text inside the
    element content box

  ## Defaults

  Text defaults to:

  - family: `"default"`
  - weight: `400` (`regular/0`)
  - italic: `false`
  - size: `16`
  - color: black
  - alignment: left

  ## Inheritance

  Font attrs inherit through the element tree. For example, setting
  `Font.color/1` or `Font.family/1` on a parent element affects descendant text
  unless a child overrides that specific font attr.

  ## Text Alignment

  `align_left/0`, `align_right/0`, and `center/0` align text inside the
  element's content box after padding and border insets are applied. They do not
  change where the element itself is placed in a `row/2` or `column/2`.

  ## Font Weights

  Use `weight/1` for the full `100..900` range in `100` steps, or use the named
  helpers for common weights. `regular/0` is the canonical `400` weight.
  `normal/0` is an alias for `regular/0`.

  ## Examples

  This example shows inherited family, weight, size, and color in the top card,
  then a lighter, tracked, right-aligned label below it.

  #{Examples.code_block!("ui-font-overview")}

  #{Examples.image_tag!("ui-font-overview", "Rendered font overview example")}

  Text alignment and decoration are easier to understand visually than in a list
  of helper names.

  #{Examples.code_block!("ui-font-alignment")}

  #{Examples.image_tag!("ui-font-alignment", "Rendered font alignment and decoration example")}
  """

  @type family :: atom() | binary()
  @type color_value :: Emerge.UI.Color.color() | Emerge.UI.Color.t()
  @type text_align :: :left | :right | :center
  @type weight_value :: 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900
  @type weight_name ::
          :thin
          | :extra_light
          | :light
          | :regular
          | :medium
          | :semi_bold
          | :bold
          | :extra_bold
          | :black

  @type t ::
          {:font_size, number()}
          | {:font_color, color_value()}
          | {:font, family()}
          | {:font_weight, weight_name()}
          | {:font_style, :italic}
          | {:font_underline, true}
          | {:font_strike, true}
          | {:font_letter_spacing, number()}
          | {:font_word_spacing, number()}
          | {:text_align, text_align()}

  @weight_names %{
    100 => :thin,
    200 => :extra_light,
    300 => :light,
    400 => :regular,
    500 => :medium,
    600 => :semi_bold,
    700 => :bold,
    800 => :extra_bold,
    900 => :black
  }

  @doc """
  Set font size.

  The value is in logical pixels and inherits to descendants.

  ## Example

  ```elixir
  el([Font.size(18)], text("Section heading"))
  ```
  """
  @spec size(number()) :: {:font_size, number()}
  def size(s), do: {:font_size, s}

  @doc """
  Set font color.

  Accepts plain named colors like `:black` and normalized color tuples from
  `Emerge.UI.Color`.

  ## Example

  ```elixir
  el([Font.color(color(:emerald, 600))], text("Saved"))
  ```
  """
  @spec color(color_value()) :: {:font_color, color_value()}
  def color(c), do: {:font_color, c}

  @doc """
  Set font family.

  Accepts either an atom or a binary family name. The family inherits to
  descendant text until overridden.

  ## Example

  ```elixir
  el(
    [Font.family("Inter"), Font.regular(), Font.size(16)],
    text("Body copy")
  )
  ```
  """
  @spec family(family()) :: {:font, family()}
  def family(f), do: {:font, f}

  @doc """
  Set an explicit font weight using the numeric `100..900` scale.

  The value must be one of `100, 200, 300, 400, 500, 600, 700, 800, 900`.
  Returned attrs use the canonical named weight atoms.

  ## Examples

  ```elixir
  column([spacing(8)], [
    el([Font.weight(400)], text("Body")),
    el([Font.weight(600)], text("Section title")),
    el([Font.weight(800)], text("Display accent"))
  ])
  ```
  """
  @spec weight(weight_value()) :: {:font_weight, weight_name()}
  def weight(value) when value in [100, 200, 300, 400, 500, 600, 700, 800, 900] do
    {:font_weight, Map.fetch!(@weight_names, value)}
  end

  def weight(value) do
    raise ArgumentError,
          "Font.weight/1 expects one of 100, 200, 300, 400, 500, 600, 700, 800, 900, got: #{inspect(value)}"
  end

  @doc """
  Set the thinnest named weight (`100`).

  Sugar for `weight(100)`.
  """
  @spec thin() :: {:font_weight, :thin}
  def thin, do: {:font_weight, :thin}

  @doc """
  Set extra light weight (`200`).

  Sugar for `weight(200)`.
  """
  @spec extra_light() :: {:font_weight, :extra_light}
  def extra_light, do: {:font_weight, :extra_light}

  @doc """
  Set light weight (`300`).

  Sugar for `weight(300)`.
  """
  @spec light() :: {:font_weight, :light}
  def light, do: {:font_weight, :light}

  @doc """
  Set regular weight (`400`).

  This is the canonical helper for the default text weight.
  """
  @spec regular() :: {:font_weight, :regular}
  def regular, do: {:font_weight, :regular}

  @doc """
  Alias for `regular/0`.
  """
  @spec normal() :: {:font_weight, :regular}
  def normal, do: regular()

  @doc """
  Set medium weight (`500`).

  Sugar for `weight(500)`.
  """
  @spec medium() :: {:font_weight, :medium}
  def medium, do: {:font_weight, :medium}

  @doc """
  Set semi-bold weight (`600`).

  Sugar for `weight(600)`.
  """
  @spec semi_bold() :: {:font_weight, :semi_bold}
  def semi_bold, do: {:font_weight, :semi_bold}

  @doc """
  Set bold weight (`700`).

  Sugar for `weight(700)`.
  """
  @spec bold() :: {:font_weight, :bold}
  def bold, do: {:font_weight, :bold}

  @doc """
  Set extra-bold weight (`800`).

  Sugar for `weight(800)`.
  """
  @spec extra_bold() :: {:font_weight, :extra_bold}
  def extra_bold, do: {:font_weight, :extra_bold}

  @doc """
  Set black weight (`900`).

  Sugar for `weight(900)`.
  """
  @spec black() :: {:font_weight, :black}
  def black, do: {:font_weight, :black}

  @doc """
  Set italic text style.

  ## Example

  ```elixir
  el([Font.italic()], text("Draft"))
  ```
  """
  @spec italic() :: {:font_style, :italic}
  def italic, do: {:font_style, :italic}

  @doc """
  Underline text.

  ## Example

  ```elixir
  el([Font.underline()], text("Open settings"))
  ```
  """
  @spec underline() :: {:font_underline, true}
  def underline, do: {:font_underline, true}

  @doc """
  Strike through text.

  ## Example

  ```elixir
  el([Font.strike()], text("Deprecated"))
  ```
  """
  @spec strike() :: {:font_strike, true}
  def strike, do: {:font_strike, true}

  @doc """
  Add extra spacing between letters.

  This changes both text measurement and final glyph placement.

  ## Example

  ```elixir
  el([Font.extra_light(), Font.letter_spacing(1.5)], text("TRACKED"))
  ```
  """
  @spec letter_spacing(number()) :: {:font_letter_spacing, number()}
  def letter_spacing(value) when is_number(value), do: {:font_letter_spacing, value}

  @doc """
  Add extra spacing between words.

  This changes both text measurement and final glyph placement.

  ## Example

  ```elixir
  el([Font.word_spacing(3)], text("Status updated today"))
  ```
  """
  @spec word_spacing(number()) :: {:font_word_spacing, number()}
  def word_spacing(value) when is_number(value), do: {:font_word_spacing, value}

  @doc """
  Left-align text within the element content box.

  This is the default text alignment.

  ## Example

  ```elixir
  el([width(px(280)), Font.align_left()], text("Left aligned body copy"))
  ```
  """
  @spec align_left() :: {:text_align, :left}
  def align_left, do: {:text_align, :left}

  @doc """
  Right-align text within the element content box.

  ## Example

  ```elixir
  el([width(px(280)), Font.align_right()], text("12:45 PM"))
  ```
  """
  @spec align_right() :: {:text_align, :right}
  def align_right, do: {:text_align, :right}

  @doc """
  Center text within the element content box.

  ## Example

  ```elixir
  el([width(px(280)), Font.center()], text("Welcome back"))
  ```
  """
  @spec center() :: {:text_align, :center}
  def center, do: {:text_align, :center}
end
