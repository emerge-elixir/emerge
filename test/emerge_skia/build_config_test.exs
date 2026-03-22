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
