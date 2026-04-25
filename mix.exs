defmodule Emerge.MixProject do
  use Mix.Project

  @version "0.2.1"
  @source_url "https://github.com/emerge-elixir/emerge"

  @nerves_rust_target_triple_mapping %{
    "armv6-nerves-linux-gnueabihf" => "arm-unknown-linux-gnueabihf",
    "armv7-nerves-linux-gnueabihf" => "armv7-unknown-linux-gnueabihf",
    "aarch64-nerves-linux-gnu" => "aarch64-unknown-linux-gnu",
    "x86_64-nerves-linux-musl" => "x86_64-unknown-linux-musl"
  }

  @rustler_passthrough_env_keys [
    "CC",
    "CXX",
    "CFLAGS",
    "CLANGCC",
    "CLANGCXX",
    "CPPFLAGS",
    "CXXFLAGS",
    "LDFLAGS",
    "NERVES_SDK_SYSROOT",
    "NERVES_TOOLCHAIN",
    "PKG_CONFIG_SYSROOT_DIR",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_PATH",
    "TARGET_ARCH",
    "TARGET_OS",
    "TARGET_ABI",
    "TARGET_VENDOR"
  ]

  def project do
    [
      app: :emerge,
      version: @version,
      elixir: "~> 1.19",
      start_permanent: Mix.env() == :prod,
      rustler_opts: rustler_opts(),
      package: package(),
      source_url: @source_url,
      deps: deps(),
      aliases: aliases(),
      dialyzer: [plt_add_apps: [:mix]],
      name: "Emerge",
      docs: docs_config()
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
      {:rustler, "~> 0.37.0", optional: true},
      {:rustler_precompiled, "~> 0.8.4"},
      {:jason, "~> 1.4"},
      {:benchee, "~> 1.3", only: :dev, runtime: false},
      {:ex_doc, "~> 0.35", only: :dev, runtime: false},
      {:credo, "~> 1.7", only: [:dev, :test], runtime: false},
      {:dialyxir, "~> 1.4", only: [:dev], runtime: false}
    ]
  end

  defp aliases do
    [
      bench: ["bench.fixtures", "bench.engine", "bench.native"],
      "bench.engine": ["bench.engine.diff", "bench.engine.serialization"],
      "bench.engine.diff": ["run bench/engine_diff_bench.exs"],
      "bench.engine.serialization": ["run bench/serialization_bench.exs"],
      "bench.fixtures": ["run bench/generate_fixtures.exs"],
      "bench.native": [
        "bench.native.layout",
        "bench.native.retained_layout",
        "bench.native.patch"
      ],
      "bench.native.layout": ["run bench/native_layout_bench.exs"],
      "bench.native.retained_layout": ["run bench/native_retained_layout_bench.exs"],
      "bench.native.patch": ["run bench/native_patch_bench.exs"],
      docs: ["docs.screenshots", "docs"],
      quality: ["format --check-formatted", "credo --strict", "dialyzer"],
      "quality.fast": ["format --check-formatted", "credo --strict"]
    ]
  end

  defp preferred_cli_env do
    [
      bench: :dev,
      "bench.engine": :dev,
      "bench.engine.diff": :dev,
      "bench.engine.serialization": :dev,
      "bench.fixtures": :dev,
      "bench.native": :dev,
      "bench.native.layout": :dev,
      "bench.native.retained_layout": :dev,
      "bench.native.patch": :dev,
      credo: :test,
      dialyzer: :dev,
      quality: :test,
      "quality.fast": :test
    ]
  end

  defp package do
    [
      description: "Write native GUI directly from Elixir using declarative API.",
      files: package_files(),
      licenses: ["Apache-2.0"],
      links: %{
        "GitHub" => @source_url
      }
    ]
  end

  defp package_files do
    [
      "lib",
      "guides/tutorials",
      "native/emerge_skia/src",
      "native/emerge_skia/Cargo.toml",
      "native/emerge_skia/Cargo.lock",
      "native/emerge_skia/Cross.toml",
      "LICENSE",
      "NOTICE",
      "THIRD_PARTY_ASSETS.md",
      "licenses",
      "README.md",
      "CHANGELOG.md",
      "mix.exs",
      "mix.lock"
    ] ++ package_assets() ++ Path.wildcard("checksum-*.exs")
  end

  defp package_assets do
    [
      "assets/counter-basic.png",
      "assets/dashboard-functions.png",
      "assets/assets-image-and-background.png"
    ]
    |> Kernel.++(Path.wildcard("assets/ui-*.png"))
    |> Enum.uniq()
  end

  defp docs_config do
    [
      main: "readme",
      source_url: @source_url,
      source_ref: "v#{@version}",
      assets: %{
        "assets" => "assets",
        "guides/tutorials/assets" => "assets"
      },
      before_closing_body_tag: &before_closing_body_tag/1,
      extras: docs_extras(),
      groups_for_extras: docs_groups_for_extras(),
      groups_for_modules: [
        "Public API": [Emerge],
        UI: ~r/^Emerge\.UI(\.|$)/,
        Assets: ~r/^Emerge\.Assets(\.|$)/,
        Runtime: ~r/^Emerge\.Runtime\./,
        Rendering: ~r/^EmergeSkia(\.|$)/,
        Engine: ~r/^Emerge\.Engine(\.|$)/
      ]
    ]
  end

  defp docs_extras do
    public_docs_extras() ++ internal_docs_extras()
  end

  defp internal_docs_extras do
    if include_internal_docs?() do
      optional_internal_docs_extras()
    else
      []
    end
  end

  defp public_docs_extras do
    [
      "README.md",
      "guides/tutorials/set_up_viewport.md",
      "guides/tutorials/describe_ui.md",
      "guides/tutorials/use_assets.md",
      "guides/tutorials/state_management.md"
    ]
    |> Enum.filter(&File.exists?/1)
  end

  defp optional_internal_docs_extras do
    [
      "guides/internals/architecture.md",
      "guides/internals/assets-images.md",
      "guides/internals/macos-backend.md",
      "guides/internals/feature-roadmap.md",
      "guides/internals/emrg-format.md",
      "guides/internals/events.md",
      "guides/internals/tree-patching.md"
    ]
    |> Enum.filter(&File.exists?/1)
  end

  defp docs_groups_for_extras do
    [Tutorials: ~r/guides\/tutorials\/.*/] ++
      if internal_docs_extras() == [] do
        []
      else
        [Internals: ~r/guides\/internals\/.*/]
      end
  end

  defp include_internal_docs? do
    System.get_env("EMERGE_INCLUDE_INTERNAL_DOCS", "true") not in ["0", "false"]
  end

  defp rustler_opts do
    env = System.get_env()

    case rustler_target(env) do
      nil ->
        []

      target ->
        [
          target: target,
          env: rustler_cross_env(target, env)
        ]
    end
  end

  defp rustler_target(env) do
    rustler_target_from_cc(env) || rustler_target_from_target_env(env)
  end

  defp rustler_target_from_cc(env) do
    env
    |> Map.get("CC")
    |> compiler_prefix()
    |> then(&Map.get(@nerves_rust_target_triple_mapping, &1))
  end

  defp rustler_target_from_target_env(env) do
    with arch when is_binary(arch) and arch != "" <- Map.get(env, "TARGET_ARCH"),
         os when is_binary(os) and os != "" <- Map.get(env, "TARGET_OS"),
         abi when is_binary(abi) and abi != "" <- target_abi(env, os),
         vendor when is_binary(vendor) and vendor != "" <- target_vendor(env, os) do
      "#{arch}-#{vendor}-#{os}-#{abi}"
    else
      _ -> nil
    end
  end

  defp compiler_prefix(nil), do: nil

  defp compiler_prefix(compiler) do
    compiler
    |> String.split(~r/\s+/, trim: true)
    |> List.first()
    |> case do
      nil ->
        nil

      path ->
        path
        |> Path.basename()
        |> String.split("-")
        |> Enum.drop(-1)
        |> Enum.join("-")
    end
  end

  defp target_vendor(env, "linux") do
    case Map.get(env, "TARGET_VENDOR") do
      nil -> "unknown"
      "" -> "unknown"
      vendor -> vendor
    end
  end

  defp target_vendor(env, _os), do: Map.get(env, "TARGET_VENDOR")

  defp target_abi(env, "linux") do
    case Map.get(env, "TARGET_ABI") do
      nil -> "gnu"
      "" -> "gnu"
      abi -> abi
    end
  end

  defp target_abi(env, _os), do: Map.get(env, "TARGET_ABI")

  defp rustler_cross_env(target, env) do
    target_key =
      target
      |> String.upcase()
      |> String.replace("-", "_")

    effective_env = effective_rustler_env(env)

    effective_env
    |> passthrough_env()
    |> maybe_put_env(
      "SDKTARGETSYSROOT",
      Map.get(effective_env, "SDKTARGETSYSROOT") || Map.get(effective_env, "NERVES_SDK_SYSROOT")
    )
    |> maybe_put_env("CARGO_TARGET_#{target_key}_LINKER", Map.get(env, "CC"))
    |> maybe_put_env("HOST_CC", Map.get(env, "HOST_CC") || System.find_executable("cc"))
    |> maybe_put_env("HOST_CXX", Map.get(env, "HOST_CXX") || System.find_executable("c++"))
  end

  defp effective_rustler_env(env) do
    if nerves_build_env?(env) do
      env
      |> Map.put_new("SDKTARGETSYSROOT", Map.get(env, "NERVES_SDK_SYSROOT"))
      |> maybe_put_map_value("CC", skia_clang_command(env, "clang"))
      |> maybe_put_map_value("CXX", skia_clang_command(env, "clang++"))
      |> maybe_put_map_value("CLANGCC", skia_clang_command(env, "clang"))
      |> maybe_put_map_value("CLANGCXX", skia_clang_command(env, "clang++"))
    else
      env
    end
  end

  defp skia_clang_command(env, clang_binary) do
    if nerves_build_env?(env) do
      with sysroot when is_binary(sysroot) and sysroot != "" <- Map.get(env, "NERVES_SDK_SYSROOT"),
           clang when is_binary(clang) <- System.find_executable(clang_binary) do
        [clang | skia_clang_flags(env, sysroot)] |> Enum.join(" ")
      else
        _ -> nil
      end
    end
  end

  defp skia_clang_flags(env, sysroot) do
    [
      "--sysroot=#{sysroot}",
      gcc_toolchain_flag(env)
    ] ++ nerves_cxx_include_flags(env)
  end

  defp gcc_toolchain_flag(env) do
    env
    |> Map.get("CC")
    |> compiler_executable_path()
    |> case do
      nil -> nil
      compiler_path -> "--gcc-toolchain=#{compiler_path |> Path.dirname() |> Path.dirname()}"
    end
  end

  defp nerves_cxx_include_flags(env) do
    with toolchain when is_binary(toolchain) and toolchain != "" <-
           Map.get(env, "NERVES_TOOLCHAIN"),
         prefix when is_binary(prefix) and prefix != "" <- Map.get(env, "CC") |> compiler_prefix(),
         version when is_binary(version) and version != "" <-
           nerves_gxx_version(toolchain, prefix) do
      [
        Path.join([toolchain, prefix, "include", "c++", version]),
        Path.join([toolchain, prefix, "include", "c++", version, prefix]),
        Path.join([toolchain, "lib", "gcc", prefix, version, "include"]),
        Path.join([toolchain, "lib", "gcc", prefix, version, "include-fixed"])
      ]
      |> Enum.filter(&File.exists?/1)
      |> Enum.map(&"-I#{&1}")
    else
      _ -> []
    end
  end

  defp nerves_gxx_version(toolchain, prefix) do
    Path.join([toolchain, prefix, "include", "c++", "*"])
    |> Path.wildcard()
    |> Enum.sort()
    |> List.last()
    |> case do
      nil -> nil
      version_dir -> Path.basename(version_dir)
    end
  end

  defp compiler_executable_path(nil), do: nil

  defp compiler_executable_path(compiler) do
    compiler
    |> String.split(~r/\s+/, trim: true)
    |> List.first()
    |> case do
      nil ->
        nil

      executable ->
        if Path.type(executable) == :absolute,
          do: executable,
          else: System.find_executable(executable)
    end
  end

  defp passthrough_env(env) do
    Enum.reduce(@rustler_passthrough_env_keys, [], fn key, acc ->
      maybe_put_env(acc, key, Map.get(env, key))
    end)
  end

  defp nerves_build_env?(env) do
    value_present?(Map.get(env, "NERVES_SDK_SYSROOT")) ||
      mix_target?(env) ||
      nerves_compiler?(Map.get(env, "CC")) ||
      target_env?(env)
  end

  defp mix_target?(env) do
    case Map.get(env, "MIX_TARGET") do
      target when is_binary(target) and target not in ["", "host"] -> true
      _ -> false
    end
  end

  defp nerves_compiler?(compiler) do
    compiler
    |> compiler_prefix()
    |> then(&(&1 in Map.keys(@nerves_rust_target_triple_mapping)))
  end

  defp target_env?(env) do
    case {Map.get(env, "TARGET_ARCH"), Map.get(env, "TARGET_OS")} do
      {arch, os} when is_binary(arch) and arch != "" and is_binary(os) and os != "" -> true
      _ -> false
    end
  end

  defp value_present?(value) when is_binary(value), do: value != ""
  defp value_present?(_value), do: false

  defp maybe_put_map_value(map, _key, nil), do: map
  defp maybe_put_map_value(map, _key, ""), do: map
  defp maybe_put_map_value(map, key, value), do: Map.put(map, key, value)

  defp maybe_put_env(env, _key, nil), do: env
  defp maybe_put_env(env, _key, ""), do: env
  defp maybe_put_env(env, key, value), do: [{key, value} | env]

  defp before_closing_body_tag(:html) do
    """
    <script type="module">
      import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";

      mermaid.initialize({ startOnLoad: false });

      async function render() {
        const blocks = document.querySelectorAll(
          "pre > code.language-mermaid, pre > code.mermaid, pre > code[class*='mermaid']"
        );

        for (const code of blocks) {
          const pre = code.parentElement;
          if (!pre || pre.tagName !== "PRE") continue;

          const container = document.createElement("div");
          container.className = "mermaid";
          container.textContent = code.textContent;

          pre.replaceWith(container);
        }

        await mermaid.run({ querySelector: ".mermaid" });
      }

      if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", () => {
          render().catch(console.error);
        });
      } else {
        render().catch(console.error);
      }
    </script>
    """
  end

  defp before_closing_body_tag(_), do: ""
end
