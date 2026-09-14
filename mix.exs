# These helpers must load before dependencies or application modules compile.
for file <- ["native.exs", "package.exs", "docs.exs"] do
  Code.require_file("mix/" <> file, __DIR__)
end

defmodule Emerge.MixProject do
  use Mix.Project

  @version "0.4.0"
  @source_url "https://github.com/emerge-elixir/emerge"

  for file <- ~w(native.exs package.exs docs.exs targets.exs) do
    @external_resource Path.join([__DIR__, "mix", file])
  end

  def project do
    [
      app: :emerge,
      version: @version,
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      rustler_opts: Emerge.Mix.Native.options(__DIR__),
      package: Emerge.Mix.Package.config(__DIR__, @source_url),
      source_url: @source_url,
      deps: deps(),
      aliases: aliases(),
      dialyzer: [plt_add_apps: [:mix]],
      name: "Emerge",
      docs: Emerge.Mix.Docs.config(__DIR__, @version, @source_url)
    ]
  end

  def application do
    [
      extra_applications: [:logger]
    ]
  end

  def cli do
    [
      preferred_envs: preferred_cli_env()
    ]
  end

  defp deps do
    [
      {:rustler, "~> 0.38.0", optional: true},
      {:rustler_precompiled, "~> 0.8.4"},
      {:video_interop, "~> 0.1.1"},
      {:jason, "~> 1.4"},
      {:benchee, "~> 1.3", only: :dev, runtime: false},
      {:ex_doc, "~> 0.35", only: :dev, runtime: false},
      {:credo, "~> 1.7", only: [:dev, :test], runtime: false},
      {:dialyxir, "~> 1.4", only: [:dev], runtime: false}
    ]
  end

  defp aliases do
    bench_aliases() ++
      [
        docs: ["docs.screenshots", "docs"],
        quality: ["format --check-formatted", "credo --strict", "dialyzer"],
        "quality.fast": ["format --check-formatted", "credo --strict"]
      ]
  end

  defp bench_aliases do
    [
      bench: ["bench.fixtures", "bench.engine", "bench.native"],
      "bench.engine": ["bench.engine.diff", "bench.engine.serialization"],
      "bench.engine.diff": ["run bench/engine_diff_bench.exs"],
      "bench.engine.update_breakdown": ["run bench/engine_update_breakdown_bench.exs"],
      "bench.engine.update_compare": ["run bench/engine_update_compare_bench.exs"],
      "bench.engine.serialization": ["run bench/serialization_bench.exs"],
      "bench.fixtures": ["run bench/generate_fixtures.exs"],
      "bench.native": [
        "bench.native.layout",
        "bench.native.retained_layout",
        "bench.native.patch"
      ],
      "bench.native.layout": ["run bench/native_layout_bench.exs"],
      "bench.native.retained_layout": ["run bench/native_retained_layout_bench.exs"],
      "bench.native.patch": ["run bench/native_patch_bench.exs"]
    ]
  end

  defp preferred_cli_env do
    Enum.map(bench_aliases(), fn {name, _tasks} -> {name, :dev} end) ++
      [credo: :test, dialyzer: :dev, quality: :test, "quality.fast": :test]
  end
end
