defmodule EmergeSkia.BuildInfo do
  @moduledoc false
  alias EmergeSkia.BuildConfig.Schema
  alias EmergeSkia.BuildProfiles, as: Profiles

  def run(args, config \\ []) do
    {opts, rest, invalid} =
      OptionParser.parse(args,
        strict: [backend: :keep, target: :string, systems: :boolean, profiles: :boolean]
      )

    if rest != [] or invalid != [],
      do:
        raise(
          ArgumentError,
          "usage: build-info [--backend headless:opengl] [--target RUST_TARGET] [--systems | --profiles]"
        )

    cond do
      opts[:systems] -> IO.puts(system_table())
      opts[:profiles] -> IO.puts(profile_table())
      true -> IO.puts(report(System.get_env(), config, opts))
    end
  end

  def report(env, config \\ [], opts \\ []) do
    defaults = Profiles.defaults(env)
    cli = Keyword.get_values(opts, :backend)
    configured = Keyword.get(config, :compiled_backends, :unset)

    entries =
      cond do
        cli != [] -> Enum.map(cli, &parse_backend!/1)
        configured != :unset -> configured
        true -> defaults.matrix
      end

    legacy =
      Keyword.has_key?(config, :compiled_opengl_backends) or
        Keyword.has_key?(config, :compiled_vulkan_backends)

    entries =
      if cli == [] and configured == :unset and legacy, do: Keyword.keys(entries), else: entries

    matrix =
      Schema.resolve!(
        entries,
        if(cli == [], do: Keyword.get(config, :compiled_opengl_backends, :unset), else: :unset),
        if(cli == [], do: Keyword.get(config, :compiled_vulkan_backends, :unset), else: :unset)
      )

    features = Profiles.features(matrix)
    target = opts[:target] || Profiles.rust_target(env)
    explicit = cli != [] or configured != :unset or legacy
    system = defaults.system
    transport = if features == ["macos"], do: :macos_host, else: :nif

    platform =
      if transport == :macos_host, do: [], else: Profiles.platform_features(env, features)

    selection =
      case Profiles.select(target, features, transport) do
        {:ok, profile} ->
          "Artifact variant: #{profile.variant || "baseline (no suffix)"}\n" <>
            "Artifact Cargo features: #{inspect(profile.cargo_features)}"

        {:error, error} ->
          "No exact artifact: #{error.reason}; available feature sets: #{inspect(error.available)}"
      end

    """
    System: #{if system, do: "#{system.app} #{system.version} (#{system.board})", else: env["NERVES_SYSTEM"] || "not identified"}
    Profile: #{defaults.reason}#{if system, do: " — #{system.note}", else: ""}
    Rust target: #{target || "unresolved; pass --target or use the Nerves build environment"}
    Configuration: #{if explicit, do: "explicit override", else: "environment default"}
    compiled_backends: #{inspect(matrix)}
    Optional features: #{inspect(features)}
    Transport: #{transport}
    Cargo features: #{inspect(Profiles.cargo_features(features) ++ platform)}
    #{selection}
    No device probing was performed. Stock-system profiles assume the unmodified image.
    Override in config/config.exs with config :emerge, compiled_backends: [...].
    Rebuild: mix deps.compile emerge --force (then MIX_TARGET=your_target mix firmware).
    Source build: EMERGE_SKIA_BUILD=true mix deps.compile emerge --force (requires Rustler).
    Guide: https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration
    Source guide: lib/emerge.ex (viewport configuration documentation)
    """
  end

  def system_table do
    rows =
      Enum.map(Profiles.systems(), fn system ->
        caps = system.capabilities

        "| #{system.board} | `#{system.app}` #{system.version} | #{caps.atomic_kms} | #{caps.opengl} | #{caps.vulkan} | `#{inspect(Profiles.default_matrix(caps))}` | [Image configuration](#{system.evidence}) |"
      end)

    Enum.join(
      [
        "| Board | Stock system | Atomic KMS | GLES | Vulkan | Default | Evidence |",
        "| --- | --- | --- | --- | --- | --- | --- |" | rows
      ],
      "\n"
    )
  end

  def profile_table do
    rows =
      for target <- Profiles.targets(),
          transport <- [:nif, :macos_host],
          profile <- Profiles.profiles(target, transport) do
        "| `#{target}` | `#{transport}` | `#{inspect(profile.features)}` | `#{profile.variant || "baseline"}` |"
      end

    Enum.join(
      [
        "| Rust target | Transport | Optional features | Variant |",
        "| --- | --- | --- | --- |" | rows
      ],
      "\n"
    )
  end

  defp parse_backend!(entry) do
    case String.split(entry, ":", parts: 2) do
      [backend, apis] ->
        backend = existing_atom!(backend, ~w(headless drm wayland macos))

        apis =
          Enum.map(
            String.split(apis, ",", trim: true),
            &existing_atom!(&1, ~w(raster opengl vulkan metal))
          )

        {backend, apis}

      _ ->
        raise ArgumentError, "Expected --backend headless:opengl, got: #{entry}"
    end
  end

  defp existing_atom!(value, allowed) do
    if value not in allowed, do: raise(ArgumentError, "Unknown backend/API: #{value}")
    String.to_existing_atom(value)
  end
end
