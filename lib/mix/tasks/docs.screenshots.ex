defmodule Mix.Tasks.Docs.Screenshots do
  @moduledoc false
  use Mix.Task

  alias Emerge.Docs.Screenshots

  @shortdoc "Generates screenshots for UI examples in the docs"

  @impl true
  def run(args) do
    Mix.Task.run("app.start")

    {opts, positional, invalid} =
      OptionParser.parse(args,
        strict: [check: :boolean, only: :keep]
      )

    if positional != [] or invalid != [] do
      Mix.raise("mix docs.screenshots only supports --check and --only <id>")
    end

    only_ids =
      opts
      |> Keyword.get_values(:only)
      |> Enum.flat_map(&String.split(&1, ",", trim: true))
      |> MapSet.new()

    specs = filter_specs(Screenshots.specs(), only_ids)

    if specs == [] do
      Mix.raise("no screenshot specs matched the requested ids")
    end

    stale_paths =
      specs
      |> Enum.flat_map(fn spec ->
        png = Screenshots.render_png(spec)
        sync_destinations(png, spec.destinations, Keyword.get(opts, :check, false))
      end)

    if stale_paths != [] do
      message =
        ["docs screenshots are missing or out of date:"] ++ Enum.map(stale_paths, &"  #{&1}")

      Mix.raise(Enum.join(message, "\n"))
    end

    Mix.shell().info("generated #{length(specs)} documentation screenshot(s)")
  end

  defp filter_specs(specs, only_ids) do
    if MapSet.size(only_ids) == 0 do
      specs
    else
      Enum.filter(specs, &MapSet.member?(only_ids, &1.id))
    end
  end

  defp sync_destinations(png, destinations, check?) do
    Enum.flat_map(destinations, fn destination ->
      if check? do
        case File.read(destination) do
          {:ok, ^png} ->
            []

          _ ->
            [Path.relative_to_cwd(destination)]
        end
      else
        File.mkdir_p!(Path.dirname(destination))
        File.write!(destination, png)
        []
      end
    end)
  end
end
