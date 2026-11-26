defmodule EmergeSkia.InlineLayoutRotationTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.Engine
  alias Emerge.TestSupport.Headless
  alias EmergeSkia.{Native, TreeRenderer}

  @size {260, 240}
  @red <<255, 0, 0, 255>>
  @blue <<0, 0, 255, 255>>
  defp scene(pose) do
    column([width(fill()), height(fill())], [
      paragraph(
        [
          key(:paper),
          width(px(164)),
          padding_xy(4, 45),
          Font.size(16),
          Font.family(:monospace),
          Background.color(:blue),
          Event.on_click(:paper)
        ],
        [
          el([key(:prefix)], text("A ")),
          el(
            [
              key(:phrase),
              pose,
              padding(2),
              Background.color(:red),
              Font.color(:white),
              Event.on_click(:phrase)
            ],
            text("ride the bus")
          ),
          el([key(:suffix), Background.color(:yellow), Event.on_click(:suffix)], text(" Z")),
          el([key(:tail)], text(" tail"))
        ]
      ),
      el(
        [
          key(:next),
          width(px(16)),
          height(px(8)),
          Background.color(:green),
          Event.on_click(:next)
        ],
        none()
      )
    ])
  end

  defp keys(t), do: [{t.id, t.key} | Enum.flat_map(t.children, &keys/1)]

  defp boxes(t) do
    {bytes, _, tree} = Engine.encode_full(Engine.diff_state_new(), t)
    names = Map.new(keys(tree))
    detached = Native.tree_new()
    assert {:ok, _} = Native.tree_upload(detached, bytes)
    assert {:ok, rects} = Native.tree_layout(detached, 260.0, 240.0, 1.0)

    for {id, x, y, w, h} <- rects,
        key = names[Engine.NodeId.decode(id)],
        into: %{},
        do: {key, {x, y, w, h}}
  end

  defp export(t, scale \\ 1),
    do:
      TreeRenderer.render_to_pixels(
        t,
        [otp_app: :emerge, width: round(260 * scale), height: round(240 * scale), scale: scale],
        5000
      )

  defp pixel(frame, {x, y}), do: binary_part(frame, 4 * (trunc(y) * 260 + trunc(x)), 4)

  defp click(r, {px, py}, who, {lx, ly}) do
    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, px, py)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {x, y}}}}
      assert_in_delta x, px, 0.001
      assert_in_delta y, py, 0.001
    end

    assert_receive {:application, {^who, %{x: x, y: y}}}
    assert_in_delta x, lx, 0.002
    assert_in_delta y, ly, 0.002
  end

  test "only a layout turn reserves the diagonal between two spans and grows their line" do
    reference = boxes(scene(rotate(0)))
    {x, y, w, h} = reference.phrase
    edge = (w + h) / :math.sqrt(2)
    assert w > edge and edge > h
    {_, wrap_y, _, _} = reference.suffix
    assert_in_delta wrap_y, y + h, 0.001

    {pid, r} = Headless.mount(@size, fn -> scene(rotate(0)) end, [:pointer_button])
    plain = Headless.pixels()
    assert plain == export(scene(rotate(0)))

    for {pose, layout?, sign} <- [
          {rotate(45), true, 1},
          {Transform.rotate(45), false, 1},
          {rotate(-45), true, -1},
          {rotate(45), true, 1}
        ] do
      build = fn -> scene(pose) end
      send(pid, {:scene, build})
      frame = Headless.pixels()
      assert frame == export(build.())
      refute frame == plain
      rects = boxes(build.())

      {cx, cy} =
        if layout? do
          {px, py, pw, ph} = rects.phrase

          for {value, expected} <- [{px, x}, {py, y}, {pw, edge}, {ph, edge}],
              do: assert_in_delta(value, expected, 0.001)

          {sx, sy, _, _} = rects.suffix
          assert_in_delta sx, x + edge, 0.001
          assert sy == y
          assert_in_delta elem(rects.tail, 1), y + edge, 0.001
          assert_in_delta elem(rects.next, 1) - elem(reference.next, 1), edge - h, 0.001
          {x + edge / 2, y + edge / 2}
        else
          assert rects == reference
          {x + w / 2, y + h / 2}
        end

      # Sample the padding at two known local points, not the center which
      # would also hit an unrotated rectangle. Test the signed inverse too.
      for u <- [30, 90] do
        v = 1.25
        dx = u - w / 2
        dy = v - h / 2
        point = {cx + (dx - sign * dy) / :math.sqrt(2), cy + (sign * dx + dy) / :math.sqrt(2)}
        assert pixel(frame, point) == @red
        click(r, point, :phrase, {u, v})
      end

      # White pixels are glyphs, not the rotated red decoration. Their end-to-end
      # slope distinguishes the signed turn from a horizontal or counter-turned run.
      ink =
        for y <- 0..239, x <- 0..259, pixel(frame, {x, y}) == <<255, 255, 255, 255>>, do: {x, y}

      {{left_x, left_y}, {right_x, right_y}} = Enum.min_max_by(ink, &elem(&1, 0))
      assert right_x - left_x > 65 and sign * (right_y - left_y) > 55
      {sx, sy, _, _} = rects.suffix
      click(r, {sx + 2.0, sy + 2.0}, :suffix, {2, 2})
      {_, ny, _, _} = rects.next
      click(r, {2.0, ny + 2.0}, :next, {2, 2})

      if layout? do
        outside = if sign > 0, do: {x + 4, y + edge - 4}, else: {x + 4, y + 4}
        assert pixel(frame, outside) == @blue
        click(r, outside, :paper, outside)
      end
    end
  end

  test "vertical alignment moves only the continuation sharing the tall line, including its hit" do
    build = fn align ->
      paragraph(
        [
          width(px(134)),
          Font.family(:monospace),
          Font.size(16),
          Background.color(:blue),
          Event.on_click(:paper)
        ],
        [
          el([key(:phrase), rotate(45), padding(2)], text("ride the bus")),
          el([key(:peer), align, Background.color(:red), Event.on_click(:peer)], text("AB CDEFG"))
        ]
      )
    end

    top = build.(align_top())
    %{phrase: {_, _, edge, tall}, peer: {0.0, 0.0, _, both_lines}} = boxes(top)
    short = both_lines - tall
    assert_in_delta tall, 102.05, 0.01
    assert_in_delta short, 21.12, 0.01
    {pid, r} = Headless.mount(@size, fn -> build.(align_top()) end, [:pointer_button])
    assert Headless.pixels() == export(top)

    for {align, factor} <- [{center_y(), 0.5}, {align_bottom(), 1}, {align_top(), 0}] do
      scene = fn -> build.(align) end
      send(pid, {:scene, scene})
      frame = Headless.pixels()
      assert frame == export(scene.())
      y = (tall - short) * factor
      # Both red patches belong to one listener, with one local origin.
      for point = {px, py} <- [{edge + 2, y + 2}, {2.0, tall + 2}] do
        assert pixel(frame, point) == @red
        click(r, point, :peer, {px, py - y})
      end

      if factor > 0 do
        vacant = {edge + 2, 2.0}
        assert pixel(frame, vacant) == @blue
        click(r, vacant, :paper, vacant)
      end
    end
  end

  test "an inline half turn has the same turned continuations and local clicks for either tag" do
    build = fn pose ->
      fn ->
        paragraph(
          [width(px(64)), height(px(98)), Background.color(:blue), Event.on_click(:paper)],
          [
            el(
              [pose, padding(2), Background.color(:red), Event.on_click(:phrase)],
              text("AB CD EF")
            )
          ]
        )
      end
    end

    plain = build.(rotate(0))
    {pid, r} = Headless.mount(@size, plain, [:pointer_button])
    upright = Headless.pixels()
    turned = export(build.(Transform.rotate(180)).())
    refute turned == upright

    for pose <- [rotate(180), Transform.rotate(180), rotate(180)] do
      send(pid, {:scene, build.(pose)})
      assert Headless.pixels() == turned
      assert {:ok, @blue} = EmergeSkia.render_to_pixels(r, region: {1, 1, 1, 1})
      click(r, {1.0, 1.0}, :paper, {1, 1})
    end

    send(pid, {:scene, plain})
    assert Headless.pixels() == upright
    click(r, {1.0, 1.0}, :phrase, {1, 1})
  end

  test "layout and paint poses compose with ancestor rotation, clipping and negative or zero scale" do
    view = fn pose, zoom ->
      paragraph([rotate(90), padding(6), Border.rounded(8), width(px(120)), height(px(98))], [
        el(
          [pose, Transform.scale(zoom), Transform.move_x(8), Background.color(:red)],
          text("AB CD EF")
        )
      ])
    end

    for zoom <- [1, -1] do
      assert export(view.(rotate(180), zoom), 1.5) ==
               export(view.(Transform.rotate(180), zoom), 1.5)

      refute export(view.(rotate(45), zoom), 1.5) == export(view.(rotate(45), -zoom), 1.5)

      refute export(view.(rotate(45), zoom), 1.5) ==
               export(view.(Transform.rotate(45), zoom), 1.5)
    end

    assert export(view.(rotate(45), 0), 1.5) == :binary.copy(<<0::32>>, 390 * 360)
  end
end
