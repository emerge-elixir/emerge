defmodule EmergeSkia.WeightedFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.TestSupport.Headless

  defp panel(axis, key, share, color) do
    size =
      if axis == :column, do: [width(fill()), height(share)], else: [width(share), height(px(12))]

    el([key(key), Background.color(color), Event.on_click(key) | size], none())
  end

  defp view(axis, second) do
    children = [panel(axis, :a, fill(), :red), panel(axis, :b, second, :blue)]

    if axis == :column,
      do: column([height(fill()), width(fill())], children),
      else: row([width(fill())], children)
  end

  defp click(r, {x, y}, key, pos) do
    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, x, y)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
    end

    assert_receive {:application, {^key, ^pos}}
  end

  defp bands(axis, first) do
    red = fn n -> :binary.copy(<<255, 0, 0, 255>>, n) end
    blue = fn n -> :binary.copy(<<0, 0, 255, 255>>, n) end

    if axis == :column,
      do: red.(first * 12) <> blue.((90 - first) * 12),
      else: :binary.copy(red.(first) <> blue.(90 - first), 12)
  end

  test "fractional weights clip scaled children without rounding the hit boundary" do
    scene = fn b ->
      cell =
        column([key(:lane), width(fill()), height(fill(0.5)), scale(2)], [
          el(
            [width(fill()), height(px(50)), Background.color(:red), Event.on_click(:ink)],
            none()
          )
        ])

      column(
        [width(fill()), height(fill()), spacing(1), Event.on_click(:gap)],
        [cell, panel(:column, :b, fill(b), :blue)]
      )
    end

    size = {12, 91}
    Headless.mount(size, fn -> scene.(0.5) end)
    wide = Headless.pixels()
    {pid, r} = Headless.mount(size, fn -> scene.(1.5) end, [:pointer_button])
    narrow = Headless.pixels()
    refute narrow == wide
    assert binary_part(narrow, 4 * (12 * 22 + 5), 4) == <<255, 0, 0, 255>>
    click(r, {5.0, 22.5}, :ink, %{x: 2.5, y: 11.25})
    click(r, {5.0, 22.75}, :gap, %{x: 5.0, y: 22.75})
    click(r, {5.0, 24.5}, :b, %{x: 5.0, y: 1.0})
    send(pid, {:scene, fn -> scene.(0.5) end})
    assert Headless.pixels() == wide
    click(r, {5.0, 22.75}, :ink, %{x: 2.5, y: 11.375})
  end

  for axis <- [:column, :row] do
    test "#{axis}: a weight-only edit changes the submitted split and its click target together" do
      axis = unquote(axis)

      {size, point, local_a, local_b} =
        if axis == :column do
          {{12, 90}, {5.0, 35.0}, %{x: 5.0, y: 35.0}, %{x: 5.0, y: 5.0}}
        else
          {{90, 12}, {35.0, 5.0}, %{x: 35.0, y: 5.0}, %{x: 5.0, y: 5.0}}
        end

      Headless.mount(size, fn -> view(axis, fill(2)) end)
      cold = Headless.pixels()
      assert cold == bands(axis, 30)

      {pid, r} =
        Headless.mount(size, fn -> view(axis, fill()) end, [:pointer_button], target_fps: 1)

      equal = Headless.pixels()
      assert equal == bands(axis, 45)

      send(
        pid,
        {:scene,
         fn ->
           send(self(), :submitted)
           view(axis, fill(2))
         end}
      )

      assert_receive {:application, :submitted}
      assert {:ok, ^equal} = EmergeSkia.render_to_pixels(r)
      click(r, point, :a, local_a)
      assert Headless.pixels(2500) == cold
      click(r, point, :b, local_b)
      send(pid, {:scene, fn -> view(axis, fill(1)) end})
      assert Headless.pixels(2500) == equal
      click(r, point, :a, local_a)
    end
  end
end
