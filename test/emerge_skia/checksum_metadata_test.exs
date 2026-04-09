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
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu--drm_wayland.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-aarch64-unknown-linux-gnu.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--drm.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu--drm_wayland.so.tar.gz",
             "libemerge_skia-v#{version}-nif-2.15-x86_64-unknown-linux-gnu.so.tar.gz"
           ]

    on_exit(fn -> File.rm_rf!(cache_path) end)
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
