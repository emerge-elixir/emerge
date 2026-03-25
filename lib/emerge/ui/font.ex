defmodule Emerge.UI.Font do
  @moduledoc "Font styling attributes"

  @type family :: atom() | binary()
  @type color_value :: Emerge.UI.Color.color() | Emerge.UI.Color.t()
  @type text_align :: :left | :right | :center

  @type t ::
          {:font_size, number()}
          | {:font_color, color_value()}
          | {:font, family()}
          | {:font_weight, :bold}
          | {:font_style, :italic}
          | {:font_underline, true}
          | {:font_strike, true}
          | {:font_letter_spacing, number()}
          | {:font_word_spacing, number()}
          | {:text_align, text_align()}

  @doc "Set font size"
  @spec size(number()) :: {:font_size, number()}
  def size(s), do: {:font_size, s}

  @doc "Set font color"
  @spec color(color_value()) :: {:font_color, color_value()}
  def color(c), do: {:font_color, c}

  @doc "Set font family"
  @spec family(family()) :: {:font, family()}
  def family(f), do: {:font, f}

  @doc "Bold text"
  @spec bold() :: {:font_weight, :bold}
  def bold, do: {:font_weight, :bold}

  @doc "Italic text"
  @spec italic() :: {:font_style, :italic}
  def italic, do: {:font_style, :italic}

  @doc "Underline text"
  @spec underline() :: {:font_underline, true}
  def underline, do: {:font_underline, true}

  @doc "Strike-through text"
  @spec strike() :: {:font_strike, true}
  def strike, do: {:font_strike, true}

  @doc "Extra spacing between letters"
  @spec letter_spacing(number()) :: {:font_letter_spacing, number()}
  def letter_spacing(value) when is_number(value), do: {:font_letter_spacing, value}

  @doc "Extra spacing between words"
  @spec word_spacing(number()) :: {:font_word_spacing, number()}
  def word_spacing(value) when is_number(value), do: {:font_word_spacing, value}

  @doc "Left-align text within element (default)"
  @spec align_left() :: {:text_align, :left}
  def align_left, do: {:text_align, :left}

  @doc "Right-align text within element"
  @spec align_right() :: {:text_align, :right}
  def align_right, do: {:text_align, :right}

  @doc "Center text within element"
  @spec center() :: {:text_align, :center}
  def center, do: {:text_align, :center}
end
