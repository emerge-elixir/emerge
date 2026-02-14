defmodule Mix.Tasks.Emerge.Assets.Digest do
  use Mix.Task

  @shortdoc "Digests Emerge static image assets"

  @switches [output: :string, source: :keep]

  @impl true
  def run(args) do
    Mix.Task.run("app.start")

    {opts, positional, _invalid} =
      OptionParser.parse(args, switches: @switches, aliases: [o: :output])

    config = Emerge.Assets.Config.fetch()

    output =
      opts[:output] ||
        get_in(config, [:manifest, :path])
        |> Path.dirname()

    sources =
      case Keyword.get_values(opts, :source) ++ positional do
        [] -> Map.get(config, :sources, ["assets"])
        list -> list
      end

    case Emerge.Assets.Digester.compile(sources, output) do
      {:ok, count} ->
        Mix.shell().info([:green, "Digested #{count} assets into #{output}"])

      {:error, reason} ->
        Mix.raise("emerge.assets.digest failed: #{Exception.message(reason)}")
    end
  end
end
