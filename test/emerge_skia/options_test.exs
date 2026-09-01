defmodule EmergeSkia.OptionsTest do
  use ExUnit.Case, async: true

  import ExUnit.CaptureIO

  alias Emerge.Assets.Ref
  alias EmergeSkia.Assets
  alias EmergeSkia.BuildConfig
  alias EmergeSkia.Options

  test "build_start_native_opts! defaults backend from build config" do
    expected_backend = Atom.to_string(BuildConfig.default_runtime_backend())

    assert %{
             backend: ^expected_backend,
             drm_startup_retries: 40,
             drm_retry_interval_ms: 250,
             drm_force_gpu_finish: false
           } = Options.build_start_native_opts!([])
  end

  test "build_start_native_opts! keeps explicit backend selection" do
    assert %{backend: "drm"} = Options.build_start_native_opts!(backend: :drm)
    assert %{backend: "wayland"} = Options.build_start_native_opts!(backend: "wayland")
  end

  test "build_start_native_opts! normalizes rendering_api" do
    assert %{rendering_api: %{kind: "auto", raster_present: "auto"}} =
             Options.build_start_native_opts!([])

    assert %{rendering_api: %{kind: "opengl", raster_present: "auto"}} =
             Options.build_start_native_opts!(backend: :wayland, rendering_api: :opengl)

    assert %{
             rendering_api: %{kind: "vulkan", raster_present: "auto"},
             vulkan_drm_node: "/dev/dri/renderD128"
           } =
             Options.build_start_native_opts!(
               backend: :drm,
               rendering_api: :vulkan,
               vulkan_drm_node: "/dev/dri/renderD128"
             )

    assert %{rendering_api: %{kind: "vulkan", raster_present: "auto"}} =
             Options.build_start_native_opts!(backend: :wayland, rendering_api: :vulkan)

    assert %{
             rendering_api: %{
               kind: "raster",
               raster_present: "auto",
               raster_present_configured: false
             }
           } = Options.build_start_native_opts!(rendering_api: :raster)

    assert %{
             rendering_api: %{
               kind: "raster",
               raster_present: "cpu",
               raster_present_configured: true
             }
           } =
             Options.build_start_native_opts!(
               backend: :wayland,
               rendering_api: [raster: [present: :cpu]]
             )

    assert %{
             rendering_api: %{
               kind: "auto",
               raster_present: "gpu_upload",
               raster_present_configured: true
             }
           } =
             Options.build_start_native_opts!(
               backend: :drm,
               rendering_api: [auto: [raster: [present: "gpu_upload"]]]
             )
  end

  test "rendering_api_start_error delegates Linux Vulkan availability to native code" do
    drm_opts =
      Options.build_start_native_opts!(
        backend: :drm,
        rendering_api: :vulkan,
        vulkan_drm_node: "/dev/dri/renderD128"
      )

    wayland_opts = Options.build_start_native_opts!(backend: :wayland, rendering_api: :vulkan)

    headless_opts =
      Options.build_start_native_opts!(
        backend: :headless,
        rendering_api: :vulkan,
        headless: [target: self()]
      )

    assert Options.rendering_api_start_error(drm_opts) == nil
    assert Options.rendering_api_start_error(wayland_opts) == nil
    assert Options.rendering_api_start_error(headless_opts) == nil
  end

  test "DRM Vulkan requires an exact independent Vulkan node and rejects GL-only finish" do
    assert_raise ArgumentError, ~r/requires an explicit :vulkan_drm_node absolute path/, fn ->
      Options.build_start_native_opts!(backend: :drm, rendering_api: :vulkan)
    end

    assert_raise ArgumentError, ~r/:vulkan_drm_node must be an absolute path/, fn ->
      Options.build_start_native_opts!(
        backend: :drm,
        rendering_api: :vulkan,
        vulkan_drm_node: "renderD128"
      )
    end

    assert_raise ArgumentError, ~r/OpenGL-only diagnostic/, fn ->
      Options.build_start_native_opts!(
        backend: :drm,
        rendering_api: :vulkan,
        vulkan_drm_node: "/dev/dri/renderD128",
        drm_force_gpu_finish: true
      )
    end

    assert %{rendering_api: %{kind: "opengl"}, drm_force_gpu_finish: true} =
             Options.build_start_native_opts!(
               backend: :drm,
               rendering_api: :opengl,
               vulkan_drm_node: "/dev/dri/renderD128",
               drm_force_gpu_finish: true
             )
  end

  test "build_start_native_opts! keeps deprecated rendering aliases" do
    warning =
      capture_io(:stderr, fn ->
        send(
          self(),
          {:normalized_alias,
           Options.build_start_native_opts!(backend: :wayland, backend_renderer: :gl)}
        )
      end)

    assert_receive {:normalized_alias, %{rendering_api: %{kind: "opengl"}}}
    assert warning =~ "backend_renderer is deprecated; use rendering_api instead"
    assert warning =~ "rendering API :gl is deprecated; use :opengl"

    assert_raise ArgumentError, ~r/cannot be used together/, fn ->
      Options.build_start_native_opts!(rendering_api: :opengl, backend_renderer: :gl)
    end
  end

  test "build_start_native_opts! rejects invalid rendering_api" do
    assert_raise ArgumentError, ~r/:rendering_api must be/, fn ->
      Options.build_start_native_opts!(rendering_api: :bogus)
    end

    assert_raise ArgumentError, ~r/raster present must be :auto, :gpu_upload, or :cpu/, fn ->
      Options.build_start_native_opts!(rendering_api: [raster: [present: :bogus]])
    end

    assert_raise ArgumentError, ~r/:rendering_api.auto has unsupported option/, fn ->
      Options.build_start_native_opts!(rendering_api: [auto: [present: :cpu]])
    end
  end

  test "build_start_native_opts! rejects removed macos_backend" do
    assert_raise ArgumentError, ~r/macos_backend has been removed.*rendering_api/, fn ->
      Options.build_start_native_opts!(macos_backend: :raster)
    end
  end

  test "build_start_native_opts! validates rendering_api against backend" do
    assert_raise ArgumentError,
                 ~r/rendering_api: :opengl is not supported with backend: :macos/,
                 fn ->
                   Options.build_start_native_opts!(backend: :macos, rendering_api: :opengl)
                 end

    assert_raise ArgumentError,
                 ~r/rendering_api: :metal is only supported with backend: :macos/,
                 fn ->
                   Options.build_start_native_opts!(backend: :wayland, rendering_api: :metal)
                 end

    assert_raise ArgumentError, ~r/raster present options are only supported/, fn ->
      Options.build_start_native_opts!(
        backend: :macos,
        rendering_api: [raster: [present: :cpu]]
      )
    end

    assert_raise ArgumentError, ~r/raster present options are only supported/, fn ->
      Options.build_start_native_opts!(
        backend: :headless,
        rendering_api: [auto: [raster: [present: :cpu]]],
        headless: [target: self()]
      )
    end
  end

  test "build_start_native_opts! validates drm diagnostic and retry options" do
    assert %{
             drm_startup_retries: 5,
             drm_retry_interval_ms: 100,
             drm_force_gpu_finish: true
           } =
             Options.build_start_native_opts!(
               drm_startup_retries: 5,
               drm_retry_interval_ms: 100,
               drm_force_gpu_finish: true
             )

    assert_raise ArgumentError, ~r/:drm_startup_retries must be a non-negative integer/, fn ->
      Options.build_start_native_opts!(drm_startup_retries: -1)
    end

    assert_raise ArgumentError, ~r/:drm_retry_interval_ms must be a non-negative integer/, fn ->
      Options.build_start_native_opts!(drm_retry_interval_ms: -1)
    end

    assert_raise ArgumentError, ~r/:drm_force_gpu_finish must be a boolean/, fn ->
      Options.build_start_native_opts!(drm_force_gpu_finish: :yes)
    end
  end

  test "build_start_native_opts! normalizes scroll_line_pixels" do
    assert %{scroll_line_pixels: 45.0} =
             Options.build_start_native_opts!(scroll_line_pixels: 45)

    assert %{scroll_line_pixels: 18.5} =
             Options.build_start_native_opts!(scroll_line_pixels: 18.5)

    assert_raise ArgumentError, ~r/:scroll_line_pixels must be a positive number/, fn ->
      Options.build_start_native_opts!(scroll_line_pixels: 0)
    end
  end

  test "build_start_native_opts! keeps native diagnostic logs disabled by default" do
    assert %{input_log: false, render_log: false, close_signal_log: false} =
             Options.build_start_native_opts!([])

    assert %{render_log: true, close_signal_log: true} =
             Options.build_start_native_opts!(render_log: true, close_signal_log: true)
  end

  test "build_start_native_opts! keeps renderer_stats_log option" do
    assert %{renderer_stats_log: false} = Options.build_start_native_opts!([])

    assert %{renderer_stats_log: true} =
             Options.build_start_native_opts!(renderer_stats_log: true)
  end

  test "build_start_native_opts! keeps renderer_animation_log option separate from stats log" do
    assert %{renderer_animation_log: false, renderer_stats_log: false} =
             Options.build_start_native_opts!([])

    assert %{renderer_animation_log: true, renderer_stats_log: false} =
             Options.build_start_native_opts!(renderer_animation_log: true)

    assert %{renderer_animation_log: false, renderer_stats_log: true} =
             Options.build_start_native_opts!(renderer_stats_log: true)
  end

  test "build_start_native_opts! keeps stats option" do
    assert %{stats_enabled: false} = Options.build_start_native_opts!([])
    assert %{stats_enabled: true} = Options.build_start_native_opts!(stats: true)
  end

  test "build_start_native_opts! normalizes headless options" do
    assert %{
             headless: %{
               target: nil,
               mode: "binary",
               pixel_format: "rgba8888",
               bw1_polarity: "one_is_black",
               dither: false,
               target_fps: nil,
               frame_message: "emerge_skia_frame"
             }
           } = Options.build_start_native_opts!([])

    assert %{
             headless: %{
               target: target,
               mode: "binary",
               pixel_format: "bw1",
               bw1_polarity: "one_is_white",
               dither: false,
               target_fps: 30,
               frame_message: "my_frame"
             }
           } =
             Options.build_start_native_opts!(
               backend: :headless,
               headless: [
                 target: self(),
                 pixel_format: :bw1,
                 bw1_polarity: :one_is_white,
                 target_fps: 30,
                 frame_message: :my_frame
               ]
             )

    assert target == self()

    for pixel_format <- [:bw1, :gray2] do
      assert %{headless: %{dither: true, pixel_format: normalized}} =
               Options.build_start_native_opts!(
                 backend: :headless,
                 rendering_api: :raster,
                 headless: [target: self(), pixel_format: pixel_format, dither: true]
               )

      assert normalized == Atom.to_string(pixel_format)
    end

    for opts <- [
          [
            backend: :headless,
            rendering_api: :raster,
            headless: [target: self(), pixel_format: :gray4, dither: true]
          ],
          [
            backend: :headless,
            rendering_api: :opengl,
            headless: [target: self(), pixel_format: :bw1, dither: true]
          ],
          [
            backend: :headless,
            rendering_api: :raster,
            headless: [target: self(), mode: :prime, dither: true]
          ]
        ] do
      assert_raise ArgumentError, ~r/headless.dither true currently requires/, fn ->
        Options.build_start_native_opts!(opts)
      end
    end

    assert_raise ArgumentError, ~r/:headless.dither must be a boolean/, fn ->
      Options.build_start_native_opts!(
        backend: :headless,
        rendering_api: :raster,
        headless: [target: self(), pixel_format: :bw1, dither: :atkinson]
      )
    end

    assert %{
             backend: "headless",
             rendering_api: %{kind: "opengl"},
             headless: %{target: target}
           } =
             Options.build_start_native_opts!(
               backend: :headless,
               rendering_api: :opengl,
               headless: [target: self()]
             )

    assert target == self()

    assert %{
             headless: %{
               target: target,
               mode: "prime",
               prime: %{
                 drm_node: "/dev/dri/renderD128",
                 max_in_flight: 3,
                 on_backpressure: "drop_new"
               }
             }
           } =
             Options.build_start_native_opts!(
               backend: :headless,
               headless: [
                 target: self(),
                 mode: :prime,
                 prime: [
                   drm_node: "/dev/dri/renderD128",
                   max_in_flight: 3,
                   on_backpressure: :drop_new
                 ]
               ]
             )

    assert target == self()

    assert %{headless: %{mode: "prime", target: nil}} =
             Options.build_start_native_opts!(
               backend: :headless,
               rendering_api: :opengl,
               headless: [mode: :prime]
             )

    assert_raise ArgumentError, ~r/:headless.prime has unsupported option/, fn ->
      Options.build_start_native_opts!(
        backend: :headless,
        headless: [target: self(), mode: :prime, prime: [max_inflight: 3]]
      )
    end

    assert_raise ArgumentError, ~r/:headless.prime.drm_node must be an absolute path/, fn ->
      Options.build_start_native_opts!(
        backend: :headless,
        headless: [target: self(), mode: :prime, prime: [drm_node: "renderD128"]]
      )
    end

    assert_raise ArgumentError, ~r/:headless.prime.drm_node must be nil or a non-empty/, fn ->
      Options.build_start_native_opts!(
        backend: :headless,
        headless: [target: self(), mode: :prime, prime: [drm_node: ""]]
      )
    end

    assert_raise ArgumentError, ~r/:headless.target must be a live local pid/, fn ->
      Options.build_start_native_opts!(backend: :headless)
    end

    dead_target = spawn(fn -> :ok end)
    monitor = Process.monitor(dead_target)
    assert_receive {:DOWN, ^monitor, :process, ^dead_target, _reason}

    assert_raise ArgumentError, ~r/:headless.target must be a live local pid/, fn ->
      Options.build_start_native_opts!(backend: :headless, headless: [target: dead_target])
    end
  end

  test "build_start_native_opts! normalizes renderer cache limits" do
    assert %{
             renderer_cache: %{
               enabled: true,
               max_new_payloads_per_frame: 16,
               paint_layer: %{
                 max_entries: 512,
                 max_bytes: 671_088_640,
                 max_entry_bytes: 268_435_456,
                 min_visible_before_store: 1,
                 max_stale_frames: 120
               }
             }
           } = Options.build_start_native_opts!([])

    assert %{
             renderer_cache: %{
               enabled: true,
               max_new_payloads_per_frame: 0,
               paint_layer: %{
                 max_entries: 16,
                 max_bytes: 1_048_576,
                 max_entry_bytes: 131_072,
                 min_visible_before_store: 2,
                 max_stale_frames: 30
               }
             }
           } =
             Options.build_start_native_opts!(
               renderer_cache: [
                 enabled: true,
                 max_new_payloads_per_frame: 0,
                 paint_layer: [
                   max_entries: 16,
                   max_bytes: 1_048_576,
                   max_entry_bytes: 131_072,
                   min_visible_before_store: 2,
                   max_stale_frames: 30
                 ]
               ]
             )

    assert_raise ArgumentError,
                 ~r/:renderer_cache.paint_layer.max_bytes must be a non-negative integer/,
                 fn ->
                   Options.build_start_native_opts!(
                     renderer_cache: [paint_layer: [max_bytes: -1]]
                   )
                 end
  end

  test "build_start_native_opts! disables renderer cache by default for raster renderer" do
    assert %{renderer_cache: %{enabled: false}} =
             Options.build_start_native_opts!(rendering_api: :raster)

    assert %{renderer_cache: %{enabled: true}} =
             Options.build_start_native_opts!(
               rendering_api: :raster,
               renderer_cache: [enabled: true]
             )
  end

  test "normalize_asset_config! bounds the decoded cache and supports sized decode" do
    assert %{
             cache_max_entries: 256,
             cache_max_bytes: 268_435_456,
             decode_at_size: false
           } = Assets.normalize_asset_config!(otp_app: :emerge)

    assert %{decode_at_size: true} =
             Assets.normalize_asset_config!([otp_app: :emerge], decode_at_size: true)

    config =
      Assets.normalize_asset_config!(
        otp_app: :emerge,
        assets: %{
          cache: %{max_entries: 0, max_bytes: 1_024},
          decode_at_size: true
        }
      )

    assert %{
             cache_max_entries: 0,
             cache_max_bytes: 1_024,
             decode_at_size: true
           } = config

    assert %{
             asset_cache_max_entries: 0,
             asset_cache_max_bytes: 1_024,
             asset_decode_at_size: true
           } = Assets.native_start_asset_config(config)

    assert_raise ArgumentError, ~r/assets.cache.max_bytes must be a non-negative integer/, fn ->
      Assets.normalize_asset_config!(
        otp_app: :emerge,
        assets: [cache: [max_bytes: -1]]
      )
    end

    assert_raise ArgumentError, ~r/assets.decode_at_size must be a boolean/, fn ->
      Assets.normalize_asset_config!(otp_app: :emerge, assets: [decode_at_size: :yes])
    end
  end

  test "normalize_drm_cursor_overrides! normalizes logical and runtime sources" do
    runtime_path =
      Path.join(System.tmp_dir!(), "emerge_cursor_#{System.unique_integer([:positive])}.svg")

    on_exit(fn ->
      File.rm(runtime_path)
    end)

    File.write!(runtime_path, ~S(<svg width="1" height="1" xmlns="http://www.w3.org/2000/svg"/>))

    assert [
             %{
               icon: "default",
               source: "sample_assets/tile_quad.svg",
               hotspot_x: 1.0,
               hotspot_y: 1.0
             },
             %{
               icon: "text",
               source: ^runtime_path,
               hotspot_x: 11.5,
               hotspot_y: 11.5
             }
           ] =
             Assets.normalize_drm_cursor_overrides!(
               drm_cursor: [
                 default: [source: %Ref{path: "sample_assets/tile_quad.svg"}, hotspot: {1, 1}],
                 text: [source: runtime_path, hotspot: {11.5, 11.5}]
               ]
             )
  end

  test "normalize_drm_cursor_overrides! accepts string keyed maps" do
    assert [
             %{
               icon: "pointer",
               source: "sample_assets/tile_quad.svg",
               hotspot_x: 7.0,
               hotspot_y: 2.0
             }
           ] =
             Assets.normalize_drm_cursor_overrides!(
               drm_cursor: %{
                 "pointer" => %{"source" => "sample_assets/tile_quad.svg", "hotspot" => {7, 2}}
               }
             )
  end

  test "normalize_drm_cursor_overrides! rejects unsupported extensions" do
    assert_raise ArgumentError, ~r/drm_cursor\.default\.source extension must be one of/, fn ->
      Assets.normalize_drm_cursor_overrides!(
        drm_cursor: [default: [source: "sample_assets/static.jpg", hotspot: {1, 1}]]
      )
    end
  end
end
