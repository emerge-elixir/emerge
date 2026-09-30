defmodule Emerge.Mix.EmbeddedArtifact do
  @moduledoc false

  @runtime ~w(libc.so.6 libm.so.6 libdl.so.2 libpthread.so.0 librt.so.1 libgcc_s.so.1 libstdc++.so.6 libatomic.so.1)
  @graphics ~w(libgbm.so.1 libdrm.so.2 libEGL.so.1 libGLESv2.so.2 libGL.so.1 libvulkan.so.1)
  @targets %{
    "x86_64-unknown-linux-gnu" =>
      {"ELF64", "Advanced Micro Devices X86-64", "ld-linux-x86-64.so.2"},
    "aarch64-unknown-linux-gnu" => {"ELF64", "AArch64", "ld-linux-aarch64.so.1"},
    "armv7-unknown-linux-gnueabihf" => {"ELF32", "ARM", "ld-linux-armhf.so.3"}
  }

  def verify!(artifact, target, profile) do
    read = fn args ->
      case System.cmd("readelf", args ++ [artifact], env: [{"LC_ALL", "C"}]) do
        {output, 0} -> output
        _ -> raise "readelf failed for #{artifact}"
      end
    end

    validate!(
      read.(["--file-header", "--arch-specific"]),
      read.(["--dynamic"]),
      read.(["--dyn-syms", "--wide"]),
      target,
      profile
    )

    IO.puts("Verified embedded ELF: #{target}/#{profile}")
  end

  def validate!(header, dynamic, symbols, target, profile) do
    {class, machine, loader} = Map.fetch!(@targets, target)

    unless profile in ~w(raster drm drm_vulkan drm_all opengl),
      do: raise(ArgumentError, "Unsupported embedded profile: #{profile}")

    require_match!(header, ~r/Class:\s+#{class}\s/, "ELF class")
    require_match!(header, ~r/Machine:\s+#{machine}\s/, "ELF architecture")
    require_match!(header, ~r/Type:\s+DYN\s/, "shared library")

    if target == "armv7-unknown-linux-gnueabihf" do
      require_match!(header, ~r/Tag_CPU_arch: v7\s/, "ARMv7")
      require_match!(header, ~r/Tag_ABI_VFP_args: VFP registers/, "hard-float ABI")
    end

    require_match!(symbols, ~r/GLOBAL\s+DEFAULT\s+\d+\s+nif_init\s*$/m, "NIF entry point")

    graphics =
      case profile do
        "raster" -> []
        "drm_vulkan" -> ~w(libgbm.so.1 libdrm.so.2 libvulkan.so.1)
        _ -> @graphics
      end

    allowed = @runtime ++ [loader] ++ graphics

    needed =
      Regex.scan(~r/\(NEEDED\).*\[([^\]]+)\]/, dynamic, capture: :all_but_first) |> List.flatten()

    unexpected = needed -- allowed

    if needed == [] or unexpected != [] do
      raise ArgumentError,
            "Unexpected embedded dependencies: #{inspect(unexpected)} (needed: #{inspect(needed)})"
    end

    if Regex.match?(~r/UND\s+(Fc|FT_|xkb_|wl_|XOpenDisplay)/, symbols) do
      raise ArgumentError, "Unresolved desktop/font symbol in embedded artifact"
    end

    :ok
  end

  defp require_match!(text, pattern, name) do
    unless Regex.match?(pattern, text), do: raise(ArgumentError, "Invalid #{name}")
  end
end
