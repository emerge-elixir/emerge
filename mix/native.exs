defmodule Emerge.Mix.Native do
  @moduledoc false

  @targets_path Path.join(__DIR__, "targets.exs")
  @external_resource @targets_path
  @nerves_rust_target_triple_mapping elem(Code.eval_file(@targets_path), 0)

  @rustler_passthrough_env_keys [
    "CC",
    "CXX",
    "CFLAGS",
    "BINDGEN_EXTRA_CLANG_ARGS",
    "CLANGCC",
    "CLANGCXX",
    "CPPFLAGS",
    "CXXFLAGS",
    "LDFLAGS",
    "RUSTFLAGS",
    "SKIA_GN_ARGS",
    "CARGO_PROFILE_RELEASE_STRIP",
    "EMERGE_SOURCE_REVISION",
    "EMERGE_SKIA_HOST_PYTHON",
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

  # This adapter intentionally prepares SDK host tools during project evaluation,
  # including precompiled builds, just as the original project configuration did.
  def options(root, env \\ System.get_env()) do
    case rustler_target(env) do
      nil ->
        []

      target ->
        [
          target: target,
          env: rustler_cross_env(target, env, root)
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
      arch = if arch == "riscv64", do: "riscv64gc", else: arch
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

  defp rustler_cross_env(target, env, root) do
    target_key =
      target
      |> String.upcase()
      |> String.replace("-", "_")

    effective_env = effective_rustler_env(env, root)

    # Rust's musl target defaults to a static CRT, which cannot produce a NIF cdylib.
    effective_env =
      if String.ends_with?(target, "-musl"),
        do: append_env(effective_env, "RUSTFLAGS", "-Ctarget-feature=-crt-static"),
        else: effective_env

    effective_env
    |> passthrough_env()
    |> maybe_put_env(
      "SDKTARGETSYSROOT",
      Map.get(effective_env, "SDKTARGETSYSROOT") || Map.get(effective_env, "NERVES_SDK_SYSROOT")
    )
    |> maybe_put_env("CARGO_TARGET_#{target_key}_LINKER", Map.get(env, "CC"))
    |> maybe_put_env("HOST_CC", Map.get(env, "HOST_CC") || System.find_executable("cc"))
    |> maybe_put_env("HOST_CXX", Map.get(env, "HOST_CXX") || System.find_executable("c++"))
    |> maybe_put_env("PATH", rustler_path(effective_env, env, root))
  end

  defp rustler_path(effective_env, source_env, root) do
    path = Map.get(effective_env, "PATH")

    if nerves_sdk_env?(source_env) do
      host_tools = Path.expand("native/emerge_skia/target/nerves-host-tools", root)
      host_python = Map.get(source_env, "EMERGE_SKIA_HOST_PYTHON", "/usr/bin/python3")

      unless Path.type(host_python) == :absolute and File.regular?(host_python) do
        raise "Nerves rust-skia builds require host Python at an absolute path; " <>
                "set EMERGE_SKIA_HOST_PYTHON, got: #{inspect(host_python)}"
      end

      wrapper =
        "#!/bin/sh\nunset PYTHONHOME PYTHONPATH LD_LIBRARY_PATH\nexec #{shell_quote(host_python)} \"$@\"\n"

      File.mkdir_p!(host_tools)

      Enum.each(["python", "python3"], fn name ->
        destination = Path.join(host_tools, name)
        File.write!(destination, wrapper)
        File.chmod!(destination, 0o755)
      end)

      Enum.join([host_tools, path], ":")
    else
      path
    end
  end

  defp shell_quote(value), do: "'#{String.replace(value, "'", "'\\\"'\\\"'")}'"

  defp effective_rustler_env(env, root) do
    if nerves_build_env?(env) do
      env
      |> Map.put_new("SDKTARGETSYSROOT", Map.get(env, "NERVES_SDK_SYSROOT"))
      |> configure_nerves_clang_flags(env)
      |> configure_nerves_skia(root)
      |> maybe_put_map_value("CC", skia_clang_command(env, "clang"))
      |> maybe_put_map_value("CXX", skia_clang_command(env, "clang++"))
      |> maybe_put_map_value("CLANGCC", skia_clang_command(env, "clang"))
      |> maybe_put_map_value("CLANGCXX", skia_clang_command(env, "clang++"))
    else
      env
    end
  end

  defp configure_nerves_skia(env, root) do
    link_stubs = Path.expand("native/emerge_skia/support/embedded-linux-link-stubs", root)

    env
    |> append_env("SKIA_GN_ARGS", "skia_use_fontconfig=false skia_use_system_freetype2=false")
    |> append_env("RUSTFLAGS", "-Lnative=#{link_stubs}")
    |> Map.put_new("CARGO_PROFILE_RELEASE_STRIP", "symbols")
  end

  defp append_env(env, key, value) do
    Map.update(env, key, value, fn
      "" -> value
      existing -> existing <> " " <> value
    end)
  end

  defp configure_nerves_clang_flags(effective_env, source_env) do
    if nerves_sdk_env?(source_env) do
      effective_env
      |> sanitize_clang_cross_flags("CFLAGS")
      |> sanitize_clang_cross_flags("CXXFLAGS")
      |> append_clang_cxx_flags(source_env)
    else
      effective_env
    end
  end

  defp sanitize_clang_cross_flags(env, key) do
    case Map.get(env, key) do
      flags when is_binary(flags) ->
        sanitized =
          flags
          |> String.split(~r/\s+/, trim: true)
          |> Enum.reject(&(&1 == "-mabi=lp64"))
          |> Enum.join(" ")

        Map.put(env, key, sanitized)

      _other ->
        env
    end
  end

  defp append_clang_cxx_flags(effective_env, source_env) do
    flags =
      [Map.get(effective_env, "CXXFLAGS") | nerves_cxx_include_flags(source_env)]
      |> List.flatten()
      |> Enum.reject(&(&1 in [nil, ""]))
      |> Kernel.++(["-Wno-invalid-constexpr"])
      |> Enum.join(" ")

    bindgen_flags =
      [Map.get(effective_env, "BINDGEN_EXTRA_CLANG_ARGS"), flags]
      |> Enum.reject(&(&1 in [nil, ""]))
      |> Enum.join(" ")

    effective_env
    |> Map.put("CXXFLAGS", flags)
    |> Map.put("BINDGEN_EXTRA_CLANG_ARGS", bindgen_flags)
  end

  defp skia_clang_command(env, clang_binary) do
    if nerves_build_env?(env) do
      with sysroot when is_binary(sysroot) and sysroot != "" <- Map.get(env, "NERVES_SDK_SYSROOT"),
           clang when is_binary(clang) <- System.find_executable(clang_binary) do
        flags =
          skia_clang_flags(env, sysroot) ++
            if(clang_binary == "clang++" and nerves_sdk_env?(env),
              do: ["-Wno-invalid-constexpr"],
              else: []
            )

        [clang | flags] |> Enum.join(" ")
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
    packaged_toolchain =
      case Map.get(env, "NERVES_TOOLCHAIN") do
        toolchain when is_binary(toolchain) and toolchain != "" ->
          [toolchain, Path.join(toolchain, "opt/ext-toolchain")]
          |> Enum.find(&File.dir?(Path.join(&1, "lib/gcc")))

        _other ->
          nil
      end

    case packaged_toolchain || compiler_toolchain_root(Map.get(env, "CC")) do
      nil -> nil
      toolchain -> "--gcc-toolchain=#{toolchain}"
    end
  end

  defp compiler_toolchain_root(compiler) do
    case compiler_executable_path(compiler) do
      nil -> nil
      compiler_path -> compiler_path |> Path.dirname() |> Path.dirname()
    end
  end

  defp nerves_cxx_include_flags(env) do
    with toolchain when is_binary(toolchain) and toolchain != "" <-
           Map.get(env, "NERVES_TOOLCHAIN"),
         prefix when is_binary(prefix) and prefix != "" <- Map.get(env, "CC") |> compiler_prefix(),
         cxx_root when is_binary(cxx_root) <- nerves_cxx_root(toolchain, prefix) do
      [cxx_root, Path.join(cxx_root, prefix)]
      |> Enum.filter(&File.dir?/1)
      |> Enum.map(&"-I#{&1}")
    else
      _ -> []
    end
  end

  defp nerves_cxx_root(toolchain, prefix) do
    [
      Path.join([toolchain, prefix, "include", "c++", "*"]),
      Path.join([toolchain, "opt", "ext-toolchain", prefix, "include", "c++", "*"])
    ]
    |> Enum.flat_map(&Path.wildcard/1)
    |> Enum.filter(&File.dir?/1)
    |> Enum.sort()
    |> List.last()
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

  defp nerves_sdk_env?(env) do
    value_present?(Map.get(env, "NERVES_SDK_SYSROOT")) and
      value_present?(Map.get(env, "NERVES_TOOLCHAIN")) and
      nerves_compiler?(Map.get(env, "CC"))
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
end
