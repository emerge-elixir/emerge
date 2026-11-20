defmodule EmergeSkia.ColumnCrossWidthTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp scene(policy, demand, viewport \\ 120) do
    a =
      el(
        [key(:a), width(policy), height(px(12)), Background.color(:red), Event.on_click(:a)],
        el([width(px(demand))], none())
      )

    b =
      el(
        [key(:b), width(fill()), height(px(12)), Background.color(:blue), Event.on_click(:b)],
        none()
      )

    host =
      column(
        [
          width(px(120)),
          height(px(40)),
          padding(6),
          spacing(4),
          Background.color(:white),
          Event.on_click(:host)
        ],
        [a, b]
      )

    el(
      [width(px(viewport)), height(px(40)), Background.color(:black), Event.on_click(:outside)],
      host
    )
  end

  defp frame(a, viewport \\ 120) do
    for y <- 0..39, x <- 0..(viewport - 1), into: <<>> do
      cond do
        x >= 120 -> <<0, 0, 0, 255>>
        y in 6..17 and x >= 6 and x < 6 + a -> <<255, 0, 0, 255>>
        y in 22..33 and x in 6..113 -> <<0, 0, 255, 255>>
        true -> <<255, 255, 255, 255>>
      end
    end
  end

  defp button(r, phase, {x, y}) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, point, target, {lx, ly}) do
    for phase <- [:press, :release], do: button(r, phase, point)

    assert_receive {:application, {^target, %{x: ^lx, y: ^ly}}}
  end

  test "plain fill takes the entire inner width on both lines" do
    {_, r} = Headless.mount({120, 40}, fn -> scene(fill(), 30) end, [:pointer_button])
    assert Headless.pixels() == frame(108)
    click(r, {76.0, 10.0}, :a, {70.0, 4.0})
    click(r, {76.0, 26.0}, :b, {70.0, 4.0})
  end

  test "a weighted width does not borrow from the line below" do
    for weight <- [1, 3] do
      {_, r} = Headless.mount({120, 40}, fn -> scene(fill(weight), 30) end, [:pointer_button])
      assert Headless.pixels() == frame(108)
      click(r, {113.0, 10.0}, :a, {107.0, 4.0})
      click(r, {113.0, 26.0}, :b, {107.0, 4.0})
    end
  end

  for start <- [:cold_cap, :patch_to_cap] do
    test "#{start}: changing only the content cap's demand moves the click on submission" do
      cap = min(content(), fill(3))
      first = if unquote(start) == :cold_cap, do: cap, else: px(30)

      {pid, r} =
        Headless.mount({120, 40}, fn -> scene(first, 30) end, [:pointer_button], target_fps: 1)

      small = frame(30)
      assert Headless.pixels() == small

      send(
        pid,
        {:scene,
         fn ->
           send(self(), :built)
           scene(cap, 90)
         end}
      )

      assert_receive {:application, :built}
      assert {:ok, ^small} = EmergeSkia.render_to_pixels(r)
      click(r, {76.0, 10.0}, :host, {76.0, 10.0})
      click(r, {76.0, 26.0}, :b, {70.0, 4.0})

      assert Headless.pixels(2500) == frame(90)
      click(r, {76.0, 10.0}, :a, {70.0, 4.0})
      click(r, {76.0, 26.0}, :b, {70.0, 4.0})
      send(pid, {:scene, fn -> scene(cap, 150) end})
      assert Headless.pixels(2500) == frame(108)
      click(r, {115.0, 10.0}, :host, {115.0, 10.0})
    end
  end

  defp move_scene(site) do
    card =
      el(
        [
          key(:card),
          width(max(content(), fill(3))),
          height(px(12)),
          Background.color(:red),
          Event.on_click(site)
        ],
        el([width(px(30))], none())
      )

    peer = el([key(:peer), width(fill()), height(px(12)), Background.color(:blue)], none())

    column([width(px(120)), height(px(24)), Background.color(:white)], [
      row(
        [key(:row), width(px(120)), height(px(12))],
        if(site == :row, do: [card, peer], else: [peer])
      ),
      column(
        [key(:column), width(px(108)), height(px(12))],
        if(site == :column, do: [card], else: [])
      ),
      wrapped_row([key(:wrap), height(px(0))], if(site == :wrap, do: [card], else: []))
    ])
  end

  test "a press follows a keyed Row ↔ Column move, even after a rejected sparse edit" do
    {pid, r} =
      Headless.mount({120, 24}, fn -> move_scene(:row) end, [:pointer_button], target_fps: 1)

    red = fn n -> :binary.copy(<<255, 0, 0, 255>>, n) end
    blue = fn n -> :binary.copy(<<0, 0, 255, 255>>, n) end
    white = fn n -> :binary.copy(<<255, 255, 255, 255>>, n) end
    top = :binary.copy(red.(90) <> blue.(30), 12) <> white.(120 * 12)
    bottom = blue.(120 * 12) <> :binary.copy(red.(108) <> white.(12), 12)
    assert Headless.pixels() == top
    button(r, :press, {85.0, 5.0})

    send(
      pid,
      {:scene,
       fn ->
         send(self(), :moved)
         move_scene(:column)
       end}
    )

    assert_receive {:application, :moved}

    # Use the public producer at the same accepted declaration. Only send
    # the rejected move this way, so the viewport and its installed routes live on.
    before = Emerge.Engine.diff_state_new(move_scene(:row))
    {_, pending} = Emerge.Engine.diff_state_update_binary(before, move_scene(:column))

    assert_raise RuntimeError,
                 ~r/renderer_patch failed: .*fill width is not supported on a WrappedRow child/,
                 fn ->
                   EmergeSkia.patch_tree(r, pending, move_scene(:wrap))
                 end

    assert {:ok, ^top} = EmergeSkia.render_to_pixels(r)
    assert Headless.pixels(2500) == bottom
    button(r, :release, {100.0, 17.0})
    assert_receive {:application, {:column, %{x: 100.0, y: 5.0}}}
    button(r, :press, {100.0, 17.0})
    send(pid, {:scene, fn -> move_scene(:row) end})
    assert Headless.pixels(2500) == top
    button(r, :release, {85.0, 5.0})
    assert_receive {:application, {:row, %{x: 85.0, y: 5.0}}}
  end

  test "a content floor can exceed the Column's offer, not its paint or hit clip" do
    # At demand 150 all three controls are 42 wider than the 108px offer.
    # Pinned main instead limits the fill-bearing floor to 108. Red in the
    # right padding distinguishes the two, and the black strip tests the clip.
    for floor <- [content(), max(content(), px(108)), max(content(), fill(3))] do
      {_, r} = Headless.mount({160, 40}, fn -> scene(floor, 150, 160) end, [:pointer_button])
      assert Headless.pixels() == frame(150, 160)
      click(r, {117.0, 10.0}, :a, {111.0, 4.0})
      click(r, {121.0, 10.0}, :outside, {121.0, 10.0})
      click(r, {117.0, 26.0}, :host, {117.0, 26.0})
    end
  end
end
