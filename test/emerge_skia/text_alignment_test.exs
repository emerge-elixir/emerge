defmodule EmergeSkia.TextAlignmentTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions, only: [button: 3, click: 4, fresh: 3]
  alias Emerge.Engine.Element
  alias Emerge.TestSupport.Headless

  @size {160, 64}
  defp label(content, attrs) do
    # text/1 has no attrs; a Text can also be declared directly.
    %Element{type: :text, key: :label, attrs: Map.put(Map.new(attrs), :content, content)}
  end

  defp panel(alignment, override, route) do
    el(
      [
        width(px(100)),
        height(px(40)),
        padding(4),
        Font.color(:white),
        Event.on_click(:panel) | alignment
      ],
      label("Hi", [Background.color(:blue), Event.on_click(route) | override])
    )
  end

  defp white_bounds(pixels) do
    {w, h} = @size

    points =
      for y <- 0..(h - 1),
          x <- 0..(w - 1),
          binary_part(pixels, 4 * (y * w + x), 4) == <<255, 255, 255, 255>>,
          do: {x, y}

    {xs, ys} = Enum.unzip(points)
    {Enum.min(xs), Enum.min(ys), Enum.max(xs) - Enum.min(xs) + 1, Enum.max(ys) - Enum.min(ys) + 1}
  end

  test "inherited ink moves within the submitted box; explicit left overrides and removal inherits again" do
    {p, r} = Headless.mount(@size, fn -> panel([], [], :text) end, [:pointer_button])
    left = Headless.pixels()
    assert white_bounds(left) == {6, 8, 8, 12}
    click(r, {90.0, 10.0}, :panel, {90.0, 10.0})

    for {align, own, x, route} <- [
          {[Font.center()], [], 44, :text},
          {[Font.align_right()], [], 82, :text},
          {[Font.align_right()], [Font.align_left()], 6, :text},
          {[Font.align_right()], [], 82, :text},
          {[Font.align_right()], [], 82, :renamed},
          {[], [], 6, :text}
        ] do
      button(r, :press, {10.0, 10.0})
      build = fn -> panel(align, own, route) end
      send(p, {:scene, build})
      bytes = Headless.pixels()
      assert white_bounds(bytes) == {x, 8, 8, 12}
      button(r, :release, {10.0, 10.0})
      assert_receive {:application, {^route, %{x: 6.0, y: 6.0}}}

      if x == 6 do
        assert bytes == left
        click(r, {90.0, 10.0}, :panel, {90.0, 10.0})
      else
        click(r, {90.0, 10.0}, route, {86.0, 6.0})
      end

      fresh(@size, build, bytes)
    end
  end

  test "explicit widths, asymmetric insets and a leading overhang use visual run metrics" do
    scene = fn align, w, right ->
      el(
        [width(px(100)), height(px(40)), padding(4), Font.color(:white), align],
        label("jA", [width(w), padding_each(2, right, 1, 3), Background.color(:blue)])
      )
    end

    {p, _} = Headless.mount(@size, fn -> scene.(Font.center(), px(58), 11) end)
    initial = Headless.pixels()
    assert white_bounds(initial) == {27, 10, 9, 12}

    for {align, w, right, expected} <- [
          {Font.align_right(), px(58), 11, {41, 10, 9, 12}},
          {Font.align_right(), px(58), 7, {45, 10, 9, 12}},
          {Font.center(), px(10), 11, {5, 10, 7, 11}},
          {Font.align_right(), content(), 11, {13, 10, 9, 12}},
          {Font.center(), px(58), 11, {27, 10, 9, 12}}
        ] do
      build = fn -> scene.(align, w, right) end
      send(p, {:scene, build})
      bytes = Headless.pixels()
      assert white_bounds(bytes) == expected
      fresh(@size, build, bytes)
      if align == Font.center() and w == px(58), do: assert(bytes == initial)
    end
  end

  test "aligning an un-sized line does not give it a Row share or stretch a content-sized El" do
    for host <- [:row, :wrapped_row, :content_el] do
      scene = fn align ->
        attrs = [
          width(px(100)),
          height(px(40)),
          padding(4),
          spacing(4),
          Font.color(:white),
          align
        ]

        leaf = label("Hi", [Event.on_click(:label)])
        peer = el([key(:peer), width(px(20)), height(px(12)), Background.color(:blue)], none())

        case host do
          :row -> row(attrs, [leaf, peer])
          :wrapped_row -> wrapped_row(attrs, [leaf, peer])
          :content_el -> el([width(content()), padding(4), Font.color(:white), align], leaf)
        end
      end

      {p, r} = Headless.mount(@size, fn -> scene.(Font.align_left()) end, [:pointer_button])
      initial = Headless.pixels()
      assert white_bounds(initial) == {6, 8, 8, 12}

      if host != :content_el,
        do: assert(binary_part(initial, 4 * (8 * 160 + 30), 4) == <<0, 0, 255, 255>>)

      for align <- [Font.center(), Font.align_right(), Font.align_left()] do
        send(p, {:scene, fn -> scene.(align) end})
        assert Headless.pixels() == initial
        click(r, {8.0, 8.0}, :label, {4.0, 4.0})
      end
    end
  end

  test "right alignment scales the run and both insets, including the undeclared 16px default" do
    scene = fn font ->
      el(
        [
          width(px(100)),
          height(px(40)),
          padding(4),
          scale(1.5),
          Font.color(:white),
          Font.align_right()
        ],
        label("jA", [
          width(px(58)),
          padding_each(2, 11, 1, 3),
          Background.color(:blue),
          Event.on_click(:leaf) | font
        ])
      )
    end

    {p, r} = Headless.mount(@size, fn -> scene.([]) end, [:pointer_button])
    initial = Headless.pixels()
    assert white_bounds(initial) == {55, 15, 20, 21}
    build = fn -> scene.([Font.size(16)]) end
    send(p, {:scene, build})
    assert Headless.pixels() == initial
    click(r, {81.0, 15.0}, :leaf, {50.0, 6.0})
    fresh(@size, build, initial)
  end

  test "aligning auto Text changes a packed content cap before its Row divides the width" do
    scene = fn align ->
      labels =
        wrapped_column(
          [key(:pack), width(min(content(), fill())), height(px(24)), spacing(4), align],
          [
            label("Hi", [Background.color(:blue), Event.on_click(:text)]),
            el([key(:card), width(px(30)), height(px(20))], none())
          ]
        )

      peer =
        el(
          [
            key(:peer),
            width(fill()),
            height(px(24)),
            Background.color(:red),
            Event.on_click(:peer)
          ],
          none()
        )

      row([width(px(120)), height(px(24)), Font.color(:white)], [labels, peer])
    end

    {p, r} = Headless.mount(@size, fn -> scene.(Font.align_left()) end, [:pointer_button])
    left = Headless.pixels()
    point = {54.0, 2.0}
    offset = 4 * (2 * 160 + 54)
    assert binary_part(left, offset, 4) == <<255, 0, 0, 255>>
    click(r, point, :peer, {4.234375, 2.0})

    for align <- [Font.center(), Font.align_right(), Font.align_left()] do
      build = fn -> scene.(align) end
      send(p, {:scene, build})
      bytes = Headless.pixels()

      if align == Font.align_left() do
        assert bytes == left
        click(r, point, :peer, {4.234375, 2.0})
      else
        assert binary_part(bytes, offset, 4) == <<0, 0, 255, 255>>
        click(r, point, :text, point)
      end

      fresh(@size, build, bytes)
    end
  end

  test "a kept label moves between font contexts without moving its ink or hit to the other occurrence" do
    scene = fn side, peer ->
      kept = el([key(:kept), width(fill())], label("Hi", [Event.on_click(:kept)]))
      fixed = fn -> el([width(px(peer)), Font.align_left()], text("Hi")) end

      panels =
        for {id, w, align} <- [{:a, 60, Font.center()}, {:b, 100, Font.align_right()}] do
          el(
            [key(id), width(px(w)), height(px(32)), align],
            if(id == side, do: kept, else: fixed.())
          )
        end

      row([width(px(160)), Font.color(:white)], panels)
    end

    {p, r} = Headless.mount(@size, fn -> scene.(:a, 20) end, [:pointer_button])
    a = Headless.pixels()
    button(r, :press, {10.0, 10.0})
    build = fn -> scene.(:b, 20) end
    send(p, {:scene, build})
    b = Headless.pixels()
    refute a == b
    button(r, :release, {90.0, 10.0})
    assert_receive {:application, {:kept, %{x: 30.0, y: 10.0}}}
    fresh(@size, build, b)
    send(p, {:scene, fn -> scene.(:b, 28) end})
    assert Headless.pixels() == b
    send(p, {:scene, fn -> scene.(:a, 20) end})
    assert Headless.pixels() == a
    click(r, {10.0, 10.0}, :kept, {10.0, 10.0})
  end
end
