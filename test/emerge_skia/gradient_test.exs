defmodule EmergeSkia.GradientTest do
  use ExUnit.Case
  use Emerge.UI

  alias Emerge.Engine.{Patch, Serialization}
  alias EmergeSkia.{Native, TreeRenderer}

  defp panel(color), do: el([width(px(100)), height(px(50)), Background.color(color)], none())

  defp render(tree) do
    TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: 100, height: 50], 30_000)
  end

  defp pixel(data, x, y) do
    offset = (y * 100 + x) * 4
    <<_::binary-size(^offset), r, g, b, a, _::binary>> = data
    {r, g, b, a}
  end

  test "native rendering includes interior stops, alpha, duplicates, and large angles" do
    rgb = [:red, :green, :blue]
    pixels = render(panel(gradient(rgb)))
    {r, g, b, 255} = pixel(pixels, 50, 25)
    assert g > 245 and r < 10 and b < 10

    refute pixels == render(panel(gradient([:red, :white, :blue])))
    assert render(panel(gradient(List.duplicate(:red, 4)))) == render(panel(:red))
    translucent = color_rgba(20, 40, 60, 0.5)
    assert render(panel(gradient(List.duplicate(translucent, 4)))) == render(panel(translucent))

    huge = 1.0e300

    assert render(panel(gradient(rgb, huge))) ==
             render(panel(gradient(rgb, :math.fmod(huge, 360))))

    assert render(panel(gradient(rgb, -270))) == render(panel(gradient(rgb, 90)))
    refute render(panel(gradient(rgb, 90))) == pixels
  end

  test "two-stop output retains the original diagonal-axis geometry" do
    pixels = render(panel(gradient([:black, :white])))

    for x <- [0, 25, 50, 75, 99] do
      {r, g, b, 255} = pixel(pixels, x, 25)
      expected = round(255 * (0.5 + (x + 0.5 - 50) / :math.sqrt(100 * 100 + 50 * 50)))
      assert abs(r - expected) <= 1
      assert r == g and g == b
    end
  end

  test "native roundtrip and retained attr patches preserve all gradient stops" do
    {bytes, assigned} = Serialization.encode(panel(gradient([:red, :green, :blue], 45)))
    tree = Native.tree_new()
    assert {:ok, roundtrip} = Native.tree_upload_roundtrip(tree, bytes)
    assert Serialization.decode(roundtrip).attrs.background == assigned.attrs.background

    attrs = Map.put(assigned.attrs, :background, gradient([:red, :white, :blue], 90))
    patch = Patch.encode([{:set_attrs, assigned.id, attrs}])
    assert {:ok, updated} = Native.tree_patch_roundtrip(tree, patch)
    decoded = Serialization.decode(updated)
    assert decoded.attrs.background == attrs.background
    assert render(decoded) == render(panel(gradient([:red, :white, :blue], 90)))

    <<"EMRG", 9, rest::binary>> = bytes
    assert {:error, _} = Native.tree_upload(tree, <<"EMRG", 7, rest::binary>>)
    assert {:error, _} = Native.tree_patch(tree, <<1, assigned.id::64, 4::32, 0, 1, 12, 1>>)
  end

  test "headless retained patches repaint middle-stop changes and solid switches" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 100,
        height: 50,
        headless: [target: self(), pixel_format: :rgb888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    {state, _} = EmergeSkia.upload_tree(renderer, panel(gradient([:red, :green, :blue])))
    wait_for_center(fn {r, g, b} -> g > 245 and r < 10 and b < 10 end)
    {state, _} = EmergeSkia.patch_tree(renderer, state, panel(gradient([:red, :white, :blue])))
    wait_for_center(fn {r, g, b} -> r > 245 and g > 245 and b > 245 end)
    {state, _} = EmergeSkia.patch_tree(renderer, state, panel(:red))
    wait_for_center(&(&1 == {255, 0, 0}))

    {_state, _} =
      EmergeSkia.patch_tree(renderer, state, panel(gradient(List.duplicate(:blue, 4))))

    wait_for_center(&(&1 == {0, 0, 255}))
  end

  test "every public color consumer renders gradients and constant gradients match solids" do
    size = [width(px(100)), height(px(50))]
    text_attrs = [Font.size(20), Font.underline(), Font.strike(), Font.letter_spacing(0.5)]

    consumers =
      [
        fn c -> panel(c) end,
        fn c -> el([Font.color(c) | size ++ text_attrs], text("MMMMMMMM")) end,
        fn c ->
          paragraph([Font.color(c) | size ++ text_attrs], [text("MMMM MMMM"), text(" MMMM")])
        end,
        fn c -> Input.text([Font.color(c) | size ++ text_attrs], "MMMMMMMM") end,
        fn c -> Input.multiline([Font.color(c) | size ++ text_attrs], "MMMM\nMMMM") end
      ] ++
        for style <- [Border.solid(), Border.dashed(), Border.dotted()],
            widths <- [Border.width(8), Border.width_each(4, 8, 12, 6)] do
          fn c ->
            el([Border.color(c), widths, style, Border.rounded_each(3, 6, 9, 12) | size], none())
          end
        end ++
        for shadow <- [&Border.shadow/1, &Border.inner_shadow/1] do
          fn c ->
            el(
              size,
              el(
                [
                  width(px(70)),
                  height(px(30)),
                  Transform.move_x(12),
                  Transform.move_y(10),
                  shadow.(color: c, blur: 4, size: 3),
                  Border.rounded(5)
                ],
                none()
              )
            )
          end
        end ++
        [
          fn c ->
            el(
              size,
              el(
                [
                  width(px(70)),
                  height(px(30)),
                  Transform.move_x(12),
                  Transform.move_y(10),
                  Border.glow(c, 4)
                ],
                none()
              )
            )
          end
        ] ++
        for fit <- [:contain, :cover] do
          fn c -> svg([Svg.color(c), image_fit(fit) | size], "test_assets/gradient_mask.svg") end
        end

    for {build, index} <- Enum.with_index(consumers) do
      a = render(build.(gradient([:red, :green, :blue])))
      b = render(build.(gradient([:red, :white, :blue])))
      refute a == b, "consumer #{index} lost interior stops"

      for c <- [:red, color_rgba(20, 40, 60, 0.5), color_rgba(255, 255, 255, 0)] do
        assert render(build.(gradient(List.duplicate(c, 3)))) == render(build.(c)),
               "consumer #{index} changed constant-gradient semantics"
      end
    end
  end

  test "reported transparent SVG gradient leaves the background untouched" do
    clear = color_rgba(255, 255, 255, 0)

    for fit <- [:contain, :cover] do
      tree =
        el(
          [width(px(100)), height(px(50)), Background.color(:blue)],
          svg(
            [width(fill()), height(fill()), image_fit(fit), Svg.color(gradient([clear, clear]))],
            "test_assets/gradient_mask.svg"
          )
        )

      assert render(tree) == render(panel(:blue))
    end
  end

  test "font border shadow and SVG gradient attrs survive native roundtrip and patches" do
    gradient = gradient([:red, :green, :blue])

    element =
      svg(
        [
          width(px(100)),
          height(px(50)),
          Font.color(gradient),
          Border.color(gradient),
          Border.shadow(color: gradient),
          Svg.color(gradient)
        ],
        "test_assets/gradient_mask.svg"
      )

    {bytes, assigned} = Serialization.encode(element)
    tree = Native.tree_new()
    assert {:ok, bytes} = Native.tree_upload_roundtrip(tree, bytes)

    assert Serialization.decode(bytes).attrs ==
             Emerge.Engine.Tree.Attrs.strip_runtime_attrs(assigned.attrs)

    changed = gradient([:red, :white, :blue], 90)

    for key <- [:font_color, :border_color, :svg_color] do
      attrs = Map.put(assigned.attrs, key, changed)

      assert {:ok, bytes} =
               Native.tree_patch_roundtrip(tree, Patch.encode([{:set_attrs, assigned.id, attrs}]))

      assert Serialization.decode(bytes).attrs ==
               Emerge.Engine.Tree.Attrs.strip_runtime_attrs(attrs)
    end
  end

  test "retained inherited paragraph colors and SVG tints match fresh rendering after patches" do
    size = [width(px(100)), height(px(50)), Background.color(:white)]

    builders = [
      fn c ->
        el(
          [Font.color(c), Font.size(20) | size],
          paragraph([width(fill()), height(fill())], [text("MMMM MMMM"), text(" MMMM")])
        )
      end,
      fn c ->
        el(
          size,
          svg([width(fill()), height(fill()), Svg.color(c)], "test_assets/gradient_mask.svg")
        )
      end
    ]

    for build <- builders do
      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :emerge,
          backend: :headless,
          rendering_api: :raster,
          width: 100,
          height: 50,
          headless: [target: self(), pixel_format: :rgb888]
        )

      on_exit(fn -> EmergeSkia.stop(renderer) end)
      initial = build.(gradient([:red, :green, :blue]))
      {state, _} = EmergeSkia.upload_tree(renderer, initial)
      expected = rgb(render(initial))
      wait_for_frame(fn %{storage: %{data: data}} -> data == expected end)

      _state =
        Enum.reduce(
          [
            gradient([:red, :white, :blue]),
            gradient([:red, :white, :blue], 90),
            :black,
            gradient([:black, :white])
          ],
          state,
          fn color, state ->
            tree = build.(color)
            expected = rgb(render(tree))
            {state, _} = EmergeSkia.patch_tree(renderer, state, tree)
            wait_for_frame(fn %{storage: %{data: data}} -> data == expected end)
            state
          end
        )

      assert :ok = EmergeSkia.stop(renderer)
    end
  end

  defp rgb(data), do: for(<<r, g, b, _a <- data>>, into: <<>>, do: <<r, g, b>>)

  test "SVG gradient alpha survives direct Gray8 and packed grayscale output" do
    for format <- [:gray8, :gray2] do
      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :emerge,
          backend: :headless,
          rendering_api: :raster,
          width: 100,
          height: 50,
          headless: [target: self(), pixel_format: format, dither: false]
        )

      on_exit(fn -> EmergeSkia.stop(renderer) end)

      build = fn color ->
        el(
          [width(px(100)), height(px(50)), Background.color(:white)],
          svg([width(fill()), height(fill()), Svg.color(color)], "test_assets/gradient_mask.svg")
        )
      end

      {state, _} =
        EmergeSkia.upload_tree(renderer, build.(gradient([color_rgba(0, 0, 0, 0), :black])))

      wait_for_frame(fn frame -> luma(frame, 50, 25) == 255 and luma(frame, 90, 25) < 128 end)

      EmergeSkia.patch_tree(
        renderer,
        state,
        build.(gradient([color_rgba(255, 255, 255, 0), color_rgba(255, 0, 0, 0)]))
      )

      wait_for_frame(fn frame ->
        Enum.all?(0..49, fn y -> Enum.all?(0..99, fn x -> luma(frame, x, y) == 255 end) end)
      end)

      assert :ok = EmergeSkia.stop(renderer)
    end
  end

  defp luma(
         %VideoInterop.Frame{
           format: %{storage: %{pixel_format: format}},
           storage: %VideoInterop.Binary{data: data, planes: [%{stride: stride} | _]}
         },
         x,
         y
       ) do
    bits = if format == :gray8, do: 8, else: 2
    per_byte = div(8, bits)
    byte = :binary.at(data, y * stride + div(x, per_byte))
    max_value = Bitwise.bsl(1, bits) - 1
    value = Bitwise.band(Bitwise.bsr(byte, 8 - bits - rem(x, per_byte) * bits), max_value)
    div(value * 255, max_value)
  end

  defp wait_for_frame(predicate, deadline \\ nil) do
    deadline = deadline || System.monotonic_time(:millisecond) + 5_000
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {:emerge_skia_frame, %VideoInterop.Frame{} = frame} ->
        if predicate.(frame), do: frame, else: wait_for_frame(predicate, deadline)
    after
      remaining -> flunk("gradient pixels did not reach grayscale output")
    end
  end

  defp wait_for_center(predicate, deadline \\ nil) do
    deadline = deadline || System.monotonic_time(:millisecond) + 5_000
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {:emerge_skia_frame, %VideoInterop.Frame{storage: %VideoInterop.Binary{data: data}}} ->
        offset = (25 * 100 + 50) * 3
        <<_::binary-size(^offset), r, g, b, _::binary>> = data
        if predicate.({r, g, b}), do: :ok, else: wait_for_center(predicate, deadline)
    after
      remaining -> flunk("gradient update did not reach the headless output")
    end
  end
end
