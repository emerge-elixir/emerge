Code.require_file("../mix/embedded_artifact.exs", __DIR__)

defmodule Emerge.EmbeddedArtifactTest do
  use ExUnit.Case, async: true
  alias Emerge.Mix.EmbeddedArtifact

  @header "Class: ELF64\nType: DYN (Shared object file)\nMachine: AArch64\n"
  @symbols "1: 0000000000 123 FUNC GLOBAL DEFAULT 12 nif_init\n"
  @target "aarch64-unknown-linux-gnu"

  test "accepts the minimal DRM runtime and rejects every unexpected SONAME" do
    dependencies =
      ~w(libc.so.6 libm.so.6 libstdc++.so.6 libgcc_s.so.1 libgbm.so.1 ld-linux-aarch64.so.1)

    for profile <- ~w(drm drm_vulkan drm_all) do
      assert :ok = validate(dependencies, profile)

      for library <-
            ~w(libfontconfig.so.1 libfreetype.so.6 libxkbcommon.so.0 libwayland-client.so.0 libX11.so.6 libunknown.so.1) do
        assert_raise ArgumentError, ~r/Unexpected embedded dependencies/, fn ->
          validate([library | dependencies], profile)
        end
      end
    end
  end

  test "raster forbids GPU libraries and Vulkan-only forbids GL" do
    assert :ok = validate(["libc.so.6"], "raster")

    for library <- ~w(libgbm.so.1 libdrm.so.2 libEGL.so.1 libvulkan.so.1) do
      assert_raise ArgumentError, fn -> validate([library], "raster") end
    end

    assert_raise ArgumentError, fn -> validate(["libEGL.so.1"], "drm_vulkan") end
  end

  test "rejects wrong architecture, missing NIF entry and unresolved desktop symbols" do
    for {header, symbols} <- [
          {String.replace(@header, "AArch64", "ARM"), @symbols},
          {String.replace(@header, "ELF64", "ELF32"), @symbols},
          {String.replace(@header, "DYN", "EXEC"), @symbols},
          {@header, String.replace(@symbols, "DEFAULT 12", "DEFAULT UND")},
          {@header, @symbols <> "2: 0 0 FUNC GLOBAL DEFAULT UND FcInit\n"},
          {@header, @symbols <> "2: 0 0 FUNC GLOBAL DEFAULT UND FT_Init_FreeType\n"},
          {@header, @symbols <> "2: 0 0 FUNC GLOBAL DEFAULT UND xkb_context_new\n"}
        ] do
      assert_raise ArgumentError, fn ->
        EmbeddedArtifact.validate!(header, needed(["libc.so.6"]), symbols, @target, "drm")
      end
    end
  end

  test "retains ARMv7 hard-float checks" do
    header =
      "Class: ELF32\nType: DYN (Shared object file)\nMachine: ARM\nTag_CPU_arch: v7\nTag_ABI_VFP_args: VFP registers\n"

    assert :ok =
             EmbeddedArtifact.validate!(
               header,
               needed(["libc.so.6"]),
               @symbols,
               "armv7-unknown-linux-gnueabihf",
               "opengl"
             )

    assert_raise ArgumentError, ~r/hard-float/, fn ->
      EmbeddedArtifact.validate!(
        String.replace(header, "VFP registers", "base"),
        needed(["libc.so.6"]),
        @symbols,
        "armv7-unknown-linux-gnueabihf",
        "opengl"
      )
    end
  end

  defp validate(libraries, profile),
    do: EmbeddedArtifact.validate!(@header, needed(libraries), @symbols, @target, profile)

  defp needed(libraries),
    do: Enum.map_join(libraries, "\n", &"0x1 (NEEDED) Shared library: [#{&1}]")
end
