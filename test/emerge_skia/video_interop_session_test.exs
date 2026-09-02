defmodule EmergeSkia.DirectVideoFrameTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  alias VideoInterop.{Binary, Format, Frame, Lease, Rect}
  alias VideoInterop.DMABuf
  alias VideoInterop.DMABuf.{Descriptor, FourCC, Layer, Object, Plane}

  test "binary headless output and video submission use VideoInterop.Frame" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 2,
        height: 1,
        headless: [target: self(), pixel_format: :rgba8888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    EmergeSkia.upload_tree(renderer, video([width(px(2)), height(px(1))], :preview))
    assert_receive {:emerge_skia_frame, %Frame{storage: %Binary{}} = output}, 1_000
    assert :ok = VideoInterop.validate(output)

    input =
      Frame.binary(<<255, 0, 0, 255, 0, 255, 0, 255>>,
        width: 2,
        height: 1,
        pixel_format: :rgba8888
      )

    assert :ok = EmergeSkia.submit_video_frame(renderer, :preview, input)

    assert_receive {:emerge_skia_frame,
                    %Frame{
                      coded_width: 2,
                      coded_height: 1,
                      storage: %Binary{
                        data: <<255, 0, 0, 255, 0, 255, 0, 255>>,
                        planes: [%Binary.Plane{offset: 0, stride: 8}]
                      },
                      lease: nil
                    }},
                   1_000
  end

  test "hidden targets consume and release borrowed frames without GPU setup" do
    {fd, file, path} = open_test_fd()

    on_exit(fn ->
      :ok = :file.close(file)
      File.rm(path)
    end)

    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 2,
        height: 1,
        headless: [target: self(), pixel_format: :rgba8888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)
    EmergeSkia.upload_tree(renderer, text("hidden"))
    assert_receive {:emerge_skia_frame, %Frame{}}, 1_000

    fourcc = FourCC.from_string!("AB24")
    token = make_ref()
    lease = Lease.new(self(), token)

    frame = %Frame{
      coded_width: 1,
      coded_height: 1,
      visible_rect: %Rect{x: 0, y: 0, width: 1, height: 1},
      format: %Format{
        width: 1,
        height: 1,
        framerate: nil,
        storage: %DMABuf.Format{fourcc: fourcc}
      },
      storage: %Descriptor{
        objects: [%Object{fd: fd, size: 4, modifier: :implicit}],
        layers: [
          %Layer{
            fourcc: fourcc,
            planes: [%Plane{object_index: 0, offset: 0, pitch: 4}]
          }
        ]
      },
      lease: lease
    }

    assert :ok = EmergeSkia.submit_video_frame(renderer, :preview, frame)
    assert_receive {:video_interop_release, ^token, holder}, 1_000
    assert holder == lease.holder
  end

  test "hidden targets consume and drop binary frames" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 2,
        height: 1,
        headless: [target: self(), pixel_format: :rgba8888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)
    EmergeSkia.upload_tree(renderer, text("hidden"))
    assert_receive {:emerge_skia_frame, %Frame{}}, 1_000

    frame = Frame.binary(<<0, 0, 0, 255>>, width: 1, height: 1, pixel_format: :rgba8888)
    assert :ok = EmergeSkia.submit_video_frame(renderer, :preview, frame)
  end

  defp open_test_fd do
    path =
      Path.join(System.tmp_dir!(), "emerge-video-frame-#{System.unique_integer([:positive])}")

    {:ok, file} = :file.open(String.to_charlist(path), [:read, :write])

    fd =
      "/proc/self/fd/*"
      |> Path.wildcard()
      |> Enum.find_value(fn fd_path ->
        case File.read_link(fd_path) do
          {:ok, ^path} -> fd_path |> Path.basename() |> String.to_integer()
          _other -> nil
        end
      end)

    {fd || flunk("could not find test file descriptor"), file, path}
  end
end
