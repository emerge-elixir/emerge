defmodule Emerge.Runtime.Viewport.Config do
  @moduledoc false
  alias Emerge.Config.Normalize

  @required_renderer_callbacks [
    start: 2,
    stop: 1,
    running?: 1,
    set_input_target: 2,
    set_log_target: 2,
    set_input_mask: 2,
    upload_tree: 2,
    patch_tree: 3,
    configure: 3
  ]

  @enforce_keys [
    :config,
    :skia_opts,
    :renderer_module,
    :renderer_opts,
    :input_mask,
    :renderer_check_interval_ms
  ]
  defstruct @enforce_keys

  @type t :: %__MODULE__{
          config: Emerge.Config.normalized_t(),
          skia_opts: keyword(),
          renderer_module: module(),
          renderer_opts: keyword(),
          input_mask: non_neg_integer(),
          renderer_check_interval_ms: non_neg_integer() | nil
        }

  def parse!(module, config) do
    config = Normalize.config!(config, module)
    validate_renderer!(config.runtime.adapter)

    if config.runtime.adapter == Emerge.Runtime.Viewport.Renderer.Skia,
      do: EmergeSkia.validate_config!(config)

    %__MODULE__{
      config: config,
      skia_opts: Emerge.Config.Native.options(config),
      renderer_module: config.runtime.adapter,
      renderer_opts: config.runtime.adapter_opts,
      input_mask: Emerge.Config.Native.input_mask(config.input),
      renderer_check_interval_ms: config.runtime.health_check_interval_ms
    }
  end

  defp validate_renderer!(module) do
    case Code.ensure_loaded(module) do
      {:module, _} ->
        :ok

      {:error, reason} ->
        raise ArgumentError,
              "config.runtime.adapter #{inspect(module)} could not be loaded: #{inspect(reason)}"
    end

    missing =
      Enum.reject(@required_renderer_callbacks, fn {name, arity} ->
        function_exported?(module, name, arity)
      end)

    if missing != [],
      do:
        raise(
          ArgumentError,
          "config.runtime.adapter #{inspect(module)} is missing #{inspect(missing)}"
        )
  end
end
