defmodule EmergeSkia.Assets do
  @moduledoc false

  alias Emerge.Assets.Ref
  alias EmergeSkia.Native
  alias EmergeSkia.Options

  @supported_drm_cursor_icons [:default, :text, :pointer]
  @supported_drm_cursor_extensions [".png", ".svg"]
  @default_runtime_extensions [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]
  @default_runtime_max_file_size 25_000_000
  @default_font_extensions [".ttf", ".otf", ".ttc"]

  @type config :: %{
          otp_app: atom(),
          priv_dir: String.t(),
          runtime_enabled: boolean(),
          runtime_allowlist: [String.t()],
          runtime_follow_symlinks: boolean(),
          runtime_max_file_size: pos_integer(),
          runtime_extensions: [String.t()],
          fonts: [font()]
        }

  @type font :: %{
          family: String.t(),
          source: String.t(),
          weight: 100..900,
          italic: boolean()
        }

  @type drm_cursor_override :: %{
          icon: String.t(),
          source: String.t(),
          hotspot_x: float(),
          hotspot_y: float()
        }

  @doc false
  @spec normalize_asset_config!(keyword()) :: config()
  def normalize_asset_config!(opts) do
    otp_app = normalize_otp_app!(opts)

    assets_opts =
      opts
      |> Keyword.get(:assets, [])
      |> Options.normalize_keyword_or_map!("assets")

    runtime_opts =
      assets_opts
      |> Keyword.get(:runtime_paths, [])
      |> Options.normalize_keyword_or_map!("assets.runtime_paths")

    runtime_allowlist =
      runtime_opts
      |> Keyword.get(:allowlist, [])
      |> normalize_path_list!("assets.runtime_paths.allowlist")

    runtime_extensions =
      runtime_opts
      |> Keyword.get(:extensions, @default_runtime_extensions)
      |> Options.normalize_string_list!("assets.runtime_paths.extensions")

    fonts =
      assets_opts
      |> Keyword.get(:fonts, [])
      |> normalize_fonts!()

    runtime_max_file_size =
      Keyword.get(runtime_opts, :max_file_size, @default_runtime_max_file_size)

    runtime_enabled = Keyword.get(runtime_opts, :enabled, false)
    runtime_follow_symlinks = Keyword.get(runtime_opts, :follow_symlinks, false)

    if not is_boolean(runtime_enabled) do
      raise ArgumentError, "assets.runtime_paths.enabled must be a boolean"
    end

    if not is_boolean(runtime_follow_symlinks) do
      raise ArgumentError, "assets.runtime_paths.follow_symlinks must be a boolean"
    end

    if not (is_integer(runtime_max_file_size) and runtime_max_file_size > 0) do
      raise ArgumentError, "assets.runtime_paths.max_file_size must be a positive integer"
    end

    %{
      otp_app: otp_app,
      priv_dir: otp_app_priv_dir!(otp_app),
      runtime_enabled: runtime_enabled,
      runtime_allowlist: runtime_allowlist,
      runtime_follow_symlinks: runtime_follow_symlinks,
      runtime_max_file_size: runtime_max_file_size,
      runtime_extensions: runtime_extensions,
      fonts: fonts
    }
  end

  @doc false
  @spec initialize_renderer_assets(reference(), config()) :: :ok | {:error, term()}
  def initialize_renderer_assets(renderer, asset_config) do
    :ok = configure_assets_for_renderer(renderer, asset_config)
    preload_font_assets(asset_config)
  end

  @doc false
  @spec native_start_asset_config(config()) :: map()
  def native_start_asset_config(asset_config) do
    %{
      asset_sources: [asset_config.priv_dir],
      asset_runtime_enabled: asset_config.runtime_enabled,
      asset_allowlist: asset_config.runtime_allowlist,
      asset_follow_symlinks: asset_config.runtime_follow_symlinks,
      asset_max_file_size: asset_config.runtime_max_file_size,
      asset_extensions: asset_config.runtime_extensions
    }
  end

  @doc false
  @spec normalize_drm_cursor_overrides!(keyword()) :: [drm_cursor_override()]
  def normalize_drm_cursor_overrides!(opts) do
    opts
    |> Keyword.get(:drm_cursor, [])
    |> normalize_drm_cursor_entries!()
  end

  @doc false
  @spec preload_font_assets(config()) :: :ok | {:error, term()}
  def preload_font_assets(%{fonts: []}), do: :ok

  def preload_font_assets(%{fonts: fonts, priv_dir: priv_dir}) do
    Enum.reduce_while(fonts, :ok, fn font, :ok ->
      absolute_path = Path.join(priv_dir, font.source)

      case File.read(absolute_path) do
        {:ok, data} ->
          case normalize_native_ok(
                 Native.load_font_nif(font.family, font.weight, font.italic, data)
               ) do
            :ok ->
              {:cont, :ok}

            {:error, reason} ->
              {:halt,
               {:error,
                {:font_asset_load_failed,
                 %{font: font_key(font), source: font.source, reason: reason}}}}
          end

        {:error, reason} ->
          {:halt,
           {:error,
            {:font_asset_read_failed,
             %{font: font_key(font), source: font.source, path: absolute_path, reason: reason}}}}
      end
    end)
  end

  @spec load_font_file(String.t(), non_neg_integer(), boolean(), Path.t()) ::
          :ok | {:error, term()}
  def load_font_file(name, weight, italic, path) do
    case File.read(path) do
      {:ok, data} -> normalize_native_ok(Native.load_font_nif(name, weight, italic, data))
      {:error, reason} -> {:error, reason}
    end
  end

  defp configure_assets_for_renderer(renderer, asset_config) do
    Native.configure_assets_nif(
      renderer,
      [asset_config.priv_dir],
      asset_config.runtime_enabled,
      asset_config.runtime_allowlist,
      asset_config.runtime_follow_symlinks,
      asset_config.runtime_max_file_size,
      asset_config.runtime_extensions
    )
  end

  defp normalize_otp_app!(opts) do
    case Keyword.fetch(opts, :otp_app) do
      {:ok, value} when is_atom(value) ->
        value

      {:ok, value} ->
        raise ArgumentError,
              "otp_app must be an atom, got: #{inspect(value)}"

      :error ->
        raise ArgumentError,
              "missing required :otp_app option; use EmergeSkia.start(otp_app: :my_app, ...)"
    end
  end

  defp normalize_fonts!(fonts) do
    entries = Options.normalize_list!(fonts, "assets.fonts")

    normalized =
      Enum.map(entries, fn entry ->
        opts = Options.normalize_keyword_or_map!(entry, "assets.fonts[]")

        family =
          opts
          |> Keyword.fetch!(:family)
          |> Options.normalize_non_empty_string!("assets.fonts[].family")

        source =
          opts
          |> Keyword.fetch!(:source)
          |> normalize_font_source!()

        weight =
          opts
          |> Keyword.get(:weight, 400)
          |> normalize_font_weight!()

        italic =
          opts
          |> Keyword.get(:italic, false)
          |> Options.normalize_boolean!("assets.fonts[].italic")

        extension = Path.extname(source) |> String.downcase()

        if extension not in @default_font_extensions do
          raise ArgumentError,
                "assets.fonts[].source extension must be one of #{inspect(@default_font_extensions)}, got: #{inspect(source)}"
        end

        %{
          family: family,
          source: source,
          weight: weight,
          italic: italic
        }
      end)

    ensure_unique_fonts!(normalized)
    normalized
  end

  defp normalize_font_weight!(weight) when is_integer(weight) and weight in 100..900, do: weight

  defp normalize_font_weight!(weight) do
    raise ArgumentError,
          "assets.fonts[].weight must be an integer between 100 and 900, got: #{inspect(weight)}"
  end

  defp normalize_font_source!(%Emerge.Assets.Ref{path: path}) when is_binary(path) do
    normalize_logical_source!(path)
  end

  defp normalize_font_source!(path) when is_binary(path) do
    normalize_logical_source!(path)
  end

  defp normalize_font_source!(other) do
    raise ArgumentError,
          "assets.fonts[].source must be a logical string path or %Emerge.Assets.Ref{}, got: #{inspect(other)}"
  end

  defp normalize_logical_source!(path) do
    normalized =
      path
      |> String.trim()
      |> String.trim_leading("/")

    if normalized == "" do
      raise ArgumentError, "assets.fonts[].source must not be empty"
    end

    if Enum.any?(Path.split(normalized), &(&1 == "..")) do
      raise ArgumentError,
            "assets.fonts[].source must be relative and may not contain '..': #{inspect(path)}"
    end

    normalized
  end

  defp normalize_drm_cursor_entries!([]), do: []

  defp normalize_drm_cursor_entries!(entries) do
    entries
    |> Options.normalize_keyword_or_map!("drm_cursor")
    |> Enum.map(fn {icon, entry} ->
      icon = normalize_drm_cursor_icon!(icon)
      entry = Options.normalize_keyword_or_map!(entry, "drm_cursor.#{icon}")

      validate_allowed_keys!(entry, ["source", "hotspot"], "drm_cursor.#{icon}")

      source =
        entry
        |> fetch_option!(:source, "drm_cursor.#{icon}.source")
        |> normalize_drm_cursor_source!("drm_cursor.#{icon}.source")

      {hotspot_x, hotspot_y} =
        entry
        |> fetch_option!(:hotspot, "drm_cursor.#{icon}.hotspot")
        |> normalize_drm_cursor_hotspot!("drm_cursor.#{icon}.hotspot")

      %{
        icon: icon,
        source: source,
        hotspot_x: hotspot_x,
        hotspot_y: hotspot_y
      }
    end)
  end

  defp normalize_drm_cursor_icon!(icon) when icon in @supported_drm_cursor_icons,
    do: Atom.to_string(icon)

  defp normalize_drm_cursor_icon!(icon) when is_binary(icon) do
    normalized = icon |> String.trim() |> String.downcase()

    if normalized in Enum.map(@supported_drm_cursor_icons, &Atom.to_string/1) do
      normalized
    else
      raise ArgumentError,
            "drm_cursor keys must be one of #{inspect(@supported_drm_cursor_icons)}, got: #{inspect(icon)}"
    end
  end

  defp normalize_drm_cursor_icon!(icon) do
    raise ArgumentError,
          "drm_cursor keys must be one of #{inspect(@supported_drm_cursor_icons)}, got: #{inspect(icon)}"
  end

  defp normalize_drm_cursor_source!(%Ref{path: path}, field_name) when is_binary(path) do
    path
    |> normalize_logical_source!()
    |> validate_drm_cursor_extension!(field_name)
  end

  defp normalize_drm_cursor_source!(path, field_name) when is_binary(path) do
    normalized = Options.normalize_non_empty_string!(path, field_name)

    case Path.type(normalized) do
      :absolute ->
        normalized
        |> Path.expand()
        |> validate_drm_cursor_extension!(field_name)

      _ ->
        normalized
        |> normalize_logical_source!()
        |> validate_drm_cursor_extension!(field_name)
    end
  end

  defp normalize_drm_cursor_source!(other, field_name) do
    raise ArgumentError,
          "#{field_name} must be a logical string path, absolute runtime path, or %Emerge.Assets.Ref{}, got: #{inspect(other)}"
  end

  defp normalize_drm_cursor_hotspot!({x, y}, field_name) do
    {normalize_non_negative_number!(x, field_name), normalize_non_negative_number!(y, field_name)}
  end

  defp normalize_drm_cursor_hotspot!(value, field_name) do
    raise ArgumentError,
          "#{field_name} must be a {x, y} tuple of non-negative numbers, got: #{inspect(value)}"
  end

  defp normalize_non_negative_number!(value, _field_name) when is_integer(value) and value >= 0,
    do: value / 1.0

  defp normalize_non_negative_number!(value, _field_name)
       when is_float(value) and value >= 0.0,
       do: value

  defp normalize_non_negative_number!(value, field_name) do
    raise ArgumentError,
          "#{field_name} must contain non-negative finite numbers, got: #{inspect(value)}"
  end

  defp validate_drm_cursor_extension!(path, field_name) do
    extension = Path.extname(path) |> String.downcase()

    if extension in @supported_drm_cursor_extensions do
      path
    else
      raise ArgumentError,
            "#{field_name} extension must be one of #{inspect(@supported_drm_cursor_extensions)}, got: #{inspect(path)}"
    end
  end

  defp validate_allowed_keys!(opts, allowed_keys, field_name) do
    invalid_keys =
      opts
      |> Enum.map(fn {key, _value} -> key end)
      |> Enum.reject(&(to_string(&1) in allowed_keys))

    if invalid_keys != [] do
      raise ArgumentError,
            "#{field_name} supports only #{inspect(allowed_keys)} keys, got: #{inspect(invalid_keys)}"
    end
  end

  defp fetch_option!(opts, key, field_name) do
    string_key = Atom.to_string(key)

    case Enum.find_value(opts, fn
           {^key, value} -> {:ok, value}
           {^string_key, value} -> {:ok, value}
           _ -> nil
         end) do
      {:ok, value} -> value
      nil -> raise ArgumentError, "missing required #{field_name} option"
    end
  end

  defp normalize_path_list!(list, field_name) do
    list
    |> Options.normalize_string_list!(field_name)
    |> Enum.map(&Path.expand/1)
  end

  defp ensure_unique_fonts!(fonts) do
    keys = Enum.map(fonts, &font_key/1)
    duplicates = keys -- Enum.uniq(keys)

    if duplicates != [] do
      duplicates = duplicates |> Enum.uniq() |> Enum.map_join(", ", &inspect/1)
      raise ArgumentError, "duplicate assets.fonts entries for variants: #{duplicates}"
    end
  end

  defp font_key(%{family: family, weight: weight, italic: italic}), do: {family, weight, italic}

  defp normalize_native_ok(:ok), do: :ok
  defp normalize_native_ok({:ok, _}), do: :ok
  defp normalize_native_ok({:error, reason}), do: {:error, reason}

  defp otp_app_priv_dir!(otp_app) do
    case :code.priv_dir(otp_app) do
      path when is_list(path) ->
        List.to_string(path)

      _ ->
        raise ArgumentError,
              "could not resolve priv dir for otp_app #{inspect(otp_app)}; ensure the application is part of your release"
    end
  end
end
