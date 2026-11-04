defmodule Emerge.SwipeMetadataTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias EmergeSkia.TestHarness

  test "completed swipes deliver local endpoints and elapsed milliseconds through the NIF" do
    for {helper, kind, x, y} <- [
          {:on_swipe_up, :swipe_up, 30.0, -10.0},
          {:on_swipe_down, :swipe_down, 30.0, 80.0},
          {:on_swipe_left, :swipe_left, -10.0, 20.0},
          {:on_swipe_right, :swipe_right, 120.0, 20.0}
        ] do
      tree =
        el([scale(2), width(px(100)), height(px(60)), apply(Event, helper, [:swiped])], none())

      {binary, _diff, assigned} = Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), tree)
      id = <<assigned.id::64>>
      harness = TestHarness.new(400, 300)
      on_exit(fn -> TestHarness.stop(harness) end)
      :ok = TestHarness.set_input_target(harness, self())
      :ok = TestHarness.upload_full_bin(harness, binary)
      :ok = TestHarness.await_render(harness, 1000)
      :ok = TestHarness.cursor_button(harness, :press, 100.0, 80.0)
      :ok = TestHarness.cursor_pos(harness, x * 2, y * 2)
      :ok = TestHarness.cursor_button(harness, :release, x * 2, y * 2)

      assert_receive {:emerge_skia_event,
                      {^id, ^kind,
                       %{start: %{x: 50.0, y: 40.0}, end: %{x: ^x, y: ^y}, duration_ms: elapsed}}},
                     1000

      assert is_float(elapsed) and elapsed >= 0.0
      refute_receive {:emerge_skia_event, {^id, ^kind, _}}
    end
  end
end
