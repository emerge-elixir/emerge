defmodule Emerge.MouseCoordinatesTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

  test "mouse events use their listener's logical border box through scrolling and transforms" do
    {_, renderer} =
      Headless.mount({400, 200}, fn ->
        column(
          [
            scale(2),
            width(px(200)),
            height(px(100)),
            scrollbar_y(),
            scroll_y(1.0),
            Event.on_mouse_enter(:parent_enter)
          ],
          [
            el([height(px(80))], none()),
            el(
              [
                width(px(100)),
                height(px(60)),
                padding(8),
                Border.width(4),
                Transform.scale(2),
                Transform.rotate(90),
                Event.on_mouse_down(:down),
                Event.on_mouse_up(:up),
                Event.on_mouse_move(:move),
                Event.on_mouse_enter(:enter),
                Event.on_mouse_leave(:leave),
                Event.on_click(:click),
                Event.on_press(:press)
              ],
              none()
            )
          ]
        )
      end)

    Headless.pixels()

    # At display size 400x200, scrolling 80 surface pixels puts the child's
    # 200x120 border box at (0, 80). Paint scales 2x and rotates 90° about (100, 140).
    :ok = Headless.move(renderer, 139.0, 82.0)
    assert_position(:move, 35.5, 20.25)
    assert_position(:enter, 35.5, 20.25)
    assert_position(:parent_enter, 69.5, 41.0)
    :ok = Headless.button(renderer, :press, 139.0, 82.0)
    assert_position(:down, 35.5, 20.25)
    :ok = Headless.move(renderer, 135.0, 86.0)
    assert_position(:move, 36.5, 21.25)
    :ok = Headless.button(renderer, :release, 135.0, 86.0)
    assert_position(:up, 36.5, 21.25)
    assert_position(:click, 36.5, 21.25)
    assert_receive {:application, :press}

    :ok = Headless.move(renderer, 300.0, 60.0)
    assert_position(:leave, 30.0, -20.0)
    refute_receive {:application, {:move, _}}
  end

  defp assert_position(kind, x, y) do
    assert_receive {:application, {^kind, %{x: actual_x, y: actual_y}}}, 1000
    assert is_float(actual_x) and is_float(actual_y)
    assert_in_delta actual_x, x, 0.0001
    assert_in_delta actual_y, y, 0.0001
  end
end
