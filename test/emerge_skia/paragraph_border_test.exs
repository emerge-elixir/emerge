defmodule EmergeSkia.ParagraphBorderTest do
  use ExUnit.Case
  use Emerge.UI

  alias Emerge.UI.Color
  alias EmergeSkia.TreeRenderer

  defp pixels(attrs) do
    paragraph(
      [width(px(180)), padding(16), Font.size(24), Font.color(:white), center_x()],
      [text("before "), el(attrs, text("user interaction")), text(" after")]
    )
    |> TreeRenderer.render_to_pixels([otp_app: :emerge, width: 180, height: 180], 30_000)
  end

  defp blue_pixels(pixels) do
    for <<r, g, b, a <- pixels>>, reduce: 0 do
      n -> if(a > 20 and b > r + 30 and g > r + 20, do: n + 1, else: n)
    end
  end

  test "inline bottom borders paint blue without recoloring white text" do
    assert blue_pixels(
             pixels([Border.width_each(0, 0, 1, 0), Border.color(color_rgb(43, 145, 203))])
           ) > 40
  end

  test "the supplied nested quote paints its bottom border" do
    tree =
      column([center_x(), center_y(), spacing(48), padding(48), width(fill())], [
        el([Font.color(color(:white)), center_x()], text("User Interface")),
        el(
          [Font.color(color(:white)), Font.size(48), width(fill())],
          paragraph([center_x()], [
            text("“User Interface is a point of "),
            el(
              [Border.width_each(0, 0, 1, 0), Border.color(color_rgb(43, 145, 203))],
              text("user interaction")
            ),
            text(" with the system”")
          ])
        )
      ])

    data =
      TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: 1280, height: 720], 30_000)

    assert blue_pixels(data) > 200
  end

  test "inline shadow and glow paint without a border" do
    for effect <- [
          Border.shadow(color: color_rgb(43, 145, 203), offset: {0, 3}, blur: 2),
          Border.glow(color_rgb(43, 145, 203), 2),
          Border.inner_shadow(color: color_rgb(43, 145, 203), size: 2, blur: 0)
        ] do
      assert blue_pixels(pixels([effect])) > 20
    end
  end

  test "glow is identical to its shadow expansion and transparent decoration stays transparent" do
    blue = color_rgb(43, 145, 203)

    assert pixels([Border.glow(blue, 2)]) ==
             pixels([Border.shadow(color: blue, size: 2, blur: 4, offset: {0, 0})])

    assert pixels([
             Border.shadow(color: color_rgba(0, 0, 255, 0)),
             Border.inner_shadow(color: color_rgba(0, 0, 255, 0))
           ]) == pixels([])
  end

  test "all border styles and corners render at fractional scale" do
    blue = color_rgb(43, 145, 203)
    gradient = Color.gradient([blue, color_rgba(0, 100, 255, 0.3), blue], 35)

    for scale <- [1, 1.5],
        style <- [Border.solid(), Border.dashed(), Border.dotted()],
        paint <- [blue, gradient] do
      tree =
        paragraph([width(px(180)), padding(16), Font.size(24), Font.color(:white), center_x()], [
          el(
            [
              Border.width_each(1, 2, 3, 4),
              Border.rounded_each(8, 0, 4, 2),
              Border.color(paint),
              style
            ],
            text("user interaction")
          )
        ])

      data =
        TreeRenderer.render_to_pixels(
          tree,
          [otp_app: :emerge, width: 300, height: 300, scale: scale],
          30_000
        )

      assert blue_pixels(data) > 30
    end
  end

  test "live decoration patches, reflow and parent movement match fresh pixels" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 240,
        height: 200,
        headless: [target: self(), pixel_format: :rgba8888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)
    blue = color_rgb(43, 145, 203)
    border = [Border.width_each(1, 2, 3, 4), Border.color(blue)]
    shadow = Border.shadow(color: blue, offset: {3, 4}, size: 2, blur: 4)
    inset = Border.inner_shadow(color: color_rgba(255, 0, 0, 0.6), size: 2, blur: 1)

    cases = [
      {[], 180, 0},
      {[shadow], 180, 0},
      {[Background.color(:black), shadow], 180, 0},
      {[Background.color(color_rgba(0, 0, 0, 0.5)), shadow], 180, 0},
      {[Background.color(color_rgba(0, 0, 0, 0)), shadow], 180, 0},
      {[
         Background.color(Color.gradient([blue, color_rgba(255, 0, 0, 0.3)], 90)),
         Border.rounded_each(12, 0, 4, 0),
         shadow
       ], 140, 30},
      {[Background.color(:black)], 140, 30},
      {[shadow, inset], 180, 0},
      {[inset, shadow, Border.glow(color_rgba(0, 255, 0, 0.3), 3)], 180, 0},
      {border, 180, 0},
      {[Border.dashed(), Border.rounded_each(12, 0, 6, 2), shadow | border], 180, 0},
      {[Border.dotted(), inset | border], 140, 0},
      {[Border.rounded_each(12, 0, 6, 2), shadow | border], 140, 30},
      {[shadow, Border.color(Color.gradient([blue, color(:red)], 90)), Border.width(3)], 180, 10},
      {[], 180, 0},
      {:remove_wrapper, 180, 0},
      {[shadow], 180, 0}
    ]

    Enum.reduce(cases, nil, fn {attrs, width, move}, state ->
      tree =
        el(
          [
            width(fill()),
            height(fill()),
            Background.color(:black),
            Font.color(:white),
            Font.size(24)
          ],
          paragraph([width(px(width)), padding(16), center_x(), Transform.move_x(move)], [
            text("before "),
            if(attrs == :remove_wrapper,
              do: text("user interaction"),
              else: el(attrs, text("user interaction"))
            ),
            text(" after")
          ])
        )

      expected =
        TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: 240, height: 200], 30_000)

      {next, _} =
        if state,
          do: EmergeSkia.patch_tree(renderer, state, tree),
          else: EmergeSkia.upload_tree(renderer, tree)

      await_pixels(expected, System.monotonic_time(:millisecond) + 5000)
      next
    end)
  end

  test "inline background alpha controls interior shadow visibility at every orbit position" do
    render = fn shadow, background ->
      paragraph([width(px(280)), padding(24), Font.size(24), Font.color(:white), center_x()], [
        el(
          [
            Background.color(background),
            Border.width_each(0, 0, 1, 0),
            Border.color(color_rgb(43, 145, 203)),
            shadow
          ],
          text("MMMM MMMM")
        )
      ])
      |> TreeRenderer.render_to_pixels([otp_app: :emerge, width: 280, height: 100], 30_000)
    end

    at_gap = fn pixels ->
      offset = (36 * 280 + 140) * 4
      <<r, g, b, a>> = binary_part(pixels, offset, 4)
      {r, g, b, a}
    end

    clear = render.(Border.shadow(color: color_rgba(43, 145, 203, 0)), color_rgba(0, 0, 0, 0))
    {0, 0, 0, _} = at_gap.(clear)

    for offset <- [
          {0, -14},
          {14, -14},
          {14, 0},
          {14, 14},
          {0, 14},
          {-14, 14},
          {-14, 0},
          {-14, -14}
        ] do
      shadow =
        Border.shadow(color: color_rgba(43, 145, 203, 0.26), blur: 14, size: 2, offset: offset)

      transparent = render.(shadow, color_rgba(0, 0, 0, 0))
      opaque = render.(shadow, color(:black))
      translucent = render.(shadow, color_rgba(0, 0, 0, 0.5))
      {r, g, b, alpha} = at_gap.(transparent)
      assert b > 8 and b > g and g > r, "missing interior shadow at #{inspect(offset)}"

      assert {0, 0, 0, 255} == at_gap.(opaque),
             "opaque background did not cover shadow at #{inspect(offset)}"

      {half_r, half_g, half_b, half_alpha} = at_gap.(translucent)
      assert_in_delta half_r, r / 2, 1
      assert_in_delta half_g, g / 2, 1
      assert_in_delta half_b, b / 2, 1
      assert_in_delta half_alpha, 128 + alpha * 127 / 255, 1
      # Above the wrapper's top edge, the background must not erase the halo.
      halo_offset = (20 * 280 + 140) * 4
      assert binary_part(opaque, halo_offset, 4) == binary_part(transparent, halo_offset, 4)
    end
  end

  test "ordinary backgrounds occlude the full box shadow according to their alpha" do
    for {alpha, expected} <- [
          {0, {0, 0, 128, 255}},
          {0.5, {128, 128, 192, 255}},
          {1, {255, 255, 255, 255}}
        ] do
      tree =
        el(
          [width(px(48)), height(px(48)), padding(12), Background.color(:black)],
          el(
            [
              width(px(24)),
              height(px(24)),
              Background.color(color_rgba(255, 255, 255, alpha)),
              Border.rounded_each(8, 0, 4, 0),
              Border.shadow(color: color_rgba(0, 0, 255, 0.5), offset: {0, 0}, size: 0, blur: 0)
            ],
            text("")
          )
        )

      pixels =
        TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: 48, height: 48], 30_000)

      <<r, g, b, a>> = binary_part(pixels, (24 * 48 + 24) * 4, 4)
      assert {r, g, b, a} == expected
    end
  end

  defp await_pixels(expected, deadline) do
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {:emerge_skia_frame, %VideoInterop.Frame{storage: %VideoInterop.Binary{data: data}}} ->
        if data != expected, do: await_pixels(expected, deadline)
    after
      remaining -> flunk("decoration patch did not match fresh pixels")
    end
  end
end
