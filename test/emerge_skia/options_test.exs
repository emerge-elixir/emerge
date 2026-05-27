defmodule EmergeSkia.OptionsTest do
  use ExUnit.Case, async: true

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

  test "build_start_native_opts! normalizes backend_renderer" do
    assert %{backend_renderer: %{kind: "auto", raster_present: "auto"}} =
             Options.build_start_native_opts!([])

    assert %{backend_renderer: %{kind: "gl", raster_present: "auto"}} =
             Options.build_start_native_opts!(backend_renderer: :gl)

    assert %{
             backend_renderer: %{
               kind: "raster",
               raster_present: "auto",
               raster_present_configured: false
             }
           } = Options.build_start_native_opts!(backend_renderer: :raster)

    assert %{
             backend_renderer: %{
               kind: "raster",
               raster_present: "cpu",
               raster_present_configured: true
             }
           } =
             Options.build_start_native_opts!(
               backend: :wayland,
               backend_renderer: [raster: [present: :cpu]]
             )

    assert %{
             backend_renderer: %{
               kind: "auto",
               raster_present: "gpu_upload",
               raster_present_configured: true
             }
           } =
             Options.build_start_native_opts!(
               backend: :drm,
               backend_renderer: [auto: [raster: [present: "gpu_upload"]]]
             )
  end

  test "build_start_native_opts! rejects invalid backend_renderer" do
    assert_raise ArgumentError, ~r/:backend_renderer must be/, fn ->
      Options.build_start_native_opts!(backend_renderer: :bogus)
    end

    assert_raise ArgumentError, ~r/raster present must be :auto, :gpu_upload, or :cpu/, fn ->
      Options.build_start_native_opts!(backend_renderer: [raster: [present: :bogus]])
    end

    assert_raise ArgumentError, ~r/:backend_renderer.auto has unsupported option/, fn ->
      Options.build_start_native_opts!(backend_renderer: [auto: [present: :cpu]])
    end
  end

  test "build_start_native_opts! rejects removed macos_backend" do
    assert_raise ArgumentError, ~r/macos_backend has been removed.*backend_renderer/, fn ->
      Options.build_start_native_opts!(macos_backend: :raster)
    end
  end

  test "build_start_native_opts! validates backend_renderer against backend" do
    assert_raise ArgumentError,
                 ~r/backend_renderer: :gl is not supported with backend: :macos/,
                 fn ->
                   Options.build_start_native_opts!(backend: :macos, backend_renderer: :gl)
                 end

    assert_raise ArgumentError,
                 ~r/backend_renderer: :metal is only supported with backend: :macos/,
                 fn ->
                   Options.build_start_native_opts!(backend: :wayland, backend_renderer: :metal)
                 end

    assert_raise ArgumentError, ~r/raster present options are only supported/, fn ->
      Options.build_start_native_opts!(
        backend: :macos,
        backend_renderer: [raster: [present: :cpu]]
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

  test "build_start_native_opts! keeps close_signal_log option" do
    assert %{close_signal_log: false} = Options.build_start_native_opts!([])
    assert %{close_signal_log: true} = Options.build_start_native_opts!(close_signal_log: true)
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
