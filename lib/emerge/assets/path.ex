defmodule Emerge.Assets.Path do
  @moduledoc """
  Verified media path sigil.

  `~m"images/logo.png"` returns an `%Emerge.Assets.Ref{}` and validates that the
  referenced file exists in the `priv` directory of the configured OTP app at compile time.
  """

  alias Emerge.Assets.Ref

  defmacro __using__(opts) do
    otp_app =
      case Keyword.fetch(opts, :otp_app) do
        {:ok, value} when is_atom(value) ->
          value

        {:ok, value} ->
          raise ArgumentError,
                "Emerge.Assets.Path expects otp_app: <atom>, got: #{inspect(value)}"

        :error ->
          raise ArgumentError,
                "Emerge.Assets.Path requires otp_app: <atom>, for example: use Emerge.Assets.Path, otp_app: :my_app"
      end

    quote do
      @emerge_assets_otp_app unquote(otp_app)
      import Emerge.Assets.Path, only: [sigil_m: 2]
    end
  end

  defmacro sigil_m({:<<>>, _meta, [path]}, modifiers) when is_binary(path) do
    validate_modifiers!(modifiers)
    normalized_path = normalize_logical_path!(path)
    otp_app = otp_app_for_caller!(__CALLER__)
    resolved = resolve_source_file!(normalized_path, otp_app, __CALLER__)

    if __CALLER__.module do
      Module.put_attribute(__CALLER__.module, :external_resource, resolved)
    end

    quote do
      %Ref{path: unquote(normalized_path), verified?: true}
    end
  end

  defmacro sigil_m(ast, _modifiers) do
    raise ArgumentError,
          "~m expects a literal string path, got: #{Macro.to_string(ast)}"
  end

  defp validate_modifiers!([]), do: :ok

  defp validate_modifiers!(mods) do
    raise ArgumentError, "~m does not support modifiers, got: #{inspect(mods)}"
  end

  defp resolve_source_file!(path, otp_app, caller) do
    priv_dir = otp_app_priv_dir!(otp_app, caller)
    candidate = Path.join(priv_dir, path)

    if File.regular?(candidate) do
      candidate
    else
      raise ArgumentError,
            "~m could not find #{inspect(path)} under #{inspect(priv_dir)} for otp_app #{inspect(otp_app)}"
    end
  end

  defp normalize_logical_path!(path) do
    normalized =
      path
      |> String.trim()
      |> String.trim_leading("/")

    if normalized == "" do
      raise ArgumentError, "~m path must not be empty"
    end

    if Enum.any?(Path.split(normalized), &(&1 == "..")) do
      raise ArgumentError, "~m path must be relative and may not contain '..': #{inspect(path)}"
    end

    normalized
  end

  defp caller_dir(caller) do
    caller.file
    |> Path.dirname()
    |> Path.expand()
  end

  defp otp_app_for_caller!(caller) do
    module =
      case caller.module do
        nil ->
          raise ArgumentError,
                "~m must be used inside a module that calls use Emerge.Assets.Path, otp_app: ..."

        module ->
          module
      end

    case Module.get_attribute(module, :emerge_assets_otp_app) do
      app when is_atom(app) ->
        app

      _ ->
        raise ArgumentError,
              "~m requires use Emerge.Assets.Path, otp_app: :my_app in #{inspect(module)}"
    end
  end

  defp otp_app_priv_dir!(otp_app, caller) do
    case :code.priv_dir(otp_app) do
      path when is_list(path) ->
        List.to_string(path)

      _ ->
        caller_dir(caller)
        |> nearest_project_root!()
        |> Path.join("priv")
    end
  end

  defp nearest_project_root!(start_dir) do
    start_dir
    |> Path.expand()
    |> Stream.unfold(fn
      nil -> nil
      dir -> {dir, parent_dir(dir)}
    end)
    |> Enum.find(fn dir -> File.exists?(Path.join(dir, "mix.exs")) end)
    |> case do
      nil -> raise ArgumentError, "could not locate project root for ~m compile-time verification"
      root -> root
    end
  end

  defp parent_dir(dir) do
    parent = Path.dirname(dir)
    if parent == dir, do: nil, else: parent
  end
end
