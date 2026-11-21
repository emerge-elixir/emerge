defmodule EmergeSkia.ElChildAlignmentTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions
  alias Emerge.Engine
  alias Emerge.Engine.Patch
  alias Emerge.TestSupport.Headless

  @white 0xFFFFFFFF
  @red 0xFF0000FF
  @blue 0x0000FFFF

  test "the direct child's right and center override is installed with its pixels and local click" do
    scene = fn attrs, callback ->
      el(
        [
          width(px(100)),
          height(px(64)),
          padding(4),
          Background.color(:white),
          Event.on_click(:host)
        ],
        el(
          [
            width(px(20)),
            height(px(20)),
            Background.color(:red),
            Event.on_click(callback) | attrs
          ],
          none()
        )
      )
    end

    aligned = [align_right(), center_y()]
    size = {100, 64}
    point = {80.0, 26.0}
    pixels = fn x, y -> frame(size, [{0, 0, 100, 64, @white}, {x, y, 20, 20, @red}]) end
    {pid, r} = Headless.mount(size, fn -> scene.([], :card) end, [:pointer_button], target_fps: 1)
    leading = pixels.(4, 4)
    placed = pixels.(76, 22)
    assert Headless.pixels() == leading

    send(
      pid,
      {:scene,
       fn ->
         send(self(), :accepted)
         scene.(aligned, :card)
       end}
    )

    assert_receive {:application, :accepted}
    assert {:ok, ^leading} = EmergeSkia.render_to_pixels(r)
    click(r, point, :host, point)
    assert Headless.pixels(2500) == placed
    click(r, point, :card, {4.0, 4.0})
    fresh(size, fn -> scene.(aligned, :card) end, placed)
    send(pid, {:scene, fn -> scene.(aligned, :new_callback) end})
    assert Headless.pixels(2500) == placed
    click(r, point, :new_callback, {4.0, 4.0})
    send(pid, {:scene, fn -> scene.([], :card) end})
    assert Headless.pixels(2500) == leading
    click(r, point, :host, point)
  end

  test "signed overflow outside the El remains painted and clickable" do
    view = fn ->
      el(
        [width(px(80)), height(px(64)), padding(12)],
        el(
          [width(px(40)), height(px(24)), padding_each(4, 2, 6, 6), Background.color(:white)],
          el(
            [
              width(px(60)),
              height(px(32)),
              align_right(),
              align_bottom(),
              Background.color(:red),
              Event.on_click(:card)
            ],
            none()
          )
        )
      )
    end

    {_, r} = Headless.mount({80, 64}, view, [:pointer_button])

    assert Headless.pixels() ==
             frame({80, 64}, [{12, 12, 40, 24, @white}, {-10, -2, 60, 32, @red}])

    click(r, {4.0, 4.0}, :card, {14.0, 6.0})
  end

  defp moved(site) do
    card =
      el(
        [
          key(:card),
          width(px(16)),
          height(px(10)),
          align_right(),
          center_y(),
          Background.color(:red),
          Event.on_click(site)
        ],
        none()
      )

    column([width(px(100)), height(px(96)), Background.color(:white)], [
      row([key(:line), width(px(100)), height(px(32))], if(site == :row, do: [card], else: [])),
      el(
        [key(:slot), width(px(50)), height(px(32)), padding(2), scale(2)],
        if(site == :el, do: card, else: none())
      )
    ])
  end

  test "a pressed card moves as a kept ID between a Row and a scaled El in both directions" do
    {_, s, assigned} = Engine.encode_full(Engine.diff_state_new(), moved(:row))
    [line, _] = assigned.children
    [card] = line.children
    id = card.id

    Enum.reduce([:el, :row], s, fn site, state ->
      {wire, next, _} = Engine.diff_state_update(state, moved(site))

      assert [{:patch, _, {:node, _, :column, :same, [child: line, child: slot]}}] =
               Patch.decode(wire)

      assert {:node, _, _, :same, [child: ^id]} = if(site == :el, do: slot, else: line)
      next
    end)

    size = {100, 96}
    {pid, r} = Headless.mount(size, fn -> moved(:row) end, [:pointer_button])
    base = {0, 0, 100, 96, @white}
    row_frame = frame(size, [base, {84, 11, 16, 10, @red}])
    el_frame = frame(size, [base, {64, 54, 32, 20, @red}])
    assert Headless.pixels() == row_frame

    for {at, site, expected, to} <- [
          {{88.0, 15.0}, :el, el_frame, {72.0, 62.0}},
          {{72.0, 62.0}, :row, row_frame, {88.0, 15.0}}
        ] do
      button(r, :press, at)
      send(pid, {:scene, fn -> moved(site) end})
      assert Headless.pixels() == expected
      button(r, :release, to)
      assert_receive {:application, {^site, %{x: 4.0, y: 4.0}}}
      fresh(size, fn -> moved(site) end, expected)
    end
  end

  test "right alignment follows packed width on height and width edits at equal early fit" do
    view = fn width_policy, height_policy ->
      el(
        [width(px(64)), height(px(43)), Background.color(:white), Event.on_click(:host)],
        wrapped_column(
          [
            width(width_policy),
            height(height_policy),
            align_right(),
            spacing_xy(6, 4),
            Background.color(:red),
            Event.on_click(:pack)
          ],
          [
            el([width(px(12))], el([width(px(12)), height(px(20))], none())),
            el([width(px(12)), height(px(20)), Background.color(:blue)], none())
          ]
        )
      )
    end

    size = {64, 44}
    point = {40.0, 6.0}
    floor = max(content(), fill(3))
    cap = min(content(), fill(3))
    {pid, r} = Headless.mount(size, fn -> view.(content(), floor) end, [:pointer_button])

    for {{wp, hp, x, w, h, blue, target, local}, idx} <-
          Enum.with_index([
            {content(), floor, 52, 12, 44, [{52, 24, 12, 20, @blue}], :host, point},
            {content(), cap, 34, 30, 43, [{52, 0, 12, 20, @blue}], :pack, {6.0, 6.0}},
            {px(12), cap, 52, 12, 43, [], :host, point},
            {min(content(), px(20)), cap, 44, 20, 43, [{62, 0, 2, 20, @blue}], :host, point},
            {content(), floor, 52, 12, 44, [{52, 24, 12, 20, @blue}], :host, point}
          ]) do
      scene = fn -> view.(wp, hp) end
      if idx > 0, do: send(pid, {:scene, scene})
      expected = frame(size, [{0, 0, 64, 43, @white}, {x, 0, w, h, @red} | blue])
      assert Headless.pixels() == expected
      click(r, point, target, local)
      fresh(size, scene, expected)
    end
  end
end
