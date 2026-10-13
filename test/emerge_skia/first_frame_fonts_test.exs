defmodule EmergeSkia.FirstFrameFontsTest do
  use ExUnit.Case
  use Emerge.UI

  alias Emerge.Config
  alias VideoInterop.{Binary, Frame}

  @width 180
  @height 40
  @family "first-frame-lobster"
  @fonts [[family: @family, source: "test_assets/Lobster-Regular.ttf", weight: 400]]

  test "font assets do not present a blank frame before the first tree upload" do
    expected =
      EmergeSkia.TreeRenderer.render_to_pixels(
        tree(),
        [otp_app: :emerge, width: @width, height: @height, assets: [fonts: @fonts]],
        30_000
      )

    {:ok, renderer} =
      EmergeSkia.start(%Config{
        output: %Config.Headless{
          target: self(),
          size: {@width, @height},
          frames: %Config.BinaryFrames{pixel_format: :rgba8888}
        },
        renderer: %Config.Renderer{api: :raster},
        assets: %Config.Assets{otp_app: :emerge, fonts: @fonts}
      })

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree())

    assert_receive {:emerge_skia_frame, %Frame{storage: %Binary{data: first}}}, 5_000
    assert first == expected
    assert EmergeSkia.render_to_pixels(renderer) == {:ok, expected}
  end

  defp tree do
    el(
      [
        width(px(@width)),
        height(px(@height)),
        Font.family(@family),
        Font.size(22),
        Font.color(:black),
        Background.color(:white)
      ],
      text("Asset Fonts 123")
    )
  end
end
