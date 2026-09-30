defmodule Emerge.Mix.MacosRasterArtifact do
  @moduledoc false

  @architectures %{"aarch64-apple-darwin" => "arm64", "x86_64-apple-darwin" => "x86_64"}
  @libraries ~w(/usr/lib/libSystem.B.dylib /usr/lib/libc++.1.dylib /usr/lib/libobjc.A.dylib)
  # rust-skia links ApplicationServices for the macOS CPU/font APIs.
  @frameworks ~w(ApplicationServices CoreFoundation CoreGraphics CoreServices CoreText Foundation)

  def verify!(artifact, target) do
    architecture = command!("lipo", ["-archs", artifact])
    [_, install_name] = command!("otool", ["-D", artifact]) |> String.split("\n", trim: true)
    dependencies = command!("otool", ["-L", artifact])
    commands = command!("otool", ["-l", artifact])
    symbols = command!("nm", ["-gU", artifact])
    validate!(architecture, install_name, dependencies, commands, symbols, target)
    IO.puts("Verified macOS raster NIF: #{target}")
  end

  def validate!(architecture, install_name, dependencies, commands, symbols, target) do
    unless String.trim(architecture) == Map.fetch!(@architectures, target),
      do: raise(ArgumentError, "Incorrect Mach-O architecture")

    unless Regex.match?(~r/\bT\s+_nif_init\s*$/m, symbols),
      do: raise(ArgumentError, "Missing NIF entry point")

    needed =
      ~r/^\s+(.+?)\s+\(compatibility version /m
      |> Regex.scan(dependencies, capture: :all_but_first)
      |> List.flatten()
      |> Enum.reject(&(&1 == String.trim(install_name)))

    if needed == [] or Enum.any?(needed, &(not system_library?(&1))) do
      raise ArgumentError, "Unexpected raster dependencies: #{inspect(needed)}"
    end

    minimums =
      Regex.scan(~r/\bminos (\d+(?:\.\d+){1,2})/, commands, capture: :all_but_first) ++
        Regex.scan(
          ~r/cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version (\d+(?:\.\d+){1,2})/,
          commands,
          capture: :all_but_first
        )

    if minimums == [] or
         Enum.any?(minimums, fn [version] -> version_tuple(version) > {11, 0, 0} end) do
      raise ArgumentError, "Raster NIF requires newer than macOS 11 or has no deployment version"
    end

    :ok
  end

  defp system_library?(path) do
    path in @libraries or
      Enum.any?(@frameworks, fn name ->
        path in [
          "/System/Library/Frameworks/#{name}.framework/Versions/A/#{name}",
          "/System/Library/Frameworks/#{name}.framework/Versions/C/#{name}",
          "/System/Library/Frameworks/#{name}.framework/#{name}"
        ]
      end)
  end

  defp version_tuple(version) do
    version
    |> String.split(".")
    |> Enum.map(&String.to_integer/1)
    |> Kernel.++([0, 0])
    |> Enum.take(3)
    |> List.to_tuple()
  end

  defp command!(command, args) do
    case System.cmd(command, args, stderr_to_stdout: true) do
      {output, 0} -> output
      {output, _} -> raise "#{command} failed: #{output}"
    end
  end
end
