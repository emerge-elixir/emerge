defmodule EmergeSkiaTest do
  use ExUnit.Case
  doctest EmergeSkia
  use Emerge.UI
  alias Emerge.UI.Svg
  alias EmergeSkia.BuildConfig
  alias EmergeSkia.Macos.Renderer
  alias VideoInterop.{Binary, Frame}
  alias VideoInterop.Binary.{Format, Plane}

  defp restore_env(name, nil) do
    System.delete_env(name)
  end

  defp restore_env(name, value) do
    System.put_env(name, value)
  end

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

  defp receive_latest_headless_frame(timeout \\ 1000) do
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
    pixels = render_tree_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)

    # 10x10 pixels, 4 bytes each = 400 bytes
    assert byte_size(pixels) == 400
  end

  test "render_to_pixels leaves uncovered pixels transparent by default" do
    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
    pixels = render_tree_to_pixels(tree, otp_app: :emerge, width: 10, height: 10)
    assert rgba_at(pixels, 10, 1, 1) == {255, 0, 0, 255}
    assert rgba_at(pixels, 10, 9, 9) == {0, 0, 0, 0}
  end

  test "render_to_png returns encoded PNG binary" do
    tree = el([width(px(10)), height(px(10)), Emerge.UI.Background.color(:red)], none())
    png = render_tree_to_png(tree, otp_app: :emerge, width: 10, height: 10)
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
      render_tree_to_pixels(tree, otp_app: :emerge, width: 32, height: 24, asset_mode: :snapshot)

    awaited = render_tree_to_pixels(tree, otp_app: :emerge, width: 32, height: 24)
    assert byte_size(snapshot) == 32 * 24 * 4
    assert byte_size(awaited) == 32 * 24 * 4
    refute snapshot == awaited
  end

  test "render_to_pixels await mode resolves logical image assets" do
    good_tree = image([width(px(32)), height(px(24))], "sample_assets/static.jpg")
    bad_tree = image([width(px(32)), height(px(24))], "sample_assets/missing.jpg")
    good = render_tree_to_pixels(good_tree, otp_app: :emerge, width: 32, height: 24)
    bad = render_tree_to_pixels(bad_tree, otp_app: :emerge, width: 32, height: 24)
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
      render_tree_to_pixels(tree,
        otp_app: :emerge,
        width: 180,
        height: 40,
        assets: [
          fonts: [
            [family: "offscreen-lobster", source: "test_assets/Lobster-Regular.ttf", weight: 400]
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

  test "single-axis SVG sizing places the next sibling immediately after the image" do
    for build <- [&svg/2, &image/2],
        {source, attrs, expected_width, expected_height} <- [
          {"test_assets/single_axis_square.svg", [height(px(68))], 68, 68},
          {"test_assets/single_axis_wide.svg", [height(px(68))], 136, 68},
          {"test_assets/single_axis_wide.svg", [width(px(68)), height(content())], 68, 34}
        ],
        scale <- [1, 2] do
      tree =
        row([], [
          build.(attrs, source),
          el([width(px(8)), height(px(8)), Emerge.UI.Background.color(:blue)], none())
        ])

      pixels =
        render_tree_to_pixels(tree, otp_app: :emerge, width: 320, height: 200, scale: scale)

      right = expected_width * scale
      bottom = expected_height * scale
      assert byte_size(pixels) == 320 * 200 * 4
      assert rgba_at(pixels, 320, 1, 1) == {255, 0, 0, 255}
      assert rgba_at(pixels, 320, right - 1, bottom - 1) == {255, 0, 0, 255}
      assert rgba_at(pixels, 320, right, 1) == {0, 0, 255, 255}
      assert rgba_at(pixels, 320, right + 8 * scale, 1) == {0, 0, 0, 0}
      # Relaxed image drawing intentionally bleeds by one device pixel.
      assert rgba_at(pixels, 320, 1, bottom + 2) == {0, 0, 0, 0}
    end
  end

  test "fill-axis SVGs and raster images grow the automatic dimension and move the next sibling" do
    for {build, source} <- [
          {&svg/2, "test_assets/single_axis_wide.svg"},
          {&image/2, "test_assets/single_axis_wide.svg"},
          {&image/2, "test_assets/single_axis_wide.png"}
        ],
        available_width <- [80, 300],
        explicit_content <- [false, true],
        axis <- [:width, :height],
        scale <- [1, 2] do
      {container, parent_attrs, image_attrs} =
        if axis == :width do
          {&column/2, [width(px(available_width))],
           [width(fill())] ++
             if explicit_content do
               [height(content())]
             else
               []
             end}
        else
          {&row/2, [height(px(div(available_width, 2)))],
           [height(fill())] ++
             if explicit_content do
               [width(content())]
             else
               []
             end}
        end

      tree =
        container.(parent_attrs, [
          build.([image_fit(:contain) | image_attrs], source),
          el([width(px(8)), height(px(8)), Emerge.UI.Background.color(:blue)], none())
        ])

      pixels =
        render_tree_to_pixels(tree, otp_app: :emerge, width: 640, height: 360, scale: scale)

      right = available_width * scale
      bottom = div(right, 2)
      assert rgba_at(pixels, 640, 1, 1) == {255, 0, 0, 255}
      assert rgba_at(pixels, 640, right - 1, bottom - 1) == {255, 0, 0, 255}

      {sibling_x, sibling_y} =
        if axis == :width do
          {1, bottom}
        else
          {right, 1}
        end

      assert rgba_at(pixels, 640, sibling_x, sibling_y) == {0, 0, 255, 255}
      assert rgba_at(pixels, 640, right - 1, bottom + 2) == {0, 0, 0, 0}
    end
  end

  test "a centered responsive SVG and its in_front tint occupy the same frame" do
    for viewport_width <- [160, 400], scale <- [1, 2] do
      source = "test_assets/single_axis_wide.svg"

      tree =
        column([width(fill()), height(fill()), padding(48), Background.color(:black)], [
          el([width(px(80)), height(px(20))], none()),
          el(
            [
              center_x(),
              center_y(),
              width(fill()),
              Nearby.in_front(svg([Svg.color(:white), width(fill())], source))
            ],
            svg([width(fill())], source)
          ),
          el([align_bottom(), width(px(80)), height(px(20))], none())
        ])

      width = viewport_width * scale
      height = 640 * scale

      pixels =
        render_tree_to_pixels(tree, otp_app: :emerge, width: width, height: height, scale: scale)

      left = 48 * scale
      right = width - left
      image_height = div(right - left, 2)
      top = div(height - image_height, 2)
      bottom = top + image_height

      # A displaced overlay either leaves red content exposed or paints white
      # outside the correctly centered image. Ignore the one-pixel draw bleed.
      assert rgba_at(pixels, width, left + 3, top + 3) == {255, 255, 255, 255}
      assert rgba_at(pixels, width, right - 3, bottom - 3) == {255, 255, 255, 255}
      assert rgba_at(pixels, width, left + 3, top - 3) == {0, 0, 0, 255}
      assert rgba_at(pixels, width, left + 3, bottom + 3) == {0, 0, 0, 255}
    end
  end

  test "a live renderer reuses parsed SVGs and exact-size pixels across scenes without placeholders" do
    {:ok, renderer} =
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {240, 220},
          frames: %Emerge.Config.BinaryFrames{pixel_format: :rgb888},
          target: self()
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    on_exit(fn -> EmergeSkia.stop(renderer) end)

    reference = fn tree ->
      rgba = render_tree_to_pixels(tree, otp_app: :emerge, width: 240, height: 220)
      for <<r, g, b, _a <- rgba>>, into: <<>>, do: <<r, g, b>>
    end

    scene = fn size, color ->
      row([], [
        svg([width(px(size)), height(px(size))], "test_assets/cache_complex.svg"),
        el([width(px(8)), height(px(8)), Emerge.UI.Background.color(color)], none())
      ])
    end

    # The cold load may show a placeholder. Wait for its completed image first.
    initial = scene.(24, :blue)
    expected = reference.(initial)
    EmergeSkia.upload_tree(renderer, initial)
    await_svg_cache_frame(fn data -> data == expected end)
    blank = el([width(px(240)), height(px(220)), Emerge.UI.Background.color(:green)], none())
    blank_pixels = reference.(blank)

    for size <- [68, 200, 24, 68, 200] do
      EmergeSkia.upload_tree(renderer, blank)
      await_svg_cache_frame(fn data -> data == blank_pixels end)
      tree = scene.(size, :blue)
      expected = reference.(tree)
      EmergeSkia.upload_tree(renderer, tree)
      # Find the first frame of the new scene by its blue sibling, not by
      # whether the SVG has finished loading. A placeholder must fail here.
      data =
        await_svg_cache_frame(fn data ->
          offset = (240 + size) * 3
          binary_part(data, offset, 3) == <<0, 0, 255>>
        end)

      assert data == expected
    end
  end

  defp await_svg_cache_frame(predicate, deadline \\ nil) do
    deadline = deadline || System.monotonic_time(:millisecond) + 5000
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {:emerge_skia_frame, %Frame{storage: %Binary{data: data}}} ->
        if predicate.(data) do
          data
        else
          await_svg_cache_frame(predicate, deadline)
        end
    after
      remaining -> flunk("timed out waiting for SVG cache frame")
    end
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

  test "raw input subscriptions are typed categories, not public numeric masks" do
    assert :key in Emerge.Config.Input.all_events()
    refute function_exported?(EmergeSkia, :input_mask_all, 0)
    refute function_exported?(EmergeSkia, :set_input_mask, 2)
  end

  test "start/1 requires otp_app option" do
    assert_raise ArgumentError, ~r/assets.otp_app is required/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Window{backend: :auto, size: {800, 600}, title: "No otp app"},
        assets: []
      })
    end
  end

  test "start/1 validates otp_app type" do
    assert_raise ArgumentError, ~r/assets.otp_app expects :module/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Window{backend: :auto, size: {800, 600}},
        assets: [otp_app: "emerge"]
      })
    end
  end

  for backend <- [:wayland_legacy, :bogus] do
    test "start/1 rejects unsupported backend #{backend}" do
      assert_raise ArgumentError, ~r/output.backend expects one of/, fn ->
        EmergeSkia.start(%Emerge.Config{output: %Emerge.Config.Window{backend: unquote(backend)}})
      end
    end
  end

  test "start/1 rejects removed macos_backend option" do
    assert_raise ArgumentError, ~r/keyword configuration was removed/, fn ->
      EmergeSkia.start(otp_app: :emerge, backend: :drm, macos_backend: :raster)
    end
  end

  # Availability tests must stay hardware-free in Vulkan-capable source profiles.
  if :wayland not in EmergeSkia.BuildConfig.compiled_vulkan_backends() do
    @tag :linux_only
    test "start/1 delegates Wayland Vulkan availability to the native feature matrix" do
      reason =
        if :wayland in EmergeSkia.BuildConfig.compiled_backends(),
          do: "Vulkan rendering support is not available in this build",
          else:
            "Wayland backend is not compiled; add :wayland to config :emerge, compiled_backends: [...]"

      assert {:error, {:error, ^reason}} =
               EmergeSkia.start(%Emerge.Config{
                 output: %Emerge.Config.Window{backend: :wayland, size: {800, 600}},
                 renderer: [api: :vulkan],
                 assets: [otp_app: :emerge]
               })
    end
  end

  test "start/1 validates the headless target before native startup" do
    assert_raise ArgumentError, ~r/output.target expects a live local PID/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{size: {800, 600}, frames: %Emerge.Config.BinaryFrames{}},
        assets: [otp_app: :emerge]
      })
    end
  end

  test "headless PRIME rejects raster renderer instead of falling back" do
    assert {:error, {:error, reason}} =
             EmergeSkia.start(%Emerge.Config{
               output: %Emerge.Config.Headless{
                 size: {4, 4},
                 frames: %Emerge.Config.PrimeFrames{},
                 target: self()
               },
               renderer: [api: :raster],
               assets: [otp_app: :emerge]
             })

    assert reason =~ "raster cannot export dma-buf frames"
  end

  test "headless backend delivers binary frames" do
    {:ok, renderer} =
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {4, 4},
          frames: %Emerge.Config.BinaryFrames{pixel_format: :rgb888},
          target: self()
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

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
  test("concurrent headless renderers isolate asset roots and worker shutdown", %{
    tmp_dir: tmp_dir
  }) do
    red_root = Path.join(tmp_dir, "red")
    blue_root = Path.join(tmp_dir, "blue")
    File.mkdir_p!(red_root)
    File.mkdir_p!(blue_root)
    write_solid_svg(Path.join(red_root, "shared.svg"), "#ff0000")
    write_solid_svg(Path.join(blue_root, "shared.svg"), "#0000ff")

    start_renderer = fn frame_message ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {4, 4},
          frames: %Emerge.Config.BinaryFrames{pixel_format: :rgb888},
          target: self(),
          frame_message: frame_message
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })
    end

    {:ok, red_renderer} = start_renderer.("red_asset_frame")
    {:ok, blue_renderer} = start_renderer.("blue_asset_frame")

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
    assert_frame_color("red_asset_frame", {255, 0, 0})
    {_state, _assigned} = EmergeSkia.upload_tree(blue_renderer, tree)
    assert_frame_color("blue_asset_frame", {0, 0, 255})
    assert :ok = EmergeSkia.stop(red_renderer)
    write_solid_svg(Path.join(blue_root, "shared.svg"), "#00ff00")

    assert :ok =
             EmergeSkia.Assets.initialize_renderer_assets(
               blue_renderer,
               Map.put(base_config, :priv_dir, blue_root)
             )

    {_state, _assigned} = EmergeSkia.upload_tree(blue_renderer, tree)
    assert_frame_color("blue_asset_frame", {0, 255, 0})
  end

  for pixel_format <- [:rgba8888, :bgra8888] do
    test "headless raster preserves pixel order and alpha in #{pixel_format} frames and RGBA screenshots" do
      pixel_format = unquote(pixel_format)

      {:ok, renderer} =
        EmergeSkia.start(%Emerge.Config{
          output: %Emerge.Config.Headless{
            size: {3, 2},
            frames: %Emerge.Config.BinaryFrames{pixel_format: pixel_format},
            target: self()
          },
          renderer: [api: :raster],
          assets: [otp_app: :emerge]
        })

      on_exit(fn -> EmergeSkia.stop(renderer) end)

      rows = [
        [{10, 20, 30, 255}, {200, 100, 50, 128}, {70, 90, 110, 255}],
        [{255, 100, 50, 0}, {80, 120, 160, 64}, {210, 160, 90, 255}]
      ]

      tree =
        column(
          [width(px(3)), height(px(2))],
          Enum.map(rows, fn colors ->
            row(
              [width(px(3)), height(px(1))],
              Enum.map(colors, fn color ->
                el(
                  [width(px(1)), height(px(1)), Emerge.UI.Background.color({:color_rgba, color})],
                  none()
                )
              end)
            )
          end)
        )

      {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

      assert %Frame{
               format: %{
                 storage: %Format{pixel_format: ^pixel_format},
                 alpha_mode: :premultiplied
               },
               storage: %Binary{data: data, planes: [%{stride: 12}]}
             } = frame = receive_latest_headless_frame()

      # Distinct pixels across rows catch swaps, row reordering, and alpha loss.
      rgba =
        <<10, 20, 30, 255, 100, 50, 25, 128, 70, 90, 110, 255, 0, 0, 0, 0, 20, 30, 40, 64, 210,
          160, 90, 255>>

      bgra = for <<r, g, b, a <- rgba>>, into: <<>>, do: <<b, g, r, a>>
      expected = Map.fetch!(%{rgba8888: rgba, bgra8888: bgra}, pixel_format)
      assert data == expected
      assert {:ok, ^rgba} = EmergeSkia.render_to_pixels(renderer)
      # Screenshot conversion must not mutate the stored BGRA snapshot.
      assert {:ok, ^rgba} = EmergeSkia.render_to_pixels(renderer)
      assert :ok = VideoInterop.validate(frame)
    end
  end

  test "headless BW1 packs rows independently and expands Gray8 screenshots" do
    {:ok, renderer} =
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {9, 2},
          frames: %Emerge.Config.BinaryFrames{
            pixel_format: :bw1,
            bw1_polarity: :one_is_white,
            dither: false
          },
          target: self()
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    tree =
      el(
        [width(px(9)), height(px(2)), Emerge.UI.Background.color(:white)],
        none()
      )

    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert %Frame{
             format: %{storage: %Format{pixel_format: :bw1, bw1_polarity: :one_is_white}},
             storage: %Binary{data: <<255, 128, 255, 128>>, planes: [%Plane{stride: 2}]}
           } = receive_latest_headless_frame()

    assert {:ok, pixels} = EmergeSkia.render_to_pixels(renderer)
    assert pixels == :binary.copy(<<255, 255, 255, 255>>, 18)
    assert :ok = EmergeSkia.stop(renderer)
  end

  test "headless Gray2 packs canonical rows and expands Gray8 screenshots" do
    {:ok, renderer} =
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {5, 2},
          frames: %Emerge.Config.BinaryFrames{pixel_format: :gray2, dither: false},
          target: self()
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

    tree =
      el(
        [width(px(5)), height(px(2)), Emerge.UI.Background.color(:white)],
        none()
      )

    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)

    assert %Frame{
             format: %{storage: %Format{pixel_format: :gray2}},
             storage: %Binary{data: <<255, 192, 255, 192>>, planes: [%Plane{stride: 2}]}
           } = receive_latest_headless_frame()

    assert {:ok, pixels} = EmergeSkia.render_to_pixels(renderer)
    assert pixels == :binary.copy(<<255, 255, 255, 255>>, 10)
    assert :ok = EmergeSkia.stop(renderer)
  end

  test "packed grayscale dithering changes gradients but not text-only output" do
    render = fn pixel_format, dither, tree ->
      {:ok, renderer} =
        EmergeSkia.start(%Emerge.Config{
          output: %Emerge.Config.Headless{
            size: {64, 36},
            frames: %Emerge.Config.BinaryFrames{
              pixel_format: pixel_format,
              bw1_polarity: :one_is_white,
              dither: dither
            },
            target: self()
          },
          renderer: [api: :raster],
          assets: [otp_app: :emerge]
        })

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
          Emerge.UI.Background.color(Emerge.UI.Color.gradient([:black, :white]))
        ],
        none()
      )

    for pixel_format <- [:bw1, :gray2] do
      assert render.(pixel_format, false, text_tree) == render.(pixel_format, true, text_tree)

      refute render.(pixel_format, false, gradient_tree) ==
               render.(pixel_format, true, gradient_tree)
    end
  end

  @tag :hardware
  test "headless GL backend delivers binary frames when explicitly enabled" do
    if System.get_env("EMERGE_SKIA_HEADLESS_GL_TEST") == "1" do
      {:ok, renderer} =
        EmergeSkia.start(%Emerge.Config{
          output: %Emerge.Config.Headless{
            size: {4, 4},
            frames: %Emerge.Config.BinaryFrames{pixel_format: :rgba8888},
            target: self()
          },
          renderer: [api: :opengl],
          assets: [otp_app: :emerge]
        })

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
      assert <<137, "PNG", _rest::binary>> = png
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
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {4, 4},
          frames: %Emerge.Config.PrimeFrames{max_in_flight: 1},
          target: self()
        },
        assets: [otp_app: :emerge]
      })

    assert {:ok,
            %{rendering_api: %{selected: :opengl}, capabilities: %{gpu: true, screenshot: false}}} =
             EmergeSkia.renderer_info(renderer)

    assert {:error, "screenshot capture is not supported for headless PRIME output"} =
             EmergeSkia.render_to_png(renderer)

    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
    assert_receive {:emerge_skia_frame, dma_buf}, 1000

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
    assert eventually(fn -> VideoInterop.LeaseOwner.stats(lease.owner).active_leases == 0 end)
    next_tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:green)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, next_tree)
    assert_receive {:emerge_skia_frame, %VideoInterop.Frame{} = next_dma_buf}, 1000
    [%VideoInterop.DMABuf.Object{fd: next_fd}] = next_dma_buf.storage.objects
    session_monitor = Process.monitor(renderer.pid)
    test_pid = self()
    spawn(fn -> send(test_pid, {:stop_result, EmergeSkia.stop(renderer)}) end)
    refute_receive {:stop_result, _result}, 100
    assert {:ok, _stat} = File.stat("/proc/self/fd/#{next_fd}")
    assert :ok = VideoInterop.release(next_dma_buf)
    assert_receive {:stop_result, :ok}, 1000
    refute EmergeSkia.running?(renderer)
    assert_receive {:DOWN, ^session_monitor, :process, _pid, :normal}, 1000
  end

  @tag :hardware
  @tag :headless_prime_hardware
  test "headless Vulkan PRIME publishes the complete fd-backed allocation size" do
    assert :headless in BuildConfig.compiled_vulkan_backends(),
           "compile the test NIF with the headless Vulkan backend"

    frames = %Emerge.Config.PrimeFrames{
      max_in_flight: 1,
      device: System.get_env("EMERGE_DEMO_PRIME_DRM_NODE")
    }

    output = %Emerge.Config.Headless{target: self(), size: {640, 420}, frames: frames}

    {:ok, renderer} =
      EmergeSkia.start(%Emerge.Config{
        output: output,
        renderer: [api: :vulkan],
        assets: [otp_app: :emerge]
      })

    assert {:ok, %{rendering_api: %{selected: :vulkan}}} = EmergeSkia.renderer_info(renderer)
    tree = el([width(px(640)), height(px(420)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
    assert_receive {:emerge_skia_frame, dma_buf}, 5000

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

    assert {:ok, %File.Stat{size: fd_allocation_size}} = File.stat("/proc/self/fd/#{object.fd}")
    visible_span = plane.offset + plane.pitch * 419 + 640 * 4
    assert layer.fourcc == :binary.decode_unsigned("AB24", :little)
    assert object.modifier == 0
    assert object.size == fd_allocation_size
    assert object.size >= visible_span
    assert :ok = VideoInterop.validate(dma_buf)
    assert :ok = VideoInterop.release(dma_buf)
    next_tree = el([width(px(640)), height(px(420)), Emerge.UI.Background.color(:blue)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, next_tree)
    assert_receive {:emerge_skia_frame, %VideoInterop.Frame{} = next_dma_buf}, 5000
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
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {4, 4},
          frames: %Emerge.Config.PrimeFrames{max_in_flight: 1},
          target: self()
        },
        renderer: [api: :opengl],
        assets: [otp_app: :emerge]
      })

    tree = el([width(px(4)), height(px(4)), Emerge.UI.Background.color(:red)], none())
    {_state, _assigned} = EmergeSkia.upload_tree(renderer, tree)
    assert_receive {:emerge_skia_frame, %VideoInterop.Frame{} = dma_buf}, 1000

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

    output =
      if backend == :drm do
        %Emerge.Config.DRM{}
      else
        %Emerge.Config.Window{backend: backend}
      end

    assert {:error, {:error, ^message}} =
             EmergeSkia.start(%Emerge.Config{output: output, assets: [otp_app: :emerge]})
  end

  test "legacy start arities raise explicit otp_app guidance" do
    refute function_exported?(EmergeSkia, :start, 0)
    assert_raise ArgumentError, ~r/expects %Emerge.Config/, fn -> EmergeSkia.start("Legacy") end
  end

  test "start/1 validates assets.fonts source type" do
    assert_raise ArgumentError, ~r/assets\.fonts\[\]\.source expects/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Window{backend: :auto, size: {800, 600}},
        assets: [otp_app: :emerge, fonts: [[family: "my-font", source: {:path, "/tmp/font.ttf"}]]]
      })
    end
  end

  test "start/1 validates assets.fonts weight range" do
    assert_raise ArgumentError,
                 ~r/assets\.fonts\[\]\.weight expects/,
                 fn ->
                   EmergeSkia.start(%Emerge.Config{
                     output: %Emerge.Config.Window{backend: :auto, size: {800, 600}},
                     assets: [
                       otp_app: :emerge,
                       fonts: [[family: "my-font", source: "fonts/MyFont.ttf", weight: 50]]
                     ]
                   })
                 end
  end

  test "start/1 rejects duplicate font variants" do
    assert_raise ArgumentError, ~r/duplicate assets\.fonts entries/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Window{backend: :auto, size: {800, 600}},
        assets: [
          otp_app: :emerge,
          fonts: [
            [family: "my-font", source: "fonts/MyFont-Regular.ttf", weight: 400],
            [family: "my-font", source: "fonts/MyFont-Regular2.ttf", weight: 400]
          ]
        ]
      })
    end
  end

  test "start/1 validates assets.fonts extension allowlist" do
    assert_raise ArgumentError, ~r/extension must be one of/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Window{backend: :auto, size: {800, 600}},
        assets: [
          otp_app: :emerge,
          fonts: [[family: "my-font", source: "fonts/MyFont.woff2", weight: 400]]
        ]
      })
    end
  end

  test "start/1 rejects drm_cursor on wayland" do
    assert_raise ArgumentError, ~r/drm_cursor is only supported with backend: :drm/, fn ->
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Window{backend: :wayland, size: {800, 600}},
        input: [cursor: [default: [source: "cursor.png"]]],
        assets: [otp_app: :emerge]
      })
    end
  end

  test "load_font_file/5 loads into the selected renderer" do
    {:ok, renderer} =
      EmergeSkia.start(%Emerge.Config{
        output: %Emerge.Config.Headless{
          size: {4, 4},
          frames: %Emerge.Config.BinaryFrames{pixel_format: :rgb888},
          target: self()
        },
        renderer: [api: :raster],
        assets: [otp_app: :emerge]
      })

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

  defp assert_frame_color(message_tag, {red, green, blue}) do
    expected = :binary.copy(<<red, green, blue>>, 16)
    deadline = System.monotonic_time(:millisecond) + 5000
    assert_frame_color_until(message_tag, expected, deadline, nil)
  end

  defp assert_frame_color_until(message_tag, expected, deadline, last_frame) do
    remaining = Kernel.max(deadline - System.monotonic_time(:millisecond), 0)

    receive do
      {^message_tag, %Frame{storage: %Binary{data: data, planes: [%Plane{stride: 12}]}}} ->
        if data == expected do
          :ok
        else
          assert_frame_color_until(message_tag, expected, deadline, data)
        end
    after
      remaining ->
        flunk(
          "did not receive expected #{inspect(message_tag)} pixels; last frame was #{inspect(last_frame)}"
        )
    end
  end

  defp eventually(predicate, timeout_ms \\ 1000) do
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
