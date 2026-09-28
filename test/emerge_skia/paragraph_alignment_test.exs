defmodule EmergeSkia.ParagraphAlignmentTest do
  use ExUnit.Case
  use Emerge.UI

  alias EmergeSkia.TreeRenderer

  defp render(tree, width, height) do
    tree
    |> TreeRenderer.render_to_pixels([otp_app: :emerge, width: width, height: height], 30_000)
    |> ink_bands(width)
  end

  # Scan pixels and rows once, accumulating only bounding boxes for text lines.
  defp ink_bands(pixels, width) do
    stride = width * 4

    {_, bands} =
      for <<row::binary-size(^stride) <- pixels>>, reduce: {0, []} do
        {y, bands} ->
          {_, left, right} =
            for <<r, g, b, a <- row>>, reduce: {0, width, -1} do
              {x, left, right} ->
                if a > 128 and r > 128 and g > 128 and b > 128 do
                  {x + 1, Kernel.min(left, x), x}
                else
                  {x + 1, left, right}
                end
            end

          bands =
            case bands do
              _ when right < 0 ->
                bands

              [%{bottom: bottom} = band | rest] when bottom == y - 1 ->
                [
                  %{
                    band
                    | left: Kernel.min(band.left, left),
                      right: Kernel.max(band.right, right),
                      bottom: y
                  }
                  | rest
                ]

              _ ->
                [%{left: left, right: right, top: y, bottom: y} | bands]
            end

          {y + 1, bands}
      end

    Enum.reverse(bands)
  end

  defp quote_tree(alignment) do
    column(
      [center_x(), center_y(), spacing(48), padding(48), width(fill())],
      [
        el([Font.color(color(:white)), Font.size(48), center_x()], text("User Interface")),
        el(
          [Font.color(color(:white)), Font.size(44), width(fill())],
          paragraph(alignment, [
            text("“User Interface is a point of user interaction with the system”")
          ])
        )
      ]
    )
  end

  test "paragraph alignment positions each line of the supplied quote, not only its box" do
    [heading | left_lines] = render(quote_tree([align_left()]), 1280, 360)
    assert length(left_lines) == 2
    assert Enum.all?(left_lines, &(&1.left in 48..54))

    for alignment <- [[center_x()], [Font.center()]] do
      assert [^heading | centered] = render(quote_tree(alignment), 1280, 360)
      assert length(centered) == 2

      for {left, center} <- Enum.zip(left_lines, centered) do
        assert_in_delta (center.left + center.right) / 2, 640, 4
        assert_in_delta center.right - center.left, left.right - left.left, 1
        assert center.top == left.top
        assert center.bottom == left.bottom
      end
    end

    for alignment <- [[align_right()], [Font.align_right()]] do
      assert [^heading | right_lines] = render(quote_tree(alignment), 1280, 360)
      assert length(right_lines) == 2

      for {left, right} <- Enum.zip(left_lines, right_lines) do
        assert_in_delta right.right, 1231, 4
        assert_in_delta right.right - right.left, left.right - left.left, 1
        assert right.top == left.top
        assert right.bottom == left.bottom
      end
    end
  end

  test "live alignment patches and inherited changes match fresh raster frames" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 200,
        height: 100,
        headless: [target: self(), pixel_format: :rgba8888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    cases = [
      {[align_left()], [], 140},
      {[center_x()], [], 140},
      {[align_right()], [], 140},
      {[align_left()], [], 140},
      {[], [Font.center()], 140},
      {[], [Font.align_right()], 140},
      {[], [Font.align_left()], 140},
      {[center_x()], [], 100},
      {[center_x(), Font.align_left()], [Font.align_right()], 140}
    ]

    Enum.reduce(cases, nil, fn {alignment, inherited, paragraph_width}, state ->
      tree =
        el(
          [
            width(fill()),
            height(fill()),
            Background.color(:black),
            Font.color(:white),
            Font.size(20) | inherited
          ],
          paragraph([width(px(paragraph_width)), padding(10) | alignment], [text("AAAA BBBB C")])
        )

      expected =
        TreeRenderer.render_to_pixels(tree, [otp_app: :emerge, width: 200, height: 100], 30_000)

      {state, _} =
        if state do
          EmergeSkia.patch_tree(renderer, state, tree)
        else
          EmergeSkia.upload_tree(renderer, tree)
        end

      await_pixels(expected, System.monotonic_time(:millisecond) + 5_000)
      state
    end)
  end

  defp await_pixels(expected, deadline) do
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {:emerge_skia_frame, %VideoInterop.Frame{storage: %VideoInterop.Binary{data: data}}} ->
        if data != expected, do: await_pixels(expected, deadline)
    after
      remaining -> flunk("paragraph patch did not match the fresh raster frame")
    end
  end

  test "small paragraphs align within padded content and explicit font alignment wins" do
    make_tree = fn alignment ->
      paragraph(
        [width(px(140)), padding(10), Font.size(20), Font.color(:white) | alignment],
        [text("AAAA BBBB C")]
      )
    end

    left = render(make_tree.([align_left()]), 140, 100)
    assert length(left) == 2
    assert Enum.all?(left, &(&1.left in 10..13))
    assert render(make_tree.([center_x(), Font.align_left()]), 140, 100) == left

    for line <- render(make_tree.([center_x()]), 140, 100) do
      assert_in_delta (line.left + line.right) / 2, 70, 2
    end

    for line <- render(make_tree.([align_right()]), 140, 100) do
      assert_in_delta line.right, 129, 2
    end
  end
end
