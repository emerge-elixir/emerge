defmodule Emerge.PointerDragTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Engine.{AttrCodec, EventRegistry}
  alias EmergeSkia.TestHarness

  test "drag helper registers a distinct payload route and presence flag" do
    assert {:on_drag, {pid, :picked}} = Event.on_drag(:picked)
    assert pid == self()
    attrs = Map.new([Event.on_drag({self(), {:picked, 42}})])
    assert EventRegistry.node_events(attrs) == %{drag: {self(), {:picked, 42}}}
    assert AttrCodec.decode_attrs(AttrCodec.encode_attrs(attrs)) == %{on_drag: true}
  end

  test "captured dragging cannot compete with activation and swipe on the same element" do
    for attr <- [Event.on_click(:click), Event.on_press(:press), Event.on_swipe_left(:swipe)] do
      assert_raise ArgumentError, ~r/does not allow :on_drag/, fn ->
        el([Event.on_drag(:drag), attr], none())
      end
    end
  end

  test "built-in inputs keep their native drag behavior" do
    for {helper, value} <- [text: "", multiline: "", slider: 0.5] do
      assert_raise ArgumentError, ~r/owns its native drag behavior/, fn ->
        apply(Input, helper, [[Event.on_drag(:drag)], value])
      end
    end
  end

  test "NIF drag maps stay logical outside the source and rebind scale after patches" do
    scene = fn factor ->
      el(
        [key(:pad), scale(factor), width(px(180)), height(px(120)), Event.on_drag(:picked)],
        none()
      )
    end

    {binary, diff, assigned} =
      Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), scene.(1.6))

    id = <<assigned.id::64>>
    harness = TestHarness.new(500, 400)
    on_exit(fn -> TestHarness.stop(harness) end)
    assert :ok = TestHarness.set_input_target(harness, self())
    assert :ok = TestHarness.upload_full_bin(harness, binary)
    assert :ok = TestHarness.await_render(harness, 1000)
    assert :ok = TestHarness.cursor_button(harness, :press, 35.5 * 1.6, 20.25 * 1.6)
    assert_receive {:emerge_skia_event, {^id, :drag, start}}, 1000
    assert_sample(start, :start, 35.5, 20.25)

    {patch, _diff, _tree} = Emerge.Engine.diff_state_update(diff, scene.(2.0))
    assert :ok = TestHarness.apply_patch_bin(harness, patch)
    assert :ok = TestHarness.await_render(harness, 1000)
    refute_receive {:emerge_skia_event, {^id, :drag, _}}

    # Beyond both edges: the fresh 2x geometry, not press-time 1.6x geometry.
    assert :ok = TestHarness.cursor_pos(harness, 190.25 * 2.0, 130.5 * 2.0)
    assert_receive {:emerge_skia_event, {^id, :drag, move}}, 1000
    assert_sample(move, :move, 190.25, 130.5)
    assert :ok = TestHarness.cursor_button(harness, :release, 195.75 * 2.0, 140.25 * 2.0)
    assert_receive {:emerge_skia_event, {^id, :drag, ending}}, 1000
    assert_sample(ending, :end, 195.75, 140.25)
    assert :ok = TestHarness.cursor_pos(harness, 50.0, 50.0)
    refute_receive {:emerge_skia_event, {^id, :drag, _}}
  end

  defp assert_sample(sample, phase, x, y) do
    assert sample.phase == phase
    assert is_float(sample.x) and is_float(sample.y)
    assert_in_delta sample.x, x, 0.0001
    assert_in_delta sample.y, y, 0.0001
  end
end
