defmodule Emerge.SwipeMetadataTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

  test "completed swipes deliver local endpoints and elapsed milliseconds through the NIF" do
    for {helper, x, y} <- [
          {:on_swipe_up, 30.0, -10.0},
          {:on_swipe_down, 30.0, 80.0},
          {:on_swipe_left, -10.0, 20.0},
          {:on_swipe_right, 120.0, 20.0}
        ] do
      {_, r} =
        Headless.mount({400, 300}, fn ->
          el([scale(2), width(px(100)), height(px(60)), apply(Event, helper, [:swiped])], none())
        end)

      Headless.pixels()
      :ok = Headless.button(r, :press, 100.0, 80.0)
      :ok = Headless.move(r, x * 2, y * 2)
      :ok = Headless.button(r, :release, x * 2, y * 2)

      assert_receive {:application,
                      {:swiped,
                       %{start: %{x: 50.0, y: 40.0}, end: %{x: ^x, y: ^y}, duration_ms: elapsed}}},
                     1000

      assert is_float(elapsed) and elapsed >= 0.0
      refute_receive {:application, {:swiped, _}}
    end
  end
end
