defmodule EmergeSkiaTest do
  use ExUnit.Case
  doctest EmergeSkia
  use Emerge.UI

  alias Emerge.UI.Svg
  alias EmergeSkia.BuildConfig
  alias EmergeSkia.Macos.Renderer

  defp restore_env(name, nil), do: System.delete_env(name)
  defp restore_env(name, value), do: System.put_env(name, value)

  defp rgba_at(pixels, width, x, y) do
    offset = (y * width + x) * 4
    <<_::binary-size(^offset), r, g, b, a, _::binary>> = pixels
    {r, g, b, a}
  end

  defp png_dimensions(png) do
    <<137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, "IHDR", width::32, height::32, _::binary>> =
      png

    {width, height}
  end

  defp render_tree_to_pixels(tree, opts) do
    EmergeSkia.TreeRenderer.render_to_pixels(tree, opts, 30_000)
  end

  defp render_tree_to_png(tree, opts) do
    EmergeSkia.TreeRenderer.render_to_png(tree, opts, 30_000)
  end

  test "render_to_pixels returns RGBA binary" do
    tree = el([width(px(10)), height(px(10)), Emerge.UI.Background.color(:red)], none())

    pixels =
      render_tree_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)

    # 10x10 pixels, 4 bytes each = 400 bytes
    assert byte_size(pixels) == 400
  end

  test "render_to_pixels leaves uncovered pixels transparent by default" do
    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())

    pixels =
      render_tree_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)

    assert rgba_at(pixels, 10, 1, 1) == {255, 0, 0, 255}
    assert rgba_at(pixels, 10, 9, 9) == {0, 0, 0, 0}
  end

  test "render_to_png returns encoded PNG binary" do
    tree = el([width(px(10)), height(px(10)), Emerge.UI.Background.color(:red)], none())

    png =
      render_tree_to_png(tree, otp_app: :emerge, width: 10, height: 10)

    assert <<137, 80, 78, 71, 13, 10, 26, 10, _::binary>> = png
    assert png_dimensions(png) == {10, 10}
    assert byte_size(png) > 50
  end

  test "public screenshot APIs reject old tree-render signatures" do
    tree = el([width(px(10)), height(px(10))], none())

    assert_raise ArgumentError, ~r/now expects a renderer handle/, fn ->
      EmergeSkia.render_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)
    end

    assert_raise ArgumentError, ~r/now expects a renderer handle/, fn ->
      EmergeSkia.render_to_png(tree, otp_app: :emerge, width: 10, height: 10)
    end
  end

  test "render_to_pixels supports snapshot placeholders" do
    tree = image([width(px(32)), height(px(24))], "sample_assets/missing.jpg")

    snapshot =
      render_tree_to_pixels(
        tree,
        otp_app: :emerge,
        width: 32,
        height: 24,
        asset_mode: :snapshot
      )

    awaited =
      render_tree_to_pixels(tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(snapshot) == 32 * 24 * 4
    assert byte_size(awaited) == 32 * 24 * 4
    refute snapshot == awaited
  end

  test "render_to_pixels await mode resolves logical image assets" do
    good_tree = image([width(px(32)), height(px(24))], "sample_assets/static.jpg")
    bad_tree = image([width(px(32)), height(px(24))], "sample_assets/missing.jpg")

    good =
      render_tree_to_pixels(good_tree, otp_app: :emerge, width: 32, height: 24)

    bad =
      render_tree_to_pixels(bad_tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(good) == 32 * 24 * 4
    assert byte_size(bad) == 32 * 24 * 4
    refute good == bad
  end

  test "render_to_pixels resolves logical SVG image assets" do
    tree = image([width(px(8)), height(px(8)), image_fit(:cover)], "sample_assets/tile_quad.svg")

    pixels = render_tree_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 1, 1) == {255, 0, 0, 255}
    assert rgba_at(pixels, 8, 6, 1) == {0, 255, 0, 255}
    assert rgba_at(pixels, 8, 1, 6) == {0, 0, 255, 255}
    assert rgba_at(pixels, 8, 6, 6) == {255, 255, 0, 255}
  end

  test "render_to_pixels svg/2 preserves original multicolor SVGs by default" do
    tree = svg([width(px(8)), height(px(8)), image_fit(:cover)], "sample_assets/tile_quad.svg")

    pixels = render_tree_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 1, 1) == {255, 0, 0, 255}
    assert rgba_at(pixels, 8, 6, 1) == {0, 255, 0, 255}
    assert rgba_at(pixels, 8, 1, 6) == {0, 0, 255, 255}
    assert rgba_at(pixels, 8, 6, 6) == {255, 255, 0, 255}
  end

  test "render_to_pixels svg/2 applies template tint when Svg.color is set" do
    tree =
      svg(
        [
          width(px(8)),
          height(px(8)),
          image_fit(:cover),
          Svg.color({:color_rgb, {255, 255, 255}})
        ],
        "sample_assets/tile_quad.svg"
      )

    pixels = render_tree_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 1, 1) == {255, 255, 255, 255}
    assert rgba_at(pixels, 8, 6, 1) == {255, 255, 255, 255}
    assert rgba_at(pixels, 8, 1, 6) == {255, 255, 255, 255}
    assert rgba_at(pixels, 8, 6, 6) == {255, 255, 255, 255}
  end

  test "render_to_pixels svg/2 fails when source resolves to raster" do
    bad_tree = svg([width(px(32)), height(px(24))], "sample_assets/static.jpg")
    failed_tree = image([width(px(32)), height(px(24))], "sample_assets/missing.jpg")

    bad = render_tree_to_pixels(bad_tree, otp_app: :emerge, width: 32, height: 24)
    failed = render_tree_to_pixels(failed_tree, otp_app: :emerge, width: 32, height: 24)

    assert byte_size(bad) == 32 * 24 * 4
    assert bad == failed
  end

  test "render_to_pixels resolves logical SVG background repeat assets" do
    tree =
      el(
        [
          width(px(8)),
          height(px(8)),
          Emerge.UI.Background.image("sample_assets/tile_quad.svg", fit: :repeat)
        ],
        none()
      )

    pixels = render_tree_to_pixels(tree, otp_app: :emerge, width: 8, height: 8)

    assert byte_size(pixels) == 8 * 8 * 4
    assert rgba_at(pixels, 8, 0, 0) == {255, 0, 0, 255}
    assert rgba_at(pixels, 8, 1, 0) == {0, 255, 0, 255}
    assert rgba_at(pixels, 8, 0, 1) == {0, 0, 255, 255}
    assert rgba_at(pixels, 8, 1, 1) == {255, 255, 0, 255}
    assert rgba_at(pixels, 8, 0, 0) == rgba_at(pixels, 8, 2, 0)
    assert rgba_at(pixels, 8, 0, 0) == rgba_at(pixels, 8, 0, 2)
  end

  test "input mask constants" do
    assert EmergeSkia.input_mask_key() == 0x01
    assert EmergeSkia.input_mask_codepoint() == 0x02
    assert EmergeSkia.input_mask_cursor_pos() == 0x04
    assert EmergeSkia.input_mask_cursor_button() == 0x08
    assert EmergeSkia.input_mask_cursor_scroll() == 0x10
    assert EmergeSkia.input_mask_cursor_enter() == 0x20
    assert EmergeSkia.input_mask_resize() == 0x40
    assert EmergeSkia.input_mask_focus() == 0x80
    assert EmergeSkia.input_mask_all() == 0xFF
  end

  test "start/1 requires otp_app option" do
    assert_raise ArgumentError, ~r/missing required :otp_app option/, fn ->
      EmergeSkia.start(title: "No otp app")
    end
  end

  test "start/1 validates otp_app type" do
    assert_raise ArgumentError, ~r/otp_app must be an atom/, fn ->
      EmergeSkia.start(otp_app: "emerge")
    end
  end

  test "start/1 rejects removed legacy window backends" do
    assert {:error, {:error, "backend :wayland_legacy has been removed; use :wayland"}} =
             EmergeSkia.start(otp_app: :emerge, backend: :wayland_legacy)
  end

  test "start/1 rejects unsupported backends" do
    assert {:error, {:error, "unsupported backend: bogus"}} =
             EmergeSkia.start(otp_app: :emerge, backend: :bogus)
  end

  test "start/1 rejects removed macos_backend option" do
    assert_raise ArgumentError, ~r/macos_backend has been removed.*rendering_api/, fn ->
      EmergeSkia.start(otp_app: :emerge, backend: :drm, macos_backend: :raster)
    end
  end

  test "start/1 delegates Wayland Vulkan availability to the native feature matrix" do
    assert {:error, {:error, "Vulkan rendering support is not available in this build"}} =
             EmergeSkia.start(otp_app: :emerge, backend: :wayland, rendering_api: :vulkan)
  end

  test "start/1 validates the headless target before native startup" do
    assert_raise ArgumentError, ~r/:headless.target must be a live local pid/, fn ->
      EmergeSkia.start(otp_app: :emerge, backend: :headless)
    end
  end

  test "headless PRIME rejects raster renderer instead of falling back" do
    assert {:error, {:error, reason}} =
             EmergeSkia.start(
               otp_app: :emerge,
               backend: :headless,
               rendering_api: :raster,
               width: 4,
               height: 4,
               headless: [target: self(), mode: :prime]
             )

    assert reason =~ "raster cannot export dma-buf frames"
  end

  test "headless backend delivers binary frames" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 4,
        height: 4,
        headless: [target: self(), pixel_format: :rgb888]
      )

    assert {:ok,
            %{
              backend: :headless,
              rendering_api: %{requested: :raster, selected: :raster},
              vulkan_device: nil
            }} = EmergeSkia.renderer_info(renderer)

    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert_receive {:emerge_skia_frame, frame}, 1_000
    frame = Map.new(frame)
    assert frame["mode"] == "binary"
    assert frame["width"] == 4
    assert frame["height"] == 4
    assert frame["pixel_format"] == "rgb888"
    assert frame["stride_bytes"] == 12
    assert byte_size(frame["data"]) == 4 * 4 * 3

    assert :ok = EmergeSkia.stop(renderer)
  end

  @tag :hardware
  test "headless GL backend delivers binary frames when explicitly enabled" do
    if System.get_env("EMERGE_SKIA_HEADLESS_GL_TEST") == "1" do
      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :emerge,
          backend: :headless,
          rendering_api: :opengl,
          width: 4,
          height: 4,
          headless: [target: self(), pixel_format: :rgba8888]
        )

      assert {:ok, %{rendering_api: %{selected: :opengl}, capabilities: %{gpu: true}}} =
               EmergeSkia.renderer_info(renderer)

      tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
      {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

      assert_receive {:emerge_skia_frame, frame}, 1_000
      frame = Map.new(frame)
      assert frame["pixel_format"] == "rgba8888"
      assert byte_size(frame["data"]) == 4 * 4 * 4

      assert {:ok, pixels} = EmergeSkia.render_to_pixels(renderer)
      assert byte_size(pixels) == 4 * 4 * 4
      assert {:ok, png} = EmergeSkia.render_to_png(renderer)
      assert <<0x89, "PNG", _rest::binary>> = png

      assert :ok = EmergeSkia.stop(renderer)
    end
  end

  @tag :hardware
  @tag :headless_prime_hardware
  test "headless PRIME supported explicit-sync path delivers a canonical sync-file" do
    previous_force = System.get_env("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC")
    System.delete_env("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC")

    on_exit(fn ->
      restore_env("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC", previous_force)
    end)

    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :auto,
        width: 4,
        height: 4,
        headless: [target: self(), mode: :prime, prime: [max_in_flight: 1]]
      )

    assert {:ok,
            %{
              rendering_api: %{selected: :opengl},
              capabilities: %{gpu: true, screenshot: false}
            }} = EmergeSkia.renderer_info(renderer)

    assert {:error, "screenshot capture is not supported for headless PRIME output"} =
             EmergeSkia.render_to_png(renderer)

    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert_receive {:emerge_skia_frame, frame}, 1_000
    frame = Map.new(frame)
    assert frame["mode"] == "prime"
    assert frame["width"] == 4
    assert frame["height"] == 4

    dma_buf = frame["dmabuf"]

    assert %VideoInterop.Frame{
             coded_width: 4,
             coded_height: 4,
             visible_rect: %VideoInterop.Rect{x: 0, y: 0, width: 4, height: 4},
             storage: %VideoInterop.DMABuf.Descriptor{
               version: 1,
               objects: [object],
               layers: [%VideoInterop.DMABuf.Layer{fourcc: fourcc, planes: [plane]}]
             },
             acquire_sync: %VideoInterop.SyncFile{acquire_fence_fd: acquire_fence_fd},
             lease: %VideoInterop.Lease{} = lease
           } = dma_buf

    assert is_integer(acquire_fence_fd) and acquire_fence_fd >= 0
    assert {:ok, _stat} = File.stat("/proc/self/fd/#{acquire_fence_fd}")
    assert is_integer(object.fd) and object.fd >= 0
    assert object.size > 0
    assert object.modifier == :implicit or is_integer(object.modifier)
    assert is_integer(fourcc) and fourcc > 0
    assert plane.object_index == 0
    assert plane.pitch > 0
    assert plane.offset >= 0
    assert :ok = VideoInterop.validate(dma_buf)
    assert lease.owner != self()
    assert %VideoInterop.AbandonmentGuard{} = lease.abandonment_guard
    assert VideoInterop.AbandonmentGuard.valid?(lease.abandonment_guard)
    assert {:ok, child_lease} = VideoInterop.Lease.retain(lease)
    assert child_lease.token == lease.token
    assert child_lease.holder != lease.holder
    assert %VideoInterop.AbandonmentGuard{} = child_lease.abandonment_guard
    assert VideoInterop.AbandonmentGuard.valid?(child_lease.abandonment_guard)
    refute child_lease.abandonment_guard == lease.abandonment_guard

    assert :ok = VideoInterop.release(dma_buf)
    blocked_tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:blue)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, blocked_tree)
    refute_receive {:emerge_skia_frame, _frame}, 100

    assert :ok = VideoInterop.release(child_lease)
    assert %{active_leases: 0} = VideoInterop.LeaseOwner.stats(lease.owner)
    next_tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:green)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, next_tree)
    assert_receive {:emerge_skia_frame, next_frame}, 1_000
    next_dma_buf = next_frame |> Map.new() |> Map.fetch!("dmabuf")
    [%VideoInterop.DMABuf.Object{fd: next_fd}] = next_dma_buf.storage.objects
    session_monitor = Process.monitor(renderer.pid)
    test_pid = self()
    spawn(fn -> send(test_pid, {:stop_result, EmergeSkia.stop(renderer)}) end)
    refute_receive {:stop_result, _result}, 100
    assert {:ok, _stat} = File.stat("/proc/self/fd/#{next_fd}")

    assert :ok = VideoInterop.release(next_dma_buf)
    assert_receive {:stop_result, :ok}, 1_000
    refute EmergeSkia.running?(renderer)
    assert_receive {:DOWN, ^session_monitor, :process, _pid, :normal}, 1_000
  end

  @tag :hardware
  @tag :headless_prime_hardware
  test "headless PRIME forced implicit-sync path is explicit and non-vacuous" do
    previous_force = System.get_env("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC")
    System.put_env("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC", "1")

    on_exit(fn ->
      restore_env("EMERGE_SKIA_HEADLESS_PRIME_FORCE_IMPLICIT_SYNC", previous_force)
    end)

    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :opengl,
        width: 4,
        height: 4,
        headless: [target: self(), mode: :prime, prime: [max_in_flight: 1]]
      )

    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert_receive {:emerge_skia_frame, frame}, 1_000
    dma_buf = frame |> Map.new() |> Map.fetch!("dmabuf")
    assert %VideoInterop.Frame{acquire_sync: :implicit} = dma_buf
    assert :ok = VideoInterop.validate(dma_buf)
    assert :ok = VideoInterop.release(dma_buf)
    assert :ok = EmergeSkia.stop(renderer)
  end

  test "renderer_info reports macOS renderer selection without stats" do
    renderer = %Renderer{
      session_id: 1,
      host_id: 1,
      host_pid: 1,
      requested_rendering_api: :auto,
      rendering_api: :metal,
      renderer_cache_enabled: true
    }

    assert {:ok,
            %{
              backend: :macos,
              rendering_api: %{requested: :auto, selected: :metal},
              capabilities: %{
                gpu: true,
                renderer_cache: true,
                screenshot: false,
                raster_present: [],
                prime_video: false,
                prime_video_formats: []
              }
            }} = EmergeSkia.renderer_info(renderer)
  end

  test "start/1 rejects backends that were not compiled in" do
    {backend, message} =
      if :drm in BuildConfig.compiled_backends() do
        {:wayland,
         "Wayland backend is not compiled; add :wayland to config :emerge, compiled_backends: [...]"}
      else
        {:drm,
         "DRM backend is not compiled; add :drm to config :emerge, compiled_backends: [...]"}
      end

    assert {:error, {:error, ^message}} = EmergeSkia.start(otp_app: :emerge, backend: backend)
  end

  test "legacy start arities raise explicit otp_app guidance" do
    assert_raise ArgumentError, ~r/requires explicit otp_app/, fn ->
      EmergeSkia.start()
    end

    assert_raise ArgumentError, ~r/no longer supported/, fn ->
      EmergeSkia.start("Legacy")
    end
  end

  test "start/1 validates assets.fonts source type" do
    assert_raise ArgumentError, ~r/assets\.fonts\[\]\.source must be a logical string path/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        assets: [fonts: [[family: "my-font", source: {:path, "/tmp/font.ttf"}]]]
      )
    end
  end

  test "start/1 validates assets.fonts weight range" do
    assert_raise ArgumentError,
                 ~r/assets\.fonts\[\]\.weight must be an integer between 100 and 900/,
                 fn ->
                   EmergeSkia.start(
                     otp_app: :emerge,
                     assets: [
                       fonts: [[family: "my-font", source: "fonts/MyFont.ttf", weight: 50]]
                     ]
                   )
                 end
  end

  test "start/1 rejects duplicate font variants" do
    assert_raise ArgumentError, ~r/duplicate assets\.fonts entries/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        assets: [
          fonts: [
            [family: "my-font", source: "fonts/MyFont-Regular.ttf", weight: 400],
            [family: "my-font", source: "fonts/MyFont-Regular2.ttf", weight: 400]
          ]
        ]
      )
    end
  end

  test "start/1 validates assets.fonts extension allowlist" do
    assert_raise ArgumentError, ~r/extension must be one of/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        assets: [fonts: [[family: "my-font", source: "fonts/MyFont.woff2", weight: 400]]]
      )
    end
  end

  test "start/1 rejects drm_cursor on wayland" do
    assert_raise ArgumentError, ~r/drm_cursor is only supported with backend: :drm/, fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :wayland,
        drm_cursor: [default: [source: "sample_assets/tile_quad.svg", hotspot: {1, 1}]]
      )
    end
  end

  test "load_font_file/4 normalizes native ok tuple" do
    priv_dir = :code.priv_dir(:emerge) |> List.to_string()
    path = Path.join(priv_dir, "test_assets/Lobster-Regular.ttf")

    assert File.regular?(path)
    assert :ok = EmergeSkia.load_font_file("lobster-test", 400, false, path)
  end

  test "video targets implement canonical consumer format validation before native open" do
    target = %EmergeSkia.VideoTarget{
      id: "preview",
      width: 64,
      height: 32,
      mode: :prime,
      ref: make_ref()
    }

    wrong_size = %VideoInterop.Format{
      width: 16,
      height: 16,
      framerate: nil,
      storage: %VideoInterop.DMABuf.Format{
        fourcc: VideoInterop.DMABuf.FourCC.nv12(),
        modifier: :per_buffer
      }
    }

    assert VideoInterop.Consumer.impl_for(target)

    assert {:error, {:wrong_size, {16, 16}, {64, 32}}} =
             VideoInterop.open_consumer(target, wrong_size)

    assert {:error, {:unsupported_interlace_mode, :interlaced_top_first}} =
             VideoInterop.open_consumer(target, %{
               wrong_size
               | width: 64,
                 height: 32,
                 interlace_mode: :interlaced_top_first
             })
  end

  test "video_target/2 accepts :prime mode at the Elixir API layer" do
    assert_raise ArgumentError, ~r/argument error/, fn ->
      EmergeSkia.video_target(make_ref(), id: "preview", width: 64, height: 32, mode: :prime)
    end
  end
end
