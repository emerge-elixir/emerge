defmodule EmergeSkia.WrappedColumnTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.Engine.{Element, Patch, Serialization}
  alias Emerge.TestSupport.Headless

  defp card(id, w, h, color, extra \\ []) do
    el(
      [key(id), width(px(w)), height(px(h)), Background.color(color), Event.on_click(id) | extra],
      none()
    )
  end

  defp stack(w, h) do
    wrapped_column(
      [width(w), height(h), spacing_xy(6, 4), Background.color(:white), Event.on_click(:host)],
      [card(:a, 12, 20, :red), card(:b, 12, 20, :blue)]
    )
  end

  defp frame(boxes) do
    for y <- 0..79, x <- 0..79, into: <<>> do
      Enum.find_value(boxes, <<0::32>>, fn {cx, cy, w, h, color} ->
        x >= cx and x < cx + w and y >= cy and y < cy + h and <<color::32>>
      end)
    end
  end

  defp picture(w, h, bx, by, top \\ 0) do
    frame([
      {0, top, 12, 20, 0xFF0000FF},
      {bx, top + by, 12, 20, 0x0000FFFF},
      {0, top, w, h, 0xFFFFFFFF}
    ])
  end

  defp click(r, x, y, id, lx, ly) do
    for phase <- [:press, :release] do
      assert :ok = Headless.button(r, phase, x, y)
      assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {^x, ^y}}}}
    end

    assert_receive {:application, {^id, %{x: ^lx, y: ^ly}}}
  end

  test "a one-pixel height edit moves the second card and its target only on submission" do
    build = fn h -> stack(px(64), px(h)) end
    {p, r} = Headless.mount({80, 80}, fn -> build.(44) end, [:pointer_button], target_fps: 1)
    one = picture(64, 44, 0, 24)
    two = picture(64, 43, 18, 0)
    assert Headless.pixels() == one

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         build.(43)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^one} = EmergeSkia.render_to_pixels(r)
    click(r, 19.0, 5.0, :host, 19.0, 5.0)
    assert Headless.pixels(2500) == two
    click(r, 19.0, 5.0, :b, 1.0, 5.0)
    Headless.mount({80, 80}, fn -> build.(43) end)
    assert Headless.pixels() == two
    send(p, {:scene, fn -> build.(44) end})
    assert Headless.pixels(2500) == one
  end

  test "packed content width uses the actual height, including a late share" do
    for mode <- [:root, :assigned] do
      build = fn h ->
        case mode do
          :root ->
            stack(content(), px(h))

          :assigned ->
            column([width(px(64)), height(px(60))], [
              el([height(px(60 - h))], none()),
              stack(content(), fill())
            ])
        end
      end

      top = fn h -> if mode == :root, do: 0, else: 60 - h end
      {p, _} = Headless.mount({80, 80}, fn -> build.(44) end)
      assert Headless.pixels() == picture(12, 44, 0, 24, top.(44))

      for {h, w, x, y} <- [{43, 30, 18, 0}, {44, 12, 0, 24}] do
        send(p, {:scene, fn -> build.(h) end})
        expected = picture(w, h, x, y, top.(h))
        assert Headless.pixels() == expected
        Headless.mount({80, 80}, fn -> build.(h) end)
        assert Headless.pixels() == expected
      end
    end
  end

  test "a sparse width edit that begins reading packed content matches a fresh frame and click" do
    alias Emerge.TestSupport.BoxAssertions, as: Box
    size = {64, 80}

    build = fn basis ->
      column([width(px(16)), height(px(60)), Background.color(:white)], [
        el([width(px(1)), height(px(17))], none()),
        wrapped_column(
          [width(min(px(80), basis)), height(fill()), align_right(), spacing_xy(6, 4)],
          [card(:a, 12, 20, :red), card(:b, 12, 20, :blue)]
        )
      ])
    end

    initial = fn -> build.(fill()) end
    packed = fn -> build.(max(content(), fill())) end
    host = {0, 0, 16, 60, 0xFFFFFFFF}
    one = Box.frame(size, [host, {0, 17, 12, 20, 0xFF0000FF}])
    # The 30px-wide pack starts at −14. Its red card is clipped; blue starts at 4.
    two = Box.frame(size, [host, {4, 17, 12, 20, 0x0000FFFF}])
    {p, r} = Headless.mount(size, initial, [:pointer_button])
    assert Headless.pixels() == one
    click(r, 6.0, 22.0, :a, 6.0, 5.0)
    send(p, {:scene, packed})
    assert Headless.pixels() == two
    click(r, 6.0, 22.0, :b, 2.0, 5.0)
    {_, fresh} = Headless.mount(size, packed, [:pointer_button])
    assert Headless.pixels() == two
    click(fresh, 6.0, 22.0, :b, 2.0, 5.0)
    send(p, {:scene, initial})
    assert Headless.pixels() == one
    click(r, 6.0, 22.0, :a, 6.0, 5.0)
  end

  test "width reflow inside a card decides its vertical break" do
    build = fn cap ->
      chip = el([width(px(10)), height(px(8))], none())

      a =
        wrapped_row([key(:a), width(min(px(cap), fill())), spacing(2), Background.color(:red)], [
          chip,
          chip
        ])

      wrapped_column([width(px(64)), height(px(24)), spacing_xy(6, 4)], [
        a,
        card(:b, 12, 12, :blue)
      ])
    end

    {p, r} = Headless.mount({80, 80}, fn -> build.(22) end, [:pointer_button])
    assert Headless.pixels() == frame([{0, 0, 22, 8, 0xFF0000FF}, {0, 12, 12, 12, 0x0000FFFF}])
    send(p, {:scene, fn -> build.(21) end})
    assert Headless.pixels() == frame([{0, 0, 21, 18, 0xFF0000FF}, {27, 0, 12, 12, 0x0000FFFF}])
    click(r, 29.0, 5.0, :b, 2.0, 5.0)
  end

  test "cross alignment uses the actual host or the individual wrapped group" do
    for {kind, attrs, {ax, ay, bx, by}} <- [
          {:row, [center_y()], {4, 24, 20, 4}},
          {:column, [center_x()], {24, 4, 4, 20}},
          {:wrapped_row, [align_bottom()], {4, 20, 20, 4}},
          {:wrapped_column, [align_right()], {20, 4, 4, 20}}
        ] do
      build = fn ->
        apply(Emerge.UI, kind, [
          [width(px(64)), height(px(64)), padding(4)],
          [card(:a, 16, 16, :red, attrs), card(:b, 32, 32, :blue)]
        ])
      end

      {_, r} = Headless.mount({80, 80}, build, [:pointer_button])

      assert Headless.pixels() ==
               frame([{ax, ay, 16, 16, 0xFF0000FF}, {bx, by, 32, 32, 0x0000FFFF}])

      click(r, ax + 2.0, ay + 2.0, :a, 2.0, 2.0)
    end
  end

  test "changing just a Row child's alignment publishes its new local origin with the frame" do
    build = fn alignment ->
      row(
        [
          width(px(64)),
          height(px(64)),
          padding(4),
          Background.color(:white),
          Event.on_click(:host)
        ],
        [card(:a, 16, 16, :red, [alignment])]
      )
    end

    expected = fn y -> frame([{4, y, 16, 16, 0xFF0000FF}, {0, 0, 64, 64, 0xFFFFFFFF}]) end

    {p, r} =
      Headless.mount({80, 80}, fn -> build.(align_top()) end, [:pointer_button], target_fps: 1)

    top = expected.(4)
    assert Headless.pixels() == top

    send(
      p,
      {:scene,
       fn ->
         send(self(), :centered)
         build.(center_y())
       end}
    )

    assert_receive {:application, :centered}
    assert {:ok, ^top} = EmergeSkia.render_to_pixels(r)
    click(r, 10.0, 26.0, :host, 10.0, 26.0)
    assert Headless.pixels(2500) == expected.(24)
    click(r, 10.0, 26.0, :a, 6.0, 2.0)
    send(p, {:scene, fn -> build.(align_bottom()) end})
    assert Headless.pixels(2500) == expected.(44)
    click(r, 10.0, 26.0, :host, 10.0, 26.0)
  end

  test "an independent height supplies content width before a fill sibling wraps" do
    build = fn h ->
      # All children of the outer Row are keyed; the cards retain their own keys.
      vertical =
        wrapped_column(
          [
            key(:vertical),
            width(content()),
            height(fill()),
            spacing_xy(6, 4),
            Background.color(:white)
          ],
          [card(:a, 12, 20, :red), card(:b, 12, 20, :blue)]
        )

      chip = el([width(px(24)), height(px(8))], none())

      peer =
        wrapped_row(
          [
            key(:peer),
            width(fill()),
            spacing(4),
            Background.color(color_rgb(0, 255, 0)),
            Event.on_click(:peer)
          ],
          [chip, chip]
        )

      row([width(px(64)), height(px(h))], [vertical, peer])
    end

    one =
      frame([
        {0, 0, 12, 20, 0xFF0000FF},
        {0, 24, 12, 20, 0x0000FFFF},
        {0, 0, 12, 44, 0xFFFFFFFF},
        {12, 0, 52, 8, 0x00FF00FF}
      ])

    two =
      frame([
        {0, 0, 12, 20, 0xFF0000FF},
        {18, 0, 12, 20, 0x0000FFFF},
        {0, 0, 30, 43, 0xFFFFFFFF},
        {30, 0, 34, 20, 0x00FF00FF}
      ])

    {p, r} = Headless.mount({80, 80}, fn -> build.(44) end, [:pointer_button])
    assert Headless.pixels() == one
    click(r, 19.0, 5.0, :peer, 7.0, 5.0)
    send(p, {:scene, fn -> build.(43) end})
    assert Headless.pixels() == two
    click(r, 19.0, 5.0, :b, 1.0, 5.0)
    Headless.mount({80, 80}, fn -> build.(43) end)
    assert Headless.pixels() == two
  end

  test "an assigned Column share can change the width of a different Row child" do
    build = fn header ->
      left =
        column([width(content()), height(px(60))], [
          el([width(px(0)), height(px(header))], none()),
          stack(content(), fill())
        ])

      chip = el([width(px(24)), height(px(8))], none())

      peer =
        wrapped_row(
          [
            width(fill()),
            spacing(4),
            Background.color(color_rgb(0, 255, 0)),
            Event.on_click(:peer)
          ],
          [chip, chip]
        )

      row([width(px(64)), height(px(60))], [left, peer])
    end

    {p, r} = Headless.mount({80, 80}, fn -> build.(16) end, [:pointer_button])

    for {{h, w, bx, by, peer_h}, i} <-
          Enum.with_index([{16, 12, 0, 24, 8}, {17, 30, 18, 0, 20}, {16, 12, 0, 24, 8}]) do
      expected =
        frame([
          {0, h, 12, 20, 0xFF0000FF},
          {bx, h + by, 12, 20, 0x0000FFFF},
          {0, h, w, 60 - h, 0xFFFFFFFF},
          {w, 0, 64 - w, peer_h, 0x00FF00FF}
        ])

      if i > 0, do: send(p, {:scene, fn -> build.(h) end})
      assert Headless.pixels() == expected
      click(r, w + 2.0, 5.0, :peer, 2.0, 5.0)
    end
  end

  test "a held press follows a keyed card into the new container kind" do
    build = fn kind ->
      b =
        el(
          [key(:b), width(px(12)), height(px(20)), Background.color(:blue), Event.on_click(kind)],
          none()
        )

      apply(Emerge.UI, kind, [
        [width(px(64)), height(px(44)), spacing_xy(6, 4), Background.color(:white)],
        [card(:a, 12, 20, :red), b]
      ])
    end

    {p, r} = Headless.mount({80, 80}, fn -> build.(:row) end, [:pointer_button])
    assert Headless.pixels() == picture(64, 44, 18, 0)
    assert :ok = Headless.button(r, :press, 19.0, 5.0)
    assert_receive {:raw, {:pointer_button, {:left, :press, [], {19.0, 5.0}}}}
    send(p, {:scene, fn -> build.(:wrapped_column) end})
    assert Headless.pixels() == picture(64, 44, 0, 24)
    assert :ok = Headless.button(r, :release, 1.0, 29.0)
    assert_receive {:raw, {:pointer_button, {:left, :release, [], {1.0, 29.0}}}}
    assert_receive {:application, {:wrapped_column, %{x: 1.0, y: 5.0}}}
  end

  test "the additional kind appends to v10 without renumbering an existing declaration" do
    kinds =
      ~w(row wrapped_row column el text none paragraph text_column image text_input video multiline slider wrapped_column)a

    for {kind, tag} <- Enum.with_index(kinds, 1) do
      element = %Element{id: 1, type: kind}
      bytes = Serialization.encode_tree(element)

      assert bytes ==
               <<"EMRG", 10, 0::64, 1::64, 1::32, 1::64, tag, 3, 2::32, 0::16, 0::16, 0::16>>

      assert %Element{type: ^kind} = Serialization.decode(bytes)
    end

    patch = Patch.patch(1, {:node, 1, :wrapped_column, :same, [{:child, 2}]})
    bytes = Patch.encode(patch)
    assert bytes == <<"EMRG", 10, 1::64, 1::64, 1::32, 1::64, 14, 2, 1::16, 2::64, 0::16>>
    assert [^patch] = Patch.decode(bytes)
  end
end
