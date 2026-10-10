defmodule Emerge.Config.Normalize do
  @moduledoc false

  alias Emerge.Config

  @spec config!(Config.t(), module() | nil) :: Config.normalized_t()
  def config!(config, viewport_module \\ nil)

  def config!(%Config{} = config, viewport_module) do
    %Config{assets: %Config.Assets{} = assets} = config = record!(Config, config, "config")
    app = assets.otp_app || infer_app(viewport_module)
    assets = %Config.Assets{assets | otp_app: app}
    config = %Config{config | assets: assets}
    validate_output!(config)
    config
  end

  def config!(value, _module),
    do: invalid!("config", value, "%Emerge.Config{} (keyword configuration was removed in 0.5)")

  defp infer_app(nil), do: nil

  defp infer_app(module) do
    Application.get_application(module) ||
      Enum.find_value(Application.loaded_applications(), fn {app, _, _} ->
        if Atom.to_string(app) == module |> Module.split() |> hd() |> Macro.underscore(), do: app
      end)
  end

  defp record!(module, value, path) do
    values =
      case value do
        %{__struct__: ^module} ->
          Map.from_struct(value)

        list when is_list(list) ->
          unless Keyword.keyword?(list),
            do: invalid!(path, value, "#{inspect(module)} or its typed keyword options")

          if length(Keyword.keys(list)) != length(Enum.uniq(Keyword.keys(list))),
            do: invalid!(path, value, "unique keyword fields")

          Map.new(list)

        _ ->
          invalid!(path, value, "#{inspect(module)} or its typed keyword options")
      end

    schema = module.__schema__()
    allowed = Keyword.keys(schema)

    Enum.each(Map.keys(values), fn key ->
      unless key in allowed,
        do: invalid!("#{path}.#{key}", Map.fetch!(values, key), "one of #{inspect(allowed)}")
    end)

    defaults = struct(module)

    fields =
      Enum.map(schema, fn {key, type} ->
        {key, value!(Map.get(values, key, Map.fetch!(defaults, key)), type, "#{path}.#{key}")}
      end)

    struct!(module, fields)
  end

  defp value!(nil, {:optional, _}, _), do: nil
  defp value!(v, {:optional, type}, p), do: value!(v, type, p)
  defp value!(v, {:record, module}, p), do: record!(module, v, p)

  defp value!(%{__struct__: module} = v, {:variants, modules}, p) do
    if module in modules, do: record!(module, v, p), else: invalid!(p, v, inspect(modules))
  end

  defp value!(v, {:enum, values}, p) do
    if v in values, do: v, else: invalid!(p, v, "one of #{inspect(values)}")
  end

  defp value!(v, {:one_of, types}, p) do
    cond do
      v in types -> v
      :boolean in types and is_boolean(v) -> v
      :path in types and is_binary(v) -> value!(v, :path, p)
      true -> invalid!(p, v, inspect(types))
    end
  end

  defp value!(v, {:list, type}, p) when is_list(v),
    do: Enum.map(v, &value!(&1, type, p <> "[]"))

  defp value!(v, {:set, type}, p) when is_list(v),
    do: v |> Enum.map(&value!(&1, type, p <> "[]")) |> Enum.uniq() |> Enum.sort()

  defp value!(v, {:range, lo, hi}, _) when is_number(v) and v >= lo and v <= hi, do: v / 1.0
  defp value!(v, {:integer_range, lo, hi}, _) when is_integer(v) and v >= lo and v <= hi, do: v
  defp value!(v, :boolean, _) when is_boolean(v), do: v
  defp value!(v, :string, _) when is_binary(v), do: v
  defp value!(v, :non_empty_string, _) when is_binary(v) and byte_size(v) > 0, do: v

  defp value!(v, :path, p) when is_binary(v) and byte_size(v) > 0 do
    if Path.type(v) == :absolute, do: v, else: invalid!(p, v, "an absolute path")
  end

  defp value!(v, :module, _) when is_atom(v) and not is_nil(v), do: v
  defp value!(v, :positive_integer, _) when is_integer(v) and v > 0, do: v
  defp value!(v, :non_negative_integer, _) when is_integer(v) and v >= 0, do: v
  defp value!(v, :positive_number, _) when is_number(v) and v > 0, do: v / 1.0

  defp value!({w, h} = v, :size, _) when is_integer(w) and is_integer(h) and w > 0 and h > 0,
    do: v

  defp value!({x, y} = v, :position, _) when is_number(x) and is_number(y) and x >= 0 and y >= 0,
    do: v

  defp value!(v, :local_pid, p) when is_pid(v) do
    if node(v) == node() and Process.alive?(v), do: v, else: invalid!(p, v, "a live local PID")
  end

  defp value!(v, :message_tag, _) when is_atom(v) or is_binary(v), do: v
  defp value!(nil, :mode, _), do: nil
  defp value!(%{id: id}, :mode, p), do: value!(id, :non_empty_string, p)
  defp value!(v, :mode, p), do: value!(v, :non_empty_string, p)
  defp value!(nil, :asset_source, _), do: nil
  defp value!(%Emerge.Assets.Ref{} = v, :asset_source, _), do: v
  defp value!(v, :asset_source, p), do: value!(v, :non_empty_string, p)

  defp value!(v, :keyword, p) when is_list(v) do
    if Keyword.keyword?(v), do: v, else: invalid!(p, v, "a keyword list")
  end

  defp value!(v, type, p), do: invalid!(p, v, inspect(type))

  defp validate_output!(%Config{output: %Config.DRM{} = drm, renderer: renderer}) do
    if drm.mode && is_nil(drm.connector),
      do: invalid!("config.output.mode", drm.mode, "an explicit connector")

    if renderer.api == :vulkan and is_nil(renderer.device),
      do: invalid!("config.renderer.device", nil, "an explicit Vulkan DRM device")

    if drm.backlight.device == nil and drm.backlight.brightness != nil,
      do:
        invalid!(
          "config.output.backlight.brightness",
          drm.backlight.brightness,
          "a managed backlight device"
        )
  end

  defp validate_output!(%Config{output: %Config.Headless{target: nil}}),
    do: invalid!("config.output.target", nil, "a live local PID")

  defp validate_output!(_), do: :ok

  defp invalid!(path, value, expected),
    do: raise(ArgumentError, "#{path} expects #{expected}, got: #{inspect(value)}")
end
