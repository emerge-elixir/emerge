defmodule EmergeSkia.ScrollInputsTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.Headless

  defp key_input(r, key, mods \\ [], action \\ :press),
    do: EmergeSkia.send_input(r, {:key, {key, action, mods}})

  defp touch(r, id, phase, x, y), do: EmergeSkia.send_input(r, {:touch, {id, phase, {x, y}}})
  defp box(w, h, attrs \\ []), do: el([width(px(w)), height(px(h)) | attrs], none())
  defp pixel(p, w, x, y), do: binary_part(p, 4 * (y * w + x), 4)

  test "plain navigation keys use extent, range and installed ancestry; releases and shortcuts do not scroll" do
    {_, r} =
      Headless.mount({160, 60}, fn ->
        left = [scrollbar_y(), Event.on_scroll(:left)]
        right = [scrollbar_y(), Event.on_scroll(:right)]

        row([], [
          el([width(px(80)), height(px(60)) | left], box(70, 300, [Event.on_focus(:left_focus)])),
          el([width(px(80)), height(px(60)) | right], box(70, 300))
        ])
      end)

    Headless.pixels()
    :ok = key_input(r, :arrow_down)
    assert_receive {:application, {:right, %{y: 0.125}}}
    :ok = Headless.move(r, 5, 5)
    :ok = key_input(r, :page_down)
    assert_receive {:application, {:left, %{y: 0.25}}}
    :ok = key_input(r, :tab)
    assert_receive {:application, :left_focus}
    :ok = Headless.move(r, 100, 5)

    for {key, expected} <- [end: 1.0, page_up: 0.75, arrow_up: 0.625, home: 0.0] do
      assert :ok = key_input(r, key)
      assert_receive {:application, {:left, %{y: ^expected}}}
    end

    :ok = key_input(r, :end, [], :release)
    for mod <- [:shift, :ctrl, :alt, :meta], do: assert(:ok = key_input(r, :end, [mod]))
    refute_receive {:application, {_, %{y: _}}}, 40
    refute_received {:raw, _}
  end

  test "Tab wraps past unpainted rows, reveals, and sends unwrapped messages; autofocus belongs to a mount" do
    build = fn name, auto ->
      column(
        [width(px(64)), height(px(40)), scrollbar_y(), Event.on_scroll(:port)],
        for i <- 0..63 do
          attrs =
            if i in [0, 63],
              do: [
                Event.on_focus({:focus, i}),
                Event.on_blur({:blur, i}),
                Event.on_press({:press, i})
              ],
              else: []

          attrs = if i == 0 && auto, do: [focus_on_mount() | attrs], else: attrs

          box(60, 20, [
            key(if(i == 0, do: name, else: i)),
            Event.on_click({:row, i}),
            Background.color(if(i == 63, do: :blue, else: :red)) | attrs
          ])
        end
      )
    end

    {v, r} = Headless.mount({64, 40}, fn -> build.(:first, true) end, [:key])
    Headless.pixels()
    assert_receive {:application, {:focus, 0}}
    :ok = key_input(r, :tab, [:shift])
    assert_receive {:raw, {:key, {:tab, :press, [:shift]}}}
    assert_receive {:application, {:blur, 0}}
    assert_receive {:application, {:focus, 63}}
    assert_receive {:application, {:port, %{y: 1.0}}}
    Headless.pixels_matching(&(pixel(&1, 64, 5, 25) == <<0, 0, 255, 255>>))
    :ok = key_input(r, :enter)
    assert_receive {:application, {:press, 63}}
    :ok = Headless.button(r, :press, 5, 25)
    :ok = Headless.button(r, :release, 5, 25)
    assert_receive {:application, {:row, 63, %{x: 5.0, y: 5.0}}}
    assert_receive {:application, {:press, 63}}

    for auto <- [false, true] do
      send(v, {:scene, fn -> build.(:first, auto) end})
      Headless.pixels()
      refute_receive {:application, {:focus, _}}, 20
    end

    send(v, {:scene, fn -> build.(:replacement, true) end})
    assert_receive {:application, {:blur, 63}}, 1000
    assert_receive {:application, {:focus, 0}}
    assert_receive {:application, {:port, %{y: pos}}} when pos == 0.0
    :ok = key_input(r, :tab)
    assert_receive {:application, {:focus, 63}}
    :ok = key_input(r, :tab)
    assert_receive {:application, {:focus, 0}}
  end

  test "an offscreen inner port and its outer port reveal the same stop inside-out" do
    {_, r} =
      Headless.mount({90, 60}, fn ->
        column([width(px(90)), height(px(60)), scrollbar_y(), Event.on_scroll(:outer)], [
          box(80, 110),
          column([width(px(80)), height(px(40)), scrollbar_y(), Event.on_scroll(:inner)], [
            box(60, 70),
            box(60, 20, [Background.color(:blue), Event.on_focus(:hidden), Event.on_click(:here)]),
            box(60, 30)
          ]),
          box(80, 10)
        ])
      end)

    p = Headless.pixels()
    refute pixel(p, 90, 12, 50) == <<0, 0, 255, 255>>
    :ok = key_input(r, :tab)
    assert_receive {:application, :hidden}
    assert_receive {:application, {:inner, %{y: 0.625}}}
    assert_receive {:application, {:outer, %{y: 0.9}}}
    Headless.pixels_matching(&(pixel(&1, 90, 12, 50) == <<0, 0, 255, 255>>))
    for phase <- [:press, :release], do: assert(:ok = Headless.button(r, phase, 12, 50))
    assert_receive {:application, {:here, %{x: 12.0, y: 10.0}}}
    :ok = key_input(r, :end)
    assert_receive {:application, {:inner, %{y: 1.0}}}
    :ok = key_input(r, :end)
    assert_receive {:application, {:outer, %{y: 1.0}}}
  end

  test "a horizontal-only strip uses its width for pages and does not claim vertical arrows" do
    {_, r} =
      Headless.mount({80, 50}, fn ->
        el([width(px(80)), height(px(50)), scrollbar_x(), Event.on_scroll(:strip)], box(400, 40))
      end)

    Headless.pixels()

    for {key, expected} <- [
          arrow_right: 0.09375,
          page_down: 0.34375,
          end: 1.0,
          page_up: 0.75,
          home: 0.0
        ] do
      assert :ok = key_input(r, key)
      assert_receive {:application, {:strip, %{x: ^expected}}}
    end

    :ok = key_input(r, :arrow_down)
    refute_receive {:application, {:strip, _}}, 30
  end

  test "a port does not pan to reveal its own nearby" do
    {_, r} =
      Headless.mount({90, 70}, fn ->
        el(
          [
            width(px(80)),
            height(px(40)),
            scrollbar_y(),
            Event.on_scroll(:port),
            Nearby.below(box(60, 20, [Event.on_focus(:pin)]))
          ],
          box(80, 240)
        )
      end)

    Headless.pixels()
    :ok = key_input(r, :tab)
    assert_receive {:application, :pin}
    refute_receive {:application, {:port, _}}, 40
  end

  test "touch preflight, raw masks, primary tap and cancel stay separate from pointer observation" do
    {_, r} =
      Headless.mount({80, 60}, fn -> box(80, 60, [Event.on_click(:tap)]) end, [
        :pointer_button,
        :pointer_move,
        :touch
      ])

    Headless.pixels()

    for {id, phase, xy} <- [
          {-1, :down, {0, 0}},
          {0x1_0000_0000_0000_0000, :up, {0, 0}},
          {1, :hover, {0, 0}},
          {1, :move, {:nan, 5}},
          {1, :move, {1.0e300, 5}}
        ] do
      assert {:error, _} = EmergeSkia.send_input(r, {:touch, {id, phase, xy}})
    end

    primary = 0xFFFF_FFFF_FFFF_FFFF

    for {id, phase, x} <- [
          {primary, :down, 5},
          {2, :down, 20},
          {2, :move, 40},
          {2, :up, 55},
          {primary, :cancel, 6},
          {primary, :up, 5}
        ] do
      assert :ok = touch(r, id, phase, x, 10)
      x = x / 1
      assert_receive {:raw, {:touch, {^id, ^phase, {^x, 10.0}}}}
    end

    refute_received {:application, _}
    for {phase, x} <- [down: 5, move: 6, up: 7], do: assert(:ok = touch(r, primary, phase, x, 10))
    assert_receive {:application, {:tap, %{x: 7.0, y: 10.0}}}
    refute_received {:raw, {:pointer_button, _}}
    refute_received {:raw, {:pointer_move, _}}
    assert :ok = EmergeSkia.Native.set_input_mask(r, 0)
    for phase <- [:down, :up], do: assert(:ok = touch(r, 3, phase, 9, 15))
    assert_receive {:application, {:tap, %{x: 9.0, y: 15.0}}}
    refute_received {:raw, {:touch, {3, _, _}}}
  end

  test "tap, captured drag, directional swipe and a vertical pan have one owner" do
    {_, r} =
      Headless.mount({90, 90}, fn ->
        column([width(px(90)), height(px(90)), scrollbar_y(), Event.on_scroll(:page)], [
          box(80, 30, [Event.on_drag(:drag)]),
          box(80, 200, [Event.on_click(:tap), Event.on_swipe_right(:swipe)])
        ])
      end)

    Headless.pixels()
    :ok = touch(r, 1, :down, 10, 10)
    assert_receive {:application, {:drag, %{phase: :start}}}
    :ok = touch(r, 1, :move, 14, -15)
    assert_receive {:application, {:drag, %{phase: :move, x: 14.0, y: -15.0}}}
    :ok = touch(r, 1, :cancel, 14, -15)
    assert_receive {:application, {:drag, %{phase: :cancel}}}
    :ok = touch(r, 2, :down, 15, 50)
    :ok = touch(r, 2, :move, 80, 46)
    :ok = touch(r, 2, :up, 100, 44)

    assert_receive {:application,
                    {:swipe,
                     %{start: %{x: 15.0, y: 20.0}, end: %{x: 100.0, y: 14.0}, duration_ms: ms}}}
                   when ms >= 0.0

    refute_received {:application, {:page, _}}
    :ok = touch(r, 3, :down, 15, 70)
    :ok = touch(r, 3, :move, 30, 35)
    assert_receive {:application, {:page, %{x: x, y: 0.25}}} when x == 0.0
    :ok = touch(r, 3, :cancel, 30, 35)
    refute_receive {:application, {:tap, _}}, 40
    refute_received {:application, {:page, _}}
  end

  test "orthogonal nested ports compete once; an ambiguous diagonal never becomes a tap" do
    {_, r} =
      Headless.mount({80, 60}, fn ->
        strip =
          el(
            [width(px(70)), height(px(50)), scrollbar_x(), Event.on_scroll(:strip)],
            box(230, 45, [Event.on_click(:tap), Event.on_swipe_up(:swipe)])
          )

        column([width(px(80)), height(px(60)), scrollbar_y(), Event.on_scroll(:page)], [
          strip,
          box(70, 170)
        ])
      end)

    Headless.pixels()
    :ok = touch(r, 1, :down, 30, 26)
    :ok = touch(r, 1, :move, 42, 38)
    :ok = touch(r, 1, :up, 30, 26)
    refute_receive {:application, _}, 30
    :ok = touch(r, 2, :down, 30, 26)
    :ok = touch(r, 2, :move, -50, 25)
    assert_receive {:application, {:strip, %{x: 0.5}}}
    :ok = touch(r, 2, :move, -50, -55)
    :ok = touch(r, 2, :cancel, -50, -55)
    refute_receive {:application, _}, 30
    # The bottom 5px is the horizontal track, not content. Starting in
    # content lets the enclosing vertical port (not that track) accept.
    :ok = touch(r, 3, :down, 30, 36)
    :ok = touch(r, 3, :move, 31, -4)
    assert_receive {:application, {:page, %{y: 0.25}}}
    :ok = touch(r, 3, :cancel, 31, -4)
    refute_received {:application, _}
  end

  test "release advances a touch pan to the range edge then stops producing frames" do
    {_, r} =
      Headless.mount({90, 60}, fn ->
        el(
          [width(px(90)), height(px(60)), scrollbar_y(), Event.on_scroll(:pan)],
          box(80, 260, [Event.on_click(:tap)])
        )
      end)

    Headless.pixels()
    :ok = touch(r, 1, :down, 20, 50)
    Process.sleep(16)
    :ok = touch(r, 1, :move, 20, -130)
    assert_receive {:application, {:pan, %{y: 0.9}}}
    :ok = touch(r, 1, :up, 20, -130)
    assert_receive {:application, {:pan, %{y: 1.0}}}, 1000
    refute_receive {:application, {:pan, _}}, 80
    refute_received {:application, {:tap, _}}
    drain_frames()
    refute_receive {:emerge_skia_frame, _}, 80
  end

  test "an echo preserves a contact; replacement cancels both a running fling and a stale release" do
    build = fn port, pos, color ->
      el(
        [
          key(port),
          width(px(90)),
          height(px(60)),
          scrollbar_y(),
          scroll_y(pos),
          Event.on_scroll({:pan, color})
        ],
        box(80, 4060, [Background.color(color), Event.on_click(:tap)])
      )
    end

    {v, r} = Headless.mount({90, 60}, fn -> build.(:port, 0.0, :red) end)
    Headless.pixels()
    :ok = touch(r, 1, :down, 20, 50)
    :ok = touch(r, 1, :move, 20, -150)
    assert_receive {:application, {:pan, :red, %{y: 0.05}}}
    send(v, {:scene, fn -> build.(:port, 0.05, :blue) end})
    Headless.pixels_matching(&(pixel(&1, 90, 20, 40) == <<0, 0, 255, 255>>))
    # Two real samples after the echo: do not depend on time spent in layout.
    :ok = touch(r, 1, :move, 20, -190)
    assert_receive {:application, {:pan, :blue, %{y: a}}} when a > 0.05
    Process.sleep(16)
    :ok = touch(r, 1, :move, 20, -330)
    assert_receive {:application, {:pan, :blue, %{y: b}}} when b > a
    :ok = touch(r, 1, :up, 20, -330)
    assert_receive {:application, {:pan, :blue, %{y: c}}} when c > b, 1000
    send(v, {:scene, fn -> build.(:port, 0.75, :green) end})
    Headless.pixels_matching(&(pixel(&1, 90, 20, 40) == <<0, 255, 0, 255>>))
    refute_receive {:application, {:pan, :green, _}}, 80
    :ok = touch(r, 2, :down, 20, 50)
    :ok = touch(r, 2, :move, 20, -50)
    assert_receive {:application, {:pan, :green, %{y: pos}}}
    assert_in_delta pos, 0.775, 1.0e-9
    send(v, {:scene, fn -> build.(:new, 0.25, :white) end})
    Headless.pixels_matching(&(pixel(&1, 90, 20, 40) == <<255, 255, 255, 255>>))
    :ok = touch(r, 2, :up, 20, 40)
    refute_receive {:application, {:pan, :white, _}}, 80
    refute_received {:application, {:tap, _}}
  end

  defp drain_frames do
    receive do
      {:emerge_skia_frame, _} -> drain_frames()
    after
      0 -> :ok
    end
  end
end
