defmodule EmergeSkia.RowEvenSpacingTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions
  alias Emerge.Engine
  alias Emerge.Engine.Patch
  alias Emerge.TestSupport.Headless

  @size {160, 64}
  @white 0xFFFFFFFF
  @red 0xFF0000FF
  @blue 0x0000FFFF
  @black 0x000000FF

  defp card(id, callback, attrs) do
    {color, route} = %{a: {:red, :a}, b: {:blue, callback}, c: {:black, :c}}[id]

    el(
      [
        key(id),
        width(px(20)),
        height(px(20)),
        Background.color(color),
        Event.on_click(route) | attrs
      ],
      none()
    )
  end

  defp host(attrs, children) do
    row(
      [height(px(40)), padding(4), spacing_xy(4, 7), Background.color(:white) | attrs],
      children
    )
  end

  defp boxes(flag, callback, order) do
    host([width(px(100)) | flag], Enum.map(order, &card(&1, callback, [])))
  end

  defp pixels(boxes), do: frame(@size, [{0, 0, 100, 40, @white} | boxes])

  test "the installed gap owns the local point, including a keyed reorder while pressed" do
    ids = [:a, :b, :c]
    fixed = pixels([{4, 4, 20, 20, @red}, {28, 4, 20, 20, @blue}, {52, 4, 20, 20, @black}])
    even = pixels([{4, 4, 20, 20, @red}, {40, 4, 20, 20, @blue}, {76, 4, 20, 20, @black}])
    swapped = pixels([{4, 4, 20, 20, @blue}, {40, 4, 20, 20, @red}, {76, 4, 20, 20, @black}])

    {p, r} =
      Headless.mount(@size, fn -> boxes([], :old, ids) end, [:pointer_button], target_fps: 1)

    assert Headless.pixels() == fixed

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         boxes([space_evenly()], :even, ids)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^fixed} = EmergeSkia.render_to_pixels(r)
    click(r, {44.0, 10.0}, :old, {16.0, 6.0})
    assert Headless.pixels(2500) == even
    click(r, {44.0, 10.0}, :even, {4.0, 6.0})

    button(r, :press, {44.0, 10.0})
    build = fn -> boxes([space_evenly()], :moved, [:b, :a, :c]) end
    send(p, {:scene, build})
    assert Headless.pixels(2500) == swapped
    button(r, :release, {8.0, 10.0})
    assert_receive {:application, {:moved, %{x: 4.0, y: 6.0}}}
    fresh(@size, build, swapped)

    for {flag, id_order, route, bytes, x, lx} <- [
          {[space_evenly()], [:b, :a, :c], :callback_only, swapped, 8.0, 4.0},
          {[{:space_evenly, false}], ids, :fixed, fixed, 44.0, 16.0},
          {[space_evenly()], ids, :again, even, 44.0, 4.0},
          {[], ids, :removed, fixed, 44.0, 16.0}
        ] do
      build = fn -> boxes(flag, route, id_order) end
      send(p, {:scene, build})
      assert Headless.pixels(2500) == bytes
      click(r, {x, 10.0}, route, {lx, 6.0})
      fresh(@size, build, bytes)
    end
  end

  test "a kept even Row uses its new slot on promotion and demotion" do
    scene = fn nested ->
      box =
        host([key(:box), width(fill()), space_evenly()], [
          card(:a, :b, [align_right()]),
          card(:b, :b, []),
          card(:c, :b, [])
        ])

      if nested, do: row([key(:wrapper), width(content())], [box]), else: box
    end

    {_, state, wrapper} = Engine.encode_full(Engine.diff_state_new(), scene.(true))
    [box] = wrapper.children
    id = box.id
    wrapper_id = wrapper.id
    {wire, state, _} = Engine.diff_state_update(state, scene.(false))
    assert [{:patch, ^wrapper_id, ^id}] = Patch.decode(wire)
    {wire, _, _} = Engine.diff_state_update(state, scene.(true))
    assert [{:patch, ^id, {:node, _, :row, _, [child: ^id]}}] = Patch.decode(wire)

    natural =
      frame(@size, [
        {0, 0, 76, 40, @white},
        {4, 4, 20, 20, @red},
        {28, 4, 20, 20, @blue},
        {52, 4, 20, 20, @black}
      ])

    offered =
      frame(@size, [
        {0, 0, 160, 40, @white},
        {4, 4, 20, 20, @red},
        {70, 4, 20, 20, @blue},
        {136, 4, 20, 20, @black}
      ])

    {p, r} = Headless.mount(@size, fn -> scene.(true) end, [:pointer_button])
    assert Headless.pixels() == natural

    for {nested, before_x, after_x, expected} <- [
          {false, 32.0, 74.0, offered},
          {true, 74.0, 32.0, natural}
        ] do
      button(r, :press, {before_x, 10.0})
      build = fn -> scene.(nested) end
      send(p, {:scene, build})
      assert Headless.pixels() == expected
      button(r, :release, {after_x, 10.0})
      assert_receive {:application, {:b, %{x: 4.0, y: 6.0}}}
      fresh(@size, build, expected)
    end
  end

  test "removing the fixed gap before sharing lets the middle fill's contents rewrap" do
    scene = fn attrs ->
      chip = fn -> el([width(px(24)), height(px(8)), Background.color(:black)], none()) end

      middle =
        wrapped_row(
          [
            key(:b),
            width(fill()),
            spacing_xy(2, 2),
            Background.color(:blue),
            Event.on_click(:middle)
          ],
          [chip.(), chip.()]
        )

      host([width(px(100)) | attrs], [card(:a, :unused, []), middle, card(:c, :unused, [])])
    end

    fixed =
      pixels([
        {4, 4, 20, 20, @red},
        {28, 4, 44, 18, @blue},
        {28, 4, 24, 8, @black},
        {28, 14, 24, 8, @black},
        {76, 4, 20, 20, @black}
      ])

    even =
      pixels([
        {4, 4, 20, 20, @red},
        {24, 4, 52, 8, @blue},
        {24, 4, 24, 8, @black},
        {50, 4, 24, 8, @black},
        {76, 4, 20, 20, @black}
      ])

    {p, r} = Headless.mount(@size, fn -> scene.([]) end, [:pointer_button])
    assert Headless.pixels() == fixed

    for {attrs, expected, lx} <- [{[space_evenly()], even, 20.0}, {[], fixed, 16.0}] do
      build = fn -> scene.(attrs) end
      send(p, {:scene, build})
      assert Headless.pixels() == expected
      click(r, {44.0, 10.0}, :middle, {lx, 6.0})
      fresh(@size, build, expected)
    end
  end

  test "a content floor is not a definite width; at equal extent only the active Row ignores horizontal zones" do
    scene = fn policy ->
      host([width(policy), space_evenly(), scale(1.5)], [
        card(:a, :b, [align_right(), align_bottom()]),
        card(:b, :b, []),
        card(:c, :b, [])
      ])
    end

    floor = max(px(100), content())
    {p, r} = Headless.mount(@size, fn -> scene.(floor) end, [:pointer_button])

    for {{policy, ax, bx, cx}, i} <-
          Enum.with_index([{floor, 114, 6, 42}, {px(100), 6, 60, 114}, {floor, 114, 6, 42}]) do
      build = fn -> scene.(policy) end
      if i > 0, do: send(p, {:scene, build})

      expected =
        frame(@size, [
          {0, 0, 150, 60, @white},
          {ax, 24, 30, 30, @red},
          {bx, 6, 30, 30, @blue},
          {cx, 6, 30, 30, @black}
        ])

      assert Headless.pixels() == expected
      click(r, {ax + 3.0, 27.0}, :a, {2.0, 2.0})
      fresh(@size, build, expected)
    end
  end
end
