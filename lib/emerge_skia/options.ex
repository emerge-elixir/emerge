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
            "macos_backend has been removed; use backend_renderer: :auto | :metal | :raster instead"
    end

    backend =
      opts
      |> Keyword.get(:backend, EmergeSkia.BuildConfig.default_runtime_backend())
      |> normalize_backend!()

    backend_renderer =
      opts
      |> Keyword.get(:backend_renderer, :auto)
      |> normalize_backend_renderer!()

    validate_backend_renderer_for_backend!(backend, backend_renderer)

    renderer_cache =
      opts
      |> Keyword.get(:renderer_cache, [])
      |> normalize_renderer_cache_opts!()
      |> maybe_disable_renderer_cache_for_raster_default!(
        backend_renderer,
        Keyword.has_key?(opts, :renderer_cache)
      )

    %{
      backend: backend,
      backend_renderer: backend_renderer,
      title: Keyword.get(opts, :title, "Emerge"),
      width: Keyword.get(opts, :width, 800),
      height: Keyword.get(opts, :height, 600),
      drm_card: normalize_optional_string(Keyword.get(opts, :drm_card)),
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
      renderer_cache: renderer_cache
    }
  end

  @doc false
  def backend_renderer_start_error(%{backend: backend, backend_renderer: backend_renderer}) do
    case {String.downcase(backend), backend_renderer.kind} do
      {"headless", _kind} ->
        "headless backend is not implemented yet"

      {backend, "raster"} when backend in ["wayland", "drm"] ->
        "backend_renderer :raster is not implemented yet for backend :#{backend}"

      {backend, "vulkan"} when backend in ["wayland", "drm", "headless"] ->
        "backend_renderer :vulkan is not implemented yet"

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
         renderer_cache,
         %{kind: "raster"},
         false
       ) do
    %{renderer_cache | enabled: false}
  end

  defp maybe_disable_renderer_cache_for_raster_default!(
         renderer_cache,
         _backend_renderer,
         _configured?
       ),
       do: renderer_cache

  defp normalize_backend_renderer!(value) when value in [:auto, "auto"] do
    backend_renderer_config("auto", "auto", false)
  end

  defp normalize_backend_renderer!(value) when value in [:gl, "gl"] do
    backend_renderer_config("gl", "auto", false)
  end

  defp normalize_backend_renderer!(value) when value in [:raster, "raster"] do
    backend_renderer_config("raster", "auto", false)
  end

  defp normalize_backend_renderer!(value) when value in [:metal, "metal"] do
    backend_renderer_config("metal", "auto", false)
  end

  defp normalize_backend_renderer!(value) when value in [:vulkan, "vulkan"] do
    backend_renderer_config("vulkan", "auto", false)
  end

  defp normalize_backend_renderer!(value) when is_list(value) or is_map(value) do
    opts = normalize_keyword_or_map!(value, ":backend_renderer")

    case opts do
      [raster: raster_opts] ->
        raster_opts = normalize_keyword_or_map!(raster_opts, ":backend_renderer.raster")
        ensure_only_keys!(raster_opts, [:present], ":backend_renderer.raster")

        present =
          raster_opts
          |> Keyword.get(:present, :auto)
          |> normalize_raster_present!()

        backend_renderer_config("raster", present, Keyword.has_key?(raster_opts, :present))

      [auto: auto_opts] ->
        auto_opts = normalize_keyword_or_map!(auto_opts, ":backend_renderer.auto")
        ensure_only_keys!(auto_opts, [:raster], ":backend_renderer.auto")
        raster_opts = Keyword.get(auto_opts, :raster, [])
        raster_opts = normalize_keyword_or_map!(raster_opts, ":backend_renderer.auto.raster")
        ensure_only_keys!(raster_opts, [:present], ":backend_renderer.auto.raster")

        present =
          raster_opts
          |> Keyword.get(:present, :auto)
          |> normalize_raster_present!()

        backend_renderer_config("auto", present, Keyword.has_key?(raster_opts, :present))

      _other ->
        raise ArgumentError,
              ":backend_renderer must be :auto, :gl, :raster, :metal, :vulkan, [raster: [present: ...]], or [auto: [raster: [present: ...]]]"
    end
  end

  defp normalize_backend_renderer!(value) do
    raise ArgumentError,
          ":backend_renderer must be :auto, :gl, :raster, :metal, :vulkan, [raster: [present: ...]], or [auto: [raster: [present: ...]]], got: #{inspect(value)}"
  end

  defp backend_renderer_config(kind, raster_present, raster_present_configured?) do
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
          ":backend_renderer raster present must be :auto, :gpu_upload, or :cpu, got: #{inspect(value)}"
  end

  defp validate_backend_renderer_for_backend!(backend, backend_renderer) do
    case String.downcase(backend) do
      "macos" ->
        validate_macos_backend_renderer!(backend_renderer)

      backend when backend in ["wayland", "drm"] ->
        validate_linux_backend_renderer!(backend, backend_renderer)

      _other ->
        :ok
    end
  end

  defp validate_macos_backend_renderer!(%{kind: "gl"}) do
    raise ArgumentError, "backend_renderer: :gl is not supported with backend: :macos"
  end

  defp validate_macos_backend_renderer!(%{kind: "vulkan"}) do
    raise ArgumentError, "backend_renderer: :vulkan is not supported with backend: :macos"
  end

  defp validate_macos_backend_renderer!(%{raster_present_configured: true}) do
    raise ArgumentError,
          "backend_renderer raster present options are only supported with backend: :wayland or :drm"
  end

  defp validate_macos_backend_renderer!(_backend_renderer), do: :ok

  defp validate_linux_backend_renderer!(_backend, %{kind: "metal"}) do
    raise ArgumentError, "backend_renderer: :metal is only supported with backend: :macos"
  end

  defp validate_linux_backend_renderer!(_backend, _backend_renderer), do: :ok

  defp normalize_keyword_list!(opts, error_message) when is_list(opts) do
    if Keyword.keyword?(opts) do
      Keyword.new(opts)
    else
      raise ArgumentError, error_message
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
