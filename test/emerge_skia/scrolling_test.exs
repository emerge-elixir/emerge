defmodule EmergeSkia.ScrollingTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  alias Emerge.TestSupport.{BoxAssertions, Headless}

  defp wheel(r, dx, dy, x, y), do: EmergeSkia.send_input(r, {:pointer_scroll, {{dx, dy}, {x, y}}})
  defp pixel(p, w, x, y), do: binary_part(p, 4 * (w * y + x), 4)

  defp card(name, w, h, color, attrs \\ []) do
    el(
      [key(name), width(px(w)), height(px(h)), Background.color(color), Event.on_click(name)] ++
        attrs,
      none()
    )
  end

  defp board(y, h) do
    bind = if is_nil(y), do: [], else: [scroll_y(y)]

    column(
      [width(px(100)), height(px(60)), scrollbar_x(), scrollbar_y(), Event.on_scroll(:board)] ++
        bind,
      [
        card(:first, 200, h, :red),
        card(:second, 200, 60, :blue)
      ]
    )
  end

  test "both wheel components use the installed port; echoes, releases and resizes do not rewind it" do
    {pid, r} = Headless.mount({110, 70}, fn -> board(0.0, 100) end, [:pointer_button])
    p = Headless.pixels()

    assert p ==
             EmergeSkia.TreeRenderer.render_to_pixels(
               board(0.0, 100),
               [otp_app: :emerge, width: 110, height: 70],
               5000
             )

    assert :ok = wheel(r, -50, -25, 10, 10)
    assert_receive {:application, {:board, %{x: 0.5, y: 0.25}}}
    assert :ok = wheel(r, 0, -25, 10, 10)
    assert_receive {:application, {:board, %{x: 0.5, y: 0.5}}}
    p = Headless.pixels_matching(&(pixel(&1, 110, 15, 54) == <<0, 0, 255, 255>>))
    assert {:ok, ^p} = EmergeSkia.render_to_pixels(r)
    BoxAssertions.click(r, {10.0, 6.0}, :first, {60.0, 56.0})

    for {v, h, expected} <- [{0.25, 100, 56.0}, {nil, 140, 76.0}, {0.75, 140, 111.0}] do
      send(pid, {:scene, fn -> board(v, h) end})
      Headless.pixels()
      BoxAssertions.click(r, {10.0, 6.0}, :first, {60.0, expected})
      refute_receive {:application, {:board, _}}, 20
    end

    assert :ok = wheel(r, 0, -200, 10, 10)
    assert_receive {:application, {:board, %{y: 1.0}}}
    Headless.pixels()
    assert :ok = wheel(r, 0, -10, 10, 10)
    refute_receive {:application, {:board, _}}, 40
  end

  test "an integer key jumps to a newly inserted child; a missing key does not arm a later jump" do
    scene = fn binding, keys ->
      column(
        [width(px(80)), height(px(45)), padding(5), scrollbar_y(), scroll_y(binding)],
        Enum.map(keys, fn {k, name, h, color} ->
          el(
            [key(k), width(px(60)), height(px(h)), Background.color(color), Event.on_click(name)],
            none()
          )
        end)
      )
    end

    a = {:a, :first, 40, :red}
    b = {1, :target, 32, :blue}
    c = {:c, :last, 55, :white}
    d = {:absent, :later, 28, :green}
    {pid, r} = Headless.mount({90, 60}, fn -> scene.(0.0, [a, c]) end, [:pointer_button])
    Headless.pixels()

    for {binding, keys, name, color} <- [
          {1, [a, b, c], :target, <<0, 0, 255, 255>>},
          {:absent, [a, b, c], :target, <<0, 0, 255, 255>>},
          {:absent, [d, a, b, c], :first, <<255, 0, 0, 255>>}
        ] do
      send(pid, {:scene, fn -> scene.(binding, keys) end})
      p = Headless.pixels()
      assert pixel(p, 90, 10, 9) == color
      if name == :target, do: BoxAssertions.click(r, {10.0, 9.0}, name, {5.0, 4.0})
    end
  end

  test "a subthreshold motion can click; panning over the threshold captures and suppresses the click" do
    {_, r} = Headless.mount({110, 70}, fn -> board(0.0, 100) end, [:pointer_button])
    Headless.pixels()
    BoxAssertions.button(r, :press, {20.0, 40.0})
    assert :ok = Headless.move(r, 20, 37)
    BoxAssertions.button(r, :release, {20.0, 37.0})
    assert_receive {:application, {:first, %{x: 20.0, y: 37.0}}}
    BoxAssertions.button(r, :press, {20.0, 40.0})
    assert :ok = Headless.move(r, 20, 20)
    assert_receive {:application, {:board, %{x: x, y: 0.2}}} when x == 0.0
    assert :ok = Headless.move(r, 20, -20)
    assert_receive {:application, {:board, %{y: fraction}}}
    assert_in_delta fraction, 0.6, 1.0e-12
    Headless.pixels_matching(&(pixel(&1, 110, 5, 48) == <<0, 0, 255, 255>>))
    BoxAssertions.button(r, :release, {20.0, 20.0})
    refute_receive {:application, {:first, _}}, 30
  end

  test "hover thickens only the thumb and removing a captured port cannot drag its replacement" do
    scene = fn key ->
      el(
        [width(px(100)), height(px(60))],
        el(
          [key(key), width(px(100)), height(px(60)), scrollbar_y(), Event.on_scroll(key)],
          card(:red, 100, 160, :red)
        )
      )
    end

    {pid, r} = Headless.mount({100, 60}, fn -> scene.(:old) end)
    red = <<255, 0, 0, 255>>
    p = Headless.pixels()
    thumb = pixel(p, 100, 97, 10)
    assert thumb != red
    assert pixel(p, 100, 94, 10) == red
    assert :ok = Headless.move(r, 97, 10)
    assert pixel(Headless.pixels(), 100, 94, 10) == thumb
    assert :ok = Headless.move(r, 97, 42)
    assert pixel(Headless.pixels(), 100, 94, 10) == red
    assert :ok = Headless.button(r, :press, 97, 10)
    send(pid, {:scene, fn -> scene.(:new) end})
    Headless.pixels()
    assert :ok = Headless.move(r, 97, 42)
    assert :ok = Headless.button(r, :release, 97, 42)
    refute_receive {:application, {_, _}}, 30
    assert :ok = Headless.button(r, :press, 97, 42)
    assert_receive {:application, {:new, %{y: y}}} when y > 0.0
  end

  test "wheel and click use the inverse of a turned port, without bubbling into a covered sibling" do
    port = fn name, more ->
      el(
        [key(name), width(px(100)), height(px(60)), scrollbar_y(), Event.on_scroll(name)] ++ more,
        card({:content, name}, 100, 160, :blue)
      )
    end

    build = fn ->
      row([width(px(200)), height(px(60))], [
        port.(:covered, []),
        port.(:top, [Transform.move_x(-100), Transform.scale(-1)])
      ])
    end

    {_, r} = Headless.mount({200, 60}, build, [:pointer_button])
    Headless.pixels()
    assert :ok = wheel(r, 0, 25, 92, 54)
    assert_receive {:application, {:top, %{y: 0.25}}}
    Headless.pixels()
    for phase <- [:press, :release], do: BoxAssertions.button(r, phase, {92.0, 54.0})
    assert_receive {:application, {:content, :top, %{x: 8.0, y: 31.0}}}
    assert :ok = wheel(r, 0, -80, 92, 54)
    assert_receive {:application, {:top, %{y: y}}} when y == 0.0
    Headless.pixels()
    assert :ok = wheel(r, 0, -25, 92, 54)
    refute_receive {:application, {:covered, _}}, 30
  end

  test "a horizontal key aligns at the leading inset and controller fractions route to an external PID" do
    to = self()

    build = fn ->
      row(
        [
          width(px(70)),
          height(px(30)),
          padding(4),
          scrollbar_x(),
          scroll_x(2),
          Event.on_scroll({to, {:list, :changed}})
        ],
        for {k, w, c} <- [{1, 40, :red}, {2, 30, :blue}, {3, 86, :green}] do
          el(
            [key(k), width(px(w)), height(px(20)), Background.color(c), Event.on_click(c)],
            none()
          )
        end
      )
    end

    {_, r} = Headless.mount({80, 40}, build, [:pointer_button])
    p = Headless.pixels()
    assert pixel(p, 80, 6, 10) == <<0, 0, 255, 255>>
    BoxAssertions.click(r, {6.0, 10.0}, :blue, {2.0, 6.0})
    assert :ok = wheel(r, -47, -10, 10, 10)
    assert_receive {:list, :changed, %{x: f, y: y}} when y == 0.0
    assert_in_delta f, 87 / 94, 1.0e-12
    Headless.pixels()
    BoxAssertions.click(r, {6.0, 10.0}, :green, {19.0, 6.0})
    refute_receive {:raw, {:pointer_scroll, _}}, 20
  end

  defp page(clip) do
    knob =
      Nearby.in_front(card(:knob, 20, 20, :green, [Transform.move_x(40), Transform.move_y(10)]))

    row = card(:row, 100, 20, :white, if(clip == :row, do: [clip_nearby(), knob], else: [knob]))

    attrs = [
      key(:port),
      width(px(100)),
      height(px(50)),
      scrollbar_y(),
      scroll_y(0.625),
      Nearby.below(card(:fixed, 20, 12, :blue))
    ]

    column([width(px(100)), height(px(100))], [
      column(if(clip == :port, do: [clip_nearby() | attrs], else: attrs), [
        card(:spacer, 100, 50, :black),
        row,
        card(:last, 100, 20, :red)
      ]),
      card(:nav, 100, 50, :yellow)
    ])
  end

  test "the port guards a row's moving overlay but needs clip_nearby to guard its own overlay" do
    {pid, r} = Headless.mount({100, 100}, fn -> page(:open) end, [:pointer_button])
    # The row ends at y=45, the scrollport at y=50. The knob spans y=35..55.
    sites = [{50, 42}, {50, 47}, {50, 52}, {10, 52}]

    surfaces = %{
      knob: {<<0, 255, 0, 255>>, {40.0, 35.0}},
      fixed: {<<0, 0, 255, 255>>, {0.0, 50.0}},
      last: {<<255, 0, 0, 255>>, {0.0, 45.0}},
      nav: {<<255, 255, 0, 255>>, {0.0, 50.0}}
    }

    cases = [
      open: [:knob, :knob, :nav, :fixed],
      row: [:knob, :last, :nav, :fixed],
      port: [:knob, :knob, :nav, :nav],
      open: [:knob, :knob, :nav, :fixed]
    ]

    for {{clip, winners}, step} <- Enum.with_index(cases) do
      if step > 0, do: send(pid, {:scene, fn -> page(clip) end})
      p = Headless.pixels()

      assert p ==
               EmergeSkia.TreeRenderer.render_to_pixels(
                 page(clip),
                 [otp_app: :emerge, width: 100, height: 100],
                 5000
               )

      assert {:ok, ^p} = EmergeSkia.render_to_pixels(r)

      for {{x, y}, winner} <- Enum.zip(sites, winners) do
        {rgba, {ox, oy}} = Map.fetch!(surfaces, winner)
        assert pixel(p, 100, x, y) == rgba
        BoxAssertions.click(r, {x * 1.0, y * 1.0}, winner, {x - ox, y - oy})
      end
    end
  end

  defp dims(w, h), do: [width(px(w)), height(px(h))]

  defp cast(x, y, color), do: Border.shadow(offset: {x, y}, size: 0, blur: 0, color: color)

  defp installed(r, tree) do
    pixels = Headless.pixels()
    assert {:ok, ^pixels} = EmergeSkia.render_to_pixels(r)

    assert pixels ==
             EmergeSkia.TreeRenderer.render_to_pixels(
               tree,
               [otp_app: :emerge, width: 100, height: 100],
               5000
             )

    pixels
  end

  defp cross(axis, both) do
    {stack, across, guard, other, spacer, box, filler, effect} =
      if axis == :y do
        {&column/2, &row/2, scrollbar_y(), scrollbar_x(), {20, 40}, {40, 20}, {50, 80},
         cast(12, 2, :green)}
      else
        {&row/2, &column/2, scrollbar_x(), scrollbar_y(), {40, 20}, {20, 40}, {80, 50},
         cast(2, 12, :green)}
      end

    port = dims(40, 40) ++ [guard, Background.color(:blue)] ++ if(both, do: [other], else: [])
    rect = fn {w, h}, props -> el(dims(w, h) ++ props, none()) end

    across.(dims(100, 100) ++ [Background.color(:yellow), Event.on_click(:paper)], [
      rect.(spacer, []),
      stack.(port, [rect.(box, [Background.color(:red), effect]), rect.(filler, [])])
    ])
  end

  test "ordinary outer shadows can cross the free axis, never the biaxial scroll guard" do
    {pid, r} = Headless.mount({100, 100}, fn -> cross(:y, false) end, [:pointer_button])

    for {{axis, both}, i} <-
          Enum.with_index([{:y, false}, {:y, true}, {:x, false}, {:x, true}, {:y, false}]) do
      if i > 0, do: send(pid, {:scene, fn -> cross(axis, both) end})
      frame = installed(r, cross(axis, both))
      {x, y} = if axis == :y, do: {65, 12}, else: {12, 65}

      assert pixel(frame, 100, x, y) ==
               if(both, do: <<255, 255, 0, 255>>, else: <<0, 255, 0, 255>>)

      BoxAssertions.click(r, {x * 1.0, y * 1.0}, :paper, {x * 1.0, y * 1.0})
    end
  end

  defp strip(mode, shadows) do
    fx = if shadows, do: [cast(8, 0, color_rgba(0, 0, 0, 0.5))], else: []

    pages =
      Enum.map([:red, :green, :blue], fn c ->
        el(dims(50, 100) ++ [padding(5)], card(c, 40, 90, c, fx))
      end)

    position =
      case mode do
        :moved -> Transform.move_x(-50)
        :placed -> center_x()
        :turned -> Transform.rotate(180)
      end

    el(
      dims(100, 100) ++ [Background.color(color_rgb(128, 128, 128)), Event.on_click(:paper)],
      row(dims(50, 100), [el(dims(50, 100), row([position | dims(150, 100)], pages))])
    )
  end

  test "a moved or turned carousel clips actual cards and clicks without cutting their outer shadows" do
    {pid, r} = Headless.mount({100, 100}, fn -> strip(:moved, true) end, [:pointer_button])

    for {{mode, shadows}, i} <-
          Enum.with_index(for s <- [true, false], m <- [:moved, :placed, :turned], do: {m, s}) do
      if i > 0, do: send(pid, {:scene, fn -> strip(mode, shadows) end})
      frame = installed(r, strip(mode, shadows))

      assert pixel(frame, 100, 75, 50) ==
               if(shadows, do: <<64, 64, 64, 255>>, else: <<128, 128, 128, 255>>)

      assert pixel(frame, 100, 25, 50) ==
               if(mode == :turned, do: <<0, 0, 255, 255>>, else: <<0, 255, 0, 255>>)

      BoxAssertions.click(r, {75.0, 50.0}, :paper, {75.0, 50.0})

      BoxAssertions.click(
        r,
        {25.0, 50.0},
        if(mode == :turned, do: :blue, else: :green),
        {20.0, 45.0}
      )
    end
  end

  test "a nested track revealed after clipping pages and drags outside without clicking through" do
    scene = fn pos ->
      column(
        [
          width(px(120)),
          height(px(80)),
          scale(1.5),
          scrollbar_y(),
          scroll_y(pos),
          Event.on_scroll(:outer)
        ],
        [
          card(:spacer, 120, 160, :red),
          el(
            [
              key(:inner),
              width(px(100)),
              height(px(60)),
              scrollbar_y(),
              scrollbar_x(),
              Event.on_scroll(:inner)
            ],
            card(:large, 200, 180, :blue)
          )
        ]
      )
    end

    {pid, r} = Headless.mount({210, 150}, fn -> scene.(0.0) end, [:pointer_button])
    Headless.pixels()

    for p <- [1.0, 0.0, 1.0] do
      send(pid, {:scene, fn -> scene.(p) end})

      assert pixel(Headless.pixels(), 210, 10, 90) ==
               if(p == 1.0, do: <<0, 0, 255, 255>>, else: <<255, 0, 0, 255>>)
    end

    # Inner port is at (0,30), 150 x 90. Thumb length 30, travel 60.
    BoxAssertions.button(r, :press, {148.0, 90.0})
    assert_receive {:application, {:inner, %{x: x, y: 0.75}}} when x == 0.0
    Headless.pixels()
    assert :ok = Headless.move(r, 190.0, 129.0)
    assert_receive {:application, {:inner, %{x: x, y: 1.0}}} when x == 0.0
    Headless.pixels()
    BoxAssertions.button(r, :release, {148.0, 110.0})
    refute_receive {:application, {:large, _}}, 20
    # Blocked horizontal/vertical movement is independent for nested axes.
    assert :ok = wheel(r, -75, 105, 10, 40)
    assert_receive {:application, {:inner, %{x: 0.5, y: inner}}} when inner < 1.0
    Headless.pixels()
    assert :ok = wheel(r, 0, 300, 10, 40)
    assert_receive {:application, {:inner, %{y: y}}} when y == 0.0
    Headless.pixels()
    assert :ok = wheel(r, 0, 105, 10, 40)
    assert_receive {:application, {:outer, %{y: 0.5}}}
  end
end
