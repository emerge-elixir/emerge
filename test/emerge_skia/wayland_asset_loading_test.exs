defmodule EmergeSkia.WaylandAssetLoadingTest do
  use ExUnit.Case, async: false
  use Emerge.UI

  alias Emerge.Config

  @moduletag :wayland_hardware
  @moduletag timeout: 60_000

  test "idle cached backgrounds present completed images without capture or animation wakes" do
    dir =
      Path.join(System.tmp_dir!(), "emerge-wayland-assets-#{System.unique_integer([:positive])}")

    File.mkdir_p!(dir)
    on_exit(fn -> File.rm_rf!(dir) end)

    # Large uncompressed sources keep the real worker busy long enough to
    # observe the pending frame, without adding a native test-only loading hook.
    sources =
      for {name, bgr, rgb} <- [
            {"red", <<0, 0, 255>>, <<255, 0, 0>>},
            {"green", <<0, 255, 0>>, <<0, 255, 0>>}
          ] do
        path = Path.join(dir, name <> ".bmp")
        write_bmp!(path, bgr)
        {path, rgb}
      end

    assert {:ok, renderer} =
             EmergeSkia.start(%Config{
               output: %Config.Window{
                 backend: :wayland,
                 title: "Emerge idle asset redraw #107",
                 size: {800, 600}
               },
               renderer: [api: :opengl],
               diagnostics: [stats: true, logs: [:render]],
               assets: [
                 otp_app: :emerge,
                 cache: [max_bytes: 128 * 1024 * 1024],
                 runtime_paths: [enabled: true, allowlist: [dir], max_file_size: 64_000_000]
               ]
             })

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    assert {:ok, %{backend: :wayland, rendering_api: %{selected: :opengl}}} =
             EmergeSkia.renderer_info(renderer)

    upload(renderer, nil)
    await_draw(0)
    await_frames(renderer, 1)
    assert {:ok, <<0, 0, 255>>} = capture(renderer)
    assert_idle(renderer)

    for {path, rgb} <- sources do
      flush_logs()
      before = frame_count(renderer)
      upload(renderer, path)
      await_draw(0)
      await_draw(1)
      # Read-only statistics prove both frames reached presentation before any
      # capture request, input, animation or unrelated tree update can wake it.
      await_frames(renderer, before + 2)
      assert {:ok, ^rgb} = capture(renderer)
      assert_idle(renderer)

      IO.puts(
        "Wayland idle completion: #{Path.basename(path)}, RGB=#{inspect(rgb)}, frames=#{frame_count(renderer)}"
      )
    end

    before = frame_count(renderer)
    upload(renderer, Path.join(dir, "missing.bmp"))
    await_frames(renderer, before + 1)
    assert_idle(renderer)
    assert {:ok, failed} = capture(renderer)
    refute failed in [<<0, 0, 255>>, <<0, 255, 0>>]
    assert_idle(renderer)
    IO.puts("Wayland idle failure: placeholder presented; renderer returned to idle")
  end

  defp upload(renderer, path) do
    image_attrs = if path, do: [Background.image({:path, path}, fit: :contain)], else: []

    photo = el([key(:photo), width(px(800)), height(px(600)) | image_attrs], none())

    overlay =
      el(
        [key(:overlay), width(px(800)), height(px(600)), Background.color(:blue)],
        photo
      )

    tree =
      el(
        [key(:root), width(px(800)), height(px(600)), Nearby.in_front(overlay)],
        none()
      )

    assert {%Emerge.Engine.DiffState{}, %Emerge.Engine.Element{}} =
             EmergeSkia.upload_tree(renderer, tree)
  end

  defp write_bmp!(path, bgr) do
    size = 4096
    pixels_size = size * size * 3

    header =
      <<"BM", pixels_size + 54::little-32, 0::little-32, 54::little-32, 40::little-32,
        size::little-32, size::little-32, 1::little-16, 24::little-16, 0::little-32,
        pixels_size::little-32, 0::little-32, 0::little-32, 0::little-32, 0::little-32>>

    File.write!(path, [header, List.duplicate(:binary.copy(bgr, size), size)])
  end

  defp capture(renderer) do
    EmergeSkia.render_to_pixels(renderer,
      region: {400, 300, 1, 1},
      pixel_format: :rgb888,
      timeout: 5_000
    )
  end

  defp frame_count(renderer) do
    assert {:ok, snapshot} = EmergeSkia.stats(renderer)
    snapshot.frames.frame_count
  end

  defp assert_idle(renderer) do
    # Allow any already-queued capture/commit callback to finish, then require
    # a quiet interval; neither checking stats nor this wait requests a redraw.
    Process.sleep(100)
    count = frame_count(renderer)
    Process.sleep(150)
    assert frame_count(renderer) == count, "an idle asset must not start a frame loop"
  end

  defp await_frames(renderer, minimum, deadline \\ nil) do
    deadline = deadline || System.monotonic_time(:millisecond) + 5_000

    if frame_count(renderer) < minimum do
      assert System.monotonic_time(:millisecond) < deadline, "idle presentation stalled"
      Process.sleep(5)
      await_frames(renderer, minimum, deadline)
    end
  end

  defp await_draw(images, deadline \\ nil) do
    deadline = deadline || System.monotonic_time(:millisecond) + 5_000
    remaining = Kernel.max(0, deadline - System.monotonic_time(:millisecond))

    receive do
      {:emerge_skia_log, _, "wayland_render", "draw start\n" <> rest} ->
        assert rest =~ "animate: false", "asset completion must not rely on animation"

        unless rest =~ "images=#{images} " do
          await_draw(images, deadline)
        end

      {:emerge_skia_log, _, _, _} ->
        await_draw(images, deadline)
    after
      remaining -> flunk("no Wayland draw with #{images} images before capture")
    end
  end

  defp flush_logs do
    receive do
      {:emerge_skia_log, _, _, _} -> flush_logs()
    after
      0 -> :ok
    end
  end
end
