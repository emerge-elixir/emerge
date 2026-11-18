defmodule EmergeSkia.LiveLengthChangeTest do
  use ExUnit.Case, async: false
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp tree(length, child) do
    row([key(:root), width(px(600)), height(px(60))], [
      el(
        [
          key(:target),
          Animation.change([width(length)], 1000, :linear),
          height(fill()),
          Font.size(20),
          Background.color(:red),
          Interactive.mouse_over([Background.color(:blue)]),
          Event.on_mouse_enter(:entered)
        ],
        child
      ),
      el([key(:peer), width(fill()), height(fill())], none())
    ])
  end

  test "a public pixel-to-fill change advances real actor hit geometry" do
    changes(fn -> tree(px(40), none()) end, fn -> tree(fill(), none()) end, 200)
  end

  test "unchanged content width policy animates a descendant-only text update" do
    changes(
      fn -> tree(content(), text("S")) end,
      fn -> tree(content(), text("Something")) end,
      50
    )
  end

  defp changes(initial, edited, x) do
    {pid, renderer} = Headless.mount({600, 60}, initial, [:pointer_move])
    before = Headless.pixels()
    :ok = Headless.move(renderer, x, 30)
    assert_receive {:raw, {:pointer_move, _}}
    refute_receive {:application, {:entered, _}}
    send(pid, {:scene, edited})
    # A symbolic switch would skip every intermediate width. Observe the edge
    # on actual frames; never prescribe times to the animation clock.
    width = fn pixels ->
      for(<<r, g, b, 255 <- binary_part(pixels, 600 * 4, 600 * 4)>>, {r, g, b} == {255, 0, 0},
        do: 1
      )
      |> length()
    end

    deadline = System.monotonic_time(:millisecond) + 2000
    advance(x, width, width.(before), false, deadline)
    assert_receive {:application, {:entered, %{x: _, y: _}}}, 500
  end

  defp advance(x, width, initial, intermediate?, deadline) do
    pixels = Headless.pixels(Kernel.max(deadline - System.monotonic_time(:millisecond), 0))

    if binary_part(pixels, (30 * 600 + x) * 4, 4) == <<0, 0, 255, 255>> do
      assert intermediate?, "jumped to hover without rendering an intermediate width"
    else
      w = width.(pixels)
      assert System.monotonic_time(:millisecond) < deadline
      advance(x, width, initial, intermediate? or (initial < w and w < x), deadline)
    end
  end
end
