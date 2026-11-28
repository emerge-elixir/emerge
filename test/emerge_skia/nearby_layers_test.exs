defmodule EmergeSkia.NearbyLayersTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.{BoxAssertions, Headless}

  defp panel(name, color, w, attrs), do: card(name, color, w, 20, attrs)

  defp menus(level, order) do
    tip = if level >= 2, do: [Nearby.in_front(panel(:tip, :green, 50, []))], else: []
    top = if level >= 4, do: [Nearby.in_front(panel(:top, :white, 32, []))], else: []
    scrim = if level >= 3, do: [Nearby.in_front(panel(:scrim, :black, 36, top))], else: []

    row(
      [width(px(100)), height(px(40)), Background.color(:yellow), Event.on_click(:paper)] ++ scrim,
      Enum.map(order, fn name ->
        menu = if name == :a, do: panel(:am, :red, 60, tip), else: panel(:bm, :blue, 60, [])
        el([key(name), width(px(20)), height(px(20)), Nearby.in_front(menu)], none())
      end)
    )
  end

  defp card(name, color, w, h, attrs) do
    listener = if name == :blocker, do: [], else: [Event.on_click(name)]

    el(
      [key(name), width(px(w)), height(px(h)), Background.color(color)] ++ attrs ++ listener,
      none()
    )
  end

  defp compass(dx) do
    host =
      el(
        [
          key(:host),
          width(px(40)),
          height(px(20)),
          center_x(),
          center_y(),
          padding(3),
          Transform.move_x(dx),
          Background.color(:black),
          Event.on_click(:host),
          Nearby.above(card(:above, :green, 12, 6, [center_x()])),
          Nearby.on_right(card(:right, :blue, 12, 6, [center_y()])),
          Nearby.below(card(:below, :white, 12, 6, [center_x()])),
          Nearby.on_left(card(:left, :magenta, 12, 6, [center_y()])),
          Nearby.in_front(card(:blocker, :white, 8, 5, [align_right(), align_bottom()])),
          Nearby.behind_content(card(:behind, :red, 20, 10, [align_right(), align_bottom()]))
        ],
        card(:child, :cyan, 6, 6, [align_left(), align_top()])
      )

    el([width(px(100)), height(px(80)), Background.color(:yellow), Event.on_click(:paper)], host)
  end

  test "all six mounts follow the padded host; the front without a handler blocks only its box" do
    {pid, r} = Headless.mount({100, 80}, fn -> compass(0) end, [:pointer_button])

    for dx <- [0, 6, -4] do
      build = fn -> compass(dx) end
      unless dx == 0, do: send(pid, {:scene, build})

      boxes = [
        {0, 0, 100, 80, 0xFFFF00FF},
        {30 + dx, 30, 40, 20, 0x000000FF},
        {50 + dx, 40, 20, 10, 0xFF0000FF},
        {33 + dx, 33, 6, 6, 0x00FFFFFF},
        {44 + dx, 24, 12, 6, 0x00FF00FF},
        {70 + dx, 37, 12, 6, 0x0000FFFF},
        {44 + dx, 50, 12, 6, 0xFFFFFFFF},
        {18 + dx, 37, 12, 6, 0xFF00FFFF},
        {62 + dx, 45, 8, 5, 0xFFFFFFFF}
      ]

      expected = BoxAssertions.frame({100, 80}, boxes)
      assert Headless.pixels() == expected
      assert {:ok, ^expected} = EmergeSkia.render_to_pixels(r)

      for {name, x, y} <- [
            {:above, 44, 24},
            {:right, 70, 37},
            {:below, 44, 50},
            {:left, 18, 37},
            {:behind, 50, 40},
            {:child, 33, 33},
            {:host, 30, 30}
          ] do
        BoxAssertions.click(r, {x + dx + 1.0, y + 1.0}, name, {1.0, 1.0})
      end

      for phase <- [:press, :release], do: BoxAssertions.button(r, phase, {dx + 64.0, 47.0})
      BoxAssertions.click(r, {95.0, 72.0}, :paper, {95.0, 72.0})
      refute_receive {:application, {_, %{x: _, y: _}}}, 20
    end
  end

  # Geometry oracle from the same ordinary declaration without a panel.
  defp phrase_id(%{key: :phrase, id: id}), do: id
  defp phrase_id(node), do: Enum.find_value(node.children, &phrase_id/1)

  defp phrase_box(t, {w, h}) do
    {wire, _, tree} = Emerge.Engine.encode_full(Emerge.Engine.diff_state_new(), t)
    id = phrase_id(tree)
    tree = EmergeSkia.Native.tree_new()
    assert {:ok, _} = EmergeSkia.Native.tree_upload(tree, wire)
    assert {:ok, frames} = EmergeSkia.Native.tree_layout(tree, w * 1.0, h * 1.0, 1.0)

    Enum.find_value(frames, fn {k, x, y, w, h} ->
      if Emerge.Engine.NodeId.decode(k) == id, do: {x, y, w, h}
    end)
  end

  defp sample(pixels, width, x, y), do: binary_part(pixels, 4 * (width * trunc(y) + trunc(x)), 4)

  defp export(t, {w, h}),
    do: EmergeSkia.TreeRenderer.render_to_pixels(t, [otp_app: :emerge, width: w, height: h], 5000)

  test "the menu spans all aligned lines; an uncovered continuation gap still hits the paper" do
    size = {160, 128}

    scene = fn w, mounted ->
      menu =
        el(
          [width(fill()), height(px(6)), Background.color(:green), Event.on_click(:menu)],
          none()
        )

      mounts = if mounted, do: [Nearby.below(menu)], else: []

      span =
        el(
          [key(:phrase), Background.color(:blue), Event.on_click(:phrase) | mounts],
          text("AA BBB CC DDDDD E F")
        )

      el(
        [width(fill()), height(fill()), Background.color(:white), Event.on_click(:paper)],
        paragraph(
          [
            width(px(w)),
            Font.size(12),
            Font.center(),
            Font.family(:monospace),
            Font.color(color_rgba(0, 0, 0, 0))
          ],
          [el([key(:prefix)], text("X ")), span, el([key(:suffix)], text(" ZZZZZ"))]
        )
      )
    end

    {v, r} = Headless.mount(size, fn -> scene.(150, true) end, [:pointer_button])

    for {w, i} <- Enum.with_index([150, 48, 150]) do
      if i > 0, do: send(v, {:scene, fn -> scene.(w, true) end})
      frame = Headless.pixels()
      assert {:ok, ^frame} = EmergeSkia.render_to_pixels(r)
      assert frame == export(scene.(w, true), size)
      twin = scene.(w, false)
      {x, y, width, h} = phrase_box(twin, size)
      assert sample(frame, 160, x + 2, y + h + 2) == <<0, 255, 0, 255>>
      assert sample(frame, 160, x + width - 2, y + h + 2) == <<0, 255, 0, 255>>
      BoxAssertions.click(r, {x + 2.0, y + h + 2.0}, :menu, {2.0, 2.0})

      if w == 48 do
        assert h >= 42
        plain = export(twin, size)

        {gx, gy} =
          Enum.find_value(ceil(y + 1)..floor(y + h - 1), fn row ->
            Enum.find_value(ceil(x + 1)..floor(x + width - 1), fn col ->
              if sample(plain, 160, col, row) == <<255, 255, 255, 255>>,
                do: {col + 0.5, row + 0.5}
            end)
          end)

        BoxAssertions.click(r, {gx, gy}, :paper, {gx, gy})
      end
    end
  end

  test "a scrolled inline escape keeps its owner's inverse inside the stationary port guard" do
    size = {120, 100}

    scene = fn guard, mounted ->
      menu =
        el(
          [width(fill()), height(px(32)), Background.color(:green), Event.on_click(:menu)],
          none()
        )

      mounts = if mounted, do: [Nearby.above(menu)], else: []
      span = el([key(:phrase), Transform.rotate(180), Transform.move_x(8) | mounts], text("AAA"))
      port = [width(px(100)), height(px(40)), scrollbar_y(), scroll_y(0.5)]

      content =
        column([width(px(100)), height(px(80))], [
          el([height(px(24))], none()),
          paragraph([width(fill()), Font.size(16)], [span])
        ])

      el(
        [width(fill()), height(fill()), Background.color(:yellow), Event.on_click(:paper)],
        el(if(guard, do: [clip_nearby() | port], else: port), content)
      )
    end

    {x, y, w, h} = phrase_box(scene.(false, false), size)
    point = {x + 8 + w - 3, y - 20 + h + 24}
    assert elem(point, 1) > 40
    {v, r} = Headless.mount(size, fn -> scene.(false, true) end, [:pointer_button])

    for {clip, i} <- Enum.with_index([false, true, false]) do
      if i > 0, do: send(v, {:scene, fn -> scene.(clip, true) end})
      frame = Headless.pixels()
      assert {:ok, ^frame} = EmergeSkia.render_to_pixels(r)
      assert frame == export(scene.(clip, true), size)

      assert sample(frame, 120, elem(point, 0), elem(point, 1)) == <<255, 255, 0, 255>>
      BoxAssertions.click(r, point, :paper, point)
      inside = {elem(point, 0), 38.0}
      assert sample(frame, 120, elem(inside, 0), 38) == <<0, 255, 0, 255>>
      for phase <- [:press, :release], do: BoxAssertions.button(r, phase, inside)
      assert_receive {:application, {:menu, %{x: lx, y: ly}}}
      assert_in_delta lx, 3, 0.001
      assert_in_delta ly, 8 + elem(point, 1) - 38, 0.001
    end
  end

  defp moving(site) do
    kept =
      el(
        [key(:kept), width(px(12)), height(px(8)), Background.color(:red), Event.on_click(site)],
        none()
      )

    a = if site == :front, do: [Nearby.in_front(kept)], else: []
    b = if site == :below, do: [Nearby.below(kept)], else: []

    row([width(px(80)), height(px(48)), Background.color(:white)], [
      el([key(:a), width(px(20)), height(px(20))] ++ a, none()),
      el(
        [key(:b), width(px(20)), height(px(20)), scale(1.5)] ++ b,
        if(site == :ordinary, do: kept, else: none())
      )
    ])
  end

  test "a held click follows a keyed panel across a new scaled anchor, slot and ordinary role" do
    size = {80, 48}
    frame = fn box -> BoxAssertions.frame(size, [{0, 0, 80, 48, 0xFFFFFFFF} | box]) end
    {pid, r} = Headless.mount(size, fn -> moving(:front) end, [:pointer_button])
    front = {0, 0, 12, 8, 0xFF0000FF}
    assert Headless.pixels() == frame.([front])

    Enum.reduce(
      [
        {:below, {20, 30, 18, 12, 0xFF0000FF}, {23.0, 33.0}},
        {:ordinary, {20, 0, 18, 12, 0xFF0000FF}, {23.0, 3.0}},
        {:front, front, {2.0, 2.0}}
      ],
      {2.0, 2.0},
      fn {site, box, release}, press ->
        BoxAssertions.button(r, :press, press)
        send(pid, {:scene, fn -> moving(site) end})
        assert Headless.pixels() == frame.([box])
        BoxAssertions.button(r, :release, release)
        assert_receive {:application, {^site, %{x: 2.0, y: 2.0}}}
        release
      end
    )

    BoxAssertions.button(r, :press, {2.0, 2.0})
    send(pid, {:scene, fn -> moving(:removed) end})
    assert Headless.pixels() == frame.([])
    send(pid, {:scene, fn -> moving(:front) end})
    assert Headless.pixels() == frame.([front])
    BoxAssertions.button(r, :release, {2.0, 2.0})
    refute_receive {:application, {:front, _}}, 30
    BoxAssertions.click(r, {2.0, 2.0}, :front, {2.0, 2.0})
  end

  test "sibling menus, a submenu and the root overlay are successive installed layers" do
    {pid, r} = Headless.mount({100, 40}, fn -> menus(4, [:a, :b]) end, [:pointer_button])

    for {level, order, color, target, local_x} <- [
          {4, [:a, :b], 0xFFFFFFFF, :top, 25},
          {3, [:a, :b], 0x000000FF, :scrim, 25},
          {2, [:a, :b], 0x00FF00FF, :tip, 25},
          {1, [:a, :b], 0x0000FFFF, :bm, 5},
          {1, [:b, :a], 0xFF0000FF, :am, 5},
          {2, [:b, :a], 0x00FF00FF, :tip, 5}
        ] do
      build = fn -> menus(level, order) end
      unless level == 4, do: send(pid, {:scene, build})
      pixels = Headless.pixels()
      assert binary_part(pixels, 4 * (100 * 8 + 25), 4) == <<color::32>>

      assert pixels ==
               EmergeSkia.TreeRenderer.render_to_pixels(
                 build.(),
                 [otp_app: :emerge, width: 100, height: 40],
                 5000
               )

      BoxAssertions.click(r, {25.0, 8.0}, target, {local_x * 1.0, 8.0})
      BoxAssertions.click(r, {95.0, 8.0}, :paper, {95.0, 8.0})
    end
  end
end
