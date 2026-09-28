defmodule Emerge.Mix.Package do
  @moduledoc false

  def config(root, source_url) do
    [
      description: "Write native GUI directly from Elixir using declarative API.",
      files: files(root),
      licenses: ["Apache-2.0"],
      links: %{"GitHub" => source_url}
    ]
  end

  defp files(root) do
    [
      "lib",
      "mix",
      "bench",
      "guides/tutorials",
      "guides/migrations",
      "guides/reference",
      "native/emerge_skia/Cargo.toml",
      "native/emerge_skia/Cargo.lock",
      "native/emerge_skia/Cross.toml",
      "LICENSE",
      "NOTICE",
      "THIRD_PARTY_ASSETS.md",
      "licenses",
      "priv/sample_assets",
      "priv/test_assets/gradient_mask.svg",
      "README.md",
      "CHANGELOG.md",
      "mix.exs",
      "mix.lock"
    ] ++
      Enum.flat_map(["src", "benches", "support"], fn dir ->
        regular_files(root, "native/emerge_skia/#{dir}/**/*")
      end) ++
      Enum.flat_map(["scripts/**/*.exs", "scripts/**/*.sh"], &regular_files(root, &1)) ++
      assets(root) ++ regular_files(root, "checksum-*.exs")
  end

  defp regular_files(root, pattern) do
    root
    |> Path.join(pattern)
    |> Path.wildcard()
    |> Enum.reject(&File.dir?/1)
    |> Enum.map(&Path.relative_to(&1, root))
  end

  defp assets(root), do: regular_files(root, "assets/*.png")
end
