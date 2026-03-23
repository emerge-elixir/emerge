defmodule EmergeSkia.BuildConfigTest do
  use ExUnit.Case, async: true

  alias EmergeSkia.BuildConfig

  test "normalize_compiled_backends! defaults to canonical backend order" do
    assert BuildConfig.normalize_compiled_backends!([:drm, :wayland, :drm]) == [:wayland, :drm]
  end

  test "compiled_backends_to_rustler_features returns stable feature order" do
    assert BuildConfig.compiled_backends_to_rustler_features([:drm, :wayland]) == [
             "wayland",
             "drm"
           ]
  end

  test "default_compiled_backends uses drm when NERVES_SDK_SYSROOT is present" do
    assert BuildConfig.default_compiled_backends(%{"NERVES_SDK_SYSROOT" => "/tmp/nerves/staging"}) ==
             [:drm]
  end

  test "default_compiled_backends uses drm for non-host MIX_TARGET values" do
    assert BuildConfig.default_compiled_backends(%{"MIX_TARGET" => "rpi5"}) == [:drm]
  end

  test "default_compiled_backends uses drm for known Nerves compiler prefixes" do
    assert BuildConfig.default_compiled_backends(%{"CC" => "aarch64-nerves-linux-gnu-gcc"}) ==
             [:drm]
  end

  test "default_compiled_backends uses drm for target environment variables" do
    assert BuildConfig.default_compiled_backends(%{
             "TARGET_ARCH" => "aarch64",
             "TARGET_OS" => "linux",
             "TARGET_ABI" => "gnu"
           }) == [:drm]
  end

  test "default_compiled_backends uses wayland outside Nerves build environments" do
    assert BuildConfig.default_compiled_backends(%{}) == [:wayland]
    assert BuildConfig.default_compiled_backends(%{"MIX_TARGET" => "host"}) == [:wayland]
  end

  test "normalize_compiled_backends! accepts an empty backend list" do
    assert BuildConfig.normalize_compiled_backends!([]) == []
    assert BuildConfig.compiled_backends_to_rustler_features([]) == []
  end

  test "default_runtime_backend prefers wayland and falls back to drm" do
    assert BuildConfig.default_runtime_backend([:drm, :wayland]) == :wayland
    assert BuildConfig.default_runtime_backend([:drm]) == :drm
    assert BuildConfig.default_runtime_backend([]) == :wayland
  end

  test "precompiled_variants marks the Pi 5 variant for nerves builds only" do
    nerves_variant =
      BuildConfig.precompiled_variants(%{"NERVES_SDK_SYSROOT" => "/tmp/nerves/staging"})
      |> Map.fetch!("aarch64-unknown-linux-gnu")
      |> Keyword.fetch!(:nerves_rpi5)

    desktop_variant =
      BuildConfig.precompiled_variants(%{})
      |> Map.fetch!("aarch64-unknown-linux-gnu")
      |> Keyword.fetch!(:nerves_rpi5)

    assert nerves_variant.(%{})
    refute desktop_variant.(%{})
  end

  test "force_precompiled_build? forces builds when checksum is missing" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: "/tmp/emerge-missing-checksum",
             compiled_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? forces builds when backend profile is custom" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland, :drm],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? uses precompiled artifacts when checksum, target, and backend match" do
    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? respects the explicit force-build env var" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             env: %{BuildConfig.force_precompiled_build_env_key() => "true"},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? falls back to source builds for unsupported targets" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions -> {:error, :unsupported_target} end
           )
  end

  test "normalize_compiled_backends! rejects invalid config shapes" do
    assert_raise ArgumentError, ~r/compiled_backends: .*must be a list of backend atoms/, fn ->
      BuildConfig.normalize_compiled_backends!(:wayland)
    end
  end

  test "normalize_compiled_backends! rejects invalid entries" do
    assert_raise ArgumentError, ~r/containing only :wayland and :drm/, fn ->
      BuildConfig.normalize_compiled_backends!([:wayland, :bogus, "drm"])
    end
  end
end
