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

    backend =
      opts
      |> Keyword.get(:backend, EmergeSkia.BuildConfig.default_runtime_backend())
      |> normalize_backend!()

    %{
      backend: backend,
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
      hw_cursor: Keyword.get(opts, :hw_cursor, true),
      input_log: Keyword.get(opts, :input_log, false),
      render_log: Keyword.get(opts, :render_log, false)
    }
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
  def normalize_asset_mode!(:await), do: "await"
  def normalize_asset_mode!(:snapshot), do: "snapshot"
  def normalize_asset_mode!("await"), do: "await"
  def normalize_asset_mode!("snapshot"), do: "snapshot"

  def normalize_asset_mode!(value) do
    raise ArgumentError,
          ":asset_mode must be :await or :snapshot, got: #{inspect(value)}"
  end

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
