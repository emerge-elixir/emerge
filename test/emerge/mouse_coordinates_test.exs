defmodule Emerge.MouseCoordinatesTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Engine.{AttrCodec, EventRegistry}
  alias Emerge.UI.Event
  alias EmergeSkia.TestHarness

  test "native press, movement and release deliver logical float maps through the NIF" do
    tree =
      el(
        [
          scale(2),
          width(px(100)),
          height(px(60)),
          Event.on_mouse_down(:down),
          Event.on_mouse_move(:move),
          Event.on_mouse_up(:up)
        ],
        none()
      )

    {binary, _state, assigned} = Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), tree)
    id = <<assigned.id::64>>
    harness = TestHarness.new(300, 200)
    on_exit(fn -> TestHarness.stop(harness) end)
    assert :ok = TestHarness.set_input_target(harness, self())
    assert :ok = TestHarness.upload_full_bin(harness, binary)
    assert :ok = TestHarness.await_render(harness, 1000)
    assert :ok = TestHarness.cursor_button(harness, :press, 71.0, 40.5)
    assert_receive {:emerge_skia_event, {^id, :mouse_down, %{x: 35.5, y: 20.25}}}, 1000
    assert :ok = TestHarness.cursor_pos(harness, 91.0, 44.5)
    assert :ok = TestHarness.cursor_button(harness, :release, 91.0, 44.5)
    assert_receive {:emerge_skia_event, {^id, :mouse_move, %{x: 45.5, y: 22.25}}}, 1000
    assert_receive {:emerge_skia_event, {^id, :mouse_up, %{x: 45.5, y: 22.25}}}, 1000
    refute_receive {:emerge_skia_event, {^id, :mouse_move, _position}}
  end

  test "mouse helpers keep their signatures, routes and encoded presence flags" do
    for kind <- [:click, :mouse_down, :mouse_up, :mouse_enter, :mouse_leave, :mouse_move] do
      helper = String.to_existing_atom("on_#{kind}")
      assert {^helper, {pid, :moved}} = apply(Event, helper, [:moved])
      assert pid == self()
      assert {^helper, {^pid, {:pointer, 42}}} = apply(Event, helper, [{self(), {:pointer, 42}}])
      attrs = Map.new([apply(Event, helper, [{self(), {:pointer, 42}}])])
      assert EventRegistry.node_events(attrs) == %{kind => {self(), {:pointer, 42}}}
      assert AttrCodec.decode_attrs(AttrCodec.encode_attrs(attrs)) == %{helper => true}
    end
  end
end
