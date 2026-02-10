defmodule Emerge.UITest do
  use ExUnit.Case, async: true

  import Emerge.UI

  alias Emerge.UI.{Background, Font}

  test "mouse_over stores decorative attrs" do
    element =
      el(
        [
          mouse_over([
            Background.color(:red),
            Font.color(:white),
            Font.size(18),
            Font.underline(),
            Font.strike(),
            Font.letter_spacing(1.5),
            Font.word_spacing(3),
            alpha(0.8),
            move_x(2)
          ])
        ],
        text("hi")
      )

    assert element.attrs.mouse_over == %{
             background: :red,
             font_color: :white,
             font_size: 18,
             font_underline: true,
             font_strike: true,
             font_letter_spacing: 1.5,
             font_word_spacing: 3,
             alpha: 0.8,
             move_x: 2
           }
  end

  test "font decoration and spacing helpers return attrs" do
    assert Font.underline() == {:font_underline, true}
    assert Font.strike() == {:font_strike, true}
    assert Font.letter_spacing(2.5) == {:font_letter_spacing, 2.5}
    assert Font.word_spacing(4) == {:font_word_spacing, 4}
  end

  test "mouse_over rejects non-decorative attrs" do
    assert_raise ArgumentError, ~r/mouse_over only supports decorative attributes/, fn ->
      el([mouse_over([width(fill())])], text("bad"))
    end
  end

  test "mouse_over rejects nested mouse_over" do
    assert_raise ArgumentError, ~r/mouse_over does not support nested mouse_over/, fn ->
      el([mouse_over([mouse_over([alpha(0.5)])])], text("bad"))
    end
  end

  test "paragraph creates a paragraph element with children" do
    element =
      paragraph([spacing(4), Font.size(16)], [
        text("Hello "),
        el([Font.bold()], text("world"))
      ])

    assert element.type == :paragraph
    assert element.attrs.spacing == 4
    assert element.attrs.font_size == 16
    assert length(element.children) == 2
  end

  test "paragraph/1 creates paragraph with default attrs" do
    element = paragraph([text("Hello")])

    assert element.type == :paragraph
    assert element.attrs == %{__attrs_hash: Emerge.Tree.attrs_hash(%{})}
    assert length(element.children) == 1
  end

  test "paragraph supports key attribute" do
    element = paragraph([key(:my_para), spacing(8)], [text("Hi")])

    assert element.type == :paragraph
    assert element.id == :my_para
    assert element.attrs.spacing == 8
  end
end
