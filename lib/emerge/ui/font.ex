defmodule Emerge.UI.Font do
  @moduledoc "Font styling attributes"

  @doc "Set font size"
  def size(s), do: {:font_size, s}

  @doc "Set font color"
  def color(c), do: {:font_color, c}

  @doc "Set font family"
  def family(f), do: {:font, f}

  @doc "Bold text"
  def bold, do: {:font_weight, :bold}

  @doc "Italic text"
  def italic, do: {:font_style, :italic}

  @doc "Underline text"
  def underline, do: {:font_underline, true}

  @doc "Strike-through text"
  def strike, do: {:font_strike, true}

  @doc "Extra spacing between letters"
  def letter_spacing(value) when is_number(value), do: {:font_letter_spacing, value}

  @doc "Extra spacing between words"
  def word_spacing(value) when is_number(value), do: {:font_word_spacing, value}

  @doc "Left-align text within element (default)"
  def align_left, do: {:text_align, :left}

  @doc "Right-align text within element"
  def align_right, do: {:text_align, :right}

  @doc "Center text within element"
  def center, do: {:text_align, :center}
end
