defmodule Emerge.Mix.RasterArtifactSmoke do
  @moduledoc false

  # Exercise the packaged library in fresh VMs without Rustler on the code path.
  # Checksum-only compilation of the parent project supplies the Elixir modules
  # without building or loading a different NIF first.
  def run!(archive_path, source_root) do
    root =
      Path.join(System.tmp_dir!(), "emerge-raster-smoke-#{System.unique_integer([:positive])}")

    archive = File.read!(archive_path)
    name = Path.basename(archive_path)
    cache = Path.join(root, "cache")
    app_dir = Path.join(root, "lib/emerge")
    hash = :crypto.hash(:sha256, archive) |> Base.encode16(case: :lower)

    try do
      File.mkdir_p!(cache)
      File.mkdir_p!(Path.join(app_dir, "ebin"))
      File.mkdir_p!(Path.join(app_dir, "priv/native"))
      File.mkdir_p!(Path.join(app_dir, "priv/test_assets"))
      File.mkdir_p!(Path.join(root, "lib/emerge_skia"))
      File.cp!(Path.join(source_root, "mix.exs"), Path.join(root, "mix.exs"))
      File.cp_r!(Path.join(source_root, "mix"), Path.join(root, "mix"))

      File.cp!(
        Path.join(source_root, "lib/emerge_skia/native.ex"),
        Path.join(root, "lib/emerge_skia/native.ex")
      )

      File.cp!(
        Path.join(source_root, "native/emerge_skia/src/fonts/inter/Inter-Regular.ttf"),
        Path.join(app_dir, "priv/test_assets/Probe.ttf")
      )

      File.write!(
        Path.join(root, "checksum-Elixir.EmergeSkia.Native.exs"),
        inspect(%{name => "sha256:#{hash}"})
      )

      File.write!(Path.join(cache, name), archive)

      for _attempt <- 1..2, do: probe!(root, source_root, cache, app_dir, :ok)
      File.write!(Path.join(cache, name), archive <> "corrupt")
      probe!(root, source_root, cache, app_dir, :corrupt)

      IO.puts(
        "Packaged raster NIF: load, cache reuse, packed rendering and checksum rejection passed"
      )
    after
      File.rm_rf!(root)
    end
  end

  defp probe!(root, source_root, cache, app_dir, expected) do
    paths =
      :code.get_path()
      |> Enum.map(&to_string/1)
      |> Enum.reject(&(Path.basename(Path.dirname(&1)) == "rustler"))
      |> Enum.flat_map(&["-pa", &1])

    env =
      Enum.map(
        ~w(TARGET_ARCH TARGET_OS TARGET_ABI TARGET_VENDOR TARGET_CPU CC CXX MIX_TARGET NERVES_SDK_SYSROOT NERVES_TOOLCHAIN EMERGE_SKIA_BUILD EMERGE_SKIA_CHECKSUM_ONLY RUSTLER_PRECOMPILED_FORCE_BUILD_ALL),
        &{&1, nil}
      ) ++
        [
          {"ERL_FLAGS", "+S 2:2"},
          {"MIX_ENV", "prod"},
          {"RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH", cache},
          {"EMERGE_SKIA_PRECOMPILED_SOURCE_URL", "http://127.0.0.1:1/must-not-download"}
        ]

    {output, status} =
      System.cmd(
        System.find_executable("elixir"),
        paths ++
          [Path.join(source_root, "scripts/raster-artifact-probe.exs"), source_root, app_dir],
        cd: root,
        env: env,
        stderr_to_stdout: true
      )

    case expected do
      :ok ->
        unless status == 0 and output =~ "raster probe passed",
          do: raise("Raster artifact failed:\n#{output}")

      :corrupt ->
        unless status != 0 and output =~ "integrity check failed",
          do: raise("Corrupt archive was not rejected:\n#{output}")
    end
  end
end
