defmodule Emerge.Config.Changes do
  @moduledoc false
  alias Emerge.Config

  def validate!(previous, config) do
    if static(previous) != static(config) do
      raise ArgumentError,
            "config changed a startup-only field (output type/device, rendering API, assets, cursor or adapter); start a new viewport for that change"
    end

    :ok
  end

  def display_off?(%Config{output: %Config.DRM{display: :off}}), do: true
  def display_off?(_), do: false

  defp static(config) do
    %{
      output: static_output(config.output),
      renderer: Map.drop(config.renderer, [:target_fps]),
      input: Map.drop(config.input, [:events]),
      assets: config.assets,
      diagnostics: config.diagnostics,
      runtime: Map.drop(config.runtime, [:health_check_interval_ms])
    }
  end

  defp static_output(%Config.Window{} = output), do: Map.drop(output, [:title])

  defp static_output(%Config.DRM{} = output) do
    output
    |> Map.drop([:connector, :mode, :display])
    |> Map.put(:backlight, Map.drop(output.backlight, [:brightness]))
  end

  defp static_output(output), do: output
end
