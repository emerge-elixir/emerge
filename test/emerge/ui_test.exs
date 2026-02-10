defmodule Emerge.UITest do
  use ExUnit.Case, async: true

  import ExUnit.CaptureIO
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

  test "text_column creates a text_column element with document defaults" do
    element =
      text_column([spacing(12)], [
        paragraph([spacing(4)], [text("First")]),
        paragraph([spacing(4)], [text("Second")])
      ])

    assert element.type == :text_column
    assert element.attrs.spacing == 12
    assert element.attrs.width == :fill
    assert element.attrs.height == :content
    assert length(element.children) == 2
  end

  test "text_column allows overriding default width and height" do
    element =
      text_column([width(px(420)), height(px(300))], [
        paragraph([text("Body")])
      ])

    assert element.type == :text_column
    assert element.attrs.width == {:px, 420}
    assert element.attrs.height == {:px, 300}
  end

  test "text_column supports key attribute" do
    element = text_column([key(:doc), spacing(10)], [paragraph([text("Hello")])])

    assert element.type == :text_column
    assert element.id == :doc
    assert element.attrs.spacing == 10
  end

  test "warns when an attribute key is overridden with a different value" do
    stderr =
      capture_io(:stderr, fn ->
        _ = el([align_left(), center_x()], text("warn"))
      end)

    assert stderr =~ "attribute :align_x is set multiple times"
    assert stderr =~ "last value wins"
  end

  test "does not warn when duplicate attribute uses the same value" do
    stderr =
      capture_io(:stderr, fn ->
        _ = el([align_left(), align_left()], text("same"))
      end)

    assert stderr == ""
  end

  test "warns only once per process for identical override signature" do
    stderr =
      capture_io(:stderr, fn ->
        _ = el([align_left(), center_x()], text("first"))
        _ = el([align_left(), center_x()], text("second"))
      end)

    assert length(Regex.scan(~r/attribute :align_x is set multiple times/, stderr)) == 1
  end

  test "text_column default width and height overrides do not emit warnings" do
    stderr =
      capture_io(:stderr, fn ->
        _ = text_column([width(px(420)), height(px(300))], [paragraph([text("Body")])])
      end)

    assert stderr == ""
  end
end
