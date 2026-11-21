defmodule EmergeSkia.RowCrossHeightTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp cell(k, policy, child) do
    el(
      [
        key(k),
        width(px(12)),
        height(policy),
        Background.color(if(k == :a, do: :red, else: :blue)),
        Event.on_click(k)
      ],
      child
    )
  end

  defp line(a, demand, b) do
    row(
      [
        width(px(40)),
        height(px(120)),
        padding(6),
        spacing(4),
        Background.color(:white),
        Event.on_click(:row)
      ],
      [
        cell(:a, a, el([height(px(demand))], none())),
        cell(:b, b, none())
      ]
    )
  end

  defp frame(a, viewport \\ 120) do
    for y <- 0..(viewport - 1), x <- 0..39, into: <<>> do
      cond do
        y >= 120 -> <<0, 0, 0, 255>>
        x in 6..17 and y >= 6 and y < 6 + a -> <<255, 0, 0, 255>>
        x in 22..33 and y in 6..113 -> <<0, 0, 255, 255>>
        true -> <<255, 255, 255, 255>>
      end
    end
  end

  defp button(r, phase, {x, y}) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, point, target, {lx, ly}) do
    for phase <- [:press, :release], do: button(r, phase, point)

    assert_receive {:application, {^target, %{x: ^lx, y: ^ly}}}
  end

  test "fixed-height control leaves space below A, not B" do
    {_, r} = Headless.mount({40, 120}, fn -> line(px(30), 30, px(108)) end, [:pointer_button])
    assert Headless.pixels() == frame(30)
    click(r, {10.0, 76.0}, :row, {10.0, 76.0})
    click(r, {26.0, 76.0}, :b, {4.0, 70.0})
  end

  test "plain and weighted fills each use the whole Row height, not a peer share" do
    expected = frame(108)

    for a <- [fill(), fill(1), fill(3)] do
      {_, r} = Headless.mount({40, 120}, fn -> line(a, 30, fill()) end, [:pointer_button])
      assert Headless.pixels() == expected
      click(r, {10.0, 110.0}, :a, {4.0, 104.0})
      click(r, {26.0, 110.0}, :b, {4.0, 104.0})
    end
  end

  for start <- [:two_fills, :fixed_to_fill] do
    test "#{start}: a growing content cap moves the raw click only after submission" do
      cap = min(content(), fill(3))
      {a, b} = if unquote(start) == :two_fills, do: {cap, fill()}, else: {px(30), px(108)}

      {pid, r} =
        Headless.mount({40, 120}, fn -> line(a, 30, b) end, [:pointer_button], target_fps: 1)

      small = frame(30)
      assert Headless.pixels() == small

      send(
        pid,
        {:scene,
         fn ->
           send(self(), :built)
           line(cap, 90, fill())
         end}
      )

      assert_receive {:application, :built}
      assert {:ok, ^small} = EmergeSkia.render_to_pixels(r)
      click(r, {10.0, 76.0}, :row, {10.0, 76.0})
      click(r, {26.0, 76.0}, :b, {4.0, 70.0})
      assert Headless.pixels(2500) == frame(90)
      click(r, {10.0, 76.0}, :a, {4.0, 70.0})
      click(r, {26.0, 76.0}, :b, {4.0, 70.0})
      send(pid, {:scene, fn -> line(cap, 150, fill()) end})
      assert Headless.pixels(2500) == frame(108)
      click(r, {10.0, 117.0}, :row, {10.0, 117.0})
    end
  end

  defp move_scene(site, limit \\ nil) do
    policy = max(content(), fill(3))

    card =
      el(
        [
          key(:card),
          width(fill()),
          height(if(limit, do: min(px(limit), policy), else: policy)),
          Background.color(:red),
          Event.on_click(site)
        ],
        el([height(px(30))], none())
      )

    peer = cell(:b, fill(), none())

    second =
      case site do
        :row -> [card]
        :wrap -> [wrapped_row([key(:wrap)], [card])]
        :column -> []
      end

    row([width(px(24)), height(px(120)), Background.color(:white)], [
      column(
        [key(:column), width(px(12)), height(px(120))],
        if(site == :column, do: [card, peer], else: [peer])
      ),
      row([key(:row), width(px(6)), height(px(54)), scale(2)], second)
    ])
  end

  test "a press survives a rejected move and follows the card's new height, route and scale" do
    {pid, r} =
      Headless.mount({24, 120}, fn -> move_scene(:column) end, [:pointer_button], target_fps: 1)

    line = fn left, right, n ->
      :binary.copy(:binary.copy(left, 12) <> :binary.copy(right, 12), n)
    end

    {red, blue, white} = {<<255, 0, 0, 255>>, <<0, 0, 255, 255>>, <<255, 255, 255, 255>>}
    shared = line.(red, white, 90) <> line.(blue, white, 30)
    independent = line.(blue, red, 108) <> line.(blue, white, 12)
    assert Headless.pixels() == shared
    button(r, :press, {5.0, 85.0})

    send(
      pid,
      {:scene,
       fn ->
         send(self(), :moved)
         move_scene(:row)
       end}
    )

    assert_receive {:application, :moved}
    # A masked length valid at scale 1 is out of range at the destination's
    # scale 2. Its rejection must not cancel the press or the pending move.
    {_, pending} =
      Emerge.Engine.diff_state_update_binary(
        Emerge.Engine.diff_state_new(move_scene(:column)),
        move_scene(:row)
      )

    assert_raise RuntimeError,
                 ~r/renderer_patch failed: scaled length exceeds device range/,
                 fn ->
                   EmergeSkia.patch_tree(r, pending, move_scene(:wrap, 2.0e38))
                 end

    assert {:ok, ^shared} = EmergeSkia.render_to_pixels(r)
    assert Headless.pixels(2500) == independent
    button(r, :release, {17.0, 100.0})
    assert_receive {:application, {:row, %{x: 2.5, y: 50.0}}}
    button(r, :press, {17.0, 100.0})
    send(pid, {:scene, fn -> move_scene(:column) end})
    assert Headless.pixels(2500) == shared
    button(r, :release, {5.0, 85.0})
    assert_receive {:application, {:column, %{x: 5.0, y: 85.0}}}
  end

  test "a 150px content floor outgrows the offer but cannot escape the Row clip" do
    for floor <- [content(), max(content(), px(108)), max(content(), fill(3))] do
      {_, r} =
        Headless.mount(
          {40, 160},
          fn ->
            el(
              [
                width(px(40)),
                height(px(160)),
                Background.color(:black),
                Event.on_click(:outside)
              ],
              line(floor, 150, px(108))
            )
          end,
          [:pointer_button]
        )

      assert Headless.pixels() == frame(150, 160)
      click(r, {10.0, 117.0}, :a, {4.0, 111.0})
      click(r, {10.0, 121.0}, :outside, {10.0, 121.0})
      click(r, {26.0, 117.0}, :row, {26.0, 117.0})
    end
  end

  test "a sparse move that also changes the cap does not carry the Column's one-line basis into the Row" do
    scene = fn in_row ->
      policy = if in_row, do: min(px(90), min(content(), fill())), else: min(px(90), fill())
      chip = fn -> el([width(px(38)), height(px(12))], none()) end

      card =
        wrapped_row(
          [
            key(:wrap),
            width(px(76)),
            height(policy),
            spacing(4),
            Background.color(:red),
            Event.on_click(:card)
          ],
          [chip.(), chip.()]
        )

      column([width(px(76)), height(px(40)), Background.color(:white), Event.on_click(:host)], [
        column(
          [key(:column), width(px(76)), height(content())],
          if(in_row, do: [], else: [card])
        ),
        row([key(:row), width(px(76)), height(content())], if(in_row, do: [card], else: []))
      ])
    end

    pixels = fn h ->
      :binary.copy(<<255, 0, 0, 255>>, 76 * h) <>
        :binary.copy(<<255, 255, 255, 255>>, 76 * (40 - h))
    end

    {pid, r} = Headless.mount({76, 40}, fn -> scene.(false) end, [:pointer_button])
    assert Headless.pixels() == pixels.(12)
    click(r, {5.0, 20.0}, :host, {5.0, 20.0})
    send(pid, {:scene, fn -> scene.(true) end})
    assert Headless.pixels() == pixels.(28)
    click(r, {5.0, 20.0}, :card, {5.0, 20.0})
    send(pid, {:scene, fn -> scene.(false) end})
    assert Headless.pixels() == pixels.(12)
    click(r, {5.0, 20.0}, :host, {5.0, 20.0})
  end

  test "the height cap uses the wrapped child's second line, not its measured first line" do
    view = fn w ->
      chip = fn -> el([width(px(36)), height(content())], el([height(px(12))], none())) end

      a =
        wrapped_row(
          [
            key(:a),
            width(fill()),
            height(min(content(), fill(3))),
            padding(2),
            spacing(4),
            Background.color(:red),
            Event.on_click(:a)
          ],
          [chip.(), chip.()]
        )

      row(
        [
          width(px(w)),
          height(px(80)),
          padding(2),
          spacing(4),
          Background.color(:white),
          Event.on_click(:row)
        ],
        [a, cell(:b, fill(), none())]
      )
    end

    {pid, r} = Headless.mount({100, 80}, fn -> view.(100) end, [:pointer_button])

    for {{w, h}, i} <- Enum.with_index([{100, 16}, {99, 32}, {100, 16}]) do
      if i > 0, do: send(pid, {:scene, fn -> view.(w) end})

      expected =
        for y <- 0..79, x <- 0..99, into: <<>> do
          cond do
            x >= w -> <<0::32>>
            x >= 2 and x < w - 18 and y >= 2 and y < 2 + h -> <<255, 0, 0, 255>>
            x >= w - 14 and x < w - 2 and y in 2..77 -> <<0, 0, 255, 255>>
            true -> <<255, 255, 255, 255>>
          end
        end

      assert Headless.pixels() == expected

      if h == 32,
        do: click(r, {10.0, 24.0}, :a, {8.0, 22.0}),
        else: click(r, {10.0, 24.0}, :row, {10.0, 24.0})

      click(r, {w - 10.0, 60.0}, :b, {4.0, 58.0})
    end
  end
end
