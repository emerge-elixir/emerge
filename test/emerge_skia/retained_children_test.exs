defmodule EmergeSkia.RetainedChildrenTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp scene(keys) do
    row([width(px(80))], Enum.map(keys, &child/1))
  end

  defp child(k) do
    {w, color} = %{a: {20, :red}, b: {20, color_rgb(0, 255, 0)}, c: {40, :blue}}[k]
    el([key(k), width(px(w)), height(px(20)), Background.color(color), Event.on_click(k)], none())
  end

  defp bands(order) do
    rgb = %{a: <<255, 0, 0, 255>>, b: <<0, 255, 0, 255>>, c: <<0, 0, 255, 255>>, empty: <<0::32>>}
    line = for k <- order, into: <<>>, do: :binary.copy(rgb[k], 20)
    :binary.copy(line, 20)
  end

  defp button(r, action, x) do
    pos = {x * 1.0, 5.0}
    assert :ok = EmergeSkia.send_input(r, {:pointer_button, {:left, action, [], pos}})
    assert_receive {:raw, {:pointer_button, {:left, ^action, [], ^pos}}}
  end

  defp click(r, x, target) do
    pos = {x * 1.0, 5.0}
    assert :ok = EmergeSkia.send_input(r, {:pointer_move, pos})
    assert_receive {:raw, {:pointer_move, ^pos}}
    button(r, :press, x)
    button(r, :release, x)

    if target do
      assert_receive {:application, {^target, %{x: 5.0, y: 5.0}}}
    else
      refute_receive {:application, _}
    end
  end

  test "a continuous keyed move changes the drawn site and route, not the press attachment" do
    view = fn site ->
      card =
        el(
          [
            key(:card),
            width(px(20)),
            height(px(20)),
            Background.color(:red),
            Event.on_click(site)
          ],
          none()
        )

      parents =
        for k <- [:left, :right],
            do: column([key(k), width(px(60))], if(k == site, do: [card], else: []))

      row([], parents)
    end

    {pid, r} =
      Headless.mount({120, 20}, fn -> view.(:left) end, [:pointer_move, :pointer_button],
        target_fps: 1
      )

    red = :binary.copy(<<255, 0, 0, 255>>, 20)
    blank = fn w -> :binary.copy(<<0::32>>, w) end
    left = :binary.copy(red <> blank.(100), 20)
    right = :binary.copy(blank.(60) <> red <> blank.(40), 20)
    assert Headless.pixels() == left

    send(
      pid,
      {:scene,
       fn ->
         send(self(), :moved)
         view.(:right)
       end}
    )

    assert_receive {:application, :moved}
    click(r, 5, :left)
    button(r, :press, 5)
    assert Headless.pixels(2500) == right
    button(r, :release, 65)
    assert_receive {:application, {:right, %{x: 5.0, y: 5.0}}}
    click(r, 5, nil)
    click(r, 65, :right)
  end

  test "several callback edits install together even when the pixels are reused" do
    view = fn label ->
      el([width(px(80)), height(px(20)), Background.color(:red), Event.on_click(label)], none())
    end

    {pid, r} = Headless.mount({80, 20}, fn -> view.(:first) end, [:pointer_button], target_fps: 1)
    pixels = Headless.pixels()

    for label <- [:middle, :last] do
      send(
        pid,
        {:scene,
         fn ->
           send(self(), {:submitted, label})
           view.(label)
         end}
      )

      assert_receive {:application, {:submitted, ^label}}
    end

    for {phase, expected} <- [pending: :first, drawn: :last] do
      if phase == :drawn, do: assert(Headless.pixels(2500) == pixels)
      button(r, :press, 25)
      button(r, :release, 25)
      assert_receive {:application, {^expected, %{x: 25.0, y: 5.0}}}
    end
  end

  test "a removed listener still receives clicks while its frame is displayed" do
    {pid, r} =
      Headless.mount({80, 20}, fn -> scene([:a, :b, :c]) end, [:pointer_move, :pointer_button],
        target_fps: 1
      )

    abc = bands([:a, :b, :c, :c])
    assert Headless.pixels() == abc
    # A normal application message sent from render runs after the viewport
    # returns from submitting its patch. The cap leaves the old frame installed.
    send(
      pid,
      {:scene,
       fn ->
         send(self(), :removed_b)
         scene([:a, :c])
       end}
    )

    assert_receive {:application, :removed_b}
    assert {:ok, ^abc} = EmergeSkia.render_to_pixels(r)
    click(r, 25, :b)
    assert Headless.pixels(2500) == bands([:a, :c, :c, :empty])
    click(r, 25, :c)
  end

  test "inserting and removing a keyed sibling moves the surviving click target with its pixels" do
    {pid, r} =
      Headless.mount({80, 20}, fn -> scene([:a, :c]) end, [:pointer_move, :pointer_button])

    ac = bands([:a, :c, :c, :empty])
    abc = bands([:a, :b, :c, :c])
    assert Headless.pixels() == ac
    click(r, 25, :c)
    click(r, 65, nil)
    button(r, :press, 45)
    send(pid, {:scene, fn -> scene([:a, :b, :c]) end})
    assert Headless.pixels() == abc
    button(r, :release, 45)
    assert_receive {:application, {:c, %{x: 5.0, y: 5.0}}}
    click(r, 25, :b)
    button(r, :press, 65)
    button(r, :release, 65)
    assert_receive {:application, {:c, %{x: 25.0, y: 5.0}}}

    button(r, :press, 25)
    send(pid, {:scene, fn -> scene([:a, :c]) end})
    assert Headless.pixels() == ac
    send(pid, {:scene, fn -> scene([:a, :b, :c]) end})
    assert Headless.pixels() == abc
    button(r, :release, 25)
    refute_receive {:application, _}
    click(r, 25, :b)
    send(pid, {:scene, fn -> scene([:a, :c]) end})
    assert Headless.pixels() == ac
    click(r, 25, :c)
    click(r, 65, nil)
  end
end
