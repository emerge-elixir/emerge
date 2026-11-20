defmodule EmergeSkia.WrappedRowFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp cards(w, b) do
    card = fn id, size, color ->
      el(
        [key(id), width(size), height(content()), Background.color(color), Event.on_click(id)],
        el([width(px(20)), height(px(12))], none())
      )
    end

    wrapped_row(
      [width(px(w)), height(px(40)), spacing(4), Background.color(:white), Event.on_click(:host)],
      [card.(:a, fill(), :red), card.(:b, b, :blue)]
    )
  end

  defp pixels(w, aw, bx, by, bw) do
    for y <- 0..39, x <- 0..99, into: <<>> do
      cond do
        x >= w -> <<0::32>>
        x < aw and y < 12 -> <<255, 0, 0, 255>>
        x >= bx and x < bx + bw and y >= by and y < by + 12 -> <<0, 0, 255, 255>>
        true -> <<255, 255, 255, 255>>
      end
    end
  end

  defp button(r, phase, x, y) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, x, y, id, lx, ly) do
    for phase <- [:press, :release], do: button(r, phase, x, y)
    assert_receive {:application, {^id, %{x: ^lx, y: ^ly}}}
  end

  test "two plain fills share a wrapped line; an explicit weight is not a full-line width" do
    for {policy, aw, bx, bw} <- [
          {fill(), 48, 52, 48},
          {fill(1), 48, 52, 48},
          {fill(3), 24, 28, 72}
        ] do
      {_, r} = Headless.mount({100, 40}, fn -> cards(100, policy) end, [:pointer_button])
      assert Headless.pixels() == pixels(100, aw, bx, 0, bw)
      click(r, bx + 2.0, 6.0, :b, 2.0, 6.0)
    end
  end

  test "the weight-only edit publishes the new width and click target together" do
    {p, r} =
      Headless.mount({100, 40}, fn -> cards(100, fill()) end, [:pointer_button], target_fps: 1)

    before = pixels(100, 48, 52, 0, 48)
    assert Headless.pixels() == before

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         cards(100, fill(3))
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^before} = EmergeSkia.render_to_pixels(r)
    click(r, 50.0, 6.0, :host, 50.0, 6.0)
    assert Headless.pixels(2500) == pixels(100, 24, 28, 0, 72)
    click(r, 50.0, 6.0, :b, 22.0, 6.0)
  end

  test "the minimum decides the break and each new line divides only its own room" do
    {p, r} = Headless.mount({100, 40}, fn -> cards(44, fill(3)) end, [:pointer_button])
    assert Headless.pixels() == pixels(44, 20, 24, 0, 20)
    click(r, 28.0, 6.0, :b, 4.0, 6.0)

    for {w, aw, bx, by, bw} <- [{43, 43, 0, 16, 43}, {100, 24, 28, 0, 72}, {44, 20, 24, 0, 20}] do
      send(p, {:scene, fn -> cards(w, fill(3)) end})
      assert Headless.pixels() == pixels(w, aw, bx, by, bw)
      click(r, bx + 4.0, by + 6.0, :b, 4.0, 6.0)
    end
  end
end
