defmodule Emerge.Runtime.Viewport.Config do
  @moduledoc false

  alias Emerge.Runtime.Viewport.Renderer.Skia

  @default_renderer_check_interval_ms 500
  @required_renderer_callbacks [
    start: 2,
    stop: 1,
    running?: 1,
    set_input_target: 2,
    set_input_mask: 2,
    upload_tree: 2,
    patch_tree: 3
  ]

  @enforce_keys [
    :skia_opts,
    :renderer_module,
    :renderer_opts,
    :input_mask,
    :renderer_check_interval_ms
  ]
  defstruct @enforce_keys

  @type t :: %__MODULE__{
          skia_opts: keyword(),
          renderer_module: module(),
          renderer_opts: keyword(),
          input_mask: non_neg_integer() | nil,
          renderer_check_interval_ms: non_neg_integer() | nil
        }

  @spec parse!(module(), keyword()) :: t()
  def parse!(module, opts) when is_atom(module) and is_list(opts) do
    viewport_opts = viewport_opts!(opts)

    %__MODULE__{
      skia_opts: normalize_skia_opts!(opts, module),
      renderer_module:
        validate_renderer_module!(Keyword.get(viewport_opts, :renderer_module, Skia)),
      renderer_opts: renderer_opts!(viewport_opts),
      input_mask: input_mask!(viewport_opts),
      renderer_check_interval_ms: renderer_check_interval_ms!(viewport_opts)
    }
  end

  defp normalize_skia_opts!(opts, module) when is_list(opts) and is_atom(module) do
    explicit_skia_opts =
      case Keyword.fetch(opts, :emerge_skia) do
        {:ok, value} when is_list(value) ->
          value

        {:ok, other} ->
          raise ArgumentError,
                "mount/1 emerge_skia option must be a keyword list, got: #{inspect(other)}"

        :error ->
          []
      end

    opts
    |> Keyword.drop([:emerge_skia, :viewport])
    |> Keyword.merge(explicit_skia_opts)
    |> ensure_skia_otp_app!(module)
  end

  defp viewport_opts!(opts) when is_list(opts) do
    case Keyword.get(opts, :viewport, []) do
      value when is_list(value) ->
        value

      other ->
        raise ArgumentError,
              "mount/1 viewport option must be a keyword list, got: #{inspect(other)}"
    end
  end

  defp validate_renderer_module!(renderer_module) when is_atom(renderer_module) do
    case Code.ensure_loaded(renderer_module) do
      {:module, _module} ->
        :ok

      {:error, reason} ->
        raise ArgumentError,
              "viewport renderer_module #{inspect(renderer_module)} could not be loaded: #{inspect(reason)}"
    end

    missing_renderer_callbacks =
      @required_renderer_callbacks
      |> Enum.reject(fn {name, arity} -> function_exported?(renderer_module, name, arity) end)
      |> Enum.map_join(", ", fn {name, arity} -> "#{name}/#{arity}" end)

    unless missing_renderer_callbacks == "" do
      raise ArgumentError,
            "viewport renderer_module #{inspect(renderer_module)} must implement Emerge.Runtime.Viewport.Renderer callbacks (missing: #{missing_renderer_callbacks})"
    end

    renderer_module
  end

  defp validate_renderer_module!(renderer_module) do
    raise ArgumentError,
          "viewport renderer_module must be a module, got: #{inspect(renderer_module)}"
  end

  defp renderer_opts!(viewport_opts) when is_list(viewport_opts) do
    case Keyword.get(viewport_opts, :renderer_opts, []) do
      value when is_list(value) ->
        value

      other ->
        raise ArgumentError,
              "viewport renderer_opts must be a keyword list, got: #{inspect(other)}"
    end
  end

  defp input_mask!(viewport_opts) when is_list(viewport_opts) do
    input_mask = Keyword.get(viewport_opts, :input_mask, nil)

    unless is_nil(input_mask) or (is_integer(input_mask) and input_mask >= 0) do
      raise ArgumentError,
            "viewport input_mask must be nil or a non-negative integer, got: #{inspect(input_mask)}"
    end

    input_mask
  end

  defp renderer_check_interval_ms!(viewport_opts) when is_list(viewport_opts) do
    renderer_check_interval_ms =
      Keyword.get(viewport_opts, :renderer_check_interval_ms, @default_renderer_check_interval_ms)

    unless is_nil(renderer_check_interval_ms) or
             (is_integer(renderer_check_interval_ms) and renderer_check_interval_ms >= 0) do
      raise ArgumentError,
            "viewport renderer_check_interval_ms must be nil or a non-negative integer, got: #{inspect(renderer_check_interval_ms)}"
    end

    renderer_check_interval_ms
  end

  defp ensure_skia_otp_app!(skia_opts, module) when is_list(skia_opts) and is_atom(module) do
    case Keyword.fetch(skia_opts, :otp_app) do
      {:ok, otp_app} when is_atom(otp_app) ->
        skia_opts

      {:ok, other} ->
        raise ArgumentError,
              "mount/1 otp_app must be an atom, got: #{inspect(other)}"

      :error ->
        Keyword.put(skia_opts, :otp_app, infer_otp_app!(module))
    end
  end

  defp infer_otp_app!(module) when is_atom(module) do
    case Application.get_application(module) || infer_otp_app_from_module_root(module) do
      otp_app when is_atom(otp_app) ->
        otp_app

      nil ->
        raise ArgumentError,
              "mount/1 could not infer otp_app for #{inspect(module)}; pass otp_app: :my_app or emerge_skia: [otp_app: :my_app]"
    end
  end

  defp infer_otp_app_from_module_root(module) when is_atom(module) do
    module
    |> Module.split()
    |> List.first()
    |> case do
      nil ->
        nil

      root ->
        root = Macro.underscore(root)

        Enum.find_value(Application.loaded_applications(), fn {otp_app, _description, _version} ->
          if Atom.to_string(otp_app) == root, do: otp_app
        end)
    end
  end
end
