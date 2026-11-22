defmodule EmergeSkia.LinearEvenSpacingTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions
  alias Emerge.Engine
  alias Emerge.Engine.Patch
  alias Emerge.TestSupport.Headless

  @size {128, 128}
  @ink %{red: 0xFF0000FF, blue: 0x0000FFFF, black: 0x000000FF, white: 0xFFFFFFFF}
  @colors %{a: :red, b: :blue, c: :red, d: :black, e: :blue}

  defp axes(:row, u, v), do: {u, v}
  defp axes(:column, u, v), do: {v, u}
  defp wrap(:row, attrs, children), do: wrapped_row(attrs, children)
  defp wrap(:column, attrs, children), do: wrapped_column(attrs, children)

  defp box(w, h, color, attrs \\ []) do
    el([width(px(w)), height(px(h)), Background.color(color) | attrs], none())
  end

  defp chips(axis, ids, flag, callback) do
    {w, h} = axes(axis, 60, 32)
    {cw, ch} = axes(axis, 20, 8)
    {gx, gy} = axes(axis, 4, 2)

    children =
      Enum.map(ids, fn id ->
        box(cw, ch, @colors[id], [key(id), Event.on_click(if(id == :d, do: callback, else: id))])
      end)

    wrap(
      axis,
      [width(px(w)), height(px(h)), spacing_xy(gx, gy), Background.color(:white) | flag],
      children
    )
  end

  defp chip_frame(axis, ids, second) do
    {w, h} = axes(axis, 60, 32)
    {cw, ch} = axes(axis, 20, 8)

    boxes =
      Enum.zip_with(ids, [{0, 0}, {second, 0}, {0, 10}, {second, 10}, {0, 20}], fn id, {u, v} ->
        {x, y} = axes(axis, u, v)
        {x, y, cw, ch, @ink[@colors[id]]}
      end)

    frame(@size, [{0, 0, w, h, @ink.white} | boxes])
  end

  for axis <- [:row, :column] do
    test "wrapped #{axis}: each group owns its submitted gap and local clicks" do
      axis = unquote(axis)
      ids = [:a, :b, :c, :d, :e]
      swap = [:a, :d, :c, :b, :e]
      at = axes(axis, 42.0, 12.0)
      fixed = chip_frame(axis, ids, 24)
      even = chip_frame(axis, ids, 40)
      moved = chip_frame(axis, swap, 40)

      {p, r} =
        Headless.mount(@size, fn -> chips(axis, ids, [], :old) end, [:pointer_button],
          target_fps: 1
        )

      assert Headless.pixels() == fixed

      send(
        p,
        {:scene,
         fn ->
           send(self(), :built)
           chips(axis, ids, [space_evenly()], :even)
         end}
      )

      assert_receive {:application, :built}
      assert {:ok, ^fixed} = EmergeSkia.render_to_pixels(r)
      click(r, at, :old, axes(axis, 18.0, 2.0))
      assert Headless.pixels(2500) == even
      click(r, at, :even, {2.0, 2.0})
      button(r, :press, at)
      build = fn -> chips(axis, swap, [space_evenly()], :moved) end
      send(p, {:scene, build})
      assert Headless.pixels(2500) == moved
      button(r, :release, axes(axis, 42.0, 2.0))
      assert_receive {:application, {:moved, %{x: 2.0, y: 2.0}}}
      fresh(@size, build, moved)

      for {flag, order, callback, bytes, point, local} <- [
            {[space_evenly()], swap, :callback, moved, axes(axis, 42.0, 2.0), {2.0, 2.0}},
            {[{:space_evenly, false}], ids, :fixed, fixed, at, axes(axis, 18.0, 2.0)},
            {[space_evenly()], ids, :even, even, at, {2.0, 2.0}},
            {[], ids, :removed, fixed, at, axes(axis, 18.0, 2.0)}
          ] do
        build = fn -> chips(axis, order, flag, callback) end
        send(p, {:scene, build})
        assert Headless.pixels(2500) == bytes
        click(r, point, callback, local)
        fresh(@size, build, bytes)
      end
    end
  end

  test "only the submitted Column changes the middle card's height and local point" do
    scene = fn flag, middle ->
      column(
        [width(px(40)), height(px(100)), padding(4), spacing(4), Background.color(:white) | flag],
        [
          box(20, 20, :red),
          el(
            [width(px(20)), height(middle), Background.color(:black), Event.on_click(:middle)],
            none()
          ),
          box(20, 20, :blue)
        ]
      )
    end

    expected = fn y, h, last ->
      frame(@size, [
        {0, 0, 40, 100, @ink.white},
        {4, 4, 20, 20, @ink.red},
        {4, y, 20, h, @ink.black},
        {4, last, 20, 20, @ink.blue}
      ])
    end

    {p, r} = Headless.mount(@size, fn -> scene.([], px(20)) end, [:pointer_button], target_fps: 1)
    fixed = expected.(28, 20, 52)
    assert Headless.pixels() == fixed

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         scene.([space_evenly()], px(20))
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^fixed} = EmergeSkia.render_to_pixels(r)
    click(r, {10.0, 44.0}, :middle, {6.0, 16.0})
    assert Headless.pixels(2500) == expected.(40, 20, 76)
    click(r, {10.0, 44.0}, :middle, {6.0, 4.0})

    for {flag, h, y, used} <- [
          {[], fill(), 28, 44},
          {[space_evenly()], fill(), 24, 52},
          {[space_evenly()], min(px(32), fill()), 34, 32}
        ] do
      build = fn -> scene.(flag, h) end
      send(p, {:scene, build})
      bytes = expected.(y, used, 76)
      assert Headless.pixels(2500) == bytes
      click(r, {10.0, 44.0}, :middle, {6.0, 44.0 - y})
      fresh(@size, build, bytes)
    end
  end

  test "Column distributes the height left after its fixed header has reflowed" do
    scene = fn w ->
      header =
        wrapped_row([width(fill()), spacing(4), Background.color(:red)], [
          box(20, 8, :red),
          box(20, 8, :red)
        ])

      column(
        [
          width(px(w)),
          height(px(100)),
          padding(4),
          spacing(4),
          space_evenly(),
          Background.color(:white)
        ],
        [header, box(20, 20, :black, [Event.on_click(:middle)]), box(20, 20, :blue)]
      )
    end

    {p, r} = Headless.mount(@size, fn -> scene.(52) end, [:pointer_button])

    for {{w, header, y}, i} <- Enum.with_index([{52, 8, 34}, {51, 20, 40}, {52, 8, 34}]) do
      build = fn -> scene.(w) end
      if i > 0, do: send(p, {:scene, build})

      expected =
        frame(@size, [
          {0, 0, w, 100, @ink.white},
          {4, 4, w - 8, header, @ink.red},
          {4, y, 20, 20, @ink.black},
          {4, 76, 20, 20, @ink.blue}
        ])

      assert Headless.pixels() == expected
      click(r, {10.0, 44.0}, :middle, {6.0, 44.0 - y})
      fresh(@size, build, expected)
    end
  end

  test "a Column's even share changes its packed width before its peer wraps" do
    scene = fn flag ->
      pack =
        wrapped_column([width(content()), height(fill()), spacing_xy(6, 4), space_evenly()], [
          box(12, 20, :red),
          box(12, 20, :black)
        ])

      left =
        column([width(content()), height(px(60)), spacing(1) | flag], [box(0, 16, :red), pack])

      peer =
        wrapped_row([width(fill()), spacing(4)], [
          box(24, 8, :blue, [Event.on_click(:peer)]),
          box(24, 8, :blue)
        ])

      row([width(px(64)), height(px(60)), Background.color(:white)], [left, peer])
    end

    fixed =
      frame(@size, [
        {0, 0, 64, 60, @ink.white},
        {0, 17, 12, 20, @ink.red},
        {18, 17, 12, 20, @ink.black},
        {30, 0, 24, 8, @ink.blue},
        {30, 12, 24, 8, @ink.blue}
      ])

    even =
      frame(@size, [
        {0, 0, 64, 60, @ink.white},
        {0, 16, 12, 20, @ink.red},
        {0, 40, 12, 20, @ink.black},
        {12, 0, 24, 8, @ink.blue},
        {40, 0, 24, 8, @ink.blue}
      ])

    {p, r} = Headless.mount(@size, fn -> scene.([]) end, [:pointer_button])
    assert Headless.pixels() == fixed

    for {flag, bytes, lx} <- [{[space_evenly()], even, 20.0}, {[], fixed, 2.0}] do
      build = fn -> scene.(flag) end
      send(p, {:scene, build})
      assert Headless.pixels() == bytes
      click(r, {32.0, 5.0}, :peer, {lx, 5.0})
      fresh(@size, build, bytes)
    end
  end

  test "a kept even WrappedColumn changes height offer and scale on promotion" do
    scene = fn nested ->
      col =
        wrapped_column(
          [
            key(:cards),
            width(px(40)),
            height(fill()),
            padding(4),
            spacing(4),
            space_evenly(),
            Background.color(:white)
          ],
          [box(20, 20, :red), box(20, 20, :black, [Event.on_click(:kept)]), box(20, 20, :blue)]
        )

      if nested, do: el([key(:shell), width(px(40)), height(px(80)), scale(1.5)], col), else: col
    end

    {_, s, shell} = Engine.encode_full(Engine.diff_state_new(), scene.(true))
    [col] = shell.children
    {id, parent} = {col.id, shell.id}
    {wire, s, _} = Engine.diff_state_update(s, scene.(false))
    assert [{:patch, ^parent, ^id}] = Patch.decode(wire)
    {wire, _, _} = Engine.diff_state_update(s, scene.(true))
    assert [{:patch, ^id, {:node, _, :el, _, [child: ^id]}}] = Patch.decode(wire)

    nested =
      frame(@size, [
        {0, 0, 60, 120, @ink.white},
        {6, 6, 30, 30, @ink.red},
        {6, 45, 30, 30, @ink.black},
        {6, 84, 30, 30, @ink.blue}
      ])

    promoted =
      frame(@size, [
        {0, 0, 40, 128, @ink.white},
        {4, 4, 20, 20, @ink.red},
        {4, 54, 20, 20, @ink.black},
        {4, 104, 20, 20, @ink.blue}
      ])

    {p, r} = Headless.mount(@size, fn -> scene.(true) end, [:pointer_button])
    assert Headless.pixels() == nested

    for {nest, bytes, from, to} <- [
          {false, promoted, {12.0, 51.0}, {8.0, 58.0}},
          {true, nested, {8.0, 58.0}, {12.0, 51.0}}
        ] do
      button(r, :press, from)
      build = fn -> scene.(nest) end
      send(p, {:scene, build})
      assert Headless.pixels() == bytes
      button(r, :release, to)
      assert_receive {:application, {:kept, %{x: 4.0, y: 4.0}}}
      fresh(@size, build, bytes)
    end
  end
end
