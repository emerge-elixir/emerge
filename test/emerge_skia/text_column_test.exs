defmodule EmergeSkia.TextColumnTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions, only: [button: 3, click: 4, fresh: 3, frame: 2]
  alias Emerge.TestSupport.Headless

  @size {112, 160}
  defp box(attrs), do: el(attrs, none())

  defp side(id, w, h, where),
    do:
      box([
        key(id),
        where,
        width(px(w)),
        height(px(h)),
        Background.color(:red),
        Event.on_click(id)
      ])

  defp block,
    do:
      box([
        key(:block),
        center_x(),
        align_bottom(),
        width(px(20)),
        height(px(10)),
        Background.color(:orange),
        Event.on_click(:block)
      ])

  defp document(floats, route) do
    host = [
      width(px(100)),
      padding(4),
      spacing_xy(2, 2),
      Font.size(12),
      Font.family(:monospace),
      Background.color(:black)
    ]

    p = paragraph([key(:first), width(fill()), height(px(24)), spacing_xy(2, 0)], [text("A")])

    link =
      el([key(:span), padding(1), Background.color(:blue), Event.on_click(route)], text("AA BB"))

    q = paragraph([key(:second), width(fill()), height(px(24)), spacing_xy(2, 0)], [link])
    text_column(host, floats ++ [p, q, block()])
  end

  test "a centered block clears the float; two paragraphs share it until removal and undo" do
    start = fn -> document([side(:margin, 30, 70, align_left())], :span) end
    {p, r} = Headless.mount(@size, start, [:pointer_button])
    a = Headless.pixels()
    click(r, {7.0, 7.0}, :margin, {3.0, 3.0})
    click(r, {39.0, 35.0}, :span, {3.0, 3.0})
    click(r, {43.0, 77.0}, :block, {3.0, 3.0})
    button(r, :press, {39.0, 35.0})
    no_float = fn -> document([], :moved) end
    send(p, {:scene, no_float})
    b = Headless.pixels()
    refute a == b
    button(r, :release, {7.0, 33.0})
    assert_receive {:application, {:moved, %{x: 3.0, y: 3.0}}}
    click(r, {43.0, 59.0}, :block, {3.0, 3.0})
    fresh(@size, no_float, b)
    send(p, {:scene, fn -> document([], :renamed) end})
    assert Headless.pixels() == b
    click(r, {7.0, 33.0}, :renamed, {3.0, 3.0})
    send(p, {:scene, start})
    assert Headless.pixels() == a
    click(r, {39.0, 35.0}, :span, {3.0, 3.0})
  end

  test "block defaults and explicit extents do not turn text_column into a height-sharing Column" do
    make = fn attrs ->
      column([width(px(100)), height(fill())], [
        text_column(attrs ++ [spacing(5), space_evenly(), Background.color(:black)], [
          block(),
          box([
            key(:wide),
            center_x(),
            center_y(),
            width(px(40)),
            height(px(12)),
            Background.color(:blue)
          ])
        ]),
        box([width(px(7)), height(px(2)), Background.color(:red)])
      ])
    end

    expected = fn w, h ->
      frame(@size, [
        {0, 0, w, h, 0x000000FF},
        {div(w - 20, 2), 0, 20, 10, 0xFFA500FF},
        {div(w - 40, 2), 15, 40, 12, 0x0000FFFF},
        {0, h, 7, 2, 0xFF0000FF}
      ])
    end

    {p, _} = Headless.mount(@size, fn -> make.([]) end)
    assert Headless.pixels() == expected.(100, 27)

    for {attrs, w, h} <- [
          {[width(px(82)), height(px(60))], 82, 60},
          {[width(content())], 40, 27},
          {[], 100, 27}
        ] do
      build = fn -> make.(attrs) end
      send(p, {:scene, build})
      bytes = Headless.pixels()
      assert bytes == expected.(w, h)
      fresh(@size, build, bytes)
    end
  end

  test "unbounded block fill keeps the visible part of its background and listener" do
    make = fn length ->
      text_column([width(px(90)), padding(3), Background.color(:blue)], [
        box([width(fill()), height(length), Background.color(:red), Event.on_click(:tall)])
      ])
    end

    {pid, r} = Headless.mount(@size, fn -> make.(px(400)) end, [:pointer_button])
    tall = Headless.pixels()
    assert tall == frame(@size, [{0, 0, 90, 406, 0x0000FFFF}, {3, 3, 84, 400, 0xFF0000FF}])

    for length <- [fill(), fill(3), max(px(30), fill()), min(px(400), fill()), px(400)] do
      build = fn -> make.(length) end
      send(pid, {:scene, build})
      assert Headless.pixels() == tall
      click(r, {7.0, 146.0}, :tall, {4.0, 143.0})
      fresh(@size, build, tall)
    end
  end

  test "an opaque later block paints and receives clicks over the earlier stacked float" do
    build = fn order ->
      f = side(:first_float, 34, 44, align_left())
      s = side(:stacked, 20, 32, align_left())

      p =
        paragraph(
          [
            key(:body),
            width(fill()),
            height(px(60)),
            Background.color(:blue),
            Event.on_click(:body)
          ],
          [text("ink")]
        )

      text_column(
        [width(px(90)), spacing(2)],
        if(order == :before, do: [f, s, p], else: [f, p, s])
      )
    end

    start = fn -> build.(:before) end
    {pid, r} = Headless.mount(@size, start, [:pointer_button])
    a = Headless.pixels()
    assert binary_part(a, 4 * (48 * 112 + 5), 4) == <<0, 0, 255, 255>>
    click(r, {5.0, 48.0}, :body, {5.0, 44.0})
    after_p = fn -> build.(:after) end
    send(pid, {:scene, after_p})
    b = Headless.pixels()
    click(r, {5.0, 68.0}, :stacked, {5.0, 4.0})
    fresh(@size, after_p, b)
    send(pid, {:scene, start})
    assert Headless.pixels() == a
  end
end
