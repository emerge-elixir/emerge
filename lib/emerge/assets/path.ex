defmodule Emerge.Assets.Path do
  @moduledoc """
  Verified media path sigil.

  `~m"images/logo.png"` returns an `%Emerge.Assets.Ref{}` and validates that the
  referenced file exists in configured asset sources at compile time.
  """

  alias Emerge.Assets.Config
  alias Emerge.Assets.Ref

  defmacro __using__(_opts) do
    quote do
      import Emerge.Assets.Path, only: [sigil_m: 2]
    end
  end

  defmacro sigil_m({:<<>>, _meta, [path]}, modifiers) when is_binary(path) do
    validate_modifiers!(modifiers)
    resolved = resolve_source_file!(path, __CALLER__)

    if __CALLER__.module do
      Module.put_attribute(__CALLER__.module, :external_resource, resolved)
    end

    quote do
      %Ref{path: unquote(path), verified?: true}
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

  defp resolve_source_file!(path, caller) do
    config = Config.fetch()

    sources =
      config
      |> Map.get(:sources, [])
      |> Enum.map(&Path.expand(&1, caller_dir(caller)))

    found =
      Enum.find_value(sources, fn source_root ->
        candidate = Path.join(source_root, path)
        if File.regular?(candidate), do: candidate, else: nil
      end)

    if found do
      found
    else
      raise ArgumentError,
            "~m could not find #{inspect(path)} in configured asset sources: #{inspect(sources)}"
    end
  end

  defp caller_dir(caller) do
    caller.file
    |> Path.dirname()
    |> Path.expand()
  end
end
