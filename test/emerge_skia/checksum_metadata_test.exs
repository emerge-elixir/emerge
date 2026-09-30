defmodule EmergeSkia.ChecksumMetadataTest do
  use ExUnit.Case, async: false

  alias EmergeSkia.BuildConfig
  alias EmergeSkia.ChecksumMetadata

  test "writes metadata that rustler_precompiled can use for checksum downloads" do
    cache_path =
      Path.join(
        System.tmp_dir!(),
        "emerge-rustler-precompiled-#{System.unique_integer([:positive])}"
      )

    restore_env_on_exit("RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH")
    restore_env_on_exit("MIX_XDG")

    System.put_env("RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH", cache_path)
    System.delete_env("MIX_XDG")

    :ok =
      ChecksumMetadata.ensure_written!(
        EmergeSkia.Native,
        otp_app: :emerge,
        crate: "emerge_skia",
        base_url: {BuildConfig, :precompiled_tar_gz_url},
        version: Mix.Project.config()[:version],
        targets: BuildConfig.precompiled_targets(),
        nif_versions: BuildConfig.precompiled_nif_versions(),
        variants: BuildConfig.precompiled_variants()
      )

    assert File.exists?(ChecksumMetadata.metadata_file_path(EmergeSkia.Native))

    version = Mix.Project.config()[:version]

    assert RustlerPrecompiled.available_nifs(EmergeSkia.Native)
           |> Enum.map(&elem(&1, 0))
           |> Enum.sort() == [
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--drm.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--drm_all.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--drm_vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--drm_wayland.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--headless_vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--raster.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--wayland_vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-armv7-unknown-linux-gnueabihf--opengl.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-armv7-unknown-linux-gnueabihf.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-riscv64gc-unknown-linux-gnu--opengl.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-riscv64gc-unknown-linux-gnu.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--drm.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--drm_all.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--drm_vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--drm_wayland.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--headless_vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--raster.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--wayland_vulkan.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-musl--opengl.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-musl.so.tar.gz"
           ]

    on_exit(fn -> File.rm_rf!(cache_path) end)
  end

  test "checksum-only Native compilation is warning-free without Rustler" do
    cache =
      Path.join(System.tmp_dir!(), "emerge-checksum-only-#{System.unique_integer([:positive])}")

    on_exit(fn -> File.rm_rf!(cache) end)

    paths =
      :code.get_path()
      |> Enum.map(&to_string/1)
      |> Enum.reject(&(Path.basename(Path.dirname(&1)) == "rustler"))
      |> Enum.flat_map(&["-pa", &1])

    script = """
    Mix.start()
    Code.compile_file("mix.exs")
    false = Code.ensure_loaded?(Rustler)
    Code.compiler_options(ignore_module_conflict: true)
    {_modules, diagnostics} = Code.with_diagnostics(fn -> Code.compile_file("lib/emerge_skia/native.ex") end)
    [] = Enum.filter(diagnostics, &(&1.severity == :warning))
    IO.puts("checksum-only compilation passed")
    """

    {output, status} =
      System.cmd(System.find_executable("elixir"), paths ++ ["-e", script],
        cd: Path.expand("../..", __DIR__),
        stderr_to_stdout: true,
        env: [
          {"ERL_FLAGS", "+S 2:2"},
          {"EMERGE_SKIA_CHECKSUM_ONLY", "true"},
          {"RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH", cache}
        ]
      )

    assert status == 0, output
    assert output =~ "checksum-only compilation passed"
  end

  defp restore_env_on_exit(name) do
    previous = System.get_env(name)

    on_exit(fn ->
      if previous do
        System.put_env(name, previous)
      else
        System.delete_env(name)
      end
    end)
  end
end
