defmodule EmergeSkia.LiveAnimationHoverTest do
  use ExUnit.Case, async: false
  alias Emerge.TestSupport.Headless
  alias EmergeSkia.TestSupport.AnimatedHitCase, as: Scene

  # The first render tells us where the target really was, including nesting,
  # scrolling and transforms. Never lay out a second copy to locate the cursor.
  defp start_scene(size, initial, animated) do
    {pid, r} = Headless.mount(size, initial || animated, [:pointer_move])
    first = Headless.pixels()

    first =
      if initial do
        send(pid, {:scene, animated})
        Headless.pixels()
      else
        first
      end

    {width, _} = size
    {offset, _} = :binary.match(first, :binary.copy(<<70, 96, 148, 255>>, 8))
    origin = div(offset, 4)
    {dx, dy} = Scene.probe(:newly_occupied_outside_host)
    point = {trunc(rem(origin, width) + dx), trunc(div(origin, width) + dy)}
    {pid, r, point, width}
  end

  defp hover({_, r, {x, y}, width}, within \\ 1400) do
    :ok = Headless.move(r, x, y)
    assert_receive {:raw, {:pointer_move, _}}
    assert_receive {:application, {:target_enter, %{x: _, y: _}}}, within
    await_paint(width, x, y, System.monotonic_time(:millisecond) + within)
  end

  defp await_paint(w, x, y, deadline) do
    pixels = Headless.pixels(max(deadline - System.monotonic_time(:millisecond), 0))

    if binary_part(pixels, (y * w + x) * 4, 4) != <<96, 132, 188, 255>> do
      assert System.monotonic_time(:millisecond) < deadline
      await_paint(w, x, y, deadline)
    end
  end

  for {label, size, initial, animated} <- [
        {"full upload", {180, 82}, nil, :tree},
        {"placeholder patch", {180, 82}, :placeholder_tree, :tree},
        {"page switch", {180, 82}, :page_switch_placeholder_tree, :page_switch_tree},
        {"nested page switch", {320, 260}, :demo_like_page_switch_placeholder_tree,
         :demo_like_tree},
        {"full wrapping row", {760, 260}, :demo_like_full_row_placeholder_tree,
         :demo_like_full_row_tree}
      ] do
    test "#{label} activates hover when animated paint reaches a static cursor" do
      start_scene(unquote(size), builder(unquote(initial)), builder(unquote(animated)))
      |> hover()
    end
  end

  defp builder(nil), do: nil
  defp builder(fun), do: fn -> apply(Scene, fun, []) end

  defp demo do
    start_scene(
      {320, 260},
      &Scene.demo_like_page_switch_placeholder_tree/0,
      &Scene.demo_like_tree/0
    )
  end

  @tag :full_sweep
  test "first activation is in the first loop, not a later loop" do
    hover(demo(), 1350)
  end

  @tag :full_sweep
  test "an element move that edits its log does not restart the animation" do
    {pid, r, {x, y}, width} = demo()
    :ok = Headless.move(r, x - 50, y)
    assert_receive {:application, {:probe, :target, _}}
    send(pid, {:scene, fn -> Scene.demo_like_tree_with_log("Element move") end})
    Headless.pixels()
    hover({pid, r, {x, y}, width})
  end

  @tag :full_sweep
  test "raw move repatches while travelling right do not delay activation" do
    {pid, r, {x, y}, width} = demo()

    for dx <- [-60, -40, -20, 0] do
      :ok = Headless.move(r, x + dx, y)
      assert_receive {:raw, {:pointer_move, _}}
      send(pid, {:scene, fn -> Scene.demo_like_tree_with_log("Mouse #{x + dx}") end})
      Headless.pixels()
    end

    assert_receive {:application, {:target_enter, _}}, 1400
    await_paint(width, x, y, System.monotonic_time(:millisecond) + 1400)
  end

  @tag :full_sweep
  test "cursor placement at different phases activates within one loop" do
    {_, r, point, _} = scene = demo()

    for phase <- 0..1350//50 do
      :ok = Headless.move(r, 0, 0)
      assert_receive {:raw, {:pointer_move, {+0.0, +0.0}}}
      until_time(System.monotonic_time(:millisecond) + phase)
      hover(scene)
      :ok = Headless.move(r, 0, 0)
      assert_receive {:application, {:target_leave, _}}, 1400, "did not leave #{inspect(point)}"
    end
  end

  defp until_time(time) do
    if System.monotonic_time(:millisecond) < time do
      Headless.pixels()
      until_time(time)
    end
  end

  @tag :full_sweep
  test "hover does not clear while the animation still covers the cursor" do
    {_, _, {x, y}, w} = scene = demo()
    hover(scene)
    # The highlighted run must persist on consecutive frames; a premature
    # leave exposes the ordinary target fill under the same stationary cursor.
    assert_hover_until_uncovered(w, x, y, System.monotonic_time(:millisecond) + 1600)
  end

  defp assert_hover_until_uncovered(w, x, y, deadline) do
    frame = Headless.pixels()

    case binary_part(frame, (y * w + x) * 4, 4) do
      <<96, 132, 188, 255>> ->
        assert System.monotonic_time(:millisecond) < deadline
        assert_hover_until_uncovered(w, x, y, deadline)

      <<70, 96, 148, 255>> ->
        flunk("hover cleared over the target")

      _ ->
        assert_receive {:application, {:target_leave, _}}, 1400
    end
  end
end
