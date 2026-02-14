defmodule Emerge.Assets.Config do
  @moduledoc false

  @default_manifest_path "priv/static/cache_manifest.json"
  @default_images_meta_path "priv/static/cache_manifest_images.json"

  @default_extensions [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]

  @spec fetch() :: map()
  def fetch do
    defaults()
    |> deep_merge(normalize(Application.get_env(:emerge_skia, :assets, [])))
    |> validate!()
  end

  @spec defaults() :: map()
  def defaults do
    %{
      sources: ["assets"],
      manifest: %{
        path: @default_manifest_path,
        images_meta_path: @default_images_meta_path
      },
      runtime_paths: %{
        enabled: false,
        allowlist: [],
        follow_symlinks: false,
        max_file_size: 25_000_000,
        extensions: @default_extensions
      }
    }
  end

  defp normalize(value) when is_map(value) do
    value
    |> Enum.map(fn {k, v} -> {k, normalize(v)} end)
    |> Map.new()
  end

  defp normalize(value) when is_list(value) do
    if Keyword.keyword?(value) do
      value
      |> Enum.map(fn {k, v} -> {k, normalize(v)} end)
      |> Map.new()
    else
      Enum.map(value, &normalize/1)
    end
  end

  defp normalize(value), do: value

  defp deep_merge(a, b) when is_map(a) and is_map(b) do
    Map.merge(a, b, fn _key, left, right -> deep_merge(left, right) end)
  end

  defp deep_merge(_left, right), do: right

  defp validate!(config) do
    sources = Map.get(config, :sources, [])

    if not (is_list(sources) and Enum.all?(sources, &is_binary/1)) do
      raise ArgumentError, "assets.sources must be a list of paths"
    end

    runtime = Map.get(config, :runtime_paths, %{})
    allowlist = Map.get(runtime, :allowlist, [])

    if not (is_list(allowlist) and Enum.all?(allowlist, &is_binary/1)) do
      raise ArgumentError, "assets.runtime_paths.allowlist must be a list of paths"
    end

    extensions = Map.get(runtime, :extensions, @default_extensions)

    if not (is_list(extensions) and Enum.all?(extensions, &is_binary/1)) do
      raise ArgumentError, "assets.runtime_paths.extensions must be a list of extensions"
    end

    max_file_size = Map.get(runtime, :max_file_size, 0)

    if not (is_integer(max_file_size) and max_file_size > 0) do
      raise ArgumentError, "assets.runtime_paths.max_file_size must be a positive integer"
    end

    config
  end
end
