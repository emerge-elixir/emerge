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
             alpha: 0.8,
             move_x: 2
           }
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
end
