defmodule EmergeSkia.Options do
  @moduledoc false

  @doc false
  def normalize_start_keyword_opts!(opts) do
    normalize_keyword_list!(
      opts,
      "EmergeSkia.start/1 expects a keyword list, for example: EmergeSkia.start(otp_app: :my_app, ...)"
    )
  end

  @doc false
  def build_start_native_opts!(opts) do
    if Keyword.has_key?(opts, :dispatch_mode) do
      raise ArgumentError,
            "dispatch_mode option has been removed; EmergeSkia now runs a single dispatch engine"
    end

    if Keyword.has_key?(opts, :macos_backend) do
      raise ArgumentError,
            "macos_backend has been removed; use rendering_api: :auto | :metal | :raster instead"
    end

    backend =
      opts
      |> Keyword.get(:backend, EmergeSkia.BuildConfig.default_runtime_backend())
      |> normalize_backend!()

    rendering_api =
      opts
      |> option_with_deprecated_alias!(:rendering_api, :backend_renderer, :auto)
      |> normalize_rendering_api!()

    validate_rendering_api_for_backend!(backend, rendering_api)

    vulkan_drm_node =
      opts
      |> Keyword.get(:vulkan_drm_node)
      |> normalize_optional_drm_node!(":vulkan_drm_node")

    if backend == "drm" and rendering_api.kind == "vulkan" and is_nil(vulkan_drm_node) do
      raise ArgumentError,
            "rendering_api: :vulkan with backend: :drm requires an explicit :vulkan_drm_node absolute path, selected independently from :drm_card"
    end

    if backend == "drm" and rendering_api.kind == "vulkan" and
         Keyword.get(opts, :drm_force_gpu_finish, false) == true do
      raise ArgumentError,
            ":drm_force_gpu_finish is an OpenGL-only diagnostic and cannot be used with rendering_api: :vulkan"
    end

    renderer_cache =
      opts
      |> Keyword.get(:renderer_cache, [])
      |> normalize_renderer_cache_opts!()
      |> maybe_disable_renderer_cache_for_raster_default!(rendering_api)

    headless =
      opts
      |> Keyword.get(:headless, [])
      |> normalize_headless_opts!(backend)

    %{
      backend: backend,
      rendering_api: rendering_api,
      title: Keyword.get(opts, :title, "Emerge"),
      width: Keyword.get(opts, :width, 800),
      height: Keyword.get(opts, :height, 600),
      drm_card: normalize_optional_string(Keyword.get(opts, :drm_card)),
      vulkan_drm_node: vulkan_drm_node,
      drm_startup_retries:
        opts
        |> Keyword.get(:drm_startup_retries, 40)
        |> normalize_non_negative_integer!(":drm_startup_retries"),
      drm_retry_interval_ms:
        opts
        |> Keyword.get(:drm_retry_interval_ms, 250)
        |> normalize_non_negative_integer!(":drm_retry_interval_ms"),
      drm_force_gpu_finish:
        opts
        |> Keyword.get(:drm_force_gpu_finish, false)
        |> normalize_boolean!(":drm_force_gpu_finish"),
      scroll_line_pixels:
        opts
        |> Keyword.get(:scroll_line_pixels, 30.0)
        |> normalize_positive_number!(":scroll_line_pixels"),
      hw_cursor: Keyword.get(opts, :hw_cursor, true),
      input_log: Keyword.get(opts, :input_log, false),
      render_log: Keyword.get(opts, :render_log, false),
      close_signal_log: Keyword.get(opts, :close_signal_log, false),
      stats_enabled: Keyword.get(opts, :stats, false) == true,
      renderer_stats_log: Keyword.get(opts, :renderer_stats_log, false),
      renderer_animation_log: Keyword.get(opts, :renderer_animation_log, false),
      renderer_cache: renderer_cache,
      headless: headless
    }
  end

  @doc false
  def rendering_api_start_error(%{backend: backend, rendering_api: rendering_api}) do
    case {String.downcase(backend), rendering_api.kind} do
      {backend, "vulkan"} when backend in ["drm", "wayland", "headless"] ->
        nil

      _other ->
        nil
    end
  end

  @doc false
  def normalize_render_to_pixels_keyword_opts!(opts) do
    normalize_keyword_list!(
      opts,
      "EmergeSkia.render_to_pixels/2 expects a keyword list, for example: EmergeSkia.render_to_pixels(tree, otp_app: :my_app, width: 800, height: 600)"
    )
  end

  @doc false
  def normalize_render_to_png_keyword_opts!(opts) do
    normalize_keyword_list!(
      opts,
      "EmergeSkia.render_to_png/2 expects a keyword list, for example: EmergeSkia.render_to_png(tree, otp_app: :my_app, width: 800, height: 600)"
    )
  end

  @doc false
  def normalize_screenshot_opts!(opts) do
    opts =
      normalize_keyword_list!(
        opts,
        "screenshot options must be a keyword list"
      )

    {region_x, region_y, region_width, region_height} = normalize_screenshot_region!(opts)

    %{
      pixel_format:
        opts
        |> Keyword.get(:pixel_format, :rgba8888)
        |> normalize_screenshot_pixel_format!(),
      scale:
        opts
        |> Keyword.get(:scale, 1.0)
        |> normalize_positive_number!(":scale"),
      region_x: region_x,
      region_y: region_y,
      region_width: region_width,
      region_height: region_height,
      timeout_ms:
        opts
        |> Keyword.get(:timeout, 5_000)
        |> normalize_non_negative_integer!(":timeout"),
      background:
        opts
        |> Keyword.get(:background, :transparent)
        |> normalize_screenshot_background!(),
      png_compression:
        opts
        |> Keyword.get(:png, [])
        |> normalize_png_compression!()
    }
  end

  @doc false
  def normalize_raster_opts!(opts, default_asset_timeout_ms) do
    %{
      width: opts |> Keyword.fetch!(:width) |> normalize_positive_integer!(":width"),
      height: opts |> Keyword.fetch!(:height) |> normalize_positive_integer!(":height"),
      scale: opts |> Keyword.get(:scale, 1.0) |> normalize_positive_number!(":scale"),
      asset_mode:
        opts
        |> Keyword.get(:asset_mode, :await)
        |> normalize_asset_mode!(),
      asset_timeout_ms:
        opts
        |> Keyword.get(:asset_timeout_ms, default_asset_timeout_ms)
        |> normalize_positive_integer!(":asset_timeout_ms")
    }
  end

  @doc false
  def normalize_keyword_or_map!(value, field_name) do
    cond do
      is_map(value) ->
        Map.to_list(value)

      is_list(value) and Keyword.keyword?(value) ->
        Keyword.new(value)

      true ->
        raise ArgumentError, "#{field_name} must be a keyword list or map, got: #{inspect(value)}"
    end
  end

  @doc false
  def normalize_list!(list, _field_name) when is_list(list), do: list

  def normalize_list!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a list, got: #{inspect(value)}"
  end

  @doc false
  def normalize_string_list!(list, field_name) do
    if not (is_list(list) and Enum.all?(list, &is_binary/1)) do
      raise ArgumentError, "#{field_name} must be a list of strings"
    end

    list
  end

  @doc false
  def normalize_non_empty_string!(value, field_name) when is_binary(value) do
    case String.trim(value) do
      "" -> raise ArgumentError, "#{field_name} must not be empty"
      trimmed -> trimmed
    end
  end

  def normalize_non_empty_string!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a string, got: #{inspect(value)}"
  end

  @doc false
  def normalize_boolean!(value, _field_name) when is_boolean(value), do: value

  def normalize_boolean!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a boolean, got: #{inspect(value)}"
  end

  @doc false
  def normalize_positive_integer!(value, _field_name)
      when is_integer(value) and value > 0,
      do: value

  def normalize_positive_integer!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a positive integer, got: #{inspect(value)}"
  end

  @doc false
  def normalize_non_negative_integer!(value, _field_name)
      when is_integer(value) and value >= 0,
      do: value

  def normalize_non_negative_integer!(value, field_name) do
    raise ArgumentError,
          "#{field_name} must be a non-negative integer, got: #{inspect(value)}"
  end

  @doc false
  def normalize_positive_number!(value, _field_name)
      when is_integer(value) and value > 0,
      do: value / 1.0

  def normalize_positive_number!(value, _field_name)
      when is_float(value) and value > 0.0,
      do: value

  def normalize_positive_number!(value, field_name) do
    raise ArgumentError, "#{field_name} must be a positive number, got: #{inspect(value)}"
  end

  @doc false
  def normalize_renderer_cache_opts!(opts) do
    opts = normalize_keyword_or_map!(opts, ":renderer_cache")

    paint_layer =
      opts
      |> Keyword.get(:paint_layer, [])
      |> normalize_keyword_or_map!(":renderer_cache.paint_layer")

    %{
      enabled:
        opts
        |> Keyword.get(:enabled, true)
        |> normalize_boolean!(":renderer_cache.enabled"),
      enabled_configured: Keyword.has_key?(opts, :enabled),
      max_new_payloads_per_frame:
        opts
        |> Keyword.get(:max_new_payloads_per_frame, 16)
        |> normalize_non_negative_integer!(":renderer_cache.max_new_payloads_per_frame"),
      paint_layer: %{
        max_entries:
          paint_layer
          |> Keyword.get(:max_entries, 512)
          |> normalize_non_negative_integer!(":renderer_cache.paint_layer.max_entries"),
        max_bytes:
          paint_layer
          |> Keyword.get(:max_bytes, 640 * 1024 * 1024)
          |> normalize_non_negative_integer!(":renderer_cache.paint_layer.max_bytes"),
        max_entry_bytes:
          paint_layer
          |> Keyword.get(:max_entry_bytes, 256 * 1024 * 1024)
          |> normalize_non_negative_integer!(":renderer_cache.paint_layer.max_entry_bytes"),
        min_visible_before_store:
          paint_layer
          |> Keyword.get(:min_visible_before_store, 1)
          |> normalize_non_negative_integer!(
            ":renderer_cache.paint_layer.min_visible_before_store"
          ),
        max_stale_frames:
          paint_layer
          |> Keyword.get(:max_stale_frames, 120)
          |> normalize_non_negative_integer!(":renderer_cache.paint_layer.max_stale_frames")
      }
    }
  end

  @doc false
  def normalize_asset_mode!(:await), do: "await"
  def normalize_asset_mode!(:snapshot), do: "snapshot"
  def normalize_asset_mode!("await"), do: "await"
  def normalize_asset_mode!("snapshot"), do: "snapshot"

  def normalize_asset_mode!(value) do
    raise ArgumentError,
          ":asset_mode must be :await or :snapshot, got: #{inspect(value)}"
  end

  defp maybe_disable_renderer_cache_for_raster_default!(
         %{enabled_configured: false} = renderer_cache,
         %{kind: "raster"}
       ) do
    %{renderer_cache | enabled: false}
  end

  defp maybe_disable_renderer_cache_for_raster_default!(renderer_cache, _rendering_api),
    do: renderer_cache

  defp normalize_headless_opts!(value, backend) do
    opts = normalize_keyword_or_map!(value, ":headless")

    target = Keyword.get(opts, :target)

    mode =
      opts
      |> Keyword.get(:mode, :binary)
      |> normalize_headless_mode!()

    if backend == "headless" and mode == "binary" and
         (not is_pid(target) or node(target) != node() or not Process.alive?(target)) do
      raise ArgumentError,
            ":headless.target must be a live local pid for binary headless output"
    end

    if backend == "headless" and mode == "prime" and not is_nil(target) and
         (not is_pid(target) or node(target) != node() or not Process.alive?(target)) do
      raise ArgumentError,
            ":headless.target must be nil or a live local pid for PRIME headless output"
    end

    %{
      target: target,
      mode: mode,
      pixel_format:
        opts
        |> Keyword.get(:pixel_format, :rgba8888)
        |> normalize_headless_pixel_format!(),
      bw1_polarity:
        opts
        |> Keyword.get(:bw1_polarity, :one_is_black)
        |> normalize_bw1_polarity!(),
      target_fps:
        opts
        |> Keyword.get(:target_fps)
        |> normalize_optional_positive_integer!(":headless.target_fps"),
      frame_message:
        opts
        |> Keyword.get(:frame_message, :emerge_skia_frame)
        |> normalize_frame_message!(),
      prime:
        opts
        |> Keyword.get(:prime, [])
        |> normalize_headless_prime_opts!()
    }
  end

  defp normalize_headless_mode!(value) when value in [:binary, "binary"], do: "binary"
  defp normalize_headless_mode!(value) when value in [:prime, "prime"], do: "prime"

  defp normalize_headless_mode!(value),
    do: raise(ArgumentError, ":headless.mode must be :binary or :prime, got: #{inspect(value)}")

  defp normalize_headless_pixel_format!(value)
       when value in [:rgba8888, "rgba8888"],
       do: "rgba8888"

  defp normalize_headless_pixel_format!(value) when value in [:rgb888, "rgb888"], do: "rgb888"
  defp normalize_headless_pixel_format!(value) when value in [:gray8, "gray8"], do: "gray8"
  defp normalize_headless_pixel_format!(value) when value in [:gray4, "gray4"], do: "gray4"
  defp normalize_headless_pixel_format!(value) when value in [:gray2, "gray2"], do: "gray2"
  defp normalize_headless_pixel_format!(value) when value in [:bw1, "bw1"], do: "bw1"

  defp normalize_headless_pixel_format!(value) do
    raise ArgumentError,
          ":headless.pixel_format must be :rgba8888, :rgb888, :gray8, :gray4, :gray2, or :bw1, got: #{inspect(value)}"
  end

  defp normalize_bw1_polarity!(value) when value in [:one_is_black, "one_is_black"],
    do: "one_is_black"

  defp normalize_bw1_polarity!(value) when value in [:one_is_white, "one_is_white"],
    do: "one_is_white"

  defp normalize_bw1_polarity!(value),
    do:
      raise(
        ArgumentError,
        ":headless.bw1_polarity must be :one_is_black or :one_is_white, got: #{inspect(value)}"
      )

  defp normalize_headless_prime_opts!(value) do
    opts = normalize_keyword_or_map!(value, ":headless.prime")

    ensure_only_keys!(
      opts,
      [:drm_node, :max_in_flight, :on_backpressure],
      ":headless.prime"
    )

    %{
      drm_node:
        opts
        |> Keyword.get(:drm_node)
        |> normalize_optional_drm_node!(":headless.prime.drm_node"),
      max_in_flight:
        opts
        |> Keyword.get(:max_in_flight, 2)
        |> normalize_positive_integer!(":headless.prime.max_in_flight"),
      on_backpressure:
        opts
        |> Keyword.get(:on_backpressure, :drop_new)
        |> normalize_headless_prime_backpressure!()
    }
  end

  defp normalize_headless_prime_backpressure!(value) when value in [:drop_new, "drop_new"],
    do: "drop_new"

  defp normalize_headless_prime_backpressure!(value),
    do:
      raise(
        ArgumentError,
        ":headless.prime.on_backpressure must be :drop_new, got: #{inspect(value)}"
      )

  defp normalize_optional_drm_node!(nil, _field_name), do: nil

  defp normalize_optional_drm_node!(value, field_name)
       when is_binary(value) and byte_size(value) > 0 do
    if Path.type(value) == :absolute do
      value
    else
      raise ArgumentError, "#{field_name} must be an absolute path, got: #{inspect(value)}"
    end
  end

  defp normalize_optional_drm_node!(value, field_name) do
    raise ArgumentError,
          "#{field_name} must be nil or a non-empty absolute path, got: #{inspect(value)}"
  end

  defp normalize_optional_positive_integer!(nil, _field_name), do: nil

  defp normalize_optional_positive_integer!(value, field_name),
    do: normalize_positive_integer!(value, field_name)

  defp normalize_frame_message!(value) when is_atom(value), do: Atom.to_string(value)
  defp normalize_frame_message!(value) when is_binary(value), do: value

  defp normalize_frame_message!(value),
    do:
      raise(
        ArgumentError,
        ":headless.frame_message must be an atom or string, got: #{inspect(value)}"
      )

  defp normalize_screenshot_region!(opts) do
    case Keyword.get(opts, :region) do
      nil ->
        {nil, nil, nil, nil}

      {x, y, width, height} ->
        {
          normalize_non_negative_integer!(x, ":region x"),
          normalize_non_negative_integer!(y, ":region y"),
          normalize_positive_integer!(width, ":region width"),
          normalize_positive_integer!(height, ":region height")
        }

      value ->
        raise ArgumentError,
              ":region must be {x, y, width, height}, got: #{inspect(value)}"
    end
  end

  defp normalize_screenshot_pixel_format!(value) when value in [:rgba8888, "rgba8888"],
    do: "rgba8888"

  defp normalize_screenshot_pixel_format!(value) when value in [:rgb888, "rgb888"],
    do: "rgb888"

  defp normalize_screenshot_pixel_format!(value) do
    raise ArgumentError,
          ":pixel_format must be :rgba8888 or :rgb888 for screenshots, got: #{inspect(value)}"
  end

  defp normalize_screenshot_background!(value) when value in [:transparent, "transparent"],
    do: "transparent"

  defp normalize_screenshot_background!(value) do
    raise ArgumentError,
          ":background currently only supports :transparent, got: #{inspect(value)}"
  end

  defp normalize_png_compression!(opts) do
    opts = normalize_keyword_or_map!(opts, ":png")

    opts
    |> Keyword.get(:compression, :default)
    |> case do
      value when value in [:default, "default"] -> "default"
      value -> raise ArgumentError, ":png.compression must be :default, got: #{inspect(value)}"
    end
  end

  defp normalize_rendering_api!(value) when value in [:auto, "auto"] do
    rendering_api_config("auto", "auto", false)
  end

  defp normalize_rendering_api!(value) when value in [:opengl, "opengl"] do
    rendering_api_config("opengl", "auto", false)
  end

  defp normalize_rendering_api!(value) when value in [:gl, "gl"] do
    IO.warn("rendering API :gl is deprecated; use :opengl")
    rendering_api_config("opengl", "auto", false)
  end

  defp normalize_rendering_api!(value) when value in [:raster, "raster"] do
    rendering_api_config("raster", "auto", false)
  end

  defp normalize_rendering_api!(value) when value in [:metal, "metal"] do
    rendering_api_config("metal", "auto", false)
  end

  defp normalize_rendering_api!(value) when value in [:vulkan, "vulkan"] do
    rendering_api_config("vulkan", "auto", false)
  end

  defp normalize_rendering_api!(value) when is_list(value) or is_map(value) do
    opts = normalize_keyword_or_map!(value, ":rendering_api")

    case opts do
      [raster: raster_opts] ->
        raster_opts = normalize_keyword_or_map!(raster_opts, ":rendering_api.raster")
        ensure_only_keys!(raster_opts, [:present], ":rendering_api.raster")

        present =
          raster_opts
          |> Keyword.get(:present, :auto)
          |> normalize_raster_present!()

        rendering_api_config("raster", present, Keyword.has_key?(raster_opts, :present))

      [auto: auto_opts] ->
        auto_opts = normalize_keyword_or_map!(auto_opts, ":rendering_api.auto")
        ensure_only_keys!(auto_opts, [:raster], ":rendering_api.auto")
        raster_opts = Keyword.get(auto_opts, :raster, [])
        raster_opts = normalize_keyword_or_map!(raster_opts, ":rendering_api.auto.raster")
        ensure_only_keys!(raster_opts, [:present], ":rendering_api.auto.raster")

        present =
          raster_opts
          |> Keyword.get(:present, :auto)
          |> normalize_raster_present!()

        rendering_api_config("auto", present, Keyword.has_key?(raster_opts, :present))

      _other ->
        raise ArgumentError,
              ":rendering_api must be :auto, :opengl, :raster, :metal, :vulkan, [raster: [present: ...]], or [auto: [raster: [present: ...]]]"
    end
  end

  defp normalize_rendering_api!(value) do
    raise ArgumentError,
          ":rendering_api must be :auto, :opengl, :raster, :metal, :vulkan, [raster: [present: ...]], or [auto: [raster: [present: ...]]], got: #{inspect(value)}"
  end

  defp rendering_api_config(kind, raster_present, raster_present_configured?) do
    %{
      kind: kind,
      raster_present: raster_present,
      raster_present_configured: raster_present_configured?
    }
  end

  defp ensure_only_keys!(opts, allowed_keys, field_name) do
    case Enum.reject(Keyword.keys(opts), &(&1 in allowed_keys)) do
      [] ->
        :ok

      unknown ->
        raise ArgumentError,
              "#{field_name} has unsupported option(s): #{inspect(unknown)}"
    end
  end

  defp normalize_raster_present!(value) when value in [:auto, "auto"], do: "auto"

  defp normalize_raster_present!(value) when value in [:gpu_upload, "gpu_upload"],
    do: "gpu_upload"

  defp normalize_raster_present!(value) when value in [:cpu, "cpu"], do: "cpu"

  defp normalize_raster_present!(value) do
    raise ArgumentError,
          ":rendering_api raster present must be :auto, :gpu_upload, or :cpu, got: #{inspect(value)}"
  end

  defp validate_rendering_api_for_backend!(backend, rendering_api) do
    case String.downcase(backend) do
      "macos" ->
        validate_macos_rendering_api!(rendering_api)

      backend when backend in ["wayland", "drm"] ->
        validate_linux_rendering_api!(backend, rendering_api)

      "headless" ->
        validate_headless_rendering_api!(rendering_api)

      _other ->
        :ok
    end
  end

  defp validate_macos_rendering_api!(%{kind: "opengl"}) do
    raise ArgumentError, "rendering_api: :opengl is not supported with backend: :macos"
  end

  defp validate_macos_rendering_api!(%{kind: "vulkan"}) do
    raise ArgumentError, "rendering_api: :vulkan is not supported with backend: :macos"
  end

  defp validate_macos_rendering_api!(%{raster_present_configured: true}) do
    raise ArgumentError,
          "rendering_api raster present options are only supported with backend: :wayland or :drm"
  end

  defp validate_macos_rendering_api!(_rendering_api), do: :ok

  defp validate_linux_rendering_api!(_backend, %{kind: "metal"}) do
    raise ArgumentError, "rendering_api: :metal is only supported with backend: :macos"
  end

  defp validate_linux_rendering_api!(_backend, _rendering_api), do: :ok

  defp validate_headless_rendering_api!(%{kind: "metal"}) do
    raise ArgumentError, "rendering_api: :metal is only supported with backend: :macos"
  end

  defp validate_headless_rendering_api!(%{raster_present_configured: true}) do
    raise ArgumentError,
          "rendering_api raster present options are only supported with backend: :wayland or :drm"
  end

  defp validate_headless_rendering_api!(_rendering_api), do: :ok

  defp normalize_keyword_list!(opts, error_message) when is_list(opts) do
    if Keyword.keyword?(opts) do
      Keyword.new(opts)
    else
      raise ArgumentError, error_message
    end
  end

  defp option_with_deprecated_alias!(opts, canonical, deprecated, default) do
    case {Keyword.fetch(opts, canonical), Keyword.fetch(opts, deprecated)} do
      {{:ok, _canonical_value}, {:ok, _deprecated_value}} ->
        raise ArgumentError,
              "#{inspect(canonical)} and deprecated #{inspect(deprecated)} cannot be used together"

      {{:ok, value}, :error} ->
        value

      {:error, {:ok, value}} ->
        IO.warn("#{deprecated} is deprecated; use #{canonical} instead")
        value

      {:error, :error} ->
        default
    end
  end

  defp normalize_backend!(value) when is_atom(value), do: Atom.to_string(value)
  defp normalize_backend!(value) when is_binary(value), do: value

  defp normalize_backend!(_value) do
    raise ArgumentError, "backend must be an atom or string"
  end

  defp normalize_optional_string(nil), do: nil
  defp normalize_optional_string(value), do: to_string(value)
end
