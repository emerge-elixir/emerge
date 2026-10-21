defmodule EmergeSkia.BuildProfiles do
  @moduledoc false
  @catalog_path Path.expand("../../mix/native_profiles.exs", __DIR__)
  @targets_path Path.expand("../../mix/targets.exs", __DIR__)
  @external_resource @catalog_path
  @external_resource @targets_path
  @catalog elem(Code.eval_file(@catalog_path), 0)
  @compilers elem(Code.eval_file(@targets_path), 0)
  @feature_order @catalog.feature_order
  @gnu @catalog.gnu
  @embedded @catalog.embedded
  @darwin @catalog.darwin
  @systems @catalog.systems

  def targets, do: @gnu ++ @embedded ++ @darwin
  def host_targets, do: @darwin
  def systems, do: @systems

  def canonical_features(features) do
    invalid = Enum.reject(features, &(&1 in @feature_order))
    if invalid != [], do: raise(ArgumentError, "Unknown native features: #{inspect(invalid)}")
    Enum.filter(@feature_order, &(&1 in features))
  end

  def features(matrix) do
    matrix
    |> Enum.flat_map(fn {backend, apis} ->
      presenter = if backend in [:drm, :wayland, :macos], do: [Atom.to_string(backend)], else: []
      presenter ++ for(api <- [:opengl, :vulkan], api in apis, do: Atom.to_string(api))
    end)
    |> canonical_features()
  end

  def cargo_features(features) do
    features = canonical_features(features)

    features ++
      if("drm" in features and "vulkan" in features, do: ["drm-vulkan-deps"], else: []) ++
      if("wayland" in features and "vulkan" in features, do: ["wayland-vulkan-deps"], else: [])
  end

  def variant([]), do: nil

  def variant(features),
    do: features |> canonical_features() |> Enum.join("_") |> String.to_atom()

  def profiles(target) when target in @gnu do
    for presenters <- [[], ["drm"], ["wayland"], ["drm", "wayland"]],
        apis <- [[], ["opengl"], ["vulkan"], ["opengl", "vulkan"]],
        do: profile(target, presenters ++ apis)
  end

  def profiles(target) when target in @embedded do
    Enum.map([[], ["opengl"], ["drm"], ["drm", "opengl"]], &profile(target, &1))
  end

  def profiles(target) when target in @darwin, do: [profile(target, [])]
  def profiles(_target), do: []

  def profiles(target, :nif), do: profiles(target)

  def profiles(target, :macos_host) when target in @darwin do
    [
      %{
        target: target,
        transport: :macos_host,
        features: ["macos"],
        variant: :macos_host,
        cargo_features: ["macos"]
      }
    ]
  end

  def profiles(_target, :macos_host), do: []

  def validate_target!(target, features) do
    incompatible =
      cond do
        is_binary(target) and String.ends_with?(target, "-apple-darwin") ->
          Enum.reject(features, &(&1 == "macos"))

        is_binary(target) and String.contains?(target, "-linux-") ->
          Enum.filter(features, &(&1 == "macos"))

        true ->
          []
      end

    if incompatible != [] do
      raise ArgumentError,
            "Native features #{inspect(incompatible)} are not supported on #{target}; " <>
              "see lib/emerge.ex (viewport configuration documentation) (headless Metal is deferred)"
    end

    :ok
  end

  def select(target, features, transport \\ :nif) do
    features = canonical_features(features)
    candidates = profiles(target, transport)

    case Enum.find(candidates, &(&1.features == features)) do
      nil ->
        {:error,
         %{
           reason: if(target in targets(), do: :unsupported_features, else: :unsupported_target),
           target: target,
           features: features,
           transport: transport,
           available: Enum.map(candidates, & &1.features)
         }}

      profile ->
        {:ok, profile}
    end
  end

  def default_matrix(%{atomic_kms: true, vulkan: true}), do: [drm: [:vulkan]]
  def default_matrix(%{atomic_kms: true, opengl: true}), do: [drm: [:opengl]]
  def default_matrix(%{atomic_kms: true}), do: [drm: [:raster]]
  def default_matrix(%{opengl: true}), do: [headless: [:opengl]]
  def default_matrix(_capabilities), do: []

  def nerves?(env) do
    present?(env["NERVES_SYSTEM"]) or present?(env["NERVES_SDK_SYSROOT"]) or
      env["MIX_TARGET"] not in [nil, "", "host"] or compiler_target(env["CC"]) != nil
  end

  def system_profile(env) do
    name = env |> Map.get("NERVES_SYSTEM", "") |> Path.basename()

    Enum.find(@systems, fn system ->
      name == "#{system.app}-portable-#{system.version}"
    end)
  end

  def defaults(env) do
    cond do
      nerves?(env) ->
        case system_profile(env) do
          nil ->
            %{matrix: [], reason: :unknown_nerves_system, system: nil}

          system ->
            %{matrix: default_matrix(system.capabilities), reason: :known_system, system: system}
        end

      target_os(env) == "darwin" ->
        %{matrix: [macos: [:metal]], reason: :macos, system: nil}

      true ->
        %{matrix: [wayland: [:opengl]], reason: :linux_desktop, system: nil}
    end
  end

  def rust_target(env) do
    compiler_target(env["CC"]) || env_target(env)
  end

  def platform_features(env, features) do
    if nerves?(env) or target_os(env) == "darwin" or "wayland" not in features do
      if features == [], do: ["embedded-cpu"], else: ["embedded-freetype"]
    else
      []
    end
  end

  def release_jobs do
    for target <- targets(), profile <- profiles(target), do: release_job(profile)
  end

  def release_builders do
    release_jobs()
    |> Enum.group_by(&builder_name(&1.target))
    |> Enum.sort_by(&elem(&1, 0))
    |> Enum.map(fn {name, jobs} ->
      targets = jobs |> Enum.map(& &1.target) |> Enum.uniq()

      %{
        name: name,
        os: hd(jobs).os,
        targets: targets,
        use_cross: Enum.any?(jobs, & &1.use_cross),
        macos: Enum.any?(targets, &(&1 in @darwin)),
        artifact_count: length(jobs) + Enum.count(host_targets(), &(&1 in targets)),
        system_packages:
          jobs
          |> Enum.flat_map(&String.split(&1.system_packages))
          |> Enum.uniq()
          |> Enum.sort()
          |> Enum.join(" ")
      }
    end)
  end

  defp builder_name(target) do
    arch = target |> String.split("-") |> hd()
    platform = if target in @darwin, do: "macos", else: "linux"
    "#{platform}_#{arch}"
  end

  def host_release_jobs do
    for target <- host_targets(), profile <- profiles(target, :macos_host) do
      %{
        name: target,
        target: target,
        os: if(target == "aarch64-apple-darwin", do: "macos-15", else: "macos-15-intel"),
        cargo_args: "--no-default-features --features #{Enum.join(profile.cargo_features, ",")}"
      }
    end
  end

  defp profile(target, features) do
    features = canonical_features(features)

    %{
      target: target,
      transport: :nif,
      features: features,
      variant: variant(features),
      cargo_features:
        cargo_features(features) ++
          [if(features == [], do: "embedded-cpu", else: "embedded-freetype")],
      embedded: "wayland" not in features,
      qualification: qualification(features)
    }
  end

  defp qualification([]), do: "raster"
  defp qualification(["drm"]), do: "drm_raster"
  defp qualification(["vulkan"]), do: "vulkan"
  defp qualification(["drm", "vulkan"]), do: "drm_vulkan"
  defp qualification(["drm", "opengl", "vulkan"]), do: "drm_all"
  defp qualification(_features), do: "opengl"

  defp release_job(profile) do
    target = profile.target

    container =
      cond do
        target == "x86_64-unknown-linux-musl" -> "musl"
        target == "riscv64gc-unknown-linux-gnu" -> "riscv64"
        true -> ""
      end

    os =
      cond do
        target == "aarch64-apple-darwin" -> "macos-15"
        target == "x86_64-apple-darwin" -> "macos-15-intel"
        target == "aarch64-unknown-linux-gnu" -> "ubuntu-24.04-arm"
        true -> "ubuntu-24.04"
      end

    name = if profile.variant, do: Atom.to_string(profile.variant), else: "raster"

    %{
      name: "#{target}_#{name}",
      target: target,
      os: os,
      use_cross: target == "armv7-unknown-linux-gnueabihf",
      container: container,
      profile: name,
      variant: if(profile.variant, do: Atom.to_string(profile.variant), else: ""),
      cargo_features: Enum.join(profile.cargo_features, ","),
      cargo_args: "--no-default-features --features #{Enum.join(profile.cargo_features, ",")}",
      system_packages:
        if(container == "" and target not in @darwin,
          do: system_packages(profile.features),
          else: ""
        ),
      embedded_skia: profile.embedded and target not in @darwin,
      raster_only: profile.features == [],
      macos_raster: target in @darwin,
      qualification_profile: profile.qualification,
      vulkan_only: "vulkan" in profile.features and "opengl" not in profile.features,
      cpu_drm: profile.features == ["drm"],
      qualify_embedded: profile.embedded and target in (@gnu ++ ["armv7-unknown-linux-gnueabihf"])
    }
  end

  defp system_packages(features) do
    desktop =
      if "wayland" in features,
        do: ~w(libwayland-dev libxkbcommon-dev libfontconfig1-dev libfreetype6-dev),
        else: []

    graphics =
      cond do
        "opengl" in features -> ~w(libegl1-mesa-dev libgbm-dev libdrm-dev)
        "drm" in features and "vulkan" in features -> ~w(libgbm-dev libdrm-dev)
        true -> []
      end

    Enum.join(desktop ++ graphics, " ")
  end

  defp compiler_target(nil), do: nil

  defp compiler_target(command) do
    prefix =
      command
      |> String.split()
      |> List.first("")
      |> Path.basename()
      |> String.replace(~r/-[^-]+$/, "")

    @compilers[prefix]
  end

  defp env_target(env) do
    case {env["TARGET_ARCH"], env["TARGET_OS"]} do
      {arch, "darwin"} when arch in ["arm64", "aarch64", "arm"] ->
        "aarch64-apple-darwin"

      {"x86_64", "darwin"} ->
        "x86_64-apple-darwin"

      {arch, "linux"} when is_binary(arch) ->
        arch = if arch == "riscv64", do: "riscv64gc", else: arch
        "#{arch}-unknown-linux-#{env["TARGET_ABI"] || "gnu"}"

      _ ->
        if nerves?(env), do: nil, else: host_target()
    end
  end

  defp host_target do
    [arch | parts] = :erlang.system_info(:system_architecture) |> to_string() |> String.split("-")

    case :os.type() do
      {:unix, :darwin} ->
        arch = if arch in ["arm", "arm64"], do: "aarch64", else: arch
        "#{arch}-apple-darwin"

      _ ->
        abi =
          case Enum.drop_while(parts, &(&1 != "linux")) do
            ["linux", abi | _] -> abi
            _ -> "gnu"
          end

        "#{arch}-unknown-linux-#{abi}"
    end
  end

  defp target_os(env),
    do: env["TARGET_OS"] || if(:os.type() == {:unix, :darwin}, do: "darwin", else: "linux")

  defp present?(value), do: value not in [nil, ""]
end
