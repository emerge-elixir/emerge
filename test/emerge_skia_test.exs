defmodule EmergeSkiaTest do
  use ExUnit.Case
  doctest EmergeSkia
  use Emerge.UI

  alias Emerge.UI.Svg
  alias EmergeSkia.BuildConfig
  alias EmergeSkia.Macos.Renderer
  alias VideoInterop.{Binary, Frame}
  alias VideoInterop.Binary.{Format, Plane}

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

  defp receive_latest_headless_frame(timeout \\ 1_000) do
    assert_receive {:emerge_skia_frame, frame}, timeout
    drain_headless_frames(frame, System.monotonic_time(:millisecond) + 50)
  end

  defp drain_headless_frames(frame, deadline) do
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {:emerge_skia_frame, newer} -> drain_headless_frames(newer, deadline)
    after
      remaining -> frame
    end
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

  test "measure_text uses an isolated temporary default-font context" do
    assert {width, line_height, ascent, descent} = EmergeSkia.measure_text("Hello", 16)
    assert width > 0
    assert line_height > 0
    assert ascent > 0
    assert descent >= 0
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

  test "render_to_pixels remains correct when decoded asset retention is disabled" do
    tree = image([width(px(32)), height(px(24))], "sample_assets/static.jpg")

    opts = [
      otp_app: :emerge,
      width: 32,
      height: 24,
      assets: [cache: [max_entries: 0, max_bytes: 0], decode_at_size: true]
    ]

    first = render_tree_to_pixels(tree, opts)
    second = render_tree_to_pixels(tree, opts)

    assert byte_size(first) == 32 * 24 * 4
    assert first == second
  end

  test "offscreen rendering loads fonts only into its temporary asset context" do
    tree =
      el(
        [
          width(px(180)),
          height(px(40)),
          Emerge.UI.Font.family("offscreen-lobster"),
          Emerge.UI.Font.size(22),
          Emerge.UI.Background.color(:white)
        ],
        text("Asset Fonts 123")
      )

    fallback = render_tree_to_pixels(tree, otp_app: :emerge, width: 180, height: 40)

    custom =
      render_tree_to_pixels(
        tree,
        otp_app: :emerge,
        width: 180,
        height: 40,
        assets: [
          fonts: [
            [
              family: "offscreen-lobster",
              source: "test_assets/Lobster-Regular.ttf",
              weight: 400
            ]
          ]
        ]
      )

    refute custom == fallback
    assert render_tree_to_pixels(tree, otp_app: :emerge, width: 180, height: 40) == fallback
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

  @tag :linux_only
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

    assert %Frame{
             coded_width: 4,
             coded_height: 4,
             format: %{storage: %Format{pixel_format: :rgb888}},
             storage: %Binary{data: data, planes: [%Plane{stride: 12}]},
             lease: nil
           } = receive_latest_headless_frame()

    assert byte_size(data) == 4 * 4 * 3

    assert :ok = EmergeSkia.stop(renderer)
  end

  @tag :tmp_dir
  test "concurrent headless renderers isolate asset roots and worker shutdown", %{
    tmp_dir: tmp_dir
  } do
    red_root = Path.join(tmp_dir, "red")
    blue_root = Path.join(tmp_dir, "blue")
    File.mkdir_p!(red_root)
    File.mkdir_p!(blue_root)

    write_solid_svg(Path.join(red_root, "shared.svg"), "#ff0000")
    write_solid_svg(Path.join(blue_root, "shared.svg"), "#0000ff")

    start_renderer = fn ->
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 4,
        height: 4,
        headless: [target: self(), pixel_format: :rgb888]
      )
    end

    {:ok, red_renderer} = start_renderer.()
    {:ok, blue_renderer} = start_renderer.()

    on_exit(fn ->
      EmergeSkia.stop(red_renderer)
      EmergeSkia.stop(blue_renderer)
    end)

    base_config = EmergeSkia.Assets.normalize_asset_config!(otp_app: :emerge)

    assert :ok =
             EmergeSkia.Assets.initialize_renderer_assets(
               red_renderer,
               Map.put(base_config, :priv_dir, red_root)
             )

    assert :ok =
             EmergeSkia.Assets.initialize_renderer_assets(
               blue_renderer,
               Map.put(base_config, :priv_dir, blue_root)
             )

    tree = image([width(px(4)), height(px(4))], "shared.svg")
    {_state, _assigned} = EmergeSkia.upload_tree(red_renderer, tree)
    assert_frame_color({255, 0, 0})

    {_state, _assigned} = EmergeSkia.upload_tree(blue_renderer, tree)
    assert_frame_color({0, 0, 255})

    assert :ok = EmergeSkia.stop(red_renderer)

    write_solid_svg(Path.join(blue_root, "shared.svg"), "#00ff00")

    assert :ok =
             EmergeSkia.Assets.initialize_renderer_assets(
               blue_renderer,
               Map.put(base_config, :priv_dir, blue_root)
             )

    {_state, _assigned} = EmergeSkia.upload_tree(blue_renderer, tree)
    assert_frame_color({0, 255, 0})
  end

  test "headless BW1 packs rows independently and expands Gray8 screenshots" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 9,
        height: 2,
        headless: [
          target: self(),
          pixel_format: :bw1,
          bw1_polarity: :one_is_white,
          dither: false
        ]
      )

    tree =
      el(
        [width(px(9)), height(px(2)), Emerge.UI.Background.color(:white)],
        none()
      )

    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert %Frame{
             format: %{
               storage: %Format{pixel_format: :bw1, bw1_polarity: :one_is_white}
             },
             storage: %Binary{
               data: <<0xFF, 0x80, 0xFF, 0x80>>,
               planes: [%Plane{stride: 2}]
             }
           } = receive_latest_headless_frame()

    assert {:ok, pixels} = EmergeSkia.render_to_pixels(renderer)
    assert pixels == :binary.copy(<<255, 255, 255, 255>>, 18)
    assert :ok = EmergeSkia.stop(renderer)
  end

  test "headless Gray2 packs canonical rows and expands Gray8 screenshots" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 5,
        height: 2,
        headless: [target: self(), pixel_format: :gray2, dither: false]
      )

    tree =
      el(
        [width(px(5)), height(px(2)), Emerge.UI.Background.color(:white)],
        none()
      )

    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert %Frame{
             format: %{storage: %Format{pixel_format: :gray2}},
             storage: %Binary{
               data: <<0xFF, 0xC0, 0xFF, 0xC0>>,
               planes: [%Plane{stride: 2}]
             }
           } = receive_latest_headless_frame()

    assert {:ok, pixels} = EmergeSkia.render_to_pixels(renderer)
    assert pixels == :binary.copy(<<255, 255, 255, 255>>, 10)
    assert :ok = EmergeSkia.stop(renderer)
  end

  test "packed grayscale dithering changes gradients but not text-only output" do
    render = fn pixel_format, dither, tree ->
      {:ok, renderer} =
        EmergeSkia.start(
          otp_app: :emerge,
          backend: :headless,
          rendering_api: :raster,
          width: 64,
          height: 36,
          headless: [
            target: self(),
            pixel_format: pixel_format,
            bw1_polarity: :one_is_white,
            dither: dither
          ]
        )

      {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
      assert %Frame{storage: %Binary{data: data}} = receive_latest_headless_frame()
      assert :ok = EmergeSkia.stop(renderer)
      data
    end

    text_tree =
      el(
        [
          width(px(64)),
          height(px(36)),
          Emerge.UI.Background.color(:white),
          Emerge.UI.Font.color(:black),
          Emerge.UI.Font.size(26)
        ],
        text("O")
      )

    gradient_tree =
      el(
        [
          width(px(64)),
          height(px(36)),
          Emerge.UI.Background.gradient(:black, :white)
        ],
        none()
      )

    for pixel_format <- [:bw1, :gray2] do
      assert render.(pixel_format, false, text_tree) ==
               render.(pixel_format, true, text_tree)

      refute render.(pixel_format, false, gradient_tree) ==
               render.(pixel_format, true, gradient_tree)
    end
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

      assert %Frame{
               format: %{storage: %Format{pixel_format: :rgba8888}},
               storage: %Binary{data: data}
             } = receive_latest_headless_frame()

      assert byte_size(data) == 4 * 4 * 4

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

    assert_receive {:emerge_skia_frame, dma_buf}, 1_000

    assert %VideoInterop.Frame{
             coded_width: 4,
             coded_height: 4,
             visible_rect: %VideoInterop.Rect{x: 0, y: 0, width: 4, height: 4},
             format: %VideoInterop.Format{
               acquire_sync: :sync_file,
               storage: %VideoInterop.DMABuf.Format{modifier: stream_modifier}
             },
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
    assert stream_modifier == object.modifier
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

    assert eventually(fn ->
             VideoInterop.LeaseOwner.stats(lease.owner).active_leases == 0
           end)

    next_tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:green)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, next_tree)
    assert_receive {:emerge_skia_frame, %VideoInterop.Frame{} = next_dma_buf}, 1_000
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
  test "headless Vulkan PRIME publishes the complete fd-backed allocation size" do
    assert :headless in BuildConfig.compiled_vulkan_backends(),
           "compile the test NIF with the headless Vulkan backend"

    prime_opts =
      case System.get_env("EMERGE_DEMO_PRIME_DRM_NODE") do
        nil -> [max_in_flight: 1]
        drm_node -> [max_in_flight: 1, drm_node: drm_node]
      end

    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :vulkan,
        width: 640,
        height: 420,
        headless: [target: self(), mode: :prime, prime: prime_opts]
      )

    assert {:ok, %{rendering_api: %{selected: :vulkan}}} = EmergeSkia.renderer_info(renderer)

    tree = el([width(px(640)), height(px(420)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert_receive {:emerge_skia_frame, dma_buf}, 5_000

    assert %VideoInterop.Frame{
             coded_width: 640,
             coded_height: 420,
             format: %VideoInterop.Format{
               acquire_sync: :sync_file,
               storage: %VideoInterop.DMABuf.Format{modifier: 0}
             },
             storage: %VideoInterop.DMABuf.Descriptor{
               objects: [%VideoInterop.DMABuf.Object{} = object],
               layers: [%VideoInterop.DMABuf.Layer{planes: [plane]} = layer]
             },
             acquire_sync: %VideoInterop.SyncFile{}
           } = dma_buf

    assert {:ok, %File.Stat{size: fd_allocation_size}} =
             File.stat("/proc/self/fd/#{object.fd}")

    visible_span = plane.offset + plane.pitch * 419 + 640 * 4
    assert layer.fourcc == :binary.decode_unsigned("AB24", :little)
    assert object.modifier == 0
    assert object.size == fd_allocation_size
    assert object.size >= visible_span
    assert :ok = VideoInterop.validate(dma_buf)
    assert :ok = VideoInterop.release(dma_buf)

    next_tree = el([width(px(640)), height(px(420)), Emerge.UI.Background.color(:blue)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, next_tree)
    assert_receive {:emerge_skia_frame, %VideoInterop.Frame{} = next_dma_buf}, 5_000
    assert :ok = VideoInterop.release(next_dma_buf)
    assert :ok = EmergeSkia.stop(renderer)
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

    assert_receive {:emerge_skia_frame, %VideoInterop.Frame{} = dma_buf}, 1_000

    assert %VideoInterop.Frame{
             format: %VideoInterop.Format{
               acquire_sync: :implicit,
               storage: %VideoInterop.DMABuf.Format{modifier: stream_modifier}
             },
             storage: %VideoInterop.DMABuf.Descriptor{
               objects: [%VideoInterop.DMABuf.Object{} = object]
             },
             acquire_sync: :implicit
           } = dma_buf

    assert stream_modifier == object.modifier
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

  test "load_font_file/5 loads into the selected renderer" do
    {:ok, renderer} =
      EmergeSkia.start(
        otp_app: :emerge,
        backend: :headless,
        rendering_api: :raster,
        width: 4,
        height: 4,
        headless: [target: self(), pixel_format: :rgb888]
      )

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    priv_dir = :code.priv_dir(:emerge) |> List.to_string()
    path = Path.join(priv_dir, "test_assets/Lobster-Regular.ttf")

    assert File.regular?(path)
    assert :ok = EmergeSkia.load_font_file(renderer, "lobster-test", 400, false, path)
  end

  defp write_solid_svg(path, color) do
    File.write!(
      path,
      ~s(<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#{color}"/></svg>)
    )
  end

  defp assert_frame_color({red, green, blue}) do
    assert %Frame{
             storage: %Binary{data: data, planes: [%Plane{stride: 12}]}
           } = receive_latest_headless_frame(2_000)

    assert data == :binary.copy(<<red, green, blue>>, 16)
  end

  defp eventually(predicate, timeout_ms \\ 1_000) do
    wait_until(predicate, System.monotonic_time(:millisecond) + timeout_ms)
  end

  defp wait_until(predicate, deadline) do
    if predicate.() do
      true
    else
      if System.monotonic_time(:millisecond) >= deadline do
        false
      else
        Process.sleep(10)
        wait_until(predicate, deadline)
      end
    end
  end
end
