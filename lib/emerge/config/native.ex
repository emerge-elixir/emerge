defmodule Emerge.Config.Native do
  @moduledoc false
  alias Emerge.Config

  # Private adaptation to the existing native transport. This is not a second
  # user-facing configuration shape; all public input is normalized structs.
  def options(%Config{} = config) do
    renderer = config.renderer
    diagnostics = config.diagnostics
    assets = config.assets
    cache = renderer.cache

    api =
      case {renderer.api, renderer.raster_present} do
        {api, :auto} -> api
        {:raster, present} -> [raster: [present: present]]
        {:auto, present} -> [auto: [raster: [present: present]]]
        {api, _} -> raise ArgumentError, "raster_present is not supported with #{inspect(api)}"
      end

    cache_opts = [
      max_new_payloads_per_frame: cache.max_new_payloads_per_frame,
      paint_layer: fields(cache.paint_layer)
    ]

    cache_opts =
      if cache.enabled == :auto,
        do: cache_opts,
        else: Keyword.put(cache_opts, :enabled, cache.enabled)

    [
      otp_app: assets.otp_app,
      rendering_api: api,
      target_fps: renderer.target_fps,
      vulkan_drm_node: renderer.device,
      scroll_line_pixels: config.input.scroll_line_pixels,
      hw_cursor: config.input.cursor.hardware,
      drm_cursor: cursor_options(config.input.cursor),
      drm_startup_retries: config.runtime.startup_retries,
      drm_retry_interval_ms: config.runtime.retry_interval_ms,
      drm_force_gpu_finish: diagnostics.force_gpu_finish,
      input_log: :input in diagnostics.logs,
      render_log: :render in diagnostics.logs,
      close_signal_log: :close in diagnostics.logs,
      stats: diagnostics.stats,
      renderer_stats_log: :stats in diagnostics.logs,
      renderer_animation_log: :animation in diagnostics.logs,
      renderer_cache: cache_opts,
      assets: [
        fonts: Enum.map(assets.fonts, &fields/1),
        decode_at_size: assets.decode_at_size,
        runtime_paths: fields(assets.runtime_paths),
        cache: fields(assets.cache)
      ]
    ] ++ output_options(config.output)
  end

  defp output_options(%Config.Window{} = window) do
    {width, height} = window.size

    backend =
      if window.backend == :auto,
        do: EmergeSkia.BuildConfig.default_runtime_backend(),
        else: window.backend

    [
      backend: backend,
      title: window.title,
      width: width,
      height: height,
      always_on_top: window.always_on_top
    ]
  end

  defp output_options(%Config.DRM{} = drm) do
    {width, height} = drm.preferred_size || {800, 600}

    [
      backend: :drm,
      drm_card: drm.card,
      drm_power: %{display_on: drm.display == :on, brightness: drm.backlight.brightness},
      drm_backlight: if(drm.backlight.device == :auto, do: "auto", else: drm.backlight.device),
      drm_output: drm.connector,
      drm_mode: drm.mode,
      drm_hotplug_interval_ms: drm.hotplug_interval_ms,
      width: width,
      height: height
    ]
  end

  defp output_options(%Config.Headless{} = output) do
    {width, height} = output.size

    delivery =
      case output.frames do
        %Config.BinaryFrames{} = frames ->
          Keyword.put(fields(frames), :mode, :binary)

        %Config.PrimeFrames{} = frames ->
          [
            mode: :prime,
            prime: [
              drm_node: frames.device,
              max_in_flight: frames.max_in_flight,
              on_backpressure: frames.on_backpressure
            ]
          ]
      end

    [
      backend: :headless,
      width: width,
      height: height,
      headless: [target: output.target, frame_message: output.frame_message] ++ delivery
    ]
  end

  defp cursor_options(cursor) do
    [:default, :text, :pointer]
    |> Enum.flat_map(fn icon ->
      case Map.fetch!(cursor, icon) do
        nil -> []
        value -> [{icon, fields(value)}]
      end
    end)
  end

  defp fields(value), do: value |> Map.from_struct() |> Map.to_list()

  @masks %{
    key: 0x01,
    text: 0x02,
    pointer_move: 0x04,
    pointer_button: 0x08,
    pointer_scroll: 0x10,
    pointer_enter: 0x20,
    resize: 0x40,
    focus: 0x80
  }
  def input_mask(%Config.Input{events: events}) do
    Enum.reduce(events, 0, fn event, mask -> Bitwise.bor(mask, Map.fetch!(@masks, event)) end)
  end
end
