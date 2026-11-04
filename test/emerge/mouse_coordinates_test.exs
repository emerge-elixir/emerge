defmodule Emerge.MouseCoordinatesTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias EmergeSkia.TestHarness

  test "mouse events use their listener's logical border box through scrolling and transforms" do
    tree =
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

    {binary, _state, assigned} = Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), tree)
    id = <<List.last(assigned.children).id::64>>
    parent_id = <<assigned.id::64>>
    harness = TestHarness.new(400, 200)
    on_exit(fn -> TestHarness.stop(harness) end)
    :ok = TestHarness.set_input_target(harness, self())
    :ok = TestHarness.upload_full_bin(harness, binary)
    :ok = TestHarness.await_render(harness, 1000)

    # At display size 400x200, scrolling 80 surface pixels puts the child's
    # 200x120 border box at (0, 80). Paint scales 2x and rotates 90° about (100, 140).
    :ok = TestHarness.cursor_pos(harness, 139.0, 82.0)
    assert_position(id, :mouse_move, 35.5, 20.25)
    assert_position(id, :mouse_enter, 35.5, 20.25)
    assert_position(parent_id, :mouse_enter, 69.5, 41.0)
    :ok = TestHarness.cursor_button(harness, :press, 139.0, 82.0)
    assert_position(id, :mouse_down, 35.5, 20.25)
    :ok = TestHarness.cursor_button(harness, :release, 135.0, 86.0)
    assert_position(id, :mouse_up, 36.5, 21.25)
    assert_position(id, :click, 36.5, 21.25)
    assert_receive {:emerge_skia_event, {^id, :press}}, 1000

    :ok = TestHarness.cursor_pos(harness, 300.0, 60.0)
    assert_position(id, :mouse_leave, 30.0, -20.0)
    refute_receive {:emerge_skia_event, {^id, :mouse_move, _}}
  end

  defp assert_position(id, kind, x, y) do
    assert_receive {:emerge_skia_event, {^id, ^kind, %{x: actual_x, y: actual_y}}}, 1000
    assert is_float(actual_x) and is_float(actual_y)
    assert_in_delta actual_x, x, 0.0001
    assert_in_delta actual_y, y, 0.0001
  end
end
