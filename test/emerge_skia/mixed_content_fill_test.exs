defmodule EmergeSkia.MixedContentFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp cap(:content, _demand), do: min(content(), fill())
  defp cap(:pixel, demand), do: min(px(demand), fill())

  defp scene(policy, demand) do
    a =
      el(
        [
          key(:a),
          width(cap(policy, demand)),
          height(px(12)),
          Background.color(:red),
          Event.on_click(:a)
        ],
        el([width(px(demand))], none())
      )

    b =
      el(
        [key(:b), width(fill()), height(px(12)), Background.color(:blue), Event.on_click(:b)],
        none()
      )

    row([width(px(120)), height(px(12))], [a, b])
  end

  defp bands(a, b) do
    :binary.copy(:binary.copy(<<255, 0, 0, 255>>, a) <> :binary.copy(<<0, 0, 255, 255>>, b), 12)
  end

  defp click_at_35(r, target, local_x) do
    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, 35.0, 5.0)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {35.0, 5.0}}}}
    end

    assert_receive {:application, {^target, %{x: ^local_x, y: 5.0}}}
  end

  # Same pixels and input in all three cases. Only the last two require the
  # renderer to read the cap from the child's content instead of a px leaf.
  for {label, initial, edited} <- [
        {"pixel-cap control", :pixel, :pixel},
        {"content cap on cold mount", :content, :content},
        {"content cap introduced by a patch", :pixel, :content}
      ] do
    test "#{label}: 20/100 becomes 60/60 and the click follows submission" do
      {pid, r} =
        Headless.mount({120, 12}, fn -> scene(unquote(initial), 20) end, [:pointer_button],
          target_fps: 1
        )

      small = bands(20, 100)
      assert Headless.pixels() == small
      click_at_35(r, :b, 15.0)

      send(
        pid,
        {:scene,
         fn ->
           send(self(), :built)
           scene(unquote(edited), 90)
         end}
      )

      assert_receive {:application, :built}
      assert {:ok, ^small} = EmergeSkia.render_to_pixels(r)
      click_at_35(r, :b, 15.0)

      assert Headless.pixels(2500) == bands(60, 60)
      click_at_35(r, :a, 35.0)
    end
  end
end
