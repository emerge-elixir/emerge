defmodule EmergeSkia.StaticSizeTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.{BoxAssertions, Headless}

  @red <<255, 0, 0, 255>>
  @blue <<0, 0, 255, 255>>
  @clear <<0, 0, 0, 0>>
  @long "AA BB CC DD EE"
  defp axes(w, h), do: [width(px(w)), height(px(h))]
  defp chip(attrs, color), do: el([Background.color(color) | attrs], none())
  defp point(frame, w, x, y), do: binary_part(frame, 4 * (y * w + x), 4)
  defp art(attrs), do: image([height(fill()) | attrs], "test_assets/single_axis_wide.png")
  defp words(s), do: paragraph([width(px(48))], [text(s)])
  defp marker(w), do: chip([width(w), height(px(8)), Event.on_click(:peer)], :blue)

  test "a document block and its floating replacement install different local hits" do
    scene = fn h, side ->
      fn ->
        text_column([Event.on_click(:doc) | axes(160, 80)], [
          chip([rotate(90), width(px(24)), height(h), Event.on_click(:card) | side], :red),
          marker(px(8))
        ])
      end
    end

    {pid, r} = Headless.mount({160, 80}, scene.(px(24), []), [:pointer_button])

    for {build, x, target, local} <- [
          {nil, 0, :doc, {40.0, 6.0}},
          {scene.(min(px(80), fill()), []), 0, :card, {6.0, 40.0}},
          {scene.(fill(), [align_right()]), 136, :doc, {40.0, 6.0}},
          {scene.(fill(), [align_left()]), 0, :doc, {40.0, 6.0}},
          {scene.(fill(), []), nil, :doc, {40.0, 6.0}},
          {scene.(min(px(24), fill()), []), 0, :doc, {40.0, 6.0}}
        ] do
      if build, do: send(pid, {:scene, build})
      frame = Headless.pixels()

      if x,
        do: assert(point(frame, 160, x + 2, 2) == @red),
        else: assert(point(frame, 160, 2, 2) == @clear)

      assert point(frame, 160, 2, 26) == @blue
      # The enormous bare block has no sampled ink. Only assert inverse
      # geometry at finite boxes; absence of ink alone isn't a hit mask.
      if x, do: BoxAssertions.click(r, {40.0, 6.0}, target, local)
      if x == 136, do: BoxAssertions.click(r, {140.0, 6.0}, :card, {6.0, 20.0})
    end
  end

  test "a measured Image float excludes the phrase by its allocated width, not its search demand" do
    build = fn side ->
      fn ->
        paragraph(axes(160, 80), [
          image(
            [side, height(min(px(24), fill())), Event.on_click(:art)],
            "test_assets/single_axis_wide.png"
          ),
          el([Background.color(:blue), Event.on_click(:phrase)], text("M M MMMM"))
        ])
      end
    end

    {pid, r} =
      Headless.mount({160, 80}, build.(align_left()), [:pointer_button], decode_at_size: false)

    for {scene, x, at} <- [
          {nil, 48, 4.0},
          {build.(align_right()), 0, 116.0},
          {build.(align_left()), 48, 4.0}
        ] do
      if scene, do: send(pid, {:scene, scene})

      Headless.pixels_matching(
        &(point(&1, 160, x, 2) == @blue && point(&1, 160, trunc(at), 4) == @red)
      )

      BoxAssertions.click(r, {at, 4.0}, :art, {4.0, 4.0})
      BoxAssertions.click(r, {x + 2.0, 3.0}, :phrase, {2.0, 3.0})
    end
  end

  test "inline declarations contribute preferred demand, not a substitute for a word fragment" do
    build = fn width_attr, dims ->
      fn ->
        paragraph([width(width_attr)], [
          el(
            [padding(2), Background.color(:blue), Event.on_click(:span) | dims],
            text("AA BB CC")
          )
        ])
      end
    end

    [first | edits] =
      for w <- [px(64), min(px(64), content()), content()],
          dims <- [[], axes(90, 2), [width(fill()), height(fill())]],
          do: build.(w, dims)

    {pid, r} = Headless.mount({180, 120}, first, [:pointer_button])
    initial = Headless.pixels()

    frames = [
      initial
      | Enum.map(edits, fn scene ->
          send(pid, {:scene, scene})
          Headless.pixels()
        end)
    ]

    BoxAssertions.click(r, {3.0, 3.0}, :span, {3.0, 3.0})

    assert [a, a, a, a, a, a, b, c, b] = Enum.map(frames, &:crypto.hash(:sha256, &1))
    refute a == b
    refute b == c

    assert Enum.map(frames, &point(&1, 180, 1, 26)) ==
             List.duplicate(@blue, 6) ++ List.duplicate(@clear, 3)
  end

  test "natural Row height and explicit height offer different widths to loaded art" do
    build = fn host ->
      fn -> row([width(px(300)) | host], [chip(axes(8, 20), :green), art([]), marker(fill())]) end
    end

    {pid, r} = Headless.mount({300, 120}, build.([]), [:pointer_button], decode_at_size: false)

    for {edit, x} <- [{nil, 208}, {build.([height(px(40))]), 88}, {build.([]), 208}] do
      if edit, do: send(pid, {:scene, edit})

      Headless.pixels_matching(
        &(point(&1, 300, x, 3) == @blue && point(&1, 300, x - 1, 3) != @blue)
      )

      BoxAssertions.click(r, {212.0, 3.0}, :peer, {212.0 - x, 3.0})
    end
  end

  test "an outer Column assigns both nested height shares before the Row allocates its peer" do
    scene = fn h ->
      fn ->
        header = fn h -> chip(axes(8, h), :green) end
        inner = column([height(fill()), width(content())], [header.(8), art([])])

        row(axes(320, 165), [
          column([width(content()), height(px(144))], [header.(h), inner]),
          marker(fill())
        ])
      end
    end

    {pid, r} = Headless.mount({320, 165}, scene.(8), [:pointer_button], decode_at_size: false)

    for {edit, edge} <- [{nil, 256}, {scene.(24), 224}, {scene.(8), 256}] do
      if edit, do: send(pid, {:scene, edit})

      Headless.pixels_matching(
        &(point(&1, 320, edge - 1, 3) != @blue && point(&1, 320, edge, 3) == @blue)
      )

      BoxAssertions.click(r, {260.0, 3.0}, :peer, {260.0 - edge, 3.0})
    end
  end

  test "content width retains the Image minimum while height and the clip follow the header fit" do
    scene = fn s ->
      fn ->
        row(axes(320, 165), [
          column([width(content()), height(px(100))], [words(s), art([Event.on_click(:art)])]),
          marker(fill())
        ])
      end
    end

    {pid, r} =
      Headless.mount({320, 165}, scene.("AA BB"), [:pointer_button], decode_at_size: false)

    {_, line, _, _} = EmergeSkia.measure_text("AA", 16)

    for {edit, lines, last} <- [{nil, 1, 160}, {scene.(@long), 3, 82}, {scene.("AA BB"), 1, 160}] do
      if edit, do: send(pid, {:scene, edit})

      Headless.pixels_matching(
        &(point(&1, 320, 200, 3) == @blue && point(&1, 320, 199, 3) != @blue &&
            point(&1, 320, last, 70) == @red && point(&1, 320, last + 4, 70) == @clear)
      )

      BoxAssertions.click(r, {201.0, 3.0}, :peer, {1.0, 3.0})
      for phase <- [:press, :release], do: BoxAssertions.button(r, phase, {4.0, 74.0})
      assert_receive {:application, {:art, %{x: 4.0, y: y}}}
      assert_in_delta y, 74 - lines * line, 0.0001
    end
  end

  test "a turned cap uses intrinsic height when its own Image would change the line count" do
    scene = fn cap, header ->
      fn ->
        inner =
          row([width(fill()), height(fill())], [
            art([]),
            paragraph([width(fill())], [text(String.duplicate("AA ", 20))])
          ])

        card = el([width(px(200)), height(min(content(), fill())), rotate(90)], inner)

        column(axes(280, cap), [
          words(header),
          row([width(px(280)), height(fill())], [card, marker(fill())])
        ])
      end
    end

    {pid, r} =
      Headless.mount({300, 240}, scene.(180, "AA BB"), [:pointer_button], decode_at_size: false)

    {_, line, _, _} = EmergeSkia.measure_text("AA", 16)

    for {build, cap, lines} <- [
          {nil, 180, 1},
          {scene.(80, "AA BB"), 80, 1},
          {scene.(80, @long), 80, 3},
          {scene.(180, @long), 180, 3}
        ] do
      if build, do: send(pid, {:scene, build})
      top = lines * line
      x = Kernel.min(cap - top, 100.0)
      y = ceil(top) + 3

      Headless.pixels_matching(
        &(point(&1, 300, trunc(x) - 2, y) != @blue && point(&1, 300, ceil(x) + 1, y) == @blue)
      )

      BoxAssertions.click(r, {164.0, top + 3.0}, :peer, {164.0 - x, 3.0})
    end
  end

  test "the horizontal and vertical pack use settled non-fill charges at their break" do
    short = "AA BB"

    for {scene, steps} <- [
          {fn w ->
             fn ->
               wrapped_row(axes(w, 120), [
                 el([rotate(90), Background.color(:red)], words(@long)),
                 marker(px(8))
               ])
             end
           end, [{66, 0, 48}, {67, 59, 0}, {66, 0, 48}]},
          {fn s ->
             fn ->
               row(axes(160, 120), [
                 wrapped_column([width(content()), height(px(60)), spacing_xy(6, 4)], [
                   words(s),
                   chip(axes(20, 20), :red)
                 ]),
                 marker(fill())
               ])
             end
           end, [{short, 48, 0}, {@long, 74, 0}, {short, 48, 0}]}
        ] do
      [{first, _, _} | _] = steps
      {pid, _} = Headless.mount({160, 120}, scene.(first))

      for {{attr, x, y}, i} <- Enum.with_index(steps) do
        if i != 0, do: send(pid, {:scene, scene.(attr)})
        frame = Headless.pixels()
        assert point(frame, 160, x, y + 2) == @blue
        if x > 0, do: refute(point(frame, 160, x - 1, y + 2) == @blue)
      end
    end
  end
end
