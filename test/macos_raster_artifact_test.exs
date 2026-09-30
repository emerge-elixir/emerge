Code.require_file("../mix/macos_raster_artifact.exs", __DIR__)

defmodule Emerge.MacosRasterArtifactTest do
  use ExUnit.Case, async: true
  alias Emerge.Mix.MacosRasterArtifact

  @commands "cmd LC_BUILD_VERSION\ncmdsize 32\nplatform 1\nminos 11.0\nsdk 15.0\n"
  @symbols "00000000001000 T _nif_init\n"
  @install_name "@rpath/libemerge_skia.dylib"
  @libraries [
    "/usr/lib/libSystem.B.dylib",
    "/usr/lib/libc++.1.dylib",
    "/System/Library/Frameworks/ApplicationServices.framework/Versions/A/ApplicationServices",
    "/System/Library/Frameworks/CoreText.framework/Versions/A/CoreText"
  ]

  test "both architectures allow only system CPU/font dependencies" do
    for {target, arch} <- [{"aarch64-apple-darwin", "arm64"}, {"x86_64-apple-darwin", "x86_64"}] do
      assert :ok = validate(arch: arch, target: target)
    end
  end

  test "rejects wrong/universal architecture and missing NIF exports" do
    for arch <- ["x86_64", "arm64 x86_64"] do
      assert_raise ArgumentError, ~r/architecture/, fn -> validate(arch: arch) end
    end

    assert_raise ArgumentError, ~r/entry point/, fn -> validate(symbols: "U _nif_init\n") end
  end

  test "rejects Homebrew, GPU, windowing and unresolved rpath dependencies" do
    for library <- [
          "/opt/homebrew/lib/libfreetype.6.dylib",
          "/usr/local/lib/libfontconfig.1.dylib",
          "@rpath/libfreetype.6.dylib",
          "/System/Library/Frameworks/Metal.framework/Versions/A/Metal",
          "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit"
        ] do
      assert_raise ArgumentError, ~r/Unexpected raster dependencies/, fn ->
        validate(libraries: @libraries ++ [library])
      end
    end

    assert_raise ArgumentError, ~r/Unexpected raster dependencies/, fn ->
      validate(libraries: [])
    end
  end

  test "deployment floor must be present and no newer than macOS 11" do
    for version <- ["11.1", "11.0.1", "11.10", "12.0", "15.0"] do
      assert_raise ArgumentError, ~r/deployment version/, fn ->
        validate(commands: String.replace(@commands, "minos 11.0", "minos #{version}"))
      end
    end

    assert_raise ArgumentError, ~r/deployment version/, fn -> validate(commands: "") end

    assert :ok =
             validate(commands: "cmd LC_VERSION_MIN_MACOSX\ncmdsize 16\nversion 11.0\nsdk 15.0\n")
  end

  test "release matrix builds and verifies both raster archives before upload" do
    workflow =
      File.read!(Path.expand("../.github/workflows/build_release_artifacts.yml", __DIR__))

    for {name, target, os} <- [
          {"macos_arm64_raster", "aarch64-apple-darwin", "macos-15"},
          {"macos_x86_64_raster", "x86_64-apple-darwin", "macos-15-intel"}
        ] do
      [block] = Regex.run(~r/          - name: #{name}\n.*?(?=          - name:)/s, workflow)
      assert block =~ "target: #{target}"
      assert block =~ "os: #{os}\n"
      assert block =~ "variant: \"\""
      assert block =~ "--no-default-features --features embedded-cpu"
      assert block =~ "macos_raster: true"
      refute block =~ "embedded_skia: true"
    end

    assert workflow =~ "MACOSX_DEPLOYMENT_TARGET=11.0"
    assert workflow =~ "FORCE_SKIA_BUILD=1"
    assert workflow =~ "EMERGE_SKIA_CHECKSUM_ONLY: \"true\""
    assert workflow =~ "if: ${{ runner.os == 'Linux' }}"
    assert workflow =~ "elixir scripts/check-macos-raster-artifact.exs"
    assert workflow =~ "mix run --no-start scripts/smoke-raster-artifact.exs"

    [_, verification] =
      String.split(workflow, "- name: Verify packaged macOS raster NIF", parts: 2)

    [verification, _] = String.split(verification, "- name: Artifact upload", parts: 2)
    refute verification =~ "continue-on-error"
    refute workflow =~ "mix test"
  end

  defp validate(opts) do
    dependencies =
      [@install_name | Keyword.get(opts, :libraries, @libraries)]
      |> Enum.map_join("\n", &"\t#{&1} (compatibility version 1.0.0, current version 1.0.0)")

    MacosRasterArtifact.validate!(
      Keyword.get(opts, :arch, "arm64") <> "\n",
      @install_name,
      "artifact.so:\n" <> dependencies,
      Keyword.get(opts, :commands, @commands),
      Keyword.get(opts, :symbols, @symbols),
      Keyword.get(opts, :target, "aarch64-apple-darwin")
    )
  end
end
