defmodule EmergeSkia.RootAlignmentTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions
  alias Emerge.Engine
  alias Emerge.Engine.Patch
  alias Emerge.TestSupport.Headless

  @white 0xFFFFFFFF
  @red 0xFF0000FF
  @blue 0x0000FFFF

  defp chip(attrs \\ []) do
    el(
      [width(px(20)), height(px(20)), Background.color(:red), Event.on_click(:chip) | attrs],
      none()
    )
  end

  test "aligning the root El moves its child, not its own box, hits or viewport origin" do
    scene = fn root_attrs, child_attrs, callback ->
      el(
        [
          width(px(100)),
          height(px(64)),
          padding(4),
          Background.color(:white),
          Event.on_click(callback) | root_attrs
        ],
        chip(child_attrs)
      )
    end

    size = {120, 96}
    pixels = fn x, y -> frame(size, [{0, 0, 100, 64, @white}, {x, y, 20, 20, @red}]) end

    {p, r} =
      Headless.mount(size, fn -> scene.([], [], :root) end, [:pointer_button], target_fps: 1)

    leading = pixels.(4, 4)
    assert Headless.pixels() == leading
    alignment = [center_x(), align_bottom()]

    send(
      p,
      {:scene,
       fn ->
         send(self(), :built)
         scene.(alignment, [], :aligned)
       end}
    )

    assert_receive {:application, :built}
    assert {:ok, ^leading} = EmergeSkia.render_to_pixels(r)
    click(r, {42.0, 42.0}, :root, {42.0, 42.0})
    assert Headless.pixels(2500) == pixels.(40, 40)
    click(r, {42.0, 42.0}, :chip, {2.0, 2.0})
    click(r, {2.0, 2.0}, :aligned, {2.0, 2.0})

    for {child_attrs, callback, x} <- [
          {[align_left()], :aligned, 4},
          {[align_left()], :new_callback, 4},
          {[], :aligned, 40}
        ] do
      build = fn -> scene.(alignment, child_attrs, callback) end
      send(p, {:scene, build})
      expected = pixels.(x, 40)
      assert Headless.pixels(2500) == expected
      click(r, {x + 2.0, 42.0}, :chip, {2.0, 2.0})
      click(r, {2.0, 2.0}, callback, {2.0, 2.0})
      fresh(size, build, expected)
    end
  end

  defp panel(kind, nested) do
    attrs = [
      key(:panel),
      width(px(60)),
      height(px(40)),
      padding(4),
      center_x(),
      align_bottom(),
      Background.color(:white),
      Event.on_click(if(nested, do: :nested, else: :root))
    ]

    box =
      case kind do
        :el -> el(attrs, chip())
        :row -> row(attrs, [chip()])
      end

    if nested,
      do:
        el(
          [key(:wrapper), width(px(100)), height(px(80)), scale(1.5), Background.color(:blue)],
          box
        ),
      else: box
  end

  for kind <- [:el, :row] do
    test "a pressed keyed #{kind} moves to the origin on promotion and regains its alignment on demotion" do
      kind = unquote(kind)
      size = {160, 128}
      {_, state, wrapper} = Engine.encode_full(Engine.diff_state_new(), panel(kind, true))
      [box] = wrapper.children
      id = box.id
      wrapper_id = wrapper.id
      {wire, state, _} = Engine.diff_state_update(state, panel(kind, false))
      assert [{:patch, ^wrapper_id, ^id}] = Patch.decode(wire)
      {wire, _, _} = Engine.diff_state_update(state, panel(kind, true))
      assert [{:patch, ^id, {:node, _, :el, _, [child: ^id]}}] = Patch.decode(wire)

      {p, r} = Headless.mount(size, fn -> panel(kind, true) end, [:pointer_button])
      # The 1.5× wrapper aligns the panel at (30,60). Only an El passes
      # its own center/bottom to its child. Neither root uses viewport alignment.
      {cx, cy, nx, ny} = if kind == :el, do: {20, 16, 60, 84}, else: {4, 4, 36, 66}

      nested =
        frame(size, [{0, 0, 150, 120, @blue}, {30, 60, 90, 60, @white}, {nx, ny, 30, 30, @red}])

      root = frame(size, [{0, 0, 60, 40, @white}, {cx, cy, 20, 20, @red}])
      assert Headless.pixels() == nested

      for {inside, press, release, callback, child_point, expected} <- [
            {false, {33.0, 63.0}, {2.0, 2.0}, :root, {cx + 2.0, cy + 2.0}, root},
            {true, {2.0, 2.0}, {33.0, 63.0}, :nested, {nx + 3.0, ny + 3.0}, nested}
          ] do
        button(r, :press, press)
        build = fn -> panel(kind, inside) end
        send(p, {:scene, build})
        assert Headless.pixels() == expected
        button(r, :release, release)
        assert_receive {:application, {^callback, %{x: 2.0, y: 2.0}}}
        click(r, child_point, :chip, {2.0, 2.0})
        fresh(size, build, expected)
      end
    end
  end
end
