defmodule EmergeSkia.ElChildFillTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp box(w, h, {dw, dh}) do
    child =
      el(
        [key(:a), width(w), height(h), Background.color(:red), Event.on_click(:a)],
        el([width(px(dw)), height(px(dh))], none())
      )

    el(
      [
        width(px(120)),
        height(px(80)),
        padding(6),
        Background.color(:white),
        Event.on_click(:host)
      ],
      child
    )
  end

  # Compare the whole submitted frame, including pixels outside the El when
  # observed in a larger view. Only the enclosing Column imposes a clip.
  defp frame(w, h, {vw, vh} \\ {120, 80}) do
    for y <- 0..(vh - 1), x <- 0..(vw - 1), into: <<>> do
      cond do
        x >= 132 or y >= 92 -> <<0, 0, 0, 255>>
        x >= 6 and y >= 6 and x < 6 + w and y < 6 + h -> <<255, 0, 0, 255>>
        x < 120 and y < 80 -> <<255, 255, 255, 255>>
        true -> <<0, 0, 255, 255>>
      end
    end
  end

  defp button(r, phase, {x, y}) do
    assert :ok = Headless.button(r, phase, x, y)
    assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
  end

  defp click(r, point, name, {lx, ly}) do
    for phase <- [:press, :release], do: button(r, phase, point)
    assert_receive {:application, {^name, %{x: ^lx, y: ^ly}}}
  end

  test "fixed sizes and ordinary width fill keep their existing offers" do
    for {w, outer} <- [{px(30), 30}, {fill(), 108}] do
      {_, r} = Headless.mount({120, 80}, fn -> box(w, px(16), {30, 16}) end, [:pointer_button])
      assert Headless.pixels() == frame(outer, 16)
      click(r, {10.0, 10.0}, :a, {4.0, 4.0})
      click(r, {76.0, 40.0}, :host, {76.0, 40.0})
    end
  end

  for axis <- [:width, :height] do
    test "#{axis} fills use the full independent El offer" do
      for policy <- [fill(), fill(1), fill(3)] do
        {w, h, expected} =
          case unquote(axis) do
            :width -> {policy, px(16), frame(108, 16)}
            :height -> {px(30), policy, frame(30, 68)}
          end

        {_, r} = Headless.mount({120, 80}, fn -> box(w, h, {30, 16}) end, [:pointer_button])
        assert Headless.pixels() == expected
        click(r, {10.0, 10.0}, :a, {4.0, 4.0})
      end
    end
  end

  for axis <- [:width, :height] do
    test "a sparse #{axis}-only cap edit uses that axis without changing the other" do
      cap = min(content(), fill(3))

      {w, h, expected, point, local} =
        case unquote(axis) do
          :width -> {cap, px(16), frame(90, 16), {76.0, 10.0}, {70.0, 4.0}}
          :height -> {px(30), cap, frame(30, 44), {10.0, 40.0}, {4.0, 34.0}}
        end

      {p, r} =
        Headless.mount({120, 80}, fn -> box(px(30), px(16), {90, 44}) end, [:pointer_button])

      assert Headless.pixels() == frame(30, 16)
      click(r, point, :host, point)
      send(p, {:scene, fn -> box(w, h, {90, 44}) end})
      assert Headless.pixels() == expected
      click(r, point, :a, local)
    end
  end

  for start <- [:capped, :fixed] do
    test "#{start}: changing both axes moves the raw click only on submission" do
      cap = min(content(), fill(3))
      {w, h} = if unquote(start) == :capped, do: {cap, cap}, else: {px(30), px(16)}

      {p, r} =
        Headless.mount({120, 80}, fn -> box(w, h, {30, 16}) end, [:pointer_button], target_fps: 1)

      small = frame(30, 16)
      assert Headless.pixels() == small

      send(
        p,
        {:scene,
         fn ->
           send(self(), :grown)
           box(cap, cap, {90, 44})
         end}
      )

      assert_receive {:application, :grown}
      assert {:ok, ^small} = EmergeSkia.render_to_pixels(r)
      click(r, {76.0, 40.0}, :host, {76.0, 40.0})
      assert Headless.pixels(2500) == frame(90, 44)
      click(r, {76.0, 40.0}, :a, {70.0, 34.0})
      send(p, {:scene, fn -> box(cap, cap, {150, 90}) end})
      assert Headless.pixels(2500) == frame(108, 68)
      click(r, {116.0, 76.0}, :host, {116.0, 76.0})
    end
  end

  defp moved(site) do
    card =
      el(
        [
          key(:a),
          width(min(fill(3), px(100))),
          height(fill(0.5)),
          Background.color(:red),
          Event.on_click(site)
        ],
        none()
      )

    peer = el([key(:b), width(fill()), height(px(12)), Background.color(:blue)], none())

    column([width(px(120)), height(px(48)), Background.color(:white)], [
      row(
        [key(:row), width(px(120)), height(px(12))],
        if(site == :row, do: [card, peer], else: [peer])
      ),
      el(
        [key(:el), width(px(54)), height(px(12)), scale(2)],
        if(site == :el, do: card, else: none())
      )
    ])
  end

  test "a press follows a keyed child from a Row share to a scaled El offer and back" do
    {p, r} = Headless.mount({120, 48}, fn -> moved(:row) end, [:pointer_button], target_fps: 1)
    red = fn n -> :binary.copy(<<255, 0, 0, 255>>, n) end
    blue = fn n -> :binary.copy(<<0, 0, 255, 255>>, n) end
    white = fn n -> :binary.copy(<<255, 255, 255, 255>>, n) end
    shared = :binary.copy(red.(90) <> blue.(30), 12) <> white.(120 * 36)
    independent = blue.(120 * 12) <> :binary.copy(red.(108) <> white.(12), 24) <> white.(120 * 12)
    assert Headless.pixels() == shared
    button(r, :press, {85.0, 5.0})
    send(p, {:scene, fn -> moved(:el) end})
    assert Headless.pixels(2500) == independent
    button(r, :release, {100.0, 32.0})
    assert_receive {:application, {:el, %{x: 50.0, y: 10.0}}}
    button(r, :press, {100.0, 32.0})
    send(p, {:scene, fn -> moved(:row) end})
    assert Headless.pixels(2500) == shared
    button(r, :release, {85.0, 5.0})
    assert_receive {:application, {:row, %{x: 85.0, y: 5.0}}}
  end

  test "content floors overflow the El but not an enclosing Column's paint and hit clip" do
    for {w, h} <- [
          {content(), content()},
          {max(content(), px(108)), max(content(), px(68))},
          {max(content(), fill(3)), max(content(), fill(3))}
        ] do
      view = fn ->
        el(
          [width(px(160)), height(px(120)), Background.color(:black), Event.on_click(:outside)],
          column(
            [width(px(132)), height(px(92)), Background.color(:blue), Event.on_click(:clip)],
            [box(w, h, {150, 90})]
          )
        )
      end

      {_, r} = Headless.mount({160, 120}, view, [:pointer_button])
      assert Headless.pixels() == frame(150, 90, {160, 120})
      click(r, {123.0, 86.0}, :a, {117.0, 80.0})
      click(r, {133.0, 86.0}, :outside, {133.0, 86.0})
    end
  end

  test "a one-pixel width edit updates both the wrapper and the wrapped child's own paint and listener" do
    scene = fn w, outer_h, child_h ->
      chip = fn -> el([width(px(36)), height(content())], el([height(px(12))], none())) end

      a =
        wrapped_row(
          [
            width(fill(3)),
            height(child_h),
            padding(2),
            spacing(4),
            Background.color(:red),
            Event.on_click(:a)
          ],
          [chip.(), chip.()]
        )

      el(
        [width(px(w)), padding(2), Background.color(:white), Event.on_click(:host)] ++ outer_h,
        a
      )
    end

    cap = min(content(), fill(3))

    for {policy, child_h, fixed} <- [
          {[height(px(80))], cap, true},
          {[], cap, false},
          {[height(content())], fill(), false}
        ] do
      {p, r} = Headless.mount({84, 80}, fn -> scene.(84, policy, child_h) end, [:pointer_button])

      for {w, i} <- Enum.with_index([84, 83, 84]) do
        if i > 0, do: send(p, {:scene, fn -> scene.(w, policy, child_h) end})
        h = if w == 84, do: 16, else: 32
        outer = if fixed, do: 80, else: h + 4

        pixels =
          for y <- 0..79, x <- 0..83, into: <<>> do
            cond do
              x >= w or y >= outer -> <<0::32>>
              x >= 2 and x < w - 2 and y >= 2 and y < h + 2 -> <<255, 0, 0, 255>>
              true -> <<255, 255, 255, 255>>
            end
          end

        assert Headless.pixels() == pixels

        if h == 32,
          do: click(r, {10.0, 24.0}, :a, {8.0, 22.0}),
          else: click(r, {10.0, 19.0}, :host, {10.0, 19.0})
      end
    end
  end
end
