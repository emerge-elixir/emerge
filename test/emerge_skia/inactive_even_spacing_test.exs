defmodule EmergeSkia.InactiveEvenSpacingTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions
  alias Emerge.Engine.Element
  alias Emerge.TestSupport.Headless

  @size {64, 32}
  @point {44.0, 9.0}

  defp cards(kind, flag, route) do
    attrs = [
      key(:host),
      width(px(64)),
      height(px(24)),
      padding(4),
      spacing(4),
      Background.color(:white) | flag
    ]

    children =
      for {id, color, callback} <- [{:a, :red, :a}, {:b, :blue, route}] do
        el(
          [
            key(id),
            width(px(20)),
            height(px(12)),
            Background.color(color),
            Event.on_click(callback)
          ],
          none()
        )
      end

    case kind do
      :el -> el(attrs, row([width(fill()), spacing(4)], children))
      :row -> row(attrs, children)
    end
  end

  defp boxes(bx) do
    frame(@size, [
      {0, 0, 64, 24, 0xFFFFFFFF},
      {4, 4, 20, 12, 0xFF0000FF},
      {bx, 4, 20, 12, 0x0000FFFF}
    ])
  end

  test "El does not pass even spacing to its child; a replacement Row uses it" do
    {p, r} = Headless.mount(@size, fn -> cards(:el, [], :card) end, [:pointer_button])
    fixed = boxes(28)
    assert Headless.pixels() == fixed

    for {flag, route} <- [
          {[space_evenly()], :card},
          {[space_evenly()], :renamed},
          {[space_evenly: false], :card},
          {[], :card},
          {[space_evenly()], :card}
        ] do
      build = fn -> cards(:el, flag, route) end
      send(p, {:scene, build})
      assert Headless.pixels() == fixed
      click(r, @point, route, {16.0, 5.0})
      fresh(@size, build, fixed)
    end

    for {kind, bx, lx} <- [{:row, 40, 4.0}, {:el, 28, 16.0}] do
      button(r, :press, @point)
      build = fn -> cards(kind, [space_evenly()], :moved) end
      send(p, {:scene, build})
      assert Headless.pixels() == boxes(bx)
      button(r, :release, @point)
      assert_receive {:application, {:moved, %{x: ^lx, y: 5.0}}}
      fresh(@size, build, boxes(bx))
    end
  end

  for kind <- [:text, :none] do
    test "a flag on the #{kind} declaration leaves its pixels and local click alone" do
      kind = unquote(kind)

      scene = fn flag ->
        # These leaf helpers don't take attributes. Exercise the public Element declaration.
        attrs =
          Map.new([
            width(px(24)),
            height(px(20)),
            Background.color(:blue),
            Font.color(:white),
            Event.on_click(:leaf) | flag
          ])

        leaf = %Element{
          type: kind,
          attrs: if(kind == :text, do: Map.put(attrs, :content, "Hi"), else: attrs)
        }

        el([padding(4)], leaf)
      end

      {p, r} = Headless.mount(@size, fn -> scene.([]) end, [:pointer_button])
      initial = Headless.pixels()

      if kind == :none,
        do: assert(initial == frame(@size, [{4, 4, 24, 20, 0x0000FFFF}])),
        else: assert(:binary.match(initial, <<255, 255, 255, 255>>) != :nomatch)

      for flag <- [[space_evenly()], [space_evenly: false], []] do
        build = fn -> scene.(flag) end
        send(p, {:scene, build})
        assert Headless.pixels() == initial
        click(r, {9.0, 10.0}, :leaf, {5.0, 6.0})
        fresh(@size, build, initial)
      end
    end
  end
end
