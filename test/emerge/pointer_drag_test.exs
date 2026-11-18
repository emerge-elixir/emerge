defmodule Emerge.PointerDragTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

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

    {pid, r} = Headless.mount({500, 400}, fn -> scene.(1.6) end)
    Headless.pixels()
    assert :ok = Headless.button(r, :press, 35.5 * 1.6, 20.25 * 1.6)
    assert_receive {:application, {:picked, start}}, 1000
    assert_sample(start, :start, 35.5, 20.25)

    send(pid, {:scene, fn -> scene.(2.0) end})
    Headless.pixels()
    refute_receive {:application, {:picked, _}}

    # Beyond both edges: the fresh 2x geometry, not press-time 1.6x geometry.
    assert :ok = Headless.move(r, 190.25 * 2.0, 130.5 * 2.0)
    assert_receive {:application, {:picked, move}}, 1000
    assert_sample(move, :move, 190.25, 130.5)
    assert :ok = Headless.button(r, :release, 195.75 * 2.0, 140.25 * 2.0)
    assert_receive {:application, {:picked, ending}}, 1000
    assert_sample(ending, :end, 195.75, 140.25)
    assert :ok = Headless.move(r, 50.0, 50.0)
    refute_receive {:application, {:picked, _}}
  end

  test "capture does not revive after losing usable geometry" do
    scene = fn factor ->
      el(
        [
          key(:pad),
          width(px(100)),
          height(px(60)),
          Transform.scale(factor),
          Event.on_drag(:drag)
        ],
        none()
      )
    end

    {pid, r} = Headless.mount({300, 200}, fn -> scene.(1.0) end)
    Headless.pixels()
    :ok = Headless.button(r, :press, 20.0, 10.0)
    assert_receive {:application, {:drag, %{phase: :start}}}, 1000

    for factor <- [0.0, 1.0] do
      send(pid, {:scene, fn -> scene.(factor) end})
      Headless.pixels()
    end

    :ok = Headless.move(r, 200.0, 100.0)
    :ok = Headless.button(r, :release, 200.0, 100.0)
    refute_receive {:application, {:drag, _}}, 500
  end

  defp assert_sample(sample, phase, x, y) do
    assert sample.phase == phase
    assert is_float(sample.x) and is_float(sample.y)
    assert_in_delta sample.x, x, 0.0001
    assert_in_delta sample.y, y, 0.0001
  end
end
