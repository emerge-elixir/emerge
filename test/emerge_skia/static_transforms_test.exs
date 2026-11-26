defmodule EmergeSkia.StaticTransformsTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.{BoxAssertions, Headless}
  alias EmergeSkia.TreeRenderer

  defp axes(w, h), do: [width(px(w)), height(px(h))]
  defp chip(attrs, fill), do: el([Background.color(fill) | attrs], none())

  defp pixels(tree, w, h, z \\ 1),
    do:
      TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: w, height: h, scale: z], 5000)

  defp pixel(data, w, x, y), do: binary_part(data, 4 * (w * y + x), 4)

  test "turning a container changes the installed local point; only a layout turn moves its peer" do
    scene = fn attr, label ->
      row(axes(100, 80), [
        el(
          [attr | axes(60, 20)],
          chip([width(fill()), height(fill()), Event.on_click(label)], :red)
        ),
        chip([Event.on_click(:marker) | axes(8, 8)], :blue)
      ])
    end

    paint = fn -> scene.(Transform.rotate(90), :paint) end
    layout = fn -> scene.(rotate(90), :layout) end
    red = 0xFF0000FF
    blue = 0x0000FFFF
    {pid, r} = Headless.mount({100, 80}, paint, [:pointer_button])

    for {build, target, boxes, at, local} <- [
          {nil, :paint, [{20, 0, 20, 40, red}, {60, 0, 8, 8, blue}], {30.0, 2.0}, {22.0, 10.0}},
          {layout, :layout, [{0, 0, 20, 60, red}, {20, 0, 8, 8, blue}], {6.0, 55.0},
           {55.0, 14.0}},
          {paint, :paint, [{20, 0, 20, 40, red}, {60, 0, 8, 8, blue}], {30.0, 2.0}, {22.0, 10.0}}
        ] do
      if build, do: send(pid, {:scene, build})
      expected = BoxAssertions.frame({100, 80}, boxes)
      assert Headless.pixels() == expected
      BoxAssertions.click(r, at, target, local)
      if build, do: BoxAssertions.fresh({100, 80}, build, expected)
    end
  end

  test "fractional asymmetric insets align the reserved box, and snap flags do not change metrics" do
    scene = fn attrs, card ->
      el(
        [
          align_right(),
          align_bottom(),
          padding_each(10, 20, 30, 40),
          Border.width_each(1, 2, 3, 4) | axes(200, 120)
        ] ++ attrs,
        el([Background.color(:red) | axes(50, 20)] ++ card, text("Hi"))
      )
    end

    for z <- [1, 1.5] do
      {w, h} = {round(200 * z), round(120 * z)}
      plain = pixels(scene.([], []), w, h, z)

      assert pixels(scene.([{:snap_layout, true}, {:snap_text_metrics, true}], []), w, h, z) ==
               plain

      assert pixel(plain, w, ceil(128 * z), ceil(67 * z)) == <<255, 0, 0, 255>>
      assert pixel(plain, w, ceil(128 * z) - 1, ceil(67 * z)) == <<0::32>>
      turned = pixels(scene.([], [rotate(90)]), w, h, z)
      assert pixel(turned, w, ceil(158 * z), ceil(37 * z)) == <<255, 0, 0, 255>>
      assert pixel(turned, w, ceil(158 * z) - 1, ceil(37 * z)) == <<0::32>>
    end
  end

  test "even gaps size a bounded fill against the layout-rotated charge, not the painted width" do
    row = fn turn ->
      row([space_evenly(), spacing(8) | axes(100, 60)], [
        chip([turn | axes(60, 20)], :red),
        chip([width(min(px(30), fill())), height(px(8))], :blue),
        chip(axes(10, 8), :green)
      ])
    end

    # 20 + 30 + 10 occupied, two 20px gaps; Transform.rotate keeps the 60px slot.
    for {attr, x} <- [{rotate(90), 40}, {Transform.rotate(90), 60}] do
      b = pixels(row.(attr), 100, 60)
      assert pixel(b, 100, x, 2) == <<0, 0, 255, 255>>
      assert pixel(b, 100, x + 29, 2) == <<0, 0, 255, 255>>
      refute pixel(b, 100, x - 1, 2) == <<0, 0, 255, 255>>
    end
  end

  test "a Row wraps a turned fixed-width paragraph before sharing the remaining width" do
    size = {160, 120}
    long = "AA BB CC DD EE"
    {_, line, _, _} = EmergeSkia.measure_text("AA", 16)

    scene = fn words, height_attrs ->
      fn ->
        row(axes(160, 120), [
          el(
            [
              width(px(48)),
              rotate(90),
              Background.color(:red),
              Event.on_click(:card) | height_attrs
            ],
            paragraph([width(px(48))], [text(words)])
          ),
          chip([width(fill()), height(px(8)), Event.on_click(:peer)], :blue)
        ])
      end
    end

    {pid, r} = Headless.mount(size, scene.(long, []), [:pointer_button])
    first = Headless.pixels()
    blue = <<0, 0, 255, 255>>

    for {build, edge, x} <- [
          {nil, 3 * line, 59},
          {scene.("AA BB", []), line, ceil(line)},
          {scene.(long, [height(px(20))]), 20, 20},
          {scene.(long, []), 3 * line, 59}
        ] do
      if build, do: send(pid, {:scene, build})
      frame = if build, do: Headless.pixels(), else: first
      # x is the first fully blue pixel, not the fractional layout edge.
      refute pixel(frame, 160, x - 1, 2) == blue
      assert pixel(frame, 160, x, 2) == blue
      assert pixel(frame, 160, 159, 2) == blue

      for {target, at, {lx, ly}} <- [
            {:card, {4.0, 12.0}, {12.0, edge - 4}},
            {:peer, {64.0, 2.0}, {64 - edge, 2.0}}
          ] do
        for phase <- [:press, :release], do: BoxAssertions.button(r, phase, at)
        assert_receive {:application, {^target, %{x: actual_x, y: actual_y}}}
        assert_in_delta actual_x, lx, 0.0001
        assert_in_delta actual_y, ly, 0.0001
      end

      if build, do: BoxAssertions.fresh(size, build, frame)
      if x == 59, do: assert(frame == first)
    end
  end

  test "a fill share is local: Row advances by an assigned width, Column by the occupied height" do
    blue = <<0, 0, 255, 255>>
    red = <<255, 0, 0, 255>>

    for {host, expected_x} <- [{width(px(100)), 92}, {width(content()), 20}] do
      card =
        el(
          [width(fill()), height(px(20)), rotate(90), Background.color(:red)],
          chip(axes(40, 8), :red)
        )

      b = pixels(row([host, height(px(100))], [card, chip(axes(8, 8), :blue)]), 100, 100)
      assert pixel(b, 100, expected_x, 2) == blue
    end

    # All 92px are allocated before computing a gap/centering the group. The
    # turned box is 92px wide, but the next child starts below its 20px height.
    for {host, zone, start, peer} <- [
          {[], [], 0, 20},
          {[space_evenly()], [], 0, 20},
          {[], [center_y()], 0, 20},
          {[], [align_bottom()], 72, 92}
        ] do
      card = chip([width(px(20)), height(fill()), rotate(90) | zone], :red)

      b =
        pixels(column(axes(100, 100) ++ host, [card, chip(axes(8, 8) ++ zone, :blue)]), 100, 100)

      assert pixel(b, 100, 91, start) == red
      assert pixel(b, 100, 0, peer) == blue
    end
  end

  test "group alpha on Row composites once and a transparent Column keeps its listener" do
    fg = fn alpha ->
      row([Transform.alpha(alpha), Background.color(:red) | axes(50, 16)], [
        chip(axes(30, 16), :blue)
      ])
    end

    assert pixel(pixels(fg.(0.5), 50, 16), 50, 4, 4) == <<0, 0, 128, 128>>
    assert pixel(pixels(fg.(0.5), 50, 16), 50, 36, 4) == <<128, 0, 0, 128>>

    {_, r} =
      Headless.mount(
        {50, 16},
        fn -> column([Transform.alpha(0), Event.on_click(:clear) | axes(50, 16)], [fg.(0.5)]) end,
        [:pointer_button]
      )

    assert Headless.pixels() == :binary.copy(<<0::32>>, 50 * 16)
    BoxAssertions.click(r, {10.0, 8.0}, :clear, {10.0, 8.0})
  end

  test "the outer shadow turns with the box without acquiring its ordinary host's clip" do
    shadow = Border.shadow(color: :red, offset: {30, 0}, blur: 0)

    for {alpha, red} <- [{1, <<255, 0, 0, 255>>}, {0.5, <<128, 0, 0, 128>>}] do
      frame = fn pose ->
        row(axes(24, 24), [el([Transform.alpha(alpha), shadow | axes(20, 20)] ++ pose, none())])
        |> pixels(60, 60)
      end

      plain = frame.([])
      assert pixel(plain, 60, 32, 8) == red
      assert frame.([Transform.scale(-1), Transform.rotate(180)]) == plain
      turned = frame.([Transform.rotate(90)])
      assert pixel(turned, 60, 8, 32) == red
      assert pixel(turned, 60, 32, 8) == <<0::32>>
    end
  end

  test "SVG repeat bounds use both ancestor scales; a cover still fits" do
    src = "test_assets/single_axis_wide.svg"

    frame = fn name, fit, zoom ->
      el(
        [Transform.scale(zoom) | axes(16, 16)],
        el([Transform.scale(-4), Background.image(name, fit: fit) | axes(16, 16)], none())
      )
      |> pixels(16, 16)
    end

    red = :binary.copy(<<255, 0, 0, 255>>, 16 * 16)
    assert frame.(src, :repeat, 1) == red
    assert frame.(src, :repeat, 8) == frame.("absent.svg", :repeat, 8)
    refute frame.(src, :repeat, 8) == red
    assert frame.(src, :cover, 8) == red
  end

  test "paragraph continuations inherit a turned host and move without rebreaking the line" do
    scene = fn move ->
      paragraph([padding(6), Font.size(16), rotate(90) | axes(80, 64)], [
        el(
          [
            Transform.move_y(move),
            Transform.scale(-1),
            Background.color(:red),
            Event.on_click(:span)
          ],
          text("MMMM MMMM")
        )
      ])
    end

    {pid, r} = Headless.mount({100, 100}, fn -> scene.(0) end, [:pointer_button])
    first = Headless.pixels()

    for {dy, frame} <- [{0, first}, {3, nil}, {0, nil}] do
      if frame == nil, do: send(pid, {:scene, fn -> scene.(dy) end})
      b = frame || Headless.pixels()
      assert b == pixels(scene.(dy), 100, 100)
      # The first solid interior point must route through both inverse transforms.
      i = Enum.find(0..9999, fn i -> binary_part(b, 4 * i, 4) == <<255, 0, 0, 255>> end)
      assert is_integer(i)
      pos = {rem(i, 100) + 0.5, div(i, 100) + 0.5}
      for phase <- [:press, :release], do: BoxAssertions.button(r, phase, pos)
      assert_receive {:application, {:span, %{x: x, y: y}}}
      assert x >= 0 and y >= 0
      if dy == 0, do: assert(b == first), else: refute(b == first)
    end
  end
end
