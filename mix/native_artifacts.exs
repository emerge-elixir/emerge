defmodule Emerge.Mix.NativeArtifacts do
  @moduledoc false
  alias EmergeSkia.BuildProfiles

  def build!(name, version, opts \\ []) do
    builder =
      Enum.find(BuildProfiles.release_builders(), &(&1.name == name)) ||
        raise ArgumentError, "Unknown native builder: #{name}"

    Version.parse!(version)
    root = Path.expand(Keyword.get(opts, :root, Path.join(__DIR__, "..")))
    output = Path.expand(Keyword.get(opts, :output, Path.join(root, "dist/native")))

    context = %{
      root: root,
      crate: Path.join(root, "native/emerge_skia"),
      output: output,
      target_dir: Path.join(root, "native/emerge_skia/target"),
      container_dir: Path.expand(Path.join(output, "../native-build")),
      version: version,
      input_cache: Keyword.get(opts, :input_cache, System.get_env("EMERGE_INPUT_CACHE_DIR")),
      run: Keyword.get(opts, :run, &System.cmd/3)
    }

    File.mkdir_p!(output)
    jobs = jobs(builder)
    jobs |> Enum.uniq_by(& &1.target) |> Enum.each(&verify_nif_version!(&1, context))
    prepare_containers!(jobs, context)
    Enum.each(jobs, &build_nif!(&1, context))

    BuildProfiles.host_release_jobs()
    |> Enum.filter(&(&1.target in builder.targets))
    |> Enum.each(&build_host!(&1, context))

    archives = Path.wildcard(Path.join(output, "*.tar.gz"))

    unless length(archives) == builder.artifact_count,
      do: raise("Unexpected native archive set for #{name}")
  end

  def jobs(builder) do
    BuildProfiles.release_jobs()
    |> Enum.filter(&(&1.target in builder.targets))
    |> Enum.sort_by(fn job ->
      apis = String.split(job.cargo_features, ",") |> Enum.filter(&(&1 in ~w(opengl vulkan)))
      {job.container, not job.embedded_skia, apis, job.variant}
    end)
  end

  def archive_name(job, version) do
    suffix = if job.variant == "", do: "", else: "--#{job.variant}"
    "libemerge_skia-v#{version}-nif-2.15-#{job.target}#{suffix}.so.tar.gz"
  end

  defp prepare_containers!(jobs, context) do
    jobs
    |> Enum.map(& &1.container)
    |> Enum.reject(&(&1 == ""))
    |> Enum.uniq()
    |> Enum.each(fn container ->
      command!(context, "docker", [
        "build",
        "-t",
        "emerge-release-#{container}",
        "-f",
        "native/emerge_skia/support/release/Dockerfile.#{container}",
        "native/emerge_skia/support/release"
      ])
    end)
  end

  defp build_nif!(job, context) do
    IO.puts("::group::#{job.name}")

    try do
      args = ["--locked", "--target", job.target] ++ String.split(job.cargo_args)
      verify_dependencies!(job, context, args, build_env(job, context))

      if job.container == "",
        do: build_native!(job, context),
        else: build_container!(job, context)

      archive = Path.join(context.output, archive_name(job, context.version))
      unless File.regular?(archive), do: raise("Missing native archive: #{archive}")
    after
      IO.puts("::endgroup::")
    end
  end

  defp build_native!(job, context) do
    env = build_env(job, context)
    args = ["--locked", "--target", job.target] ++ String.split(job.cargo_args)
    tool = if job.use_cross, do: "cross", else: "cargo"
    command!(context, tool, ["build", "--release", "--lib" | args], cd: context.crate, env: env)

    extension = if job.macos_raster, do: "dylib", else: "so"

    library =
      Path.join([context.target_dir, job.target, "release", "libemerge_skia.#{extension}"])

    if job.qualify_embedded do
      command!(context, "elixir", [
        "scripts/check-embedded-artifact.exs",
        library,
        job.target,
        job.qualification_profile
      ])
    end

    if job.macos_raster do
      command!(context, "elixir", ["scripts/check-macos-raster-artifact.exs", library, job.target])
    end

    # Snapshot now: the next feature build writes the same Cargo library path.
    package!(context, library, archive_name(job, context.version))
  end

  defp build_env(job, context) do
    [
      {"CARGO_TARGET_DIR", context.target_dir},
      {"CARGO_PROFILE_RELEASE_STRIP", "symbols"},
      {"FORCE_SKIA_BUILD", if(job.embedded_skia or job.macos_raster, do: "1")},
      {"SKIA_GN_ARGS",
       if(job.embedded_skia,
         do: "skia_use_fontconfig=false skia_use_system_freetype2=false"
       )},
      {"RUSTFLAGS", if(job.embedded_skia, do: "-Lnative=support/embedded-linux-link-stubs")}
    ]
  end

  defp verify_nif_version!(job, context) do
    features =
      command!(
        context,
        "cargo",
        [
          "tree",
          "--locked",
          "--target",
          job.target,
          "--depth",
          "0",
          "--invert",
          "rustler",
          "--prefix",
          "none",
          "--format",
          "{f}"
        ] ++ String.split(job.cargo_args),
        cd: context.crate,
        capture: true
      )

    versions =
      Regex.scan(~r/nif_version_2_(\d+)/, features, capture: :all_but_first)
      |> Enum.map(fn [minor] -> String.to_integer(minor) end)

    unless Enum.max(versions, fn -> 0 end) == 15,
      do: raise("Rustler NIF version does not match the archive ABI 2.15")
  end

  defp verify_dependencies!(job, context, args, env) do
    if job.vulkan_only or job.cpu_drm do
      tree =
        command!(context, "cargo", ["tree" | args] ++ ["--edges", "normal,features"],
          cd: context.crate,
          env: env,
          capture: true
        )

      if job.vulkan_only and
           Regex.match?(
             ~r/\bgl v[0-9]|glutin|video-interop feature "egl"|skia-safe feature "gl"/,
             tree
           ),
         do: raise("Vulkan-only artifact enables an OpenGL dependency or feature")

      if job.cpu_drm and
           Regex.match?(
             ~r/\b(gbm|gbm-sys|gl|glutin|glutin_egl_sys|ash|ash-window|wayland-client) v/,
             tree
           ),
         do: raise("CPU DRM artifact enables a GPU/window-system dependency")
    end
  end

  defp build_container!(job, context) do
    cache = Path.join(context.container_dir, job.target)
    File.mkdir_p!(cache)
    input_cache = context.input_cache || raise("EMERGE_INPUT_CACHE_DIR is required")

    command!(context, "docker", [
      "run",
      "--rm",
      "--env",
      "EMERGE_SOURCE_REVISION",
      "--env",
      "CARGO_NET_RETRY",
      "--env",
      "EMERGE_CARGO_FEATURES=#{job.cargo_features}",
      "--env",
      "EMERGE_VERIFY_PROFILE=#{job.qualification_profile}",
      "--env",
      "EMERGE_INPUT_CACHE_DIR=/input-sources",
      "--env",
      "CARGO_HOME=/build/cargo",
      "--mount",
      "type=bind,source=#{input_cache},target=/input-sources",
      "--mount",
      "type=bind,source=#{context.root},target=/source,readonly",
      "--mount",
      "type=bind,source=#{cache},target=/build",
      "--mount",
      "type=bind,source=#{context.output},target=/artifacts",
      "emerge-release-#{job.container}",
      job.target,
      job.profile,
      context.version,
      "2.15"
    ])
  end

  defp build_host!(job, context) do
    env = [
      {"CARGO_TARGET_DIR", context.target_dir},
      {"CARGO_PROFILE_RELEASE_STRIP", "symbols"},
      {"FORCE_SKIA_BUILD", nil},
      {"SKIA_GN_ARGS", nil},
      {"RUSTFLAGS", nil}
    ]

    command!(
      context,
      "cargo",
      ["build", "--locked", "--release", "--target", job.target, "--bin", "macos_host"] ++
        String.split(job.cargo_args),
      cd: context.crate,
      env: env
    )

    name = "macos_host-v#{context.version}-#{job.target}.tar.gz"

    package!(
      context,
      Path.join([context.target_dir, job.target, "release", "macos_host"]),
      name,
      "macos_host"
    )

    archive = Path.join(context.output, name)
    checksum = :crypto.hash(:sha256, File.read!(archive)) |> Base.encode16(case: :lower)
    File.write!(archive <> ".sha256", "#{checksum}  #{name}\n")
  end

  defp package!(context, library, archive_name, entry \\ nil) do
    name = entry || String.replace_suffix(archive_name, ".tar.gz", "")
    staged = Path.join(context.output, name)
    File.cp!(library, staged)

    command!(context, "tar", [
      "-czf",
      Path.join(context.output, archive_name),
      "-C",
      context.output,
      name
    ])

    File.rm!(staged)
  end

  defp command!(context, program, args, opts \\ []) do
    {capture, opts} = Keyword.pop(opts, :capture, false)
    opts = Keyword.merge([cd: context.root, stderr_to_stdout: true], opts)
    opts = if capture, do: opts, else: Keyword.put(opts, :into, IO.stream(:stdio, :line))

    result =
      if program in ["cargo", "cross", "docker"],
        do: EmergeSkia.BuildRetry.cmd(program, args, opts, run: context.run),
        else: context.run.(program, args, opts)

    case result do
      {output, 0} -> output
      {_output, status} -> raise "#{program} #{Enum.join(args, " ")} failed (#{status})"
    end
  end
end
