defmodule EmergeSkia.Macos.VideoTest do
  use ExUnit.Case, async: true
  alias EmergeSkia.Macos.Video

  test "packs cropped rows without stride padding and carries alpha mode" do
    frame =
      VideoInterop.Frame.binary(
        <<1, 2, 3, 255, 10, 20, 30, 128, 0, 0, 0, 0, 4, 5, 6, 255, 40, 50, 60, 128>>,
        width: 2,
        height: 2,
        stride: 12,
        pixel_format: :rgba8888,
        alpha_mode: :straight
      )

    frame = %{frame | visible_rect: %VideoInterop.Rect{x: 1, y: 0, width: 1, height: 2}}

    assert {:ok, <<1::16, "v", 1::32, 2::32, 1, 10, 20, 30, 128, 40, 50, 60, 128>>} =
             Video.encode(:v, frame)
  end

  test "rejects malformed and unsupported binary frames" do
    frame =
      VideoInterop.Frame.binary(<<1, 2, 3, 255>>, width: 1, height: 1, pixel_format: :rgba8888)

    assert {:error, _} = Video.encode(:v, %{frame | storage: %{frame.storage | data: <<>>}})
    rgb = VideoInterop.Frame.binary(<<1, 2, 3>>, width: 1, height: 1, pixel_format: :rgb888)
    assert {:error, :video_submission_unsupported} = Video.encode(:v, rgb)
  end
end
