defmodule Emerge.FixedChildLayoutTest do
  use ExUnit.Case, async: true
  use Emerge.UI

  alias Emerge.Engine
  alias EmergeSkia.Native
  alias EmergeSkia.TreeRenderer

  test "content-sized buttons preserve fixed child boxes and sibling positions across patches" do
    for scale <- [1.0, 1.5, 2.0] do
      resource = Native.tree_new()
      {binary, state, assigned} = Engine.encode_full(Engine.diff_state_new(), tree(""))
      assert {:ok, true} = Native.tree_upload(resource, binary)
      assert_boxes(resource, assigned, scale)

      Enum.reduce(["x", "", "text wider than the circle", ""], state, fn label, state ->
        {patch, next_state, assigned} = Engine.diff_state_update(state, tree(label))
        assert {:ok, true} = Native.tree_patch(resource, patch)
        assert_boxes(resource, assigned, scale)
        next_state
      end)
    end
  end

  test "empty and checked circle pixels match an explicitly sized button at every scale" do
    for scale <- [1.0, 1.5, 2.0], label <- ["", "x"] do
      opts = [
        otp_app: :emerge,
        width: round(240 * scale),
        height: round(80 * scale),
        scale: scale
      ]

      actual = TreeRenderer.render_to_pixels(tree(label), opts, 5_000)
      reference = TreeRenderer.render_to_pixels(tree(label, true), opts, 5_000)

      assert actual == reference,
             "circle layout/clip mismatch: label=#{inspect(label)} scale=#{scale}"
    end
  end

  defp assert_boxes(resource, %{children: [button, sibling]}, scale) do
    assert {:ok, frames} = Native.tree_layout(resource, 240 * scale, 80 * scale, scale)
    assert {:ok, ^frames} = Native.tree_layout(resource, 240 * scale, 80 * scale, scale)
    boxes = Map.new(frames, fn {<<id::unsigned-big-64>>, x, y, w, h} -> {id, {x, y, w, h}} end)
    [circle] = button.children

    # 28px child + 12px padding and 1px border on each side = 54px wrapper.
    assert boxes[button.id] == {8 * scale, 13 * scale, 54 * scale, 54 * scale}
    assert boxes[circle.id] == {21 * scale, 26 * scale, 28 * scale, 28 * scale}
    {sibling_x, _, _, _} = boxes[sibling.id]
    assert sibling_x == 62 * scale
  end

  defp tree(label, explicit_size? \\ false) do
    size = if explicit_size?, do: [width(px(54)), height(px(54))], else: []

    row(
      [width(px(240)), height(px(80)), padding(8), Background.color(color(:white))],
      [
        Input.button(
          [
            key(:toggle),
            padding(12),
            center_y(),
            Border.width(1),
            Border.color(color_rgba(255, 255, 255, 0.0))
          ] ++ size,
          el(
            [
              key(:circle),
              width(px(28)),
              height(px(28)),
              center_x(),
              center_y(),
              Border.rounded(999),
              Border.width(1),
              Border.color(color_rgb(148, 148, 148)),
              Font.size(16),
              Font.center()
            ],
            el([Transform.move_y(-1), Transform.move_x(0.5)], text(label))
          )
        ),
        Input.button(
          [
            key(:title),
            width(fill()),
            padding_each(15, 0, 15, 15),
            Background.color(color(:white)),
            Font.size(24)
          ],
          paragraph([width(fill()), Font.align_left()], [text("Task")])
        )
      ]
    )
  end
end
